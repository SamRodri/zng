#![cfg_attr(doc_nightly, feature(doc_cfg))]

//! View-Process implementation using [`glutin`].
//!
//! This backend supports both headed and headless apps
//!
//! # Usage
//!
//! First add this to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! zero-ui = "0.1"
//! zero-ui-view = "0.1"
//! ```
//!
//! Then call [`init`] before any other code in `main` to setup a view-process that uses
//! the same app executable:
//!
//! ```no_run
//! # pub mod zero_ui { pub mod prelude {
//! # pub struct App { }
//! # impl App {
//! # pub fn default() -> Self { todo!() }
//! # pub fn run_window(self, f: impl FnOnce(bool)) { }
//! # } } }
//! use zero_ui::prelude::*;
//!
//! fn main() {
//!     zero_ui_view::init();
//!
//!     App::default().run_window(|ctx| {
//!         todo!()
//!     })
//! }
//! ```
//!
//! When the app is executed `init` setup its startup and returns, `run_window` gets called and
//! internally starts the view-process, using the `init` setup. The current executable is started
//! again, this time configured to be a view-process, `init` detects this and highjacks the process
//! **never returning**.
//!
//! [`glutin`]: https://docs.rs/glutin/

use std::{
    fmt, process,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use glutin::{
    event::{DeviceEvent, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopProxy, EventLoopWindowTarget},
    monitor::MonitorHandle,
};
use image_cache::ImageCache;
use util::{GlContextManager, WinitToPx};

// /*

/// Doc-only `webrender` re-export.
///
#[cfg(doc)]
#[doc(inline)]
pub use webrender;

// */
mod config;
mod image_cache;
mod surface;
mod util;
mod window;
use surface::*;

use webrender::api::*;
use window::Window;
use zero_ui_view_api::{units::*, *};

/// Runs the view-process server if called in the environment of a view-process.
///
/// If this function is called in a process not configured to be a view-process it will return
/// immediately, with the expectation that the app will be started. If called in a view-process
/// if will highjack the process **never returning**.
///
/// # Examples
///
/// ```no_run
/// # pub mod zero_ui { pub mod prelude {
/// # pub struct App { }
/// # impl App {
/// # pub fn default() -> Self { todo!() }
/// # pub fn run_window(self, f: impl FnOnce(bool)) { }
/// # } } }
/// use zero_ui::prelude::*;
///
/// fn main() {
///     zero_ui_view::init();
///
///     App::default().run_window(|ctx| {
///         todo!()
///     })
/// }
/// ```
///
/// # Panics
///
/// Panics if not called in the main thread, this is a requirement of OpenGL.
///
/// If there was an error connecting with the app-process.
#[cfg(feature = "ipc")]
#[cfg_attr(doc_nightly, doc(cfg(feature = "ipc")))]
pub fn init() {
    if !is_main_thread::is_main_thread().unwrap_or(true) {
        panic!("only call `init` in the main thread, this is a requirement of OpenGL");
    }

    if let Some(config) = ViewConfig::from_env() {
        let c = connect_view_process(config.server_name).expect("failed to connect to app-process");

        if config.headless {
            App::run_headless(c);
        } else {
            App::run_headed(c);
        }
    } else {
        tracing::trace!("init not in view-process");
    }
}

#[cfg(feature = "ipc")]
#[doc(hidden)]
#[no_mangle]
pub extern "C" fn extern_init() {
    std::panic::set_hook(Box::new(ffi_abort));
    init()
}

/// Runs the view-process server in the current process and calls `run_app` to also
/// run the app in the current process. Note that `run_app` will be called in a different thread
/// so it must be [`Send`].
///
/// In this mode the app only uses a single process, reducing the memory footprint, but it is also not
/// resilient to video driver crashes, the view server **does not** respawn in this mode.
///
/// # Examples
///
/// The example demonstrates a setup that runs the view server in the same process in debug builds and
/// runs
///
/// ```no_run
/// # pub mod zero_ui { pub mod prelude {
/// # pub struct App { }
/// # impl App {
/// # pub fn default() -> Self { todo!() }
/// # pub fn run_window(self, f: impl FnOnce(bool)) { }
/// # } } }
/// use zero_ui::prelude::*;
///
/// fn main() {
///     if cfg!(debug_assertions) {
///         zero_ui_view::run_same_process(app_main);
///     } else {
///         zero_ui_view::init();
///         app_main();
///     }
/// }
///
/// fn app_main() {
///     App::default().run_window(|ctx| {
///         todo!()
///     })
/// }
/// ```
///
/// # Panics
///
/// Panics if not called in the main thread, this is a requirement o OpenGL.
pub fn run_same_process(run_app: impl FnOnce() + Send + 'static) -> ! {
    if !is_main_thread::is_main_thread().unwrap_or(true) {
        panic!("only call `run_same_process` in the main thread, this is a requirement of OpenGL");
    }

    thread::Builder::new().name("app".to_owned()).spawn(run_app).unwrap();

    let config = ViewConfig::wait_same_process();

    let c = connect_view_process(config.server_name).expect("failed to connect to app in same process");

    if config.headless {
        App::run_headless(c);
    } else {
        App::run_headed(c);
    }
}

#[cfg(feature = "ipc")]
#[doc(hidden)]
#[no_mangle]
pub extern "C" fn extern_run_same_process(run_app: extern "C" fn()) -> ! {
    std::panic::set_hook(Box::new(ffi_abort));

    #[allow(clippy::redundant_closure)]
    run_same_process(move || run_app())
}

fn ffi_abort(info: &std::panic::PanicInfo) {
    // see `default_hook` in https://doc.rust-lang.org/src/std/panicking.rs.html#182

    let current_thread = std::thread::current();
    let name = current_thread.name().unwrap_or("<unnamed>");

    let location = info.location().unwrap();

    let msg = match info.payload().downcast_ref::<&'static str>() {
        Some(s) => *s,
        None => match info.payload().downcast_ref::<String>() {
            Some(s) => &s[..],
            None => "Box<dyn Any>",
        },
    };

    eprintln!(
        "thread '{}' panicked at '{}', {}\nnote: aborting to avoid unwind across FFI",
        name, msg, location
    );
    std::process::exit(101) // Rust panic exit code.
}

