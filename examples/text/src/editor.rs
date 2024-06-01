use std::sync::Arc;

use zng::{
    app::{NEW_CMD, OPEN_CMD, SAVE_AS_CMD, SAVE_CMD},
    button,
    clipboard::{COPY_CMD, CUT_CMD, PASTE_CMD},
    color::filter::opacity,
    focus::{alt_focus_scope, focus_click_behavior, FocusClickBehavior},
    gesture::click_shortcut,
    icon::{self, Icon},
    layout::{align, margin, padding, Dip},
    prelude::*,
    rule_line,
    scroll::ScrollMode,
    undo::UNDO_CMD,
    var::ArcVar,
    widget::{corner_radius, enabled, visibility, Visibility},
    window::{native_dialog, WindowRoot},
};

pub fn text_editor() -> impl UiNode {
    let is_open = var(false);

    Button! {
        child = Text!(is_open.map(|&i| if i { "show text editor" } else { "open text editor" }.into()));
        style_fn = button::LinkStyle!();
        on_click = hn!(|_| {
            let editor_id = WindowId::named("text-editor");
            if is_open.get() {
                if WINDOWS.focus(editor_id).is_err() {
                    is_open.set(false);
                }
            } else {
                WINDOWS.open_id(editor_id, async_clmv!(is_open, {
                    text_editor_window(is_open)
                }));
            }
        });
    }
}

fn text_editor_window(is_open: ArcVar<bool>) -> WindowRoot {
    let editor = TextEditor::init();
    Window! {
        title = editor.title();
        on_open = hn!(is_open, |_| {
            is_open.set(true);
        });
        on_close = hn!(is_open, |_| {
            is_open.set(false);
        });
        enabled = editor.enabled();
        on_close_requested = async_hn!(editor, |args: WindowCloseRequestedArgs| {
            editor.on_close_requested(args).await;
        });
        min_width = 450;

        child_top = text_editor_menu(editor.clone()), 0;

        child = Scroll! {
            mode = ScrollMode::VERTICAL;
            child_align = Align::FILL;
            scroll_to_focused_mode = None;

            // line numbers
            child_start = Text! {
                padding = (7, 4);
                txt_align = Align::TOP_RIGHT;
                opacity = 80.pct();
                layout::min_width = 24;
                txt = editor.lines.map(|s| {
                    use std::fmt::Write;
                    let mut txt = String::new();
                    match s {
                        text::LinesWrapCount::NoWrap(len) => {
                            for i in 1..=(*len).max(1) {
                                let _ = writeln!(&mut txt, "{i}");
                            }
                        },
                        text::LinesWrapCount::Wrap(counts) => {
                            for (i, &c) in counts.iter().enumerate() {
                                let _ = write!(&mut txt, "{}", i + 1);
                                for _ in 0..c {
                                    txt.push('\n');
                                }
                            }
                        }
                    }
                    Txt::from_str(&txt)
                });
            }, 0;

            // editor
            child = TextInput! {
                id = editor.input_wgt_id();
                txt = editor.txt.clone();
                accepts_tab = true;
                accepts_enter = true;
                get_caret_status = editor.caret_status.clone();
                get_lines_wrap_count = editor.lines.clone();
                widget::border = unset!;
            };
        };

        child_bottom = Text! {
            margin = (0, 4);
            align = Align::RIGHT;
            txt = editor.caret_status.map_to_txt();
        }, 0;
    }
}

