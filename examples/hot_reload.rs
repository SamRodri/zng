//! Demonstrates the `"hot_reload"` feature.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use zng::prelude::*;

use zng::view_process::prebuilt as view_process;

fn main() {
    examples_util::print_info();
    view_process::init();
    zng::app::crash_handler::init_debug();
    app_main();
}

fn app_main() {
    // examples/Cargo.toml enables the `"hot_reload"` feature for `zng`,
    // so the hot reload extension is available in `APP.defaults()`.
    APP.defaults().run_window(async {
        Window! {
            title = "Hot Reload Example";

            child = Container! {
                // hot reloading node, edit the code in `examples/hot-reload-lib` to see updates.
                child = examples_hot_reload::hot_node();
            };

             // layout affects the hot node correctly.
            child_align = Align::CENTER;
            // context values propagate to the hot node correctly.
            text::font_size = 2.em();

        }
    })
}
