#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use zero_ui::core::text::FontsExt;
use zero_ui::prelude::*;

use zero_ui_view_prebuilt as zero_ui_view;

fn main() {
    examples_util::print_info();
    zero_ui_view::init();

    //let rec = examples_util::record_profile("profile-text.json.gz", &[("example", &"text")], |_| true);

    // zero_ui_view::run_same_process(app_main);
    app_main();

    // rec.finish();
}

fn app_main() {
    App::default().run_window(|ctx| {
        let fs = var(Length::Pt(11.0));
        window! {
            title = fs.map(|s| formatx!("Text Example - font_size: {s}"));
            font_size = fs.clone();
            content_align = unset!;
            content = z_stack(widgets![
                h_stack! {
                    align = Align::CENTER;
                    spacing = 40;
                    items = widgets![
                        v_stack! {
                            spacing = 20;
                            items = widgets![
                                basic(),
                                defaults(ctx),
                            ];
                        },
                        v_stack! {
                            spacing = 20;
                            items = widgets![
                                line_height(),
                                line_spacing(),
                                word_spacing(),
                                letter_spacing(),
                            ];
                        },
                    ];
                },
                container! {
                    align = Align::TOP;
                    margin = 10;
                    content = font_size(fs);
                },
            ])
        }
    })
}

fn font_size(font_size: RcVar<Length>) -> impl Widget {
    fn change_size(font_size: &RcVar<Length>, change: f32, ctx: &mut WidgetContext) {
        font_size.modify(ctx, move |s| {
            **s += Length::Pt(change);
        });
    }
    h_stack! {
        button::theme::padding = (0, 5);
        spacing = 5;
        corner_radius = 4;
        background_color = rgba(0, 0, 0, 20.pct());
        padding = 4;
        items = widgets![
            button! {
                content = text("-");
                click_shortcut = [shortcut!(Minus), shortcut!(NumpadSubtract)];
                on_click = hn!(font_size, |ctx, _| {
                    change_size(&font_size, -1.0, ctx)
                });
            },
            text! {
                text = font_size.map(|s| formatx!("{s}"));
            },
            button! {
                content = text("+");
                click_shortcut = [shortcut!(Plus), shortcut!(NumpadAdd)];
                on_click = hn!(font_size, |ctx, _| {
                    change_size(&font_size, 1.0, ctx)
                });
            },
        ]
    }
}

fn basic() -> impl Widget {
    section(
        "basic",
        widgets![
            text("Basic Text"),
            strong("Strong Text"),
            em("Emphasis Text"),
            text! {
                color = colors::LIGHT_GREEN;
                text = "Colored Text";
            },
        ],
    )
}

fn line_height() -> impl Widget {
    section(
        "line_height",
        widgets![
            text! {
                text = "Default: 'Émp Giga Ç'";
                background_color = colors::LIGHT_BLUE;
                color = colors::BLACK;
            },
            text! {
                text = "150%: 'Émp Giga Ç'";
                background_color = colors::LIGHT_BLUE;
                color = colors::BLACK;
                line_height = 150.pct();
            },
        ],
    )
}

fn line_spacing() -> impl Widget {
    section(
        "line_spacing",
        widgets![container! {
            content = text! {
                text = "Hello line 1!\nHello line 2!\nHover to change `line_spacing`";
                background_color = rgba(0.5, 0.5, 0.5, 0.3);

                when self.is_hovered {
                    line_spacing = 30.pct();
                }
            };
            content_align = Align::TOP;
            height = 80;
        }],
    )
}

fn word_spacing() -> impl Widget {
    section(
        "word_spacing",
        widgets![text! {
            text = "Word spacing\n\thover to change";
            background_color = rgba(0.5, 0.5, 0.5, 0.3);

            when self.is_hovered {
                word_spacing = 100.pct();
            }
        }],
    )
}

fn letter_spacing() -> impl Widget {
    section(
        "letter_spacing",
        widgets![text! {
            text = "Letter spacing\n\thover to change";
            background_color = rgba(0.5, 0.5, 0.5, 0.3);

            when self.is_hovered {
                letter_spacing = 30.pct();
            }
        }],
    )
}

fn defaults(ctx: &mut WindowContext) -> impl Widget {
    fn demo(ctx: &mut WindowContext, title: &str, font_family: impl Into<FontNames>) -> impl Widget {
        let font_family = font_family.into();

        let font = ctx.services.fonts().get_list(
            &font_family,
            FontStyle::Normal,
            FontWeight::NORMAL,
            FontStretch::NORMAL,
            &lang!(und),
        );

        h_stack(widgets![
            text(if title.is_empty() {
                formatx!("{font_family}: ")
            } else {
                formatx!("{title}: ")
            }),
            text! {
                text = font.best().display_name().to_text();
                font_family;
            }
        ])
    }

    section(
        "defaults",
        widgets![
            // Generic
            demo(ctx, "", FontName::serif()),
            demo(ctx, "", FontName::sans_serif()),
            demo(ctx, "", FontName::monospace()),
            demo(ctx, "", FontName::cursive()),
            demo(ctx, "", FontName::fantasy()),
            demo(ctx, "Fallback", "not-a-font-get-fallback"),
            demo(ctx, "UI", FontNames::default())
        ],
    )
}

fn section(header: &'static str, items: impl WidgetList) -> impl Widget {
    v_stack! {
        spacing = 5;
        items = widgets![text! {
            text = header;
            font_weight = FontWeight::BOLD;
            margin = (0, 4);
        }].chain(items);
    }
}
