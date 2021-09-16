#[cfg(feature = "full")]
use std::{cell::Cell, rc::Rc};

#[cfg(feature = "full")]
use crate::units::*;
#[cfg(feature = "full")]
use crate::{FramePixels, WinId};
#[cfg(feature = "full")]
use gleam::gl;
#[cfg(feature = "full")]
use glutin::{ContextWrapper, NotCurrent, PossiblyCurrent};
#[cfg(feature = "full")]
use serde_bytes::ByteBuf;

pub type AnyResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Managed headed Open-GL context.
#[cfg(feature = "full")]
pub(crate) struct GlContext {
    id: WinId,
    ctx: Option<ContextWrapper<PossiblyCurrent, ()>>,
    current: Rc<Cell<Option<WinId>>>,
}
#[cfg(feature = "full")]
impl GlContext {
    /// Gets the context as current.
    ///
    /// It can already be current or it is made current.
    pub fn make_current(&mut self) -> &mut ContextWrapper<PossiblyCurrent, ()> {
        let id = Some(self.id);
        if self.current.get() != id {
            self.current.set(id);
            let c = self.ctx.take().unwrap();
            // glutin docs says that calling `make_not_current` is not necessary and
            // that "If you call make_current on some context, you should call treat_as_not_current as soon
            // as possible on the previously current context."
            //
            // As far as the glutin code goes `treat_as_not_current` just changes the state tag, so we can call
            // `treat_as_not_current` just to get access to the `make_current` when we know it is not current
            // anymore, and just ignore the whole "current state tag" thing.
            let c = unsafe { c.treat_as_not_current().make_current() }.expect("failed to make current");
            self.ctx = Some(c);
        }
        self.ctx.as_mut().unwrap()
    }

    /// Glutin requires that the context is [dropped before the window][1], calling this
    /// function safely disposes of the context, the winit window should be dropped immediately after.
    ///
    /// [1]: https://docs.rs/glutin/0.27.0/glutin/type.WindowedContext.html#method.split
    pub fn drop_before_winit(&mut self) {
        if self.current.get() == Some(self.id) {
            let _ = unsafe { self.ctx.take().unwrap().make_not_current() };
            self.current.set(None);
        } else {
            let _ = unsafe { self.ctx.take().unwrap().treat_as_not_current() };
        }
    }
}
#[cfg(feature = "full")]
impl Drop for GlContext {
    fn drop(&mut self) {
        if self.ctx.is_some() {
            panic!("call `drop_before_winit` before dropping")
        }
    }
}

/// Managed headless Open-GL context.
#[cfg(feature = "full")]
pub(crate) struct GlHeadlessContext {
    id: WinId,
    ctx: Option<glutin::Context<PossiblyCurrent>>,
    current: Rc<Cell<Option<WinId>>>,
}
#[cfg(feature = "full")]
impl GlHeadlessContext {
    /// Gets the context as current.
    ///
    /// It can already be current or it is made current.
    pub fn make_current(&mut self) -> &mut glutin::Context<PossiblyCurrent> {
        let id = Some(self.id);
        if self.current.get() != id {
            self.current.set(id);
            let c = self.ctx.take().unwrap();
            // glutin docs says that calling `make_not_current` is not necessary and
            // that "If you call make_current on some context, you should call treat_as_not_current as soon
            // as possible on the previously current context."
            //
            // As far as the glutin code goes `treat_as_not_current` just changes the state tag, so we can call
            // `treat_as_not_current` just to get access to the `make_current` when we know it is not current
            // anymore, and just ignore the whole "current state tag" thing.
            let c = unsafe { c.treat_as_not_current().make_current() }.expect("failed to make current");
            self.ctx = Some(c);
        }
        self.ctx.as_mut().unwrap()
    }
}
#[cfg(feature = "full")]
impl Drop for GlHeadlessContext {
    fn drop(&mut self) {
        if self.current.get() == Some(self.id) {
            let _ = unsafe { self.ctx.take().unwrap().make_not_current() };
            self.current.set(None);
        } else {
            let _ = unsafe { self.ctx.take().unwrap().treat_as_not_current() };
        }
    }
}

/// Manages the "current" glutin OpenGL context.
#[derive(Default)]
#[cfg(feature = "full")]
pub(crate) struct GlContextManager {
    current: Rc<Cell<Option<WinId>>>,
}
#[cfg(feature = "full")]
impl GlContextManager {
    pub fn manage_headed(&self, id: WinId, ctx: glutin::RawContext<NotCurrent>) -> GlContext {
        GlContext {
            id,
            ctx: Some(unsafe { ctx.treat_as_current() }),
            current: Rc::clone(&self.current),
        }
    }

    pub fn manage_headless(&self, id: WinId, ctx: glutin::Context<NotCurrent>) -> GlHeadlessContext {
        GlHeadlessContext {
            id,
            ctx: Some(unsafe { ctx.treat_as_current() }),
            current: Rc::clone(&self.current),
        }
    }
}

