//! Focus manager, events and services.

use crate::core::app::AppExtension;
use crate::core::context::*;
use crate::core::event::*;
use crate::core::keyboard::*;
use crate::core::mouse::*;
use crate::core::render::{FrameInfo, WidgetInfo, WidgetPath};
use crate::core::types::*;
use crate::core::window::{WindowIsActiveArgs, WindowIsActiveChanged, Windows};

event_args! {
    /// [`FocusChanged`](FocusChanged) event args.
    pub struct FocusChangedArgs {
        /// Previously focused widget.
        pub prev_focus: Option<WidgetPath>,

        /// Newly focused widget.
        pub new_focus: Option<WidgetPath>,

        ..

        /// If the widget is [prev_focus](FocusChangedArgs::prev_focus) or
        /// [`new_focus`](FocusChangedArgs::new_focus).
        fn concerns_widget(&self, ctx: &mut WidgetContext) -> bool {
            if let Some(prev) = &self.prev_focus {
                if prev.widget_id() == ctx.widget_id {
                    return true
                }
            }

            if let Some(new) = &self.new_focus {
                if new.widget_id() == ctx.widget_id {
                    return true
                }
            }

            false
        }
    }
}

impl FocusChangedArgs {
    /// If the focus is still in the same widget but the widget path changed.
    #[inline]
    pub fn is_widget_move(&self) -> bool {
        match (&self.prev_focus, &self.new_focus) {
            (Some(prev), Some(new)) => prev.widget_id() == new.widget_id(),
            _ => false,
        }
    }
}

state_key! {
    pub(crate) struct IsFocusable: bool;
    pub(crate) struct FocusTabIndex: TabIndex;
    pub(crate) struct IsFocusScope: bool;
    pub(crate) struct FocusTabNav: TabNav;
    pub(crate) struct FocusDirectionalNav: DirectionalNav;
}

/// Widget order index during TAB navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabIndex(u32);

impl TabIndex {
    /// Widget is skipped during TAB navigation.
    pub const SKIP: TabIndex = TabIndex(0);

    /// Widget is focused during TAB navigation using its order of declaration.
    pub const AUTO: TabIndex = TabIndex(u32::max_value());

    /// If is [`SKIP`](TabIndex::SKIP).
    #[inline]
    pub fn is_skip(self) -> bool {
        self == Self::SKIP
    }

