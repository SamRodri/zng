use super::*;

use std::cell::Cell;
use std::rc::{Rc, Weak};

#[cfg(not(dyn_closure))]
use std::marker::PhantomData;

///<span data-del-macro-root></span> Initializes a new conditional var.
///
/// A condition var updates when the first `true` condition changes or the mapped var for the current condition changes.
///
/// # Syntax
///
/// The macro expects a list of `condition-var => condition-value-var`, the list is separated by comma.
/// The last condition must be the `_` token that maps to the value for when none of the conditions are `true`.
///
/// The `condition-var` must be an expression that evaluates to an `impl Var<bool>` type. The `condition-value-var` must
/// by any type that implements `IntoVar`. All condition values must be of the same [`VarValue`] type.
///
/// # Examples
///
/// ```
/// # use zero_ui_core::var::*;
/// # use zero_ui_core::text::*;
/// # fn text(text: impl IntoVar<Text>) { }
/// let condition = var(true);
/// let when_false = var("condition: false".to_text());
///
/// let t = text(when_var! {
///     condition.clone() => "condition: true".to_text(),
///     _ => when_false.clone(),
/// });
/// ```
///
/// In the example if `condition` or `when_false` are modified the text updates.
///
/// # `cfg`
///
/// Every condition can be annotated with attributes, including `#[cfg(..)]`.
///
/// ```
/// # use zero_ui_core::var::*;
/// # use zero_ui_core::text::*;
/// # fn text(text: impl IntoVar<Text>) { }
/// # let condition0 = var(true);
/// # let condition1 = var(true);
/// let t = text(when_var! {
///     #[cfg(some_flag)]
///     condition0 => "is condition 0".to_text(),
///     #[cfg(not(some_flag))]
///     condition1 => "is condition 1".to_text(),
///     _ => "is default".to_text(),
/// });
/// ```
///
/// In the example above only one of the conditions will be compiled, the generated variable is the same
/// type as if you had written a single condition.
///
/// # Return Type
///
/// The return type is an opaque `impl Var<T>` that can one of two implementations depending on the compilation profile, you
/// can use the [`rc_when_var!`] macro to ensure that a [`RcWhenVar<T>`] is created.
#[macro_export]
macro_rules! when_var {
    ($($tt:tt)*) => {
        $crate::var::types::__when_var! {
            $crate::var
            false
            $($tt)*
        }.as_impl_var()
    }
}

///<span data-del-macro-root></span> Initializes a new conditional var.
///
/// This macro uses the same syntax as [`when_var!`], but returns an [`RcWhenVar<T>`] instead of an opaque `impl Var<T>`.
#[macro_export]
macro_rules! rc_when_var {
    ($($tt:tt)*) => {
        $crate::var::types::__when_var! {
            $crate::var
            true
            $($tt)*
        }
    }
}

#[doc(inline)]
pub use crate::{rc_when_var, when_var};

#[doc(hidden)]
pub use zero_ui_proc_macros::when_var as __when_var;

