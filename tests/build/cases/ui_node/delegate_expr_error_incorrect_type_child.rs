use zero_ui::core::{ui_node, widget_instance::UiNode};

struct NotANode;

struct MyNode {
    child: NotANode,
}

#[ui_node(child)]
impl UiNode for MyNode {}

fn main() {}
