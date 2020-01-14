use crate::core2::*;
use crate::property;
use zero_ui_macros::impl_ui_node_crate;

pub fn rgb<C: Into<ColorFComponent>>(r: C, g: C, b: C) -> ColorF {
    rgba(r, g, b, 1.0)
}

pub fn rgba<C: Into<ColorFComponent>, A: Into<ColorFComponent>>(r: C, g: C, b: C, a: A) -> ColorF {
    ColorF::new(r.into().0, g.into().0, b.into().0, a.into().0)
}

/// `ColorF` component value.
pub struct ColorFComponent(pub f32);

impl From<f32> for ColorFComponent {
    fn from(f: f32) -> Self {
        ColorFComponent(f)
    }
}

impl From<u8> for ColorFComponent {
    fn from(u: u8) -> Self {
        ColorFComponent(f32::from(u) / 255.)
    }
}

impl IntoVar<Vec<GradientStop>> for Vec<(f32, ColorF)> {
    type Var = OwnedVar<Vec<GradientStop>>;

    fn into_var(self) -> Self::Var {
        OwnedVar(
            self.into_iter()
                .map(|(offset, color)| GradientStop { offset, color })
                .collect(),
        )
    }
}

impl IntoVar<Vec<GradientStop>> for Vec<ColorF> {
    type Var = OwnedVar<Vec<GradientStop>>;

    fn into_var(self) -> Self::Var {
        let point = 1. / (self.len() as f32 - 1.);
        OwnedVar(
            self.into_iter()
                .enumerate()
                .map(|(i, color)| GradientStop {
                    offset: (i as f32) * point,
                    color,
                })
                .collect(),
        )
    }
}

struct FillColor<C: Var<ColorF>> {
    color: C,
    render_color: ColorF,
}

#[impl_ui_node_crate]
impl<C: Var<ColorF>> UiNode for FillColor<C> {
    fn init(&mut self, ctx: &mut AppContext) {
        self.render_color = *self.color.get(ctx);
    }
    fn update(&mut self, ctx: &mut AppContext) {
        if let Some(color) = self.color.update(ctx) {
            self.render_color = *color;
            ctx.push_frame();
        }
    }
    fn render(&self, frame: &mut FrameBuilder) {
        profile_scope!("render_color");
        frame.push_fill_color(&LayoutRect::from_size(frame.final_size()), self.render_color);
    }
}

pub fn fill_color<C: IntoVar<ColorF>>(color: C) -> impl UiNode {
    FillColor {
        color: color.into_var(),
        render_color: ColorF::BLACK,
    }
}

struct FillGradient<A: Var<LayoutPoint>, B: Var<LayoutPoint>, S: Var<Vec<GradientStop>>> {
    start: A,
    end: B,
    stops: S,
    render_start: LayoutPoint,
    render_end: LayoutPoint,
    render_stops: Vec<GradientStop>,
    final_size: LayoutSize,
}

#[impl_ui_node_crate]
impl<A: Var<LayoutPoint>, B: Var<LayoutPoint>, S: Var<Vec<GradientStop>>> UiNode for FillGradient<A, B, S> {
    fn init(&mut self, ctx: &mut AppContext) {
        self.render_start = *self.start.get(ctx);
        self.render_end = *self.end.get(ctx);
        self.render_stops = self.stops.get(ctx).clone();
    }

    fn update(&mut self, ctx: &mut AppContext) {
        if let Some(start) = self.start.update(ctx) {
            self.render_start = *start;
            self.render_start.x *= self.final_size.width;
            self.render_start.y *= self.final_size.height;
            ctx.push_frame();
        }
        if let Some(end) = self.end.update(ctx) {
            self.render_end = *end;
            self.render_end.x *= self.final_size.width;
            self.render_end.y *= self.final_size.height;
            ctx.push_frame();
        }
        if let Some(stops) = self.stops.update(ctx) {
            self.render_stops = stops.clone();
            ctx.push_frame();
        }
    }

    fn arrange(&mut self, final_size: LayoutSize) {
        self.render_start.x /= self.final_size.width;
        self.render_start.y /= self.final_size.height;
        self.render_end.x /= self.final_size.width;
        self.render_end.y /= self.final_size.height;

        self.final_size = final_size;

        self.render_start.x *= self.final_size.width;
        self.render_start.y *= self.final_size.height;
        self.render_end.x *= self.final_size.width;
        self.render_end.y *= self.final_size.height;
    }

    fn render(&self, frame: &mut FrameBuilder) {
        profile_scope!("render_gradient");

        frame.push_fill_gradient(
            &LayoutRect::from_size(self.final_size),
            self.render_start,
            self.render_end,
            self.render_stops.clone(),
        );
    }
}

pub fn fill_gradient(
    start: impl IntoVar<LayoutPoint>,
    end: impl IntoVar<LayoutPoint>,
    stops: impl IntoVar<Vec<GradientStop>>,
) -> impl UiNode {
    FillGradient {
        start: start.into_var(),
        end: end.into_var(),
        stops: stops.into_var(),
        render_start: LayoutPoint::zero(),
        render_end: LayoutPoint::zero(),
        render_stops: Vec::default(),
        final_size: LayoutSize::zero(),
    }
}

struct Background<T: UiNode, B: UiNode> {
    child: T,
    background: B,
}

impl<T: UiNode, B: UiNode> UiNode for Background<T, B> {
    fn init(&mut self, ctx: &mut AppContext) {
        self.background.init(ctx);
        self.child.init(ctx);
    }

    fn deinit(&mut self, ctx: &mut AppContext) {
        self.background.deinit(ctx);
        self.child.deinit(ctx);
    }

    fn update(&mut self, ctx: &mut AppContext) {
        self.background.update(ctx);
        self.child.update(ctx);
    }
    fn update_hp(&mut self, ctx: &mut AppContext) {
        self.background.update_hp(ctx);
        self.child.update_hp(ctx);
    }

    fn measure(&mut self, available_size: LayoutSize) -> LayoutSize {
        let available_size = self.child.measure(available_size);
        self.background.measure(available_size);
        available_size
    }

    fn arrange(&mut self, final_size: LayoutSize) {
        self.background.arrange(final_size);
        self.child.arrange(final_size);
    }

    fn render(&self, frame: &mut FrameBuilder) {
        self.background.render(frame);
        self.child.render(frame);
    }
}

#[property(inner)]
pub fn background(child: impl UiNode, background: impl UiNode) -> impl UiNode {
    Background { child, background }
}

#[property(inner)]
pub fn background_color(child: impl UiNode, color: impl IntoVar<ColorF>) -> impl UiNode {
    Background {
        child,
        background: fill_color(color),
    }
}

#[property(inner)]
pub fn background_gradient(
    child: impl UiNode,
    start: impl IntoVar<LayoutPoint>,
    end: impl IntoVar<LayoutPoint>,
    stops: impl IntoVar<Vec<GradientStop>>,
) -> impl UiNode {
    Background {
        child,
        background: fill_gradient(start, end, stops),
    }
}
