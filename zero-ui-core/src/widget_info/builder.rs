use std::hash::Hash;

use crate::{
    border::BORDER,
    context::{
        InlineConstraints, InlineConstraintsLayout, InlineConstraintsMeasure, StateId, StateMapMut, StateValue, UpdateFlags, WidgetUpdates,
        LAYOUT, WIDGET, WINDOW,
    },
    text::TextSegmentKind,
};

use super::*;

/// Tag for the [`WidgetInfo::meta`] state-map.
pub enum WidgetInfoMeta {}

/// Widget info tree builder.
///
/// See [`WidgetInfoTree`] for more details.
pub struct WidgetInfoBuilder {
    info_widgets: Arc<WidgetUpdates>,
    window_id: WindowId,

    node: tree::NodeId,
    widget_id: WidgetId,
    meta: Arc<Mutex<OwnedStateMap<WidgetInfoMeta>>>,

    tree: Tree<WidgetInfoData>,
    interactivity_filters: InteractivityFilters,

    scale_factor: Factor,

    build_meta: Arc<Mutex<OwnedStateMap<WidgetInfoMeta>>>,

    build_start: Instant,
    pushed_widgets: u32,
}
impl WidgetInfoBuilder {
    /// Starts building a info tree with the root information.
    pub fn new(
        info_widgets: Arc<WidgetUpdates>,
        window_id: WindowId,
        root_id: WidgetId,
        root_bounds_info: WidgetBoundsInfo,
        root_border_info: WidgetBorderInfo,
        scale_factor: Factor,
    ) -> Self {
        let tree = Tree::new(WidgetInfoData {
            id: root_id,
            bounds_info: root_bounds_info,
            border_info: root_border_info,
            meta: Arc::new(OwnedStateMap::new()),
            interactivity_filters: vec![],
            local_interactivity: Interactivity::ENABLED,
            cache: Mutex::new(WidgetInfoCache { interactivity: None }),
        });
        let mut lookup = IdMap::default();
        let root_node = tree.root().id();
        lookup.insert(root_id, root_node);

        WidgetInfoBuilder {
            info_widgets,
            window_id,
            node: root_node,
            tree,
            interactivity_filters: vec![],
            meta: Arc::default(),
            widget_id: root_id,
            scale_factor,
            build_meta: Arc::default(),
            build_start: Instant::now(),
            pushed_widgets: 1, // root is always new.
        }
    }

    fn node(&mut self, id: tree::NodeId) -> tree::NodeMut<WidgetInfoData> {
        self.tree.index_mut(id)
    }

    /// Current widget id.
    pub fn widget_id(&self) -> WidgetId {
        self.widget_id
    }

    /// Widget info tree build metadata.
    ///
    /// This metadata can be modified only by pushed widgets, **not** by the reused widgets.
    pub fn with_build_meta<R>(&mut self, visitor: impl FnOnce(StateMapMut<WidgetInfoMeta>) -> R) -> R {
        visitor(self.build_meta.lock().borrow_mut())
    }
    /// Set the info tree build metadata `id` to `value`.
    pub fn set_build_meta<T: StateValue>(&mut self, id: impl Into<StateId<T>>, value: impl Into<T>) {
        let id = id.into();
        let value = value.into();
        self.with_build_meta(|mut s| s.set(id, value));
    }
    /// Sets the info tree build metadata `id` without value.
    pub fn flag_build_meta(&mut self, id: impl Into<StateId<()>>) {
        let id = id.into();
        self.with_build_meta(|mut s| s.flag(id));
    }

    /// Current widget info metadata.
    pub fn with_meta<R>(&mut self, visitor: impl FnOnce(StateMapMut<WidgetInfoMeta>) -> R) -> R {
        visitor(self.meta.lock().borrow_mut())
    }
    /// Set the widget info metadata `id` to `value`.
    ///
    /// Returns the previous set value.
    pub fn set_meta<T: StateValue>(&mut self, id: impl Into<StateId<T>>, value: impl Into<T>) {
        let id = id.into();
        let value = value.into();
        self.with_meta(|mut s| s.set(id, value));
    }
    /// Sets the widget info metadata `id` without value.
    pub fn flag_meta(&mut self, id: impl Into<StateId<()>>) {
        let id = id.into();
        self.with_meta(|mut s| s.flag(id));
    }

    /// Calls `f` to build the context widget info, note that `f` is only called if the widget info cannot be reused.
    pub fn push_widget(&mut self, f: impl FnOnce(&mut Self)) {
        let id = WIDGET.id();
        if !WIDGET.take_update(UpdateFlags::INFO) && !self.info_widgets.delivery_list().enter_widget(id) {
            // reuse
            let tree = WINDOW.widget_tree();
            if let Some(wgt) = tree.get(id) {
                self.tree.index_mut(self.node).push_reuse(wgt.node(), &mut |old_data| {
                    let r = old_data.clone();
                    r.cache.lock().interactivity = None;
                    for filter in &r.interactivity_filters {
                        self.interactivity_filters.push(filter.clone());
                    }
                    r
                });
                return;
            }
        }

        let parent_node = self.node;
        let parent_widget_id = self.widget_id;
        let parent_meta = mem::take(&mut self.meta);

        let bounds_info = WIDGET.bounds();
        let border_info = WIDGET.border();

        self.widget_id = id;
        self.node = self
            .node(parent_node)
            .push_child(WidgetInfoData {
                id,
                bounds_info,
                border_info,
                meta: Arc::new(OwnedStateMap::new()),
                interactivity_filters: vec![],
                local_interactivity: Interactivity::ENABLED,
                cache: Mutex::new(WidgetInfoCache { interactivity: None }),
            })
            .id();

        self.pushed_widgets += 1;

        f(self);

        let meta = mem::replace(&mut self.meta, parent_meta);
        let mut node = self.node(self.node);
        node.value().meta = Arc::new(Arc::try_unwrap(meta).unwrap().into_inner());
        node.close();

        self.node = parent_node;
        self.widget_id = parent_widget_id;
    }

