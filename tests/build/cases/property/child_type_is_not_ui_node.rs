use zero_ui::wgt_prelude::{property, IntoVar, NilUiNode, UiNode};

struct NotUiNode;

#[property(CONTEXT)]
pub fn invalid_child(child: NotUiNode, input: impl IntoVar<bool>) -> impl UiNode {
    let _ = (child, input);
    NilUiNode
}

fn main() {}
