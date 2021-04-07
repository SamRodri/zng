use crate::prelude::new_widget::*;
use std::marker::PhantomData;

trait StackDimension: 'static {
    fn length(size: LayoutSize) -> f32;
    /// Orthogonal length.
    fn ort_length(size: LayoutSize) -> f32;
    /// (length, ort_length).
    fn lengths_mut(size: &mut LayoutSize) -> (&mut f32, &mut f32);
    fn origin_mut(origin: &mut LayoutPoint) -> &mut f32;
}

struct StackNode<C, S, D> {
    children: C,
    rectangles: Box<[LayoutRect]>,
    spacing: S,
    _d: PhantomData<D>,
}
#[impl_ui_node(children)]
impl<C, S, D> StackNode<C, S, D>
where
    C: WidgetList,
    S: VarLocal<Length>,
    D: StackDimension,
{
    fn new(children: C, spacing: S, _dimension: D) -> Self {
        StackNode {
            rectangles: vec![LayoutRect::zero(); children.len()].into_boxed_slice(),
            children,
            spacing,
            _d: PhantomData,
        }
    }

    #[UiNode]
    fn init(&mut self, ctx: &mut WidgetContext) {
        self.spacing.init_local(ctx.vars);
        self.children.init_all(ctx);
    }

    #[UiNode]
    fn update(&mut self, ctx: &mut WidgetContext) {
        if self.spacing.update_local(ctx.vars).is_some() {
            ctx.updates.layout();
        }
        self.children.update_all(ctx);
    }

    #[UiNode]
    fn measure(&mut self, mut available_size: LayoutSize, ctx: &mut LayoutContext) -> LayoutSize {
        *D::lengths_mut(&mut available_size).0 = LAYOUT_ANY_SIZE;

        let mut total_size = LayoutSize::zero();
        let (total_len, max_ort_len) = D::lengths_mut(&mut total_size);
        let spacing = self
            .spacing
            .get_local()
            .to_layout(LayoutLength::new(D::length(available_size)), ctx)
            .get();
        let mut first = true;
        let rectangles = &mut self.rectangles;
        self.children.measure_all(
            |_, _| available_size,
            |i, s, _| {
                let r = &mut rectangles[i];
                r.size = s;

                let origin = D::origin_mut(&mut r.origin);
                *origin = *total_len;
                *total_len += D::length(r.size);

                if first {
                    first = false;
                } else {
                    *origin += spacing;
                    *total_len += spacing;
                }

                *max_ort_len = max_ort_len.max(D::ort_length(r.size));
            },
            ctx,
        );

        total_size
    }

    #[UiNode]
    fn arrange(&mut self, final_size: LayoutSize, ctx: &mut LayoutContext) {
        let max_ort_len = D::ort_length(final_size);
        let rectangles = &mut self.rectangles;
        self.children.arrange_all(
            |i, _| {
                let mut size = rectangles[i].size;
                *D::lengths_mut(&mut size).1 = max_ort_len;
                size
            },
            ctx,
        );
    }

    #[UiNode]
    fn render(&self, frame: &mut FrameBuilder) {
        self.children.render_all(|i| self.rectangles[i].origin, frame);
    }
}

/// Horizontal stack layout.
///
/// # Example
///
/// ```
/// # use zero_ui::prelude::*;
/// let text = v_stack! {
///     spacing = 5.0;
///     items = widgets![
///         text("1. Hello"),
///         text("2. World"),
///     ];
/// };
/// ```
///
/// ## `h_stack()`
///
/// If you only want to set the `items` property you can use the [`h_stack`](function@h_stack) shortcut function.
#[widget($crate::widgets::layouts::h_stack)]
pub mod h_stack {
    use super::*;

    properties! {
        child {
            /// Space in-between items.
            spacing: impl IntoVar<Length> = 0.0;
            /// Widget items.
            #[allowed_in_when = false]
            items: impl WidgetList = widgets![];
            /// Items margin.
            margin as padding;
        }
    }

    #[inline]
    fn new_child(items: impl WidgetList, spacing: impl IntoVar<Length>) -> impl UiNode {
        StackNode::new(items, spacing.into_local(), HorizontalD)
    }

