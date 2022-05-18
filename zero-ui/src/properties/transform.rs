//! Transform properties, [`scale`](module@scale), [`rotate`](module@rotate), [`transform`](module@transform) and more.

use crate::prelude::new_property::*;

/// Custom transform.
///
/// See [`Transform`] for how to initialize a custom transform. The [`transform_origin`] is applied using the widget's inner size
/// for relative values.
///
/// [`transform_origin`]: fn@transform_origin
#[property(layout, default(Transform::identity()))]
pub fn transform(child: impl UiNode, transform: impl IntoVar<Transform>) -> impl UiNode {
    struct TransformNode<C, T> {
        child: C,
        transform: T,
    }
    #[impl_ui_node(child)]
    impl<C, T> UiNode for TransformNode<C, T>
    where
        C: UiNode,
        T: Var<Transform>,
    {
        fn subscriptions(&self, ctx: &mut InfoContext, subscriptions: &mut WidgetSubscriptions) {
            subscriptions.var(ctx, &self.transform);
            self.child.subscriptions(ctx, subscriptions);
        }

        fn update(&mut self, ctx: &mut WidgetContext) {
            self.child.update(ctx);
            if self.transform.is_new(ctx.vars) {
                ctx.updates.layout();
            }
        }

        fn layout(&mut self, ctx: &mut LayoutContext, wl: &mut WidgetLayout) -> PxSize {
            let size = self.child.layout(ctx, wl);

            let transform = self.transform.get(ctx.vars).layout(ctx.metrics);
            let av_size = ctx.widget_info.inner.size();
            let default_origin = PxPoint::new(av_size.width / 2.0, av_size.height / 2.0);
            let origin = ctx.with_constrains(
                |c| c.with_max_fill(av_size),
                |ctx| TransformOriginVar::get(ctx.vars).layout(ctx.metrics, |_| default_origin),
            );

            let x = origin.x.0 as f32;
            let y = origin.y.0 as f32;
            let transform = RenderTransform::translation(-x, -y, 0.0)
                .then(&transform)
                .then_translate(euclid::vec3(x, y, 0.0));

            wl.pre_transform(&transform);

            size
        }
    }
    TransformNode {
        child: child.cfg_boxed(),
        transform: transform.into_var(),
    }
    .cfg_boxed()
}

/// Rotate transform.
///
/// This property is a shorthand way of setting [`transform`] to [`rotate(angle)`](units::rotate) using variable mapping.
///
/// The rotation is done *around* the [`transform_origin`].
///
/// [`transform`]: fn@transform
/// [`transform_origin`]: fn@transform_origin
#[property(layout, default(0.rad()))]
pub fn rotate(child: impl UiNode, angle: impl IntoVar<AngleRadian>) -> impl UiNode {
    transform(child, angle.into_var().map(|&a| units::rotate(a)))
}

/// Scale transform.
///
/// This property is a shorthand way of setting [`transform`] to [`scale(s)`](units::scale) using variable mapping.
///
/// [`transform`]: fn@transform
#[property(layout, default(1.0))]
pub fn scale(child: impl UiNode, s: impl IntoVar<Factor>) -> impl UiNode {
    transform(child, s.into_var().map(|&x| units::scale(x)))
}

/// Scale X and Y transform.
///
/// This property is a shorthand way of setting [`transform`] to [`scale_xy(x, y)`](units::scale) using variable merging.
///
/// [`transform`]: fn@transform
#[property(layout, default(1.0, 1.0))]
pub fn scale_xy(child: impl UiNode, x: impl IntoVar<Factor>, y: impl IntoVar<Factor>) -> impl UiNode {
    transform(child, merge_var!(x.into_var(), y.into_var(), |&x, &y| units::scale_xy(x, y)))
}

