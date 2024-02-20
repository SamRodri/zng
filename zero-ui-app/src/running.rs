use std::{
    fmt, mem,
    path::PathBuf,
    sync::Arc,
    task::Waker,
    time::{Duration, Instant},
};

use crate::Deadline;
use zero_ui_app_context::{app_local, AppScope};
use zero_ui_task::DEADLINE_APP;
use zero_ui_time::{InstantMode, INSTANT_APP};
use zero_ui_var::{response_var, ArcVar, ResponderVar, ResponseVar, Var as _, VARS, VARS_APP};

use crate::{
    event::{
        command, event, AnyEventArgs, AppDisconnected, CommandHandle, CommandInfoExt, CommandNameExt, EventPropagationHandle,
        TimeoutOrAppDisconnected, EVENTS,
    },
    event_args,
    shortcut::shortcut,
    shortcut::CommandShortcutExt,
    timer::TimersService,
    update::{
        ContextUpdates, EventUpdate, InfoUpdates, LayoutUpdates, RenderUpdates, UpdateOp, UpdateTrace, UpdatesTrace, WidgetUpdates, UPDATES,
    },
    view_process::{raw_device_events::DeviceId, *},
    widget::WidgetId,
    window::WindowId,
    AppControlFlow, AppEventObserver, AppExtension, AppExtensionsInfo, DInstant, APP, INSTANT,
};

/// Represents a running app controlled by an external event loop.
pub(crate) struct RunningApp<E: AppExtension> {
    extensions: (AppIntrinsic, E),

    receiver: flume::Receiver<AppEvent>,

    loop_timer: LoopTimer,
    loop_monitor: LoopMonitor,

    pending_view_events: Vec<zero_ui_view_api::Event>,
    pending_view_frame_events: Vec<zero_ui_view_api::window::EventFrameRendered>,
    pending: ContextUpdates,

    exited: bool,

    // cleans on drop
    _scope: AppScope,
}
impl<E: AppExtension> RunningApp<E> {
    pub(crate) fn start(
        scope: AppScope,
        mut extensions: E,
        is_headed: bool,
        with_renderer: bool,
        view_process_exe: Option<PathBuf>,
    ) -> Self {
        let _s = tracing::debug_span!("APP::start").entered();

        let (sender, receiver) = AppEventSender::new();

        UPDATES.init(sender);

        fn app_waker() {
            UPDATES.update(None);
        }
        VARS_APP.init_app_waker(app_waker);
        VARS_APP.init_modify_trace(UpdatesTrace::log_var);
        DEADLINE_APP.init_deadline_service(crate::timer::deadline_service);
        zero_ui_var::types::TRANSITIONABLE_APP.init_rgba_lerp(zero_ui_color::lerp_rgba);

        let mut info = AppExtensionsInfo::start();
        {
            let _t = INSTANT_APP.pause_for_update();
            extensions.register(&mut info);
        }
        let device_events = extensions.enable_device_events();

        {
            let mut sv = APP_PROCESS_SV.write();
            sv.set_extensions(info, device_events);
        }

        let process = AppIntrinsic::pre_init(is_headed, with_renderer, view_process_exe, device_events);

        {
            let _s = tracing::debug_span!("extensions.init").entered();
            extensions.init();
        }

        RunningApp {
            extensions: (process, extensions),

            receiver,

            loop_timer: LoopTimer::default(),
            loop_monitor: LoopMonitor::default(),

            pending_view_events: Vec::with_capacity(100),
            pending_view_frame_events: Vec::with_capacity(5),
            pending: ContextUpdates {
                events: Vec::with_capacity(100),
                update: false,
                info: false,
                layout: false,
                render: false,
                update_widgets: WidgetUpdates::default(),
                info_widgets: InfoUpdates::default(),
                layout_widgets: LayoutUpdates::default(),
                render_widgets: RenderUpdates::default(),
                render_update_widgets: RenderUpdates::default(),
            },
            exited: false,

            _scope: scope,
        }
    }

    pub fn has_exited(&self) -> bool {
        self.exited
    }

    /// Notify an event directly to the app extensions.
    pub fn notify_event<O: AppEventObserver>(&mut self, mut update: EventUpdate, observer: &mut O) {
        let _scope = tracing::trace_span!("notify_event", event = update.event().name()).entered();

        let _t = INSTANT_APP.pause_for_update();

        update.event().on_update(&mut update);

        self.extensions.event_preview(&mut update);
        observer.event_preview(&mut update);
        update.call_pre_actions();

        self.extensions.event_ui(&mut update);
        observer.event_ui(&mut update);

        self.extensions.event(&mut update);
        observer.event(&mut update);
        update.call_pos_actions();
    }

    fn device_id(&mut self, id: zero_ui_view_api::DeviceId) -> DeviceId {
        VIEW_PROCESS.device_id(id)
    }

