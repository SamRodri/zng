//! Widget info tree.

use std::cell::Cell;
use std::fmt;
use std::mem;
use std::ops;
use std::rc::Rc;

use ego_tree::Tree;

use crate::context::LayoutMetrics;
use crate::context::Updates;
use crate::crate_util::IdMap;
use crate::event::EventUpdateArgs;
use crate::handler::WidgetHandler;
use crate::state::OwnedStateMap;
use crate::state::StateMap;
use crate::units::*;
use crate::var::Var;
use crate::var::VarValue;
use crate::var::VarsRead;
use crate::var::WithVarsRead;
use crate::widget_base::Visibility;
use crate::window::WindowId;
use crate::WidgetId;

unique_id_64! {
    /// Identifies a [`WidgetInfoTree`] snapshot, can be use for more speedy [`WidgetPath`] resolution.
    struct WidgetInfoTreeId;
}

/// Represents the in-progress layout arrange pass for an widget.
pub struct WidgetLayout {
    global_transform: RenderTransform,

    pre_translate: PxVector,
    transform: RenderTransform,
    transform_origin: Point,
    inner_bounds: BoundsInfo,
}
impl WidgetLayout {
    /// Start the layout arrange pass from the window root widget.
    pub fn with_root_widget(outer_bounds: &BoundsInfo, inner_bounds: &BoundsInfo, final_size: PxSize, f: impl FnOnce(&mut Self)) {
        let mut self_ = Self {
            global_transform: RenderTransform::identity(),
            pre_translate: PxVector::zero(),
            transform: RenderTransform::identity(),
            transform_origin: Point::center(),
            inner_bounds: inner_bounds.clone(),
        };
        self_.with_widget(outer_bounds, inner_bounds, final_size, f);
    }

    /// Mark the widget outer-boundaries.
    ///
    /// Must be called in the widget `new`, the [`implicit_base::new`] node does this.
    ///
    /// [`implicit_base::new`]: crate::widget_base::implicit_base::new
    pub fn with_widget(&mut self, outer_bounds: &BoundsInfo, inner_bounds: &BoundsInfo, final_size: PxSize, f: impl FnOnce(&mut Self)) {
        outer_bounds.set_size(final_size);
        // includes offsets from properties line "content_align".
        outer_bounds.set_transform(self.global_transform.then_translate(euclid::vec3(
            self.pre_translate.x.0 as f32,
            self.pre_translate.y.0 as f32,
            0.0,
        )));

        let pre_inner_bounds = mem::replace(&mut self.inner_bounds, inner_bounds.clone());

        f(self);

        self.inner_bounds = pre_inner_bounds;
    }

    /// Register a custom `transform` that is applied by the current UI node during render.
    ///
    /// Nodes that declare [reference frames] during render must also use this method.
    ///
    /// [reference frames]: crate::render::FrameBuilder::push_reference_frame
    pub fn with_custom_transform(&mut self, transform: &RenderTransform, f: impl FnOnce(&mut Self)) {
        let global_transform = self.global_transform.then(transform);
        let prev_global_transform = mem::replace(&mut self.global_transform, global_transform);
        f(self);
        self.global_transform = prev_global_transform;
    }

    /// Shortcut for declaring a custom translate transform.
    pub fn with_custom_translate(&mut self, offset: PxVector, f: impl FnOnce(&mut Self)) {
        let transform = RenderTransform::translation_px(offset);
        self.with_custom_transform(&transform, f);
    }

    /// Appends a translate transform that will be applied to the next inner bounds before the other inner transforms.
    pub fn with_pre_translate(&mut self, offset: PxVector, f: impl FnOnce(&mut Self)) {
        self.pre_translate += offset;
        f(self);
        self.pre_translate -= offset;
    }

    /// Appends a transform that will be applied to the next inner bounds.
    pub fn with_inner_transform(&mut self, transform: &RenderTransform, f: impl FnOnce(&mut Self)) {
        let transform = self.transform.then(transform);
        let prev_transform = mem::replace(&mut self.transform, transform);
        f(self);
        self.transform = prev_transform;
    }

    /// Sets the *center point* of the inner transform that will be applied to the next inner bounds.
    ///
    /// The `origin` will be resolved in the layout context of the inner bounds final size.
    pub fn with_inner_transform_origin(&mut self, origin: &Point, f: impl FnOnce(&mut Self)) {
        let prev_origin = mem::replace(&mut self.transform_origin, origin.clone());
        f(self);
        self.transform_origin = prev_origin;
    }

