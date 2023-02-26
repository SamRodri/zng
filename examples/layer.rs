#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use zero_ui::prelude::*;
use zero_ui::{
    properties::events::{
        mouse::{on_mouse_enter, on_mouse_leave},
        widget::on_pre_init,
    },
    widgets::window::{AnchorMode, AnchorSize, AnchorTransform, LayerIndex, WindowLayers},
};

use zero_ui_view_prebuilt as zero_ui_view;

fn main() {
    examples_util::print_info();
    zero_ui_view::init();

    // let rec = examples_util::record_profile("layer");

    // zero_ui_view::run_same_process(app_main);
    app_main();

    // rec.finish();
}

fn app_main() {
    App::default().run_window(|| {
        window! {
            title = "Layer Example";

            // zero_ui::properties::inspector::show_bounds = true;
            // zero_ui::properties::inspector::show_hit_test = true;

            // you can use the pre-init to insert layered widgets
            // before the first render.
            on_pre_init = hn!(|_| {
                WindowLayers::insert(LayerIndex::TOP_MOST - 100, text! {
                    hit_test_mode = HitTestMode::Disabled;
                    txt = "on_pre_init";
                    font_size = 72;
                    font_family = "monospace";
                    opacity = 3.pct();
                    // rotate = 45.deg();
                    align = Align::CENTER;
                })
            });

            child_align = Align::CENTER;
            child = stack! {
                direction = StackDirection::top_to_bottom();
                spacing = 5;
                children = ui_vec![
                    overlay_example(),
                    layer_index_example(),
                    anchor_example(),
                    transform_anchor_example(),
                ];
            };
        }
    })
}

fn overlay_example() -> impl UiNode {
    button! {
        child = text!("TOP_MOST");
        on_click = hn!(|_| {
            WindowLayers::insert(LayerIndex::TOP_MOST, overlay("overlay", 0));
        });
    }
}
fn overlay(id: impl Into<WidgetId>, offset: i32) -> impl UiNode {
    let id = id.into();
    container! {
        id;
        modal = true;
        background_color = color_scheme_map(colors::WHITE.with_alpha(10.pct()), colors::BLACK.with_alpha(10.pct()));
        child_align = Align::CENTER;
        child = container! {
            offset = (offset, offset);
            focus_scope = true;
            tab_nav = TabNav::Cycle;
            directional_nav = DirectionalNav::Cycle;
            background_color = color_scheme_map(
                colors::GREEN.darken(80.pct()),
                colors::WHITE.with_alpha(80.pct()).mix_normal(colors::GREEN)
            );
            button::vis::extend_style = style_gen!(|_| style! {
                corner_radius = unset!;
            });
            padding = 2;
            child = stack! {
                direction = StackDirection::top_to_bottom();
                children_align = Align::RIGHT;
                children = ui_vec![
                    text! {
                        txt = "Overlay inserted in the TOP_MOST layer.";
                        margin = 15;
                    },
                    stack! {
                        direction = StackDirection::left_to_right();
                        spacing = 2;
                        children = ui_vec![
                            button! {
                                visibility = offset < 50;
                                child = text!("Open Another");
                                on_click = hn!(|_| {
                                    WindowLayers::insert(LayerIndex::TOP_MOST, overlay(WidgetId::new_unique(), offset + 10));
                                })
                            },
                            button! {
                                child = text!("Remove");
                                on_click = hn!(|_| {
                                    WindowLayers::remove(id);
                                })
                            },
                        ]
                    }
                ]
            }
        }
    }
}

fn layer_index_example() -> impl UiNode {
    // demonstrates that the z-order is not affected by the order of insertion.
    stack! {
        direction = StackDirection::left_to_right();
        spacing = 5;
        children = ui_vec![
            layer_n_btn(7, colors::DARK_GREEN),
            layer_n_btn(8, colors::DARK_BLUE),
            layer_n_btn(9, colors::DARK_RED),
        ]
    }
}
fn layer_n_btn(n: u32, color: Rgba) -> impl UiNode {
    let label = formatx!("Layer {n}");
    button! {
        child = text!(label.clone());
        on_click = async_hn!(label, |_| {
            let id = WidgetId::new_unique();
            WindowLayers::insert(n, container! {
                id;
                child = text! {
                    txt = label.clone();
                    txt_color = rgb(0.92, 0.92, 0.92);
                    font_size = 16;
                    font_weight = FontWeight::BOLD;
                };
                background_color = color.with_alpha(80.pct());
                padding = 10;
                margin = {
                    let inc = n as i32 * 10;
                    (20 + inc, 10, 0, inc - 40)
                };
                align = Align::TOP;
                hit_test_mode = HitTestMode::Disabled;
            });

            task::deadline(2.secs()).await; // wait fade-out

            WindowLayers::remove(id);
        });
    }
}

fn anchor_example() -> impl UiNode {
    let points = [
        Point::top_left(),
        Point::top(),
        Point::top_right(),
        Point::right(),
        Point::bottom_right(),
        Point::bottom(),
        Point::bottom_left(),
        Point::left(),
    ];
    let points_len = points.len();
    let point_index = var(0);
    let point = point_index.map(move |&i| points[i].clone());

    let anchor_mode = point.map(move |p| AnchorMode {
        transform: AnchorTransform::InnerOffset(p.clone()),
        size: AnchorSize::Unbounded,
        visibility: true,
        interaction: false,
        corner_radius: false,
    });

    let next_point = hn!(|_| {
        point_index.modify(move |i| {
            let next = **i + 1;
            *i.to_mut() = if next == points_len { 0 } else { next };
        })
    });

    button! {
        id = "anchor";
        child = text!("Anchored");

        margin = (60, 0);
        align = Align::CENTER;

        on_mouse_enter = hn!(|_| {
            WindowLayers::insert_anchored(LayerIndex::ADORNER, "anchor", anchor_mode.clone(), text! {
                id = "anchored";
                txt = "Example";
                txt_color = rgb(0.92, 0.92, 0.92);
                padding = 4;
                font_weight = FontWeight::BOLD;
                background_color = colors::DARK_GREEN.with_alpha(80.pct());
                border = 1, colors::GREEN.darken(20.pct());
                offset = point.map(|p|p.clone().as_vector() - Vector::splat(100.pct()));
                margin = 2;
                hit_test_mode = HitTestMode::Disabled;
            })
        });
        on_mouse_leave = hn!(|_| {
            WindowLayers::remove("anchored");
        });

        on_click = next_point;
    }
}

fn transform_anchor_example() -> impl UiNode {
    let mut insert = true;
    button! {
        id = "t-anchor";
        child = text!("Transform Anchored");

        rotate = 20.deg();
        scale = 110.pct();

        on_click = hn!(|_| {
            if insert {
                WindowLayers::insert_anchored(LayerIndex::ADORNER, "t-anchor", AnchorMode::foreground(), container! {
                    id = "t-anchored";
                    child_align = Align::TOP_LEFT;
                    border = 1, colors::GREEN.lighten(30.pct());
                    hit_test_mode = HitTestMode::Disabled;
                    child = text! {
                        y = -(2.dip() + 100.pct());
                        txt = "example";
                        font_weight = FontWeight::BOLD;
                    }
                })
            } else {
                WindowLayers::remove("t-anchored");
            }
            insert = !insert;
        })
    }
}
