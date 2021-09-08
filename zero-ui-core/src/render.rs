//! Frame render and metadata API.

use crate::{
    app::view_process::ViewRenderer,
    border::{BorderSides, LayoutBorderRadius},
    color::{RenderColor, RenderFilter},
    context::{OwnedStateMap, RenderContext, StateMap},
    crate_util::IdMap,
    gradient::{RenderExtendMode, RenderGradientStop},
    task,
    units::*,
    var::impl_from_and_into_var,
    window::{CursorIcon, WindowId},
    UiNode, WidgetId,
};
use derive_more as dm;
use ego_tree::Tree;
use std::{fmt, io::Write, marker::PhantomData, mem, sync::Arc, time::Instant};
use webrender_api::*;

#[doc(no_inline)]
pub use webrender_api;

macro_rules! debug_assert_aligned {
    ($value:expr, $grid: expr) => {
        #[cfg(debug_assertions)]
        {
            let value = $value;
            let grid = $grid;
            if !value.is_aligned_to(grid) {
                log::error!(
                    target: "render",
                    "{}: `{:?}` is not aligned, expected `{:?}`",
                    stringify!($value),
                    value,
                    value.snap_to(grid)
                );
            }
        }
    };
}

/// Id of a rendered or rendering window frame. Not unique across windows.
pub type FrameId = webrender_api::Epoch;

/// A text font.
///
/// This trait is an interface for the renderer into the font API used in the application.
///
/// # Font API
///
/// The default font API is provided by [`FontManager`] that is included
/// in the app default extensions. The default font type is [`Font`] that implements this trait.
///
/// [`FontManager`]: crate::text::FontManager
/// [`Font`]: crate::text::Font
pub trait Font {
    /// Gets the instance key in the `renderer` namespace.
    ///
    /// The font configuration must be provided by `self`, except the `synthesis` that is used in the font instance.
    fn instance_key(&self, renderer: &ViewRenderer, synthesis: FontSynthesis) -> webrender_api::FontInstanceKey;
}

/// A loaded or loading image.
///
/// This trait is an interface for the renderer into the image API used in the application.
///
/// The ideal image format is BGRA with pre-multiplied alpha.
///
/// # Image API
///
/// The default image API is provided by [`ImageManager`] that is included
/// in the app default extensions. The default image type is [`Image`] that implements this trait.
///
/// [`ImageManager`]: crate::image::ImageManager
/// [`Image`]: crate::image::Image
pub trait Image {
    /// Gets the image key in the `renderer` namespace.
    ///
    /// The image must be loaded asynchronously by `self` and does not need to
    /// be loaded yet when the key is returned.
    fn image_key(&self, renderer: &ViewRenderer) -> webrender_api::ImageKey;

    /// Returns a value that indicates if the image is already pre-multiplied.
    ///
    /// The faster option is pre-multiplied, that is also the default return value.
    fn alpha_type(&self) -> webrender_api::AlphaType {
        webrender_api::AlphaType::PremultipliedAlpha
    }
}

/// Image scaling algorithm in the renderer.
///
/// If an image is not rendered at the same size as in their source it must be up-scaled or
/// down-scaled. The algorithm used for this scaling can be selected using this `enum`.
///
/// Note that the algorithms used in the renderer value performance over quality and do a good
/// enough job for small or temporary changes in scale only, such as a small size correction or a scaling animation.
/// If and image is constantly rendered at a different scale you should considered scaling it on the CPU using a
/// slower but more complex algorithm or pre-scaling it before including in the app.
///
/// You can use the [`Image`] type to re-scale an image.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ImageRendering {
    /// Let the renderer select the algorithm, currently this is the same as [`CrispEdges`].
    ///
    /// [`CrispEdges`]: ImageRendering::CrispEdges
    Auto = 0,
    /// The image is scaled with an algorithm that preserves contrast and edges in the image,
    /// and which does not smooth colors or introduce blur to the image in the process.
    ///
    /// Currently the [Bilinear] interpolation algorithm is used.
    ///
    /// [Bilinear]: https://en.wikipedia.org/wiki/Bilinear_interpolation
    CrispEdges = 1,
    /// When scaling the image up, the image appears to be composed of large pixels.
    ///
    /// Currently the [Nearest-neighbor] interpolation algorithm is used.
    ///
    /// [Nearest-neighbor]: https://en.wikipedia.org/wiki/Nearest-neighbor_interpolation
    Pixelated = 2,
}
impl From<ImageRendering> for webrender_api::ImageRendering {
    fn from(r: ImageRendering) -> Self {
        use webrender_api::ImageRendering::*;
        match r {
            ImageRendering::Auto => Auto,
            ImageRendering::CrispEdges => CrispEdges,
            ImageRendering::Pixelated => Pixelated,
        }
    }
}

/// A full frame builder.
pub struct FrameBuilder {
    renderer: Option<ViewRenderer>,

    scale_factor: f32,
    display_list: DisplayListBuilder,

    info: FrameInfoBuilder,
    info_id: WidgetInfoId,

    widget_id: WidgetId,
    widget_transform_key: WidgetTransformKey,
    widget_stack_ctx_data: Option<(LayoutTransform, Vec<FilterOp>)>,
    cancel_widget: bool,
    widget_display_mode: WidgetDisplayMode,

    meta: OwnedStateMap,
    cursor: CursorIcon,
    hit_testable: bool,

    clip_id: ClipId,
    spatial_id: SpatialId,
    parent_spatial_id: SpatialId,

