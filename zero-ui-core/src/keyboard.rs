//! Keyboard manager.
//!
//! The [`KeyboardManager`] struct is an [app extension](crate::app::AppExtension), it
//! is included in the [default app](crate::app::App::default) and provides the [`Keyboard`] service
//! and keyboard input events.

use std::time::{Duration, Instant};

use crate::app::view_process::VIEW_PROCESS_INITED_EVENT;
use crate::app::{raw_events::*, *};
use crate::event::*;
use crate::focus::Focus;
use crate::service::*;
use crate::units::TimeUnits;
use crate::var::{var, RcVar, ReadOnlyRcVar, Var, Vars};
use crate::widget_info::InteractionPath;
use crate::window::WindowId;
use crate::{context::*, WidgetId};

use linear_map::set::LinearSet;
pub use zero_ui_view_api::{Key, KeyState, ScanCode};

event_args! {
    /// Arguments for [`KEY_INPUT_EVENT`].
    pub struct KeyInputArgs {
        /// Window that received the event.
        pub window_id: WindowId,

        /// Device that generated the event.
        pub device_id: DeviceId,

        /// Raw code of key.
        pub scan_code: ScanCode,

        /// If the key was pressed or released.
        pub state: KeyState,

        /// Symbolic name of [`scan_code`](KeyInputArgs::scan_code).
        pub key: Option<Key>,

        /// What modifier keys where pressed when this event happened.
        pub modifiers: ModifiersState,

        /// If the key-down event was generated by holding the key pressed.
        pub is_repeat: bool,

        /// The focused element at the time of the key input.
        pub target: InteractionPath,

        ..

        /// The [`target`](Self::target).
        fn delivery_list(&self, list: &mut UpdateDeliveryList) {
            list.insert_path(&self.target)
        }
    }

    /// Arguments for [`CHAR_INPUT_EVENT`].
    pub struct CharInputArgs {
        /// Window that received the event.
        pub window_id: WindowId,

        /// Unicode character.
        pub character: char,

        /// The focused element at the time of the key input.
        pub target: InteractionPath,

        ..

        /// The [`target`](Self::target).
        fn delivery_list(&self, list: &mut UpdateDeliveryList) {
            list.insert_path(&self.target)
        }
    }

    /// Arguments for [`MODIFIERS_CHANGED_EVENT`].
    pub struct ModifiersChangedArgs {
        /// Previous modifiers state.
        pub prev_modifiers: ModifiersState,

        /// Current modifiers state.
        pub modifiers: ModifiersState,

        ..

        /// Broadcast to all.
        fn delivery_list(&self, list: &mut UpdateDeliveryList) {
            list.search_all()
        }
    }
}
impl KeyInputArgs {
    /// Returns `true` if the widget is enabled in [`target`].
    ///
    /// [`target`]: Self::target
    pub fn is_enabled(&self, widget_id: WidgetId) -> bool {
        self.target.interactivity_of(widget_id).map(|i| i.is_enabled()).unwrap_or(false)
    }

    /// Returns `true` if the widget is disabled in [`target`].
    ///
    /// [`target`]: Self::target
    pub fn is_disabled(&self, widget_id: WidgetId) -> bool {
        self.target.interactivity_of(widget_id).map(|i| i.is_disabled()).unwrap_or(false)
    }
}
impl CharInputArgs {
    /// Returns `true` if the widget is enabled in [`target`].
    ///
    /// [`target`]: Self::target
    pub fn is_enabled(&self, widget_id: WidgetId) -> bool {
        self.target.interactivity_of(widget_id).map(|i| i.is_enabled()).unwrap_or(false)
    }

    /// Returns `true` if the widget is disabled in [`target`].
    ///
    /// [`target`]: Self::target
    pub fn is_disabled(&self, widget_id: WidgetId) -> bool {
        self.target.interactivity_of(widget_id).map(|i| i.is_disabled()).unwrap_or(false)
    }

    /// Returns `true` if the character is the backspace.
    pub fn is_backspace(&self) -> bool {
        self.character == '\u{8}'
    }
}