#[cfg(not(dyn_closure))]
macro_rules! impl_rc_when_var {
    ($(
        $len:tt => $($n:tt),+;
    )+) => {$(
        $crate::paste!{
            impl_rc_when_var!{
                Var: [<RcWhen $len Var>];// RcWhen2Var
                WeakVar: [<WeakRcWhen $len Var>];// WeakRcWhen2Var
                Data: [<RcWhen $len VarData>];// RcWhen2VarData
                len: $len;//2
                C: $([<C $n>]),+;// C0, C1
                V: $([<V $n>]),+;// V0, V1
                n: $($n),+; // 0, 1
            }
        }
    )+};
    (
        Var: $RcWhenVar:ident;
        WeakVar: $WeakRcWhenVar:ident;
        Data: $RcWhenVarData:ident;
        len: $len:tt;
        C: $($C:ident),+;
        V: $($V:ident),+;
        n: $($n:tt),+;
    ) => {
        #[doc(hidden)]
        pub struct $RcWhenVar<O: VarValue, D: Var<O>, $($C: Var<bool>),+ , $($V: Var<O>),+>(Rc<$RcWhenVarData<O, D, $($C),+ , $($V),+>>);

        #[doc(hidden)]
        pub struct $WeakRcWhenVar<O: VarValue, D: Var<O>, $($C: Var<bool>),+ , $($V: Var<O>),+>(Weak<$RcWhenVarData<O, D, $($C),+ , $($V),+>>);

        struct $RcWhenVarData<O: VarValue, D: Var<O>, $($C: Var<bool>),+ , $($V: Var<O>),+> {
            _o: PhantomData<O>,

            default_value: D,
            default_version: VarVersionCell,

            conditions: ( $($C,)+ ),
            condition_versions: [VarVersionCell; $len],

            values: ( $($V,)+ ),
            value_versions: [VarVersionCell; $len],

            self_version: Cell<u32>,
        }

        #[allow(missing_docs)]// this is hidden
        impl<O: VarValue, D: Var<O>, $($C: Var<bool>),+ , $($V: Var<O>),+> $RcWhenVar<O, D, $($C),+ , $($V),+> {
            pub fn new(default_value: D, conditions: ($($C,)+), values: ($($V,)+)) -> Self {
                Self(
                    Rc::new($RcWhenVarData {
                        _o: PhantomData,

                        default_value,
                        default_version: VarVersionCell::new(0),

                        conditions,
                        condition_versions: array_init::array_init(|_|VarVersionCell::new(0)),

                        values: ($(values.$n,)+),
                        value_versions: array_init::array_init(|_|VarVersionCell::new(0)),

                        self_version: Cell::new(0),
                    })
                )
            }
        }

        impl<O: VarValue, D: Var<O>, $($C: Var<bool>),+ , $($V: Var<O>),+> crate::private::Sealed for $RcWhenVar<O, D, $($C),+ , $($V),+> {}

        impl<O: VarValue, D: Var<O>, $($C: Var<bool>),+ , $($V: Var<O>),+> crate::private::Sealed for $WeakRcWhenVar<O, D, $($C),+ , $($V),+> {}

        impl<O: VarValue, D: Var<O>, $($C: Var<bool>),+ , $($V: Var<O>),+> Clone for $RcWhenVar<O, D, $($C),+ , $($V),+> {
            fn clone(&self) -> Self {
                Self(Rc::clone(&self.0))
            }
        }

        impl<O: VarValue, D: Var<O>, $($C: Var<bool>),+ , $($V: Var<O>),+> Clone for $WeakRcWhenVar<O, D, $($C),+ , $($V),+> {
            fn clone(&self) -> Self {
                Self(self.0.clone())
            }
        }
        impl<O: VarValue, D: Var<O>, $($C: Var<bool>),+ , $($V: Var<O>),+> any::AnyWeakVar for $WeakRcWhenVar<O, D, $($C),+ , $($V),+> {
            any_var_impls!(WeakVar);
        }

        impl<O: VarValue, D: Var<O>, $($C: Var<bool>),+ , $($V: Var<O>),+> Var<O> for $RcWhenVar<O, D, $($C),+ , $($V),+> {
            type AsReadOnly = types::ReadOnlyVar<O, Self>;
            type Weak = $WeakRcWhenVar<O, D, $($C),+ , $($V),+>;

            fn get<'a, Vr: AsRef<VarsRead>>(&'a self, vars: &'a Vr) -> &'a O {
                let vars = vars.as_ref();

                $(
                    if *self.0.conditions.$n.get(vars) {
                        self.0.values.$n.get(vars)
                    }
                )else+
                else {
                    self.0.default_value.get(vars)
                }
            }
            fn get_new<'a, Vw: AsRef<Vars>>(&'a self, vars: &'a Vw) -> Option<&'a O> {
                let vars = vars.as_ref();

                let mut condition_is_new = false;
                $(
                    condition_is_new |= self.0.conditions.$n.is_new(vars);
                    if *self.0.conditions.$n.get(vars) {
                        return if condition_is_new {
                            Some(self.0.values.$n.get(vars))
                        } else {
                            self.0.values.$n.get_new(vars)
                        };
                    }
                )+

                if condition_is_new {
                    Some(self.0.default_value.get(vars))
                } else {
                    self.0.default_value.get_new(vars)
                }
            }
            fn into_value<Vr: WithVarsRead>(self, vars: &Vr) -> O {
                match Rc::try_unwrap(self.0) {
                    Ok(r) => vars.with_vars_read(|vars| {
                        $(
                            if *r.conditions.$n.get(vars) {
                                r.values.$n.into_value(vars)
                            }
                        )else+
                        else {
                            r.default_value.into_value(vars)
                        }
                    }),
                    Err(e) => $RcWhenVar(e).get_clone(vars)
                }
            }
            fn is_new<Vw: WithVars>(&self, vars: &Vw) -> bool {
                vars.with_vars(|vars| {
                    let mut condition_is_new = false;

                    $(
                        condition_is_new |= self.0.conditions.$n.is_new(vars);
                        if *self.0.conditions.$n.get(vars) {
                            return condition_is_new || self.0.values.$n.is_new(vars);
                        }
                    )+
                    condition_is_new || self.0.default_value.is_new(vars)
                })
            }
            fn version<Vr: WithVarsRead>(&self, vars: &Vr) -> VarVersion {
                vars.with_vars_read(|vars| {
                    let mut changed = false;

                    $(
                        let version = self.0.conditions.$n.version(vars);
                        if version != self.0.condition_versions[$n].get() {
                            changed = true;
                            self.0.condition_versions[$n].set(version);
                        }
                    )+

                    $(
                        let version = self.0.values.$n.version(vars);
                        if version != self.0.value_versions[$n].get() {
                            changed = true;
                            self.0.value_versions[$n].set(version);
                        }
                    )+

                    let version = self.0.default_value.version(vars);
                    if version != self.0.default_version.get() {
                        changed = true;
                        self.0.default_version.set(version);
                    }

                    if changed {
                        self.0.self_version.set(self.0.self_version.get().wrapping_add(1));
                    }

                    VarVersion::normal(self.0.self_version.get())
                })
            }
            fn is_read_only<Vw: WithVars>(&self, vars: &Vw) -> bool {
                vars.with_vars(|vars| {
                    $(
                        if *self.0.conditions.$n.get(vars) {
                            self.0.values.$n.is_read_only(vars)
                        }
                    )else+
                    else {
                        self.0.default_value.is_read_only(vars)
                    }
                })
            }
            fn is_animating<Vr: WithVarsRead>(&self, vars: &Vr) -> bool {
                vars.with_vars_read(|vars| {
                    $(
                        if *self.0.conditions.$n.get(vars) {
                            self.0.values.$n.is_animating(vars)
                        }
                    )else+
                    else {
                        self.0.default_value.is_animating(vars)
                    }
                })
            }

            fn always_read_only(&self) -> bool {
                $(self.0.values.$n.always_read_only())&&+ && self.0.default_value.always_read_only()
            }
            fn is_contextual(&self) -> bool {
                self.0.default_value.is_contextual() || $(self.0.values.$n.is_contextual())||+
            }
            fn actual_var<Vw: WithVars>(&self, vars: &Vw) -> BoxedVar<O> {
                if self.is_contextual() {
                    vars.with_vars(|vars| {
                        let var = $RcWhenVar(Rc::new($RcWhenVarData {
                            _o: PhantomData,

                            default_value: self.0.default_value.actual_var(vars),
                            default_version: self.0.default_version.clone(),

                            conditions: ($(self.0.conditions.$n.actual_var(vars),)+),
                            condition_versions: self.0.condition_versions.clone(),

                            values: ($(self.0.values.$n.actual_var(vars),)+),
                            value_versions:self.0.value_versions.clone(),

                            self_version: Cell::new(0),
                        }));
                        var.boxed()
                    })
                } else {
                    self.clone().boxed()
                }
            }

            fn can_update(&self) -> bool {
                true
            }
            fn set<Vw, N>(&self, vars: &Vw, new_value: N) -> Result<(), VarIsReadOnly>
            where
                Vw: WithVars,
                N: Into<O>
            {
                vars.with_vars(|vars| {
                    $(
                        if *self.0.conditions.$n.get(vars) {
                            self.0.values.$n.set(vars, new_value)
                        }
                    )else+
                    else {
                        self.0.default_value.set(vars, new_value)
                    }
                })
            }
            fn set_ne<Vw, N>(&self, vars: &Vw, new_value: N) -> Result<bool, VarIsReadOnly>
            where
                Vw: WithVars,
                N: Into<O>,
                O: PartialEq
            {
                vars.with_vars(|vars| {
                    $(
                        if *self.0.conditions.$n.get(vars) {
                            self.0.values.$n.set_ne(vars, new_value)
                        }
                    )else+
                    else {
                        self.0.default_value.set_ne(vars, new_value)
                    }
                })
            }

            fn modify<Vw: WithVars, F: FnOnce(VarModify<O>) + 'static>(&self, vars: &Vw, change: F) -> Result<(), VarIsReadOnly> {
                vars.with_vars(|vars| {
                    $(
                        if *self.0.conditions.$n.get(vars) {
                            self.0.values.$n.modify(vars, change)
                        }
                    )else+
                    else {
                        self.0.default_value.modify(vars, change)
                    }
                })
            }


            fn into_read_only(self) -> Self::AsReadOnly {
               types::ReadOnlyVar::new(self)
            }

            fn update_mask<Vr: WithVarsRead>(&self, vars: &Vr) -> UpdateMask {
                vars.with_vars_read(|vars| {
                    let mut r = self.0.default_value.update_mask(vars);
                    $(r |= self.0.conditions.$n.update_mask(vars);)+
                    $(r |= self.0.values.$n.update_mask(vars);)+
                    r
                })
            }


            fn downgrade(&self) -> Option<Self::Weak> {
                Some($WeakRcWhenVar(Rc::downgrade(&self.0)))
            }


            fn is_rc(&self) -> bool {
                true
            }


            fn strong_count(&self) -> usize {
                Rc::strong_count(&self.0)
            }


            fn weak_count(&self) -> usize {
                Rc::weak_count(&self.0)
            }


            fn as_ptr(&self) -> *const () {
                Rc::as_ptr(&self.0) as _
            }
        }

        impl<O: VarValue, D: Var<O>, $($C: Var<bool>),+ , $($V: Var<O>),+> WeakVar<O> for $WeakRcWhenVar<O, D, $($C),+ , $($V),+> {
            type Strong = $RcWhenVar<O, D, $($C),+ , $($V),+>;


            fn upgrade(&self) -> Option<Self::Strong> {
                self.0.upgrade().map($RcWhenVar)
            }


            fn strong_count(&self) -> usize {
                self.0.strong_count()
            }


            fn weak_count(&self) -> usize {
                self.0.weak_count()
            }


            fn as_ptr(&self) -> *const () {
                self.0.as_ptr() as _
            }
        }

        impl<O: VarValue, D: Var<O>, $($C: Var<bool>),+ , $($V: Var<O>),+> IntoVar<O> for $RcWhenVar<O, D, $($C),+ , $($V),+>  {
            type Var = Self;


            fn into_var(self) -> Self::Var {
                self
            }
        }

        impl<O: VarValue, D: Var<O>, $($C: Var<bool>),+ , $($V: Var<O>),+> any::AnyVar for $RcWhenVar<O, D, $($C),+ , $($V),+> {
            any_var_impls!(Var);
        }
    };
}
#[cfg(not(dyn_closure))]
impl_rc_when_var! {
    1 => 0;
    2 => 0, 1;
    3 => 0, 1, 2;
    4 => 0, 1, 2, 3;
    5 => 0, 1, 2, 3, 4;
    6 => 0, 1, 2, 3, 4, 5;
    7 => 0, 1, 2, 3, 4, 5, 6;
    8 => 0, 1, 2, 3, 4, 5, 6, 7;
}