    offset: LayoutPoint,
}
bitflags! {
    struct WidgetDisplayMode: u8 {
        const REFERENCE_FRAME = 1;
        const STACKING_CONTEXT = 2;
    }
}
impl FrameBuilder {
    /// New builder.
    ///
    /// * `frame_id` - Id of the new frame.
    /// * `window_id` - Id of the window that will render the frame.
    /// * `renderer` - Connection to the renderer connection that will render the frame, is `None` in renderless mode.
    /// * `root_id` - Id of the root widget of the frame, usually the window root.
    /// * `root_transform_key` - Frame binding for the root widget layout transform.
    /// * `root_size` - Layout size of the root widget, defines root hit area and the clear rectangle.
    /// * `scale_factor` - Scale factor that will be used to render the frame, usually the scale factor of the screen the window is at.
    /// because WebRender does not let us change the initial clear color.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub fn new(
        frame_id: FrameId,
        window_id: WindowId,
        renderer: Option<ViewRenderer>,
        root_id: WidgetId,
        root_transform_key: WidgetTransformKey,
        root_size: LayoutSize,
        scale_factor: f32,
    ) -> Self {
        debug_assert_aligned!(root_size, PixelGrid::new(scale_factor));
        let pipeline_id = renderer
            .as_ref()
            .and_then(|r| r.pipeline_id().ok())
            .unwrap_or_else(PipelineId::dummy);
        let info = FrameInfoBuilder::new(window_id, frame_id, root_id, root_size);
        let spatial_id = SpatialId::root_reference_frame(pipeline_id);
        let mut new = FrameBuilder {
            renderer,
            scale_factor,
            display_list: DisplayListBuilder::with_capacity(pipeline_id, root_size, 100),
            info_id: info.root_id(),
            info,
            widget_id: root_id,
            widget_transform_key: root_transform_key,
            widget_stack_ctx_data: None,
            cancel_widget: false,
            widget_display_mode: WidgetDisplayMode::empty(),
            meta: OwnedStateMap::new(),
            cursor: CursorIcon::default(),
            hit_testable: true,
            clip_id: ClipId::root(pipeline_id),
            spatial_id,
            parent_spatial_id: spatial_id,
            offset: LayoutPoint::zero(),
        };
        new.push_widget_hit_area(root_id, root_size);
        new.widget_stack_ctx_data = Some((LayoutTransform::identity(), Vec::default()));
        new
    }

    /// [`new`](Self::new) with only the inputs required for renderless mode.
    pub fn new_renderless(
        frame_id: FrameId,
        window_id: WindowId,
        root_id: WidgetId,
        root_transform_key: WidgetTransformKey,
        root_size: LayoutSize,
        scale_factor: f32,
    ) -> Self {
        Self::new(frame_id, window_id, None, root_id, root_transform_key, root_size, scale_factor)
    }

    /// Pixel scale factor used by the renderer.
    ///
    /// All layout values are scaled by this factor in the renderer.
    #[inline]
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Device pixel grid.
    ///
    /// All layout values must align with this grid.
    #[inline]
    pub fn pixel_grid(&self) -> PixelGrid {
        PixelGrid::new(self.scale_factor)
    }

    /// Direct access to the display list builder.
    ///
    /// # Careful
    ///
    /// This provides direct access to the underlying WebRender display list builder, modifying it
    /// can interfere with the working of the [`FrameBuilder`].
    ///
    /// Call [`open_widget_display`] before modifying the display list.
    ///
    /// Check the [`FrameBuilder`] source code before modifying the display list.
    ///
    /// Don't try to render using the [`FrameBuilder`] methods inside a custom clip or space, the methods will still
    /// use the [`clip_id`] and [`spatial_id`]. Custom items added to the display list
    /// should be self-contained and completely custom.
    ///
    /// If [`is_cancelling_widget`] don't modify the display list and try to
    /// early return pretending the operation worked.
    ///
    /// # WebRender
    ///
    /// The [`webrender`] crate used in the renderer is re-exported in `zero_ui_core::render::webrender`, and the
    /// [`webrender_api`] is re-exported in `webrender::api`.
    ///
    /// [`open_widget_display`]: Self::open_widget_display
    /// [`clip_id`]: Self::clip_id
    /// [`spatial_id`]: Self::spatial_id
    /// [`is_cancelling_widget`]: Self::is_cancelling_widget
    /// [`webrender`]: https://docs.rs/webrender
    /// [`webrender_api`]: https://docs.rs/webrender_api
    #[inline]
    pub fn display_list(&mut self) -> &mut DisplayListBuilder {
        &mut self.display_list
    }

    /// If is building a frame for a headless and renderless window.
    ///
    /// In this mode only the meta and layout information will be used as a *frame*. Methods still
    /// push to the [`display_list`](Self::display_list) when possible, custom methods should ignore this
    /// unless they need access to the [`renderer`](Self::renderer).
    #[inline]
    pub fn is_renderless(&self) -> bool {
        self.renderer.is_none()
    }

    /// Connection to the renderer that will render this frame.
    ///
    /// Returns `None` when in [renderless](Self::is_renderless) mode.
    #[inline]
    pub fn renderer(&self) -> Option<&ViewRenderer> {
        self.renderer.as_ref()
    }

    /// Window that owns the frame.
    #[inline]
    pub fn window_id(&self) -> WindowId {
        self.info.window_id
    }

    /// Current widget.
    #[inline]
    pub fn widget_id(&self) -> WidgetId {
        self.widget_id
    }

    /// Current widget metadata.
    #[inline]
    pub fn meta(&mut self) -> &mut StateMap {
        &mut self.meta.0
    }

    /// Current cursor.
    #[inline]
    pub fn cursor(&self) -> CursorIcon {
        self.cursor
    }

    /// Current clipping node.
    #[inline]
    pub fn clip_id(&self) -> ClipId {
        self.clip_id
    }

    /// Current spatial node.
    #[inline]
    pub fn spatial_id(&self) -> SpatialId {
        self.spatial_id
    }

    /// Current widget [`ItemTag`]. The first number is the raw [`widget_id`](FrameBuilder::widget_id),
    /// the second number is the raw [`cursor`](FrameBuilder::cursor).
    ///
    /// For more details on how the ItemTag is used see [`FrameHitInfo::new`](FrameHitInfo::new).
    #[inline]
    pub fn item_tag(&self) -> ItemTag {
        (self.widget_id.get(), self.cursor as u16)
    }

    /// If the context is hit-testable.
    #[inline]
    pub fn hit_testable(&self) -> bool {
        self.hit_testable
    }

    /// Common item properties given a `clip_rect` and the current context.
    ///
    /// This is a common case helper, the `clip_rect` is not snapped to pixels.
    #[inline]
    pub fn common_item_properties(&self, clip_rect: LayoutRect) -> CommonItemProperties {
        CommonItemProperties {
            clip_rect,
            hit_info: if self.hit_testable { Some(self.item_tag()) } else { None },
            clip_id: self.clip_id,
            spatial_id: self.spatial_id,
            flags: PrimitiveFlags::empty(),
        }
    }

    /// The hit-test bounding-box used to take the coordinates of the widget hit
    /// if the widget id is hit in another ItemTag that is not WIDGET_HIT_AREA.
    ///
    /// This is done so we have consistent hit coordinates with precise hit area.
    fn push_widget_hit_area(&mut self, id: WidgetId, area: LayoutSize) {
        self.open_widget_display();

        self.display_list.push_hit_test(&CommonItemProperties {
            hit_info: Some((id.get(), WIDGET_HIT_AREA)),
            clip_rect: LayoutRect::from_size(area),
            clip_id: self.clip_id,
            spatial_id: self.spatial_id,
            flags: PrimitiveFlags::empty(),
        });
    }

    /// Includes a widget transform and continues the render build.
    ///
    /// This is `Ok(_)` only when a widget started, but [`open_widget_display`](Self::open_widget_display) was not called.
    ///
    /// In case of error the `child` render is still called just without the transform.
    #[inline]
    pub fn with_widget_transform(
        &mut self,
        transform: &LayoutTransform,
        child: &impl UiNode,
        ctx: &mut RenderContext,
    ) -> Result<(), WidgetStartedError> {
        if self.cancel_widget {
            return Ok(());
        }
        if let Some((t, _)) = self.widget_stack_ctx_data.as_mut() {
            // we don't use post_transform here fore the same reason `Self::open_widget_display`
            // reverses filters, there is a detailed comment there.
            *t = t.pre_transform(transform);
            child.render(ctx, self);
            Ok(())
        } else {
            child.render(ctx, self);
            Err(WidgetStartedError)
        }
    }

    /// Includes a widget filter and continues the render build.
    ///
    /// This is `Ok(_)` only when a widget started, but [`open_widget_display`](Self::open_widget_display) was not called.
    ///
    /// In case of error the `child` render is still called just without the filter.
    #[inline]
    pub fn with_widget_filter(
        &mut self,
        filter: RenderFilter,
        child: &impl UiNode,
        ctx: &mut RenderContext,
    ) -> Result<(), WidgetStartedError> {
        if self.cancel_widget {
            return Ok(());
        }
        if let Some((_, f)) = self.widget_stack_ctx_data.as_mut() {
            f.extend(filter.into_iter().rev()); // see `Self::open_widget_display` for why it is reversed.
            child.render(ctx, self);
            Ok(())
        } else {
            child.render(ctx, self);
            Err(WidgetStartedError)
        }
    }

    /// Includes a widget opacity filter and continues the render build.
    ///
    /// This is `Ok(_)` only when a widget started, but [`open_widget_display`](Self::open_widget_display) was not called.
    ///
    /// In case of error the `child` render is still called just without the filter.
    #[inline]
    pub fn with_widget_opacity(
        &mut self,
        bind: FrameBinding<f32>,
        child: &impl UiNode,
        ctx: &mut RenderContext,
    ) -> Result<(), WidgetStartedError> {
        if self.cancel_widget {
            return Ok(());
        }
        if let Some((_, f)) = self.widget_stack_ctx_data.as_mut() {
            let value = match &bind {
                PropertyBinding::Value(v) => *v,
                PropertyBinding::Binding(_, v) => *v,
            };
            f.push(FilterOp::Opacity(bind, value));
            child.render(ctx, self);
            Ok(())
        } else {
            child.render(ctx, self);
            Err(WidgetStartedError)
        }
    }

    /// Finish widget transform and filters by starting the widget reference frame and stacking context.
    #[inline]
    pub fn open_widget_display(&mut self) {
        if self.cancel_widget {
            return;
        }
        if let Some((transform, mut filters)) = self.widget_stack_ctx_data.take() {
            if transform != LayoutTransform::identity() {
                self.widget_display_mode |= WidgetDisplayMode::REFERENCE_FRAME;

                self.parent_spatial_id = self.spatial_id;
                self.spatial_id = self.display_list.push_reference_frame(
                    LayoutPoint::zero(),
                    self.spatial_id,
                    TransformStyle::Flat,
                    self.widget_transform_key.bind(transform),
                    ReferenceFrameKind::Transform,
                );
            }

            if !filters.is_empty() {
                // we want to apply filters in the top-to-bottom, left-to-right order they appear in
                // the widget declaration, but the widget declaration expands to have the top property
                // node be inside the bottom property node, so the bottom property ends up inserting
                // a filter first, because we cannot insert filters after the child node render is called
                // so we need to reverse the filters here. Left-to-right sequences are reversed on insert
                // so they get reversed again here and everything ends up in order.
                filters.reverse();

                self.widget_display_mode |= WidgetDisplayMode::STACKING_CONTEXT;

                self.display_list.push_simple_stacking_context_with_filters(
                    LayoutPoint::zero(),
                    self.spatial_id,
                    PrimitiveFlags::empty(),
                    &filters,
                    &[],
                    &[],
                )
            }
        } // else already started widget display
    }

    fn close_widget_display(&mut self) {
        if self.widget_display_mode.contains(WidgetDisplayMode::STACKING_CONTEXT) {
            self.display_list.pop_stacking_context();
        }
        if self.widget_display_mode.contains(WidgetDisplayMode::REFERENCE_FRAME) {
            self.display_list.pop_reference_frame();
            self.spatial_id = self.parent_spatial_id;
        }
        self.widget_display_mode = WidgetDisplayMode::empty();
    }

    /// Cancel the current [`push_widget`](Self::push_widget) if we are
    /// still in before [`open_widget_display`](Self::open_widget_display).
    pub fn cancel_widget(&mut self) -> Result<(), WidgetStartedError> {
        if self.widget_stack_ctx_data.is_some() || self.cancel_widget {
            self.widget_stack_ctx_data = None;
            self.cancel_widget = true;
            Ok(())
        } else {
            Err(WidgetStartedError)
        }
    }

    /// Gets if [`cancel_widget`](Self::cancel_widget) was requested.
    ///
    /// When this is `true` all other methods just pretend to work until the [`push_widget`](Self::push_widget) ends.
    #[inline]
    pub fn is_cancelling_widget(&self) -> bool {
        self.cancel_widget
    }

    /// Calls [`render`](UiNode::render) for `child` inside a new widget context.
    pub fn push_widget(
        &mut self,
        id: WidgetId,
        transform_key: WidgetTransformKey,
        area: LayoutSize,
        child: &impl UiNode,
        ctx: &mut RenderContext,
    ) {
        if self.cancel_widget {
            return;
        }

        // NOTE: root widget is not processed by this method, if you add widget behavior here
        // similar behavior must be added in the `new` and `finalize` methods.

        debug_assert_aligned!(area, self.pixel_grid());

        self.push_widget_hit_area(id, area); // self.open_widget_display() happens here.

        self.widget_stack_ctx_data = Some((LayoutTransform::identity(), Vec::default()));

        let parent_id = mem::replace(&mut self.widget_id, id);
        let parent_transform_key = mem::replace(&mut self.widget_transform_key, transform_key);
        let parent_display_mode = mem::replace(&mut self.widget_display_mode, WidgetDisplayMode::empty());

        let parent_meta = mem::take(&mut self.meta);

        let mut bounds = LayoutRect::from_size(area);
        bounds.origin = self.offset;

        let node = self.info.push(self.info_id, id, bounds);
        let parent_node = mem::replace(&mut self.info_id, node);

        child.render(ctx, self);

        if self.cancel_widget {
            self.cancel_widget = false;
            self.info.cancel(node);
            self.meta = parent_meta;
        } else {
            self.close_widget_display();
            self.info.set_meta(node, mem::replace(&mut self.meta, parent_meta));
        }

        self.widget_id = parent_id;
        self.widget_transform_key = parent_transform_key;
        self.widget_display_mode = parent_display_mode;
        self.info_id = parent_node;
    }

    /// Push a hit-test `rect` using [`common_item_properties`](FrameBuilder::common_item_properties)
    /// if [`hit_testable`](FrameBuilder::hit_testable) is `true`.
    #[inline]
    pub fn push_hit_test(&mut self, rect: LayoutRect) {
        if self.cancel_widget {
            return;
        }

        debug_assert_aligned!(rect, self.pixel_grid());

        if self.hit_testable {
            self.open_widget_display();
            self.display_list.push_hit_test(&self.common_item_properties(rect));
        }
    }

    /// Calls `f` while [`hit_testable`](FrameBuilder::hit_testable) is set to `false`.
    #[inline]
    pub fn push_not_hit_testable(&mut self, f: impl FnOnce(&mut FrameBuilder)) {
        if self.cancel_widget {
            return;
        }

        let parent_hit_testable = mem::replace(&mut self.hit_testable, false);
        f(self);
        self.hit_testable = parent_hit_testable;
    }

    /// Calls `f` with a new [`clip_id`](FrameBuilder::clip_id) that clips to `bounds`.
    #[inline]
    pub fn push_simple_clip(&mut self, bounds: LayoutSize, f: impl FnOnce(&mut FrameBuilder)) {
        if self.cancel_widget {
            return;
        }

        debug_assert_aligned!(bounds, self.pixel_grid());

        self.open_widget_display();

        let parent_clip_id = self.clip_id;

        self.clip_id = self.display_list.define_clip(
            &SpaceAndClipInfo {
                spatial_id: self.spatial_id,
                clip_id: self.clip_id,
            },
            LayoutRect::from_size(bounds),
            None,
            None,
        );

        f(self);

        self.clip_id = parent_clip_id;
    }

    // TODO use the widget transform instead of calling this method.
    /// Calls `f` inside a new reference frame at `origin`.
    #[inline]
    pub fn push_reference_frame(&mut self, origin: LayoutPoint, f: impl FnOnce(&mut FrameBuilder)) {
        if self.cancel_widget {
            return;
        }

        if origin == LayoutPoint::zero() {
            return f(self);
        }

        debug_assert_aligned!(origin, self.pixel_grid());

        self.open_widget_display();

        let parent_spatial_id = self.spatial_id;
        self.spatial_id = self.display_list.push_reference_frame(
            origin,
            parent_spatial_id,
            TransformStyle::Flat,
            PropertyBinding::default(),
            ReferenceFrameKind::Transform,
        );

        let offset = origin.to_vector();
        self.offset += offset;

        f(self);

        self.display_list.pop_reference_frame();
        self.spatial_id = parent_spatial_id;
        self.offset -= offset;
    }

    /// Calls `f` inside a new reference frame transformed by `transform`.
    #[inline]
    pub fn push_transform(&mut self, transform: FrameBinding<LayoutTransform>, f: impl FnOnce(&mut FrameBuilder)) {
        if self.cancel_widget {
            return;
        }

        self.open_widget_display();

        let parent_spatial_id = self.spatial_id;
        self.spatial_id = self.display_list.push_reference_frame(
            LayoutPoint::zero(),
            parent_spatial_id,
            TransformStyle::Flat,
            transform,
            ReferenceFrameKind::Transform,
        );

        f(self);

        self.display_list.pop_reference_frame();
        self.spatial_id = parent_spatial_id;
    }

    /// Push a border using [`common_item_properties`].
    ///
    /// [`common_item_properties`]: FrameBuilder::common_item_properties
    #[inline]
    pub fn push_border(&mut self, bounds: LayoutRect, widths: LayoutSideOffsets, sides: BorderSides, radius: LayoutBorderRadius) {
        if self.cancel_widget {
            return;
        }

        debug_assert_aligned!(bounds, self.pixel_grid());
        debug_assert_aligned!(widths, self.pixel_grid());

        self.open_widget_display();

        let details = BorderDetails::Normal(NormalBorder {
            left: sides.left.into(),
            right: sides.right.into(),
            top: sides.top.into(),
            bottom: sides.bottom.into(),
            radius,
            do_aa: true,
        });

        self.display_list
            .push_border(&self.common_item_properties(bounds), bounds, widths, details);
    }

    /// Push a text run using [`common_item_properties`].
    ///
    /// [`common_item_properties`]: FrameBuilder::common_item_properties
    #[inline]
    pub fn push_text(&mut self, rect: LayoutRect, glyphs: &[GlyphInstance], font: &impl Font, color: ColorF, synthesis: FontSynthesis) {
        if self.cancel_widget {
            return;
        }

        debug_assert_aligned!(rect, self.pixel_grid());

        self.open_widget_display();

        if let Some(r) = &self.renderer {
            let instance_key = font.instance_key(r, synthesis);

            self.display_list
                .push_text(&self.common_item_properties(rect), rect, glyphs, instance_key, color, None);
        }
    }

    /// Push an image using [`common_item_properties`].
    ///
    /// [`common_item_properties`]: FrameBuilder::common_item_properties
    pub fn push_image(&mut self, rect: LayoutRect, image: &impl Image, rendering: ImageRendering) {
        if self.cancel_widget {
            return;
        }

        self.open_widget_display();

        if let Some(r) = &self.renderer {
            let image_key = image.image_key(r);
            self.display_list.push_image(
                &self.common_item_properties(rect),
                rect,
                rendering.into(),
                image.alpha_type(),
                image_key,
                RenderColor::WHITE,
            )
        }
    }

    /// Calls `f` while [`item_tag`](FrameBuilder::item_tag) indicates the `cursor`.
    ///
    /// Note that for the cursor to be used `node` or its children must push a hit-testable item.
    #[inline]
    pub fn push_cursor(&mut self, cursor: CursorIcon, f: impl FnOnce(&mut FrameBuilder)) {
        if self.cancel_widget {
            return;
        }

        let parent_cursor = std::mem::replace(&mut self.cursor, cursor);
        f(self);
        self.cursor = parent_cursor;
    }

    /// Push a color rectangle using [`common_item_properties`](FrameBuilder::common_item_properties).
    #[inline]
    pub fn push_color(&mut self, rect: LayoutRect, color: RenderColor) {
        if self.cancel_widget {
            return;
        }

        debug_assert_aligned!(rect, self.pixel_grid());

        self.open_widget_display();

        self.display_list.push_rect(&self.common_item_properties(rect), color);
    }

    /// Push a repeating linear gradient rectangle using [`common_item_properties`](FrameBuilder::common_item_properties).
    ///
    /// The gradient fills the `tile_size`, the tile is repeated to fill the `rect`.
    /// The `extend_mode` controls how the gradient fills the tile.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn push_linear_gradient(
        &mut self,
        rect: LayoutRect,
        line: LayoutLine,
        stops: &[RenderGradientStop],
        extend_mode: RenderExtendMode,
        tile_size: LayoutSize,
        tile_spacing: LayoutSize,
    ) {
        if self.cancel_widget {
            return;
        }

        debug_assert_aligned!(rect, self.pixel_grid());
        debug_assert_aligned!(tile_size, self.pixel_grid());
        debug_assert_aligned!(tile_spacing, self.pixel_grid());
        debug_assert!(stops.len() >= 2);
        debug_assert!(stops[0].offset.abs() < 0.00001, "first color stop must be at offset 0.0");
        debug_assert!(
            (stops[stops.len() - 1].offset - 1.0).abs() < 0.00001,
            "last color stop must be at offset 1.0"
        );

        self.open_widget_display();

        self.display_list.push_stops(stops);

        let gradient = Gradient {
            start_point: line.start,
            end_point: line.end,
            extend_mode,
        };

        self.display_list
            .push_gradient(&self.common_item_properties(rect), rect, gradient, tile_size, tile_spacing);
    }

    /// Push a styled vertical or horizontal line.
    #[inline]
    pub fn push_line(
        &mut self,
        bounds: LayoutRect,
        orientation: crate::border::LineOrientation,
        color: RenderColor,
        style: crate::border::LineStyle,
    ) {
        if self.cancel_widget {
            return;
        }

        debug_assert_aligned!(bounds, self.pixel_grid());

        self.open_widget_display();

        match style.render_command() {
            RenderLineCommand::Line(style, thickness) => self.display_list.push_line(
                &self.common_item_properties(bounds),
                &bounds,
                thickness,
                orientation.into(),
                &color,
                style,
            ),
            RenderLineCommand::Border(style) => {
                use crate::border::LineOrientation as LO;
                let widths = match orientation {
                    LO::Vertical => LayoutSideOffsets::new(0.0, 0.0, 0.0, bounds.size.width),
                    LO::Horizontal => LayoutSideOffsets::new(bounds.size.height, 0.0, 0.0, 0.0),
                };
                let details = BorderDetails::Normal(NormalBorder {
                    left: BorderSide { color, style },
                    right: BorderSide {
                        color: RenderColor::TRANSPARENT,
                        style: BorderStyle::Hidden,
                    },
                    top: BorderSide { color, style },
                    bottom: BorderSide {
                        color: RenderColor::TRANSPARENT,
                        style: BorderStyle::Hidden,
                    },
                    radius: BorderRadius::uniform(0.0),
                    do_aa: false,
                });

                self.display_list
                    .push_border(&self.common_item_properties(bounds), bounds, widths, details);
            }
        }
    }

    /// Push a `color` dot to mark the `offset`.
    ///
    /// The *dot* is a 4px/4px circle of the `color` that has two outlines white then black to increase contrast.
    #[inline]
    pub fn push_debug_dot(&mut self, offset: LayoutPoint, color: impl Into<RenderColor>) {
        // TODO use radial gradient to draw a dot.
        let offset = offset.snap_to(self.pixel_grid());

        let mut centered_rect = |mut o: LayoutPoint, s, c| {
            let s = LayoutSize::new(s, s);
            o.x -= s.width / 2.0;
            o.y -= s.height / 2.0;
            let rect = LayoutRect::new(o, s).snap_to(self.pixel_grid());
            self.push_color(rect, c);
        };

        centered_rect(offset, 8.0, crate::color::colors::BLACK.into());
        centered_rect(offset, 6.0, crate::color::colors::WHITE.into());
        centered_rect(offset, 4.0, color.into());
    }

    /// Finalizes the build.
    ///
    /// # Returns
    ///
    /// `(PipelineId, LayoutSize, BuiltDisplayList)` : The display list finalize data.
    /// `FrameInfo`: The built frame info.
    pub fn finalize(mut self) -> ((PipelineId, LayoutSize, BuiltDisplayList), FrameInfo) {
        self.close_widget_display();
        self.info.set_meta(self.info_id, self.meta);
        (self.display_list.finalize(), self.info.build())
    }
}

