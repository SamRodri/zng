use rc::WeakRcVar;

use super::*;

/// Represents a single value as [`Var<T>`].
#[derive(Clone)]
pub struct LocalVar<T: VarValue>(pub T);

impl<T: VarValue> crate::private::Sealed for LocalVar<T> {}

impl<T: VarValue> AnyVar for LocalVar<T> {
    fn clone_any(&self) -> BoxedAnyVar {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn double_boxed_any(self: Box<Self>) -> Box<dyn Any> {
        let me: BoxedVar<T> = self;
        Box::new(me)
    }

    fn var_type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }

    fn get_any(&self) -> Box<dyn AnyVarValue> {
        Box::new(self.0.clone())
    }

    fn set_any(&self, _: &Vars, _: Box<dyn AnyVarValue>) -> Result<(), VarIsReadOnlyError> {
        Err(VarIsReadOnlyError {
            capabilities: self.capabilities(),
        })
    }

    fn last_update(&self) -> VarUpdateId {
        VarUpdateId::never()
    }

    fn capabilities(&self) -> VarCapabilities {
        VarCapabilities::empty()
    }

    fn hook(&self, _: Box<dyn Fn(&Vars, &mut Updates, &dyn AnyVarValue) -> bool>) -> VarHandle {
        VarHandle::dummy()
    }

    fn subscribe(&self, _: WidgetId) -> VarHandle {
        VarHandle::dummy()
    }

    fn strong_count(&self) -> usize {
        0
    }

    fn weak_count(&self) -> usize {
        0
    }

    fn actual_var_any(&self) -> BoxedAnyVar {
        self.clone_any()
    }

    fn downgrade_any(&self) -> BoxedAnyWeakVar {
        Box::new(WeakRcVar::<T>::new())
    }

    fn is_animating(&self) -> bool {
        false
    }
}

impl<T: VarValue> IntoVar<T> for LocalVar<T> {
    type Var = Self;

    fn into_var(self) -> Self::Var {
        self
    }
}
impl<T: VarValue> IntoVar<T> for T {
    type Var = LocalVar<T>;

    fn into_var(self) -> Self::Var {
        LocalVar(self)
    }
}

impl<T: VarValue> Var<T> for LocalVar<T> {
    type ReadOnly = Self;

    type ActualVar = Self;

    type Downgrade = WeakRcVar<T>;

    fn with<R, F>(&self, read: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        read(&self.0)
    }

    fn modify<V, F>(&self, _: &V, _: F) -> Result<(), VarIsReadOnlyError>
    where
        V: WithVars,
        F: FnOnce(&mut VarModifyValue<T>) + 'static,
    {
        Err(VarIsReadOnlyError {
            capabilities: self.capabilities(),
        })
    }

    fn actual_var(&self) -> Self::ActualVar {
        self.clone()
    }

    fn downgrade(&self) -> Self::Downgrade {
        WeakRcVar::new()
    }

    fn into_value(self) -> T {
        self.0
    }

    fn read_only(&self) -> Self::ReadOnly {
        self.clone()
    }
}
