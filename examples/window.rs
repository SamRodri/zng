#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use zero_ui::{
    access::ACCESS,
    app::EXIT_CMD,
    button,
    color::{
        color_scheme_map,
        filter::{backdrop_blur, drop_shadow, opacity},
        Rgba,
    },
    event::Command,
    focus::{directional_nav, focus_scope, tab_nav, DirectionalNav, TabNav},
    handler::WidgetHandler,
    image::ImageDataFormat,
    layout::*,
    prelude::*,
    scroll::ScrollMode,
    text::Strong,
    var::ArcVar,
    widget::{background_color, corner_radius, enabled, visibility, LineStyle},
    window::{native_dialog, FocusIndicator, FrameCaptureMode, FrameImageReadyArgs, WindowChangedArgs, WindowChrome, WindowState},
};

// use zero_ui_view_prebuilt as zero_ui_view;

fn main() {
    examples_util::print_info();
    // zero_ui_view::init();

    // let rec = examples_util::record_profile("window");

    zero_ui_view::run_same_process(app_main);
    // app_main();

    // rec.finish();
}

fn app_main() {
    APP.defaults().run_window(main_window());
}

async fn main_window() -> window::WindowRoot {
    // WINDOWS.exit_on_last_close().set(false);

    let window_vars = WINDOW.vars();
    let title = merge_var!(
        window_vars.actual_position(),
        window_vars.actual_size(),
        window_vars.scale_factor(),
        move |p: &DipPoint, s: &DipSize, f: &Factor| { formatx!("Window Example - position: {p:.0?}, size: {s:.0?}, factor: {f:?}") }
    );

    let background = var(colors::BLACK);

    Window! {
        background_color = background.easing(150.ms(), easing::linear);
        clear_color = rgba(0, 0, 0, 0);
        widget::foreground = Text! {
            txt = title.clone();
            visibility = window_vars.state().map(|s| s.is_fullscreen().into());
            align = Align::TOP;
            background_color = color_scheme_map(colors::BLACK, colors::WHITE);
            padding = 4;
            corner_radius = (0, 0, 5, 5);
        };
        title;
        on_state_changed = hn!(|args: &WindowChangedArgs| {
            tracing::info!("state: {:?}", args.new_state().unwrap());
        });
        on_close_requested = confirm_close();
        child_align = Align::CENTER;
        child = Stack! {
            direction = StackDirection::left_to_right();
            spacing = 40;
            children = ui_vec![
                Stack! {
                    direction = StackDirection::top_to_bottom();
                    spacing = 20;
                    children = ui_vec![
                        state_commands(),
                        focus_control(),
                    ]
                },
                Stack! {
                    direction = StackDirection::top_to_bottom();
                    spacing = 20;
                    children = ui_vec![
                        state(),
                        visibility_example(),
                        chrome(),
                    ];
                },
                Stack! {
                    direction = StackDirection::top_to_bottom();
                    spacing = 20;
                    children = ui_vec![
                        icon_example(),
                        background_color_example(background),
                    ];
                },
                Stack! {
                    direction = StackDirection::top_to_bottom();
                    spacing = 20;
                    children = ui_vec![
                        screenshot(),
                        misc(),
                        native(),
                    ];
                },
            ];
        };
    }
}

