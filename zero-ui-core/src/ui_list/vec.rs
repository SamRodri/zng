use std::{
    cell::RefCell,
    cmp, mem,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use crate::{
    context::{
        state_map, InfoContext, LayoutContext, MeasureContext, RenderContext, StateMapMut, StateMapRef, WidgetContext, WidgetUpdates,
        WithUpdates,
    },
    event::EventUpdate,
    render::{FrameBuilder, FrameUpdate},
    ui_list::{PosLayoutArgs, PreLayoutArgs, SortedWidgetVec, UiListObserver, WidgetFilterArgs, WidgetLayoutTranslation, WidgetList},
    units::PxSize,
    widget_info::{WidgetBorderInfo, WidgetBoundsInfo, WidgetInfoBuilder, WidgetLayout},
    BoxedUiNode, BoxedWidget, UiNode, UiNodeList, Widget, WidgetId,
};

/// A vector of boxed [`Widget`] items.
///
/// This type is a [`WidgetList`] that can be modified during runtime, the downside
/// is the dynamic dispatch.
///
/// The [widget_vec!] macro is provided to make initialization more convenient.
///
/// ```
/// # use zero_ui_core::{widget_vec, UiNode, Widget, WidgetId, NilUiNode};
/// # use zero_ui_core::widget_base::*;
/// # fn text(fake: &str) -> impl Widget { implicit_base::new(NilUiNode, WidgetId::new_unique())  };
/// # use text as foo;
/// # use text as bar;
/// let mut widgets = widget_vec![];
/// widgets.push(foo("Hello"));
/// widgets.push(bar("Dynamic!"));
///
/// for widget in widgets {
///     println!("{:?}", widget.bounds_info().inner_size());
/// }
/// ```
pub struct WidgetVec {
    pub(super) vec: Vec<BoxedWidget>,
    pub(super) ctrl: WidgetVecRef,
}
impl WidgetVec {
    /// New empty (default).
    pub fn new() -> Self {
        WidgetVec {
            vec: vec![],
            ctrl: WidgetVecRef::new(),
        }
    }

    /// New empty with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        WidgetVec {
            vec: Vec::with_capacity(capacity),
            ctrl: WidgetVecRef::new(),
        }
    }

    /// Returns a [`WidgetVecRef`] that can be used to insert, resort and remove widgets from this vector
    /// after it is moved to a widget list property.
    pub fn reference(&self) -> WidgetVecRef {
        self.ctrl.clone()
    }

    /// Appends the widget, automatically calls [`Widget::boxed_wgt`].
    pub fn push<W: Widget>(&mut self, widget: W) {
        self.vec.push(widget.boxed_wgt());
    }

    /// Appends the widget, automatically calls [`Widget::boxed_wgt`].
    pub fn insert<W: Widget>(&mut self, index: usize, widget: W) {
        self.vec.insert(index, widget.boxed_wgt());
    }

    /// Returns a reference to the widget with the same `id`.
    pub fn get(&self, id: impl Into<WidgetId>) -> Option<&BoxedWidget> {
        let id = id.into();
        self.vec.iter().find(|w| w.id() == id)
    }

    /// Returns a mutable reference to the widget with the same `id`.
    pub fn get_mut(&mut self, id: impl Into<WidgetId>) -> Option<&mut BoxedWidget> {
        let id = id.into();
        self.vec.iter_mut().find(|w| w.id() == id)
    }

    /// Removes and returns the widget, without affecting the order of widgets.
    pub fn remove(&mut self, id: impl Into<WidgetId>) -> Option<BoxedWidget> {
        let id = id.into();
        if let Some(i) = self.vec.iter().position(|w| w.id() == id) {
            Some(self.vec.remove(i))
        } else {
            None
        }
    }

    /// Convert `self` to a [`SortedWidgetVec`].
    ///
    /// See [`SortedWidgetVec::from_vec`] for more details.
    pub fn sorting(self, sort: impl FnMut(&BoxedWidget, &BoxedWidget) -> cmp::Ordering + 'static) -> SortedWidgetVec {
        SortedWidgetVec::from_vec(self, sort)
    }

    fn fullfill_requests<O: UiListObserver>(&mut self, ctx: &mut WidgetContext, observer: &mut O) {
        if let Some(r) = self.ctrl.take_requests() {
            if r.clear {
                // if reset
                self.clear();
                observer.reseted();

                for (i, mut wgt) in r.insert {
                    wgt.init(ctx);
                    ctx.updates.info();
                    if i < self.len() {
                        self.insert(i, wgt);
                    } else {
                        self.push(wgt);
                    }
                }
                for mut wgt in r.push {
                    wgt.init(ctx);
                    ctx.updates.info();
                    self.push(wgt);
                }
                for (r, i) in r.move_index {
                    if r < self.len() {
                        let wgt = self.vec.remove(r);

                        if i < self.len() {
                            self.vec.insert(i, wgt);
                        } else {
                            self.vec.push(wgt);
                        }

                        ctx.updates.info();
                    }
                }
                for (id, to) in r.move_id {
                    if let Some(r) = self.vec.iter().position(|w| w.id() == id) {
                        let i = to(r, self.len());

                        if r != i {
                            let wgt = self.vec.remove(r);

                            if i < self.len() {
                                self.vec.insert(i, wgt);
                            } else {
                                self.vec.push(wgt);
                            }

                            ctx.updates.info();
                        }
                    }
                }
            } else {
                for id in r.remove {
                    if let Some(i) = self.vec.iter().position(|w| w.id() == id) {
                        let mut wgt = self.vec.remove(i);
                        wgt.deinit(ctx);
                        ctx.updates.info();

                        observer.removed(i);
                    }
                }

                for (i, mut wgt) in r.insert {
                    wgt.init(ctx);
                    ctx.updates.info();

                    if i < self.len() {
                        self.insert(i, wgt);
                        observer.inserted(i);
                    } else {
                        observer.inserted(self.len());
                        self.push(wgt);
                    }
                }

                for mut wgt in r.push {
                    wgt.init(ctx);
                    ctx.updates.info();

                    observer.inserted(self.len());
                    self.push(wgt);
                }

                for (r, i) in r.move_index {
                    if r < self.len() {
                        let wgt = self.vec.remove(r);

                        if i < self.len() {
                            self.vec.insert(i, wgt);

                            observer.moved(r, i);
                        } else {
                            let i = self.vec.len();

                            self.vec.push(wgt);

                            observer.moved(r, i);
                        }

                        ctx.updates.info();
                    }
                }

                for (id, to) in r.move_id {
                    if let Some(r) = self.vec.iter().position(|w| w.id() == id) {
                        let i = to(r, self.len());

                        if r != i {
                            let wgt = self.vec.remove(r);

                            if i < self.len() {
                                self.vec.insert(i, wgt);
                                observer.moved(r, i);
                            } else {
                                let i = self.vec.len();
                                self.vec.push(wgt);
                                observer.moved(r, i);
                            }

                            ctx.updates.info();
                        }
                    }
                }
            }
        }
    }
}
impl From<Vec<BoxedWidget>> for WidgetVec {
    fn from(vec: Vec<BoxedWidget>) -> Self {
        WidgetVec {
            vec,
            ctrl: WidgetVecRef::new(),
        }
    }
}
impl From<WidgetVec> for Vec<BoxedWidget> {
    fn from(mut s: WidgetVec) -> Self {
        mem::take(&mut s.vec)
    }
}
impl Default for WidgetVec {
    fn default() -> Self {
        Self::new()
    }
}
impl Deref for WidgetVec {
    type Target = Vec<BoxedWidget>;

