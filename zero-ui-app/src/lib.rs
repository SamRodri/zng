//! App process implementation.
//!
//! # Widget Instantiation
//!
//! See [`enable_widget_macros!`] if you want to instantiate widgets without depending on the `zero-ui` crate.

#![recursion_limit = "256"]
// suppress nag about very simple boxed closure signatures.
#![allow(clippy::type_complexity)]
#![warn(unused_extern_crates)]
#![warn(missing_docs)]

use std::{
    any::{type_name, TypeId},
    fmt,
    future::Future,
    ops,
    path::PathBuf,
    sync::Arc,
};

pub mod access;
pub mod event;
pub mod handler;
pub mod render;
pub mod shortcut;
pub mod timer;
pub mod update;
pub mod view_process;
pub mod widget;
pub mod window;

mod tests;

use view_process::VIEW_PROCESS;
use widget::UiTaskWidget;
#[doc(hidden)]
pub use zero_ui_layout as layout;
#[doc(hidden)]
pub use zero_ui_var as var;

mod running;
pub use running::*;

use update::{EventUpdate, InfoUpdates, LayoutUpdates, RenderUpdates, UpdatesTrace, WidgetUpdates, UPDATES};
use window::WindowMode;
use zero_ui_app_context::{AppId, AppScope, LocalContext};
use zero_ui_task::ui::UiTask;

/// Enable widget instantiation in crates that can't depend on the `zero-ui` crate.
///
/// This must be called at the top of the crate:
///
/// ```
/// // in lib.rs or main.rs
/// # use zero_ui_app::*;
/// enable_widget_macros!();
/// ```
#[macro_export]
macro_rules! enable_widget_macros {
    () => {
        #[doc(hidden)]
        #[allow(unused_extern_crates)]
        extern crate self as zero_ui;

        #[doc(hidden)]
        pub use $crate::__proc_macro_util;
    };
}

#[doc(hidden)]
#[allow(unused_extern_crates)]
extern crate self as zero_ui;

#[doc(hidden)]
pub mod __proc_macro_util {
    // * don't add glob re-exports, the types leak in rust-analyzer even if all is doc(hidden).
    // * don't use macro_rules! macros that use $crate , they will fail with "unresolved import" when used from the re-exports.

    #[doc(hidden)]
    pub mod widget {
        #[doc(hidden)]
        pub mod builder {
            #[doc(hidden)]
            pub use crate::widget::builder::{
                getter_var, iter_input_build_actions, nest_group_items, new_dyn_other, new_dyn_ui_node, new_dyn_ui_node_list, new_dyn_var,
                new_dyn_widget_handler, panic_input, state_var, ui_node_list_to_args, ui_node_to_args, value_to_args, var_to_args,
                when_condition_expr_var, widget_handler_to_args, AnyArcWidgetHandler, ArcWidgetHandler, Importance, InputKind,
                PropertyArgs, PropertyId, PropertyInfo, PropertyInput, PropertyInputTypes, PropertyNewArgs, SourceLocation,
                StaticPropertyId, UiNodeInWhenExprError, UiNodeListInWhenExprError, WgtInfo, WhenInput, WhenInputMember, WhenInputVar,
                WidgetHandlerInWhenExprError, WidgetType,
            };
        }

        #[doc(hidden)]
        pub mod base {
            pub use crate::widget::base::{WidgetBase, WidgetExt, WidgetImpl};
        }

        #[doc(hidden)]
        pub mod instance {
            pub use crate::widget::instance::{
                ArcNode, ArcNodeList, BoxedUiNode, BoxedUiNodeList, NilUiNode, UiNode, UiNodeList, UiNodeVec,
            };
        }

        #[doc(hidden)]
        pub mod info {
            pub use crate::widget::info::{WidgetInfoBuilder, WidgetLayout, WidgetMeasure};
        }

        #[doc(hidden)]
        pub use crate::widget::{easing_property, widget_new};
    }

    #[doc(hidden)]
    pub mod update {
        pub use crate::update::{EventUpdate, WidgetUpdates};
    }

    #[doc(hidden)]
    pub mod layout {
        #[doc(hidden)]
        pub mod units {
            #[doc(hidden)]
            pub use crate::layout::units::{PxSize, TimeUnits};
        }

        #[doc(hidden)]
        pub mod context {
            #[doc(hidden)]
            pub use crate::layout::context::LAYOUT;
        }
    }

    #[doc(hidden)]
    pub mod render {
        pub use crate::render::{FrameBuilder, FrameUpdate};
    }

    #[doc(hidden)]
    pub mod handler {
        #[doc(hidden)]
        pub use crate::handler::hn;
    }

    #[doc(hidden)]
    pub mod var {
        #[doc(hidden)]
        pub use crate::var::{expr_var, AnyVar, AnyVarValue, BoxedVar, Var};

        #[doc(hidden)]
        pub mod animation {
            #[doc(hidden)]
            pub mod easing {
                #[doc(hidden)]
                pub use crate::var::animation::easing::{
                    back, bounce, circ, cubic, cubic_bezier, ease_in, ease_in_out, ease_out, ease_out_in, elastic, expo, linear, none,
                    quad, quart, quint, reverse, reverse_out, sine, step_ceil, step_floor,
                };
            }
        }
    }
}

