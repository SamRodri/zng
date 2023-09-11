//! Commands that control the scoped scroll widget.
//!
//! The scroll widget implements all of this commands scoped to its widget ID.
//!
//! [`ScrollToTopCommand`]: crate::widgets::scroll::commands::ScrollToTopCommand
//! [`ScrollToLeftmostCommand`]: crate::widgets::scroll::commands::ScrollToLeftmostCommand

use super::*;
use zero_ui::core::gesture::*;

command! {
    /// Represents the **scroll up** by one [`v_line_unit`] action.
    ///
    /// # Parameter
    ///
    /// This command supports an optional parameter, it can be a [`bool`] that enables the alternate of the command
    /// or a [`ScrollRequest`] that contains more configurations.
    ///
    /// [`v_line_unit`]: fn@super::properties::v_line_unit
    pub static SCROLL_UP_CMD = {
        name: "Scroll Up",
        info: "Scroll Up by one scroll unit.",
        shortcut: shortcut!(ArrowUp),
        shortcut_filter: ShortcutFilter::FOCUSED | ShortcutFilter::CMD_ENABLED,
    };

    /// Represents the **scroll down** by one [`v_line_unit`] action.
    ///
    /// # Parameter
    ///
    /// This command supports an optional parameter, it can be a [`bool`] that enables the alternate of the command
    /// or a [`ScrollRequest`] that contains more configurations.
    ///
    /// [`v_line_unit`]: fn@super::properties::v_line_unit
    pub static SCROLL_DOWN_CMD = {
        name: "Scroll Down",
        info: "Scroll Down by one scroll unit.",
        shortcut: shortcut!(ArrowDown),
        shortcut_filter: ShortcutFilter::FOCUSED | ShortcutFilter::CMD_ENABLED,
    };

    /// Represents the **scroll left** by one [`h_line_unit`] action.
    ///
    /// # Parameter
    ///
    /// This command supports an optional parameter, it can be a [`bool`] that enables the alternate of the command
    /// or a [`ScrollRequest`] that contains more configurations.
    ///
    /// [`h_line_unit`]: fn@super::properties::h_line_unit
    pub static SCROLL_LEFT_CMD = {
        name: "Scroll Left",
        info: "Scroll Left by one scroll unit.",
        shortcut: shortcut!(ArrowLeft),
        shortcut_filter: ShortcutFilter::FOCUSED | ShortcutFilter::CMD_ENABLED,
    };

    /// Represents the **scroll right** by one [`h_line_unit`] action.
    ///
    /// # Parameter
    ///
    /// This command supports an optional parameter, it can be a [`bool`] that enables the alternate of the command
    /// or a [`ScrollRequest`] that contains more configurations.
    ///
    /// [`h_line_unit`]: fn@super::properties::h_line_unit
    pub static SCROLL_RIGHT_CMD = {
        name: "Scroll Right",
        info: "Scroll Right by one scroll unit.",
        shortcut: shortcut!(ArrowRight),
        shortcut_filter: ShortcutFilter::FOCUSED | ShortcutFilter::CMD_ENABLED,
    };


    /// Represents the **page up** by one [`v_page_unit`] action.
    ///
    /// # Parameter
    ///
    /// This command supports an optional parameter, it can be a [`bool`] that enables the alternate of the command
    /// or a [`ScrollRequest`] that contains more configurations.
    ///
    /// [`name`]: CommandNameExt
    /// [`info`]: CommandInfoExt
    /// [`shortcut`]: CommandShortcutExt
    /// [`v_page_unit`]: fn@super::properties::v_page_unit
    pub static PAGE_UP_CMD = {
        name: "Page Up",
        info: "Scroll Up by one page unit.",
        shortcut: shortcut!(PageUp),
        shortcut_filter: ShortcutFilter::FOCUSED | ShortcutFilter::CMD_ENABLED,
    };

    /// Represents the **page down** by one [`v_page_unit`] action.
    ///
    /// # Parameter
    ///
    /// This command supports an optional parameter, it can be a [`bool`] that enables the alternate of the command
    /// or a [`ScrollRequest`] that contains more configurations.
    ///
    /// [`v_page_unit`]: fn@super::properties::v_page_unit
    pub static PAGE_DOWN_CMD = {
        name: "Page Down",
        info: "Scroll down by one page unit.",
        shortcut: shortcut!(PageDown),
        shortcut_filter: ShortcutFilter::FOCUSED | ShortcutFilter::CMD_ENABLED,
    };

    /// Represents the **page left** by one [`h_page_unit`] action.
    ///
    /// # Parameter
    ///
    /// This command supports an optional parameter, it can be a [`bool`] that enables the alternate of the command
    /// or a [`ScrollRequest`] that contains more configurations.
    ///
    /// [`h_line_unit`]: fn@super::properties::h_line_unit
    pub static PAGE_LEFT_CMD = {
        name: "Page Left",
        info: "Scroll Left by one page unit.",
        shortcut: shortcut!(SHIFT+PageUp),
        shortcut_filter: ShortcutFilter::FOCUSED | ShortcutFilter::CMD_ENABLED,
    };

    /// Represents the **page right** by one [`h_page_unit`] action.
    ///
    /// # Parameter
    ///
    /// This command supports an optional parameter, it can be a [`bool`] that enables the alternate of the command
    /// or a [`ScrollRequest`] that contains more configurations.
    ///
    /// [`h_page_unit`]: fn@super::properties::h_page_unit
    pub static PAGE_RIGHT_CMD = {
        name: "Page Right",
        info: "Scroll Right by one page unit.",
        shortcut: shortcut!(SHIFT+PageDown),
        shortcut_filter: ShortcutFilter::FOCUSED | ShortcutFilter::CMD_ENABLED,
    };

    /// Represents the **scroll to top** action.
    pub static SCROLL_TO_TOP_CMD = {
        name: "Scroll to Top",
        info: "Scroll up to the content top.",
        shortcut: [shortcut!(Home), shortcut!(CTRL+Home)],
        shortcut_filter: ShortcutFilter::FOCUSED | ShortcutFilter::CMD_ENABLED,
    };

    /// Represents the **scroll to bottom** action.
    pub static SCROLL_TO_BOTTOM_CMD = {
        name: "Scroll to Bottom",
        info: "Scroll down to the content bottom.",
        shortcut: [shortcut!(End), shortcut!(CTRL+End)],
        shortcut_filter: ShortcutFilter::FOCUSED | ShortcutFilter::CMD_ENABLED,
    };

    /// Represents the **scroll to leftmost** action.
    pub static SCROLL_TO_LEFTMOST_CMD = {
        name: "Scroll to Leftmost",
        info: "Scroll left to the content left edge.",
        shortcut: [shortcut!(SHIFT+Home), shortcut!(CTRL|SHIFT+Home)],
        shortcut_filter: ShortcutFilter::FOCUSED | ShortcutFilter::CMD_ENABLED,
    };

    /// Represents the **scroll to rightmost** action.
    pub static SCROLL_TO_RIGHTMOST_CMD = {
        name: "Scroll to Righmost",
        info: "Scroll right to the content right edge.",
        shortcut: [shortcut!(SHIFT+End), shortcut!(CTRL|SHIFT+End)],
        shortcut_filter: ShortcutFilter::FOCUSED | ShortcutFilter::CMD_ENABLED,
    };

    /// Represents the action of scrolling until a child widget is fully visible, the command can
    /// also adjust the zoom scale.
    ///
    /// # Metadata
    ///
    /// This command initializes with no extra metadata.
    ///
    /// # Parameter
    ///
    /// This command requires a parameter to work, it can be the [`WidgetId`] of a child widget or
    /// a [`ScrollToRequest`] instance.
    ///
    /// You can use the [`scroll_to`] function to invoke this command in all parent scrolls automatically.
    pub static SCROLL_TO_CMD;

    /// Represents the **zoom in** action.
    /// 
    /// # Parameter
    ///
    /// This commands accepts an optional [`Point`] parameter that defines the origin of the
    /// scale transform, relative values are resolved in the viewport space. The default value
    /// is *top-start*.
    ///
    /// [`Point`]: crate::core::units::Point
    pub static ZOOM_IN_CMD = {
        name: "Zoom In",
        shortcut: shortcut!(CTRL+'+'),
        shortcut_filter: ShortcutFilter::FOCUSED | ShortcutFilter::CMD_ENABLED,
    };

    /// Represents the **zoom out** action.
    ///
    /// # Parameter
    ///
    /// This commands accepts an optional [`Point`] parameter that defines the origin of the
    /// scale transform, relative values are resolved in the viewport space. The default value
    /// is *top-start*.
    ///
    /// [`Point`]: crate::core::units::Point
    pub static ZOOM_OUT_CMD = {
        name: "Zoom Out",
        shortcut: shortcut!(CTRL+'-'),
        shortcut_filter: ShortcutFilter::FOCUSED | ShortcutFilter::CMD_ENABLED,
    };

    /// Represents the **reset zoom** action.
    pub static ZOOM_RESET_CMD = {
        name: "Reset Zoom",
        shortcut: shortcut!(CTRL+'0'),
        shortcut_filter: ShortcutFilter::FOCUSED | ShortcutFilter::CMD_ENABLED,
    };
}