event! {
    /// Key pressed, repeat pressed or released event.
    ///
    /// # Provider
    ///
    /// This event is provided by the [`KeyboardManager`] extension.
    pub static KEY_INPUT_EVENT: KeyInputArgs;

    /// Modifiers key state changed event.
    ///
    /// # Provider
    ///
    /// This event is provided by the [`KeyboardManager`] extension.
    pub static MODIFIERS_CHANGED_EVENT: ModifiersChangedArgs;

    /// Character received event.
    ///
    /// # Provider
    ///
    /// This event is provided by the [`KeyboardManager`] extension.
    pub static CHAR_INPUT_EVENT: CharInputArgs;
}

/// Application extension that provides keyboard events targeting the focused widget.
///
/// This [extension] processes the raw keyboard events retargeting then to the focused widget, generating derived events and variables.
///
/// # Events
///
/// Events this extension provides.
///
/// * [`KEY_INPUT_EVENT`]
/// * [`MODIFIERS_CHANGED_EVENT`]
/// * [`CHAR_INPUT_EVENT`]
///
/// # Services
///
/// Services this extension provides.
///
/// * [Keyboard]
///
/// # Default
///
/// This extension is included in the [default app], events provided by it
/// are required by multiple other extensions.
///
/// # Dependencies
///
/// This extension requires the [`Focus`] and [`Windows`] services before the first raw key input event. It does not
/// require anything for initialization.
///
/// [extension]: AppExtension
/// [default app]: crate::app::App::default
/// [`Focus`]: crate::focus::Focus
/// [`Windows`]: crate::window::Windows
#[derive(Default)]
pub struct KeyboardManager;
impl AppExtension for KeyboardManager {
    fn init(&mut self, r: &mut AppContext) {
        let kb = Keyboard::new();
        r.services.register(kb);
    }

    fn event_preview(&mut self, ctx: &mut AppContext, update: &mut EventUpdate) {
        if let Some(args) = RAW_KEY_INPUT_EVENT.on(update) {
            let focused = Focus::req(ctx.services).focused().get();
            let keyboard = Keyboard::req(ctx.services);
            keyboard.key_input(ctx.events, ctx.vars, args, focused);
        } else if let Some(args) = RAW_CHAR_INPUT_EVENT.on(update) {
            let focused = Focus::req(ctx.services).focused().get();
            if let Some(target) = focused {
                if target.window_id() == args.window_id {
                    CHAR_INPUT_EVENT.notify(ctx, CharInputArgs::now(args.window_id, args.character, target));
                }
            }
        } else if let Some(args) = RAW_KEY_REPEAT_DELAY_CHANGED_EVENT.on(update) {
            let kb = Keyboard::req(ctx.services);
            kb.repeat_delay.set_ne(ctx.vars, args.delay);
            kb.last_key_down = None;
        } else if let Some(args) = RAW_WINDOW_FOCUS_EVENT.on(update) {
            if args.new_focus.is_none() {
                let kb = Keyboard::req(ctx.services);

                kb.modifiers.set_ne(ctx.vars, ModifiersState::empty());
                kb.current_modifiers.clear();
                kb.codes.set_ne(ctx.vars, vec![]);
                kb.keys.set_ne(ctx.vars, vec![]);

                kb.last_key_down = None;
            }
        } else if let Some(args) = VIEW_PROCESS_INITED_EVENT.on(update) {
            let kb = Keyboard::req(ctx.services);
            kb.repeat_delay.set_ne(ctx.vars, args.key_repeat_delay);

            if args.is_respawn {
                kb.modifiers.set_ne(ctx.vars, ModifiersState::empty());
                kb.current_modifiers.clear();
                kb.codes.set_ne(ctx.vars, vec![]);
                kb.keys.set_ne(ctx.vars, vec![]);

                kb.last_key_down = None;
            }
        }
    }
}

/// Keyboard service.
///
/// # Provider
///
/// This service is provided by the [`KeyboardManager`] extension.
#[derive(Service)]
pub struct Keyboard {
    current_modifiers: LinearSet<Key>,

    modifiers: RcVar<ModifiersState>,
    codes: RcVar<Vec<ScanCode>>,
    keys: RcVar<Vec<Key>>,
    repeat_delay: RcVar<Duration>,

    last_key_down: Option<(DeviceId, ScanCode, Instant)>,
}
impl Keyboard {
    fn new() -> Self {
        Keyboard {
            current_modifiers: LinearSet::new(),
            modifiers: var(ModifiersState::empty()),
            codes: var(vec![]),
            keys: var(vec![]),
            repeat_delay: var(600.ms()),
            last_key_down: None,
        }
    }

