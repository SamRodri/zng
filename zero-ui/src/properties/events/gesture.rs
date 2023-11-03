//! Gesture events and control, [`on_click`](fn@on_click), [`click_shortcut`](fn@click_shortcut) and more.
//!
//! These events aggregate multiple lower-level events to represent a user interaction.
//! Prefer using these events over the events directly tied to an input device.

use super::event_property;
use crate::core::gesture::*;
use crate::prelude::events::access::access_click;
use crate::prelude::new_property::*;

event_property! {
    /// On widget click from any source and of any click count and the widget is enabled.
    ///
    /// This is the most general click handler, it raises for all possible sources of the [`CLICK_EVENT`] and any number
    /// of consecutive clicks. Use [`click`](fn@click) to handle only primary button clicks or [`on_any_single_click`](fn@on_any_single_click)
    /// to not include double/triple clicks.
    pub fn any_click {
        event: CLICK_EVENT,
        args: ClickArgs,
        filter: |args| args.is_enabled(WIDGET.id()),
        with: access_click,
    }

    /// On widget click from any source and of any click count and the widget is disabled.
    pub fn disabled_click {
        event: CLICK_EVENT,
        args: ClickArgs,
        filter: |args| args.is_disabled(WIDGET.id()),
        with: access_click,
    }

    /// On widget click from any source but excluding double/triple clicks and the widget is enabled.
    ///
    /// This raises for all possible sources of [`CLICK_EVENT`], but only when the click count is one. Use
    /// [`on_single_click`](fn@on_single_click) to handle only primary button clicks.
    pub fn any_single_click {
        event: CLICK_EVENT,
        args: ClickArgs,
        filter: |args| args.is_single() && args.is_enabled(WIDGET.id()),
        with: access_click,
    }

    /// On widget click from any source but exclusive double-clicks and the widget is enabled.
    ///
    /// This raises for all possible sources of [`CLICK_EVENT`], but only when the click count is two. Use
    /// [`on_double_click`](fn@on_double_click) to handle only primary button clicks.
    pub fn any_double_click {
        event: CLICK_EVENT,
        args: ClickArgs,
        filter: |args| args.is_double() && args.is_enabled(WIDGET.id()),
    }

    /// On widget click from any source but exclusive triple-clicks and the widget is enabled.
    ///
    /// This raises for all possible sources of [`CLICK_EVENT`], but only when the click count is three. Use
    /// [`on_triple_click`](fn@on_triple_click) to handle only primary button clicks.
    pub fn any_triple_click {
        event: CLICK_EVENT,
        args: ClickArgs,
        filter: |args| args.is_triple() && args.is_enabled(WIDGET.id()),
    }

    /// On widget click with the primary button and any click count and the widget is enabled.
    ///
    /// This raises only if the click [is primary](ClickArgs::is_primary), but raises for any click count (double/triple clicks).
    /// Use [`on_any_click`](fn@on_any_click) to handle clicks from any button or [`on_single_click`](fn@on_single_click) to not include
    /// double/triple clicks.
    pub fn click {
        event: CLICK_EVENT,
        args: ClickArgs,
        filter: |args| args.is_primary() && args.is_enabled(WIDGET.id()),
        with: access_click,
    }

    /// On widget click with the primary button, excluding double/triple clicks and the widget is enabled.
    ///
    /// This raises only if the click [is primary](ClickArgs::is_primary) and the click count is one. Use
    /// [`on_any_single_click`](fn@on_any_single_click) to handle single clicks from any button.
    pub fn single_click {
        event: CLICK_EVENT,
        args: ClickArgs,
        filter: |args| args.is_primary() && args.is_single() && args.is_enabled(WIDGET.id()),
        with: access_click,
    }

    /// On widget click with the primary button and exclusive double-clicks and the widget is enabled.
    ///
    /// This raises only if the click [is primary](ClickArgs::is_primary) and the click count is two. Use
    /// [`on_any_double_click`](fn@on_any_double_click) to handle double clicks from any button.
    pub fn double_click {
        event: CLICK_EVENT,
        args: ClickArgs,
        filter: |args| args.is_primary() && args.is_double() && args.is_enabled(WIDGET.id()),
    }

    /// On widget click with the primary button and exclusive triple-clicks and the widget is enabled.
    ///
    /// This raises only if the click [is primary](ClickArgs::is_primary) and the click count is three. Use
    /// [`on_any_double_click`](fn@on_any_double_click) to handle double clicks from any button.
    pub fn triple_click {
        event: CLICK_EVENT,
        args: ClickArgs,
        filter: |args| args.is_primary() && args.is_triple() && args.is_enabled(WIDGET.id()),
    }

    /// On widget click with the secondary/context button and the widget is enabled.
    ///
    /// This raises only if the click [is context](ClickArgs::is_context).
    pub fn context_click {
        event: CLICK_EVENT,
        args: ClickArgs,
        filter: |args| args.is_context() && args.is_enabled(WIDGET.id()),
        with: access_click,
    }
}

/// Keyboard shortcuts that focus and clicks this widget.
///
/// When any of the `shortcuts` is pressed, focus and click this widget.
#[property(CONTEXT)]
pub fn click_shortcut(child: impl UiNode, shortcuts: impl IntoVar<Shortcuts>) -> impl UiNode {
    click_shortcut_node(child, shortcuts, ShortcutClick::Primary)
}
/// Keyboard shortcuts that focus and [context clicks](fn@on_context_click) this widget.
///
/// When any of the `shortcuts` is pressed, focus and context clicks this widget.
#[property(CONTEXT)]
pub fn context_click_shortcut(child: impl UiNode, shortcuts: impl IntoVar<Shortcuts>) -> impl UiNode {
    click_shortcut_node(child, shortcuts, ShortcutClick::Context)
}

fn click_shortcut_node(child: impl UiNode, shortcuts: impl IntoVar<Shortcuts>, kind: ShortcutClick) -> impl UiNode {
    let shortcuts = shortcuts.into_var();
    let mut _handle = None;

    match_node(child, move |_, op| {
        let new = match op {
            UiNodeOp::Init => {
                WIDGET.sub_var(&shortcuts);
                Some(shortcuts.get())
            }
            UiNodeOp::Deinit => {
                _handle = None;
                None
            }
            UiNodeOp::Update { .. } => shortcuts.get_new(),
            _ => None,
        };
        if let Some(s) = new {
            _handle = Some(GESTURES.click_shortcut(s, kind, WIDGET.id()));
        }
    })
}