fn background_color_example(color: impl Var<Rgba>) -> impl UiNode {
    fn color_btn(c: impl Var<Rgba>, select_on_init: bool) -> impl UiNode {
        Toggle! {
            value::<Rgba> = c.clone();
            select_on_init;
            child = Stack! {
                direction = StackDirection::left_to_right();
                spacing = 4;
                children_align = Align::LEFT;
                children = ui_vec![
                    Wgt! {
                        background_color = c.clone();
                        size = (16, 16);
                    },
                    Text!(c.map_to_text()),
                ];
            };
        }
    }
    fn primary_color(c: Rgba) -> impl UiNode {
        let c = c.desaturate(50.pct());
        let c = color_scheme_map(rgba(0, 0, 0, 20.pct()).mix_normal(c), rgba(255, 255, 255, 20.pct()).mix_normal(c));
        color_btn(c, false)
    }

    select(
        "Background Color",
        color,
        ui_vec![
            color_btn(color_scheme_map(rgb(0.1, 0.1, 0.1), rgb(0.9, 0.9, 0.9)), true),
            primary_color(rgb(1.0, 0.0, 0.0)),
            primary_color(rgb(0.0, 0.8, 0.0)),
            primary_color(rgb(0.0, 0.0, 1.0)),
            primary_color(rgba(0, 0, 240, 20.pct())),
        ],
    )
}

fn screenshot() -> impl UiNode {
    fn of_window() -> impl UiNode {
        use std::time::Instant;
        let enabled = var(true);
        Button! {
            child = Text!(enabled.map(|&enabled| {
                if enabled {
                    "screenshot".to_text()
                } else {
                    "saving..".to_text()
                }
            }));
            on_click = async_hn!(enabled, |_| {
                // disable button until screenshot is saved.
                enabled.set(false);

                tracing::info!("taking `screenshot.png`..");

                let t = Instant::now();
                let img = WINDOW.frame_image(None).get();
                img.wait_done().await;
                tracing::info!("taken in {:?}, saving..", t.elapsed());

                let t = Instant::now();

                match img.save("screenshot.png").await {
                    Ok(_) => {
                        tracing::info!("saved in {:?}", t.elapsed());
                    }
                    Err(e) => {
                        tracing::error!("error {e}")
                    }
                }


                enabled.set(true);
            });
            enabled;
        }
    }

    fn of_headless_temp() -> impl UiNode {
        let enabled = var(true);
        Button! {
            child = Text!(enabled.map(|&enabled| {
                if enabled {
                    "headless".to_text()
                } else {
                    "saving..".to_text()
                }
            }));
            enabled = enabled.clone();
            on_click = hn!(|_| {
                enabled.set(false);

                tracing::info!("taking `screenshot.png` using a new headless window ..");
                WINDOWS.open_headless(async_clmv!(enabled, {
                    Window! {
                        size = (500, 400);
                        background_color = web_colors::DARK_GREEN;
                        font_size = 72;
                        child_align = Align::CENTER;
                        child = Text!("No Head!");

                        frame_capture_mode = FrameCaptureMode::Next;
                        on_frame_image_ready = async_hn_once!(|args: FrameImageReadyArgs| {
                            tracing::info!("saving screenshot..");
                            match args.frame_image.unwrap().save("screenshot.png").await {
                                Ok(_) => tracing::info!("saved"),
                                Err(e) => tracing::error!("{e}")
                            }
                            debug_assert_eq!(WINDOW.id(), args.window_id);
                            WINDOW.close();
                            enabled.set(true);
                        });
                    }
                }),
                    true
                );
            });
        }
    }

    section("Screenshot", ui_vec![of_window(), of_headless_temp(),])
}

fn icon_example() -> impl UiNode {
    let icon_btn = |label: &'static str, ico: WindowIcon| {
        Toggle! {
            child = Text!(label);
            value = ico;
        }
    };
    select(
        "Icon",
        WINDOW.vars().icon(),
        ui_vec![
            icon_btn("Default", WindowIcon::Default),
            icon_btn("Png File", "examples/res/window/icon-file.png".into()),
            icon_btn("Png Bytes", include_bytes!("res/window/icon-bytes.png").into()),
            icon_btn("Raw BGRA", {
                let color = [0, 0, 255, 255 / 2];

                let size = PxSize::new(Px(32), Px(32));
                let len = size.width.0 * size.height.0 * 4;
                let bgra: Vec<u8> = color.iter().copied().cycle().take(len as usize).collect();

                (bgra, ImageDataFormat::from(size)).into()
            }),
            icon_btn(
                "Render",
                WindowIcon::render(|| Text! {
                    size = (36, 36);
                    font_size = 28;
                    font_weight = FontWeight::BOLD;
                    txt_align = Align::CENTER;
                    txt = "W";
                    drop_shadow = {
                        offset: (2, 2),
                        blur_radius: 5,
                        color: colors::BLACK,
                    };
                })
            )
        ],
    )
}