enum RenderLineCommand {
    Line(LineStyle, f32),
    Border(BorderStyle),
}
impl crate::border::LineStyle {
    fn render_command(self) -> RenderLineCommand {
        use crate::border::LineStyle as LS;
        use RenderLineCommand::*;
        match self {
            LS::Solid => Line(LineStyle::Solid, 0.0),
            LS::Double => Border(BorderStyle::Double),
            LS::Dotted => Line(LineStyle::Dotted, 0.0),
            LS::Dashed => Line(LineStyle::Dashed, 0.0),
            LS::Groove => Border(BorderStyle::Groove),
            LS::Ridge => Border(BorderStyle::Ridge),
            LS::Wavy(thickness) => Line(LineStyle::Wavy, thickness),
            LS::Hidden => Border(BorderStyle::Hidden),
        }
    }
}

/// Attempt to modify/cancel a widget transform or filters when it already started
/// pushing display items.
#[derive(Debug, dm::Display, dm::Error)]
#[display(fmt = "cannot modify widget transform or filters, widget display items already pushed")]
pub struct WidgetStartedError;

/// A frame quick update.
///
/// A frame update causes a frame render without needing to fully rebuild the display list. It
/// is a more performant but also more limited way of generating a frame.
///
/// Any [`FrameBindingKey`] used in the creation of the frame can be used for updating the frame.
pub struct FrameUpdate {
    bindings: DynamicProperties,
    frame_id: FrameId,
    window_id: WindowId,
    widget_id: WidgetId,
    widget_transform: LayoutTransform,
    widget_transform_key: WidgetTransformKey,
    cancel_widget: bool,
}
impl FrameUpdate {
    /// New frame update builder.
    ///
    /// * `window_id` - Id of the window that owns the frame.
    /// * `root_id` - Id of the widget at the root of the frame.
    /// * `root_transform_key` - Frame binding for the root widget layout transform.
    /// * `frame_id` - Id of the frame that will be updated.
    pub fn new(window_id: WindowId, root_id: WidgetId, root_transform_key: WidgetTransformKey, frame_id: FrameId) -> Self {
        FrameUpdate {
            bindings: DynamicProperties::default(),
            window_id,
            widget_id: root_id,
            widget_transform: LayoutTransform::identity(),
            widget_transform_key: root_transform_key,
            frame_id,
            cancel_widget: false,
        }
    }

