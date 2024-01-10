//! Hash-map of type erased values, useful for storing assorted dynamic state.
//!
//! A new map can be instantiated using [`OwnedStateMap`], but in a typical app you use maps provided by
//! the API. The most common widget maps are [`WIDGET.with_state_mut`] that is associated
//! with the widget instance and [`WidgetInfoBuilder::with_meta`] that is associated with the widget info.
//!
//! ```
//! # fn main() { }
//! use zero_ui::{prelude::*, prelude_wgt::*};
//!
//! static STATE_ID: StaticStateId<bool> = StaticStateId::new_unique();
//!
//! /// Extends [`WidgetInfo`] with state.
//! pub trait StateWidgetInfoExt {
//!     /// Gets the state.
//!     fn state(&self) -> Option<bool>;
//! }
//! impl StateWidgetInfoExt for WidgetInfo {
//!     fn state(&self) -> Option<bool> {
//!         self.meta().get_clone(&STATE_ID)
//!     }
//! }
//!
//! /// State the state info.
//! #[property(CONTEXT)]
//! pub fn state(child: impl UiNode, state: impl IntoVar<bool>) -> impl UiNode {
//!     let state = state.into_var();
//!     match_node(child, move |_, op| match op {
//!         UiNodeOp::Init => {
//!             WIDGET.sub_var_info(&state);
//!         }
//!         UiNodeOp::Info { info } => {
//!             info.set_meta(&STATE_ID, state.get());
//!         }
//!         _ => {}
//!     })
//! }
//! ```
//!
//! # Full API
//!
//! See [`zero_ui_state_map`] for the full API.
//!
//! [`WIDGET.with_state_mut`]: crate::widget::WIDGET::with_state_mut
//! [`WidgetInfoBuilder::with_meta`]: crate::widget::info::WidgetInfoBuilder::with_meta

pub use zero_ui_state_map::{
    state_map::{OccupiedStateMapEntry, StateMapEntry, VacantStateMapEntry},
    BorrowMutStateMap, BorrowStateMap, OwnedStateMap, StateId, StateMapMut, StateMapRef, StateValue, StaticStateId,
};
