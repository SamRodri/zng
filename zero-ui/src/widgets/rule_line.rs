//! Rule line widgets.

use crate::prelude::new_widget::*;

/// Draws a horizontal or vertical rule line.
#[widget($crate::widgets::RuleLine)]
pub struct RuleLine(WidgetBase);
impl RuleLine {
    fn widget_intrinsic(&mut self) {
        self.widget_builder().push_build_action(on_build);
    }

    widget_impl! {
        /// Margin around the line.
        pub margin(margin: impl IntoVar<SideOffsets>);
    }
}

/// Line orientation.
#[property(CONTEXT, capture, default(LineOrientation::Horizontal), widget_impl(RuleLine))]
pub fn orientation(orientation: impl IntoVar<LineOrientation>) {}

/// Line color.
#[property(CONTEXT, capture, default(rgb(0, 0, 0)), widget_impl(RuleLine))]
pub fn color(color: impl IntoVar<Rgba>) {}

/// Line stroke thickness.
#[property(CONTEXT, capture, default(1), widget_impl(RuleLine))]
pub fn stroke_thickness(thickness: impl IntoVar<Length>) {}

/// Line length.
///
/// Set to [`Default`] to fill available length without requesting any length.
///
/// [`Default`]: Length::Default
#[property(CONTEXT, capture, default(Length::Default), widget_impl(RuleLine))]
pub fn length(length: impl IntoVar<Length>) {}

/// Line style.
#[property(CONTEXT, capture, default(LineStyle::Solid), widget_impl(RuleLine))]
pub fn line_style(style: impl IntoVar<LineStyle>) {}

fn on_build(wgt: &mut WidgetBuilding) {
    let mut bounds = PxSize::zero();

    let orientation = wgt
        .capture_var(property_id!(orientation))
        .unwrap_or_else(|| LineOrientation::Horizontal.into_var().boxed());

    let length = wgt
        .capture_var(property_id!(length))
        .unwrap_or_else(|| LocalVar(Length::Default).boxed());

    let stroke_thickness = wgt
        .capture_var(property_id!(stroke_thickness))
        .unwrap_or_else(|| LocalVar(Length::from(1)).boxed());

    let color = wgt
        .capture_var(property_id!(color))
        .unwrap_or_else(|| LocalVar(rgb(0, 0, 0)).boxed());

    let style = wgt
        .capture_var(property_id!(line_style))
        .unwrap_or_else(|| LineStyle::Solid.into_var().boxed());

    wgt.set_child(match_node_leaf(move |op| match op {
        UiNodeOp::Init => {
            WIDGET
                .sub_var_layout(&stroke_thickness)
                .sub_var_layout(&orientation)
                .sub_var_layout(&length)
                .sub_var_render(&color)
                .sub_var_render(&style);
        }
        UiNodeOp::Measure { desired_size, .. } => {
            let metrics = LAYOUT.metrics();
            let default_stroke = Dip::new(1).to_px(metrics.scale_factor().0);

            *desired_size = match orientation.get() {
                LineOrientation::Horizontal => PxSize::new(
                    length.layout_dft_x(metrics.constraints().x.fill()),
                    stroke_thickness.layout_dft_y(default_stroke),
                ),
                LineOrientation::Vertical => PxSize::new(
                    stroke_thickness.layout_dft_x(default_stroke),
                    length.layout_dft_y(metrics.constraints().y.fill()),
                ),
            };
        }
        UiNodeOp::Layout { final_size, .. } => {
            let metrics = LAYOUT.metrics();
            let default_stroke = Dip::new(1).to_px(metrics.scale_factor().0);

            let b = match orientation.get() {
                LineOrientation::Horizontal => PxSize::new(
                    length.layout_dft_x(metrics.constraints().x.fill()),
                    stroke_thickness.layout_dft_y(default_stroke),
                ),
                LineOrientation::Vertical => PxSize::new(
                    stroke_thickness.layout_dft_x(default_stroke),
                    length.layout_dft_y(metrics.constraints().y.fill()),
                ),
            };

            if b != bounds {
                bounds = b;
                WIDGET.render();
            }

            *final_size = b;
        }
        UiNodeOp::Render { frame } => {
            let bounds = PxRect::from_size(bounds);
            let orientation = orientation.get();
            let color = color.get();
            let style = style.get();
            frame.push_line(bounds, orientation, color.into(), style);
        }
        _ => {}
    }));
}

/// Horizontal rule line.
pub mod hr {
    use crate::prelude::new_widget::*;

    /// Draws an horizontal [`RuleLine!`](struct@RuleLine).
    #[widget($crate::widgets::Hr)]
    pub struct Hr(super::RuleLine);
    impl Hr {
        fn widget_intrinsic(&mut self) {
            widget_set! {
                self;
                orientation = LineOrientation::Horizontal;
                color = COLOR_VAR;
                stroke_thickness  = STROKE_THICKNESS_VAR;
                line_style = LINE_STYLE_VAR;
                margin = MARGIN_VAR;
            }
        }
    }