    /// Add the `interactivity` bits to the current widget's interactivity, it will affect the widget and all descendants.
    ///
    /// Also see [`push_interactivity_filter`] to affect the interactivity of widgets outside the current one.
    ///
    /// [`push_interactivity_filter`]: Self::push_interactivity_filter
    pub fn push_interactivity(&mut self, interactivity: Interactivity) {
        let mut node = self.node(self.node);
        let v = node.value();
        v.local_interactivity |= interactivity;
    }

    /// Register a closure that returns the [`Interactivity`] allowed for each widget.
    ///
    /// Widgets [`interactivity`] is computed from all interactivity filters and parents. Interactivity filters are global to the
    /// widget tree, and are re-registered for the tree if the current widget is reused.
    ///
    /// Note that the filter can make the assumption that parent widgets affect all descendants and if the filter is intended to
    /// affect only the current widget and descendants you can use [`push_interactivity`] instead.
    ///
    /// [`interactivity`]: WidgetInfo::interactivity
    /// [`push_interactivity`]: Self::push_interactivity
    pub fn push_interactivity_filter(&mut self, filter: impl Fn(&InteractivityFilterArgs) -> Interactivity + Send + Sync + 'static) {
        let filter = Arc::new(filter);
        self.interactivity_filters.push(filter.clone());
        self.node(self.node).value().interactivity_filters.push(filter);
    }

    /// Calls the `info` closure and returns the range of children inserted by it.
    pub fn with_children_range(&mut self, info: impl FnOnce(&mut Self)) -> ops::Range<usize> {
        let before_count = self.tree.index(self.node).children_count();
        info(self);
        before_count..self.tree.index(self.node).children_count()
    }

    /// Create a new info builder that can be built in parallel and merged back onto this list using [`parallel_fold`].
    ///
    /// Note that this must be called just before [`push_widget`], trying to modify the current widget info using the returned
    /// builder will not work properly.
    ///
    /// [`parallel_fold`]: Self::parallel_fold
    /// [`push_widget`]: Self::push_widget
    pub fn parallel_split(&self) -> ParallelBuilder<Self> {
        let tree = Tree::new(self.tree.root().value().clone());
        ParallelBuilder(Some(Self {
            info_widgets: self.info_widgets.clone(),
            window_id: self.window_id,
            widget_id: self.widget_id,
            meta: self.meta.clone(),
            node: tree.root().id(),
            tree,
            interactivity_filters: vec![],
            scale_factor: self.scale_factor,
            build_meta: self.build_meta.clone(),
            build_start: self.build_start,
            pushed_widgets: 0,
        }))
    }

    /// Collect info from `split` into `self`.
    pub fn parallel_fold(&mut self, mut split: ParallelBuilder<Self>) {
        let mut split = split.take();

        self.interactivity_filters.append(&mut split.interactivity_filters);
        self.pushed_widgets += split.pushed_widgets;

        self.tree.index_mut(self.node).parallel_fold(split.tree, &mut |d| WidgetInfoData {
            id: d.id,
            bounds_info: d.bounds_info.clone(),
            border_info: d.border_info.clone(),
            meta: d.meta.clone(),
            interactivity_filters: mem::take(&mut d.interactivity_filters),
            local_interactivity: d.local_interactivity,
            cache: Mutex::new(d.cache.get_mut().clone()),
        });
    }

    /// Build the info tree.
    pub fn finalize(mut self, previous_tree: Option<WidgetInfoTree>) -> WidgetInfoTree {
        let mut node = self.tree.root_mut();
        let meta = Arc::new(Arc::try_unwrap(self.meta).unwrap().into_inner());
        node.value().meta = meta;
        node.close();

        let generation;
        let widget_count_offsets;

        if let Some(t) = previous_tree {
            let t = t.0.frame.read();
            generation = t.stats.generation.wrapping_add(1);
            widget_count_offsets = t.widget_count_offsets.clone();
        } else {
            generation = 0;
            widget_count_offsets = ParallelSegmentOffsets::default()
        }

        let mut lookup = IdMap::new();
        lookup.reserve(self.tree.len());
        let mut out_of_bounds = vec![];

        for (id, data) in self.tree.iter() {
            if lookup.insert(data.id, id).is_some() {
                tracing::error!("widget `{}` repeated in info tree", data.id);
            }
            if data.bounds_info.is_actually_out_of_bounds() {
                out_of_bounds.push(id);
            }
        }
        out_of_bounds.shrink_to_fit();

        WidgetInfoTree(Arc::new(WidgetInfoTreeInner {
            window_id: self.window_id,
            lookup,
            interactivity_filters: self.interactivity_filters,
            build_meta: Arc::new(mem::take(&mut self.build_meta.lock())),

            frame: RwLock::new(WidgetInfoTreeFrame {
                stats: WidgetInfoTreeStats::new(self.build_start, self.tree.len() as u32 - self.pushed_widgets, generation),
                stats_update: Default::default(),
                out_of_bounds: Arc::new(out_of_bounds),
                out_of_bounds_update: Default::default(),
                scale_factor: self.scale_factor,
                spatial_bounds: PxBox::zero(),
                widget_count_offsets,
            }),

            tree: self.tree,
        }))
    }
}

