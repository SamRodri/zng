//! Keyboard events.

use crate::core::app::*;
use crate::core::context::*;
use crate::core::event::*;
use crate::core::focus::Focus;
use crate::core::render::WidgetPath;
use crate::core::types::*;
use std::time::Instant;

event_args! {
    /// [KeyInput], [KeyDown], [KeyUp] event args.
    pub struct KeyInputArgs {
        /// Id of window that received the event.
        pub window_id: WindowId,

        /// Id of device that generated the event.
        pub device_id: DeviceId,

        /// Raw code of key.
        pub scancode: ScanCode,

        /// If the key was pressed or released.
        pub state: ElementState,

        /// Symbolic name of [scancode](KeyInputArgs::scancode).
        pub key: Option<VirtualKeyCode>,

        /// What modifier keys where pressed when this event happened.
        pub modifiers: ModifiersState,

        /// If the key-down event was generated by holding the key pressed.
        pub repeat: bool,

        /// The focused element at the time of the key input.
        pub target: Option<WidgetPath>,

        ..

        /// If the widget is focused or contains the focused widget.
        fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
            if let Some(wp) = &self.target {
                wp.contains(ctx.widget_id)
            } else {
                false
            }
         }
    }
}

/// Key pressed or released event.
pub struct KeyInput;
impl Event for KeyInput {
    type Args = KeyInputArgs;
}

/// Key pressed or repeat event.
pub struct KeyDown;
impl Event for KeyDown {
    type Args = KeyInputArgs;
}

/// Key released event.
pub struct KeyUp;
impl Event for KeyUp {
    type Args = KeyInputArgs;
}

/// Application extension that provides keyboard events.
///
/// # Events
///
/// Events this extension provides.
///
/// * [KeyInput]
/// * [KeyDown]
/// * [KeyUp]
pub struct KeyboardEvents {
    last_key_down: Option<ScanCode>,
    modifiers: ModifiersState,
    key_input: EventEmitter<KeyInputArgs>,
    key_down: EventEmitter<KeyInputArgs>,
    key_up: EventEmitter<KeyInputArgs>,
}

impl Default for KeyboardEvents {
    fn default() -> Self {
        KeyboardEvents {
            last_key_down: None,
            modifiers: ModifiersState::default(),
            key_input: EventEmitter::new(false),
            key_down: EventEmitter::new(false),
            key_up: EventEmitter::new(false),
        }
    }
}

impl AppExtension for KeyboardEvents {
    fn init(&mut self, r: &mut AppInitContext) {
        r.events.register::<KeyInput>(self.key_input.listener());
        r.events.register::<KeyDown>(self.key_down.listener());
        r.events.register::<KeyUp>(self.key_up.listener());
    }

    fn on_device_event(&mut self, _: DeviceId, event: &DeviceEvent, _: &mut AppContext) {
        if let DeviceEvent::ModifiersChanged(m) = event {
            self.modifiers = *m;
        }
    }

    fn on_window_event(&mut self, window_id: WindowId, event: &WindowEvent, ctx: &mut AppContext) {
        if let WindowEvent::KeyboardInput {
            device_id,
            input:
                KeyboardInput {
                    scancode,
                    state,
                    virtual_keycode: key,
                    ..
                },
            ..
        } = *event
        {
            let mut repeat = false;
            if state == ElementState::Pressed {
                repeat = self.last_key_down == Some(scancode);
                if !repeat {
                    self.last_key_down = Some(scancode);
                }
            } else {
                self.last_key_down = None;
            }

            let target = ctx.services.get::<Focus>().and_then(|f| f.focused().cloned());

            let args = KeyInputArgs {
                timestamp: Instant::now(),
                window_id,
                device_id,
                scancode,
                key,
                modifiers: self.modifiers,
                state,
                repeat,
                target,
            };

            ctx.updates.push_notify(self.key_input.clone(), args.clone());

            match state {
                ElementState::Pressed => {
                    ctx.updates.push_notify(self.key_down.clone(), args);
                    todo!()
                }
                ElementState::Released => {
                    ctx.updates.push_notify(self.key_up.clone(), args);
                }
            }
        }
    }
}
