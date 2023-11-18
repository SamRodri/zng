use std::{fmt, num::NonZeroU32, time::Duration};

use crate::{
    core::{
        l10n::*,
        text::{font_features::*, *},
    },
    prelude::{new_property::*, new_widget::widget_mixin, AnchorMode, AnchorOffset},
    properties::access,
};

/// Basic text font properties.
///
/// All properties in this mixin affects [`Text!`] nodes inside the widget where they are set.
///
/// See also [`FontFeaturesMix<P>`] for the other font properties.
///
/// [`Text!`]: struct@crate::widgets::Text
#[widget_mixin]
pub struct FontMix<P>(P);

context_var! {
    /// Font family of [`Text!`] spans.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static FONT_FAMILY_VAR: FontNames = FontNames::default();

    /// Font size of [`Text!`] spans.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static FONT_SIZE_VAR: FontSize = FontSize::Pt(11.0);

    /// Font weight of [`Text!`] spans.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static FONT_WEIGHT_VAR: FontWeight = FontWeight::NORMAL;

    /// Font style of [`Text!`] spans.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static FONT_STYLE_VAR: FontStyle = FontStyle::Normal;

    /// Font stretch of [`Text!`] spans.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static FONT_STRETCH_VAR: FontStretch = FontStretch::NORMAL;

    /// Font synthesis of [`Text!`] spans.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static FONT_SYNTHESIS_VAR: FontSynthesis = FontSynthesis::ENABLED;

    /// Font anti-aliasing of [`Text!`] spans.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static FONT_AA_VAR: FontAntiAliasing = FontAntiAliasing::Default;
}

impl FontMix<()> {
    /// Insert context variables used by properties in this mix-in.
    pub fn context_vars_set(set: &mut ContextValueSet) {
        set.insert_context_var(&FONT_FAMILY_VAR);
        set.insert_context_var(&FONT_SIZE_VAR);
        set.insert_context_var(&FONT_WEIGHT_VAR);
        set.insert_context_var(&FONT_STYLE_VAR);
        set.insert_context_var(&FONT_STRETCH_VAR);
        set.insert_context_var(&FONT_SYNTHESIS_VAR);
        set.insert_context_var(&FONT_AA_VAR);
    }
}

/// Font family name or list of names for texts in this widget or descendants.
///
/// All fonts in the list are resolved according to the [`font_style`], [`font_weight`] and [`font_stretch`] config.
/// During text shaping the first font on the list is preferred, but if the font does not cover a character or word, that
/// character or word falls-back to the second font in the list and so on.
///
/// Sets the [`FONT_FAMILY_VAR`].
///
/// [`font_style`]: fn@font_style
/// [`font_weight`]: fn@font_weight
/// [`font_stretch`]: fn@font_stretch
#[property(CONTEXT, default(FONT_FAMILY_VAR), widget_impl(FontMix<P>))]
pub fn font_family(child: impl UiNode, names: impl IntoVar<FontNames>) -> impl UiNode {
    with_context_var(child, FONT_FAMILY_VAR, names)
}

/// Sets the font size for the widget and descendants.
///
/// This property affects all texts inside the widget and the [`Length::Em`] unit.
///
/// Sets the [`FONT_SIZE_VAR`] context var and the [`LayoutMetrics::font_size`].
#[property(CONTEXT, default(FONT_SIZE_VAR), widget_impl(FontMix<P>))]
pub fn font_size(child: impl UiNode, size: impl IntoVar<FontSize>) -> impl UiNode {
    let child = match_node(child, |child, op| match op {
        UiNodeOp::Init => {
            WIDGET.sub_var_layout(&FONT_SIZE_VAR);
        }
        UiNodeOp::Measure { wm, desired_size } => {
            let font_size = FONT_SIZE_VAR.get();
            let font_size_px = font_size.layout_dft_x(LAYOUT.root_font_size());
            *desired_size = if font_size_px >= Px(0) {
                LAYOUT.with_font_size(font_size_px, || child.measure(wm))
            } else {
                tracing::error!("invalid font size {font_size:?} => {font_size_px:?}");
                child.measure(wm)
            };
        }
        UiNodeOp::Layout { wl, final_size } => {
            let font_size = FONT_SIZE_VAR.get();
            let font_size_px = font_size.layout_dft_x(LAYOUT.root_font_size());
            *final_size = if font_size_px >= Px(0) {
                LAYOUT.with_font_size(font_size_px, || child.layout(wl))
            } else {
                tracing::error!("invalid font size {font_size:?} => {font_size_px:?}");
                child.layout(wl)
            };
        }
        _ => {}
    });
    with_context_var(child, FONT_SIZE_VAR, size)
}

/// Defines how condensed or expanded the preferred font should be.
///
/// This value influences font resolution, the variant within the font family that is closest to this config will be selected.
///
/// Sets the [`FONT_WEIGHT_VAR`].
#[property(CONTEXT, default(FONT_WEIGHT_VAR), widget_impl(FontMix<P>))]
pub fn font_weight(child: impl UiNode, weight: impl IntoVar<FontWeight>) -> impl UiNode {
    with_context_var(child, FONT_WEIGHT_VAR, weight)
}

/// Defines the skew style of the font glyphs.
///
/// This value influences font resolution, the variant within the font family that is closest to this config will be selected.
///
/// Sets the [`FONT_STYLE_VAR`].
#[property(CONTEXT, default(FONT_STYLE_VAR), widget_impl(FontMix<P>))]
pub fn font_style(child: impl UiNode, style: impl IntoVar<FontStyle>) -> impl UiNode {
    with_context_var(child, FONT_STYLE_VAR, style)
}

/// Defines how condensed or expanded the preferred font should be.
///
/// This value influences font resolution, the variant within the font family that is closest to this config will be selected.
///
/// Sets the [`FONT_STRETCH_VAR`].
#[property(CONTEXT, default(FONT_STRETCH_VAR), widget_impl(FontMix<P>))]
pub fn font_stretch(child: impl UiNode, stretch: impl IntoVar<FontStretch>) -> impl UiNode {
    with_context_var(child, FONT_STRETCH_VAR, stretch)
}

/// Configure if a synthetic font is generated for fonts that do not implement **bold** or *oblique* variants.
///
/// Not all fonts implement the requested [`font_weight`] and [`font_style`], this config allows the renderer
/// to try and generate the style and weight anyway, using transforms and the glyph outlines.
///
/// Sets the [`FONT_SYNTHESIS_VAR`].
///
/// [`font_weight`]: fn@font_weight
/// [`font_style`]: fn@font_style_VAR`].
#[property(CONTEXT, default(FONT_SYNTHESIS_VAR), widget_impl(FontMix<P>))]
pub fn font_synthesis(child: impl UiNode, enabled: impl IntoVar<FontSynthesis>) -> impl UiNode {
    with_context_var(child, FONT_SYNTHESIS_VAR, enabled)
}

/// Configure the anti-aliasing used to render text glyphs inside the widget.
///
/// Uses the operating system configuration by default.
///
/// Sets the [`FONT_AA_VAR`].
#[property(CONTEXT, default(FONT_AA_VAR), widget_impl(FontMix<P>))]
pub fn font_aa(child: impl UiNode, aa: impl IntoVar<FontAntiAliasing>) -> impl UiNode {
    with_context_var(child, FONT_AA_VAR, aa)
}

/// Text color properties.
///
/// All properties in this mixin affects [`Text!`] nodes inside the widget where they are set.
///
/// [`Text!`]: struct@crate::widgets::Text
#[widget_mixin]
pub struct TextFillMix<P>(P);

context_var! {
    /// Color of [`Text!`] glyphs that are not colored by palette.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static FONT_COLOR_VAR: Rgba = COLOR_SCHEME_VAR.map(|s| match s {
        ColorScheme::Light => colors::BLACK,
        ColorScheme::Dark => colors::WHITE,
    });

    /// Color of [`Text!`] glyphs that are colored by palette, mostly Emoji.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static FONT_PALETTE_VAR: FontColorPalette = COLOR_SCHEME_VAR.map_into();

    /// Overrides of specific colors in the selected colored glyph palette.
    pub static FONT_PALETTE_COLORS_VAR: Vec<(u16, Rgba)> = vec![];
}

impl TextFillMix<()> {
    /// Insert context variables used by properties in this mix-in.
    pub fn context_vars_set(set: &mut ContextValueSet) {
        set.insert_context_var(&FONT_COLOR_VAR);
        set.insert_context_var(&FONT_PALETTE_VAR);
        set.insert_context_var(&FONT_PALETTE_COLORS_VAR);
    }
}