    /// Process a View Process event.
    fn on_view_event<O: AppEventObserver>(&mut self, ev: zero_ui_view_api::Event, observer: &mut O) {
        use crate::view_process::raw_device_events::*;
        use crate::view_process::raw_events::*;
        use zero_ui_view_api::Event;

        fn window_id(id: zero_ui_view_api::window::WindowId) -> WindowId {
            WindowId::from_raw(id.get())
        }

        match ev {
            Event::MouseMoved {
                window: w_id,
                device: d_id,
                coalesced_pos,
                position,
            } => {
                let args = RawMouseMovedArgs::now(window_id(w_id), self.device_id(d_id), coalesced_pos, position);
                self.notify_event(RAW_MOUSE_MOVED_EVENT.new_update(args), observer);
            }
            Event::MouseEntered {
                window: w_id,
                device: d_id,
            } => {
                let args = RawMouseArgs::now(window_id(w_id), self.device_id(d_id));
                self.notify_event(RAW_MOUSE_ENTERED_EVENT.new_update(args), observer);
            }
            Event::MouseLeft {
                window: w_id,
                device: d_id,
            } => {
                let args = RawMouseArgs::now(window_id(w_id), self.device_id(d_id));
                self.notify_event(RAW_MOUSE_LEFT_EVENT.new_update(args), observer);
            }
            Event::WindowChanged(c) => {
                let monitor_id = c.monitor.map(|id| VIEW_PROCESS.monitor_id(id));
                let args = RawWindowChangedArgs::now(
                    window_id(c.window),
                    c.state,
                    c.position,
                    monitor_id,
                    c.size,
                    c.cause,
                    c.frame_wait_id,
                );
                self.notify_event(RAW_WINDOW_CHANGED_EVENT.new_update(args), observer);
            }
            Event::DroppedFile { window: w_id, file } => {
                let args = RawDroppedFileArgs::now(window_id(w_id), file);
                self.notify_event(RAW_DROPPED_FILE_EVENT.new_update(args), observer);
            }
            Event::HoveredFile { window: w_id, file } => {
                let args = RawHoveredFileArgs::now(window_id(w_id), file);
                self.notify_event(RAW_HOVERED_FILE_EVENT.new_update(args), observer);
            }
            Event::HoveredFileCancelled(w_id) => {
                let args = RawHoveredFileCancelledArgs::now(window_id(w_id));
                self.notify_event(RAW_HOVERED_FILE_CANCELLED_EVENT.new_update(args), observer);
            }
            Event::FocusChanged { prev, new } => {
                let args = RawWindowFocusArgs::now(prev.map(window_id), new.map(window_id));
                self.notify_event(RAW_WINDOW_FOCUS_EVENT.new_update(args), observer);
            }
            Event::KeyboardInput {
                window: w_id,
                device: d_id,
                key_code,
                state,
                key,
                key_modified,
                text,
            } => {
                let args = RawKeyInputArgs::now(window_id(w_id), self.device_id(d_id), key_code, state, key, key_modified, text);
                self.notify_event(RAW_KEY_INPUT_EVENT.new_update(args), observer);
            }
            Event::Ime { window: w_id, ime } => {
                let args = RawImeArgs::now(window_id(w_id), ime);
                self.notify_event(RAW_IME_EVENT.new_update(args), observer);
            }

            Event::MouseWheel {
                window: w_id,
                device: d_id,
                delta,
                phase,
            } => {
                let args = RawMouseWheelArgs::now(window_id(w_id), self.device_id(d_id), delta, phase);
                self.notify_event(RAW_MOUSE_WHEEL_EVENT.new_update(args), observer);
            }
            Event::MouseInput {
                window: w_id,
                device: d_id,
                state,
                button,
            } => {
                let args = RawMouseInputArgs::now(window_id(w_id), self.device_id(d_id), state, button);
                self.notify_event(RAW_MOUSE_INPUT_EVENT.new_update(args), observer);
            }
            Event::TouchpadPressure {
                window: w_id,
                device: d_id,
                pressure,
                stage,
            } => {
                let args = RawTouchpadPressureArgs::now(window_id(w_id), self.device_id(d_id), pressure, stage);
                self.notify_event(RAW_TOUCHPAD_PRESSURE_EVENT.new_update(args), observer);
            }
            Event::AxisMotion {
                window: w_id,
                device: d_id,
                axis,
                value,
            } => {
                let args = RawAxisMotionArgs::now(window_id(w_id), self.device_id(d_id), axis, value);
                self.notify_event(RAW_AXIS_MOTION_EVENT.new_update(args), observer);
            }
            Event::Touch {
                window: w_id,
                device: d_id,
                touches,
            } => {
                let args = RawTouchArgs::now(window_id(w_id), self.device_id(d_id), touches);
                self.notify_event(RAW_TOUCH_EVENT.new_update(args), observer);
            }
            Event::ScaleFactorChanged {
                monitor: id,
                windows,
                scale_factor,
            } => {
                let monitor_id = VIEW_PROCESS.monitor_id(id);
                let windows: Vec<_> = windows.into_iter().map(window_id).collect();
                let args = RawScaleFactorChangedArgs::now(monitor_id, windows, scale_factor);
                self.notify_event(RAW_SCALE_FACTOR_CHANGED_EVENT.new_update(args), observer);
            }
            Event::MonitorsChanged(monitors) => {
                let monitors: Vec<_> = monitors.into_iter().map(|(id, info)| (VIEW_PROCESS.monitor_id(id), info)).collect();
                let args = RawMonitorsChangedArgs::now(monitors);
                self.notify_event(RAW_MONITORS_CHANGED_EVENT.new_update(args), observer);
            }
            Event::ColorSchemeChanged(w_id, scheme) => {
                let args = RawColorSchemeChangedArgs::now(window_id(w_id), scheme);
                self.notify_event(RAW_COLOR_SCHEME_CHANGED_EVENT.new_update(args), observer);
            }
            Event::WindowCloseRequested(w_id) => {
                let args = RawWindowCloseRequestedArgs::now(window_id(w_id));
                self.notify_event(RAW_WINDOW_CLOSE_REQUESTED_EVENT.new_update(args), observer);
            }
            Event::WindowOpened(w_id, data) => {
                let w_id = window_id(w_id);
                let (window, data) = VIEW_PROCESS.on_window_opened(w_id, data);
                let args = RawWindowOpenArgs::now(w_id, window, data);
                self.notify_event(RAW_WINDOW_OPEN_EVENT.new_update(args), observer);
            }
            Event::HeadlessOpened(w_id, data) => {
                let w_id = window_id(w_id);
                let (surface, data) = VIEW_PROCESS.on_headless_opened(w_id, data);
                let args = RawHeadlessOpenArgs::now(w_id, surface, data);
                self.notify_event(RAW_HEADLESS_OPEN_EVENT.new_update(args), observer);
            }
            Event::WindowOrHeadlessOpenError { id: w_id, error } => {
                let w_id = window_id(w_id);
                let args = RawWindowOrHeadlessOpenErrorArgs::now(w_id, error);
                self.notify_event(RAW_WINDOW_OR_HEADLESS_OPEN_ERROR_EVENT.new_update(args), observer);
            }
            Event::WindowClosed(w_id) => {
                let args = RawWindowCloseArgs::now(window_id(w_id));
                self.notify_event(RAW_WINDOW_CLOSE_EVENT.new_update(args), observer);
            }
            Event::ImageMetadataLoaded {
                image: id,
                size,
                ppi,
                is_mask,
            } => {
                if let Some(img) = VIEW_PROCESS.on_image_metadata_loaded(id, size, ppi, is_mask) {
                    let args = RawImageArgs::now(img);
                    self.notify_event(RAW_IMAGE_METADATA_LOADED_EVENT.new_update(args), observer);
                }
            }
            Event::ImagePartiallyLoaded {
                image: id,
                partial_size,
                ppi,
                is_opaque,
                is_mask,
                partial_pixels: partial_bgra8,
            } => {
                if let Some(img) = VIEW_PROCESS.on_image_partially_loaded(id, partial_size, ppi, is_opaque, is_mask, partial_bgra8) {
                    let args = RawImageArgs::now(img);
                    self.notify_event(RAW_IMAGE_PARTIALLY_LOADED_EVENT.new_update(args), observer);
                }
            }
            Event::ImageLoaded(image) => {
                if let Some(img) = VIEW_PROCESS.on_image_loaded(image) {
                    let args = RawImageArgs::now(img);
                    self.notify_event(RAW_IMAGE_LOADED_EVENT.new_update(args), observer);
                }
            }
            Event::ImageLoadError { image: id, error } => {
                if let Some(img) = VIEW_PROCESS.on_image_error(id, error) {
                    let args = RawImageArgs::now(img);
                    self.notify_event(RAW_IMAGE_LOAD_ERROR_EVENT.new_update(args), observer);
                }
            }
            Event::ImageEncoded { image: id, format, data } => VIEW_PROCESS.on_image_encoded(id, format, data),
            Event::ImageEncodeError { image: id, format, error } => {
                VIEW_PROCESS.on_image_encode_error(id, format, error);
            }
            Event::FrameImageReady {
                window: w_id,
                frame: frame_id,
                image: image_id,
                selection,
            } => {
                if let Some(img) = VIEW_PROCESS.on_frame_image_ready(image_id) {
                    let args = RawFrameImageReadyArgs::now(img, window_id(w_id), frame_id, selection);
                    self.notify_event(RAW_FRAME_IMAGE_READY_EVENT.new_update(args), observer);
                }
            }

            Event::AccessInit { window: w_id } => {
                self.notify_event(crate::access::on_access_init(window_id(w_id)), observer);
            }
            Event::AccessCommand {
                window: win_id,
                target: wgt_id,
                command,
            } => {
                if let Some(update) = crate::access::on_access_command(window_id(win_id), WidgetId::from_raw(wgt_id.0), command) {
                    self.notify_event(update, observer);
                }
            }

            // native dialog responses
            Event::MsgDialogResponse(id, response) => {
                VIEW_PROCESS.on_message_dlg_response(id, response);
            }
            Event::FileDialogResponse(id, response) => {
                VIEW_PROCESS.on_file_dlg_response(id, response);
            }

            // custom
            Event::ExtensionEvent(id, payload) => {
                let args = RawExtensionEventArgs::now(id, payload);
                self.notify_event(RAW_EXTENSION_EVENT.new_update(args), observer);
            }

            // config events
            Event::FontsChanged => {
                let args = RawFontChangedArgs::now();
                self.notify_event(RAW_FONT_CHANGED_EVENT.new_update(args), observer);
            }
            Event::FontAaChanged(aa) => {
                let args = RawFontAaChangedArgs::now(aa);
                self.notify_event(RAW_FONT_AA_CHANGED_EVENT.new_update(args), observer);
            }
            Event::MultiClickConfigChanged(cfg) => {
                let args = RawMultiClickConfigChangedArgs::now(cfg);
                self.notify_event(RAW_MULTI_CLICK_CONFIG_CHANGED_EVENT.new_update(args), observer);
            }
            Event::AnimationsConfigChanged(cfg) => {
                VARS.set_animations_enabled(cfg.enabled);
                let args = RawAnimationsConfigChangedArgs::now(cfg);
                self.notify_event(RAW_ANIMATIONS_CONFIG_CHANGED_EVENT.new_update(args), observer);
            }
            Event::KeyRepeatConfigChanged(cfg) => {
                let args = RawKeyRepeatConfigChangedArgs::now(cfg);
                self.notify_event(RAW_KEY_REPEAT_CONFIG_CHANGED_EVENT.new_update(args), observer);
            }
            Event::TouchConfigChanged(cfg) => {
                let args = RawTouchConfigChangedArgs::now(cfg);
                self.notify_event(RAW_TOUCH_CONFIG_CHANGED_EVENT.new_update(args), observer);
            }
            Event::LocaleChanged(cfg) => {
                let args = RawLocaleChangedArgs::now(cfg);
                self.notify_event(RAW_LOCALE_CONFIG_CHANGED_EVENT.new_update(args), observer);
            }

            // `device_events`
            Event::DeviceAdded(d_id) => {
                let args = DeviceArgs::now(self.device_id(d_id));
                self.notify_event(DEVICE_ADDED_EVENT.new_update(args), observer);
            }
            Event::DeviceRemoved(d_id) => {
                let args = DeviceArgs::now(self.device_id(d_id));
                self.notify_event(DEVICE_REMOVED_EVENT.new_update(args), observer);
            }
            Event::DeviceMouseMotion { device: d_id, delta } => {
                let args = MouseMotionArgs::now(self.device_id(d_id), delta);
                self.notify_event(MOUSE_MOTION_EVENT.new_update(args), observer);
            }
            Event::DeviceMouseWheel { device: d_id, delta } => {
                let args = MouseWheelArgs::now(self.device_id(d_id), delta);
                self.notify_event(MOUSE_WHEEL_EVENT.new_update(args), observer);
            }
            Event::DeviceMotion { device: d_id, axis, value } => {
                let args = MotionArgs::now(self.device_id(d_id), axis, value);
                self.notify_event(MOTION_EVENT.new_update(args), observer);
            }
            Event::DeviceButton {
                device: d_id,
                button,
                state,
            } => {
                let args = ButtonArgs::now(self.device_id(d_id), button, state);
                self.notify_event(BUTTON_EVENT.new_update(args), observer);
            }
            Event::DeviceKey {
                device: d_id,
                key_code,
                state,
            } => {
                let args = KeyArgs::now(self.device_id(d_id), key_code, state);
                self.notify_event(KEY_EVENT.new_update(args), observer);
            }

            Event::LowMemory => {}

            Event::RecoveredFromComponentPanic { component, recover, panic } => {
                tracing::error!("view-process recovered from internal component panic\n  component: {component}\n  recover: {recover}\n```panic\n{panic}\n```");
            }

            // Others
            Event::Inited(zero_ui_view_api::Inited { .. }) | Event::Disconnected(_) | Event::FrameRendered(_) => unreachable!(), // handled before coalesce.
        }
    }