    /// If is [`AUTO`](TabIndex::AUTO).
    #[inline]
    pub fn is_auto(self) -> bool {
        self == Self::AUTO
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabNav {
    None,
    Continue,
    Contained,
    Cycle,
    Once,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DirectionalNav {
    None,
    Continue,
    Contained,
    Cycle,
}

/// Focus changed event.
pub struct FocusChanged;
impl Event for FocusChanged {
    type Args = FocusChangedArgs;
}

/// Application extension that manages keyboard focus. Provides the [`FocusChanged`](FocusChanged) event
/// and [`Focus`](Focus) service.
pub struct FocusManager {
    focus_changed: EventEmitter<FocusChangedArgs>,
    windows_activation: EventListener<WindowIsActiveArgs>,
    mouse_down: EventListener<MouseInputArgs>,
    key_down: EventListener<KeyInputArgs>,
    focused: Option<WidgetPath>,
}
impl Default for FocusManager {
    fn default() -> Self {
        Self {
            focus_changed: EventEmitter::new(false),
            windows_activation: EventListener::never(false),
            mouse_down: EventListener::never(false),
            key_down: EventListener::never(false),
            focused: None,
        }
    }
}
impl AppExtension for FocusManager {
    fn init(&mut self, ctx: &mut AppInitContext) {
        self.windows_activation = ctx.events.listen::<WindowIsActiveChanged>();
        self.mouse_down = ctx.events.listen::<MouseDown>();
        self.key_down = ctx.events.listen::<KeyDown>();

        ctx.services.register(Focus::new(ctx.updates.notifier().clone()));

        ctx.events.register::<FocusChanged>(self.focus_changed.listener());
    }

    fn update(&mut self, update: UpdateRequest, ctx: &mut AppContext) {
        if update.update_hp {
            return;
        }

        let mut request = None;

        if let Some(req) = ctx.services.req::<Focus>().request.take() {
            // custom
            request = Some(req);
        } else if let Some(args) = self.mouse_down.updates(ctx.events).last() {
            // click
            // TODO: Check click path for focusable (clicking a button doesn't focus it if the click was on the text)
            request = Some(FocusRequest::DirectOrParent(args.target.widget_id()));
        } else if let Some(args) = self.key_down.updates(ctx.events).last() {
            // keyboard
            match &args.key {
                Some(VirtualKeyCode::Tab) => {
                    request = Some(if args.modifiers.shift() {
                        FocusRequest::Prev
                    } else {
                        FocusRequest::Next
                    })
                }
                Some(VirtualKeyCode::Up) => request = Some(FocusRequest::Up),
                Some(VirtualKeyCode::Down) => request = Some(FocusRequest::Down),
                Some(VirtualKeyCode::Left) => request = Some(FocusRequest::Left),
                Some(VirtualKeyCode::Right) => request = Some(FocusRequest::Right),
                _ => {}
            }
        }

        if let Some(request) = request {
            let (focus, windows) = ctx.services.req_multi::<(Focus, Windows)>();
            self.notify(focus.fulfill_request(request, windows), ctx);
        } else if self.windows_activation.has_updates(ctx.events) {
            // foreground window maybe changed
            let (focus, windows) = ctx.services.req_multi::<(Focus, Windows)>();
            self.notify(focus.continue_focus(windows), ctx);
        }
    }

    fn on_new_frame_ready(&mut self, window_id: WindowId, ctx: &mut AppContext) {
        if self.focused.as_ref().map(|f| f.window_id() == window_id).unwrap_or_default() {
            let (focus, windows) = ctx.services.req_multi::<(Focus, Windows)>();
            // new window frame, check if focus is still valid
            self.notify(focus.continue_focus(windows), ctx);
        }
    }
}
impl FocusManager {
    fn notify(&mut self, args: Option<FocusChangedArgs>, ctx: &mut AppContext) {
        if let Some(args) = args {
            self.focused = args.new_focus.clone();
            ctx.updates.push_notify(self.focus_changed.clone(), args);
        }
    }
}

/// Keyboard focus service.
pub struct Focus {
    request: Option<FocusRequest>,
    update_notifier: UpdateNotifier,
    focused: Option<WidgetPath>,
}

impl Focus {
    #[inline]
    pub fn new(update_notifier: UpdateNotifier) -> Self {
        Focus {
            request: None,
            update_notifier,
            focused: None,
        }
    }

    /// Current focused widget.
    #[inline]
    pub fn focused(&self) -> Option<&WidgetPath> {
        self.focused.as_ref()
    }

    #[inline]
    pub fn focus(&mut self, request: FocusRequest) {
        self.request = Some(request);
        self.update_notifier.push_update();
    }

    /// Focus the widget if it is focusable.
    #[inline]
    pub fn focus_widget(&mut self, widget_id: WidgetId) {
        self.focus(FocusRequest::Direct(widget_id))
    }

    /// Focus the widget if it is focusable, else focus the first focusable parent.
    #[inline]
    pub fn focus_widget_or_parent(&mut self, widget_id: WidgetId) {
        self.focus(FocusRequest::DirectOrParent(widget_id))
    }

    #[inline]
    pub fn focus_next(&mut self) {
        self.focus(FocusRequest::Next);
    }

    #[inline]
    pub fn focus_prev(&mut self) {
        self.focus(FocusRequest::Prev);
    }

    #[inline]
    pub fn focus_left(&mut self) {
        self.focus(FocusRequest::Left);
    }

    #[inline]
    pub fn focus_right(&mut self) {
        self.focus(FocusRequest::Right);
    }

    #[inline]
    pub fn focus_up(&mut self) {
        self.focus(FocusRequest::Up);
    }

    #[inline]
    pub fn focus_down(&mut self) {
        self.focus(FocusRequest::Down);
    }

    #[must_use]
    fn fulfill_request(&mut self, request: FocusRequest, windows: &Windows) -> Option<FocusChangedArgs> {
        match (&self.focused, request) {
            (_, FocusRequest::Direct(widget_id)) => self.focus_direct(widget_id, false, windows),
            (_, FocusRequest::DirectOrParent(widget_id)) => self.focus_direct(widget_id, true, windows),
            (Some(prev), move_) => {
                if let Ok(w) = windows.window(prev.window_id()) {
                    let frame = FrameFocusInfo::new(w.frame_info());
                    if let Some(w) = frame.find(prev.widget_id()) {
                        if let Some(new_focus) = match move_ {
                            FocusRequest::Next => w.next_focusable(),
                            FocusRequest::Prev => w.prev_focusable(),
                            FocusRequest::Left => None, //TODO
                            FocusRequest::Right => None,
                            FocusRequest::Up => None,
                            FocusRequest::Down => None,
                            FocusRequest::Direct(_) | FocusRequest::DirectOrParent(_) => unreachable!(),
                        } {
                            self.move_focus(Some(new_focus.info.path()))
                        } else {
                            // widget may have moved inside the same window.
                            self.continue_focus(windows)
                        }
                    } else {
                        // widget not found.
                        self.continue_focus(windows)
                    }
                } else {
                    // window not found
                    self.continue_focus(windows)
                }
            }
            _ => None,
        }
    }

    /// Checks if `focused()` is still valid, if not moves focus to nearest valid.
    #[must_use]
    fn continue_focus(&mut self, windows: &Windows) -> Option<FocusChangedArgs> {
        if let Some(focused) = &self.focused {
            if let Ok(window) = windows.window(focused.window_id()) {
                if window.is_active() {
                    if let Some(widget) = window.frame_info().find(focused.widget_id()).map(|w| w.as_focus_info()) {
                        if widget.is_focusable() {
                            // :-) probably in the same place, maybe moved inside same window.
                            self.move_focus(Some(widget.info.path()))
                        } else {
                            // widget no longer focusable
                            if let Some(parent) = widget.parent() {
                                // move to focusable parent
                                self.move_focus(Some(parent.info.path()))
                            } else {
                                // no focusable parent, is this an error?
                                self.move_focus(None)
                            }
                        }
                    } else {
                        // widget not found
                        self.continue_focus_moved_widget(windows)
                    }
                } else {
                    // window not active anymore
                    self.continue_focus_moved_widget(windows)
                }
            } else {
                // window not found
                self.continue_focus_moved_widget(windows)
            }
        } else {
            // no previous focus
            self.focus_active_window(windows)
        }
    }

    #[must_use]
    fn continue_focus_moved_widget(&mut self, windows: &Windows) -> Option<FocusChangedArgs> {
        let focused = self.focused.as_ref().unwrap();
        for window in windows.windows() {
            if let Some(widget) = window.frame_info().find(focused.widget_id()).map(|w| w.as_focus_info()) {
                // found the widget in another window
                if window.is_active() {
                    return if widget.is_focusable() {
                        // same widget, moved to another window
                        self.move_focus(Some(widget.info.path()))
                    } else {
                        // widget no longer focusable
                        if let Some(parent) = widget.parent() {
                            // move to focusable parent
                            self.move_focus(Some(parent.info.path()))
                        } else {
                            // no focusable parent, is this an error?
                            self.move_focus(None)
                        }
                    };
                }
                break;
            }
        }
        // did not find the widget in a focusable context, was removed or is inside an inactive window.
        self.focus_active_window(windows)
    }

    #[must_use]
    fn focus_direct(&mut self, widget_id: WidgetId, fallback_to_parents: bool, windows: &Windows) -> Option<FocusChangedArgs> {
        for w in windows.windows() {
            let frame = w.frame_info();
            if let Some(w) = frame.find(widget_id).map(|w| w.as_focus_info()) {
                if w.is_focusable() {
                    return self.move_focus(Some(w.info.path()));
                } else if fallback_to_parents {
                    if let Some(w) = w.parent() {
                        return self.move_focus(Some(w.info.path()));
                    } else {
                        // no focusable parent, just activate window?
                        //TODO
                    }
                }
                break;
            }
        }
        None
    }

    #[must_use]
    fn focus_active_window(&mut self, windows: &Windows) -> Option<FocusChangedArgs> {
        if let Some(active) = windows.windows().find(|w| w.is_active()) {
            let frame = FrameFocusInfo::new(active.frame_info());
            let root = frame.root();
            if root.is_focusable() {
                // found active window and it is focusable.
                self.move_focus(Some(root.info.path()))
            } else {
                // has active window but it is not focusable
                self.move_focus(None)
            }
        } else {
            // no active window
            self.move_focus(None)
        }
    }

    #[must_use]
    fn move_focus(&mut self, new_focus: Option<WidgetPath>) -> Option<FocusChangedArgs> {
        if self.focused != new_focus {
            let args = FocusChangedArgs::now(self.focused.take(), new_focus.clone());
            self.focused = new_focus;
            Some(args)
        } else {
            None
        }
    }
}

impl AppService for Focus {}

/// Focus change request.
#[derive(Clone, Copy, Debug)]
pub enum FocusRequest {
    /// Move focus to widget.
    Direct(WidgetId),
    /// Move focus to the widget if it is focusable or to a focusable parent.
    DirectOrParent(WidgetId),

    /// Move focus to next from current in screen, or to first in screen.
    Next,
    /// Move focus to previous from current in screen, or to last in screen.
    Prev,

    /// Move focus to the left of current.
    Left,
    /// Move focus to the right of current.
    Right,
    /// Move focus above current.
    Up,
    /// Move focus bellow current.
    Down,
}

pub struct FrameFocusInfo<'a> {
    /// Full frame info.
    pub info: &'a FrameInfo,
}

impl<'a> FrameFocusInfo<'a> {
    #[inline]
    pub fn new(frame_info: &'a FrameInfo) -> Self {
        FrameFocusInfo { info: frame_info }
    }

    /// Reference to the root widget in the frame.
    ///
    /// The root is usually a focusable focus scope but it may not be. This
    /// is the only method that returns a [`WidgetFocusInfo`](WidgetFocusInfo) that may not be focusable.
    #[inline]
    pub fn root(&self) -> WidgetFocusInfo {
        WidgetFocusInfo::new(self.info.root())
    }

    /// Reference to the widget in the frame, if it is present and has focus info.
    #[inline]
    pub fn find(&self, widget_id: WidgetId) -> Option<WidgetFocusInfo> {
        self.info.find(widget_id).and_then(|i| i.as_focusable())
    }

    /// If the frame info contains the widget and it is focusable.
    #[inline]
    pub fn contains(&self, widget_id: WidgetId) -> bool {
        self.find(widget_id).is_some()
    }
}

/// [`WidgetInfo`](WidgetInfo) extensions that build a [`WidgetFocusInfo`](WidgetFocusInfo).
pub trait WidgetInfoFocusExt<'a> {
    /// Wraps the [`WidgetInfo`](WidgetInfo) in a [`WidgetFocusInfo`](WidgetFocusInfo) even if it is not focusable.
    fn as_focus_info(self) -> WidgetFocusInfo<'a>;

    /// Returns a wrapped [`WidgetFocusInfo`](WidgetFocusInfo) if the [`WidgetInfo`](WidgetInfo) is focusable.
    fn as_focusable(self) -> Option<WidgetFocusInfo<'a>>;
}

impl<'a> WidgetInfoFocusExt<'a> for WidgetInfo<'a> {
    fn as_focus_info(self) -> WidgetFocusInfo<'a> {
        WidgetFocusInfo::new(self)
    }

    fn as_focusable(self) -> Option<WidgetFocusInfo<'a>> {
        let r = self.as_focus_info();
        if r.is_focusable() {
            Some(r)
        } else {
            None
        }
    }
}

