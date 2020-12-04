//! Text widgets.

use crate::prelude::new_widget::*;
use crate::properties::text_theme::*;

widget! {
    /// A configured [`text`](../fn.text.html).
    ///
    /// # Example
    ///
    /// ```
    /// use zero_ui::prelude::text;
    ///
    /// let hello_txt = text! {
    ///     font_family: "Arial";
    ///     font_size: 18;
    ///     text: "Hello!";
    /// };
    /// ```
    /// # `text()`
    ///
    /// If you don't need to configure the text, you can just use the function [`text`](../fn.text.html).
    pub text;

    default_child {
        /// The [`Text`](crate::core::types::Text) value.
        ///
        /// Set to an empty string (`""`).
        text -> text_value: "";
    }

    default {
        /// The text font. If not set inherits the `font_family` from the parent widget.
        font_family;
        /// The font style. If not set inherits the `font_style` from the parent widget.
        font_style;
        /// The font weight. If not set inherits the `font_weight` from the parent widget.
        font_weight;
        /// The font stretch. If not set inherits the `font_stretch` from the parent widget.
        font_stretch;
        /// The font size. If not set inherits the `font_size` from the parent widget.
        font_size;
        /// The text color. If not set inherits the `text_color` from the parent widget.
        color -> text_color;
        /// Height of each text line. If not set inherits the `line_height` from the parent widget.
        line_height;
    }

    /// Creates a [`text`](../fn.text.html).
    #[inline]
    fn new_child(text) -> impl UiNode {
        TextNode::new(text.unwrap().into_var())
    }
}

/// Simple text run.
///
/// # Configure
///
/// Text spans can be configured by setting [`font_family`](crate::properties::text_theme::font_family),
/// [`font_size`](crate::properties::text_theme::font_size) or [`text_color`](crate::properties::text_theme::text_color)
/// in parent widgets.
///
/// # Example
/// ```
/// # fn main() -> () {
/// use zero_ui::widgets::{container, text::text};
/// use zero_ui::properties::text_theme::{font_family, font_size};
///
/// let hello_txt = container! {
///     font_family: "Arial";
///     font_size: 18;
///     content: text("Hello!");
/// };
/// # }
/// ```
///
/// # `text!`
///
/// There is a specific widget for creating configured text runs: [`text!`](text/index.html).
pub fn text(text: impl IntoVar<Text> + 'static) -> impl Widget {
    // TODO remove 'static when rust issue #42940 is fixed.
    text! {
        text;
    }
}

widget! {
    strong;

    default_child {
        text -> text_value;
    }

    #[inline]
    fn new_child(text) -> impl UiNode {
        let text = TextNode::new(text.unwrap().into_var());
        font_weight::set(text, FontWeight::BOLD)
    }
}

/// A simple text run with **bold** font weight.
///
/// # Configure
///
/// Apart from the font weight this widget can be configured with contextual properties like [`text`](function@text).
pub fn strong(text: impl IntoVar<Text> + 'static) -> impl Widget {
    strong! { text; }
}

widget! {
    em;

    default_child {
        text -> text_value;
    }

    #[inline]
    fn new_child(text) -> impl UiNode {
        let text = TextNode::new(text.unwrap().into_var());
        font_style::set(text, FontStyle::Italic)
    }
}

/// A simple text run with *italic* font style.
///
/// # Configure
///
/// Apart from the font style this widget can be configured with contextual properties like [`text`](function@text).
pub fn em(text: impl IntoVar<Text> + 'static) -> impl Widget {
    em! { text; }
}

/// An UI node that renders a text using the [contextual text theme](TextContext).
pub struct TextNode<T: Var<Text>> {
    text_var: T,

    /* init, update data */
    // Transformed and white space corrected, or empty before init.
    text: SegmentedText,
    // Copy for render, or black before init.
    color: RenderColor,
    // Loaded from [font query](Fonts::get_or_default) during init.
    font_face: Option<FontFaceRef>,
    // Copy for layout, or zero before init.
    font_size: Length,
    font_synthesis: FontSynthesis,
    line_spacing: Length,

    /* measure, arrange data */
    //
    line_shaping_args: TextShapingArgs,
    layout_line_spacing: f32,
    // Font instance using the actual font_size.
    font: Option<FontRef>,
    // Shaped and wrapped text.
    shaped_text: ShapedText,
    // Box size of the text block.
    size: LayoutSize,
}

impl<T: Var<Text>> TextNode<T> {
    pub fn new(text: T) -> TextNode<T> {
        TextNode {
            text_var: text,

            text: SegmentedText::default(),
            color: colors::BLACK.into(),
            font_face: None,
            font_size: 0.into(),
            font_synthesis: FontSynthesis::DISABLED,
            line_spacing: 0.into(),

            line_shaping_args: TextShapingArgs::default(),
            layout_line_spacing: 0.0,
            font: None,
            shaped_text: ShapedText::default(),
            size: LayoutSize::zero(),
        }
    }
}