    fn deref(&self) -> &Self::Target {
        &self.vec
    }
}
impl DerefMut for WidgetVec {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.vec
    }
}
impl<'a> IntoIterator for &'a WidgetVec {
    type Item = &'a BoxedWidget;

    type IntoIter = std::slice::Iter<'a, BoxedWidget>;

    fn into_iter(self) -> Self::IntoIter {
        self.vec.iter()
    }
}
impl<'a> IntoIterator for &'a mut WidgetVec {
    type Item = &'a mut BoxedWidget;

    type IntoIter = std::slice::IterMut<'a, BoxedWidget>;

    fn into_iter(self) -> Self::IntoIter {
        self.vec.iter_mut()
    }
}
impl IntoIterator for WidgetVec {
    type Item = BoxedWidget;

    type IntoIter = std::vec::IntoIter<BoxedWidget>;

    fn into_iter(mut self) -> Self::IntoIter {
        mem::take(&mut self.vec).into_iter()
    }
}
impl FromIterator<BoxedWidget> for WidgetVec {
    fn from_iter<T: IntoIterator<Item = BoxedWidget>>(iter: T) -> Self {
        Vec::from_iter(iter).into()
    }
}
impl UiNodeList for WidgetVec {
    fn is_fixed(&self) -> bool {
        false
    }