/// Parameters for the scroll and page commands.
#[derive(Debug, Clone, PartialEq)]
pub struct ScrollRequest {
    /// If the [alt factor] should be applied to the base scroll unit when scrolling.
    ///
    /// [alt factor]: super::ALT_FACTOR_VAR
    pub alternate: bool,
    /// Only scroll within this inclusive range. The range is normalized `0.0..=1.0`, the default is `(f32::MIN, f32::MAX)`.
    ///
    /// Note that the commands are enabled and disabled for the full range, this parameter controls
    /// the range for the request only.
    pub clamp: (f32, f32),
}
impl Default for ScrollRequest {
    fn default() -> Self {
        Self {
            alternate: Default::default(),
            clamp: (f32::MIN, f32::MAX),
        }
    }
}
impl ScrollRequest {
    /// Pack the request into a command parameter.
    pub fn to_param(self) -> CommandParam {
        CommandParam::new(self)
    }

    /// Extract a clone of the request from the command parameter if it is of a compatible type.
    pub fn from_param(p: &CommandParam) -> Option<Self> {
        if let Some(req) = p.downcast_ref::<Self>() {
            Some(req.clone())
        } else {
            p.downcast_ref::<bool>().map(|&alt| ScrollRequest {
                alternate: alt,
                ..Default::default()
            })
        }
    }

