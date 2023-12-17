#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use zero_ui::{
    color::{color_scheme_map, filter::invert_color},
    image::ImageFit,
    layer::AnchorOffset,
    mouse::CursorIcon,
    prelude::*,
    stack::v_stack,
    text::font_color,
    wgt_prelude::NilUiNode,
};

use zero_ui_view_prebuilt as zero_ui_view;

fn main() {
    examples_util::print_info();
    zero_ui_view::init();

    // zero_ui_view::run_same_process(app_main);
    app_main();
}

fn app_main() {
    APP.defaults().run_window(async {
        let mut demos = ui_vec![];
        for icon in CURSORS {
            demos.push(cursor_demo(Some(*icon)));
        }

        Window! {
            title = "Cursor Example";
            resizable = false;
            auto_size = true;
            padding = 20;
            child = v_stack(ui_vec![
                Grid! {
                    columns = ui_vec![grid::Column!(1.lft()); 5];
                    auto_grow_fn = wgt_fn!(|_| grid::Row!(1.lft()));
                    cells = demos;
                },
                cursor_demo(None),
            ])
        }
    })
}

fn cursor_demo(icon: Option<(CursorIcon, &'static [u8])>) -> impl UiNode {
    Container! {
        grid::cell::at = grid::cell::AT_AUTO;
        mouse::cursor = icon.map(|i| i.0);

        layout::size = (150, 80);
        layout::align = Align::CENTER;

        tooltip = Tip!(Text!("tooltip position"));
        tip::tooltip_anchor = {
            let mut mode = AnchorMode::tooltip();
            mode.transform = layer::AnchorTransform::Cursor {
                offset: AnchorOffset::out_bottom_in_left(),
                include_touch: true,
                bounds: None,
            };
            mode
        };
        tip::tooltip_delay = 0.ms();

        layout::margin = 1;
        widget::background_color = color_scheme_map(colors::BLACK, colors::WHITE);
        widget::background = match icon {
            Some((_, img)) => Image!{
                source = img;
                img_fit = ImageFit::None;
                invert_color = color_scheme_map(true, false);
            }.boxed(),
            None => NilUiNode.boxed(),
        };

        #[easing(150.ms())]
        font_color = color_scheme_map(rgb(140, 140, 140), rgb(115, 115, 115));

        when *#gesture::is_hovered {
            #[easing(0.ms())]
            font_color = color_scheme_map(colors::WHITE, colors::BLACK);
        }

        child_align = Align::TOP_LEFT;
        padding = (2, 5);

        child = Text! {
            txt = match icon {
                Some((ico, _)) => formatx!("{ico:?}"),
                None => Txt::from_static("<none>"),
            };

            font_style = match icon {
                Some(_) => FontStyle::Normal,
                None => FontStyle::Italic,
            };

            font_family = "monospace";
            font_size = 16;
            font_weight = FontWeight::BOLD;
        };
    }
}

pub const CURSORS: &[(CursorIcon, &[u8])] = &[
    (CursorIcon::Default, include_bytes!("res/cursor/default.png")),
    (CursorIcon::Crosshair, include_bytes!("res/cursor/crosshair.png")),
    (CursorIcon::Pointer, include_bytes!("res/cursor/pointer.png")),
    (CursorIcon::Move, include_bytes!("res/cursor/move.png")),
    (CursorIcon::Text, include_bytes!("res/cursor/text.png")),
    (CursorIcon::Wait, include_bytes!("res/cursor/wait.png")),
    (CursorIcon::Help, include_bytes!("res/cursor/help.png")),
    (CursorIcon::Progress, include_bytes!("res/cursor/progress.png")),
    (CursorIcon::NotAllowed, include_bytes!("res/cursor/not-allowed.png")),
    (CursorIcon::ContextMenu, include_bytes!("res/cursor/context-menu.png")),
    (CursorIcon::Cell, include_bytes!("res/cursor/cell.png")),
    (CursorIcon::VerticalText, include_bytes!("res/cursor/vertical-text.png")),
    (CursorIcon::Alias, include_bytes!("res/cursor/alias.png")),
    (CursorIcon::Copy, include_bytes!("res/cursor/copy.png")),
    (CursorIcon::NoDrop, include_bytes!("res/cursor/no-drop.png")),
    (CursorIcon::Grab, include_bytes!("res/cursor/grab.png")),
    (CursorIcon::Grabbing, include_bytes!("res/cursor/grabbing.png")),
    (CursorIcon::AllScroll, include_bytes!("res/cursor/all-scroll.png")),
    (CursorIcon::ZoomIn, include_bytes!("res/cursor/zoom-in.png")),
    (CursorIcon::ZoomOut, include_bytes!("res/cursor/zoom-out.png")),
    (CursorIcon::EResize, include_bytes!("res/cursor/e-resize.png")),
    (CursorIcon::NResize, include_bytes!("res/cursor/n-resize.png")),
    (CursorIcon::NeResize, include_bytes!("res/cursor/ne-resize.png")),
    (CursorIcon::NwResize, include_bytes!("res/cursor/nw-resize.png")),
    (CursorIcon::SResize, include_bytes!("res/cursor/s-resize.png")),
    (CursorIcon::SeResize, include_bytes!("res/cursor/se-resize.png")),
    (CursorIcon::SwResize, include_bytes!("res/cursor/sw-resize.png")),
    (CursorIcon::WResize, include_bytes!("res/cursor/w-resize.png")),
    (CursorIcon::EwResize, include_bytes!("res/cursor/3-resize.png")),
    (CursorIcon::NsResize, include_bytes!("res/cursor/6-resize.png")),
    (CursorIcon::NeswResize, include_bytes!("res/cursor/1-resize.png")),
    (CursorIcon::NwseResize, include_bytes!("res/cursor/4-resize.png")),
    (CursorIcon::ColResize, include_bytes!("res/cursor/col-resize.png")),
    (CursorIcon::RowResize, include_bytes!("res/cursor/row-resize.png")),
];