    /// Mark the widget inner-boundaries.
    ///
    /// Must be called in the widget `new_inner`, the [`implicit_base::new_inner`] node does this.
    ///
    /// Returns the inner transform in the space of the outer bounds, the `new_inner` node must pass this value to [`FrameBuilder::push_inner`].
    ///
    /// [`implicit_base::new_inner`]: crate::widget_base::implicit_base::new_inner
    /// [`FrameBuilder::push_inner`]: crate::render::FrameBuilder::push_inner
    pub fn with_inner(&mut self, metrics: &LayoutMetrics, final_size: PxSize, f: impl FnOnce(&mut Self)) -> RenderTransform {
        let mut transform = self.transform;
        let transform_origin = self.transform_origin.to_layout(
            metrics,
            AvailableSize::finite(final_size),
            PxPoint::new(final_size.width / 2, final_size.height / 2),
        );
        if transform_origin != PxPoint::zero() {
            let x = transform_origin.x.0 as f32;
            let y = transform_origin.y.0 as f32;
            transform = RenderTransform::translation(-x, -y, 0.0)
                .then(&transform)
                .then_translate(euclid::vec3(x, y, 0.0));
        }

        if self.pre_translate != PxVector::zero() {
            transform = RenderTransform::translation_px(self.pre_translate).then(&transform);
        }

        let global_transform = self.global_transform.then(&transform);
        let prev_global_transform = mem::replace(&mut self.global_transform, global_transform);

        self.inner_bounds.set_size(final_size);
        self.inner_bounds.set_transform(self.global_transform);

        let prev_pre_translate = mem::take(&mut self.pre_translate);
        let prev_transform = mem::take(&mut self.transform);
        let prev_transform_origin = mem::replace(&mut self.transform_origin, Point::center());

        f(self);

        self.pre_translate = prev_pre_translate;
        self.transform = prev_transform;
        self.transform_origin = prev_transform_origin;

        self.global_transform = prev_global_transform;

        transform
    }
}

/// [`WidgetInfoTree`] builder.
pub struct WidgetInfoBuilder {
    window_id: WindowId,

    node: ego_tree::NodeId,
    widget_id: WidgetId,
    meta: OwnedStateMap,

    tree: Tree<WidgetInfoInner>,
    interactive_filter: Vec<Box<dyn Fn(&InteractiveFilterArgs) -> bool>>,
}
impl WidgetInfoBuilder {
    /// Starts building a info tree with the root information, the `root_bounds` must be a shared reference
    /// to the size of the window content that is always kept up-to-date, the transform must be always identity.
    #[inline]
    pub fn new(
        window_id: WindowId,
        root_id: WidgetId,
        root_bounds: BoundsInfo,
        rendered: WidgetRendered,
        used_data: Option<UsedWidgetInfoBuilder>,
    ) -> Self {
        debug_assert_eq!(RenderTransform::identity(), root_bounds.transform());

        let (tree_capacity, interactive_capacity) = used_data.map(|d| (d.tree_capacity, d.interactive_capacity)).unwrap_or((100, 30));
        let tree = Tree::with_capacity(
            WidgetInfoInner {
                widget_id: root_id,
                inner_bounds: root_bounds.clone(),
                outer_bounds: root_bounds,
                rendered,
                meta: OwnedStateMap::new(),
            },
            tree_capacity,
        );

        let root_node = tree.root().id();
        WidgetInfoBuilder {
            window_id,
            node: root_node,
            tree,
            interactive_filter: Vec::with_capacity(interactive_capacity),
            meta: OwnedStateMap::new(),
            widget_id: root_id,
        }
    }

    #[inline]
    fn node(&mut self, id: ego_tree::NodeId) -> ego_tree::NodeMut<WidgetInfoInner> {
        self.tree.get_mut(id).unwrap()
    }

    /// Current widget id.
    #[inline]
    pub fn widget_id(&self) -> WidgetId {
        self.widget_id
    }

    /// Current widget metadata.
    #[inline]
    pub fn meta(&mut self) -> &mut StateMap {
        &mut self.meta.0
    }

    /// Calls `f` in a new widget context.
    ///
    /// Both `outer_bounds` and `bounds` must be a shared reference to rectangles that are updated every layout.
    #[inline]
    pub fn push_widget(
        &mut self,
        id: WidgetId,
        outer_bounds: BoundsInfo,
        inner_bounds: BoundsInfo,
        rendered: WidgetRendered,
        f: impl FnOnce(&mut Self),
    ) {
        let parent_node = self.node;
        let parent_widget_id = self.widget_id;
        let parent_meta = mem::take(&mut self.meta);

        self.widget_id = id;
        self.node = self
            .node(parent_node)
            .append(WidgetInfoInner {
                widget_id: id,
                inner_bounds,
                outer_bounds,
                rendered,
                meta: OwnedStateMap::new(),
            })
            .id();

        f(self);

        self.node(self.node).value().meta = mem::replace(&mut self.meta, parent_meta);
        self.node = parent_node;
        self.widget_id = parent_widget_id;
    }

    /// Register a closure that returns `true` if the widget is interactive or `false` if it is not.
    ///
    /// Widgets [`allow_interaction`] if all registered closures allow it.
    ///
    /// [`allow_interaction`]: WidgetInfo::allow_interaction
    pub fn push_interactive_filter(&mut self, filter: impl Fn(&InteractiveFilterArgs) -> bool + 'static) {
        self.interactive_filter.push(Box::new(filter))
    }

