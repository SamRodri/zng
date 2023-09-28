//! This module implements Management of window content and synchronization of WindowVars and View-Process.

use std::{mem, sync::Arc};

use crate::{
    app::{
        access::ACCESS_INITED_EVENT,
        raw_events::{
            RawWindowFocusArgs, RAW_COLOR_SCHEME_CHANGED_EVENT, RAW_FRAME_RENDERED_EVENT, RAW_HEADLESS_OPEN_EVENT,
            RAW_WINDOW_CHANGED_EVENT, RAW_WINDOW_FOCUS_EVENT, RAW_WINDOW_OPEN_EVENT, RAW_WINDOW_OR_HEADLESS_OPEN_ERROR_EVENT,
        },
        view_process::*,
    },
    color::{ColorScheme, RenderColor},
    context::{
        InfoUpdates, LayoutMetrics, LayoutPassId, LayoutUpdates, RenderUpdates, WidgetCtx, WidgetUpdateMode, WidgetUpdates, DIRECTION_VAR,
        LAYOUT, UPDATES, WIDGET, WINDOW,
    },
    crate_util::{IdEntry, IdMap},
    event::{AnyEventArgs, EventUpdate},
    image::{ImageVar, Img, IMAGES},
    render::{FrameBuilder, FrameId, FrameUpdate},
    text::FONTS,
    timer::TIMERS,
    units::*,
    var::*,
    widget_info::{WidgetInfoBuilder, WidgetInfoTree, WidgetLayout},
    widget_instance::{BoxedUiNode, UiNode, WidgetId},
    window::AutoSize,
};

use super::{
    commands::{WindowCommands, MINIMIZE_CMD, RESTORE_CMD},
    FrameCaptureMode, FrameImageReadyArgs, HeadlessMonitor, MonitorInfo, StartPosition, TransformChangedArgs, WindowChangedArgs,
    WindowChrome, WindowIcon, WindowId, WindowMode, WindowRoot, WindowVars, FRAME_IMAGE_READY_EVENT, MONITORS, MONITORS_CHANGED_EVENT,
    TRANSFORM_CHANGED_EVENT, WINDOWS, WINDOW_CHANGED_EVENT,
};

/// Implementer of `App <-> View` sync in a headed window.
struct HeadedCtrl {
    window: Option<ViewWindow>,
    waiting_view: bool,
    delayed_view_updates: Vec<Box<dyn FnOnce(&ViewWindow) + Send>>,
    vars: WindowVars,
    respawned: bool,

    content: ContentCtrl,

    // init config.
    start_position: StartPosition,
    start_focused: bool,
    kiosk: Option<WindowState>, // Some(enforced_fullscreen)
    transparent: bool,
    render_mode: Option<RenderMode>,

    // current state.
    state: Option<WindowStateAll>, // None if not inited.
    monitor: Option<MonitorInfo>,
    resize_wait_id: Option<FrameWaitId>,
    icon: Option<ImageVar>,
    icon_binding: VarHandle,
    icon_deadline: Deadline,
    actual_state: Option<WindowState>, // for WindowChangedEvent
    system_color_scheme: Option<ColorScheme>,
    parent_color_scheme: Option<ReadOnlyArcVar<ColorScheme>>,
    actual_parent: Option<WindowId>,
    root_font_size: Dip,
}
impl HeadedCtrl {
    pub fn new(vars: &WindowVars, commands: WindowCommands, content: WindowRoot) -> Self {
        Self {
            window: None,
            waiting_view: false,
            delayed_view_updates: vec![],

            start_position: content.start_position,
            start_focused: content.start_focused,
            kiosk: if content.kiosk { Some(WindowState::Fullscreen) } else { None },
            transparent: content.transparent,
            render_mode: content.render_mode,

            content: ContentCtrl::new(vars.clone(), commands, content),
            vars: vars.clone(),
            respawned: false,

            state: None,
            monitor: None,
            resize_wait_id: None,
            icon: None,
            icon_binding: VarHandle::dummy(),
            icon_deadline: Deadline::timeout(1.secs()),
            system_color_scheme: None,
            parent_color_scheme: None,
            actual_parent: None,
            actual_state: None,
            root_font_size: Dip::from_px(Length::pt_to_px(11.0, 1.fct()), 1.0),
        }
    }

    fn update_gen(&mut self, update: impl FnOnce(&ViewWindow) + Send + 'static) {
        if let Some(view) = &self.window {
            // view is online, just update.
            update(view);
        } else if self.waiting_view {
            // update after view requested, but still not ready. Will apply when the view is received
            // or be discarded if the view-process respawns.
            self.delayed_view_updates.push(Box::new(update));
        } else {
            // respawning or view-process not inited, will recreate entire window.
        }
    }