#[cfg(feature = "full")]
/// Read a selection of pixels of the current frame.
///
/// This is a call to `glReadPixels`, the pixel row order is bottom-to-top and the pixel type is BGRA.
pub(crate) fn read_pixels_rect(gl: &Rc<dyn gl::Gl>, max_size: PxSize, rect: PxRect, scale_factor: f32) -> FramePixels {
    let max = PxRect::from_size(max_size);
    let rect = rect.intersection(&max).unwrap_or_default();

    if rect.size.width <= Px(0) || rect.size.height <= Px(0) {
        return FramePixels {
            width: Px(0),
            height: Px(0),
            bgra: ByteBuf::new(),
            scale_factor,
            opaque: true,
        };
    }

    let x = rect.origin.x.0;
    let inverted_y = (max.size.height - rect.origin.y - rect.size.height).0;
    let width = rect.size.width.0 as u32;
    let height = rect.size.height.0 as u32;

    let bgra = gl.read_pixels(x as _, inverted_y as _, width as _, height as _, gl::BGRA, gl::UNSIGNED_BYTE);
    assert_eq!(gl.get_error(), 0);

    FramePixels {
        width: rect.size.width,
        height: rect.size.height,
        bgra: ByteBuf::from(bgra),
        scale_factor,
        opaque: true,
    }
}

/// Sets a window subclass that calls a raw event handler.
///
/// Use this to receive Windows OS events not covered in [`raw_events`].
///
/// Returns if adding a subclass handler succeeded.
///
/// # Handler
///
/// The handler inputs are the first 4 arguments of a [`SUBCLASSPROC`].
/// You can use closure capture to include extra data.
///
/// The handler must return `Some(LRESULT)` to stop the propagation of a specific message.
///
/// The handler is dropped after it receives the `WM_DESTROY` message.
///
/// # Panics
///
/// Panics in headless mode.
///
/// [`raw_events`]: crate::app::raw_events
/// [`SUBCLASSPROC`]: https://docs.microsoft.com/en-us/windows/win32/api/commctrl/nc-commctrl-subclassproc
#[cfg(all(windows, feature = "full"))]
pub fn set_raw_windows_event_handler<
    H: FnMut(
            winapi::shared::windef::HWND,
            winapi::shared::minwindef::UINT,
            winapi::shared::minwindef::WPARAM,
            winapi::shared::minwindef::LPARAM,
        ) -> Option<winapi::shared::minwindef::LRESULT>
        + 'static,
>(
    window: &glutin::window::Window,
    subclass_id: winapi::shared::basetsd::UINT_PTR,
    handler: H,
) -> bool {
    use glutin::platform::windows::WindowExtWindows;

    let hwnd = window.hwnd() as winapi::shared::windef::HWND;
    let data = Box::new(handler);
    unsafe {
        winapi::um::commctrl::SetWindowSubclass(
            hwnd,
            Some(subclass_raw_event_proc::<H>),
            subclass_id,
            Box::into_raw(data) as winapi::shared::basetsd::DWORD_PTR,
        ) != 0
    }
}
#[cfg(all(windows, feature = "full"))]
unsafe extern "system" fn subclass_raw_event_proc<
    H: FnMut(
            winapi::shared::windef::HWND,
            winapi::shared::minwindef::UINT,
            winapi::shared::minwindef::WPARAM,
            winapi::shared::minwindef::LPARAM,
        ) -> Option<winapi::shared::minwindef::LRESULT>
        + 'static,
>(
    hwnd: winapi::shared::windef::HWND,
    msg: winapi::shared::minwindef::UINT,
    wparam: winapi::shared::minwindef::WPARAM,
    lparam: winapi::shared::minwindef::LPARAM,
    _id: winapi::shared::basetsd::UINT_PTR,
    data: winapi::shared::basetsd::DWORD_PTR,
) -> winapi::shared::minwindef::LRESULT {
    use winapi::um::winuser::WM_DESTROY;
    match msg {
        WM_DESTROY => {
            // last call and cleanup.
            let mut handler = Box::from_raw(data as *mut H);
            handler(hwnd, msg, wparam, lparam).unwrap_or_default()
        }

        msg => {
            let handler = &mut *(data as *mut H);
            if let Some(r) = handler(hwnd, msg, wparam, lparam) {
                r
            } else {
                winapi::um::commctrl::DefSubclassProc(hwnd, msg, wparam, lparam)
            }
        }
    }
}

#[cfg(feature = "full")]
pub(crate) struct RunOnDrop<F: FnOnce()>(Option<F>);
#[cfg(feature = "full")]
impl<F: FnOnce()> RunOnDrop<F> {
    pub fn new(clean: F) -> Self {
        RunOnDrop(Some(clean))
    }
}
#[cfg(feature = "full")]
impl<F: FnOnce()> Drop for RunOnDrop<F> {
    fn drop(&mut self) {
        if let Some(clean) = self.0.take() {
            clean();
        }
    }
}

/*
use io::Write;

#[doc(hidden)]
pub fn write_trace(msg: std::fmt::Arguments) {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .create(true)
        .open("dump.view-process-trace.log")
        .unwrap();

    file.write_fmt(msg).expect("failed trace");
    file.write_all(b"\n").expect("failed trace");
    file.flush().expect("failed trace");
}
#[doc(hidden)]
#[macro_export]
macro_rules! write_trace {
    ($($tt:tt)*) => {
        #[cfg(debug_assertions)]
        $crate::util::write_trace(format_args!($($tt)*))
    };
}
pub use crate::write_trace;
*/