/// The backend implementation.
pub(crate) struct App<S> {
    started: bool,

    headless: bool,

    gl_manager: GlContextManager,
    window_target: *const EventLoopWindowTarget<AppEvent>,
    app_sender: S,
    redirect_enabled: Arc<AtomicBool>,
    redirect_recv: flume::Receiver<Request>,
    response_sender: ResponseSender,
    event_sender: EventSender,
    image_cache: ImageCache<S>,

    gen: ViewProcessGen,
    device_events: bool,

    windows: Vec<Window>,
    surfaces: Vec<Surface>,

    monitor_id_gen: MonitorId,
    pub monitors: Vec<(MonitorId, MonitorHandle)>,

    device_id_gen: DeviceId,
    devices: Vec<(DeviceId, glutin::event::DeviceId)>,

    coalescing_event: Option<Event>,

    exited: bool,
}
impl<S> fmt::Debug for App<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeadlessBackend")
            .field("started", &self.started)
            .field("gen", &self.gen)
            .field("device_events", &self.device_events)
            .field("windows", &self.windows)
            .field("surfaces", &self.surfaces)
            .finish_non_exhaustive()
    }
}
impl App<()> {
    pub fn run_headless(c: ViewChannels) -> ! {
        tracing::info!("running headless view-process");

        let (app_sender, app_receiver) = flume::unbounded();
        let (redirect_sender, redirect_receiver) = flume::unbounded();
        let mut app = App::new(app_sender, c.response_sender, c.event_sender, redirect_receiver);
        app.headless = true;
        let event_loop = EventLoop::<AppEvent>::with_user_event();
        let window_target: &EventLoopWindowTarget<AppEvent> = &event_loop;
        app.window_target = window_target as *const _;

        app.start_receiving(c.request_receiver, redirect_sender);

        while !app.exited {
            match app_receiver.recv() {
                Ok(app_ev) => match app_ev {
                    AppEvent::Request(request) => {
                        let response = app.respond(request);
                        if response.must_be_send() && app.response_sender.send(response).is_err() {
                            app.exited = true;
                            break;
                        }
                    }
                    AppEvent::FrameReady(id, msg) => {
                        let r = if let Some(s) = app.surfaces.iter_mut().find(|s| s.id() == id) {
                            Some(s.on_frame_ready(msg, &mut app.image_cache))
                        } else {
                            None
                        };
                        if let Some((frame_id, image)) = r {
                            app.notify(Event::FrameRendered {
                                window: id,
                                frame: frame_id,
                                frame_image: image,
                                cursor_hits: HitTestResult::default(),
                            });
                        }
                    }
                    AppEvent::Notify(ev) => {
                        if app.event_sender.send(ev).is_err() {
                            app.exited = true;
                            break;
                        }
                    }
                    AppEvent::RefreshMonitors => {
                        panic!("no monitor info in headless mode")
                    }
                    AppEvent::ParentProcessExited => {
                        app.exited = true;
                        break;
                    }
                    AppEvent::ImageLoaded(data) => {
                        app.image_cache.loaded(data);
                    }
                },
                Err(_) => {
                    app.exited = true;
                    break;
                }
            }
        }

        process::exit(0)
    }