    /// Process a [`Event::FrameRendered`] event.
    fn on_view_rendered_event<O: AppEventObserver>(&mut self, ev: zero_ui_view_api::window::EventFrameRendered, observer: &mut O) {
        debug_assert!(ev.window != zero_ui_view_api::window::WindowId::INVALID);
        let window_id = WindowId::from_raw(ev.window.get());
        // view.on_frame_rendered(window_id); // already called in push_coalesce
        let image = ev.frame_image.map(|img| VIEW_PROCESS.on_frame_image(img));
        let args = crate::view_process::raw_events::RawFrameRenderedArgs::now(window_id, ev.frame, image);
        self.notify_event(crate::view_process::raw_events::RAW_FRAME_RENDERED_EVENT.new_update(args), observer);
    }

    pub(crate) fn run_headed(mut self) {
        #[allow(clippy::let_unit_value)]
        let mut observer = ();
        #[cfg(dyn_app_extension)]
        let mut observer = observer.as_dyn();

        self.apply_updates(&mut observer);
        self.apply_update_events(&mut observer);
        let mut wait = false;
        loop {
            wait = match self.poll_impl(wait, &mut observer) {
                AppControlFlow::Poll => false,
                AppControlFlow::Wait => true,
                AppControlFlow::Exit => break,
            };
        }
    }