/// A [`when_var!`] that uses dynamic dispatch to support any number of variables.
///
/// This type is a reference-counted pointer ([`Rc`]),
/// it implements the full [`Var`] read and write methods.
///
/// Don't use this type directly use the [macro](when_var!) instead.
pub struct RcWhenVar<O: VarValue>(Rc<RcWhenVarData<O>>);

/// A weak reference to a [`RcWhenVar`].
pub struct WeakRcWhenVar<O: VarValue>(Weak<RcWhenVarData<O>>);

struct WhenCondition<O: VarValue> {
    condition: BoxedVar<bool>,
    value: BoxedVar<O>,
    condition_version: VarVersionCell,
    value_version: VarVersionCell,
}

struct RcWhenVarData<O: VarValue> {
    default_: BoxedVar<O>,
    default_version: VarVersionCell,

    whens: Box<[WhenCondition<O>]>,

    self_version: Cell<u32>,
}

impl<O: VarValue> RcWhenVar<O> {
    fn new(default_: BoxedVar<O>, whens: Box<[WhenCondition<O>]>) -> Self {
        RcWhenVar(Rc::new(RcWhenVarData {
            default_,
            default_version: VarVersionCell::new(0),
            whens,
            self_version: Cell::new(0),
        }))
    }

    fn get_impl<'a>(&'a self, vars: &'a VarsRead) -> &'a O {
        for c in self.0.whens.iter() {
            if c.condition.copy(vars) {
                return c.value.get(vars);
            }
        }
        self.0.default_.get(vars)
    }