    pub fn run_headed(c: ViewChannels) -> ! {
        tracing::info!("running headed view-process");

        let event_loop = EventLoop::with_user_event();
        let app_sender = event_loop.create_proxy();
        let (redirect_sender, redirect_receiver) = flume::unbounded();
        let mut app = App::new(app_sender, c.response_sender, c.event_sender, redirect_receiver);
        app.start_receiving(c.request_receiver, redirect_sender);

        #[cfg(windows)]
        let config_listener = config::config_listener(app.app_sender.clone(), &event_loop);

        struct IdleTrace(Option<tracing::span::EnteredSpan>);
        impl IdleTrace {
            pub fn enter(&mut self) {
                self.0 = Some(tracing::trace_span!("<winit-idle>").entered());
            }
            pub fn exit(&mut self) {
                self.0 = None;
            }
        }
        let mut idle = IdleTrace(None);
        idle.enter();

        event_loop.run(move |event, target, flow| {
            idle.exit();

            app.window_target = target;

            *flow = ControlFlow::Wait;

            if app.exited {
                *flow = ControlFlow::Exit;
            } else {
                use glutin::event::Event as GEvent;
                match event {
                    GEvent::NewEvents(_) => {}
                    GEvent::WindowEvent { window_id, event } => {
                        #[cfg(windows)]
                        if window_id != config_listener.id() {
                            app.on_window_event(window_id, event);
                        }
                        #[cfg(not(windows))]
                        {
                            app.on_window_event(window_id, event);
                        }
                    }
                    GEvent::DeviceEvent { device_id, event } => app.on_device_event(device_id, event),
                    GEvent::UserEvent(ev) => match ev {
                        AppEvent::Request(req) => {
                            let rsp = app.respond(req);
                            if rsp.must_be_send() && app.response_sender.send(rsp).is_err() {
                                // lost connection to app-process
                                app.exited = true;
                                *flow = ControlFlow::Exit;
                            }
                        }
                        AppEvent::Notify(ev) => app.notify(ev),
                        AppEvent::FrameReady(wid, msg) => app.on_frame_ready(wid, msg),
                        AppEvent::RefreshMonitors => app.refresh_monitors(),
                        AppEvent::ParentProcessExited => {
                            app.exited = true;
                            *flow = ControlFlow::Exit;
                        }
                        AppEvent::ImageLoaded(data) => {
                            app.image_cache.loaded(data);
                        }
                    },
                    GEvent::Suspended => {}
                    GEvent::Resumed => {}
                    GEvent::MainEventsCleared => app.flush_coalesced(),
                    GEvent::RedrawRequested(w_id) => app.on_redraw(w_id),
                    GEvent::RedrawEventsCleared => {}
                    GEvent::LoopDestroyed => {}
                }
            }

            app.window_target = std::ptr::null();

            idle.enter();
        })
    }
}
impl<S: AppEventSender> App<S> {
    fn new(app_sender: S, response_sender: ResponseSender, event_sender: EventSender, redirect_recv: flume::Receiver<Request>) -> Self {
        App {
            headless: false,
            started: false,
            gl_manager: GlContextManager::default(),
            image_cache: ImageCache::new(app_sender.clone()),
            app_sender,
            redirect_enabled: Arc::default(),
            redirect_recv,
            response_sender,
            event_sender,
            window_target: std::ptr::null(),
            gen: 0,
            device_events: false,
            windows: vec![],
            surfaces: vec![],
            monitors: vec![],
            monitor_id_gen: 0,
            devices: vec![],
            device_id_gen: 0,
            coalescing_event: None,
            exited: false,
        }
    }

    fn start_receiving(&mut self, mut request_recv: RequestReceiver, redirect_sender: flume::Sender<Request>) {
        let app_sender = self.app_sender.clone();
        let redirect_enabled = self.redirect_enabled.clone();
        thread::spawn(move || {
            while let Ok(r) = request_recv.recv() {
                let disconnected = if redirect_enabled.load(Ordering::Relaxed) {
                    redirect_sender.send(r).is_err()
                } else {
                    app_sender.send(AppEvent::Request(r)).is_err()
                };
                if disconnected {
                    break;
                }
            }
        });
    }