    fn push_coalesce<O: AppEventObserver>(&mut self, ev: AppEvent, observer: &mut O) {
        match ev {
            AppEvent::ViewEvent(ev) => match ev {
                zero_ui_view_api::Event::FrameRendered(ev) => {
                    if ev.window == zero_ui_view_api::window::WindowId::INVALID {
                        tracing::error!("ignored rendered event for invalid window id, {ev:?}");
                        return;
                    }

                    let window = WindowId::from_raw(ev.window.get());

                    // update ViewProcess immediately.
                    {
                        if VIEW_PROCESS.is_available() {
                            VIEW_PROCESS.on_frame_rendered(window);
                        }
                    }

                    #[cfg(debug_assertions)]
                    if self.pending_view_frame_events.iter().any(|e| e.window == ev.window) {
                        tracing::warn!("window `{window:?}` probably sent a frame request without awaiting renderer idle");
                    }

                    self.pending_view_frame_events.push(ev);
                }
                zero_ui_view_api::Event::Inited(zero_ui_view_api::Inited {
                    generation,
                    is_respawn,
                    available_monitors,
                    multi_click_config,
                    key_repeat_config,
                    touch_config,
                    font_aa,
                    animations_config,
                    locale_config,
                    color_scheme,
                    extensions,
                }) => {
                    // notify immediately.
                    if is_respawn {
                        VIEW_PROCESS.on_respawed(generation);
                    }

                    VIEW_PROCESS.handle_inited(generation, extensions.clone());

                    let monitors: Vec<_> = available_monitors
                        .into_iter()
                        .map(|(id, info)| (VIEW_PROCESS.monitor_id(id), info))
                        .collect();

                    VARS.set_animations_enabled(animations_config.enabled);

                    let args = crate::view_process::ViewProcessInitedArgs::now(
                        generation,
                        is_respawn,
                        monitors,
                        multi_click_config,
                        key_repeat_config,
                        touch_config,
                        font_aa,
                        animations_config,
                        locale_config,
                        color_scheme,
                        extensions,
                    );
                    self.notify_event(VIEW_PROCESS_INITED_EVENT.new_update(args), observer);
                }
                zero_ui_view_api::Event::Disconnected(gen) => {
                    // update ViewProcess immediately.
                    VIEW_PROCESS.handle_disconnect(gen);
                }
                ev => {
                    if let Some(last) = self.pending_view_events.last_mut() {
                        match last.coalesce(ev) {
                            Ok(()) => {}
                            Err(ev) => self.pending_view_events.push(ev),
                        }
                    } else {
                        self.pending_view_events.push(ev);
                    }
                }
            },
            AppEvent::Event(ev) => EVENTS.notify(ev.get()),
            AppEvent::Update(op, target) => {
                UPDATES.update_op(op, target);
            }
            AppEvent::CheckUpdate => {}
            AppEvent::ResumeUnwind(p) => std::panic::resume_unwind(p),
        }
    }

    fn has_pending_updates(&mut self) -> bool {
        !self.pending_view_events.is_empty() || self.pending.has_updates() || UPDATES.has_pending_updates() || !self.receiver.is_empty()
    }

    pub(crate) fn poll<O: AppEventObserver>(&mut self, wait_app_event: bool, observer: &mut O) -> AppControlFlow {
        #[cfg(dyn_app_extension)]
        let mut observer = observer.as_dyn();
        #[cfg(dyn_app_extension)]
        let observer = &mut observer;
        self.poll_impl(wait_app_event, observer)
    }
    fn poll_impl<O: AppEventObserver>(&mut self, wait_app_event: bool, observer: &mut O) -> AppControlFlow {
        let mut disconnected = false;

        if self.exited {
            return AppControlFlow::Exit;
        }

        if wait_app_event {
            let idle = tracing::debug_span!("<idle>", ended_by = tracing::field::Empty).entered();

            let timer = if self.view_is_busy() { None } else { self.loop_timer.poll() };
            if let Some(time) = timer {
                match self.receiver.recv_deadline_sp(time) {
                    Ok(ev) => {
                        idle.record("ended_by", "event");
                        drop(idle);
                        self.push_coalesce(ev, observer)
                    }
                    Err(e) => match e {
                        flume::RecvTimeoutError::Timeout => {
                            idle.record("ended_by", "timeout");
                        }
                        flume::RecvTimeoutError::Disconnected => {
                            idle.record("ended_by", "disconnected");
                            disconnected = true
                        }
                    },
                }
            } else {
                match self.receiver.recv() {
                    Ok(ev) => {
                        idle.record("ended_by", "event");
                        drop(idle);
                        self.push_coalesce(ev, observer)
                    }
                    Err(e) => match e {
                        flume::RecvError::Disconnected => {
                            idle.record("ended_by", "disconnected");
                            disconnected = true
                        }
                    },
                }
            }
        }
        loop {
            match self.receiver.try_recv() {
                Ok(ev) => self.push_coalesce(ev, observer),
                Err(e) => match e {
                    flume::TryRecvError::Empty => break,
                    flume::TryRecvError::Disconnected => {
                        disconnected = true;
                        break;
                    }
                },
            }
        }
        if disconnected {
            panic!("app events channel disconnected");
        }

        if self.view_is_busy() {
            return AppControlFlow::Wait;
        }

        UPDATES.on_app_awake();

        // clear timers.
        let updated_timers = self.loop_timer.awake();
        if updated_timers {
            // tick timers and collect not elapsed timers.
            UPDATES.update_timers(&mut self.loop_timer);
            self.apply_updates(observer);
        }

        let mut events = mem::take(&mut self.pending_view_events);
        for ev in events.drain(..) {
            self.on_view_event(ev, observer);
            self.apply_updates(observer);
        }
        debug_assert!(self.pending_view_events.is_empty());
        self.pending_view_events = events; // reuse capacity

        let mut events = mem::take(&mut self.pending_view_frame_events);
        for ev in events.drain(..) {
            self.on_view_rendered_event(ev, observer);
        }
        self.pending_view_frame_events = events;

        if self.has_pending_updates() {
            self.apply_updates(observer);
            self.apply_update_events(observer);
        }

        if self.view_is_busy() {
            return AppControlFlow::Wait;
        }

        self.finish_frame(observer);

        UPDATES.next_deadline(&mut self.loop_timer);

        if self.extensions.0.exit() {
            UPDATES.on_app_sleep();
            self.exited = true;
            AppControlFlow::Exit
        } else if self.has_pending_updates() || UPDATES.has_pending_layout_or_render() {
            AppControlFlow::Poll
        } else {
            UPDATES.on_app_sleep();
            AppControlFlow::Wait
        }
    }