/// An app extension.
///
/// # App Loop
///
/// Methods in app extension are called in this synchronous order:
///
/// ## 1 - Init
///
/// The [`init`] method is called once at the start of the app. Extensions are initialized in the order then where *inserted* in the app.
///
/// ## 2 - Events
///
/// The [`event_preview`], [`event_ui`] and [`event`] methods are called in this order for each event message received. Events
/// received from other threads are buffered until the app is free and then are processed using these methods.
///
/// ## 3 - Updates
///
/// The [`update_preview`], [`update_ui`] and [`update`] methods are called in this order every time an [update is requested],
/// a sequence of events have processed, variables where assigned or timers elapsed. The app loops between [events] and [updates] until
/// no more updates or events are pending, if [layout] or [render] are requested they are deferred until a event-update cycle is complete.
///
/// # 4 - Layout
///
/// The [`layout`] method is called if during [init], [events] or [updates] a layout was requested, extensions should also remember which
/// unit requested layout, to avoid unnecessary work, for example the `WindowManager` remembers witch window requested layout.
///
/// If the [`layout`] call requests updates the app goes back to [updates], requests for render are again deferred.
///
/// # 5 - Render
///
/// The [`render`] method is called if during [init], [events], [updates] or [layout] a render was requested and no other
/// event, update or layout is pending. Extensions should identify which unit is pending a render or render update and generate
/// and send a display list or frame update.
///
/// This method does not block until the frame pixels are rendered, it covers only the creation of a frame request sent to the view-process.
/// A [`RAW_FRAME_RENDERED_EVENT`] is send when a frame finished rendering in the view-process.
///
/// ## 6 - Deinit
///
/// The [`deinit`] method is called once after an exit was requested and not cancelled. Exit is
/// requested using the [`APP`] service, it causes an [`EXIT_REQUESTED_EVENT`] that can be cancelled, if it
/// is not cancelled the extensions are deinited and then dropped.
///
/// Deinit happens from the last inited extension first, so in reverse of init order, the [drop] happens in undefined order. Deinit is not called
/// if the app thread is unwinding from a panic, the extensions will just be dropped in this case.
///
/// # Resize Loop
///
/// The app enters a special loop when a window is resizing,
///
/// [`init`]: AppExtension::init
/// [`event_preview`]: AppExtension::event_preview
/// [`event_ui`]: AppExtension::event_ui
/// [`event`]: AppExtension::event
/// [`update_preview`]: AppExtension::update_preview
/// [`update_ui`]: AppExtension::update_ui
/// [`update`]: AppExtension::update
/// [`layout`]: AppExtension::layout
/// [`render`]: AppExtension::event
/// [`deinit`]: AppExtension::deinit
/// [drop]: Drop
/// [update is requested]: UPDATES::update
/// [init]: #1-init
/// [events]: #2-events
/// [updates]: #3-updates
/// [layout]: #3-layout
/// [render]: #5-render
/// [`RAW_FRAME_RENDERED_EVENT`]: crate::view_process::raw_events::RAW_FRAME_RENDERED_EVENT
pub trait AppExtension: 'static {
    /// Register info abound this extension on the info list.
    fn register(&self, info: &mut AppExtensionsInfo)
    where
        Self: Sized,
    {
        info.push::<Self>()
    }

    /// Initializes this extension.
    fn init(&mut self) {}

    /// If the application should notify raw device events.
    ///
    /// Device events are raw events not targeting any window, like a mouse move on any part of the screen.
    /// They tend to be high-volume events so there is a performance cost to activating this. Note that if
    /// this is `false` you still get the mouse move over windows of the app.
    ///
    /// This is called zero or one times before [`init`](Self::init).
    ///
    /// Returns `false` by default.
    fn enable_device_events(&self) -> bool {
        false
    }

    /// Called just before [`event_ui`](Self::event_ui).
    ///
    /// Extensions can handle this method to to intersect event updates before the UI.
    ///
    /// Note that this is not related to the `on_event_preview` properties, all UI events
    /// happen in `on_event_ui`.
    fn event_preview(&mut self, update: &mut EventUpdate) {
        let _ = update;
    }

    /// Called just before [`event`](Self::event).
    ///
    /// Only extensions that generate windows must handle this method. The [`UiNode::event`](crate::widget::instance::UiNode::event)
    /// method is called here.
    fn event_ui(&mut self, update: &mut EventUpdate) {
        let _ = update;
    }

    /// Called after every [`event_ui`](Self::event_ui).
    ///
    /// This is the general extensions event handler, it gives the chance for the UI to signal stop propagation.
    fn event(&mut self, update: &mut EventUpdate) {
        let _ = update;
    }

    /// Called before and after an update cycle. The [`UiNode::info`] method is called here.
    ///
    /// [`UiNode::info`]: crate::widget::instance::UiNode::info
    fn info(&mut self, info_widgets: &mut InfoUpdates) {
        let _ = info_widgets;
    }

    /// Called just before [`update_ui`](Self::update_ui).
    ///
    /// Extensions can handle this method to interact with updates before the UI.
    ///
    /// Note that this is not related to the `on_event_preview` properties, all UI events
    /// happen in `update_ui`.
    fn update_preview(&mut self) {}

    /// Called just before [`update`](Self::update).
    ///
    /// Only extensions that manage windows must handle this method. The [`UiNode::update`]
    /// method is called here.
    ///
    /// [`UiNode::update`]: crate::widget::instance::UiNode::update
    fn update_ui(&mut self, update_widgets: &mut WidgetUpdates) {
        let _ = update_widgets;
    }

    /// Called after every [`update_ui`](Self::update_ui) and [`info`](Self::info).
    ///
    /// This is the general extensions update, it gives the chance for
    /// the UI to signal stop propagation.
    fn update(&mut self) {}

    /// Called after every sequence of updates if layout was requested.
    ///
    /// The [`UiNode::layout`] method is called here by extensions that manage windows.
    ///
    /// [`UiNode::layout`]: crate::widget::instance::UiNode::layout
    fn layout(&mut self, layout_widgets: &mut LayoutUpdates) {
        let _ = layout_widgets;
    }

    /// Called after every sequence of updates and layout if render was requested.
    ///
    /// The [`UiNode::render`] and [`UiNode::render_update`] methods are called here by extensions that manage windows.
    ///
    /// [`UiNode::render`]: crate::widget::instance::UiNode::render
    /// [`UiNode::render_update`]: crate::widget::instance::UiNode::render_update
    fn render(&mut self, render_widgets: &mut RenderUpdates, render_update_widgets: &mut RenderUpdates) {
        let _ = (render_widgets, render_update_widgets);
    }

    /// Called when the application is exiting.
    ///
    /// Update requests and event notifications generated during this call are ignored,
    /// the extensions will be dropped after every extension received this call.
    fn deinit(&mut self) {}

    /// The extension in a box.
    fn boxed(self) -> Box<dyn AppExtensionBoxed>
    where
        Self: Sized,
    {
        Box::new(self)
    }
}

