//! App thread timers, deadlines and timeouts.
//!
//! The primary `struct` of this module is [`Timers`]. You can use it to
//! create UI bound timers that run using only the main thread and can awake the app event loop
//! to notify updates.
use parking_lot::Mutex;
use std::{
    fmt, mem,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, Instant},
};

use retain_mut::RetainMut;

use crate::{
    context::AppContext,
    crate_util::{Handle, HandleOwner, WeakHandle},
    handler::{self, AppHandler, AppHandlerArgs, AppWeakHandle},
    var::{var, RcVar, ReadOnlyVar, Var, Vars, WeakVar},
};

struct DeadlineHandlerEntry {
    handle: HandleOwner<DeadlineState>,
    handler: Box<dyn FnMut(&mut AppContext, &dyn AppWeakHandle)>,
    pending: bool,
}

struct TimerHandlerEntry {
    handle: HandleOwner<TimerState>,
    handler: Box<dyn FnMut(&mut AppContext, &TimerArgs, &dyn AppWeakHandle)>,
    pending: Option<Instant>, // the `Instant` is the last expected deadline
}

struct TimerVarEntry {
    handle: HandleOwner<TimerState>,
    weak_var: WeakVar<Timer>,
}

/// App thread timers, deadlines and timeouts.
///
/// An instance of this `struct` is available in the [`AppContext`] and derived contexts. You can use it to
/// create UI bound timers, these timers run using only the main thread and can awake the app event loop
/// to notify updates.
///
/// Timer updates can be observed using variables that update when the timer elapses, or you can register
/// handlers to be called directly when the time elapses. Timers can be *one-time*, updating only once when
/// a [`deadline`](Timers::deadline) is reached or a [`timeout`](Timers::timeout) elapses; or they can update every time on a
/// set [`interval`](Timers::interval).
///
/// # Async
///
/// Timers generated by this `struct` are not [`Send`] and are bound to the UI thread, however you can `.await` then in UI bound async
/// code, like in async event handlers, by using the [variable] async update methods. You can also register async handlers for the
/// callback timers using the [`async_app_hn!`](crate::handler::async_app_hn!) or [`async_app_hn_once!`](crate::async_app_hn_once!)
/// macros.
///
/// To create timers that work in any thread and independent from the running app use the [`task`] module timers functions.
///
/// [variable]: Var
/// [`task`]: crate::task
pub struct Timers {
    deadlines: Vec<WeakVar<Deadline>>,
    timers: Vec<TimerVarEntry>,
    deadline_handlers: Vec<DeadlineHandlerEntry>,
    timer_handlers: Vec<TimerHandlerEntry>,
}
impl Timers {
    pub(crate) fn new() -> Self {
        Timers {
            deadlines: vec![],
            timers: vec![],
            deadline_handlers: vec![],
            timer_handlers: vec![],
        }
    }

    /// Returns a [`DeadlineVar`] that will update once when the `deadline` is reached.
    ///
    /// If the `deadline` is in the past the variable will still update once in the next app update.
    /// Drop all clones of the variable to cancel the timer.
    ///
    /// ```
    /// # use zero_ui_core::timer::*;
    /// # use zero_ui_core::handler::*;
    /// # use zero_ui_core::units::*;
    /// # use zero_ui_core::var::*;
    /// # use zero_ui_core::context::WidgetContext;
    /// # use std::time::Instant;
    /// # fn foo(ctx: &mut WidgetContext) {
    /// let deadline = ctx.timers.deadline(Instant::now() + 20.secs());
    ///
    /// # let
    /// text = deadline.map(|d| if d.elapsed { "20 seconds have passed" } else { "..." });
    /// # }
    /// ```
    ///
    /// In the example above the deadline variable starts with [`elapsed`](Deadline::elapsed) set to `false`,
    /// 20 seconds later the variable will update with [`elapsed`](Deadline::elapsed) set to `true`. The variable
    /// is read-only and will only update once.
    #[inline]
    #[must_use]
    pub fn deadline(&mut self, deadline: Instant) -> DeadlineVar {
        let timer = var(Deadline { deadline, elapsed: false });
        self.deadlines.push(timer.downgrade());
        timer.into_read_only()
    }