    fn len(&self) -> usize {
        self.vec.len()
    }

    fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }

    fn boxed_all(mut self) -> UiNodeVec {
        UiNodeVec {
            vec: mem::take(&mut self.vec).into_iter().map(|w| w.boxed()).collect(),
        }
    }

    fn init_all(&mut self, ctx: &mut WidgetContext) {
        self.ctrl.0.borrow_mut().target = Some(ctx.path.widget_id());
        for widget in &mut self.vec {
            widget.init(ctx);
        }
    }

    fn deinit_all(&mut self, ctx: &mut WidgetContext) {
        self.ctrl.0.borrow_mut().target = None;
        for widget in &mut self.vec {
            widget.deinit(ctx);
        }
    }

    fn update_all<O: UiListObserver>(&mut self, ctx: &mut WidgetContext, updates: &mut WidgetUpdates, observer: &mut O) {
        self.fullfill_requests(ctx, observer);
        for widget in &mut self.vec {
            widget.update(ctx, updates);

            if updates.delivery_list().is_done() {
                break;
            }
        }
    }

    fn event_all(&mut self, ctx: &mut WidgetContext, update: &mut EventUpdate) {
        for widget in &mut self.vec {
            widget.event(ctx, update);

            if update.delivery_list().is_done() {
                break;
            }
        }
    }

    fn measure_all<C, D>(&self, ctx: &mut MeasureContext, mut pre_measure: C, mut pos_measure: D)
    where
        C: FnMut(&mut MeasureContext, &mut super::PreMeasureArgs),
        D: FnMut(&mut MeasureContext, super::PosMeasureArgs),
    {
        for (i, w) in self.iter().enumerate() {
            super::default_widget_list_measure_all(i, w, ctx, &mut pre_measure, &mut pos_measure)
        }
    }

    fn item_measure(&self, index: usize, ctx: &mut MeasureContext) -> PxSize {
        self.vec[index].measure(ctx)
    }

    fn layout_all<C, D>(&mut self, ctx: &mut LayoutContext, wl: &mut WidgetLayout, mut pre_layout: C, mut pos_layout: D)
    where
        C: FnMut(&mut LayoutContext, &mut WidgetLayout, &mut PreLayoutArgs),
        D: FnMut(&mut LayoutContext, &mut WidgetLayout, PosLayoutArgs),
    {
        for (i, w) in self.iter_mut().enumerate() {
            super::default_widget_list_layout_all(i, w, ctx, wl, &mut pre_layout, &mut pos_layout);
        }
    }

    fn item_layout(&mut self, index: usize, ctx: &mut LayoutContext, wl: &mut WidgetLayout) -> PxSize {
        self.vec[index].layout(ctx, wl)
    }

    fn info_all(&self, ctx: &mut InfoContext, info: &mut WidgetInfoBuilder) {
        for widget in &self.vec {
            widget.info(ctx, info);
        }
    }

    fn render_all(&self, ctx: &mut RenderContext, frame: &mut FrameBuilder) {
        for w in self {
            w.render(ctx, frame);
        }
    }

    fn item_render(&self, index: usize, ctx: &mut RenderContext, frame: &mut FrameBuilder) {
        self.vec[index].render(ctx, frame);
    }

    fn render_update_all(&self, ctx: &mut RenderContext, update: &mut FrameUpdate) {
        for w in self {
            w.render_update(ctx, update);
        }
    }

    fn item_render_update(&self, index: usize, ctx: &mut RenderContext, update: &mut FrameUpdate) {
        self.vec[index].render_update(ctx, update);
    }

    fn try_item_id(&self, index: usize) -> Option<WidgetId> {
        self.vec[index].try_id()
    }

    fn try_item_state(&self, index: usize) -> Option<StateMapRef<state_map::Widget>> {
        self.vec[index].try_state()
    }

    fn try_item_state_mut(&mut self, index: usize) -> Option<StateMapMut<state_map::Widget>> {
        self.vec[index].try_state_mut()
    }

    fn try_item_bounds_info(&self, index: usize) -> Option<&WidgetBoundsInfo> {
        self.vec[index].try_bounds_info()
    }

    fn try_item_border_info(&self, index: usize) -> Option<&WidgetBorderInfo> {
        self.vec[index].try_border_info()
    }

    fn render_node_filtered<F>(&self, mut filter: F, ctx: &mut RenderContext, frame: &mut FrameBuilder)
    where
        F: FnMut(UiNodeFilterArgs) -> bool,
    {
        for (i, w) in self.iter().enumerate() {
            if filter(UiNodeFilterArgs::new(i, w)) {
                w.render(ctx, frame);
            }
        }
    }

    fn try_item_outer<F, R>(&mut self, index: usize, wl: &mut WidgetLayout, keep_previous: bool, transform: F) -> Option<R>
    where
        F: FnOnce(&mut WidgetLayoutTranslation, PosLayoutArgs) -> R,
    {
        let w = &mut self.vec[index];
        if let Some(size) = w.try_bounds_info().map(|i| i.outer_size()) {
            wl.try_with_outer(w, keep_previous, |wlt, w| {
                transform(wlt, PosLayoutArgs::new(index, w.try_state_mut(), size))
            })
        } else {
            None
        }
    }

    fn try_outer_all<F>(&mut self, wl: &mut WidgetLayout, keep_previous: bool, mut transform: F)
    where
        F: FnMut(&mut WidgetLayoutTranslation, PosLayoutArgs),
    {
        for (i, w) in self.vec.iter_mut().enumerate() {
            if let Some(size) = w.try_bounds_info().map(|i| i.outer_size()) {
                wl.try_with_outer(w, keep_previous, |wlt, w| {
                    transform(wlt, PosLayoutArgs::new(i, w.try_state_mut(), size));
                });
            }
        }
    }

    fn count_nodes<F>(&self, mut filter: F) -> usize
    where
        F: FnMut(UiNodeFilterArgs) -> bool,
    {
        let mut count = 0;
        for (i, w) in self.iter().enumerate() {
            if filter(UiNodeFilterArgs::new(i, w)) {
                count += 1;
            }
        }
        count
    }
}
impl WidgetList for WidgetVec {
    fn boxed_widget_all(self) -> WidgetVec {
        self
    }