/// Defines the color the most text glyphs are filled with.
///
/// Colored glyphs (Emoji) are not affected by this, you can use [`font_palette`] to modify
/// Emoji colors.
///
/// Sets the [`FONT_COLOR_VAR`].
///
/// [`font_palette`]: fn@font_palette
#[property(CONTEXT, default(FONT_COLOR_VAR), widget_impl(TextFillMix<P>))]
pub fn font_color(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode {
    with_context_var(child, FONT_COLOR_VAR, color)
}

/// Defines the palette used to render colored glyphs (Emoji).
///
/// This property only affects Emoji from fonts using COLR v0. You can use [`font_color`] to set
/// the base color, and [`font_palette_colors`] to change specific colors.
///
/// Sets the [`FONT_PALETTE_VAR`].
///
/// [`font_color`]: fn@font_color
/// [`font_palette_colors`]: fn@font_palette_colors
#[property(CONTEXT, default(FONT_PALETTE_VAR), widget_impl(TextFillMix<P>))]
pub fn font_palette(child: impl UiNode, palette: impl IntoVar<FontColorPalette>) -> impl UiNode {
    with_context_var(child, FONT_PALETTE_VAR, palette)
}

/// Set the palette color in the font palette colors.
///
/// The `index` is pushed or replaced on the context [`FONT_PALETTE_COLORS_VAR`].
///
/// This function is a helper for declaring properties that configure the colors of a specific font, you
/// can use [`font_palette_colors`] to set all color overrides directly.
///
/// [`font_palette_colors`]: fn@font_palette_colors
pub fn with_font_palette_color(child: impl UiNode, index: u16, color: impl IntoVar<Rgba>) -> impl UiNode {
    with_context_var(
        child,
        FONT_PALETTE_COLORS_VAR,
        merge_var!(FONT_PALETTE_COLORS_VAR, color.into_var(), move |set, color| {
            let mut set = set.clone();
            if let Some(i) = set.iter().position(|(i, _)| *i == index) {
                set[i].1 = *color;
            } else {
                set.push((index, *color));
            }
            set
        }),
    )
}

/// Defines custom palette colors that affect Emoji colors.
///
/// The palette is selected by [`font_palette`] and then each valid index entry in this property replaces
/// the selected color.
///
/// Sets the [`FONT_PALETTE_COLORS_VAR`].
///
/// [`font_palette`]: fn@font_palette
#[property(CONTEXT, default(FONT_PALETTE_COLORS_VAR), widget_impl(TextFillMix<P>))]
pub fn font_palette_colors(child: impl UiNode, colors: impl IntoVar<Vec<(u16, Rgba)>>) -> impl UiNode {
    with_context_var(child, FONT_PALETTE_COLORS_VAR, colors)
}

/// Text align, justify.
///
/// All properties in this mixin affects [`Text!`] nodes inside the widget where they are set.
///
/// [`Text!`]: struct@crate::widgets::Text
#[widget_mixin]
pub struct TextAlignMix<P>(P);

context_var! {
    /// Text alignment inside the available space.
    pub static TEXT_ALIGN_VAR: Align = Align::START;

    /// Text alignment inside the available space when it overflows.
    pub static TEXT_OVERFLOW_ALIGN_VAR: Align = Align::TOP_START;

    /// Text justify mode when text align is fill.
    pub static JUSTIFY_VAR: Option<Justify> = None;
}

impl TextAlignMix<()> {
    /// Insert context variables used by properties in this mix-in.
    pub fn context_vars_set(set: &mut ContextValueSet) {
        set.insert_context_var(&TEXT_ALIGN_VAR);
        set.insert_context_var(&TEXT_OVERFLOW_ALIGN_VAR);
        set.insert_context_var(&JUSTIFY_VAR);
    }
}

/// Alignment of text inside available space.
///
/// Horizontal alignment is applied for each line independently, vertical alignment is applied for the entire
/// text block together.
///
/// Note that the [`Text!`] widget only implements this for text inside each instance in isolation, multiple
/// text instances in an inline row will not all align together by the [`Text!`] layout implementation alone.
///
/// Sets the [`TEXT_ALIGN_VAR`].
///
/// See also [`txt_overflow_align`], used when the text overflows.
///
/// [`Text!`]: struct@crate::widgets::Text
/// [`txt_overflow_align`]: fn@txt_overflow_align
#[property(CONTEXT, default(TEXT_ALIGN_VAR), widget_impl(TextAlignMix<P>))]
pub fn txt_align(child: impl UiNode, mode: impl IntoVar<Align>) -> impl UiNode {
    with_context_var(child, TEXT_ALIGN_VAR, mode)
}

/// Alignment of text inside available space when the text overflows.
///
/// Note that the [`Text!`] widget only implements this for text inside each instance in isolation, multiple
/// text instances in an inline row will not all align together. Also note that [`txt_overflow`] truncation
/// only applies to the end of the text after it is aligned, so unless this is [`Align::TOP_START`] (default) the
/// start of the text maybe still be clipped after truncation.
///
/// Sets the [`TEXT_OVERFLOW_ALIGN_VAR`].
///
/// [`Text!`]: struct@crate::widgets::Text
/// [`txt_overflow`]: fn@txt_overflow
#[property(CONTEXT, default(TEXT_OVERFLOW_ALIGN_VAR), widget_impl(TextAlignMix<P>))]
pub fn txt_overflow_align(child: impl UiNode, mode: impl IntoVar<Align>) -> impl UiNode {
    with_context_var(child, TEXT_OVERFLOW_ALIGN_VAR, mode)
}

/// Config the automatic spacing inserted between words and letters when text is aligned to fill.
///
/// Text alignment can be set to [`Align::FILL`], if this config is set to `Some(mode)` when that happens
/// the text layout will automatically insert spaces to try and *fill* the text block. When justify is not
/// enabled, that is set to `None`, fill alignment is the same as [`Align::START`].
///
/// Sets the [`JUSTIFY_VAR`].
#[property(CONTEXT, default(JUSTIFY_VAR), widget_impl(TextAlignMix<P>))]
pub fn justify(child: impl UiNode, mode: impl IntoVar<Option<Justify>>) -> impl UiNode {
    with_context_var(child, JUSTIFY_VAR, mode)
}

/// Text wrap, hyphenation.
///
/// All properties in this mixin affects [`Text!`] nodes inside the widget where they are set.
///
/// [`Text!`]: struct@crate::widgets::Text
#[widget_mixin]
pub struct TextWrapMix<P>(P);

context_var! {
    /// If line breaks are automatically inserted to fill the available space.
    ///
    /// The [`LINE_BREAK_VAR`], [`WORD_BREAK_VAR`] and [`HYPHENS_VAR`] configure how the text is split.
    ///
    /// Is `true` by default.
    pub static TEXT_WRAP_VAR: bool = true;

    /// Configuration of line breaks inside words during text wrap.
    pub static WORD_BREAK_VAR: WordBreak = WordBreak::Normal;

    /// Configuration of line breaks in Chinese, Japanese, or Korean text.
    pub static LINE_BREAK_VAR: LineBreak = LineBreak::Auto;

    /// Text hyphenation config.
    pub static HYPHENS_VAR: Hyphens = Hyphens::default();

    /// Hyphen text rendered when auto-hyphenating.
    pub static HYPHEN_CHAR_VAR: Txt = Txt::from_char('-');

    /// Text overflow handling.
    pub static TEXT_OVERFLOW_VAR: TextOverflow = TextOverflow::Ignore;
}

impl TextWrapMix<()> {
    /// Insert context variables used by properties in this mix-in.
    pub fn context_vars_set(set: &mut ContextValueSet) {
        set.insert_context_var(&TEXT_WRAP_VAR);
        set.insert_context_var(&WORD_BREAK_VAR);
        set.insert_context_var(&LINE_BREAK_VAR);
        set.insert_context_var(&HYPHENS_VAR);
        set.insert_context_var(&HYPHEN_CHAR_VAR);
        set.insert_context_var(&TEXT_OVERFLOW_VAR);
    }
}

/// Defines how text overflow is handled by the text widgets.
#[derive(Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TextOverflow {
    /// Text is allowed to overflow.
    ///
    /// Note that the text widget can still [`clip_to_bounds`], and text widgets also clip any text
    /// that overflows over one line-height in any direction. Text overflow is tracked even if `Ignore`
    /// is set, so custom properties may also implement some form of overflow handling.
    ///
    /// [`clip_to_bounds`]: fn@crate::properties::clip_to_bounds
    Ignore,
    /// Truncate the text so it will fit, the associated `Txt` is a suffix appended to the truncated text.
    ///
    /// Note that if the suffix is not empty the text will truncated more to reserve space for the suffix. If
    /// the suffix itself is too wide it will overflow.
    Truncate(Txt),
}
impl fmt::Debug for TextOverflow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            write!(f, "TextOverflow")?
        }
        match self {
            Self::Ignore => write!(f, "Ignore"),
            Self::Truncate(arg0) => f.debug_tuple("Truncate").field(arg0).finish(),
        }
    }
}
impl TextOverflow {
    /// Truncate without suffix.
    pub fn truncate() -> Self {
        Self::Truncate(Txt::from_static(""))
    }

    /// Truncate with the ellipses `'…'` char as suffix.
    pub fn ellipses() -> Self {
        Self::Truncate(Txt::from_char('…'))
    }
}
impl_from_and_into_var! {
    /// Truncate (no suffix), or ignore.
    fn from(truncate: bool) -> TextOverflow {
        if truncate {
            TextOverflow::truncate()
        } else {
            TextOverflow::Ignore
        }
    }

    fn from(truncate: Txt) -> TextOverflow {
        TextOverflow::Truncate(truncate)
    }
    fn from(s: &'static str) -> TextOverflow {
        Txt::from(s).into()
    }
    fn from(s: String) -> TextOverflow {
        Txt::from(s).into()
    }
    fn from(c: char) -> TextOverflow {
        Txt::from(c).into()
    }
}

