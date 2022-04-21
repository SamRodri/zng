use std::{
    cell::{Cell, UnsafeCell},
    rc::Rc,
};

use once_cell::unsync::OnceCell;

use crate::widget_info::UpdateSlot;

use super::*;

/// A clone-on-write variable.
///
/// This variable returns the value of another variable until it is set or modified. When
/// it is set or modified it clones the value and detaches from the source variable and
/// behaves like the [`RcVar<T>`].
///
/// You can use this variable in contexts where a value is *inherited* from a source but
/// can optionally be overridden locally.
///
/// # Examples
///
/// The example has two variables `source` and `test` at the beginning when `source` updates the
/// update is visible in `test`, but after `test` is assigned it disconnects from `source` and
/// contains its own value.
///
/// ```
/// # use zero_ui_core::{var::*, handler::*, context::*};
/// # TestWidgetContext::doc_test((),
/// async_hn!(|ctx, _| {
///     let source = var(0u8);
///     let test = RcCowVar::new(source.clone());
///
///     // update in source is visible in test var:
///     source.set(&ctx, 1);
///     ctx.update().await;
///     // both are new
///     assert_eq!(source.copy_new(&ctx).unwrap(), test.copy_new(&ctx).unwrap());
///     // test var is not cloned
///     assert!(!test.is_cloned(&ctx));
///
///     // update test var directly, disconnecting it from source:
///     test.set(&ctx, 2);
///     ctx.update().await;
///     // only test is new
///     assert!(!source.is_new(&ctx));
///     assert_eq!(Some(2), test.copy_new(&ctx));
///     // it is now cloned
///     assert!(test.is_cloned(&ctx));
///
///     // the source no longer updates the test:
///     source.set(&ctx, 3);
///     ctx.update().await;
///     assert!(!test.is_new(&ctx));
/// })
/// # );
/// ```
pub struct RcCowVar<T, V>(Rc<CowData<T, V>>);
bitflags! {
    struct Flags: u8 {
        const SOURCE_ALWAYS_READ_ONLY = 0b_0000_0001;
        const SOURCE_CAN_UPDATE =       0b_0000_0010;
        const SOURCE_IS_CONTEXTUAL =    0b_0000_0100;
        const IS_PASS_THROUGH =         0b_0000_1000;
        const IS_ANIMATING =            0b_0001_0000;
    }
}
struct CowData<T, V> {
    source: UnsafeCell<Option<V>>,
    flags: Cell<Flags>,
    is_contextual: Cell<bool>,
    update_mask: OnceCell<UpdateMask>,

    value: UnsafeCell<Option<T>>,
    version: VarVersionCell,
    last_update_id: Cell<u32>,
}
impl<T: VarValue, V: Var<T>> Clone for RcCowVar<T, V> {
    fn clone(&self) -> Self {
        RcCowVar(Rc::clone(&self.0))
    }
}
impl<T: VarValue, V: Var<T>> RcCowVar<T, V> {
    /// Returns a new var that reads from `source`.
    #[inline]
    pub fn new(source: V) -> Self {
        Self::new_(source, false)
    }

    /// Returns a new [`RcCowVar`] that **is not clone-on-write**.
    ///
    /// Modifying the returned variable modifies the `source`. You can use this to
    /// avoid boxing variables in methods that can return either the source variable
    /// or an override variable.
    #[inline]
    pub fn pass_through(source: V) -> Self {
        Self::new_(source, true)
    }

    fn new_(source: V, is_pass_through: bool) -> Self {
        let mut flags = Flags::empty();
        if source.always_read_only() {
            flags.insert(Flags::SOURCE_ALWAYS_READ_ONLY);
        }
        if source.can_update() {
            flags.insert(Flags::SOURCE_CAN_UPDATE);
        }
        if source.is_contextual() {
            flags.insert(Flags::SOURCE_IS_CONTEXTUAL);
        }
        if is_pass_through {
            flags.insert(Flags::IS_PASS_THROUGH);
        }

        RcCowVar(Rc::new(CowData {
            update_mask: OnceCell::default(),
            flags: Cell::new(flags),
            is_contextual: Cell::new(source.is_contextual()),
            source: UnsafeCell::new(Some(source)),
            value: UnsafeCell::new(None),
            version: VarVersionCell::new(0),
            last_update_id: Cell::new(0),
        }))
    }

