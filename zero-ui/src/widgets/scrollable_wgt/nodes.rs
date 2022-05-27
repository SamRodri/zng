//! UI nodes used for building the scrollable widget.
//!
use crate::prelude::new_widget::*;

use crate::core::mouse::{MouseScrollDelta, MouseWheelEvent};

use super::commands::*;
use super::parts::*;
use super::properties::*;
use super::types::*;

/// The actual content presenter.
pub fn viewport(child: impl UiNode, mode: impl IntoVar<ScrollMode>) -> impl UiNode {
    struct ViewportNode<C, M> {
        scroll_id: ScrollId,
        child: C,
        mode: M,

        viewport_unit: PxSize,
        viewport_size: PxSize,
        content_size: PxSize,
        content_offset: PxVector,

        info: ScrollableInfo,
    }
    #[impl_ui_node(child)]
    impl<C: UiNode, M: Var<ScrollMode>> UiNode for ViewportNode<C, M> {
        fn info(&self, ctx: &mut InfoContext, builder: &mut WidgetInfoBuilder) {
            builder.meta().set(ScrollableInfoKey, self.info.clone());
            self.child.info(ctx, builder);
        }

        fn subscriptions(&self, ctx: &mut InfoContext, subscriptions: &mut WidgetSubscriptions) {
            subscriptions
                .vars(ctx)
                .var(&self.mode)
                .var(&ScrollVerticalOffsetVar::new())
                .var(&ScrollHorizontalOffsetVar::new());
            self.child.subscriptions(ctx, subscriptions);
        }

        fn update(&mut self, ctx: &mut WidgetContext) {
            self.child.update(ctx);

            if self.mode.is_new(ctx) || ScrollVerticalOffsetVar::is_new(ctx) || ScrollHorizontalOffsetVar::is_new(ctx) {
                ctx.updates.layout();
            }
        }

        fn layout(&mut self, ctx: &mut LayoutContext, wl: &mut WidgetLayout) -> PxSize {
            let mode = self.mode.copy(ctx);

            let constrains = ctx.constrains();
            let viewport_unit = constrains.fill_size();
            let define_vp_unit = *DefineViewportUnitVar::get(ctx) // requested
                && viewport_unit.width > Px(0) // and has fill-size
                && viewport_unit.height > Px(0)
                && constrains.max_size() == Some(viewport_unit); // that is not just min size.

            let ct_size = ctx.with_constrains(
                |mut c| {
                    c = c.with_min_size(viewport_unit);
                    if mode.contains(ScrollMode::VERTICAL) {
                        c = c.with_unbounded_y();
                    }
                    if mode.contains(ScrollMode::HORIZONTAL) {
                        c = c.with_unbounded_x();
                    }
                    c
                },
                |ctx| {
                    if define_vp_unit {
                        ctx.with_viewport(viewport_unit, self.viewport_unit != viewport_unit, |ctx| {
                            self.viewport_unit = viewport_unit;
                            self.child.layout(ctx, wl)
                        })
                    } else {
                        self.child.layout(ctx, wl)
                    }
                },
            );

            let viewport_size = constrains.fill_size_or(ct_size);
            if self.viewport_size != viewport_size {
                self.viewport_size = viewport_size;
                ScrollViewportSizeVar::set(ctx, viewport_size).unwrap();
                ctx.updates.render();
            }

            self.info.set_viewport_size(viewport_size);

            self.content_size = ct_size;
            if !mode.contains(ScrollMode::VERTICAL) {
                self.content_size.height = viewport_size.height;
            }
            if !mode.contains(ScrollMode::HORIZONTAL) {
                self.content_size.width = viewport_size.width;
            }

            let mut content_offset = PxVector::zero();
            let v_offset = *ScrollVerticalOffsetVar::get(ctx.vars);
            content_offset.y = (self.viewport_size.height - self.content_size.height) * v_offset;
            let h_offset = *ScrollHorizontalOffsetVar::get(ctx.vars);
            content_offset.x = (self.viewport_size.width - self.content_size.width) * h_offset;

            if content_offset != self.content_offset {
                self.content_offset = content_offset;
                ctx.updates.render_update();
            }

            let v_ratio = self.viewport_size.height.0 as f32 / self.content_size.height.0 as f32;
            let h_ratio = self.viewport_size.width.0 as f32 / self.content_size.width.0 as f32;

            ScrollVerticalRatioVar::new().set_ne(ctx, v_ratio.fct()).unwrap();
            ScrollHorizontalRatioVar::new().set_ne(ctx, h_ratio.fct()).unwrap();
            ScrollContentSizeVar::new().set_ne(ctx, self.content_size).unwrap();

            self.viewport_size
        }

        fn render(&self, ctx: &mut RenderContext, frame: &mut FrameBuilder) {
            self.info.set_viewport_transform(*frame.transform());

            frame.push_scroll_frame(
                self.scroll_id,
                self.viewport_size,
                PxRect::new(self.content_offset.to_point(), self.content_size),
                |frame| {
                    self.child.render(ctx, frame);
                },
            )
        }

        fn render_update(&self, ctx: &mut RenderContext, update: &mut FrameUpdate) {
            self.info.set_viewport_transform(*update.transform());

            update.with_scroll(self.scroll_id, self.content_offset, |update| self.child.render_update(ctx, update));
        }
    }
    ViewportNode {
        child: child.cfg_boxed(),
        scroll_id: ScrollId::new_unique(),
        mode: mode.into_var(),
        viewport_size: PxSize::zero(),
        viewport_unit: PxSize::zero(),
        content_size: PxSize::zero(),
        content_offset: PxVector::zero(),
        info: ScrollableInfo::default(),
    }
    .cfg_boxed()
}