/// Enables or disables text wrap.
///
/// If enabled, line-breaks and hyphens are automatically inserted to flow the text to fill the available width. Wrap
/// can be configured using the [`line_break`], [`word_break`] and [`hyphens`] properties.
///
/// Sets the [`TEXT_WRAP_VAR`].
///
/// [`line_break`]: fn@line_break
/// [`word_break`]: fn@word_break
/// [`hyphens`]: fn@hyphens
#[property(CONTEXT, default(TEXT_WRAP_VAR), widget_impl(TextWrapMix<P>))]
pub fn txt_wrap(child: impl UiNode, enabled: impl IntoVar<bool>) -> impl UiNode {
    with_context_var(child, TEXT_WRAP_VAR, enabled)
}

/// Configure line breaks inside words during text wrap.
///
/// This value is only considered if it is impossible to fit a full word to a line.
///
/// Hyphens can be inserted in word breaks using the [`hyphens`] configuration.
///
/// Sets the [`WORD_BREAK_VAR`].
///
/// [`hyphens`]: fn@hyphens
#[property(CONTEXT, default(WORD_BREAK_VAR), widget_impl(TextWrapMix<P>))]
pub fn word_break(child: impl UiNode, mode: impl IntoVar<WordBreak>) -> impl UiNode {
    with_context_var(child, WORD_BREAK_VAR, mode)
}

/// Configuration of text wrapping for Chinese, Japanese, or Korean text.
///
/// Sets the [`LINE_BREAK_VAR`].
#[property(CONTEXT, default(LINE_BREAK_VAR), widget_impl(TextWrapMix<P>))]
pub fn line_break(child: impl UiNode, mode: impl IntoVar<LineBreak>) -> impl UiNode {
    with_context_var(child, LINE_BREAK_VAR, mode)
}

/// Configure hyphenation.
///
/// Note that for automatic hyphenation to work the [`lang`] must also be set and the [`Hyphenation`] service must support it.
///
/// The auto hyphenation char can be defined using [`hyphen_char`].
///
/// [`Hyphenation`]: crate::core::text::Hyphenation
/// [`lang`]: fn@lang
/// [`hyphen_char`]: fn@hyphen_char
#[property(CONTEXT, default(HYPHENS_VAR), widget_impl(TextWrapMix<P>))]
pub fn hyphens(child: impl UiNode, hyphens: impl IntoVar<Hyphens>) -> impl UiNode {
    with_context_var(child, HYPHENS_VAR, hyphens)
}

/// The char or small string that is rendered when text is auto-hyphenated.
///
/// Note that hyphenation is enabled by the [`hyphens`] property.
///
/// [`hyphens`]: fn@hyphens
#[property(CONTEXT, default(HYPHEN_CHAR_VAR), widget_impl(TextWrapMix<P>))]
pub fn hyphen_char(child: impl UiNode, hyphen: impl IntoVar<Txt>) -> impl UiNode {
    with_context_var(child, HYPHEN_CHAR_VAR, hyphen)
}

/// Defines if text overflow is truncated, with optional suffix append.
///
/// When enabled overflow is truncated by character or by the wrap rules if [`txt_wrap`] is enabled (it is by default).
///
/// Overflow is always ignored when the text is editable.
///
/// [`txt_wrap`]: fn@txt_wrap
#[property(CONTEXT, default(TEXT_OVERFLOW_VAR), widget_impl(TextWrapMix<P>))]
pub fn txt_overflow(child: impl UiNode, overflow: impl IntoVar<TextOverflow>) -> impl UiNode {
    with_context_var(child, TEXT_OVERFLOW_VAR, overflow)
}

/// Gets if the text is overflown.
#[property(CHILD_LAYOUT+100, widget_impl(TextWrapMix<P>))]
pub fn is_overflown(child: impl UiNode, state: impl IntoVar<bool>) -> impl UiNode {
    let state = state.into_var();
    match_node(child, move |_, op| match op {
        UiNodeOp::Deinit => {
            let _ = state.set(false);
        }
        UiNodeOp::Layout { .. } => {
            let is_o = super::nodes::LayoutText::get().overflow.is_some();
            if is_o != state.get() {
                let _ = state.set(is_o);
            }
        }
        _ => {}
    })
}

/// Gets if the text has an entire line overflown.
///
/// This is `true` when the text has multiple lines, either due to line-break or wrap, and at
/// least one line overflows the allowed height, partially or fully.
#[property(CHILD_LAYOUT+100, widget_impl(TextWrapMix<P>))]
pub fn is_line_overflown(child: impl UiNode, state: impl IntoVar<bool>) -> impl UiNode {
    let state = state.into_var();
    match_node(child, move |_, op| match op {
        UiNodeOp::Deinit => {
            let _ = state.set(false);
        }
        UiNodeOp::Layout { .. } => {
            let txt = super::nodes::LayoutText::get();
            let is_o = if let Some(info) = &txt.overflow {
                info.line < txt.shaped_text.lines_len().saturating_sub(1) as _
            } else {
                false
            };
            if is_o != state.get() {
                let _ = state.set(is_o);
            }
        }
        _ => {}
    })
}

/// Gets the overflow text, that is a clone of the text starting from the first overflow character.
///
/// Note that overflow is tracked even if [`txt_overflow`] is set to [`TextOverflow::Ignore`].
///
/// [`txt_overflow`]: fn@txt_overflow
#[property(CHILD_LAYOUT+100, widget_impl(TextWrapMix<P>))]
pub fn get_overflow(child: impl UiNode, txt: impl IntoVar<Txt>) -> impl UiNode {
    let txt = txt.into_var();
    match_node(child, move |_, op| {
        if let UiNodeOp::Layout { .. } = op {
            let l_txt = super::nodes::LayoutText::get();
            if let Some(info) = &l_txt.overflow {
                let r = super::nodes::ResolvedText::get();
                let tail = &r.segmented_text.text()[info.text_char..];
                if txt.with(|t| t != tail) {
                    let _ = txt.set(Txt::from_str(tail));
                }
            } else if txt.with(|t| !t.is_empty()) {
                let _ = txt.set(Txt::from_static(""));
            }
        }
    })
}

/// Text underline, overline and strikethrough lines.
///
/// All properties in this mixin affects [`Text!`] nodes inside the widget where they are set.
///
/// [`Text!`]: struct@crate::widgets::Text
#[widget_mixin]
pub struct TextDecorationMix<P>(P);

context_var! {
    /// Underline thickness.
    pub static UNDERLINE_THICKNESS_VAR: UnderlineThickness = 0;
    /// Underline style.
    pub static UNDERLINE_STYLE_VAR: LineStyle = LineStyle::Hidden;
    /// Underline color, inherits from [`FONT_COLOR_VAR`].
    pub static UNDERLINE_COLOR_VAR: Rgba = FONT_COLOR_VAR;
    /// Parts of text skipped by underline.
    pub static UNDERLINE_SKIP_VAR: UnderlineSkip = UnderlineSkip::DEFAULT;
    /// Position of the underline.
    pub static UNDERLINE_POSITION_VAR: UnderlinePosition = UnderlinePosition::Font;

    /// Overline thickness.
    pub static OVERLINE_THICKNESS_VAR: TextLineThickness = 0;
    /// Overline style.
    pub static OVERLINE_STYLE_VAR: LineStyle = LineStyle::Hidden;
    /// Overline color, inherits from [`FONT_COLOR_VAR`].
    pub static OVERLINE_COLOR_VAR: Rgba = FONT_COLOR_VAR;

    /// Strikethrough thickness.
    pub static STRIKETHROUGH_THICKNESS_VAR: TextLineThickness = 0;
    /// Strikethrough style.
    pub static  STRIKETHROUGH_STYLE_VAR: LineStyle = LineStyle::Hidden;
    /// Strikethrough color, inherits from [`FONT_COLOR_VAR`].
    pub static STRIKETHROUGH_COLOR_VAR: Rgba = FONT_COLOR_VAR;
}

impl TextDecorationMix<()> {
    /// Insert context variables used by properties in this mix-in.
    pub fn context_vars_set(set: &mut ContextValueSet) {
        set.insert_context_var(&UNDERLINE_THICKNESS_VAR);
        set.insert_context_var(&UNDERLINE_STYLE_VAR);
        set.insert_context_var(&UNDERLINE_COLOR_VAR);
        set.insert_context_var(&UNDERLINE_SKIP_VAR);
        set.insert_context_var(&UNDERLINE_POSITION_VAR);
        set.insert_context_var(&OVERLINE_THICKNESS_VAR);
        set.insert_context_var(&OVERLINE_STYLE_VAR);
        set.insert_context_var(&OVERLINE_COLOR_VAR);
        set.insert_context_var(&STRIKETHROUGH_THICKNESS_VAR);
        set.insert_context_var(&STRIKETHROUGH_STYLE_VAR);
        set.insert_context_var(&STRIKETHROUGH_COLOR_VAR);
    }
}