/// Represents a segment in an inlined widget first or last row.
///
/// This info is used by inlining parent to sort the joiner row in a way that preserves bidirectional text flow.
///
/// See [`WidgetInlineMeasure::first_segs`] for more details.
#[derive(Clone, Copy, Debug)]
pub struct InlineSegment {
    /// Width of the segment, in pixels.
    pub width: f32,
    /// Info for bidirectional reorder.
    pub kind: TextSegmentKind,
}
impl PartialEq for InlineSegment {
    fn eq(&self, other: &Self) -> bool {
        about_eq(self.width, other.width, 0.001) && self.kind == other.kind
    }
}
impl Eq for InlineSegment {}
impl Hash for InlineSegment {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        about_eq_hash(self.width, 0.001, state);
        self.kind.hash(state);
    }
}

/// Represents an [`InlineSegment`] positioned by the inlining parent.
///
/// See [`InlineConstraintsLayout::first_segs`] for more details.
///
/// [`InlineConstraintsLayout::first_segs`]: crate::context::InlineConstraintsLayout::first_segs
#[derive(Clone, Copy, Debug)]
pub struct InlineSegmentPos {
    /// Seg offset to the right from the row origin, in pixels.
    pub x: f32,
}
impl PartialEq for InlineSegmentPos {
    fn eq(&self, other: &Self) -> bool {
        about_eq(self.x, other.x, 0.001)
    }
}
impl Eq for InlineSegmentPos {}
impl Hash for InlineSegmentPos {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        about_eq_hash(self.x, 0.001, state);
    }
}

/// Info about the input inline connecting rows of the widget.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WidgetInlineMeasure {
    /// Preferred first size.
    ///
    /// In left-to-right direction the origin is `top_left`, in right-to-left direction the origin is `top_right - first.width`.
    pub first: PxSize,

    /// Indicates that `first` starts in the next row, not in the *current* row defined by the inline constraints.
    pub first_wrapped: bool,

    /// Inline segments in the first row.
    ///
    /// The sum of segment widths must be less or equal to the `first.width`.
    pub first_segs: Arc<Vec<InlineSegment>>,

    /// Preferred last size.
    ///
    /// In left-to-right direction the origin is `bottom_left - last.height`, in right-to-left direction
    /// the origin is `bottom_right - last`.
    pub last: PxSize,

    /// Indicates that `last` starts in the next row, not in the same row as the first.
    pub last_wrapped: bool,

    /// Inline segments in the last row.
    ///
    /// The sum of segment widths must be less or equal to the `last.width`.
    pub last_segs: Arc<Vec<InlineSegment>>,
}
impl WidgetInlineMeasure {
    /// Visit a mutable reference to the new [`first_segs`] value, `f` is called with
    /// an empty vec that can be reused or new.
    ///
    /// [`first_segs`]: Self::first_segs
    pub fn with_first_segs(&mut self, f: impl FnOnce(&mut Vec<InlineSegment>)) {
        Self::with_segs(&mut self.first_segs, f)
    }

    /// Visit a mutable reference to the new [`last_segs`] value, `f` is called with
    /// an empty vec that can be reused or new.
    ///
    /// [`last_segs`]: Self::last_segs
    pub fn with_last_segs(&mut self, f: impl FnOnce(&mut Vec<InlineSegment>)) {
        Self::with_segs(&mut self.last_segs, f)
    }

    fn with_segs(items: &mut Arc<Vec<InlineSegment>>, f: impl FnOnce(&mut Vec<InlineSegment>)) {
        match Arc::get_mut(items) {
            Some(items) => {
                items.clear();
                f(items);
            }
            None => {
                let mut new = vec![];
                f(&mut new);
                *items = Arc::new(new);
            }
        }
    }

    /// If all value are not different from initial.
    ///
    /// This indicates the widget has not handled the inline config yet.
    pub fn is_default(&self) -> bool {
        self.first.is_empty()
            && !self.first_wrapped
            && self.first_segs.is_empty()
            && self.last.is_empty()
            && !self.last_wrapped
            && self.last_segs.is_empty()
    }
}

/// Info about a segment in the first or last row of an inlined widget.
///
/// See [`WidgetInlineInfo::first_segs`] for more details.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineSegmentInfo {
    /// Segment offset from the row rectangle origin.
    pub x: Px,
    /// Segment width.
    ///
    /// The segment height is the row rectangle height.
    pub width: Px,
}

/// Info about the inlined rows of the widget.
#[derive(Debug, Default)]
pub struct WidgetInlineInfo {
    /// Last layout rows of the widget.
    ///
    /// The rectangles are in the widget's inner space, from top to bottom.
    pub rows: Vec<PxRect>,

    /// Segments of the first row.
    ///
    /// If this is empty the entire row width is a continuous segment, otherwise the row is segmented and
    /// the widget can be interleaved with sibling widgets due to Unicode bidirectional text sorting algorithm.
    ///
    /// Note that the segment count may be less then [`WidgetInlineMeasure::first_segs`] as contiguous segments
    /// may be merged.
    ///
    /// The segments are from left to right.
    pub first_segs: Vec<InlineSegmentInfo>,

    /// Segments of the last row.
    pub last_segs: Vec<InlineSegmentInfo>,

    /// Widget inner size when the rows where last updated.
    pub inner_size: PxSize,

    negative_space: Mutex<(Arc<Vec<PxRect>>, bool)>,
}
impl WidgetInlineInfo {
    /// Replace the [`first_segs`] with `segs`.
    ///
    /// The segments are sorted when needed, but prefer inputs that are mostly sorted.
    ///
    /// The segments are merged when there is no gap or there is a small one pixel overlap to the previous segment.
    ///
    /// [`first_segs`]: Self::first_segs
    pub fn set_first_segs(&mut self, segs: impl Iterator<Item = InlineSegmentInfo>) {
        Self::set_segs(&mut self.first_segs, segs);
        self.invalidate_negative_space();
    }

