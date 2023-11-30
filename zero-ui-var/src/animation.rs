//! Var animation types and functions.

use std::{
    mem,
    time::{Duration, Instant},
};

use zero_ui_app_context::context_local;
use zero_ui_clone_move::clmv;

use zero_ui_handle::{Handle, HandleOwner, WeakHandle};
use zero_ui_units::{euclid, CornerRadius2D, Deadline, Dip, Px, TimeUnits};
pub use zero_ui_var_proc_macros::Transitionable;

use super::*;

pub mod easing;

#[derive(Default)]
pub(super) struct AnimationHandleData {
    on_drop: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
}
impl Drop for AnimationHandleData {
    fn drop(&mut self) {
        for f in self.on_drop.get_mut().drain(..) {
            f()
        }
    }
}
/// Represents a running animation created by [`Animations.animate`].
///
/// Drop all clones of this handle to stop the animation, or call [`perm`] to drop the handle
/// but keep the animation alive until it is stopped from the inside.
///
/// [`perm`]: AnimationHandle::perm
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[repr(transparent)]
#[must_use = "the animation stops if the handle is dropped"]
pub struct AnimationHandle(Handle<AnimationHandleData>);
impl Default for AnimationHandle {
    /// `dummy`.
    fn default() -> Self {
        Self::dummy()
    }
}
impl AnimationHandle {
    pub(super) fn new() -> (HandleOwner<AnimationHandleData>, Self) {
        let (owner, handle) = Handle::new(AnimationHandleData::default());
        (owner, AnimationHandle(handle))
    }

    /// Create dummy handle that is always in the *stopped* state.
    ///
    /// Note that `Option<AnimationHandle>` takes up the same space as `AnimationHandle` and avoids an allocation.
    pub fn dummy() -> Self {
        AnimationHandle(Handle::dummy(AnimationHandleData::default()))
    }

    /// Drop the handle but does **not** stop.
    ///
    /// The animation stays in memory for the duration of the app or until another handle calls [`stop`](Self::stop).
    pub fn perm(self) {
        self.0.perm();
    }

    /// If another handle has called [`perm`](Self::perm).
    /// If `true` the animation will stay active until the app exits, unless [`stop`](Self::stop) is called.
    pub fn is_permanent(&self) -> bool {
        self.0.is_permanent()
    }

    /// Drops the handle and forces the animation to drop.
    pub fn stop(self) {
        self.0.force_drop();
    }

    /// If another handle has called [`stop`](Self::stop).
    ///
    /// The animation is already dropped or will be dropped in the next app update, this is irreversible.
    pub fn is_stopped(&self) -> bool {
        self.0.is_dropped()
    }

    /// Create a weak handle.
    pub fn downgrade(&self) -> WeakAnimationHandle {
        WeakAnimationHandle(self.0.downgrade())
    }

    /// Register a `handler` to be called once when the animation stops.
    ///
    /// Returns the `handler` if the animation has already stopped.
    ///
    /// [`importance`]: ModifyInfo::importance
    pub fn hook_animation_stop(&self, handler: Box<dyn FnOnce() + Send>) -> Result<(), Box<dyn FnOnce() + Send>> {
        if !self.is_stopped() {
            self.0.data().on_drop.lock().push(handler);
            Ok(())
        } else {
            Err(handler)
        }
    }
}

/// Weak [`AnimationHandle`].
#[derive(Clone, PartialEq, Eq, Hash, Default, Debug)]
pub struct WeakAnimationHandle(pub(super) WeakHandle<AnimationHandleData>);
impl WeakAnimationHandle {
    /// New weak handle that does not upgrade.
    pub fn new() -> Self {
        Self(WeakHandle::new())
    }

    /// Get the animation handle if it is still animating.
    pub fn upgrade(&self) -> Option<AnimationHandle> {
        self.0.upgrade().map(AnimationHandle)
    }
}

struct AnimationData {
    start_time: Instant,
    restart_count: usize,
    stop: bool,
    sleep: Option<Deadline>,
    animations_enabled: bool,
    now: Instant,
    time_scale: Factor,
}

/// Represents an animation in its closure.
///
/// See the [`VARS.animate`] method for more details.
#[derive(Clone)]
pub struct Animation(Arc<Mutex<AnimationData>>);
impl Animation {
    pub(super) fn new(animations_enabled: bool, now: Instant, time_scale: Factor) -> Self {
        Animation(Arc::new(Mutex::new(AnimationData {
            start_time: now,
            restart_count: 0,
            stop: false,
            now,
            sleep: None,
            animations_enabled,
            time_scale,
        })))
    }