/// Draw lines *under* each text line.
///
/// Sets the [`UNDERLINE_THICKNESS_VAR`] and [`UNDERLINE_STYLE_VAR`].
#[property(CONTEXT, default(UNDERLINE_THICKNESS_VAR, UNDERLINE_STYLE_VAR), widget_impl(TextDecorationMix<P>))]
pub fn underline(child: impl UiNode, thickness: impl IntoVar<UnderlineThickness>, style: impl IntoVar<LineStyle>) -> impl UiNode {
    let child = with_context_var(child, UNDERLINE_THICKNESS_VAR, thickness);
    with_context_var(child, UNDERLINE_STYLE_VAR, style)
}
/// Custom [`underline`](fn@underline) color, if not set
/// the [`font_color`](fn@font_color) is used.
///
/// Sets the [`UNDERLINE_COLOR_VAR`].
#[property(CONTEXT, default(UNDERLINE_COLOR_VAR), widget_impl(TextDecorationMix<P>))]
pub fn underline_color(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode {
    with_context_var(child, UNDERLINE_COLOR_VAR, color)
}
/// Defines what segments of each text line are skipped when tracing the [`underline`](fn@underline).
///
/// By default skips glyphs that intercept the underline.
///
/// Sets the [`UNDERLINE_SKIP_VAR`].
#[property(CONTEXT, default(UNDERLINE_SKIP_VAR), widget_impl(TextDecorationMix<P>))]
pub fn underline_skip(child: impl UiNode, skip: impl IntoVar<UnderlineSkip>) -> impl UiNode {
    with_context_var(child, UNDERLINE_SKIP_VAR, skip)
}
/// Defines what font line gets traced by the underline.
///
/// By default uses the font configuration, but it usually crosses over glyph *descents* causing skips on
/// the line, you can set this [`UnderlinePosition::Descent`] to fully clear all glyph *descents*.
///
/// Sets the [`UNDERLINE_POSITION_VAR`].
#[property(CONTEXT, default(UNDERLINE_POSITION_VAR), widget_impl(TextDecorationMix<P>))]
pub fn underline_position(child: impl UiNode, position: impl IntoVar<UnderlinePosition>) -> impl UiNode {
    with_context_var(child, UNDERLINE_POSITION_VAR, position)
}

/// Draw lines *above* each text line.
///
/// Sets the [`OVERLINE_THICKNESS_VAR`] and [`OVERLINE_STYLE_VAR`].
#[property(CONTEXT, default(OVERLINE_THICKNESS_VAR, OVERLINE_STYLE_VAR), widget_impl(TextDecorationMix<P>))]
pub fn overline(child: impl UiNode, thickness: impl IntoVar<TextLineThickness>, style: impl IntoVar<LineStyle>) -> impl UiNode {
    let child = with_context_var(child, OVERLINE_THICKNESS_VAR, thickness);
    with_context_var(child, OVERLINE_STYLE_VAR, style)
}
/// Custom [`overline`](fn@overline) color, if not set
/// the [`font_color`](fn@font_color) is used.
///
/// Sets the [`OVERLINE_COLOR_VAR`].
#[property(CONTEXT, default(OVERLINE_COLOR_VAR), widget_impl(TextDecorationMix<P>))]
pub fn overline_color(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode {
    with_context_var(child, OVERLINE_COLOR_VAR, color)
}

/// Draw lines across each text line.
///
/// Sets the [`STRIKETHROUGH_THICKNESS_VAR`] and [`STRIKETHROUGH_STYLE_VAR`].
#[property(CONTEXT, default(STRIKETHROUGH_THICKNESS_VAR, STRIKETHROUGH_STYLE_VAR), widget_impl(TextDecorationMix<P>))]
pub fn strikethrough(child: impl UiNode, thickness: impl IntoVar<TextLineThickness>, style: impl IntoVar<LineStyle>) -> impl UiNode {
    let child = with_context_var(child, STRIKETHROUGH_THICKNESS_VAR, thickness);
    with_context_var(child, STRIKETHROUGH_STYLE_VAR, style)
}
/// Custom [`strikethrough`](fn@strikethrough) color, if not set
/// the [`font_color`](fn@font_color) is used.
///
/// Sets the [`STRIKETHROUGH_COLOR_VAR`].
#[property(CONTEXT, default(STRIKETHROUGH_COLOR_VAR), widget_impl(TextDecorationMix<P>))]
pub fn strikethrough_color(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode {
    with_context_var(child, STRIKETHROUGH_COLOR_VAR, color)
}

/// Text spacing properties.
///
/// All properties in this mixin affects [`Text!`] nodes inside the widget where they are set.
///
/// See also [`ParagraphMix<P>`] for paragraph spacing.
///
/// [`Text!`]: struct@crate::widgets::Text
#[widget_mixin]
pub struct TextSpacingMix<P>(P);

context_var! {
    /// Text line height of [`Text!`] spans.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static LINE_HEIGHT_VAR: LineHeight = LineHeight::Default;

    /// Extra spacing in between lines of [`Text!`] spans.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static LINE_SPACING_VAR: LineSpacing = LineSpacing::Default;

    /// Extra letter spacing of [`Text!`] spans.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static LETTER_SPACING_VAR: LetterSpacing = LetterSpacing::Default;

    /// Extra word spacing of [`Text!`] spans.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static WORD_SPACING_VAR: WordSpacing = WordSpacing::Default;

    /// Length of the `TAB` space.
    pub static TAB_LENGTH_VAR: TabLength = 400.pct();
}

impl TextSpacingMix<()> {
    /// Insert context variables used by properties in this mix-in.
    pub fn context_vars_set(set: &mut ContextValueSet) {
        set.insert_context_var(&LINE_HEIGHT_VAR);
        set.insert_context_var(&LINE_SPACING_VAR);
        set.insert_context_var(&LETTER_SPACING_VAR);
        set.insert_context_var(&WORD_SPACING_VAR);
        set.insert_context_var(&TAB_LENGTH_VAR);
    }
}

/// Height of each text line. If not set inherits the `line_height` from the parent widget.
///
/// The [`Default`] value is computed from the font metrics, `ascent - descent + line_gap`, this is
/// usually similar to `1.2.em()`. Relative values are computed from the default value, so `200.pct()` is double
/// the default line height.
///
/// The text is vertically centralized inside the height.
///
/// [`Default`]: Length::Default
///
/// Sets the [`LINE_HEIGHT_VAR`].
#[property(CONTEXT, default(LINE_HEIGHT_VAR), widget_impl(TextSpacingMix<P>))]
pub fn line_height(child: impl UiNode, height: impl IntoVar<LineHeight>) -> impl UiNode {
    with_context_var(child, LINE_HEIGHT_VAR, height)
}

/// Extra spacing added in between text letters. If not set inherits the `letter_spacing` from the parent widget.
///
/// Letter spacing is computed using the font data, this unit represents
/// extra space added to the computed spacing.
///
/// A "letter" is a character glyph cluster, e.g.: `a`, `â`, `1`, `-`, `漢`.
///
/// The [`Default`] value signals that letter spacing can be tweaked when text *justification* is enabled, all other
/// values disable automatic adjustments for justification inside words.
///
/// Relative values are computed from the length of the space `' '` character.
///
/// [`Default`]: Length::Default
///
/// This property sets the [`LETTER_SPACING_VAR`] context var that affects all inner texts.
#[property(CONTEXT, default(LETTER_SPACING_VAR), widget_impl(TextSpacingMix<P>))]
pub fn letter_spacing(child: impl UiNode, extra: impl IntoVar<LetterSpacing>) -> impl UiNode {
    with_context_var(child, LETTER_SPACING_VAR, extra)
}

/// Extra spacing in-between text lines. If not set inherits the `line_spacing` from the parent widget.
///
/// The [`Default`] value is zero. Relative values are calculated from the [`LineHeight`], so `50.pct()` is half
/// the computed line height. If the text only has one line this property is not used.
///
/// [`Default`]: Length::Default
///
/// Sets the [`LINE_SPACING_VAR`].
#[property(CONTEXT, default(LINE_SPACING_VAR), widget_impl(TextSpacingMix<P>))]
pub fn line_spacing(child: impl UiNode, extra: impl IntoVar<LineSpacing>) -> impl UiNode {
    with_context_var(child, LINE_SPACING_VAR, extra)
}

/// Extra spacing added to the Unicode `U+0020 SPACE` character. If not set inherits the `letter_spacing` from the parent widget.
///
/// Word spacing is done using the space character "advance" as defined in the font,
/// this unit represents extra spacing added to that default spacing.
///
/// A "word" is the sequence of characters in-between space characters. This extra
/// spacing is applied per space character not per word, if there are three spaces between words
/// the extra spacing is applied thrice. Usually the number of spaces between words is collapsed to one,
/// see [`WhiteSpace`](crate::core::text::WhiteSpace).
///
/// The [`Default`] value signals that word spacing can be tweaked when text *justification* is enabled, all other
/// values disable automatic adjustments for justification. Relative values are computed from the length of the space `' '` character,
/// so a word spacing of `100.pct()` visually adds *another* space in between words.
///
/// [`Default`]: Length::Default
///
/// This property sets the [`WORD_SPACING_VAR`] context var that affects all inner widgets.
#[property(CONTEXT, default(WORD_SPACING_VAR), widget_impl(TextSpacingMix<P>))]
pub fn word_spacing(child: impl UiNode, extra: impl IntoVar<WordSpacing>) -> impl UiNode {
    with_context_var(child, WORD_SPACING_VAR, extra)
}

/// Length of the TAB character space, relative to the normal space advance.
///
/// Is set to `400.pct()` by default, so 4 times a space.
///
/// Sets the [`TAB_LENGTH_VAR`].
#[property(CONTEXT, default(TAB_LENGTH_VAR), widget_impl(TextSpacingMix<P>))]
pub fn tab_length(child: impl UiNode, length: impl IntoVar<TabLength>) -> impl UiNode {
    with_context_var(child, TAB_LENGTH_VAR, length)
}

/// Text transform properties.
///
/// All properties in this mixin affects [`Text!`] nodes inside the widget where they are set.
///
/// [`Text!`]: struct@crate::widgets::Text
#[widget_mixin]
pub struct TextTransformMix<P>(P);

context_var! {
    /// Text white space transform of [`Text!`] spans.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static WHITE_SPACE_VAR: WhiteSpace = WhiteSpace::Preserve;

    /// Text transformation function applied to [`Text!`] spans.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static TEXT_TRANSFORM_VAR: TextTransformFn = TextTransformFn::None;
}

impl TextTransformMix<()> {
    /// Insert context variables used by properties in this mix-in.
    pub fn context_vars_set(set: &mut ContextValueSet) {
        set.insert_context_var(&WHITE_SPACE_VAR);
        set.insert_context_var(&TEXT_TRANSFORM_VAR);
    }
}

/// Text white space transform.
///
/// Can be used to collapse a sequence of spaces into a single one, or to ignore line-breaks.
/// Is [`WhiteSpace::Preserve`] by default.
///
/// This property is not applied when the text is [`txt_editable`].
///
/// Sets the [`WHITE_SPACE_VAR`].
///
/// [`txt_editable`]: fn@txt_editable
#[property(CONTEXT, default(WHITE_SPACE_VAR), widget_impl(TextTransformMix<P>))]
pub fn white_space(child: impl UiNode, transform: impl IntoVar<WhiteSpace>) -> impl UiNode {
    with_context_var(child, WHITE_SPACE_VAR, transform)
}

/// Text transform, character replacement applied to the text before it is processed by the text widget.
///
/// This property is not applied when the text is [`txt_editable`].
///
/// Sets the [`TEXT_TRANSFORM_VAR`].
///  
/// [`txt_editable`]: fn@txt_editable
#[property(CONTEXT, default(TEXT_TRANSFORM_VAR), widget_impl(TextTransformMix<P>))]
pub fn txt_transform(child: impl UiNode, transform: impl IntoVar<TextTransformFn>) -> impl UiNode {
    with_context_var(child, TEXT_TRANSFORM_VAR, transform)
}

/// Language and text direction properties.
///
/// All properties in this mixin affects [`Text!`] nodes inside the widget where they are set.
///
/// [`Text!`]: struct@crate::widgets::Text
#[widget_mixin]
pub struct LangMix<P>(P);

/// Sets the text language and script for the widget and descendants.
///
/// This property affects all texts inside the widget and the layout direction.
///
/// Sets the [`LANG_VAR`] and [`DIRECTION_VAR`] context vars and the [`LayoutMetrics::direction`].
/// Also sets the [`access::lang`] when accessibility is enabled.
///
/// [`access::lang`]: fn@access::lang
#[property(CONTEXT, default(LANG_VAR), widget_impl(LangMix<P>))]
pub fn lang(child: impl UiNode, lang: impl IntoVar<Langs>) -> impl UiNode {
    let lang = lang.into_var();
    let child = direction(child, lang.map(|l| l.best().character_direction().into()));
    let child = access::lang(child, lang.map(|l| l.best().clone()));
    with_context_var(child, LANG_VAR, lang)
}

/// Sets the layout direction used in the layout of the widget and descendants.
///
/// Note that the [`lang`] property already sets the direction, this property can be used to directly override the direction.
///
/// Sets the [`DIRECTION_VAR`] context var and the [`LayoutMetrics::direction`].
///
/// [`lang`]: fn@lang
#[property(CONTEXT+1, default(DIRECTION_VAR), widget_impl(LangMix<P>))]
pub fn direction(child: impl UiNode, direction: impl IntoVar<LayoutDirection>) -> impl UiNode {
    let child = match_node(child, |child, op| match op {
        UiNodeOp::Init => {
            WIDGET.sub_var_layout(&DIRECTION_VAR);
        }
        UiNodeOp::Measure { wm, desired_size } => {
            *desired_size = LAYOUT.with_direction(DIRECTION_VAR.get(), || child.measure(wm));
        }
        UiNodeOp::Layout { wl, final_size } => *final_size = LAYOUT.with_direction(DIRECTION_VAR.get(), || child.layout(wl)),
        _ => {}
    });
    with_context_var(child, DIRECTION_VAR, direction)
}

impl LangMix<()> {
    /// Insert context variables used by properties in this mix-in.
    pub fn context_vars_set(set: &mut ContextValueSet) {
        set.insert_context_var(&LANG_VAR);
        set.insert_context_var(&DIRECTION_VAR);
    }
}

/// Advanced font config, features, kerning, variations and more.
///
/// All properties in this mixin affects [`Text!`] nodes inside the widget where they are set.
///
/// [`Text!`]: struct@crate::widgets::Text
#[widget_mixin]
pub struct FontFeaturesMix<P>(P);

context_var! {
    /// Font features of [`Text!`] spans.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static FONT_FEATURES_VAR: FontFeatures = FontFeatures::new();

    /// Font variations of [`Text!`] spans.
    ///
    /// [`Text!`]: struct@crate::widgets::Text
    pub static FONT_VARIATIONS_VAR: FontVariations = FontVariations::new();
}

impl FontFeaturesMix<()> {
    /// Insert context variables used by properties in this mix-in.
    pub fn context_vars_set(set: &mut ContextValueSet) {
        set.insert_context_var(&FONT_FEATURES_VAR);
        set.insert_context_var(&FONT_VARIATIONS_VAR);
    }
}

/// Includes the font variation config in the widget context.
///
/// The variation `name` is set for the [`FONT_VARIATIONS_VAR`] in this context, variations already set in the parent
/// context that are not the same `name` are also included.
pub fn with_font_variation(child: impl UiNode, name: FontVariationName, value: impl IntoVar<f32>) -> impl UiNode {
    with_context_var(
        child,
        FONT_VARIATIONS_VAR,
        merge_var!(FONT_VARIATIONS_VAR, value.into_var(), move |variations, value| {
            let mut variations = variations.clone();
            variations.insert(name, *value);
            variations
        }),
    )
}

/// Include the font feature config in the widget context.
///
/// The modifications done in `set_feature` are visible only in the [`FONT_FEATURES_VAR`] in this context, and features
/// already set in a parent context are included.
pub fn with_font_feature<C, S, V, D>(child: C, state: V, set_feature: D) -> impl UiNode
where
    C: UiNode,
    S: VarValue,
    V: IntoVar<S>,
    D: FnMut(&mut FontFeatures, S) -> S + Send + 'static,
{
    let mut set_feature = set_feature;
    with_context_var(
        child,
        FONT_FEATURES_VAR,
        merge_var!(FONT_FEATURES_VAR, state.into_var(), move |features, state| {
            let mut features = features.clone();
            set_feature(&mut features, state.clone());
            features
        }),
    )
}

/// Sets font variations.
///
/// **Note:** This property fully replaces the font variations for the widget and descendants, use [`with_font_variation`]
/// to create a property that sets a variation but retains others from the context.
#[property(CONTEXT, default(FONT_VARIATIONS_VAR), widget_impl(FontFeaturesMix<P>))]
pub fn font_variations(child: impl UiNode, variations: impl IntoVar<FontVariations>) -> impl UiNode {
    with_context_var(child, FONT_VARIATIONS_VAR, variations)
}

/// Sets font features.
///
/// **Note:** This property fully replaces the font variations for the widget and descendants, use [`with_font_variation`]
/// to create a property that sets a variation but retains others from the context.
#[property(CONTEXT, default(FONT_FEATURES_VAR), widget_impl(FontFeaturesMix<P>))]
pub fn font_features(child: impl UiNode, features: impl IntoVar<FontFeatures>) -> impl UiNode {
    with_context_var(child, FONT_FEATURES_VAR, features)
}

/// Sets the font kerning feature.
#[property(CONTEXT, default(FontFeatureState::auto()), widget_impl(FontFeaturesMix<P>))]
pub fn font_kerning(child: impl UiNode, state: impl IntoVar<FontFeatureState>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.kerning().set(s))
}

/// Sets the font common ligatures features.
#[property(CONTEXT, default(FontFeatureState::auto()), widget_impl(FontFeaturesMix<P>))]
pub fn font_common_lig(child: impl UiNode, state: impl IntoVar<FontFeatureState>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.common_lig().set(s))
}

/// Sets the font discretionary ligatures feature.
#[property(CONTEXT, default(FontFeatureState::auto()), widget_impl(FontFeaturesMix<P>))]
pub fn font_discretionary_lig(child: impl UiNode, state: impl IntoVar<FontFeatureState>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.discretionary_lig().set(s))
}