    fn on_window_event(&mut self, window_id: glutin::window::WindowId, event: WindowEvent) {
        let i = if let Some((i, _)) = self.windows.iter_mut().enumerate().find(|(_, w)| w.window_id() == window_id) {
            i
        } else {
            return;
        };

        let _s = tracing::trace_span!("on_window_event", ?event).entered();

        let id = self.windows[i].id();
        let scale_factor = self.windows[i].scale_factor();

        match event {
            WindowEvent::Resized(size) => {
                let size = size.to_px().to_dip(scale_factor);

                if let Some(state) = self.windows[i].state_change() {
                    self.notify(Event::WindowStateChanged {
                        window: id,
                        state,
                        cause: EventCause::System,
                    });
                }

                if !self.windows[i].resized(size) {
                    return;
                }
                // give the app 300ms to send a new frame, this is the collaborative way to
                // resize, it should reduce the changes of the user seeing the clear color.

                let redirect_enabled = self.redirect_enabled.clone();
                redirect_enabled.store(true, Ordering::Relaxed);
                let stop_redirect = util::RunOnDrop::new(|| redirect_enabled.store(false, Ordering::Relaxed));

                self.notify(Event::WindowResized {
                    window: id,
                    size,
                    cause: EventCause::System,
                });
                self.flush_coalesced();

                let deadline = Instant::now() + Duration::from_millis(300);
                if self.windows[i].is_rendering_frame() {
                    if let Some((frame_id, image, cursor_hits)) = self.windows[i].wait_frame_ready(deadline, &mut self.image_cache) {
                        let id = self.windows[i].id();

                        self.notify(Event::FrameRendered {
                            window: id,
                            frame: frame_id,
                            frame_image: image,
                            cursor_hits,
                        });
                        self.flush_coalesced();
                    }
                }

                let mut received_frame = false;
                loop {
                    match self.redirect_recv.recv_deadline(deadline) {
                        Ok(req) => {
                            received_frame = req.is_frame(id);
                            if received_frame || req.is_move_or_resize(id) {
                                // received new frame
                                drop(stop_redirect);
                                self.windows[i].on_resized();
                                let rsp = self.respond(req);
                                if rsp.must_be_send() {
                                    let _ = self.response_sender.send(rsp);
                                }
                                break;
                            } else {
                                let rsp = self.respond(req);
                                if rsp.must_be_send() {
                                    let _ = self.response_sender.send(rsp);
                                }
                            }
                        }

                        Err(flume::RecvTimeoutError::Timeout) => {
                            drop(stop_redirect);
                            self.windows[i].on_resized();
                            break;
                        }
                        Err(flume::RecvTimeoutError::Disconnected) => {
                            unreachable!()
                        }
                    }
                }

                let drained: Vec<_> = self.redirect_recv.drain().collect();
                for req in drained {
                    let _ = self.app_sender.send(AppEvent::Request(req));
                }

                // if we are still within 1 second, wait webrender, and if a frame was rendered here, notify.
                if received_frame && deadline > Instant::now() {
                    if let Some((frame_id, image, cursor_hits)) = self.windows[i].wait_frame_ready(deadline, &mut self.image_cache) {
                        let id = self.windows[i].id();

                        self.notify(Event::FrameRendered {
                            window: id,
                            frame: frame_id,
                            frame_image: image,
                            cursor_hits,
                        });
                    }
                }
            }
            WindowEvent::Moved(p) => {
                let p = p.to_px().to_dip(scale_factor);

                if !self.windows[i].moved(p) {
                    return;
                }

                self.notify(Event::WindowMoved {
                    window: id,
                    position: p,
                    cause: EventCause::System,
                });
            }
            WindowEvent::CloseRequested => self.notify(Event::WindowCloseRequested(id)),
            WindowEvent::Destroyed => {
                self.windows.remove(i);
                self.notify(Event::WindowClosed(id));
            }
            WindowEvent::DroppedFile(file) => self.notify(Event::DroppedFile { window: id, file }),
            WindowEvent::HoveredFile(file) => self.notify(Event::HoveredFile { window: id, file }),
            WindowEvent::HoveredFileCancelled => self.notify(Event::HoveredFileCancelled(id)),
            WindowEvent::ReceivedCharacter(c) => self.notify(Event::ReceivedCharacter(id, c)),
            WindowEvent::Focused(focused) => self.notify(Event::Focused { window: id, focused }),
            WindowEvent::KeyboardInput { device_id, input, .. } => {
                let d_id = self.device_id(device_id);
                self.notify(Event::KeyboardInput {
                    window: id,
                    device: d_id,
                    scan_code: input.scancode,
                    state: util::element_state_to_key_state(input.state),
                    key: input.virtual_keycode.map(util::v_key_to_key),
                });
            }
            WindowEvent::ModifiersChanged(m) => {
                self.refresh_monitors();
                self.notify(Event::ModifiersChanged {
                    window: id,
                    state: util::winit_modifiers_state_to_zui(m),
                });
            }
            WindowEvent::CursorMoved { device_id, position, .. } => {
                let px_p = position.to_px();
                let p = px_p.to_dip(scale_factor);
                let d_id = self.device_id(device_id);

                self.windows[i].set_cursor_pos(px_p);

                self.notify(Event::CursorMoved {
                    window: id,
                    device: d_id,
                    coalesced_pos: vec![],
                    position: p,
                });
            }
            WindowEvent::CursorEntered { device_id } => {
                let d_id = self.device_id(device_id);
                self.notify(Event::CursorEntered { window: id, device: d_id });
            }
            WindowEvent::CursorLeft { device_id } => {
                let d_id = self.device_id(device_id);
                self.notify(Event::CursorLeft { window: id, device: d_id });
            }
            WindowEvent::MouseWheel {
                device_id, delta, phase, ..
            } => {
                let d_id = self.device_id(device_id);
                self.notify(Event::MouseWheel {
                    window: id,
                    device: d_id,
                    delta: util::winit_mouse_wheel_delta_to_zui(delta),
                    phase: util::winit_touch_phase_to_zui(phase),
                });
            }
            WindowEvent::MouseInput {
                device_id, state, button, ..
            } => {
                let d_id = self.device_id(device_id);
                self.notify(Event::MouseInput {
                    window: id,
                    device: d_id,
                    state: util::element_state_to_button_state(state),
                    button: util::winit_mouse_button_to_zui(button),
                });
            }
            WindowEvent::TouchpadPressure {
                device_id,
                pressure,
                stage,
            } => {
                let d_id = self.device_id(device_id);
                self.notify(Event::TouchpadPressure {
                    window: id,
                    device: d_id,
                    pressure,
                    stage,
                });
            }
            WindowEvent::AxisMotion { device_id, axis, value } => {
                let d_id = self.device_id(device_id);
                self.notify(Event::AxisMotion(id, d_id, axis, value));
            }
            WindowEvent::Touch(t) => {
                let d_id = self.device_id(t.device_id);
                let location = t.location.to_px().to_dip(scale_factor);
                self.notify(Event::Touch(
                    id,
                    d_id,
                    util::winit_touch_phase_to_zui(t.phase),
                    location,
                    t.force.map(util::winit_force_to_zui),
                    t.id,
                ));
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => self.notify(Event::ScaleFactorChanged {
                window: id,
                scale_factor: scale_factor as f32,
            }),
            WindowEvent::ThemeChanged(t) => self.notify(Event::WindowThemeChanged(id, util::winit_theme_to_zui(t))),
        }
    }

    fn refresh_monitors(&mut self) {
        let mut monitors = Vec::with_capacity(self.monitors.len());

        let mut added_check = false; // set to `true` if a new id is generated.
        let mut removed_check = self.monitors.len(); // `-=1` every existing reused `id`.

        let window_target = unsafe { &*self.window_target };

        for handle in window_target.available_monitors() {
            let id = self
                .monitors
                .iter()
                .find_map(|(id, h)| {
                    if h == &handle {
                        removed_check = removed_check.checked_sub(1).unwrap();
                        Some(*id)
                    } else {
                        added_check = true;
                        None
                    }
                })
                .unwrap_or_else(|| {
                    let mut id = self.monitor_id_gen.wrapping_add(1);
                    if id == 0 {
                        id += 1;
                    }
                    self.monitor_id_gen = id;
                    id
                });
            monitors.push((id, handle))
        }

        if added_check || removed_check > 1 {
            self.monitors = monitors;

            let monitors = self.available_monitors();
            self.notify(Event::MonitorsChanged(monitors));
        }
    }

    fn on_frame_ready(&mut self, window_id: WindowId, msg: FrameReadyMsg) {
        let _s = tracing::trace_span!("on_frame_ready").entered();

        if let Some(w) = self.windows.iter_mut().find(|w| w.id() == window_id) {
            let ((frame_id, image, cursor_hits), first) = w.on_frame_ready(msg, &mut self.image_cache);

            let _ = self.event_sender.send(Event::FrameRendered {
                window: window_id,
                frame: frame_id,
                frame_image: image,
                cursor_hits,
            });

            if first {
                let size = w.size();

                // Windows not notifying this one.
                #[cfg(windows)]
                if w.is_maximized() && w.is_active_window() {
                    self.notify(Event::Focused {
                        window: window_id,
                        focused: true,
                    });
                }

                self.notify(Event::WindowResized {
                    window: window_id,
                    size,
                    cause: EventCause::App,
                });
            }
        } else if let Some(s) = self.surfaces.iter_mut().find(|w| w.id() == window_id) {
            let (frame_id, image) = s.on_frame_ready(msg, &mut self.image_cache);

            self.notify(Event::FrameRendered {
                window: window_id,
                frame: frame_id,
                frame_image: image,
                cursor_hits: HitTestResult::default(),
            })
        }
    }

    pub(crate) fn notify(&mut self, event: Event) {
        if let Some(mut coal) = self.coalescing_event.take() {
            match coal.coalesce(event) {
                Ok(()) => self.coalescing_event = Some(coal),
                Err(event) => {
                    let mut error = self.event_sender.send(coal).is_err();
                    error |= self.event_sender.send(event).is_err();

                    if error {
                        let _ = self.app_sender.send(AppEvent::ParentProcessExited);
                    }
                }
            }
        } else {
            self.coalescing_event = Some(event);
        }

        if self.headless {
            self.flush_coalesced();
        }
    }

    /// Send pending coalesced events.
    pub(crate) fn flush_coalesced(&mut self) {
        if let Some(coal) = self.coalescing_event.take() {
            if self.event_sender.send(coal).is_err() {
                let _ = self.app_sender.send(AppEvent::ParentProcessExited);
            }
        }
    }

    fn on_device_event(&mut self, device_id: glutin::event::DeviceId, event: DeviceEvent) {
        if self.device_events {
            let _s = tracing::trace_span!("on_device_event", ?event);

            let d_id = self.device_id(device_id);
            match event {
                DeviceEvent::Added => self.notify(Event::DeviceAdded(d_id)),
                DeviceEvent::Removed => self.notify(Event::DeviceRemoved(d_id)),
                DeviceEvent::MouseMotion { delta } => self.notify(Event::DeviceMouseMotion {
                    device: d_id,
                    delta: euclid::vec2(delta.0, delta.1),
                }),
                DeviceEvent::MouseWheel { delta } => self.notify(Event::DeviceMouseWheel {
                    device: d_id,
                    delta: util::winit_mouse_wheel_delta_to_zui(delta),
                }),
                DeviceEvent::Motion { axis, value } => self.notify(Event::DeviceMotion { device: d_id, axis, value }),
                DeviceEvent::Button { button, state } => self.notify(Event::DeviceButton {
                    device: d_id,
                    button,
                    state: util::element_state_to_button_state(state),
                }),
                DeviceEvent::Key(k) => self.notify(Event::DeviceKey {
                    device: d_id,
                    scan_code: k.scancode,
                    state: util::element_state_to_key_state(k.state),
                    key: k.virtual_keycode.map(util::v_key_to_key),
                }),
                DeviceEvent::Text { codepoint } => self.notify(Event::DeviceText(d_id, codepoint)),
            }
        }
    }

    fn on_redraw(&mut self, window_id: glutin::window::WindowId) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.window_id() == window_id) {
            w.redraw();
        }
    }

    fn assert_started(&self) {
        if !self.started {
            panic!("not started")
        }
    }

    fn with_window<R>(&mut self, id: WindowId, action: impl FnOnce(&mut Window) -> R, not_found: impl FnOnce() -> R) -> R {
        self.assert_started();
        self.windows.iter_mut().find(|w| w.id() == id).map(action).unwrap_or_else(|| {
            tracing::error!("headed window `{}` not found, will return fallback result", id);
            not_found()
        })
    }

    fn monitor_id(&mut self, handle: &MonitorHandle) -> MonitorId {
        if let Some((id, _)) = self.monitors.iter().find(|(_, h)| h == handle) {
            *id
        } else {
            let mut id = self.monitor_id_gen.wrapping_add(1);
            if id == 0 {
                id = 1;
            }
            self.monitor_id_gen = id;
            self.monitors.push((id, handle.clone()));
            id
        }
    }

    fn device_id(&mut self, device_id: glutin::event::DeviceId) -> DeviceId {
        if let Some((id, _)) = self.devices.iter().find(|(_, id)| *id == device_id) {
            *id
        } else {
            let mut id = self.device_id_gen.wrapping_add(1);
            if id == 0 {
                id = 1;
            }
            self.device_id_gen = id;
            self.devices.push((id, device_id));
            id
        }
    }
}
macro_rules! with_window_or_surface {
    ($self:ident, $id:ident, |$el:ident|$action:expr, ||$fallback:expr) => {
        if let Some($el) = $self.windows.iter_mut().find(|w| w.id() == $id) {
            $action
        } else if let Some($el) = $self.surfaces.iter_mut().find(|w| w.id() == $id) {
            $action
        } else {
            tracing::error!("window `{}` not found, will return fallback result", $id);
            $fallback
        }
    };
}