    /// Returns `true` if this variable value is a cloned local.
    ///
    /// When this is `false` the value is read from another variable, when it is `true` it is read from local value.
    pub fn is_cloned<Vr: WithVarsRead>(&self, vars: &Vr) -> bool {
        vars.with_vars_read(|v| self.source(v).is_none())
    }

    fn source<'a>(&'a self, _vars: &'a VarsRead) -> Option<&'a V> {
        // SAFETY: this is safe because we are holding a reference to vars and
        // variables require the mutable reference to vars for modifying.
        unsafe { &*self.0.source.get() }.as_ref()
    }

    /// Returns `true` if **the source variable is written* when modifying this variable.
    ///
    /// You can use [`pass_through`] to create a pass-through variable.
    ///
    /// [`pass_through`]: Self::pass_through
    #[inline]
    pub fn is_pass_through(&self) -> bool {
        self.0.flags.get().contains(Flags::IS_PASS_THROUGH)
    }

    /// Reference the current value.
    ///
    /// The value can be from the source variable or a local clone if [`is_cloned`].
    ///
    /// [`is_cloned`]: Self::is_cloned
    #[inline]
    pub fn get<'a, Vr: AsRef<VarsRead>>(&'a self, vars: &'a Vr) -> &'a T {
        let vars = vars.as_ref();

        if let Some(source) = self.source(vars) {
            source.get(vars)
        } else {
            // SAFETY: this is safe because we are tying the `Vars` lifetime to the value
            // and we require `&mut Vars` to modify the value.
            unsafe { &*self.0.value.get() }.as_ref().unwrap()
        }
    }

    /// Reference the current value if it [`is_new`].
    ///
    /// [`is_new`]: Self::is_new
    pub fn get_new<'a, Vw: AsRef<Vars>>(&'a self, vars: &'a Vw) -> Option<&'a T> {
        let vars = vars.as_ref();

        if let Some(source) = self.source(vars) {
            source.get_new(vars)
        } else if self.0.last_update_id.get() == vars.update_id() {
            Some(self.get(vars))
        } else {
            None
        }
    }

    /// If the current value changed in the last update.
    ///
    /// Returns `true` is the source variable is new or if [`is_cloned`] returns if the
    /// value was set in the previous update.
    ///
    /// [`is_cloned`]: Self::is_cloned
    #[inline]
    pub fn is_new<Vw: WithVars>(&self, vars: &Vw) -> bool {
        vars.with_vars(|vars| {
            if let Some(source) = self.source(vars) {
                source.is_new(vars)
            } else {
                self.0.last_update_id.get() == vars.update_id()
            }
        })
    }

    /// Gets the current value version.
    ///
    /// Returns the source variable version of if [`is_cloned`] returns the cloned version.
    /// The source version is copied and incremented by one on the first *write*. Subsequent
    /// *writes* increment the version by one.
    ///
    /// [`is_cloned`]: Self::is_cloned
    #[inline]
    pub fn version<Vr: WithVarsRead>(&self, vars: &Vr) -> VarVersion {
        vars.with_vars_read(|vars| {
            if let Some(source) = self.source(vars) {
                source.version(vars)
            } else {
                self.0.version.get()
            }
        })
    }

    /// Returns `false` unless [`is_pass_through`] and the source variable is read-only.
    ///
    /// [`is_pass_through`]: Self::is_pass_through.
    #[inline]
    pub fn is_read_only<Vw: WithVars>(&self, vars: &Vw) -> bool {
        self.is_pass_through() && self.is_read_only(vars)
    }

    #[inline]
    fn is_animating<Vr: WithVarsRead>(&self, vars: &Vr) -> bool {
        vars.with_vars_read(|vars| {
            if let Some(s) = self.source(vars) {
                s.is_animating(vars)
            } else {
                self.0.flags.get().contains(Flags::IS_ANIMATING)
            }
        })
    }

