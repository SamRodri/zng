#[crate::widget($crate::tests::fob)]
pub mod foo {
    use crate::{UiNode, WidgetId};

    fn new(child: impl UiNode, id: WidgetId) -> &'static str {
        let _ = child;
        let _ = id;
        "b"
    }
}