impl<S: AppEventSender> Api for App<S> {
    fn api_version(&mut self) -> String {
        VERSION.to_owned()
    }

    fn startup(&mut self, gen: ViewProcessGen, device_events: bool, headless: bool) {
        if self.started {
            panic!("already started");
        }
        if self.exited {
            panic!("cannot restart exited");
        }
        self.started = true;
        self.gen = gen;
        self.device_events = device_events;
        self.headless = headless;

        #[cfg(windows)]
        if !self.device_events {
            util::unregister_raw_input();
        }
    }

    fn exit(&mut self) {
        self.assert_started();
        self.started = false;
        self.exited = true;
    }

    fn primary_monitor(&mut self) -> Option<(MonitorId, MonitorInfo)> {
        self.assert_started();

        let window_target = unsafe { &*self.window_target };

        window_target
            .primary_monitor()
            .or_else(|| window_target.available_monitors().next())
            .map(|m| {
                let id = self.monitor_id(&m);
                let mut info = util::monitor_handle_to_info(&m);
                info.is_primary = true;
                (id, info)
            })
    }

    fn monitor_info(&mut self, id: MonitorId) -> Option<MonitorInfo> {
        self.assert_started();

        let window_target = unsafe { &*self.window_target };

        self.monitors.iter().find(|(i, _)| *i == id).map(|(_, h)| {
            let mut info = util::monitor_handle_to_info(h);
            info.is_primary = window_target.primary_monitor().map(|p| &p == h).unwrap_or(false);
            info
        })
    }