    /// Does updates, collects pending update generated events and layout + render.
    fn apply_updates<O: AppEventObserver>(&mut self, observer: &mut O) {
        let _s = tracing::debug_span!("apply_updates").entered();

        let mut run = true;
        while run {
            run = self.loop_monitor.update(|| {
                let mut any = false;

                self.pending |= UPDATES.apply_info();
                if mem::take(&mut self.pending.info) {
                    any = true;
                    let _s = tracing::debug_span!("info").entered();

                    let mut info_widgets = mem::take(&mut self.pending.info_widgets);

                    let _t = INSTANT_APP.pause_for_update();

                    {
                        let _s = tracing::debug_span!("ext.info").entered();
                        self.extensions.info(&mut info_widgets);
                    }
                    {
                        let _s = tracing::debug_span!("obs.info").entered();
                        observer.info(&mut info_widgets);
                    }
                }

                self.pending |= UPDATES.apply_updates();
                TimersService::notify();
                if mem::take(&mut self.pending.update) {
                    any = true;
                    let _s = tracing::debug_span!("update").entered();

                    let mut update_widgets = mem::take(&mut self.pending.update_widgets);

                    let _t = INSTANT_APP.pause_for_update();

                    {
                        let _s = tracing::debug_span!("ext.update_preview").entered();
                        self.extensions.update_preview();
                    }
                    {
                        let _s = tracing::debug_span!("obs.update_preview").entered();
                        observer.update_preview();
                    }
                    UPDATES.on_pre_updates();

                    {
                        let _s = tracing::debug_span!("ext.update_ui").entered();
                        self.extensions.update_ui(&mut update_widgets);
                    }
                    {
                        let _s = tracing::debug_span!("obs.update_ui").entered();
                        observer.update_ui(&mut update_widgets);
                    }

                    {
                        let _s = tracing::debug_span!("ext.update").entered();
                        self.extensions.update();
                    }
                    {
                        let _s = tracing::debug_span!("obs.update").entered();
                        observer.update();
                    }
                    UPDATES.on_updates();
                }

                any
            });
        }
    }

    // apply the current pending update generated events.
    fn apply_update_events<O: AppEventObserver>(&mut self, observer: &mut O) {
        let _s = tracing::debug_span!("apply_update_events").entered();

        loop {
            let events: Vec<_> = self.pending.events.drain(..).collect();
            if events.is_empty() {
                break;
            }
            for mut update in events {
                let _s = tracing::debug_span!("update_event", ?update).entered();

                self.loop_monitor.maybe_trace(|| {
                    let _t = INSTANT_APP.pause_for_update();

                    {
                        let _s = tracing::debug_span!("ext.event_preview").entered();
                        self.extensions.event_preview(&mut update);
                    }
                    {
                        let _s = tracing::debug_span!("obs.event_preview").entered();
                        observer.event_preview(&mut update);
                    }
                    update.call_pre_actions();

                    {
                        let _s = tracing::debug_span!("ext.event_ui").entered();
                        self.extensions.event_ui(&mut update);
                    }
                    {
                        let _s = tracing::debug_span!("obs.event_ui").entered();
                        observer.event_ui(&mut update);
                    }
                    {
                        let _s = tracing::debug_span!("ext.event").entered();
                        self.extensions.event(&mut update);
                    }
                    {
                        let _s = tracing::debug_span!("obs.event").entered();
                        observer.event(&mut update);
                    }
                    update.call_pos_actions();
                });

                self.apply_updates(observer);
            }
        }
    }

    fn view_is_busy(&mut self) -> bool {
        VIEW_PROCESS.is_available() && VIEW_PROCESS.pending_frames() > 0
    }

    // apply pending layout & render if the view-process is not already rendering.
    fn finish_frame<O: AppEventObserver>(&mut self, observer: &mut O) {
        debug_assert!(!self.view_is_busy());

        self.pending |= UPDATES.apply_layout_render();

        while mem::take(&mut self.pending.layout) {
            let _s = tracing::debug_span!("apply_layout").entered();

            let mut layout_widgets = mem::take(&mut self.pending.layout_widgets);

            self.loop_monitor.maybe_trace(|| {
                let _t = INSTANT_APP.pause_for_update();

                {
                    let _s = tracing::debug_span!("ext.layout").entered();
                    self.extensions.layout(&mut layout_widgets);
                }
                {
                    let _s = tracing::debug_span!("obs.layout").entered();
                    observer.layout(&mut layout_widgets);
                }
            });

            self.apply_updates(observer);
            self.pending |= UPDATES.apply_layout_render();
        }

        if mem::take(&mut self.pending.render) {
            let _s = tracing::debug_span!("apply_render").entered();

            let mut render_widgets = mem::take(&mut self.pending.render_widgets);
            let mut render_update_widgets = mem::take(&mut self.pending.render_update_widgets);

            let _t = INSTANT_APP.pause_for_update();

            {
                let _s = tracing::debug_span!("ext.render").entered();
                self.extensions.render(&mut render_widgets, &mut render_update_widgets);
            }
            {
                let _s = tracing::debug_span!("obs.render").entered();
                observer.render(&mut render_widgets, &mut render_update_widgets);
            }
        }

        self.loop_monitor.finish_frame();
    }
}
impl<E: AppExtension> Drop for RunningApp<E> {
    fn drop(&mut self) {
        let _s = tracing::debug_span!("ext.deinit").entered();
        self.extensions.deinit();
        VIEW_PROCESS.exit();
    }
}

/// App main loop timer.
#[derive(Debug)]
pub(crate) struct LoopTimer {
    now: DInstant,
    deadline: Option<Deadline>,
}
impl Default for LoopTimer {
    fn default() -> Self {
        Self {
            now: INSTANT.now(),
            deadline: None,
        }
    }
}
impl LoopTimer {
    /// Returns `true` if the `deadline` has elapsed, `false` if the `deadline` was
    /// registered for future waking.
    pub fn elapsed(&mut self, deadline: Deadline) -> bool {
        if deadline.0 <= self.now {
            true
        } else {
            self.register(deadline);
            false
        }
    }

    /// Register the future `deadline`.
    pub fn register(&mut self, deadline: Deadline) {
        if let Some(d) = &mut self.deadline {
            if deadline < *d {
                *d = deadline;
            }
        } else {
            self.deadline = Some(deadline)
        }
    }

    /// Get next recv deadline.
    pub(crate) fn poll(&mut self) -> Option<Deadline> {
        self.deadline
    }

    /// Maybe awake timer.
    pub(crate) fn awake(&mut self) -> bool {
        self.now = INSTANT.now();
        if let Some(d) = self.deadline {
            if d.0 <= self.now {
                self.deadline = None;
                return true;
            }
        }
        false
    }

    /// Awake timestamp.
    pub fn now(&self) -> DInstant {
        self.now
    }
}
impl zero_ui_var::animation::AnimationTimer for LoopTimer {
    fn elapsed(&mut self, deadline: Deadline) -> bool {
        self.elapsed(deadline)
    }

    fn register(&mut self, deadline: Deadline) {
        self.register(deadline)
    }

    fn now(&self) -> DInstant {
        self.now()
    }
}

#[derive(Default)]
struct LoopMonitor {
    update_count: u16,
    skipped: bool,
    trace: Vec<UpdateTrace>,
}
impl LoopMonitor {
    /// Returns `false` if the loop should break.
    pub fn update(&mut self, update_once: impl FnOnce() -> bool) -> bool {
        self.update_count += 1;

        if self.update_count < 500 {
            update_once()
        } else if self.update_count < 1000 {
            UpdatesTrace::collect_trace(&mut self.trace, update_once)
        } else if self.update_count == 1000 {
            self.skipped = true;
            let trace = UpdatesTrace::format_trace(mem::take(&mut self.trace));
            tracing::error!(
                "updated 1000 times without rendering, probably stuck in an infinite loop\n\
                 will start skipping updates to render and poll system events\n\
                 top 20 most frequent update requests (in 500 cycles):\n\
                 {trace}\n\
                    you can use `UpdatesTraceUiNodeExt` and `updates_trace_event` to refine the trace"
            );
            false
        } else if self.update_count == 1500 {
            self.update_count = 1001;
            false
        } else {
            update_once()
        }
    }