    fn item_id(&self, index: usize) -> WidgetId {
        self.vec[index].id()
    }

    fn item_state(&self, index: usize) -> StateMapRef<state_map::Widget> {
        self.vec[index].state()
    }

    fn item_state_mut(&mut self, index: usize) -> StateMapMut<state_map::Widget> {
        self.vec[index].state_mut()
    }

    fn item_bounds_info(&self, index: usize) -> &WidgetBoundsInfo {
        self.vec[index].bounds_info()
    }

    fn item_border_info(&self, index: usize) -> &WidgetBorderInfo {
        self.vec[index].border_info()
    }

    fn render_filtered<F>(&self, mut filter: F, ctx: &mut RenderContext, frame: &mut FrameBuilder)
    where
        F: FnMut(WidgetFilterArgs) -> bool,
    {
        for (i, w) in self.iter().enumerate() {
            if filter(WidgetFilterArgs::new(i, w)) {
                w.render(ctx, frame);
            }
        }
    }

    // default implementation uses indexing, this is faster.
    fn count<F>(&self, mut filter: F) -> usize
    where
        F: FnMut(WidgetFilterArgs) -> bool,
        Self: Sized,
    {
        let mut count = 0;
        for (i, w) in self.iter().enumerate() {
            if filter(WidgetFilterArgs::new(i, w)) {
                count += 1;
            }
        }
        count
    }

    fn item_outer<F, R>(&mut self, index: usize, wl: &mut WidgetLayout, keep_previous: bool, transform: F) -> R
    where
        F: FnOnce(&mut WidgetLayoutTranslation, PosLayoutArgs) -> R,
    {
        let w = &mut self.vec[index];
        let size = w.bounds_info().outer_size();
        wl.with_outer(w, keep_previous, |wlt, w| {
            transform(wlt, PosLayoutArgs::new(index, Some(w.state_mut()), size))
        })
    }

    fn outer_all<F>(&mut self, wl: &mut WidgetLayout, keep_previous: bool, mut transform: F)
    where
        F: FnMut(&mut WidgetLayoutTranslation, PosLayoutArgs),
    {
        for (i, w) in self.iter_mut().enumerate() {
            let size = w.bounds_info().outer_size();
            wl.with_outer(w, keep_previous, |wlt, w| {
                transform(wlt, PosLayoutArgs::new(i, Some(w.state_mut()), size));
            });
        }
    }
}
impl Drop for WidgetVec {
    fn drop(&mut self) {
        self.ctrl.0.borrow_mut().alive = false;
    }
}