    /// Instant this animation (re)started.
    pub fn start_time(&self) -> Instant {
        self.0.lock().start_time
    }

    /// Instant the current animation update started.
    ///
    /// Use this value instead of [`Instant::now`], animations update sequentially, but should behave as if
    /// they are updating exactly in parallel, using this timestamp ensures that.
    pub fn now(&self) -> Instant {
        self.0.lock().now
    }

    /// Global time scale for animations.
    pub fn time_scale(&self) -> Factor {
        self.0.lock().time_scale
    }

    pub(crate) fn reset_state(&self, enabled: bool, now: Instant, time_scale: Factor) {
        let mut m = self.0.lock();
        m.animations_enabled = enabled;
        m.now = now;
        m.time_scale = time_scale;
        m.sleep = None;
    }

    pub(crate) fn reset_sleep(&self) {
        self.0.lock().sleep = None;
    }

    /// Set the duration to the next animation update. The animation will *sleep* until `duration` elapses.
    ///
    /// The animation awakes in the next [`VARS.frame_duration`] after the `duration` elapses, if the sleep duration is not
    /// a multiple of the frame duration it will delay an extra `frame_duration - 1ns` in the worst case. The minimum
    /// possible `duration` is the frame duration, shorter durations behave the same as if not set.
    pub fn sleep(&self, duration: Duration) {
        let mut me = self.0.lock();
        me.sleep = Some(Deadline(me.now + duration));
    }

    pub(crate) fn sleep_deadline(&self) -> Option<Deadline> {
        self.0.lock().sleep
    }

    /// Returns a value that indicates if animations are enabled in the operating system.
    ///
    /// If `false` all animations must be skipped to the end, users with photo-sensitive epilepsy disable animations system wide.
    pub fn animations_enabled(&self) -> bool {
        self.0.lock().animations_enabled
    }

    /// Compute the time elapsed from [`start_time`] to [`now`].
    ///
    /// [`start_time`]: Self::start_time
    /// [`now`]: Self::now
    pub fn elapsed_dur(&self) -> Duration {
        let me = self.0.lock();
        me.now - me.start_time
    }

    /// Compute the elapsed [`EasingTime`], in the span of the total `duration`, if [`animations_enabled`].
    ///
    /// If animations are disabled, returns [`EasingTime::end`], the returned time is scaled.
    ///
    /// [`animations_enabled`]: Self::animations_enabled
    pub fn elapsed(&self, duration: Duration) -> EasingTime {
        let me = self.0.lock();
        if me.animations_enabled {
            EasingTime::elapsed(duration, me.now - me.start_time, me.time_scale)
        } else {
            EasingTime::end()
        }
    }

    /// Compute the elapsed [`EasingTime`], if the time [`is_end`] requests animation stop.
    ///
    /// [`is_end`]: EasingTime::is_end
    pub fn elapsed_stop(&self, duration: Duration) -> EasingTime {
        let t = self.elapsed(duration);
        if t.is_end() {
            self.stop()
        }
        t
    }

    /// Compute the elapsed [`EasingTime`], if the time [`is_end`] restarts the animation.
    ///
    /// [`is_end`]: EasingTime::is_end
    pub fn elapsed_restart(&self, duration: Duration) -> EasingTime {
        let t = self.elapsed(duration);
        if t.is_end() {
            self.restart()
        }
        t
    }

    /// Compute the elapsed [`EasingTime`], if the time [`is_end`] restarts the animation, repeats until has
    /// restarted `max_restarts` inclusive, then stops the animation.
    ///
    /// [`is_end`]: EasingTime::is_end
    pub fn elapsed_restart_stop(&self, duration: Duration, max_restarts: usize) -> EasingTime {
        let t = self.elapsed(duration);
        if t.is_end() {
            if self.restart_count() < max_restarts {
                self.restart();
            } else {
                self.stop();
            }
        }
        t
    }

    /// Drop the animation after applying the current update.
    pub fn stop(&self) {
        self.0.lock().stop = true;
    }

    /// If the animation will be dropped after applying the update.
    pub fn stop_requested(&self) -> bool {
        self.0.lock().stop
    }

