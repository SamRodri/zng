#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use zero_ui::prelude::*;

use zero_ui::core::config::*;

use zero_ui::prelude::new_widget::{Dip, DipPoint, DipVector};
use zero_ui_view_prebuilt as zero_ui_view;

fn main() {
    examples_util::print_info();
    zero_ui_view::init();

    // let rec = examples_util::record_profile("button");

    // zero_ui_view::run_same_process(app_main);
    app_main();

    // rec.finish();
}

fn load_config() -> Box<dyn FallbackConfigReset> {
    // config file for the app, keys with prefix "main." are saved here.
    let user_cfg = JsonConfig::sync("target/tmp/example.config.json");
    // entries not found in `user_cfg` bind to this file first before going to embedded fallback.
    let default_cfg = ReadOnlyConfig::new(JsonConfig::sync("examples/res/config/defaults.json"));

    // the app settings.
    let main_cfg = FallbackConfig::new(user_cfg, default_cfg);

    // Clone a ref that can be used to reset specific entries.
    let main_ref = main_cfg.clone_boxed();

    // any other configs (Window::save_state for example)
    let other_cfg = JsonConfig::sync("target/tmp/example.config.other.json");

    CONFIG.load(SwitchConfig::new().with_prefix("main.", main_cfg).with_prefix("", other_cfg));

    main_ref
}

fn config_editor<T: ConfigValue, E: UiNode>(
    main_cfg_key: &'static str,
    default: impl FnOnce() -> T,
    main_cfg: Box<dyn FallbackConfigReset>,
    editor: impl FnOnce(BoxedVar<T>) -> E,
) -> impl UiNode {
    let main_cfg_key = ConfigKey::from_static(main_cfg_key);

    Container! {
        child = editor(CONFIG.get(formatx!("main.{main_cfg_key}"), default));
        child_insert_start = {
            insert: Icon! {
                enabled = main_cfg.can_reset(main_cfg_key.clone());
                on_click = hn!(|_| {
                    main_cfg.reset(&main_cfg_key);
                });

                ico = zero_ui_material_icons::outlined::SETTINGS_BACKUP_RESTORE;
                tooltip = Tip!(Text!("reset"));
                disabled_tooltip = Tip!(Text!("is default"));

                ico_size = 18;

                opacity = 70.pct();
                when *#is_cap_hovered {
                    opacity = 100.pct();
                }
                when *#is_disabled {
                    opacity = 30.pct();
                }
            },
            spacing: 5,
        }
    }
}

fn app_main() {
    App::default().extend(zero_ui_material_icons::MaterialFonts).run_window(async {
        let main_cfg = load_config();

        let checked = config_editor(
            "checked",
            || false,
            main_cfg.clone(),
            |checked| {
                Toggle! {
                    child = Text!(checked.map(|c| formatx!("Checked: {c:?}")));
                    checked = checked.clone();
                }
            },
        );
        let count = config_editor(
            "count",
            || 0,
            main_cfg.clone(),
            |count| {
                Button! {
                    child = Text!(count.map(|c| formatx!("Count: {c:?}")));
                    on_click = hn!(count, |_| {
                        count.modify(|c| *c.to_mut() += 1).unwrap();
                    })
                }
            },
        );
        let txt = config_editor(
            "txt",
            || Txt::from_static(""),
            main_cfg,
            |txt| {
                TextInput! {
                    txt;
                    min_width = 100;
                }
            },
        );

        Window! {
            title = if std::env::var("MOVE-TO").is_err() { "Config Example" } else { "Config Example - Other Process" };
            background = Text! {
                txt = CONFIG.status().map_to_text();
                margin = 10;
                font_family = "monospace";
                align = Align::TOP_LEFT;
                font_weight = FontWeight::BOLD;

                when *#{CONFIG.status().map(|s| s.is_err())} {
                    txt_color = colors::RED;
                }
            };
            child = Stack! {
                direction = StackDirection::top_to_bottom();
                align = Align::CENTER;
                spacing = 5;
                children = ui_vec![
                    checked,
                    count,
                    txt,
                    separator(),
                    Button! {
                        child = Text!("Open Another Process");
                        on_click = hn!(|_| {
                            let offset = Dip::new(30);
                            let pos = WINDOW.vars().actual_position().get() + DipVector::new(offset, offset);
                            let pos = pos.to_i32();
                            let r: Result<(), Box<dyn std::error::Error>> = (|| {
                                let exe = std::env::current_exe()?;
                                std::process::Command::new(exe).env("MOVE-TO", format!("{},{}", pos.x, pos.y)).spawn()?;
                                Ok(())
                            })();
                            match r {
                                Ok(_) => println!("Opened another process"),
                                Err(e) => eprintln!("Error opening another process, {e:?}"),
                            }
                        })
                    }
                ];
            };
            on_load = hn_once!(|_| {
                if let Ok(pos) = std::env::var("MOVE-TO") {
                    if let Some((x, y)) = pos.split_once(',') {
                        if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
                            let pos = DipPoint::new(Dip::new(x), Dip::new(y));
                            WINDOW.vars().position().set(pos);
                            WINDOWS.focus(WINDOW.id()).unwrap();
                        }
                    }
                }
            });
        }
    })
}

fn separator() -> impl UiNode {
    Hr! {
        color = rgba(1.0, 1.0, 1.0, 0.2);
        margin = (0, 8);
        line_style = LineStyle::Dashed;
    }
}
