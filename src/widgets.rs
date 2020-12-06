//! Common widgets.

pub mod layouts;
pub mod mixins;
pub mod text;

mod button_;
mod container_;
mod window_;

mod fill;
mod line_;
mod ui_n;
mod view_;

pub use button_::*;
pub use container_::*;
pub use fill::*;
pub use line_::*;
pub use ui_n::*;
pub use view_::*;
pub use window_::*;

/// Tests on the widget! code generator.
#[cfg(test)]
mod build_tests {
    use super::*;
    use crate::prelude::*;

    fn _basic(child: impl UiNode) -> impl UiNode {
        button! {
            on_click: |_,_|{};
            background_gradient: 90.deg(), vec![rgb(0.0, 0.0, 0.0), rgb(1.0, 1.0, 1.0)];
            content: child;
        }
    }

    fn _args(child: impl UiNode) -> impl UiNode {
        button! {
            on_click: {
                handler: |_,_|{},
            };

            background_gradient: {
                angle: 90.deg(),
                stops: vec![rgb(0.0, 0.0, 0.0), rgb(1.0, 1.0, 1.0)]
            };

            content: child;
        }
    }

    fn _id(child: impl UiNode) -> impl UiNode {
        button! {
            on_click: |_,_|{};
            id: WidgetId::new_unique();
            content: child;
        }
    }

    fn _id_args(child: impl UiNode) -> impl UiNode {
        button! {
            on_click: |_,_|{};
            id: {
                id: WidgetId::new_unique()
            };
            content: child;
        }
    }
}