/// See [`WidgetVecRef::move_to`] for more details
type WidgetMoveToFn = fn(usize, usize) -> usize;

/// Represents a [`WidgetVec`] controller that can be used to insert, push or remove widgets
/// after the vector is placed in a widget list property.
#[derive(Clone)]
pub struct WidgetVecRef(Rc<RefCell<WidgetVecRequests>>);
struct WidgetVecRequests {
    target: Option<WidgetId>,
    insert: Vec<(usize, BoxedWidget)>,
    push: Vec<BoxedWidget>,
    remove: Vec<WidgetId>,
    move_index: Vec<(usize, usize)>,
    move_id: Vec<(WidgetId, WidgetMoveToFn)>,
    clear: bool,

    alive: bool,
}
impl WidgetVecRef {
    pub(super) fn new() -> Self {
        Self(Rc::new(RefCell::new(WidgetVecRequests {
            target: None,
            insert: vec![],
            push: vec![],
            remove: vec![],
            move_index: vec![],
            move_id: vec![],
            clear: false,
            alive: true,
        })))
    }

    /// Returns `true` if the [`WidgetVec`] still exists.
    pub fn alive(&self) -> bool {
        self.0.borrow().alive
    }

    /// Request an update for the insertion of the `widget`.
    ///
    /// The `index` is resolved after all [`remove`] requests, if it is out-of-bounds the widget is pushed.
    ///
    /// The `widget` will be initialized, inserted and the info tree updated.
    ///
    /// [`remove`]: Self::remove
    pub fn insert(&self, updates: &mut impl WithUpdates, index: usize, widget: impl Widget) {
        updates.with_updates(|u| {
            let mut s = self.0.borrow_mut();
            s.insert.push((index, widget.boxed_wgt()));
            u.update(s.target);
        })
    }

    /// Request an update for the insertion of the `widget` at the end of the list.
    ///
    /// The widget will be pushed after all [`insert`] requests.
    ///
    /// The `widget` will be initialized, inserted and the info tree updated.
    ///
    /// [`insert`]: Self::insert
    pub fn push(&self, updates: &mut impl WithUpdates, widget: impl Widget) {
        updates.with_updates(|u| {
            let mut s = self.0.borrow_mut();
            s.push.push(widget.boxed_wgt());
            u.update(s.target);
        })
    }

    /// Request an update for the removal of the widget identified by `id`.
    ///
    /// The widget will be deinitialized, dropped and the info tree will update, nothing happens
    /// if the widget is not found.
    pub fn remove(&self, updates: &mut impl WithUpdates, id: impl Into<WidgetId>) {
        updates.with_updates(|u| {
            let mut s = self.0.borrow_mut();
            s.remove.push(id.into());
            u.update(s.target);
        })
    }

    /// Request a widget remove and re-insert.
    ///
    /// If the `remove_index` is out of bounds nothing happens, if the `insert_index` is out-of-bounds
    /// the widget is pushed to the end of the vector, if `remove_index` and `insert_index` are equal nothing happens.
    ///
    /// Move requests happen after all other requests.
    pub fn move_index(&self, updates: &mut impl WithUpdates, remove_index: usize, insert_index: usize) {
        if remove_index != insert_index {
            updates.with_updates(|u| {
                let mut s = self.0.borrow_mut();
                s.move_index.push((remove_index, insert_index));
                u.update(s.target);
            })
        }
    }

