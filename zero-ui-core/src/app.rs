//! App startup and app extension API.

use crate::context::*;
use crate::crate_util::PanicPayload;
use crate::event::{cancelable_event_args, AnyEventUpdate, EventUpdate, EventUpdateArgs, Events};
use crate::image::ImageManager;
use crate::profiler::*;
use crate::timer::Timers;
use crate::var::{response_var, ResponderVar, ResponseVar, Vars};
use crate::{
    focus::FocusManager,
    gesture::GestureManager,
    keyboard::KeyboardManager,
    mouse::MouseManager,
    service::Service,
    text::FontManager,
    window::{WindowId, WindowManager},
};

use linear_map::LinearMap;
use std::future::Future;
use std::sync::Arc;
use std::task::Waker;
use std::{
    any::{type_name, TypeId},
    fmt,
    time::Instant,
};

pub use zero_ui_wr::{init, ControlFlow};

/// Error when the app connected to a sender/receiver channel has shutdown.
///
/// Contains the value that could not be send or `()` for receiver errors.
pub struct AppShutdown<T>(pub T);
impl From<flume::RecvError> for AppShutdown<()> {
    fn from(_: flume::RecvError) -> Self {
        AppShutdown(())
    }
}
impl<T> From<flume::SendError<T>> for AppShutdown<T> {
    fn from(e: flume::SendError<T>) -> Self {
        AppShutdown(e.0)
    }
}
impl<T> fmt::Debug for AppShutdown<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AppHasShutdown<{}>", type_name::<T>())
    }
}
impl<T> fmt::Display for AppShutdown<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot send/receive because the app has shutdown")
    }
}
impl<T> std::error::Error for AppShutdown<T> {}

/// Error when the app connected to a sender channel has shutdown or taken to long to respond.
pub enum TimeoutOrAppShutdown {
    /// Connected app has not responded.
    Timeout,
    /// Connected app has shutdown.
    AppShutdown,
}
impl From<flume::RecvTimeoutError> for TimeoutOrAppShutdown {
    fn from(e: flume::RecvTimeoutError) -> Self {
        match e {
            flume::RecvTimeoutError::Timeout => TimeoutOrAppShutdown::Timeout,
            flume::RecvTimeoutError::Disconnected => TimeoutOrAppShutdown::AppShutdown,
        }
    }
}
impl fmt::Debug for TimeoutOrAppShutdown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            write!(f, "AppHasNotRespondedOrShutdown::")?;
        }
        match self {
            TimeoutOrAppShutdown::Timeout => write!(f, "Timeout"),
            TimeoutOrAppShutdown::AppShutdown => write!(f, "AppShutdown"),
        }
    }
}
impl fmt::Display for TimeoutOrAppShutdown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimeoutOrAppShutdown::Timeout => write!(f, "failed send, timeout"),
            TimeoutOrAppShutdown::AppShutdown => write!(f, "cannot send because the app has shutdown"),
        }
    }
}
impl std::error::Error for TimeoutOrAppShutdown {}

