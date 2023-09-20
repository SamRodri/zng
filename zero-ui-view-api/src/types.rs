//! General event types.

use crate::{
    access::{AccessCommand, AccessNodeId},
    api_extension::{ApiExtensionId, ApiExtensionPayload, ApiExtensions},
    config::{AnimationsConfig, ColorScheme, FontAntiAliasing, KeyRepeatConfig, LocaleConfig, MultiClickConfig, TouchConfig},
    dialog::{DialogId, FileDialogResponse, MsgDialogResponse},
    image::{ImageId, ImageLoadedData, ImagePpi},
    ipc::IpcBytes,
    keyboard::{Key, KeyCode, KeyState},
    mouse::{ButtonId, ButtonState, MouseButton, MouseScrollDelta},
    touch::{TouchPhase, TouchUpdate},
    units::*,
    window::{EventFrameRendered, FrameId, HeadlessOpenData, MonitorId, MonitorInfo, WindowChanged, WindowId, WindowOpenData},
};
use serde::{Deserialize, Serialize};
use std::{fmt, path::PathBuf};

macro_rules! declare_id {
    ($(
        $(#[$docs:meta])+
        pub struct $Id:ident(_);
    )+) => {$(
        $(#[$docs])+
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $Id(u32);

        impl $Id {
            /// Dummy ID, zero.
            pub const INVALID: Self = Self(0);

            /// Create the first valid ID.
            pub const fn first() -> Self {
                Self(1)
            }

            /// Create the next ID.
            ///
            /// IDs wrap around to [`first`] when the entire `u32` space is used, it is never `INVALID`.
            ///
            /// [`first`]: Self::first
            #[must_use]
            pub const fn next(self) -> Self {
                let r = Self(self.0.wrapping_add(1));
                if r.0 == Self::INVALID.0 {
                    Self::first()
                } else {
                    r
                }
            }

            /// Replace self with [`next`] and returns.
            ///
            /// [`next`]: Self::next
            #[must_use]
            pub fn incr(&mut self) -> Self {
                std::mem::replace(self, self.next())
            }

            /// Get the raw ID.
            pub const fn get(self) -> u32 {
                self.0
            }

            /// Create an ID using a custom value.
            ///
            /// Note that only the documented process must generate IDs, and that it must only
            /// generate IDs using this function or the [`next`] function.
            ///
            /// If the `id` is zero it will still be [`INVALID`] and handled differently by the other process,
            /// zero is never valid.
            ///
            /// [`next`]: Self::next
            /// [`INVALID`]: Self::INVALID
            pub const fn from_raw(id: u32) -> Self {
                Self(id)
            }
        }
    )+};
}

pub(crate) use declare_id;

declare_id! {
    /// Device ID in channel.
    ///
    /// In the View Process this is mapped to a system id.
    ///
    /// In the App Process this is mapped to an unique id, but does not survived View crashes.
    ///
    /// The View Process defines the ID.
    pub struct DeviceId(_);

    /// View-process generation, starts at one and changes every respawn, it is never zero.
    ///
    /// The View Process defines the ID.
    pub struct ViewProcessGen(_);
}

/// Identifier for a specific analog axis on some device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AxisId(pub u32);

#[derive(Debug, Clone, Serialize, Deserialize)]
/// View process is online.
///
/// The [`ViewProcessGen`] is the generation of the new view-process, it must be passed to
/// [`Controller::handle_inited`].
///
/// [`Controller::handle_inited`]: crate::Controller::handle_inited
pub struct Inited {
    /// View-process generation, changes after respawns and is never zero.
    pub generation: ViewProcessGen,
    /// If the view-process is a respawn from a previous crashed process.
    pub is_respawn: bool,

    /// Available monitors.
    pub available_monitors: Vec<(MonitorId, MonitorInfo)>,
    /// System multi-click config.
    pub multi_click_config: MultiClickConfig,
    /// System keyboard pressed key repeat start delay config.
    pub key_repeat_config: KeyRepeatConfig,
    /// System touch config.
    pub touch_config: TouchConfig,
    /// System font anti-aliasing config.
    pub font_aa: FontAntiAliasing,
    /// System animations config.
    pub animations_config: AnimationsConfig,
    /// System locale config.
    pub locale_config: LocaleConfig,
    /// System preferred color scheme.
    pub color_scheme: ColorScheme,
    /// API extensions implemented by the view-process.
    ///
    /// The extension IDs will stay valid for the duration of the view-process.
    pub extensions: ApiExtensions,
}

/// System and User events sent from the View Process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    /// View-process inited.
    Inited(Inited),

    /// The event channel disconnected, probably because the view-process crashed.
    ///
    /// The [`ViewProcessGen`] is the generation of the view-process that was lost, it must be passed to
    /// [`Controller::handle_disconnect`].
    ///
    /// [`Controller::handle_disconnect`]: crate::Controller::handle_disconnect
    Disconnected(ViewProcessGen),

    /// Window, context and renderer have finished initializing and is ready to receive commands.
    WindowOpened(WindowId, WindowOpenData),

    /// Headless context and renderer have finished initializing and is ready to receive commands.
    HeadlessOpened(WindowId, HeadlessOpenData),

    /// Window open or headless context open request failed.
    WindowOrHeadlessOpenError {
        /// Id from the request.
        id: WindowId,
        /// Error message.
        error: String,
    },

    /// A frame finished rendering.
    ///
    /// `EventsCleared` is not send after this event.
    FrameRendered(EventFrameRendered),

    /// Window moved, resized, or minimized/maximized etc.
    ///
    /// This event coalesces events usually named `WindowMoved`, `WindowResized` and `WindowStateChanged` into a
    /// single event to simplify tracking composite changes, for example, the window changes size and position
    /// when maximized, this can be trivially observed with this event.
    ///
    /// The [`EventCause`] can be used to identify a state change initiated by the app.
    ///
    /// [`EventCause`]: crate::window::EventCause
    WindowChanged(WindowChanged),

    /// A file has been dropped into the window.
    ///
    /// When the user drops multiple files at once, this event will be emitted for each file separately.
    DroppedFile {
        /// Window that received the file drop.
        window: WindowId,
        /// Path to the file that was dropped.
        file: PathBuf,
    },
    /// A file is being hovered over the window.
    ///
    /// When the user hovers multiple files at once, this event will be emitted for each file separately.
    HoveredFile {
        /// Window that was hovered by drag-drop.
        window: WindowId,
        /// Path to the file being dragged.
        file: PathBuf,
    },
    /// A file was hovered, but has exited the window.
    ///
    /// There will be a single event triggered even if multiple files were hovered.
    HoveredFileCancelled(WindowId),

    /// App window(s) focus changed.
    FocusChanged {
        /// Window that lost focus.
        prev: Option<WindowId>,
        /// Window that got focus.
        new: Option<WindowId>,
    },
    /// An event from the keyboard has been received.
    ///
    /// This event is only send if the window is focused, all pressed keys should be considered released
    /// after [`FocusChanged`] to `None`. Modifier keys receive special treatment, after they are pressed,
    /// the modifier key state is monitored directly so that the `Released` event is always send, unless the
    /// focus changed to none.
    ///
    /// [`FocusChanged`]: Self::FocusChanged
    KeyboardInput {
        /// Window that received the key event.
        window: WindowId,
        /// Device that generated the key event.
        device: DeviceId,
        /// Physical key.
        key_code: KeyCode,
        /// If the key was pressed or released.
        state: KeyState,

        /// Semantic key.
        ///
        /// Pressing `Shift+A` key will produce `Key::Char('a')` in QWERT keyboards, the modifiers are not applied.
        key: Option<Key>,
        /// Semantic key modified by the current active modifiers.
        ///
        /// Pressing `Shift+A` key will produce `Key::Char('A')` in QWERT keyboards, the modifiers are applied.
        key_modified: Option<Key>,
        /// Text typed.
        ///
        /// This is only set during [`KeyState::Pressed`] of a key that generates text.
        ///
        /// This is usually the `key_modified` char, but is also `'\r'` for `Key::Enter`. On Windows when a dead key was
        /// pressed earlier but cannot be combined with the character from this key press, the produced text
        /// will consist of two characters: the dead-key-character followed by the character resulting from this key press.
        text: String,
    },
    /// The mouse cursor has moved on the window.
    ///
    /// This event can be coalesced, i.e. multiple cursor moves packed into the same event.
    MouseMoved {
        /// Window that received the cursor move.
        window: WindowId,
        /// Device that generated the cursor move.
        device: DeviceId,

        /// Cursor positions in between the previous event and this one.
        coalesced_pos: Vec<DipPoint>,

        /// Cursor position, relative to the window top-left in device independent pixels.
        position: DipPoint,
    },

    /// The mouse cursor has entered the window.
    MouseEntered {
        /// Window that now is hovered by the cursor.
        window: WindowId,
        /// Device that generated the cursor move event.
        device: DeviceId,
    },
    /// The mouse cursor has left the window.
    MouseLeft {
        /// Window that is no longer hovered by the cursor.
        window: WindowId,
        /// Device that generated the cursor move event.
        device: DeviceId,
    },
    /// A mouse wheel movement or touchpad scroll occurred.
    MouseWheel {
        /// Window that was hovered by the cursor when the mouse wheel was used.
        window: WindowId,
        /// Device that generated the mouse wheel event.
        device: DeviceId,
        /// Delta of change in the mouse scroll wheel state.
        delta: MouseScrollDelta,
        /// Touch state if the device that generated the event is a touchpad.
        phase: TouchPhase,
    },
    /// An mouse button press has been received.
    MouseInput {
        /// Window that was hovered by the cursor when the mouse button was used.
        window: WindowId,
        /// Mouse device that generated the event.
        device: DeviceId,
        /// If the button was pressed or released.
        state: ButtonState,
        /// The mouse button.
        button: MouseButton,
    },
    /// Touchpad pressure event.
    TouchpadPressure {
        /// Window that was hovered when the touchpad was touched.
        window: WindowId,
        /// Touchpad device.
        device: DeviceId,
        /// Pressure level between 0 and 1.
        pressure: f32,
        /// Click level.
        stage: i64,
    },
    /// Motion on some analog axis. May report data redundant to other, more specific events.
    AxisMotion {
        /// Window that was focused when the motion was realized.
        window: WindowId,
        /// Analog device.
        device: DeviceId,
        /// Axis.
        axis: AxisId,
        /// Motion value.
        value: f64,
    },
    /// Touch event has been received.
    Touch {
        /// Window that was touched.
        window: WindowId,
        /// Touch device.
        device: DeviceId,

        /// Coalesced touch updates, never empty.
        touches: Vec<TouchUpdate>,
    },
    /// The monitor’s scale factor has changed.
    ScaleFactorChanged {
        /// Monitor that has changed.
        monitor: MonitorId,
        /// Windows affected by this change.
        ///
        /// Note that a window's scale factor can also change if it is moved to another monitor,
        /// the [`Event::WindowChanged`] event notifies this using the [`WindowChanged::monitor`].
        windows: Vec<WindowId>,
        /// The new scale factor.
        scale_factor: f32,
    },

    /// The available monitors have changed.
    MonitorsChanged(Vec<(MonitorId, MonitorInfo)>),

    /// The preferred color scheme for a window has changed.
    ColorSchemeChanged(WindowId, ColorScheme),
    /// The window has been requested to close.
    WindowCloseRequested(WindowId),
    /// The window has closed.
    WindowClosed(WindowId),

    /// An image resource already decoded size and PPI.
    ImageMetadataLoaded {
        /// The image that started loading.
        image: ImageId,
        /// The image pixel size.
        size: PxSize,
        /// The image pixels-per-inch metadata.
        ppi: Option<ImagePpi>,
        /// The image is a single channel R8.
        is_mask: bool,
    },
    /// An image resource finished decoding.
    ImageLoaded(ImageLoadedData),
    /// An image resource, progressively decoded has decoded more bytes.
    ImagePartiallyLoaded {
        /// The image that has decoded more pixels.
        image: ImageId,
        /// The size of the decoded pixels, can be different then the image size if the
        /// image is not *interlaced*.
        partial_size: PxSize,
        /// The image pixels-per-inch metadata.
        ppi: Option<ImagePpi>,
        /// If the decoded pixels so-far are all opaque (255 alpha).
        is_opaque: bool,
        /// If the decoded pixels so-far are a single channel.
        is_mask: bool,
        /// Updated BGRA8 pre-multiplied pixel buffer or R8 if `is_mask`. This includes all the pixels
        /// decoded so-far.
        partial_pixels: IpcBytes,
    },
    /// An image resource failed to decode, the image ID is not valid.
    ImageLoadError {
        /// The image that failed to decode.
        image: ImageId,
        /// The error message.
        error: String,
    },
    /// An image finished encoding.
    ImageEncoded {
        /// The image that finished encoding.
        image: ImageId,
        /// The format of the encoded data.
        format: String,
        /// The encoded image data.
        data: IpcBytes,
    },
    /// An image failed to encode.
    ImageEncodeError {
        /// The image that failed to encode.
        image: ImageId,
        /// The encoded format that was requested.
        format: String,
        /// The error message.
        error: String,
    },

    /// An image generated from a rendered frame is ready.
    FrameImageReady {
        /// Window that had pixels copied.
        window: WindowId,
        /// The frame that was rendered when the pixels where copied.
        frame: FrameId,
        /// The frame image.
        image: ImageId,
        /// The pixel selection relative to the top-left.
        selection: PxRect,
    },

    // Config events
    /// System fonts have changed.
    FontsChanged,
    /// System text-antialiasing configuration has changed.
    FontAaChanged(FontAntiAliasing),
    /// System double-click definition changed.
    MultiClickConfigChanged(MultiClickConfig),
    /// System animations config changed.
    AnimationsConfigChanged(AnimationsConfig),
    /// System definition of pressed key repeat event changed.
    KeyRepeatConfigChanged(KeyRepeatConfig),
    /// System touch config changed.
    TouchConfigChanged(TouchConfig),
    /// System locale changed.
    LocaleChanged(LocaleConfig),

    // Raw device events
    /// Device added or installed.
    DeviceAdded(DeviceId),
    /// Device removed.
    DeviceRemoved(DeviceId),
    /// Mouse pointer motion.
    ///
    /// The values if the delta of movement (x, y), not position.
    DeviceMouseMotion {
        /// Device that generated the event.
        device: DeviceId,
        /// Delta of change in the cursor position.
        delta: euclid::Vector2D<f64, ()>,
    },
    /// Mouse scroll wheel turn.
    DeviceMouseWheel {
        /// Mouse device that generated the event.
        device: DeviceId,
        /// Delta of change in the mouse scroll wheel state.
        delta: MouseScrollDelta,
    },
    /// Motion on some analog axis.
    ///
    /// This includes the mouse device and any other that fits.
    DeviceMotion {
        /// Device that generated the event.
        device: DeviceId,
        /// Device dependent axis of the motion.
        axis: AxisId,
        /// Device dependent value.
        value: f64,
    },
    /// Device button press or release.
    DeviceButton {
        /// Device that generated the event.
        device: DeviceId,
        /// Device dependent button that was used.
        button: ButtonId,
        /// If the button was pressed or released.
        state: ButtonState,
    },
    /// Device key press or release.
    DeviceKey {
        /// Device that generated the key event.
        device: DeviceId,
        /// Physical key.
        key_code: KeyCode,
        /// If the key was pressed or released.
        state: KeyState,
    },
    /// User responded to a native message dialog.
    MsgDialogResponse(DialogId, MsgDialogResponse),
    /// User responded to a native file dialog.
    FileDialogResponse(DialogId, FileDialogResponse),

    /// Accessibility command.
    AccessCommand {
        /// Window that had pixels copied.
        window: WindowId,
        /// Target widget.
        target: AccessNodeId,
        /// Command.
        command: AccessCommand,
    },

    /// Represents a custom event send by the extension.
    ExtensionEvent(ApiExtensionId, ApiExtensionPayload),
}
impl Event {
    /// Change `self` to incorporate `other` or returns `other` if both events cannot be coalesced.
    #[allow(clippy::result_large_err)]
    pub fn coalesce(&mut self, other: Event) -> Result<(), Event> {
        use Event::*;

        match (self, other) {
            (
                MouseMoved {
                    window,
                    device,
                    coalesced_pos,
                    position,
                },
                MouseMoved {
                    window: n_window,
                    device: n_device,
                    coalesced_pos: n_coal_pos,
                    position: n_pos,
                },
            ) if *window == n_window && *device == n_device => {
                coalesced_pos.push(*position);
                coalesced_pos.extend(n_coal_pos);
                *position = n_pos;
            }
            // raw mouse motion.
            (
                DeviceMouseMotion { device, delta },
                DeviceMouseMotion {
                    device: n_device,
                    delta: n_delta,
                },
            ) if *device == n_device => {
                *delta += n_delta;
            }

            // wheel scroll.
            (
                MouseWheel {
                    window,
                    device,
                    delta: MouseScrollDelta::LineDelta(delta_x, delta_y),
                    phase,
                },
                MouseWheel {
                    window: n_window,
                    device: n_device,
                    delta: MouseScrollDelta::LineDelta(n_delta_x, n_delta_y),
                    phase: n_phase,
                },
            ) if *window == n_window && *device == n_device && *phase == n_phase => {
                *delta_x += n_delta_x;
                *delta_y += n_delta_y;
            }

            // trackpad scroll-move.
            (
                MouseWheel {
                    window,
                    device,
                    delta: MouseScrollDelta::PixelDelta(delta_x, delta_y),
                    phase,
                },
                MouseWheel {
                    window: n_window,
                    device: n_device,
                    delta: MouseScrollDelta::PixelDelta(n_delta_x, n_delta_y),
                    phase: n_phase,
                },
            ) if *window == n_window && *device == n_device && *phase == n_phase => {
                *delta_x += n_delta_x;
                *delta_y += n_delta_y;
            }

            // raw wheel scroll.
            (
                DeviceMouseWheel {
                    device,
                    delta: MouseScrollDelta::LineDelta(delta_x, delta_y),
                },
                DeviceMouseWheel {
                    device: n_device,
                    delta: MouseScrollDelta::LineDelta(n_delta_x, n_delta_y),
                },
            ) if *device == n_device => {
                *delta_x += n_delta_x;
                *delta_y += n_delta_y;
            }

            // raw trackpad scroll-move.
            (
                DeviceMouseWheel {
                    device,
                    delta: MouseScrollDelta::PixelDelta(delta_x, delta_y),
                },
                DeviceMouseWheel {
                    device: n_device,
                    delta: MouseScrollDelta::PixelDelta(n_delta_x, n_delta_y),
                },
            ) if *device == n_device => {
                *delta_x += n_delta_x;
                *delta_y += n_delta_y;
            }

            // touch
            (
                Touch { window, device, touches },
                Touch {
                    window: n_window,
                    device: n_device,
                    touches: mut n_touches,
                },
            ) if *window == n_window && *device == n_device => {
                touches.append(&mut n_touches);
            }

            // window changed.
            (WindowChanged(change), WindowChanged(n_change))
                if change.window == n_change.window && change.cause == n_change.cause && change.frame_wait_id.is_none() =>
            {
                if n_change.state.is_some() {
                    change.state = n_change.state;
                }

                if n_change.position.is_some() {
                    change.position = n_change.position;
                }

                if n_change.monitor.is_some() {
                    change.monitor = n_change.monitor;
                }

                if n_change.size.is_some() {
                    change.size = n_change.size;
                }

                change.frame_wait_id = n_change.frame_wait_id;
            }
            // window focus changed.
            (FocusChanged { prev, new }, FocusChanged { prev: n_prev, new: n_new })
                if prev.is_some() && new.is_none() && n_prev.is_none() && n_new.is_some() =>
            {
                *new = n_new;
            }
            // scale factor.
            (
                ScaleFactorChanged {
                    monitor,
                    windows,
                    scale_factor,
                },
                ScaleFactorChanged {
                    monitor: n_monitor,
                    windows: n_windows,
                    scale_factor: n_scale_factor,
                },
            ) if *monitor == n_monitor => {
                for w in n_windows {
                    if !windows.contains(&w) {
                        windows.push(w);
                    }
                }
                *scale_factor = n_scale_factor;
            }
            // fonts changed.
            (FontsChanged, FontsChanged) => {}
            // text aa.
            (FontAaChanged(config), FontAaChanged(n_config)) => {
                *config = n_config;
            }
            // double-click timeout.
            (MultiClickConfigChanged(config), MultiClickConfigChanged(n_config)) => {
                *config = n_config;
            }
            // touch config.
            (TouchConfigChanged(config), TouchConfigChanged(n_config)) => {
                *config = n_config;
            }
            // animation enabled and caret speed.
            (AnimationsConfigChanged(config), AnimationsConfigChanged(n_config)) => {
                *config = n_config;
            }
            // key repeat delay and speed.
            (KeyRepeatConfigChanged(config), KeyRepeatConfigChanged(n_config)) => {
                *config = n_config;
            }
            // locale
            (LocaleChanged(config), LocaleChanged(n_config)) => {
                *config = n_config;
            }
            (_, e) => return Err(e),
        }
        Ok(())
    }
}

/// The View-Process disconnected or has not finished initializing, try again after the *inited* event.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub struct ViewProcessOffline;
impl fmt::Display for ViewProcessOffline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "view-process disconnected or is initing, try again after the init event")
    }
}
impl std::error::Error for ViewProcessOffline {}

/// View Process IPC result.
pub(crate) type VpResult<T> = std::result::Result<T, ViewProcessOffline>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_code_iter() {
        let mut iter = KeyCode::all_identified();
        let first = iter.next().unwrap();
        assert_eq!(first, KeyCode::Backquote);

        for k in iter {
            assert_eq!(k.name(), &format!("{:?}", k));
        }
    }
}