    pub fn update(&mut self, update_widgets: &WidgetUpdates) {
        if self.window.is_none() && !self.waiting_view {
            // we request a view on the first layout.
            UPDATES.layout_window(WINDOW.id());

            if let Some(enforced_fullscreen) = self.kiosk {
                // enforce kiosk in pre-init.

                if !self.vars.state().get().is_fullscreen() {
                    self.vars.state().set(enforced_fullscreen);
                }
            }
        }

        if let Some(enforced_fullscreen) = &mut self.kiosk {
            // always fullscreen, but can be windowed or exclusive.

            if let Some(state) = self.vars.state().get_new() {
                if !state.is_fullscreen() {
                    tracing::error!("window in `kiosk` mode can only be fullscreen");

                    self.vars.state().set(*enforced_fullscreen);
                } else {
                    *enforced_fullscreen = state;
                }
            }

            if let Some(false) = self.vars.visible().get_new() {
                tracing::error!("window in `kiosk` mode can not be hidden");

                self.vars.visible().set(true);
            }

            if let Some(mode) = self.vars.chrome().get_new() {
                if !mode.is_none() {
                    tracing::error!("window in `kiosk` mode can not show chrome");
                    self.vars.chrome().set(WindowChrome::None);
                }
            }
        } else {
            // not kiosk mode.

            if let Some(prev_state) = self.state.clone() {
                debug_assert!(self.window.is_some() || self.waiting_view || self.respawned);

                let mut new_state = prev_state.clone();

                if let Some(query) = self.vars.monitor().get_new() {
                    if self.monitor.is_none() {
                        let monitor = query.select_fallback();
                        let scale_factor = monitor.scale_factor().get();
                        self.vars.0.scale_factor.set(scale_factor);
                        self.monitor = Some(monitor);
                    } else if let Some(new) = query.select() {
                        let current = self.vars.0.actual_monitor.get();
                        if Some(new.id()) != current {
                            let scale_factor = new.scale_factor().get();
                            self.vars.0.scale_factor.set(scale_factor);
                            self.vars.0.actual_monitor.set(new.id());
                            self.monitor = Some(new);
                        }
                    }
                }

                if let Some(chrome) = self.vars.chrome().get_new() {
                    new_state.chrome_visible = chrome.is_default();
                }

                if let Some(req_state) = self.vars.state().get_new() {
                    new_state.set_state(req_state);
                    self.vars.0.restore_state.set(new_state.restore_state);
                }

                if self.vars.min_size().is_new() || self.vars.max_size().is_new() {
                    if let Some(m) = &self.monitor {
                        let scale_factor = m.scale_factor().get();
                        let screen_ppi = m.ppi().get();
                        let screen_size = m.size().get();
                        let (min_size, max_size) = self.content.outer_layout(scale_factor, screen_ppi, screen_size, || {
                            let min_size = self.vars.min_size().layout_dft(default_min_size(scale_factor));
                            let max_size = self.vars.max_size().layout_dft(screen_size);

                            (min_size.to_dip(scale_factor.0), max_size.to_dip(scale_factor.0))
                        });

                        let size = new_state.restore_rect.size;

                        new_state.restore_rect.size = size.min(max_size).max(min_size);
                        new_state.min_size = min_size;
                        new_state.max_size = max_size;
                    }
                }

                if let Some(auto) = self.vars.auto_size().get_new() {
                    if auto != AutoSize::DISABLED {
                        UPDATES.layout_window(WINDOW.id());
                    }
                }

                if self.vars.size().is_new() {
                    let auto_size = self.vars.auto_size().get();

                    if auto_size != AutoSize::CONTENT {
                        if let Some(m) = &self.monitor {
                            let scale_factor = m.scale_factor().get();
                            let screen_ppi = m.ppi().get();
                            let screen_size = m.size().get();
                            let size = self.content.outer_layout(scale_factor, screen_ppi, screen_size, || {
                                self.vars.size().layout_dft(default_size(scale_factor)).to_dip(scale_factor.0)
                            });

                            let size = size.min(new_state.max_size).max(new_state.min_size);

                            if !auto_size.contains(AutoSize::CONTENT_WIDTH) {
                                new_state.restore_rect.size.width = size.width;
                            }
                            if !auto_size.contains(AutoSize::CONTENT_HEIGHT) {
                                new_state.restore_rect.size.height = size.height;
                            }
                        }
                    }
                }

                if let Some(font_size) = self.vars.font_size().get_new() {
                    if let Some(m) = &self.monitor {
                        let scale_factor = m.scale_factor().get();
                        let screen_ppi = m.ppi().get();
                        let screen_size = m.size().get();
                        let mut font_size_px = self.content.outer_layout(scale_factor, screen_ppi, screen_size, || {
                            font_size.layout_dft_x(Length::pt_to_px(11.0, scale_factor))
                        });
                        if font_size_px < Px(0) {
                            tracing::error!("invalid font size {font_size:?} => {font_size_px:?}");
                            font_size_px = Length::pt_to_px(11.0, scale_factor);
                        }
                        let font_size_dip = font_size_px.to_dip(scale_factor.0);

                        if font_size_dip != self.root_font_size {
                            self.root_font_size = font_size_dip;
                            UPDATES.layout_window(WINDOW.id());
                        }
                    }
                }

                if let Some(pos) = self.vars.position().get_new() {
                    if let Some(m) = &self.monitor {
                        let scale_factor = m.scale_factor().get();
                        let screen_ppi = m.ppi().get();
                        let screen_size = m.size().get();
                        let pos = self.content.outer_layout(scale_factor, screen_ppi, screen_size, || {
                            pos.layout_dft(PxPoint::new(Px(50), Px(50)))
                        });
                        new_state.restore_rect.origin = pos.to_dip(scale_factor.0);
                    }
                }

                if let Some(visible) = self.vars.visible().get_new() {
                    self.update_gen(move |view| {
                        let _: Ignore = view.set_visible(visible);
                    });
                }

                if let Some(movable) = self.vars.movable().get_new() {
                    self.update_gen(move |view| {
                        let _: Ignore = view.set_movable(movable);
                    });
                }

                if let Some(resizable) = self.vars.resizable().get_new() {
                    self.update_gen(move |view| {
                        let _: Ignore = view.set_resizable(resizable);
                    });
                }

                if prev_state != new_state {
                    self.update_gen(move |view| {
                        let _: Ignore = view.set_state(new_state);
                    })
                }
            }

            // icon:
            let mut send_icon = false;
            if let Some(ico) = self.vars.icon().get_new() {
                use crate::image::ImageSource;

                self.icon = match ico {
                    WindowIcon::Default => None,
                    WindowIcon::Image(ImageSource::Render(ico, _)) => Some(IMAGES.cache(ImageSource::Render(
                        ico.clone(),
                        Some(crate::image::ImageRenderArgs { parent: Some(WINDOW.id()) }),
                    ))),
                    WindowIcon::Image(source) => Some(IMAGES.cache(source)),
                };

                if let Some(ico) = &self.icon {
                    self.icon_binding = ico.bind_map(&self.vars.0.actual_icon, |img| Some(img.clone()));

                    if ico.get().is_loading() && self.window.is_none() && !self.waiting_view {
                        if self.icon_deadline.has_elapsed() {
                            UPDATES.layout_window(WINDOW.id());
                        } else {
                            let window_id = WINDOW.id();
                            TIMERS
                                .on_deadline(
                                    self.icon_deadline,
                                    app_hn_once!(ico, |_| {
                                        if ico.get().is_loading() {
                                            UPDATES.layout_window(window_id);
                                        }
                                    }),
                                )
                                .perm();
                        }
                    }
                } else {
                    self.vars.0.actual_icon.set(None);
                    self.icon_binding = VarHandle::dummy();
                }

                send_icon = true;
            } else if self.icon.as_ref().map(|ico| ico.is_new()).unwrap_or(false) {
                send_icon = true;
            }
            if send_icon {
                let icon = self.icon.as_ref().and_then(|ico| ico.get().view().cloned());
                self.update_gen(move |view| {
                    let _: Ignore = view.set_icon(icon.as_ref());
                });
            }

            if let Some(title) = self.vars.title().get_new() {
                self.update_gen(move |view| {
                    let _: Ignore = view.set_title(title.into_owned());
                });
            }

            if let Some(mode) = self.vars.video_mode().get_new() {
                self.update_gen(move |view| {
                    let _: Ignore = view.set_video_mode(mode);
                });
            }

            if let Some(cursor) = self.vars.cursor().get_new() {
                self.update_gen(move |view| {
                    let _: Ignore = view.set_cursor(cursor);
                });
            }

            if let Some(visible) = self.vars.taskbar_visible().get_new() {
                self.update_gen(move |view| {
                    let _: Ignore = view.set_taskbar_visible(visible);
                });
            }

            if let Some(top) = self.vars.always_on_top().get_new() {
                self.update_gen(move |view| {
                    let _: Ignore = view.set_always_on_top(top);
                });
            }

            if let Some(mode) = self.vars.frame_capture_mode().get_new() {
                self.update_gen(move |view| {
                    let _: Ignore = view.set_capture_mode(matches!(mode, FrameCaptureMode::All));
                });
            }

            if let Some(m) = &self.monitor {
                if let Some(fct) = m.scale_factor().get_new() {
                    self.vars.0.scale_factor.set(fct);
                }
                if m.scale_factor().is_new() || m.size().is_new() || m.ppi().is_new() {
                    UPDATES.layout_window(WINDOW.id());
                }
            }

            if let Some(indicator) = self.vars.focus_indicator().get_new() {
                if WINDOWS.is_focused(WINDOW.id()).unwrap_or(false) {
                    self.vars.focus_indicator().set(None);
                } else if let Some(view) = &self.window {
                    let _ = view.set_focus_indicator(indicator);
                    // will be set to `None` once the window is focused.
                }
                // else indicator is send with init.
            }

            let mut update_color_scheme = false;

            if update_parent(&mut self.actual_parent, &self.vars) {
                self.parent_color_scheme = self
                    .actual_parent
                    .and_then(|id| WINDOWS.vars(id).ok().map(|v| v.actual_color_scheme()));
                update_color_scheme = true;
            }

            if update_color_scheme
                || self.vars.color_scheme().is_new()
                || self.parent_color_scheme.as_ref().map(|t| t.is_new()).unwrap_or(false)
            {
                let scheme = self
                    .vars
                    .color_scheme()
                    .get()
                    .or_else(|| self.parent_color_scheme.as_ref().map(|t| t.get()))
                    .or(self.system_color_scheme)
                    .unwrap_or_default();
                self.vars.0.actual_color_scheme.set(scheme);
            }

            self.vars.renderer_debug().with_new(|dbg| {
                if let Some(view) = &self.window {
                    if let Some(key) = dbg.extension_id() {
                        let _ = view.renderer().render_extension::<_, ()>(key, dbg);
                    }
                }
            });
        }

        self.content.update(update_widgets);
    }

