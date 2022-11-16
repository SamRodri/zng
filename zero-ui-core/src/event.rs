//! App event and commands API.

use std::{
    any::Any,
    cell::{Cell, RefCell},
    fmt,
    marker::PhantomData,
    mem,
    ops::Deref,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::LocalKey,
    time::Instant,
};

use crate::{
    app::AppProcess,
    clone_move,
    context::{AppContext, UpdateDeliveryList, UpdateSubscribers, WidgetContext, WindowContext},
    crate_util::{IdMap, IdSet},
    handler::{AppHandler, AppHandlerArgs},
    widget_info::WidgetInfoTree,
    widget_instance::WidgetId,
};

mod args;
pub use args::*;

mod command;
pub use command::*;

mod events;
pub use events::*;

mod channel;
pub use channel::*;

mod properties;
use parking_lot::Mutex;
pub use properties::*;

///<span data-del-macro-root></span> Declares new [`Event<A>`] keys.
///
/// Event keys usually represent external events or [`AppExtension`] events, you can also use [`command!`]
/// to declare events specialized for commanding widgets and services.
///
/// [`AppExtension`]: crate::app::AppExtension
///
/// # Examples
///
/// The example defines two events with the same arguments type.
///
/// ```
/// # use zero_ui_core::event::event;
/// # use zero_ui_core::gesture::ClickArgs;
/// event! {
///     /// Event docs.
///     pub static CLICK_EVENT: ClickArgs;
///
///     /// Other event docs.
///     pub static DOUBLE_CLICK_EVENT: ClickArgs;
/// }
/// ```
///
/// # Properties
///
/// If the event targets widgets you can use [`event_property!`] to declare properties that setup event handlers for the event.
///
/// # Naming Convention
///
/// It is recommended that the type name ends with the `_VAR` suffix.
#[macro_export]
macro_rules! event_macro {
    ($(
        $(#[$attr:meta])*
        $vis:vis static $EVENT:ident: $Args:path;
    )+) => {
        $(
            paste::paste! {
                std::thread_local! {
                    #[doc(hidden)]
                    static [<$EVENT _LOCAL>]: $crate::event::EventData  = $crate::event::EventData::new(std::stringify!($EVENT));
                }

                $(#[$attr])*
                $vis static $EVENT: $crate::event::Event<$Args> = $crate::event::Event::new(&[<$EVENT _LOCAL>]);
            }
        )+
    }
}
#[doc(inline)]
pub use crate::event_macro as event;

#[doc(hidden)]
pub struct EventData {
    name: &'static str,
    widget_subs: RefCell<IdMap<WidgetId, EventHandle>>,
    hooks: RefCell<Vec<EventHook>>,
    app_inited: Cell<bool>,
}
impl EventData {
    #[doc(hidden)]
    pub fn new(name: &'static str) -> Self {
        EventData {
            name,
            widget_subs: RefCell::default(),
            hooks: RefCell::new(vec![]),
            app_inited: Cell::new(false),
        }
    }

    fn name(&self) -> &'static str {
        self.name
    }
}

/// Represents an event.
pub struct Event<A: EventArgs> {
    local: &'static LocalKey<EventData>,
    _args: PhantomData<fn(A)>,
}
impl<A: EventArgs> fmt::Debug for Event<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            write!(f, "Event({})", self.name())
        } else {
            write!(f, "{}", self.name())
        }
    }
}
impl<A: EventArgs> Event<A> {
    #[doc(hidden)]
    pub const fn new(local: &'static LocalKey<EventData>) -> Self {
        Event { local, _args: PhantomData }
    }

    /// Gets the event without the args type.
    pub fn as_any(&self) -> AnyEvent {
        AnyEvent { local: self.local }
    }

    /// Register the widget to receive targeted events from this event.
    ///
    /// Widgets only receive events if they are in the delivery list generated by the event arguments and are
    /// subscribers to the event, app extensions receive all events.
    pub fn subscribe(&self, widget_id: WidgetId) -> EventHandle {
        self.as_any().subscribe(widget_id)
    }