    /// Build the info tree.
    #[inline]
    pub fn finalize(mut self) -> (WidgetInfoTree, UsedWidgetInfoBuilder) {
        self.tree.root_mut().value().meta = self.meta;
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

        let r = WidgetInfoTree(Rc::new(WidgetInfoTreeInner {
            id: WidgetInfoTreeId::new_unique(),
            window_id: self.window_id,
            lookup,
            tree: self.tree,
            interactive_filter: self.interactive_filter,
        }));

        #[cfg(debug_assertions)]
        for (widget_id, repeats) in repeats {
            tracing::error!(target: "render", "widget id `{widget_id:?}` appears more then once in {:?}{}", self.window_id, {
                let mut places = String::new();
                use std::fmt::Write;
                for node in &repeats {
                    let info = WidgetInfo::new(&r, *node);
                    write!(places, "\n    {}", info.path()).unwrap();
                }
                places
            });
        }

        let cap = UsedWidgetInfoBuilder {
            tree_capacity: r.0.lookup.capacity(),
            interactive_capacity: r.0.interactive_filter.len(),
        };

        (r, cap)
    }
}

/// A tree of [`WidgetInfo`].
///
/// The tree is behind an `Rc` pointer so cloning and storing this type is very cheap.
///
/// Instantiated using [`WidgetInfoBuilder`].
#[derive(Clone)]
pub struct WidgetInfoTree(Rc<WidgetInfoTreeInner>);
struct WidgetInfoTreeInner {
    id: WidgetInfoTreeId,
    window_id: WindowId,
    tree: Tree<WidgetInfoInner>,
    lookup: IdMap<WidgetId, ego_tree::NodeId>,
    interactive_filter: Vec<Box<dyn Fn(&InteractiveFilterArgs) -> bool>>,
}
impl WidgetInfoTree {
    /// Blank window that contains only the root widget taking no space.
    #[inline]
    pub fn blank(window_id: WindowId, root_id: WidgetId) -> Self {
        WidgetInfoBuilder::new(window_id, root_id, BoundsInfo::new(), WidgetRendered::new(), None)
            .finalize()
            .0
    }

    /// Reference to the root widget in the tree.
    #[inline]
    pub fn root(&self) -> WidgetInfo {
        WidgetInfo::new(self, self.0.tree.root().id())
    }

    /// All widgets including `root`.
    #[inline]
    pub fn all_widgets(&self) -> impl Iterator<Item = WidgetInfo> {
        self.0.tree.root().descendants().map(move |n| WidgetInfo::new(self, n.id()))
    }

    /// Id of the window that owns all widgets represented in the tree.
    #[inline]
    pub fn window_id(&self) -> WindowId {
        self.0.window_id
    }

    /// Reference to the widget in the tree, if it is present.
    #[inline]
    pub fn find(&self, widget_id: WidgetId) -> Option<WidgetInfo> {
        self.0
            .lookup
            .get(&widget_id)
            .and_then(|i| self.0.tree.get(*i).map(|n| WidgetInfo::new(self, n.id())))
    }

    /// If the tree contains the widget.
    #[inline]
    pub fn contains(&self, widget_id: WidgetId) -> bool {
        self.0.lookup.contains_key(&widget_id)
    }

    /// Reference to the widget in the tree, if it is present.
    ///
    /// Faster then [`find`](Self::find) if the widget path was generated by `self`.
    #[inline]
    pub fn get(&self, path: &WidgetPath) -> Option<WidgetInfo> {
        if let Some((tree_id, id)) = path.node_id {
            if tree_id == self.0.id {
                return self.0.tree.get(id).map(|n| WidgetInfo::new(self, n.id()));
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
impl fmt::Debug for WidgetInfoTree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let nl = if f.alternate() { "\n   " } else { " " };

        write!(
            f,
            "WidgetInfoTree(Rc<{{{nl}id: {},{nl}window_id: {},{nl}widget_count: {},{nl}...}}>)",
            self.0.id.sequential(),
            self.0.window_id,
            self.0.lookup.len(),
            nl = nl
        )
    }
}

/// Full address of a widget in a specific [`WidgetInfoTree`].
#[derive(Clone)]
pub struct WidgetPath {
    node_id: Option<(WidgetInfoTreeId, ego_tree::NodeId)>,
    window_id: WindowId,
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
                .finish_non_exhaustive()
        } else {
            write!(f, "{self}")
        }
    }
}
impl fmt::Display for WidgetPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}//", self.window_id)?;
        for w in self.ancestors() {
            write!(f, "{w}/")?;
        }
        write!(f, "{}", self.widget_id())
    }
}
impl WidgetPath {
    /// New custom widget path.
    ///
    /// The path is not guaranteed to have ever existed.
    pub fn new<P: Into<Box<[WidgetId]>>>(window_id: WindowId, path: P) -> WidgetPath {
        WidgetPath {
            node_id: None,
            window_id,
            path: path.into(),
        }
    }

    /// Id of the window that contains the widgets.
    #[inline]
    pub fn window_id(&self) -> WindowId {
        self.window_id
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
            path: self.path[..i].iter().copied().collect(),
        })
    }

    /// Get the inner most widget parent shared by both `self` and `other`.
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
            path: Box::new([self.path[0]]),
        }
    }
}

#[derive(Default, Debug)]
struct BoundsData {
    transform: Cell<RenderTransform>,
    size: Cell<PxSize>,
}