    fn get_new_impl<'a>(&'a self, vars: &'a Vars) -> Option<&'a O> {
        let mut condition_is_new = false;
        for c in self.0.whens.iter() {
            condition_is_new |= c.condition.is_new(vars);
            if c.condition.copy(vars) {
                return if condition_is_new {
                    // a higher priority condition is new `false` of the current condition is new `true`.
                    Some(c.value.get(vars))
                } else {
                    c.value.get_new(vars)
                };
            }
        }

        if condition_is_new {
            Some(self.0.default_.get(vars))
        } else {
            self.0.default_.get_new(vars)
        }
    }

    #[doc(hidden)]
    pub fn as_impl_var(self) -> impl Var<O> {
        self
    }
}

impl<O: VarValue> crate::private::Sealed for RcWhenVar<O> {}
impl<O: VarValue> crate::private::Sealed for WeakRcWhenVar<O> {}

impl<O: VarValue> Clone for RcWhenVar<O> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}
impl<O: VarValue> Clone for WeakRcWhenVar<O> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<O: VarValue> Var<O> for RcWhenVar<O> {
    type AsReadOnly = types::ReadOnlyVar<O, Self>;
    type Weak = WeakRcWhenVar<O>;

    /// Gets the the first variable with `true` condition or the default variable.
    fn get<'a, Vr: AsRef<VarsRead>>(&'a self, vars: &'a Vr) -> &'a O {
        self.get_impl(vars.as_ref())
    }