/// Create a node that generates and presents the [vertical scrollbar].
///
/// [vertical scrollbar]: VerticalScrollBarViewVar
pub fn v_scrollbar_presenter() -> impl UiNode {
    scrollbar_presenter(VerticalScrollBarViewVar, scrollbar::Orientation::Vertical)
}

/// Create a node that generates and presents the [horizontal scrollbar].
///
/// [horizontal scrollbar]: HorizontalScrollBarViewVar
pub fn h_scrollbar_presenter() -> impl UiNode {
    scrollbar_presenter(HorizontalScrollBarViewVar, scrollbar::Orientation::Horizontal)
}

fn scrollbar_presenter(var: impl IntoVar<ViewGenerator<ScrollBarArgs>>, orientation: scrollbar::Orientation) -> impl UiNode {
    ViewGenerator::presenter(
        var,
        |_, _| {},
        move |_, is_new| {
            if is_new {
                DataUpdate::Update(ScrollBarArgs::new(orientation))
            } else {
                DataUpdate::Same
            }
        },
    )
}

/// Create a node that generates and presents the [scrollbar joiner].
///
/// [scrollbar joiner]: ScrollBarJoinerViewVar
pub fn scrollbar_joiner_presenter() -> impl UiNode {
    ViewGenerator::presenter_default(ScrollBarJoinerViewVar)
}

