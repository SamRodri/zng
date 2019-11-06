//#![windows_subsystem = "windows"]

use zero_ui::{core::*, primitive::*, *};

fn main() {
    //start_logger_for("test");
    app::run(rgba(0.1, 0.2, 0.3, 1.0), LayoutSize::new(800., 600.), window);
}

fn window(_: &mut NextUpdate) -> impl Ui {
    h_stack((0..4).map(item).collect::<Vec<_>>()).center()
}

fn item(i: usize) -> impl Ui {
    let border = Var::new(rgba(0, 0, 0, 0.0));
    let text_border = Var::new(rgba(0, 0, 0, 0.0));
    text("Ola")
        .font_family("Arial".to_owned())
        .font_size(90)
        .background_color(rgba(1., 1., 1., 0.5))
        .border(4., (Var::clone(&text_border), BorderStyle::Dashed))
        .text_color(rgb(0, 0, 0))
        .focusable()
        .focused(i == 2)
        .on_key_down(move |k, _| println!("Key down @ {}: {}", i, k))
        .center()
        .cursor(CursorIcon::Hand)
        .on_focus(enclose! {(text_border) move |u| {
            u.set(&text_border,rgb(145, 218, 255));
        }})
        .on_blur(move |u| {
            u.set(&text_border, rgba(0, 0, 0, 0.0));
        })
        .background_gradient((0., 0.), (1., 1.), vec![rgb(0, 200, 0), rgb(200, 0, 0)])
        .border(4., (Var::clone(&border), BorderStyle::Dashed))
        .margin(2.)
        .width(200.)
        .focusable()
        .on_focus(enclose! {(border)move |u| {
            u.set(&border, rgb(145, 218, 255));
        }})
        .on_blur(move |u| {
            u.set(&border, rgba(0, 0, 0, 0.0));
        })
}
