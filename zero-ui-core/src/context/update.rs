use std::{
    fmt, mem,
    ops::{Deref, DerefMut},
};

use linear_map::set::LinearSet;

use crate::{
    app::AppEventSender,
    crate_util::{Handle, HandleOwner, IdSet, WeakHandle},
    event::EventUpdate,
    handler::{AppHandler, AppHandlerArgs, AppWeakHandle},
    widget_info::{WidgetInfo, WidgetInfoTree, WidgetPath},
    widget_instance::WidgetId,
    window::WindowId,
};

use super::{AppContext, UpdatesTrace};

/// Represents an [`on_pre_update`](Updates::on_pre_update) or [`on_update`](Updates::on_update) handler.
///
/// Drop all clones of this handle to drop the binding, or call [`perm`](Self::perm) to drop the handle
/// but keep the handler alive for the duration of the app.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[repr(transparent)]
#[must_use = "dropping the handle unsubscribes update handler"]
pub struct OnUpdateHandle(Handle<()>);
impl OnUpdateHandle {
    fn new() -> (HandleOwner<()>, OnUpdateHandle) {
        let (owner, handle) = Handle::new(());
        (owner, OnUpdateHandle(handle))
    }

    /// Create a handle to nothing, the handle always in the *unsubscribed* state.
    ///
    /// Note that `Option<OnUpdateHandle>` takes up the same space as `OnUpdateHandle` and avoids an allocation.
    pub fn dummy() -> Self {
        assert_non_null!(OnUpdateHandle);
        OnUpdateHandle(Handle::dummy(()))
    }

    /// Drop the handle but does **not** unsubscribe.
    ///
    /// The handler stays in memory for the duration of the app or until another handle calls [`unsubscribe`](Self::unsubscribe.)
    pub fn perm(self) {
        self.0.perm();
    }

    /// If another handle has called [`perm`](Self::perm).
    /// If `true` the var binding will stay active until the app exits, unless [`unsubscribe`](Self::unsubscribe) is called.
    pub fn is_permanent(&self) -> bool {
        self.0.is_permanent()
    }

    /// Drops the handle and forces the handler to drop.
    pub fn unsubscribe(self) {
        self.0.force_drop()
    }

    /// If another handle has called [`unsubscribe`](Self::unsubscribe).
    ///
    /// The handler is already dropped or will be dropped in the next app update, this is irreversible.
    pub fn is_unsubscribed(&self) -> bool {
        self.0.is_dropped()
    }

    /// Create a weak handle.
    pub fn downgrade(&self) -> WeakOnUpdateHandle {
        WeakOnUpdateHandle(self.0.downgrade())
    }
}

/// Weak [`OnUpdateHandle`].
#[derive(Clone, PartialEq, Eq, Hash, Default, Debug)]
pub struct WeakOnUpdateHandle(WeakHandle<()>);
impl WeakOnUpdateHandle {
    /// New weak handle that does not upgrade.
    pub fn new() -> Self {
        Self(WeakHandle::new())
    }

    /// Gets the strong handle if it is still subscribed.
    pub fn upgrade(&self) -> Option<OnUpdateHandle> {
        self.0.upgrade().map(OnUpdateHandle)
    }
}

struct UpdateHandler {
    handle: HandleOwner<()>,
    count: usize,
    handler: Box<dyn FnMut(&mut AppContext, &UpdateArgs, &dyn AppWeakHandle)>,
}

/// Arguments for an [`on_pre_update`](Updates::on_pre_update), [`on_update`](Updates::on_update) or [`run`](Updates::run) handler.
#[derive(Debug, Clone, Copy)]
pub struct UpdateArgs {
    /// Number of times the handler was called.
    pub count: usize,
}

/// Schedule of actions to apply after an update.
///
/// An instance of this struct is available in [`AppContext`] and derived contexts.
pub struct Updates {
    event_sender: AppEventSender,
    update: bool,
    reinit: bool,
    update_widgets: UpdateDeliveryList,
    layout: bool,
    l_updates: LayoutUpdates,

    pre_handlers: Vec<UpdateHandler>,
    pos_handlers: Vec<UpdateHandler>,
}
impl Updates {
    pub(super) fn new(event_sender: AppEventSender) -> Self {
        Updates {
            event_sender,
            update: false,
            reinit: false,
            update_widgets: UpdateDeliveryList::new_any(),
            layout: false,
            l_updates: LayoutUpdates {
                render: false,
                window_updates: InfoLayoutRenderUpdates::default(),
            },

            pre_handlers: vec![],
            pos_handlers: vec![],
        }
    }