fn chrome() -> impl UiNode {
    let chrome_btn = |c: WindowChrome| {
        Toggle! {
            child = Text!("{c:?}");
            value = c;
        }
    };
    select(
        "Chrome",
        WINDOW.vars().chrome(),
        ui_vec![
            chrome_btn(WindowChrome::Default),
            chrome_btn(WindowChrome::None),
            Text!("TODO custom"),
        ],
    )
}

fn state_commands() -> impl UiNode {
    use zero_ui::window::cmd::*;

    let window_id = WINDOW.id();

    section(
        "Commands",
        ui_vec![
            cmd_btn(MINIMIZE_CMD.scoped(window_id)),
            separator(),
            cmd_btn(RESTORE_CMD.scoped(window_id)),
            cmd_btn(MAXIMIZE_CMD.scoped(window_id)),
            separator(),
            cmd_btn(FULLSCREEN_CMD.scoped(window_id)),
            cmd_btn(EXCLUSIVE_FULLSCREEN_CMD.scoped(window_id)),
            separator(),
            cmd_btn(CLOSE_CMD.scoped(window_id)),
            cmd_btn(EXIT_CMD),
        ],
    )
}

fn focus_control() -> impl UiNode {
    let enabled = var(true);
    let focus_btn = Button! {
        enabled = enabled.clone();
        child = Text!("Focus in 5s");
        on_click = async_hn!(enabled, |_| {
            enabled.set(false);
            task::deadline(5.secs()).await;

            WINDOWS.focus(WINDOW.id()).unwrap();
            enabled.set(true);
        });
    };

    let enabled = var(true);
    let critical_btn = Button! {
        enabled = enabled.clone();
        child = Text!("Critical Alert in 5s");
        on_click = async_hn!(enabled, |_| {
            enabled.set(false);
            task::deadline(5.secs()).await;

            WINDOW.vars().focus_indicator().set(FocusIndicator::Critical);
            enabled.set(true);
        });
    };

    let enabled = var(true);
    let info_btn = Button! {
        enabled = enabled.clone();
        child = Text!("Info Alert in 5s");
        on_click = async_hn!(enabled, |_| {
            enabled.set(false);
            task::deadline(5.secs()).await;

            WINDOW.vars().focus_indicator().set(FocusIndicator::Info);
            enabled.set(true);
        });
    };

    section("Focus", ui_vec![focus_btn, critical_btn, info_btn,])
}

fn state() -> impl UiNode {
    let state_btn = |s: WindowState| {
        Toggle! {
            child = Text!("{s:?}");
            value = s;
        }
    };

    select(
        "State",
        WINDOW.vars().state(),
        ui_vec![
            state_btn(WindowState::Minimized),
            separator(),
            state_btn(WindowState::Normal),
            state_btn(WindowState::Maximized),
            separator(),
            state_btn(WindowState::Fullscreen),
            Stack! {
                direction = StackDirection::top_to_bottom();
                children = ui_vec![
                    Toggle! {
                        child = Text!("Exclusive");
                        value = WindowState::Exclusive;
                        corner_radius = (4, 4, 0, 0);
                    },
                    exclusive_mode(),
                ]
            }
        ],
    )
}