    /// Returns a [`DeadlineVar`] that will update once when the `timeout` has elapsed.
    ///
    /// This method just calculates the [`deadline`](Self::deadline), from the time this method is called plus `timeout`.
    #[inline]
    #[must_use]
    pub fn timeout(&mut self, timeout: Duration) -> DeadlineVar {
        self.deadline(Instant::now() + timeout)
    }

    /// Returns a [`TimerVar`] that will update every time the `interval` elapses.
    ///
    /// The timer can be controlled using methods in the variable value.
    ///
    /// ```
    /// # use zero_ui_core::timer::*;
    /// # use zero_ui_core::handler::*;
    /// # use zero_ui_core::units::*;
    /// # use zero_ui_core::var::*;
    /// # use zero_ui_core::text::*;
    /// # use zero_ui_core::context::WidgetContext;
    /// # use std::time::Instant;
    /// # fn foo(ctx: &mut WidgetContext) {
    /// let timer = ctx.timers.interval(1.secs());
    ///
    /// # let
    /// text = timer.map(|t| match t.count() {
    ///     0 => formatx!(""),
    ///     1 => formatx!("1 second elapsed"),
    ///     c => formatx!("{} seconds elapsed", c)
    /// });
    /// # }
    /// ```
    ///
    /// In the example above the timer variable will update every second, the variable keeps a [`count`](Timer::count)
    /// of times the time elapsed, that is incremented every update. The variable is read-only but the value can
    /// be used to control the timer to some extent, see [`TimerVar`] for details.
    #[inline]
    #[must_use]
    pub fn interval(&mut self, interval: Duration) -> TimerVar {
        let (owner, handle) = TimerHandle::new(interval);
        let timer = var(Timer(handle));
        self.timers.push(TimerVarEntry {
            handle: owner,
            weak_var: timer.downgrade(),
        });
        timer.into_read_only()
    }

    /// Register a `handler` that will be called once when the `deadline` is reached.
    ///
    /// If the `deadline` is in the past the `handler` will be called in the next app update.
    ///
    /// ```
    /// # use zero_ui_core::timer::*;
    /// # use zero_ui_core::handler::*;
    /// # use zero_ui_core::units::*;
    /// # use zero_ui_core::context::AppContext;
    /// # use std::time::Instant;
    /// # fn foo(ctx: &mut AppContext) {
    /// let handle = ctx.timers.on_deadline(Instant::now() + 20.secs(), app_hn_once!(|ctx, _| {
    ///     println!("20 seconds have passed");
    /// }));
    /// # }
    /// ```
    ///
    /// # Handler
    ///
    /// The `handler` can be any of the *once* [`AppHandler`] implementers. You can use the macros
    /// [`app_hn_once!`](crate::handler::app_hn_once!) or [`async_hn_once!`](crate::handler::async_app_hn_once!)
    /// to declare a handler closure.
    ///
    /// Async handlers execute up to the first `.await` immediately when the `deadline` is reached, subsequent awakes
    /// are scheduled like an async *preview* event handler.
    ///
    /// # Handle
    ///
    /// Returns a [`DeadlineHandle`] that can be used to cancel the timer, either by dropping the handle or by
    /// calling [`cancel`](DeadlineHandle::cancel). You can also call [`permanent`](DeadlineHandle::permanent)
    /// to drop the handle without cancelling.
    pub fn on_deadline<H>(&mut self, deadline: Instant, mut handler: H) -> DeadlineHandle
    where
        H: AppHandler<DeadlineArgs> + handler::marker::OnceHn,
    {
        let (handle_owner, handle) = DeadlineHandle::new(deadline);
        self.deadline_handlers.push(DeadlineHandlerEntry {
            handle: handle_owner,
            handler: Box::new(move |ctx, handle| {
                handler.event(
                    ctx,
                    &DeadlineArgs {
                        timestamp: Instant::now(),
                        deadline,
                    },
                    &AppHandlerArgs { handle, is_preview: true },
                )
            }),
            pending: false,
        });
        handle
    }

