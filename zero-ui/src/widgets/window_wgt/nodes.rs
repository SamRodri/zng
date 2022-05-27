//! UI nodes used for building a window widget.

use crate::prelude::new_property::*;

/// Windows layers.
///
/// The window layers is z-order stacking panel that fills the window content area, widgets can be inserted
/// with a *z-index* that is the [`LayerIndex`]. The inserted widgets parent is the window root widget and
/// it is affected by the context properties set on the window only.
///
/// # Layout & Render
///
/// Layered widgets are measured and arranged using the same constrains as the window root widget, the desired
/// size is discarded, only the root widget desired size can affect the window size. Layered widgets are all layout
/// and rendered after the window content and from the bottom layer up to the top-most, this means that the [`WidgetBoundsInfo`]
/// and [`WidgetRenderInfo`] of normal widgets are always up-to-date when the layered widget is arranged and rendered, so if you
/// implement custom layouts that align the layered widget with a normal widget using the info values it will always be in sync with
/// a single layout pass, see [`insert_anchored`] for more details.
///
/// [`WindowContext`]: crate::core::context::WindowContext
/// [`insert_anchored`]: Self::insert_anchored
pub struct WindowLayers {
    items: SortedWidgetVecRef,
}
impl WindowLayers {
    /// Insert the `widget` in the layer identified by a [`LayerIndex`].
    ///
    /// If the `layer` variable updates the widget is moved to the new layer, if multiple widgets
    /// are inserted in the same layer the later inserts are on top of the previous.
    pub fn insert(ctx: &mut WidgetContext, layer: impl IntoVar<LayerIndex>, widget: impl Widget) {
        struct LayeredWidget<L, W> {
            layer: L,
            widget: W,
        }
        #[impl_ui_node(
                delegate = &self.widget,
                delegate_mut = &mut self.widget,
            )]
        impl<L: Var<LayerIndex>, W: Widget> UiNode for LayeredWidget<L, W> {
            fn init(&mut self, ctx: &mut WidgetContext) {
                self.widget.state_mut().set(LayerIndexKey, self.layer.copy(ctx.vars));
                self.widget.init(ctx);
            }

            fn update(&mut self, ctx: &mut WidgetContext) {
                if let Some(index) = self.layer.copy_new(ctx) {
                    self.widget.state_mut().set(LayerIndexKey, index);
                    ctx.window_state.req(WindowLayersKey).items.sort(ctx.updates, ctx.path.widget_id());
                }
                self.widget.update(ctx);
            }
        }
        impl<L: Var<LayerIndex>, W: Widget> Widget for LayeredWidget<L, W> {
            fn id(&self) -> WidgetId {
                self.widget.id()
            }

            fn state(&self) -> &StateMap {
                self.widget.state()
            }

            fn state_mut(&mut self) -> &mut StateMap {
                self.widget.state_mut()
            }

            fn bounds_info(&self) -> &WidgetBoundsInfo {
                self.widget.bounds_info()
            }

            fn border_info(&self) -> &WidgetBorderInfo {
                self.widget.border_info()
            }

            fn render_info(&self) -> &WidgetRenderInfo {
                self.widget.render_info()
            }
        }

        ctx.window_state.req(WindowLayersKey).items.insert(
            ctx.updates,
            LayeredWidget {
                layer: layer.into_var(),
                widget: widget.cfg_boxed_wgt(),
            },
        );
    }

    /// Insert the `widget` in the layer and *anchor* it to the offset/transform of another widget.
    ///
    /// The `anchor` is the ID of another widget, the inserted `widget` will be offset/transform so that it aligns
    /// with the `anchor` widget top-left. The `mode` is a value of [`AnchorMode`] that defines if the `widget` will
    /// receive the full transform or just the offset.
    ///
    /// If the `anchor` widget is not found the `widget` is not rendered (visibility `Collapsed`).
    pub fn insert_anchored(
        ctx: &mut WidgetContext,
        layer: impl IntoVar<LayerIndex>,
        anchor: impl IntoVar<WidgetId>,
        mode: impl IntoVar<AnchorMode>,

        widget: impl Widget,
    ) {
        struct AnchoredWidget<A, M, W> {
            anchor: A,
            mode: M,
            widget: W,

            anchor_info: Option<(WidgetBoundsInfo, WidgetBorderInfo, WidgetRenderInfo)>,
            offset_point: PxPoint,
            interaction: bool,

            spatial_id: SpatialFrameId,
            transform_key: FrameBindingKey<RenderTransform>,
        }
        #[impl_ui_node(
                delegate = &self.widget,
                delegate_mut = &mut self.widget,
            )]
        impl<A, M, W> UiNode for AnchoredWidget<A, M, W>
        where
            A: Var<WidgetId>,
            M: Var<AnchorMode>,
            W: Widget,
        {
            fn subscriptions(&self, ctx: &mut InfoContext, subs: &mut WidgetSubscriptions) {
                subs.event(WidgetInfoChangedEvent);

                self.widget.subscriptions(ctx, subs)
            }

            fn info(&self, ctx: &mut InfoContext, info: &mut WidgetInfoBuilder) {
                if self.interaction {
                    let anchor = self.anchor.copy(ctx);
                    let widget = ctx.path.widget_id();
                    info.push_interaction_filter(move |args| {
                        if args.info.self_and_ancestors().any(|w| w.widget_id() == widget) {
                            args.info.tree().find(anchor).map(|a| a.allow_interaction()).unwrap_or(false)
                        } else {
                            true
                        }
                    });
                }
                self.widget.info(ctx, info)
            }

            fn init(&mut self, ctx: &mut WidgetContext) {
                if let Some(w) = ctx.info_tree.find(self.anchor.copy(ctx.vars)) {
                    self.anchor_info = Some((w.bounds_info(), w.border_info(), w.render_info()));
                }

                self.interaction = self.mode.get(ctx).interaction;

                self.widget.init(ctx);
            }

            fn deinit(&mut self, ctx: &mut WidgetContext) {
                self.anchor_info = None;
                self.widget.deinit(ctx);
            }

            fn event<Args: EventUpdateArgs>(&mut self, ctx: &mut WidgetContext, args: &Args) {
                if let Some(args) = WidgetInfoChangedEvent.update(args) {
                    if args.window_id == ctx.path.window_id() {
                        self.anchor_info = ctx
                            .info_tree
                            .find(self.anchor.copy(ctx.vars))
                            .map(|w| (w.bounds_info(), w.border_info(), w.render_info()));
                    }
                    self.widget.event(ctx, args);
                } else {
                    self.widget.event(ctx, args);
                }
            }

            fn update(&mut self, ctx: &mut WidgetContext) {
                if let Some(anchor) = self.anchor.copy_new(ctx) {
                    self.anchor_info = ctx
                        .info_tree
                        .find(anchor)
                        .map(|w| (w.bounds_info(), w.border_info(), w.render_info()));
                    if self.mode.get(ctx).interaction {
                        ctx.updates.info();
                    }
                    ctx.updates.layout_and_render();
                }
                if let Some(mode) = self.mode.get_new(ctx) {
                    if mode.interaction != self.interaction {
                        self.interaction = mode.interaction;
                        ctx.updates.info();
                    }
                    ctx.updates.layout_and_render();
                }
                self.widget.update(ctx);
            }

            fn layout(&mut self, ctx: &mut LayoutContext, wl: &mut WidgetLayout) -> PxSize {
                if let Some((bounds, border, _)) = &self.anchor_info {
                    let mode = self.mode.get(ctx.vars);

                    if !mode.visibility || bounds.inner_size() != PxSize::zero() {
                        // if we don't link visibility or anchor is not collapsed.

                        self.offset_point = match &mode.transform {
                            AnchorTransform::InnerOffset(p) => ctx.with_constrains(
                                |_| PxConstrains2d::new_exact_size(bounds.inner_size()),
                                |ctx| p.layout(ctx, |_| PxPoint::zero()),
                            ),
                            AnchorTransform::InnerBorderOffset(p) => ctx.with_constrains(
                                |_| PxConstrains2d::new_exact_size(border.inner_size(bounds)),
                                |ctx| p.layout(ctx, |_| PxPoint::zero()),
                            ),
                            AnchorTransform::OuterOffset(p) => ctx.with_constrains(
                                |_| PxConstrains2d::new_exact_size(bounds.outer_size()),
                                |ctx| p.layout(ctx, |_| PxPoint::zero()),
                            ),
                            _ => PxPoint::zero(),
                        };

                        return ctx.with_constrains(
                            |c| match mode.size {
                                AnchorSize::Unbounded => PxConstrains2d::new_unbounded(),
                                AnchorSize::Window => c,
                                AnchorSize::InnerSize => PxConstrains2d::new_exact_size(bounds.inner_size()),
                                AnchorSize::InnerBorder => PxConstrains2d::new_exact_size(border.inner_size(bounds)),
                                AnchorSize::OuterSize => PxConstrains2d::new_exact_size(bounds.outer_size()),
                            },
                            |ctx| {
                                if mode.corner_radius {
                                    let mut cr = border.corner_radius();
                                    if let AnchorSize::InnerBorder = mode.size {
                                        cr = cr.deflate(border.offsets());
                                    }
                                    ctx.vars.with_context_var(CornerRadiusVar, ContextVarData::fixed(&cr.into()), || {
                                        ContextBorders::with_corner_radius(ctx, |ctx| self.widget.layout(ctx, wl))
                                    })
                                } else {
                                    self.widget.layout(ctx, wl)
                                }
                            },
                        );
                    }
                }

                wl.collapse(ctx);
                PxSize::zero()
            }

            fn render(&self, ctx: &mut RenderContext, frame: &mut FrameBuilder) {
                if let Some((_, border_info, render_info)) = &self.anchor_info {
                    let mode = &self.mode.get(ctx.vars);
                    if !self.mode.get(ctx).visibility || render_info.rendered() {
                        match &mode.transform {
                            AnchorTransform::InnerOffset(_) => {
                                let point_in_window = render_info
                                    .inner_transform()
                                    .transform_px_point(self.offset_point)
                                    .unwrap_or_default();

                                frame.push_reference_frame(
                                    self.spatial_id,
                                    self.transform_key
                                        .bind(RenderTransform::translation_px(point_in_window.to_vector())),
                                    true,
                                    |frame| self.widget.render(ctx, frame),
                                )
                            }
                            AnchorTransform::InnerBorderOffset(_) => {
                                let point_in_window = border_info
                                    .inner_transform(render_info)
                                    .transform_px_point(self.offset_point)
                                    .unwrap_or_default();
                                frame.push_reference_frame(
                                    self.spatial_id,
                                    self.transform_key
                                        .bind(RenderTransform::translation_px(point_in_window.to_vector())),
                                    true,
                                    |frame| self.widget.render(ctx, frame),
                                )
                            }
                            AnchorTransform::OuterOffset(_) => {
                                let point_in_window = render_info
                                    .outer_transform()
                                    .transform_px_point(self.offset_point)
                                    .unwrap_or_default();
                                frame.push_reference_frame(
                                    self.spatial_id,
                                    self.transform_key
                                        .bind(RenderTransform::translation_px(point_in_window.to_vector())),
                                    true,
                                    |frame| self.widget.render(ctx, frame),
                                )
                            }
                            AnchorTransform::InnerTransform => frame.push_reference_frame(
                                self.spatial_id,
                                self.transform_key.bind(render_info.inner_transform()),
                                false,
                                |frame| self.widget.render(ctx, frame),
                            ),
                            AnchorTransform::InnerBorderTransform => frame.push_reference_frame(
                                self.spatial_id,
                                self.transform_key.bind(border_info.inner_transform(render_info)),
                                false,
                                |frame| self.widget.render(ctx, frame),
                            ),
                            AnchorTransform::OuterTransform => frame.push_reference_frame(
                                self.spatial_id,
                                self.transform_key.bind(render_info.outer_transform()),
                                false,
                                |frame| self.widget.render(ctx, frame),
                            ),
                            _ => self.widget.render(ctx, frame),
                        }
                        return;
                    }
                }

                frame.skip_render(ctx.info_tree);
            }

            fn render_update(&self, ctx: &mut RenderContext, update: &mut FrameUpdate) {
                if let Some((_, border_info, render_info)) = &self.anchor_info {
                    let mode = &self.mode.get(ctx.vars);
                    if !mode.visibility || render_info.rendered() {
                        match &mode.transform {
                            AnchorTransform::InnerOffset(_) => {
                                let point_in_window = render_info
                                    .inner_transform()
                                    .transform_px_point(self.offset_point)
                                    .unwrap_or_default();
                                update.with_transform(
                                    self.transform_key
                                        .update(RenderTransform::translation_px(point_in_window.to_vector())),
                                    |update| self.widget.render_update(ctx, update),
                                )
                            }
                            AnchorTransform::InnerBorderOffset(_) => {
                                let point_in_window = border_info
                                    .inner_transform(render_info)
                                    .transform_px_point(self.offset_point)
                                    .unwrap_or_default();
                                update.with_transform(
                                    self.transform_key
                                        .update(RenderTransform::translation_px(point_in_window.to_vector())),
                                    |update| self.widget.render_update(ctx, update),
                                )
                            }
                            AnchorTransform::OuterOffset(_) => {
                                let point_in_window = render_info
                                    .outer_transform()
                                    .transform_px_point(self.offset_point)
                                    .unwrap_or_default();
                                update.with_transform(
                                    self.transform_key
                                        .update(RenderTransform::translation_px(point_in_window.to_vector())),
                                    |update| self.widget.render_update(ctx, update),
                                )
                            }
                            AnchorTransform::InnerTransform => {
                                update.with_transform(self.transform_key.update(render_info.inner_transform()), |update| {
                                    self.widget.render_update(ctx, update)
                                });
                            }
                            AnchorTransform::InnerBorderTransform => {
                                update.with_transform(self.transform_key.update(border_info.inner_transform(render_info)), |update| {
                                    self.widget.render_update(ctx, update)
                                });
                            }
                            AnchorTransform::OuterTransform => {
                                update.with_transform(self.transform_key.update(render_info.outer_transform()), |update| {
                                    self.widget.render_update(ctx, update)
                                });
                            }
                            _ => self.widget.render_update(ctx, update),
                        }
                    }
                }
            }
        }
        impl<A: Var<WidgetId>, M: Var<AnchorMode>, W: Widget> Widget for AnchoredWidget<A, M, W> {
            fn id(&self) -> WidgetId {
                self.widget.id()
            }

            fn state(&self) -> &StateMap {
                self.widget.state()
            }

            fn state_mut(&mut self) -> &mut StateMap {
                self.widget.state_mut()
            }

            fn bounds_info(&self) -> &WidgetBoundsInfo {
                self.widget.bounds_info()
            }

            fn border_info(&self) -> &WidgetBorderInfo {
                self.widget.border_info()
            }

            fn render_info(&self) -> &WidgetRenderInfo {
                self.widget.render_info()
            }
        }

        Self::insert(
            ctx,
            layer,
            AnchoredWidget {
                anchor: anchor.into_var(),
                mode: mode.into_var(),
                widget: widget.cfg_boxed_wgt(),

                anchor_info: None,
                offset_point: PxPoint::zero(),
                interaction: false,

                transform_key: FrameBindingKey::new_unique(),
                spatial_id: SpatialFrameId::new_unique(),
            },
        );
    }

    /// Remove the widget from the layers overlay in the next update.
    ///
    /// The `id` must the widget id of a previous inserted widget, nothing happens if the widget is not found.
    pub fn remove(ctx: &mut WidgetContext, id: impl Into<WidgetId>) {
        ctx.window_state.req(WindowLayersKey).items.remove(ctx.updates, id);
    }
}

