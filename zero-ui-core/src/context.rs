//! Context information for app extensions, windows and widgets.

use super::event::Events;
use super::service::Services;
use super::units::{LayoutSize, PixelGrid};
use super::var::Vars;
use super::window::WindowId;
use super::AnyMap;
use super::WidgetId;
use retain_mut::RetainMut;
use std::cell::Cell;
use std::future::Future;
use std::ptr;
use std::rc::Rc;
use std::{any::type_name, fmt, mem};
use std::{any::TypeId, time::Instant};
use std::{marker::PhantomData, sync::Arc};
use unsafe_any::UnsafeAny;
use webrender::api::RenderApi;

/// Required updates for a window layout and frame.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum UpdateDisplayRequest {
    /// No update required.
    None = 0,
    /// No new full frame required, just update the current one and render again.
    RenderUpdate = 1,
    /// No re-layout required, just render again.
    Render = 2,
    /// Full update required, re-layout then render again.
    Layout = 3,
}
impl Default for UpdateDisplayRequest {
    #[inline]
    fn default() -> Self {
        UpdateDisplayRequest::None
    }
}
impl std::ops::BitOrAssign for UpdateDisplayRequest {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        let a = (*self) as u8;
        let b = rhs as u8;
        *self = match a.max(b) {
            3 => UpdateDisplayRequest::Layout,
            2 => UpdateDisplayRequest::Render,
            1 => UpdateDisplayRequest::RenderUpdate,
            n => {
                debug_assert_eq!(n, 0);
                UpdateDisplayRequest::None
            }
        };
    }
}
impl std::ops::BitOr for UpdateDisplayRequest {
    type Output = Self;

    #[inline]
    fn bitor(mut self, rhs: Self) -> Self {
        self |= rhs;
        self
    }
}
impl std::cmp::PartialOrd for UpdateDisplayRequest {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        std::cmp::PartialOrd::partial_cmp(&(*self as u8), &(*other as u8))
    }
}
impl std::cmp::Ord for UpdateDisplayRequest {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        std::cmp::Ord::cmp(&(*self as u8), &(*other as u8))
    }
}

impl UpdateDisplayRequest {
    /// If contains any update.
    #[inline]
    pub fn is_some(self) -> bool {
        !self.is_none()
    }

    /// If does not contain any update.
    #[inline]
    pub fn is_none(self) -> bool {
        self == UpdateDisplayRequest::None
    }
}

/// A key to a value in a [`StateMap`].
///
/// The type that implements this trait is the key. You
/// can use the [`state_key!`](crate::context::state_key) macro.
pub trait StateKey: 'static {
    /// The value type.
    type Type: 'static;
}

/// Declares new [`StateKey`](crate::context::StateKey) types.
///
/// # Example
///
/// ```
/// # use zero_ui_core::context::state_key;
/// state_key! {
///     /// Key docs.
///     pub struct FooKey: u32;
/// }
/// ```
/// # Naming Convention
///
/// It is recommended that the type name ends with the key suffix.
#[macro_export]
macro_rules! state_key {
    ($($(#[$outer:meta])* $vis:vis struct $ident:ident: $type: ty;)+) => {$(
        $(#[$outer])*
        /// # StateKey
        /// This `struct` is a [`StateKey`](crate::context::StateKey).
        #[derive(Clone, Copy)]
        $vis struct $ident;

        impl $crate::context::StateKey for $ident {
            type Type = $type;
        }
    )+};
}

use crate::app::AppEventSender;
use crate::app::WindowTarget;
use crate::crate_util::{Handle, HandleOwner, RunOnDrop};
use crate::event::BoxedEventUpdate;
use crate::handler::{self, AppHandler, AppHandlerArgs, AppWeakHandle};
#[doc(inline)]
pub use crate::state_key;
use crate::timer::Timers;
use crate::{var::VarsRead, window::WindowMode};

/// A map of [state keys](StateKey) to values of their associated types that exists for
/// a stage of the application.
///
/// # No Remove
///
/// Note that there is no way to clear the map, remove a key or replace the map with a new empty one.
/// This is by design, if you want to make a key *removable* make its value `Option<T>`.
pub struct StateMap {
    map: AnyMap,
}
impl fmt::Debug for StateMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StateMap[{} entries]", self.map.len())
    }
}
impl StateMap {
    fn new() -> Self {
        StateMap { map: AnyMap::default() }
    }

    /// Set the key `value`.
    ///
    /// # Key
    ///
    /// Use [`state_key!`](crate::context::state_key) to generate a key, any static type can be a key,
    /// the [type id](TypeId) is the actual key.
    pub fn set<S: StateKey>(&mut self, value: S::Type) -> Option<S::Type> {
        self.map.insert(TypeId::of::<S>(), Box::new(value)).map(|any| {
            // SAFETY: The type system asserts this is valid.
            unsafe { *any.downcast_unchecked::<S::Type>() }
        })
    }

    /// Sets a value that is its own [`StateKey`].
    pub fn set_single<S: StateKey<Type = S>>(&mut self, value: S) -> Option<S> {
        self.map.insert(TypeId::of::<S>(), Box::new(value)).map(|any| {
            // SAFETY: The type system asserts this is valid.
            unsafe { *any.downcast_unchecked::<S>() }
        })
    }

    /// Gets if the key is set in this map.
    pub fn contains<S: StateKey>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<S>())
    }

    /// Reference the key value set in this map.
    pub fn get<S: StateKey>(&self) -> Option<&S::Type> {
        self.map.get(&TypeId::of::<S>()).map(|any| {
            // SAFETY: The type system asserts this is valid.
            unsafe { any.downcast_ref_unchecked::<S::Type>() }
        })
    }

    /// Mutable borrow the key value set in this map.
    pub fn get_mut<S: StateKey>(&mut self) -> Option<&mut S::Type> {
        self.map.get_mut(&TypeId::of::<S>()).map(|any| {
            // SAFETY: The type system asserts this is valid.
            unsafe { any.downcast_mut_unchecked::<S::Type>() }
        })
    }

    /// Reference the key value set in this map or panics if the key is not set.
    pub fn req<S: StateKey>(&self) -> &S::Type {
        self.get::<S>()
            .unwrap_or_else(|| panic!("expected `{}` in state map", type_name::<S>()))
    }

    /// Mutable borrow the key value set in this map or panics if the key is not set.
    pub fn req_mut<S: StateKey>(&mut self) -> &mut S::Type {
        self.get_mut::<S>()
            .unwrap_or_else(|| panic!("expected `{}` in state map", type_name::<S>()))
    }

    /// Gets the given key's corresponding entry in the map for in-place manipulation.
    pub fn entry<S: StateKey>(&mut self) -> StateMapEntry<S> {
        StateMapEntry {
            _key: PhantomData,
            entry: self.map.entry(TypeId::of::<S>()),
        }
    }

    /// Sets a state key without value.
    ///
    /// Returns if the state key was already flagged.
    pub fn flag<S: StateKey<Type = ()>>(&mut self) -> bool {
        self.set::<S>(()).is_some()
    }

    /// Gets if a state key without value is set.
    pub fn flagged<S: StateKey<Type = ()>>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<S>())
    }

    /// If no state is set.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// A view into a single entry in a state map, which may either be vacant or occupied.
