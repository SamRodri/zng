use std::{fmt, sync::Arc};

use atomic::{Atomic, Ordering};
use parking_lot::Mutex;
use zero_ui_app::{
    event::EventHandles,
    property_id,
    render::FrameValueKey,
    update::UPDATES,
    widget::{
        base::WidgetBase,
        node::{match_node, match_node_leaf, UiNode, UiNodeOp},
        property, widget, WidgetId, WIDGET,
    },
};
use zero_ui_app_context::{context_local, LocalContext};
use zero_ui_color::colors;
use zero_ui_ext_input::{
    focus::FOCUS_CHANGED_EVENT,
    mouse::{MOUSE_INPUT_EVENT, MOUSE_MOVE_EVENT},
    pointer_capture::{POINTER_CAPTURE, POINTER_CAPTURE_EVENT},
    touch::{TOUCH_INPUT_EVENT, TOUCH_MOVE_EVENT},
};
use zero_ui_layout::{
    context::LAYOUT,
    unit::{Dip, DipToPx as _, DipVector, Px, PxCornerRadius, PxPoint, PxRect, PxSize, PxTransform, PxVector},
};
use zero_ui_view_api::{display_list::FrameValue, touch::TouchId};
use zero_ui_wgt::{prelude::*, WidgetFn};
use zero_ui_wgt_layer::{AnchorMode, LayerIndex, LAYERS};

use crate::{
    cmd::{TextSelectOp, SELECT_CMD},
    CaretShape, CARET_COLOR_VAR, INTERACTIVE_CARET_VAR, INTERACTIVE_CARET_VISUAL_VAR, TEXT_EDITABLE_VAR,
};

use super::TEXT;

/// An Ui node that renders the edit caret visual.
///
/// The caret is rendered after `child`, over it.
///
/// The `Text!` widgets introduces this node in `new_child`, around the [`render_text`] node.
///
/// [`render_text`]: super::render_text
pub fn non_interactive_caret(child: impl UiNode) -> impl UiNode {
    let color_key = FrameValueKey::new_unique();

    match_node(child, move |child, op| match op {
        UiNodeOp::Init => {
            WIDGET
                .sub_var_render_update(&CARET_COLOR_VAR)
                .sub_var_render_update(&INTERACTIVE_CARET_VAR);
        }
        UiNodeOp::Render { frame } => {
            child.render(frame);

            if TEXT_EDITABLE_VAR.get() {
                let t = TEXT.laidout();
                let resolved = TEXT.resolved();

                if let (false, Some(mut origin)) = (
                    resolved.selection_by.matches_interactive_mode(INTERACTIVE_CARET_VAR.get()),
                    t.caret_origin,
                ) {
                    let mut c = CARET_COLOR_VAR.get();
                    c.alpha = resolved.caret.opacity.get().0;

                    let caret_thickness = Dip::new(1).to_px(frame.scale_factor());
                    origin.x -= caret_thickness / 2;

                    let clip_rect = PxRect::new(origin, PxSize::new(caret_thickness, t.shaped_text.line_height()));
                    frame.push_color(clip_rect, color_key.bind(c.into(), true));
                }
            }
        }
        UiNodeOp::RenderUpdate { update } => {
            child.render_update(update);

            if TEXT_EDITABLE_VAR.get() {
                let resolved = TEXT.resolved();

                if !resolved.selection_by.matches_interactive_mode(INTERACTIVE_CARET_VAR.get()) {
                    let mut c = CARET_COLOR_VAR.get();
                    c.alpha = TEXT.resolved().caret.opacity.get().0;

                    update.update_color(color_key.update(c.into(), true))
                }
            }
        }
        _ => {}
    })
}