/// Create a node that implements [`ScrollUpCommand`], [`ScrollDownCommand`],
/// [`ScrollLeftCommand`] and [`ScrollRightCommand`] scoped on the widget.
pub fn scroll_commands_node(child: impl UiNode) -> impl UiNode {
    struct ScrollCommandsNode<C> {
        child: C,

        up: CommandHandle,
        down: CommandHandle,
        left: CommandHandle,
        right: CommandHandle,

        layout_line: PxVector,
    }
    #[impl_ui_node(child)]
    impl<C: UiNode> UiNode for ScrollCommandsNode<C> {
        fn init(&mut self, ctx: &mut WidgetContext) {
            let scope = ctx.path.widget_id();

            self.up = ScrollUpCommand.scoped(scope).new_handle(ctx, ScrollContext::can_scroll_up(ctx));
            self.down = ScrollDownCommand.scoped(scope).new_handle(ctx, ScrollContext::can_scroll_down(ctx));
            self.left = ScrollLeftCommand.scoped(scope).new_handle(ctx, ScrollContext::can_scroll_left(ctx));
            self.right = ScrollRightCommand
                .scoped(scope)
                .new_handle(ctx, ScrollContext::can_scroll_right(ctx));

            self.child.init(ctx);
        }

        fn deinit(&mut self, ctx: &mut WidgetContext) {
            self.child.deinit(ctx);

            self.up = CommandHandle::dummy();
            self.down = CommandHandle::dummy();
            self.left = CommandHandle::dummy();
            self.right = CommandHandle::dummy();
        }

        fn subscriptions(&self, ctx: &mut InfoContext, subscriptions: &mut WidgetSubscriptions) {
            let scope = ctx.path.widget_id();

            subscriptions
                .event(ScrollUpCommand.scoped(scope))
                .event(ScrollDownCommand.scoped(scope))
                .event(ScrollLeftCommand.scoped(scope))
                .event(ScrollRightCommand.scoped(scope))
                .vars(ctx)
                .var(&VerticalLineUnitVar::new())
                .var(&HorizontalLineUnitVar::new());

            self.child.subscriptions(ctx, subscriptions);
        }

        fn update(&mut self, ctx: &mut WidgetContext) {
            self.child.update(ctx);

            self.up.set_enabled(ScrollContext::can_scroll_up(ctx));
            self.down.set_enabled(ScrollContext::can_scroll_down(ctx));
            self.left.set_enabled(ScrollContext::can_scroll_left(ctx));
            self.right.set_enabled(ScrollContext::can_scroll_right(ctx));

            if VerticalLineUnitVar::is_new(ctx) || HorizontalLineUnitVar::is_new(ctx) {
                ctx.updates.layout();
            }
        }

        fn event<A: EventUpdateArgs>(&mut self, ctx: &mut WidgetContext, args: &A) {
            let scope = ctx.path.widget_id();

            if let Some(args) = ScrollUpCommand.scoped(scope).update(args) {
                self.child.event(ctx, args);

                args.handle(|_| {
                    let mut offset = -self.layout_line.y;
                    if ScrollRequest::from_args(args).map(|f| f.alternate).unwrap_or(false) {
                        offset *= AltFactorVar::get_clone(ctx);
                    }
                    ScrollContext::scroll_vertical(ctx.vars, offset);
                });
            } else if let Some(args) = ScrollDownCommand.scoped(scope).update(args) {
                self.child.event(ctx, args);

                args.handle(|_| {
                    let mut offset = self.layout_line.y;
                    if ScrollRequest::from_args(args).map(|f| f.alternate).unwrap_or(false) {
                        offset *= AltFactorVar::get_clone(ctx);
                    }
                    ScrollContext::scroll_vertical(ctx.vars, offset);
                });
            } else if let Some(args) = ScrollLeftCommand.scoped(scope).update(args) {
                self.child.event(ctx, args);

                args.handle(|_| {
                    let mut offset = -self.layout_line.x;
                    if ScrollRequest::from_args(args).map(|f| f.alternate).unwrap_or(false) {
                        offset *= AltFactorVar::get_clone(ctx);
                    }
                    ScrollContext::scroll_horizontal(ctx.vars, offset);
                });
            } else if let Some(args) = ScrollRightCommand.scoped(scope).update(args) {
                self.child.event(ctx, args);

                args.handle(|_| {
                    let mut offset = self.layout_line.x;
                    if ScrollRequest::from_args(args).map(|f| f.alternate).unwrap_or(false) {
                        offset *= AltFactorVar::get_clone(ctx);
                    }
                    ScrollContext::scroll_horizontal(ctx.vars, offset);
                });
            } else {
                self.child.event(ctx, args);
            }
        }

        fn layout(&mut self, ctx: &mut LayoutContext, wl: &mut WidgetLayout) -> PxSize {
            let r = self.child.layout(ctx, wl);

            let viewport = *ScrollViewportSizeVar::get(ctx);
            ctx.with_constrains(
                |c| c.with_max_size(viewport).with_fill(true, true),
                |ctx| {
                    self.layout_line = PxVector::new(
                        HorizontalLineUnitVar::get(ctx.vars).layout(ctx.metrics.for_x(), |_| Px(20)),
                        VerticalLineUnitVar::get(ctx.vars).layout(ctx.metrics.for_y(), |_| Px(20)),
                    );
                },
            );

            r
        }
    }

    ScrollCommandsNode {
        child: child.cfg_boxed(),

        up: CommandHandle::dummy(),
        down: CommandHandle::dummy(),
        left: CommandHandle::dummy(),
        right: CommandHandle::dummy(),

        layout_line: PxVector::zero(),
    }
    .cfg_boxed()
}