pub struct StateMapEntry<'a, S: StateKey> {
    _key: PhantomData<S>,
    entry: std::collections::hash_map::Entry<'a, TypeId, Box<dyn UnsafeAny>>,
}
impl<'a, S: StateKey> StateMapEntry<'a, S> {
    /// Ensures a value is in the entry by inserting the default if empty, and
    /// returns a mutable reference to the value in the entry.
    pub fn or_insert(self, default: S::Type) -> &'a mut S::Type {
        // SAFETY: The type system asserts this is valid.
        unsafe { self.entry.or_insert_with(|| Box::new(default)).downcast_mut_unchecked::<S::Type>() }
    }

    /// Ensures a value is in the entry by inserting the result of the
    /// default function if empty, and returns a mutable reference to the value in the entry.
    pub fn or_insert_with<F: FnOnce() -> S::Type>(self, default: F) -> &'a mut S::Type {
        // SAFETY: The type system asserts this is valid.
        unsafe {
            self.entry
                .or_insert_with(|| Box::new(default()))
                .downcast_mut_unchecked::<S::Type>()
        }
    }

    /// Provides in-place mutable access to an occupied entry before any potential inserts into the map.
    pub fn and_modify<F: FnOnce(&mut S::Type)>(self, f: F) -> Self {
        let entry = self.entry.and_modify(|a| {
            f({
                // SAFETY: The type system asserts this is valid.
                unsafe { a.downcast_mut_unchecked::<S::Type>() }
            })
        });
        StateMapEntry { _key: PhantomData, entry }
    }
}
impl<'a, S: StateKey> StateMapEntry<'a, S>
where
    S::Type: Default,
{
    /// Ensures a value is in the entry by inserting the default value if empty,
    /// and returns a mutable reference to the value in the entry.
    pub fn or_default(self) -> &'a mut S::Type {
        // SAFETY: The type system asserts this is valid.
        unsafe {
            self.entry
                .or_insert_with(|| Box::new(<S::Type as Default>::default()))
                .downcast_mut_unchecked::<S::Type>()
        }
    }
}

/// Private [`StateMap`].
///
/// The owner of a state map has full access including to the `remove` and `clear` function that is not
/// provided in the [`StateMap`] type.
pub struct OwnedStateMap(pub(crate) StateMap); // TODO deref StateMap?
impl Default for OwnedStateMap {
    fn default() -> Self {
        OwnedStateMap(StateMap::new())
    }
}
impl OwnedStateMap {
    /// New default, empty.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Remove the key.
    pub fn remove<S: StateKey>(&mut self) -> Option<S::Type> {
        self.0.map.remove(&TypeId::of::<S>()).map(|a| {
            // SAFETY: The type system asserts this is valid.
            unsafe { *a.downcast_unchecked::<S::Type>() }
        })
    }

    /// Removes all entries.
    #[inline]
    pub fn clear(&mut self) {
        self.0.map.clear()
    }

    /// Set the key `value`.
    ///
    /// # Key
    ///
    /// Use [`state_key!`](crate::context::state_key) to generate a key, any static type can be a key,
    /// the [type id](TypeId) is the actual key.
    pub fn set<S: StateKey>(&mut self, value: S::Type) -> Option<S::Type> {
        self.0.set::<S>(value)
    }

    /// Sets a value that is its own [`StateKey`].
    pub fn set_single<S: StateKey<Type = S>>(&mut self, value: S) -> Option<S> {
        self.0.set_single::<S>(value)
    }

    /// Gets if the key is set in this map.
    pub fn contains<S: StateKey>(&self) -> bool {
        self.0.contains::<S>()
    }

    /// Reference the key value set in this map.
    pub fn get<S: StateKey>(&self) -> Option<&S::Type> {
        self.0.get::<S>()
    }

    /// Mutable borrow the key value set in this map.
    pub fn get_mut<S: StateKey>(&mut self) -> Option<&mut S::Type> {
        self.0.get_mut::<S>()
    }

    /// Reference the key value set in this map, or panics if the key is not set.
    pub fn req<S: StateKey>(&self) -> &S::Type {
        self.0.req::<S>()
    }

    /// Mutable borrow the key value set in this map, or panics if the key is not set.
    pub fn req_mut<S: StateKey>(&mut self) -> &mut S::Type {
        self.0.req_mut::<S>()
    }

    /// Gets the given key's corresponding entry in the map for in-place manipulation.
    pub fn entry<S: StateKey>(&mut self) -> StateMapEntry<S> {
        self.0.entry::<S>()
    }

    /// Sets a state key without value.
    ///
    /// Returns if the state key was already flagged.
    pub fn flag<S: StateKey<Type = ()>>(&mut self) -> bool {
        self.0.flag::<S>()
    }

    /// Gets if a state key without value is set.
    pub fn flagged<S: StateKey<Type = ()>>(&self) -> bool {
        self.0.flagged::<S>()
    }

    /// If no state is set.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Represents an [`on_pre_update`](Updates::on_pre_update) or [`on_update`](Updates::on_update) handler.
///
/// Drop all clones of this handle to drop the binding, or call [`permanent`](Self::permanent) to drop the handle
/// but keep the handler alive for the duration of the app.
#[derive(Clone)]
#[must_use = "dropping the handle unsubscribes update handler"]
pub struct OnUpdateHandle(Handle<()>);
impl OnUpdateHandle {
    fn new() -> (HandleOwner<()>, OnUpdateHandle) {
        let (owner, handle) = Handle::new(());
        (owner, OnUpdateHandle(handle))
    }

    /// Create a handle to nothing, the handle always in the *unsubscribed* state.
    #[inline]
    pub fn dummy() -> Self {
        OnUpdateHandle(Handle::dummy(()))
    }

    /// Drop the handle but does **not** unsubscribe.
    ///
    /// The handler stays in memory for the duration of the app or until another handle calls [`unsubscribe`](Self::unsubscribe.)
    #[inline]
    pub fn permanent(self) {
        self.0.permanent();
    }

    /// If another handle has called [`permanent`](Self::permanent).
    /// If `true` the var binding will stay active until the app shutdown, unless [`unsubscribe`](Self::unsubscribe) is called.
    #[inline]
    pub fn is_permanent(&self) -> bool {
        self.0.is_permanent()
    }

    /// Drops the handle and forces the handler to drop.
    #[inline]
    pub fn unsubscribe(self) {
        self.0.force_drop()
    }

