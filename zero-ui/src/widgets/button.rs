//! Button widget.

use crate::prelude::new_widget::*;

/// A clickable container.
#[widget($crate::widgets::Button)]
pub struct Button(FocusableMix<StyleMix<EnabledMix<Container>>>);
impl Button {
    fn widget_intrinsic(&mut self) {
        widget_set! {
            self;
            style_fn = STYLE_VAR;
            capture_mouse = true;
        }
    }

    widget_impl! {
        /// Button click event.
        ///
        /// # Examples
        ///
        /// ```
        /// # use zero_ui::prelude::*;
        /// # let _scope = App::minimal();
        /// #
        /// # Button! {
        /// on_click = hn!(|args: &ClickArgs| {
        ///     assert!(args.is_primary());
        ///     println!("button {:?} clicked!", WIDGET.id());
        /// });
        /// child = Text!("Click Me!");
        /// # }
        /// # ;
        /// ```
        pub crate::properties::events::gesture::on_click(handler: impl WidgetHandler<crate::core::gesture::ClickArgs>);

        /// If pointer interaction with other widgets is blocked while the button is pressed.
        ///
        /// Enabled by default in this widget.
        pub crate::properties::capture_mouse(mode: impl IntoVar<crate::core::mouse::CaptureMode>);
    }
}

context_var! {
    /// Button style in a context.
    ///
    /// Is the [`DefaultStyle!`] by default.
    ///
    /// [`DefaultStyle!`]: struct@DefaultStyle
    pub static STYLE_VAR: StyleFn = StyleFn::new(|_| DefaultStyle!());

    /// Idle background dark and light color.
    pub static BASE_COLORS_VAR: ColorPair = (rgb(0.18, 0.18, 0.18), rgb(0.82, 0.82, 0.82));
}

/// Sets the [`BASE_COLORS_VAR`] that is used to compute all background and border colors in the button style.
#[property(CONTEXT, default(BASE_COLORS_VAR), widget_impl(DefaultStyle))]
pub fn base_colors(child: impl UiNode, color: impl IntoVar<ColorPair>) -> impl UiNode {
    with_context_var(child, BASE_COLORS_VAR, color)
}

/// Sets the button style in a context, the parent style is fully replaced.
#[property(CONTEXT, default(STYLE_VAR))]
pub fn replace_style(child: impl UiNode, style: impl IntoVar<StyleFn>) -> impl UiNode {
    with_context_var(child, STYLE_VAR, style)
}

/// Extends the button style in a context, the parent style is used, properties of the same name set in
/// `style` override the parent style.
#[property(CONTEXT, default(StyleFn::nil()))]
pub fn extend_style(child: impl UiNode, style: impl IntoVar<StyleFn>) -> impl UiNode {
    style::with_style_extension(child, STYLE_VAR, style)
}

/// Create a [`color_scheme_highlight`] of `0.08`.
pub fn color_scheme_hovered(pair: impl IntoVar<ColorPair>) -> impl Var<Rgba> {
    color_scheme_highlight(pair, 0.08)
}

/// Create a [`color_scheme_highlight`] of `0.16`.
pub fn color_scheme_pressed(pair: impl IntoVar<ColorPair>) -> impl Var<Rgba> {
    color_scheme_highlight(pair, 0.16)
}

/// Button default style.
#[widget($crate::widgets::button::DefaultStyle)]
pub struct DefaultStyle(Style);
impl DefaultStyle {
    fn widget_intrinsic(&mut self) {
        use crate::properties::*;
        widget_set! {
            self;

            padding = (7, 15);
            corner_radius = 4;
            child_align = Align::CENTER;

            #[easing(150.ms())]
            background_color = color_scheme_pair(BASE_COLORS_VAR);

            #[easing(150.ms())]
            border = {
                widths: 1,
                sides: color_scheme_pair(BASE_COLORS_VAR).map_into()
            };

            when *#is_cap_hovered {
                #[easing(0.ms())]
                background_color = color_scheme_hovered(BASE_COLORS_VAR);
                #[easing(0.ms())]
                border = {
                    widths: 1,
                    sides: color_scheme_pressed(BASE_COLORS_VAR).map_into(),
                };
            }

            when *#is_pressed {
                #[easing(0.ms())]
                background_color = color_scheme_pressed(BASE_COLORS_VAR);
            }

            when *#is_disabled {
                saturate = false;
                child_opacity = 50.pct();
                cursor = CursorIcon::NotAllowed;
            }
        }
    }
}

/// Button link style.
///
/// Looks like a web hyperlink.
#[widget($crate::widgets::button::LinkStyle)]
pub struct LinkStyle(Style);
impl LinkStyle {
    fn widget_intrinsic(&mut self) {
        widget_set! {
            self;
            text::font_color = color_scheme_map(colors::LIGHT_BLUE, colors::BLUE);
            crate::properties::cursor = CursorIcon::Hand;

            when *#is_cap_hovered {
                text::underline = 1, LineStyle::Solid;
            }

            when *#is_pressed {
                text::font_color = color_scheme_map(colors::YELLOW, colors::BROWN);
            }

            when *#is_disabled {
                saturate = false;
                child_opacity = 50.pct();
                cursor = CursorIcon::NotAllowed;
            }
        }
    }
}

/// The flyout widget that can be set on a [`Button!`] to turn it into a *split-button*.
#[widget($crate::widgets::button::Split)]
pub struct Split(super::Toggle); // !!: inherit from ComboStyle.
impl Split {
    fn widget_intrinsic(&mut self) {
        widget_set! {
            self;
            crate::core::widget_base::child = NilUiNode;
            capture_mouse = true;
            on_pre_click = hn!(|args: &crate::core::gesture::ClickArgs| {
                args.propagation().stop();
            });
        }
    }
}

/// Turns the button into an *split-button*.
///
/// Can be set to any widget, the [`Split!`] is recommended.
///
/// [`Split!`]: struct@Split
#[property(CHILD_LAYOUT - 1, default(NilUiNode), widget_impl(Button))]
pub fn split(child: impl UiNode, split: impl UiNode) -> impl UiNode {
    child_insert(child, SPLIT_PLACE_VAR, split, SPLIT_SPACING_VAR)
}

/// Placement of the [`split`] widget in relation to the button content.
///
/// Sets the [`SPLIT_PLACE_VAR`].
///     
/// [`split`]: fn@split
#[property(CONTEXT, default(SPLIT_PLACE_VAR), widget_impl(Button))]
pub fn split_place(child: impl UiNode, place: impl IntoVar<ChildInsertPlace>) -> impl UiNode {
    with_context_var(child, SPLIT_PLACE_VAR, place)
}

/// Space between the [`split`] widget and the button content.
///
/// Sets the [`SPLIT_SPACING_VAR`].
///     
/// [`split`]: fn@split
#[property(CONTEXT, default(SPLIT_SPACING_VAR), widget_impl(Button))]
pub fn split_spacing(child: impl UiNode, spacing: impl IntoVar<Length>) -> impl UiNode {
    with_context_var(child, SPLIT_SPACING_VAR, spacing)
}

context_var! {
    /// Placement of the [`split`] widget.
    ///
    /// [`split`]: fn@split
    pub static SPLIT_PLACE_VAR: ChildInsertPlace = ChildInsertPlace::End;

    /// Space between the [`split`] widget and the button content.
    ///
    /// [`split`]: fn@split
    pub static SPLIT_SPACING_VAR: Length = 4;
}
