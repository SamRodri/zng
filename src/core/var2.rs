use fnv::FnvHashMap;
use std::{
    any::TypeId,
    cell::RefCell,
    cell::{Cell, UnsafeCell},
    fmt::Debug,
    marker::PhantomData,
    mem::MaybeUninit,
    rc::Rc,
};

mod boxed_var;
pub use boxed_var::*;

mod owned_var;
pub use owned_var::*;

mod rc_var;
pub use rc_var::*;

mod force_read_only_var;
pub use force_read_only_var::*;

mod cloning_local_var;
pub use cloning_local_var::*;

mod rc_map_var;
pub use rc_map_var::*;

mod rc_map_bidi_var;
pub use rc_map_bidi_var::*;

mod context_var_proxy;
pub use context_var_proxy::*;

/// A type that can be a [`Var`](crate::core::var::Var) value.
///
/// # Trait Alias
///
/// This trait is used like a type alias for traits and is
/// already implemented for all types it applies to.
pub trait VarValue: Debug + Clone + 'static {}
impl<T: Debug + Clone + 'static> VarValue for T {}

/// Type Id if a contextual variable.
pub trait ContextVar: Clone + Copy + 'static {
    /// The variable type.
    type Type: VarValue;

    /// Default value, used when the variable is not set in the context.
    fn default_value() -> &'static Self::Type;

    /// Gets the variable.
    fn var() -> ContextVarProxy<Self> {
        ContextVarProxy::default()
    }
}

/// Error when trying to set or modify a read-only variable.
#[derive(Debug, Hash, PartialEq, Eq)]
pub struct VarIsReadOnly;
impl std::fmt::Display for VarIsReadOnly {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "cannot set or modify read-only variable")
    }
}

mod protected {
    /// Ensures that only `zero-ui` can implement var types.
    pub trait Var {}
}

/// Part of [`Var`] that can be boxed.
pub trait VarObj<T: VarValue>: protected::Var + 'static {
    /// References the current value.
    fn get<'a>(&'a self, vars: &'a Vars) -> &'a T;

    /// References the current value if it is new.
    fn get_new<'a>(&'a self, vars: &'a Vars) -> Option<&'a T>;

    /// If [`set`](Self::set) or [`modify`](Var::modify) was called in the previous update.
    ///
    /// When you set the variable, the new value is only applied after the UI tree finishes
    /// the current update. The value is then applied causing a new update to happen, in the new
    /// update this method returns `true`. After the new update it returns `false` again.
    fn is_new(&self, vars: &Vars) -> bool;

    /// Version of the current value.
    ///
    /// The version number changes every update where [`set`](Self::set) or [`modify`](Var::modify) are called.
    fn version(&self, vars: &Vars) -> u32;

    /// If the variable cannot be set.
    ///
    /// Variables can still change if [`can_update`](Self::can_update) is `true`.
    ///
    /// Some variables can stop being read-only after an update, see also [`always_read_only`](Self::always_read_only).
    fn is_read_only(&self, vars: &Vars) -> bool;

    /// If the variable type is read-only, unlike [`is_read_only`](Self::is_read_only) this never changes.
    fn always_read_only(&self) -> bool;

    /// If the variable type allows the value to change.
    ///
    /// Some variables can change even if they are read-only, for example mapping variables.
    fn can_update(&self) -> bool;

    /// Schedules an assign for after the current update.
    ///
    /// Variables are not changed immediately, the full UI tree gets a chance to see the current value,
    /// after the current UI update, the values set here are applied.
    ///
    /// ### Error
    ///
    /// Returns [`VarIsReadOnly`] if [`is_read_only`](Self::is_read_only) is `true`.
    fn set(&self, vars: &Vars, new_value: T) -> Result<(), VarIsReadOnly>;

    /// Boxed version of the [`modify`](Var::modify) method.
    fn modify_boxed(&self, vars: &Vars, change: Box<dyn FnOnce(&mut T)>) -> Result<(), VarIsReadOnly>;

    /// Boxes `self`.
    ///
    /// A boxed var is also a var, that implementation just returns `self`.
    fn boxed(self) -> Box<dyn VarObj<T>>
    where
        Self: Sized,
    {
        Box::new(self)
    }
}

/// Represents a variable that has a value that can be accessed directly.
///
/// For the normal variables you need a reference to [`Vars`] to access the value,
/// this reference is not available in all [`UiNode`](crate::core::UiNode) methods.
///
/// Some variable types are safe to reference the inner value at any moment, other variables
/// can be wrapped in a type that makes a local clone of the current value. You can get any
/// variable as a local variable by calling [`Var::as_local`].
pub trait VarLocal<T: VarValue>: VarObj<T> {
    /// Reference the value.
    fn get_local(&self) -> &T;

    /// Initializes local clone of the value, if needed.
    ///
    /// This must be called in the [`UiNode::init`](crate::core::UiNode::init) method.
    ///
    /// Returns a reference to the local value for convenience.
    fn init_local(&mut self, vars: &Vars) -> &T;