    pub fn maybe_trace(&mut self, notify_once: impl FnOnce()) {
        if (500..1000).contains(&self.update_count) {
            UpdatesTrace::collect_trace(&mut self.trace, notify_once);
        } else {
            notify_once();
        }
    }

    pub fn finish_frame(&mut self) {
        if !self.skipped {
            self.skipped = false;
            self.update_count = 0;
            self.trace = vec![];
        }
    }
}

impl APP {
    /// Register a request for process exit with code `0` in the next update.
    ///
    /// The [`EXIT_REQUESTED_EVENT`] will notify, and if propagation is not cancelled the app process will exit.
    ///
    /// Returns a response variable that is updated once with the unit value [`ExitCancelled`]
    /// if the exit operation is cancelled.
    ///
    /// See also the [`EXIT_CMD`].
    pub fn exit(&self) -> ResponseVar<ExitCancelled> {
        APP_PROCESS_SV.write().exit()
    }
}

/// App time control.
///
/// The manual time methods are only recommended for headless apps.
impl APP {
    /// Gets a variable that configures if [`INSTANT.now`] is the same exact value during each update, info, layout or render pass.
    ///
    /// Time is paused for each single pass by default, setting this to `false` will cause [`INSTANT.now`] to read
    /// the system time for every call.
    ///
    /// [`INSTANT.now`]: crate::INSTANT::now
    pub fn pause_time_for_update(&self) -> ArcVar<bool> {
        APP_PROCESS_SV.read().pause_time_for_updates.clone()
    }

    /// Pause the [`INSTANT.now`] value, after this call it must be updated manually using
    /// [`advance_manual_time`] or [`set_manual_time`]. To resume normal time use [`end_manual_time`].
    ///
    /// [`INSTANT.now`]: crate::INSTANT::now
    /// [`advance_manual_time`]: Self::advance_manual_time
    /// [`set_manual_time`]: Self::set_manual_time
    /// [`end_manual_time`]: Self::end_manual_time
    pub fn start_manual_time(&self) {
        INSTANT_APP.set_mode(InstantMode::Manual);
        INSTANT_APP.set_now(INSTANT.now());
        UPDATES.update(None);
    }

    /// Add the `advance` to the current manual time.
    ///
    /// Note that you must ensure an update reaches the code that controls manual time, otherwise
    /// the app loop may end-up stuck on idle or awaiting a timer that never elapses.
    ///
    /// # Panics
    ///
    /// Panics if called before [`start_manual_time`].
    ///
    /// [`start_manual_time`]: Self::start_manual_time
    pub fn advance_manual_time(&self, advance: Duration) {
        INSTANT_APP.advance_now(advance);
        UPDATES.update(None);
    }

    /// Set the current [`INSTANT.now`].
    ///
    /// # Panics
    ///
    /// Panics if called before [`start_manual_time`].
    ///
    /// [`INSTANT.now`]: crate::INSTANT::now
    /// [`start_manual_time`]: Self::start_manual_time
    pub fn set_manual_time(&self, now: DInstant) {
        INSTANT_APP.set_now(now);
        UPDATES.update(None);
    }

    /// Resume normal time.
    pub fn end_manual_time(&self) {
        INSTANT_APP.set_mode(match APP.pause_time_for_update().get() {
            true => InstantMode::UpdatePaused,
            false => InstantMode::Now,
        });
        UPDATES.update(None);
    }
}

command! {
    /// Represents the app process [`exit`] request.
    ///
    /// [`exit`]: APP::exit
    pub static EXIT_CMD = {
        name: "Exit",
        info: "Close all windows and exit.",
        shortcut: shortcut!(Exit),
    };
}

/// Cancellation message of an [exit request].
///
/// [exit request]: APP::exit
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitCancelled;
impl fmt::Display for ExitCancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "exit request cancelled")
    }
}

struct AppIntrinsic {
    #[allow(dead_code)]
    exit_handle: CommandHandle,
    pending_exit: Option<PendingExit>,
}
struct PendingExit {
    handle: EventPropagationHandle,
    response: ResponderVar<ExitCancelled>,
}
impl AppIntrinsic {
    /// Pre-init intrinsic services and commands, must be called before extensions init.
    pub(super) fn pre_init(is_headed: bool, with_renderer: bool, view_process_exe: Option<PathBuf>, device_events: bool) -> Self {
        APP_PROCESS_SV
            .read()
            .pause_time_for_updates
            .hook(|a| {
                if !matches!(INSTANT.mode(), zero_ui_time::InstantMode::Manual) {
                    if *a.value() {
                        INSTANT_APP.set_mode(InstantMode::UpdatePaused);
                    } else {
                        INSTANT_APP.set_mode(InstantMode::Now);
                    }
                }
                true
            })
            .perm();

        if is_headed {
            debug_assert!(with_renderer);

            let view_evs_sender = UPDATES.sender();
            VIEW_PROCESS.start(view_process_exe, device_events, false, move |ev| {
                let _ = view_evs_sender.send_view_event(ev);
            });
        } else if with_renderer {
            let view_evs_sender = UPDATES.sender();
            VIEW_PROCESS.start(view_process_exe, false, true, move |ev| {
                let _ = view_evs_sender.send_view_event(ev);
            });
        }

        AppIntrinsic {
            exit_handle: EXIT_CMD.subscribe(true),
            pending_exit: None,
        }
    }

    /// Returns if exit was requested and not cancelled.
    pub(super) fn exit(&mut self) -> bool {
        if let Some(pending) = self.pending_exit.take() {
            if pending.handle.is_stopped() {
                pending.response.respond(ExitCancelled);
                false
            } else {
                true
            }
        } else {
            false
        }
    }
}
impl AppExtension for AppIntrinsic {
    fn event_preview(&mut self, update: &mut EventUpdate) {
        if let Some(args) = EXIT_CMD.on(update) {
            args.handle_enabled(&self.exit_handle, |_| {
                APP.exit();
            });
        }
    }

    fn update(&mut self) {
        if let Some(response) = APP_PROCESS_SV.write().take_requests() {
            let args = ExitRequestedArgs::now();
            self.pending_exit = Some(PendingExit {
                handle: args.propagation().clone(),
                response,
            });
            EXIT_REQUESTED_EVENT.notify(args);
        }
    }
}

pub(crate) fn assert_not_view_process() {
    if zero_ui_view_api::ViewConfig::from_env().is_some() {
        panic!("cannot start App in view-process");
    }
}