    /// Replace the [`last_segs`] with `segs`.
    ///
    /// The segments are sorted when needed, but prefer inputs that are mostly sorted.
    ///
    /// The segments are merged when there is no gap or there is a small one pixel overlap to the previous segment.
    ///
    /// [`last_segs`]: Self::last_segs
    pub fn set_last_segs(&mut self, segs: impl Iterator<Item = InlineSegmentInfo>) {
        Self::set_segs(&mut self.last_segs, segs);
        self.invalidate_negative_space();
    }

    fn set_segs(vec: &mut Vec<InlineSegmentInfo>, segs: impl Iterator<Item = InlineSegmentInfo>) {
        vec.clear();

        let mut needs_sort = false;

        for seg in segs {
            if seg.width <= Px(0) {
                continue;
            }

            if let Some(last) = vec.last_mut() {
                let la = last.x;
                let lb = last.x + last.width;

                let a = seg.x;
                let b = seg.x + seg.width;

                if la.max(a) <= lb.min(b) {
                    // merge overlap
                    last.x = a.min(la);
                    last.width = b.max(lb) - last.x;
                    continue;
                }

                needs_sort |= a < la;
            }
            vec.push(seg);
        }

        if needs_sort {
            vec.sort_unstable_by_key(|s| s.x);
        }
    }

    /// Gets the union of all row rectangles.
    pub fn union(&self) -> PxRect {
        self.rows.iter().fold(PxRect::zero(), |union, row| union.union(row))
    }

    /// Gets or computes the negative space of the [`rows`] in the [`inner_size`] space, that is, all the areas that are
    /// not covered by any row and not covered by the first and last row segments.
    ///
    /// This is computed on demand and cached.
    ///
    /// [`rows`]: Self::rows
    /// [`inner_size`]: Self::inner_size
    pub fn negative_space(&self) -> Arc<Vec<PxRect>> {
        let mut space = self.negative_space.lock();
        if space.1 {
            return space.0.clone();
        }

        let mut vec = Arc::try_unwrap(mem::take(&mut space.0)).unwrap_or_default();
        vec.clear();

        self.negative_enveloped(&mut vec, PxRect::from_size(self.inner_size));

        let r = Arc::new(vec);
        *space = (r.clone(), true);
        r
    }

    /// Invalidates the [`negative_space`] cache.
    ///
    /// [`negative_space`]: Self::negative_space
    pub fn invalidate_negative_space(&mut self) {
        self.negative_space.get_mut().1 = false;
    }

    fn negative_enveloped(&self, space: &mut Vec<PxRect>, bounds: PxRect) {
        let bounds_max_x = bounds.max_x();
        let mut last_max_y = bounds.origin.y;

        for r in &self.rows {
            let spacing_y = r.origin.y - last_max_y;
            if spacing_y > Px(0) {
                space.push(PxRect::new(
                    PxPoint::new(bounds.origin.x, last_max_y),
                    PxSize::new(bounds.size.width, spacing_y),
                ));
            }
            last_max_y = r.max_y();

            let left = r.origin.x - bounds.origin.x;
            if left > Px(0) {
                space.push(PxRect::new(
                    PxPoint::new(bounds.origin.x, r.origin.y),
                    PxSize::new(left, r.size.height),
                ));
            }
            let max_x = r.max_x();
            let right = bounds_max_x - max_x;
            if right > Px(0) {
                space.push(PxRect::new(PxPoint::new(max_x, r.origin.y), PxSize::new(right, r.size.height)));
            }
        }
        let spacing_y = bounds.max_y() - last_max_y;
        if spacing_y > Px(0) {
            space.push(PxRect::new(
                PxPoint::new(bounds.origin.x, last_max_y),
                PxSize::new(bounds.size.width, spacing_y),
            ));
        }

        if let Some(r) = self.rows.first() {
            if !self.first_segs.is_empty() {
                let mut x = r.origin.x;
                for seg in self.first_segs.iter() {
                    let blank = seg.x - x;
                    if blank > Px(0) {
                        space.push(PxRect::new(PxPoint::new(x, r.origin.y), PxSize::new(blank, r.size.height)));
                    }
                    x = seg.x + seg.width;
                }
                let blank = r.max_x() - x;
                if blank > Px(0) {
                    space.push(PxRect::new(PxPoint::new(x, r.origin.y), PxSize::new(blank, r.size.height)));
                }
            }
        }
        if let Some(r) = self.rows.last() {
            if !self.last_segs.is_empty() {
                let mut x = r.origin.x;
                for seg in self.last_segs.iter() {
                    let blank = seg.x - x;
                    if blank > Px(0) {
                        space.push(PxRect::new(PxPoint::new(x, r.origin.y), PxSize::new(blank, r.size.height)));
                    }
                    x = seg.x + seg.width;
                }
                let blank = r.max_x() - x;
                if blank > Px(0) {
                    space.push(PxRect::new(PxPoint::new(x, r.origin.y), PxSize::new(blank, r.size.height)));
                }
            }
        }
    }

    ///Return info to default state, but retain memory for reuse.
    pub fn clear(&mut self) {
        self.first_segs.clear();
        self.last_segs.clear();
        self.rows.clear();
        self.inner_size = PxSize::zero();
        self.invalidate_negative_space();
    }