state_key! {
    struct WindowLayersKey: WindowLayers;
    struct LayerIndexKey: LayerIndex;
}

/// Wrap around the window outer-most event node to create the layers.
///
/// This node is automatically included in the `window::new_event` constructor.
pub fn layers(child: impl UiNode) -> impl UiNode {
    struct LayersNode<C> {
        children: C,
        layered: SortedWidgetVecRef,
    }
    #[impl_ui_node(children)]
    impl<C: UiNodeList> UiNode for LayersNode<C> {
        fn init(&mut self, ctx: &mut WidgetContext) {
            ctx.window_state.set(
                WindowLayersKey,
                WindowLayers {
                    items: self.layered.clone(),
                },
            );

            self.children.init_all(ctx);
        }

        fn update(&mut self, ctx: &mut WidgetContext) {
            let mut changed = false;

            self.children.update_all(ctx, &mut changed);

            if changed {
                ctx.updates.layout_and_render();
            }
        }

        fn layout(&mut self, ctx: &mut LayoutContext, wl: &mut WidgetLayout) -> PxSize {
            let mut size = PxSize::zero();
            self.children.layout_all(
                ctx,
                wl,
                |_, _, _| {},
                |_, _, args| {
                    if args.index == 0 {
                        size = args.size;
                    }
                },
            );
            size
        }
    }

    let layers_vec = SortedWidgetVec::new(|a, b| {
        let a = a.state().req(LayerIndexKey);
        let b = b.state().req(LayerIndexKey);

        a.cmp(b)
    });
    let layered = layers_vec.reference();

    LayersNode {
        children: nodes![child].chain_nodes(layers_vec),
        layered,
    }
    .cfg_boxed()
}