    /// If another handle has called [`unsubscribe`](Self::unsubscribe).
    ///
    /// The handler is already dropped or will be dropped in the next app update, this is irreversible.
    #[inline]
    pub fn is_unsubscribed(&self) -> bool {
        self.0.is_dropped()
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
    display_update: UpdateDisplayRequest,
    win_display_update: UpdateDisplayRequest,

    pre_handlers: Vec<UpdateHandler>,
    pos_handlers: Vec<UpdateHandler>,
}
impl Updates {
    fn new(event_sender: AppEventSender) -> Self {
        Updates {
            event_sender,
            update: false,
            display_update: UpdateDisplayRequest::None,
            win_display_update: UpdateDisplayRequest::None,

            pre_handlers: vec![],
            pos_handlers: vec![],
        }
    }

    /// Create an [`AppEventSender`] that can be used to awake the app and send app events.
    #[inline]
    pub fn sender(&self) -> AppEventSender {
        self.event_sender.clone()
    }

    /// Schedules a low-pressure update.
    #[inline]
    pub fn update(&mut self) {
        self.update = true;
    }

    /// Gets `true` if a low-pressure update was requested.
    #[inline]
    pub fn update_requested(&self) -> bool {
        self.update
    }

    /// Schedules a layout update.
    #[inline]
    pub fn layout(&mut self) {
        self.win_display_update |= UpdateDisplayRequest::Layout;
        self.display_update |= UpdateDisplayRequest::Layout;
    }

    /// Gets `true` if a layout update is scheduled.
    #[inline]
    pub fn layout_requested(&self) -> bool {
        self.win_display_update == UpdateDisplayRequest::Layout
    }

    /// Schedules a new frame.
    #[inline]
    pub fn render(&mut self) {
        self.win_display_update |= UpdateDisplayRequest::Render;
        self.display_update |= UpdateDisplayRequest::Render;
    }

    /// Gets `true` if a new frame is scheduled.
    #[inline]
    pub fn render_requested(&self) -> bool {
        self.win_display_update >= UpdateDisplayRequest::Render
    }

    /// Schedule a frame update.
    #[inline]
    pub fn render_update(&mut self) {
        self.win_display_update |= UpdateDisplayRequest::RenderUpdate;
        self.display_update |= UpdateDisplayRequest::RenderUpdate;
    }

    /// Gets `true` if a frame update is scheduled.
    #[inline]
    pub fn render_update_requested(&self) -> bool {
        self.win_display_update >= UpdateDisplayRequest::RenderUpdate
    }

    /// Schedule the `updates`.
    #[inline]
    pub fn schedule_display_updates(&mut self, updates: UpdateDisplayRequest) {
        self.win_display_update |= updates;
        self.display_update |= updates;
    }

    /// Schedule an *once* handler to run when these updates are applied.
    ///
    /// The callback is any of the *once* [`AppHandler`], including async handlers. You can use [`app_hn_once!`](handler::app_hn_once!)
    /// or [`async_app_hn_once!`](handler::async_app_hn_once!) to declare the closure. If the handler is async and does not finish in
    /// one call it is scheduled to update in *preview* updates.
    pub fn run<H: AppHandler<UpdateArgs> + handler::marker::OnceHn>(&mut self, handler: H) -> OnUpdateHandle {
        self.update(); // in case of this was called outside of an update.
        Self::push_handler(&mut self.pos_handlers, true, handler)
    }

    /// Create a preview update handler.
    ///
    /// The `handler` is called every time the app updates, just before the UI updates. It can be any of the non-async [`AppHandler`],
    /// use the [`app_hn!`] or [`app_hn_once!`] macros to declare the closure. Async handlers are not allowed because UI bound async
    /// tasks cause app updates to awake, so it is very easy to lock the app in a constant sequence of updates. You can use [`run`](Self::run)
    /// to start an async app context task.
    ///
    /// Returns an [`OnUpdateHandle`] that can be used to unsubscribe, you can also unsubscribe from inside the handler by calling
    /// [`unsubscribe`](crate::handler::AppWeakHandle::unsubscribe) in the third parameter of [`app_hn!`] or [`async_app_hn!`].
    pub fn on_pre_update<H>(&mut self, handler: H) -> OnUpdateHandle
    where
        H: AppHandler<UpdateArgs> + handler::marker::NotAsyncHn,
    {
        Self::push_handler(&mut self.pre_handlers, true, handler)
    }

    /// Create an update handler.
    ///
    /// The `handler` is called every time the app updates, just after the UI updates.
    ///
    /// Returns an [`OnUpdateHandle`] that can be used to unsubscribe, you can also unsubscribe from inside the handler by calling
    /// [`unsubscribe`](crate::handler::AppWeakHandle::unsubscribe) in the third parameter of [`app_hn!`] or [`async_app_hn!`].
    pub fn on_update<H>(&mut self, handler: H) -> OnUpdateHandle
    where
        H: AppHandler<UpdateArgs> + handler::marker::NotAsyncHn,
    {
        Self::push_handler(&mut self.pos_handlers, false, handler)
    }

    fn push_handler<H>(entries: &mut Vec<UpdateHandler>, is_preview: bool, mut handler: H) -> OnUpdateHandle
    where
        H: AppHandler<UpdateArgs>,
    {
        let (handle_owner, handle) = OnUpdateHandle::new();
        entries.push(UpdateHandler {
            handle: handle_owner,
            count: 0,
            handler: Box::new(move |ctx, args, handle| {
                handler.event(ctx, args, &AppHandlerArgs { handle, is_preview });
            }),
        });
        handle
    }

    pub(crate) fn on_pre_updates(ctx: &mut AppContext) {
        let mut handlers = mem::take(&mut ctx.updates.pre_handlers);
        Self::retain_updates(ctx, &mut handlers);
        handlers.extend(ctx.updates.pre_handlers.drain(..));
        ctx.updates.pre_handlers = handlers;
    }