/// Sets the font historical ligatures feature.
#[property(CONTEXT, default(FontFeatureState::auto()), widget_impl(FontFeaturesMix<P>))]
pub fn font_historical_lig(child: impl UiNode, state: impl IntoVar<FontFeatureState>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.historical_lig().set(s))
}

/// Sets the font contextual alternatives feature.
#[property(CONTEXT, default(FontFeatureState::auto()), widget_impl(FontFeaturesMix<P>))]
pub fn font_contextual_alt(child: impl UiNode, state: impl IntoVar<FontFeatureState>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.contextual_alt().set(s))
}

/// Sets the font capital variant features.
#[property(CONTEXT, default(CapsVariant::Auto), widget_impl(FontFeaturesMix<P>))]
pub fn font_caps(child: impl UiNode, state: impl IntoVar<CapsVariant>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.caps().set(s))
}

/// Sets the font numeric variant features.
#[property(CONTEXT, default(NumVariant::Auto), widget_impl(FontFeaturesMix<P>))]
pub fn font_numeric(child: impl UiNode, state: impl IntoVar<NumVariant>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.numeric().set(s))
}

/// Sets the font numeric spacing features.
#[property(CONTEXT, default(NumSpacing::Auto), widget_impl(FontFeaturesMix<P>))]
pub fn font_num_spacing(child: impl UiNode, state: impl IntoVar<NumSpacing>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.num_spacing().set(s))
}

