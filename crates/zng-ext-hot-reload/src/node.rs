use std::{any::Any, sync::Arc};

use zng_app::{
    render::{FrameBuilder, FrameUpdate},
    update::{EventUpdate, WidgetUpdates},
    widget::{
        info::{WidgetInfoBuilder, WidgetLayout, WidgetMeasure},
        node::{ArcNode, ArcNodeList, BoxedUiNode, BoxedUiNodeList, NilUiNode, UiNode, UiNodeList},
    },
};
use zng_app_context::LocalContext;
use zng_unit::PxSize;
use zng_var::{BoxedVar, IntoValue, IntoVar, Var, VarValue};

use crate::HOT_LIB;

trait Arg: Any + Send {
    fn clone_boxed(&self) -> Box<dyn Arg>;
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}
impl<T: VarValue> Arg for BoxedVar<T> {
    fn clone_boxed(&self) -> Box<dyn Arg> {
        Box::new(self.clone())
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}
#[derive(Clone)]
struct ValueArg<T>(T);
impl<T: Clone + Send + Any> Arg for ValueArg<T> {
    fn clone_boxed(&self) -> Box<dyn Arg> {
        Box::new(self.clone())
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}
impl Arg for ArcNode<BoxedUiNode> {
    fn clone_boxed(&self) -> Box<dyn Arg> {
        Box::new(self.clone())
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}
impl Arg for ArcNodeList<BoxedUiNodeList> {
    fn clone_boxed(&self) -> Box<dyn Arg> {
        Box::new(self.clone())
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// Arguments for hot node.
#[doc(hidden)]
pub struct HotNodeArgs {
    args: Vec<Box<dyn Arg>>,
}
impl HotNodeArgs {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            args: Vec::with_capacity(capacity),
        }
    }

    pub fn push_var<T: VarValue>(&mut self, arg: impl IntoVar<T>) {
        let arg = arg.into_var().boxed();
        self.args.push(Box::new(arg));
    }

    pub fn push_value<T: VarValue>(&mut self, arg: impl IntoValue<T>) {
        let arg = ValueArg(arg.into());
        self.args.push(Box::new(arg))
    }

    pub fn push_ui_node(&mut self, arg: impl UiNode) {
        let arg = ArcNode::new(arg.boxed());
        self.args.push(Box::new(arg))
    }

    pub fn push_ui_node_list(&mut self, arg: impl UiNodeList) {
        let arg = ArcNodeList::new(arg.boxed());
        self.args.push(Box::new(arg))
    }

    pub fn push_clone<T: Clone + Send + Any>(&mut self, arg: T) {
        let arg = ValueArg(arg);
        self.args.push(Box::new(arg));
    }

    fn pop_downcast<T: Any>(&mut self) -> T {
        *self.args.pop().unwrap().into_any().downcast().unwrap()
    }

    pub fn pop_var<T: VarValue>(&mut self) -> BoxedVar<T> {
        self.pop_downcast()
    }

    pub fn pop_value<T: VarValue>(&mut self) -> T {
        self.pop_downcast::<ValueArg<T>>().0
    }

    pub fn pop_ui_node(&mut self) -> BoxedUiNode {
        self.pop_downcast::<ArcNode<BoxedUiNode>>().take_on_init().boxed()
    }

    pub fn pop_ui_node_list(&mut self) -> BoxedUiNodeList {
        self.pop_downcast::<ArcNodeList<BoxedUiNodeList>>().take_on_init().boxed()
    }

    pub fn pop_clone<T: Clone + Send + Any>(&mut self) -> T {
        self.pop_downcast::<ValueArg<T>>().0
    }
}
impl Clone for HotNodeArgs {
    fn clone(&self) -> Self {
        let mut r = Self { args: vec![] };
        r.clone_from(self);
        r
    }

    fn clone_from(&mut self, source: &Self) {
        self.args.clear();
        self.args.reserve(source.args.len());
        for a in &source.args {
            self.args.push(a.clone_boxed());
        }
    }
}

/// Hot node host, dynamically re-inits the widget when the library rebuilds.
///
/// Captures and propagates the `LocalContext` because `static` variables are not the same
/// in the dynamically loaded library.
#[doc(hidden)]
pub struct HotNodeHost {
    manifest_dir: &'static str,
    name: &'static str,
    args: HotNodeArgs,
    fallback: fn(HotNodeArgs) -> HotNode,
    instance: HotNode,
}
impl HotNodeHost {
    pub fn new(manifest_dir: &'static str, name: &'static str, args: HotNodeArgs, fallback: fn(HotNodeArgs) -> HotNode) -> Self {
        Self {
            manifest_dir,
            name,
            args,
            fallback,
            instance: HotNode::new(NilUiNode),
        }
    }
}
impl UiNode for HotNodeHost {
    fn init(&mut self) {
        self.instance = match HOT_LIB.instantiate(self.manifest_dir, self.name, self.args.clone()) {
            Some(n) => n,
            None => (self.fallback)(self.args.clone()),
        };
        let mut ctx = LocalContext::capture();
        self.instance.init(&mut ctx);
    }