/// A future that receives a single message from a running [app](App).
pub struct RecvFut<'a, M>(flume::r#async::RecvFut<'a, M>);
impl<'a, M> From<flume::r#async::RecvFut<'a, M>> for RecvFut<'a, M> {
    fn from(f: flume::r#async::RecvFut<'a, M>) -> Self {
        Self(f)
    }
}
impl<'a, M> Future for RecvFut<'a, M> {
    type Output = Result<M, AppShutdown<()>>;

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        match std::pin::Pin::new(&mut self.0).poll(cx) {
            std::task::Poll::Ready(r) => std::task::Poll::Ready(r.map_err(|_| AppShutdown(()))),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

/// An [`App`] extension.
#[cfg_attr(doc_nightly, doc(notable_trait))]
pub trait AppExtension: 'static {
    /// Type id of this extension.
    #[inline]
    fn id(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    /// If this extension is the `app_extension_id` or dispatches to it.
    #[inline]
    fn is_or_contain(&self, app_extension_id: TypeId) -> bool {
        self.id() == app_extension_id
    }

    /// Initializes this extension.
    #[inline]
    fn init(&mut self, ctx: &mut AppContext) {
        let _ = ctx;
    }

    /// If the application should notify raw device events.
    ///
    /// Device events are raw events not targeting any window, like a mouse move on any part of the screen.
    /// They tend to be high-volume events so there is a performance cost to activating this. Note that if
    /// this is `false` you still get the mouse move over windows of the app.
    ///
    /// This is called zero or one times after [`init`](Self::init).
    ///
    /// Returns `false` by default.
    #[inline]
    fn enable_device_events(&self) -> bool {
        false
    }

    /// Called just before [`update_ui`](Self::update_ui).
    ///
    /// Extensions can handle this method to interact with updates before the UI.
    ///
    /// Note that this is not related to the `on_event_preview` properties, all UI events
    /// happen in `update_ui`.
    #[inline]
    fn update_preview(&mut self, ctx: &mut AppContext) {
        let _ = ctx;
    }

    /// Called just before [`update`](Self::update).
    ///
    /// Only extensions that generate windows must handle this method. The [`UiNode::update`](super::UiNode::update)
    /// method is called here.
    #[inline]
    fn update_ui(&mut self, ctx: &mut AppContext) {
        let _ = ctx;
    }

    /// Called after every [`update_ui`](Self::update_ui).
    ///
    /// This is the general extensions update, it gives the chance for
    /// the UI to signal stop propagation.
    #[inline]
    fn update(&mut self, ctx: &mut AppContext) {
        let _ = ctx;
    }

    /// Called just before [`event_ui`](Self::event_ui).
    ///
    /// Extensions can handle this method to to intersect event updates before the UI.
    ///
    /// Note that this is not related to the `on_event_preview` properties, all UI events
    /// happen in `on_event_ui`.
    #[inline]
    fn event_preview<EV: EventUpdateArgs>(&mut self, ctx: &mut AppContext, args: &EV) {
        let _ = (ctx, args);
    }

    /// Called just before [`event`](Self::event).
    ///
    /// Only extensions that generate windows must handle this method. The [`UiNode::event`](super::UiNode::event)
    /// method is called here.
    #[inline]
    fn event_ui<EV: EventUpdateArgs>(&mut self, ctx: &mut AppContext, args: &EV) {
        let _ = (ctx, args);
    }

    /// Called after every [`event_ui`](Self::event_ui).
    ///
    /// This is the general extensions event handler, it gives the chance for the UI to signal stop propagation.
    #[inline]
    fn event<EV: EventUpdateArgs>(&mut self, ctx: &mut AppContext, args: &EV) {
        let _ = (ctx, args);
    }

    /// Called after every sequence of updates if display update was requested.
    #[inline]
    fn update_display(&mut self, ctx: &mut AppContext, update: UpdateDisplayRequest) {
        let _ = (ctx, update);
    }

    /// Called when a new frame is ready to be presented.
    #[inline]
    fn new_frame_ready(&mut self, ctx: &mut AppContext, window_id: WindowId) {
        let _ = (ctx, window_id);
    }

    /// Called when the OS sends a request for re-drawing the last frame.
    #[inline]
    fn redraw_requested(&mut self, ctx: &mut AppContext, window_id: WindowId) {
        let _ = (ctx, window_id);
    }

    /// Called when a shutdown was requested.
    #[inline]
    fn shutdown_requested(&mut self, ctx: &mut AppContext, args: &ShutdownRequestedArgs) {
        let _ = (ctx, args);
    }

    /// Called when the application is shutting down.
    ///
    /// Update requests and event notifications generated during this call are ignored.
    #[inline]
    fn deinit(&mut self, ctx: &mut AppContext) {
        let _ = ctx;
    }

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
    fn id_boxed(&self) -> TypeId;
    fn is_or_contain_boxed(&self, app_extension_id: TypeId) -> bool;
    fn init_boxed(&mut self, ctx: &mut AppContext);
    fn enable_device_events_boxed(&self) -> bool;
    fn update_preview_boxed(&mut self, ctx: &mut AppContext);
    fn update_ui_boxed(&mut self, ctx: &mut AppContext);
    fn update_boxed(&mut self, ctx: &mut AppContext);
    fn event_preview_boxed(&mut self, ctx: &mut AppContext, args: &AnyEventUpdate);
    fn event_ui_boxed(&mut self, ctx: &mut AppContext, args: &AnyEventUpdate);
    fn event_boxed(&mut self, ctx: &mut AppContext, args: &AnyEventUpdate);
    fn update_display_boxed(&mut self, ctx: &mut AppContext, update: UpdateDisplayRequest);
    fn new_frame_ready_boxed(&mut self, ctx: &mut AppContext, window_id: WindowId);
    fn redraw_requested_boxed(&mut self, ctx: &mut AppContext, window_id: WindowId);
    fn shutdown_requested_boxed(&mut self, ctx: &mut AppContext, args: &ShutdownRequestedArgs);
    fn deinit_boxed(&mut self, ctx: &mut AppContext);
}
impl<T: AppExtension> AppExtensionBoxed for T {
    fn id_boxed(&self) -> TypeId {
        self.id()
    }

    fn is_or_contain_boxed(&self, app_extension_id: TypeId) -> bool {
        self.is_or_contain(app_extension_id)
    }

    fn init_boxed(&mut self, ctx: &mut AppContext) {
        self.init(ctx);
    }

    fn enable_device_events_boxed(&self) -> bool {
        self.enable_device_events()
    }

    fn update_preview_boxed(&mut self, ctx: &mut AppContext) {
        self.update_preview(ctx);
    }

    fn update_ui_boxed(&mut self, ctx: &mut AppContext) {
        self.update_ui(ctx);
    }

    fn update_boxed(&mut self, ctx: &mut AppContext) {
        self.update(ctx);
    }

    fn event_preview_boxed(&mut self, ctx: &mut AppContext, args: &AnyEventUpdate) {
        self.event_preview(ctx, args);
    }

    fn event_ui_boxed(&mut self, ctx: &mut AppContext, args: &AnyEventUpdate) {
        self.event_ui(ctx, args);
    }

    fn event_boxed(&mut self, ctx: &mut AppContext, args: &AnyEventUpdate) {
        self.event(ctx, args);
    }

    fn update_display_boxed(&mut self, ctx: &mut AppContext, update: UpdateDisplayRequest) {
        self.update_display(ctx, update);
    }

    fn new_frame_ready_boxed(&mut self, ctx: &mut AppContext, window_id: WindowId) {
        self.new_frame_ready(ctx, window_id);
    }

    fn redraw_requested_boxed(&mut self, ctx: &mut AppContext, window_id: WindowId) {
        self.redraw_requested(ctx, window_id);
    }

    fn shutdown_requested_boxed(&mut self, ctx: &mut AppContext, args: &ShutdownRequestedArgs) {
        self.shutdown_requested(ctx, args);
    }

    fn deinit_boxed(&mut self, ctx: &mut AppContext) {
        self.deinit(ctx);
    }
}
impl AppExtension for Box<dyn AppExtensionBoxed> {
    fn id(&self) -> TypeId {
        self.as_ref().id_boxed()
    }

    fn is_or_contain(&self, app_extension_id: TypeId) -> bool {
        self.as_ref().is_or_contain_boxed(app_extension_id)
    }

    fn init(&mut self, ctx: &mut AppContext) {
        self.as_mut().init_boxed(ctx);
    }

    fn enable_device_events(&self) -> bool {
        self.as_ref().enable_device_events_boxed()
    }

    fn update_preview(&mut self, ctx: &mut AppContext) {
        self.as_mut().update_preview_boxed(ctx);
    }

    fn update_ui(&mut self, ctx: &mut AppContext) {
        self.as_mut().update_ui_boxed(ctx);
    }

    fn update(&mut self, ctx: &mut AppContext) {
        self.as_mut().update_boxed(ctx);
    }

    fn event_preview<EV: EventUpdateArgs>(&mut self, ctx: &mut AppContext, args: &EV) {
        let args = args.as_any();
        self.as_mut().event_preview_boxed(ctx, &args);
    }

    fn event_ui<EV: EventUpdateArgs>(&mut self, ctx: &mut AppContext, args: &EV) {
        let args = args.as_any();
        self.as_mut().event_ui_boxed(ctx, &args);
    }

    fn event<EV: EventUpdateArgs>(&mut self, ctx: &mut AppContext, args: &EV) {
        let args = args.as_any();
        self.as_mut().event_boxed(ctx, &args);
    }

    fn update_display(&mut self, ctx: &mut AppContext, update: UpdateDisplayRequest) {
        self.as_mut().update_display_boxed(ctx, update);
    }

    fn new_frame_ready(&mut self, ctx: &mut AppContext, window_id: WindowId) {
        self.as_mut().new_frame_ready_boxed(ctx, window_id);
    }

    fn redraw_requested(&mut self, ctx: &mut AppContext, window_id: WindowId) {
        self.as_mut().redraw_requested_boxed(ctx, window_id);
    }

    fn shutdown_requested(&mut self, ctx: &mut AppContext, args: &ShutdownRequestedArgs) {
        self.as_mut().shutdown_requested_boxed(ctx, args);
    }

    fn deinit(&mut self, ctx: &mut AppContext) {
        self.as_mut().deinit_boxed(ctx);
    }

    fn boxed(self) -> Box<dyn AppExtensionBoxed>
    where
        Self: Sized,
    {
        self
    }
}

cancelable_event_args! {
    /// Arguments for `on_shutdown_requested`.
    pub struct ShutdownRequestedArgs {
        ..
        /// Always true.
        fn concerns_widget(&self, _: &mut WidgetContext) -> bool {
            true
        }
    }
}

/// Defines and runs an application.
///
/// # Init
///
/// You must call the [`init`] function before all other code in the app `main` function, see [`init`]
/// for more details.
///
/// The [`init`] function is called in [`blank`] and [`default`], so if this is the first thing
/// you are doing in the `main` function you don't need to call [`init`].
///
/// # Debug Log
///
/// In debug builds, `App` sets a [`logger`](log) that prints warnings and errors to `stderr`
/// if no logger was registered before the call to [`blank`] or [`default`].
///
/// [`blank`]: App::blank
/// [`default`]: App::default
pub struct App;

impl App {
    /// If an app is already running in the current thread.
    ///
    /// Only a single app is allowed per-thread.
    #[inline]
    pub fn is_running() -> bool {
        crate::var::Vars::instantiated() || crate::event::Events::instantiated()
    }
}

// In release mode we use generics tricks to compile all app extensions with
// static dispatch optimized to a direct call to the extension handle.
#[cfg(not(debug_assertions))]
impl App {
    /// Application without any extension.
    #[inline]
    pub fn blank() -> AppExtended<()> {
        AppExtended { extensions: () }
    }

    /// Application with default extensions.
    ///
    /// # Extensions
    ///
    /// Extensions included.
    ///
    /// * [MouseManager]
    /// * [KeyboardManager]
    /// * [GestureManager]
    /// * [WindowManager]
    /// * [FontManager]
    /// * [FocusManager]
    /// * [ImageManager]
    #[inline]
    pub fn default() -> AppExtended<impl AppExtension> {
        App::blank()
            .extend(MouseManager::default())
            .extend(KeyboardManager::default())
            .extend(GestureManager::default())
            .extend(WindowManager::default())
            .extend(FontManager::default())
            .extend(FocusManager::default())
            .extend(ImageManager::default())
    }
}

// In debug mode we use dynamic dispatch to reduce the number of types
// in the stack-trace and compile more quickly.
#[cfg(debug_assertions)]
impl App {
    /// Application without any extension and without device events.
    pub fn blank() -> AppExtended<Vec<Box<dyn AppExtensionBoxed>>> {
        init();
        DebugLogger::init();
        AppExtended { extensions: vec![] }
    }

    /// Application with default extensions.
    ///
    /// # Extensions
    ///
    /// Extensions included.
    ///
    /// * [MouseManager]
    /// * [KeyboardManager]
    /// * [GestureManager]
    /// * [WindowManager]
    /// * [FontManager]
    /// * [FocusManager]
    /// * [ImageManager]
    pub fn default() -> AppExtended<Vec<Box<dyn AppExtensionBoxed>>> {
        App::blank()
            .extend(MouseManager::default())
            .extend(KeyboardManager::default())
            .extend(GestureManager::default())
            .extend(WindowManager::default())
            .extend(FontManager::default())
            .extend(FocusManager::default())
            .extend(ImageManager::default())
    }
}

/// Application with extensions.
pub struct AppExtended<E: AppExtension> {
    extensions: E,
}

/// Cancellation message of a [shutdown request](AppProcess::shutdown).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShutdownCancelled;
impl fmt::Display for ShutdownCancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "shutdown cancelled")
    }
}

/// Service for managing the application process.
///
/// This service is registered for all apps.
#[derive(Service)]
pub struct AppProcess {
    shutdown_requests: Option<ResponderVar<ShutdownCancelled>>,
    update_sender: AppEventSender,
}
impl AppProcess {
    fn new(update_sender: AppEventSender) -> Self {
        AppProcess {
            shutdown_requests: None,
            update_sender,
        }
    }

    /// Register a request for process shutdown in the next update.
    ///
    /// Returns an event listener that is updated once with the unit value [`ShutdownCancelled`]
    /// if the shutdown operation is cancelled.
    pub fn shutdown(&mut self) -> ResponseVar<ShutdownCancelled> {
        if let Some(r) = &self.shutdown_requests {
            r.response_var()
        } else {
            let (responder, response) = response_var();
            self.shutdown_requests = Some(responder);
            let _ = self.update_sender.send_update();
            response
        }
    }