/// Boxed version of [`AppExtension`].
#[doc(hidden)]
pub trait AppExtensionBoxed: 'static {
    fn register_boxed(&self, info: &mut AppExtensionsInfo);
    fn init_boxed(&mut self);
    fn enable_device_events_boxed(&self) -> bool;
    fn update_preview_boxed(&mut self);
    fn update_ui_boxed(&mut self, updates: &mut WidgetUpdates);
    fn update_boxed(&mut self);
    fn event_preview_boxed(&mut self, update: &mut EventUpdate);
    fn event_ui_boxed(&mut self, update: &mut EventUpdate);
    fn event_boxed(&mut self, update: &mut EventUpdate);
    fn info_boxed(&mut self, info_widgets: &mut InfoUpdates);
    fn layout_boxed(&mut self, layout_widgets: &mut LayoutUpdates);
    fn render_boxed(&mut self, render_widgets: &mut RenderUpdates, render_update_widgets: &mut RenderUpdates);
    fn deinit_boxed(&mut self);
}
impl<T: AppExtension> AppExtensionBoxed for T {
    fn register_boxed(&self, info: &mut AppExtensionsInfo) {
        self.register(info);
    }

    fn init_boxed(&mut self) {
        self.init();
    }

    fn enable_device_events_boxed(&self) -> bool {
        self.enable_device_events()
    }

    fn update_preview_boxed(&mut self) {
        self.update_preview();
    }

    fn update_ui_boxed(&mut self, updates: &mut WidgetUpdates) {
        self.update_ui(updates);
    }

    fn info_boxed(&mut self, info_widgets: &mut InfoUpdates) {
        self.info(info_widgets);
    }

    fn update_boxed(&mut self) {
        self.update();
    }

    fn event_preview_boxed(&mut self, update: &mut EventUpdate) {
        self.event_preview(update);
    }

    fn event_ui_boxed(&mut self, update: &mut EventUpdate) {
        self.event_ui(update);
    }

    fn event_boxed(&mut self, update: &mut EventUpdate) {
        self.event(update);
    }

    fn layout_boxed(&mut self, layout_widgets: &mut LayoutUpdates) {
        self.layout(layout_widgets);
    }

    fn render_boxed(&mut self, render_widgets: &mut RenderUpdates, render_update_widgets: &mut RenderUpdates) {
        self.render(render_widgets, render_update_widgets);
    }

    fn deinit_boxed(&mut self) {
        self.deinit();
    }
}
impl AppExtension for Box<dyn AppExtensionBoxed> {
    fn register(&self, info: &mut AppExtensionsInfo) {
        self.as_ref().register_boxed(info);
    }

    fn init(&mut self) {
        self.as_mut().init_boxed();
    }

    fn enable_device_events(&self) -> bool {
        self.as_ref().enable_device_events_boxed()
    }

    fn update_preview(&mut self) {
        self.as_mut().update_preview_boxed();
    }

    fn update_ui(&mut self, update_widgets: &mut WidgetUpdates) {
        self.as_mut().update_ui_boxed(update_widgets);
    }

    fn update(&mut self) {
        self.as_mut().update_boxed();
    }

    fn event_preview(&mut self, update: &mut EventUpdate) {
        self.as_mut().event_preview_boxed(update);
    }

    fn event_ui(&mut self, update: &mut EventUpdate) {
        self.as_mut().event_ui_boxed(update);
    }

    fn event(&mut self, update: &mut EventUpdate) {
        self.as_mut().event_boxed(update);
    }

    fn info(&mut self, info_widgets: &mut InfoUpdates) {
        self.as_mut().info_boxed(info_widgets);
    }

    fn layout(&mut self, layout_widgets: &mut LayoutUpdates) {
        self.as_mut().layout_boxed(layout_widgets);
    }

    fn render(&mut self, render_widgets: &mut RenderUpdates, render_update_widgets: &mut RenderUpdates) {
        self.as_mut().render_boxed(render_widgets, render_update_widgets);
    }

    fn deinit(&mut self) {
        self.as_mut().deinit_boxed();
    }

    fn boxed(self) -> Box<dyn AppExtensionBoxed>
    where
        Self: Sized,
    {
        self
    }
}

struct TraceAppExt<E: AppExtension>(E);
impl<E: AppExtension> AppExtension for TraceAppExt<E> {
    fn register(&self, info: &mut AppExtensionsInfo) {
        self.0.register(info)
    }

    fn init(&mut self) {
        let _span = UpdatesTrace::extension_span::<E>("init");
        self.0.init();
    }