    /// Returns `true` if the widget is subscribed to this event.
    pub fn is_subscriber(&self, widget_id: WidgetId) -> bool {
        self.as_any().is_subscriber(widget_id)
    }

    /// Returns `true`  if at least one widget is subscribed to this event.
    pub fn has_subscribers(&self) -> bool {
        self.as_any().has_subscribers()
    }

    /// Event name.
    pub fn name(&self) -> &'static str {
        self.local.with(EventData::name)
    }

    /// Returns `true` if the update is for this event.
    pub fn has(&self, update: &EventUpdate) -> bool {
        *self == update.event
    }

    /// Get the event update args if the update is for this event.
    pub fn on<'a>(&self, update: &'a EventUpdate) -> Option<&'a A> {
        if *self == update.event {
            update.args.as_any().downcast_ref()
        } else {
            None
        }
    }

    /// Get the event update args if the update is for this event and propagation is not stopped.
    pub fn on_unhandled<'a>(&self, update: &'a EventUpdate) -> Option<&'a A> {
        self.on(update).filter(|a| a.propagation().is_stopped())
    }

    /// Calls `handler` if the update is for this event and propagation is not stopped, after the handler is called propagation is stopped.
    pub fn handle<R>(&self, update: &EventUpdate, handler: impl FnOnce(&A) -> R) -> Option<R> {
        if let Some(args) = self.on(update) {
            args.handle(handler)
        } else {
            None
        }
    }

    /// Create an event update for this event with delivery list filtered by the event subscribers.
    pub fn new_update(&self, args: A) -> EventUpdate {
        self.new_update_custom(args, UpdateDeliveryList::new(Box::new(self.as_any())))
    }

    /// Create and event update for this event with a custom delivery list.
    pub fn new_update_custom(&self, args: A, mut delivery_list: UpdateDeliveryList) -> EventUpdate {
        args.delivery_list(&mut delivery_list);
        EventUpdate {
            event: self.as_any(),
            delivery_list,
            args: Box::new(args),
            pre_actions: vec![],
            pos_actions: vec![],
        }
    }

    /// Schedule an event update.
    pub fn notify<Ev>(&self, events: &mut Ev, args: A)
    where
        Ev: WithEvents,
    {
        let update = self.new_update(args);
        events.with_events(|ev| {
            ev.notify(update);
        })
    }

    /// Creates a preview event handler.
    ///
    /// The event `handler` is called for every update of `E` that has not stopped [`propagation`](AnyEventArgs::propagation).
    /// The handler is called before UI handlers and [`on_event`](Self::on_event) handlers, it is called after all previous registered
    /// preview handlers.
    ///
    /// Returns an [`EventHandle`] that can be dropped to unsubscribe, you can also unsubscribe from inside the handler by calling
    /// [`unsubscribe`](crate::handler::AppWeakHandle::unsubscribe) in the third parameter of [`app_hn!`] or [`async_app_hn!`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use zero_ui_core::event::*;
    /// # use zero_ui_core::handler::app_hn;
    /// # use zero_ui_core::focus::{FOCUS_CHANGED_EVENT, FocusChangedArgs};
    /// #
    /// let handle = FOCUS_CHANGED_EVENT.on_pre_event(app_hn!(|_ctx, args: &FocusChangedArgs, _| {
    ///     println!("focused: {:?}", args.new_focus);
    /// }));
    /// ```
    /// The example listens to all `FOCUS_CHANGED_EVENT` events, independent of widget context and before all UI handlers.
    ///
    /// # Handlers
    ///
    /// the event handler can be any type that implements [`AppHandler`], there are multiple flavors of handlers, including
    /// async handlers that allow calling `.await`. The handler closures can be declared using [`app_hn!`], [`async_app_hn!`],
    /// [`app_hn_once!`] and [`async_app_hn_once!`].
    ///
    /// ## Async
    ///
    /// Note that for async handlers only the code before the first `.await` is called in the *preview* moment, code after runs in
    /// subsequent event updates, after the event has already propagated, so stopping [`propagation`](AnyEventArgs::propagation)
    /// only causes the desired effect before the first `.await`.
    ///
    /// [`app_hn!`]: crate::handler::app_hn!
    /// [`async_app_hn!`]: crate::handler::async_app_hn!
    /// [`app_hn_once!`]: crate::handler::app_hn_once!
    /// [`async_app_hn_once!`]: crate::handler::async_app_hn_once!
    pub fn on_pre_event<H>(&self, handler: H) -> EventHandle
    where
        H: AppHandler<A>,
    {
        self.on_event_impl(handler, true)
    }

    /// Creates an event handler.
    ///
    /// The event `handler` is called for every update of `E` that has not stopped [`propagation`](AnyEventArgs::propagation).
    /// The handler is called after all [`on_pre_event`],(Self::on_pre_event) all UI handlers and all [`on_event`](Self::on_event) handlers
    /// registered before this one.
    ///
    /// Returns an [`EventHandle`] that can be dropped to unsubscribe, you can also unsubscribe from inside the handler by calling
    /// [`unsubscribe`](crate::handler::AppWeakHandle::unsubscribe) in the third parameter of [`app_hn!`] or [`async_app_hn!`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use zero_ui_core::event::*;
    /// # use zero_ui_core::handler::app_hn;
    /// # use zero_ui_core::focus::{FOCUS_CHANGED_EVENT, FocusChangedArgs};
    /// #
    /// let handle = FOCUS_CHANGED_EVENT.on_event(app_hn!(|_ctx, args: &FocusChangedArgs, _| {
    ///     println!("focused: {:?}", args.new_focus);
    /// }));
    /// ```
    /// The example listens to all `FOCUS_CHANGED_EVENT` events, independent of widget context, after the UI was notified.
    ///
    /// # Handlers
    ///
    /// the event handler can be any type that implements [`AppHandler`], there are multiple flavors of handlers, including
    /// async handlers that allow calling `.await`. The handler closures can be declared using [`app_hn!`], [`async_app_hn!`],
    /// [`app_hn_once!`] and [`async_app_hn_once!`].
    ///
    /// ## Async
    ///
    /// Note that for async handlers only the code before the first `.await` is called in the *preview* moment, code after runs in
    /// subsequent event updates, after the event has already propagated, so stopping [`propagation`](AnyEventArgs::propagation)
    /// only causes the desired effect before the first `.await`.
    ///
    /// [`app_hn!`]: crate::handler::app_hn!
    /// [`async_app_hn!`]: crate::handler::async_app_hn!
    /// [`app_hn_once!`]: crate::handler::app_hn_once!
    /// [`async_app_hn_once!`]: crate::handler::async_app_hn_once!
    pub fn on_event(&self, handler: impl AppHandler<A>) -> EventHandle {
        self.on_event_impl(handler, false)
    }

    fn on_event_impl(&self, handler: impl AppHandler<A>, is_preview: bool) -> EventHandle {
        let handler = Arc::new(Mutex::new(handler));
        let (inner_handle_owner, inner_handle) = crate::crate_util::Handle::new(());
        self.as_any().hook(move |_, update| {
            if inner_handle_owner.is_dropped() {
                return false;
            }

            let handle = inner_handle.downgrade();
            update.push_once_action(
                Box::new(clone_move!(handler, |ctx, update| {
                    let args = update.args().as_any().downcast_ref::<A>().unwrap();
                    if !args.propagation().is_stopped() {
                        handler.lock().event(
                            ctx,
                            args,
                            &AppHandlerArgs {
                                handle: &handle,
                                is_preview,
                            },
                        );
                    }
                })),
                is_preview,
            );

            true
        })
    }

    /// Creates a receiver that can listen to the event from another thread. The event updates are sent as soon as the
    /// event update cycle starts in the UI thread.
    ///
    /// Drop the receiver to stop listening.
    pub fn receiver(&self) -> EventReceiver<A>
    where
        A: Send,
    {
        let (sender, receiver) = flume::unbounded();

        self.as_any()
            .hook(move |_, update| sender.send(update.args().as_any().downcast_ref::<A>().unwrap().clone()).is_ok())
            .perm();

        EventReceiver { receiver, event: *self }
    }

    /// Creates a sender that can raise an event from other threads and without access to [`Events`].
    pub fn sender(&self, ev: &mut impl WithEvents) -> EventSender<A>
    where
        A: Send,
    {
        ev.with_events(|ev| ev.sender(*self))
    }
}
impl<A: EventArgs> Clone for Event<A> {
    fn clone(&self) -> Self {
        Self {
            local: self.local,
            _args: PhantomData,
        }
    }
}
impl<A: EventArgs> Copy for Event<A> {}
impl<A: EventArgs> PartialEq for Event<A> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.local, other.local)
    }
}
impl<A: EventArgs> Eq for Event<A> {}