    pub fn pre_event(&mut self, update: &EventUpdate) {
        if let Some(args) = RAW_WINDOW_CHANGED_EVENT.on(update) {
            if args.window_id == WINDOW.id() {
                let mut state_change = None;
                let mut pos_change = None;
                let mut size_change = None;

                if let Some(monitor) = args.monitor {
                    if self.vars.0.actual_monitor.get().map(|m| m != monitor).unwrap_or(true) {
                        self.vars.0.actual_monitor.set(Some(monitor));
                        self.monitor = MONITORS.monitor(monitor);
                        if let Some(m) = &self.monitor {
                            self.vars.0.scale_factor.set(m.scale_factor().get());
                        }
                        UPDATES.layout_window(WINDOW.id());
                    }
                }

                if let Some(state) = args.state.clone() {
                    self.vars.state().set(state.state);
                    self.vars.0.restore_rect.set(state.restore_rect);
                    self.vars.0.restore_state.set(state.restore_state);

                    let new_state = state.state;
                    if self.actual_state != Some(new_state) {
                        let prev_state = self.actual_state.unwrap_or(WindowState::Normal);
                        state_change = Some((prev_state, new_state));
                        self.actual_state = Some(new_state);

                        match (prev_state, new_state) {
                            (_, WindowState::Minimized) => {
                                // minimized, minimize children.
                                self.vars.0.children.with(|c| {
                                    for &c in c.iter() {
                                        MINIMIZE_CMD.scoped(c).notify();
                                    }
                                });
                            }
                            (WindowState::Minimized, _) => {
                                // restored, restore children.
                                self.vars.0.children.with(|c| {
                                    for &c in c.iter() {
                                        RESTORE_CMD.scoped(c).notify();
                                    }
                                });

                                // we skip layout & render when minimized.
                                let w_id = WINDOW.id();
                                UPDATES.layout_window(w_id).render_window(w_id);
                            }
                            _ => {}
                        }
                    }

                    self.state = Some(state);
                }

                if let Some((global_pos, pos)) = args.position {
                    if self.vars.0.actual_position.get() != pos || self.vars.0.global_position.get() != global_pos {
                        self.vars.0.actual_position.set(pos);
                        self.vars.0.global_position.set(global_pos);
                        pos_change = Some((global_pos, pos));
                    }
                }

                if let Some(size) = args.size {
                    if self.vars.0.actual_size.get() != size {
                        self.vars.0.actual_size.set(size);
                        size_change = Some(size);

                        UPDATES.layout_window(WINDOW.id());

                        if args.cause == EventCause::System {
                            // resize by system (user)
                            self.vars.auto_size().set(AutoSize::DISABLED);
                        }
                    }
                }

                if let Some(id) = args.frame_wait_id {
                    self.resize_wait_id = Some(id);

                    UPDATES.render_update_window(WINDOW.id());
                }

                if state_change.is_some() || pos_change.is_some() || size_change.is_some() {
                    let args = WindowChangedArgs::new(
                        args.timestamp,
                        args.propagation().clone(),
                        args.window_id,
                        state_change,
                        pos_change,
                        size_change,
                        args.cause,
                    );
                    WINDOW_CHANGED_EVENT.notify(args);
                }
            } else if self.actual_state.unwrap_or(WindowState::Normal) == WindowState::Minimized
                && args.state.as_ref().map(|s| s.state != WindowState::Minimized).unwrap_or(false)
                && self.vars.0.children.with(|c| c.contains(&args.window_id))
            {
                // child restored.
                RESTORE_CMD.scoped(WINDOW.id()).notify();
            }
        } else if let Some(args) = RAW_WINDOW_FOCUS_EVENT.on(update) {
            if args.new_focus == Some(WINDOW.id()) {
                self.vars.0.children.with(|c| {
                    for &c in c.iter() {
                        let _ = WINDOWS.bring_to_top(c);
                    }
                });
            } else if let Some(new_focus) = args.new_focus {
                self.vars.0.children.with(|c| {
                    if c.contains(&new_focus) {
                        let _ = WINDOWS.bring_to_top(WINDOW.id());

                        for c in c.iter() {
                            if *c != new_focus {
                                let _ = WINDOWS.bring_to_top(WINDOW.id());
                            }
                        }

                        let _ = WINDOWS.bring_to_top(new_focus);
                    }
                });
            }
        } else if let Some(args) = MONITORS_CHANGED_EVENT.on(update) {
            if let Some(m) = &self.monitor {
                if args.removed.contains(&m.id()) {
                    self.monitor = None;
                    self.vars.0.actual_monitor.set(None);
                }
            }
            self.vars.monitor().update();
        } else if let Some(args) = RAW_WINDOW_OPEN_EVENT.on(update) {
            if args.window_id == WINDOW.id() {
                self.waiting_view = false;

                WINDOWS.set_renderer(args.window_id, args.window.renderer());

                self.window = Some(args.window.clone());
                self.vars.0.render_mode.set(args.data.render_mode);
                self.vars.state().set(args.data.state.state);
                self.actual_state = Some(args.data.state.state);
                self.vars.0.restore_state.set(args.data.state.restore_state);
                self.vars.0.restore_rect.set(args.data.state.restore_rect);
                self.vars.0.global_position.set(args.data.position.0);
                self.vars.0.actual_position.set(args.data.position.1);
                self.vars.0.actual_size.set(args.data.size);
                self.vars.0.actual_monitor.set(args.data.monitor);
                self.vars.0.scale_factor.set(args.data.scale_factor);

                self.state = Some(args.data.state.clone());
                self.system_color_scheme = Some(args.data.color_scheme);

                let scheme = self
                    .vars
                    .color_scheme()
                    .get()
                    .or_else(|| self.parent_color_scheme.as_ref().map(|t| t.get()))
                    .or(self.system_color_scheme)
                    .unwrap_or_default();
                self.vars.0.actual_color_scheme.set(scheme);

                UPDATES.layout_window(args.window_id).render_window(args.window_id);

                for update in mem::take(&mut self.delayed_view_updates) {
                    update(&args.window);
                }
            }
        } else if let Some(args) = RAW_COLOR_SCHEME_CHANGED_EVENT.on(update) {
            if args.window_id == WINDOW.id() {
                self.system_color_scheme = Some(args.color_scheme);

                let scheme = self
                    .vars
                    .color_scheme()
                    .get()
                    .or_else(|| self.parent_color_scheme.as_ref().map(|t| t.get()))
                    .or(self.system_color_scheme)
                    .unwrap_or_default();
                self.vars.0.actual_color_scheme.set(scheme);
            }
        } else if let Some(args) = RAW_WINDOW_OR_HEADLESS_OPEN_ERROR_EVENT.on(update) {
            let w_id = WINDOW.id();
            if args.window_id == w_id && self.window.is_none() && self.waiting_view {
                tracing::error!("view-process failed to open a window, {}", args.error);

                // was waiting view and failed, treat like a respawn.

                self.waiting_view = false;
                self.delayed_view_updates = vec![];
                self.respawned = true;

                UPDATES.layout_window(w_id).render_window(w_id);
            }
        } else if let Some(args) = ACCESS_INITED_EVENT.on(update) {
            if args.window_id == WINDOW.id() {
                tracing::info!("accessibility info enabled for {:?}", args.window_id);

                self.vars.0.access_enabled.set(true);
                UPDATES.update_info_window(args.window_id);
            }
        } else if let Some(args) = VIEW_PROCESS_INITED_EVENT.on(update) {
            if let Some(view) = &self.window {
                if view.renderer().generation() != Ok(args.generation) {
                    debug_assert!(args.is_respawn);

                    self.window = None;
                    self.waiting_view = false;
                    self.delayed_view_updates = vec![];
                    self.respawned = true;

                    let w_id = WINDOW.id();
                    UPDATES.layout_window(w_id).render_window(w_id);
                }
            }
        }

        self.content.pre_event(update);
    }

    pub fn ui_event(&mut self, update: &EventUpdate) {
        self.content.ui_event(update);
    }

    #[must_use]
    pub fn info(&mut self, info_widgets: Arc<InfoUpdates>) -> Option<WidgetInfoTree> {
        let info = self.content.info(info_widgets);
        if let (Some(info), Some(view)) = (&info, &self.window) {
            if info.access_enabled() {
                let info = info.to_access_tree();
                let root_id = info.root().id;
                let _ = view.access_update(zero_ui_view_api::access::AccessTreeUpdate {
                    updates: vec![info],
                    full_root: Some(root_id),
                    focused: root_id,
                });
            }
        }
        info
    }

    pub fn layout(&mut self, layout_widgets: Arc<LayoutUpdates>) {
        if !layout_widgets.delivery_list().enter_window(WINDOW.id()) {
            return;
        }

        if self.window.is_some() {
            if matches!(self.state.as_ref().map(|s| s.state), Some(WindowState::Minimized)) {
                return;
            }
            self.layout_update(layout_widgets);
        } else if self.respawned && !self.waiting_view {
            self.layout_respawn();
        } else if !self.waiting_view {
            self.layout_init();
        }
    }