/// An Ui node that implements interaction and renders the interactive carets.
///
/// Caret visuals defined by [`INTERACTIVE_CARET_VISUAL_VAR`].
pub fn interactive_carets(child: impl UiNode) -> impl UiNode {
    let mut carets: Vec<Caret> = vec![];
    let mut is_focused = false;

    struct Caret {
        id: WidgetId,
        input: Arc<Mutex<InteractiveCaretInputMut>>,
    }
    match_node(child, move |c, op| match op {
        UiNodeOp::Init => {
            WIDGET.sub_var(&INTERACTIVE_CARET_VISUAL_VAR).sub_var_layout(&INTERACTIVE_CARET_VAR);
            is_focused = false;
        }
        UiNodeOp::Deinit => {
            for caret in carets.drain(..) {
                LAYERS.remove(caret.id);
            }
        }
        UiNodeOp::Event { update } => {
            if let Some(args) = FOCUS_CHANGED_EVENT.on(update) {
                let new_is_focused = args.is_focus_within(WIDGET.id());
                if is_focused != new_is_focused {
                    WIDGET.layout();
                    is_focused = new_is_focused;
                }
            }
        }
        UiNodeOp::Update { .. } => {
            if !carets.is_empty() && INTERACTIVE_CARET_VISUAL_VAR.is_new() {
                for caret in carets.drain(..) {
                    LAYERS.remove(caret.id);
                }
                WIDGET.layout();
            }
        }
        UiNodeOp::Layout { wl, final_size } => {
            *final_size = c.layout(wl);

            let r_txt = TEXT.resolved();
            let line_height_half = TEXT.laidout().shaped_text.line_height() / Px(2);

            let mut expected_len = 0;

            if r_txt.caret.index.is_some()
                && (is_focused || r_txt.selection_toolbar_is_open)
                && r_txt.selection_by.matches_interactive_mode(INTERACTIVE_CARET_VAR.get())
            {
                if r_txt.caret.selection_index.is_some() {
                    expected_len = 2;
                } else {
                    expected_len = 1;
                }
            }

            if expected_len != carets.len() {
                for caret in carets.drain(..) {
                    LAYERS.remove(caret.id);
                }
                carets.reserve_exact(expected_len);

                // caret shape node, inserted as ADORNER+1, anchored, propagates LocalContext and collects size+caret mid
                for i in 0..expected_len {
                    let input = Arc::new(Mutex::new(InteractiveCaretInputMut {
                        inner_text: PxTransform::identity(),
                        x: Px::MIN,
                        y: Px::MIN,
                        shape: CaretShape::Insert,
                        width: Px::MIN,
                        spot: PxPoint::zero(),
                    }));
                    let id = WidgetId::new_unique();

                    let caret = InteractiveCaret! {
                        id;
                        interactive_caret_input = InteractiveCaretInput {
                            ctx: LocalContext::capture(),
                            parent_id: WIDGET.id(),
                            visual_fn: INTERACTIVE_CARET_VISUAL_VAR.get(),
                            is_selection_index: i == 1,
                            m: input.clone(),
                        };
                    };

                    LAYERS.insert_anchored(LayerIndex::ADORNER + 1, WIDGET.id(), AnchorMode::foreground(), caret);
                    carets.push(Caret { id, input })
                }
            }

            if carets.is_empty() {
                // no caret.
                return;
            }

            let t = TEXT.laidout();
            let Some(origin) = t.caret_origin else {
                tracing::error!("caret instance, but no caret in context");
                return;
            };

            if carets.len() == 1 {
                // no selection, one caret rendered.

                let mut l = carets[0].input.lock();
                if l.shape != CaretShape::Insert {
                    l.shape = CaretShape::Insert;
                    UPDATES.update(carets[0].id);
                }

                let mut origin = origin;
                origin.x -= l.spot.x;
                origin.y += line_height_half - l.spot.y;

                if l.x != origin.x || l.y != origin.y {
                    l.x = origin.x;
                    l.y = origin.y;

                    UPDATES.render(carets[0].id);
                }
            } else {
                // selection, two carets rendered, but if text is bidirectional the two can have the same shape.

                let (Some(index), Some(s_index), Some(s_origin)) =
                    (r_txt.caret.index, r_txt.caret.selection_index, t.caret_selection_origin)
                else {
                    tracing::error!("caret instance, but no caret in context");
                    return;
                };

                let mut index_is_left = index.index <= s_index.index;
                let seg_txt = &r_txt.segmented_text;
                if let Some((_, seg)) = seg_txt.get(seg_txt.seg_from_char(index.index)) {
                    if seg.direction().is_rtl() {
                        index_is_left = !index_is_left;
                    }
                } else if seg_txt.base_direction().is_rtl() {
                    index_is_left = !index_is_left;
                }

                let mut s_index_is_left = s_index.index < index.index;
                if let Some((_, seg)) = seg_txt.get(seg_txt.seg_from_char(s_index.index)) {
                    if seg.direction().is_rtl() {
                        s_index_is_left = !s_index_is_left;
                    }
                } else if seg_txt.base_direction().is_rtl() {
                    s_index_is_left = !s_index_is_left;
                }

                let mut l = [carets[0].input.lock(), carets[1].input.lock()];

                let mut delay = false;

                let shapes = [
                    if index_is_left {
                        CaretShape::SelectionLeft
                    } else {
                        CaretShape::SelectionRight
                    },
                    if s_index_is_left {
                        CaretShape::SelectionLeft
                    } else {
                        CaretShape::SelectionRight
                    },
                ];

                for i in 0..2 {
                    if l[i].shape != shapes[i] {
                        l[i].shape = shapes[i];
                        l[i].width = Px::MIN;
                        UPDATES.update(carets[i].id);
                        delay = true;
                    } else if l[i].width == Px::MIN {
                        delay = true;
                    }
                }

                if delay {
                    // wait first layout of shape.
                    return;
                }

                let mut origins = [origin, s_origin];
                for i in 0..2 {
                    origins[i].x -= l[i].spot.x;
                    origins[i].y += line_height_half - l[i].spot.y;
                    if l[i].x != origins[i].x || l[i].y != origins[i].y {
                        l[i].x = origins[i].x;
                        l[i].y = origins[i].y;
                        UPDATES.render(carets[i].id);
                    }
                }
            }
        }
        UiNodeOp::Render { .. } | UiNodeOp::RenderUpdate { .. } => {
            if let Some(inner_rev) = WIDGET.info().inner_transform().inverse() {
                let text = TEXT.laidout().render_info.transform.then(&inner_rev);

                for c in &carets {
                    let mut l = c.input.lock();
                    if l.inner_text != text {
                        l.inner_text = text;

                        if l.x > Px::MIN && l.y > Px::MIN {
                            UPDATES.render(c.id);
                        }
                    }
                }
            }
        }
        _ => {}
    })
}