    fn take_requests(&mut self) -> Option<ResponderVar<ShutdownCancelled>> {
        self.shutdown_requests.take()
    }
}

#[cfg(debug_assertions)]
impl AppExtended<Vec<Box<dyn AppExtensionBoxed>>> {
    /// Includes an application extension.
    ///
    /// # Panics
    ///
    /// * `"app already extended with `{}`"` when the app is already [`extended_with`](AppExtended::extended_with) the
    /// extension type.
    #[inline]
    pub fn extend<F: AppExtension>(self, extension: F) -> AppExtended<Vec<Box<dyn AppExtensionBoxed>>> {
        if self.extended_with::<F>() {
            panic!("app already extended with `{}`", type_name::<F>())
        }

        let mut extensions = self.extensions;
        extensions.push(extension.boxed());

        AppExtended { extensions }
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

#[cfg(not(debug_assertions))]
impl<E: AppExtension> AppExtended<E> {
    /// Includes an application extension.
    ///
    /// # Panics
    ///
    /// * `"app already extended with `{}`"` when the app is already [`extended_with`](AppExtended::extended_with) the
    /// extension type.
    #[inline]
    pub fn extend<F: AppExtension>(self, extension: F) -> AppExtended<impl AppExtension> {
        if self.extended_with::<F>() {
            panic!("app already extended with `{}`", type_name::<F>())
        }
        AppExtended {
            extensions: (self.extensions, extension),
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
    /// Gets if the application is already extended with the extension type.
    #[inline]
    pub fn extended_with<F: AppExtension>(&self) -> bool {
        self.extensions.is_or_contain(TypeId::of::<F>())
    }

    /// Runs the application calling `start` once at the beginning.
    ///
    /// # Panics
    ///
    /// Panics if not called by the main thread. This means you cannot run an app in unit tests, use a headless
    /// app without renderer for that. The main thread is required by some operating systems and OpenGL.
    pub fn run(self, start: impl FnOnce(&mut AppContext)) -> ! {
        #[cfg(feature = "app_profiler")]
        register_thread_with_profiler();

        profile_scope!("app::run");

        let mut app = RunningApp::start(self.extensions);

        start(&mut app.ctx());

        app.run_headed()
    }

    /// Initializes extensions in headless mode and returns an [`HeadlessApp`].
    ///
    /// If `with_renderer` is `true` spawns a renderer process for headless rendering. See [`HeadlessApp::renderer_enabled`]
    /// for more details.
    ///
    /// # Tests
    ///
    /// If called in a test (`cfg(test)`) this blocks until no other instance of [`HeadlessApp`] and
    /// [`TestWidgetContext`] are running in the current thread.
    pub fn run_headless(self, with_renderer: bool) -> HeadlessApp {
        #[cfg(feature = "app_profiler")]
        let profile_scope = {
            register_thread_with_profiler();
            ProfileScope::new("app::run_headless")
        };

        let mut app = RunningApp::start(self.extensions.boxed());

        if with_renderer {
            let renderer = view_process::ViewProcess::start(
                view_process::StartRequest {
                    device_events: false,
                    headless: true,
                },
                |_| unreachable!(),
            );

            app.ctx().services.register(renderer);
        }

        HeadlessApp {
            app,

            #[cfg(feature = "app_profiler")]
            _pf: profile_scope,
        }
    }
}

/// Represents a running app controlled by an external event loop.
pub struct RunningApp<E: AppExtension> {
    extensions: E,
    device_events: bool,
    owned_ctx: OwnedAppContext,
    receiver: flume::Receiver<AppEvent>,

    // need to probe context to see if there are updates.
    maybe_has_updates: bool,
    // WaitUntil time.
    wake_time: Option<Instant>,

    // shutdown was requested.
    exiting: bool,

    window_ids: LinearMap<u32, WindowId>,
    device_ids: LinearMap<u32, WindowId>,
}
impl<E: AppExtension> RunningApp<E> {
    fn start(mut extensions: E) -> Self {
        if App::is_running() {
            if cfg!(any(test, doc, feature = "test_util")) {
                panic!("only one app or `TestWidgetContext` is allowed per thread")
            } else {
                panic!("only one app is allowed per thread")
            }
        }

        let (sender, receiver) = AppEventSender::new();

        let mut owned_ctx = OwnedAppContext::instance(sender);

        let mut ctx = owned_ctx.borrow();
        ctx.services.register(AppProcess::new(ctx.updates.sender()));
        extensions.init(&mut ctx);

        RunningApp {
            device_events: extensions.enable_device_events(),
            extensions,
            owned_ctx,
            receiver,
            maybe_has_updates: true,
            wake_time: None,
            exiting: false,
            window_ids: LinearMap::new(),
            device_ids: LinearMap::new(),
        }
    }

    fn run_headed(mut self) -> ! {
        let view_evs_sender = self.ctx().updates.sender();

        let view_app = view_process::ViewProcess::start(
            view_process::StartRequest {
                device_events: self.device_events,
                headless: false,
            },
            move |ev| view_evs_sender.send_view_event(ev).unwrap(),
        );
        self.ctx().services.register(view_app);

        loop {
            let ev = self.receiver.recv().unwrap();
            self.app_event(ev);
        }
    }

    /// Exclusive borrow the app context.
    pub fn ctx(&mut self) -> AppContext {
        self.maybe_has_updates = true;
        self.owned_ctx.borrow()
    }

    /// Borrow the [`Vars`] only.
    pub fn vars(&self) -> &Vars {
        self.owned_ctx.vars()
    }

    /// Event loop has awakened because [`WaitUntil`](ControlFlow::WaitUntil) was requested.
    pub fn wait_until_elapsed(&mut self) {
        self.maybe_has_updates = true;
    }

    /// Notify an event directly to the app extensions.
    pub fn notify_event<Ev: crate::event::Event>(&mut self, _event: Ev, args: Ev::Args) {
        let update = EventUpdate::<Ev>(args);
        let mut ctx = self.owned_ctx.borrow();
        self.extensions.event_preview(&mut ctx, &update);
        self.extensions.event_ui(&mut ctx, &update);
        self.extensions.event(&mut ctx, &update);
        self.maybe_has_updates = true;
    }

    fn window_id(&mut self, id: u32) -> WindowId {
        *self.window_ids.entry(id).or_insert_with(WindowId::new_unique)
    }

    fn device_id(&mut self, id: u32) -> DeviceId {
        *self.window_ids.entry(id).or_insert_with(WindowId::new_unique)
    }

    /// Process a View Process event.
    pub fn view_event(&mut self, ev: zero_ui_wr::Ev) {
        use raw_device_events::*;
        use raw_events::*;

        match ev {
            zero_ui_wr::Ev::WindowResized(w_id, size) => {
                let args = RawWindowResizedArgs::now(self.window_id(w_id), size);
                self.notify_event(RawWindowResizedEvent, args);
            }
            zero_ui_wr::Ev::WindowMoved(w_id, pos) => {
                let args = RawWindowMovedArgs::now(self.window_id(w_id), pos);
                self.notify_event(RawWindowMovedEvent, args);
            }
            zero_ui_wr::Ev::DroppedFile(w_id, file) => {
                let args = RawDroppedFileArgs::now(self.window_id(w_id), file);
                self.notify_event(RawDroppedFileEvent, args);
            }
            zero_ui_wr::Ev::HoveredFile(w_id, file) => {
                let args = RawHoveredFileArgs::now(self.window_id(w_id), file);
                self.notify_event(RawHoveredFileEvent, args);
            }
            zero_ui_wr::Ev::HoveredFileCancelled(w_id) => {
                let args = RawHoveredFileCancelledArgs::now(self.window_id(w_id));
                self.notify_event(RawHoveredFileCancelledEvent, args);
            }
            zero_ui_wr::Ev::ReceivedCharacter(w_id, c) => {
                let args = RawCharInputArgs::now(self.window_id(w_id), c);
                self.notify_event(RawCharInputEvent, args);
            }
            zero_ui_wr::Ev::Focused(w_id, focused) => {
                let args = RawWindowFocusArgs::now(self.window_id(w_id), focused);
                self.notify_event(RawWindowFocusEvent, args);
            }
            zero_ui_wr::Ev::KeyboardInput(w_id, d_id, input) => {
                let args = RawKeyInputArgs::now(
                    self.window_id(w_id),
                    self.device_id(d_id),
                    input.scancode,
                    input.state,
                    input.virtual_keycode.map(Into::into),
                );
                self.notify_event(RawKeyInputEvent, args);
            }
            zero_ui_wr::Ev::ModifiersChanged(w_id, state) => {
                let args = RawModifiersChangedArgs::now(self.window_id(w_id), state);
                self.notify_event(RawModifiersChangedEvent, args);
            }
            zero_ui_wr::Ev::CursorMoved(w_id, d_id, pos) => {
                let args = RawCursorMovedArgs::now(self.window_id(w_id), self.device_id(d_id), pos);
                self.notify_event(RawCursorMovedEvent, args);
            }
            zero_ui_wr::Ev::CursorEntered(w_id, d_id) => {
                let args = RawCursorArgs::now(self.window_id(w_id), self.device_id(d_id));
                self.notify_event(RawCursorEnteredEvent, args);
            }
            zero_ui_wr::Ev::CursorLeft(w_id, d_id) => {
                let args = RawCursorArgs::now(self.window_id(w_id), self.device_id(d_id));
                self.notify_event(RawCursorLeftEvent, args);
            }
            zero_ui_wr::Ev::MouseWheel(w_id, d_id, delta, phase) => {
                // TODO
                let _ = (delta, phase);
                let args = RawMouseWheelArgs::now(self.window_id(w_id), self.device_id(d_id));
                self.notify_event(RawMouseWheelEvent, args);
            }
            zero_ui_wr::Ev::MouseInput(w_id, d_id, state, button) => {
                let args = RawMouseInputArgs::now(self.window_id(w_id), self.device_id(d_id), state, button);
                self.notify_event(RawMouseInputEvent, args);
            }
            zero_ui_wr::Ev::TouchpadPressure(w_id, d_id, pressure, stage) => {
                // TODO
                let _ = (pressure, stage);
                let args = RawTouchpadPressureArgs::now(self.window_id(w_id), self.device_id(d_id));
                self.notify_event(RawTouchpadPressureEvent, args);
            }
            zero_ui_wr::Ev::AxisMotion(w_id, d_id, axis, value) => {
                let args = RawAxisMotionArgs::now(self.window_id(w_id), self.device_id(d_id), axis, value);
                self.notify_event(RawAxisMotionEvent, args);
            }
            zero_ui_wr::Ev::Touch(w_id, d_id, phase, pos, force, finger_id) => {
                // TODO
                let args = RawTouchArgs::now(self.window_id(w_id), self.device_id(d_id));
                self.notify_event(RawTouchEvent, args);
            }
            zero_ui_wr::Ev::ScaleFactorChanged(w_id, scale, new_size) => {
                let args = RawWindowScaleFactorChangedArgs::now(self.window_id(w_id), scale, new_size);
                self.notify_event(RawWindowScaleFactorChangedEvent, args);
            }
            zero_ui_wr::Ev::ThemeChanged(w_id, theme) => {
                let args = RawWindowThemeChangedArgs::now(self.window_id(w_id), theme);
                self.notify_event(RawWindowThemeChangedEvent, args);
            }
            zero_ui_wr::Ev::WindowCloseRequested(w_id) => {
                let args = RawWindowCloseRequestedArgs::now(self.window_id(w_id));
                self.notify_event(RawWindowCloseRequestedEvent, args);
            }
            zero_ui_wr::Ev::WindowClosed(w_id) => {
                let args = RawWindowClosedArgs::now(self.window_id(w_id));
                self.notify_event(RawWindowClosedEvent, args);
            }

            // `device_events`
            zero_ui_wr::Ev::DeviceAdded(d_id) => {
                let args = DeviceArgs::now(self.device_id(d_id));
                self.notify_event(DeviceAddedEvent, args);
            }
            zero_ui_wr::Ev::DeviceRemoved(d_id) => {
                let args = DeviceArgs::now(self.device_id(d_id));
                self.notify_event(DeviceRemovedEvent, args);
            }
            zero_ui_wr::Ev::DeviceMouseMotion(d_id, delta) => {
                let args = MouseMotionArgs::now(self.device_id(d_id), delta);
                self.notify_event(MouseMotionEvent, args);
            }
            zero_ui_wr::Ev::DeviceMouseWheel(d_id, delta) => {
                let args = MouseWheelArgs::now(self.device_id(d_id), *delta);
                self.notify_event(MouseWheelEvent, args);
            }
            zero_ui_wr::Ev::DeviceMotion(d_id, axis, value) => {
                let args = MotionArgs::now(self.device_id(d_id), axis, value);
                self.notify_event(MotionEvent, args);
            }
            zero_ui_wr::Ev::DeviceButton(d_id, button, state) => {
                let args = ButtonArgs::now(self.device_id(d_id), button, state);
                self.notify_event(ButtonEvent, args);
            }
            zero_ui_wr::Ev::DeviceKey(d_id, k) => {
                let args = KeyArgs::now(self.device_id(d_id), k.scancode, k.state, k.virtual_keycode.map(Into::into));
                self.notify_event(KeyEvent, args);
            }
            zero_ui_wr::Ev::DeviceText(d_id, c) => {
                let args = TextArgs::now(self.device_id(d_id), c);
                self.notify_event(TextEvent, args);
            }
        }

        self.maybe_has_updates = true;
    }

    /// Process an [`AppEvent`].
    pub fn app_event(&mut self, app_event: AppEvent) {
        match app_event {
            AppEvent::ViewEvent(ev) => self.view_event(ev),
            AppEvent::NewFrameReady(window_id) => {
                let mut ctx = self.owned_ctx.borrow();
                self.extensions.new_frame_ready(&mut ctx, window_id);
            }
            AppEvent::Update => {
                self.owned_ctx.borrow().updates.update();
            }
            AppEvent::Event(e) => {
                self.owned_ctx.borrow().events.notify_app_event(e);
            }
            AppEvent::Var => {
                self.owned_ctx.borrow().vars.receive_sended_modify();
            }
            AppEvent::ResumeUnwind(p) => std::panic::resume_unwind(p),
        }
        self.maybe_has_updates = true;
    }

    /// Process application suspension.
    pub fn suspended(&mut self) {
        log::error!(target: "app", "TODO suspended");
    }

    /// Process application resume from suspension.
    pub fn resumed(&mut self) {
        log::error!(target: "app", "TODO resumed");
    }

    /// Does pending event and updates until there is no more updates generated, then returns
    /// [`WaitUntil`](ControlFlow::WaitUntil) are timers running or returns [`Wait`](ControlFlow::WaitUntil)
    /// if there aren't.
    ///
    /// You can use an [`AppUpdateObserver`] to watch all of these actions or pass `&mut ()` as a NOP observer.
    pub fn update<O: AppUpdateObserver>(&mut self, observer: &mut O) -> ControlFlow {
        if self.maybe_has_updates {
            self.maybe_has_updates = false;

            let mut display_update = UpdateDisplayRequest::None;

            let mut limit = 100_000;
            loop {
                limit -= 1;
                if limit == 0 {
                    panic!("update loop polled 100,000 times, probably stuck in an infinite loop");
                }

                let u = self.owned_ctx.apply_updates();

                self.wake_time = u.wake_time;
                display_update |= u.display_update;

                if u.update {
                    let mut ctx = self.owned_ctx.borrow();

                    // check shutdown.
                    if let Some(r) = ctx.services.app_process().take_requests() {
                        let args = ShutdownRequestedArgs::now();
                        self.extensions.shutdown_requested(&mut ctx, &args);
                        if args.cancel_requested() {
                            r.respond(ctx.vars, ShutdownCancelled);
                        }
                        self.exiting = !args.cancel_requested();
                        if self.exiting {
                            return ControlFlow::Exit;
                        }
                    }

                    // does `Timers::on_*` notifications.
                    Timers::notify(&mut ctx);

                    // does `Event` notifications.
                    for event in u.events {
                        self.extensions.event_preview(&mut ctx, &event);
                        observer.event_preview(&mut ctx, &event);
                        Events::on_pre_events(&mut ctx, &event);

                        self.extensions.event_ui(&mut ctx, &event);
                        observer.event_ui(&mut ctx, &event);

                        self.extensions.event(&mut ctx, &event);
                        observer.event(&mut ctx, &event);
                        Events::on_events(&mut ctx, &event);
                    }

                    // does general updates.
                    self.extensions.update_preview(&mut ctx);
                    observer.update_preview(&mut ctx);
                    Updates::on_pre_updates(&mut ctx);

                    self.extensions.update_ui(&mut ctx);
                    observer.update_ui(&mut ctx);

                    self.extensions.update(&mut ctx);
                    observer.update(&mut ctx);
                    Updates::on_updates(&mut ctx);
                } else if display_update != UpdateDisplayRequest::None {
                    display_update = UpdateDisplayRequest::None;

                    let mut ctx = self.owned_ctx.borrow();

                    self.extensions.update_display(&mut ctx, display_update);
                    observer.update_display(&mut ctx, display_update);
                } else {
                    break;
                }
            }
        }

        if self.exiting {
            ControlFlow::Exit
        } else if let Some(wake) = self.wake_time {
            ControlFlow::WaitUntil(wake)
        } else {
            ControlFlow::Wait
        }
    }

    /// De-initializes extensions and drops.
    pub fn shutdown(mut self) {
        let mut ctx = self.owned_ctx.borrow();
        self.extensions.deinit(&mut ctx);
    }
}

/// A headless app controller.
///
/// Headless apps don't cause external side-effects like visible windows and don't listen to system events.
/// They can be used for creating apps like a command line app that renders widgets, or for creating integration tests.
pub struct HeadlessApp {
    app: RunningApp<Box<dyn AppExtensionBoxed>>,
    #[cfg(feature = "app_profiler")]
    _pf: ProfileScope,
}
impl HeadlessApp {
    /// App state.
    pub fn app_state(&self) -> &StateMap {
        self.app.owned_ctx.app_state()
    }

    /// Mutable app state.
    pub fn app_state_mut(&mut self) -> &mut StateMap {
        self.app.owned_ctx.app_state_mut()
    }

    /// If headless rendering is enabled.
    ///
    /// When enabled windows are still not visible but you can request [frame pixels](crate::window::OpenWindow::frame_pixels)
    /// to get the frame image. Renderer is disabled by default in a headless app.
    ///
    /// Only windows opened after enabling have a renderer. Already open windows are not changed by this method. When enabled
    /// headless windows can only be initialized in the main thread due to limitations of OpenGL, this means you cannot run
    /// a headless renderer in units tests.
    ///
    /// Note that [`UiNode::render`](crate::UiNode::render) is still called when a renderer is disabled and you can still
    /// query the latest frame from [`OpenWindow::frame_info`](crate::window::OpenWindow::frame_info). The only thing that
    /// is disabled is WebRender and the generation of frame textures.
    pub fn renderer_enabled(&self) -> bool {
        self.ctx().services.get::<view_process::ViewProcess>().is_some()
    }

    /// Borrows the app context.
    pub fn ctx(&mut self) -> AppContext {
        self.app.ctx()
    }

    /// Borrow the [`Vars`] only.
    pub fn vars(&self) -> &Vars {
        self.app.vars()
    }

    /// Does updates unobserved.
    ///
    /// See [`update_observed`](Self::update_observed) for more details.
    #[inline]
    pub fn update(&mut self, wait_app_event: bool) -> ControlFlow {
        self.update_observed(&mut (), wait_app_event)
    }

    /// Does updates observing [`update`](AppUpdateObserver::update) only.
    ///
    /// See [`update_observed`](Self::update_observed) for more details.
    pub fn update_observe(&mut self, on_update: impl FnMut(&mut AppContext), wait_app_event: bool) -> ControlFlow {
        struct Observer<F>(F);
        impl<F: FnMut(&mut AppContext)> AppUpdateObserver for Observer<F> {
            fn update(&mut self, ctx: &mut AppContext) {
                (self.0)(ctx)
            }
        }
        let mut observer = Observer(on_update);
        self.update_observed(&mut observer, wait_app_event)
    }

    /// Does updates observing [`event`](AppUpdateObserver::event) only.
    ///
    /// See [`update_observed`](Self::update_observed) for more details.
    pub fn update_observe_event(&mut self, on_event: impl FnMut(&mut AppContext, &AnyEventUpdate), wait_app_event: bool) -> ControlFlow {
        struct Observer<F>(F);
        impl<F: FnMut(&mut AppContext, &AnyEventUpdate)> AppUpdateObserver for Observer<F> {
            fn event<EU: EventUpdateArgs>(&mut self, ctx: &mut AppContext, args: &EU) {
                let args = args.as_any();
                (self.0)(ctx, &args);
            }
        }
        let mut observer = Observer(on_event);
        self.update_observed(&mut observer, wait_app_event)
    }

    /// Does updates with an [`AppUpdateObserver`].
    ///
    /// If `wait_app_event` is `true` the thread sleeps until at least one app event is received,
    /// if it is `false` only responds to app events already in the buffer.
    ///
    /// Does updates until there are no more updates to do, returns [`Exit`](ControlFlow::Exit) if app has shutdown,
    /// or returns [`WaitUntil`](ControlFlow::WaitUntil) if a timer is running or returns [`Wait`](ControlFlow::Wait)
    /// if the app is sleeping.
    pub fn update_observed<O: AppUpdateObserver>(&mut self, observer: &mut O, wait_app_event: bool) -> ControlFlow {
        if wait_app_event {
            if let Ok(event) = self.app_event_receiver.recv() {
                self.app.app_event(event);
            }
        }
        for event in self.app_event_receiver.try_iter() {
            self.app.app_event(event);
        }

        let r = self.app.update(observer);
        debug_assert!(r != ControlFlow::Poll);

        r
    }
}

/// Observer for [`HeadlessApp::update_observed`] and [`RunningApp::update`].
pub trait AppUpdateObserver {
    /// Called just after [`AppExtension::event_preview`].
    fn event_preview<EU: EventUpdateArgs>(&mut self, ctx: &mut AppContext, args: &EU) {
        let _ = (ctx, args);
    }

    /// Called just after [`AppExtension::event_ui`].
    fn event_ui<EU: EventUpdateArgs>(&mut self, ctx: &mut AppContext, args: &EU) {
        let _ = (ctx, args);
    }

    /// Called just after [`AppExtension::event`].
    fn event<EU: EventUpdateArgs>(&mut self, ctx: &mut AppContext, args: &EU) {
        let _ = (ctx, args);
    }

    /// Called just after [`AppExtension::update_preview`].
    fn update_preview(&mut self, ctx: &mut AppContext) {
        let _ = ctx;
    }

    /// Called just after [`AppExtension::update_ui`].
    fn update_ui(&mut self, ctx: &mut AppContext) {
        let _ = ctx;
    }

    /// Called just after [`AppExtension::update`].
    fn update(&mut self, ctx: &mut AppContext) {
        let _ = ctx;
    }

    /// Called just after [`AppExtension::update_display`].
    fn update_display(&mut self, ctx: &mut AppContext, update: UpdateDisplayRequest) {
        let _ = (ctx, update);
    }
}
/// Nil observer, does nothing.
impl AppUpdateObserver for () {}

impl AppExtension for () {
    #[inline]
    fn is_or_contain(&self, _: TypeId) -> bool {
        false
    }
}
impl<A: AppExtension, B: AppExtension> AppExtension for (A, B) {
    #[inline]
    fn init(&mut self, ctx: &mut AppContext) {
        self.0.init(ctx);
        self.1.init(ctx);
    }

    #[inline]
    fn is_or_contain(&self, app_extension_id: TypeId) -> bool {
        self.0.is_or_contain(app_extension_id) || self.1.is_or_contain(app_extension_id)
    }

    #[inline]
    fn enable_device_events(&self) -> bool {
        self.0.enable_device_events() || self.1.enable_device_events()
    }

    #[inline]
    fn new_frame_ready(&mut self, ctx: &mut AppContext, window_id: WindowId) {
        self.0.new_frame_ready(ctx, window_id);
        self.1.new_frame_ready(ctx, window_id);
    }

    #[inline]
    fn update_preview(&mut self, ctx: &mut AppContext) {
        self.0.update_preview(ctx);
        self.1.update_preview(ctx);
    }

    #[inline]
    fn update_ui(&mut self, ctx: &mut AppContext) {
        self.0.update_ui(ctx);
        self.1.update_ui(ctx);
    }

    #[inline]
    fn update(&mut self, ctx: &mut AppContext) {
        self.0.update(ctx);
        self.1.update(ctx);
    }

    #[inline]
    fn update_display(&mut self, ctx: &mut AppContext, update: UpdateDisplayRequest) {
        self.0.update_display(ctx, update);
        self.1.update_display(ctx, update);
    }

    #[inline]
    fn event_preview<EV: EventUpdateArgs>(&mut self, ctx: &mut AppContext, args: &EV) {
        self.0.event_preview(ctx, args);
        self.1.event_preview(ctx, args);
    }

    #[inline]
    fn event_ui<EV: EventUpdateArgs>(&mut self, ctx: &mut AppContext, args: &EV) {
        self.0.event_ui(ctx, args);
        self.1.event_ui(ctx, args);
    }

    #[inline]
    fn event<EV: EventUpdateArgs>(&mut self, ctx: &mut AppContext, args: &EV) {
        self.0.event(ctx, args);
        self.1.event(ctx, args);
    }

    #[inline]
    fn redraw_requested(&mut self, ctx: &mut AppContext, window_id: WindowId) {
        self.0.redraw_requested(ctx, window_id);
        self.1.redraw_requested(ctx, window_id);
    }

    #[inline]
    fn shutdown_requested(&mut self, ctx: &mut AppContext, args: &ShutdownRequestedArgs) {
        self.0.shutdown_requested(ctx, args);
        self.1.shutdown_requested(ctx, args);
    }

    #[inline]
    fn deinit(&mut self, ctx: &mut AppContext) {
        self.0.deinit(ctx);
        self.1.deinit(ctx);
    }
}

#[cfg(debug_assertions)]
impl AppExtension for Vec<Box<dyn AppExtensionBoxed>> {
    fn init(&mut self, ctx: &mut AppContext) {
        for ext in self {
            ext.init(ctx);
        }
    }

    fn is_or_contain(&self, app_extension_id: TypeId) -> bool {
        for ext in self {
            if ext.is_or_contain(app_extension_id) {
                return true;
            }
        }
        false
    }

    fn enable_device_events(&self) -> bool {
        self.iter().any(|e| e.enable_device_events())
    }

    fn new_frame_ready(&mut self, ctx: &mut AppContext, window_id: WindowId) {
        for ext in self {
            ext.new_frame_ready(ctx, window_id);
        }
    }

    fn update_preview(&mut self, ctx: &mut AppContext) {
        for ext in self {
            ext.update_preview(ctx);
        }
    }

    fn update_ui(&mut self, ctx: &mut AppContext) {
        for ext in self {
            ext.update_ui(ctx);
        }
    }

    fn update(&mut self, ctx: &mut AppContext) {
        for ext in self {
            ext.update(ctx);
        }
    }

    fn update_display(&mut self, ctx: &mut AppContext, update: UpdateDisplayRequest) {
        for ext in self {
            ext.update_display(ctx, update);
        }
    }

    fn event_preview<EV: EventUpdateArgs>(&mut self, ctx: &mut AppContext, args: &EV) {
        for ext in self {
            ext.event_preview(ctx, args);
        }
    }

    fn event_ui<EV: EventUpdateArgs>(&mut self, ctx: &mut AppContext, args: &EV) {
        for ext in self {
            ext.event_ui(ctx, args);
        }
    }

    fn event<EV: EventUpdateArgs>(&mut self, ctx: &mut AppContext, args: &EV) {
        for ext in self {
            ext.event(ctx, args);
        }
    }

    fn redraw_requested(&mut self, ctx: &mut AppContext, window_id: WindowId) {
        for ext in self {
            ext.redraw_requested(ctx, window_id);
        }
    }

    fn shutdown_requested(&mut self, ctx: &mut AppContext, args: &ShutdownRequestedArgs) {
        for ext in self {
            ext.shutdown_requested(ctx, args);
        }
    }

    fn deinit(&mut self, ctx: &mut AppContext) {
        for ext in self {
            ext.deinit(ctx);
        }
    }
}

#[derive(Debug)]
enum AppEvent {
    /// Event from the View Process.
    ViewEvent(zero_ui_wr::Ev),
    /// Notify [`Events`](crate::var::Events).
    Event(crate::event::BoxedSendEventUpdate),
    /// Notify [`Vars`](crate::var::Vars).
    Var,
    /// Do an update cycle.
    Update,
    /// Resume a panic in the app thread.
    ResumeUnwind(PanicPayload),
}

/// An [`AppEvent`] sender that can awake apps and insert events into the main loop.
#[derive(Clone)]
pub struct AppEventSender(flume::Sender<AppEvent>);
impl AppEventSender {
    pub(crate) fn new() -> (Self, flume::Receiver<AppEvent>) {
        let (sender, receiver) = flume::unbounded();
        (Self(sender), receiver)
    }

    #[inline(always)]
    fn send_app_event(&self, event: AppEvent) -> Result<(), AppShutdown<AppEvent>> {
        self.0.send(event)?;
        Ok(())
    }

    #[inline(always)]
    fn send_view_event(&self, event: zero_ui_wr::Ev) -> Result<(), AppShutdown<AppEvent>> {
        self.0.send(AppEvent::ViewEvent(event))?;
        Ok(())
    }

    /// Causes an update cycle to happen in the app.
    #[inline]
    pub fn send_update(&self) -> Result<(), AppShutdown<()>> {
        self.send_app_event(AppEvent::Update).map_err(|_| AppShutdown(()))
    }

    /// [`VarSender`](crate::var::VarSender) util.
    #[inline]
    pub(crate) fn send_var(&self) -> Result<(), AppShutdown<()>> {
        self.send_app_event(AppEvent::Var).map_err(|_| AppShutdown(()))
    }

    /// [`EventSender`](crate::event::EventSender) util.
    pub(crate) fn send_event(
        &self,
        event: crate::event::BoxedSendEventUpdate,
    ) -> Result<(), AppShutdown<crate::event::BoxedSendEventUpdate>> {
        self.send_app_event(AppEvent::Event(event)).map_err(|e| match e.0 .0 {
            AppEvent::Event(ev) => AppShutdown(ev),
            _ => unreachable!(),
        })
    }

    /// Resume a panic in the app thread.
    pub fn send_resume_unwind(&self, payload: PanicPayload) -> Result<(), AppShutdown<PanicPayload>> {
        self.send_app_event(AppEvent::ResumeUnwind(payload)).map_err(|e| match e.0 .0 {
            AppEvent::ResumeUnwind(p) => AppShutdown(p),
            _ => unreachable!(),
        })
    }

    /// Create an [`Waker`] that causes a [`send_update`](Self::send_update).
    pub fn waker(&self) -> Waker {
        Arc::new(AppWaker(self.0.clone())).into()
    }
}
struct AppWaker(flume::Sender<AppEvent>);
impl std::task::Wake for AppWaker {
    fn wake(self: std::sync::Arc<Self>) {
        let _ = self.0.send(AppEvent::Update);
    }
}

#[cfg(test)]
mod headless_tests {
    use super::*;

    #[test]
    fn new_default() {
        let mut app = App::default().run_headless(false);
        app.update(false);
    }

    #[test]
    fn new_empty() {
        let mut app = App::blank().run_headless(false);
        app.update(false);
    }

    #[test]
    pub fn new_window_no_render() {
        let mut app = App::default().run_headless(false);
        assert!(!app.renderer_enabled());
        app.update(false);
    }

    #[test]
    pub fn new_window_with_render() {
        let mut app = App::default().run_headless(true);
        assert!(app.renderer_enabled());
        app.update(false);
    }

    #[test]
    #[should_panic(expected = "only one app or `TestWidgetContext` is allowed per thread")]
    pub fn two_in_one_thread() {
        let _a = App::default().run_headless(false);
        let _b = App::default().run_headless(false);
    }

    #[test]
    #[should_panic(expected = "only one `TestWidgetContext` or app is allowed per thread")]
    pub fn app_and_test_ctx() {
        let _a = App::default().run_headless(false);
        let _b = TestWidgetContext::new();
    }

    #[test]
    #[should_panic(expected = "only one app or `TestWidgetContext` is allowed per thread")]
    pub fn test_ctx_and_app() {
        let _a = TestWidgetContext::new();
        let _b = App::default().run_headless(false);
    }
}

#[cfg(debug_assertions)]
struct DebugLogger;

#[cfg(debug_assertions)]
impl DebugLogger {
    fn init() {
        if log::set_logger(&DebugLogger).is_ok() {
            log::set_max_level(log::LevelFilter::Warn);
        }
    }
}

#[cfg(debug_assertions)]
impl log::Log for DebugLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            use colored::*;
            match record.metadata().level() {
                log::Level::Error => {
                    eprintln!("{}: [{}] {}", "error".bright_red().bold(), record.target(), record.args())
                }
                log::Level::Warn => {
                    eprintln!("{}: [{}] {}", "warn".bright_yellow().bold(), record.target(), record.args())
                }
                _ => {}
            }
        }
    }

    fn flush(&self) {}
}

unique_id! {
    /// Unique identifier of a device event source.
    #[derive(Debug)]
    pub struct DeviceId;
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DeviceId({})", self.get())
    }
}