    /// Create an [`AppEventSender`] that can be used to awake the app and send app events.
    pub fn sender(&self) -> AppEventSender {
        self.event_sender.clone()
    }

    /// Schedules an update.
    pub fn update(&mut self, target: impl Into<Option<WidgetId>>) {
        UpdatesTrace::log_update();
        self.update_internal(target.into());
    }

    /// Flags the current widget to deinit and init.
    ///
    /// Note that this is not a scheduled update, the widget will reinit as soon as the execution returns to it.
    pub fn reinit(&mut self) {
        self.reinit = true;
    }

    /// If the current widget will deinit and init as soon as the execution returns to it.
    pub fn reinit_flagged(&self) -> bool {
        self.reinit
    }

    pub(crate) fn update_internal(&mut self, target: Option<WidgetId>) {
        self.update = true;
        if let Some(id) = target {
            self.update_widgets.search_widget(id);
        }
    }

    pub(crate) fn recv_update_internal(&mut self, targets: Vec<WidgetId>) {
        self.update = true;
        for id in targets {
            self.update_widgets.search_widget(id);
        }
    }

    /// Schedules an update that only affects the app extensions.
    ///
    /// This is the equivalent of calling [`update`] with an empty vec.
    ///
    /// [`update`]: Self::update
    pub fn update_ext(&mut self) {
        UpdatesTrace::log_update();
        self.update_ext_internal();
    }

    pub(crate) fn update_ext_internal(&mut self) {
        self.update = true;
    }

    /// Gets `true` if an update was requested.
    pub(crate) fn update_requested(&self) -> bool {
        self.update
    }

    /// Schedules a info tree rebuild, layout and render.
    pub fn info_layout_render(&mut self) {
        self.info();
        self.layout();
        self.render();
    }

    /// Schedules a layout and render update.
    pub fn layout_render(&mut self) {
        self.layout();
        self.render();
    }

    /// Schedules a layout update for the parent window.
    pub fn layout(&mut self) {
        UpdatesTrace::log_layout();
        self.layout = true;
        self.l_updates.window_updates.layout = true;
    }

    /// Gets `true` if a layout update is scheduled.
    pub(crate) fn layout_requested(&self) -> bool {
        self.layout
    }

    /// Flags a widget tree info rebuild and subscriptions aggregation for the parent window.
    ///
    /// The window will call [`UiNode::info`] as soon as the current UI node method finishes,
    /// requests outside windows are ignored.
    ///
    /// [`UiNode::info`]: crate::widget_instance::UiNode::info
    pub fn info(&mut self) {
        // tracing::trace!("requested `info`");
        self.l_updates.window_updates.info = true;
    }

    /// Schedules a new full frame for the parent window.
    pub fn render(&mut self) {
        // tracing::trace!("requested `render`");
        self.l_updates.render();
    }

    /// Returns `true` if a new frame or frame update is scheduled.
    pub(crate) fn render_requested(&self) -> bool {
        self.l_updates.render_requested()
    }

    /// Schedule a frame update for the parent window.
    ///
    /// Note that if another widget requests a full [`render`] this update will not run.
    ///
    /// [`render`]: Updates::render
    pub fn render_update(&mut self) {
        // tracing::trace!("requested `render_update`");
        self.l_updates.render_update();
    }

    /// Schedule an *once* handler to run when these updates are applied.
    ///
    /// The callback is any of the *once* [`AppHandler`], including async handlers. If the handler is async and does not finish in
    /// one call it is scheduled to update in *preview* updates.
    pub fn run<H: AppHandler<UpdateArgs>>(&mut self, handler: H) -> OnUpdateHandle {
        self.update = true; // in case of this was called outside of an update.
        Self::push_handler(&mut self.pos_handlers, true, handler, true)
    }