/// [`WidgetInfo`](WidgetInfo) wrapper that uses focus metadata for computing navigation.
#[derive(Clone, Copy)]
pub struct WidgetFocusInfo<'a> {
    /// Full widget info.
    pub info: WidgetInfo<'a>,
}

impl<'a> WidgetFocusInfo<'a> {
    #[inline]
    pub fn new(widget_info: WidgetInfo<'a>) -> Self {
        WidgetFocusInfo { info: widget_info }
    }

    /// Root focusable.
    #[inline]
    pub fn root(self) -> Self {
        self.ancestors().last().unwrap_or(self)
    }

    #[inline]
    pub fn is_focusable(self) -> bool {
        self.focus_info().is_focusable()
    }

    /// Is focus scope.
    #[inline]
    pub fn is_scope(self) -> bool {
        self.focus_info().is_scope()
    }

    #[inline]
    pub fn focus_info(self) -> FocusInfo {
        let m = self.info.meta();
        match (
            m.get(IsFocusable).copied(),
            m.get(IsFocusScope).copied(),
            m.get(FocusTabIndex).copied(),
            m.get(FocusTabNav).copied(),
            m.get(FocusDirectionalNav).copied(),
        ) {
            // Set as not focusable.
            (Some(false), _, _, _, _) => FocusInfo::NotFocusable,

            // Set as focus scope and not set as not focusable
            // or set tab navigation and did not set as not focus scope
            // or set directional navigation and did not set as not focus scope.
            (_, Some(true), idx, tab, dir) | (_, None, idx, tab @ Some(_), dir) | (_, None, idx, tab, dir @ Some(_)) => {
                FocusInfo::FocusScope(
                    idx.unwrap_or(TabIndex::AUTO),
                    tab.unwrap_or(TabNav::Continue),
                    dir.unwrap_or(DirectionalNav::None),
                )
            }

            // Set as focusable and was not focus scope
            // or set tab index and was not focus scope and did not set as not focusable.
            (Some(true), _, idx, _, _) | (_, _, idx @ Some(_), _, _) => FocusInfo::Focusable(idx.unwrap_or(TabIndex::AUTO)),

            _ => FocusInfo::NotFocusable,
        }
    }