/// View process controller types.
pub mod view_process {
    use std::{cell::RefCell, rc::Rc};

    use linear_map::LinearMap;

    use zero_ui_wr::{DevId, WinId};
    pub use zero_ui_wr::{Ev, OpenWindowRequest, StartRequest, WindowNotFound};

    use super::DeviceId;
    use crate::service::Service;
    use crate::window::WindowId;

    /// Reference to the running View Process.
    ///
    /// This is the lowest level API, used for implementing fundamental services and is a service available
    /// in headed apps or headless apps with renderer.
    #[derive(Service)]
    pub struct ViewProcess(Rc<ViewApp>);
    struct ViewApp {
        process: zero_ui_wr::App,
        window_ids: RefCell<LinearMap<WinId, WindowId>>,
        device_ids: RefCell<LinearMap<DevId, DeviceId>>,
    }
    impl ViewProcess {
        /// Spawn the View Process.
        pub(super) fn start<F>(request: StartRequest, on_event: F) -> Self
        where
            F: FnMut(Ev) + Send + 'static,
        {
            Self(Rc::new(ViewApp {
                process: zero_ui_wr::App::start(request, on_event),
                window_ids: LinearMap::new(),
                device_ids: LinearMap::new(),
            }))
        }

        /// If is running in headless renderer mode.
        #[inline]
        pub fn headless(&self) -> bool {
            self.0.headless()
        }

