use std::{
    cell::{Cell, RefCell, UnsafeCell},
    marker::PhantomData,
    rc::Rc,
};

use crate::widget_info::UpdateSlot;

use super::*;

/// A [`Var`] that maps from and to another var selected from a source var and is a [`Rc`] pointer to its value.
pub struct RcFlatMapVar<A, B, V, M, S>(Rc<MapData<A, B, V, M, S>>)
where
    A: VarValue,
    B: VarValue,
    V: Var<B>,
    M: FnMut(&A) -> V + 'static,
    S: Var<A>;

struct MapData<A, B, V, M, S> {
    _a: PhantomData<(A, B)>,

    source: S,
    map: RefCell<M>,
    var: UnsafeCell<Option<V>>,

    source_version: Cell<u32>,
    var_version: Cell<u32>,

    version: Cell<u32>,
    update_slot: UpdateSlot,
}

impl<A, B, V, M, S> Clone for RcFlatMapVar<A, B, V, M, S>
where
    A: VarValue,
    B: VarValue,
    V: Var<B>,
    M: FnMut(&A) -> V + 'static,
    S: Var<A>,
{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<A, B, V, M, S> crate::private::Sealed for RcFlatMapVar<A, B, V, M, S>
where
    A: VarValue,
    B: VarValue,
    V: Var<B>,
    M: FnMut(&A) -> V + 'static,
    S: Var<A>,
{
}

impl<A, B, V, M, S> RcFlatMapVar<A, B, V, M, S>
where
    A: VarValue,
    B: VarValue,
    V: Var<B>,
    M: FnMut(&A) -> V + 'static,
    S: Var<A>,
{
    /// New mapping var.
    ///
    /// Prefer using the [`Var::flat_map`] method.
    #[inline]
    pub fn new(source: S, map: M) -> Self {
        RcFlatMapVar(Rc::new(MapData {
            _a: PhantomData,
            source,
            map: RefCell::new(map),
            var: UnsafeCell::new(None),
            source_version: Cell::new(0),
            var_version: Cell::new(0),
            version: Cell::new(0),
            update_slot: UpdateSlot::next(),
        }))
    }

    fn var(&self, vars: &VarsRead) -> &V {
        let version = self.0.source.version(vars);
        let var = unsafe { &mut *self.0.var.get() };

        let first = var.is_none();
        if self.0.source_version.get() != version || first {
            let mut map = self.0.map.borrow_mut();
            let v = map(self.0.source.get(vars));

            self.0.version.set(self.0.version.get().wrapping_add(1));
            self.0.source_version.set(version);
            self.0.var_version.set(v.version(vars));

            *var = Some(v);
        }

        if first {
            let slot = self.0.update_slot;
            let self_ = Rc::downgrade(&self.0);
            vars.link_updates(move |vars, updates| {
                let mut retain = false;
                if let Some(self_) = self_.upgrade() {
                    retain = true;

                    if Self(self_).var(vars).is_new(vars) {
                        updates.insert(slot);
                    }
                }
                retain
            });
        }

        var.as_ref().unwrap()
    }
}

impl<A, B, V, M, S> Var<B> for RcFlatMapVar<A, B, V, M, S>
where
    A: VarValue,
    B: VarValue,
    V: Var<B>,
    M: FnMut(&A) -> V + 'static,
    S: Var<A>,
{
    type AsReadOnly = ReadOnlyVar<B, Self>;

    fn get<'a, Vr: AsRef<VarsRead>>(&'a self, vars: &'a Vr) -> &'a B {
        let vars = vars.as_ref();
        self.var(vars).get(vars)
    }

    fn get_new<'a, Vw: AsRef<Vars>>(&'a self, vars: &'a Vw) -> Option<&'a B> {
        let vars = vars.as_ref();
        if self.0.source.is_new(vars) {
            Some(self.var(vars).get(vars))
        } else {
            self.var(vars).get_new(vars)
        }
    }

    fn is_new<Vw: WithVars>(&self, vars: &Vw) -> bool {
        vars.with_vars(|vars| self.0.source.is_new(vars) || self.var(vars).is_new(vars))
    }

    fn version<Vr: WithVarsRead>(&self, vars: &Vr) -> u32 {
        let _ = vars.with_vars_read(|vars| self.var(vars));
        self.0.version.get()
    }

    fn is_read_only<Vw: WithVars>(&self, vars: &Vw) -> bool {
        vars.with_vars(|vars| self.var(vars).is_read_only(vars))
    }

    fn always_read_only(&self) -> bool {
        false
    }

    fn can_update(&self) -> bool {
        true
    }

    fn into_value<Vr: WithVarsRead>(self, vars: &Vr) -> B {
        vars.with_vars_read(|vars| {
            let _ = self.var(vars);
            match Rc::try_unwrap(self.0) {
                Ok(d) => d.var.into_inner().unwrap().into_value(vars),
                Err(r) => (Self(r)).var(vars).get_clone(vars),
            }
        })
    }

    fn modify<Vw, M2>(&self, vars: &Vw, modify: M2) -> Result<(), VarIsReadOnly>
    where
        Vw: WithVars,
        M2: FnOnce(&mut VarModify<B>) + 'static,
    {
        vars.with_vars(|vars| self.var(vars).modify(vars, modify))
    }

    fn strong_count(&self) -> usize {
        Rc::strong_count(&self.0)
    }

    fn into_read_only(self) -> Self::AsReadOnly {
        ReadOnlyVar::new(self)
    }

    fn update_mask<Vr: WithVarsRead>(&self, vars: &Vr) -> UpdateMask {
        let mut mask = self.0.source.update_mask(vars);
        mask.insert(self.0.update_slot);
        mask
    }
}
impl<A, B, V, M, S> IntoVar<B> for RcFlatMapVar<A, B, V, M, S>
where
    A: VarValue,
    B: VarValue,
    V: Var<B>,
    M: FnMut(&A) -> V + 'static,
    S: Var<A>,
{
    type Var = Self;

    fn into_var(self) -> Self::Var {
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::{var::*, context::TestWidgetContext};
    use std::fmt;

    #[derive(Clone)]
    pub struct Foo {
        pub bar: bool,
        pub var: RcVar<usize>,
    }
    impl fmt::Debug for Foo {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Foo").field("bar", &self.bar).finish_non_exhaustive()
        }
    }

    #[test]
    pub fn flat_map() {
        let source = var(Foo {
            bar: true,
            var: var(32)
        });

        let test = source.flat_map(|f| f.var.clone());

        let mut ctx = TestWidgetContext::new();

        assert_eq!(32, test.copy(&ctx));

        source.get(&ctx.vars).var.set(&ctx.vars, 42usize);

        let (_, ctx_updates) = ctx.apply_updates();

        assert!(ctx_updates.update);
        assert!(ctx.updates.current().intersects(&test.update_mask(&ctx.vars)));
        assert!(test.is_new(&ctx));
        assert_eq!(42, test.copy(&ctx));

        let (_, ctx_updates) = ctx.apply_updates();
        assert!(!ctx_updates.update);

        let old_var = source.get(&ctx).var.clone();
        source.set(&ctx, Foo {
            bar: false,
            var: var(192)
        });
        let (_, ctx_updates) = ctx.apply_updates();

        assert!(ctx_updates.update);
        assert!(ctx.updates.current().intersects(&test.update_mask(&ctx.vars)));
        assert!(test.is_new(&ctx));
        assert_eq!(192, test.copy(&ctx));

        let (_, ctx_updates) = ctx.apply_updates();
        assert!(!ctx_updates.update);

        old_var.set(&ctx, 220usize);
        let (_, ctx_updates) = ctx.apply_updates();
        assert!(ctx_updates.update);
        assert!(!ctx.updates.current().intersects(&test.update_mask(&ctx.vars)));
        assert!(!test.is_new(&ctx));
        assert_eq!(192, test.copy(&ctx));
    }
}