    /// If all value are not different from initial.
    ///
    /// This indicates the widget has not handled the inline config yet.
    pub fn is_default(&self) -> bool {
        self.rows.is_empty() && self.first_segs.is_empty() && self.last_segs.is_empty() && self.inner_size.is_empty()
    }
}
impl Clone for WidgetInlineInfo {
    fn clone(&self) -> Self {
        Self {
            rows: self.rows.clone(),
            first_segs: self.first_segs.clone(),
            last_segs: self.last_segs.clone(),
            inner_size: self.inner_size,
            negative_space: Mutex::new((Arc::new(vec![]), false)),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.clear();
        self.rows.extend_from_slice(&source.rows);
        self.first_segs.extend_from_slice(&source.first_segs);
        self.last_segs.extend_from_slice(&source.last_segs);
        self.inner_size = source.inner_size;
    }
}
impl PartialEq for WidgetInlineInfo {
    fn eq(&self, other: &Self) -> bool {
        self.rows == other.rows
            && self.first_segs == other.first_segs
            && self.last_segs == other.last_segs
            && self.inner_size == other.inner_size
    }
}

/// Represents the in-progress measure pass for a widget tree.
///
/// Use [`WidgetLayout::to_measure`] to instantiate.
pub struct WidgetMeasure {
    layout_widgets: Arc<WidgetUpdates>,
    inline: Option<WidgetInlineMeasure>,
    inline_locked: bool,
}
impl WidgetMeasure {
    pub(crate) fn new(layout_widgets: Arc<WidgetUpdates>) -> Self {
        Self {
            layout_widgets,
            inline: None,
            inline_locked: false,
        }
    }

    /// New with no widget layouts invalidated.
    ///
    /// Prefer [`WidgetLayout::to_measure`] instead of this.
    pub fn new_reuse(inline: Option<WidgetInlineMeasure>) -> Self {
        let mut r = Self::new(Arc::default());
        r.inline = inline;
        r
    }

    /// If the parent widget is doing inline flow layout.
    pub fn is_inline(&self) -> bool {
        self.inline.is_some()
    }

    /// Mutable reference to the current widget's inline info.
    ///
    /// The widget must configure this to be inlined in parent layout. This is only `Some(_)` if inline is enabled.
    ///
    /// See [`WidgetInlineMeasure`] for more details.
    pub fn inline(&mut self) -> Option<&mut WidgetInlineMeasure> {
        self.inline.as_mut()
    }

    /// Sets [`is_inline`] to `false`.
    ///
    /// Must be called before child delegation, otherwise children that inline may render expecting to fit in
    /// the inline flow.
    ///
    /// [`is_inline`]: Self::is_inline
    pub(crate) fn disable_inline(&mut self) {
        if !self.inline_locked {
            self.inline = None;
        }
    }

    /// Measure an widget.
    pub fn with_widget(&mut self, measure: impl FnOnce(&mut Self) -> PxSize) -> PxSize {
        let metrics = LAYOUT.metrics();
        let bounds = WIDGET.bounds();

        let snap = metrics.snapshot();
        if !WIDGET.layout_is_pending(&self.layout_widgets) {
            let measure_uses = bounds.measure_metrics_used();
            if bounds.measure_metrics().map(|m| m.masked_eq(&snap, measure_uses)).unwrap_or(false) {
                let mut reused = false;
                if let Some(inline) = self.inline() {
                    if let Some(prev) = bounds.measure_inline() {
                        *inline = prev;
                        reused = true;
                    }
                } else {
                    reused = bounds.measure_inline().is_none();
                }

                if reused {
                    // LAYOUT.register_metrics_use(measure_uses); // measure does not propagate uses.
                    return bounds.measure_outer_size();
                }
            }
        }

        let parent_inline = self.inline.take();
        if LAYOUT.inline_constraints().is_some() {
            self.inline = Some(Default::default());
        }

        let (measure_uses, size) = LAYOUT.capture_metrics_use(|| measure(self));

        bounds.set_measure_metrics(Some(snap), measure_uses);
        bounds.set_measure_outer_size(size);

        if let Some(inline) = self.inline.take() {
            if inline.is_default() && !size.is_empty() {
                // widget did not handle inline
                bounds.set_measure_inline(None);
            } else {
                bounds.set_measure_inline(Some(inline));
            }
        } else {
            bounds.set_measure_inline(None);
        }
        self.inline = parent_inline;

        size
    }

    /// Calls `measure` with inline force enabled on the widget.
    ///
    /// The widget will be inlining even if the parent widget is not inlining, if properties request [`disable_inline`]
    /// these requests are ignored.
    ///
    /// [`disable_inline`]: LAYOUT::disable_inline
    pub fn with_inline_visual(&mut self, measure: impl FnOnce(&mut Self) -> PxSize) -> PxSize {
        self.inline_locked = true;
        if self.inline.is_none() {
            self.inline = Some(Default::default());
        }
        let metrics = LAYOUT.metrics();
        let size = if metrics.inline_constraints().is_none() {
            let constraints = crate::context::InlineConstraints::Measure(InlineConstraintsMeasure {
                first_max: metrics.constraints().x.max_or(Px::MAX),
                mid_clear_min: Px(0),
            });
            let metrics = metrics.with_inline_constraints(Some(constraints));
            LAYOUT.with_context(metrics, || measure(self))
        } else {
            measure(self)
        };
        self.inline_locked = false;

        let inline = self.inline.clone().unwrap();
        let bounds = WIDGET.bounds();
        if inline.is_default() && !size.is_empty() {
            // widget did not handle inline
            bounds.set_measure_inline(None);
        } else {
            bounds.set_measure_inline(Some(inline));
        }
        bounds.set_measure_outer_size(size);

        size
    }

    /// Start a parallel measure.
    ///
    /// Returns an instance that can be used to acquire multiple mutable [`WidgetMeasure`] during measure.
    /// The [`parallel_fold`] method must be called after the parallel processing is done.
    ///
    /// [`parallel_fold`]: Self::parallel_fold
    pub fn parallel_split(&self) -> ParallelBuilder<WidgetMeasure> {
        ParallelBuilder(Some(Self {
            layout_widgets: self.layout_widgets.clone(),
            inline: self.inline.clone(),
            inline_locked: self.inline_locked,
        }))
    }