    fn available_monitors(&mut self) -> Vec<(MonitorId, MonitorInfo)> {
        self.assert_started();

        let window_target = unsafe { &*self.window_target };

        let primary = window_target.primary_monitor();

        window_target
            .available_monitors()
            .map(|m| {
                let id = self.monitor_id(&m);
                let is_primary = primary.as_ref().map(|h| h == &m).unwrap_or(false);
                let mut info = util::monitor_handle_to_info(&m);
                info.is_primary = is_primary;
                (id, info)
            })
            .collect()
    }

    fn open_window(&mut self, config: WindowRequest) -> WindowOpenData {
        let _s = tracing::debug_span!("open_window", ?config).entered();

        if self.headless {
            let data = self.open_headless(HeadlessRequest {
                id: config.id,
                scale_factor: 1.0,
                size: config.size,
                text_aa: config.text_aa,
            });
            WindowOpenData {
                id_namespace: data.id_namespace,
                pipeline_id: data.pipeline_id,
                document_id: data.document_id,
                position: DipPoint::zero(),
                size: config.size,
                scale_factor: 1.0,
            }
        } else {
            self.assert_started();

            let win = Window::open(
                self.gen,
                config.icon.and_then(|i| self.image_cache.get(i)).and_then(|i| i.icon()),
                config,
                unsafe { &*self.window_target },
                &mut self.gl_manager,
                self.app_sender.clone(),
            );

            let data = WindowOpenData {
                id_namespace: win.id_namespace(),
                pipeline_id: win.pipeline_id(),
                document_id: win.document_id(),
                position: win.outer_position(),
                size: win.size(),
                scale_factor: win.scale_factor(),
            };

            self.windows.push(win);

            data
        }
    }

    fn open_headless(&mut self, config: HeadlessRequest) -> HeadlessOpenData {
        let _s = tracing::debug_span!("open_headless", ?config).entered();

        self.assert_started();
        let surf = Surface::open(
            self.gen,
            config,
            unsafe { &*self.window_target },
            &mut self.gl_manager,
            self.app_sender.clone(),
        );
        let id_namespace = surf.id_namespace();
        let pipeline_id = surf.pipeline_id();
        let document_id = surf.document_id();

        self.surfaces.push(surf);

        HeadlessOpenData {
            id_namespace,
            pipeline_id,
            document_id,
        }
    }

    fn open_document(&mut self, request: DocumentRequest) -> HeadlessOpenData {
        self.assert_started();
        if let Some(surf) = self.surfaces.iter_mut().find(|s| s.id() == request.renderer) {
            surf.open_document(request.scale_factor, request.size)
        } else if let Some(win) = self.windows.iter_mut().find(|s| s.id() == request.renderer) {
            win.open_document(request.scale_factor, request.size)
        } else {
            HeadlessOpenData::invalid()
        }
    }

    fn close_window(&mut self, id: WindowId) {
        let _s = tracing::debug_span!("close_window", ?id);

        self.assert_started();
        if let Some(i) = self.windows.iter().position(|w| w.id() == id) {
            let _ = self.windows.swap_remove(i);
        }
        if let Some(i) = self.surfaces.iter().position(|w| w.id() == id) {
            let _ = self.surfaces.swap_remove(i);
        }
    }