    /// Gets the first variable with `true` condition if that condition or previous conditions are new.
    ///
    /// Gets the first variable with `true` condition if that variable value is new.
    ///
    /// Gets the default variable if any of the conditions are new and all are `false`.
    ///
    /// Gets the default variable if all conditions are `false` and the default variable value is new.
    fn get_new<'a, Vw: AsRef<Vars>>(&'a self, vars: &'a Vw) -> Option<&'a O> {
        self.get_new_impl(vars.as_ref())
    }

    /// Gets if [`get_new`](Self::get_new) will return `Some(_)` if called.
    ///
    /// This is slightly more performant than `when_var.get_new(vars).is_some()`.
    fn is_new<Vw: WithVars>(&self, vars: &Vw) -> bool {
        vars.with_vars(|vars| {
            let mut condition_is_new = false;
            for c in self.0.whens.iter() {
                condition_is_new |= c.condition.is_new(vars);
                if c.condition.copy(vars) {
                    return condition_is_new || c.value.is_new(vars);
                }
            }
            condition_is_new || self.0.default_.is_new(vars)
        })
    }

    /// If `self` is the only reference calls `into_value` on the first variable with condition `true`.
    ///
    /// If `self` is not the only reference returns a clone of the value.
    fn into_value<Vr: WithVarsRead>(self, vars: &Vr) -> O {
        match Rc::try_unwrap(self.0) {
            Ok(r) => vars.with_vars_read(move |vars| {
                for c in Vec::from(r.whens) {
                    if c.condition.copy(vars) {
                        return c.value.into_value(vars);
                    }
                }

                r.default_.into_value(vars)
            }),
            Err(e) => RcWhenVar(e).get_clone(vars),
        }
    }

    /// Gets the version.
    ///
    /// The version is new when any of the condition and value variables version is new.
    fn version<Vr: WithVarsRead>(&self, vars: &Vr) -> VarVersion {
        vars.with_vars_read(|vars| {
            let mut changed = false;

            let dv = self.0.default_.version(vars);
            if dv != self.0.default_version.get() {
                changed = true;
                self.0.default_version.set(dv);
            }

            for c in self.0.whens.iter() {
                let cv = c.condition.version(vars);
                if cv != c.condition_version.get() {
                    changed = true;
                    c.condition_version.set(cv);
                }
                let vv = c.value.version(vars);
                if vv != c.value_version.get() {
                    changed = true;
                    c.value_version.set(vv);
                }
            }

            if changed {
                self.0.self_version.set(self.0.self_version.get().wrapping_add(1));
            }

            VarVersion::normal(self.0.self_version.get())
        })
    }

    /// If the [current value variable](Self::get) is read-only.
    fn is_read_only<Vw: WithVars>(&self, vars: &Vw) -> bool {
        vars.with_vars(|vars| {
            for c in self.0.whens.iter() {
                if c.condition.copy(vars) {
                    return c.value.is_read_only(vars);
                }
            }
            self.0.default_.is_read_only(vars)
        })
    }

    fn is_animating<Vr: WithVarsRead>(&self, vars: &Vr) -> bool {
        vars.with_vars_read(|vars| {
            for c in self.0.whens.iter() {
                if c.condition.copy(vars) {
                    return c.value.is_animating(vars);
                }
            }
            self.0.default_.is_animating(vars)
        })
    }

    /// If all value variables (including default) are always read-only.
    fn always_read_only(&self) -> bool {
        self.0.whens.iter().all(|c| c.value.always_read_only()) && self.0.default_.always_read_only()
    }

    /// If any value variables (including default)
    fn is_contextual(&self) -> bool {
        self.0.default_.is_contextual() || self.0.whens.iter().any(|c| c.value.is_contextual())
    }