/// Represents a layer in a window.
///
/// See the [`WindowLayers`] struct for more information.
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord)]
pub struct LayerIndex(pub u32);
impl LayerIndex {
    /// The top-most layer.
    ///
    /// Only widgets that are pretending to be a child window should use this layer, including menus,
    /// drop-downs, pop-ups and tool-tips.
    ///
    /// This is the [`u32::MAX`] value.
    pub const TOP_MOST: LayerIndex = LayerIndex(u32::MAX);

    /// The layer for *adorner* display items.
    ///
    /// Adorner widgets are related to another widget but not as a visual part of it, examples of adorners
    /// are resize handles in a widget visual editor, or an interactive help/guide feature.
    ///
    /// This is the [`TOP_MOST - u16::MAX`] value.
    pub const ADORNER: LayerIndex = LayerIndex(Self::TOP_MOST.0 - u16::MAX as u32);

    /// The default layer, just above the normal window content.
    ///
    /// This is the `0` value.
    pub const DEFAULT: LayerIndex = LayerIndex(0);

    /// Compute `self + other` saturating at the [`TOP_MOST`] bound instead of overflowing.
    ///
    /// [`TOP_MOST`]: Self::TOP_MOST
    pub fn saturating_add(self, other: impl Into<LayerIndex>) -> Self {
        Self(self.0.saturating_add(other.into().0))
    }