    /// Extract a clone of the request from [`CommandArgs::param`] if it is set to a compatible type and
    /// stop-propagation was not requested for the event.
    pub fn from_args(args: &CommandArgs) -> Option<Self> {
        if let Some(p) = &args.param {
            if args.propagation().is_stopped() {
                None
            } else {
                Self::from_param(p)
            }
        } else {
            None
        }
    }
}
impl_from_and_into_var! {
    fn from(alternate: bool) -> ScrollRequest {
        ScrollRequest {
            alternate,
            ..Default::default()
        }
    }
}

/// Parameters for the [`SCROLL_TO_CMD`].
#[derive(Debug, Clone, PartialEq)]
pub struct ScrollToRequest {
    /// Widget that will be scrolled into view.
    pub widget_id: WidgetId,

    /// How much the scroll position will change to showcase the target widget.
    pub mode: ScrollToMode,

    /// Optional zoom scale target.
    ///
    /// If set the offsets and scale will animate so that the `mode`
    /// is fullfilled when this zoom factor is reached. If not set the scroll will happen in
    /// the current zoom scale.
    ///
    /// Note that the viewport size can change due to a scrollbar visibility changing, this size
    /// change is not accounted for when calculating minimals.
    pub zoom: Option<Factor>,
}
impl ScrollToRequest {
    /// Pack the request into a command parameter.
    pub fn to_param(self) -> CommandParam {
        CommandParam::new(self)
    }

    /// Extract a clone of the request from the command parameter if it is of a compatible type.
    pub fn from_param(p: &CommandParam) -> Option<Self> {
        if let Some(req) = p.downcast_ref::<Self>() {
            Some(req.clone())
        } else {
            p.downcast_ref::<WidgetId>().map(|id| ScrollToRequest {
                widget_id: *id,
                mode: ScrollToMode::default(),
                zoom: None,
            })
        }
    }

    /// Extract a clone of the request from [`CommandArgs::param`] if it is set to a compatible type and
    /// stop-propagation was not requested for the event and the command was enabled when it was send.
    pub fn from_args(args: &CommandArgs) -> Option<Self> {
        if let Some(p) = &args.param {
            if !args.enabled || args.propagation().is_stopped() {
                None
            } else {
                Self::from_param(p)
            }
        } else {
            None
        }
    }
}
impl_from_and_into_var! {
    fn from(widget_id: WidgetId) -> ScrollToRequest {
        ScrollToRequest {
            widget_id,
            mode: ScrollToMode::default(),
            zoom: None,
        }
    }
}