    fn actual_var<Vw: WithVars>(&self, vars: &Vw) -> BoxedVar<O> {
        if self.is_contextual() {
            vars.with_vars(|vars| {
                let var = RcWhenVar(Rc::new(RcWhenVarData {
                    default_: self.0.default_.actual_var(vars),
                    default_version: self.0.default_version.clone(),
                    whens: self
                        .0
                        .whens
                        .iter()
                        .map(|c| WhenCondition {
                            condition: c.condition.actual_var(vars),
                            value: c.value.actual_var(vars),
                            condition_version: c.condition_version.clone(),
                            value_version: c.value_version.clone(),
                        })
                        .collect(),
                    self_version: self.0.self_version.clone(),
                }));
                var.boxed()
            })
        } else {
            self.clone().boxed()
        }
    }

    /// Always `true`.
    fn can_update(&self) -> bool {
        true
    }

    /// Sets the [current value variable](Self::get).
    fn set<Vw, N>(&self, vars: &Vw, new_value: N) -> Result<(), VarIsReadOnly>
    where
        Vw: WithVars,
        N: Into<O>,
    {
        vars.with_vars(|vars| {
            for c in self.0.whens.iter() {
                if c.condition.copy(vars) {
                    return c.value.set(vars, new_value);
                }
            }
            self.0.default_.set(vars, new_value)
        })
    }

    fn set_ne<Vw, N>(&self, vars: &Vw, new_value: N) -> Result<bool, VarIsReadOnly>
    where
        Vw: WithVars,
        N: Into<O>,
        O: PartialEq,
    {
        vars.with_vars(|vars| {
            for c in self.0.whens.iter() {
                if c.condition.copy(vars) {
                    return c.value.set_ne(vars, new_value);
                }
            }
            self.0.default_.set_ne(vars, new_value)
        })
    }

    /// Modify the [current value variable](Self::get).
    fn modify<Vw: WithVars, F: FnOnce(VarModify<O>) + 'static>(&self, vars: &Vw, change: F) -> Result<(), VarIsReadOnly> {
        vars.with_vars(|vars| {
            for c in self.0.whens.iter() {
                if c.condition.copy(vars) {
                    return c.value.modify(vars, change);
                }
            }
            self.0.default_.modify(vars, change)
        })
    }

    fn into_read_only(self) -> Self::AsReadOnly {
        types::ReadOnlyVar::new(self)
    }

    fn update_mask<Vr: WithVarsRead>(&self, vars: &Vr) -> UpdateMask {
        vars.with_vars_read(|vars| {
            let mut r = self.0.default_.update_mask(vars);
            for c in self.0.whens.iter() {
                r |= c.condition.update_mask(vars);
                r |= c.value.update_mask(vars);
            }
            r
        })
    }

    fn downgrade(&self) -> Option<Self::Weak> {
        Some(WeakRcWhenVar(Rc::downgrade(&self.0)))
    }

    fn is_rc(&self) -> bool {
        true
    }

    fn strong_count(&self) -> usize {
        Rc::strong_count(&self.0)
    }

    fn weak_count(&self) -> usize {
        Rc::weak_count(&self.0)
    }

    fn as_ptr(&self) -> *const () {
        Rc::as_ptr(&self.0) as _
    }
}
impl<O: VarValue> any::AnyWeakVar for WeakRcWhenVar<O> {
    any_var_impls!(WeakVar);
}
impl<O: VarValue> WeakVar<O> for WeakRcWhenVar<O> {
    type Strong = RcWhenVar<O>;

    fn upgrade(&self) -> Option<Self::Strong> {
        self.0.upgrade().map(RcWhenVar)
    }

    fn strong_count(&self) -> usize {
        self.0.strong_count()
    }

    fn weak_count(&self) -> usize {
        self.0.weak_count()
    }

    fn as_ptr(&self) -> *const () {
        self.0.as_ptr() as _
    }
}
impl<O: VarValue> IntoVar<O> for RcWhenVar<O> {
    type Var = Self;

    fn into_var(self) -> Self::Var {
        self
    }
}
impl<O: VarValue> any::AnyVar for RcWhenVar<O> {
    any_var_impls!(Var);
}

/// Builder used in [`when_var!`] when there is more then 8 conditions. Boxes the variables.
#[doc(hidden)]
pub struct WhenVarBuilderDyn<O: VarValue> {
    default_: BoxedVar<O>,
    whens: Vec<WhenCondition<O>>,
}
#[allow(missing_docs)] // this is hidden
impl<O: VarValue> WhenVarBuilderDyn<O> {
    pub fn new<D: IntoVar<O>>(default_: D) -> Self {
        Self {
            default_: default_.into_var().boxed(),
            whens: vec![],
        }
    }