    pub(crate) fn on_updates(ctx: &mut AppContext) {
        let mut handlers = mem::take(&mut ctx.updates.pos_handlers);
        Self::retain_updates(ctx, &mut handlers);
        handlers.extend(ctx.updates.pos_handlers.drain(..));
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

    fn take_updates(&mut self) -> (bool, UpdateDisplayRequest) {
        (mem::take(&mut self.update), mem::take(&mut self.display_update))
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
impl<'a, 'w> WithUpdates for crate::context::AppContext<'a, 'w> {
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

/// Owner of [`AppContext`] objects.
///
/// You can only have one instance of this at a time per-thread at a time.
pub(crate) struct OwnedAppContext {
    app_state: StateMap,
    vars: Vars,
    events: Events,
    services: Services,
    timers: Timers,
    updates: Updates,
}
impl OwnedAppContext {
    /// Produces the single instance of `AppContext` for a normal app run.
    pub fn instance(app_event_sender: AppEventSender) -> Self {
        let updates = Updates::new(app_event_sender.clone());
        OwnedAppContext {
            app_state: StateMap::new(),
            vars: Vars::instance(app_event_sender.clone()),
            events: Events::instance(app_event_sender),
            services: Services::default(),
            timers: Timers::new(),
            updates,
        }
    }

    /// State that lives for the duration of an application, including a headless application.
    pub fn app_state(&self) -> &StateMap {
        &self.app_state
    }

    /// State that lives for the duration of an application, including a headless application.
    pub fn app_state_mut(&mut self) -> &mut StateMap {
        &mut self.app_state
    }

    /// Borrow the app context as an [`AppContext`].
    pub fn borrow<'a, 'w>(&'a mut self, window_target: WindowTarget<'w>) -> AppContext<'a, 'w> {
        AppContext {
            app_state: &mut self.app_state,
            vars: &self.vars,
            events: &mut self.events,
            services: &mut self.services,
            timers: &mut self.timers,
            updates: &mut self.updates,
            window_target,
        }
    }

    /// Applies pending `timers`, `sync`, `vars` and `events` and returns the update
    /// requests and a time for the loop to awake and update.
    #[must_use]
    pub fn apply_updates(&mut self) -> ContextUpdates {
        let wake_time = self.timers.apply_updates(&self.vars);
        let events = self.events.apply_updates(&self.vars, &mut self.updates);
        self.vars.apply_updates(&mut self.updates);

        let (update, display_update) = self.updates.take_updates();

        ContextUpdates {
            events,
            update,
            display_update,
            wake_time,
        }
    }
}

/// Full application context.
pub struct AppContext<'a, 'w> {
    /// State that lives for the duration of the application.
    pub app_state: &'a mut StateMap,

    /// Access to variables.
    pub vars: &'a Vars,
    /// Access to application events.
    pub events: &'a mut Events,
    /// Access to application services.
    pub services: &'a mut Services,

    /// Event loop based timers.
    pub timers: &'a mut Timers,

    /// Schedule of actions to apply after this update.
    pub updates: &'a mut Updates,

    /// Reference to event loop for headed windows.
    pub window_target: WindowTarget<'w>,
}
impl<'a, 'w> AppContext<'a, 'w> {
    /// If the context is in headless mode.
    pub fn is_headless(&self) -> bool {
        self.updates.event_sender.is_headless()
    }

    /// Runs a function `f` in the context of a window.
    #[inline(always)]
    pub fn window_context<R>(
        &mut self,
        window_id: WindowId,
        mode: WindowMode,
        window_state: &mut OwnedStateMap,
        render_api: &Option<Arc<RenderApi>>,
        f: impl FnOnce(&mut WindowContext) -> R,
    ) -> (R, UpdateDisplayRequest) {
        self.updates.win_display_update = UpdateDisplayRequest::None;

        let mut update_state = StateMap::new();

        let r = f(&mut WindowContext {
            window_id: &window_id,
            mode: &mode,
            render_api,
            app_state: self.app_state,
            window_state: &mut window_state.0,
            update_state: &mut update_state,
            vars: self.vars,
            events: self.events,
            services: self.services,
            timers: self.timers,
            updates: self.updates,
        });

        (r, mem::take(&mut self.updates.win_display_update))
    }

    /// Run a function `f` in the layout context of the monitor that contains a window.
    #[inline(always)]
    pub fn outer_layout_context<R>(
        &mut self,
        screen_size: LayoutSize,
        scale_factor: f32,
        window_id: WindowId,
        root_id: WidgetId,
        f: impl FnOnce(&mut LayoutContext) -> R,
    ) -> R {
        f(&mut LayoutContext {
            font_size: &14.0,
            root_font_size: &14.0,
            pixel_grid: &PixelGrid::new(scale_factor),
            viewport_size: &screen_size,
            viewport_min: &screen_size.width.min(screen_size.height),
            viewport_max: &screen_size.width.max(screen_size.height),
            path: &mut WidgetContextPath::new(window_id, root_id),
            app_state: &mut self.app_state,
            window_state: &mut StateMap::new(),
            widget_state: &mut StateMap::new(),
            update_state: &mut StateMap::new(),
            vars: &self.vars,
        })
    }
}

/// A window context.
pub struct WindowContext<'a> {
    /// Id of the context window.
    pub window_id: &'a WindowId,

    /// Window mode, headed or not, renderer or not.
    pub mode: &'a WindowMode,

    /// Reference to the render API of the window.
    ///
    /// This is `None` if the [`mode`](Self::mode) is [`Headless`](WindowMode::Headless).
    pub render_api: &'a Option<Arc<RenderApi>>,

    /// State that lives for the duration of the application.
    pub app_state: &'a mut StateMap,

    /// State that lives for the duration of the window.
    pub window_state: &'a mut StateMap,

    /// State that lives for the duration of the node tree method call in the window.
    ///
    /// This state lives only for the duration of the function `f` call in [`AppContext::window_context`].
    /// Usually `f` calls one of the [`UiNode`](crate::UiNode) methods and [`WidgetContext`] shares this
    /// state so properties and event handlers can use this state to communicate to further nodes along the
    /// update sequence.
    pub update_state: &'a mut StateMap,

    /// Access to variables.
    pub vars: &'a Vars,
    /// Access to application events.
    pub events: &'a mut Events,
    /// Access to application services.
    pub services: &'a mut Services,

    /// Event loop based timers.
    pub timers: &'a mut Timers,

    /// Schedule of actions to apply after this update.
    pub updates: &'a mut Updates,
}
impl<'a> WindowContext<'a> {
    /// Runs a function `f` in the context of a widget.
    #[inline(always)]
    pub fn widget_context<R>(
        &mut self,
        widget_id: WidgetId,
        widget_state: &mut OwnedStateMap,
        f: impl FnOnce(&mut WidgetContext) -> R,
    ) -> R {
        f(&mut WidgetContext {
            path: &mut WidgetContextPath::new(*self.window_id, widget_id),

            app_state: self.app_state,
            window_state: self.window_state,
            widget_state: &mut widget_state.0,
            update_state: self.update_state,

            vars: self.vars,
            events: self.events,
            services: self.services,

            timers: self.timers,

            updates: self.updates,
        })
    }

    /// Runs a function `f` in the layout context of a widget.
    #[inline(always)]
    pub fn layout_context<R>(
        &mut self,
        font_size: f32,
        pixel_grid: PixelGrid,
        viewport_size: LayoutSize,
        widget_id: WidgetId,
        widget_state: &mut OwnedStateMap,
        f: impl FnOnce(&mut LayoutContext) -> R,
    ) -> R {
        f(&mut LayoutContext {
            font_size: &font_size,
            root_font_size: &font_size,
            pixel_grid: &pixel_grid,
            viewport_size: &viewport_size,
            viewport_min: &viewport_size.width.min(viewport_size.height),
            viewport_max: &viewport_size.width.max(viewport_size.height),

            path: &mut WidgetContextPath::new(*self.window_id, widget_id),

            app_state: self.app_state,
            window_state: self.window_state,
            widget_state: &mut widget_state.0,
            update_state: self.update_state,

            vars: &self.vars,
        })
    }

    /// Runs a function `f` in the render context of a widget.
    #[inline(always)]
    pub fn render_context<R>(&mut self, widget_id: WidgetId, widget_state: &OwnedStateMap, f: impl FnOnce(&mut RenderContext) -> R) -> R {
        f(&mut RenderContext {
            path: &mut WidgetContextPath::new(*self.window_id, widget_id),
            app_state: self.app_state,
            window_state: self.window_state,
            widget_state: &widget_state.0,
            update_state: self.update_state,
            vars: &self.vars,
        })
    }
}

/// <span class="stab portability" title="This is supported on `any(test, doc, feature="pub_test")` only"><code>any(test, doc, feature="pub_test")</code></span> A mock [`WidgetContext`] for testing widgets.
///
/// Only a single instance of this type can exist per-thread at a time, see [`new`](Self::new) for details.
///
/// This is less cumbersome to use then a full headless app, but also more limited. Use a [`HeadlessApp`](crate::app::HeadlessApp)
/// for more complex integration tests.
///
/// # Conditional Compilation
///
/// This is only compiled with the `any(test, doc, feature="pub_test")` feature enabled.
#[cfg(any(test, doc, feature = "pub_test"))]
pub struct TestWidgetContext {
    /// Id of the pretend window that owns the pretend root widget.
    ///
    /// This is a new unique [headless window id](WindowId::Headless).
    pub window_id: WindowId,
    /// Id of the pretend root widget that is the context widget.
    pub root_id: WidgetId,

    /// The [`app_state`](WidgetContext::app_state) value. Empty by default.
    pub app_state: OwnedStateMap,
    /// The [`window_state`](WidgetContext::window_state) value. Empty by default.
    pub window_state: OwnedStateMap,

    /// The [`widget_state`](WidgetContext::widget_state) value. Empty by default.
    pub widget_state: OwnedStateMap,

    /// The [`update_state`](WidgetContext::update_state) value. Empty by default.
    ///
    /// WARNING: In a real context this is reset after each update, in this test context the same map is reused
    /// unless you call [`clear`](OwnedStateMap::clear).
    pub update_state: OwnedStateMap,

    /// The [`services`](WidgetContext::services) repository. Empty by default.
    pub services: Services,

    /// A headless event loop.
    ///
    /// WARNING: In a full headless app this is drained of app events in each update.
    /// You should probably use the full [`HeadlessApp`](crate::app::HeadlessApp) if you
    /// are needing to do this.
    pub event_loop: (AppEventSender, flume::Receiver<crate::app::AppEvent>),

    /// The [`updates`](WidgetContext::updates) repository. No request by default.
    ///
    /// WARNING: This is drained of requests after each update, you can do this manually by calling
    /// [`apply_updates`](Self::apply_updates).
    pub updates: Updates,

    /// The [`vars`](WidgetContext::vars) instance.
    pub vars: Vars,

    /// The [`events`](WidgetContext::events) instance.
    ///
    /// WARNING: In a real app events can only be registered at the start of the application.
    /// In this context you can always register a service, you should probably not reuse a test widget
    /// instance after registering an event.
    pub events: Events,

    /// Event loop bases timers.
    ///
    /// TODO testable timers.
    pub timers: Timers,
}
#[cfg(any(test, doc, feature = "pub_test"))]
impl Default for TestWidgetContext {
    /// [`TestWidgetContext::new`]
    fn default() -> Self {
        Self::new()
    }
}
#[cfg(any(test, doc, feature = "pub_test"))]
impl TestWidgetContext {
    /// Gets a new [`TestWidgetContext`] instance. Panics is another instance is alive in the current thread
    /// or if an app is running in the current thread.
    pub fn new() -> Self {
        if crate::app::App::is_running() {
            panic!("only one `TestWidgetContext` or app is allowed per thread")
        }

        let (sender, receiver) = AppEventSender::new_headless();
        Self {
            window_id: WindowId::new_unique(),
            root_id: WidgetId::new_unique(),
            app_state: OwnedStateMap::new(),
            window_state: OwnedStateMap::new(),
            widget_state: OwnedStateMap::new(),
            update_state: OwnedStateMap::new(),
            services: Services::default(),
            events: Events::instance(sender.clone()),
            vars: Vars::instance(sender.clone()),
            updates: Updates::new(sender.clone()),
            timers: Timers::new(),
            event_loop: (sender, receiver),
        }
    }

    /// Calls `action` in a fake widget context.
    pub fn widget_context<R>(&mut self, action: impl FnOnce(&mut WidgetContext) -> R) -> R {
        action(&mut WidgetContext {
            path: &mut WidgetContextPath::new(self.window_id, self.root_id),
            app_state: &mut self.app_state.0,
            window_state: &mut self.window_state.0,
            widget_state: &mut self.widget_state.0,
            update_state: &mut self.update_state.0,
            vars: &self.vars,
            events: &mut self.events,
            services: &mut self.services,
            timers: &mut self.timers,
            updates: &mut self.updates,
        })
    }

    /// Calls `action` in a fake layout context.
    pub fn layout_context<R>(
        &mut self,
        root_font_size: f32,
        font_size: f32,
        viewport_size: LayoutSize,
        pixel_grid: PixelGrid,
        action: impl FnOnce(&mut LayoutContext) -> R,
    ) -> R {
        action(&mut LayoutContext {
            font_size: &font_size,
            root_font_size: &root_font_size,
            pixel_grid: &pixel_grid,
            viewport_size: &viewport_size,
            viewport_min: &viewport_size.width.min(viewport_size.height),
            viewport_max: &viewport_size.width.max(viewport_size.height),

            path: &mut WidgetContextPath::new(self.window_id, self.root_id),
            app_state: &mut self.app_state.0,
            window_state: &mut self.window_state.0,
            widget_state: &mut self.widget_state.0,
            update_state: &mut self.update_state.0,
            vars: &self.vars,
        })
    }

    /// Calls `action` in a fake render context.
    pub fn render_context<R>(&mut self, action: impl FnOnce(&mut RenderContext) -> R) -> R {
        action(&mut RenderContext {
            path: &mut WidgetContextPath::new(self.window_id, self.root_id),
            app_state: &self.app_state.0,
            window_state: &self.window_state.0,
            widget_state: &self.widget_state.0,
            update_state: &mut self.update_state.0,
            vars: &self.vars,
        })
    }

    /// Applies pending, `sync`, `vars`, `events` and takes all the update requests.
    ///
    /// Returns the [`ContextUpdates`] a full app would use to update the application.
    pub fn apply_updates(&mut self) -> ContextUpdates {
        let wake_time = self.timers.apply_updates(&self.vars);
        let events = self.events.apply_updates(&self.vars, &mut self.updates);
        self.vars.apply_updates(&mut self.updates);
        let (update, display_update) = self.updates.take_updates();
        ContextUpdates {
            events,
            update,
            display_update,
            wake_time,
        }
    }
}

/// Updates that must be reacted by an app context owner.
#[derive(Debug, Default)]
pub struct ContextUpdates {
    /// Events update to notify.
    ///
    /// When this is not empty [`update`](Self::update) is `true`.
    pub events: Vec<BoxedEventUpdate>,

    /// Update requested.
    pub update: bool,

    /// Display update to notify.
    pub display_update: UpdateDisplayRequest,

    /// Time for the loop to awake and update.
    pub wake_time: Option<Instant>,
}
impl ContextUpdates {
    /// If [`update`](Self::update) or [`display_update`](Self::display_update) where requested.
    #[inline]
    pub fn has_updates(&self) -> bool {
        self.update || self.display_update.is_some()
    }
}
impl std::ops::BitOrAssign for ContextUpdates {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        self.events.extend(rhs.events);
        self.update |= rhs.update;
        self.display_update = rhs.display_update;
        self.wake_time = match (self.wake_time, rhs.wake_time) {
            (None, None) => None,
            (None, Some(t)) | (Some(t), None) => Some(t),
            (Some(a), Some(b)) => Some(a.min(b)),
        };
    }
}
impl std::ops::BitOr for ContextUpdates {
    type Output = Self;

    #[inline]
    fn bitor(mut self, rhs: Self) -> Self {
        self |= rhs;
        self
    }
}

/// A widget context.
pub struct WidgetContext<'a> {
    /// Current widget path.
    pub path: &'a mut WidgetContextPath,

    /// State that lives for the duration of the application.
    pub app_state: &'a mut StateMap,

    /// State that lives for the duration of the window.
    pub window_state: &'a mut StateMap,

    /// State that lives for the duration of the widget.
    pub widget_state: &'a mut StateMap,

    /// State that lives for the duration of the node tree method call in the window.
    ///
    /// This state lives only for the current [`UiNode`](crate::UiNode) method call in all nodes
    /// of the window. You can use this to signal properties and event handlers from nodes that
    /// will be updated further then the current one.
    pub update_state: &'a mut StateMap,

    /// Access to variables.
    pub vars: &'a Vars,
    /// Access to application events.
    pub events: &'a mut Events,
    /// Access to application services.
    pub services: &'a mut Services,

    /// Event loop based timers.
    pub timers: &'a mut Timers,

    /// Schedule of actions to apply after this update.
    pub updates: &'a mut Updates,
}
impl<'a> WidgetContext<'a> {
    /// Runs a function `f` in the context of a widget.
    #[inline(always)]
    pub fn widget_context<R>(
        &mut self,
        widget_id: WidgetId,
        widget_state: &mut OwnedStateMap,
        f: impl FnOnce(&mut WidgetContext) -> R,
    ) -> R {
        self.path.push(widget_id);

        let r = self.vars.with_widget_clear(|| {
            f(&mut WidgetContext {
                path: self.path,

                app_state: self.app_state,
                window_state: self.window_state,
                widget_state: &mut widget_state.0,
                update_state: self.update_state,

                vars: self.vars,
                events: self.events,
                services: self.services,

                timers: self.timers,

                updates: self.updates,
            })
        });

        self.path.pop();

        r
    }
}

/// Current widget context path.
pub struct WidgetContextPath {
    window_id: WindowId,
    widget_ids: Vec<WidgetId>,
}
impl WidgetContextPath {
    fn new(window_id: WindowId, root_id: WidgetId) -> Self {
        let mut widget_ids = Vec::with_capacity(50);
        widget_ids.push(root_id);
        WidgetContextPath { window_id, widget_ids }
    }