/// Create a node that implements [`PageUpCommand`], [`PageDownCommand`],
/// [`PageLeftCommand`] and [`PageRightCommand`] scoped on the widget.
pub fn page_commands_node(child: impl UiNode) -> impl UiNode {
    struct PageCommandsNode<C> {
        child: C,

        up: CommandHandle,
        down: CommandHandle,
        left: CommandHandle,
        right: CommandHandle,

        layout_page: PxVector,
    }
    #[impl_ui_node(child)]
    impl<C: UiNode> UiNode for PageCommandsNode<C> {
        fn init(&mut self, ctx: &mut WidgetContext) {
            let scope = ctx.path.widget_id();

            self.up = PageUpCommand.scoped(scope).new_handle(ctx, ScrollContext::can_scroll_up(ctx));
            self.down = PageDownCommand.scoped(scope).new_handle(ctx, ScrollContext::can_scroll_down(ctx));
            self.left = PageLeftCommand.scoped(scope).new_handle(ctx, ScrollContext::can_scroll_left(ctx));
            self.right = PageRightCommand.scoped(scope).new_handle(ctx, ScrollContext::can_scroll_right(ctx));

            self.child.init(ctx);
        }

        fn deinit(&mut self, ctx: &mut WidgetContext) {
            self.child.deinit(ctx);

            self.up = CommandHandle::dummy();
            self.down = CommandHandle::dummy();
            self.left = CommandHandle::dummy();
            self.right = CommandHandle::dummy();
        }

        fn subscriptions(&self, ctx: &mut InfoContext, subscriptions: &mut WidgetSubscriptions) {
            let scope = ctx.path.widget_id();

            subscriptions
                .event(PageUpCommand.scoped(scope))
                .event(PageDownCommand.scoped(scope))
                .event(PageLeftCommand.scoped(scope))
                .event(PageRightCommand.scoped(scope))
                .vars(ctx)
                .var(&VerticalPageUnitVar::new())
                .var(&HorizontalPageUnitVar::new());

            self.child.subscriptions(ctx, subscriptions);
        }

        fn update(&mut self, ctx: &mut WidgetContext) {
            self.child.update(ctx);

            self.up.set_enabled(ScrollContext::can_scroll_up(ctx));
            self.down.set_enabled(ScrollContext::can_scroll_down(ctx));
            self.left.set_enabled(ScrollContext::can_scroll_left(ctx));
            self.right.set_enabled(ScrollContext::can_scroll_right(ctx));

            if VerticalPageUnitVar::is_new(ctx) || HorizontalPageUnitVar::is_new(ctx) {
                ctx.updates.layout();
            }
        }

        fn event<A: EventUpdateArgs>(&mut self, ctx: &mut WidgetContext, args: &A) {
            let scope = ctx.path.widget_id();

            if let Some(args) = PageUpCommand.scoped(scope).update(args) {
                self.child.event(ctx, args);

                args.handle(|_| {
                    let mut offset = -self.layout_page.y;
                    if ScrollRequest::from_args(args).map(|f| f.alternate).unwrap_or(false) {
                        offset *= AltFactorVar::get_clone(ctx);
                    }
                    ScrollContext::scroll_vertical(ctx.vars, offset);
                });
            } else if let Some(args) = PageDownCommand.scoped(scope).update(args) {
                self.child.event(ctx, args);

                args.handle(|_| {
                    let mut offset = self.layout_page.y;
                    if ScrollRequest::from_args(args).map(|f| f.alternate).unwrap_or(false) {
                        offset *= AltFactorVar::get_clone(ctx);
                    }
                    ScrollContext::scroll_vertical(ctx.vars, offset);
                });
            } else if let Some(args) = PageLeftCommand.scoped(scope).update(args) {
                self.child.event(ctx, args);

                args.handle(|_| {
                    let mut offset = -self.layout_page.x;
                    if ScrollRequest::from_args(args).map(|f| f.alternate).unwrap_or(false) {
                        offset *= AltFactorVar::get_clone(ctx);
                    }
                    ScrollContext::scroll_horizontal(ctx.vars, offset);
                });
            } else if let Some(args) = PageRightCommand.scoped(scope).update(args) {
                self.child.event(ctx, args);

                args.handle(|_| {
                    let mut offset = self.layout_page.x;
                    if ScrollRequest::from_args(args).map(|f| f.alternate).unwrap_or(false) {
                        offset *= AltFactorVar::get_clone(ctx);
                    }
                    ScrollContext::scroll_horizontal(ctx.vars, offset);
                });
            } else {
                self.child.event(ctx, args);
            }
        }

        fn layout(&mut self, ctx: &mut LayoutContext, wl: &mut WidgetLayout) -> PxSize {
            let r = self.child.layout(ctx, wl);

            let viewport = *ScrollViewportSizeVar::get(ctx);
            ctx.with_constrains(
                |c| c.with_max_size(viewport).with_fill(true, true),
                |ctx| {
                    self.layout_page = PxVector::new(
                        HorizontalPageUnitVar::get(ctx.vars).layout(ctx.metrics.for_x(), |_| Px(20)),
                        VerticalPageUnitVar::get(ctx.vars).layout(ctx.metrics.for_y(), |_| Px(20)),
                    );
                },
            );

            r
        }
    }

    PageCommandsNode {
        child,

        up: CommandHandle::dummy(),
        down: CommandHandle::dummy(),
        left: CommandHandle::dummy(),
        right: CommandHandle::dummy(),

        layout_page: PxVector::zero(),
    }
}