/// Represents an [`Event`] without the args type.
#[derive(Clone, Copy)]
pub struct AnyEvent {
    local: &'static LocalKey<EventData>,
}
impl fmt::Debug for AnyEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            write!(f, "AnyEvent({})", self.name())
        } else {
            write!(f, "{}", self.name())
        }
    }
}
impl AnyEvent {
    /// Display name.
    pub fn name(&self) -> &'static str {
        self.local.with(EventData::name)
    }

    /// Returns `true` is `self` is the type erased `event`.
    pub fn is<A: EventArgs>(&self, event: &Event<A>) -> bool {
        self == event
    }

    /// Returns `true` if the update is for this event.
    pub fn has(&self, update: &EventUpdate) -> bool {
        *self == update.event
    }

    /// Register a callback that is called just before an event begins notifying.
    pub fn hook(&self, hook: impl Fn(&mut Events, &mut EventUpdate) -> bool + Send + Sync + 'static) -> EventHandle {
        self.init_app();
        self.hook_impl(Box::new(hook))
    }
    fn hook_impl(&self, hook: Box<dyn Fn(&mut Events, &mut EventUpdate) -> bool + Send + Sync>) -> EventHandle {
        let (handle, hook) = EventHandle::new(hook);
        self.local.with(move |l| l.hooks.borrow_mut().push(hook));
        handle
    }

    /// Register the widget to receive targeted events from this event.
    ///
    /// Widgets only receive events if they are in the delivery list generated by the event arguments and are
    /// subscribers to the event, app extensions receive all events.
    pub fn subscribe(&self, widget_id: WidgetId) -> EventHandle {
        self.init_app();
        self.local.with(|l| {
            l.widget_subs
                .borrow_mut()
                .entry(widget_id)
                .or_insert_with(EventHandle::new_none)
                .clone()
        })
    }

    /// Returns `true` if the widget is subscribed to this event.
    pub fn is_subscriber(&self, widget_id: WidgetId) -> bool {
        self.local.with(|l| l.widget_subs.borrow().contains_key(&widget_id))
    }

    /// Returns `true`  if at least one widget is subscribed to this event.
    pub fn has_subscribers(&self) -> bool {
        self.local.with(|l| !l.widget_subs.borrow().is_empty())
    }

    fn init_app(&self) {
        self.local.with(|l| {
            if !l.app_inited.replace(true) {
                let ev = *self;
                AppProcess::on_exited(move || {
                    ev.local.with(|l| {
                        l.widget_subs.borrow_mut().clear();
                        l.hooks.borrow_mut().clear();
                        l.app_inited.set(false);
                    })
                })
            }
        })
    }

    fn on_update(&self, events: &mut Events, update: &mut EventUpdate) {
        self.local.with(|l| {
            let mut hooks = mem::take(&mut *l.hooks.borrow_mut());
            hooks.retain(|h| h.call(events, update));
            let mut h = l.hooks.borrow_mut();
            hooks.extend(h.drain(..));
            *h = hooks;
        })
    }
}
impl PartialEq for AnyEvent {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.local, other.local)
    }
}
impl Eq for AnyEvent {}
impl<A: EventArgs> PartialEq<AnyEvent> for Event<A> {
    fn eq(&self, other: &AnyEvent) -> bool {
        std::ptr::eq(self.local, other.local)
    }
}
impl<A: EventArgs> PartialEq<Event<A>> for AnyEvent {
    fn eq(&self, other: &Event<A>) -> bool {
        std::ptr::eq(self.local, other.local)
    }
}

