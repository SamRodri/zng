use crate::core::context::*;
use crate::core::var::*;
use crate::core::UiNode;
use crate::impl_ui_node;

struct SetContextVar<U: UiNode, T: VarValue, C: ContextVar<Type = T>, V: Var<T>> {
    child: U,
    var: C,
    value: V,
}
#[impl_ui_node(child)]
impl<U: UiNode, T: VarValue, C: ContextVar<Type = T>, V: Var<T>> UiNode for SetContextVar<U, T, C, V> {
    fn init(&mut self, ctx: &mut WidgetContext) {
        let child = &mut self.child;
        ctx.vars.with_context_bind(self.var, &self.value, || child.init(ctx));
    }
    fn deinit(&mut self, ctx: &mut WidgetContext) {
        let child = &mut self.child;
        ctx.vars.with_context_bind(self.var, &self.value, || child.deinit(ctx));
    }
    fn update(&mut self, ctx: &mut WidgetContext) {
        let child = &mut self.child;
        ctx.vars.with_context_bind(self.var, &self.value, || child.update(ctx));
    }
    fn update_hp(&mut self, ctx: &mut WidgetContext) {
        let child = &mut self.child;
        ctx.vars.with_context_bind(self.var, &self.value, || child.update_hp(ctx));
    }
}

/// Helper for declaring properties that set a context var.
///
/// # Example
/// ```
/// # #[macro_use] extern crate zero_ui;
/// # fn main() -> () { }
/// use zero_ui::properties::set_context_var;
/// use zero_ui::core::{UiNode, var::IntoVar};
///
/// context_var! {
///     pub struct FontSize: u32 = 14;
/// }
///
/// /// Sets the [`FontSize`](FontSize) context var.
/// #[property(context)]
/// pub fn font_size(child: impl UiNode, size: impl IntoVar<u32>) -> impl UiNode {
///     set_context_var(child, FontSize, size)
/// }
/// ```
pub fn set_context_var<T: VarValue>(child: impl UiNode, var: impl ContextVar<Type = T>, value: impl IntoVar<T>) -> impl UiNode {
    SetContextVar {
        child,
        var,
        value: value.into_var(),
    }
}