/// Sets the font numeric fraction features.
#[property(CONTEXT, default(NumFraction::Auto), widget_impl(FontFeaturesMix<P>))]
pub fn font_num_fraction(child: impl UiNode, state: impl IntoVar<NumFraction>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.num_fraction().set(s))
}

/// Sets the font swash features.
#[property(CONTEXT, default(FontFeatureState::auto()), widget_impl(FontFeaturesMix<P>))]
pub fn font_swash(child: impl UiNode, state: impl IntoVar<FontFeatureState>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.swash().set(s))
}

/// Sets the font stylistic alternative feature.
#[property(CONTEXT, default(FontFeatureState::auto()), widget_impl(FontFeaturesMix<P>))]
pub fn font_stylistic(child: impl UiNode, state: impl IntoVar<FontFeatureState>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.stylistic().set(s))
}

/// Sets the font historical forms alternative feature.
#[property(CONTEXT, default(FontFeatureState::auto()), widget_impl(FontFeaturesMix<P>))]
pub fn font_historical_forms(child: impl UiNode, state: impl IntoVar<FontFeatureState>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.historical_forms().set(s))
}

/// Sets the font ornaments alternative feature.
#[property(CONTEXT, default(FontFeatureState::auto()), widget_impl(FontFeaturesMix<P>))]
pub fn font_ornaments(child: impl UiNode, state: impl IntoVar<FontFeatureState>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.ornaments().set(s))
}

/// Sets the font annotation alternative feature.
#[property(CONTEXT, default(FontFeatureState::auto()), widget_impl(FontFeaturesMix<P>))]
pub fn font_annotation(child: impl UiNode, state: impl IntoVar<FontFeatureState>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.annotation().set(s))
}

/// Sets the font stylistic set alternative feature.
#[property(CONTEXT, default(FontStyleSet::auto()), widget_impl(FontFeaturesMix<P>))]
pub fn font_style_set(child: impl UiNode, state: impl IntoVar<FontStyleSet>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.style_set().set(s))
}

/// Sets the font character variant alternative feature.
#[property(CONTEXT, default(CharVariant::auto()), widget_impl(FontFeaturesMix<P>))]
pub fn font_char_variant(child: impl UiNode, state: impl IntoVar<CharVariant>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.char_variant().set(s))
}

/// Sets the font sub/super script position alternative feature.
#[property(CONTEXT, default(FontPosition::Auto), widget_impl(FontFeaturesMix<P>))]
pub fn font_position(child: impl UiNode, state: impl IntoVar<FontPosition>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.position().set(s))
}

/// Sets the Japanese logographic set.
#[property(CONTEXT, default(JpVariant::Auto), widget_impl(FontFeaturesMix<P>))]
pub fn font_jp_variant(child: impl UiNode, state: impl IntoVar<JpVariant>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.jp_variant().set(s))
}

/// Sets the Chinese logographic set.
#[property(CONTEXT, default(CnVariant::Auto), widget_impl(FontFeaturesMix<P>))]
pub fn font_cn_variant(child: impl UiNode, state: impl IntoVar<CnVariant>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.cn_variant().set(s))
}

/// Sets the East Asian figure width.
#[property(CONTEXT, default(EastAsianWidth::Auto), widget_impl(FontFeaturesMix<P>))]
pub fn font_ea_width(child: impl UiNode, state: impl IntoVar<EastAsianWidth>) -> impl UiNode {
    with_font_feature(child, state, |f, s| f.ea_width().set(s))
}

/// Text edit properties.
///
/// All properties in this mixin affects [`Text!`] nodes inside the widget where they are set.
///
/// [`Text!`]: struct@crate::widgets::Text
#[widget_mixin]
pub struct TextEditMix<P>(P);

context_var! {
    /// Text is editable.
    pub static TEXT_EDITABLE_VAR: bool = false;

    /// Text is selectable.
    pub static TEXT_SELECTABLE_VAR: bool = false;

    /// Accepts `'\t'` input when editable.
    pub static ACCEPTS_TAB_VAR: bool = false;

    /// Accepts `'\n'` input when editable.
    pub static ACCEPTS_ENTER_VAR: bool = false;

    /// Caret color, inherits from [`FONT_COLOR_VAR`].
    pub static CARET_COLOR_VAR: Rgba = FONT_COLOR_VAR;

    /// Touch caret shape.
    pub static CARET_TOUCH_SHAPE_VAR: WidgetFn<CaretShape> = wgt_fn!(|s| super::nodes::default_touch_caret(s));

    /// Selection background color.
    pub static SELECTION_COLOR_VAR: Rgba = colors::AZURE.with_alpha(30.pct());

    /// If text parse updated for every text change.
    pub static TXT_PARSE_LIVE_VAR: bool = true;

    /// Debounce time for change stop.
    pub static CHANGE_STOP_DELAY_VAR: Duration = 1.secs();

    /// Auto selection on keyboard focus.
    pub static AUTO_SELECTION_VAR: AutoSelection = AutoSelection::default();

    /// Maximum number of characters that can be input.
    ///
    /// Zero means no limit. Is zero by default.
    pub static MAX_CHARS_COUNT_VAR: usize = 0;

    /// Replacement character used when obscuring text.
    pub static OBSCURING_CHAR_VAR: char = '•';

    /// If text characters are replaced with [`OBSCURING_CHAR_VAR`] for rendering.
    pub static OBSCURE_TXT_VAR: bool = false;

    pub(super) static TXT_PARSE_PENDING_VAR: bool = false;
}

impl TextEditMix<()> {
    /// Insert context variables used by properties in this mix-in.
    pub fn context_vars_set(set: &mut ContextValueSet) {
        set.insert_context_var(&TEXT_EDITABLE_VAR);
        set.insert_context_var(&TEXT_SELECTABLE_VAR);
        set.insert_context_var(&ACCEPTS_ENTER_VAR);
        set.insert_context_var(&CARET_COLOR_VAR);
        set.insert_context_var(&CARET_TOUCH_SHAPE_VAR);
        set.insert_context_var(&SELECTION_COLOR_VAR);
        set.insert_context_var(&TXT_PARSE_LIVE_VAR);
        set.insert_context_var(&CHANGE_STOP_DELAY_VAR);
        set.insert_context_var(&AUTO_SELECTION_VAR);
        set.insert_context_var(&MAX_CHARS_COUNT_VAR);
        set.insert_context_var(&OBSCURING_CHAR_VAR);
        set.insert_context_var(&OBSCURE_TXT_VAR);
    }
}

/// Defines the position of a caret in relation to the selection.
///
/// See [`caret_touch_shape`](fn@caret_touch_shape) for more details.
#[derive(Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CaretShape {
    /// Caret defines the selection start in LTR and end in RTL text.
    SelectionLeft,
    /// Caret defines the selection end in LTR and start in RTL text.
    SelectionRight,
    /// Caret defines the insert point, when there is no selection.
    Insert,
}

/// Enable text caret, input and makes the widget focusable.
///
/// If the `txt` variable is read-only, this is ignored, if the var is writeable this
/// enables text input and modifies the variable.
///
/// Sets the [`TEXT_EDITABLE_VAR`].
#[property(CONTEXT, default(TEXT_EDITABLE_VAR), widget_impl(TextEditMix<P>))]
pub fn txt_editable(child: impl UiNode, enabled: impl IntoVar<bool>) -> impl UiNode {
    with_context_var(child, TEXT_EDITABLE_VAR, enabled)
}