    /// Set the animation start time to now.
    pub fn restart(&self) {
        let now = self.0.lock().now;
        self.set_start_time(now);
        let mut me = self.0.lock();
        me.restart_count += 1;
    }

    /// Number of times the animation restarted.
    pub fn restart_count(&self) -> usize {
        self.0.lock().restart_count
    }

    /// Change the start time to an arbitrary value.
    ///
    /// Note that this does not affect the restart count.
    pub fn set_start_time(&self, instant: Instant) {
        self.0.lock().start_time = instant;
    }

    /// Change the start to an instant that computes the `elapsed` for the `duration` at the moment
    /// this method is called.
    ///
    /// Note that this does not affect the restart count.
    pub fn set_elapsed(&self, elapsed: EasingTime, duration: Duration) {
        let now = self.0.lock().now;
        self.set_start_time(now.checked_sub(duration * elapsed.fct()).unwrap());
    }
}

/// A type that can be animated between two values.
///
/// This trait is auto-implemented for all [`Copy`] types that can add, subtract and multiply by [`Factor`], [`Clone`]
/// only types must implement this trait manually.
pub trait Transitionable: VarValue {
    /// Sample the linear interpolation from `self` -> `to` by `step`.  
    fn lerp(self, to: &Self, step: EasingStep) -> Self;
}
impl Transitionable for f64 {
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        self + (*to - self) * step.0 as f64
    }
}
impl Transitionable for f32 {
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        self + (*to - self) * step.0
    }
}
macro_rules! impl_transitionable {
    ($FT:ident => $($T:ty,)+) => {$(
        impl Transitionable for $T {
            fn lerp(self, to: &Self, step: EasingStep) -> Self {
                $FT::lerp(self as $FT, &((*to) as $FT), step).round() as _
            }
        }
    )+}
}
impl_transitionable! {
    f32 => i8, u8, i16, u16, i32, u32,
}
impl_transitionable! {
    f64 => u64, i64, u128, i128, isize, usize,
}
impl Transitionable for Px {
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        Px(self.0.lerp(&to.0, step))
    }
}
impl Transitionable for Dip {
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        Dip::new_f32(self.to_f32().lerp(&to.to_f32(), step))
    }
}
impl<T, U> Transitionable for euclid::Point2D<T, U>
where
    T: Transitionable,
    U: Send + Sync + Any,
{
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        euclid::point2(self.x.lerp(&to.x, step), self.y.lerp(&to.y, step))
    }
}
impl<T, U> Transitionable for euclid::Box2D<T, U>
where
    T: Transitionable,
    U: Send + Sync + Any,
{
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        euclid::Box2D::new(self.min.lerp(&to.min, step), self.max.lerp(&to.max, step))
    }
}
impl<T, U> Transitionable for euclid::Point3D<T, U>
where
    T: Transitionable,
    U: Send + Sync + Any,
{
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        euclid::point3(self.x.lerp(&to.x, step), self.y.lerp(&to.y, step), self.z.lerp(&to.z, step))
    }
}
impl<T, U> Transitionable for euclid::Box3D<T, U>
where
    T: Transitionable,
    U: Send + Sync + Any,
{
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        euclid::Box3D::new(self.min.lerp(&to.min, step), self.max.lerp(&to.max, step))
    }
}
impl<T, U> Transitionable for euclid::Length<T, U>
where
    T: Transitionable,
    U: Send + Sync + Any,
{
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        euclid::Length::new(self.get().lerp(&to.clone().get(), step))
    }
}
impl<T, U> Transitionable for euclid::Size2D<T, U>
where
    T: Transitionable,
    U: Send + Sync + Any,
{
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        euclid::size2(self.width.lerp(&to.width, step), self.height.lerp(&to.height, step))
    }
}
impl<T, U> Transitionable for euclid::Size3D<T, U>
where
    T: Transitionable,
    U: Send + Sync + Any,
{
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        euclid::size3(
            self.width.lerp(&to.width, step),
            self.height.lerp(&to.height, step),
            self.depth.lerp(&to.depth, step),
        )
    }
}
impl<T, U> Transitionable for euclid::Rect<T, U>
where
    T: Transitionable,
    U: Send + Sync + Any,
{
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        euclid::Rect::new(self.origin.lerp(&to.origin, step), self.size.lerp(&to.size, step))
    }
}
impl<T, U> Transitionable for euclid::Vector2D<T, U>
where
    T: Transitionable,
    U: Send + Sync + Any,
{
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        euclid::vec2(self.x.lerp(&to.x, step), self.y.lerp(&to.y, step))
    }
}
impl<T, U> Transitionable for euclid::Vector3D<T, U>
where
    T: Transitionable,
    U: Send + Sync + Any,
{
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        euclid::vec3(self.x.lerp(&to.x, step), self.y.lerp(&to.y, step), self.z.lerp(&to.z, step))
    }
}
impl Transitionable for Factor {
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        Factor(self.0.lerp(&to.0, step))
    }
}
impl<T, U> Transitionable for euclid::SideOffsets2D<T, U>
where
    T: Transitionable,
    U: Send + Sync + Any,
{
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        euclid::SideOffsets2D::new(
            self.top.lerp(&to.top, step),
            self.right.lerp(&to.right, step),
            self.bottom.lerp(&to.bottom, step),
            self.left.lerp(&to.left, step),
        )
    }
}
impl Transitionable for bool {
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        if step >= 1.fct() {
            *to
        } else {
            self
        }
    }
}
impl<T, U> Transitionable for CornerRadius2D<T, U>
where
    T: Transitionable,
    U: Send + Sync + Any,
{
    fn lerp(self, to: &Self, step: EasingStep) -> Self {
        Self {
            top_left: self.top_left.lerp(&to.top_left, step),
            top_right: self.top_right.lerp(&to.top_right, step),
            bottom_right: self.bottom_right.lerp(&to.bottom_right, step),
            bottom_left: self.bottom_left.lerp(&to.bottom_left, step),
        }
    }
}