    pub fn push<C: Var<bool>, V: IntoVar<O>>(mut self, condition: C, value: V) -> Self {
        self.whens.push(WhenCondition {
            condition: condition.boxed(),
            value: value.into_var().boxed(),
            condition_version: VarVersionCell::new(0),
            value_version: VarVersionCell::new(0),
        });
        self
    }

    pub fn build(self) -> RcWhenVar<O> {
        RcWhenVar::new(self.default_, self.whens.into_boxed_slice())
    }
}

#[cfg(dyn_closure)]
pub use WhenVarBuilderDyn as WhenVarBuilder;

/// Builder used in [`when_var!`], designed to support #[cfg(..)] attributes in conditions.
#[doc(hidden)]
#[cfg(not(dyn_closure))]
pub struct WhenVarBuilder<O: VarValue, D: Var<O>> {
    _v: PhantomData<O>,
    default_value: D,
}
#[allow(missing_docs)] // this is hidden
#[cfg(not(dyn_closure))]
impl<O: VarValue, D: Var<O>> WhenVarBuilder<O, D> {
    /// Start the builder with the last item, it is the only *condition* that cannot be excluded by #[cfg(..)].
    pub fn new<ID: IntoVar<O, Var = D>>(default_value: ID) -> Self {
        Self {
            _v: PhantomData,
            default_value: default_value.into_var(),
        }
    }

    pub fn push<C0: Var<bool>, IV0: IntoVar<O>>(self, condition: C0, value: IV0) -> WhenVarBuilder1<O, D, C0, IV0::Var> {
        WhenVarBuilder1 {
            _v: self._v,
            default_value: self.default_value,
            condition: (condition,),
            value: (value.into_var(),),
        }
    }

    /// Only default condition included, if [`when_var!`] is implemented correctly other conditions where typed
    /// but are excluded by #[cfg(..)].
    pub fn build(self) -> D {
        self.default_value
    }
}
#[cfg(not(dyn_closure))]
macro_rules! impl_when_var_builder {
    ($(
        $len:tt => $($n:tt),+ => $next_len:tt ;
    )+) => {$(
        $crate::paste!{
            impl_when_var_builder!{
                Builder: [<WhenVarBuilder $len>];// WhenVarBuilder2
                Var: [<RcWhen $len Var>];// RcWhen2Var
                C: $([<C $n>]),+;// C0, C1
                V: $([<V $n>]),+;// V0, V1
                n: $($n),+; // 0, 1
                BuilderNext: [<WhenVarBuilder $next_len>];//WhenVarBuilder3
            }
        }
    )+};
    (
        Builder: $Builder:ident;
        Var: $Var:ident;
        C: $($C:ident),+;
        V: $($V:ident),+;
        n: $($n:tt),+;
        BuilderNext: $BuilderNext:ident;
    ) => {
        #[doc(hidden)]
        pub struct $Builder<
            O: VarValue,
            D: Var<O>,
            $($C: Var<bool>,)*
            $($V: Var<O>,)*
        > {
            _v: PhantomData<O>,
            default_value: D,
            condition: ($($C,)*),
            value: ($($V,)*),
        }
        #[allow(missing_docs)] // this is hidden
        impl<
            O: VarValue,
            D: Var<O>,
            $($C: Var<bool>,)*
            $($V: Var<O>,)*
        > $Builder<O, D, $($C,)* $($V),*> {
            pub fn push<C: Var<bool>, IV: IntoVar<O>>(self, condition: C, value: IV) -> $BuilderNext<O, D, $($C,)* C, $($V,)* IV::Var> {
                $BuilderNext {
                    _v: self._v,
                    default_value: self.default_value,
                    condition: ( $(self.condition.$n,)* condition),
                    value: ( $(self.value.$n,)* value.into_var()),
                }
            }

            pub fn build(self) -> $Var<O, D, $($C,)* $($V,)*> {
                $Var::new(self.default_value, self.condition, self.value)
            }
        }
    }
}
#[cfg(not(dyn_closure))]
impl_when_var_builder! {
    1 => 0 => 2;
    2 => 0, 1 => 3;
    3 => 0, 1, 2 => 4;
    4 => 0, 1, 2, 3 => 5;
    5 => 0, 1, 2, 3, 4 => 6;
    6 => 0, 1, 2, 3, 4, 5 => 7;
    7 => 0, 1, 2, 3, 4, 5, 6 => 8;
    8 => 0, 1, 2, 3, 4, 5, 6, 7 => 9;
}
/// Generic builder stops at WhenVarBuilder8, this only type
/// exists because of the nature of the [`impl_when_var_builder`] code.
#[doc(hidden)]
#[allow(unused)]
#[cfg(not(dyn_closure))]
pub struct WhenVarBuilder9<O, D, C0, C1, C2, C3, C4, C5, C6, C7, C8, V0, V1, V2, V3, V4, V5, V6, V7, V8> {
    _v: PhantomData<O>,
    default_value: D,
    condition: (C0, C1, C2, C3, C4, C5, C6, C7, C8),
    value: (V0, V1, V2, V3, V4, V5, V6, V7, V8),
}

