use parking_lot::Mutex;

use super::*;
use crate::widget::ui_node;
use std::{mem, sync::Arc};

/// Represents a node setup to dynamically swap child.
///
/// Any property node can be made adoptive by wrapping it with this node.
pub struct AdoptiveNode<U> {
    child: Arc<Mutex<BoxedUiNode>>,
    node: U,
    is_inited: bool,
}
impl<U: UiNode> AdoptiveNode<U> {
    /// Create the adoptive node, the [`AdoptiveChildNode`] must be used as the child of the created node.
    ///
    /// The created node is assumed to not be inited.
    pub fn new(create: impl FnOnce(AdoptiveChildNode) -> U) -> Self {
        let ad_child = AdoptiveChildNode::nil();
        let child = ad_child.child.clone();
        let node = create(ad_child);
        Self {
            child,
            node,
            is_inited: false,
        }
    }

    /// Create the adoptive node using a closure that can fail.
    ///
    /// The created node is assumed to not be inited.
    pub fn try_new<E>(create: impl FnOnce(AdoptiveChildNode) -> Result<U, E>) -> Result<Self, E> {
        let ad_child = AdoptiveChildNode::nil();
        let child = ad_child.child.clone();
        let node = create(ad_child)?;
        Ok(Self {
            child,
            node,
            is_inited: false,
        })
    }

    /// Replaces the child node.
    ///
    /// Returns the previous child, the initial child is a [`NilUiNode`].
    ///
    /// # Panics
    ///
    /// Panics if [`is_inited`](Self::is_inited).
    pub fn replace_child(&mut self, new_child: impl UiNode) -> BoxedUiNode {
        assert!(!self.is_inited);
        mem::replace(&mut *self.child.lock(), new_child.boxed())
    }

    /// Returns `true` if this node is inited.
    pub fn is_inited(&self) -> bool {
        self.is_inited
    }

    /// Into child reference and node.
    ///
    /// # Panics
    ///
    /// Panics if [`is_inited`](Self::is_inited).
    pub fn into_parts(self) -> (Arc<Mutex<BoxedUiNode>>, U) {
        assert!(!self.is_inited);
        (self.child, self.node)
    }

    /// From parts, assumes the nodes are not inited and that `child` is the actual child of `node`.
    pub fn from_parts(child: Arc<Mutex<BoxedUiNode>>, node: U) -> Self {
        Self {
            child,
            node,
            is_inited: false,
        }
    }
}
#[ui_node(delegate = &mut self.node)]
impl<U: UiNode> UiNode for AdoptiveNode<U> {
    fn init(&mut self) {
        self.is_inited = true;
        self.node.init();
    }
    fn deinit(&mut self) {
        self.is_inited = false;
        self.node.deinit();
    }
}

/// Placeholder for the dynamic child of an adoptive node.
///
/// This node must be used as the child of the adoptive node, see [`AdoptiveNode::new`] for more details.
pub struct AdoptiveChildNode {
    child: Arc<Mutex<BoxedUiNode>>,
}
impl AdoptiveChildNode {
    fn nil() -> Self {
        Self {
            child: Arc::new(Mutex::new(NilUiNode.boxed())),
        }
    }
}
#[ui_node(delegate = self.child.lock())]
impl UiNode for AdoptiveChildNode {}