        /// Open a window and associate it with the `window_id`.
        pub fn open_window(&self, window_id: WindowId, request: OpenWindowRequest) -> ViewWindow {
            assert!(self.0.window_ids.borrow().values().all(|v| v != window_id));

            let id = self.0.process.open_window(request);

            self.0.window_ids.borrow_mut().insert(id, window_id);

            ViewWindow(id, self.0.clone())
        }

        /// Translate `WinId` to `WindowId`.
        pub(super) fn window_id(&self, id: WinId) -> Option<WindowId> {
            self.0.window_ids.borrow().get(&id).copied()
        }

        /// Translate `DevId` to `DeviceId`, generates a device id if it was unknown.
        pub(super) fn device_id(&self, id: DevId) -> DeviceId {
            self.0.device_ids.borrow_mut().entry(id).or_insert_with(DeviceId::new_unique)
        }
    }

    /// Reference to a window open in the View Process.
    ///
    /// The window closes when this struct is dropped.
    pub struct ViewWindow(WinId, Rc<ViewApp>);
    impl ViewWindow {
        /// Set the window title.
        #[inline]
        pub fn set_title(&self, title: String) -> Result<(), WindowNotFound> {
            self.1.process.set_title(self.0, title)
        }

        /// Set the window visibility.
        #[inline]
        pub fn set_visible(&self, visible: bool) -> Result<(), WindowNotFound> {
            self.1.process.set_visible(self.0, visible)
        }