    /// Iterator over focusable parent -> grandparent -> .. -> root.
    #[inline]
    pub fn ancestors(self) -> impl Iterator<Item = WidgetFocusInfo<'a>> {
        self.info.ancestors().focusable()
    }

    /// Iterator over focus scopes parent -> grandparent -> .. -> root.
    #[inline]
    pub fn scopes(self) -> impl Iterator<Item = WidgetFocusInfo<'a>> {
        self.info.ancestors().filter_map(|i| {
            let i = i.as_focus_info();
            if i.is_scope() {
                Some(i)
            } else {
                None
            }
        })
    }

    /// Reference to the focusable parent that contains this widget.
    #[inline]
    pub fn parent(self) -> Option<WidgetFocusInfo<'a>> {
        self.ancestors().next()
    }

    /// Reference the focus scope parent that contains the widget.
    #[inline]
    pub fn scope(self) -> Option<WidgetFocusInfo<'a>> {
        let scope = self.scopes().next();
        if scope.is_some() {
            scope
        } else if self.is_scope() {
            Some(self)
        } else {
            None
        }
    }

    /// Iterator over the focusable widgets contained by this widget.
    #[inline]
    pub fn descendants(self) -> impl Iterator<Item = WidgetFocusInfo<'a>> {
        self.info.descendants().focusable()
    }

    /// Iterator over all focusable widgets in the same scope after this widget.
    #[inline]
    pub fn next_focusables(self) -> impl Iterator<Item = WidgetFocusInfo<'a>> {
        let self_id = self.info.widget_id();
        self.scope()
            .into_iter()
            .flat_map(|s| s.descendants())
            .skip_while(move |f| f.info.widget_id() != self_id)
            .skip(1)
    }

    /// Next focusable in the same scope after this widget.
    #[inline]
    pub fn next_focusable(self) -> Option<WidgetFocusInfo<'a>> {
        self.next_focusables().next()
    }

    /// Iterator over all focusable widgets in the same scope before this widget in reverse.
    #[inline]
    pub fn prev_focusables(self) -> impl Iterator<Item = WidgetFocusInfo<'a>> {
        let self_id = self.info.widget_id();

        let mut prev: Vec<_> = self
            .scope()
            .into_iter()
            .flat_map(|s| s.descendants())
            .take_while(move |f| f.info.widget_id() != self_id)
            .collect();

        prev.reverse();

        prev.into_iter()
    }

    /// Previous focusable in the same scope after this widget.
    #[inline]
    pub fn prev_focusable(self) -> Option<WidgetFocusInfo<'a>> {
        let self_id = self.info.widget_id();

        self.scope()
            .and_then(move |s| s.descendants().take_while(move |f| f.info.widget_id() != self_id).last())
    }

    /// Widget to focus when pressing TAB from this widget.
    #[inline]
    pub fn tab(self) -> WidgetFocusInfo<'a> {
        todo!()
    }

    /// Widget to focus when pressing CTRL+TAB from this widget.
    #[inline]
    pub fn ctrl_tab(self) -> WidgetFocusInfo<'a> {
        todo!()
    }
}