/// Create a node that implements [`ScrollToTopCommand`], [`ScrollToBottomCommand`],
/// [`ScrollToLeftmostCommand`] and [`ScrollToRightmostCommand`] scoped on the widget.
pub fn scroll_to_edge_commands_node(child: impl UiNode) -> impl UiNode {
    struct ScrollToEdgeCommandsNode<C> {
        child: C,

        top: CommandHandle,
        bottom: CommandHandle,
        leftmost: CommandHandle,
        rightmost: CommandHandle,
    }
    #[impl_ui_node(child)]
    impl<C: UiNode> UiNode for ScrollToEdgeCommandsNode<C> {
        fn init(&mut self, ctx: &mut WidgetContext) {
            let scope = ctx.path.widget_id();

            self.top = ScrollToTopCommand.scoped(scope).new_handle(ctx, ScrollContext::can_scroll_up(ctx));
            self.bottom = ScrollToBottomCommand
                .scoped(scope)
                .new_handle(ctx, ScrollContext::can_scroll_down(ctx));
            self.leftmost = ScrollToLeftmostCommand
                .scoped(scope)
                .new_handle(ctx, ScrollContext::can_scroll_left(ctx));
            self.rightmost = ScrollToRightmostCommand
                .scoped(scope)
                .new_handle(ctx, ScrollContext::can_scroll_right(ctx));

            self.child.init(ctx);
        }

        fn deinit(&mut self, ctx: &mut WidgetContext) {
            self.child.deinit(ctx);

            self.top = CommandHandle::dummy();
            self.bottom = CommandHandle::dummy();
            self.leftmost = CommandHandle::dummy();
            self.rightmost = CommandHandle::dummy();
        }

        fn subscriptions(&self, ctx: &mut InfoContext, subscriptions: &mut WidgetSubscriptions) {
            let scope = ctx.path.widget_id();

            subscriptions
                .event(ScrollToTopCommand.scoped(scope))
                .event(ScrollToBottomCommand.scoped(scope))
                .event(ScrollToLeftmostCommand.scoped(scope))
                .event(ScrollToRightmostCommand.scoped(scope));

            self.child.subscriptions(ctx, subscriptions);
        }

        fn update(&mut self, ctx: &mut WidgetContext) {
            self.child.update(ctx);

            self.top.set_enabled(ScrollContext::can_scroll_up(ctx));
            self.bottom.set_enabled(ScrollContext::can_scroll_down(ctx));
            self.leftmost.set_enabled(ScrollContext::can_scroll_left(ctx));
            self.rightmost.set_enabled(ScrollContext::can_scroll_right(ctx));
        }

        fn event<A: EventUpdateArgs>(&mut self, ctx: &mut WidgetContext, args: &A) {
            let scope = ctx.path.widget_id();

            if let Some(args) = ScrollToTopCommand.scoped(scope).update(args) {
                self.child.event(ctx, args);

                args.handle(|_| {
                    ScrollContext::scroll_to_top(ctx);
                });
            } else if let Some(args) = ScrollToBottomCommand.scoped(scope).update(args) {
                self.child.event(ctx, args);

                args.handle(|_| {
                    ScrollContext::scroll_to_bottom(ctx);
                });
            } else if let Some(args) = ScrollToLeftmostCommand.scoped(scope).update(args) {
                self.child.event(ctx, args);

                args.handle(|_| {
                    ScrollContext::scroll_to_leftmost(ctx);
                });
            } else if let Some(args) = ScrollToRightmostCommand.scoped(scope).update(args) {
                self.child.event(ctx, args);

                args.handle(|_| {
                    ScrollContext::scroll_to_rightmost(ctx);
                });
            } else {
                self.child.event(ctx, args);
            }
        }
    }
    ScrollToEdgeCommandsNode {
        child: child.cfg_boxed(),

        top: CommandHandle::dummy(),
        bottom: CommandHandle::dummy(),
        leftmost: CommandHandle::dummy(),
        rightmost: CommandHandle::dummy(),
    }
    .cfg_boxed()
}