    /// Request a widget move, the widget is searched by `id`, if found `get_move_to` id called with the index of the widget and length
    /// of the vector, it must return the index the widget is inserted after it is removed.
    ///
    /// If the widget is not found nothing happens, if the returned index is the same nothing happens, if the returned index
    /// is out-of-bounds the widget if pushed to the end of the vector.
    ///
    /// Move requests happen after all other requests.
    ///
    /// # Examples
    ///
    /// If the widget vectors is layout as a vertical stack to move the widget *up* by one stopping at the top:
    ///
    /// ```
    /// # fn demo(ctx: &mut zero_ui_core::context::WidgetContext, items: zero_ui_core::ui_list::WidgetVecRef) {
    /// items.move_id(ctx.updates, "my-widget", |i, _len| i.saturating_sub(1));
    /// # }
    /// ```
    ///
    /// And to move *down* stopping at the bottom:
    ///
    /// ```
    /// # fn demo(ctx: &mut zero_ui_core::context::WidgetContext, items: zero_ui_core::ui_list::WidgetVecRef) {
    /// items.move_id(ctx.updates, "my-widget", |i, _len| i.saturating_add(1));
    /// # }
    /// ```
    ///
    /// Note that if the returned index overflows the length the widget is
    /// pushed as the last item.
    ///
    /// The length can be used for implementing wrapping move *down*:
    ///
    /// ```
    /// # fn demo(ctx: &mut zero_ui_core::context::WidgetContext, items: zero_ui_core::ui_list::WidgetVecRef) {
    /// items.move_id(ctx.updates, "my-widget", |i, len| {
    ///     let next = i.saturating_add(1);
    ///     if next < len { next } else { 0 }
    /// });
    /// # }
    /// ```
    pub fn move_id(&self, updates: &mut impl WithUpdates, id: impl Into<WidgetId>, get_move_to: WidgetMoveToFn) {
        updates.with_updates(|u| {
            let mut s = self.0.borrow_mut();
            s.move_id.push((id.into(), get_move_to));
            u.update(s.target);
        })
    }

    /// Request a removal of all current widgets.
    ///
    /// All other requests will happen after the clear.
    pub fn clear(&self, updates: &mut impl WithUpdates) {
        updates.with_updates(|u| {
            let mut s = self.0.borrow_mut();
            s.clear = true;
            u.update(s.target);
        })
    }

    fn take_requests(&self) -> Option<WidgetVecRequests> {
        let mut s = self.0.borrow_mut();

        if s.clear
            || !s.insert.is_empty()
            || !s.push.is_empty()
            || !s.remove.is_empty()
            || !s.move_index.is_empty()
            || !s.move_id.is_empty()
        {
            let empty = WidgetVecRequests {
                target: s.target,
                alive: s.alive,

                insert: vec![],
                push: vec![],
                remove: vec![],
                move_index: vec![],
                move_id: vec![],
                clear: false,
            };
            Some(mem::replace(&mut *s, empty))
        } else {
            None
        }
    }
}

/// A vector of boxed [`UiNode`] items.
///
/// This type is a [`UiNodeList`] that can be modified during runtime, the downside
/// is the dynamic dispatch.
///
/// The [node_vec!] macro is provided to make initialization more convenient.
///
/// ```
/// # use zero_ui_core::{node_vec, UiNode, Widget, WidgetId, NilUiNode};
/// # use zero_ui_core::widget_base::*;
/// # fn text(fake: &str) -> impl UiNode { zero_ui_core::NilUiNode };
/// # use text as foo;
/// # use text as bar;
/// let mut nodes = node_vec![];
/// nodes.push(foo("Hello"));
/// nodes.push(bar("Dynamic!"));
/// ```
pub struct UiNodeVec {
    pub(super) vec: Vec<BoxedUiNode>,
}
impl UiNodeVec {
    /// New empty (default).
    pub fn new() -> Self {
        UiNodeVec { vec: vec![] }
    }

    /// New empty with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        UiNodeVec {
            vec: Vec::with_capacity(capacity),
        }
    }

    /// Appends the node, automatically calls [`UiNode::boxed`].
    pub fn push<N: UiNode>(&mut self, node: N) {
        self.vec.push(node.boxed());
    }

    /// Insert the node, automatically calls [`UiNode::boxed`].
    pub fn insert<N: UiNode>(&mut self, index: usize, node: N) {
        self.vec.insert(index, node.boxed())
    }
}
impl Default for UiNodeVec {
    fn default() -> Self {
        Self::new()
    }
}
impl Deref for UiNodeVec {
    type Target = Vec<BoxedUiNode>;

    fn deref(&self) -> &Self::Target {
        &self.vec
    }
}
impl DerefMut for UiNodeVec {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.vec
    }
}
impl IntoIterator for UiNodeVec {
    type Item = BoxedUiNode;

    type IntoIter = std::vec::IntoIter<BoxedUiNode>;

    fn into_iter(self) -> Self::IntoIter {
        self.vec.into_iter()
    }
}
impl<'a> IntoIterator for &'a UiNodeVec {
    type Item = &'a BoxedUiNode;