    fn push(&mut self, widget_id: WidgetId) {
        self.widget_ids.push(widget_id);
    }

    fn pop(&mut self) {
        debug_assert!(self.widget_ids.len() > 1, "cannot pop root");
        self.widget_ids.pop();
    }

    /// Parent window id.
    #[inline]
    pub fn window_id(&self) -> WindowId {
        self.window_id
    }

    /// Window root widget id.
    #[inline]
    pub fn root_id(&self) -> WidgetId {
        self.widget_ids[0]
    }

    /// Current widget id.
    #[inline]
    pub fn widget_id(&self) -> WidgetId {
        self.widget_ids[self.widget_ids.len() - 1]
    }

    /// Ancestor widgets, parent first.
    #[inline]
    #[allow(clippy::needless_lifetimes)] // clippy bug
    pub fn ancestors<'s>(&'s self) -> impl Iterator<Item = WidgetId> + 's {
        let max = self.widget_ids.len() - 1;
        self.widget_ids[0..max].iter().copied().rev()
    }

    /// Parent widget id.
    #[inline]
    pub fn parent(&self) -> Option<WidgetId> {
        self.ancestors().next()
    }

    /// If the `widget_id` is part of the path.
    #[inline]
    pub fn contains(&self, widget_id: WidgetId) -> bool {
        self.widget_ids.iter().any(move |&w| w == widget_id)
    }
}
impl fmt::Debug for WidgetContextPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            f.debug_struct("WidgetContextPath")
                .field("window_id", &self.window_id)
                .field("widget_ids", &self.widget_ids)
                .finish()
        } else {
            write!(f, "{}", self)
        }
    }
}
impl fmt::Display for WidgetContextPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // "WinId(1)//Wgt(1)/Wgt(23)"
        write!(f, "{}/", self.window_id)?;
        for w in &self.widget_ids {
            write!(f, "/{}", w)?;
        }
        Ok(())
    }
}