/// Create a node that implements [`ScrollToCommand`] scoped on the widget.
pub fn scroll_to_command_node(child: impl UiNode) -> impl UiNode {
    struct ScrollToCommandNode<C> {
        child: C,

        handle: CommandHandle,
        scroll_to: Option<(WidgetBoundsInfo, WidgetRenderInfo, ScrollToMode)>,
    }
    #[impl_ui_node(child)]
    impl<C: UiNode> UiNode for ScrollToCommandNode<C> {
        fn init(&mut self, ctx: &mut WidgetContext) {
            self.handle = ScrollToCommand.scoped(ctx.path.widget_id()).new_handle(ctx, true);
            self.child.init(ctx);
        }

        fn deinit(&mut self, ctx: &mut WidgetContext) {
            self.handle = CommandHandle::dummy();
            self.child.deinit(ctx);
        }

        fn subscriptions(&self, ctx: &mut InfoContext, subscriptions: &mut WidgetSubscriptions) {
            subscriptions.event(ScrollToCommand.scoped(ctx.path.widget_id()));
            self.child.subscriptions(ctx, subscriptions);
        }

        fn event<A: EventUpdateArgs>(&mut self, ctx: &mut WidgetContext, args: &A) {
            let self_id = ctx.path.widget_id();
            if let Some(args) = ScrollToCommand.scoped(self_id).update(args) {
                // event send to us
                if let Some(request) = ScrollToRequest::from_args(args) {
                    // has unhandled request
                    if let Some(target) = ctx.info_tree.find(request.widget_id) {
                        // target exists
                        if let Some(us) = target.ancestors().find(|w| w.widget_id() == self_id) {
                            // target is descendant
                            if us.is_scrollable() {
                                // we are a scrollable.

                                let bounds = target.bounds_info();
                                let render = target.render_info();
                                let mode = request.mode;

                                // will scroll on the next arrange.
                                self.scroll_to = Some((bounds, render, mode));
                                ctx.updates.layout();

                                args.stop_propagation();
                            }
                        }
                    }
                }
                self.child.event(ctx, args);
            } else {
                self.child.event(ctx, args);
            }
        }

        fn layout(&mut self, ctx: &mut LayoutContext, wl: &mut WidgetLayout) -> PxSize {
            let r = self.child.layout(ctx, wl);

            if let Some((bounds, render, mode)) = self.scroll_to.take() {
                let us = ctx.info_tree.find(ctx.path.widget_id()).unwrap();
                if let Some(viewport_bounds) = us.viewport() {
                    let target_bounds = bounds.inner_bounds(&render);
                    match mode {
                        ScrollToMode::Minimal { margin } => {
                            let margin = ctx.with_constrains(
                                |c| c.with_max_size(target_bounds.size).with_fill(true, true),
                                |ctx| margin.layout(ctx, |_| PxSideOffsets::zero()),
                            );
                            let mut target_bounds = target_bounds;
                            target_bounds.origin.x -= margin.left;
                            target_bounds.origin.y -= margin.top;
                            target_bounds.size.width += margin.horizontal();
                            target_bounds.size.height += margin.vertical();

                            if target_bounds.size.width < viewport_bounds.size.width {
                                if target_bounds.origin.x < viewport_bounds.origin.x {
                                    let diff = target_bounds.origin.x - viewport_bounds.origin.x;
                                    ScrollContext::scroll_horizontal(ctx, diff);
                                } else if target_bounds.max_x() > viewport_bounds.max_x() {
                                    let diff = target_bounds.max_x() - viewport_bounds.max_x();
                                    ScrollContext::scroll_horizontal(ctx, diff);
                                }
                            } else {
                                let target_center_x = (target_bounds.size.width / Px(2)) + target_bounds.origin.x;
                                let viewport_center_x = (target_bounds.size.width / Px(2)) + viewport_bounds.origin.x;

                                let diff = target_center_x - viewport_center_x;
                                ScrollContext::scroll_horizontal(ctx, diff);
                            }
                            if target_bounds.size.height < viewport_bounds.size.height {
                                if target_bounds.origin.y < viewport_bounds.origin.y {
                                    let diff = target_bounds.origin.y - viewport_bounds.origin.y;
                                    ScrollContext::scroll_vertical(ctx, diff);
                                } else if target_bounds.max_y() > viewport_bounds.max_y() {
                                    let diff = target_bounds.max_y() - viewport_bounds.max_y();
                                    ScrollContext::scroll_vertical(ctx, diff);
                                }
                            } else {
                                let target_center_y = (target_bounds.size.height / Px(2)) + target_bounds.origin.y;
                                let viewport_center_y = (target_bounds.size.height / Px(2)) + viewport_bounds.origin.y;

                                let diff = target_center_y - viewport_center_y;
                                ScrollContext::scroll_vertical(ctx, diff);
                            }
                        }
                        ScrollToMode::Center {
                            widget_point,
                            scrollable_point,
                        } => {
                            let default = (target_bounds.size / Px(2)).to_vector().to_point();
                            let widget_point = ctx.with_constrains(
                                |c| c.with_max_size(target_bounds.size).with_fill(true, true),
                                |ctx| widget_point.layout(ctx, |_| default),
                            );

                            let default = (viewport_bounds.size / Px(2)).to_vector().to_point();
                            let scrollable_point = ctx.with_constrains(
                                |c| c.with_max_size(viewport_bounds.size).with_fill(true, true),
                                |ctx| scrollable_point.layout(ctx, |_| default),
                            );

                            let widget_point = widget_point + target_bounds.origin.to_vector();
                            let scrollable_point = scrollable_point + viewport_bounds.origin.to_vector(); // TODO origin non-zero?

                            let diff = widget_point - scrollable_point;

                            ScrollContext::scroll_vertical(ctx, diff.y);
                            ScrollContext::scroll_horizontal(ctx, diff.x);
                        }
                    }
                }
            }

            r
        }
    }

    ScrollToCommandNode {
        child: child.cfg_boxed(),

        handle: CommandHandle::dummy(),
        scroll_to: None,
    }
    .cfg_boxed()
}

