#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use zero_ui::prelude::*;

use zero_ui::core::config::*;

use zero_ui_view_prebuilt as zero_ui_view;

fn main() {
    examples_util::print_info();
    zero_ui_view::init();

    // let rec = examples_util::record_profile("button");

    // zero_ui_view::run_same_process(app_main);
    app_main();

    // rec.finish();
}

fn app_main() {
    App::default().run_window(|ctx| {
        let cfg = Config::req(ctx);
        cfg.set_backend(ConfigFile::new("target/tmp/example.config.json", true, 3.secs()));

        let checked = cfg.var("main.checked", || false);
        let count = cfg.var("main.count", || 0);

        window! {
            title = "Config Example";
            background = text! {
                text = cfg.status().map_to_text();
                margin = 10;
                font_family = "monospace";
                align = Align::TOP_LEFT;
            };
            content = v_stack! {
                align = Align::CENTER;
                spacing = 5;
                items = widgets![                    
                    button! {
                        content = text(checked.map(|c| formatx!("Checked: {c:?}")));
                        on_click = hn!(checked, |ctx, _| {
                            checked.modify(ctx, |mut c| *c = !*c).unwrap();
                        });
                    },
                    button! {
                        content = text(count.map(|c| formatx!("Count: {c:?}")));
                        on_click = hn!(count, |ctx, _| {
                            count.modify(ctx, |mut c| *c += 1).unwrap();
                        })
                    },
                    separator(),
                    button! {
                        content = text("Reset");
                        on_click = hn!(|ctx, _| {
                            checked.set_ne(ctx, false).unwrap();
                            count.set_ne(ctx, 0).unwrap();
                        })
                    }
                ];
            };
        }
    })
}

fn separator() -> impl Widget {
    hr! {
        color = rgba(1.0, 1.0, 1.0, 0.2);
        margin = (0, 8);
        style = LineStyle::Dashed;
    }
}