    /// Window that owns the frame.
    #[inline]
    pub fn window_id(&self) -> WindowId {
        self.window_id
    }

    /// Current widget.
    #[inline]
    pub fn widget_id(&self) -> WidgetId {
        self.widget_id
    }

    /// The frame that will be updated.
    #[inline]
    pub fn frame_id(&self) -> FrameId {
        self.frame_id
    }

    /// Includes the widget transform.
    #[inline]
    pub fn with_widget_transform(&mut self, transform: &LayoutTransform, child: &impl UiNode, ctx: &mut RenderContext) {
        self.widget_transform = self.widget_transform.post_transform(transform);
        child.render_update(ctx, self);
    }

    /// Update a layout transform value.
    #[inline]
    pub fn update_transform(&mut self, new_value: FrameValue<LayoutTransform>) {
        self.bindings.transforms.push(new_value);
    }

    /// Update a float value.
    #[inline]
    pub fn update_f32(&mut self, new_value: FrameValue<f32>) {
        self.bindings.floats.push(new_value);
    }

    /// Calls [`render_update`](UiNode::render_update) for `child` inside a new widget context.
    #[inline]
    pub fn update_widget(&mut self, id: WidgetId, transform_key: WidgetTransformKey, child: &impl UiNode, ctx: &mut RenderContext) {
        if self.cancel_widget {
            return;
        }

        // NOTE: root widget is not processed by this method, if you add widget behavior here
        // similar behavior must be added in the `new` and `finalize` methods.

        let parent_id = mem::replace(&mut self.widget_id, id);
        let parent_transform_key = mem::replace(&mut self.widget_transform_key, transform_key);
        let parent_transform = mem::replace(&mut self.widget_transform, LayoutTransform::identity());
        let transforms_len = self.bindings.transforms.len();
        let floats_len = self.bindings.floats.len();

        child.render_update(ctx, self);

        self.widget_id = parent_id;
        self.widget_transform_key = parent_transform_key;

        if self.cancel_widget {
            self.cancel_widget = false;
            self.widget_transform = parent_transform;
            self.bindings.transforms.truncate(transforms_len);
            self.bindings.floats.truncate(floats_len);
        } else {
            let widget_transform = mem::replace(&mut self.widget_transform, parent_transform);
            if widget_transform != LayoutTransform::identity() {
                self.update_transform(self.widget_transform_key.update(widget_transform));
            }
        }
    }

    /// Rollback the current [`update_widget`](Self::update_widget).
    pub fn cancel_widget(&mut self) {
        self.cancel_widget = true;
    }

    /// Finalize the update.
    pub fn finalize(mut self) -> DynamicProperties {
        if self.widget_transform != LayoutTransform::identity() {
            self.update_transform(self.widget_transform_key.update(self.widget_transform));
        }

        self.bindings
    }
}

/// A frame value that can be updated without regenerating the full frame.
///
/// Use `FrameBinding::Value(value)` to not use the quick update feature.
///
/// Create a [`FrameBindingKey`] and use its [`bind`](FrameBindingKey::bind) method to
/// setup a frame binding.
pub type FrameBinding<T> = PropertyBinding<T>; // we rename this to not conflict with the zero_ui property terminology.

/// A frame value update.
pub type FrameValue<T> = PropertyValue<T>;

unique_id! {
    #[derive(Debug)]
    struct FrameBindingKeyId;
}

/// Unique key of a [`FrameBinding`] value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameBindingKey<T> {
    id: FrameBindingKeyId,
    _type: PhantomData<T>,
}
impl<T> FrameBindingKey<T> {
    /// Generates a new unique ID.
    ///
    /// # Panics
    /// Panics if called more then `u64::MAX` times.
    #[inline]
    pub fn new_unique() -> Self {
        FrameBindingKey {
            id: FrameBindingKeyId::new_unique(),
            _type: PhantomData,
        }
    }

    fn property_key(&self) -> PropertyBindingKey<T> {
        PropertyBindingKey::new(self.id.get())
    }

    /// Create a binding with this key.
    #[inline]
    pub fn bind(self, value: T) -> FrameBinding<T> {
        FrameBinding::Binding(self.property_key(), value)
    }

    /// Create a value update with this key.
    #[inline]
    pub fn update(self, value: T) -> FrameValue<T> {
        FrameValue {
            key: self.property_key(),
            value,
        }
    }
}

/// `FrameBindingKey<LayoutTransform>`.
pub type WidgetTransformKey = FrameBindingKey<LayoutTransform>;

/// Complement of [`ItemTag`] that indicates the hit area of a widget.
pub const WIDGET_HIT_AREA: u16 = u16::max_value();

fn unpack_cursor(raw: u16) -> CursorIcon {
    debug_assert!(raw <= CursorIcon::RowResize as u16);

    if raw <= CursorIcon::RowResize as u16 {
        unsafe { std::mem::transmute(raw as u8) }
    } else {
        CursorIcon::Default
    }
}

/// A hit-test hit.
#[derive(Clone, Debug)]
pub struct HitInfo {
    /// ID of widget hit.
    pub widget_id: WidgetId,
    /// Exact hit point in the widget space.
    pub point: LayoutPoint,
    /// Cursor icon selected for the widget.
    pub cursor: CursorIcon,
}