/// Represents a simple transition between two values.
pub struct Transition<T> {
    /// Value sampled at the `0.fct()` step.
    pub from: T,
    ///
    /// Value sampled at the `1.fct()` step.
    pub to: T,
}
impl<T> Transition<T>
where
    T: Transitionable,
{
    /// New transition.
    pub fn new(from: T, to: T) -> Self {
        Self { from, to }
    }

    /// Compute the transition value at the `step`.
    pub fn sample(&self, step: EasingStep) -> T {
        self.from.clone().lerp(&self.to, step)
    }
}

/// Represents a transition across multiple keyed values that can be sampled using [`EasingStep`].
#[derive(Clone, Debug)]
pub struct TransitionKeyed<T> {
    keys: Vec<(Factor, T)>,
}
impl<T> TransitionKeyed<T>
where
    T: Transitionable,
{
    /// New transition.
    ///
    /// Returns `None` if `keys` is empty.
    pub fn new(mut keys: Vec<(Factor, T)>) -> Option<Self> {
        if keys.is_empty() {
            return None;
        }

        // correct backtracking keyframes.
        for i in 1..keys.len() {
            if keys[i].0 < keys[i - 1].0 {
                keys[i].0 = keys[i - 1].0;
            }
        }

        Some(TransitionKeyed { keys })
    }

    /// Keyed values.
    pub fn keys(&self) -> &[(Factor, T)] {
        &self.keys
    }

    /// Compute the transition value at the `step`.
    pub fn sample(&self, step: EasingStep) -> T {
        if let Some(i) = self.keys.iter().position(|(f, _)| *f > step) {
            if i == 0 {
                // step before first
                self.keys[0].1.clone()
            } else {
                let (from_step, from_value) = self.keys[i - 1].clone();
                if from_step == step {
                    // step exact key
                    from_value
                } else {
                    // linear interpolate between steps

                    let (_, to_value) = &self.keys[i];
                    let step = step - from_step;

                    from_value.lerp(to_value, step)
                }
            }
        } else {
            // step is after last
            self.keys[self.keys.len() - 1].1.clone()
        }
    }
}

pub(super) struct Animations {
    animations: Mutex<Vec<AnimationFn>>,
    animation_imp: usize,
    pub(super) current_modify: ModifyInfo,
    pub(super) animation_start_time: Option<Instant>,
    next_frame: Option<Deadline>,
    pub(super) animations_enabled: ArcVar<bool>,
    pub(super) frame_duration: ArcVar<Duration>,
    pub(super) animation_time_scale: ArcVar<Factor>,
}
impl Animations {
    pub(crate) fn new() -> Self {
        Self {
            animations: Mutex::default(),
            animation_imp: 1,
            current_modify: ModifyInfo {
                handle: None,
                importance: 1,
            },
            animation_start_time: None,
            next_frame: None,
            animations_enabled: var(true),
            frame_duration: var((1.0 / 60.0).secs()),
            animation_time_scale: var(1.fct()),
        }
    }