    fn enable_device_events(&self) -> bool {
        self.0.enable_device_events()
    }

    fn event_preview(&mut self, update: &mut EventUpdate) {
        let _span = UpdatesTrace::extension_span::<E>("event_preview");
        self.0.event_preview(update);
    }

    fn event_ui(&mut self, update: &mut EventUpdate) {
        let _span = UpdatesTrace::extension_span::<E>("event_ui");
        self.0.event_ui(update);
    }

    fn event(&mut self, update: &mut EventUpdate) {
        let _span = UpdatesTrace::extension_span::<E>("event");
        self.0.event(update);
    }

    fn update_preview(&mut self) {
        let _span = UpdatesTrace::extension_span::<E>("update_preview");
        self.0.update_preview();
    }

    fn update_ui(&mut self, update_widgets: &mut WidgetUpdates) {
        let _span = UpdatesTrace::extension_span::<E>("update_ui");
        self.0.update_ui(update_widgets);
    }

    fn update(&mut self) {
        let _span = UpdatesTrace::extension_span::<E>("update");
        self.0.update();
    }

    fn info(&mut self, info_widgets: &mut InfoUpdates) {
        let _span = UpdatesTrace::extension_span::<E>("info");
        self.0.info(info_widgets);
    }

    fn layout(&mut self, layout_widgets: &mut LayoutUpdates) {
        let _span = UpdatesTrace::extension_span::<E>("layout");
        self.0.layout(layout_widgets);
    }

    fn render(&mut self, render_widgets: &mut RenderUpdates, render_update_widgets: &mut RenderUpdates) {
        let _span = UpdatesTrace::extension_span::<E>("render");
        self.0.render(render_widgets, render_update_widgets);
    }

    fn deinit(&mut self) {
        let _span = UpdatesTrace::extension_span::<E>("deinit");
        self.0.deinit();
    }

    fn boxed(self) -> Box<dyn AppExtensionBoxed>
    where
        Self: Sized,
    {
        Box::new(self)
    }
}

/// Info about an app-extension.
///
/// See [`APP::extensions`] for more details.
#[derive(Clone, Copy)]
pub struct AppExtensionInfo {
    /// Extension type ID.
    pub type_id: TypeId,
    /// Extension type name.
    pub type_name: &'static str,
}
impl PartialEq for AppExtensionInfo {
    fn eq(&self, other: &Self) -> bool {
        self.type_id == other.type_id
    }
}
impl fmt::Debug for AppExtensionInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.type_name)
    }
}
impl Eq for AppExtensionInfo {}
impl AppExtensionInfo {
    /// New info for `E`.
    pub fn new<E: AppExtension>() -> Self {
        Self {
            type_id: TypeId::of::<E>(),
            type_name: type_name::<E>(),
        }
    }
}

/// List of app-extensions that are part of an app.
#[derive(Clone, PartialEq)]
pub struct AppExtensionsInfo {
    infos: Vec<AppExtensionInfo>,
}
impl fmt::Debug for AppExtensionsInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.infos).finish()
    }
}
impl AppExtensionsInfo {
    pub(crate) fn start() -> Self {
        Self { infos: vec![] }
    }

    /// Push the extension info.
    pub fn push<E: AppExtension>(&mut self) {
        let info = AppExtensionInfo::new::<E>();
        assert!(!self.contains::<E>(), "app-extension `{info:?}` is already in the list");
        self.infos.push(info);
    }

    /// Gets if the extension `E` is in the list.
    pub fn contains<E: AppExtension>(&self) -> bool {
        self.contains_info(AppExtensionInfo::new::<E>())
    }

    /// Gets i the extension is in the list.
    pub fn contains_info(&self, info: AppExtensionInfo) -> bool {
        self.infos.iter().any(|e| e.type_id == info.type_id)
    }

    /// Panics if the extension `E` is not present.
    #[track_caller]
    pub fn require<E: AppExtension>(&self) {
        let info = AppExtensionInfo::new::<E>();
        assert!(self.contains_info(info), "app-extension `{info:?}` is required");
    }
}
impl ops::Deref for AppExtensionsInfo {
    type Target = [AppExtensionInfo];

    fn deref(&self) -> &Self::Target {
        &self.infos
    }
}

/// Desired next step of app main loop.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[must_use = "methods that return `ControlFlow` expect to be inside a controlled loop"]
pub enum ControlFlow {
    /// Immediately try to receive more app events.
    Poll,
    /// Sleep until an app event is received.
    ///
    /// Note that a deadline might be set in case a timer is running.
    Wait,
    /// Exit the loop and drop the app.
    Exit,
}
impl ControlFlow {
    /// Assert that the value is [`ControlFlow::Wait`].
    #[track_caller]
    pub fn assert_wait(self) {
        assert_eq!(ControlFlow::Wait, self)
    }

    /// Assert that the value is [`ControlFlow::Exit`].
    #[track_caller]
    pub fn assert_exit(self) {
        assert_eq!(ControlFlow::Exit, self)
    }
}

/// A headless app controller.
///
/// Headless apps don't cause external side-effects like visible windows and don't listen to system events.
/// They can be used for creating apps like a command line app that renders widgets, or for creating integration tests.
pub struct HeadlessApp {
    app: RunningApp<Box<dyn AppExtensionBoxed>>,
}
impl HeadlessApp {
    /// If headless rendering is enabled.
    ///
    /// When enabled windows are still not visible but frames will be rendered and the frame
    /// image can be requested. Renderer is disabled by default in a headless app.
    ///
    /// Apps with render enabled can only be initialized in the main thread due to limitations of some operating systems,
    /// this means you cannot run a headless renderer in units tests.
    ///
    /// Note that [`UiNode::render`] is still called when a renderer is disabled and you can still
    /// query the latest frame from `WINDOWS.widget_tree`. The only thing that
    /// is disabled is WebRender and the generation of frame textures.
    ///
    /// [`UiNode::render`]: crate::widget::instance::UiNode::render
    pub fn renderer_enabled(&mut self) -> bool {
        VIEW_PROCESS.is_available()
    }