/// Shared reference to the transform and size of a [`WidgetInfo`] outer or inner bounds.
#[derive(Default, Clone, Debug)]
pub struct BoundsInfo(Rc<BoundsData>);
impl BoundsInfo {
    /// New default.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// New with a size.
    #[inline]
    pub fn from_size(final_size: PxSize) -> Self {
        let self_ = BoundsInfo::new();
        self_.set_size(final_size);
        self_
    }

    /// Get a copy of the current transform.
    ///
    /// The origin is the window root top-left.
    #[inline]
    pub fn transform(&self) -> RenderTransform {
        self.0.transform.get()
    }

    /// Set the current transform.
    #[inline]
    fn set_transform(&self, transform: RenderTransform) {
        self.0.transform.set(transform)
    }

    /// Get a copy of the current raw size.
    ///
    /// Note that this is not transformed.
    #[inline]
    pub fn size(&self) -> PxSize {
        self.0.size.get()
    }

    /// Set the current raw size.
    #[inline]
    fn set_size(&self, size: PxSize) {
        self.0.size.set(size)
    }

    /// Calculate the bounding box.
    pub fn bounds(&self) -> PxRect {
        Self::bounds_impl(self.transform(), self.size())
    }

    /// Compute the transform in the context of a parent transform.
    ///
    /// Returns `None` if the `parent_transform` is not invertible.
    pub fn local_transform(&self, parent_transform: &RenderTransform) -> Option<RenderTransform> {
        parent_transform.inverse().map(|m| m.then(&self.transform()))
    }

    /// Compute the bounding box in the context of a parent transform.
    ///
    /// Returns `None` if the `parent_transform` is not invertible.
    pub fn local_bounds(&self, parent_transform: &RenderTransform) -> Option<PxRect> {
        self.local_transform(parent_transform).map(|m| Self::bounds_impl(m, self.size()))
    }

    fn bounds_impl(transform: RenderTransform, size: PxSize) -> PxRect {
        let rect = PxRect::from_size(size).to_wr();
        // TODO: Verify that this unwrap will never panic.
        let bounds = transform.outer_transformed_box2d(&rect).unwrap();
        bounds.to_px()
    }
}

/// Shared reference to the rendered status of a [`WidgetInfo`].
///
/// This status is updated every [`render`] without causing a tree rebuild.
///
/// [`render`]: crate::UiNode::render
#[derive(Default, Clone, Debug)]
pub struct WidgetRendered(Rc<Cell<bool>>);
impl WidgetRendered {
    /// New default.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Get if the widget or child widgets rendered.
    #[inline]
    pub fn get(&self) -> bool {
        self.0.get()
    }

    /// Set if the widget or child widgets rendered.
    #[inline]
    pub(super) fn set(&self, rendered: bool) {
        self.0.set(rendered);
    }
}

struct WidgetInfoInner {
    widget_id: WidgetId,
    outer_bounds: BoundsInfo,
    inner_bounds: BoundsInfo,
    rendered: WidgetRendered,
    meta: OwnedStateMap,
}

/// Reference to a widget info in a [`WidgetInfoTree`].
#[derive(Clone, Copy)]
pub struct WidgetInfo<'a> {
    tree: &'a WidgetInfoTree,
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
            .field("[path]", &self.path().to_string())
            .field("[meta]", self.meta())
            .finish()
    }
}

impl<'a> WidgetInfo<'a> {
    #[inline]
    fn new(tree: &'a WidgetInfoTree, node_id: ego_tree::NodeId) -> Self {
        Self { tree, node_id }
    }

    #[inline]
    fn node(&self) -> ego_tree::NodeRef<'a, WidgetInfoInner> {
        unsafe { self.tree.0.tree.get_unchecked(self.node_id) }
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
            window_id: self.tree.0.window_id,
            node_id: Some((self.tree.0.id, self.node_id)),
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

    /// Returns `true` if the widget or the widget's descendants rendered in the last frame.
    ///
    /// This value is updated every [`render`] without causing a tree rebuild.
    ///
    /// [`render`]: crate::UiNode::render
    #[inline]
    pub fn rendered(self) -> bool {
        // widgets that don't render tend to not call `UiNode::render` on children,
        // so we need to check all parents because our flag can be out-of-date.
        self.info().rendered.get() && self.ancestors().all(|p| p.info().rendered.get())
    }

    /// Compute the visibility of the widget or the widget's descendants.
    ///
    /// If is [`rendered`] is [`Visible`], if not and the [`outer_bounds`] size is zero then is [`Collapsed`] else
    /// is [`Hidden`].
    ///
    /// [`rendered`]: Self::rendered
    /// [`Visible`]: Visibility::Visible
    /// [`outer_bounds`]: Self::outer_bounds
    /// [`Collapsed`]: Visibility::Collapsed
    /// [`Hidden`]: Visibility::Hidden
    #[inline]
    pub fn visibility(self) -> Visibility {
        if self.rendered() {
            Visibility::Visible
        } else if self.outer_bounds().size == PxSize::zero() {
            Visibility::Collapsed
        } else {
            Visibility::Hidden
        }
    }

