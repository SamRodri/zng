use crate::core::focus::*;
use crate::core::gesture::*;
use crate::core::window::{AutoSize, StartPosition, Window};
use crate::prelude::new_widget::*;

/// A window container.
///
/// The instance type is [`Window`], witch can be given to the [`Windows`](crate::core::window::Windows) service
/// to open a system window that is kept in sync with the window properties set in the widget.
///
/// # Example
///
/// ```no_run
/// use zero_ui::prelude::*;
///
/// App::default().run_window(|_| {
///     window! {
///         title = "Window 1";
///         content = text("Window 1");
///     }
/// })
/// ```
/// See [`run_window`](crate::core::window::AppRunWindow::run_window) for more details.
#[widget($crate::widgets::window)]
pub mod window {
    use super::*;

    inherit!(container);

    properties! {
        /// Window title.
        title = "";

        start_position = StartPosition::Default;

        /// Window position (left, top).
        ///
        ///  If set to a variable it is kept in sync.
        ///
        /// Set to [`f32::NAN`](f32::NAN) to not give an initial position.
        position = {
            // use shared var in debug to allow inspecting the value.
            #[cfg(debug_assertions)]
            let r = crate::core::var::var(crate::core::units::Point::new(f32::NAN, f32::NAN));

            #[cfg(not(debug_assertions))]
            let r = (f32::NAN, f32::NAN);

            r
        };

        /// Window size.
        ///
        /// If set to a variable it is kept in sync.
        ///
        /// Does not include the OS window border.
        size = {
            #[cfg(debug_assertions)]
            let r = crate::core::var::var(crate::core::units::Size::new(800.0, 600.0));

            #[cfg(not(debug_assertions))]
            let r = (800.0, 600.0);

            r
        };

        /// Window auto size config.
        ///
        /// If enabled overwrites the other sizes with the content size.
        auto_size = false;

        /// Window clear color.
        ///
        /// Changes to this property are ignored, only the initial value is used.
        ///
        /// Color filters applied to the window don't affect this color.
        background_color as clear_color = rgb(0.1, 0.1, 0.1);

        /// Window background color.
        background_color = rgb(0.1, 0.1, 0.1);

        /// Unique identifier of the window root widget.
        root_id: WidgetId = WidgetId::new_unique();
        id = unset!;

        /// Windows are focus scopes by default.
        focus_scope = true;

        /// Windows cycle TAB navigation by default.
        tab_nav = TabNav::Cycle;

        /// Windows cycle arrow navigation by default.
        directional_nav = DirectionalNav::Cycle;

        /// Windows remember the last focused widget and return focus when activated again.
        focus_scope_behavior = FocusScopeOnFocus::LastFocused;

        /// Test inspector.
        on_shortcut as on_shortcut_inspect = print_frame_inspector();

        /// If the user can resize the window.
        ///
        /// Not that the window can still change size, this only disables
        /// the OS window frame controls that change size.
        enabled as resizable = true;

        /// If the window is visible.
        enabled as visible = true;
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn new(
        child: impl UiNode,
        root_id: WidgetId,
        title: impl IntoVar<Text>,
        start_position: impl Into<StartPosition>,
        position: impl IntoVar<Point>,
        size: impl IntoVar<Size>,
        auto_size: impl IntoVar<AutoSize>,
        resizable: impl IntoVar<bool>,
        clear_color: impl IntoVar<Rgba>,
        visible: impl IntoVar<bool>,
    ) -> Window {
        Window::new(
            root_id,
            title,
            start_position,
            position,
            size,
            auto_size,
            resizable,
            clear_color,
            visible,
            child,
        )
    }
}

#[cfg(not(debug_assertions))]
fn print_frame_inspector() -> impl FnMut(&mut WidgetContext, &ShortcutArgs) {
    |_, _| {}
}

#[cfg(debug_assertions)]
fn print_frame_inspector() -> impl FnMut(&mut WidgetContext, &ShortcutArgs) {
    use crate::core::debug::{write_frame, WriteFrameState};

    let mut state = WriteFrameState::none();
    move |ctx, args| {
        if args.shortcut == shortcut!(CTRL | SHIFT + I) {
            args.stop_propagation();

            let frame = ctx
                .services
                .req::<crate::core::window::Windows>()
                .window(ctx.path.window_id())
                .unwrap()
                .frame_info();

            write_frame(frame, &state, &mut std::io::stderr());

            state = WriteFrameState::new(&frame);
        }
    }
}