fn exclusive_mode() -> impl UiNode {
    Toggle! {
        style_fn = toggle::ComboStyle!();
        corner_radius = (0, 0, 4, 4);

        tooltip = Tip!(Text!("Exclusive video mode"));

        child = Text! {
            txt = WINDOW.vars().video_mode().map_to_text();
            txt_align = Align::CENTER;
            padding = 2;
        };
        checked_popup = wgt_fn!(|_| {
            let vars = WINDOW.vars();
            let selected_opt = vars.video_mode();
            let default_opt = zero_ui::window::VideoMode::MAX;
            let opts = vars.video_modes().get();
            popup::Popup! {
                max_height = 80.vh_pct();
                child = Scroll! {
                    mode = ScrollMode::VERTICAL;
                    child = Stack! {
                        toggle::selector = toggle::Selector::single(selected_opt);
                        direction = StackDirection::top_to_bottom();
                        children = [default_opt].into_iter().chain(opts).map(|o| Toggle! {
                            child = Text!(formatx!("{o}"));
                            value = o;
                        })
                        .collect::<UiNodeVec>();
                    }
                };
            }
        });
    }
}

fn visibility_example() -> impl UiNode {
    let visible = WINDOW.vars().visible();
    let btn = Button! {
        enabled = visible.clone();
        child = Text!("Hide for 1s");
        on_click = async_hn!(visible, |_| {
            visible.set(false);
            tracing::info!("visible=false");
            task::deadline(1.secs()).await;
            visible.set(true);
            tracing::info!("visible=true");
        });
    };

    section("Visibility", ui_vec![btn])
}

fn misc() -> impl UiNode {
    let window_vars = WINDOW.vars();
    let window_id = WINDOW.id();

    let can_open_windows = window_vars.state().map(|&s| s != WindowState::Exclusive);
    section(
        "Misc.",
        ui_vec![
            Toggle! {
                child = Text!("Taskbar Visible");
                checked = window_vars.taskbar_visible();
            },
            Toggle! {
                child = Text!("Always on Top");
                checked = window_vars.always_on_top();
            },
            separator(),
            cmd_btn(zero_ui::window::cmd::INSPECT_CMD.scoped(window_id)),
            separator(),
            {
                let mut child_count = 0;
                Button! {
                    child = Text!("Open Child Window");
                    enabled = can_open_windows.clone();
                    on_click = hn!(|_| {
                        child_count += 1;

                        let parent = WINDOW.id();
                        WINDOWS.open(async move {
                            Window! {
                                title = formatx!("Window Example - Child {child_count}");
                                size = (400, 300);
                                parent;
                                child_align = Align::CENTER;
                                start_position = window::StartPosition::CenterParent;
                                child = Text! {
                                    txt = formatx!("Child {child_count}");
                                    font_size = 20;
                                };
                            }
                        });
                    })
                }
            },
            {
                let mut other_count = 0;
                Button! {
                    child = Text!("Open Other Window");
                    enabled = can_open_windows;
                    on_click = hn!(|_| {
                        other_count += 1;

                        WINDOWS.open(async move {
                            Window! {
                                title = formatx!("Window Example - Other {other_count}");
                                size = (400, 300);
                                child_align = Align::CENTER;
                                child = Text! {
                                    txt = formatx!("Other {other_count}");
                                    font_size = 20;
                                };
                            }
                        });
                    })
                }
            }
        ],
    )
}