    fn key_input(&mut self, events: &mut Events, vars: &Vars, args: &RawKeyInputArgs, focused: Option<InteractionPath>) {
        let mut repeat = false;

        // update state and vars
        match args.state {
            KeyState::Pressed => {
                if let Some((d_id, code, time)) = &mut self.last_key_down {
                    let max_t = self.repeat_delay.get() * 2;
                    if args.scan_code == *code && args.device_id == *d_id && (args.timestamp - *time) < max_t {
                        repeat = true;
                    } else {
                        *d_id = args.device_id;
                        *code = args.scan_code;
                    }
                    *time = args.timestamp;
                } else {
                    self.last_key_down = Some((args.device_id, args.scan_code, args.timestamp));
                }

                let scan_code = args.scan_code;
                if !self.codes.with(|c| c.contains(&scan_code)) {
                    self.codes.modify(vars, move |cs| {
                        cs.get_mut().push(scan_code);
                    });
                }

                if let Some(key) = args.key {
                    if !self.keys.with(|c| c.contains(&key)) {
                        self.keys.modify(vars, move |ks| {
                            ks.get_mut().push(key);
                        });
                    }

                    if key.is_modifier() {
                        self.set_modifiers(events, vars, key, true);
                    }
                }
            }
            KeyState::Released => {
                self.last_key_down = None;

                let key = args.scan_code;
                if self.codes.with(|c| c.contains(&key)) {
                    self.codes.modify(vars, move |cs| {
                        if let Some(i) = cs.get().iter().position(|c| *c == key) {
                            cs.get_mut().swap_remove(i);
                        }
                    });
                }

                if let Some(key) = args.key {
                    if self.keys.with(|c| c.contains(&key)) {
                        self.keys.modify(vars, move |ks| {
                            if let Some(i) = ks.get().iter().position(|k| *k == key) {
                                ks.get_mut().swap_remove(i);
                            }
                        });
                    }

                    if key.is_modifier() {
                        self.set_modifiers(events, vars, key, false);
                    }
                }
            }
        }

        // notify events
        if let Some(target) = focused {
            if target.window_id() == args.window_id {
                let args = KeyInputArgs::now(
                    args.window_id,
                    args.device_id,
                    args.scan_code,
                    args.state,
                    args.key,
                    self.current_modifiers(),
                    repeat,
                    target,
                );
                KEY_INPUT_EVENT.notify(events, args);
            }
        }
    }
    fn set_modifiers(&mut self, events: &mut Events, vars: &Vars, key: Key, pressed: bool) {
        let prev_modifiers = self.current_modifiers();

        if pressed {
            self.current_modifiers.insert(key);
        } else {
            self.current_modifiers.remove(&key);
        }

        let new_modifiers = self.current_modifiers();

        if prev_modifiers != new_modifiers {
            self.modifiers.set(vars, new_modifiers);
            MODIFIERS_CHANGED_EVENT.notify(events, ModifiersChangedArgs::now(prev_modifiers, new_modifiers));
        }
    }

    fn current_modifiers(&self) -> ModifiersState {
        let mut state = ModifiersState::empty();
        for key in &self.current_modifiers {
            state |= ModifiersState::from_key(*key);
        }
        state
    }

    /// Returns a read-only variable that tracks the currently pressed modifier keys.
    pub fn modifiers(&self) -> ReadOnlyRcVar<ModifiersState> {
        self.modifiers.read_only()
    }

    /// Returns a read-only variable that tracks the [`ScanCode`] of the keys currently pressed.
    pub fn codes(&self) -> ReadOnlyRcVar<Vec<ScanCode>> {
        self.codes.read_only()
    }

    /// Returns a read-only variable that tracks the [`Key`] identifier of the keys currently pressed.
    pub fn keys(&self) -> ReadOnlyRcVar<Vec<Key>> {
        self.keys.read_only()
    }