    /// First layout, opens the window.
    fn layout_init(&mut self) {
        self.monitor = Some(self.vars.monitor().get().select_fallback());
        let m = self.monitor.as_ref().unwrap();
        self.vars.0.scale_factor.set(m.scale_factor().get());

        // await icon load for up to 1s.
        if let Some(icon) = &self.icon {
            if !self.icon_deadline.has_elapsed() && icon.get().is_loading() {
                // block on icon loading.
                return;
            }
        }
        // update window "load" state, `is_loaded` and the `WindowLoadEvent` happen here.
        if !WINDOWS.try_load(WINDOW.id()) {
            // block on loading handles.
            return;
        }

        let scale_factor = m.scale_factor().get();
        let screen_ppi = m.ppi().get();
        let screen_rect = m.px_rect();

        // Layout min, max and size in the monitor space.
        let (min_size, max_size, mut size, root_font_size) = self.content.outer_layout(scale_factor, screen_ppi, screen_rect.size, || {
            let min_size = self.vars.min_size().layout_dft(default_min_size(scale_factor));
            let max_size = self.vars.max_size().layout_dft(screen_rect.size);
            let size = self.vars.size().layout_dft(default_size(scale_factor));

            let font_size = self.vars.font_size().get();
            let mut root_font_size = font_size.layout_dft_x(Length::pt_to_px(11.0, scale_factor));
            if root_font_size < Px(0) {
                tracing::error!("invalid font size {font_size:?} => {root_font_size:?}");
                root_font_size = Length::pt_to_px(11.0, scale_factor);
            }

            (min_size, max_size, size.min(max_size).max(min_size), root_font_size)
        });

        self.root_font_size = root_font_size.to_dip(scale_factor.0);

        let state = self.vars.state().get();
        if state == WindowState::Normal && self.vars.auto_size().get() != AutoSize::DISABLED {
            // layout content to get auto-size size.
            size = self.content.layout(
                Arc::default(),
                scale_factor,
                screen_ppi,
                min_size,
                max_size,
                size,
                root_font_size,
                false,
            );
        }

        // Layout initial position in the monitor space.
        let mut system_pos = false;
        let position = match self.start_position {
            StartPosition::Default => {
                let pos = self.vars.position().get();
                if pos.x.is_default() || pos.y.is_default() {
                    system_pos = true;
                    screen_rect.origin + PxVector::splat(Px(40))
                } else {
                    self.content.outer_layout(scale_factor, screen_ppi, screen_rect.size, || {
                        pos.layout() + screen_rect.origin.to_vector()
                    })
                }
            }
            StartPosition::CenterMonitor => {
                PxPoint::new(
                    (screen_rect.size.width - size.width) / Px(2),
                    (screen_rect.size.height - size.height) / Px(2),
                ) + screen_rect.origin.to_vector()
            }
            StartPosition::CenterParent => {
                // center monitor if no parent
                let mut parent_rect = screen_rect;

                if let Some(parent) = self.vars.parent().get() {
                    if let Ok(w) = WINDOWS.vars(parent) {
                        let factor = w.scale_factor().get();
                        let pos = w.actual_position().get().to_px(factor.0);
                        let size = w.actual_size().get().to_px(factor.0);

                        parent_rect = PxRect::new(pos, size);
                    }
                }

                PxPoint::new(
                    (parent_rect.size.width - size.width) / Px(2),
                    (parent_rect.size.height - size.height) / Px(2),
                ) + parent_rect.origin.to_vector()
            }
        };

        // send view window request:

        let m_position = (position - screen_rect.origin.to_vector()).to_dip(scale_factor.0);
        let size = size.to_dip(scale_factor.0);

        let state = WindowStateAll {
            state,
            global_position: position,
            restore_rect: DipRect::new(m_position, size),
            restore_state: WindowState::Normal,
            min_size: min_size.to_dip(scale_factor.0),
            max_size: max_size.to_dip(scale_factor.0),
            chrome_visible: self.vars.chrome().get().is_default(),
        };

        let request = WindowRequest {
            id: crate::app::view_process::ApiWindowId::from_raw(WINDOW.id().get()),
            title: self.vars.title().get().to_string(),
            state: state.clone(),
            kiosk: self.kiosk.is_some(),
            default_position: system_pos,
            video_mode: self.vars.video_mode().get(),
            visible: self.vars.visible().get(),
            taskbar_visible: self.vars.taskbar_visible().get(),
            always_on_top: self.vars.always_on_top().get(),
            movable: self.vars.movable().get(),
            resizable: self.vars.resizable().get(),
            icon: self.icon.as_ref().and_then(|ico| ico.get().view().map(|ico| ico.id())).flatten(),
            cursor: self.vars.cursor().get(),
            transparent: self.transparent,
            capture_mode: matches!(self.vars.frame_capture_mode().get(), FrameCaptureMode::All),
            render_mode: self.render_mode.unwrap_or_else(|| WINDOWS.default_render_mode().get()),
            access_root: self.content.root_ctx.id().into(),

            focus: self.start_focused,
            focus_indicator: self.vars.focus_indicator().get(),

            extensions: {
                let mut exts = vec![];
                self.vars.renderer_debug().with(|d| d.push_extension(&mut exts));
                exts
            },
        };

        match VIEW_PROCESS.open_window(request) {
            Ok(()) => {
                self.state = Some(state);
                self.waiting_view = true;
            }
            Err(ViewProcessOffline) => {} //respawn
        };
    }

    /// Layout for already open window.
    fn layout_update(&mut self, layout_widgets: Arc<LayoutUpdates>) {
        let m = self.monitor.as_ref().unwrap();
        let scale_factor = m.scale_factor().get();
        let screen_ppi = m.ppi().get();

        let mut state = self.state.clone().unwrap();

        let current_size = self.vars.0.actual_size.get().to_px(scale_factor.0);
        let mut size = current_size;
        let min_size = state.min_size.to_px(scale_factor.0);
        let max_size = state.max_size.to_px(scale_factor.0);
        let root_font_size = self.root_font_size.to_px(scale_factor.0);

        let skip_auto_size = !matches!(state.state, WindowState::Normal);

        if !skip_auto_size {
            let auto_size = self.vars.auto_size().get();

            if auto_size.contains(AutoSize::CONTENT_WIDTH) {
                size.width = max_size.width;
            }
            if auto_size.contains(AutoSize::CONTENT_HEIGHT) {
                size.height = max_size.height;
            }
        }

        let size = self.content.layout(
            layout_widgets,
            scale_factor,
            screen_ppi,
            min_size,
            max_size,
            size,
            root_font_size,
            skip_auto_size,
        );

        if size != current_size {
            assert!(!skip_auto_size);

            let auto_size_origin = self.vars.auto_size_origin().get();
            let auto_size_origin = |size| {
                let metrics = LayoutMetrics::new(scale_factor, size, root_font_size)
                    .with_screen_ppi(screen_ppi)
                    .with_direction(DIRECTION_VAR.get());
                LAYOUT.with_context(metrics, || auto_size_origin.layout().to_dip(scale_factor.0))
            };
            let prev_origin = auto_size_origin(current_size);
            let new_origin = auto_size_origin(size);

            let size = size.to_dip(scale_factor.0);

            state.restore_rect.size = size;
            state.restore_rect.origin += prev_origin - new_origin;

            if let Some(view) = &self.window {
                let _: Ignore = view.set_state(state);
            } else {
                debug_assert!(self.respawned);
                self.state = Some(state);
            }
        }
    }

    /// First layout after respawn, opens the window but used previous sizes.
    fn layout_respawn(&mut self) {
        if self.monitor.is_none() {
            self.monitor = Some(self.vars.monitor().get().select_fallback());
            let m = self.monitor.as_ref().unwrap();
            self.vars.0.scale_factor.set(m.scale_factor().get());
        }

        self.layout_update(Arc::default());

        let request = WindowRequest {
            id: crate::app::view_process::ApiWindowId::from_raw(WINDOW.id().get()),
            title: self.vars.title().get_string(),
            state: self.state.clone().unwrap(),
            kiosk: self.kiosk.is_some(),
            default_position: false,
            video_mode: self.vars.video_mode().get(),
            visible: self.vars.visible().get(),
            taskbar_visible: self.vars.taskbar_visible().get(),
            always_on_top: self.vars.always_on_top().get(),
            movable: self.vars.movable().get(),
            resizable: self.vars.resizable().get(),
            icon: self.icon.as_ref().and_then(|ico| ico.get().view().map(|ico| ico.id())).flatten(),
            cursor: self.vars.cursor().get(),
            transparent: self.transparent,
            capture_mode: matches!(self.vars.frame_capture_mode().get(), FrameCaptureMode::All),
            render_mode: self.render_mode.unwrap_or_else(|| WINDOWS.default_render_mode().get()),

            focus: WINDOWS.is_focused(WINDOW.id()).unwrap_or(false),
            focus_indicator: self.vars.focus_indicator().get(),

            access_root: self.content.root_ctx.id().into(),

            extensions: {
                let mut exts = vec![];
                self.vars.renderer_debug().with(|d| d.push_extension(&mut exts));
                exts
            },
        };

        match VIEW_PROCESS.open_window(request) {
            Ok(()) => self.waiting_view = true,
            Err(ViewProcessOffline) => {} // respawn.
        }
    }