        /// Set the window position (in device pixels).
        #[inline]
        pub fn set_position(&self, x: i32, y: i32) -> Result<(), WindowNotFound> {
            self.1.process.set_position(self.0, (x, y))
        }

        /// Set the window size (in device pixels).
        #[inline]
        pub fn set_size(&self, width: u32, height: u32) -> Result<(), WindowNotFound> {
            self.1.process.set_size(self.0, (width, height))
        }

        /// Reference the window renderer.
        #[inline]
        pub fn renderer(&self) -> ViewRenderer {
            ViewRenderer(self.0, self.1.clone())
        }

        /// Drop `self`.
        pub fn close(self) {
            drop(self)
        }
    }
    impl Drop for ViewWindow {
        fn drop(&mut self) {
            self.1.process.close_window(self.0);
        }
    }

    /// Reference to a window renderer in the View Process.
    pub struct ViewRenderer(WinId, Rc<ViewApp>);
    impl ViewRenderer {
        pub fn read_pixels(&self, x: u32, y: u32, width: u32, height: u32) -> Result<Vec<u8>, WindowNotFound> {
            todo!()
        }
    }
}

/// Events directly from `winit` targeting the app windows.
///
/// These events get processed by [app extensions] to generate the events used in widgets, for example
/// the [`KeyboardManager`] uses the [`RawKeyInputEvent`] into focus targeted events.
///
/// # Synthetic Input
///
/// You can [`notify`] these events to fake hardware input, please be careful that you mimic the exact sequence a real
/// hardware would generate, [app extensions] can assume that the raw events are correct. The [`DeviceId`] for fake
/// input must be unique but constant for each distinctive *synthetic event source*.
///
/// [app extensions]: crate::app::AppExtension
/// [`KeyboardManager`]: crate::keyboard::KeyboardManager
/// [`RawKeyInputEvent`]: crate::app::raw_events::RawKeyInputEvent
/// [`notify`]: crate::event::Event::notify
/// [`DeviceId`]: crate::app::DeviceId
pub mod raw_events {
    use std::path::PathBuf;