/// Filter-maps an iterator of [`WidgetInfo`](WidgetInfo) to [`WidgetFocusInfo`](WidgetFocusInfo).
pub trait IterFocusable<'a, I: Iterator<Item = WidgetInfo<'a>>> {
    fn focusable(self) -> std::iter::FilterMap<I, fn(WidgetInfo<'a>) -> Option<WidgetFocusInfo<'a>>>;
}

impl<'a, I: Iterator<Item = WidgetInfo<'a>>> IterFocusable<'a, I> for I {
    fn focusable(self) -> std::iter::FilterMap<I, fn(WidgetInfo<'a>) -> Option<WidgetFocusInfo<'a>>> {
        self.filter_map(|i| i.as_focusable())
    }
}

#[derive(Debug, Clone, Copy)]
pub enum FocusInfo {
    NotFocusable,
    Focusable(TabIndex),
    FocusScope(TabIndex, TabNav, DirectionalNav),
}

impl FocusInfo {
    #[inline]
    pub fn is_focusable(self) -> bool {
        match self {
            FocusInfo::NotFocusable => false,
            _ => true,
        }
    }

    #[inline]
    pub fn is_scope(self) -> bool {
        match self {
            FocusInfo::FocusScope(..) => true,
            _ => false,
        }
    }
}