    /// If device events are enabled in this app.
    pub fn device_events(&self) -> bool {
        self.app.device_events()
    }

    /// Does updates unobserved.
    ///
    /// See [`update_observed`] for more details.
    ///
    /// [`update_observed`]: HeadlessApp::update
    pub fn update(&mut self, wait_app_event: bool) -> ControlFlow {
        self.update_observed(&mut (), wait_app_event)
    }

    /// Does updates observing [`update`] only.
    ///
    /// See [`update_observed`] for more details.
    ///
    /// [`update`]: AppEventObserver::update
    /// [`update_observed`]: HeadlessApp::update
    pub fn update_observe(&mut self, on_update: impl FnMut(), wait_app_event: bool) -> ControlFlow {
        struct Observer<F>(F);
        impl<F: FnMut()> AppEventObserver for Observer<F> {
            fn update(&mut self) {
                (self.0)()
            }
        }
        let mut observer = Observer(on_update);

        self.update_observed(&mut observer, wait_app_event)
    }

    /// Does updates observing [`event`] only.
    ///
    /// See [`update_observed`] for more details.
    ///
    /// [`event`]: AppEventObserver::event
    /// [`update_observed`]: HeadlessApp::update
    pub fn update_observe_event(&mut self, on_event: impl FnMut(&mut EventUpdate), wait_app_event: bool) -> ControlFlow {
        struct Observer<F>(F);
        impl<F: FnMut(&mut EventUpdate)> AppEventObserver for Observer<F> {
            fn event(&mut self, update: &mut EventUpdate) {
                (self.0)(update);
            }
        }
        let mut observer = Observer(on_event);
        self.update_observed(&mut observer, wait_app_event)
    }

    /// Does updates with an [`AppEventObserver`].
    ///
    /// If `wait_app_event` is `true` the thread sleeps until at least one app event is received or a timer elapses,
    /// if it is `false` only responds to app events already in the buffer.
    pub fn update_observed<O: AppEventObserver>(&mut self, observer: &mut O, mut wait_app_event: bool) -> ControlFlow {
        loop {
            match self.app.poll(wait_app_event, observer) {
                ControlFlow::Poll => {
                    wait_app_event = false;
                    continue;
                }
                flow => return flow,
            }
        }
    }

    /// Execute the async `task` in the UI thread, updating the app until it finishes or the app shuts-down.
    ///
    /// Returns the task result if the app has not shut-down.
    pub fn run_task<R, T>(&mut self, task: T) -> Option<R>
    where
        R: 'static,
        T: Future<Output = R> + Send + Sync + 'static,
    {
        let mut task = UiTask::new(None, task);

        let mut flow = self.update_observe(
            || {
                task.update();
            },
            false,
        );

        if task.update().is_some() {
            let r = task.into_result().ok();
            debug_assert!(r.is_some());
            return r;
        }

        let mut n = 0;
        while flow != ControlFlow::Exit {
            flow = self.update_observe(
                || {
                    task.update();
                },
                true,
            );

            if n == 10_000 {
                tracing::error!("excessive future awaking, run_task ran 10_000 update cycles without finishing");
            } else if n == 100_000 {
                panic!("run_task stuck, ran 100_000 update cycles without finishing");
            }
            n += 1;

            match task.into_result() {
                Ok(r) => return Some(r),
                Err(t) => task = t,
            }
        }

        None
    }

    /// Requests and wait for app exit.
    ///
    /// Forces deinit if exit is cancelled.
    pub fn exit(mut self) {
        self.run_task(async move {
            let req = APP.exit();
            req.wait_rsp().await;
        });
    }
}

/// Observer for [`HeadlessApp::update_observed`].
///
/// This works like a temporary app extension that runs only for the update call.
pub trait AppEventObserver {
    /// Called for each raw event received.
    fn raw_event(&mut self, ev: &zero_ui_view_api::Event) {
        let _ = ev;
    }

    /// Called just after [`AppExtension::event_preview`].
    fn event_preview(&mut self, update: &mut EventUpdate) {
        let _ = update;
    }

    /// Called just after [`AppExtension::event_ui`].
    fn event_ui(&mut self, update: &mut EventUpdate) {
        let _ = update;
    }

    /// Called just after [`AppExtension::event`].
    fn event(&mut self, update: &mut EventUpdate) {
        let _ = update;
    }

    /// Called just after [`AppExtension::update_preview`].
    fn update_preview(&mut self) {}

    /// Called just after [`AppExtension::update_ui`].
    fn update_ui(&mut self, update_widgets: &mut WidgetUpdates) {
        let _ = update_widgets;
    }

    /// Called just after [`AppExtension::update`].
    fn update(&mut self) {}

    /// Called just after [`AppExtension::info`].
    fn info(&mut self, info_widgets: &mut InfoUpdates) {
        let _ = info_widgets;
    }

    /// Called just after [`AppExtension::layout`].
    fn layout(&mut self, layout_widgets: &mut LayoutUpdates) {
        let _ = layout_widgets;
    }

    /// Called just after [`AppExtension::render`].
    fn render(&mut self, render_widgets: &mut RenderUpdates, render_update_widgets: &mut RenderUpdates) {
        let _ = (render_widgets, render_update_widgets);
    }