    pub fn render(&mut self, render_widgets: Arc<RenderUpdates>, render_update_widgets: Arc<RenderUpdates>) {
        let w_id = WINDOW.id();
        if !render_widgets.delivery_list().enter_window(w_id) && !render_update_widgets.delivery_list().enter_window(w_id) {
            return;
        }

        if let Some(view) = &self.window {
            if matches!(self.state.as_ref().map(|s| s.state), Some(WindowState::Minimized)) {
                return;
            }

            let scale_factor = self.monitor.as_ref().unwrap().scale_factor().get();
            self.content.render(
                Some(view.renderer()),
                scale_factor,
                self.resize_wait_id.take(),
                render_widgets,
                render_update_widgets,
            );
        }
    }

    pub fn focus(&mut self) {
        self.update_gen(|view| {
            let _ = view.focus();
        });
    }

    pub fn bring_to_top(&mut self) {
        self.update_gen(|view| {
            let _ = view.bring_to_top();
        });
    }

    pub fn close(&mut self) {
        self.content.close();
        self.window = None;
    }

    fn view_task(&mut self, task: Box<dyn FnOnce(Option<&ViewWindow>) + Send>) {
        if let Some(view) = &self.window {
            task(Some(view));
        } else if self.waiting_view {
            self.delayed_view_updates.push(Box::new(move |v| task(Some(v))));
        } else {
            task(None);
        }
    }
}

/// Respond to `parent_var` updates, returns `true` if the `parent` value has changed.
fn update_parent(parent: &mut Option<WindowId>, vars: &WindowVars) -> bool {
    let parent_var = vars.parent();
    if let Some(parent_id) = parent_var.get_new() {
        if parent_id == *parent {
            return false;
        }

        match parent_id {
            Some(mut parent_id) => {
                if parent_id == WINDOW.id() {
                    tracing::error!("cannot set `{:?}` as it's own parent", parent_id);
                    parent_var.set(*parent);
                    return false;
                }
                if !vars.0.children.with(|c| c.is_empty()) {
                    tracing::error!("cannot set parent for `{:?}` because it already has children", WINDOW.id());
                    parent_var.set(*parent);
                    return false;
                }

                if let Ok(parent_vars) = WINDOWS.vars(parent_id) {
                    // redirect to parent's parent.
                    if let Some(grand) = parent_vars.parent().get() {
                        tracing::debug!("using `{grand:?}` as parent, because it is the parent of requested `{parent_id:?}`");
                        parent_var.set(grand);

                        parent_id = grand;
                        if Some(parent_id) == *parent {
                            return false;
                        }
                    }

                    // remove previous
                    if let Some(parent_id) = parent.take() {
                        if let Ok(parent_vars) = WINDOWS.vars(parent_id) {
                            let id = WINDOW.id();
                            parent_vars.0.children.modify(move |c| {
                                c.to_mut().remove(&id);
                            });
                        }
                    }

                    // insert new
                    *parent = Some(parent_id);
                    let id = WINDOW.id();
                    parent_vars.0.children.modify(move |c| {
                        c.to_mut().insert(id);
                    });

                    true
                } else {
                    tracing::error!("cannot use `{:?}` as a parent because it does not exist", parent_id);
                    parent_var.set(*parent);
                    false
                }
            }
            None => {
                if let Some(parent_id) = parent.take() {
                    if let Ok(parent_vars) = WINDOWS.vars(parent_id) {
                        let id = WINDOW.id();
                        parent_vars.0.children.modify(move |c| {
                            c.to_mut().remove(&id);
                        });
                    }
                    true
                } else {
                    false
                }
            }
        }
    } else {
        false
    }
}

/// Implementer of `App <-> View` sync in a headless window.
struct HeadlessWithRendererCtrl {
    surface: Option<ViewHeadless>,
    waiting_view: bool,
    delayed_view_updates: Vec<Box<dyn FnOnce(&ViewHeadless) + Send>>,
    vars: WindowVars,
    content: ContentCtrl,

    // init config.
    render_mode: Option<RenderMode>,
    headless_monitor: HeadlessMonitor,
    headless_simulator: HeadlessSimulator,

    // current state.
    size: DipSize,

    actual_parent: Option<WindowId>,
    /// actual_color_scheme and scale_factor binding.
    var_bindings: VarHandles,
}
impl HeadlessWithRendererCtrl {
    pub fn new(vars: &WindowVars, commands: WindowCommands, content: WindowRoot) -> Self {
        Self {
            surface: None,
            waiting_view: false,
            delayed_view_updates: vec![],
            vars: vars.clone(),

            render_mode: content.render_mode,
            headless_monitor: content.headless_monitor,
            headless_simulator: HeadlessSimulator::new(),

            content: ContentCtrl::new(vars.clone(), commands, content),

            actual_parent: None,
            size: DipSize::zero(),
            var_bindings: VarHandles::dummy(),
        }
    }

    pub fn update(&mut self, update_widgets: &WidgetUpdates) {
        if self.surface.is_some() {
            if self.vars.size().is_new()
                || self.vars.min_size().is_new()
                || self.vars.max_size().is_new()
                || self.vars.auto_size().is_new()
                || self.vars.font_size().is_new()
            {
                UPDATES.layout_window(WINDOW.id());
            }
        } else {
            // we init on the first layout.
            UPDATES.layout_window(WINDOW.id());
        }

        if update_parent(&mut self.actual_parent, &self.vars) || self.var_bindings.is_dummy() {
            self.var_bindings = update_headless_vars(self.headless_monitor.scale_factor, &self.vars);
        }

        self.vars.renderer_debug().with_new(|dbg| {
            if let Some(view) = &self.surface {
                if let Some(key) = dbg.extension_id() {
                    let _ = view.renderer().render_extension::<_, ()>(key, dbg);
                }
            }
        });

        self.content.update(update_widgets);
    }

    #[must_use]
    pub fn info(&mut self, info_widgets: Arc<InfoUpdates>) -> Option<WidgetInfoTree> {
        self.content.info(info_widgets)
    }

    pub fn pre_event(&mut self, update: &EventUpdate) {
        if let Some(args) = RAW_HEADLESS_OPEN_EVENT.on(update) {
            if args.window_id == WINDOW.id() {
                self.waiting_view = false;

                WINDOWS.set_renderer(args.window_id, args.surface.renderer());

                self.surface = Some(args.surface.clone());
                self.vars.0.render_mode.set(args.data.render_mode);

                UPDATES.render_window(args.window_id);

                for update in mem::take(&mut self.delayed_view_updates) {
                    update(&args.surface);
                }
            }
        } else if let Some(args) = RAW_WINDOW_OR_HEADLESS_OPEN_ERROR_EVENT.on(update) {
            if args.window_id == WINDOW.id() && self.surface.is_none() && self.waiting_view {
                tracing::error!("view-process failed to open a headless surface, {}", args.error);

                // was waiting view and failed, treat like a respawn.

                self.waiting_view = false;
                self.delayed_view_updates = vec![];

                UPDATES.layout_window(args.window_id).render_window(args.window_id);
            }
        } else if let Some(args) = VIEW_PROCESS_INITED_EVENT.on(update) {
            if let Some(view) = &self.surface {
                if view.renderer().generation() != Ok(args.generation) {
                    debug_assert!(args.is_respawn);

                    self.surface = None;

                    let w_id = WINDOW.id();
                    UPDATES.layout_window(w_id).render_window(w_id);
                }
            }
        }

        self.content.pre_event(update);

        self.headless_simulator.pre_event(update);
    }

    pub fn ui_event(&mut self, update: &EventUpdate) {
        self.content.ui_event(update);
    }

    pub fn layout(&mut self, layout_widgets: Arc<LayoutUpdates>) {
        if !layout_widgets.delivery_list().enter_window(WINDOW.id()) {
            return;
        }

        let scale_factor = self.vars.0.scale_factor.get();
        let screen_ppi = self.headless_monitor.ppi;
        let screen_size = self.headless_monitor.size.to_px(scale_factor.0);

        let (min_size, max_size, size, root_font_size) = self.content.outer_layout(scale_factor, screen_ppi, screen_size, || {
            let min_size = self.vars.min_size().layout_dft(default_min_size(scale_factor));
            let max_size = self.vars.max_size().layout_dft(screen_size);
            let size = self.vars.size().layout_dft(default_size(scale_factor));
            let root_font_size = self.vars.font_size().layout_dft_x(Length::pt_to_px(11.0, scale_factor));

            (min_size, max_size, size.min(max_size).max(min_size), root_font_size)
        });

        let size = self.content.layout(
            layout_widgets,
            scale_factor,
            screen_ppi,
            min_size,
            max_size,
            size,
            root_font_size,
            false,
        );
        let size = size.to_dip(scale_factor.0);

        if let Some(view) = &self.surface {
            // already has surface, maybe resize:
            if self.size != size {
                self.size = size;
                let _: Ignore = view.set_size(size, scale_factor);
            }
        } else if !self.waiting_view {
            // (re)spawn the view surface:

            if !WINDOWS.try_load(WINDOW.id()) {
                return;
            }

            let render_mode = self.render_mode.unwrap_or_else(|| WINDOWS.default_render_mode().get());

            let r = VIEW_PROCESS.open_headless(HeadlessRequest {
                id: crate::app::view_process::ApiWindowId::from_raw(WINDOW.id().get()),
                scale_factor: scale_factor.0,
                size,
                render_mode,
                extensions: {
                    let mut exts = vec![];
                    self.vars.renderer_debug().with(|d| d.push_extension(&mut exts));
                    exts
                },
            });

            match r {
                Ok(()) => self.waiting_view = true,
                Err(ViewProcessOffline) => {} // respawn
            }
        }

        self.headless_simulator.layout();
    }