/// Enable text selection, copy and makes the widget focusable.
///
/// Sets the [`TEXT_SELECTABLE_VAR`].
#[property(CONTEXT, default(TEXT_SELECTABLE_VAR), widget_impl(TextEditMix<P>))]
pub fn txt_selectable(child: impl UiNode, enabled: impl IntoVar<bool>) -> impl UiNode {
    with_context_var(child, TEXT_SELECTABLE_VAR, enabled)
}

/// If the `'\t'` character is inserted when tab is pressed and the text is editable.
///
/// If not enabled or the text is not editable, then pressing tab moves the focus like normal.
///
/// Sets the [`ACCEPTS_TAB_VAR`].
#[property(CONTEXT, default(ACCEPTS_TAB_VAR), widget_impl(TextEditMix<P>))]
pub fn accepts_tab(child: impl UiNode, enabled: impl IntoVar<bool>) -> impl UiNode {
    with_context_var(child, ACCEPTS_TAB_VAR, enabled)
}

/// If the `'\n'` character is inserted when enter is pressed and the text is editable.
///
/// Sets the [`ACCEPTS_ENTER_VAR`].
#[property(CONTEXT, default(ACCEPTS_ENTER_VAR), widget_impl(TextEditMix<P>))]
pub fn accepts_enter(child: impl UiNode, enabled: impl IntoVar<bool>) -> impl UiNode {
    with_context_var(child, ACCEPTS_ENTER_VAR, enabled)
}

/// Sets the [`CARET_COLOR_VAR`].
#[property(CONTEXT, default(CARET_COLOR_VAR), widget_impl(TextEditMix<P>))]
pub fn caret_color(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode {
    with_context_var(child, CARET_COLOR_VAR, color)
}

/// Defines custom caret shapes for touch caret.
///
/// The `shape` node becomes the content of a [layered widget] at the `ADORNER+1` layer, the text widget context is
/// propagated so contextual variables and value work seamless inside the node.
///
/// The `shape` node must set one special value during layout, the [`set_touch_caret_mid`] must be called to
/// set the offset to the middle of the caret line in the shape inner-bounds, this is used to position the caret.
///
/// [layered widget]: crate::widgets::window::layers
/// [`set_touch_caret_mid`]: super::nodes::set_touch_caret_mid
#[property(CONTEXT, default(CARET_TOUCH_SHAPE_VAR), widget_impl(TextEditMix<P>))]
pub fn caret_touch_shape(child: impl UiNode, shape: impl IntoVar<WidgetFn<CaretShape>>) -> impl UiNode {
    with_context_var(child, CARET_TOUCH_SHAPE_VAR, shape)
}

/// Sets the [`SELECTION_COLOR_VAR`].
#[property(CONTEXT, default(SELECTION_COLOR_VAR), widget_impl(TextEditMix<P>))]
pub fn selection_color(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode {
    with_context_var(child, SELECTION_COLOR_VAR, color)
}

/// Gets the caret char index, if the text is editable.
#[property(EVENT, default(None), widget_impl(TextEditMix<P>))]
pub fn get_caret_index(child: impl UiNode, index: impl IntoVar<Option<CaretIndex>>) -> impl UiNode {
    super::nodes::get_caret_index(child, index)
}

/// Gets the caret display status, if the text is editable.
#[property(EVENT, default(CaretStatus::none()), widget_impl(TextEditMix<P>))]
pub fn get_caret_status(child: impl UiNode, status: impl IntoVar<CaretStatus>) -> impl UiNode {
    super::nodes::get_caret_status(child, status)
}

/// Gets the number of lines in the text, including wrap lines.
///
/// This is very cheap, the text widget already has the length, but it does include wrapped lines. You
/// can use [`get_lines_wrap_count`] to get text lines and a count of wrapped lines for each.
///
/// [`get_lines_wrap_count`]: fn@get_lines_wrap_count
#[property(CHILD_LAYOUT+100, default(0), widget_impl(TextEditMix<P>))]
pub fn get_lines_len(child: impl UiNode, len: impl IntoVar<usize>) -> impl UiNode {
    super::nodes::get_lines_len(child, len)
}

/// Gets the number of wrap lines per text lines.
#[property(CHILD_LAYOUT+100, default(LinesWrapCount::NoWrap(0)), widget_impl(TextEditMix<P>))]
pub fn get_lines_wrap_count(child: impl UiNode, lines: impl IntoVar<LinesWrapCount>) -> impl UiNode {
    super::nodes::get_lines_wrap_count(child, lines)
}

/// Gets the number of character in the text.
#[property(EVENT, default(0), widget_impl(TextEditMix<P>))]
pub fn get_chars_count(child: impl UiNode, chars: impl IntoVar<usize>) -> impl UiNode {
    let chars = chars.into_var();
    match_node(child, move |_, op| {
        if let UiNodeOp::Init = op {
            let ctx = super::nodes::ResolvedText::get();
            let _ = chars.set_from_map(&ctx.txt, |t| t.chars().count());
            let handle = ctx.txt.bind_map(&chars, |t| t.chars().count());
            WIDGET.push_var_handle(handle);
        }
    })
}

/// If [`txt_parse`] tries to parse after any text change immediately.
///
/// This is enabled by default, if disabled the [`PARSE_CMD`] can be used to update pending parse.
///
/// This property sets the [`TXT_PARSE_LIVE_VAR`].
///
/// [`txt_parse`]: fn@super::txt_parse
/// [`PARSE_CMD`]: super::commands::PARSE_CMD
#[property(CONTEXT, default(TXT_PARSE_LIVE_VAR), widget_impl(TextEditMix<P>))]
pub fn txt_parse_live(child: impl UiNode, enabled: impl IntoVar<bool>) -> impl UiNode {
    with_context_var(child, TXT_PARSE_LIVE_VAR, enabled)
}

/// Shorthand property, disables live parsing and parse on change stop.
///
/// This property sets [`txt_parse_live`] and [`on_change_stop`] on the widget.
///
/// [`txt_parse_live`]: fn@txt_parse_live
/// [`on_change_stop`]: fn@on_change_stop
#[property(EVENT, widget_impl(TextEditMix<P>))]
pub fn txt_parse_on_stop(child: impl UiNode, enabled: impl IntoVar<bool>) -> impl UiNode {
    let enabled = enabled.into_var();
    let child = txt_parse_live(child, enabled.map(|&b| !b));
    on_change_stop(
        child,
        hn!(|_| {
            if enabled.get() {
                super::commands::PARSE_CMD.scoped(WIDGET.id()).notify();
            }
        }),
    )
}

/// Maximum number of characters that can be input.
///
/// Zero means no limit. Is zero by default.
///
/// This property sets the [`MAX_CHARS_COUNT_VAR`].
#[property(CONTEXT, default(MAX_CHARS_COUNT_VAR), widget_impl(TextEditMix<P>))]
pub fn max_chars_count(child: impl UiNode, max: impl IntoVar<usize>) -> impl UiNode {
    with_context_var(child, MAX_CHARS_COUNT_VAR, max)
}

/// If text has changed but [`txt_parse`] has not tried to parse the new text yet.
///
/// This can only be `true` if [`txt_parse_live`] is `false`.
///
/// [`txt_parse`]: fn@super::txt_parse
/// [`txt_parse_live`]: fn@txt_parse_live
#[property(CONTEXT, default(false), widget_impl(TextEditMix<P>))]
pub fn is_parse_pending(child: impl UiNode, state: impl IntoVar<bool>) -> impl UiNode {
    // reverse context, `txt_parse` sets `TXT_PARSE_PENDING_VAR`
    with_context_var(child, TXT_PARSE_PENDING_VAR, state)
}

/// Called after the text changed and interaction has stopped.
///
/// The `handler` will be called after change and [`change_stop_delay`] elapses, or the widget loses focus,
/// or the [`Key::Enter`] is pressed and [`accepts_enter`] is `false`.
///
/// [`change_stop_delay`]: fn@change_stop_delay
/// [`accepts_enter`]: fn@accepts_enter
/// [`Key::Enter`]: crate::core::keyboard::Key::Enter
#[property(EVENT, widget_impl(TextEditMix<P>))]
pub fn on_change_stop(child: impl UiNode, handler: impl WidgetHandler<ChangeStopArgs>) -> impl UiNode {
    super::nodes::on_change_stop(child, handler)
}

/// Debounce time for [`on_change_stop`].
///
/// After the text stops changing and `delay` is elapsed the change stop handled is called, even
/// if the widget is still focused.
///
/// Is `1.secs()` by default.
///
/// Sets [`CHANGE_STOP_DELAY_VAR`].
///
/// [`on_change_stop`]: fn@on_change_stop
#[property(CONTEXT, default(CHANGE_STOP_DELAY_VAR), widget_impl(TextEditMix<P>))]
pub fn change_stop_delay(child: impl UiNode, delay: impl IntoVar<Duration>) -> impl UiNode {
    with_context_var(child, CHANGE_STOP_DELAY_VAR, delay)
}

/// Auto-selection on focus when the text is editable.
///
/// If enabled on keyboard focus all text is selected and on blur any selection is cleared.
#[property(CONTEXT, default(AUTO_SELECTION_VAR), widget_impl(TextEditMix<P>))]
pub fn auto_selection(child: impl UiNode, mode: impl IntoVar<AutoSelection>) -> impl UiNode {
    with_context_var(child, AUTO_SELECTION_VAR, mode)
}

/// Replacement character used when obscuring text.
///
/// When [`obscure_txt`] is enabled the text characters are replaced by this one.
///
/// [`obscure_txt`]: fn@obscure_txt
#[property(CONTEXT, default(OBSCURING_CHAR_VAR), widget_impl(TextEditMix<P>))]
pub fn obscuring_char(child: impl UiNode, character: impl IntoVar<char>) -> impl UiNode {
    with_context_var(child, OBSCURING_CHAR_VAR, character)
}

/// If the typed text is obscured in render.
///
/// When enabled each text character is replaced with [`obscuring_char`], cut, copy and undo commands are disabled.
///
/// Note that the text variable is still **plain text** in memory, a memory dump while the widget is filled can leak
/// the password, this is a potential security problem shared by apps that accept typed passwords. To mitigate the problem
/// don't use automatic crash reports with memory dump, drop the widget and the text variable as soon as possible,
/// design the app to show the password widget last to minimize its lifetime.
///
/// [`obscuring_char`]: fn@obscuring_char
#[property(CONTEXT, default(OBSCURE_TXT_VAR), widget_impl(TextEditMix<P>))]
pub fn obscure_txt(child: impl UiNode, enabled: impl IntoVar<bool>) -> impl UiNode {
    with_context_var(child, OBSCURE_TXT_VAR, enabled)
}

/// Defines when text is auto-selected on focus.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum AutoSelection {
    /// Does not select-all on focus.
    Disabled,

    /// Select-all on keyboard focus ([highlight]).
    ///
    ///  [highlight]: crate::core::focus::FocusChangedArgs::highlight
    Enabled,

    /// Select-all on keyboard focus ([highlight]) if the text is not [`accepts_enter`].
    ///
    /// [highlight]: crate::core::focus::FocusChangedArgs::highlight
    /// [`accepts_enter`]: fn@accepts_enter
    #[default]
    Auto,
}

/// Arguments for [`on_change_stop`].
///
/// [`on_change_stop`]: fn@on_change_stop
#[derive(Debug, Clone)]
pub struct ChangeStopArgs {
    /// Event cause.
    pub cause: ChangeStopCause,
}
impl ChangeStopArgs {
    /// Resolved text is available in the handler.
    pub fn txt(&self) -> std::sync::Arc<super::nodes::ResolvedText> {
        super::nodes::ResolvedText::get()
    }
}

/// Cause of an [`on_change_stop`].
///
/// [`on_change_stop`]: fn@on_change_stop
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeStopCause {
    /// The [`change_stop_delay`] elapsed.
    ///
    /// [`change_stop_delay`]: fn@change_stop_delay
    DelayElapsed,
    /// The [`Key::Enter`] was pressed and [`accepts_enter`] is `false`.
    ///
    /// [`Key::Enter`]: crate::core::keyboard::Key::Enter
    /// [`accepts_enter`]: fn@accepts_enter
    Enter,
    /// The widget lost keyboard focus.
    Blur,
}

/// Display info of edit caret position.
#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone, serde::Serialize, serde::Deserialize)]
pub struct CaretStatus {
    index: usize,
    line: u32,
    column: u32,
}
impl CaretStatus {
    /// Status for text without caret.
    pub fn none() -> Self {
        Self {
            index: usize::MAX,
            line: 0,
            column: 0,
        }
    }

    /// New position from char index and text.
    ///
    /// # Panics
    ///
    /// Panics if `index` is greater then `text` length.
    pub fn new(index: usize, text: &SegmentedText) -> Self {
        assert!(index <= text.text().len());

        if text.text().is_empty() {
            Self { line: 1, column: 1, index }
        } else {
            let mut line = 1;
            let mut line_start = 0;
            for seg in text.segs() {
                if seg.end > index {
                    break;
                }
                if let TextSegmentKind::LineBreak = seg.kind {
                    line += 1;
                    line_start = seg.end;
                }
            }

            let column = text.text()[line_start..index].chars().count() + 1;

            Self {
                line,
                column: column as _,
                index,
            }
        }
    }

    /// Char index on the text string, starts a 0, can be the length of the text.
    pub fn index(&self) -> Option<usize> {
        match self.index {
            usize::MAX => None,
            i => Some(i),
        }
    }

    /// Display line, starts at 1.
    ///
    /// Note that this does not count soft line breaks (wrapped lines), this is the actual text line.
    pub fn line(&self) -> Option<NonZeroU32> {
        NonZeroU32::new(self.line)
    }

    /// Display column, starts at 1.
    ///
    /// This is the char count from the start of the text line to the index.
    pub fn column(&self) -> Option<NonZeroU32> {
        NonZeroU32::new(self.column)
    }
}
impl fmt::Display for CaretStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.index().is_some() {
            write!(f, "Ln {}, Col {}", self.line, self.column)
        } else {
            Ok(())
        }
    }
}