    /// Cast to dynamically dispatched observer, this can help avoid code bloat.
    ///
    /// The app methods that accept observers automatically use this method if the feature `"dyn_app_extension"` is active.
    fn as_dyn(&mut self) -> DynAppEventObserver
    where
        Self: Sized,
    {
        DynAppEventObserver(self)
    }
}
/// Nil observer, does nothing.
impl AppEventObserver for () {}

#[doc(hidden)]
pub struct DynAppEventObserver<'a>(&'a mut dyn AppEventObserverDyn);

trait AppEventObserverDyn {
    fn raw_event_dyn(&mut self, ev: &zero_ui_view_api::Event);
    fn event_preview_dyn(&mut self, update: &mut EventUpdate);
    fn event_ui_dyn(&mut self, update: &mut EventUpdate);
    fn event_dyn(&mut self, update: &mut EventUpdate);
    fn update_preview_dyn(&mut self);
    fn update_ui_dyn(&mut self, updates: &mut WidgetUpdates);
    fn update_dyn(&mut self);
    fn info_dyn(&mut self, info_widgets: &mut InfoUpdates);
    fn layout_dyn(&mut self, layout_widgets: &mut LayoutUpdates);
    fn render_dyn(&mut self, render_widgets: &mut RenderUpdates, render_update_widgets: &mut RenderUpdates);
}
impl<O: AppEventObserver> AppEventObserverDyn for O {
    fn raw_event_dyn(&mut self, ev: &zero_ui_view_api::Event) {
        self.raw_event(ev)
    }

    fn event_preview_dyn(&mut self, update: &mut EventUpdate) {
        self.event_preview(update)
    }

    fn event_ui_dyn(&mut self, update: &mut EventUpdate) {
        self.event_ui(update)
    }

    fn event_dyn(&mut self, update: &mut EventUpdate) {
        self.event(update)
    }

    fn update_preview_dyn(&mut self) {
        self.update_preview()
    }

    fn update_ui_dyn(&mut self, update_widgets: &mut WidgetUpdates) {
        self.update_ui(update_widgets)
    }

    fn update_dyn(&mut self) {
        self.update()
    }

    fn info_dyn(&mut self, info_widgets: &mut InfoUpdates) {
        self.info(info_widgets)
    }

    fn layout_dyn(&mut self, layout_widgets: &mut LayoutUpdates) {
        self.layout(layout_widgets)
    }

    fn render_dyn(&mut self, render_widgets: &mut RenderUpdates, render_update_widgets: &mut RenderUpdates) {
        self.render(render_widgets, render_update_widgets)
    }
}
impl<'a> AppEventObserver for DynAppEventObserver<'a> {
    fn raw_event(&mut self, ev: &zero_ui_view_api::Event) {
        self.0.raw_event_dyn(ev)
    }

    fn event_preview(&mut self, update: &mut EventUpdate) {
        self.0.event_preview_dyn(update)
    }

    fn event_ui(&mut self, update: &mut EventUpdate) {
        self.0.event_ui_dyn(update)
    }

    fn event(&mut self, update: &mut EventUpdate) {
        self.0.event_dyn(update)
    }

    fn update_preview(&mut self) {
        self.0.update_preview_dyn()
    }

    fn update_ui(&mut self, update_widgets: &mut WidgetUpdates) {
        self.0.update_ui_dyn(update_widgets)
    }

    fn update(&mut self) {
        self.0.update_dyn()
    }

    fn info(&mut self, info_widgets: &mut InfoUpdates) {
        self.0.info_dyn(info_widgets)
    }

    fn layout(&mut self, layout_widgets: &mut LayoutUpdates) {
        self.0.layout_dyn(layout_widgets)
    }

    fn render(&mut self, render_widgets: &mut RenderUpdates, render_update_widgets: &mut RenderUpdates) {
        self.0.render_dyn(render_widgets, render_update_widgets)
    }

    fn as_dyn(&mut self) -> DynAppEventObserver {
        DynAppEventObserver(self.0)
    }
}

impl AppExtension for () {
    fn register(&self, _: &mut AppExtensionsInfo) {}
}
impl<A: AppExtension, B: AppExtension> AppExtension for (A, B) {
    fn init(&mut self) {
        self.0.init();
        self.1.init();
    }

    fn register(&self, info: &mut AppExtensionsInfo) {
        self.0.register(info);
        self.1.register(info);
    }

    fn enable_device_events(&self) -> bool {
        self.0.enable_device_events() || self.1.enable_device_events()
    }

    fn update_preview(&mut self) {
        self.0.update_preview();
        self.1.update_preview();
    }

    fn update_ui(&mut self, update_widgets: &mut WidgetUpdates) {
        self.0.update_ui(update_widgets);
        self.1.update_ui(update_widgets);
    }

    fn update(&mut self) {
        self.0.update();
        self.1.update();
    }

    fn info(&mut self, info_widgets: &mut InfoUpdates) {
        self.0.info(info_widgets);
        self.1.info(info_widgets);
    }

    fn layout(&mut self, layout_widgets: &mut LayoutUpdates) {
        self.0.layout(layout_widgets);
        self.1.layout(layout_widgets);
    }

    fn render(&mut self, render_widgets: &mut RenderUpdates, render_update_widgets: &mut RenderUpdates) {
        self.0.render(render_widgets, render_update_widgets);
        self.1.render(render_widgets, render_update_widgets);
    }

    fn event_preview(&mut self, update: &mut EventUpdate) {
        self.0.event_preview(update);
        self.1.event_preview(update);
    }