    /// Register a `handler` that will be called once when `timeout` elapses.
    ///
    /// This method just calculates the deadline for [`on_deadline`](Self::on_deadline). The deadline is calculated
    /// from the time this method is called plus `timeout`.
    pub fn on_timeout<H>(&mut self, timeout: Duration, handler: H) -> DeadlineHandle
    where
        H: AppHandler<DeadlineArgs> + handler::marker::OnceHn,
    {
        self.on_deadline(Instant::now() + timeout, handler)
    }

    /// Register a `handler` that will be called every time the `interval` elapses.
    pub fn on_interval<H>(&mut self, interval: Duration, mut handler: H) -> TimerHandle
    where
        H: AppHandler<TimerArgs>,
    {
        let (owner, handle) = TimerHandle::new(interval);

        self.timer_handlers.push(TimerHandlerEntry {
            handle: owner,
            handler: Box::new(move |ctx, args, handle| {
                handler.event(ctx, args, &AppHandlerArgs { handle, is_preview: true });
            }),
            pending: None,
        });
        handle
    }

    /// Update timer vars, flag handlers to be called in [`Self::notify`], returns new app wake time.
    pub(crate) fn apply_updates(&mut self, vars: &Vars) -> Option<Instant> {
        let now = Instant::now();

        let mut min_next_some = false;
        let mut min_next = now + Duration::from_secs(60 * 60 * 60);

        // update `deadline` vars
        self.deadlines.retain(|wk| {
            if let Some(var) = wk.upgrade() {
                let deadline = var.get(vars).deadline;
                if deadline > now {
                    return true; // retain
                }
                var.modify(vars, |t| t.elapsed = true);
            }
            false // don't retain
        });

        // update `interval` vars
        self.timers.retain(|t| {
            if let Some(var) = t.weak_var.upgrade() {
                if !t.handle.is_dropped() {
                    let timer = var.get(vars);
                    let mut deadline = timer.0 .0.data().deadline.lock();
                    if deadline.next_deadline() <= now {
                        // timer elapses, but only update if is enabled:
                        if timer.is_enabled() {
                            timer.0 .0.data().count.fetch_add(1, Ordering::Relaxed);
                            var.touch(vars);
                        }

                        deadline.last = now;
                    }

                    min_next_some = true;
                    min_next = min_next.min(deadline.next_deadline());

                    return true; // retain, has at least one var and did not call stop.
                }
            }
            false // don't retain.
        });

        // flag `on_deadline` handlers that need to run.
        self.deadline_handlers.retain_mut(|e| {
            if e.handle.is_dropped() {
                return false; // cancel
            }

            let deadline = e.handle.data().deadline;
            e.pending = deadline <= now;

            if !e.pending {
                min_next_some = true;
                min_next = min_next.min(deadline);
            }

            true // retain if not canceled, elapsed deadlines will be dropped in [`Self::notify`].
        });

        // flag `on_interval` handlers that need to run.
        self.timer_handlers.retain_mut(|e| {
            if e.handle.is_dropped() {
                return false; // stop
            }

            let state = e.handle.data();
            let mut deadline = state.deadline.lock();
            if deadline.next_deadline() <= now {
                // timer elapsed, but only flag for handler call if is enabled:
                if state.enabled.load(Ordering::Relaxed) {
                    // this is wrapping_add
                    state.count.fetch_add(1, Ordering::Relaxed);
                    e.pending = Some(deadline.next_deadline());
                }
                deadline.last = now;
            }

            min_next_some = true;
            min_next = min_next.min(deadline.next_deadline());

            true // retain if stop was not called
        });

        if min_next_some {
            Some(min_next)
        } else {
            None
        }
    }

