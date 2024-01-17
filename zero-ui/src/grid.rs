//! Grid layout widgets.
//! 
//! The [`Grid!`](struct@Grid) layout widget that defines a grid using column and row widgets and then size and position
//! cell widgets into this grid.
//! 
//! The example below defines a 3x3 grid that demonstrates different length units.
//! 
//! ```
//! use zero_ui::prelude::*;
//! 
//! # let _scope = APP.defaults();
//! let length_color = [
//!     (Length::Default, colors::RED), // default (auto)
//!     (200.dip(), colors::GREEN),     // exact
//!     (1.lft(), colors::BLUE),        // leftover
//! ];
//! 
//! # let _ = 
//! Grid! {
//!     columns = length_color.iter().map(|(length, color)| grid::Column! {
//!         width = length.clone();
//!         widget::background_color = color.with_alpha(10.pct());
//!     })
//!     .collect::<UiNodeVec>();
//! 
//!     rows = length_color.iter().map(|(length, color)| grid::Row! {
//!         height = length.clone();
//!         widget::background_color = color.with_alpha(10.pct());
//!     })
//!     .collect::<UiNodeVec>();
//! 
//!     cells = (0..3).flat_map(|col| (0..3usize).map(move |row| Text! {
//!         grid::cell::at = (col, row);
//!         txt = formatx!("({col}, {row})");
//! 
//!         txt_align = Align::CENTER;
//!         layout::padding = 10;
//!         widget::border = 1, colors::AZURE.transparent();
//!         when *#gesture::is_hovered {
//!             widget::border = 1, colors::AZURE;
//!         }
//!     }))
//!     .collect::<UiNodeVec>();
//! };
//! # ;
//! ```
//! 
//! The grid can also auto-grow rows or columns and auto-position cells, the following example
//! defines a 3x6 grid that auto-grows rows (by default) and generates custom row widgets that
//! have an alternating background color.
//! 
//! ```
//! use zero_ui::prelude::*;
//! # let _scope = APP.defaults();
//! 
//! # let _ = 
//! Grid! {
//!     columns = ui_vec![grid::Column!(1.lft()), grid::Column!(2.lft()), grid::Column!(1.lft())];
//!     auto_grow_fn = wgt_fn!(|_| grid::Row! {
//!         when *#is_odd {
//!             widget::background_color = colors::BLACK.with_alpha(10.pct());
//!         }
//!     });
//! 
//!      cells = (0..6).flat_map(|row| (0..3usize).map(move |col| Text! {
//!         grid::cell::at = grid::cell::AT_AUTO;
//!         txt = formatx!("({col}, {row})");
//! 
//!         txt_align = Align::CENTER;
//!         layout::padding = 10;
//!         widget::border = 1, colors::AZURE.transparent();
//!         when *#gesture::is_hovered {
//!             widget::border = 1, colors::AZURE;
//!         }
//!     }))
//!     .collect::<UiNodeVec>();
//! }
//! # ;
//! ```
//!
//! # Full API
//!
//! See [`zero_ui_wgt_grid`] for the full widget API.

pub use zero_ui_wgt_grid::{node, AutoGrowFnArgs, AutoGrowMode, Cell, Column, Grid, Row};

/// Cell widget and properties.
pub mod cell {
    pub use zero_ui_wgt_grid::cell::{at, column, column_span, row, row_span, span, Cell, CellInfo, AT_AUTO};
}

/// Column widget and properties.
pub mod column {
    pub use zero_ui_wgt_grid::column::{
        get_index, get_index_fct, get_index_len, get_rev_index, is_even, is_first, is_last, is_odd, Column,
    };
}

/// Row widget and properties.
pub mod row {
    pub use zero_ui_wgt_grid::row::{get_index, get_index_fct, get_index_len, get_rev_index, is_even, is_first, is_last, is_odd, Row};
}