/// A hit-test result.
#[derive(Clone, Debug)]
pub struct FrameHitInfo {
    window_id: WindowId,
    frame_id: FrameId,
    point: LayoutPoint,
    hits: Vec<HitInfo>,
}

impl FrameHitInfo {
    /// Initializes from a Webrender hit-test result.
    ///
    /// Only item tags produced by [`FrameBuilder`] are expected.
    ///
    /// The tag format is:
    ///
    /// * `u64`: Raw [`WidgetId`].
    /// * `u16`: Raw [`CursorIcon`] or `WIDGET_HIT_AREA`.
    ///
    /// The tag marked with `WIDGET_HIT_AREA` is used to determine the [`HitInfo::point`](HitInfo::point).
    #[inline]
    pub fn new(window_id: WindowId, frame_id: FrameId, point: LayoutPoint, hits: &HitTestResult) -> Self {
        let mut candidates = Vec::default();
        let mut actual_hits = IdMap::default();

        for hit in &hits.items {
            if hit.tag.0 == 0 {
                continue;
            }

            // SAFETY: we skip zero so the value is memory safe.
            let widget_id = unsafe { WidgetId::from_raw(hit.tag.0) };
            if hit.tag.1 == WIDGET_HIT_AREA {
                candidates.push((widget_id, hit.point_relative_to_item));
            } else {
                actual_hits.insert(widget_id, hit.tag.1);
            }
        }

        let mut hits = Vec::default();

        for (widget_id, point) in candidates {
            if let Some(raw_cursor) = actual_hits.remove(&widget_id) {
                hits.push(HitInfo {
                    widget_id,
                    point,
                    cursor: unpack_cursor(raw_cursor),
                })
            }
        }

        // hits outside WIDGET_HIT_AREA
        for (widget_id, raw_cursor) in actual_hits.drain() {
            hits.push(HitInfo {
                widget_id,
                point: LayoutPoint::new(-1.0, -1.0),
                cursor: unpack_cursor(raw_cursor),
            })
        }

        hits.shrink_to_fit();

        FrameHitInfo {
            window_id,
            frame_id,
            point,
            hits,
        }
    }

    /// No hits info
    #[inline]
    pub fn no_hits(window_id: WindowId) -> Self {
        FrameHitInfo::new(
            window_id,
            FrameId::invalid(),
            LayoutPoint::new(-1.0, -1.0),
            &HitTestResult::default(),
        )
    }

    /// The window that was hit-tested.
    #[inline]
    pub fn window_id(&self) -> WindowId {
        self.window_id
    }

    /// The window frame that was hit-tested.
    #[inline]
    pub fn frame_id(&self) -> FrameId {
        self.frame_id
    }

    /// The point in the window that was hit-tested.
    #[inline]
    pub fn point(&self) -> LayoutPoint {
        self.point
    }

    /// Top-most cursor or `CursorIcon::Default` if there was no hit.
    #[inline]
    pub fn cursor(&self) -> CursorIcon {
        self.hits.first().map(|h| h.cursor).unwrap_or(CursorIcon::Default)
    }

    /// All hits, from top-most.
    #[inline]
    pub fn hits(&self) -> &[HitInfo] {
        &self.hits
    }

    /// The top hit.
    #[inline]
    pub fn target(&self) -> Option<&HitInfo> {
        self.hits.first()
    }

    /// Finds the widget in the hit-test result if it was hit.
    #[inline]
    pub fn find(&self, widget_id: WidgetId) -> Option<&HitInfo> {
        self.hits.iter().find(|h| h.widget_id == widget_id)
    }

    /// If the widget is in was hit.
    #[inline]
    pub fn contains(&self, widget_id: WidgetId) -> bool {
        self.hits.iter().any(|h| h.widget_id == widget_id)
    }

    /// Gets a clone of `self` that only contains the hits that also happen in `other`.
    #[inline]
    pub fn intersection(&self, other: &FrameHitInfo) -> FrameHitInfo {
        let mut hits: Vec<_> = self.hits.iter().filter(|h| other.contains(h.widget_id)).cloned().collect();
        hits.shrink_to_fit();

        FrameHitInfo {
            window_id: self.window_id,
            frame_id: self.frame_id,
            point: self.point,
            hits,
        }
    }
}

/// [`FrameInfo`] builder.
pub struct FrameInfoBuilder {
    window_id: WindowId,
    frame_id: FrameId,
    tree: Tree<WidgetInfoInner>,
}

impl FrameInfoBuilder {
    /// Starts building a frame info with the frame root information.
    #[inline]
    pub fn new(window_id: WindowId, frame_id: FrameId, root_id: WidgetId, size: LayoutSize) -> Self {
        let tree = Tree::new(WidgetInfoInner {
            widget_id: root_id,
            bounds: LayoutRect::from_size(size),
            meta: OwnedStateMap::new(),
        });

        FrameInfoBuilder { window_id, frame_id, tree }
    }

    /// Gets the root widget info id.
    #[inline]
    pub fn root_id(&self) -> WidgetInfoId {
        WidgetInfoId(self.tree.root().id())
    }

    #[inline]
    fn node(&mut self, id: WidgetInfoId) -> ego_tree::NodeMut<WidgetInfoInner> {
        self.tree
            .get_mut(id.0)
            .ok_or_else(|| format!("`{:?}` not found in this builder", id))
            .unwrap()
    }

    /// Takes the widget metadata already set for `id`.
    #[inline]
    pub fn take_meta(&mut self, id: WidgetInfoId) -> OwnedStateMap {
        mem::take(&mut self.node(id).value().meta)
    }

    /// Sets the widget metadata for `id`.
    #[inline]
    pub fn set_meta(&mut self, id: WidgetInfoId, meta: OwnedStateMap) {
        self.node(id).value().meta = meta;
    }

    /// Appends a widget child.
    #[inline]
    pub fn push(&mut self, parent: WidgetInfoId, widget_id: WidgetId, bounds: LayoutRect) -> WidgetInfoId {
        WidgetInfoId(
            self.node(parent)
                .append(WidgetInfoInner {
                    widget_id,
                    bounds,
                    meta: OwnedStateMap::new(),
                })
                .id(),
        )
    }

    /// Detaches the widget node.
    #[inline]
    pub fn cancel(&mut self, widget: WidgetInfoId) {
        self.node(widget).detach();
    }

    /// Builds the final frame info.
    #[inline]
    pub fn build(self) -> FrameInfo {
        let root_id = self.tree.root().id();

        // we build a WidgetId => NodeId lookup
        //
        // in debug mode we validate that the same WidgetId is not repeated
        //
        let valid_nodes = self
            .tree
            .nodes()
            .filter(|n| n.parent().is_some() || n.id() == root_id)
            .map(|n| (n.value().widget_id, n.id()));

        #[cfg(debug_assertions)]
        let (repeats, lookup) = {
            let mut repeats = IdMap::default();
            let mut lookup = IdMap::default();

            for (widget_id, node_id) in valid_nodes {
                if let Some(prev) = lookup.insert(widget_id, node_id) {
                    repeats.entry(widget_id).or_insert_with(|| vec![prev]).push(node_id);
                }
            }

            (repeats, lookup)
        };
        #[cfg(not(debug_assertions))]
        let lookup = valid_nodes.collect();

        let r = FrameInfo {
            timestamp: Instant::now(),
            window_id: self.window_id,
            frame_id: self.frame_id,
            lookup,
            tree: self.tree,
        };

        #[cfg(debug_assertions)]
        for (widget_id, repeats) in repeats {
            log::error!(target: "render", "widget id `{:?}` appears more then once in {:?}:FrameId({}){}",
            widget_id, self.window_id, self.frame_id.0, {
                let mut places = String::new();
                use std::fmt::Write;
                for node in repeats {
                    let info = WidgetInfo::new(&r, node);
                    write!(places, "\n    {}", info.path()).unwrap();
                }
                places
            });
        }

        r
    }
}

/// Id of a *building* widget info.
#[derive(Clone, Debug, Copy, Eq, PartialEq, Hash)]
pub struct WidgetInfoId(ego_tree::NodeId);

/// Information about a rendered frame.
///
/// Instantiated using [`FrameInfoBuilder`].
pub struct FrameInfo {
    timestamp: Instant,
    window_id: WindowId,
    frame_id: FrameId,
    tree: Tree<WidgetInfoInner>,
    lookup: IdMap<WidgetId, ego_tree::NodeId>,
}
impl FrameInfo {
    /// Blank window frame that contains only the root widget taking no space.
    #[inline]
    pub fn blank(window_id: WindowId, root_id: WidgetId) -> Self {
        FrameInfoBuilder::new(window_id, Epoch(0), root_id, LayoutSize::zero()).build()
    }

    /// Moment the frame info was finalized.
    ///
    /// Note that the frame may not be rendered on screen yet.
    #[inline]
    pub fn timestamp(&self) -> Instant {
        self.timestamp
    }

    /// Reference to the root widget in the frame.
    #[inline]
    pub fn root(&self) -> WidgetInfo {
        WidgetInfo::new(self, self.tree.root().id())
    }

    /// All widgets including `root`.
    #[inline]
    pub fn all_widgets(&self) -> impl Iterator<Item = WidgetInfo> {
        self.tree.root().descendants().map(move |n| WidgetInfo::new(self, n.id()))
    }

    /// ID of the window that rendered the frame.
    #[inline]
    pub fn window_id(&self) -> WindowId {
        self.window_id
    }

    /// ID of the rendered frame.
    #[inline]
    pub fn frame_id(&self) -> FrameId {
        self.frame_id
    }

