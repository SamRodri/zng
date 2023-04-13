#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use icons::MaterialIcon;
use zero_ui::prelude::*;

use zero_ui_material_icons as icons;
use zero_ui_view_prebuilt as zero_ui_view;

fn main() {
    examples_util::print_info();
    zero_ui_view::init();

    // let rec = examples_util::record_profile("icon");

    // zero_ui_view::run_same_process(app_main);
    app_main();

    // rec.finish();
}

fn app_main() {
    App::default().extend(icons::MaterialFonts).run_window(async {
        Window! {
            title = "Icon Example";
            icon = WindowIcon::render(|| icon! {
                ico = icons::filled::LIGHTBULB;
                ico_color = colors::YELLOW;
                drop_shadow = (0, 0), 3, colors::WHITE;
            });
            child = scroll! {
                mode = ScrollMode::VERTICAL;
                child = icons();
            };
            // zero_ui::properties::inspector::show_hit_test = true;
        }
    })
}

fn icons() -> impl UiNode {
    let selected_font = var("outlined");
    fn select_font(key: &'static str) -> impl UiNode {
        Toggle! {
            child = Text!(key);
            value::<&'static str> = key;
        }
    }
    fn show_font(icons: Vec<MaterialIcon>) -> impl UiNode {
        let _scope = tracing::error_span!("show_font").entered();
        Wrap! {
            spacing = 5;
            icon::vis::ico_size = 48;
            children = {
                let mut r = vec![];
                icons
                .par_chunks(100)
                .map(|c| Wrap! { // segment into multiple inlined lazy inited `Wrap!` for better performance.
                    spacing = 5;
                    lazy = {
                        let len = c.len();
                        LazyMode::lazy(wgt_fn!(|_| {
                            wrap::lazy_size(len, 5, (80, 80))
                        }))
                    };
                    children = {
                        let mut r = vec![];
                        c.par_iter()
                                .map(|i| icon_btn(i.clone()).boxed())
                                .collect_into_vec(&mut r);
                        r
                    };
                }.boxed())
                .collect_into_vec(&mut r);
                r
            },
        }
    }
    Stack! {
        direction = StackDirection::top_to_bottom();
        padding = (20, 5, 5, 5);
        spacing = 20;
        children_align = Align::TOP;
        children = ui_vec![
            Stack! {
                direction = StackDirection::left_to_right();
                toggle::selector = toggle::Selector::single(selected_font.clone());
                spacing = 5;
                children = ui_vec![
                    select_font("filled"),
                    select_font("outlined"),
                    select_font("rounded"),
                    select_font("sharp"),
                    select_font("two_tone"),
                ]
            },
            view(selected_font, show_font(icons::outlined::all()), |font| {
                match font.get_new() {
                    Some("filled") => View::Update(show_font(icons::filled::all())),
                    Some("outlined") => View::Update(show_font(icons::outlined::all())),
                    Some("rounded") => View::Update(show_font(icons::rounded::all())),
                    Some("sharp") => View::Update(show_font(icons::sharp::all())),
                    Some("two_tone") => View::Update(show_font(icons::two_tone::all())),
                    None => View::Same,
                    Some(_) => unreachable!(),
                }
            }),
        ]
    }
}

fn icon_btn(ico: icons::MaterialIcon) -> impl UiNode {
    Button! {
        padding = 2;
        size = (80, 80);
        child = Stack! {
            direction = StackDirection::top_to_bottom();
            spacing = 2;
            children_align = Align::CENTER;
            children = ui_vec![
                icon! {
                    ico = ico.clone();
                },
                Text! {
                    txt = formatx!("{ico}");
                    txt_wrap = false;
                    font_size = 10;
                },
            ]
        };
        on_click = hn!(|_| {
            LAYERS.insert(LayerIndex::TOP_MOST, expanded_icon(ico.clone()));
        })
    }
}