fn native() -> impl UiNode {
    section(
        "Native Dialogs",
        ui_vec![
            Button! {
                child = Text!("Messages");
                tooltip = Tip!(Text!(r#"Shows a "Yes/No" message, then an "Ok" message dialogs."#));
                on_click = async_hn!(|_| {
                    let rsp = WINDOWS.native_message_dialog(WINDOW.id(),  native_dialog::MsgDialog {
                        title: Txt::from_static("Question?"),
                        message: Txt::from_static("Example message. Yes -> Warn, No -> Error."),
                        icon:  native_dialog::MsgDialogIcon::Info,
                        buttons:  native_dialog::MsgDialogButtons::YesNo,
                    }).wait_rsp().await;
                    let icon = match rsp {
                        native_dialog::MsgDialogResponse::Yes => {
                            native_dialog::MsgDialogIcon::Warn
                        },
                        native_dialog::MsgDialogResponse::No => {
                            native_dialog::MsgDialogIcon::Error
                        }
                        e => {
                            tracing::error!("unexpected message response {e:?}");
                            return;
                        },
                    };
                    WINDOWS.native_message_dialog(WINDOW.id(),  native_dialog::MsgDialog {
                        title: Txt::from_static("Title"),
                        message: Txt::from_static("Message"),
                        icon,
                        buttons:  native_dialog::MsgDialogButtons::Ok,
                    });
                });
            },
            Button! {
                child = Text!("File Picker");
                tooltip = Tip!(Text!(r#"Shows a "Directory Picker", then an "Open Many Files", then a "Save File" dialogs."#));
                on_click = async_hn!(|_| {
                    let res = WINDOWS.native_file_dialog(WINDOW.id(),  native_dialog::FileDialog {
                        title: "Select Dir".into(),
                        starting_dir: "".into(),
                        starting_name: "".into(),
                        filters: "".into(),
                        kind:  native_dialog::FileDialogKind::SelectFolder,
                    }).wait_rsp().await;
                    let dir = match res {
                        native_dialog::FileDialogResponse::Selected(mut s) => {
                            s.remove(0)
                        }
                        native_dialog::FileDialogResponse::Cancel => {
                            tracing::info!("canceled");
                            return;
                        }
                        native_dialog::FileDialogResponse::Error(e) => {
                            tracing::error!("unexpected select dir response {e:?}");
                            return;
                        }
                    };

                    let mut dlg =  native_dialog::FileDialog {
                        title: "Open Files".into(),
                        starting_dir: dir,
                        starting_name: "".into(),
                        filters: "".into(),
                        kind:  native_dialog::FileDialogKind::OpenFiles,
                    };
                    dlg.push_filter("Text", &["*.txt", "*.md"]);
                    dlg.push_filter("All", &["*.*"]);
                    let res = WINDOWS.native_file_dialog(WINDOW.id(), dlg.clone()).wait_rsp().await;
                    let first_file = match res {
                        native_dialog::FileDialogResponse::Selected(mut s) => {
                            tracing::info!("selected {} file(s)", s.len());
                            s.remove(0)
                        }
                        native_dialog::FileDialogResponse::Cancel => {
                            tracing::info!("canceled");
                            return;
                        }
                        native_dialog::FileDialogResponse::Error(e) => {
                            tracing::error!("unexpected open files response {e:?}");
                            return;
                        }
                    };

                    dlg.title = "Save File".into();
                    dlg.kind =  native_dialog::FileDialogKind::SaveFile;
                    dlg.starting_dir = first_file.parent().map(|p| p.to_owned()).unwrap_or_default();
                    dlg.starting_name = first_file.file_name().map(|p| Txt::from_str(&p.to_string_lossy())).unwrap_or_default();
                    let res = WINDOWS.native_file_dialog(WINDOW.id(), dlg.clone()).wait_rsp().await;
                    let save_file = match res {
                        native_dialog::FileDialogResponse::Selected(mut s) => {
                            s.remove(0)
                        }
                        native_dialog::FileDialogResponse::Cancel => {
                            tracing::info!("canceled");
                            return;
                        }
                        native_dialog::FileDialogResponse::Error(e) => {
                            tracing::error!("unexpected save file response {e:?}");
                            return;
                        }
                    };
                    tracing::info!("save {}", save_file.display());
                });
            }
        ],
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CloseState {
    Ask,
    Asking,
    Close,
}
fn confirm_close() -> impl WidgetHandler<WindowCloseRequestedArgs> {
    let state = var(CloseState::Ask);
    hn!(|args: &WindowCloseRequestedArgs| {
        match state.get() {
            CloseState::Ask => {
                args.propagation().stop();
                state.set(CloseState::Asking);

                let dlg = close_dialog(args.windows.iter().copied().collect(), state.clone());
                LAYERS.insert(LayerIndex::TOP_MOST, dlg)
            }
            CloseState::Asking => args.propagation().stop(),
            CloseState::Close => {}
        }
    })
}

fn close_dialog(windows: Vec<WindowId>, state: ArcVar<CloseState>) -> impl UiNode {
    let opacity = var(0.fct());
    opacity.ease(1.fct(), 300.ms(), easing::linear).perm();
    Container! {
        opacity = opacity.clone();

        id = "close-dialog";
        widget::modal = true;
        backdrop_blur = 2;
        background_color = color_scheme_map(colors::WHITE.with_alpha(10.pct()), colors::BLACK.with_alpha(10.pct()));
        child_align = Align::CENTER;
        gesture::on_click = hn!(|args: &gesture::ClickArgs| {
            if WIDGET.id() == args.target.widget_id() {
                args.propagation().stop();
                ACCESS.click(WINDOW.id(), "cancel-btn", true);
            }
        });
        child = Container! {
            background_color = color_scheme_map(colors::BLACK.with_alpha(90.pct()), colors::WHITE.with_alpha(90.pct()));
            focus_scope = true;
            tab_nav = TabNav::Cycle;
            directional_nav = DirectionalNav::Cycle;
            drop_shadow = (0, 0), 4, colors::BLACK;
            padding = 4;

            button::extend_style = Style! {
                padding = 4;
                corner_radius = unset!;
            };

            child = Stack! {
                direction = StackDirection::top_to_bottom();
                children_align = Align::RIGHT;
                children = ui_vec![
                    Text! {
                        txt_selectable = true;
                        txt = match windows.len() {
                            1 => "Close Confirmation\n\nClose 1 window?".to_text(),
                            n => formatx!("Close Confirmation\n\nClose {n} windows?")
                        };
                        margin = 15;
                    },
                    Stack! {
                        direction = StackDirection::left_to_right();
                        spacing = 4;
                        children = ui_vec![
                            Button! {
                                focus_on_init = true;
                                child = Strong!("Close");
                                on_click = hn_once!(state, |_| {
                                    state.set(CloseState::Close);
                                    WINDOWS.close_together(windows).unwrap();
                                })
                            },
                            Button! {
                                id = "cancel-btn";
                                child = Text!("Cancel");
                                on_click = async_hn!(opacity, state, |_| {
                                    opacity.ease(0.fct(), 150.ms(), easing::linear).perm();
                                    opacity.wait_animation().await;

                                    state.set(CloseState::Ask);
                                    LAYERS.remove("close-dialog");
                                });
                            },
                        ]
                    }
                ]
            }
        }
    }
}

fn cmd_btn(cmd: Command) -> impl UiNode {
    Button! {
        child = Text!(cmd.name_with_shortcut());
        enabled = cmd.is_enabled();
        visibility = cmd.has_handlers().map_into();
        on_click = hn!(|_| {
            cmd.notify();
        })
    }
}

fn section(header: &'static str, items: impl UiNodeList) -> impl UiNode {
    Stack! {
        direction = StackDirection::top_to_bottom();
        spacing = 5;
        children = ui_vec![Text! {
            txt = header;
            font_weight = FontWeight::BOLD;
            margin = (0, 4);
        }].chain(items);
    }
}

fn select<T: VarValue + PartialEq>(header: &'static str, selection: impl Var<T>, items: impl UiNodeList) -> impl UiNode {
    Stack! {
        direction = StackDirection::top_to_bottom();
        spacing = 5;
        toggle::selector = toggle::Selector::single(selection);
        children = ui_vec![Text! {
            txt = header;
            font_weight = FontWeight::BOLD;
            margin = (0, 4);
        }].chain(items);
    }
}

fn separator() -> impl UiNode {
    Hr! {
        color = rgba(1.0, 1.0, 1.0, 0.2);
        margin = (0, 8);
        line_style = LineStyle::Dashed;
    }
}