#[derive(Clone)]
struct InteractiveCaretInput {
    visual_fn: WidgetFn<CaretShape>,
    ctx: LocalContext,
    parent_id: WidgetId,
    is_selection_index: bool,
    m: Arc<Mutex<InteractiveCaretInputMut>>,
}
impl fmt::Debug for InteractiveCaretInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InteractiveCaretInput")
    }
}
impl PartialEq for InteractiveCaretInput {
    fn eq(&self, other: &Self) -> bool {
        self.visual_fn == other.visual_fn && Arc::ptr_eq(&self.m, &other.m)
    }
}

struct InteractiveCaretInputMut {
    // # set by Text:
    inner_text: PxTransform,
    // request render for Caret after changing.
    x: Px,
    y: Px,
    // request update for Caret after changing.
    shape: CaretShape,

    // # set by Caret:
    // request layout for Text after changing.
    width: Px,
    spot: PxPoint,
}

fn interactive_caret_shape_node(input: Arc<Mutex<InteractiveCaretInputMut>>, visual_fn: WidgetFn<CaretShape>) -> impl UiNode {
    let mut shape = CaretShape::Insert;

    match_node(NilUiNode.boxed(), move |visual, op| match op {
        UiNodeOp::Init => {
            shape = input.lock().shape;
            *visual.child() = visual_fn(shape);
            visual.init();
        }
        UiNodeOp::Deinit => {
            visual.deinit();
            *visual.child() = NilUiNode.boxed();
        }
        UiNodeOp::Update { .. } => {
            let new_shape = input.lock().shape;
            if new_shape != shape {
                shape = new_shape;
                visual.deinit();
                *visual.child() = visual_fn(shape);
                visual.init();
                WIDGET.layout().render();
            }
        }
        _ => {}
    })
}