fn expanded_icon(ico: icons::MaterialIcon) -> impl UiNode {
    let opacity = var(0.fct());
    opacity.ease(1.fct(), 200.ms(), easing::linear).perm();
    Container! {
        opacity = opacity.clone();

        id = "expanded-icon";
        modal = true;
        background_color = color_scheme_map(colors::WHITE.with_alpha(10.pct()), colors::BLACK.with_alpha(10.pct()));
        child_align = Align::CENTER;
        on_click = hn!(|args: &ClickArgs| {
            if WIDGET.id() == args.target.widget_id() {
                LAYERS.remove("expanded-icon");
                args.propagation().stop();
            }
        });
        child = Container! {
            id = "panel";
            background_color = color_scheme_map(colors::BLACK.with_alpha(90.pct()), colors::WHITE.with_alpha(90.pct()));
            focus_scope = true;
            tab_nav = TabNav::Cycle;
            directional_nav = DirectionalNav::Cycle;
            drop_shadow = (0, 0), 4, colors::BLACK;
            child = Stack!(children = ui_vec![
                Stack! {
                    direction = StackDirection::top_to_bottom();
                    spacing = 5;
                    padding = 10;
                    children_align = Align::TOP_LEFT;
                    children = ui_vec![
                        title(formatx!("{ico}")),
                        Text! {
                            txt = ico.name;
                            font_family = FontName::monospace();
                            font_size = 18;
                        },
                        sub_title("Using `icon!`:"),
                        Stack! {
                            direction = StackDirection::left_to_right();
                            spacing = 5;
                            children_align = Align::TOP_LEFT;
                            children = [64, 48, 32, 24, 16].into_iter().map(clmv!(ico, |size| {
                                Stack! {
                                    direction = StackDirection::top_to_bottom();
                                    spacing = 3;
                                    children = ui_vec![
                                        size_label(formatx!("{size}")),
                                        icon! {
                                            ico = ico.clone();
                                            ico_size = size;

                                            background_color =  color_scheme_map(
                                                colors::BLACK.with_alpha(85.pct()),
                                                colors::WHITE.with_alpha(85.pct())
                                            );
                                            corner_radius = 4;
                                            padding = 2;
                                        }
                                    ]
                                }.boxed()
                            })).collect::<Vec<_>>()
                        },

                        sub_title("Using `Text!`:"),
                        Stack! {
                            direction = StackDirection::left_to_right();
                            spacing = 5;
                            children_align = Align::TOP_LEFT;
                            children = [64, 48, 32, 24, 16].into_iter().map(clmv!(ico, |size| {
                                Stack! {
                                    direction = StackDirection::top_to_bottom();
                                    spacing = 3;
                                    children = ui_vec![
                                        size_label(formatx!("{size}")),
                                        Text! {
                                            txt = ico.code;
                                            font_family = ico.font.clone();
                                            font_size = size;

                                            background_color = color_scheme_map(
                                                colors::BLACK.with_alpha(85.pct()),
                                                colors::WHITE.with_alpha(85.pct())
                                            );
                                            corner_radius = 4;
                                            padding = 2;
                                        }
                                    ]
                                }.boxed()
                            })).collect::<Vec<_>>()
                        }
                    ]
                },
                Button! {
                    id = "close-btn";
                    icon::ico_size = 14;
                    child = icon(icons::filled::CLOSE);
                    align = Align::TOP_RIGHT;
                    padding = 2;
                    margin = 4;
                    on_click = async_hn!(opacity, |args: ClickArgs| {
                        args.propagation().stop();

                        opacity.ease(0.fct(), 150.ms(), easing::linear).perm();
                        task::yield_one().await;
                        opacity.wait_animation().await;

                        LAYERS.remove("expanded-icon");
                    });
                }
            ])
        }
    }
}

fn title(title: Txt) -> impl UiNode {
    Text! {
        txt = title;
        font_size = 24;
        txt_align = Align::CENTER;
    }
}
fn sub_title(title: impl Into<Txt>) -> impl UiNode {
    Text! {
        txt = title.into();
        font_size = 16;
    }
}
fn size_label(size: Txt) -> impl UiNode {
    Text! {
        txt = size;
        font_size = 10;
        txt_align = Align::CENTER;
    }
}