    fn deinit(&mut self) {
        let mut ctx = LocalContext::capture();
        self.instance.deinit(&mut ctx);
        self.instance.child = NilUiNode.boxed();
    }

    fn info(&mut self, info: &mut WidgetInfoBuilder) {
        let mut ctx = LocalContext::capture();
        self.instance.info(&mut ctx, info);
    }

    fn event(&mut self, update: &EventUpdate) {
        let mut ctx = LocalContext::capture();
        self.instance.event(&mut ctx, update);
    }

    fn update(&mut self, updates: &WidgetUpdates) {
        // !!: TODO, on library reload WIDGET.reinit();

        let mut ctx = LocalContext::capture();
        self.instance.update(&mut ctx, updates);
    }

    fn measure(&mut self, wm: &mut WidgetMeasure) -> PxSize {
        let mut ctx = LocalContext::capture();
        self.instance.measure(&mut ctx, wm)
    }

    fn layout(&mut self, wl: &mut WidgetLayout) -> PxSize {
        let mut ctx = LocalContext::capture();
        self.instance.layout(&mut ctx, wl)
    }

    fn render(&mut self, frame: &mut FrameBuilder) {
        let mut ctx = LocalContext::capture();
        self.instance.render(&mut ctx, frame)
    }

    fn render_update(&mut self, update: &mut FrameUpdate) {
        let mut ctx = LocalContext::capture();
        self.instance.render_update(&mut ctx, update)
    }

    fn is_widget(&self) -> bool {
        let mut ctx = LocalContext::capture();
        self.instance.is_widget(&mut ctx)
    }

    fn is_nil(&self) -> bool {
        let mut ctx = LocalContext::capture();
        self.instance.is_nil(&mut ctx)
    }

    fn with_context<R, F>(&mut self, update_mode: zng_app::widget::WidgetUpdateMode, f: F) -> Option<R>
    where
        F: FnOnce() -> R,
    {
        let mut ctx = LocalContext::capture();
        let mut r = None;
        let mut f = Some(f);
        self.instance.with_context(&mut ctx, update_mode, &mut || {
            r = Some(f.take().unwrap()());
        });
        r
    }
}

/// Hot loaded node.
#[doc(hidden)]
pub struct HotNode {
    child: BoxedUiNode,
    // keep alive because `child` is code from it.
    pub(crate) _lib: Option<Arc<libloading::Library>>,
}
impl HotNode {
    pub fn new(node: impl UiNode) -> Self {
        Self {
            child: node.boxed(),
            _lib: None,
        }
    }

    fn init(&mut self, ctx: &mut LocalContext) {
        ctx.with_context(|| self.child.init())
    }

    fn deinit(&mut self, ctx: &mut LocalContext) {
        ctx.with_context(|| self.child.deinit())
    }

    fn info(&mut self, ctx: &mut LocalContext, info: &mut WidgetInfoBuilder) {
        ctx.with_context(|| self.child.info(info))
    }

    fn event(&mut self, ctx: &mut LocalContext, update: &EventUpdate) {
        ctx.with_context(|| self.child.event(update))
    }

    fn update(&mut self, ctx: &mut LocalContext, updates: &WidgetUpdates) {
        ctx.with_context(|| self.child.update(updates))
    }

    fn measure(&mut self, ctx: &mut LocalContext, wm: &mut WidgetMeasure) -> PxSize {
        ctx.with_context(|| self.child.measure(wm))
    }

    fn layout(&mut self, ctx: &mut LocalContext, wl: &mut WidgetLayout) -> PxSize {
        ctx.with_context(|| self.child.layout(wl))
    }

    fn render(&mut self, ctx: &mut LocalContext, frame: &mut FrameBuilder) {
        ctx.with_context(|| self.child.render(frame))
    }

    fn render_update(&mut self, ctx: &mut LocalContext, update: &mut FrameUpdate) {
        ctx.with_context(|| self.child.render_update(update))
    }

    fn is_widget(&self, ctx: &mut LocalContext) -> bool {
        ctx.with_context(|| self.child.is_widget())
    }

    fn is_nil(&self, ctx: &mut LocalContext) -> bool {
        ctx.with_context(|| self.child.is_nil())
    }

    fn with_context(&mut self, ctx: &mut LocalContext, update_mode: zng_app::widget::WidgetUpdateMode, f: &mut dyn FnMut()) {
        ctx.with_context(|| {
            self.child.with_context(update_mode, f);
        })
    }
}