/// A widget layout context.
#[derive(Debug)]
pub struct LayoutContext<'a> {
    /// Current computed font size.
    pub font_size: &'a f32,

    /// Computed font size at the root widget.
    pub root_font_size: &'a f32,

    /// Pixel grid of the surface that is rendering the root widget.
    pub pixel_grid: &'a PixelGrid,

    /// Size of the window content.
    pub viewport_size: &'a LayoutSize,
    /// Smallest dimension of the [`viewport_size`](Self::viewport_size).
    pub viewport_min: &'a f32,
    /// Largest dimension of the [`viewport_size`](Self::viewport_size).
    pub viewport_max: &'a f32,

    /// Current widget path.
    pub path: &'a mut WidgetContextPath,

    /// State that lives for the duration of the application.
    pub app_state: &'a mut StateMap,

    /// State that lives for the duration of the window.
    pub window_state: &'a mut StateMap,

    /// State that lives for the duration of the widget.
    pub widget_state: &'a mut StateMap,

    /// State that lives for the duration of the node tree layout update call in the window.
    ///
    /// This state lives only for the sequence of two [`UiNode::measure`](crate::UiNode::measure) and [`UiNode::arrange`](crate::UiNode::arrange)
    /// method calls in all nodes of the window. You can use this to signal nodes that have not participated in the current
    /// layout update yet, or from `measure` signal `arrange`.
    pub update_state: &'a mut StateMap,

    /// Read-only access to variables.
    pub vars: &'a VarsRead,
}
impl<'a> LayoutContext<'a> {
    /// Runs a function `f` in a layout context that has the new computed font size.
    #[inline(always)]
    pub fn with_font_size<R>(&mut self, new_font_size: f32, f: impl FnOnce(&mut LayoutContext) -> R) -> R {
        f(&mut LayoutContext {
            font_size: &new_font_size,
            root_font_size: self.root_font_size,
            pixel_grid: self.pixel_grid,
            viewport_size: self.viewport_size,
            viewport_min: self.viewport_min,
            viewport_max: self.viewport_max,

            path: self.path,

            app_state: self.app_state,
            window_state: self.window_state,
            widget_state: self.widget_state,
            update_state: self.update_state,

            vars: self.vars,
        })
    }