    /// Create a preview update handler.
    ///
    /// The `handler` is called every time the app updates, just before the UI updates. It can be any of the non-async [`AppHandler`],
    /// use the [`app_hn!`] or [`app_hn_once!`] macros to declare the closure. You must avoid using async handlers because UI bound async
    /// tasks cause app updates to awake, so it is very easy to lock the app in a constant sequence of updates. You can use [`run`](Self::run)
    /// to start an async app context task.
    ///
    /// Returns an [`OnUpdateHandle`] that can be used to unsubscribe, you can also unsubscribe from inside the handler by calling
    /// [`unsubscribe`](crate::handler::AppWeakHandle::unsubscribe) in the third parameter of [`app_hn!`] or [`async_app_hn!`].
    ///
    /// [`app_hn_once!`]: macro@crate::handler::app_hn_once
    /// [`app_hn!`]: macro@crate::handler::app_hn
    /// [`async_app_hn!`]: macro@crate::handler::async_app_hn
    pub fn on_pre_update<H>(&mut self, handler: H) -> OnUpdateHandle
    where
        H: AppHandler<UpdateArgs>,
    {
        Self::push_handler(&mut self.pre_handlers, true, handler, false)
    }

    /// Create an update handler.
    ///
    /// The `handler` is called every time the app updates, just after the UI updates. It can be any of the non-async [`AppHandler`],
    /// use the [`app_hn!`] or [`app_hn_once!`] macros to declare the closure. You must avoid using async handlers because UI bound async
    /// tasks cause app updates to awake, so it is very easy to lock the app in a constant sequence of updates. You can use [`run`](Self::run)
    /// to start an async app context task.
    ///
    /// Returns an [`OnUpdateHandle`] that can be used to unsubscribe, you can also unsubscribe from inside the handler by calling
    /// [`unsubscribe`](crate::handler::AppWeakHandle::unsubscribe) in the third parameter of [`app_hn!`] or [`async_app_hn!`].
    ///
    /// [`app_hn!`]: macro@crate::handler::app_hn
    /// [`async_app_hn!`]: macro@crate::handler::async_app_hn
    pub fn on_update<H>(&mut self, handler: H) -> OnUpdateHandle
    where
        H: AppHandler<UpdateArgs>,
    {
        Self::push_handler(&mut self.pos_handlers, false, handler, false)
    }

    fn push_handler<H>(entries: &mut Vec<UpdateHandler>, is_preview: bool, mut handler: H, force_once: bool) -> OnUpdateHandle
    where
        H: AppHandler<UpdateArgs>,
    {
        let (handle_owner, handle) = OnUpdateHandle::new();
        entries.push(UpdateHandler {
            handle: handle_owner,
            count: 0,
            handler: Box::new(move |ctx, args, handle| {
                let handler_args = AppHandlerArgs { handle, is_preview };
                handler.event(ctx, args, &handler_args);
                if force_once {
                    handler_args.handle.unsubscribe();
                }
            }),
        });
        handle
    }

    pub(crate) fn on_pre_updates(ctx: &mut AppContext) {
        let mut handlers = mem::take(&mut ctx.updates.pre_handlers);
        Self::retain_updates(ctx, &mut handlers);
        handlers.append(&mut ctx.updates.pre_handlers);
        ctx.updates.pre_handlers = handlers;
    }

    pub(crate) fn on_updates(ctx: &mut AppContext) {
        let mut handlers = mem::take(&mut ctx.updates.pos_handlers);
        Self::retain_updates(ctx, &mut handlers);
        handlers.append(&mut ctx.updates.pos_handlers);
        ctx.updates.pos_handlers = handlers;
    }

    fn retain_updates(ctx: &mut AppContext, handlers: &mut Vec<UpdateHandler>) {
        handlers.retain_mut(|e| {
            !e.handle.is_dropped() && {
                e.count = e.count.wrapping_add(1);
                (e.handler)(ctx, &UpdateArgs { count: e.count }, &e.handle.weak_handle());
                !e.handle.is_dropped()
            }
        });
    }

    pub(super) fn enter_window_ctx(&mut self) {
        self.l_updates.window_updates = InfoLayoutRenderUpdates::default();
    }
    pub(super) fn exit_window_ctx(&mut self) -> InfoLayoutRenderUpdates {
        mem::take(&mut self.l_updates.window_updates)
    }

    pub(super) fn enter_widget_ctx(&mut self) -> InfoLayoutRenderUpdates {
        mem::take(&mut self.l_updates.window_updates)
    }
    pub(super) fn exit_widget_ctx(&mut self, mut prev: InfoLayoutRenderUpdates) -> (InfoLayoutRenderUpdates, bool) {
        prev |= self.l_updates.window_updates;
        (mem::replace(&mut self.l_updates.window_updates, prev), mem::take(&mut self.reinit))
    }