    type IntoIter = std::slice::Iter<'a, BoxedUiNode>;

    fn into_iter(self) -> Self::IntoIter {
        self.vec.iter()
    }
}
impl FromIterator<BoxedUiNode> for UiNodeVec {
    fn from_iter<T: IntoIterator<Item = BoxedUiNode>>(iter: T) -> Self {
        UiNodeVec { vec: Vec::from_iter(iter) }
    }
}
impl From<Vec<BoxedUiNode>> for UiNodeVec {
    fn from(vec: Vec<BoxedUiNode>) -> Self {
        UiNodeVec { vec }
    }
}
impl From<UiNodeVec> for Vec<BoxedUiNode> {
    fn from(s: UiNodeVec) -> Self {
        s.vec
    }
}
impl UiNodeList for UiNodeVec {
    fn is_fixed(&self) -> bool {
        false
    }
    fn len(&self) -> usize {
        self.vec.len()
    }
    fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }
    fn boxed_all(self) -> UiNodeVec {
        self
    }
    fn init_all(&mut self, ctx: &mut WidgetContext) {
        for node in self.iter_mut() {
            node.init(ctx);
        }
    }
    fn deinit_all(&mut self, ctx: &mut WidgetContext) {
        for node in self.iter_mut() {
            node.deinit(ctx);
        }
    }
    fn update_all<O: UiListObserver>(&mut self, ctx: &mut WidgetContext, updates: &mut WidgetUpdates, _: &mut O) {
        for node in self.iter_mut() {
            node.update(ctx, updates);
        }
    }
    fn event_all(&mut self, ctx: &mut WidgetContext, update: &mut EventUpdate) {
        for node in self.iter_mut() {
            node.event(ctx, update);
        }
    }

    fn measure_all<C, D>(&self, ctx: &mut MeasureContext, mut pre_measure: C, mut pos_measure: D)
    where
        C: FnMut(&mut MeasureContext, &mut super::PreMeasureArgs),
        D: FnMut(&mut MeasureContext, super::PosMeasureArgs),
    {
        for (i, node) in self.iter().enumerate() {
            super::default_ui_node_list_measure_all(i, node, ctx, &mut pre_measure, &mut pos_measure);
        }
    }

    fn item_measure(&self, index: usize, ctx: &mut MeasureContext) -> PxSize {
        self.vec[index].measure(ctx)
    }

    fn layout_all<C, D>(&mut self, ctx: &mut LayoutContext, wl: &mut WidgetLayout, mut pre_layout: C, mut pos_layout: D)
    where
        C: FnMut(&mut LayoutContext, &mut WidgetLayout, &mut PreLayoutArgs),
        D: FnMut(&mut LayoutContext, &mut WidgetLayout, PosLayoutArgs),
    {
        for (i, node) in self.iter_mut().enumerate() {
            super::default_ui_node_list_layout_all(i, node, ctx, wl, &mut pre_layout, &mut pos_layout);
        }
    }

    fn item_layout(&mut self, index: usize, ctx: &mut LayoutContext, wl: &mut WidgetLayout) -> PxSize {
        self.vec[index].layout(ctx, wl)
    }

    fn info_all(&self, ctx: &mut InfoContext, info: &mut WidgetInfoBuilder) {
        for w in &self.vec {
            w.info(ctx, info);
        }
    }

    fn render_all(&self, ctx: &mut RenderContext, frame: &mut FrameBuilder) {
        for w in self {
            w.render(ctx, frame);
        }
    }

    fn item_render(&self, index: usize, ctx: &mut RenderContext, frame: &mut FrameBuilder) {
        self.vec[index].render(ctx, frame);
    }

    fn render_update_all(&self, ctx: &mut RenderContext, update: &mut FrameUpdate) {
        for w in self.iter() {
            w.render_update(ctx, update)
        }
    }

    fn item_render_update(&self, index: usize, ctx: &mut RenderContext, update: &mut FrameUpdate) {
        self.vec[index].render_update(ctx, update);
    }

    fn try_item_id(&self, index: usize) -> Option<WidgetId> {
        self.vec[index].try_id()
    }

    fn try_item_state(&self, index: usize) -> Option<StateMapRef<state_map::Widget>> {
        self.vec[index].try_state()
    }

    fn try_item_state_mut(&mut self, index: usize) -> Option<StateMapMut<state_map::Widget>> {
        self.vec[index].try_state_mut()
    }

    fn try_item_bounds_info(&self, index: usize) -> Option<&WidgetBoundsInfo> {
        self.vec[index].try_bounds_info()
    }

    fn try_item_border_info(&self, index: usize) -> Option<&WidgetBorderInfo> {
        self.vec[index].try_border_info()
    }

    fn render_node_filtered<F>(&self, mut filter: F, ctx: &mut RenderContext, frame: &mut FrameBuilder)
    where
        F: FnMut(UiNodeFilterArgs) -> bool,
    {
        for (i, w) in self.iter().enumerate() {
            if filter(UiNodeFilterArgs::new(i, w)) {
                w.render(ctx, frame);
            }
        }
    }

    fn try_item_outer<F, R>(&mut self, index: usize, wl: &mut WidgetLayout, keep_previous: bool, transform: F) -> Option<R>
    where
        F: FnOnce(&mut WidgetLayoutTranslation, PosLayoutArgs) -> R,
    {
        let w = &mut self.vec[index];
        if let Some(size) = w.try_bounds_info().map(|i| i.outer_size()) {
            wl.try_with_outer(w, keep_previous, |wlt, w| {
                transform(wlt, PosLayoutArgs::new(index, w.try_state_mut(), size))
            })
        } else {
            None
        }
    }

    fn try_outer_all<F>(&mut self, wl: &mut WidgetLayout, keep_previous: bool, mut transform: F)
    where
        F: FnMut(&mut WidgetLayoutTranslation, PosLayoutArgs),
    {
        for (i, w) in self.vec.iter_mut().enumerate() {
            if let Some(size) = w.try_bounds_info().map(|i| i.outer_size()) {
                wl.try_with_outer(w, keep_previous, |wlt, w| {
                    transform(wlt, PosLayoutArgs::new(i, w.try_state_mut(), size));
                });
            }
        }
    }

    fn count_nodes<F>(&self, mut filter: F) -> usize
    where
        F: FnMut(UiNodeFilterArgs) -> bool,
    {
        let mut count = 0;
        for (i, w) in self.iter().enumerate() {
            if filter(UiNodeFilterArgs::new(i, w)) {
                count += 1;
            }
        }
        count
    }
}

