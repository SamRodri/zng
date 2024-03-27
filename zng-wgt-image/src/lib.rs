#![doc = include_str!("../../zng-app/README.md")]
//!
//! Image widget, properties and nodes.

#![warn(unused_extern_crates)]
#![warn(missing_docs)]

use zero_ui_ext_image::{ImageSource, Img};
use zero_ui_wgt::prelude::*;

mod image_properties;
pub use image_properties::*;

pub mod mask;

use zero_ui_wgt_access::{access_role, AccessRole};

pub mod node;

/// Image presenter.
///
/// This widget loads a still image from a variety of sources and presents it.
///
#[widget($crate::Image {
    ($source:expr) => {
        source = $source;
    };
})]
pub struct Image(WidgetBase);
impl Image {
    fn widget_intrinsic(&mut self) {
        self.widget_builder().push_build_action(on_build);

        widget_set! {
            self;
            access_role = AccessRole::Image;
        }
    }
}

/// The image source.
///
/// Can be a file path, an URI, binary included in the app and more.
#[property(CONTEXT, capture, widget_impl(Image))]
pub fn source(source: impl IntoVar<ImageSource>) {}

fn on_build(wgt: &mut WidgetBuilding) {
    let node = node::image_presenter();
    let node = node::image_error_presenter(node);
    let node = node::image_loading_presenter(node);
    wgt.set_child(node);

    let source = wgt.capture_var::<ImageSource>(property_id!(source)).unwrap_or_else(|| {
        let error = Img::dummy(Some(Txt::from_static("no source")));
        let error = ImageSource::Image(var(error).read_only());
        LocalVar(error).boxed()
    });
    wgt.push_intrinsic(NestGroup::EVENT, "image_source", |child| node::image_source(child, source));
}