    /// Returns a read-only variable that tracks the operating system key press repeat delay.
    ///
    /// This delay is roughly the time the user must hold a key pressed to generate a new key
    /// press event. When a second key press happens without any other keyboard event and within twice this
    /// value if is marked [`is_repeat`] by the [`KeyboardManager`].
    ///
    /// [`is_repeat`]: KeyInputArgs::is_repeat
    pub fn repeat_delay(&self) -> ReadOnlyRcVar<Duration> {
        self.repeat_delay.read_only()
    }
}

/// Extension trait that adds keyboard simulation methods to [`HeadlessApp`].
pub trait HeadlessAppKeyboardExt {
    /// Notifies keyboard input event.
    ///
    /// Note that the app is not updated so the event is pending after this call.
    fn on_keyboard_input(&mut self, window_id: WindowId, key: Key, state: KeyState);

    /// Does a key-down, key-up and updates.
    fn press_key(&mut self, window_id: WindowId, key: Key);

    /// Does a modifiers changed, key-down, key-up, reset modifiers and updates.
    fn press_modified_key(&mut self, window_id: WindowId, modifiers: ModifiersState, key: Key);
}
impl HeadlessAppKeyboardExt for HeadlessApp {
    fn on_keyboard_input(&mut self, window_id: WindowId, key: Key, state: KeyState) {
        use crate::app::raw_events::*;

        let args = RawKeyInputArgs::now(window_id, DeviceId::virtual_keyboard(), key as u32, state, Some(key));
        RAW_KEY_INPUT_EVENT.notify(self.ctx().events, args);
    }

    fn press_key(&mut self, window_id: WindowId, key: Key) {
        self.on_keyboard_input(window_id, key, KeyState::Pressed);
        self.on_keyboard_input(window_id, key, KeyState::Released);
        let _ = self.update(false);
    }

    fn press_modified_key(&mut self, window_id: WindowId, modifiers: ModifiersState, key: Key) {
        if modifiers.is_empty() {
            self.press_key(window_id, key);
        } else {
            let modifiers = modifiers.keys();
            for &key in &modifiers {
                self.on_keyboard_input(window_id, key, KeyState::Pressed);
            }

            // pressed the modifiers.
            let _ = self.update(false);

            self.on_keyboard_input(window_id, key, KeyState::Pressed);
            self.on_keyboard_input(window_id, key, KeyState::Released);

            // pressed the key.
            let _ = self.update(false);

            for key in modifiers {
                self.on_keyboard_input(window_id, key, KeyState::Released);
            }

            // released the modifiers.
            let _ = self.update(false);
        }
    }
}

bitflags! {
    /// Represents the current state of the keyboard modifiers.
    ///
    /// Each flag represents a modifier and is set if this modifier is active.
    #[derive(Default)]
    pub struct ModifiersState: u8 {
        /// The left "shift" key.
        const L_SHIFT = 0b0000_0001;
        /// The right "shift" key.
        const R_SHIFT = 0b0000_0010;
        /// Any "shift" key.
        const SHIFT   = 0b0000_0011;

        /// The left "control" key.
        const L_CTRL = 0b0000_0100;
        /// The right "control" key.
        const R_CTRL = 0b0000_1000;
        /// Any "control" key.
        const CTRL   = 0b0000_1100;

        /// The left "alt" key.
        const L_ALT = 0b0001_0000;
        /// The right "alt" key.
        const R_ALT = 0b0010_0000;
        /// Any "alt" key.
        const ALT   = 0b0011_0000;

        /// The left "logo" key.
        const L_LOGO = 0b0100_0000;
        /// The right "logo" key.
        const R_LOGO = 0b1000_0000;
        /// Any "logo" key.
        ///
        /// This is the "windows" key on PC and "command" key on Mac.
        const LOGO   = 0b1100_0000;
    }
}
impl ModifiersState {
    /// Returns `true` if any shift key is pressed.
    pub fn has_shift(self) -> bool {
        self.intersects(Self::SHIFT)
    }
    /// Returns `true` if any control key is pressed.
    pub fn has_ctrl(self) -> bool {
        self.intersects(Self::CTRL)
    }
    /// Returns `true` if any alt key is pressed.
    pub fn has_alt(self) -> bool {
        self.intersects(Self::ALT)
    }
    /// Returns `true` if any logo key is pressed.
    pub fn has_logo(self) -> bool {
        self.intersects(Self::LOGO)
    }