    /// Collect the parallel changes back.
    pub fn parallel_fold(&mut self, mut split: ParallelBuilder<WidgetMeasure>) {
        let _ = split.take();
    }
}

/// Represents the in-progress layout pass for a widget tree.
pub struct WidgetLayout {
    layout_widgets: Arc<WidgetUpdates>,
    bounds: WidgetBoundsInfo,
    nest_group: LayoutNestGroup,
    inline: Option<WidgetInlineInfo>,
    child_count: Option<u32>,
}
impl WidgetLayout {
    /// Defines the root widget outer-bounds scope.
    ///
    /// The default window implementation calls this.
    pub fn with_root_widget(layout_widgets: Arc<WidgetUpdates>, layout: impl FnOnce(&mut Self) -> PxSize) -> PxSize {
        Self {
            layout_widgets,
            bounds: WIDGET.bounds(),
            nest_group: LayoutNestGroup::Inner,
            inline: None,
            child_count: None,
        }
        .with_widget(layout)
    }

    /// Start a parallel layout.
    ///
    /// Returns an instance that can be used to acquire multiple mutable [`WidgetLayout`] during layout.
    /// The [`parallel_fold`] method must be called after the parallel processing is done.
    ///
    /// Must be called outside of the [child] scope.
    ///
    /// [child]: Self::with_child
    /// [`parallel_fold`]: Self::parallel_fold
    pub fn parallel_split(&self) -> ParallelBuilder<WidgetLayout> {
        if self.nest_group != LayoutNestGroup::Child && WIDGET.parent_id().is_some() {
            tracing::error!("called `parallel_split` outside child scope");
        }
        ParallelBuilder(Some(WidgetLayout {
            layout_widgets: self.layout_widgets.clone(),
            bounds: self.bounds.clone(),
            nest_group: LayoutNestGroup::Child,
            inline: None,
            child_count: None,
        }))
    }

    /// Collect the parallel changes back.
    pub fn parallel_fold(&mut self, mut split: ParallelBuilder<WidgetLayout>) {
        let folded = split.take();
        assert_eq!(self.bounds, folded.bounds);

        let count = self.child_count.unwrap_or(0) + folded.child_count.unwrap_or(0);
        self.child_count = Some(count);
    }

    /// Defines a widget scope, translations inside `layout` target the widget's inner offset.
    ///
    /// If the widget layout is not invalidated and none of the used metrics have changed skips calling
    /// `layout` and returns the current outer-size, the outer transform is still updated.
    ///
    /// The default widget constructor calls this, see [`widget_base::nodes::widget`].
    ///
    /// [`widget_base::nodes::widget`]: crate::widget_base::nodes::widget
    pub fn with_widget(&mut self, layout: impl FnOnce(&mut Self) -> PxSize) -> PxSize {
        let metrics = LAYOUT.metrics();
        let bounds = WIDGET.bounds();

        let snap = metrics.snapshot();
        if let Some(child_count) = &mut self.child_count {
            *child_count += 1;
        }

        if !WIDGET.take_update(UpdateFlags::LAYOUT) && !self.layout_widgets.delivery_list().enter_widget(WIDGET.id()) {
            // layout not invalidated by request
            let uses = bounds.metrics_used();
            if bounds.metrics().map(|m| m.masked_eq(&snap, uses)).unwrap_or(false) {
                // layout not invalidated by used metrics
                LAYOUT.register_metrics_use(uses); // propagate to parent
                return bounds.outer_size();
            }
        }

        let parent_child_count = self.child_count.take();
        let parent_inline = self.inline.take();
        if LAYOUT.inline_constraints().is_some() && bounds.measure_inline().is_some() {
            // inline enabled by parent and widget
            self.inline = bounds.take_inline();
            if let Some(inline) = self.inline.as_mut() {
                inline.clear();
            } else {
                self.inline = Some(Default::default());
            }
        }
        let parent_bounds = mem::replace(&mut self.bounds, bounds);
        self.nest_group = LayoutNestGroup::Inner;
        let prev_inner_offset = self.bounds.inner_offset();
        let prev_child_offset = self.bounds.child_offset();
        let prev_baseline = self.bounds.baseline();
        let prev_inner_offset_baseline = self.bounds.inner_offset_baseline();
        let prev_can_auto_hide = self.bounds.can_auto_hide();
        self.bounds.set_inner_offset(PxVector::zero());
        self.bounds.set_child_offset(PxVector::zero());
        self.bounds.set_baseline(Px(0));
        self.bounds.set_inner_offset_baseline(false);
        self.bounds.set_can_auto_hide(true);

        // layout
        let (uses, size) = LAYOUT.capture_metrics_use(|| layout(self));

        LAYOUT.register_metrics_use(uses);
        self.bounds.set_outer_size(size);
        self.bounds.set_metrics(Some(snap), uses);
        if let Some(inline) = &mut self.inline {
            inline.inner_size = self.bounds.inner_size();
            inline.invalidate_negative_space();
        }
        self.bounds.set_inline(self.inline.take());

        if prev_can_auto_hide != self.bounds.can_auto_hide() {
            WIDGET.render();
        } else if prev_inner_offset != self.bounds.inner_offset()
            || prev_child_offset != self.bounds.child_offset()
            || prev_inner_offset_baseline != self.bounds.inner_offset_baseline()
            || (self.bounds.inner_offset_baseline() && prev_baseline != self.bounds.baseline())
        {
            WIDGET.render_update();
        }

        self.child_count = parent_child_count;
        self.inline = parent_inline;
        self.bounds = parent_bounds;
        self.nest_group = LayoutNestGroup::Child;

        size
    }