/// Creates a [`WidgetVec`] containing the arguments.
///
/// # Examples
///
/// ```
/// # use zero_ui_core::{widget_vec, UiNode, Widget, WidgetId, NilUiNode};
/// # use zero_ui_core::widget_base::*;
/// # fn text(fake: &str) -> impl Widget { implicit_base::new(NilUiNode, WidgetId::new_unique())  };
/// # use text as foo;
/// # use text as bar;
/// let widgets = widget_vec![
///     foo("Hello"),
///     bar("World!")
/// ];
/// ```
///
/// `widget_vec!` automatically calls [`Widget::boxed_wgt`] for each item.
///
/// [`WidgetVec`]: crate::ui_list::WidgetVec
#[macro_export]
macro_rules! widget_vec {
    () => { $crate::ui_list::WidgetVec::new() };
    ($($widget:expr),+ $(,)?) => {
        $crate::ui_list::WidgetVec::from(vec![
            $($crate::Widget::boxed_wgt($widget)),*
        ])
    };
}
#[doc(inline)]
pub use crate::widget_vec;

/// Creates a [`UiNodeVec`] containing the arguments.
///
/// # Examples
///
/// ```
/// # use zero_ui_core::{node_vec, UiNode, Widget, WidgetId, NilUiNode};
/// # use zero_ui_core::widget_base::*;
/// # fn text(fake: &str) -> impl Widget { implicit_base::new(NilUiNode, WidgetId::new_unique())  };
/// # use text as foo;
/// # use text as bar;
/// let widgets = node_vec![
///     foo("Hello"),
///     bar("World!")
/// ];
/// ```
///
/// `node_vec!` automatically calls [`UiNode::boxed`] for each item.
///
/// [`UiNodeVec`]: crate::ui_list::UiNodeVec
#[macro_export]
macro_rules! node_vec {
    () => { $crate::ui_list::UiNodeVec::new() };
    ($($node:expr),+ $(,)?) => {
        $crate::ui_list::UiNodeVec::from(vec![
            $($crate::UiNode::boxed($node)),*
        ])
    };
}
#[doc(inline)]
pub use crate::node_vec;

use super::UiNodeFilterArgs;