    fn close_document(&mut self, renderer: WindowId, document_id: DocumentId) {
        self.assert_started();
        if let Some(surf) = self.surfaces.iter_mut().find(|w| w.id() == renderer) {
            surf.close_document(document_id);
        }
        if let Some(win) = self.windows.iter_mut().find(|w| w.id() == renderer) {
            win.close_document(document_id);
        }
    }

    fn text_aa(&mut self) -> TextAntiAliasing {
        self.assert_started();
        config::text_aa()
    }

    fn multi_click_config(&mut self) -> MultiClickConfig {
        self.assert_started();
        config::multi_click_config()
    }

    fn animation_enabled(&mut self) -> bool {
        self.assert_started();
        config::animation_enabled()
    }

    fn key_repeat_delay(&mut self) -> Duration {
        self.assert_started();
        config::key_repeat_delay()
    }

    fn set_title(&mut self, id: WindowId, title: String) {
        self.with_window(id, |w| w.set_title(title), || ())
    }

    fn set_visible(&mut self, id: WindowId, visible: bool) {
        self.with_window(id, |w| w.set_visible(visible), || ())
    }

    fn set_always_on_top(&mut self, id: WindowId, always_on_top: bool) {
        self.with_window(id, |w| w.set_always_on_top(always_on_top), || ())
    }

    fn set_movable(&mut self, id: WindowId, movable: bool) {
        self.with_window(id, |w| w.set_movable(movable), || ())
    }

    fn set_resizable(&mut self, id: WindowId, resizable: bool) {
        self.with_window(id, |w| w.set_resizable(resizable), || ())
    }

    fn set_taskbar_visible(&mut self, id: WindowId, visible: bool) {
        self.with_window(id, |w| w.set_taskbar_visible(visible), || ())
    }

    fn set_parent(&mut self, id: WindowId, parent: Option<WindowId>, modal: bool) {
        let parent = parent.and_then(|id| self.windows.iter().find(|w| w.id() == id).map(|w| w.window_id()));
        self.with_window(id, |w| w.set_parent(parent, modal), || ())
    }

    fn set_chrome_visible(&mut self, id: WindowId, visible: bool) {
        self.with_window(id, |w| w.set_chrome_visible(visible), || ())
    }

    fn set_position(&mut self, id: WindowId, pos: DipPoint) {
        if self.with_window(id, |w| w.set_outer_pos(pos), || false) {
            let _ = self.app_sender.send(AppEvent::Notify(Event::WindowMoved {
                window: id,
                position: pos,
                cause: EventCause::App,
            }));
        }
    }