    /// does on_* notifications.
    pub(crate) fn notify(ctx: &mut AppContext) {
        // we need to detach the handlers from the AppContext, so we can pass the context for then
        // so we `mem::take` for the duration of the call. But new timers can be registered inside
        // the handlers, so we add those handlers using `extend`.

        // call `on_deadline` handlers.
        let mut handlers = mem::take(&mut ctx.timers.deadline_handlers);
        handlers.retain_mut(|h| {
            if h.pending {
                (h.handler)(ctx, &h.handle.weak_handle());
                h.handle.data().executed.store(true, Ordering::Relaxed);
            }
            !h.pending // drop if just called, deadline handlers are *once*.
        });
        handlers.append(&mut ctx.timers.deadline_handlers);
        ctx.timers.deadline_handlers = handlers;

        // call `on_interval` handlers.
        let mut handlers = mem::take(&mut ctx.timers.timer_handlers);
        handlers.retain_mut(|h| {
            if let Some(deadline) = h.pending.take() {
                let args = TimerArgs {
                    timestamp: Instant::now(),
                    deadline,
                    wk_handle: h.handle.weak_handle(),
                };
                (h.handler)(ctx, &args, &h.handle.weak_handle());
            }

            !h.handle.is_dropped() // drop if called stop inside the handler.
        });
        handlers.append(&mut ctx.timers.timer_handlers);
        ctx.timers.timer_handlers = handlers;
    }
}

/// Represents the state of a [`DeadlineVar`].
#[derive(Debug, Clone)]
pub struct Deadline {
    /// Deadline for the timer to elapse, this value does not change.
    pub deadline: Instant,
    /// If the timer has elapsed, the initial value is `false`, once the timer elapses the value is updated to `true`.
    pub elapsed: bool,
}

/// A [`deadline`](Timers::deadline) or [`timeout`](Timers::timeout) timer.
///
/// This is a read-only variable of type [`Deadline`], it will update once when the timer elapses.
///
/// Drop all clones of this variable to cancel the timer.
///
/// ```
/// # use zero_ui_core::timer::*;
/// # use zero_ui_core::handler::*;
/// # use zero_ui_core::units::*;
/// # use zero_ui_core::var::*;
/// # use zero_ui_core::context::WidgetContext;
/// # use std::time::Instant;
/// # fn foo(ctx: &mut WidgetContext) {
/// let deadline: DeadlineVar = ctx.timers.deadline(Instant::now() + 20.secs());
///
/// # let
/// text = deadline.map(|d| if d.elapsed { "20 seconds have passed" } else { "..." });
/// # }
/// ```
///
/// In the example above the variable is mapped to a text, there are many other things you can do with variables,
/// including `.await` for the update in UI bound async tasks. See [`Var`] for details.
pub type DeadlineVar = ReadOnlyVar<Deadline, RcVar<Deadline>>;

/// Represents a [`on_deadline`](Timers::on_deadline) or [`on_timeout`](Timers::on_timeout) handler.
///
/// Drop all clones of this handle to cancel the timer, or call [`permanent`](Self::permanent) to drop the handle
/// without cancelling the timer.
#[derive(Clone)]
#[must_use = "the timer is canceled if the handler is dropped"]
pub struct DeadlineHandle(Handle<DeadlineState>);
struct DeadlineState {
    deadline: Instant,
    executed: AtomicBool,
}
impl DeadlineHandle {
    /// Create a handle to nothing, the handle always in the *canceled* state.
    pub fn dummy() -> DeadlineHandle {
        DeadlineHandle(Handle::dummy(DeadlineState {
            deadline: Instant::now(),
            executed: AtomicBool::new(false),
        }))
    }

    fn new(deadline: Instant) -> (HandleOwner<DeadlineState>, Self) {
        let (owner, handle) = Handle::new(DeadlineState {
            deadline,
            executed: AtomicBool::new(false),
        });
        (owner, DeadlineHandle(handle))
    }

    /// Drops the handle but does **not** drop the handler closure.
    ///
    /// The handler closure will be dropped after it is executed or when the app shutdown.
    #[inline]
    pub fn permanent(self) {
        self.0.permanent();
    }

    /// If [`permanent`](Self::permanent) was called in another handle.
    /// If `true` the closure will be dropped when it executes, when the app shutdown or if [`cancel`](Self::cancel) is called.
    #[inline]
    pub fn is_permanent(&self) -> bool {
        self.0.is_permanent()
    }