    pub(super) fn take_updates(&mut self) -> (bool, WidgetUpdates, bool, bool) {
        (
            mem::take(&mut self.update),
            WidgetUpdates {
                delivery_list: mem::take(&mut self.update_widgets),
            },
            mem::take(&mut self.layout),
            mem::take(&mut self.l_updates.render),
        )
    }
}
/// crate::app::HeadlessApp::block_on
impl Updates {
    pub(crate) fn handler_lens(&self) -> (usize, usize) {
        (self.pre_handlers.len(), self.pos_handlers.len())
    }
    pub(crate) fn new_update_handlers(&self, pre_from: usize, pos_from: usize) -> Vec<Box<dyn Fn() -> bool>> {
        self.pre_handlers
            .iter()
            .skip(pre_from)
            .chain(self.pos_handlers.iter().skip(pos_from))
            .map(|h| h.handle.weak_handle())
            .map(|h| {
                let r: Box<dyn Fn() -> bool> = Box::new(move || h.upgrade().is_some());
                r
            })
            .collect()
    }
}
impl Deref for Updates {
    type Target = LayoutUpdates;

    fn deref(&self) -> &Self::Target {
        &self.l_updates
    }
}
impl DerefMut for Updates {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.l_updates
    }
}

/// Subsect of [`Updates`] that is accessible in [`LayoutContext`].
///
/// [`LayoutContext`]: crate::context::LayoutContext
pub struct LayoutUpdates {
    render: bool,
    window_updates: InfoLayoutRenderUpdates,
}
impl LayoutUpdates {
    /// Schedules a new frame for the parent window.
    pub fn render(&mut self) {
        self.render = true;
        self.window_updates.render = WindowRenderUpdate::Render;
    }

    /// Schedule a frame update for the parent window.
    ///
    /// Note that if another widget requests a full [`render`] this update will not run.
    ///
    /// [`render`]: LayoutUpdates::render
    pub fn render_update(&mut self) {
        self.render = true;
        self.window_updates.render |= WindowRenderUpdate::RenderUpdate;
    }

    /// Returns `true` if a new frame or frame update is scheduled.
    pub(crate) fn render_requested(&self) -> bool {
        self.render
    }

    pub(super) fn enter_widget_ctx(&mut self) -> InfoLayoutRenderUpdates {
        mem::take(&mut self.window_updates)
    }
    pub(super) fn exit_widget_ctx(&mut self, mut prev: InfoLayoutRenderUpdates) -> InfoLayoutRenderUpdates {
        prev |= self.window_updates;
        mem::replace(&mut self.window_updates, prev)
    }
}

/// Represents a type that can provide access to [`Updates`] inside the window of function call.
///
/// This is implemented to all sync and async context types and [`Updates`] it-self.
pub trait WithUpdates {
    /// Calls `action` with the [`Updates`] reference.
    fn with_updates<R, A: FnOnce(&mut Updates) -> R>(&mut self, action: A) -> R;
}
impl WithUpdates for Updates {
    fn with_updates<R, A: FnOnce(&mut Updates) -> R>(&mut self, action: A) -> R {
        action(self)
    }
}
impl<'a> WithUpdates for crate::context::AppContext<'a> {
    fn with_updates<R, A: FnOnce(&mut Updates) -> R>(&mut self, action: A) -> R {
        action(self.updates)
    }
}
impl<'a> WithUpdates for crate::context::WindowContext<'a> {
    fn with_updates<R, A: FnOnce(&mut Updates) -> R>(&mut self, action: A) -> R {
        action(self.updates)
    }
}
impl<'a> WithUpdates for crate::context::WidgetContext<'a> {
    fn with_updates<R, A: FnOnce(&mut Updates) -> R>(&mut self, action: A) -> R {
        action(self.updates)
    }
}
impl WithUpdates for crate::context::AppContextMut {
    fn with_updates<R, A: FnOnce(&mut Updates) -> R>(&mut self, action: A) -> R {
        self.with(move |ctx| action(ctx.updates))
    }
}
impl WithUpdates for crate::context::WidgetContextMut {
    fn with_updates<R, A: FnOnce(&mut Updates) -> R>(&mut self, action: A) -> R {
        self.with(move |ctx| action(ctx.updates))
    }
}
#[cfg(any(test, doc, feature = "test_util"))]
impl WithUpdates for crate::context::TestWidgetContext {
    fn with_updates<R, A: FnOnce(&mut Updates) -> R>(&mut self, action: A) -> R {
        action(&mut self.updates)
    }
}
impl WithUpdates for crate::app::HeadlessApp {
    fn with_updates<R, A: FnOnce(&mut Updates) -> R>(&mut self, action: A) -> R {
        action(self.ctx().updates)
    }
}