    /// Returns `true` if interaction with this widget is allowed by all interactive filters.
    ///
    /// If `false` interaction behavior implementers must consider this widget *disabled*, disabled widgets do not receive keyboard
    /// or pointer events but can block hit-test if rendered above others.
    ///
    /// Note that not only [disabled] widgets can return `false` here, but only [disabled] widgets visually indicate that they are disabled.
    /// An example of a widget that is [enabled] but not interactive is one outside of a *modal overlay*.
    ///
    /// [disabled]: fn@crate::widget_base::enabled
    /// [enabled]: fn@crate::widget_base::enabled
    pub fn allow_interaction(self) -> bool {
        for filter in &self.tree.0.interactive_filter {
            if !filter(&InteractiveFilterArgs { info: self }) {
                return false;
            }
        }
        true
    }

    /// Clone a reference to the widget outer bounds information.
    ///
    /// This information is up-to-date, it is updated every layout without causing a tree rebuild.
    #[inline]
    pub fn outer_info(self) -> BoundsInfo {
        self.info().outer_bounds.clone()
    }

    /// Clone a reference to the widget inner bounds information.
    ///
    /// This information is up-to-date, it is updated every layout without causing a tree rebuild.
    #[inline]
    pub fn inner_info(self) -> BoundsInfo {
        self.info().outer_bounds.clone()
    }

    /// Side of the widget outer area, not transformed.
    ///
    /// Returns an up-to-date size, the size is updated every layout without causing a tree rebuild.
    #[inline]
    pub fn outer_final_size(self) -> PxSize {
        self.info().outer_bounds.size()
    }

    /// Side of the widget inner area, not transformed.
    ///
    /// Returns an up-to-date size, the size is updated every layout without causing a tree rebuild.
    #[inline]
    pub fn inner_final_size(self) -> PxSize {
        self.info().inner_bounds.size()
    }

    /// Widget outer transform in the window space, before its own transforms are applied.
    ///
    /// Returns an up-to-date transform, the transform is updated every layout without causing a tree rebuild.
    #[inline]
    pub fn outer_transform(self) -> RenderTransform {
        self.info().outer_bounds.transform()
    }

    /// Widget outer transform in the `parent` space.
    ///
    /// Returns `None` if `parent` is not invertible.
    #[inline]
    pub fn outer_transform_in(self, parent: &RenderTransform) -> Option<RenderTransform> {
        self.info().outer_bounds.local_transform(parent)
    }

    /// Widget transform in the window space, including its own transforms.
    ///
    /// Returns an up-to-date transform, the transform is updated every layout without causing a tree rebuild.
    #[inline]
    pub fn inner_transform(self) -> RenderTransform {
        self.info().inner_bounds.transform()
    }

    /// Widget inner transform in the `parent` space.
    ///
    /// Returns `None` if `parent` is not invertible.
    #[inline]
    pub fn inner_transform_in(self, parent: &RenderTransform) -> Option<RenderTransform> {
        self.info().inner_bounds.local_transform(parent)
    }

    /// Widget rectangle in the window space, including *outer* properties like margin.
    ///
    /// Returns an up-to-date rect, the bounds are updated every layout without causing a tree rebuild.
    #[inline]
    pub fn outer_bounds(self) -> PxRect {
        self.info().outer_bounds.bounds()
    }

    /// Widget outer bounds in the `parent` space.
    ///
    /// Returns `None` if `parent` is not invertible.
    #[inline]
    pub fn outer_bounds_in(self, parent: &RenderTransform) -> Option<PxRect> {
        self.info().outer_bounds.local_bounds(parent)
    }

    /// Widget rectangle in the window space, but only the visible *inner* properties.
    ///
    /// Returns an up-to-date rect, the bounds are updated every layout without causing a tree rebuild.
    #[inline]
    pub fn inner_bounds(self) -> PxRect {
        self.info().inner_bounds.bounds()
    }

    /// Widget inner bounds in the `parent` space.
    ///
    /// Returns `None` if `parent` is not invertible.
    #[inline]
    pub fn inner_bounds_in(self, parent: &RenderTransform) -> Option<PxRect> {
        self.info().inner_bounds.local_bounds(parent)
    }

    /// Calculate the offsets from `outer_bounds` to `bounds`.
    pub fn outer_offsets(self) -> PxSideOffsets {
        let info = self.info();
        let outer_bounds = info.outer_bounds.bounds();
        let bounds = info.inner_bounds.bounds();

        Self::calc_offsets(outer_bounds, bounds)
    }

    /// Calculate the offsets from root's `outer_bounds` to this widget's `bounds`.
    pub fn root_offsets(self) -> PxSideOffsets {
        let outer = self.root().outer_bounds();
        let inner = self.inner_bounds();

        Self::calc_offsets(outer, inner)
    }

    fn calc_offsets(outer: PxRect, inner: PxRect) -> PxSideOffsets {
        let top = outer.origin.y - inner.origin.y;
        let left = outer.origin.x - inner.origin.x;
        let right = outer.size.width - inner.size.width;
        let bottom = outer.size.height - inner.size.height;

        PxSideOffsets::new(top, right, bottom, left)
    }

    /// Widget inner bounds center in the window space.
    #[inline]
    pub fn center(self) -> PxPoint {
        self.inner_bounds().center()
    }