    fn set_size(&mut self, id: WindowId, size: DipSize) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id() == id) {
            if w.set_inner_size(size) {
                let _ = self.app_sender.send(AppEvent::Notify(Event::WindowResized {
                    window: id,
                    size,
                    cause: EventCause::App,
                }));
            }
        } else {
            tracing::error!("headed window `{}` not found, will return fallback result", id);
        }
    }

    fn set_state(&mut self, id: WindowId, state: WindowState) {
        if self.with_window(id, |w| w.set_state(state), || false) {
            let _ = self.app_sender.send(AppEvent::Notify(Event::WindowStateChanged {
                window: id,
                state,
                cause: EventCause::App,
            }));
        }
    }

    fn set_headless_size(&mut self, renderer: WindowId, document_id: DocumentId, size: DipSize, scale_factor: f32) {
        self.assert_started();
        if let Some(surf) = self.surfaces.iter_mut().find(|s| s.id() == renderer) {
            surf.set_size(document_id, size, scale_factor)
        } else if let Some(win) = self.windows.iter_mut().find(|s| s.id() == renderer) {
            win.set_document_size(document_id, size, scale_factor)
        }
    }

    fn set_video_mode(&mut self, id: WindowId, mode: VideoMode) {
        self.with_window(id, |w| w.set_video_mode(mode), || ())
    }

    fn set_min_size(&mut self, id: WindowId, size: DipSize) {
        self.with_window(id, |w| w.set_min_inner_size(size), || ())
    }

    fn set_max_size(&mut self, id: WindowId, size: DipSize) {
        self.with_window(id, |w| w.set_max_inner_size(size), || ())
    }

    fn set_icon(&mut self, id: WindowId, icon: Option<ImageId>) {
        let icon = icon.and_then(|i| self.image_cache.get(i)).and_then(|i| i.icon());
        self.with_window(id, |w| w.set_icon(icon), || ())
    }

    fn pipeline_id(&mut self, id: WindowId) -> PipelineId {
        with_window_or_surface!(self, id, |w| w.pipeline_id(), || PipelineId::dummy())
    }

    fn id_namspace(&mut self, id: WindowId) -> IdNamespace {
        with_window_or_surface!(self, id, |w| w.id_namespace(), || IdNamespace(0))
    }

    fn image_decoders(&mut self) -> Vec<String> {
        image_cache::DECODERS.iter().map(|&s| s.to_owned()).collect()
    }

    fn image_encoders(&mut self) -> Vec<String> {
        image_cache::ENCODERS.iter().map(|&s| s.to_owned()).collect()
    }

    fn add_image(&mut self, format: ImageDataFormat, data: IpcBytes, max_decoded_size: u64) -> ImageId {
        self.image_cache.add(format, data, max_decoded_size)
    }

    fn add_image_pro(&mut self, format: ImageDataFormat, data: IpcBytesReceiver, max_decoded_size: u64) -> ImageId {
        self.image_cache.add_pro(format, data, max_decoded_size)
    }

    fn forget_image(&mut self, id: ImageId) {
        self.image_cache.forget(id)
    }

    fn encode_image(&mut self, id: ImageId, format: String) {
        self.image_cache.encode(id, format)
    }

    fn use_image(&mut self, id: WindowId, image_id: ImageId) -> ImageKey {
        if let Some(img) = self.image_cache.get(image_id) {
            with_window_or_surface!(self, id, |w| w.use_image(img), || ImageKey::DUMMY)
        } else {
            ImageKey::DUMMY
        }
    }

    fn update_image_use(&mut self, id: WindowId, key: ImageKey, image_id: ImageId) {
        if let Some(img) = self.image_cache.get(image_id) {
            with_window_or_surface!(self, id, |w| w.update_image(key, img), || ())
        }
    }

    fn delete_image_use(&mut self, id: WindowId, key: ImageKey) {
        with_window_or_surface!(self, id, |w| w.delete_image(key), || ())
    }

    fn add_font(&mut self, id: WindowId, bytes: IpcBytes, index: u32) -> FontKey {
        with_window_or_surface!(self, id, |w| w.add_font(bytes.to_vec(), index), || FontKey(IdNamespace(0), 0))
    }

    fn delete_font(&mut self, id: WindowId, key: FontKey) {
        with_window_or_surface!(self, id, |w| w.delete_font(key), || ())
    }

    fn add_font_instance(
        &mut self,
        id: WindowId,
        font_key: FontKey,
        glyph_size: Px,
        options: Option<FontInstanceOptions>,
        plataform_options: Option<FontInstancePlatformOptions>,
        variations: Vec<FontVariation>,
    ) -> FontInstanceKey {
        with_window_or_surface!(
            self,
            id,
            |w| w.add_font_instance(font_key, glyph_size, options, plataform_options, variations),
            || FontInstanceKey(IdNamespace(0), 0)
        )
    }

    fn delete_font_instance(&mut self, id: WindowId, instance_key: FontInstanceKey) {
        with_window_or_surface!(self, id, |w| w.delete_font_instance(instance_key), || ())
    }

    fn size(&mut self, id: WindowId) -> DipSize {
        with_window_or_surface!(self, id, |w| w.size(), || DipSize::zero())
    }

    fn scale_factor(&mut self, id: WindowId) -> f32 {
        with_window_or_surface!(self, id, |w| w.scale_factor(), || 1.0)
    }

    fn set_allow_alt_f4(&mut self, id: WindowId, allow: bool) {
        self.with_window(id, |w| w.set_allow_alt_f4(allow), || ())
    }

    fn set_capture_mode(&mut self, id: WindowId, enabled: bool) {
        self.with_window(id, |w| w.set_capture_mode(enabled), || ())
    }

    fn frame_image(&mut self, id: WindowId) -> ImageId {
        with_window_or_surface!(self, id, |w| w.frame_image(&mut self.image_cache), || 0)
    }

    fn frame_image_rect(&mut self, id: WindowId, rect: PxRect) -> ImageId {
        with_window_or_surface!(self, id, |w| w.frame_image_rect(&mut self.image_cache, rect), || 0)
    }

    fn hit_test(&mut self, id: WindowId, point: DipPoint) -> (FrameId, HitTestResult) {
        with_window_or_surface!(self, id, |w| w.hit_test(point), || (FrameId::INVALID, HitTestResult::default()))
    }

    fn set_text_aa(&mut self, id: WindowId, aa: TextAntiAliasing) {
        with_window_or_surface!(self, id, |w| w.set_text_aa(aa), || ())
    }

    fn render(&mut self, id: WindowId, frame: FrameRequest) {
        with_window_or_surface!(self, id, |w| w.render(frame), || ())
    }

    fn render_update(&mut self, id: WindowId, frame: FrameUpdateRequest) {
        with_window_or_surface!(self, id, |w| w.render_update(frame), || ())
    }

    #[cfg(debug_assertions)]
    fn crash(&mut self) {
        panic!("CRASH")
    }
}

/// Message inserted in the event loop from the view-process.
#[derive(Debug)]
pub(crate) enum AppEvent {
    /// A request.
    Request(Request),
    /// Notify an event.
    Notify(Event),
    /// A frame is ready for redraw.
    FrameReady(WindowId, FrameReadyMsg),
    /// Re-query available monitors and send update event.
    RefreshMonitors,
    /// Lost connection with app-process.
    ParentProcessExited,

    /// Image finished decoding, must call [`ImageCache::loaded`].
    ImageLoaded(ImageLoadedData),
}

#[derive(Debug)]
pub(crate) struct FrameReadyMsg {
    pub document_id: DocumentId,
    pub composite_needed: bool,
    // pub scrolled: bool,
}

/// Abstraction over channel senders  that can inject [`AppEvent`] in the app loop.
pub(crate) trait AppEventSender: Clone + Send + 'static {
    /// Send an event.
    fn send(&self, ev: AppEvent) -> Result<(), Disconnected>;
}
/// headless
impl AppEventSender for flume::Sender<AppEvent> {
    fn send(&self, ev: AppEvent) -> Result<(), Disconnected> {
        self.send(ev).map_err(|_| Disconnected)
    }
}
/// headed
impl AppEventSender for EventLoopProxy<AppEvent> {
    fn send(&self, ev: AppEvent) -> Result<(), Disconnected> {
        self.send_event(ev).map_err(|_| Disconnected)
    }
}