/// Widget updates of the current cycle.
#[derive(Debug, Default)]
pub struct WidgetUpdates {
    delivery_list: UpdateDeliveryList,
}
impl WidgetUpdates {
    /// New with list.
    pub fn new(delivery_list: UpdateDeliveryList) -> Self {
        Self { delivery_list }
    }

    /// Updates delivery list.
    pub fn delivery_list(&self) -> &UpdateDeliveryList {
        &self.delivery_list
    }

    /// Find all targets.
    ///
    /// This must be called before the first window visit, see [`UpdateDeliveryList::fulfill_search`] for details.
    pub fn fulfill_search<'a, 'b>(&'a mut self, windows: impl Iterator<Item = &'b WidgetInfoTree>) {
        self.delivery_list.fulfill_search(windows)
    }

    /// Calls `handle` if the event targets the window.
    pub fn with_window<H, R>(&mut self, ctx: &mut super::WindowContext, handle: H) -> Option<R>
    where
        H: FnOnce(&mut super::WindowContext, &mut Self) -> R,
    {
        if self.delivery_list.enter_window(*ctx.window_id) {
            Some(handle(ctx, self))
        } else {
            None
        }
    }

    /// Calls `handle` if the event targets the widget.
    pub fn with_widget<H, R>(&mut self, ctx: &mut super::WidgetContext, handle: H) -> Option<R>
    where
        H: FnOnce(&mut super::WidgetContext, &mut Self) -> R,
    {
        if self.delivery_list.enter_widget(ctx.path.widget_id()) {
            Some(handle(ctx, self))
        } else {
            None
        }
    }

    /// Copy all delivery from `other` onto `self`.
    pub fn extend(&mut self, other: WidgetUpdates) {
        self.delivery_list.extend_unchecked(other.delivery_list)
    }
}

/// Updates that must be reacted by an app context owner.
#[derive(Debug, Default)]
pub struct ContextUpdates {
    /// Events to notify.
    ///
    /// When this is not empty [`update`](Self::update) is `true`.
    pub events: Vec<EventUpdate>,

    /// Update requested.
    ///
    /// When this is `true`, [`update`](Self::update) may contain widgets, if not then only
    /// app extensions must update.
    pub update: bool,

    /// Update targets.
    ///
    /// When this is not empty [`update`](Self::update) is `true`.
    pub update_widgets: WidgetUpdates,

    /// Layout requested.
    pub layout: bool,

    /// Full frame or frame update requested.
    pub render: bool,
}
impl ContextUpdates {
    /// If has events, update, layout or render was requested.
    pub fn has_updates(&self) -> bool {
        self.update || self.layout || self.render
    }
}
impl std::ops::BitOrAssign for ContextUpdates {
    fn bitor_assign(&mut self, rhs: Self) {
        self.events.extend(rhs.events);
        self.update |= rhs.update;
        self.update_widgets.extend(rhs.update_widgets);
        self.layout |= rhs.layout;
        self.render |= rhs.render;
    }
}
impl std::ops::BitOr for ContextUpdates {
    type Output = Self;

    fn bitor(mut self, rhs: Self) -> Self {
        self |= rhs;
        self
    }
}

/// Info, Layout or render updates that where requested by the content of a window.
///
/// Unlike the general updates, layout and render can be optimized to only apply if
/// the window content requested it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct InfoLayoutRenderUpdates {
    /// Info tree rebuild requested.
    ///
    /// Windows should call [`UiNode::info`] to rebuild the info tree as soon as they receive this flag.
    ///
    /// [`UiNode::info`]: crate::widget_instance::UiNode::info
    pub info: bool,

    /// Layout requested.
    pub layout: bool,
    /// Full frame or frame update requested.
    pub render: WindowRenderUpdate,
}
impl InfoLayoutRenderUpdates {
    /// No updates, this the default value.
    pub fn none() -> Self {
        Self::default()
    }

    /// Update layout and render frame.
    pub fn all() -> Self {
        InfoLayoutRenderUpdates {
            info: true,
            layout: true,
            render: WindowRenderUpdate::Render,
        }
    }