    /// Reference to the widget in the frame, if it is present.
    #[inline]
    pub fn find(&self, widget_id: WidgetId) -> Option<WidgetInfo> {
        self.lookup
            .get(&widget_id)
            .and_then(|i| self.tree.get(*i).map(|n| WidgetInfo::new(self, n.id())))
    }

    /// If the frame contains the widget.
    #[inline]
    pub fn contains(&self, widget_id: WidgetId) -> bool {
        self.lookup.contains_key(&widget_id)
    }

    /// Reference to the widget in the frame, if it is present.
    ///
    /// Faster then [`find`](Self::find) if the widget path was generated by the same frame.
    #[inline]
    pub fn get(&self, path: &WidgetPath) -> Option<WidgetInfo> {
        if path.window_id() == self.window_id() && path.frame_id() == self.frame_id() {
            if let Some(id) = path.node_id {
                return self.tree.get(id).map(|n| WidgetInfo::new(self, n.id()));
            }
        }
        self.find(path.widget_id())
    }

    /// Reference to the widget or first parent that is present.
    #[inline]
    pub fn get_or_parent(&self, path: &WidgetPath) -> Option<WidgetInfo> {
        self.get(path)
            .or_else(|| path.ancestors().iter().rev().find_map(|&id| self.find(id)))
    }
}

/// Full address of a widget in a specific [`FrameInfo`].
#[derive(Clone)]
pub struct WidgetPath {
    node_id: Option<ego_tree::NodeId>,
    window_id: WindowId,
    frame_id: FrameId,
    path: Box<[WidgetId]>,
}
impl PartialEq for WidgetPath {
    /// Paths are equal if they share the same [window](Self::window_id) and [widget paths](Self::widgets_path).
    fn eq(&self, other: &Self) -> bool {
        self.window_id == other.window_id && self.path == other.path
    }
}
impl Eq for WidgetPath {}
impl fmt::Debug for WidgetPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            f.debug_struct("WidgetPath")
                .field("window_id", &self.window_id)
                .field("path", &self.path)
                .field("frame_id", &format_args!("FrameId({})", self.frame_id.0))
                .field("node_id", &self.node_id)
                .finish()
        } else {
            write!(f, "{}", self)
        }
    }
}
impl fmt::Display for WidgetPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}//", self.window_id)?;
        for w in self.ancestors() {
            write!(f, "{}/", w)?;
        }
        write!(f, "{}", self.widget_id())
    }
}
impl WidgetPath {
    /// New custom widget path.
    ///
    /// The path is not guaranteed to have ever existed, the [`frame_id`](Self::frame_id) is `FrameId::invalid`.
    pub fn new<P: Into<Box<[WidgetId]>>>(window_id: WindowId, path: P) -> WidgetPath {
        WidgetPath {
            node_id: None,
            window_id,
            frame_id: FrameId::invalid(),
            path: path.into(),
        }
    }

    /// Id of the window that contains the widgets.
    #[inline]
    pub fn window_id(&self) -> WindowId {
        self.window_id
    }

    /// Id of the frame from which this path was taken.
    #[inline]
    pub fn frame_id(&self) -> FrameId {
        self.frame_id
    }

    /// Widgets that contain [`widget_id`](WidgetPath::widget_id), root first.
    #[inline]
    pub fn ancestors(&self) -> &[WidgetId] {
        &self.path[..self.path.len() - 1]
    }

    /// The widget.
    #[inline]
    pub fn widget_id(&self) -> WidgetId {
        self.path[self.path.len() - 1]
    }

    /// [`ancestors`](WidgetPath::ancestors) and [`widget_id`](WidgetPath::widget_id), root first.
    #[inline]
    pub fn widgets_path(&self) -> &[WidgetId] {
        &self.path[..]
    }

    /// If the `widget_id` is part of the path.
    #[inline]
    pub fn contains(&self, widget_id: WidgetId) -> bool {
        self.path.iter().any(move |&w| w == widget_id)
    }

    /// Make a path to an ancestor id that is contained in the current path.
    #[inline]
    pub fn ancestor_path(&self, ancestor_id: WidgetId) -> Option<WidgetPath> {
        self.path.iter().position(|&id| id == ancestor_id).map(|i| WidgetPath {
            node_id: None,
            window_id: self.window_id,
            frame_id: self.frame_id,
            path: self.path[..i].iter().copied().collect(),
        })
    }

    /// Get the inner most widget parent shared by both `self` and `other`.
    ///
    /// The [`frame_id`](WidgetPath::frame_id) of `self` is used in the result.
    #[inline]
    pub fn shared_ancestor(&self, other: &WidgetPath) -> Option<WidgetPath> {
        if self.window_id == other.window_id {
            let mut path = Vec::default();

            for (a, b) in self.path.iter().zip(other.path.iter()) {
                if a != b {
                    break;
                }
                path.push(*a);
            }

            if !path.is_empty() {
                return Some(WidgetPath {
                    node_id: None,
                    window_id: self.window_id,
                    frame_id: self.frame_id,
                    path: path.into(),
                });
            }
        }
        None
    }

    /// Gets a path to the root widget of this path.
    #[inline]
    pub fn root_path(&self) -> WidgetPath {
        WidgetPath {
            node_id: None,
            window_id: self.window_id,
            frame_id: self.frame_id,
            path: Box::new([self.path[0]]),
        }
    }
}

struct WidgetInfoInner {
    widget_id: WidgetId,
    bounds: LayoutRect,
    meta: OwnedStateMap,
}

/// Reference to a widget info in a [`FrameInfo`].
#[derive(Clone, Copy)]
pub struct WidgetInfo<'a> {
    frame: &'a FrameInfo,
    node_id: ego_tree::NodeId,
}
impl<'a> PartialEq for WidgetInfo<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.node_id == other.node_id
    }
}
impl<'a> Eq for WidgetInfo<'a> {}
impl<'a> std::hash::Hash for WidgetInfo<'a> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.node_id, state)
    }
}
impl<'a> std::fmt::Debug for WidgetInfo<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WidgetInfo")
            .field("[frame_id]", &self.frame.frame_id.0)
            .field("[path]", &self.path().to_string())
            .field("[meta]", self.meta())
            .finish()
    }
}

impl<'a> WidgetInfo<'a> {
    #[inline]
    fn new(frame: &'a FrameInfo, node_id: ego_tree::NodeId) -> Self {
        Self { frame, node_id }
    }