    pub(super) fn update_animations(timer: &mut impl AnimationTimer) {
        let mut vars = VARS_SV.write();
        if let Some(next_frame) = vars.ans.next_frame {
            if timer.elapsed(next_frame) {
                let mut animations = mem::take(vars.ans.animations.get_mut());
                debug_assert!(!animations.is_empty());

                let info = AnimationUpdateInfo {
                    animations_enabled: vars.ans.animations_enabled.get(),
                    time_scale: vars.ans.animation_time_scale.get(),
                    now: timer.now(),
                    next_frame: next_frame + vars.ans.frame_duration.get(),
                };

                let mut min_sleep = Deadline(info.now + Duration::from_secs(60 * 60));

                drop(vars);

                animations.retain_mut(|animate| {
                    if let Some(sleep) = animate(info) {
                        min_sleep = min_sleep.min(sleep);
                        true
                    } else {
                        false
                    }
                });

                let mut vars = VARS_SV.write();

                let self_animations = vars.ans.animations.get_mut();
                if !self_animations.is_empty() {
                    min_sleep = Deadline(info.now);
                }
                animations.append(self_animations);
                *self_animations = animations;

                if !self_animations.is_empty() {
                    vars.ans.next_frame = Some(min_sleep);
                    timer.register(min_sleep);
                } else {
                    vars.ans.next_frame = None;
                }
            }
        }
    }

    pub(super) fn next_deadline(timer: &mut impl AnimationTimer) {
        if let Some(next_frame) = VARS_SV.read().ans.next_frame {
            timer.register(next_frame);
        }
    }

    pub(crate) fn animate<A>(mut animation: A) -> AnimationHandle
    where
        A: FnMut(&Animation) + Send + 'static,
    {
        let mut vars = VARS_SV.write();

        // # Modify Importance
        //
        // Variables only accept modifications from an importance (IMP) >= the previous IM that modified it.
        //
        // Direct modifications always overwrite previous animations, so we advance the IMP for each call to
        // this method **and then** advance the IMP again for all subsequent direct modifications.
        //
        // Example sequence of events:
        //
        // |IM| Modification  | Accepted
        // |--|---------------|----------
        // | 1| Var::set      | YES
        // | 2| Var::ease     | YES
        // | 2| ease update   | YES
        // | 3| Var::set      | YES
        // | 3| Var::set      | YES
        // | 2| ease update   | NO
        // | 4| Var::ease     | YES
        // | 2| ease update   | NO
        // | 4| ease update   | YES
        // | 5| Var::set      | YES
        // | 2| ease update   | NO
        // | 4| ease update   | NO

        // ensure that all animations started in this update have the same exact time, we update then with the same `now`
        // timestamp also, this ensures that synchronized animations match perfectly.
        let start_time = if let Some(t) = vars.ans.animation_start_time {
            t
        } else {
            let t = Instant::now();
            vars.ans.animation_start_time = Some(t);
            t
        };

        let mut anim_imp = None;
        if let Some(c) = VARS_MODIFY_CTX.get_clone() {
            if c.is_animating() {
                // nested animation uses parent importance.
                anim_imp = Some(c.importance);
            }
        }
        let anim_imp = match anim_imp {
            Some(i) => i,
            None => {
                // not nested, advance base imp
                let mut imp = vars.ans.animation_imp.wrapping_add(1);
                if imp == 0 {
                    imp = 1;
                }

                let mut next_imp = imp.wrapping_add(1);
                if next_imp == 0 {
                    next_imp = 1;
                }

                vars.ans.animation_imp = next_imp;
                vars.ans.current_modify.importance = next_imp;

                imp
            }
        };

        let (handle_owner, handle) = AnimationHandle::new();
        let weak_handle = handle.downgrade();

        let controller = VARS_ANIMATION_CTRL_CTX.get();

        let anim = Animation::new(vars.ans.animations_enabled.get(), start_time, vars.ans.animation_time_scale.get());

        drop(vars);

        controller.on_start(&anim);
        let mut controller = Some(controller);
        let mut anim_modify_info = Some(Arc::new(Some(ModifyInfo {
            handle: Some(weak_handle.clone()),
            importance: anim_imp,
        })));

        let mut vars = VARS_SV.write();

        vars.ans.animations.get_mut().push(Box::new(move |info| {
            let _handle_owner = &handle_owner; // capture and own the handle owner.

            if weak_handle.upgrade().is_some() {
                if anim.stop_requested() {
                    // drop
                    controller.as_ref().unwrap().on_stop(&anim);
                    return None;
                }

                if let Some(sleep) = anim.sleep_deadline() {
                    if sleep > info.next_frame {
                        // retain sleep
                        return Some(sleep);
                    } else if sleep.0 > info.now {
                        // sync-up to frame rate after sleep
                        anim.reset_sleep();
                        return Some(info.next_frame);
                    }
                }

                anim.reset_state(info.animations_enabled, info.now, info.time_scale);

                VARS_ANIMATION_CTRL_CTX.with_context(&mut controller, || {
                    VARS_MODIFY_CTX.with_context(&mut anim_modify_info, || animation(&anim))
                });

                // retain until next frame
                //
                // stop or sleep may be requested after this (during modify apply),
                // these updates are applied on the next frame.
                Some(info.next_frame)
            } else {
                // drop
                controller.as_ref().unwrap().on_stop(&anim);
                None
            }
        }));

        vars.ans.next_frame = Some(Deadline(Instant::now()));

        vars.wake_app();

        handle
    }
}