    /// Info tree rebuild and subscriptions only.
    pub fn info() -> Self {
        InfoLayoutRenderUpdates {
            info: true,
            layout: false,
            render: WindowRenderUpdate::None,
        }
    }

    /// Update layout only.
    pub fn layout() -> Self {
        InfoLayoutRenderUpdates {
            info: false,
            layout: true,
            render: WindowRenderUpdate::None,
        }
    }

    /// Update render only.
    pub fn render() -> Self {
        InfoLayoutRenderUpdates {
            info: false,
            layout: false,
            render: WindowRenderUpdate::Render,
        }
    }

    /// Update render-update only.
    pub fn render_update() -> Self {
        InfoLayoutRenderUpdates {
            info: false,
            layout: false,
            render: WindowRenderUpdate::RenderUpdate,
        }
    }

    /// Returns if `self` is not equal to [`none`].
    ///
    /// [`none`]: Self::none
    pub fn is_any(self) -> bool {
        self != Self::none()
    }

    /// Returns if `self` is equal to [`none`].
    ///
    /// [`none`]: Self::none
    pub fn is_none(self) -> bool {
        self == Self::none()
    }
}
impl std::ops::BitOrAssign for InfoLayoutRenderUpdates {
    fn bitor_assign(&mut self, rhs: Self) {
        self.info |= rhs.info;
        self.layout |= rhs.layout;
        self.render |= rhs.render;
    }
}
impl std::ops::BitOr for InfoLayoutRenderUpdates {
    type Output = Self;

    fn bitor(mut self, rhs: Self) -> Self {
        self |= rhs;
        self
    }
}

/// Kind of render updated requested by the content of a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowRenderUpdate {
    /// No render update requested.
    None,
    /// Full frame requested.
    Render,
    /// Only frame update requested.
    RenderUpdate,
}
impl WindowRenderUpdate {
    /// If full frame was requested.
    pub fn is_render(self) -> bool {
        matches!(self, Self::Render)
    }

    /// If only frame update was requested.
    pub fn is_render_update(self) -> bool {
        matches!(self, Self::RenderUpdate)
    }

    /// If no render was requested.
    pub fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    /// Returns a copy of `self` and replaces `self` with `None`
    pub fn take(&mut self) -> Self {
        mem::take(self)
    }
}
impl Default for WindowRenderUpdate {
    fn default() -> Self {
        WindowRenderUpdate::None
    }
}
impl std::ops::BitOrAssign for WindowRenderUpdate {
    fn bitor_assign(&mut self, rhs: Self) {
        use WindowRenderUpdate::*;
        *self = match (*self, rhs) {
            (Render, _) | (_, Render) => Render,
            (RenderUpdate, _) | (_, RenderUpdate) => RenderUpdate,
            _ => None,
        };
    }
}
impl std::ops::BitOr for WindowRenderUpdate {
    type Output = Self;

    fn bitor(mut self, rhs: Self) -> Self {
        self |= rhs;
        self
    }
}

/// Represents all the widgets and windows on route to an update target.
pub struct UpdateDeliveryList {
    subscribers: Box<dyn UpdateSubscribers>,

    windows: LinearSet<WindowId>,
    widgets: IdSet<WidgetId>,
    search: IdSet<WidgetId>,
}
impl fmt::Debug for UpdateDeliveryList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdateDeliveryList")
            .field("windows", &self.windows)
            .field("widgets", &self.widgets)
            .field("search", &self.search)
            .finish_non_exhaustive()
    }
}
impl Default for UpdateDeliveryList {
    fn default() -> Self {
        Self::new_any()
    }
}
impl UpdateDeliveryList {
    /// New list that only allows `subscribers`.
    pub fn new(subscribers: Box<dyn UpdateSubscribers>) -> Self {
        Self {
            subscribers,
            windows: LinearSet::default(),
            widgets: IdSet::default(),
            search: IdSet::default(),
        }
    }

    /// New list that does not allow any entry.
    pub fn new_none() -> Self {
        struct UpdateDeliveryListNone;
        impl UpdateSubscribers for UpdateDeliveryListNone {
            fn contains(&self, _: WidgetId) -> bool {
                false
            }
            fn to_set(&self) -> IdSet<WidgetId> {
                IdSet::default()
            }
        }
        Self::new(Box::new(UpdateDeliveryListNone))
    }