    /// Runs a function `f` in the layout context of a widget.
    #[inline(always)]
    pub fn with_widget<R>(&mut self, widget_id: WidgetId, widget_state: &mut OwnedStateMap, f: impl FnOnce(&mut LayoutContext) -> R) -> R {
        self.path.push(widget_id);

        let r = self.vars.with_widget_clear(|| {
            f(&mut LayoutContext {
                font_size: self.font_size,
                root_font_size: self.root_font_size,
                pixel_grid: self.pixel_grid,
                viewport_size: self.viewport_size,
                viewport_min: self.viewport_min,
                viewport_max: self.viewport_max,

                path: self.path,

                app_state: self.app_state,
                window_state: self.window_state,
                widget_state: &mut widget_state.0,
                update_state: self.update_state,

                vars: self.vars,
            })
        });

        self.path.pop();

        r
    }
}

/// A widget render context.
pub struct RenderContext<'a> {
    /// Current widget path.
    pub path: &'a mut WidgetContextPath,

    /// Read-only access to the state that lives for the duration of the application.
    pub app_state: &'a StateMap,

    /// Read-only access to the state that lives for the duration of the window.
    pub window_state: &'a StateMap,

    /// Read-only access to the state that lives for the duration of the widget.
    pub widget_state: &'a StateMap,

    /// State that lives for the duration of the node tree render or render update call in the window.
    ///
    /// This state lives only for the call to [`UiNode::render`](crate::UiNode::render) or
    /// [`UiNode::render_update`](crate::UiNode::render_update) method call in all nodes of the window.
    /// You can use this to signal nodes that have not rendered yet.
    pub update_state: &'a mut StateMap,

    /// Read-only access to variables.
    pub vars: &'a VarsRead,
}
impl<'a> RenderContext<'a> {
    /// Runs a function `f` in the render context of a widget.
    #[inline(always)]
    pub fn with_widget<R>(&mut self, widget_id: WidgetId, widget_state: &OwnedStateMap, f: impl FnOnce(&mut RenderContext) -> R) -> R {
        self.path.push(widget_id);
        let r = f(&mut RenderContext {
            path: self.path,
            app_state: self.app_state,
            window_state: self.window_state,
            widget_state: &widget_state.0,
            update_state: self.update_state,
            vars: self.vars,
        });
        self.path.pop();
        r
    }
}

macro_rules! contextual_ctx {
    ($($Context:ident),+ $(,)?) => {$(paste::paste! {

#[doc = " Represents a *contextual* reference to [`"$Context"`]."]
///
#[doc = "This type exist to provide access to a [`"$Context"`] inside [`UiTask`](crate::task::UiTask) futures."]
#[doc = "Every time the task updates the executor loads a exclusive reference to the context using the paired [`"$Context"Scope`]"]
/// to provide the context for that update. Inside the future you can then call [`with`](Self::with) to get the exclusive
/// reference to the context.
pub struct [<$Context Mut>] {
    ctx: Rc<[<$Context ScopeData>]>,
}
impl Clone for [<$Context Mut>] {
    fn clone(&self) -> Self {
        Self {
            ctx: Rc::clone(&self.ctx)
        }
    }
}
impl [<$Context Mut>] {
    #[doc = "Runs an action with the *contextual* exclusive borrow to a [`"$Context"`]."]
    ///
    /// ## Panics
    ///
    /// Panics if `with` is called again inside `action`, also panics if not called inside the paired
    #[doc = "[`"$Context"Scope::with`]. You should assume that if you have access to a [`"$Context"Mut`] it is in a valid"]
    /// state, the onus of safety is on the caller.
    #[inline]
    pub fn with<R, A>(&self, action: A) -> R
    where
        A: FnOnce(&mut $Context) -> R,
    {
        if self.ctx.borrowed.get() {
            panic!("already in `{0}Mut::with`, cannot borrow `&mut {0}` twice", stringify!($Context));
        }

        let ptr = self.ctx.ptr.get();
        if ptr.is_null() {
            panic!("no `&mut {0}` loaded for `{0}Mut`", stringify!($Context));
        }

        self.ctx.borrowed.set(true);
        let _r = RunOnDrop::new(|| {
            self.ctx.borrowed.set(false);
        });

        let ctx = unsafe { &mut *(ptr as *mut $Context) };
        action(ctx)
    }
}

#[doc = "Pair of [`"$Context"Mut`] that can setup its reference."]
pub struct [<$Context Scope>] {
    ctx: Rc<[<$Context ScopeData>]>,
}
struct [<$Context ScopeData>] {
    ptr: Cell<*mut ()>,
    borrowed: Cell<bool>,
}
impl [<$Context Scope>] {
    #[doc = "Create a new [`"$Context"Scope`], [`"$Context"Mut`] pair."]
    pub fn new() -> (Self, [<$Context Mut>]) {
        let ctx = Rc::new([<$Context ScopeData>] {
            ptr: Cell::new(ptr::null_mut()),
            borrowed: Cell::new(false)
        });

        (Self { ctx: Rc::clone(&ctx) }, [<$Context Mut>] { ctx })
    }

    #[doc = "Runs `action` while the paired [`"$Context"Mut`] points to `ctx`."]
    pub fn with<R, F>(&self, ctx: &mut $Context, action: F) -> R
    where
        F: FnOnce() -> R,
    {
        let prev = self.ctx.ptr.replace(ctx as *mut $Context as *mut ());
        let _r = RunOnDrop::new(|| {
            self.ctx.ptr.set(prev)
        });
        action()
    }
}

    })+};
}
contextual_ctx!(AppContext, WidgetContext);

impl AppContextMut {
    /// Returns a future that *yields* one update.
    ///
    /// Async event handlers run in app updates, the code each `.await` runs in a different update, but only if
    /// the `.await` does not return immediately. This future always awaits once for each new update, so the
    /// code after awaiting is guaranteed to run in a different update.
    ///
    /// Note that this does not cause an immediate update, if no update was requested it will *wait* until one is.
    /// To force an update and then yield use [`update`](Self::update) instead.
    ///
    /// You can reuse this future but it is very cheap to just make a new one.
    ///
    /// ```
    /// # use zero_ui_core::context::*;
    /// # use zero_ui_core::handler::*;
    /// # fn __() -> impl AppHandler<()> {
    /// async_app_hn!(|ctx, _, _| {
    ///     println!("First update");
    ///     ctx.yield_one().await;
    ///     println!("Second update");
    /// })
    /// # }
    /// ```
    pub fn yield_one(&self) -> YieldOneUpdateFut<Self> {
        YieldOneUpdateFut::new(self)
    }

