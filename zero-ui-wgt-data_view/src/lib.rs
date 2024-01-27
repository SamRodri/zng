#![doc = include_str!("../../zero-ui-app/README.md")]
//!
//! Data view widget.

#![warn(unused_extern_crates)]
#![warn(missing_docs)]

use parking_lot::Mutex;
use std::{any::TypeId, sync::Arc};

use zero_ui_wgt::prelude::*;

/// Arguments for the [`DataView!`] widget.
///
/// [`DataView!`]: struct@DataView
#[derive(Clone)]
pub struct DataViewArgs<D: VarValue> {
    data: BoxedVar<D>,
    replace: Arc<Mutex<Option<BoxedUiNode>>>,
    is_nil: bool,
}
impl<D: VarValue> DataViewArgs<D> {
    /// Reference the data variable.
    ///
    /// Can be cloned and used in the [`set_view`] to avoid rebuilding the info tree for every update.
    ///
    /// [`set_view`]: Self::set_view
    pub fn data(&self) -> &BoxedVar<D> {
        &self.data
    }

    /// If the current child is [`NilUiNode`];
    pub fn is_nil(&self) -> bool {
        self.is_nil
    }

    /// Get the current data value if [`is_nil`] or [`data`] is new.
    ///
    /// [`is_nil`]: Self::is_nil
    /// [`data`]: Self::data
    pub fn get_new(&self) -> Option<D> {
        if self.is_nil {
            Some(self.data.get())
        } else {
            self.data.get_new()
        }
    }

    /// Replace the child node.
    ///
    /// If set the current child node will be deinited and dropped.
    pub fn set_view(&self, new_child: impl UiNode) {
        *self.replace.lock() = Some(new_child.boxed());
    }

    /// Set the view to [`NilUiNode`].
    pub fn unset_view(&self) {
        self.set_view(NilUiNode)
    }
}

/// Dynamically presents a data variable.
///
/// The `update` widget handler is used to generate the view UI from the `data`, it is called on init and
/// every time `data` or `update` are new. The view is set by calling [`DataViewArgs::set_view`] in the widget function
/// args, note that the data variable is available in [`DataViewArgs::data`], a good view will bind to the variable
/// to support some changes, only replacing the UI for major changes.
#[widget($crate::DataView {
    (::<$T:ty>, $data:expr, $update:expr $(,)?) => {
        view::<$T> = {
            data: $data,
            update: $update,
        };
    }
})]
pub struct DataView(WidgetBase);
impl DataView {
    widget_impl! {
        /// Spacing around content, inside the border.
        pub zero_ui_wgt_container::padding(padding: impl IntoVar<SideOffsets>);

        /// Content alignment.
        pub zero_ui_wgt_container::child_align(align: impl IntoVar<Align>);

        /// Content overflow clipping.
        pub zero_ui_wgt::clip_to_bounds(clip: impl IntoVar<bool>);
    }
}

/// The view generator.
///
/// See [`DataView!`] for more details.
///
/// [`DataView!`]: struct@DataView
#[property(CHILD, widget_impl(DataView))]
pub fn view<D: VarValue>(child: impl UiNode, data: impl IntoVar<D>, update: impl WidgetHandler<DataViewArgs<D>>) -> impl UiNode {
    let data = data.into_var().boxed();
    let mut update = update.cfg_boxed();
    let replace = Arc::new(Mutex::new(None));

    match_node(child.boxed(), move |c, op| match op {
        UiNodeOp::Init => {
            WIDGET.sub_var(&data);
            update.event(&DataViewArgs {
                data: data.clone(),
                replace: replace.clone(),
                is_nil: true,
            });
            if let Some(child) = replace.lock().take() {
                *c.child() = child;
            }
        }
        UiNodeOp::Deinit => {
            c.deinit();
            *c.child() = NilUiNode.boxed();
        }
        UiNodeOp::Update { .. } => {
            if data.is_new() {
                update.event(&DataViewArgs {
                    data: data.clone(),
                    replace: replace.clone(),
                    is_nil: c.child().actual_type_id() == TypeId::of::<NilUiNode>(),
                });
            }

            update.update();

            if let Some(child) = replace.lock().take() {
                // skip update if nil -> nil, otherwise updates
                if c.child().actual_type_id() != TypeId::of::<NilUiNode>() || child.actual_type_id() != TypeId::of::<NilUiNode>() {
                    c.child().deinit();
                    *c.child() = child;
                    c.child().init();
                    c.delegated();
                    WIDGET.update_info().layout().render();
                }
            }
        }
        _ => {}
    })
}