    /// New list that allows all entries.
    ///
    /// This is the default value.
    pub fn new_any() -> Self {
        struct UpdateDeliveryListAny;
        impl UpdateSubscribers for UpdateDeliveryListAny {
            fn contains(&self, _: WidgetId) -> bool {
                true
            }
            fn to_set(&self) -> IdSet<WidgetId> {
                IdSet::default()
            }
        }
        Self::new(Box::new(UpdateDeliveryListAny))
    }

    /// Insert the widgets in the `path` up-to the inner most that is included in the subscribers.
    pub fn insert_path(&mut self, path: &WidgetPath) {
        if let Some(i) = path.widgets_path().iter().rposition(|w| self.subscribers.contains(*w)) {
            self.windows.insert(path.window_id());
            for w in &path.widgets_path()[..=i] {
                self.widgets.insert(*w);
            }
        }
    }

    /// Insert the ancestors of `wgt` and `wgt` up-to the inner most that is included in the subscribers.
    pub fn insert_wgt(&mut self, wgt: WidgetInfo) {
        let mut any = false;
        for w in wgt.self_and_ancestors() {
            if any || self.subscribers.contains(w.widget_id()) {
                any = true;
                self.widgets.insert(w.widget_id());
            }
        }
        if any {
            self.windows.insert(wgt.tree().window_id());
        }
    }

    /// Register all subscribers for search and delivery.
    pub fn search_all(&mut self) {
        self.search = self.subscribers.to_set();
    }

    /// Register the widget of unknown location for search before delivery routing starts.
    pub fn search_widget(&mut self, widget_id: WidgetId) {
        if self.subscribers.contains(widget_id) {
            self.search.insert(widget_id);
        }
    }

    /// If the the list has pending widgets that must be found before delivery can start.
    pub fn has_pending_search(&self) -> bool {
        !self.search.is_empty()
    }

    /// Search all pending widgets in all `windows`, all search items are cleared, even if not found.
    pub fn fulfill_search<'a, 'b>(&'a mut self, windows: impl Iterator<Item = &'b WidgetInfoTree>) {
        for window in windows {
            self.search.retain(|w| {
                if let Some(w) = window.get(*w) {
                    for w in w.self_and_ancestors() {
                        self.widgets.insert(w.widget_id());
                    }
                    self.windows.insert(window.window_id());
                    false
                } else {
                    true
                }
            });
        }
        self.search.clear();
    }

    /// Returns `true` if the window is on the list.
    ///
    /// The window is removed from the list.
    pub fn enter_window(&mut self, window_id: WindowId) -> bool {
        self.windows.remove(&window_id)
    }

    /// Returns `true` if the widget is on the list.
    ///
    /// The widget is removed from the list.
    pub fn enter_widget(&mut self, widget_id: WidgetId) -> bool {
        self.widgets.remove(&widget_id)
    }

    /// Returns `true` if has entered all widgets on the list.
    pub fn is_done(&self) -> bool {
        self.widgets.is_empty()
    }

    /// Copy windows, widgets and search from `other`, trusting that all values are allowed.
    fn extend_unchecked(&mut self, other: UpdateDeliveryList) {
        self.windows.extend(other.windows);
        self.widgets.extend(other.widgets);
        self.search.extend(other.search)
    }

    pub(crate) fn clear(&mut self) {
        self.widgets.clear();
        self.windows.clear();
        self.search.clear();
    }

    /// Windows in the delivery list.
    ///
    /// Note that each window that is visited is removed from the list.
    pub fn windows(&self) -> &LinearSet<WindowId> {
        &self.windows
    }

    /// Found widgets in the delivery list, can be targets of ancestors of targets.
    ///
    /// Note that each widget that is visited is removed from the list.
    pub fn widgets(&self) -> &IdSet<WidgetId> {
        &self.widgets
    }

    /// Not found target widgets.
    ///
    /// Each window searches for these widgets and adds then to the [`widgets`] list.
    ///
    /// [`widgets`]: Self::widgets
    pub fn search_widgets(&self) -> &IdSet<WidgetId> {
        &self.search
    }
}

/// Represents a set of widgets that subscribe to an event source.
pub trait UpdateSubscribers: 'static {
    /// Returns `true` if the widget is one of the subscribers.
    fn contains(&self, widget_id: WidgetId) -> bool;

    /// Gets all subscribers as a set.
    fn to_set(&self) -> IdSet<WidgetId>;
}