    /// Schedule a value modification for this variable.
    ///
    /// If [`is_pass_through`] pass the `modify` to the source variable, otherwise
    /// clones the source variable and modifies that value on the first call and then
    /// modified that cloned value in subsequent calls.
    ///
    /// Can return an error only if [`is_pass_through`], otherwise always succeeds.
    ///
    /// [`is_pass_through`]: Self::is_pass_through
    #[inline]
    pub fn modify<Vw, M>(&self, vars: &Vw, modify: M) -> Result<(), VarIsReadOnly>
    where
        Vw: WithVars,
        M: FnOnce(VarModify<T>) + 'static,
    {
        vars.with_vars(|vars| {
            if let Some(source) = self.source(vars) {
                if self.is_pass_through() {
                    return source.modify(vars, modify);
                }

                // SAFETY: this is safe because the `value` is not touched when `source` is some.
                let value = unsafe { &mut *self.0.value.get() };
                if value.is_none() {
                    *value = Some(source.get_clone(vars));
                    self.0.version.set(source.version(vars));
                }
            }

            let self_ = self.clone();
            let is_animating = vars.is_animating();
            vars.push_change::<T>(Box::new(move |update_id| {
                // SAFETY: this is safe because Vars requires a mutable reference to apply changes.
                // the `modifying` flag is only used for `deep_clone`.
                unsafe {
                    *self_.0.source.get() = None;
                }
                self_.0.is_contextual.set(false);
                let mut touched = false;
                modify(VarModify::new(unsafe { &mut *self_.0.value.get() }.as_mut().unwrap(), &mut touched));
                if touched {
                    self_.0.last_update_id.set(update_id);
                    self_.0.version.set(self_.0.version.get().wrapping_add(1));

                    let mut flags = self_.0.flags.get();
                    flags.set(Flags::IS_ANIMATING, is_animating);
                    self_.0.flags.set(flags);

                    *self_.0.update_mask.get_or_init(|| UpdateSlot::next().mask())
                } else {
                    UpdateMask::none()
                }
            }));

            Ok(())
        })
    }

    /// Causes the variable to notify update without changing the value.
    ///
    /// This counts as a *write* so unless [`is_pass_through`] is `true` the value will
    /// be cloned and [`is_cloned`] set to `true` on touch.
    ///
    /// Can return an error only if [`is_pass_through`], otherwise always succeeds.
    ///
    /// [`is_pass_through`]: Self::is_pass_through
    /// [`is_cloned`]: Self::is_cloned
    #[inline]
    pub fn touch<Vw: WithVars>(&self, vars: &Vw) -> Result<(), VarIsReadOnly> {
        self.modify(vars, |mut v| v.touch())
    }

    /// Schedule a new value for this variable.
    ///
    /// If [`is_pass_through`] pass the `new_value` to the source variable, otherwise
    /// the `new_value` will become the variable value on the next update. Unlike [`modify`]
    /// and [`touch`] this method never clones the source variable.
    ///
    /// Can return an error only if [`is_pass_through`], otherwise always succeeds.
    ///
    /// [`is_pass_through`]: Self::is_pass_through
    /// [`modify`]: Self::modify
    /// [`touch`]: Self::touch
    #[inline]
    pub fn set<Vw, N>(&self, vars: &Vw, new_value: N) -> Result<(), VarIsReadOnly>
    where
        Vw: WithVars,
        N: Into<T>,
    {
        let new_value = new_value.into();
        vars.with_vars(|vars| {
            if let Some(source) = self.source(vars) {
                if self.is_pass_through() {
                    return source.set(vars, new_value);
                }

                // SAFETY: this is safe because the `value` is not touched when `source` is some.
                unsafe {
                    *self.0.value.get() = Some(new_value);
                }
                self.0.version.set(source.version(vars));

                let self_ = self.clone();
                vars.push_change::<T>(Box::new(move |update_id| {
                    // SAFETY: this is safe because Vars requires a mutable reference to apply changes.
                    // the `modifying` flag is only used for `deep_clone`.
                    unsafe {
                        *self_.0.source.get() = None;
                    }
                    self_.0.last_update_id.set(update_id);
                    self_.0.version.set(self_.0.version.get().wrapping_add(1));

                    *self_.0.update_mask.get_or_init(|| UpdateSlot::next().mask())
                }));
            } else {
                let self_ = self.clone();
                vars.push_change::<T>(Box::new(move |update_id| {
                    // SAFETY: this is safe because Vars requires a mutable reference to apply changes.
                    // the `modifying` flag is only used for `deep_clone`.
                    unsafe {
                        *self_.0.value.get() = Some(new_value);
                    }
                    self_.0.last_update_id.set(update_id);
                    self_.0.version.set(self_.0.version.get().wrapping_add(1));

                    *self_.0.update_mask.get_or_init(|| UpdateSlot::next().mask())
                }));
            }

            Ok(())
        })
    }