/// Represents a single event update.
pub struct EventUpdate {
    event: AnyEvent,
    args: Box<dyn AnyEventArgs>,
    delivery_list: UpdateDeliveryList,
    pre_actions: Vec<Box<dyn FnOnce(&mut AppContext, &EventUpdate)>>,
    pos_actions: Vec<Box<dyn FnOnce(&mut AppContext, &EventUpdate)>>,
}
impl EventUpdate {
    /// The event.
    pub fn event(&self) -> AnyEvent {
        self.event
    }

    /// The update delivery list.
    pub fn delivery_list(&self) -> &UpdateDeliveryList {
        &self.delivery_list
    }

    /// The update args.
    pub fn args(&self) -> &dyn AnyEventArgs {
        &*self.args
    }

    /// Find all targets.
    ///
    /// This must be called before the first window visit, see [`UpdateDeliveryList::fulfill_search`] for details.
    pub fn fulfill_search<'a, 'b>(&'a mut self, windows: impl Iterator<Item = &'b WidgetInfoTree>) {
        self.delivery_list.fulfill_search(windows)
    }

    /// Calls `handle` if the event targets the window.
    pub fn with_window<H: FnOnce(&mut WindowContext, &mut Self) -> R, R>(&mut self, ctx: &mut WindowContext, handle: H) -> Option<R> {
        if self.delivery_list.enter_window(*ctx.window_id) {
            Some(handle(ctx, self))
        } else {
            None
        }
    }