    /// Drops the handle and forces the handler to drop.
    ///
    /// If the deadline has not been reached the handler will not be called, and will drop in the next app update.
    #[inline]
    pub fn cancel(self) {
        self.0.force_drop();
    }

    /// The timeout deadline.
    ///
    /// The handler is called once when this deadline is reached.
    #[inline]
    pub fn deadline(&self) -> Instant {
        self.0.data().deadline
    }

    /// If the handler has executed. The handler executes once when the deadline is reached.
    #[inline]
    pub fn has_executed(&self) -> bool {
        self.0.data().executed.load(Ordering::Relaxed)
    }

    /// If the timeout handler will never execute. Returns `true` if [`cancel`](Self::cancel) was called
    /// before the handler could execute.
    #[inline]
    pub fn is_canceled(&self) -> bool {
        !self.has_executed() && self.0.is_dropped()
    }
}

/// Arguments for the handler of [`on_deadline`](Timers::on_deadline) or [`on_timeout`](Timers::on_timeout).
#[derive(Clone, Debug)]
pub struct DeadlineArgs {
    /// When the handler was called.
    pub timestamp: Instant,
    /// Timer deadline, is less-or-equal to the [`timestamp`](Self::timestamp).
    pub deadline: Instant,
}

/// Represents a [`on_interval`](Timers::on_interval) handler.
///
/// Drop all clones of this handler to stop the timer, or call [`permanent`](Self::permanent) to drop the handler
/// without cancelling the timer.
#[derive(Clone)]
#[must_use = "the timer is stopped if the handler is dropped"]
pub struct TimerHandle(Handle<TimerState>);
struct TimerState {
    enabled: AtomicBool,
    deadline: Mutex<TimerDeadline>,
    count: AtomicUsize,
}
struct TimerDeadline {
    interval: Duration,
    last: Instant,
}
impl TimerDeadline {
    fn next_deadline(&self) -> Instant {
        self.last + self.interval
    }
}
impl TimerHandle {
    fn new(interval: Duration) -> (HandleOwner<TimerState>, TimerHandle) {
        let (owner, handle) = Handle::new(TimerState {
            enabled: AtomicBool::new(true),
            deadline: Mutex::new(TimerDeadline {
                interval,
                last: Instant::now(),
            }),
            count: AtomicUsize::new(0),
        });
        (owner, TimerHandle(handle))
    }

    /// Create a handle to nothing, the handle is always in the *stopped* state.
    pub fn dummy() -> TimerHandle {
        TimerHandle(Handle::dummy(TimerState {
            enabled: AtomicBool::new(false),
            deadline: Mutex::new(TimerDeadline {
                interval: Duration::MAX,
                last: Instant::now(),
            }),
            count: AtomicUsize::new(0),
        }))
    }

    /// Drops the handle but does **not** drop the handler closure.
    ///
    /// The handler closure will be dropped when the app shutdown or if it is stopped from the inside or using another handle.
    #[inline]
    pub fn permanent(self) {
        self.0.permanent();
    }

    /// If [`permanent`](Self::permanent) was called in another handle.
    /// If `true` the closure will keep being called until the app shutdown or the timer is stopped from the inside or using
    /// another handle.
    #[inline]
    pub fn is_permanent(&self) -> bool {
        self.0.is_permanent()
    }

    /// Drops the handle and forces the handler to drop.
    ///
    /// The handler will no longer be called and will drop in the next app update.
    #[inline]
    pub fn stop(self) {
        self.0.force_drop();
    }

    /// If the timer was stopped. The timer can be stopped from the inside, from another handle calling [`stop`](Self::stop)
    /// or from the app shutting down.
    #[inline]
    pub fn is_stopped(&self) -> bool {
        self.0.is_dropped()
    }

    /// The timer interval. Enabled handlers are called every time this interval elapses.
    #[inline]
    pub fn interval(&self) -> Duration {
        self.0.data().deadline.lock().interval
    }

    /// Sets the [`interval`](Self::interval).
    ///
    /// Note that this method does not awake the app, so if this is called from outside the app
    /// thread it will only apply on the next app update.
    #[inline]
    pub fn set_interval(&self, new_interval: Duration) {
        self.0.data().deadline.lock().interval = new_interval;
    }

