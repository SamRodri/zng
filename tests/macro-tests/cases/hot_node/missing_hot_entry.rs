use zng::prelude_wgt::{hot_node, UiNode};

// zng::hot_reload::zng_hot_entry!();

#[hot_node]
pub fn valid(child: impl UiNode) -> impl UiNode {
    child
}

fn main() {}