    /// Widget inner bounds center in the `parent` space.
    ///
    /// Returns `None` if the `parent` is not invertible.
    #[inline]
    pub fn center_in(self, parent: &RenderTransform) -> Option<PxPoint> {
        self.inner_bounds_in(parent).map(|r| r.center())
    }

    /// Metadata associated with the widget during render.
    #[inline]
    pub fn meta(self) -> &'a StateMap {
        &self.info().meta.0
    }

    /// Reference the [`WidgetInfoTree`] that owns `self`.
    #[inline]
    pub fn tree(self) -> &'a WidgetInfoTree {
        self.tree
    }

    /// Reference to the root widget.
    #[inline]
    pub fn root(self) -> Self {
        self.ancestors().last().unwrap_or(self)
    }

    /// Reference to the widget that contains this widget.
    ///
    /// Is `None` only for [`root`](WidgetInfoTree::root).
    #[inline]
    pub fn parent(self) -> Option<Self> {
        self.node().parent().map(move |n| WidgetInfo::new(self.tree, n.id()))
    }

    /// Reference to the previous widget within the same parent.
    #[inline]
    pub fn prev_sibling(self) -> Option<Self> {
        self.node().prev_sibling().map(move |n| WidgetInfo::new(self.tree, n.id()))
    }

    /// Reference to the next widget within the same parent.
    #[inline]
    pub fn next_sibling(self) -> Option<Self> {
        self.node().next_sibling().map(move |n| WidgetInfo::new(self.tree, n.id()))
    }

    /// Reference to the first widget within this widget.
    #[inline]
    pub fn first_child(self) -> Option<Self> {
        self.node().first_child().map(move |n| WidgetInfo::new(self.tree, n.id()))
    }

    /// Reference to the last widget within this widget.
    #[inline]
    pub fn last_child(self) -> Option<Self> {
        self.node().last_child().map(move |n| WidgetInfo::new(self.tree, n.id()))
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
        self.node().children().map(move |n| WidgetInfo::new(self.tree, n.id()))
    }

    /// Iterator over all widgets contained by this widget.
    #[inline]
    pub fn descendants(self) -> impl Iterator<Item = WidgetInfo<'a>> {
        //skip(1) due to ego_tree's descendants() including the node in the descendants
        self.node().descendants().skip(1).map(move |n| WidgetInfo::new(self.tree, n.id()))
    }

    /// iterator over the widget and all widgets contained by it.
    #[inline]
    pub fn self_and_descendants(self) -> impl Iterator<Item = WidgetInfo<'a>> {
        self.node().descendants().map(move |n| WidgetInfo::new(self.tree, n.id()))
    }

    /// Iterator over all widgets contained by this widget filtered by the `filter` closure.
    #[inline]
    pub fn filter_descendants<F>(self, filter: F) -> FilterDescendants<'a, F> where F: FnMut(WidgetInfo<'a>) -> DescendantFilter {
        let mut traverse = self.node().traverse();
        traverse.next(); // skip self.
        FilterDescendants {
            traverse,
            filter,
            tree: self.tree,
        }
    }

    /// Iterator over parent -> grandparent -> .. -> root.
    #[inline]
    pub fn ancestors(self) -> impl Iterator<Item = WidgetInfo<'a>> {
        self.node().ancestors().map(move |n| WidgetInfo::new(self.tree, n.id()))
    }

    /// Iterator over self -> parent -> grandparent -> .. -> root.
    #[inline]
    pub fn self_and_ancestors(self) -> impl Iterator<Item = WidgetInfo<'a>> {
        [self].into_iter().chain(self.ancestors())
    }

    /// Iterator over all previous widgets within the same parent.
    #[inline]
    pub fn prev_siblings(self) -> impl Iterator<Item = WidgetInfo<'a>> {
        self.node().prev_siblings().map(move |n| WidgetInfo::new(self.tree, n.id()))
    }

    /// Iterator over all next widgets within the same parent.
    #[inline]
    pub fn next_siblings(self) -> impl Iterator<Item = WidgetInfo<'a>> {
        self.node().next_siblings().map(move |n| WidgetInfo::new(self.tree, n.id()))
    }

    /// This widgets [`center`](Self::center) orientation in relation to a `origin`.
    #[inline]
    pub fn orientation_from(self, origin: PxPoint) -> WidgetOrientation {
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
    pub fn distance_key(self, origin: PxPoint) -> usize {
        let o = self.center();
        let a = (o.x - origin.x).0.pow(2);
        let b = (o.y - origin.y).0.pow(2);
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
    tree: &'a WidgetInfoTree,
}
impl<'a, F: FnMut(WidgetInfo<'a>) -> DescendantFilter> Iterator for FilterDescendants<'a, F> {
    type Item = WidgetInfo<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        use ego_tree::iter::Edge;
        #[allow(clippy::while_let_on_iterator)] // false positive https://github.com/rust-lang/rust-clippy/issues/7510
        while let Some(edge) = self.traverse.next() {
            if let Edge::Open(node) = edge {
                let widget = WidgetInfo::new(self.tree, node.id());
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
fn is_in_direction(direction: WidgetOrientation, origin: PxPoint, candidate: PxPoint) -> bool {
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

/// Data from a previous [`WidgetInfoBuilder`], can be reused in the next rebuild for a performance boost.
pub struct UsedWidgetInfoBuilder {
    tree_capacity: usize,
    interactive_capacity: usize,
}

macro_rules! update_slot {
    ($(
        $(#[$meta:meta])*
        $vis:vis struct $Slot:ident -> $Mask:ident;
    )+) => {$(
        $(#[$meta])*
        ///
        /// This `struct` is a single byte that represents an index in the full bitmap.
        #[derive(Clone, Copy, Debug)]
        $vis struct $Slot(u8);

        impl $Slot {
            /// Gets a slot.
            #[inline]
            pub fn next() -> Self {
                thread_local! {
                    static SLOT: Cell<u8> = Cell::new(0);
                }

                let slot = SLOT.with(|s| {
                    let slot = s.get().wrapping_add(1);
                    s.set(slot);
                    slot
                });

                Self(slot)
            }

            /// Gets a mask representing just this slot.
            #[inline]
            pub fn mask(self) -> $Mask {
                $Mask::from_slot(self)
            }
        }
    )+}
}
macro_rules! update_mask {
    ($(
        $(#[$meta:meta])*
        $vis:vis struct $Mask:ident <- $Slot:ident;
    )+) => {$(
        $(#[$meta])*
        ///
        /// This `struct` is a 256-bit bitmap of flagged slots.
        #[derive(Clone, Copy, Default)]
        $vis struct $Mask([u128; 2]);

        impl $Mask {
            /// Gets a mask representing just the `slot`.
            pub fn from_slot(slot: $Slot) -> Self {
                let mut r = Self::none();
                r.insert(slot);
                r
            }

            /// Returns a mask that represents no update.
            #[inline]
            pub const fn none() -> Self {
                $Mask([0; 2])
            }

            /// Returns a mask that represents all updates.
            #[inline]
            pub const fn all() -> Self {
                $Mask([u128::MAX; 2])
            }

            /// Returns `true` if this mask does not represent any update.
            #[inline]
            pub fn is_none(&self) -> bool {
                self.0[0] == 0 && self.0[1] == 0
            }

            /// Flags the `slot` in this mask.
            #[inline]
            pub fn insert(&mut self, slot: $Slot) {
                let slot = slot.0;
                if slot < 128 {
                    self.0[0] |= 1 << slot;
                } else {
                    self.0[1] |= 1 << (slot - 128);
                }
            }

            /// Returns `true` if the `slot` is set in this mask.
            #[inline]
            pub fn contains(&self, slot: $Slot) -> bool {
                let slot = slot.0;
                if slot < 128 {
                    (self.0[0] & (1 << slot)) != 0
                } else {
                    (self.0[1] & (1 << (slot - 128))) != 0
                }
            }

            /// Flags all slots set in `other` in `self` as well.
            #[inline]
            pub fn extend(&mut self, other: &Self) {
                self.0[0] |= other.0[0];
                self.0[1] |= other.0[1];
            }

            /// Returns `true` if any slot is set in both `self` and `other`.
            #[inline]
            pub fn intersects(&self, other: &Self) -> bool {
                (self.0[0] & other.0[0]) != 0 || (self.0[1] & other.0[1]) != 0
            }
        }
        impl ops::BitOrAssign<Self> for $Mask {
            fn bitor_assign(&mut self, rhs: Self) {
                self.extend(&rhs)
            }
        }
        impl ops::BitOrAssign<$Slot> for $Mask {
            fn bitor_assign(&mut self, rhs: $Slot) {
                self.insert(rhs)
            }
        }
        impl ops::BitOr<Self> for $Mask {
            type Output = Self;

            fn bitor(mut self, rhs: Self) -> Self {
                self.extend(&rhs);
                self
            }
        }
        impl ops::BitOr<$Slot> for $Mask {
            type Output = Self;

            fn bitor(mut self, rhs: $Slot) -> Self {
                self.insert(rhs);
                self
            }
        }
        impl ops::BitOr<Self> for $Slot {
            type Output = $Mask;

            fn bitor(self, rhs: Self) -> $Mask {
                let mut m = self.mask();
                m.insert(rhs);
                m
            }
        }
        impl fmt::Debug for $Mask {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                use std::fmt::Write;

                let rows = [
                    self.0[0] as u64,
                    (self.0[0] >> 64) as u64,
                    self.0[1] as u64,
                    (self.0[1] >> 64) as u64
                ];

                writeln!(f, "{} {{", stringify!($Mask))?;

                let mut bmp = String::with_capacity(256 + 4);

                for row in rows {
                    write!(bmp, "    ")?;
                    for i in 0..64 {
                        let b = 1u64 << i;
                        if (b & row) == 0 {
                            write!(bmp, "░")?;
                        } else {
                            write!(bmp, "█")?;
                        }
                    }
                    writeln!(bmp)?;
                }

                write!(f, "{bmp}}}")
            }
        }

    )+}
}

update_slot! {
    /// Represents a single update source in a [`UpdateMask`].
    ///
    /// Anything that generates an [`UiNode::update`] has one of these slots reserved.
    ///
    /// [`UiNode::update`]: crate::UiNode::update
    pub struct UpdateSlot -> UpdateMask;

    /// Represents a single event in a [`EventMask`].
    ///
    /// Every event is assigned on of these slots.
    pub struct EventSlot -> EventMask;
}
update_mask! {
    /// Represents the combined update sources that affect an UI tree or widget.
    pub struct UpdateMask <- UpdateSlot;

    /// Represents the combined events that are listened by an UI tree or widget.
    pub struct EventMask <- EventSlot;
}

/// Represents all event and update subscriptions of an widget.
///
/// Properties must register their interest in events and variables here otherwise a call to [`UiNode::event`] or
/// [`UiNode::update`] can end-up skipped due to optimizations.
///
/// [`UiNode::event`]: crate::UiNode::event
/// [`UiNode::update`]: crate::UiNode::update
#[derive(Debug, Default, Clone)]
pub struct WidgetSubscriptions {
    event: EventMask,
    update: UpdateMask,
}
impl WidgetSubscriptions {
    /// New default, no subscriptions.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an [`Event`] or command subscription.
    ///
    /// [`Event`]: crate::event::Event
    #[inline]
    pub fn event(&mut self, event: impl crate::event::Event) -> &mut Self {
        self.event.insert(event.slot());
        self
    }

    /// Register multiple event or command subscriptions.
    #[inline]
    pub fn events(&mut self, mask: &EventMask) -> &mut Self {
        self.event.extend(mask);
        self
    }

    /// Register async handler waker update source.
    #[inline]
    pub fn handler<A>(&mut self, handler: &impl WidgetHandler<A>) -> &mut Self
    where
        A: Clone + 'static,
    {
        handler.subscribe(self);
        self
    }

    /// Register a custom update source subscription.
    #[inline]
    pub fn update(&mut self, slot: UpdateSlot) -> &mut Self {
        self.update.insert(slot);
        self
    }

    /// Register multiple update source subscriptions.
    #[inline]
    pub fn updates(&mut self, mask: &UpdateMask) -> &mut Self {
        self.update.extend(mask);
        self
    }

    /// Register all subscriptions from `other` in `self`.
    #[inline]
    pub fn extend(&mut self, other: &WidgetSubscriptions) -> &mut Self {
        self.events(&other.event).updates(&other.update)
    }

    /// Register a variable subscription.
    #[inline]
    pub fn var<Vr, T>(&mut self, vars: &Vr, var: &impl Var<T>) -> &mut Self
    where
        Vr: WithVarsRead,
        T: VarValue,
    {
        self.update.extend(&var.update_mask(vars));
        self
    }

    /// Start a [`WidgetVarSubscriptions`] to register multiple variables without needing to reference the [`VarsRead`] for every variable.
    #[inline]
    pub fn vars<'s, 'v>(&'s mut self, vars: &'v impl AsRef<VarsRead>) -> WidgetVarSubscriptions<'v, 's> {
        WidgetVarSubscriptions {
            vars: vars.as_ref(),
            subscriptions: self,
        }
    }

    /// Returns `true` if the widget subscribes to events in the slot.
    pub fn event_contains(&self, event: &impl EventUpdateArgs) -> bool {
        self.event.contains(event.slot())
    }

    /// Returns `true` if the widget is interested in variables or other update sources that are flagged in `updates`.
    pub fn update_intersects(&self, updates: &Updates) -> bool {
        self.update.intersects(updates.current())
    }

    /// Returns the current set event subscriptions.
    #[inline]
    pub fn event_mask(&self) -> EventMask {
        self.event
    }

    /// Returns the current set update subscriptions.
    #[inline]
    pub fn update_mask(&self) -> UpdateMask {
        self.update
    }
}
impl ops::BitOr for WidgetSubscriptions {
    type Output = Self;

    fn bitor(mut self, rhs: Self) -> Self::Output {
        self |= rhs;
        self
    }
}
impl ops::BitOrAssign for WidgetSubscriptions {
    fn bitor_assign(&mut self, rhs: Self) {
        self.event |= rhs.event;
        self.update |= rhs.update;
    }
}

/// Helper for registering multiple [`WidgetSubscriptions::var`] without needing to reference the [`VarsRead`] instance for every variable.
pub struct WidgetVarSubscriptions<'v, 's> {
    vars: &'v VarsRead,
    /// The main [`WidgetSubscriptions`].
    pub subscriptions: &'s mut WidgetSubscriptions,
}
impl<'v, 's> WidgetVarSubscriptions<'v, 's> {
    /// Register a variable subscriptions.
    #[inline]
    pub fn var<T: VarValue>(self, var: &impl Var<T>) -> Self {
        Self {
            subscriptions: self.subscriptions.var(self.vars, var),
            vars: self.vars,
        }
    }
}

/// Argument for a interactive filter function.
///
/// See [WidgetInfoBuilder::push_interactive_filter].
#[derive(Debug)]
pub struct InteractiveFilterArgs<'a> {
    /// Widget being filtered.
    pub info: WidgetInfo<'a>,
}
impl<'a> InteractiveFilterArgs<'a> {
    /// New from `info`.
    pub fn new(info: WidgetInfo<'a>) -> Self {
        Self { info }
    }
}