    /// Schedule a transition animation for the variable.
    ///
    /// After the current app update finishes the variable will start animation from the current value to `new_value`
    /// for the `duration` and transitioning by the `easing` function.
    pub fn ease<Vw, N, D, F>(&self, vars: &Vw, new_value: N, duration: D, easing: F)
    where
        Vw: WithVars,
        N: Into<T>,
        D: Into<Duration>,
        F: Fn(EasingTime) -> EasingStep + 'static,

        T: Transitionable,
    {
        let _ = <Self as Var<T>>::ease(self, vars, new_value, duration, easing);
    }

    /// Schedule a transition animation for the variable, but only if the current value is not equal to `new_value`.
    ///
    /// The variable is also updated using [`set_ne`] during animation. Returns `true` is scheduled an animation.
    ///
    /// [`set_ne`]: Self::set_ne
    pub fn ease_ne<Vw, N, D, F>(&self, vars: &Vw, new_value: N, duration: D, easing: F)
    where
        Vw: WithVars,
        N: Into<T>,
        D: Into<Duration>,
        F: Fn(EasingTime) -> EasingStep + 'static,

        T: PartialEq + Transitionable,
    {
        let _ = <Self as Var<T>>::ease_ne(self, vars, new_value, duration, easing);
    }

    /// Schedule a transition animation for the variable, from `new_value` to `then`.
    ///
    /// After the current app update finishes the variable will be set to `new_value`, then start animation from `new_value`
    /// to `then` for the `duration` and transitioning by the `easing` function.
    pub fn set_ease<Vw, N, Th, D, F>(&self, vars: &Vw, new_value: N, then: Th, duration: D, easing: F)
    where
        Vw: WithVars,
        N: Into<T>,
        Th: Into<T>,
        D: Into<Duration>,
        F: Fn(EasingTime) -> EasingStep + 'static,

        T: Transitionable,
    {
        let _ = <Self as Var<T>>::set_ease(self, vars, new_value, then, duration, easing);
    }

    /// Schedule a transition animation for the variable, from `new_value` to `then`, but checks for equality at every step.
    ///
    /// The variable is also updated using [`set_ne`] during animation. Returns `true` is scheduled an animation.
    ///
    /// [`set_ne`]: Self::set_ne
    pub fn set_ease_ne<Vw, N, Th, D, F>(&self, vars: &Vw, new_value: N, then: Th, duration: D, easing: F)
    where
        Vw: WithVars,
        N: Into<T>,
        Th: Into<T>,
        D: Into<Duration>,
        F: Fn(EasingTime) -> EasingStep + 'static,

        T: PartialEq + Transitionable,
    {
        let _ = <Self as Var<T>>::set_ease_ne(self, vars, new_value, then, duration, easing);
    }

    /// Schedule a keyframed transition animation for the variable.
    ///
    /// After the current app update finishes the variable will start animation from the current value to the first key
    /// in `keys`, going across all keys for the `duration`. The `easing` function applies across all keyframes, the interpolation
    /// between keys is linear, use a full animation to control the easing between keys.
    pub fn ease_keyed<Vw, D, F>(&self, vars: &Vw, keys: Vec<(Factor, T)>, duration: D, easing: F)
    where
        Vw: WithVars,
        D: Into<Duration>,
        F: Fn(EasingTime) -> EasingStep + 'static,

        T: Transitionable,
    {
        let _ = <Self as Var<T>>::ease_keyed(self, vars, keys, duration, easing);
    }