/// Create a node that implements scroll-wheel handling for the widget.
pub fn scroll_wheel_node(child: impl UiNode) -> impl UiNode {
    struct ScrollWheelNode<C> {
        child: C,
        offset: Vector,
    }
    #[impl_ui_node(child)]
    impl<C: UiNode> UiNode for ScrollWheelNode<C> {
        fn subscriptions(&self, ctx: &mut InfoContext, subscriptions: &mut WidgetSubscriptions) {
            subscriptions.event(MouseWheelEvent);
            self.child.subscriptions(ctx, subscriptions);
        }

        fn event<A: EventUpdateArgs>(&mut self, ctx: &mut WidgetContext, args: &A) {
            if let Some(args) = MouseWheelEvent.update(args) {
                if args.concerns_widget(ctx) {
                    if let Some(delta) = args.scroll_delta(*AltFactorVar::get(ctx)) {
                        args.handle(|_| {
                            match delta {
                                MouseScrollDelta::LineDelta(x, y) => {
                                    self.offset.x -= HorizontalLineUnitVar::get_clone(ctx) * x.fct();
                                    self.offset.y -= VerticalLineUnitVar::get_clone(ctx) * y.fct();
                                }
                                MouseScrollDelta::PixelDelta(x, y) => {
                                    self.offset.x -= x.px();
                                    self.offset.y -= y.px();
                                }
                            }

                            ctx.updates.layout();
                        });
                    }
                }
                self.child.event(ctx, args);
            } else {
                self.child.event(ctx, args);
            }
        }

        fn layout(&mut self, ctx: &mut LayoutContext, wl: &mut WidgetLayout) -> PxSize {
            let r = self.child.layout(ctx, wl);

            let viewport = *ScrollViewportSizeVar::get(ctx);

            ctx.with_constrains(
                |c| c.with_max_size(viewport).with_fill(true, true),
                |ctx| {
                    let offset = self.offset.layout(ctx, |_| viewport.to_vector());
                    self.offset = Vector::zero();

                    if offset.y != Px(0) {
                        ScrollContext::scroll_vertical(ctx, offset.y);
                    }
                    if offset.x != Px(0) {
                        ScrollContext::scroll_horizontal(ctx, offset.x);
                    }
                },
            );

            r
        }
    }
    ScrollWheelNode {
        child: child.cfg_boxed(),
        offset: Vector::zero(),
    }
    .cfg_boxed()
}