#[cfg(feature = "deadlock_detection")]
pub(crate) fn check_deadlock() {
    use parking_lot::deadlock;
    use std::{
        sync::atomic::{self, AtomicBool},
        thread,
        time::*,
    };

    static CHECK_RUNNING: AtomicBool = AtomicBool::new(false);

    if CHECK_RUNNING.swap(true, atomic::Ordering::SeqCst) {
        return;
    }

    thread::spawn(|| loop {
        thread::sleep(Duration::from_secs(10));

        let deadlocks = deadlock::check_deadlock();
        if deadlocks.is_empty() {
            continue;
        }

        use std::fmt::Write;
        let mut msg = String::new();

        let _ = writeln!(&mut msg, "{} deadlocks detected", deadlocks.len());
        for (i, threads) in deadlocks.iter().enumerate() {
            let _ = writeln!(&mut msg, "Deadlock #{}, {} threads", i, threads.len());
            for t in threads {
                let _ = writeln!(&mut msg, "Thread Id {:#?}", t.thread_id());
                let _ = writeln!(&mut msg, "{:#?}", t.backtrace());
            }
        }

        #[cfg(not(feature = "test_util"))]
        eprint!("{msg}");

        #[cfg(feature = "test_util")]
        {
            // test runner captures output and ignores panics in background threads, so
            // we write directly to stderr and exit the process.
            use std::io::Write;
            let _ = write!(&mut std::io::stderr(), "{msg}");
            std::process::exit(-1);
        }
    });
}
#[cfg(not(feature = "deadlock_detection"))]
pub(crate) fn check_deadlock() {}

app_local! {
    pub(super) static APP_PROCESS_SV: AppProcessService =AppProcessService {
        exit_requests: None,
        extensions: None,
        device_events: false,
        pause_time_for_updates: zero_ui_var::var(true),
    };
}

pub(super) struct AppProcessService {
    exit_requests: Option<ResponderVar<ExitCancelled>>,
    extensions: Option<Arc<AppExtensionsInfo>>,
    pub(super) device_events: bool,
    pause_time_for_updates: ArcVar<bool>,
}
impl AppProcessService {
    pub(super) fn take_requests(&mut self) -> Option<ResponderVar<ExitCancelled>> {
        self.exit_requests.take()
    }

    fn exit(&mut self) -> ResponseVar<ExitCancelled> {
        if let Some(r) = &self.exit_requests {
            r.response_var()
        } else {
            let (responder, response) = response_var();
            self.exit_requests = Some(responder);
            UPDATES.update(None);
            response
        }
    }

    pub(super) fn extensions(&self) -> Arc<AppExtensionsInfo> {
        self.extensions
            .clone()
            .unwrap_or_else(|| Arc::new(AppExtensionsInfo { infos: vec![] }))
    }

    pub(super) fn set_extensions(&mut self, info: AppExtensionsInfo, device_events: bool) {
        self.extensions = Some(Arc::new(info));
        self.device_events = device_events;
    }
}

/// App events.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum AppEvent {
    /// Event from the View Process.
    ViewEvent(zero_ui_view_api::Event),
    /// Notify [`Events`](crate::var::Events).
    Event(crate::event::EventUpdateMsg),
    /// Do an update cycle.
    Update(UpdateOp, Option<WidgetId>),
    /// Resume a panic in the app main thread.
    ResumeUnwind(PanicPayload),
    /// Check for pending updates.
    CheckUpdate,
}

/// A sender that can awake apps and insert events into the main loop.
///
/// A Clone of the sender is available in [`UPDATES.sender`].
///
/// [`Updates.sender`]: crate::update::UPDATES.sender
#[derive(Clone)]
pub struct AppEventSender(flume::Sender<AppEvent>);
impl AppEventSender {
    pub(crate) fn new() -> (Self, flume::Receiver<AppEvent>) {
        let (sender, receiver) = flume::unbounded();
        (Self(sender), receiver)
    }

    fn send_app_event(&self, event: AppEvent) -> Result<(), AppDisconnected<AppEvent>> {
        self.0.send(event)?;
        Ok(())
    }

    fn send_view_event(&self, event: zero_ui_view_api::Event) -> Result<(), AppDisconnected<AppEvent>> {
        self.0.send(AppEvent::ViewEvent(event))?;
        Ok(())
    }

    /// Causes an update cycle to happen in the app.
    pub fn send_update(&self, op: UpdateOp, target: impl Into<Option<WidgetId>>) -> Result<(), AppDisconnected<()>> {
        UpdatesTrace::log_update();
        self.send_app_event(AppEvent::Update(op, target.into()))
            .map_err(|_| AppDisconnected(()))
    }

    /// [`EventSender`](crate::event::EventSender) util.
    pub(crate) fn send_event(&self, event: crate::event::EventUpdateMsg) -> Result<(), AppDisconnected<crate::event::EventUpdateMsg>> {
        self.send_app_event(AppEvent::Event(event)).map_err(|e| match e.0 {
            AppEvent::Event(ev) => AppDisconnected(ev),
            _ => unreachable!(),
        })
    }

    /// Resume a panic in the app main loop thread.
    pub fn send_resume_unwind(&self, payload: PanicPayload) -> Result<(), AppDisconnected<PanicPayload>> {
        self.send_app_event(AppEvent::ResumeUnwind(payload)).map_err(|e| match e.0 {
            AppEvent::ResumeUnwind(p) => AppDisconnected(p),
            _ => unreachable!(),
        })
    }

    /// [`UPDATES`] util.
    pub(crate) fn send_check_update(&self) -> Result<(), AppDisconnected<()>> {
        self.send_app_event(AppEvent::CheckUpdate).map_err(|_| AppDisconnected(()))
    }

    /// Create an [`Waker`] that causes a [`send_update`](Self::send_update).
    pub fn waker(&self, target: impl Into<Option<WidgetId>>) -> Waker {
        Arc::new(AppWaker(self.0.clone(), target.into())).into()
    }

    /// Create an unbound channel that causes an extension update for each message received.
    pub fn ext_channel<T>(&self) -> (AppExtSender<T>, AppExtReceiver<T>) {
        let (sender, receiver) = flume::unbounded();

        (
            AppExtSender {
                update: self.clone(),
                sender,
            },
            AppExtReceiver { receiver },
        )
    }

    /// Create a bounded channel that causes an extension update for each message received.
    pub fn ext_channel_bounded<T>(&self, cap: usize) -> (AppExtSender<T>, AppExtReceiver<T>) {
        let (sender, receiver) = flume::bounded(cap);

        (
            AppExtSender {
                update: self.clone(),
                sender,
            },
            AppExtReceiver { receiver },
        )
    }
}

struct AppWaker(flume::Sender<AppEvent>, Option<WidgetId>);
impl std::task::Wake for AppWaker {
    fn wake(self: std::sync::Arc<Self>) {
        self.wake_by_ref()
    }
    fn wake_by_ref(self: &Arc<Self>) {
        let _ = self.0.send(AppEvent::Update(UpdateOp::Update, self.1));
    }
}

type PanicPayload = Box<dyn std::any::Any + Send + 'static>;