    fn event_ui(&mut self, update: &mut EventUpdate) {
        self.0.event_ui(update);
        self.1.event_ui(update);
    }

    fn event(&mut self, update: &mut EventUpdate) {
        self.0.event(update);
        self.1.event(update);
    }

    fn deinit(&mut self) {
        self.1.deinit();
        self.0.deinit();
    }
}

#[cfg(dyn_app_extension)]
impl AppExtension for Vec<Box<dyn AppExtensionBoxed>> {
    fn init(&mut self) {
        for ext in self {
            ext.init();
        }
    }

    fn register(&self, info: &mut AppExtensionsInfo) {
        for ext in self {
            ext.register(info);
        }
    }

    fn enable_device_events(&self) -> bool {
        self.iter().any(|e| e.enable_device_events())
    }

    fn update_preview(&mut self) {
        for ext in self {
            ext.update_preview();
        }
    }

    fn update_ui(&mut self, update_widgets: &mut WidgetUpdates) {
        for ext in self {
            ext.update_ui(update_widgets);
        }
    }

    fn update(&mut self) {
        for ext in self {
            ext.update();
        }
    }

    fn event_preview(&mut self, update: &mut EventUpdate) {
        for ext in self {
            ext.event_preview(update);
        }
    }

    fn event_ui(&mut self, update: &mut EventUpdate) {
        for ext in self {
            ext.event_ui(update);
        }
    }

    fn event(&mut self, update: &mut EventUpdate) {
        for ext in self {
            ext.event(update);
        }
    }

    fn info(&mut self, info_widgets: &mut InfoUpdates) {
        for ext in self {
            ext.info(info_widgets);
        }
    }

    fn layout(&mut self, layout_widgets: &mut LayoutUpdates) {
        for ext in self {
            ext.layout(layout_widgets);
        }
    }

    fn render(&mut self, render_widgets: &mut RenderUpdates, render_update_widgets: &mut RenderUpdates) {
        for ext in self {
            ext.render(render_widgets, render_update_widgets);
        }
    }

    fn deinit(&mut self) {
        for ext in self.iter_mut().rev() {
            ext.deinit();
        }
    }
}

/// Start and manage an app process.
///
/// # View Process
///
/// A view-process must be initialized before starting an app. Panics on `run` if there is
/// no view-process, also panics if the current process is already executing as a view-process.
pub struct APP;
impl APP {
    /// If the crate was build with `feature="multi_app"`.
    ///
    /// If `true` multiple apps can run in the same process, but only one app per thread at a time.
    pub fn multi_app_enabled(&self) -> bool {
        cfg!(feature = "multi_app")
    }

    /// If an app is already running in the current thread.
    ///
    /// An app is *running* as soon as it starts building, and it stops running after
    /// [`AppExtended::run`] returns or the [`HeadlessApp`] is dropped.
    ///
    /// You can use `app_local!` to create *static* resources that live for the app lifetime.
    pub fn is_running(&self) -> bool {
        LocalContext::current_app().is_some()
    }

    /// Gets an unique ID for the current app.
    ///
    /// This ID usually does not change as most apps only run once per process, but it can change often during tests.
    /// Resources that interact with `app_local!` values can use this ID to ensure that they are still operating in the same
    /// app.
    pub fn id(&self) -> Option<AppId> {
        LocalContext::current_app()
    }

    #[cfg(not(feature = "multi_app"))]
    fn assert_can_run_single() {
        use std::sync::atomic::*;
        static CAN_RUN: AtomicBool = AtomicBool::new(true);

        if !CAN_RUN.swap(false, Ordering::SeqCst) {
            panic!("only one app is allowed per process")
        }
    }

    fn assert_can_run() {
        #[cfg(not(feature = "multi_app"))]
        Self::assert_can_run_single();
        if APP.is_running() {
            panic!("only one app is allowed per thread")
        }
    }

    /// Returns a [`WindowMode`] value that indicates if the app is headless, headless with renderer or headed.
    ///
    /// Note that specific windows can be in headless modes even if the app is headed.
    pub fn window_mode(&self) -> WindowMode {
        if VIEW_PROCESS.is_available() {
            if VIEW_PROCESS.is_headless_with_render() {
                WindowMode::HeadlessWithRenderer
            } else {
                WindowMode::Headed
            }
        } else {
            WindowMode::Headless
        }
    }
    /// List of app extensions that are part of the current app.
    pub fn extensions(&self) -> Arc<AppExtensionsInfo> {
        APP_PROCESS_SV.read().extensions()
    }
}

impl APP {
    /// Application without extensions.
    #[cfg(dyn_app_extension)]
    pub fn minimal(&self) -> AppExtended<Vec<Box<dyn AppExtensionBoxed>>> {
        assert_not_view_process();
        Self::assert_can_run();
        check_deadlock();
        let scope = LocalContext::start_app(AppId::new_unique());
        AppExtended {
            extensions: vec![],
            view_process_exe: None,
            _cleanup: scope,
        }
    }

    #[cfg(not(dyn_app_extension))]
    pub fn minimal(&self) -> AppExtended<()> {
        assert_not_view_process();
        Self::assert_can_run();
        check_deadlock();
        let scope = LocalContext::start_app(AppId::new_unique());
        AppExtended {
            extensions: (),
            view_process_exe: None,
            _cleanup: scope,
        }
    }
}

/// Application with extensions.
///
/// See [`APP`].
pub struct AppExtended<E: AppExtension> {
    extensions: E,
    view_process_exe: Option<PathBuf>,