    /// Requests one update and returns a future that *yields* one update.
    ///
    /// This is like [`yield_one`](Self::yield_one) but also requests the next update, causing the code after
    /// the `.await` to run immediately after one update is processed.
    ///
    /// ```
    /// # use zero_ui_core::context::*;
    /// # use zero_ui_core::handler::*;
    /// # use zero_ui_core::var::*;
    /// # let mut app = zero_ui_core::app::App::blank().run_headless();
    /// let foo_var = var(false);
    /// # app.ctx().updates.run(
    /// async_app_hn_once!(foo_var, |ctx, _| {
    ///     // variable assign will cause an update.
    ///     foo_var.set(&ctx, true);
    ///
    ///     ctx.yield_one().await;// wait next update.
    ///
    ///     // we are in the next update now, the variable value is new.
    ///     assert_eq!(Some(true), foo_var.copy_new(&ctx));
    ///
    ///     ctx.update().await;// force next update and wait.
    ///
    ///     // we are in the requested update, variable value is no longer new.
    ///     assert_eq!(None, foo_var.copy_new(&ctx));
    /// })
    /// # ).permanent();
    /// # app.update(false);
    /// # assert!(foo_var.copy(&app.ctx()));
    /// ```
    ///
    /// In the example above, the variable assign causes an app update so `yield_one` processes it immediately,
    /// but the second `.await` needs to cause an update if we don't want to depend on another part of the app
    /// to awake.
    pub async fn update(&self) {
        self.with(|c| c.updates.update());
        self.yield_one().await
    }
}

impl WidgetContextMut {
    /// Returns a future that *yields* one update.
    ///
    /// Async event handlers run in widget updates, the code each `.await` runs in a different update, but only if
    /// the `.await` does not return immediately. This future always awaits once for each new update, so the
    /// code after awaiting is guaranteed to run in a different update.
    ///
    /// Note that this does not cause an immediate update, if no update was requested it will *wait* until one is.
    /// To force an update and then yield use [`update`](Self::update) instead.
    ///
    /// You can reuse this future but it is very cheap to just make a new one.
    ///
    /// ```
    /// # use zero_ui_core::context::*;
    /// # use zero_ui_core::handler::*;
    /// # fn __() -> impl WidgetHandler<()> {
    /// async_hn!(|ctx, _| {
    ///     println!("First update");
    ///     ctx.yield_one().await;
    ///     println!("Second update");
    /// })
    /// # }
    /// ```
    pub fn yield_one(&self) -> YieldOneUpdateFut<Self> {
        YieldOneUpdateFut::new(self)
    }

    /// Requests one update and returns a future that *yields* one update.
    ///
    /// This is like [`yield_one`](Self::yield_one) but also requests the next update, causing the code after
    /// the `.await` to run immediately after one update is processed.
    ///
    /// ```
    /// # use zero_ui_core::context::*;
    /// # use zero_ui_core::handler::*;
    /// # use zero_ui_core::var::*;
    /// # fn __() -> impl WidgetHandler<()> {
    /// async_hn!(|ctx, _| {
    ///     let foo_var = var(false);
    ///     // variable assign will cause an update.
    ///     foo_var.set(&ctx, true);
    ///
    ///     ctx.yield_one().await;// wait next update.
    ///
    ///     // we are in the next update now, the variable value is new.
    ///     assert_eq!(Some(true), foo_var.copy_new(&ctx));
    ///
    ///     ctx.update().await;// force next update and wait.
    ///
    ///     // we are in the requested update, variable value is no longer new.
    ///     assert_eq!(None, foo_var.copy_new(&ctx));
    /// })
    /// # }
    /// ```
    ///
    /// In the example above, the variable assign causes an app update so `yield_one` processes it immediately,
    /// but the second `.await` needs to cause an update if we don't want to depend on another part of the app
    /// to awake.
    pub async fn update(&self) {
        self.with(|c| c.updates.update());
        self.yield_one().await
    }
}

/// A future that *yields* in UI bound async code.
///
/// Use the `yield_one` method in [`AppContextMut`](AppContextMut::yield_one) or
/// [`WidgetContextMut`](AppContextMut::yield_one) to create this future.
///
/// Async event handlers run in app updates, the code each `.await` runs in a different app update, but only if
/// the `.await` does not return immediately. This future always waits once for each new update, so the
/// code after awaiting is guaranteed to run in a different update.
///
/// Note that this does not cause an immediate subsequent update, if no update was requested it will wait until one is.
/// Updates can be requested using [`AppContextMut::update`] or [`WidgetContextMut::update`], or any of the app interactions
/// that cause update, that is, variable assigns, event notifications, timers, calls to [`Updates::update`] or
/// [`AppEventSender::send_update`] and more.
///
/// You can reuse this future but it is very cheap to just make a new one.
///
/// ```
/// # use zero_ui_core::context::*;
/// # use zero_ui_core::handler::*;
/// # fn __() -> impl WidgetHandler<()> {
/// async_hn!(|ctx, _| {
///     println!("First update");
///     ctx.yield_one().await;
///     println!("Second update");
/// })
/// # }
/// ```
pub struct YieldOneUpdateFut<'a, C: WithUpdates> {
    _ctx: &'a C,
    pending: Cell<bool>,
}
impl<'a, C: WithUpdates> YieldOneUpdateFut<'a, C> {
    #[allow(missing_docs)]
    pub fn new(ctx: &'a C) -> Self {
        YieldOneUpdateFut {
            _ctx: ctx,
            pending: Cell::new(true),
        }
    }
}
impl<'a, C: WithUpdates> Future for YieldOneUpdateFut<'a, C> {
    type Output = ();

    fn poll(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        if self.pending.get() {
            self.pending.set(false);
            std::task::Poll::Pending
        } else {
            self.pending.set(true);
            std::task::Poll::Ready(())
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::app::App;

    use super::*;

    #[test]
    #[should_panic(expected = "already in `AppContextMut::with`, cannot borrow `&mut AppContext` twice")]
    fn context_reentry() {
        let mut app = App::default().run_headless();

        let (scope, ctx) = AppContextScope::new();
        let ctx_a = Rc::new(ctx);
        let ctx_b = Rc::clone(&ctx_a);

        scope.with(&mut app.ctx(), move || {
            ctx_a.with(move |a| {
                ctx_b.with(move |b| {
                    let _invalid: (&mut AppContext, &mut AppContext) = (a, b);
                })
            })
        });
    }
}