    /// Compute `self - other` saturating at the [`DEFAULT`] bound instead of overflowing.
    ///
    /// [`DEFAULT`]: Self::DEFAULT
    pub fn saturating_sub(self, other: impl Into<LayerIndex>) -> Self {
        Self(self.0.saturating_sub(other.into().0))
    }
}
impl_from_and_into_var! {
    fn from(index: u32) -> LayerIndex {
        LayerIndex(index)
    }
}
/// Calls [`LayerIndex::saturating_add`].
impl<T: Into<Self>> std::ops::Add<T> for LayerIndex {
    type Output = Self;

    fn add(self, rhs: T) -> Self::Output {
        self.saturating_add(rhs)
    }
}
/// Calls [`LayerIndex::saturating_sub`].
impl<T: Into<Self>> std::ops::Sub<T> for LayerIndex {
    type Output = Self;

    fn sub(self, rhs: T) -> Self::Output {
        self.saturating_sub(rhs)
    }
}
/// Calls [`LayerIndex::saturating_add`].
impl<T: Into<Self>> std::ops::AddAssign<T> for LayerIndex {
    fn add_assign(&mut self, rhs: T) {
        *self = *self + rhs;
    }
}
/// Calls [`LayerIndex::saturating_sub`].
impl<T: Into<Self>> std::ops::SubAssign<T> for LayerIndex {
    fn sub_assign(&mut self, rhs: T) {
        *self = *self - rhs;
    }
}