type AnimationFn = Box<dyn FnMut(AnimationUpdateInfo) -> Option<Deadline> + Send>;

#[derive(Clone, Copy)]
struct AnimationUpdateInfo {
    animations_enabled: bool,
    now: Instant,
    time_scale: Factor,
    next_frame: Deadline,
}

pub(super) fn var_animate<T: VarValue>(
    target: &impl Var<T>,
    animate: impl FnMut(&Animation, &mut VarModify<T>) + Send + 'static,
) -> AnimationHandle {
    if !target.capabilities().is_always_read_only() {
        let target = target.clone().actual_var();
        if !target.capabilities().is_always_read_only() {
            // target var can be animated.

            let wk_target = target.downgrade();
            let animate = Arc::new(Mutex::new(animate));

            return VARS.animate(move |args| {
                // animation

                if let Some(target) = wk_target.upgrade() {
                    // target still exists

                    if target.modify_importance() > VARS.current_modify().importance {
                        // var modified by a more recent animation or directly, this animation cannot
                        // affect it anymore.
                        args.stop();
                        return;
                    }

                    // try update
                    let r = target.modify(clmv!(animate, args, |value| {
                        (animate.lock())(&args, value);
                    }));

                    if let Err(VarIsReadOnlyError { .. }) = r {
                        // var can maybe change to allow write again, but we wipe all animations anyway.
                        args.stop();
                    }
                } else {
                    // target dropped.
                    args.stop();
                }
            });
        }
    }
    AnimationHandle::dummy()
}

pub(super) fn var_sequence<T: VarValue, V: Var<T>>(
    target: &V,
    animate: impl FnMut(&<<V::ActualVar as Var<T>>::Downgrade as WeakVar<T>>::Upgrade) -> AnimationHandle + Send + 'static,
) -> VarHandle {
    if !target.capabilities().is_always_read_only() {
        let target = target.clone().actual_var();
        if !target.capabilities().is_always_read_only() {
            // target var can be animated.

            let animate = Arc::new(Mutex::new(animate));

            let (handle, handle_hook) = VarHandle::new(Box::new(|_| true));

            let wk_target = target.downgrade();
            let controller = OnStopController(clmv!(animate, wk_target, || {
                if let Some(target) = wk_target.upgrade() {
                    if target.modify_importance() <= VARS.current_modify().importance()
                        && handle_hook.is_alive()
                        && VARS.animations_enabled().get()
                    {
                        (animate.lock())(&target).perm();
                    }
                }
            }));

            VARS.with_animation_controller(controller, || {
                if let Some(target) = wk_target.upgrade() {
                    (animate.lock())(&target).perm();
                }
            });

            return handle;
        }
    }
    VarHandle::dummy()
}