    // cleanup on drop.
    _cleanup: AppScope,
}
#[cfg(dyn_app_extension)]
impl AppExtended<Vec<Box<dyn AppExtensionBoxed>>> {
    /// Includes an application extension.
    pub fn extend<F: AppExtension>(mut self, extension: F) -> AppExtended<Vec<Box<dyn AppExtensionBoxed>>> {
        self.extensions.push(TraceAppExt(extension).boxed());
        self
    }

    /// If the application should notify raw device events.
    ///
    /// Device events are raw events not targeting any window, like a mouse move on any part of the screen.
    /// They tend to be high-volume events so there is a performance cost to activating this. Note that if
    /// this is `false` you still get the mouse move over windows of the app.
    pub fn enable_device_events(self) -> AppExtended<Vec<Box<dyn AppExtensionBoxed>>> {
        struct EnableDeviceEvents;
        impl AppExtension for EnableDeviceEvents {
            fn enable_device_events(&self) -> bool {
                true
            }
        }
        self.extend(EnableDeviceEvents)
    }
}

#[cfg(not(dyn_app_extension))]
impl<E: AppExtension> AppExtended<E> {
    /// Includes an application extension.
    pub fn extend<F: AppExtension>(self, extension: F) -> AppExtended<impl AppExtension> {
        AppExtended {
            _cleanup: self._cleanup,
            extensions: (self.extensions, TraceAppExt(extension)),
            view_process_exe: self.view_process_exe,
        }
    }

    /// If the application should notify raw device events.
    ///
    /// Device events are raw events not targeting any window, like a mouse move on any part of the screen.
    /// They tend to be high-volume events so there is a performance cost to activating this. Note that if
    /// this is `false` you still get the mouse move over windows of the app.
    pub fn enable_device_events(self) -> AppExtended<impl AppExtension> {
        struct EnableDeviceEvents;
        impl AppExtension for EnableDeviceEvents {
            fn enable_device_events(&self) -> bool {
                true
            }
        }
        self.extend(EnableDeviceEvents)
    }
}
impl<E: AppExtension> AppExtended<E> {
    /// Set the path to the executable for the *View Process*.
    ///
    /// By the default the current executable is started again as a *View Process*, you can use
    /// two executables instead, by setting this value.
    ///
    /// Note that the `view_process_exe` must start a view server and both
    /// executables must be build using the same exact [`VERSION`].
    ///
    /// [`VERSION`]: zero_ui_view_api::VERSION  
    pub fn view_process_exe(mut self, view_process_exe: impl Into<PathBuf>) -> Self {
        self.view_process_exe = Some(view_process_exe.into());
        self
    }

    /// Starts the app, then starts polling `start` to run.
    ///
    /// This method only returns when the app has exited.
    ///
    /// The `start` task runs in a [`UiTask`] in the app context, note that it only needs to start the app, usually
    /// by opening a window, the app will keep running after `start` is finished.
    pub fn run(mut self, start: impl Future<Output = ()> + Send + 'static) {
        let app = RunningApp::start(self._cleanup, self.extensions, true, true, self.view_process_exe.take());

        UPDATES.run(start).perm();

        app.run_headed();
    }

    /// Initializes extensions in headless mode and returns an [`HeadlessApp`].
    ///
    /// If `with_renderer` is `true` spawns a renderer process for headless rendering. See [`HeadlessApp::renderer_enabled`]
    /// for more details.
    pub fn run_headless(mut self, with_renderer: bool) -> HeadlessApp {
        let app = RunningApp::start(
            self._cleanup,
            self.extensions.boxed(),
            false,
            with_renderer,
            self.view_process_exe.take(),
        );

        HeadlessApp { app }
    }
}

mod private {
    // https://rust-lang.github.io/api-guidelines/future-proofing.html#sealed-traits-protect-against-downstream-implementations-c-sealed
    pub trait Sealed {}
}

/// Sets a `tracing` subscriber that writes warnings to stderr and panics on errors.
///
/// Panics if another different subscriber is already set.
#[cfg(any(test, feature = "test_util"))]
pub fn test_log() {
    use std::sync::atomic::*;

    use tracing::*;

    struct TestSubscriber;
    impl Subscriber for TestSubscriber {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            metadata.is_event() && metadata.level() < &Level::WARN
        }

        fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
            unimplemented!()
        }

        fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {
            unimplemented!()
        }

        fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {
            unimplemented!()
        }

        fn event(&self, event: &Event<'_>) {
            struct MsgCollector<'a>(&'a mut String);
            impl<'a> field::Visit for MsgCollector<'a> {
                fn record_debug(&mut self, field: &field::Field, value: &dyn fmt::Debug) {
                    use std::fmt::Write;
                    write!(self.0, "\n  {} = {:?}", field.name(), value).unwrap();
                }
            }

            let meta = event.metadata();
            let file = meta.file().unwrap_or("");
            let line = meta.line().unwrap_or(0);

            let mut msg = format!("[{file}:{line}]");
            event.record(&mut MsgCollector(&mut msg));

            if meta.level() == &Level::ERROR {
                panic!("[LOG-ERROR]{msg}");
            } else {
                eprintln!("[LOG-WARN]{msg}");
            }
        }

        fn enter(&self, _span: &span::Id) {
            unimplemented!()
        }
        fn exit(&self, _span: &span::Id) {
            unimplemented!()
        }
    }

    static IS_SET: AtomicBool = AtomicBool::new(false);

    if !IS_SET.swap(true, Ordering::Relaxed) {
        if let Err(e) = subscriber::set_global_default(TestSubscriber) {
            panic!("failed to set test log subscriber, {e:?}");
        }
    }
}