    /// Calls `layout` with inline force enabled on the widget.
    ///
    /// The widget will use the inline visual even if the parent did not inline it, but it will not
    /// inline if it has properties that disable inlining.
    pub fn with_inline_visual(&mut self, layout: impl FnOnce(&mut Self) -> PxSize) -> PxSize {
        if self.is_inline() {
            let size = layout(self);
            WIDGET.bounds().set_inline(self.inline.clone());
            size
        } else {
            let bounds = WIDGET.bounds();
            if let Some(measure) = bounds.measure_inline() {
                let constraints = InlineConstraintsLayout {
                    first: PxRect::from_size(measure.first),
                    mid_clear: Px(0),
                    last: {
                        let mut r = PxRect::from_size(measure.last);
                        r.origin.y = bounds.measure_outer_size().height - measure.last.height;
                        r
                    },
                    first_segs: Arc::new(vec![]),
                    last_segs: Arc::new(vec![]),
                };

                self.inline = Some(Default::default());

                let metrics = LAYOUT
                    .metrics()
                    .with_inline_constraints(Some(InlineConstraints::Layout(constraints)));
                let size = LAYOUT.with_context(metrics, || layout(self));

                bounds.set_inline(self.inline.clone());
                size
            } else {
                layout(self)
            }
        }
    }

    /// Defines a widget inner scope, translations inside `layout` target the widget's child offset.
    ///
    /// This method also updates the border info.
    ///
    /// The default widget borders constructor calls this, see [`widget_base::nodes::widget_inner`].
    ///
    /// [`widget_base::nodes::widget_inner`]: crate::widget_base::nodes::widget_inner
    pub fn with_inner(&mut self, layout: impl FnOnce(&mut Self) -> PxSize) -> PxSize {
        self.nest_group = LayoutNestGroup::Child;
        let size = BORDER.with_inner(|| layout(self));
        WIDGET.bounds().set_inner_size(size);
        self.nest_group = LayoutNestGroup::Inner;
        size
    }

    /// Defines a widget child scope, translations inside `layout` still target the widget's child offset.
    ///
    /// Returns the child size and if a reference frame is required to offset the child.
    ///
    /// The default widget child layout constructor implements this, see [`widget_base::nodes::widget_child`].
    ///
    /// [`widget_base::nodes::widget_child`]: crate::widget_base::nodes::widget_child
    /// [`child_offset`]: WidgetBoundsInfo::child_offset
    pub fn with_child(&mut self, layout: impl FnOnce(&mut Self) -> PxSize) -> (PxSize, bool) {
        let parent_child_count = mem::replace(&mut self.child_count, Some(0));

        self.nest_group = LayoutNestGroup::Child;
        let child_size = layout(self);
        self.nest_group = LayoutNestGroup::Child;

        let need_ref_frame = self.child_count != Some(1);
        self.child_count = parent_child_count;
        (child_size, need_ref_frame)
    }

    /// Adds the `offset` to the closest *inner* bounds offset.
    ///
    /// This affects the inner offset if called from a node inside the widget and before the `BORDER` group, or it affects
    /// the child offset if called inside the widget and inside the `BORDER` group.
    pub fn translate(&mut self, offset: PxVector) {
        match self.nest_group {
            LayoutNestGroup::Inner => {
                let mut o = self.bounds.inner_offset();
                o += offset;
                self.bounds.set_inner_offset(o);
            }
            LayoutNestGroup::Child => {
                let mut o = self.bounds.child_offset();
                o += offset;
                self.bounds.set_child_offset(o);
            }
        }
    }

    /// Set the baseline offset of the widget. The value is up from the bottom of the inner bounds.
    pub fn set_baseline(&mut self, baseline: Px) {
        self.bounds.set_baseline(baseline);
    }

    /// Set if the baseline is added to the inner offset  *y* axis.
    pub fn translate_baseline(&mut self, enabled: bool) {
        self.bounds.set_inner_offset_baseline(enabled);
    }

    /// Sets if the widget only renders if [`outer_bounds`] intersects with the [`FrameBuilder::auto_hide_rect`].
    ///
    /// This is `true` by default.
    ///
    /// [`outer_bounds`]: WidgetBoundsInfo::outer_bounds
    /// [`FrameBuilder::auto_hide_rect`]: crate::render::FrameBuilder::auto_hide_rect
    pub fn allow_auto_hide(&mut self, enabled: bool) {
        self.bounds.set_can_auto_hide(enabled);
    }

    /// Collapse the layout of `self` and descendants, the size and offsets are set to zero.
    ///
    /// Nodes that set the visibility to the equivalent of [`Collapsed`] must skip layout and return [`PxSize::zero`] as
    /// the the size, ignoring the min-size constraints, and call this method to update all the descendant
    /// bounds information to be a zero-sized point.
    ///
    /// Note that the widget will automatically not be rendered when collapsed.
    ///
    /// [`Collapsed`]: Visibility::Collapsed
    pub fn collapse(&mut self) {
        let tree = WINDOW.widget_tree();
        let id = WIDGET.id();
        if let Some(w) = tree.get(id) {
            for w in w.self_and_descendants() {
                let info = w.info();
                info.bounds_info.set_outer_size(PxSize::zero());
                info.bounds_info.set_inner_size(PxSize::zero());
                info.bounds_info.set_baseline(Px(0));
                info.bounds_info.set_inner_offset_baseline(false);
                info.bounds_info.set_can_auto_hide(true);
                info.bounds_info.set_inner_offset(PxVector::zero());
                info.bounds_info.set_child_offset(PxVector::zero());
                info.bounds_info.set_measure_metrics(None, LayoutMask::empty());
                info.bounds_info.set_metrics(None, LayoutMask::empty());
                info.bounds_info.set_is_collapsed(true);
                info.bounds_info.set_rendered(None, &tree);
            }
        } else {
            tracing::error!("collapse did not find `{}` in the info tree", id)
        }
    }