pub(super) fn var_set_ease_with<T>(
    start_value: T,
    end_value: T,
    duration: Duration,
    easing: impl Fn(EasingTime) -> EasingStep + Send + 'static,
    init_step: EasingStep, // set to 0 skips first frame, set to 999 includes first frame.
    sampler: impl Fn(&Transition<T>, EasingStep) -> T + Send + 'static,
) -> impl FnMut(&Animation, &mut VarModify<T>) + Send
where
    T: VarValue + Transitionable,
{
    let transition = Transition::new(start_value, end_value);
    let mut prev_step = init_step;
    move |a, vm| {
        let step = easing(a.elapsed_stop(duration));

        if prev_step != step {
            vm.set(sampler(&transition, step));
            prev_step = step;
        }
    }
}

pub(super) fn var_set_ease_keyed_with<T>(
    transition: TransitionKeyed<T>,
    duration: Duration,
    easing: impl Fn(EasingTime) -> EasingStep + Send + 'static,
    init_step: EasingStep,
    sampler: impl Fn(&TransitionKeyed<T>, EasingStep) -> T + Send + 'static,
) -> impl FnMut(&Animation, &mut VarModify<T>) + Send
where
    T: VarValue + Transitionable,
{
    let mut prev_step = init_step;
    move |a, value| {
        let step = easing(a.elapsed_stop(duration));

        if prev_step != step {
            value.set(sampler(&transition, step));
            prev_step = step;
        }
    }
}

pub(super) fn var_step<T>(new_value: T, delay: Duration) -> impl FnMut(&Animation, &mut VarModify<T>)
where
    T: VarValue,
{
    let mut new_value = Some(new_value);
    move |a, vm| {
        if !a.animations_enabled() || a.elapsed_dur() >= delay {
            a.stop();
            if let Some(nv) = new_value.take() {
                vm.set(nv);
            }
        } else {
            a.sleep(delay);
        }
    }
}

pub(super) fn var_step_oci<T>(values: [T; 2], delay: Duration, mut count: usize) -> impl FnMut(&Animation, &mut VarModify<T>)
where
    T: VarValue,
{
    let mut first = false;
    move |a, vm| {
        if !a.animations_enabled() || a.elapsed_dur() >= delay {
            if first {
                vm.set(values[0].clone());
            } else {
                vm.set(values[1].clone());
            }
            first = !first;

            if count == 0 {
                a.stop();
            } else {
                count -= 1;
            }
        }
        a.sleep(delay);
    }
}

pub(super) fn var_steps<T: VarValue>(
    steps: Vec<(Factor, T)>,
    duration: Duration,
    easing: impl Fn(EasingTime) -> EasingStep + 'static,
) -> impl FnMut(&Animation, &mut VarModify<T>) {
    let mut prev_step = 999.fct();
    move |a, vm| {
        let step = easing(a.elapsed_stop(duration));
        if step != prev_step {
            prev_step = step;
            if let Some(val) = steps.iter().find(|(f, _)| *f >= step).map(|(_, step)| step.clone()) {
                vm.set(val);
            }
        }
    }
}

/// Represents the editable final value of a [`Var::chase`] animation.
pub struct ChaseAnimation<T: VarValue + animation::Transitionable> {
    target: T,
    var: BoxedVar<T>,
    handle: animation::AnimationHandle,
}
impl<T> fmt::Debug for ChaseAnimation<T>
where
    T: VarValue + animation::Transitionable,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChaseAnimation")
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}
impl<T> ChaseAnimation<T>
where
    T: VarValue + animation::Transitionable,
{
    /// Current animation target.
    pub fn target(&self) -> &T {
        &self.target
    }

    /// Modify the chase target, replaces the animation with a new one from the current value to the modified target.
    pub fn modify(&mut self, modify: impl FnOnce(&mut T), duration: Duration, easing: impl Fn(EasingTime) -> EasingStep + Send + 'static) {
        if self.handle.is_stopped() {
            // re-sync target
            self.target = self.var.get();
        }
        modify(&mut self.target);
        self.handle = self.var.ease(self.target.clone(), duration, easing);
    }

    /// Replace the chase target, replaces the animation with a new one from the current value to the modified target.
    pub fn set(&mut self, value: impl Into<T>, duration: Duration, easing: impl Fn(EasingTime) -> EasingStep + Send + 'static) {
        self.target = value.into();
        self.handle = self.var.ease(self.target.clone(), duration, easing);
    }
}