    /// Calls `handle` if the event targets the widget and propagation is not stopped.
    pub fn with_widget<H: FnOnce(&mut WidgetContext, &mut Self) -> R, R>(&mut self, ctx: &mut WidgetContext, handle: H) -> Option<R> {
        if self.delivery_list.enter_widget(ctx.path.widget_id()) {
            let stop = self.args.propagation().is_stopped();

            let r = if stop { None } else { Some(handle(ctx, self)) };

            if stop || self.args.propagation().is_stopped() {
                self.pre_actions.clear();
                self.pos_actions.clear();
                self.delivery_list.clear();
            }

            r
        } else {
            None
        }
    }

    fn push_once_action(&mut self, action: Box<dyn FnOnce(&mut AppContext, &EventUpdate)>, is_preview: bool) {
        if is_preview {
            self.pre_actions.push(action);
        } else {
            self.pos_actions.push(action);
        }
    }

    pub(crate) fn call_pre_actions(&mut self, ctx: &mut AppContext) {
        let actions = mem::take(&mut self.pre_actions);
        for action in actions {
            action(ctx, self)
        }
    }

    pub(crate) fn call_pos_actions(&mut self, ctx: &mut AppContext) {
        let actions = mem::take(&mut self.pos_actions);
        for action in actions {
            action(ctx, self)
        }
    }
}
impl fmt::Debug for EventUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventUpdate")
            .field("event", &self.event)
            .field("args", &self.args)
            .field("delivery_list", &self.delivery_list)
            .finish_non_exhaustive()
    }
}

impl UpdateSubscribers for AnyEvent {
    fn contains(&self, widget_id: WidgetId) -> bool {
        self.local.with(|l| match l.widget_subs.borrow_mut().entry(widget_id) {
            std::collections::hash_map::Entry::Occupied(e) => {
                let t = e.get().retain();
                if !t {
                    e.remove();
                }
                t
            }
            std::collections::hash_map::Entry::Vacant(_) => false,
        })
    }

    fn to_set(&self) -> IdSet<WidgetId> {
        self.local.with(|l| l.widget_subs.borrow().keys().copied().collect())
    }
}

/// Represents a collection of var handles.
#[derive(Clone, Default)]
pub struct EventHandles(pub Vec<EventHandle>);
impl EventHandles {
    /// Empty collection.
    pub fn dummy() -> Self {
        EventHandles(vec![])
    }