    /// Updates the local clone of the value, if needed.
    ///
    /// This must be called in the [`UiNode::update`](crate::core::UiNode::update) method.
    ///
    /// Returns a reference to the local value if the value is new.
    fn update_local(&mut self, vars: &Vars) -> Option<&T>;

    /// Boxes `self`.
    fn boxed_local(self) -> Box<dyn VarLocal<T>>
    where
        Self: Sized,
    {
        Box::new(self)
    }
}

/// Represents a variable.
///
/// Most of the methods are declared in the [`VarObj`] trait to support boxing.
pub trait Var<T: VarValue>: VarObj<T> + Clone {
    /// Return type of [`as_read_only`](Var::as_read_only).
    type AsReadOnly: Var<T>;
    /// Return type of [`as_local`](Var::as_local).
    type AsLocal: VarLocal<T>;

    /// Schedules a closure to modify the value after the current update.
    ///
    /// This is a variation of the [`set`](VarObj::set) method that does not require
    /// an entire new value to be instantiated.
    fn modify<F: FnOnce(&mut T) + 'static>(&self, vars: &Vars, change: F) -> Result<(), VarIsReadOnly>;

    /// Returns the variable as a type that is [`always_read_only`](ObjVar::always_read_only).
    fn as_read_only(self) -> Self::AsReadOnly;

    /// Returns the variable as a type that implements [`VarLocal`].
    fn as_local(self) -> Self::AsLocal;

    /// Returns a variable whos value is mapped from `self`.
    ///
    /// The value is new when the `self` value is new, `map` is only called once per new value.
    ///
    /// The variable is read-only, use [`map_bidi`](Self::map_bidi) to propagate changes back to `self`.
    fn map<O: VarValue, F: FnMut(&T) -> O + 'static>(&self, map: F) -> RcMapVar<T, O, Self, F>;

    /// Returns a variable whos value is mapped to and from `self`.
    ///
    /// The value is new when the `self` value is new, `map` is only called once per new value.
    ///
    /// The variable can be set if `self` is not read-only, when set `map_back` is called to generate
    /// a value
    fn map_bidi<O: VarValue, F: FnMut(&T) -> O + 'static, G: FnMut(O) -> T + 'static>(
        &self,
        map: F,
        map_back: G,
    ) -> RcMapBidiVar<T, O, Self, F, G>;
}

/// A value-to-[var](Var) conversion that consumes the value.
pub trait IntoVar<T: VarValue>: Clone {
    type Var: Var<T>;

    /// Converts the source value into a var.
    fn into_var(self) -> Self::Var;

    /// Shortcut call `self.into_var().as_local()`.
    fn into_local(self) -> <<Self as IntoVar<T>>::Var as Var<T>>::AsLocal
    where
        Self: Sized,
    {
        self.into_var().as_local()
    }
}

/// Access to application variables.
///
/// Only a single instance of this type exists at a time.
pub struct Vars {
    update_id: u32,
    #[allow(clippy::type_complexity)]
    pending: RefCell<Vec<Box<dyn FnOnce(u32)>>>,
    context_vars: RefCell<FnvHashMap<TypeId, (*const AnyRef, bool, u32)>>,
}
impl Vars {
    fn update_id(&self) -> u32 {
        self.update_id
    }

    /// Gets a var at the context level.
    fn context_var<C: ContextVar>(&self) -> (&C::Type, bool, u32) {
        let vars = self.context_vars.borrow();
        if let Some((any_ref, is_new, version)) = vars.get(&TypeId::of::<C>()) {
            // SAFETY: This is safe because `TypeId` keys are always associated
            // with the same type of reference. Also we are not leaking because the
            // source reference is borrowed in a [`with_context_var`] call.
            let value = unsafe { AnyRef::unpack(*any_ref) };
            (value, *is_new, *version)
        } else {
            (C::default_value(), false, 0)
        }
    }

    /// Calls `f` with the context var value.
    pub fn with_context_var<C: ContextVar, F: FnOnce(&Vars)>(&self, value: &C::Type, is_new: bool, version: u32, f: F) {
        let prev = self
            .context_vars
            .borrow_mut()
            .insert(TypeId::of::<C>(), (AnyRef::pack(value), is_new, version));
        f(self);
        if let Some(prev) = prev {
            self.context_vars.borrow_mut().insert(TypeId::of::<C>(), prev);
        }
    }

    fn push_change(&self, change: Box<dyn FnOnce(u32)>) {
        self.pending.borrow_mut().push(change);
    }

    pub(super) fn apply(&mut self) {
        self.update_id = self.update_id.wrapping_add(1);
        for f in self.pending.get_mut().drain(..) {
            f(self.update_id);
        }
    }
}
enum AnyRef {}
impl AnyRef {
    fn pack<T>(r: &T) -> *const AnyRef {
        (r as *const T) as *const AnyRef
    }

    unsafe fn unpack<'a, T>(pointer: *const Self) -> &'a T {
        &*(pointer as *const T)
    }
}