    context_var! {
        /// Line color, inherits from [`FONT_COLOR_VAR`].
        pub static COLOR_VAR: Rgba = text::FONT_COLOR_VAR.map(|c| c.with_alpha(30.pct()));

        /// Line stroke thickness, default is `1.dip()`
        pub static STROKE_THICKNESS_VAR: Length = 1.dip();

        /// Line style, default is `Solid`.
        pub static LINE_STYLE_VAR: LineStyle = LineStyle::Solid;

        /// Margin around line.
        ///
        /// Is `(5, 0)` by default, 5 top-bottom, 0 left-right.
        pub static MARGIN_VAR: SideOffsets = (5, 0);
    }

    /// Sets the [`COLOR_VAR`] that affects all horizontal rules inside the widget.
    #[property(CONTEXT, default(COLOR_VAR))]
    pub fn color(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode {
        with_context_var(child, COLOR_VAR, color)
    }

    /// Sets the [`STROKE_THICKNESS_VAR`] that affects all horizontal rules inside the widget.
    #[property(CONTEXT, default(STROKE_THICKNESS_VAR))]
    pub fn stroke_thickness(child: impl UiNode, thickness: impl IntoVar<Length>) -> impl UiNode {
        with_context_var(child, STROKE_THICKNESS_VAR, thickness)
    }

    /// Sets the [`LINE_STYLE_VAR`] that affects all horizontal rules inside the widget.
    #[property(CONTEXT, default(LINE_STYLE_VAR))]
    pub fn line_style(child: impl UiNode, style: impl IntoVar<LineStyle>) -> impl UiNode {
        with_context_var(child, LINE_STYLE_VAR, style)
    }

    /// Sets the [`MARGIN_VAR`] that affects all horizontal rules inside the widget.
    #[property(CONTEXT, default(MARGIN_VAR))]
    pub fn margin(child: impl UiNode, margin: impl IntoVar<SideOffsets>) -> impl UiNode {
        with_context_var(child, MARGIN_VAR, margin)
    }
}

/// Vertical rule line.
pub mod vr {
    use crate::prelude::new_widget::*;

    /// Draws a vertical [`RuleLine!`](struct@RuleLine).
    #[widget($crate::widgets::Vr)]
    pub struct Vr(super::RuleLine);
    impl Vr {
        fn widget_intrinsic(&mut self) {
            widget_set! {
                self;
                orientation = LineOrientation::Vertical;
                color = COLOR_VAR;
                stroke_thickness  = STROKE_THICKNESS_VAR;
                line_style = LINE_STYLE_VAR;
                margin = MARGIN_VAR;
            }
        }
    }

    context_var! {
        /// Line color, inherits from [`FONT_COLOR_VAR`].
        pub static COLOR_VAR: Rgba = text::FONT_COLOR_VAR.map(|c| c.with_alpha(30.pct()));

        /// Line stroke thickness, default is `1.dip()`
        pub static STROKE_THICKNESS_VAR: Length = 1.dip();

        /// Line style, default is `Solid`.
        pub static LINE_STYLE_VAR: LineStyle = LineStyle::Solid;

                /// Margin around line.
        ///
        /// Is `(0, 5)` by default, 0 top-bottom, 5 left-right.
        pub static MARGIN_VAR: SideOffsets = (0, 5);
    }

    /// Sets the [`COLOR_VAR`] that affects all vertical rules inside the widget.
    #[property(CONTEXT, default(COLOR_VAR))]
    pub fn color(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode {
        with_context_var(child, COLOR_VAR, color)
    }

    /// Sets the [`STROKE_THICKNESS_VAR`] that affects all vertical rules inside the widget.
    #[property(CONTEXT, default(STROKE_THICKNESS_VAR))]
    pub fn stroke_thickness(child: impl UiNode, thickness: impl IntoVar<Length>) -> impl UiNode {
        with_context_var(child, STROKE_THICKNESS_VAR, thickness)
    }

    /// Sets the [`LINE_STYLE_VAR`] that affects all vertical rules inside the widget.
    #[property(CONTEXT, default(LINE_STYLE_VAR))]
    pub fn line_style(child: impl UiNode, style: impl IntoVar<LineStyle>) -> impl UiNode {
        with_context_var(child, LINE_STYLE_VAR, style)
    }

    /// Sets the [`MARGIN_VAR`] that affects all vertical rules inside the widget.
    #[property(CONTEXT, default(MARGIN_VAR))]
    pub fn margin(child: impl UiNode, margin: impl IntoVar<SideOffsets>) -> impl UiNode {
        with_context_var(child, MARGIN_VAR, margin)
    }
}
