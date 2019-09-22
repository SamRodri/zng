//#![windows_subsystem = "windows"]
#[macro_use]
extern crate derive_new;

pub mod app;
pub mod ui;
mod window;

use ui::*;
use webrender::api::{GradientStop, LayoutPoint};

fn main() {
    //start_logger_for("log_target");
    app::run(rgbaf(0.1, 0.2, 0.3, 1.0), LayoutSize::new(800., 600.), main_window);
}

fn main_window(_: &mut NextUpdate) -> impl Ui {
    let string = r".....
    ÃÂÃÂÃÂÃÂÃÂ
    Economize tempo no Word com novos botões que são mostrados
     no local em que você precisa deles.
     Para alterar a maneira como uma imagem se ajusta ao seu documento,
    clique nela e um botão de opções de layout será exibido ao lado.";
    v_stack(
        string
            .split("\n")
            .map(|l| {
                text(l, rgb(0, 0, 0))
                    .background_color(rgb(255, 255, 255))
                    .on_click(|_c, _| {
                        //c.stop_propagation();
                    })
            })
            .collect::<Vec<_>>(),
    )
    .font_family("Arial".to_owned())
    .font_size(14)
    .on_click(|_, u| {
        u.create_window(rgbaf(0.3, 0.2, 0.1, 1.0), LayoutSize::new(1000., 800.), other_widow);
    })
}

fn other_widow(_: &mut NextUpdate) -> impl Ui {
    h_stack(
        (0..4)
            .map(|_| {
                let bkg_color = Var::new(rgb(255, 255, 255));

                text("Ola", rgb(0, 0, 0))
                    .font_family("Arial".to_owned())
                    .font_size(90)
                    .background_color(Var::clone(&bkg_color))
                    .center()
                    .cursor(CursorIcon::Hand)
                    .on_mouse_enter(enclose! {(bkg_color) move |u| {
                        u.set(&bkg_color, rgb(100, 255, 255));
                    }})
                    .on_mouse_leave(move |u| {
                        u.set(&bkg_color, rgb(255, 255, 255));
                    })
                    .background_gradient(
                        LayoutPoint::new(0., 0.),
                        LayoutPoint::new(1., 1.),
                        vec![
                            GradientStop {
                                offset: 0.,
                                color: rgb(0, 200, 0),
                            },
                            GradientStop {
                                offset: 1.,
                                color: rgb(200, 0, 0),
                            },
                        ],
                    )
                    .width(200.)
                    .margin(2.)
            })
            .collect::<Vec<_>>(),
    )
    .center()
}