/// Type erased [`when_var!`] builder.
///
/// All value variables must be of type [`BoxedVar<T>`] of the same `T`, each instance of this type represents
/// a single argument for the property and most match the `T` of the `impl IntoVar<T>` signature of the property.
///
/// This type is primarily used by the dynamic widget macros in [`DynWidget`], you probably can use the [`when_var!`] macro
/// directly, or if you need a builder to instantiate a dynamic property you can create one from [`rc_when_var!`].
///
/// [`DynWidget`]: crate::DynWidget
pub struct AnyWhenVarBuilder {
    default: Option<Box<dyn AnyVar>>,
    whens: Vec<(BoxedVar<bool>, Box<dyn AnyVar>)>,
}
impl AnyWhenVarBuilder {
    /// Start building with only the default value.
    pub fn new<O: VarValue>(default: impl IntoVar<O>) -> Self {
        Self::new_any(default.into_var().boxed().into_any())
    }

    /// Start building with an already type erased default var.
    pub fn new_any(default: Box<dyn AnyVar>) -> Self {
        Self {
            default: Some(default),
            whens: vec![],
        }
    }

    /// Start building without a default value, note that a when var can only build if a default value is set.
    pub fn new_no_default() -> Self {
        Self {
            default: None,
            whens: vec![],
        }
    }

    /// Create a builder from the parts of a formed [`rc_when_var!`].
    pub fn from_var<O: VarValue>(var: &RcWhenVar<O>) -> Self {
        Self {
            default: Some(var.0.default_.clone().into_any()),
            whens: var
                .0
                .whens
                .iter()
                .map(|w| (w.condition.clone(), w.value.clone().into_any()))
                .collect(),
        }
    }

    /// Returns `true` if a default value is set.
    pub fn has_default(&self) -> bool {
        self.default.is_some()
    }

    /// Returns the number of conditions set.
    pub fn condition_count(&self) -> usize {
        self.whens.len()
    }

    /// Set/replace the default value.
    pub fn set_default<O: VarValue>(&mut self, default: impl IntoVar<O>) {
        self.set_default_any(default.into_var().boxed().into_any());
    }

    /// Set/replace the default value with an already typed erased var.
    pub fn set_default_any(&mut self, default: Box<dyn AnyVar>) {
        self.default = Some(default);
    }

    /// Push a when condition.
    pub fn push<C: Var<bool>, O: VarValue, V: IntoVar<O>>(self, condition: C, value: V) -> Self {
        self.push_any(condition.boxed(), value.into_var().boxed().into_any())
    }

    /// Push a when condition already boxed and type erased.
    pub fn push_any(mut self, condition: BoxedVar<bool>, value: Box<dyn AnyVar>) -> Self {
        self.whens.push((condition, value));
        self
    }

    /// Replace the default value if `other` has default and extend the conditions with clones of `other`.
    pub fn replace_extend(&mut self, other: &Self) {
        if let Some(default) = &other.default {
            self.default = Some(default.clone_any());
        }
        self.extend(other);
    }

    /// Extend the conditions with clones of `other`.
    pub fn extend(&mut self, other: &Self) {
        for (c, v) in other.whens.iter() {
            self.whens.push((c.clone(), v.clone_any()));
        }
    }

    /// Build the when var if all value variables are of type [`BoxedVar<T>`] and a default value is set.
    pub fn build<T: VarValue>(&self) -> Option<RcWhenVar<T>> {
        let default = self.default.as_ref()?.as_any().downcast_ref::<BoxedVar<T>>()?;

        let mut when = WhenVarBuilderDyn::new(default.clone());

        for (c, v) in &self.whens {
            let value = v.as_any().downcast_ref::<BoxedVar<T>>()?;

            when = when.push(c.clone(), value.clone());
        }

        Some(when.build())
    }
}
impl fmt::Debug for AnyWhenVarBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnyWhenVarBuilder")
            .field("has_default", &self.has_default())
            .field("condition_count", &self.condition_count())
            .finish_non_exhaustive()
    }
}
impl Clone for AnyWhenVarBuilder {
    fn clone(&self) -> Self {
        Self {
            default: self.default.as_ref().map(|d| d.clone_any()),
            whens: self.whens.iter().map(|(c, v)| (c.clone(), v.clone_any())).collect(),
        }
    }
}