    #[inline]
    fn node(&self) -> ego_tree::NodeRef<'a, WidgetInfoInner> {
        unsafe { self.frame.tree.get_unchecked(self.node_id) }
    }

    #[inline]
    fn info(&self) -> &'a WidgetInfoInner {
        self.node().value()
    }

    /// Widget id.
    #[inline]
    pub fn widget_id(self) -> WidgetId {
        self.info().widget_id
    }

    /// Full path to this widget.
    #[inline]
    pub fn path(self) -> WidgetPath {
        let mut path: Vec<_> = self.ancestors().map(|a| a.widget_id()).collect();
        path.reverse();
        path.push(self.widget_id());

        WidgetPath {
            frame_id: self.frame.frame_id,
            window_id: self.frame.window_id,
            node_id: Some(self.node_id),
            path: path.into(),
        }
    }

    /// Gets the [`path`](Self::path) if it is different from `old_path`.
    ///
    /// Only allocates a new path if needed.
    ///
    /// # Panics
    ///
    /// If `old_path` does not point to the same widget id as `self`.
    #[inline]
    pub fn new_path(self, old_path: &WidgetPath) -> Option<WidgetPath> {
        assert_eq!(old_path.widget_id(), self.widget_id());
        if self
            .ancestors()
            .zip(old_path.ancestors().iter().rev())
            .any(|(ancestor, id)| ancestor.widget_id() != *id)
        {
            Some(self.path())
        } else {
            None
        }
    }

    /// Widget rectangle in the frame.
    #[inline]
    pub fn bounds(self) -> &'a LayoutRect {
        &self.info().bounds
    }

    /// Widget bounds center.
    #[inline]
    pub fn center(self) -> LayoutPoint {
        self.bounds().center()
    }

    /// Metadata associated with the widget during render.
    #[inline]
    pub fn meta(self) -> &'a StateMap {
        &self.info().meta.0
    }

    /// Reference the [`FrameInfo`] that owns `self`.
    #[inline]
    pub fn frame(self) -> &'a FrameInfo {
        self.frame
    }

    /// Reference to the frame root widget.
    #[inline]
    pub fn root(self) -> Self {
        self.ancestors().last().unwrap_or(self)
    }

    /// Reference to the widget that contains this widget.
    ///
    /// Is `None` only for [`root`](FrameInfo::root).
    #[inline]
    pub fn parent(self) -> Option<Self> {
        self.node().parent().map(move |n| WidgetInfo::new(self.frame, n.id()))
    }

    /// Reference to the previous widget within the same parent.
    #[inline]
    pub fn prev_sibling(self) -> Option<Self> {
        self.node().prev_sibling().map(move |n| WidgetInfo::new(self.frame, n.id()))
    }

    /// Reference to the next widget within the same parent.
    #[inline]
    pub fn next_sibling(self) -> Option<Self> {
        self.node().next_sibling().map(move |n| WidgetInfo::new(self.frame, n.id()))
    }

    /// Reference to the first widget within this widget.
    #[inline]
    pub fn first_child(self) -> Option<Self> {
        self.node().first_child().map(move |n| WidgetInfo::new(self.frame, n.id()))
    }

    /// Reference to the last widget within this widget.
    #[inline]
    pub fn last_child(self) -> Option<Self> {
        self.node().last_child().map(move |n| WidgetInfo::new(self.frame, n.id()))
    }

    /// If the parent widget has multiple children.
    #[inline]
    pub fn has_siblings(self) -> bool {
        self.node().has_siblings()
    }

    /// If the widget has at least one child.
    #[inline]
    pub fn has_children(self) -> bool {
        self.node().has_children()
    }

    /// All parent children except this widget.
    #[inline]
    pub fn siblings(self) -> impl Iterator<Item = WidgetInfo<'a>> {
        self.prev_siblings().chain(self.next_siblings())
    }

    /// Iterator over the widgets directly contained by this widget.
    #[inline]
    pub fn children(self) -> impl DoubleEndedIterator<Item = WidgetInfo<'a>> {
        self.node().children().map(move |n| WidgetInfo::new(self.frame, n.id()))
    }

    /// Iterator over all widgets contained by this widget.
    #[inline]
    pub fn descendants(self) -> impl Iterator<Item = WidgetInfo<'a>> {
        //skip(1) due to ego_tree's descendants() including the node in the descendants
        self.node().descendants().skip(1).map(move |n| WidgetInfo::new(self.frame, n.id()))
    }

    /// Iterator over all widgets contained by this widget filtered by the `filter` closure.
    #[inline]
    pub fn filter_descendants<F: FnMut(WidgetInfo<'a>) -> DescendantFilter>(self, filter: F) -> FilterDescendants<'a, F> {
        let mut traverse = self.node().traverse();
        traverse.next(); // skip self.
        FilterDescendants {
            traverse,
            filter,
            frame: self.frame,
        }
    }

    /// Iterator over parent -> grandparent -> .. -> root.
    #[inline]
    pub fn ancestors(self) -> impl Iterator<Item = WidgetInfo<'a>> {
        self.node().ancestors().map(move |n| WidgetInfo::new(self.frame, n.id()))
    }

    /// Iterator over all previous widgets within the same parent.
    #[inline]
    pub fn prev_siblings(self) -> impl Iterator<Item = WidgetInfo<'a>> {
        self.node().prev_siblings().map(move |n| WidgetInfo::new(self.frame, n.id()))
    }

    /// Iterator over all next widgets within the same parent.
    #[inline]
    pub fn next_siblings(self) -> impl Iterator<Item = WidgetInfo<'a>> {
        self.node().next_siblings().map(move |n| WidgetInfo::new(self.frame, n.id()))
    }

    /// This widgets [`center`](Self::center) orientation in relation to a `origin`.
    #[inline]
    pub fn orientation_from(self, origin: LayoutPoint) -> WidgetOrientation {
        let o = self.center();
        for &d in &[
            WidgetOrientation::Left,
            WidgetOrientation::Right,
            WidgetOrientation::Above,
            WidgetOrientation::Below,
        ] {
            if is_in_direction(d, origin, o) {
                return d;
            }
        }
        unreachable!()
    }

    ///Iterator over all parent children except this widget with orientation in relation
    /// to this widget center.
    #[inline]
    pub fn oriented_siblings(self) -> impl Iterator<Item = (WidgetInfo<'a>, WidgetOrientation)> {
        let c = self.center();
        self.siblings().map(move |s| (s, s.orientation_from(c)))
    }

    /// All parent children except this widget, sorted by closest first.
    #[inline]
    pub fn closest_siblings(self) -> Vec<WidgetInfo<'a>> {
        self.closest_first(self.siblings())
    }

    /// All parent children except this widget, sorted by closest first and with orientation in
    /// relation to this widget center.
    #[inline]
    pub fn closest_oriented_siblings(self) -> Vec<(WidgetInfo<'a>, WidgetOrientation)> {
        let mut vec: Vec<_> = self.oriented_siblings().collect();
        let origin = self.center();
        vec.sort_by_cached_key(|n| n.0.distance_key(origin));
        vec
    }

    /// Unordered siblings to the left of this widget.
    #[inline]
    pub fn un_left_siblings(self) -> impl Iterator<Item = WidgetInfo<'a>> {
        self.oriented_siblings().filter_map(|(s, o)| match o {
            WidgetOrientation::Left => Some(s),
            _ => None,
        })
    }

    /// Unordered siblings to the right of this widget.
    #[inline]
    pub fn un_right_siblings(self) -> impl Iterator<Item = WidgetInfo<'a>> {
        self.oriented_siblings().filter_map(|(s, o)| match o {
            WidgetOrientation::Right => Some(s),
            _ => None,
        })
    }

    /// Unordered siblings to the above of this widget.
    #[inline]
    pub fn un_above_siblings(self) -> impl Iterator<Item = WidgetInfo<'a>> {
        self.oriented_siblings().filter_map(|(s, o)| match o {
            WidgetOrientation::Above => Some(s),
            _ => None,
        })
    }

    /// Unordered siblings to the below of this widget.
    #[inline]
    pub fn un_below_siblings(self) -> impl Iterator<Item = WidgetInfo<'a>> {
        self.oriented_siblings().filter_map(|(s, o)| match o {
            WidgetOrientation::Below => Some(s),
            _ => None,
        })
    }

    /// Siblings to the left of this widget sorted by closest first.
    #[inline]
    pub fn left_siblings(self) -> Vec<WidgetInfo<'a>> {
        self.closest_first(self.un_left_siblings())
    }

    /// Siblings to the right of this widget sorted by closest first.
    #[inline]
    pub fn right_siblings(self) -> Vec<WidgetInfo<'a>> {
        self.closest_first(self.un_right_siblings())
    }

    /// Siblings to the above of this widget sorted by closest first.
    #[inline]
    pub fn above_siblings(self) -> Vec<WidgetInfo<'a>> {
        self.closest_first(self.un_above_siblings())
    }

    /// Siblings to the below of this widget sorted by closest first.
    #[inline]
    pub fn below_siblings(self) -> Vec<WidgetInfo<'a>> {
        self.closest_first(self.un_below_siblings())
    }

    /// Value that indicates the distance between this widget center
    /// and `origin`.
    #[inline]
    pub fn distance_key(self, origin: LayoutPoint) -> usize {
        let o = self.center();
        let a = (o.x - origin.x).powf(2.);
        let b = (o.y - origin.y).powf(2.);
        (a + b) as usize
    }

    fn closest_first(self, iter: impl Iterator<Item = WidgetInfo<'a>>) -> Vec<WidgetInfo<'a>> {
        let mut vec: Vec<_> = iter.collect();
        let origin = self.center();
        vec.sort_by_cached_key(|n| n.distance_key(origin));
        vec
    }
}

/// Widget tree filter result.
///
/// This `enum` is used by the [`filter_descendants`](WidgetInfo::filter_descendants) method on [`WidgetInfo`]. See its documentation for more.
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum DescendantFilter {
    /// Include the descendant and continue filtering its descendants.
    Include,
    /// Skip the descendant but continue filtering its descendants.
    Skip,
    /// Skip the descendant and its descendants.
    SkipAll,
    /// Include the descendant but skips its descendants.
    SkipDescendants,
}

/// An iterator that filters a widget tree.
///
/// This `struct` is created by the [`filter_descendants`](WidgetInfo::filter_descendants) method on [`WidgetInfo`]. See its documentation for more.
pub struct FilterDescendants<'a, F: FnMut(WidgetInfo<'a>) -> DescendantFilter> {
    traverse: ego_tree::iter::Traverse<'a, WidgetInfoInner>,
    filter: F,
    frame: &'a FrameInfo,
}
impl<'a, F: FnMut(WidgetInfo<'a>) -> DescendantFilter> Iterator for FilterDescendants<'a, F> {
    type Item = WidgetInfo<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        use ego_tree::iter::Edge;
        #[allow(clippy::while_let_on_iterator)] // false positive https://github.com/rust-lang/rust-clippy/issues/7510
        while let Some(edge) = self.traverse.next() {
            if let Edge::Open(node) = edge {
                let widget = WidgetInfo::new(self.frame, node.id());
                match (self.filter)(widget) {
                    DescendantFilter::Include => return Some(widget),
                    DescendantFilter::Skip => continue,
                    DescendantFilter::SkipAll => {
                        for edge in &mut self.traverse {
                            if let Edge::Close(node2) = edge {
                                if node2 == node {
                                    break; // skip to close node.
                                }
                            }
                        }
                        continue;
                    }
                    DescendantFilter::SkipDescendants => {
                        for edge in &mut self.traverse {
                            if let Edge::Close(node2) = edge {
                                if node2 == node {
                                    break; // skip to close node.
                                }
                            }
                        }
                        return Some(widget);
                    }
                }
            }
        }
        None
    }
}

#[inline]
fn is_in_direction(direction: WidgetOrientation, origin: LayoutPoint, candidate: LayoutPoint) -> bool {
    let (a, b, c, d) = match direction {
        WidgetOrientation::Left => (candidate.x, origin.x, candidate.y, origin.y),
        WidgetOrientation::Right => (origin.x, candidate.x, candidate.y, origin.y),
        WidgetOrientation::Above => (candidate.y, origin.y, candidate.x, origin.x),
        WidgetOrientation::Below => (origin.y, candidate.y, candidate.x, origin.x),
    };

    // checks if the candidate point is in between two imaginary perpendicular lines parting from the
    // origin point in the focus direction
    if a <= b {
        if c >= d {
            return c <= d + (b - a);
        } else {
            return c >= d - (b - a);
        }
    }

    false
}