    pub fn render(&mut self, render_widgets: Arc<RenderUpdates>, render_update_widgets: Arc<RenderUpdates>) {
        let w_id = WINDOW.id();
        if !render_widgets.delivery_list().enter_window(w_id) && !render_update_widgets.delivery_list().enter_window(w_id) {
            return;
        }

        if let Some(view) = &self.surface {
            let fct = self.vars.0.scale_factor.get();
            self.content
                .render(Some(view.renderer()), fct, None, render_widgets, render_update_widgets);
        }
    }

    pub fn focus(&mut self) {
        self.headless_simulator.focus();
    }

    pub fn bring_to_top(&mut self) {
        self.headless_simulator.bring_to_top();
    }

    pub fn close(&mut self) {
        self.content.close();
        self.surface = None;
    }

    fn view_task(&mut self, task: Box<dyn FnOnce(Option<&ViewWindow>) + Send>) {
        task(None)
    }
}

fn update_headless_vars(mfactor: Option<Factor>, hvars: &WindowVars) -> VarHandles {
    let mut handles = VarHandles::dummy();

    if let Some(f) = mfactor {
        hvars.0.scale_factor.set(f);
    }

    if let Some(parent_vars) = hvars.parent().get().and_then(|id| WINDOWS.vars(id).ok()) {
        // bind parent factor
        if mfactor.is_none() {
            hvars.0.scale_factor.set_from(&parent_vars.0.scale_factor);
            handles.push(parent_vars.0.scale_factor.bind(&hvars.0.scale_factor));
        }

        // merge bind color scheme.
        let user = hvars.color_scheme();
        let parent = &parent_vars.0.actual_color_scheme;
        let actual = &hvars.0.actual_color_scheme;

        handles.push(user.hook(Box::new(clmv!(parent, actual, |args| {
            let value = *args.downcast_value::<Option<ColorScheme>>().unwrap();
            let scheme = value.unwrap_or_else(|| parent.get());
            actual.set(scheme);
            true
        }))));

        handles.push(parent.hook(Box::new(clmv!(user, actual, |args| {
            let scheme = user.get().unwrap_or_else(|| *args.downcast_value::<ColorScheme>().unwrap());
            actual.set(scheme);
            true
        }))));

        actual.modify(clmv!(user, parent, |a| {
            let value = user.get().unwrap_or_else(|| parent.get());
            a.set(value);
        }));
    } else {
        // set-bind color scheme
        let from = hvars.color_scheme();
        let to = &hvars.0.actual_color_scheme;

        to.set_from_map(&from, |&s| s.unwrap_or_default());
        handles.push(from.bind_map(to, |&s| s.unwrap_or_default()));
    }

    handles
}

/// implementer of `App` only content management.
struct HeadlessCtrl {
    vars: WindowVars,
    content: ContentCtrl,

    headless_monitor: HeadlessMonitor,
    headless_simulator: HeadlessSimulator,

    actual_parent: Option<WindowId>,
    /// actual_color_scheme and scale_factor binding.
    var_bindings: VarHandles,
}
impl HeadlessCtrl {
    pub fn new(vars: &WindowVars, commands: WindowCommands, content: WindowRoot) -> Self {
        Self {
            vars: vars.clone(),
            headless_monitor: content.headless_monitor,
            content: ContentCtrl::new(vars.clone(), commands, content),
            headless_simulator: HeadlessSimulator::new(),
            actual_parent: None,
            var_bindings: VarHandles::dummy(),
        }
    }

    pub fn update(&mut self, update_widgets: &WidgetUpdates) {
        if self.vars.size().is_new() || self.vars.min_size().is_new() || self.vars.max_size().is_new() || self.vars.auto_size().is_new() {
            UPDATES.layout_window(WINDOW.id());
        }

        if matches!(self.content.init_state, InitState::Init) {
            let w_id = WINDOW.id();
            UPDATES.layout_window(w_id).render_window(w_id);
        }

        if update_parent(&mut self.actual_parent, &self.vars) || self.var_bindings.is_dummy() {
            self.var_bindings = update_headless_vars(self.headless_monitor.scale_factor, &self.vars);
        }

        self.content.update(update_widgets);
    }

    #[must_use]
    pub fn info(&mut self, info_widgets: Arc<InfoUpdates>) -> Option<WidgetInfoTree> {
        self.content.info(info_widgets)
    }

    pub fn pre_event(&mut self, update: &EventUpdate) {
        self.content.pre_event(update);
        self.headless_simulator.pre_event(update);
    }

    pub fn ui_event(&mut self, update: &EventUpdate) {
        self.content.ui_event(update);
    }

    pub fn layout(&mut self, layout_widgets: Arc<LayoutUpdates>) {
        let w_id = WINDOW.id();
        if !layout_widgets.delivery_list().enter_window(w_id) {
            return;
        }

        if !WINDOWS.try_load(w_id) {
            return;
        }

        let scale_factor = self.vars.0.scale_factor.get();
        let screen_ppi = self.headless_monitor.ppi;
        let screen_size = self.headless_monitor.size.to_px(scale_factor.0);

        let (min_size, max_size, size, root_font_size) = self.content.outer_layout(scale_factor, screen_ppi, screen_size, || {
            let min_size = self.vars.min_size().layout_dft(default_min_size(scale_factor));
            let max_size = self.vars.max_size().layout_dft(screen_size);
            let size = self.vars.size().layout_dft(default_size(scale_factor));
            let root_font_size = self.vars.font_size().layout_dft_x(Length::pt_to_px(11.0, scale_factor));

            (min_size, max_size, size.min(max_size).max(min_size), root_font_size)
        });

        let _surface_size = self.content.layout(
            layout_widgets,
            scale_factor,
            screen_ppi,
            min_size,
            max_size,
            size,
            root_font_size,
            false,
        );

        self.headless_simulator.layout();
    }

    pub fn render(&mut self, render_widgets: Arc<RenderUpdates>, render_update_widgets: Arc<RenderUpdates>) {
        let w_id = WINDOW.id();
        if !render_widgets.delivery_list().enter_window(w_id) && !render_update_widgets.delivery_list().enter_window(w_id) {
            return;
        }

        // layout and render cannot happen yet
        if !WINDOWS.try_load(w_id) {
            return;
        }

        let fct = self.vars.0.scale_factor.get();
        self.content.render(None, fct, None, render_widgets, render_update_widgets);
    }

    pub fn focus(&mut self) {
        self.headless_simulator.focus();
    }

    pub fn bring_to_top(&mut self) {
        self.headless_simulator.bring_to_top();
    }

    pub fn close(&mut self) {
        self.content.close();
    }

    fn view_task(&mut self, task: Box<dyn FnOnce(Option<&ViewWindow>) + Send>) {
        task(None);
    }
}

/// Implementer of headless apps simulation of headed events for tests.
struct HeadlessSimulator {
    is_enabled: Option<bool>,
    is_open: bool,
}
impl HeadlessSimulator {
    fn new() -> Self {
        HeadlessSimulator {
            is_enabled: None,
            is_open: false,
        }
    }

    fn enabled(&mut self) -> bool {
        *self.is_enabled.get_or_insert_with(|| crate::app::App::window_mode().is_headless())
    }

    pub fn pre_event(&mut self, update: &EventUpdate) {
        if self.enabled() && self.is_open && VIEW_PROCESS_INITED_EVENT.on(update).map(|a| a.is_respawn).unwrap_or(false) {
            self.is_open = false;
        }
    }