    /// Last elapsed time, or the start time if the timer has not elapsed yet.
    #[inline]
    pub fn timestamp(&self) -> Instant {
        self.0.data().deadline.lock().last
    }

    /// The next deadline.
    ///
    /// This is the [`timestamp`](Self::timestamp) plus the [`interval`](Self::interval).
    #[inline]
    pub fn deadline(&self) -> Instant {
        self.0.data().deadline.lock().next_deadline()
    }

    /// If the handler is called when the timer elapses.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.0.data().enabled.load(Ordering::Relaxed)
    }

    /// Disable or re-enable the timer. Disabled timers don't can call their handler and don't increase the [`count`](Self::count).
    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.data().enabled.store(enabled, Ordering::Relaxed);
    }

    /// Count incremented by one every time the timer elapses and it is [`enabled`](Self::count).
    #[inline]
    pub fn count(&self) -> usize {
        self.0.data().count.load(Ordering::Relaxed)
    }

    /// Resets the [`count`](Self::count).
    #[inline]
    pub fn set_count(&self, count: usize) {
        self.0.data().count.store(count, Ordering::Relaxed)
    }
}

/// An [`interval`](Timers::interval) timer.
///
/// This is a variable of type [`Timer`], it will update every time the timer elapses.
///
/// Drop all clones of this variable to stop the timer, you can also control the timer
/// with methods in the [`Timer`] value even though the variable is read-only.
///
/// ```
/// # use zero_ui_core::timer::*;
/// # use zero_ui_core::handler::*;
/// # use zero_ui_core::units::*;
/// # use zero_ui_core::var::*;
/// # use zero_ui_core::text::*;
/// # use zero_ui_core::context::WidgetContext;
/// # use std::time::Instant;
/// # fn foo(ctx: &mut WidgetContext) {
/// let timer: TimerVar = ctx.timers.interval(1.secs());
///
/// # let
/// text = timer.map(|d| match 20 - d.count() {
///     0 => {
///         d.stop();
///         formatx!("Done!")
///     },
///     1 => formatx!("1 second left"),
///     s => formatx!("{} seconds left", s)
/// });
/// # }
/// ```
///
/// In the example above the variable updates every second and stops after 20 seconds have elapsed. The variable
/// is mapped to a text and controls the timer from inside the mapping closure. See [`Var`] for other things you
/// can do with variables, including `.await` for updates. Also see [`Timer`] for more timer control methods.
pub type TimerVar = ReadOnlyVar<Timer, RcVar<Timer>>;

/// Represents a timer state in a [`TimerVar`] or interval handler.
///
/// This type uses interior mutability to communicate with the timer, the values provided by the methods
/// can be changed anytime by the [`TimerVar`] owners without the variable updating.
#[derive(Clone)]
pub struct Timer(TimerHandle);
impl fmt::Debug for Timer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Timer")
            .field("interval", &self.interval())
            .field("count", &self.count())
            .field("enabled", &self.is_enabled())
            .field("is_stopped", &self.is_stopped())
            .finish_non_exhaustive()
    }
}
impl Timer {
    /// Permanently stops the timer.
    #[inline]
    pub fn stop(&self) {
        self.0.clone().stop();
    }

    /// If the timer was stopped.
    ///
    /// If `true` the timer var will not update again, this is permanent.
    #[inline]
    pub fn is_stopped(&self) -> bool {
        self.0.is_stopped()
    }

    /// The timer interval. Enabled variables update every time this interval elapses.
    #[inline]
    pub fn interval(&self) -> Duration {
        self.0.interval()
    }

    /// Sets the [`interval`](Self::interval).
    ///
    /// Note that this method does not awake the app, so if this is called from outside the app
    /// thread it will only apply on the next app update.
    #[inline]
    pub fn set_interval(&self, new_interval: Duration) {
        self.0.set_interval(new_interval)
    }

    /// Last update time, or the start time if the timer has not updated yet.
    #[inline]
    pub fn timestamp(&self) -> Instant {
        self.0.timestamp()
    }