/// Orientation of a [`WidgetInfo`] relative to another point.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum WidgetOrientation {
    /// Widget is to the left of the reference point.
    Left,
    /// Widget is to the right of the reference point.
    Right,
    /// Widget is above the reference point.
    Above,
    /// Widget is below the reference point.
    Below,
}

bitflags! {
    /// Configure if a synthetic font is generated for fonts that do not implement **bold** or *oblique* variants.
    pub struct FontSynthesis: u8 {
        /// No synthetic font generated, if font resolution does not find a variant the matches the requested propertied
        /// the properties are ignored and the normal font is returned.
        const DISABLED = 0;
        /// Enable synthetic bold. Font resolution finds the closest bold variant, the difference added using extra stroke.
        const BOLD = 1;
        /// Enable synthetic oblique. If the font resolution does not find an oblique or italic variant a skew transform is applied.
        const STYLE = 2;
        /// Enabled all synthetic font possibilities.
        const ENABLED = Self::BOLD.bits | Self::STYLE.bits;
    }
}
impl Default for FontSynthesis {
    /// [`FontSynthesis::ENABLED`]
    #[inline]
    fn default() -> Self {
        FontSynthesis::ENABLED
    }
}
impl_from_and_into_var! {
    /// Convert to full [`ENABLED`](FontSynthesis::ENABLED) or [`DISABLED`](FontSynthesis::DISABLED).
    fn from(enabled: bool) -> FontSynthesis {
        if enabled { FontSynthesis::ENABLED } else { FontSynthesis::DISABLED }
    }
}

/// Pixels copied from a rendered frame.
#[derive(Clone)]
pub struct FramePixels {
    bgra: Arc<Vec<u8>>,
    width: u32,
    height: u32,
    opaque: bool,

    /// Scale factor used when rendering the frame.
    ///
    /// This value is used only as the *DPI* metadata of
    /// image files encoded from these pixels.
    pub scale_factor: f32,
}
impl Default for FramePixels {
    fn default() -> Self {
        Self {
            bgra: Arc::default(),
            width: 0,
            height: 0,
            opaque: true,
            scale_factor: 96.0,
        }
    }
}
impl fmt::Debug for FramePixels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FramePixels")
            .field("bgra", &format_args!("<{} heap bytes>", self.bgra.len()))
            .field("width", &self.width)
            .field("height", &self.height)
            .field("opaque", &self.opaque)
            .field("scale_factor", &self.scale_factor)
            .finish()
    }
}
impl FramePixels {
    /// **BGRA8** frame pixels, bottom-to-top.
    ///
    ///
    #[inline]
    pub fn bgra(&self) -> &Arc<Vec<u8>> {
        &self.bgra
    }

    /// Pixels width.
    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Pixels height.
    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns `true` if all pixels in [`bgra`] are fully opaque.
    ///
    /// [`bgra`]: Self::bgra
    #[inline]
    pub fn opaque(&self) -> bool {
        self.opaque
    }

    /// Get the underlying data `(bgra, width, height)`.
    #[inline]
    pub fn raw(self) -> (Arc<Vec<u8>>, u32, u32) {
        (self.bgra, self.width, self.height)
    }

    /// Encode and save the pixels as an image.
    ///
    /// The image format is defined by the file extension.
    ///
    /// # Scale Factor
    ///
    /// The scale factor is used only if the formats are `PNG` or `JPEG`, for both
    /// it sets the pixel density to `96dpi * scale_factor`.
    ///
    /// # ICO
    ///
    /// The `.ico` format only works if the image buffer width and height are in the `1..=256` range.
    ///
    /// # Panics
    ///
    /// If [`width`] or [`height`] are zero.
    ///
    /// [`width`]: FramePixels::width
    /// [`height`]: FramePixels::height
    pub async fn save(&self, path: impl Into<std::path::PathBuf>) -> image::ImageResult<()> {
        assert!(self.width > 0 && self.height > 0, "cannot save empty frame pixels");

        use image::*;
        use std::fs;

        let path = path.into();
        let format = ImageFormat::from_path(&path)?;
        let mut file = task::wait(move || fs::File::create(path)).await?;

        // invert rows, `image` only supports top-to-bottom buffers.
        let bgra: Vec<_> = self.bgra.rchunks_exact(self.width as usize * 4).flatten().copied().collect();

        let width = self.width;
        let height = self.height;
        let scale_factor = self.scale_factor;
        let opaque = self.opaque;

        let encoded = task::run(async move {
            let mut buffer = vec![];

            match format {
                ImageFormat::Jpeg => {
                    let mut jpg = codecs::jpeg::JpegEncoder::new(&mut buffer);
                    let dpi = (96.0 * scale_factor) as u16;
                    jpg.set_pixel_density(codecs::jpeg::PixelDensity::dpi(dpi));
                    jpg.encode(&bgra, width, height, ColorType::Bgra8)?;
                }
                ImageFormat::Farbfeld => {
                    let mut pixels = Vec::with_capacity(bgra.len() * 2);
                    for bgra in bgra.chunks(4) {
                        fn c(c: u8) -> [u8; 2] {
                            let c = (c as f32 / 255.0) * u16::MAX as f32;
                            (c as u16).to_ne_bytes()
                        }
                        pixels.extend(c(bgra[2]));
                        pixels.extend(c(bgra[1]));
                        pixels.extend(c(bgra[0]));
                        pixels.extend(c(bgra[3]));
                    }

                    let ff = codecs::farbfeld::FarbfeldEncoder::new(&mut buffer);
                    ff.encode(&pixels, width, height)?;
                }
                ImageFormat::Tga => {
                    let tga = codecs::tga::TgaEncoder::new(&mut buffer);
                    tga.encode(&bgra, width, height, ColorType::Bgra8)?;
                }
                rgb_only => {
                    let mut pixels;
                    let color_type;
                    if opaque {
                        color_type = ColorType::Rgb8;
                        pixels = Vec::with_capacity(width as usize * height as usize * 3);
                        for bgra in bgra.chunks(4) {
                            pixels.push(bgra[2]);
                            pixels.push(bgra[1]);
                            pixels.push(bgra[0]);
                        }
                    } else {
                        color_type = ColorType::Rgba8;
                        pixels = bgra;
                        for pixel in pixels.chunks_mut(4) {
                            pixel.swap(0, 2);
                        }
                    }

                    match rgb_only {
                        ImageFormat::Png => {
                            let mut png_bytes = vec![];
                            let png = codecs::png::PngEncoder::new(&mut png_bytes);

                            png.encode(&pixels, width, height, color_type)?;

                            let mut png = img_parts::png::Png::from_bytes(png_bytes.into()).unwrap();

                            let chunk_kind = *b"pHYs";
                            debug_assert!(png.chunk_by_type(chunk_kind).is_none());

                            use byteorder::*;
                            let mut chunk = Vec::with_capacity(4 * 2 + 1);

                            // 96dpi * scale / inch_to_metric
                            let ppm = (96.0 * scale_factor / 0.0254) as u32;

                            chunk.write_u32::<BigEndian>(ppm).unwrap(); // x
                            chunk.write_u32::<BigEndian>(ppm).unwrap(); // y
                            chunk.write_u8(1).unwrap(); // metric

                            let chunk = img_parts::png::PngChunk::new(chunk_kind, chunk.into());
                            png.chunks_mut().insert(1, chunk);

                            png.encoder().write_to(&mut buffer)?;
                        }
                        ImageFormat::Tiff => {
                            // TODO set ResolutionUnit to 2 (inch) and set both XResolution and YResolution
                            let mut seek_buf = std::io::Cursor::new(vec![]);
                            let tiff = codecs::tiff::TiffEncoder::new(&mut seek_buf);

                            tiff.encode(&pixels, width, height, color_type)?;

                            buffer = seek_buf.into_inner();
                        }
                        ImageFormat::Gif => {
                            let mut gif = codecs::gif::GifEncoder::new(&mut buffer);

                            gif.encode(&pixels, width, height, color_type)?;
                        }
                        ImageFormat::Bmp => {
                            // TODO set biXPelsPerMeter and biYPelsPerMeter
                            let mut bmp = codecs::bmp::BmpEncoder::new(&mut buffer);

                            bmp.encode(&pixels, width, height, color_type)?;
                        }
                        ImageFormat::Ico => {
                            // TODO set density in the inner PNG?
                            let ico = codecs::ico::IcoEncoder::new(&mut buffer);

                            ico.encode(&pixels, width, height, color_type)?;
                        }
                        unsuported => {
                            use image::error::*;
                            let hint = ImageFormatHint::Exact(unsuported);
                            return Err(ImageError::Unsupported(UnsupportedError::from_format_and_kind(
                                hint.clone(),
                                UnsupportedErrorKind::Format(hint),
                            )));
                        }
                    }
                }
            }
            Ok(buffer)
        })
        .await?;

        task::wait(move || file.write_all(&encoded)).await?;
        Ok(())
    }

    /// Create an [`Image`] from the pixel data.
    ///
    /// [`Image`]: crate::image::Image
    #[inline]
    pub fn image(&self) -> crate::image::Image {
        crate::image::Image::from_raw(self.bgra.clone(), (self.width, self.height), self.opaque)
    }
}
impl From<FramePixels> for crate::image::Image {
    fn from(pixels: FramePixels) -> Self {
        pixels.image()
    }
}
impl From<crate::app::view_process::FramePixels> for FramePixels {
    fn from(f: crate::app::view_process::FramePixels) -> Self {
        Self {
            bgra: Arc::new(f.bgra.into_vec()),
            width: f.width,
            height: f.height,
            opaque: f.opaque,
            scale_factor: f.scale_factor,
        }
    }
}