/// Options for [`AnchorMode::transform`].
#[derive(Debug, Clone, PartialEq)]
pub enum AnchorTransform {
    /// Widget does not copy any position from the anchor widget.
    None,
    /// The point is resolved in the inner space of the anchor widget, transformed to the window space
    /// and then applied as a translate offset.
    InnerOffset(Point),
    /// The point is resolved in the inner space of the anchor widget offset by the anchor border widths, transformed
    /// to the window space and then applied as a translate offset.
    InnerBorderOffset(Point),

    /// The point is resolved in the outer space of the anchor widget, transformed to the window space
    /// and then applied as a translate offset.
    OuterOffset(Point),

    /// The full inner transform of the anchor object is applied to the widget.
    InnerTransform,

    /// The full inner transform of the anchor object is applied to the widget plus the border widths offset.
    InnerBorderTransform,

    /// The full outer transform of the anchor object is applied to the widget.
    OuterTransform,
}
impl_from_and_into_var! {
    /// `InnerOffset`.
    fn from(inner_offset: Point) -> AnchorTransform {
        AnchorTransform::InnerOffset(inner_offset)
    }
    /// `InnerOffset`.
    fn from<X: Into<Length> + Clone, Y: Into<Length> + Clone>(inner_offset: (X, Y)) -> AnchorTransform {
        Point::from(inner_offset).into()
    }
    /// `InnerOffset`.
    fn from(inner_offset: PxPoint) -> AnchorTransform {
        Point::from(inner_offset).into()
    }
    /// `InnerOffset`.
    fn from(inner_offset: DipPoint) -> AnchorTransform {
        Point::from(inner_offset).into()
    }
}