/// Represents the number of lines and number of wrap lines in a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinesWrapCount {
    /// No line wrap.
    ///
    /// The associated value is the number of lines.
    NoWrap(usize),
    /// Some text lines have more than one wrap line.
    ///
    /// The associated value is a vec of wrap-line count for each text line, is `1` for lines that don't wrap.
    Wrap(Vec<u32>),
}
impl LinesWrapCount {
    /// Gets the number of text lines.
    pub fn lines_len(&self) -> usize {
        match self {
            Self::NoWrap(l) => *l,
            Self::Wrap(lns) => lns.len(),
        }
    }
}

/// Text paragraph properties.
///
/// Note that the [`Text!`] widget does not include this mixin, as raw text does not encode
/// paragraph breaks, other rich text widgets can include it to configure paragraphs.
///
/// [`Text!`]: struct@crate::widgets::Text
#[widget_mixin]
pub struct ParagraphMix<P>(P);

context_var! {
    /// Extra paragraph spacing of text blocks.
    pub static PARAGRAPH_SPACING_VAR: ParagraphSpacing = 1.em();
}

impl ParagraphMix<()> {
    /// Insert context variables used by properties in this mix-in.
    pub fn context_vars_set(set: &mut ContextValueSet) {
        set.insert_context_var(&PARAGRAPH_SPACING_VAR);
    }
}

/// Extra spacing in-between paragraphs.
///
/// The default value is `1.em()`. Note that the [`Text!`] widget does not implement this property, as raw text does not encode
/// paragraph breaks, this property and context var exists to configure *rich-text* widgets, like the [`markdown!`] widget.
///
/// Sets the [`PARAGRAPH_SPACING_VAR`].
///
/// [`Text!`]: struct@crate::widgets::Text
/// [`markdown!`]: struct@crate::widgets::Markdown
#[property(CONTEXT, default(PARAGRAPH_SPACING_VAR), widget_impl(ParagraphMix<P>))]
pub fn paragraph_spacing(child: impl UiNode, extra: impl IntoVar<ParagraphSpacing>) -> impl UiNode {
    with_context_var(child, PARAGRAPH_SPACING_VAR, extra)
}

/// Highlight a text range.
#[property(CHILD_LAYOUT+100)]
pub fn txt_highlight(child: impl UiNode, range: impl IntoVar<std::ops::Range<CaretIndex>>, color: impl IntoVar<Rgba>) -> impl UiNode {
    let range = range.into_var();
    let color = color.into_var();
    let color_key = FrameValueKey::new_unique();
    match_node(child, move |_, op| match op {
        UiNodeOp::Init => {
            WIDGET.sub_var_render(&range).sub_var_render_update(&color);
        }
        UiNodeOp::Render { frame } => {
            let l_txt = super::nodes::LayoutText::get();
            let r_txt = super::nodes::ResolvedText::get();
            let r_txt = r_txt.segmented_text.text();

            for line_rect in l_txt.shaped_text.highlight_rects(range.get(), r_txt) {
                frame.push_color(line_rect, color_key.bind_var(&color, |c| (*c).into()));
            }
        }
        UiNodeOp::RenderUpdate { update } => {
            if let Some(color_update) = color_key.update_var(&color, |c| (*c).into()) {
                update.update_color(color_update)
            }
        }
        _ => {}
    })
}

#[property(CHILD_LAYOUT+100)]
pub fn selection_toolbar(child: impl UiNode, menu: impl UiNode) -> impl UiNode {
    selection_toolbar_fn(child, WidgetFn::singleton(menu))
}

#[property(CHILD_LAYOUT+100, default(WidgetFn::nil()))]
pub fn selection_toolbar_fn(child: impl UiNode, menu: impl IntoVar<WidgetFn<()>>) -> impl UiNode {
    todo!();
    child
}

#[property(CONTEXT, default(SELECTION_TOOLBAR_ANCHOR_VAR))]
pub fn selection_toolbar_anchor(child: impl UiNode, mode: impl IntoVar<AnchorMode>) -> impl UiNode {
    with_context_var(child, SELECTION_TOOLBAR_ANCHOR_VAR, mode)
}

context_var! {
    pub static SELECTION_TOOLBAR_ANCHOR_VAR: AnchorMode = AnchorOffset::out_top();
}