    pub fn layout(&mut self) {
        if self.enabled() && !self.is_open {
            self.is_open = true;
            self.focus();
        }
    }

    pub fn focus(&mut self) {
        let mut prev = None;
        if let Some(id) = WINDOWS.focused_window_id() {
            prev = Some(id);
        }
        let args = RawWindowFocusArgs::now(prev, Some(WINDOW.id()));
        RAW_WINDOW_FOCUS_EVENT.notify(args);
    }

    pub fn bring_to_top(&mut self) {
        // we don't have "bring-to-top" event.
    }
}

#[derive(Clone, Copy)]
enum InitState {
    /// We let one update cycle happen before init
    /// to let the constructor closure setup vars
    /// that are read on init.
    SkipOne,
    Init,
    Inited,
}

/// Implementer of window UI node tree initialization and management.
struct ContentCtrl {
    vars: WindowVars,
    commands: WindowCommands,

    root_ctx: WidgetCtx,
    root: BoxedUiNode,
    layout_pass: LayoutPassId,

    init_state: InitState,
    frame_id: FrameId,
    clear_color: RenderColor,

    previous_transforms: IdMap<WidgetId, PxTransform>,
}
impl ContentCtrl {
    pub fn new(vars: WindowVars, commands: WindowCommands, window: WindowRoot) -> Self {
        Self {
            vars,
            commands,

            root_ctx: WidgetCtx::new(window.id),
            root: window.child,

            layout_pass: LayoutPassId::new(),

            init_state: InitState::SkipOne,
            frame_id: FrameId::INVALID,
            clear_color: RenderColor::BLACK,

            previous_transforms: IdMap::default(),
        }
    }

    pub fn update(&mut self, update_widgets: &WidgetUpdates) {
        match self.init_state {
            InitState::Inited => {
                self.commands.update(&self.vars);

                update_widgets.with_window(|| {
                    WIDGET.with_context(&mut self.root_ctx, WidgetUpdateMode::Bubble, || {
                        update_widgets.with_widget(|| {
                            self.root.update(update_widgets);
                        });
                    });
                });
            }

            InitState::SkipOne => {
                UPDATES.update(None);
                self.init_state = InitState::Init;
            }
            InitState::Init => {
                self.commands.init(&self.vars);
                WIDGET.with_context(&mut self.root_ctx, WidgetUpdateMode::Bubble, || {
                    self.root.init();
                    // requests info, layout and render just in case `root` is a blank.
                    WIDGET.update_info().layout().render();

                    super::WINDOW_OPEN_EVENT.notify(super::WindowOpenArgs::now(WINDOW.id()));
                });
                self.init_state = InitState::Inited;
            }
        }
    }

    #[must_use]
    pub fn info(&mut self, info_widgets: Arc<InfoUpdates>) -> Option<WidgetInfoTree> {
        let win_id = WINDOW.id();
        if info_widgets.delivery_list().enter_window(win_id) {
            let mut info = WidgetInfoBuilder::new(
                info_widgets,
                win_id,
                self.vars.0.access_enabled.get(),
                self.root_ctx.id(),
                self.root_ctx.bounds(),
                self.root_ctx.border(),
                self.vars.0.scale_factor.get(),
            );

            WIDGET.with_context(&mut self.root_ctx, WidgetUpdateMode::Bubble, || {
                self.root.info(&mut info);
            });

            let info = info.finalize(Some(WINDOW.info()));

            WINDOWS.set_widget_tree(info.clone());

            Some(info)
        } else {
            None
        }
    }

    pub fn pre_event(&mut self, update: &EventUpdate) {
        if let Some(args) = RAW_FRAME_RENDERED_EVENT.on(update) {
            if args.window_id == WINDOW.id() {
                let image = args.frame_image.as_ref().cloned().map(Img::new);

                let args = FrameImageReadyArgs::new(args.timestamp, args.propagation().clone(), args.window_id, args.frame_id, image);
                FRAME_IMAGE_READY_EVENT.notify(args);
            }
        } else {
            self.commands.event(&self.vars, update);
        }
    }

    pub fn ui_event(&mut self, update: &EventUpdate) {
        debug_assert!(matches!(self.init_state, InitState::Inited));

        update.with_window(|| {
            WIDGET.with_context(&mut self.root_ctx, WidgetUpdateMode::Bubble, || {
                update.with_widget(|| {
                    self.root.event(update);
                })
            });
        });
    }

    pub fn close(&mut self) {
        WIDGET.with_context(&mut self.root_ctx, WidgetUpdateMode::Bubble, || {
            self.root.deinit();
        });

        self.vars.0.is_open.set(false);
        self.root_ctx.deinit(false);
    }

    /// Run an `action` in the context of a monitor screen that is parent of this content.
    pub fn outer_layout<R>(&mut self, scale_factor: Factor, screen_ppi: Ppi, screen_size: PxSize, action: impl FnOnce() -> R) -> R {
        let metrics = LayoutMetrics::new(scale_factor, screen_size, Length::pt_to_px(11.0, scale_factor))
            .with_screen_ppi(screen_ppi)
            .with_direction(DIRECTION_VAR.get());
        LAYOUT.with_context(metrics, action)
    }

    /// Layout content if there was a pending request, returns `Some(final_size)`.
    #[allow(clippy::too_many_arguments)]
    pub fn layout(
        &mut self,
        layout_widgets: Arc<LayoutUpdates>,
        scale_factor: Factor,
        screen_ppi: Ppi,
        min_size: PxSize,
        max_size: PxSize,
        size: PxSize,
        root_font_size: Px,
        skip_auto_size: bool,
    ) -> PxSize {
        debug_assert!(matches!(self.init_state, InitState::Inited));

        let _s = tracing::trace_span!("window.on_layout", window = %WINDOW.id().sequential()).entered();

        let auto_size = self.vars.auto_size().get();

        let mut viewport_size = size;
        if !skip_auto_size {
            if auto_size.contains(AutoSize::CONTENT_WIDTH) {
                viewport_size.width = max_size.width;
            }
            if auto_size.contains(AutoSize::CONTENT_HEIGHT) {
                viewport_size.height = max_size.height;
            }
        }

        self.layout_pass = self.layout_pass.next();

        WIDGET.with_context(&mut self.root_ctx, WidgetUpdateMode::Bubble, || {
            let metrics = LayoutMetrics::new(scale_factor, viewport_size, root_font_size)
                .with_screen_ppi(screen_ppi)
                .with_direction(DIRECTION_VAR.get());
            LAYOUT.with_root_context(self.layout_pass, metrics, || {
                let mut root_cons = LAYOUT.constraints();
                if !skip_auto_size {
                    if auto_size.contains(AutoSize::CONTENT_WIDTH) {
                        root_cons = root_cons.with_unbounded_x();
                    }
                    if auto_size.contains(AutoSize::CONTENT_HEIGHT) {
                        root_cons = root_cons.with_unbounded_y();
                    }
                }
                let desired_size = LAYOUT.with_constraints(root_cons, || {
                    WidgetLayout::with_root_widget(layout_widgets, |wl| self.root.layout(wl))
                });

                let mut final_size = viewport_size;
                if !skip_auto_size {
                    if auto_size.contains(AutoSize::CONTENT_WIDTH) {
                        final_size.width = desired_size.width.max(min_size.width).min(max_size.width);
                    }
                    if auto_size.contains(AutoSize::CONTENT_HEIGHT) {
                        final_size.height = desired_size.height.max(min_size.height).min(max_size.height);
                    }
                }

                final_size
            })
        })
    }