    use super::raw_device_events::AxisId;
    use super::DeviceId;
    use crate::mouse::{ButtonState, MouseButton};
    use crate::window::WindowTheme;
    use crate::{event::*, keyboard::ScanCode, window::WindowId};

    use crate::keyboard::{Key, KeyState, ModifiersState};

    event_args! {
        /// Arguments for the [`RawKeyInputEvent`].
        pub struct RawKeyInputArgs {
            /// Window that received the event.
            pub window_id: WindowId,

            /// Keyboard device that generated the event.
            pub device_id: DeviceId,

            /// Raw code of key.
            pub scan_code: ScanCode,

            /// If the key was pressed or released.
            pub state: KeyState,

            /// Symbolic name of [`scan_code`](Self::scan_code).
            pub key: Option<Key>,

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawModifiersChangedEvent`].
        pub struct RawModifiersChangedArgs {
            /// Window that received the event.
            pub window_id: WindowId,

            /// New modifiers state.
            pub modifiers: ModifiersState,

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawCharInputEvent`].
        pub struct RawCharInputArgs {
            /// Window that received the event.
            pub window_id: WindowId,

            /// Unicode character.
            pub character: char,

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawWindowFocusEvent`].
        pub struct RawWindowFocusArgs {
            /// Window that was focuses/blurred.
            pub window_id: WindowId,

            /// If the window received focus.
            pub focused: bool,

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawWindowMovedEvent`].
        pub struct RawWindowMovedArgs {
            /// Window that was moved.
            pub window_id: WindowId,

            /// Window (x, y) position in raw pixels.
            pub position: (i32, i32),

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawWindowResizedEvent`].
        pub struct RawWindowResizedArgs {
            /// Window that was resized.
            pub window_id: WindowId,

            /// Window (width, height) size in raw pixels.
            pub size: (u32, u32),

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawWindowCloseRequestedEvent`].
        pub struct RawWindowCloseRequestedArgs {
            /// Window that was requested to close.
            pub window_id: WindowId,

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawWindowClosedEvent`].
        pub struct RawWindowClosedArgs {
            /// Window that was destroyed.
            pub window_id: WindowId,

            ..

            /// Returns `true` for all widgets.
            fn concerns_widget(&self, _ctx: &mut WidgetContext) -> bool {
                true
            }
        }

        /// Arguments for the [`RawDroppedFileEvent`].
        pub struct RawDroppedFileArgs {
            /// Window where it was dropped.
            pub window_id: WindowId,

            /// Path to file that was dropped.
            pub file: PathBuf,

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawHoveredFileEvent`].
        pub struct RawHoveredFileArgs {
            /// Window where it was dragged over.
            pub window_id: WindowId,

            /// Path to file that was dragged over the window.
            pub file: PathBuf,

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawHoveredFileCancelledEvent`].
        ///
        /// The file is the one that was last [hovered] into the window.
        ///
        /// [hovered]: RawHoveredFileEvent
        pub struct RawHoveredFileCancelledArgs {
            /// Window where the file was previously dragged over.
            pub window_id: WindowId,

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawCursorMovedEvent`].
        pub struct RawCursorMovedArgs {
            /// Window the cursor was moved over.
            pub window_id: WindowId,

            /// Device that generated this event.
            pub device_id: DeviceId,

            /// Position of the cursor over the [window](Self::window_id) in raw pixels.
            pub position: (i32, i32),

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawCursorEnteredEvent`] and [`RawCursorLeftEvent`].
        pub struct RawCursorArgs {
            /// Window the cursor entered or left.
            pub window_id: WindowId,

            /// Device that generated this event.
            pub device_id: DeviceId,

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawMouseWheelEvent`].
        pub struct RawMouseWheelArgs {
            /// Window that is hovered by the cursor.
            pub window_id: WindowId,

            /// Device that generated this event.
            pub device_id: DeviceId,

            // TODO

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawMouseInputEvent`].
        pub struct RawMouseInputArgs {
            /// Window that is hovered by the cursor.
            pub window_id: WindowId,

            /// Device that generated this event.
            pub device_id: DeviceId,

            /// If the button was pressed or released.
            pub state: ButtonState,

            /// What button was pressed or released.
            pub button: MouseButton,

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawTouchpadPressureEvent`].
        pub struct RawTouchpadPressureArgs {
            /// Window that is touched.
            pub window_id: WindowId,

            /// Device that generated this event.
            pub device_id: DeviceId,

            // TODO

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawAxisMotionEvent`].
        pub struct RawAxisMotionArgs {
            /// Window that received the event.
            pub window_id: WindowId,

            /// Device that generated the event.
            pub device_id: DeviceId,

            /// Analog axis.
            pub axis: AxisId,

            /// Motion amount.
            pub value: f64,

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawTouchEvent`].
        pub struct RawTouchArgs {
            /// Window that was touched.
            pub window_id: WindowId,

            /// Device that generated this event.
            pub device_id: DeviceId,

            // TODO

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawWindowScaleFactorChangedEvent`].
        pub struct RawWindowScaleFactorChangedArgs {
            /// Window for which the scale has changed.
            pub window_id: WindowId,

            /// New pixel scale factor.
            pub scale_factor: f64,

            /// New window size in raw pixels.
            ///
            /// The operating system can change the window raw size to match the new scale factor.
            pub size: (u32, u32),

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }

        /// Arguments for the [`RawWindowThemeChangedEvent`].
        pub struct RawWindowThemeChangedArgs {
            /// Window for which the theme was changed.
            pub window_id: WindowId,

            /// New theme.
            pub theme: WindowTheme,

            ..

            /// Returns `true` for all widgets in the [window](Self::window_id).
            fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
                ctx.path.window_id() == self.window_id
            }
        }
    }

    event! {
        /// A key press or release targeting a window.
        ///
        /// This event represents a key input directly from the operating system. It is processed
        /// by [`KeyboardManager`] to generate the [`KeyInputEvent`] that actually targets the focused widget.
        ///
        /// *See also the [module level documentation](self) for details of how you can fake this event*
        ///
        /// [`KeyboardManager`]: crate::keyboard::KeyboardManager
        /// [`KeyInputEvent`]: crate::keyboard::KeyInputEvent
        pub RawKeyInputEvent: RawKeyInputArgs;

        /// A modifier key press or release updated the state of the modifier keys.
        ///
        /// This event represents a key input directly from the operating system. It is processed
        /// by [`KeyboardManager`] to generate the keyboard events that are used in general.
        ///
        /// *See also the [module level documentation](self) for details of how you can fake this event*
        ///
        /// [`KeyboardManager`]: crate::keyboard::KeyboardManager
        pub RawModifiersChangedEvent: RawModifiersChangedArgs;

        /// A window received an Unicode character.
        pub RawCharInputEvent: RawCharInputArgs;

        /// A window received or lost focus.
        pub RawWindowFocusEvent: RawWindowFocusArgs;

        /// A window was moved.
        pub RawWindowMovedEvent: RawWindowMovedArgs;

        /// A window was resized.
        pub RawWindowResizedEvent: RawWindowResizedArgs;

        /// A window was requested to close.
        pub RawWindowCloseRequestedEvent: RawWindowCloseRequestedArgs;

        /// A window was destroyed.
        pub RawWindowClosedEvent: RawWindowClosedArgs;

        /// A file was drag-dropped on a window.
        pub RawDroppedFileEvent: RawDroppedFileArgs;

        /// A file was dragged over a window.
        ///
        /// If the file is dropped [`RawDroppedFileEvent`] will raise.
        pub RawHoveredFileEvent: RawHoveredFileArgs;

        /// A dragging file was moved away from the window or the operation was cancelled.
        ///
        /// The file is the last one that emitted a [`RawHoveredFileEvent`].
        pub RawHoveredFileCancelledEvent: RawHoveredFileCancelledArgs;

        /// Cursor pointer moved over a window.
        pub RawCursorMovedEvent: RawCursorMovedArgs;

        /// Cursor pointer started hovering a window.
        pub RawCursorEnteredEvent: RawCursorArgs;

        /// Cursor pointer stopped hovering a window.
        pub RawCursorLeftEvent: RawCursorArgs;

        /// Mouse wheel scrolled when the cursor was over a window.
        pub RawMouseWheelEvent: RawMouseWheelArgs;

        /// Mouse button was pressed or released when the cursor was over a window.
        pub RawMouseInputEvent: RawMouseInputArgs;

        /// Touchpad touched when the cursor was over a window.
        pub RawTouchpadPressureEvent: RawTouchpadPressureArgs;

        /// Motion on some analog axis send to a window.
        pub RawAxisMotionEvent: RawAxisMotionArgs;

        /// A window was touched.
        pub RawTouchEvent: RawTouchArgs;

        /// Pixel scale factor for a window changed.
        ///
        /// This can happen when the window is dragged to another screen or if the user
        /// change the screen scaling configuration.
        pub RawWindowScaleFactorChangedEvent: RawWindowScaleFactorChangedArgs;

        /// System theme changed for a window.
        pub RawWindowThemeChangedEvent: RawWindowThemeChangedArgs;
    }
}

/// Events directly from `winit` not targeting any windows.
///
/// These events get emitted only if the app [`enable_device_events`]. When enabled they
/// can be used like [`raw_events`].
///
/// [`enable_device_events`]: AppExtended::enable_device_events
pub mod raw_device_events {
    use super::DeviceId;
    use crate::{
        event::*,
        keyboard::{Key, KeyState, ScanCode},
        mouse::ButtonState,
    };

    pub use zero_ui_wr::{AxisId, ButtonId, MouseScrollDelta};

    event_args! {
        /// Arguments for [`DeviceAddedEvent`] and [`DeviceRemovedEvent`].
        pub struct DeviceArgs {
            /// Device that was added/removed.
            pub device_id: DeviceId,

            ..

            /// Returns `true` for all widgets.
            fn concerns_widget(&self, _ctx: &mut WidgetContext) -> bool {
                true
            }
        }

        /// Arguments for [`MouseMotionEvent`].
        pub struct MouseMotionArgs {
            /// Mouse device that generated the event.
            pub device_id: DeviceId,

            /// Motion (x, y) delta.
            pub delta: (f64, f64),

            ..

            /// Returns `true` for all widgets.
            fn concerns_widget(&self, _ctx: &mut WidgetContext) -> bool {
                true
            }
        }

        /// Arguments for [`MouseWheelEvent`].
        pub struct MouseWheelArgs {
            /// Mouse device that generated the event.
            pub device_id: DeviceId,

            /// Wheel motion delta, value be in pixels if the *wheel* is a touchpad.
            pub delta: MouseScrollDelta,

            ..

            /// Returns `true` for all widgets.
            fn concerns_widget(&self, _ctx: &mut WidgetContext) -> bool {
                true
            }
        }

        /// Arguments for [`MotionEvent`].
        pub struct MotionArgs {
            /// Device that generated the event.
            pub device_id: DeviceId,

            /// Analog axis.
            pub axis: AxisId,

            /// Motion amount.
            pub value: f64,

            ..

            /// Returns `true` for all widgets.
            fn concerns_widget(&self, _ctx: &mut WidgetContext) -> bool {
                true
            }
        }

        /// Arguments for the [`ButtonEvent`].
        pub struct ButtonArgs {
            /// Device that generated the event.
            pub device_id: DeviceId,

            /// Button raw id.
            pub button: ButtonId,

            /// If the button was pressed or released.
            pub state: ButtonState,

            ..

            /// Returns `true` for all widgets.
            fn concerns_widget(&self, _ctx: &mut WidgetContext) -> bool {
                true
            }
        }

        /// Arguments for the [`KeyEvent`].
        pub struct KeyArgs {
            /// Keyboard device that generated the event.
            pub device_id: DeviceId,

            /// Raw code of key.
            pub scan_code: ScanCode,

            /// If the key was pressed or released.
            pub state: KeyState,

            /// Symbolic name of [`scan_code`](Self::scan_code).
            pub key: Option<Key>,

            ..

            /// Returns `true` for all widgets.
            fn concerns_widget(&self, _ctx: &mut WidgetContext) -> bool {
                true
            }
        }

        /// Arguments for the [`TextEvent`].
        pub struct TextArgs {
            /// Device that generated the event.
            pub device_id: DeviceId,

            /// Character received.
            pub code_point: char,

            ..

            /// Returns `true` for all widgets.
            fn concerns_widget(&self, _ctx: &mut WidgetContext) -> bool {
                true
            }
        }
    }

    event! {
        /// A device event source was added/installed.
        pub DeviceAddedEvent: DeviceArgs;

        /// A device event source was removed/un-installed.
        pub DeviceRemovedEvent: DeviceArgs;

        /// Mouse device unfiltered move delta.
        pub MouseMotionEvent: MouseMotionArgs;

        /// Mouse device unfiltered wheel motion delta.
        pub MouseWheelEvent: MouseWheelArgs;

        /// Motion on some analog axis.
        ///
        /// This event will be reported for all arbitrary input devices that `winit` supports on this platform,
        /// including mouse devices. If the device is a mouse device then this will be reported alongside the [`MouseMotionEvent`].
        pub MotionEvent: MotionArgs;

        /// Button press/release from a device, probably a mouse.
        pub ButtonEvent: ButtonArgs;

        /// Keyboard device key press.
        pub KeyEvent: KeyArgs;

        /// Raw text input.
        pub TextEvent: TextArgs;
    }
}