    /// Schedule a keyframed transition animation for the variable, starting from the first key.
    ///
    /// After the current app update finishes the variable will be set to to the first keyframe, then animated
    /// across all other keys.
    pub fn set_ease_keyed<Vw, D, F>(&self, vars: &Vw, keys: Vec<(Factor, T)>, duration: D, easing: F)
    where
        Vw: WithVars,
        D: Into<Duration>,
        F: Fn(EasingTime) -> EasingStep + 'static,

        T: Transitionable,
    {
        let _ = <Self as Var<T>>::set_ease_keyed(self, vars, keys, duration, easing);
    }
}
impl<T: VarValue, V: Var<T>> crate::private::Sealed for RcCowVar<T, V> {}
impl<T: VarValue, V: Var<T>> Var<T> for RcCowVar<T, V> {
    type AsReadOnly = ReadOnlyVar<T, Self>;

    fn get<'a, Vr: AsRef<VarsRead>>(&'a self, vars: &'a Vr) -> &'a T {
        self.get(vars)
    }

    fn get_new<'a, Vw: AsRef<Vars>>(&'a self, vars: &'a Vw) -> Option<&'a T> {
        self.get_new(vars)
    }

    fn is_new<Vw: WithVars>(&self, vars: &Vw) -> bool {
        self.is_new(vars)
    }

    fn version<Vr: WithVarsRead>(&self, vars: &Vr) -> VarVersion {
        self.version(vars)
    }

    fn is_read_only<Vw: WithVars>(&self, vars: &Vw) -> bool {
        self.is_read_only(vars)
    }

    #[inline]
    fn is_animating<Vr: WithVarsRead>(&self, vars: &Vr) -> bool {
        self.is_animating(vars)
    }

    fn strong_count(&self) -> usize {
        Rc::strong_count(&self.0)
    }

    /// Returns `false` unless [`is_pass_through`] and the source variable is always read-only.
    ///
    /// [`is_pass_through`]: Self::is_pass_through
    fn always_read_only(&self) -> bool {
        self.is_pass_through() && self.0.flags.get().contains(Flags::SOURCE_ALWAYS_READ_ONLY)
    }

    /// Returns `true` if is still reading from the source variable and it is contextual, otherwise returns `false`.
    #[inline]
    fn is_contextual(&self) -> bool {
        self.0.is_contextual.get()
    }

    /// Returns `true` unless [`is_pass_through`] and the source variable cannot update.
    ///
    /// [`is_pass_through`]: Self::is_pass_through
    fn can_update(&self) -> bool {
        !self.is_pass_through() || self.0.flags.get().contains(Flags::SOURCE_CAN_UPDATE)
    }

    fn into_value<Vr: WithVarsRead>(self, vars: &Vr) -> T {
        match Rc::try_unwrap(self.0) {
            Ok(v) => {
                if let Some(source) = v.source.into_inner() {
                    source.into_value(vars)
                } else {
                    v.value.into_inner().unwrap()
                }
            }
            Err(v) => RcCowVar(v).get_clone(vars),
        }
    }

    fn modify<Vw, M>(&self, vars: &Vw, modify: M) -> Result<(), VarIsReadOnly>
    where
        Vw: WithVars,
        M: FnOnce(VarModify<T>) + 'static,
    {
        self.modify(vars, modify)
    }

    fn set<Vw, N>(&self, vars: &Vw, new_value: N) -> Result<(), VarIsReadOnly>
    where
        Vw: WithVars,
        N: Into<T>,
    {
        self.set(vars, new_value)
    }

    fn touch<Vw: WithVars>(&self, vars: &Vw) -> Result<(), VarIsReadOnly> {
        self.touch(vars)
    }

    fn into_read_only(self) -> Self::AsReadOnly {
        ReadOnlyVar::new(self)
    }

    fn update_mask<Vr: WithVarsRead>(&self, vars: &Vr) -> UpdateMask {
        *vars.with_vars_read(|vars| {
            self.0.update_mask.get_or_init(|| {
                if let Some(source) = self.source(vars) {
                    source.update_mask(vars)
                } else {
                    UpdateSlot::next().mask()
                }
            })
        })
    }
}
impl<T: VarValue, V: Var<T>> IntoVar<T> for RcCowVar<T, V> {
    type Var = Self;

    fn into_var(self) -> Self::Var {
        self
    }
}