    pub fn render(
        &mut self,
        renderer: Option<ViewRenderer>,
        scale_factor: Factor,
        wait_id: Option<FrameWaitId>,
        render_widgets: Arc<RenderUpdates>,
        render_update_widgets: Arc<RenderUpdates>,
    ) {
        let w_id = WINDOW.id();
        if render_widgets.delivery_list().enter_window(w_id) {
            // RENDER FULL FRAME
            let _s = tracing::trace_span!("window.on_render", window = %WINDOW.id().sequential()).entered();

            self.frame_id = self.frame_id.next();

            let default_text_aa = FONTS.system_font_aa().get();

            let mut frame = FrameBuilder::new(
                render_widgets,
                render_update_widgets,
                self.frame_id,
                self.root_ctx.id(),
                &self.root_ctx.bounds(),
                &WINDOW.info(),
                renderer.clone(),
                scale_factor,
                default_text_aa,
            );

            let frame = WIDGET.with_context(&mut self.root_ctx, WidgetUpdateMode::Bubble, || {
                self.root.render(&mut frame);
                frame.finalize(&WINDOW.info())
            });

            self.notify_transform_changes();

            self.clear_color = frame.clear_color;

            let capture = self.take_frame_capture();

            if let Some(renderer) = renderer {
                let _: Ignore = renderer.render(FrameRequest {
                    id: self.frame_id,
                    pipeline_id: frame.display_list.pipeline_id(),
                    clear_color: self.clear_color,
                    display_list: frame.display_list,
                    capture,
                    wait_id,
                });
            } else {
                // simulate frame in headless
                FRAME_IMAGE_READY_EVENT.notify(FrameImageReadyArgs::now(WINDOW.id(), self.frame_id, None));
            }
        } else if render_update_widgets.delivery_list().enter_window(w_id) {
            // RENDER UPDATE
            let _s = tracing::trace_span!("window.on_render_update", window = %WINDOW.id().sequential()).entered();

            self.frame_id = self.frame_id.next_update();

            let mut update = FrameUpdate::new(
                render_update_widgets,
                self.frame_id,
                self.root_ctx.id(),
                self.root_ctx.bounds(),
                renderer.as_ref(),
                self.clear_color,
            );

            let update = WIDGET.with_context(&mut self.root_ctx, WidgetUpdateMode::Bubble, || {
                self.root.render_update(&mut update);
                update.finalize(&WINDOW.info())
            });

            self.notify_transform_changes();

            if let Some(c) = update.clear_color {
                self.clear_color = c;
            }

            let capture = self.take_frame_capture();

            if let Some(renderer) = renderer {
                let _: Ignore = renderer.render_update(FrameUpdateRequest {
                    id: self.frame_id,
                    transforms: update.transforms,
                    floats: update.floats,
                    colors: update.colors,
                    clear_color: update.clear_color,
                    extensions: update.extensions,
                    capture,
                    wait_id,
                });
            } else {
                // simulate frame in headless
                FRAME_IMAGE_READY_EVENT.notify(FrameImageReadyArgs::now(WINDOW.id(), self.frame_id, None));
            }
        }
    }
    fn take_frame_capture(&self) -> FrameCapture {
        match self.vars.frame_capture_mode().get() {
            FrameCaptureMode::Sporadic => FrameCapture::None,
            FrameCaptureMode::Next => {
                self.vars.frame_capture_mode().set(FrameCaptureMode::Sporadic);
                FrameCapture::Full
            }
            FrameCaptureMode::All => FrameCapture::Full,
            FrameCaptureMode::NextMask(m) => {
                self.vars.frame_capture_mode().set(FrameCaptureMode::Sporadic);
                FrameCapture::Mask(m)
            }
            FrameCaptureMode::AllMask(m) => FrameCapture::Mask(m),
        }
    }

    fn notify_transform_changes(&mut self) {
        let mut changes_count = 0;

        TRANSFORM_CHANGED_EVENT.visit_subscribers(|wid| {
            let tree = WINDOW.info();
            if let Some(wgt) = tree.get(wid) {
                let transform = wgt.bounds_info().inner_transform();

                match self.previous_transforms.entry(wid) {
                    IdEntry::Occupied(mut e) => {
                        let prev = e.insert(transform);
                        if prev != transform {
                            TRANSFORM_CHANGED_EVENT.notify(TransformChangedArgs::now(wgt.path(), prev, transform));
                            changes_count += 1;
                        }
                    }
                    IdEntry::Vacant(e) => {
                        e.insert(transform);
                    }
                }
            }
        });

        if (self.previous_transforms.len() - changes_count) > 500 {
            self.previous_transforms.retain(|k, _| TRANSFORM_CHANGED_EVENT.is_subscriber(*k));
        }
    }
}

/// Management of window content and synchronization of WindowVars and View-Process.
pub(super) struct WindowCtrl(WindowCtrlMode);
enum WindowCtrlMode {
    Headed(HeadedCtrl),
    Headless(HeadlessCtrl),
    HeadlessWithRenderer(HeadlessWithRendererCtrl),
}
impl WindowCtrl {
    pub fn new(vars: &WindowVars, commands: WindowCommands, mode: WindowMode, content: WindowRoot) -> Self {
        WindowCtrl(match mode {
            WindowMode::Headed => WindowCtrlMode::Headed(HeadedCtrl::new(vars, commands, content)),
            WindowMode::Headless => WindowCtrlMode::Headless(HeadlessCtrl::new(vars, commands, content)),
            WindowMode::HeadlessWithRenderer => {
                WindowCtrlMode::HeadlessWithRenderer(HeadlessWithRendererCtrl::new(vars, commands, content))
            }
        })
    }

    pub fn update(&mut self, update_widgets: &WidgetUpdates) {
        match &mut self.0 {
            WindowCtrlMode::Headed(c) => c.update(update_widgets),
            WindowCtrlMode::Headless(c) => c.update(update_widgets),
            WindowCtrlMode::HeadlessWithRenderer(c) => c.update(update_widgets),
        }
    }

    #[must_use]
    pub fn info(&mut self, info_widgets: Arc<InfoUpdates>) -> Option<WidgetInfoTree> {
        match &mut self.0 {
            WindowCtrlMode::Headed(c) => c.info(info_widgets),
            WindowCtrlMode::Headless(c) => c.info(info_widgets),
            WindowCtrlMode::HeadlessWithRenderer(c) => c.info(info_widgets),
        }
    }

    pub fn pre_event(&mut self, update: &EventUpdate) {
        match &mut self.0 {
            WindowCtrlMode::Headed(c) => c.pre_event(update),
            WindowCtrlMode::Headless(c) => c.pre_event(update),
            WindowCtrlMode::HeadlessWithRenderer(c) => c.pre_event(update),
        }
    }

    pub fn ui_event(&mut self, update: &EventUpdate) {
        match &mut self.0 {
            WindowCtrlMode::Headed(c) => c.ui_event(update),
            WindowCtrlMode::Headless(c) => c.ui_event(update),
            WindowCtrlMode::HeadlessWithRenderer(c) => c.ui_event(update),
        }
    }

    pub fn layout(&mut self, layout_widgets: Arc<LayoutUpdates>) {
        match &mut self.0 {
            WindowCtrlMode::Headed(c) => c.layout(layout_widgets),
            WindowCtrlMode::Headless(c) => c.layout(layout_widgets),
            WindowCtrlMode::HeadlessWithRenderer(c) => c.layout(layout_widgets),
        }
    }

    pub fn render(&mut self, render_widgets: Arc<RenderUpdates>, render_update_widgets: Arc<RenderUpdates>) {
        match &mut self.0 {
            WindowCtrlMode::Headed(c) => c.render(render_widgets, render_update_widgets),
            WindowCtrlMode::Headless(c) => c.render(render_widgets, render_update_widgets),
            WindowCtrlMode::HeadlessWithRenderer(c) => c.render(render_widgets, render_update_widgets),
        }
    }

    pub fn focus(&mut self) {
        match &mut self.0 {
            WindowCtrlMode::Headed(c) => c.focus(),
            WindowCtrlMode::Headless(c) => c.focus(),
            WindowCtrlMode::HeadlessWithRenderer(c) => c.focus(),
        }
    }

    pub fn bring_to_top(&mut self) {
        match &mut self.0 {
            WindowCtrlMode::Headed(c) => c.bring_to_top(),
            WindowCtrlMode::Headless(c) => c.bring_to_top(),
            WindowCtrlMode::HeadlessWithRenderer(c) => c.bring_to_top(),
        }
    }

    pub fn close(&mut self) {
        match &mut self.0 {
            WindowCtrlMode::Headed(c) => c.close(),
            WindowCtrlMode::Headless(c) => c.close(),
            WindowCtrlMode::HeadlessWithRenderer(c) => c.close(),
        }
    }

    pub(crate) fn view_task(&mut self, task: Box<dyn FnOnce(Option<&ViewWindow>) + Send>) {
        match &mut self.0 {
            WindowCtrlMode::Headed(c) => c.view_task(task),
            WindowCtrlMode::Headless(c) => c.view_task(task),
            WindowCtrlMode::HeadlessWithRenderer(c) => c.view_task(task),
        }
    }
}

fn default_min_size(scale_factor: Factor) -> PxSize {
    DipSize::new(Dip::new(192), Dip::new(48)).to_px(scale_factor.0)
}

fn default_size(scale_factor: Factor) -> PxSize {
    DipSize::new(Dip::new(800), Dip::new(600)).to_px(scale_factor.0)
}

/// Respawned error is ok here, because we recreate the window/surface on respawn.
type Ignore = Result<(), ViewProcessOffline>;