    /// The next deadline.
    ///
    /// This is the [`timestamp`](Self::timestamp) plus the [`interval`](Self::interval).
    #[inline]
    pub fn deadline(&self) -> Instant {
        self.0.deadline()
    }

    /// If the timer variable updates when the time elapses.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.0.is_enabled()
    }

    /// Disable or re-enable the timer. Disabled timers don't update.
    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Count incremented by one every time the timer elapses and it is [`enabled`](Self::count).
    #[inline]
    pub fn count(&self) -> usize {
        self.0.count()
    }

    /// Resets the [`count`](Self::count).
    #[inline]
    pub fn set_count(&self, count: usize) {
        self.0.set_count(count)
    }
}

/// Arguments for an [`on_interval`](Timers::on_interval) handler.
///
/// Note the timer can be stopped using the handlers [`unsubscribe`](crate::handler::AppWeakHandle::unsubscribe),
/// and *once* handlers stop the timer automatically.
///
/// The field values are about the specific call to handler that received the args, the methods on the other hand
/// are **connected** with the timer by a weak reference and always show the up-to-date state of the timer.
/// For synchronous handlers this does not matter, but for async handlers this means that the values can be
/// different after each `.await`. This can be useful to for example, disable the timer until the async task finishes
/// but it can also be surprising.
#[derive(Clone)]
pub struct TimerArgs {
    /// When the handler was called.
    pub timestamp: Instant,

    /// Expected deadline, is less-or-equal to the [`timestamp`](Self::timestamp).
    pub deadline: Instant,

    wk_handle: WeakHandle<TimerState>,
}

impl TimerArgs {
    fn handle(&self) -> Option<TimerHandle> {
        self.wk_handle.upgrade().map(TimerHandle)
    }

    /// The timer interval. Enabled handlers are called every time this interval elapses.
    #[inline]
    pub fn interval(&self) -> Duration {
        self.handle().map(|h| h.interval()).unwrap_or_default()
    }

    /// Set the [`interval`](Self::interval).
    ///
    /// Note that this method does not awake the app, so if this is called from outside the app
    /// thread it will only apply on the next app update.
    #[inline]
    pub fn set_interval(&self, new_interval: Duration) {
        if let Some(h) = self.handle() {
            h.set_interval(new_interval)
        }
    }

    /// If the handler is called when the time elapses.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.handle().map(|h| h.is_enabled()).unwrap_or(false)
    }

    /// Disable or re-enable the timer. Disabled timers don't call the handler.
    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        if let Some(h) = self.handle() {
            h.set_enabled(enabled);
        }
    }

    /// Count incremented by one every time the timer elapses and it is [`enabled`](Self::count).
    #[inline]
    pub fn count(&self) -> usize {
        self.handle().map(|h| h.count()).unwrap_or(0)
    }

    /// Resets the [`count`](Self::count).
    #[inline]
    pub fn set_count(&self, count: usize) {
        if let Some(h) = self.handle() {
            h.set_count(count)
        }
    }

    /// The timestamp of the last update. This can be different from [`timestamp`](Self::timestamp)
    /// after the first `.await` in async handlers of if called from a different thread.
    #[inline]
    pub fn last_timestamp(&self) -> Instant {
        self.handle().map(|h| h.timestamp()).unwrap_or(self.timestamp)
    }

    /// The next timer deadline.
    ///
    /// This is [`last_timestamp`](Self::last_timestamp) plus [`interval`](Self::interval).
    #[inline]
    pub fn next_deadline(&self) -> Instant {
        self.handle().map(|h| h.deadline()).unwrap_or(self.deadline)
    }

    /// If the timer was stopped while the handler was running after it started handling.
    ///
    /// Note the timer can be stopped from the inside of the handler using the handlers
    /// [`unsubscribe`](crate::handler::AppWeakHandle::unsubscribe), and once handlers stop the timer automatically.
    ///
    /// Outside of the handler the [`TimerHandle`] can be used to stop the timer at any time, even from another thread.
    #[inline]
    pub fn is_stopped(&self) -> bool {
        self.handle().is_none()
    }
}