    /// Returns `true` if only any flag in `part` is pressed.
    pub fn is_only(self, part: ModifiersState) -> bool {
        !self.is_empty() && (self - part).is_empty()
    }

    /// Returns `true` if only any shift key is pressed.
    pub fn is_only_shift(self) -> bool {
        self.is_only(ModifiersState::SHIFT)
    }
    /// Returns `true` if only any control key is pressed.
    pub fn is_only_ctrl(self) -> bool {
        self.is_only(ModifiersState::CTRL)
    }
    /// Returns `true` if only any alt key is pressed.
    pub fn is_only_alt(self) -> bool {
        self.is_only(ModifiersState::ALT)
    }
    /// Returns `true` if only any logo key is pressed.
    pub fn is_only_logo(self) -> bool {
        self.is_only(ModifiersState::LOGO)
    }

    /// Removes `part` and returns if it was removed.
    pub fn take(&mut self, part: ModifiersState) -> bool {
        let r = self.intersects(part);
        if r {
            self.remove(part);
        }
        r
    }

    /// Removes `SHIFT` and returns if it was removed.
    pub fn take_shift(&mut self) -> bool {
        self.take(ModifiersState::SHIFT)
    }

    /// Removes `CTRL` and returns if it was removed.
    pub fn take_ctrl(&mut self) -> bool {
        self.take(ModifiersState::CTRL)
    }

    /// Removes `ALT` and returns if it was removed.
    pub fn take_alt(&mut self) -> bool {
        self.take(ModifiersState::ALT)
    }

    /// Removes `LOGO` and returns if it was removed.
    pub fn take_logo(&mut self) -> bool {
        self.take(ModifiersState::LOGO)
    }

    /// Returns modifiers that set both left and right flags if any side is set in `self`.
    pub fn ambit(self) -> Self {
        let mut r = Self::empty();
        if self.has_alt() {
            r |= Self::ALT;
        }
        if self.has_ctrl() {
            r |= Self::CTRL;
        }
        if self.has_shift() {
            r |= Self::SHIFT;
        }
        if self.has_logo() {
            r |= Self::LOGO;
        }
        r
    }

    /// Returns only the alt flags in `self`.
    pub fn into_alt(self) -> Self {
        self & Self::ALT
    }

    /// Returns only the control flags in `self`.
    pub fn into_ctrl(self) -> Self {
        self & Self::CTRL
    }

    /// Returns only the shift flags in `self`.
    pub fn into_shift(self) -> Self {
        self & Self::SHIFT
    }

    /// Returns only the logo flags in `self`.
    pub fn into_logo(self) -> Self {
        self & Self::LOGO
    }

    /// Modifier from `key`, returns empty if the key is not a modifier.
    pub fn from_key(key: Key) -> ModifiersState {
        match key {
            Key::LAlt => Self::L_ALT,
            Key::RAlt => Self::R_ALT,
            Key::LCtrl => Self::L_CTRL,
            Key::RCtrl => Self::R_CTRL,
            Key::LShift => Self::L_SHIFT,
            Key::RShift => Self::R_SHIFT,
            Key::LLogo => Self::L_LOGO,
            Key::RLogo => Self::R_LOGO,
            _ => Self::empty(),
        }
    }

    /// All keys that when pressed form the modifiers state.
    ///
    /// In case of multiple keys the order is `LOGO`, `CTRL`, `SHIFT`, `ALT`.
    ///
    /// In case both left and right keys are flagged for a modifier, the left key is used.
    pub fn keys(self) -> Vec<Key> {
        let mut r = vec![];

        if self.contains(Self::L_LOGO) {
            r.push(Key::LLogo);
        } else if self.contains(Self::R_LOGO) {
            r.push(Key::RLogo);
        }

        if self.contains(Self::L_CTRL) {
            r.push(Key::LCtrl);
        } else if self.contains(Self::R_CTRL) {
            r.push(Key::RCtrl);
        }

        if self.contains(Self::L_SHIFT) {
            r.push(Key::LShift);
        } else if self.contains(Self::R_SHIFT) {
            r.push(Key::RShift);
        }

        if self.contains(Self::L_ALT) {
            r.push(Key::LAlt);
        } else if self.contains(Self::R_ALT) {
            r.push(Key::RAlt);
        }

        r
    }
}