fn text_editor_menu(editor: Arc<TextEditor>) -> impl UiNode {
    let menu_width = var(Dip::MAX);
    let gt_700 = menu_width.map(|&w| Visibility::from(w > Dip::new(700)));
    let gt_600 = menu_width.map(|&w| Visibility::from(w > Dip::new(600)));
    let gt_500 = menu_width.map(|&w| Visibility::from(w > Dip::new(500)));

    let clipboard_btn = clmv!(gt_600, |cmd: zng::event::Command| {
        let cmd = cmd.focus_scoped();
        Button! {
            child = widget::node::presenter((), cmd.flat_map(|c| c.icon()));
            child_right = Text!(txt = cmd.flat_map(|c| c.name()); visibility = gt_600.clone()), 4;
            tooltip = Tip!(Text!(cmd.flat_map(|c|c.name_with_shortcut())));
            visibility = true;
            cmd;
        }
    });

    let undo_combo = clmv!(gt_700, |op: zng::undo::UndoOp| {
        let cmd = op.cmd().undo_scoped();

        Toggle! {
            style_fn = toggle::ComboStyle!();

            widget::enabled = cmd.flat_map(|c| c.is_enabled());

            child = Button! {
                child = widget::node::presenter((), cmd.flat_map(|c| c.icon()));
                child_right = Text!(txt = cmd.flat_map(|c| c.name()); visibility = gt_700.clone()), 4;
                tooltip = Tip!(Text!(cmd.flat_map(|c|c.name_with_shortcut())));
                on_click = hn!(|a: &gesture::ClickArgs| {
                    a.propagation().stop();
                    cmd.get().notify();
                });
            };

            checked_popup = wgt_fn!(|_| popup::Popup! {
                child = zng::undo::history::UndoHistory!(op);
            });
        }
    });

    Stack! {
        id = "menu";
        align = Align::FILL_TOP;
        alt_focus_scope = true;
        focus_click_behavior = FocusClickBehavior::Exit;
        spacing = 4;
        direction = StackDirection::left_to_right();
        padding = 4;
        layout::actual_width = menu_width;
        button::style_fn = Style! {
            padding = (2, 4);
            corner_radius = 2;
            icon::ico_size = 16;
        };
        rule_line::vr::margin = 0;
        children = ui_vec![
            Button! {
                child = Icon!(icon::material_sharp::INSERT_DRIVE_FILE);
                child_right = Text!(txt = NEW_CMD.name(); visibility = gt_500.clone()), 4;
                tooltip = Tip!(Text!(NEW_CMD.name_with_shortcut()));

                click_shortcut = NEW_CMD.shortcut();
                on_click = async_hn!(editor, |_| {
                    editor.create().await;
                });
            },
            Button! {
                child = Icon!(icon::material_sharp::FOLDER_OPEN);
                child_right = Text!(txt = OPEN_CMD.name(); visibility = gt_500.clone()), 4;
                tooltip = Tip!(Text!(OPEN_CMD.name_with_shortcut()));

                click_shortcut = OPEN_CMD.shortcut();
                on_click = async_hn!(editor, |_| {
                    editor.open().await;
                });
            },
            Button! {
                child = Icon!(icon::material_sharp::SAVE);
                child_right = Text!(txt = SAVE_CMD.name(); visibility = gt_500.clone()), 4;
                tooltip = Tip!(Text!(SAVE_CMD.name_with_shortcut()));

                enabled = editor.unsaved();
                click_shortcut = SAVE_CMD.shortcut();
                on_click = async_hn!(editor, |_| {
                    editor.save().await;
                });
            },
            Button! {
                child = Text!(SAVE_AS_CMD.name());
                when #{gt_500}.is_collapsed() {
                    child = Icon!(icon::material_sharp::SAVE_AS);
                }

                tooltip = Tip!(Text!(SAVE_AS_CMD.name_with_shortcut()));

                click_shortcut = SAVE_AS_CMD.shortcut();
                on_click = async_hn!(editor, |_| {
                    editor.save_as().await;
                });
            },
            rule_line::vr::Vr!(),
            clipboard_btn(CUT_CMD),
            clipboard_btn(COPY_CMD),
            clipboard_btn(PASTE_CMD),
            rule_line::vr::Vr!(),
            undo_combo(zng::undo::UndoOp::Undo),
            undo_combo(zng::undo::UndoOp::Redo),
        ]
    }
}
struct TextEditor {
    input_wgt_id: WidgetId,
    file: ArcVar<Option<std::path::PathBuf>>,
    txt: ArcVar<Txt>,

    txt_touched: ArcVar<bool>,

    caret_status: ArcVar<text::CaretStatus>,
    lines: ArcVar<text::LinesWrapCount>,
    busy: ArcVar<u32>,
}
impl TextEditor {
    pub fn init() -> Arc<Self> {
        let txt = var(Txt::from_static(""));
        let unsaved = var(false);
        txt.bind_map(&unsaved, |_| true).perm();
        Arc::new(Self {
            input_wgt_id: WidgetId::new_unique(),
            file: var(None),
            txt,
            txt_touched: unsaved,
            caret_status: var(text::CaretStatus::none()),
            lines: var(text::LinesWrapCount::NoWrap(0)),
            busy: var(0),
        })
    }

    pub fn input_wgt_id(&self) -> WidgetId {
        self.input_wgt_id
    }

    pub fn title(&self) -> impl Var<Txt> {
        merge_var!(self.unsaved(), self.file.clone(), |u, f| {
            let mut t = "Text Example - Editor".to_owned();
            if *u {
                t.push('*');
            }
            if let Some(f) = f {
                use std::fmt::Write;
                let _ = write!(&mut t, " - {}", f.display());
            }
            Txt::from_str(&t)
        })
    }

    pub fn unsaved(&self) -> impl Var<bool> {
        let can_undo = UNDO_CMD.scoped(self.input_wgt_id).is_enabled();
        merge_var!(self.txt_touched.clone(), can_undo, |&t, &u| t && u)
    }

    pub fn enabled(&self) -> impl Var<bool> {
        self.busy.map(|&b| b == 0)
    }