/// Represents a channel sender that causes an extensions update for each value transferred.
///
/// A channel can be created using the [`AppEventSender::ext_channel`] method.
pub struct AppExtSender<T> {
    update: AppEventSender,
    sender: flume::Sender<T>,
}
impl<T> Clone for AppExtSender<T> {
    fn clone(&self) -> Self {
        Self {
            update: self.update.clone(),
            sender: self.sender.clone(),
        }
    }
}
impl<T: Send> AppExtSender<T> {
    /// Send an extension update and `msg`, blocks until the app receives the message.
    pub fn send(&self, msg: T) -> Result<(), AppDisconnected<T>> {
        match self.update.send_update(UpdateOp::Update, None) {
            Ok(()) => self.sender.send(msg).map_err(|e| AppDisconnected(e.0)),
            Err(_) => Err(AppDisconnected(msg)),
        }
    }

    /// Send an extension update and `msg`, blocks until the app receives the message or `dur` elapses.
    pub fn send_timeout(&self, msg: T, dur: Duration) -> Result<(), TimeoutOrAppDisconnected> {
        match self.update.send_update(UpdateOp::Update, None) {
            Ok(()) => self.sender.send_timeout(msg, dur).map_err(|e| match e {
                flume::SendTimeoutError::Timeout(_) => TimeoutOrAppDisconnected::Timeout,
                flume::SendTimeoutError::Disconnected(_) => TimeoutOrAppDisconnected::AppDisconnected,
            }),
            Err(_) => Err(TimeoutOrAppDisconnected::AppDisconnected),
        }
    }

    /// Send an extension update and `msg`, blocks until the app receives the message or `deadline` is reached.
    pub fn send_deadline(&self, msg: T, deadline: Instant) -> Result<(), TimeoutOrAppDisconnected> {
        match self.update.send_update(UpdateOp::Update, None) {
            Ok(()) => self.sender.send_deadline(msg, deadline).map_err(|e| match e {
                flume::SendTimeoutError::Timeout(_) => TimeoutOrAppDisconnected::Timeout,
                flume::SendTimeoutError::Disconnected(_) => TimeoutOrAppDisconnected::AppDisconnected,
            }),
            Err(_) => Err(TimeoutOrAppDisconnected::AppDisconnected),
        }
    }
}

/// Represents a channel receiver in an app extension.
///
/// See [`AppExtSender`] for details.
pub struct AppExtReceiver<T> {
    receiver: flume::Receiver<T>,
}
impl<T> Clone for AppExtReceiver<T> {
    fn clone(&self) -> Self {
        Self {
            receiver: self.receiver.clone(),
        }
    }
}
impl<T> AppExtReceiver<T> {
    /// Receive an update if any was send.
    ///
    /// Returns `Ok(msg)` if there was at least one message, or returns `Err(None)` if there was no update or
    /// returns `Err(AppExtSenderDisconnected)` if the connected sender was dropped.
    pub fn try_recv(&self) -> Result<T, Option<AppExtSenderDisconnected>> {
        self.receiver.try_recv().map_err(|e| match e {
            flume::TryRecvError::Empty => None,
            flume::TryRecvError::Disconnected => Some(AppExtSenderDisconnected),
        })
    }
}

/// Error when the app connected to a sender/receiver channel has disconnected.
///
/// Contains the value that could not be send or `()` for receiver errors.
#[derive(Debug)]
pub struct AppExtSenderDisconnected;
impl fmt::Display for AppExtSenderDisconnected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot receive because the sender disconnected")
    }
}
impl std::error::Error for AppExtSenderDisconnected {}

event_args! {
    /// Arguments for [`EXIT_REQUESTED_EVENT`].
    ///
    /// Requesting [`propagation().stop()`] on this event cancels the exit.
    ///
    /// [`propagation().stop()`]: crate::event::EventPropagationHandle::stop
    pub struct ExitRequestedArgs {
        ..
        /// Broadcast to all.
        fn delivery_list(&self, list: &mut UpdateDeliveryList) {
            list.search_all()
        }
    }
}

event! {
    /// Cancellable event raised when app process exit is requested.
    ///
    /// App exit can be requested using the [`APP`] service or the [`EXIT_CMD`], some extensions
    /// also request exit if some conditions are met, `WindowManager` requests it after the last window
    /// is closed for example.
    ///
    /// Requesting [`propagation().stop()`] on this event cancels the exit.
    ///
    /// [`propagation().stop()`]: crate::event::EventPropagationHandle::stop
    pub static EXIT_REQUESTED_EVENT: ExitRequestedArgs;
}

/// Extension methods for [`flume::Receiver<T>`].
trait ReceiverExt<T> {
    /// Receive or precise timeout.
    fn recv_deadline_sp(&self, deadline: Deadline) -> Result<T, flume::RecvTimeoutError>;
}

const WORST_SLEEP_ERR: Duration = Duration::from_millis(if cfg!(windows) { 20 } else { 10 });
const WORST_SPIN_ERR: Duration = Duration::from_millis(if cfg!(windows) { 2 } else { 1 });

impl<T> ReceiverExt<T> for flume::Receiver<T> {
    fn recv_deadline_sp(&self, deadline: Deadline) -> Result<T, flume::RecvTimeoutError> {
        loop {
            if let Some(d) = deadline.0.checked_duration_since(INSTANT.now()) {
                if matches!(INSTANT.mode(), zero_ui_time::InstantMode::Manual) {
                    // manual time is probably desynched from `Instant`, so we use `recv_timeout` that
                    // is slightly less precise, but an app in manual mode probably does not care.
                    match self.recv_timeout(d.checked_sub(WORST_SLEEP_ERR).unwrap_or_default()) {
                        Err(flume::RecvTimeoutError::Timeout) => continue, // continue to try_recv spin
                        interrupt => return interrupt,
                    }
                } else if d > WORST_SLEEP_ERR {
                    // probably sleeps here.
                    match self.recv_deadline(deadline.0.checked_sub(WORST_SLEEP_ERR).unwrap().into()) {
                        Err(flume::RecvTimeoutError::Timeout) => continue, // continue to try_recv spin
                        interrupt => return interrupt,
                    }
                } else if d > WORST_SPIN_ERR {
                    let spin_deadline = Deadline(deadline.0.checked_sub(WORST_SPIN_ERR).unwrap());

                    // try_recv spin
                    while !spin_deadline.has_elapsed() {
                        match self.try_recv() {
                            Err(flume::TryRecvError::Empty) => std::thread::yield_now(),
                            Err(flume::TryRecvError::Disconnected) => return Err(flume::RecvTimeoutError::Disconnected),
                            Ok(msg) => return Ok(msg),
                        }
                    }
                    continue; // continue to timeout spin
                } else {
                    // last millis spin for better timeout precision
                    while !deadline.has_elapsed() {
                        std::thread::yield_now();
                    }
                    return Err(flume::RecvTimeoutError::Timeout);
                }
            } else {
                return Err(flume::RecvTimeoutError::Timeout);
            }
        }
    }
}
