//! Menu widgets, properties and types.
//!
//! ```no_run
//! use zero_ui::prelude::*;
//! # APP.defaults().run_window(async {
//!
//! fn main_menu() -> impl UiNode {
//!     Menu!(ui_vec![
//!         SubMenu!(
//!             "File",
//!             ui_vec![
//!                 Button!(zero_ui::app::NEW_CMD.scoped(WINDOW.id())),
//!                 Button!(zero_ui::app::OPEN_CMD.scoped(WINDOW.id())),
//!                 Toggle! {
//!                     child = Text!("Auto Save");
//!                     checked = var(true);
//!                 },
//!                 Hr!(),
//!                 SubMenu!(
//!                     "Recent",
//!                     (0..10)
//!                         .map(|i| Button! { child = Text!(formatx!("recent file {i}")) })
//!                         .collect::<UiNodeVec>()
//!                 ),
//!                 Hr!(),
//!                 Button!(zero_ui::app::EXIT_CMD),
//!             ]
//!         ),
//!         SubMenu!(
//!             "Help",
//!             ui_vec![Button! {
//!                 child = Text!("About");
//!                 on_click = hn!(|_| { });
//!             }]
//!         ),
//!     ])
//! }
//!
//! Window! {
//!     child_top = main_menu(), 0;
//!     zero_ui::app::on_new = hn!(|_| { });
//!     zero_ui::app::on_open = hn!(|_| { });
//!     // ..
//! }
//! # });
//! ```
//!
//! The example above declares a [`Menu!`](struct@Menu) for a window, it demonstrates nested [`SubMenu!`](struct@sub::SubMenu),
//! and menu items, [`Button!`](struct@crate::button::Button),
//! [`Toggle!`](struct@crate::toggle::Toggle) and [`Hr!`](struct@crate::rule_line::hr::Hr). There is no menu item widget,
//! the `SubMenu!` widget re-styles button and toggle.
//!
//! # Context Menu
//!
//! This module also provides a context menu. The example below declares a context menu for the window, it will show
//! on context click, that is, by right-clicking the window, long pressing it or pressing the context menu key.
//!
//! ```
//! use zero_ui::prelude::*;
//! # fn demo() {
//!
//! # let _ =
//! Window! {
//!     context_menu = ContextMenu!(ui_vec![
//!         Button!(zero_ui::app::NEW_CMD.scoped(WINDOW.id())),
//!         Button!(zero_ui::app::OPEN_CMD.scoped(WINDOW.id())),
//!         Toggle! {
//!             child = Text!("Auto Save");
//!             checked = var(true);
//!         },
//!         Hr!(),
//!         SubMenu!("Help", ui_vec![Button! {
//!             child = Text!("About");
//!             on_click = hn!(|_| { });
//!         }]),
//!         Hr!(),
//!         Button!(zero_ui::app::EXIT_CMD),
//!     ]);
//! }
//! # ; }
//! ```
//!
//! # Full API
//!
//! See [`zero_ui_wgt_menu`] for the full widget API.

pub use zero_ui_wgt_menu::{
    icon, icon_fn, panel_fn, shortcut_spacing, shortcut_txt, style_fn, ButtonStyle, DefaultStyle, Menu, ToggleStyle, TouchButtonStyle,
};

/// Submenu widget and properties.
///
/// See [`zero_ui_wgt_menu::sub`] for the full widget API.
pub mod sub {
    pub use zero_ui_wgt_menu::sub::{
        column_width_padding, end_column, end_column_fn, end_column_width, hover_open_delay, is_open, start_column, start_column_fn,
        start_column_width, DefaultStyle, SubMenu, SubMenuAncestors, SubMenuStyle, SubMenuWidgetInfoExt,
    };
}

/// Context menu widget and properties.
///
/// See [`zero_ui_wgt_menu::context`] for the full widget API.
pub mod context {
    pub use zero_ui_wgt_menu::context::{
        context_menu, context_menu_anchor, context_menu_fn, disabled_context_menu, disabled_context_menu_fn, panel_fn, style_fn,
        ContextMenu, ContextMenuArgs, DefaultStyle, TouchStyle,
    };
}

/// Sub-menu popup widget and properties.
///
/// See [`zero_ui_wgt_menu::popup`] for the full widget API.
pub mod popup {
    pub use zero_ui_wgt_menu::popup::{panel_fn, style_fn, DefaultStyle, SubMenuPopup};
}
