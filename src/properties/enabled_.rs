use crate::prelude::new_property::*;

struct EnabledNode<C: UiNode, E: VarLocal<bool>> {
    child: C,
    enabled: E,
}
impl<C: UiNode, E: VarLocal<bool>> EnabledNode<C, E> {
    fn with_context(&mut self, vars: &Vars, f: impl FnOnce(&mut C)) {
        if IsEnabled::get(vars) {
            if *self.enabled.get(vars) {
                // context already enabled
                f(&mut self.child);
            } else {
                // we are disabling
                let child = &mut self.child;
                vars.with_context_bind(IsEnabledVar, &self.enabled, || f(child));
            }
        } else {
            // context already disabled
            f(&mut self.child);
        }
    }
}
#[impl_ui_node(child)]
impl<C: UiNode, E: VarLocal<bool>> UiNode for EnabledNode<C, E> {
    fn init(&mut self, ctx: &mut WidgetContext) {
        if !*self.enabled.init_local(ctx.vars) {
            ctx.widget_state.set(EnabledState, false);
        }
        self.with_context(ctx.vars, |c| c.init(ctx));
    }

    fn deinit(&mut self, ctx: &mut WidgetContext) {
        self.with_context(ctx.vars, |c| c.deinit(ctx));
    }

    fn update(&mut self, ctx: &mut WidgetContext) {
        if let Some(&enabled) = self.enabled.update_local(ctx.vars) {
            ctx.widget_state.set(EnabledState, enabled);
        }
        self.with_context(ctx.vars, |c| c.update(ctx));
    }

    fn update_hp(&mut self, ctx: &mut WidgetContext) {
        self.with_context(ctx.vars, |c| c.update_hp(ctx));
    }

    fn render(&self, frame: &mut FrameBuilder) {
        if !*self.enabled.get_local() {
            frame.meta().set(EnabledState, false);
        }
        self.child.render(frame);
    }
}

/// If events are enabled in the widget and its descendants.
///
/// This property sets the enabled state of the widget, to probe the enabled state in `when` clauses
/// use [`is_enabled`]. To probe from inside the implementation of widgets use [`IsEnabled::get`].
/// To probe the widget state use [`WidgetEnabledExt`].
///
/// # Events
///
/// Most `on_<event>` properties do not fire when the widget is disabled. The event properties that ignore
/// the enabled status mention this in their documentation.
///
/// Most app events ([`Event`](crate:core::event::Event)) still get generated by the app extensions.
/// [`MouseDownEvent`](crate::core::mouse::MouseDownEvent) for example is emitted for a click in a disabled widget.
/// The enabled parents of the disabled widget can handle this event.
///
/// # Focus
///
/// Disabled widgets are not focusable. The focus manager skips disabled widgets.
#[property(context)]
pub fn enabled(child: impl UiNode, enabled: impl IntoVar<bool>) -> impl UiNode {
    EnabledNode {
        child,
        enabled: enabled.into_local(),
    }
}

state_key! {
    struct EnabledState: bool;
}

/// Extension method for accessing the [`enabled`] state of widgets.
pub trait WidgetEnabledExt {
    /// Gets the widget enabled state.
    ///
    /// The implementation for [`LazyStateMap`] and [`Widget`] only get the state configured
    /// in the widget, if a parent widget is disabled that does not show here. Use [`IsEnabled`]
    /// to get the inherited state from inside a widget.
    ///
    /// The implementation for [`WidgetInfo`] gets if the widget and all ancestors are enabled.
    fn enabled(&self) -> bool;
}
impl WidgetEnabledExt for LazyStateMap {
    fn enabled(&self) -> bool {
        self.get(EnabledState).copied().unwrap_or(true)
    }
}
impl<W: Widget> WidgetEnabledExt for W {
    fn enabled(&self) -> bool {
        self.state().enabled()
    }
}
impl<'a> WidgetEnabledExt for WidgetInfo<'a> {
    fn enabled(&self) -> bool {
        self.meta().enabled() && self.parent().map(|p| p.enabled()).unwrap_or(true)
    }
}

context_var! {
    /// Don't use this directly unless you read all the enabled related
    /// source code here and in core/window.rs
    #[doc(hidden)]
    pub struct IsEnabledVar: bool = return &true;
}

/// Contextual [`enabled`] accessor.
pub struct IsEnabled;
impl IsEnabled {
    /// Gets the enabled state in the current `vars` context.
    pub fn get(vars: &Vars) -> bool {
        *IsEnabledVar::var().get(vars)
    }
}
