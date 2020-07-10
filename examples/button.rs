#![recursion_limit = "256"]

use enclose::enclose;

use zero_ui::prelude::*;

fn main() {
    better_panic::install();

    App::default().run_window(|_| {
        let size = var((800., 600.));
        let title = size.map(|s: &LayoutSize| formatx!("Button Example - {}x{}", s.width.ceil(), s.height.ceil()));
        window! {
            size: size;
            title: title;
            content: v_stack! {
                spacing: 5.0;
                items: ui_vec![example(), example()];
            };
        }
    })
}

fn example() -> impl Widget {
    let t = var("Click Me!");
    let background_color = rgb(0, 0, 0);
    // TODO fix bugs:
    // when self.is_state is not used in button, it is searched in button.
    // switch_var with more than 4 vars does not expand to a correct new(..) call.

    button! {
        on_click: enclose!{ (t) move |a| {
            let ctx = a.ctx();
            ctx.updates.push_set(&t, "Clicked!".into(), ctx.vars).unwrap();
        }};
        align: Alignment::CENTER;

        content: text(t);

        when self.is_pressed {
            background_color;
        }
    }
}