#[impl_ui_node(none)]
impl<T: Var<Text>> UiNode for TextNode<T> {
    fn init(&mut self, ctx: &mut WidgetContext) {
        let t_ctx = TextContext::get(ctx.vars);

        // TODO use the full list.
        self.font_face = Some(
            ctx.services
                .req::<Fonts>()
                .get_list(t_ctx.font_family, t_ctx.font_style, t_ctx.font_weight, t_ctx.font_stretch)
                .best()
                .clone(),
        );

        self.font_size = t_ctx.font_size;

        self.color = t_ctx.text_color.into();

        let text = self.text_var.get(ctx.vars).clone();
        let text = t_ctx.text_transform.transform(text);
        let text = t_ctx.white_space.transform(text);
        self.text = SegmentedText::new(text)
    }

    fn deinit(&mut self, _: &mut WidgetContext) {
        self.font = None;
        self.font_face = None;
        self.shaped_text = ShapedText::default();
        self.text = SegmentedText::default();
    }

    fn update(&mut self, ctx: &mut WidgetContext) {
        // update `self.text`, affects shaping and layout
        if let Some(text) = self.text_var.get_new(ctx.vars) {
            let (text_transform, white_space) = TextContext::text(ctx.vars);
            let text = text_transform.transform(text.clone());
            let text = white_space.transform(text);
            if self.text.text() != text {
                self.text = SegmentedText::new(text);
                self.shaped_text = ShapedText::default();

                ctx.updates.layout();
            }
        } else if let Some((text_transform, white_space)) = TextContext::text_update(ctx.vars) {
            let text = self.text_var.get(ctx.vars).clone();
            let text = text_transform.transform(text);
            let text = white_space.transform(text);
            if self.text.text() != text {
                self.text = SegmentedText::new(text);
                self.shaped_text = ShapedText::default();

                ctx.updates.layout();
            }
        }

        // update `self.font_face`, affects shaping and layout
        if let Some((font_family, font_style, font_weight, font_stretch)) = TextContext::font_update(ctx.vars) {
            let face = ctx
                .services
                .req::<Fonts>()
                .get_list(font_family, font_style, font_weight, font_stretch)
                .best()
                .clone();

            if !self.font_face.as_ref().map(|f| f.ptr_eq(&face)).unwrap_or_default() {
                self.font_face = Some(face);
                self.font = None;
                self.shaped_text = ShapedText::default();

                ctx.updates.layout();
            }
        }

        // update `self.font_instance`, affects shaping and layout
        if let Some((font_size, font_synthesis)) = TextContext::font_instance_update(ctx.vars) {
            if font_size != self.font_size || font_synthesis != self.font_synthesis {
                self.font_size = font_size;
                self.font_synthesis = font_synthesis;

                self.font = None;
                self.shaped_text = ShapedText::default();

                ctx.updates.layout();
            }
        }

        // TODO features, spacing, breaking.

        // update `self.color` and `self.glyph_options`, affects render
        if let Some(color) = TextContext::render_update(ctx.vars) {
            let color = RenderColor::from(color);
            if self.color != color {
                self.color = color;

                ctx.updates.render();
            }
        }
    }

    fn measure(&mut self, available_size: LayoutSize, ctx: &mut LayoutContext) -> LayoutSize {
        if self.font.is_none() {
            let size = self.font_size.to_layout(LayoutLength::new(available_size.width), ctx);
            self.font = Some(self.font_face.as_ref().expect("font not inited in measure").sized(size));
        };

        if self.shaped_text.is_empty() {
            // TODO
            let font = self.font.as_ref().unwrap();
            self.shaped_text = font.shape_text(&self.text, &self.line_shaping_args);
            self.size = self.shaped_text.size().snap_to(ctx.pixel_grid());
        }

        if !is_layout_any_size(available_size.width) && available_size.width < self.size.width {
            //TODO wrap here? or estimate the height pos wrap?
        }

        self.size
    }

    fn arrange(&mut self, final_size: LayoutSize, ctx: &mut LayoutContext) {
        // TODO use final size for wrapping?
        // http://www.unicode.org/reports/tr14/tr14-45.html
    }

    fn render(&self, frame: &mut FrameBuilder) {
        frame.push_text(
            LayoutRect::from_size(self.size),
            self.shaped_text.glyphs(),
            self.font.as_ref().expect("font not initied in render"),
            self.color,
            self.font_synthesis,
        );
    }
}