fn interactive_caret_node(
    child: impl UiNode,
    parent_id: WidgetId,
    is_selection_index: bool,
    input: Arc<Mutex<InteractiveCaretInputMut>>,
) -> impl UiNode {
    let mut caret_spot_buf = Some(Arc::new(Atomic::new(PxPoint::zero())));
    let mut touch_move = None::<(TouchId, EventHandles)>;
    let mut mouse_move = EventHandles::dummy();
    let mut move_start_to_spot = DipVector::zero();

    match_node(child, move |visual, op| match op {
        UiNodeOp::Init => {
            WIDGET.sub_event(&TOUCH_INPUT_EVENT).sub_event(&MOUSE_INPUT_EVENT);
        }
        UiNodeOp::Deinit => {
            touch_move = None;
            mouse_move.clear();
        }
        UiNodeOp::Event { update } => {
            visual.event(update);

            if let Some(args) = TOUCH_INPUT_EVENT.on_unhandled(update) {
                if args.is_touch_start() {
                    let wgt_info = WIDGET.info();
                    move_start_to_spot = wgt_info
                        .inner_transform()
                        .transform_vector(input.lock().spot.to_vector())
                        .to_dip(wgt_info.tree().scale_factor())
                        - args.position.to_vector();

                    let mut handles = EventHandles::dummy();
                    handles.push(TOUCH_MOVE_EVENT.subscribe(WIDGET.id()));
                    handles.push(POINTER_CAPTURE_EVENT.subscribe(WIDGET.id()));
                    touch_move = Some((args.touch, handles));
                    POINTER_CAPTURE.capture_subtree(WIDGET.id());
                } else if touch_move.is_some() {
                    touch_move = None;
                    POINTER_CAPTURE.release_capture();
                }
            } else if let Some(args) = TOUCH_MOVE_EVENT.on_unhandled(update) {
                if let Some((id, _)) = &touch_move {
                    for t in &args.touches {
                        if t.touch == *id {
                            let spot = t.position() + move_start_to_spot;

                            let op = match input.lock().shape {
                                CaretShape::Insert => TextSelectOp::nearest_to(spot),
                                _ => TextSelectOp::select_index_nearest_to(spot, is_selection_index),
                            };
                            SELECT_CMD.scoped(parent_id).notify_param(op);
                            break;
                        }
                    }
                }
            } else if let Some(args) = MOUSE_INPUT_EVENT.on_unhandled(update) {
                if !args.is_click && args.is_mouse_down() && args.is_primary() {
                    let wgt_info = WIDGET.info();
                    move_start_to_spot = wgt_info
                        .inner_transform()
                        .transform_vector(input.lock().spot.to_vector())
                        .to_dip(wgt_info.tree().scale_factor())
                        - args.position.to_vector();

                    mouse_move.push(MOUSE_MOVE_EVENT.subscribe(WIDGET.id()));
                    mouse_move.push(POINTER_CAPTURE_EVENT.subscribe(WIDGET.id()));
                    POINTER_CAPTURE.capture_subtree(WIDGET.id());
                } else if !mouse_move.is_dummy() {
                    POINTER_CAPTURE.release_capture();
                    mouse_move.clear();
                }
            } else if let Some(args) = MOUSE_MOVE_EVENT.on_unhandled(update) {
                if !mouse_move.is_dummy() {
                    let spot = args.position + move_start_to_spot;

                    let op = match input.lock().shape {
                        CaretShape::Insert => TextSelectOp::nearest_to(spot),
                        _ => TextSelectOp::select_index_nearest_to(spot, is_selection_index),
                    };
                    SELECT_CMD.scoped(parent_id).notify_param(op);
                }
            } else if let Some(args) = POINTER_CAPTURE_EVENT.on(update) {
                if args.is_lost(WIDGET.id()) {
                    touch_move = None;
                    mouse_move.clear();
                }
            }
        }
        UiNodeOp::Layout { wl, final_size } => {
            *final_size = TOUCH_CARET_SPOT.with_context(&mut caret_spot_buf, || visual.layout(wl));
            let spot = caret_spot_buf.as_ref().unwrap().load(Ordering::Relaxed);

            let mut input_m = input.lock();

            if input_m.width != final_size.width || input_m.spot != spot {
                UPDATES.layout(parent_id);
                input_m.width = final_size.width;
                input_m.spot = spot;
            }
        }
        UiNodeOp::Render { frame } => {
            let input_m = input.lock();

            visual.delegated();

            let mut transform = input_m.inner_text;

            if input_m.x > Px::MIN && input_m.y > Px::MIN {
                transform = transform.then(&PxTransform::from(PxVector::new(input_m.x, input_m.y)));
                frame.push_inner_transform(&transform, |frame| {
                    visual.render(frame);
                });
            }
        }
        _ => {}
    })
}

