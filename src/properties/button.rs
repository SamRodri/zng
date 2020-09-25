use crate::core::color::Rgba;
use crate::core::property;
use crate::core::units::*;
use crate::core::var::IntoVar;
use crate::core::UiNode;
use crate::properties::{with_context_var, BorderDetails};
use crate::widgets::*;

/// Sets the [`ButtonBackgroundVar`] context var.
#[property(context)]
pub fn button_background(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode {
    with_context_var(child, ButtonBackgroundVar, color)
}

/// Sets the [`ButtonBackgroundHoveredVar`] context var.
#[property(context)]
pub fn button_background_hovered(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode {
    with_context_var(child, ButtonBackgroundHoveredVar, color)
}

/// Sets the [`ButtonBackgroundPressedVar`] context var.
#[property(context)]
pub fn button_background_pressed(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode {
    with_context_var(child, ButtonBackgroundPressedVar, color)
}

/// Sets the [`ButtonPaddingVar`] context var.
#[property(context)]
pub fn button_padding(child: impl UiNode, padding: impl IntoVar<SideOffsets>) -> impl UiNode {
    with_context_var(child, ButtonPaddingVar, padding)
}

/// Sets the [`ButtonBorderWidthsVarVar`](ButtonBorderDetailsVar) and [`ButtonBorderDetailsVarVar`] context var.
#[property(context)]
pub fn button_border(child: impl UiNode, widths: impl IntoVar<SideOffsets>, details: impl IntoVar<BorderDetails>) -> impl UiNode {
    let child = with_context_var(child, ButtonBorderWidthsVar, widths);
    with_context_var(child, ButtonBorderDetailsVar, details)
}

/// Sets the [`ButtonBorderWidthsVarVar`](ButtonBorderDetailsVar) and [`ButtonBorderDetailsVarVar`] context var.
#[property(context)]
pub fn button_border_hovered(child: impl UiNode, widths: impl IntoVar<SideOffsets>, details: impl IntoVar<BorderDetails>) -> impl UiNode {
    let child = with_context_var(child, ButtonBorderWidthsHoveredVar, widths);
    with_context_var(child, ButtonBorderDetailsHoveredVar, details)
}

/// Sets the [`ButtonBorderWidthsPressedVarVar`](ButtonBorderDetailsPressedVar) and [`ButtonBorderDetailsPressedVarVar`] context var.
#[property(context)]
pub fn button_border_pressed(child: impl UiNode, widths: impl IntoVar<SideOffsets>, details: impl IntoVar<BorderDetails>) -> impl UiNode {
    let child = with_context_var(child, ButtonBorderWidthsPressedVar, widths);
    with_context_var(child, ButtonBorderDetailsPressedVar, details)
}