/// Options for [`AnchorMode::size`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorSize {
    /// Widget does not copy any size from the anchor widget, the available size is infinite, the
    /// final size is the desired size.
    ///
    /// Note that layered widgets do not affect the window size and a widget that overflows the content
    /// boundaries is clipped.
    Unbounded,
    /// Widget does not copy any size from the anchor widget, the available size and final size
    /// are the window's root size.
    Window,
    /// The available size and final size is the anchor widget's outer size.
    OuterSize,
    /// The available size and final size is the anchor widget's inner size.
    InnerSize,
    /// The available size and final size is the anchor widget's inner size offset by the border widths.
    InnerBorder,
}

/// Defines what properties the layered widget takes from the anchor widget.
#[derive(Debug, Clone, PartialEq)]
pub struct AnchorMode {
    /// What transforms are copied from the anchor widget and applied as a *parent* transform of the widget.
    pub transform: AnchorTransform,
    /// What size is copied from the anchor widget and used as the available size and final size of the widget.
    pub size: AnchorSize,
    /// If the widget is only layout if the anchor widget is not [`Collapsed`] and is only rendered
    /// if the anchor widget is rendered.
    ///
    /// [`Collapsed`]: Visibility::Collapsed
    pub visibility: bool,
    /// The widget only allows interaction if the anchor widget [`allow_interaction`].
    ///
    /// [`allow_interaction`]: crate::core::widget_info::WidgetInfo::allow_interaction
    pub interaction: bool,

    /// The widget's corner radius is set for the layer.
    ///
    /// If `size` is [`InnerBorder`] the corner radius are deflated to fit the *inner* curve of the borders.
    ///
    /// [`InnerBorder`]: AnchorSize::InnerBorder
    pub corner_radius: bool,
}
impl AnchorMode {
    /// Mode where widget behaves like an unanchored widget, except that it is still only
    /// layout an rendered if the anchor widget exists in the same window.
    pub fn none() -> Self {
        AnchorMode {
            transform: AnchorTransform::None,
            size: AnchorSize::Window,
            visibility: false,
            interaction: false,
            corner_radius: false,
        }
    }
}
impl Default for AnchorMode {
    /// Transform `InnerOffset` top-left, size infinite, copy visibility and corner-radius.
    fn default() -> Self {
        AnchorMode {
            transform: AnchorTransform::InnerOffset(Point::top_left()),
            size: AnchorSize::Unbounded,
            visibility: true,
            interaction: false,
            corner_radius: true,
        }
    }
}
impl_from_and_into_var! {
    /// Custom transform, all else default.
    fn from(transform: AnchorTransform) -> AnchorMode {
        AnchorMode {
            transform,
            ..AnchorMode::default()
        }
    }
    /// Transform `InnerOffset`, all else default.
    fn from(inner_offset: Point) -> AnchorMode {
        AnchorTransform::from(inner_offset).into()
    }
    /// Transform `InnerOffset`, all else default.
    fn from(inner_offset: PxPoint) -> AnchorMode {
        AnchorTransform::from(inner_offset).into()
    }
    /// Transform `InnerOffset`, all else default.
    fn from(inner_offset: DipPoint) -> AnchorMode {
        AnchorTransform::from(inner_offset).into()
    }

    /// Custom transform and size, all else default.
    fn from<T: Into<AnchorTransform> + Clone, S: Into<AnchorSize> + Clone>((transform, size): (T, S)) -> AnchorMode {
        AnchorMode {
            transform: transform.into(),
            size: size.into(),
            ..AnchorMode::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn layer_index_ops() {
        let idx = LayerIndex::DEFAULT;

        let p1 = idx + 1;
        let m1 = idx - 1;

        let mut idx = idx;

        idx += 1;
        assert_eq!(idx, p1);

        idx -= 2;
        assert_eq!(idx, m1);
    }
}