/// Defines how much the [`SCROLL_TO_CMD`] will scroll to showcase the target widget.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ScrollToMode {
    /// Scroll will change only just enough so that the widget inner rect is fully visible with the optional
    /// extra margin offsets.
    Minimal {
        /// Extra margin added so that the widget is touching the scroll edge.
        margin: SideOffsets,
    },
    /// Scroll so that the point relative to the widget inner rectangle is at the same screen point on
    /// the scroll viewport.
    Center {
        /// A point relative to the target widget inner size.
        widget_point: Point,
        /// A point relative to the scroll viewport.
        scroll_point: Point,
    },
}
impl ScrollToMode {
    /// New [`Minimal`] mode.
    ///
    /// [`Minimal`]: Self::Minimal
    pub fn minimal(margin: impl Into<SideOffsets>) -> Self {
        ScrollToMode::Minimal { margin: margin.into() }
    }

    /// New [`Minimal`] mode.
    ///
    /// The minimal scroll needed so that `rect` in the content widget is fully visible.
    ///
    /// [`Minimal`]: Self::Minimal
    pub fn minimal_rect(rect: impl Into<Rect>) -> Self {
        let rect = rect.into();
        ScrollToMode::Minimal {
            margin: SideOffsets::new(
                -rect.origin.y.clone(),
                rect.origin.x.clone() + rect.size.width - 100.pct(),
                rect.origin.y + rect.size.height - 100.pct(),
                -rect.origin.x,
            ),
        }
    }

    /// New [`Center`] mode using the center points of widget and scroll.
    ///
    /// [`Center`]: Self::Center
    pub fn center() -> Self {
        Self::center_points(Point::center(), Point::center())
    }

    /// New [`Center`] mode.
    ///
    /// [`Center`]: Self::Center
    pub fn center_points(widget_point: impl Into<Point>, scroll_point: impl Into<Point>) -> Self {
        ScrollToMode::Center {
            widget_point: widget_point.into(),
            scroll_point: scroll_point.into(),
        }
    }
}
impl Default for ScrollToMode {
    /// Minimal with margin 10.
    fn default() -> Self {
        Self::minimal(10)
    }
}
impl IntoVar<Option<ScrollToMode>> for ScrollToMode {
    type Var = crate::core::var::LocalVar<Option<ScrollToMode>>;

    fn into_var(self) -> Self::Var {
        crate::core::var::LocalVar(Some(self))
    }
}
impl IntoValue<Option<ScrollToMode>> for ScrollToMode {}

/// Scroll all parent [`is_scroll`] widgets of `target` so that it becomes visible.
///
/// This function is a helper for searching for the `target` in all windows and sending [`SCROLL_TO_CMD`] for all required scroll widgets.
/// Does nothing if the `target` is not found.
///
/// [`is_scroll`]: WidgetInfoExt::is_scroll
pub fn scroll_to(target: impl Into<WidgetId>, mode: impl Into<ScrollToMode>) {
    scroll_to_impl(target.into(), mode.into(), None)
}

/// Like [`scroll_to`], but also adjusts the zoom scale.
pub fn scroll_to_zoom(target: impl Into<WidgetId>, mode: impl Into<ScrollToMode>, zoom: impl Into<Factor>) {
    scroll_to_impl(target.into(), mode.into(), Some(zoom.into()))
}

/// Scroll all parent [`is_scroll`] widgets of `target` so that it becomes visible.
///
/// This function is a helper for sending [`SCROLL_TO_CMD`] for all required scroll widgets.
///
/// [`is_scroll`]: WidgetInfoExt::is_scroll
pub fn scroll_to_info(target: &crate::core::widget_info::WidgetInfo, mode: impl Into<ScrollToMode>) {
    scroll_to_info_impl(target, mode.into(), None)
}

/// Like [`scroll_to_info`], but also adjusts the zoom scale.
pub fn scroll_to_info_zoom(target: &crate::core::widget_info::WidgetInfo, mode: impl Into<ScrollToMode>, zoom: impl Into<Factor>) {
    scroll_to_info_impl(target, mode.into(), Some(zoom.into()))
}

fn scroll_to_impl(target: WidgetId, mode: ScrollToMode, zoom: Option<Factor>) {
    for w in crate::core::window::WINDOWS.widget_trees() {
        if let Some(target) = w.get(target) {
            scroll_to_info_impl(&target, mode, zoom);
            break;
        }
    }
}

fn scroll_to_info_impl(target: &crate::core::widget_info::WidgetInfo, mode: ScrollToMode, zoom: Option<Factor>) {
    let mut t = target.id();
    for a in target.ancestors() {
        if a.is_scroll() {
            SCROLL_TO_CMD.scoped(a.id()).notify_param(ScrollToRequest {
                widget_id: t,
                mode: mode.clone(),
                zoom,
            });
            t = a.id();
        }
    }
}