#[widget($crate::node::caret::InteractiveCaret)]
struct InteractiveCaret(WidgetBase);
impl InteractiveCaret {
    fn widget_intrinsic(&mut self) {
        widget_set! {
            self;
            zero_ui_wgt::hit_test_mode = zero_ui_wgt::HitTestMode::Detailed;
        };
        self.widget_builder().push_build_action(|b| {
            let input = b
                .capture_value::<InteractiveCaretInput>(property_id!(interactive_caret_input))
                .unwrap();

            b.set_child(interactive_caret_shape_node(input.m.clone(), input.visual_fn));

            b.push_intrinsic(NestGroup::SIZE, "interactive_caret", move |child| {
                let child = interactive_caret_node(child, input.parent_id, input.is_selection_index, input.m);
                with_context_blend(input.ctx, false, child)
            });
        });
    }
}
#[property(CONTEXT, capture, widget_impl(InteractiveCaret))]
fn interactive_caret_input(input: impl IntoValue<InteractiveCaretInput>) {}

/// Default interactive caret visual.
///
/// See [`interactive_caret_visual`] for more details.
///
/// [`interactive_caret_visual`]: fn@super::interactive_caret_visual
pub fn default_interactive_caret_visual(shape: CaretShape) -> impl UiNode {
    match_node_leaf(move |op| match op {
        UiNodeOp::Layout { final_size, .. } => {
            let factor = LAYOUT.scale_factor();
            let size = Dip::new(16).to_px(factor);
            *final_size = PxSize::splat(size);
            let line_height = TEXT.laidout().shaped_text.line_height();
            final_size.height += line_height;

            let caret_thickness = Dip::new(1).to_px(factor);

            let caret_offset = match shape {
                CaretShape::SelectionLeft => {
                    final_size.width *= 0.8;
                    final_size.width - caret_thickness / 2.0 // rounds .5 to 1, to match `render_caret`
                }
                CaretShape::SelectionRight => {
                    final_size.width *= 0.8;
                    caret_thickness / 2 // rounds .5 to 0
                }
                CaretShape::Insert => final_size.width / 2 - caret_thickness / 2,
            };
            set_interactive_caret_spot(PxPoint::new(caret_offset, line_height / Px(2)));
        }
        UiNodeOp::Render { frame } => {
            let size = Dip::new(16).to_px(frame.scale_factor());
            let mut size = PxSize::splat(size);

            let corners = match shape {
                CaretShape::SelectionLeft => PxCornerRadius::new(size, PxSize::zero(), PxSize::zero(), size),
                CaretShape::Insert => PxCornerRadius::new_all(size),
                CaretShape::SelectionRight => PxCornerRadius::new(PxSize::zero(), size, size, PxSize::zero()),
            };

            if !matches!(shape, CaretShape::Insert) {
                size.width *= 0.8;
            }

            let line_height = TEXT.laidout().shaped_text.line_height();

            let rect = PxRect::new(PxPoint::new(Px(0), line_height), size);
            frame.push_clip_rounded_rect(rect, corners, false, false, |frame| {
                frame.push_color(rect, FrameValue::Value(colors::AZURE.into()));
            });

            let caret_thickness = Dip::new(1).to_px(frame.scale_factor());

            let line_pos = match shape {
                CaretShape::SelectionLeft => PxPoint::new(size.width - caret_thickness, Px(0)),
                CaretShape::Insert => PxPoint::new(size.width / 2 - caret_thickness / 2, Px(0)),
                CaretShape::SelectionRight => PxPoint::zero(),
            };
            let rect = PxRect::new(line_pos, PxSize::new(caret_thickness, line_height));
            frame.with_hit_tests_disabled(|frame| {
                frame.push_color(rect, FrameValue::Value(colors::AZURE.into()));
            });
        }
        _ => {}
    })
}

context_local! {
    static TOUCH_CARET_SPOT: Atomic<PxPoint> = Atomic::new(PxPoint::zero());
}

/// Set the caret *hotspot* that marks the middle of the caret on the text line.
///
/// See [`interactive_caret_visual`] for more details.
///
/// [`interactive_caret_visual`]: fn@super::interactive_caret_visual
pub fn set_interactive_caret_spot(caret_line_spot: PxPoint) {
    TOUCH_CARET_SPOT.get().store(caret_line_spot, Ordering::Relaxed);
}