    pub async fn create(&self) {
        let _busy = self.enter_busy();

        if self.handle_unsaved().await {
            self.txt.set(Txt::from_static(""));
            self.file.set(None);
            self.txt_touched.set(false);
        }
    }

    pub async fn open(&self) {
        let _busy = self.enter_busy();

        if !self.handle_unsaved().await {
            return;
        }

        let mut dlg = native_dialog::FileDialog {
            title: "Open Text".into(),
            kind: native_dialog::FileDialogKind::OpenFile,
            ..Default::default()
        };
        dlg.push_filter("Text Files", &["txt", "md"]).push_filter("All Files", &["*"]);
        let r = WINDOWS.native_file_dialog(WINDOW.id(), dlg).wait_rsp().await;
        match r {
            native_dialog::FileDialogResponse::Selected(mut s) => {
                let file = s.remove(0);
                let r = task::wait(clmv!(file, || std::fs::read_to_string(file))).await;
                match r {
                    Ok(t) => {
                        self.txt.set(Txt::from_str(&t));
                        self.txt_touched.set(false);
                        self.file.set(file);
                    }
                    Err(e) => {
                        self.handle_error("reading file", e.to_txt()).await;
                    }
                }
            }
            native_dialog::FileDialogResponse::Cancel => {}
            native_dialog::FileDialogResponse::Error(e) => {
                self.handle_error("opening file", e).await;
            }
        }
    }

    pub async fn save(&self) -> bool {
        if let Some(file) = self.file.get() {
            let _busy = self.enter_busy();
            let ok = self.write(file).await;
            self.txt_touched.set(!ok);
            ok
        } else {
            self.save_as().await
        }
    }

    pub async fn save_as(&self) -> bool {
        let _busy = self.enter_busy();

        let mut dlg = native_dialog::FileDialog {
            title: "Save Text".into(),
            kind: native_dialog::FileDialogKind::SaveFile,
            ..Default::default()
        };
        dlg.push_filter("Text", &["txt"])
            .push_filter("Markdown", &["md"])
            .push_filter("All Files", &["*"]);
        let r = WINDOWS.native_file_dialog(WINDOW.id(), dlg).wait_rsp().await;
        match r {
            native_dialog::FileDialogResponse::Selected(mut s) => {
                if let Some(file) = s.pop() {
                    let ok = self.write(file.clone()).await;
                    self.txt_touched.set(!ok);
                    if ok {
                        self.file.set(Some(file));
                    }
                    return ok;
                }
            }
            native_dialog::FileDialogResponse::Cancel => {}
            native_dialog::FileDialogResponse::Error(e) => {
                self.handle_error("saving file", e.to_txt()).await;
            }
        }

        false // cancel
    }

    pub async fn on_close_requested(&self, args: WindowCloseRequestedArgs) {
        if self.unsaved().get() {
            args.propagation().stop();
            if self.handle_unsaved().await {
                self.txt_touched.set(false);
                WINDOW.close();
            }
        }
    }

    async fn write(&self, file: std::path::PathBuf) -> bool {
        let txt = self.txt.clone();
        let r = task::wait(move || txt.with(move |txt| std::fs::write(file, txt.as_bytes()))).await;
        match r {
            Ok(()) => true,
            Err(e) => {
                self.handle_error("writing file", e.to_txt()).await;
                false
            }
        }
    }

    async fn handle_unsaved(&self) -> bool {
        if !self.unsaved().get() {
            return true;
        }

        let dlg = native_dialog::MsgDialog {
            title: "Save File?".into(),
            message: "Save file? All unsaved changes will be lost.".into(),
            icon: native_dialog::MsgDialogIcon::Warn,
            buttons: native_dialog::MsgDialogButtons::YesNo,
        };
        let r = WINDOWS.native_message_dialog(WINDOW.id(), dlg).wait_rsp().await;
        match r {
            native_dialog::MsgDialogResponse::Yes => self.save().await,
            native_dialog::MsgDialogResponse::No => true,
            _ => false,
        }
    }

    async fn handle_error(&self, context: &'static str, e: Txt) {
        tracing::error!("error {context}, {e}");

        let dlg = native_dialog::MsgDialog {
            title: "Error".into(),
            message: formatx!("Error {context}.\n\n{e}"),
            icon: native_dialog::MsgDialogIcon::Error,
            buttons: native_dialog::MsgDialogButtons::Ok,
        };
        let _ = WINDOWS.native_message_dialog(WINDOW.id(), dlg).wait_rsp().await;
    }

    fn enter_busy(&self) -> impl Drop {
        struct BusyTracker(ArcVar<u32>);
        impl Drop for BusyTracker {
            fn drop(&mut self) {
                self.0.modify(|b| *b.to_mut() -= 1);
            }
        }
        self.busy.modify(|b| *b.to_mut() += 1);
        BusyTracker(self.busy.clone())
    }
}