    /// Returns `true` if empty or all handles are dummy.
    pub fn is_dummy(&self) -> bool {
        self.0.is_empty() || self.0.iter().all(EventHandle::is_dummy)
    }

    /// Drop all handles without stopping their behavior.
    pub fn perm(self) {
        for handle in self.0 {
            handle.perm()
        }
    }

    /// Add `other` handle to the collection.
    pub fn push(&mut self, other: EventHandle) -> &mut Self {
        if !other.is_dummy() {
            self.0.push(other);
        }
        self
    }

    /// Drop all handles.
    pub fn clear(&mut self) {
        self.0.clear();
    }
}
impl FromIterator<EventHandle> for EventHandles {
    fn from_iter<T: IntoIterator<Item = EventHandle>>(iter: T) -> Self {
        EventHandles(iter.into_iter().filter(|h| !h.is_dummy()).collect())
    }
}
impl<const N: usize> From<[EventHandle; N]> for EventHandles {
    fn from(handles: [EventHandle; N]) -> Self {
        handles.into_iter().filter(|h| !h.is_dummy()).collect()
    }
}
impl Extend<EventHandle> for EventHandles {
    fn extend<T: IntoIterator<Item = EventHandle>>(&mut self, iter: T) {
        for handle in iter {
            self.push(handle);
        }
    }
}
impl IntoIterator for EventHandles {
    type Item = EventHandle;

    type IntoIter = std::vec::IntoIter<EventHandle>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

struct EventHandleData {
    perm: AtomicBool,
    hook: Option<Box<dyn Fn(&mut Events, &mut EventUpdate) -> bool + Send + Sync>>,
}

/// Represents an event widget subscription, handler callback or hook.
#[derive(Clone)]
pub struct EventHandle(Option<Arc<EventHandleData>>);
impl PartialEq for EventHandle {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (None, Some(_)) | (Some(_), None) => false,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        }
    }
}
impl Eq for EventHandle {}
impl std::hash::Hash for EventHandle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let i = match &self.0 {
            Some(rc) => Arc::as_ptr(rc) as usize,
            None => 0,
        };
        state.write_usize(i);
    }
}
impl fmt::Debug for EventHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let i = match &self.0 {
            Some(rc) => Arc::as_ptr(rc) as usize,
            None => 0,
        };
        f.debug_tuple("EventHandle").field(&i).finish()
    }
}
impl EventHandle {
    fn new(hook: Box<dyn Fn(&mut Events, &mut EventUpdate) -> bool + Send + Sync>) -> (Self, EventHook) {
        let rc = Arc::new(EventHandleData {
            perm: AtomicBool::new(false),
            hook: Some(hook),
        });
        (Self(Some(rc.clone())), EventHook(rc))
    }

    fn new_none() -> Self {
        Self(Some(Arc::new(EventHandleData {
            perm: AtomicBool::new(false),
            hook: None,
        })))
    }

    /// Handle to no event.
    pub fn dummy() -> Self {
        EventHandle(None)
    }

    /// If the handle is not actually registered in an event.
    pub fn is_dummy(&self) -> bool {
        self.0.is_none()
    }

    /// Drop the handle without un-registering it, the resource it represents will remain registered in the event for the duration of
    /// the process.
    pub fn perm(self) {
        if let Some(rc) = self.0 {
            rc.perm.store(true, Ordering::Relaxed);
        }
    }

    /// Create an [`EventHandles`] collection with `self` and `other`.
    pub fn with(self, other: Self) -> EventHandles {
        [self, other].into()
    }

    fn retain(&self) -> bool {
        let rc = self.0.as_ref().unwrap();
        Arc::strong_count(rc) > 1 || rc.perm.load(Ordering::Relaxed)
    }
}

struct EventHook(Arc<EventHandleData>);
impl EventHook {
    /// Callback, returns `true` if the handle must be retained.
    fn call(&self, events: &mut Events, update: &mut EventUpdate) -> bool {
        (Arc::strong_count(&self.0) > 1 || self.0.perm.load(Ordering::Relaxed)) && (self.0.hook.as_ref().unwrap())(events, update)
    }
}