    /// Collapse layout of all descendants, the size and offsets are set to zero.
    ///
    /// Widgets that control the visibility of their children can use this method and then, in the same layout pass, layout
    /// the children that should be visible.
    ///
    /// Note that the widgets will automatically not be rendered when collapsed.
    ///
    /// [`Collapsed`]: Visibility::Collapsed
    pub fn collapse_descendants(&mut self) {
        let tree = WINDOW.widget_tree();
        let id = WIDGET.id();
        if let Some(w) = tree.get(id) {
            for w in w.descendants() {
                let info = w.info();
                info.bounds_info.set_outer_size(PxSize::zero());
                info.bounds_info.set_inner_size(PxSize::zero());
                info.bounds_info.set_baseline(Px(0));
                info.bounds_info.set_inner_offset_baseline(false);
                info.bounds_info.set_can_auto_hide(true);
                info.bounds_info.set_inner_offset(PxVector::zero());
                info.bounds_info.set_child_offset(PxVector::zero());
                info.bounds_info.set_measure_metrics(None, LayoutMask::empty());
                info.bounds_info.set_metrics(None, LayoutMask::empty());
                info.bounds_info.set_is_collapsed(true);
            }
        } else {
            tracing::error!("collapse_descendants did not find `{}` in the info tree", id)
        }
    }

    /// Collapse layout of the child and all its descendants, the size and offsets are set to zero.
    ///
    /// Widgets that control the visibility of their children can use this method and then, in the same layout pass, layout
    /// the children that should be visible.
    ///
    /// Note that the widgets will automatically not be rendered when collapsed.
    ///
    /// [`Collapsed`]: Visibility::Collapsed
    pub fn collapse_child(&mut self, index: usize) {
        let tree = WINDOW.widget_tree();
        let id = WIDGET.id();
        if let Some(w) = tree.get(id) {
            if let Some(w) = w.children().nth(index) {
                for w in w.self_and_descendants() {
                    let info = w.info();
                    info.bounds_info.set_outer_size(PxSize::zero());
                    info.bounds_info.set_inner_size(PxSize::zero());
                    info.bounds_info.set_baseline(Px(0));
                    info.bounds_info.set_inner_offset_baseline(false);
                    info.bounds_info.set_can_auto_hide(true);
                    info.bounds_info.set_inner_offset(PxVector::zero());
                    info.bounds_info.set_child_offset(PxVector::zero());
                    info.bounds_info.set_measure_metrics(None, LayoutMask::empty());
                    info.bounds_info.set_metrics(None, LayoutMask::empty());
                    info.bounds_info.set_is_collapsed(true);
                }
            } else {
                tracing::error!(
                    "collapse_child out-of-bounds for `{}` in the children of `{}` in the info tree",
                    index,
                    id
                )
            }
        } else {
            tracing::error!("collapse_child did not find `{}` in the info tree", id)
        }
    }

    /// If the parent widget is doing inline layout and this widget signaled that it can support this
    /// during measure.
    ///
    /// See [`WidgetMeasure::inline`] for more details.
    pub fn is_inline(&self) -> bool {
        self.inline.is_some()
    }

    /// Mutable reference to the current widget's inline info.
    ///
    /// This is `Some(_)` if the parent widget is doing inline layout and this widget signaled that it can be inlined
    /// in the previous measure pass. You can use [`LAYOUT.with_inline_measure`] in the measure pass to disable
    /// inline in both passes, measure and layout.
    ///
    /// The rows and negative space are already reset when widget layout started, and the inner size will be updated when
    /// the widget layout ends, the inline layout node only needs to push rows.
    ///
    /// When this is `Some(_)` the [`LayoutMetrics::inline_constraints`] is also `Some(_)`.
    ///
    /// See [`WidgetInlineInfo`] for more details.
    ///
    /// [`LayoutMetrics::inline_constraints`]: crate::context::LayoutMetrics::inline_constraints
    pub fn inline(&mut self) -> Option<&mut WidgetInlineInfo> {
        self.inline.as_mut()
    }

    /// Create an [`WidgetMeasure`] for an [`UiNode::measure`] call.
    ///
    /// [`UiNode::measure`]: crate::widget_instance::UiNode::measure
    pub fn to_measure(&self, inline: Option<WidgetInlineMeasure>) -> WidgetMeasure {
        WidgetMeasure {
            layout_widgets: self.layout_widgets.clone(),
            inline,
            inline_locked: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum LayoutNestGroup {
    /// Inside widget, outside `BORDER`.
    Inner,
    /// Inside `BORDER`.
    Child,
}

/// Represents a builder split from the main builder that must be folded back onto the
/// main builder after it is filled in a parallel task.
///
/// # Error
///
/// Traces an error on drop if it was not moved to the `B::parallel_fold` method.
#[must_use = "use in parallel task, then move it to `B::parallel_fold`"]
pub struct ParallelBuilder<B>(pub(crate) Option<B>);
impl<B> ParallelBuilder<B> {
    pub(crate) fn take(&mut self) -> B {
        self.0.take().expect("parallel builder finished")
    }
}
impl<B> ops::Deref for ParallelBuilder<B> {
    type Target = B;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("parallel builder finished")
    }
}
impl<B> ops::DerefMut for ParallelBuilder<B> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().expect("parallel builder finished")
    }
}
impl<B> Drop for ParallelBuilder<B> {
    fn drop(&mut self) {
        if self.0.is_some() && !std::thread::panicking() {
            tracing::error!("builder dropped without calling `{}::parallel_fold`", std::any::type_name::<B>())
        }
    }
}