/// Scale X transform.
///
/// This property is a shorthand way of setting [`transform`] to [`scale_x(x)`](units::scale_x) using variable mapping.
///
/// [`transform`]: fn@transform
#[property(layout, default(1.0))]
pub fn scale_x(child: impl UiNode, x: impl IntoVar<Factor>) -> impl UiNode {
    transform(child, x.into_var().map(|&x| units::scale_x(x)))
}

/// Scale Y transform.
///
/// This property is a shorthand way of setting [`transform`] to [`scale_y(y)`](units::scale_y) using variable mapping.
///
/// [`transform`]: fn@transform
#[property(layout, default(1.0))]
pub fn scale_y(child: impl UiNode, y: impl IntoVar<Factor>) -> impl UiNode {
    transform(child, y.into_var().map(|&y| units::scale_y(y)))
}

/// Skew transform.
///
/// This property is a shorthand way of setting [`transform`] to [`skew(x, y)`](units::skew) using variable merging.
///
/// [`transform`]: fn@transform
#[property(layout, default(0.rad(), 0.rad()))]
pub fn skew(child: impl UiNode, x: impl IntoVar<AngleRadian>, y: impl IntoVar<AngleRadian>) -> impl UiNode {
    transform(child, merge_var!(x.into_var(), y.into_var(), |&x, &y| units::skew(x, y)))
}

/// Skew X transform.
///
/// This property is a shorthand way of setting [`transform`] to [`skew_x(x)`](units::skew_x) using variable mapping.
///
/// [`transform`]: fn@transform
#[property(layout, default(0.rad()))]
pub fn skew_x(child: impl UiNode, x: impl IntoVar<AngleRadian>) -> impl UiNode {
    transform(child, x.into_var().map(|&x| units::skew_x(x)))
}

/// Skew Y transform.
///
/// This property is a shorthand way of setting [`transform`] to [`skew_y(y)`](units::skew_y) using variable mapping.
///
/// [`transform`]: fn@transform
#[property(layout)]
pub fn skew_y(child: impl UiNode, y: impl IntoVar<AngleRadian>) -> impl UiNode {
    transform(child, y.into_var().map(|&y| units::skew_y(y)))
}

/// Translate transform.
///
/// This property is a shorthand way of setting [`transform`] to [`translate(x, y)`](units::translate) using variable merging.
///
/// [`transform`]: fn@transform
#[property(layout, default(0, 0))]
pub fn translate(child: impl UiNode, x: impl IntoVar<Length>, y: impl IntoVar<Length>) -> impl UiNode {
    transform(
        child,
        merge_var!(x.into_var(), y.into_var(), |x, y| units::translate(x.clone(), y.clone())),
    )
}

/// Translate X transform.
///
/// This property is a shorthand way of setting [`transform`] to [`translate_x(x)`](units::translate_x) using variable mapping.
///
/// [`transform`]: fn@transform
#[property(layout, default(0))]
pub fn translate_x(child: impl UiNode, x: impl IntoVar<Length>) -> impl UiNode {
    transform(child, x.into_var().map(|x| units::translate_x(x.clone())))
}

/// Translate Y transform.
///
/// This property is a shorthand way of setting [`transform`] to [`translate_y(y)`](units::translate_y) using variable mapping.
///
/// [`transform`]: fn@transform
#[property(layout, default(0))]
pub fn translate_y(child: impl UiNode, y: impl IntoVar<Length>) -> impl UiNode {
    transform(child, y.into_var().map(|y| units::translate_y(y.clone())))
}

/// Point relative to the widget inner bounds around which the [`transform`] is applied.
///
/// This property sets the [`TransformOriginVar`] context variable.
#[property(context, default(TransformOriginVar::new()))]
pub fn transform_origin(child: impl UiNode, origin: impl IntoVar<Point>) -> impl UiNode {
    with_context_var(child, TransformOriginVar, origin)
}

context_var! {
    /// Point relative to the widget inner bounds around which the [`transform`] is applied.
    ///
    /// Default origin is the center (50%, 50%).
    pub struct TransformOriginVar: Point = Point::center();
}
