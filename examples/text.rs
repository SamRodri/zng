#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use enclose::enclose;
use zero_ui::{core::units::pt_to_layout, prelude::*};

fn main() {
    App::default().run_window(|_| {
        let fs = var(Length::pt(11.0));
        window! {
            title: fs.map(|s| formatx!("Text Example - font_size: {}", s));
            font_size: fs.clone();
            content: h_stack! {
                spacing: 40;
                items: (
                    basic(),
                    line_height(),
                    pre_line_break(),
                    font_size(fs),
                );
            };
        }
    })
}

fn font_size(font_size: RcVar<Length>) -> impl Widget {
    fn change_size(font_size: &RcVar<Length>, change: f32, ctx: &mut WidgetContext) {
        let mut size = match font_size.get(ctx.vars) {
            Length::Exact(s) => *s,
            _ => todo!(),
        };
        size += pt_to_layout(change).get();
        font_size.set(ctx.vars, size.into());
    }
    section(
        "font_size",
        (
            button! {
                content: text("Increase Size");
                on_click: enclose!{ (font_size) move |ctx, _| {
                    change_size(&font_size, 1.0, ctx)
                }};
            },
            button! {
                content: text("Decrease Size");
                on_click: enclose!{ (font_size) move |ctx, _| {
                    change_size(&font_size, -1.0, ctx)
                }};
            },
        ),
    )
}

fn basic() -> impl Widget {
    section(
        "basic",
        (
            text("Basic Text"),
            strong("Strong Text"),
            em("Emphasis Text"),
            text! {
                color: colors::LIGHT_GREEN;
                text: "Colored Text";
            },
        ),
    )
}

fn line_height() -> impl Widget {
    section(
        "line_height",
        (
            text! {
                text: "Default: 'Émp Giga Ç'";
                background_color: colors::LIGHT_BLUE;
                color: colors::BLACK;
            },
            text! {
                text: "1.3em: 'Émp Giga Ç'";
                background_color: colors::LIGHT_BLUE;
                color: colors::BLACK;
                line_height: 1.3.em();
            },
        ),
    )
}

fn pre_line_break() -> impl Widget {
    section(
        "line_break",
        [text! {
            text: "Hello line 1!\n    Hello line 2!";
            background_color: rgba(1.0, 1.0, 1.0, 0.3);
        }],
    )
}

fn section(header: &'static str, items: impl UiList) -> impl Widget {
    v_stack! {
        spacing: 5;
        items: [text! {
            text: header;
            font_weight: FontWeight::BOLD;
            margin: (0, 4);
        }].chain(items);
    }
}