pub(super) fn var_chase<T>(
    var: BoxedVar<T>,
    first_target: T,
    duration: Duration,
    easing: impl Fn(EasingTime) -> EasingStep + Send + 'static,
) -> ChaseAnimation<T>
where
    T: VarValue + animation::Transitionable,
{
    ChaseAnimation {
        handle: var.ease(first_target.clone(), duration, easing),
        target: first_target,
        var,
    }
}

/// Represents the current *modify* operation when it is applying.
#[derive(Clone)]
pub struct ModifyInfo {
    handle: Option<WeakAnimationHandle>,
    importance: usize,
}
impl ModifyInfo {
    /// Initial value, is always of lowest importance.
    pub fn never() -> Self {
        ModifyInfo {
            handle: None,
            importance: 0,
        }
    }

    /// Indicates the *override* importance of the operation, when two animations target
    /// a variable only the newer one must apply, and all running animations are *overridden* by
    /// a later modify/set operation.
    ///
    /// Variables ignore modify requests from lower importance closures.
    pub fn importance(&self) -> usize {
        self.importance
    }

    /// Indicates if the *modify* request was made from inside an animation, if `true` the [`importance`]
    /// is for that animation, even if the modify request is from the current frame.
    ///
    /// You can clone this info to track this animation, when it stops or is dropped this returns `false`. Note
    /// that *paused* animations still count as animating.
    ///
    /// [`importance`]: Self::importance
    pub fn is_animating(&self) -> bool {
        self.handle.as_ref().map(|h| h.upgrade().is_some()).unwrap_or(false)
    }

    /// Returns `true` if `self` and `other` have the same animation or are both not animating.
    pub fn animation_eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }

    /// Register a `handler` to be called once when the current animation stops.
    ///
    /// [`importance`]: Self::importance
    pub fn hook_animation_stop(&self, handler: Box<dyn FnOnce() + Send>) -> Result<(), Box<dyn FnOnce() + Send>> {
        if let Some(h) = &self.handle {
            if let Some(h) = h.upgrade() {
                return h.hook_animation_stop(handler);
            }
        }
        Err(handler)
    }
}
impl fmt::Debug for ModifyInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModifyInfo")
            .field("is_animating()", &self.is_animating())
            .field("importance()", &self.importance)
            .finish()
    }
}

/// Animations controller.
///
/// See [`VARS.with_animation_controller`] for more details.
pub trait AnimationController: Send + Sync + Any {
    /// Animation started.
    fn on_start(&self, animation: &Animation) {
        let _ = animation;
    }

    /// Animation stopped.
    fn on_stop(&self, animation: &Animation) {
        let _ = animation;
    }
}

/// An [`AnimationController`] that does nothing.
pub struct NilAnimationObserver;
impl AnimationController for NilAnimationObserver {}

struct OnStopController<F>(F)
where
    F: Fn() + Send + Sync + 'static;
impl<F> AnimationController for OnStopController<F>
where
    F: Fn() + Send + Sync + 'static,
{
    fn on_stop(&self, _: &Animation) {
        (self.0)()
    }
}

context_local! {
    pub(crate) static VARS_ANIMATION_CTRL_CTX: Box<dyn AnimationController> = {
        let r: Box<dyn AnimationController> = Box::new(NilAnimationObserver);
        r
    };
}

/// System settings that control animations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationsConfig {
    /// If animation are enabled.
    ///
    /// People with photo-sensitive epilepsy usually disable animations system wide.
    pub enabled: bool,

    /// Interval of the caret blink animation.
    pub caret_blink_interval: Duration,
    /// Duration after which the blink animation stops.
    pub caret_blink_timeout: Duration,
}
impl Default for AnimationsConfig {
    /// true, 530ms, 5s.
    fn default() -> Self {
        Self {
            enabled: true,
            caret_blink_interval: Duration::from_millis(530),
            caret_blink_timeout: Duration::from_secs(5),
        }
    }
}

/// View on an app loop timer.
pub trait AnimationTimer {
    /// Returns `true` if the `deadline` has elapsed, `false` if the `deadline` was
    /// registered for future waking.
    fn elapsed(&mut self, deadline: Deadline) -> bool;

    /// Register the future `deadline` for waking.
    fn register(&mut self, deadline: Deadline);

    /// Frame timestamp.
    fn now(&self) -> Instant;
}