    struct HorizontalD;
    impl StackDimension for HorizontalD {
        fn length(size: LayoutSize) -> f32 {
            size.width
        }
        fn ort_length(size: LayoutSize) -> f32 {
            size.height
        }
        fn lengths_mut(size: &mut LayoutSize) -> (&mut f32, &mut f32) {
            (&mut size.width, &mut size.height)
        }
        fn origin_mut(origin: &mut LayoutPoint) -> &mut f32 {
            &mut origin.x
        }
    }
}

/// Vertical stack layout.
///
/// # Example
///
/// ```
/// # use zero_ui::prelude::*;
/// let text = h_stack! {
///     spacing = 5.0;
///     items = widgets![
///         text("Hello"),
///         text("World"),
///     ];
/// };
/// ```
/// ## `v_stack()`
///
/// If you only want to set the `items` property you can use the [`v_stack`](function@v_stack) shortcut function.
#[widget($crate::widgets::layouts::v_stack)]
pub mod v_stack {
    use super::*;

    properties! {
        child {
            /// Space in-between items.
            spacing: impl IntoVar<Length> = 0.0;
            /// Widget items.
            #[allowed_in_when = false]
            items: impl WidgetList = widgets![];
            /// Items margin.
            margin as padding;
        }
    }

    #[inline]
    fn new_child(items: impl WidgetList, spacing: impl IntoVar<Length>) -> impl UiNode {
        StackNode::new(items, spacing.into_local(), VerticalD)
    }

    struct VerticalD;
    impl StackDimension for VerticalD {
        fn length(size: LayoutSize) -> f32 {
            size.height
        }
        fn ort_length(size: LayoutSize) -> f32 {
            size.width
        }
        fn lengths_mut(size: &mut LayoutSize) -> (&mut f32, &mut f32) {
            (&mut size.height, &mut size.width)
        }
        fn origin_mut(origin: &mut LayoutPoint) -> &mut f32 {
            &mut origin.y
        }
    }
}

/// Basic horizontal stack layout.
///
/// # Example
///
/// ```
/// # use zero_ui::prelude::*;
/// let text = h_stack(widgets![
///     text("Hello "),
///     text("World"),
/// ]);
/// ```
///
/// # `h_stack!`
///
/// This function is just a shortcut for [`h_stack!`](module@v_stack). Use the full widget
/// to better configure the horizontal stack widget.
pub fn h_stack(items: impl WidgetList) -> impl Widget {
    h_stack! {
        items;
    }
}

/// Basic vertical stack layout.
///
/// # Example
///
/// ```
/// # use zero_ui::prelude::*;
/// let text = v_stack(widgets![
///     text("1. Hello"),
///     text("2. World"),
/// ]);
/// ```
///
/// # `v_stack!`
///
/// This function is just a shortcut for [`v_stack!`](module@v_stack). Use the full widget
/// to better configure the vertical stack widget.
pub fn v_stack(items: impl WidgetList) -> impl Widget {
    v_stack! {
        items;
    }
}

/// Layering stack layout.
///
/// # Example
///
/// ```
/// # use zero_ui::prelude::*;
/// let text = z_stack! {
///     padding = 5.0;
///     items = nodes![
///         text("under"),
///         text("over"),
///     ];
/// };
/// ```
///
/// ## `z_stack()`
///
/// If you only want to set the `items` property you can use the [`z_stack`](function@z_stack) shortcut function.
#[widget($crate::widgets::layouts::z_stack)]
pub mod z_stack {
    use super::*;

    properties! {
        child {
            /// UiNode items.
            #[allowed_in_when = false]
            items: impl UiNodeList = nodes![];
            /// Items margin.
            margin as padding;
        }
    }

    #[inline]
    fn new_child(items: impl UiNodeList) -> impl UiNode {
        ZStackNode { children: items }
    }

    struct ZStackNode<C: UiNodeList> {
        children: C,
    }
    #[impl_ui_node(children)]
    impl<C: UiNodeList> UiNode for ZStackNode<C> {}
}

/// Basic layering stack layout.
///
/// # Example
///
/// ```
/// # use zero_ui::prelude::*;
/// let text = z_stack(nodes![
///     text("under"),
///     text("over"),
/// ]);
/// ```
///
/// # `z_stack!`
///
/// This function is just a shortcut for [`z_stack!`](module@z_stack). Use the full widget
/// to better configure the layering stack widget.
pub fn z_stack(items: impl UiNodeList) -> impl Widget {
    z_stack! { items; }
}
