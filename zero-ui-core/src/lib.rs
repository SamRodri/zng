#![warn(unused_extern_crates)]
// examples of `widget! { .. }` and `#[property(..)]` need to be declared
// outside the main function, because they generate a `mod` with `use super::*;`
// that does not import `use` clauses declared inside the parent function.
#![allow(clippy::needless_doctest_main)]
#![warn(missing_docs)]
#![cfg_attr(doc_nightly, feature(doc_cfg))]
#![cfg_attr(doc_nightly, feature(doc_notable_trait))]
#![recursion_limit = "256"]

//! Core infrastructure required for creating components and running an app.

/*!
<script>
// hide macros from doc root
document.addEventListener('DOMContentLoaded', function() {
    let removes = document.querySelectorAll('span[data-inline]');
    for (let remove of removes) {
        let div = remove.parentElement.parentElement;
        div.previousElementSibling.remove();
        div.remove();
    }
 })
</script>
 */

#[macro_use]
extern crate bitflags;

// to make the proc-macro $crate substitute work in doc-tests.
#[doc(hidden)]
#[allow(unused_extern_crates)]
extern crate self as zero_ui_core;

#[macro_use]
mod crate_util;

#[doc(hidden)]
pub use paste::paste;

pub mod animation;
pub mod app;
pub mod border;
pub mod color;
pub mod command;
pub mod context;
pub mod event;
pub mod inspector;
#[macro_use]
pub mod handler;
pub mod focus;
pub mod gesture;
pub mod gradient;
pub mod image;
pub mod keyboard;
pub mod mouse;
pub mod render;
pub mod service;
pub mod task;
pub mod text;
pub mod timer;
pub mod units;
pub mod var;
pub mod widget_base;
pub mod window;

mod state;

mod ui_node;
pub use ui_node::*;

mod ui_list;
pub use ui_list::*;

pub mod widget_info;
#[doc(inline)]
pub use widget_info::WidgetPath;

// proc-macros used internally during widget creation.
#[doc(hidden)]
pub use zero_ui_proc_macros::{property_new, static_list, widget_declare, widget_inherit, widget_new};

/// Expands an `impl` block into an [`UiNode`] trait implementation.
///
/// Missing [`UiNode`] methods are generated by this macro. The generation is configured in the macro arguments.
/// The arguments can be a single keyword or a pair of assigns.
///
/// The general idea is you implement only the methods required by your node and configure this macro to generate the methods
/// that are just boilerplate Ui tree propagation.
///
/// # Delegate to single `impl UiNode`
///
/// If your node contains a single child node, like most property nodes, you can configure the code
/// generator to delegate the method calls for the child node.
///
/// ```
/// # use zero_ui_core::{impl_ui_node, UiNode};
/// struct MyNode<C> {
///     child: C
/// }
/// #[impl_ui_node(
///     // Expression that borrows the delegation target node.
///     delegate = &self.child,
///     // Expression that exclusive borrows the delegation target node.
///     delegate_mut = &mut self.child,
/// )]
/// impl<C: UiNode> UiNode for MyNode<C> { }
/// ```
///
/// If the child node is in a field named `child` you can use this shorthand to the same effect:
///
/// ```
/// # use zero_ui_core::{impl_ui_node, UiNode};
/// # struct MyNode<C> { child: C }
/// #[impl_ui_node(child)]
/// impl<C: UiNode> UiNode for MyNode<C> { }
/// ```
///
/// The generated code simply calls the same [`UiNode`] method in the child.
///
/// # Delegate to a `impl UiNodeList`
///
/// If your node contains multiple children nodes in a type that implements [`UiNodeList`],
/// you can configure the code generator to delegate to the equivalent list methods.
///
/// ```
/// # use zero_ui_core::{impl_ui_node, UiNode, UiNodeList};
/// struct MyNode<L> {
///     children: L
/// }
/// #[impl_ui_node(
///     // Expression that borrows the delegation target list.
///     delegate_list = &self.children,
///     // Expression that exclusive borrows the delegation target list.
///     delegate_list_mut = &mut self.children,
/// )]
/// impl<L: UiNodeList> UiNode for MyNode<L> { }
/// ```
///
/// If the children list is a member named `children` you can use this shorthand to the same effect:
///
/// ```
/// # use zero_ui_core::{impl_ui_node, UiNode, UiNodeList};
/// # struct MyNode<L> { children: L }
/// #[impl_ui_node(children)]
/// impl<L: UiNodeList> UiNode for MyNode<L> { }
/// ```
///
/// The generated code simply calls the equivalent [`UiNodeList`] method in the list.
/// That is the same method name with the `_all` prefix. So `UiNode::init` maps to `UiNodeList::init_all` and so on.
///
/// # Delegate to an `impl IntoIterator<impl UiNode>`
///
/// If your node can produce an iterator of its children nodes you can configure the code generator to delegate
/// to the same [`UiNode`] method on each node.
///
/// ```
/// # use zero_ui_core::{impl_ui_node, UiNode, BoxedUiNode};
/// struct MyNode {
///     children: Vec<BoxedUiNode>
/// }
/// #[impl_ui_node(
///     delegate_iter = self.children.iter(),
///     delegate_iter_mut = self.children.iter_mut(),
/// )]
/// impl UiNode for MyNode { }
/// ```
///
/// If the children nodes are in a member named `children` of a type that has the `.iter()` and `.iter_mut()` methods
/// you can use this shorthand to the same effect:
///
/// ```
/// # use zero_ui_core::{impl_ui_node, UiNode, BoxedUiNode};
/// # struct MyNode { children: Vec<BoxedUiNode> }
/// #[impl_ui_node(children_iter)]
/// impl UiNode for MyNode { }
/// ```
///
/// The generated code calls [`into_iter`] and uses the iterator to apply the
/// same [`UiNode`] method on each child.
///
/// The generated [`measure`] code returns the desired size of the largest child.
///
/// The generated [`render`] code simply draws each child on top of the previous one.
///
/// ## Don't Delegate
///
/// If your node does not have any child nodes you can configure the code generator to generate empty missing methods.
///
/// ```
/// # use zero_ui_core::{impl_ui_node, UiNode};
/// # struct MyNode { }
/// #[impl_ui_node(none)]
/// impl UiNode for MyNode { }
/// ```
///
/// The generated [`measure`] code fills the available space or collapses if
/// any space is available (positive infinity).
///
/// The other generated methods are empty.
///
/// # Validation
///
/// If delegation is configured but no delegation occurs in the manually implemented methods
/// you get the error ``"auto impl delegates call to `{}` but this manual impl does not"``.
///
/// To disable this error use `#[allow_(zero_ui::missing_delegate)]` in the method or in the `impl` block.
///
/// # Mixing Methods
///
/// You can use the same `impl` block to define [`UiNode`] methods and
/// associated methods by using this attribute in a `impl` block without trait. The [`UiNode`]
/// methods must be tagged with the `#[UiNode]` pseudo-attribute.
///
/// ```
/// # use zero_ui_core::{impl_ui_node, UiNode, BoxedUiNode, context::WidgetContext};
/// # struct MyNode { child: BoxedUiNode }
/// #[impl_ui_node(child)]
/// impl MyNode {
///     fn do_the_thing(&mut self, ctx: &mut WidgetContext) {
///         // ..
///     }
///
///     #[UiNode]
///     fn init(&mut self, ctx: &mut WidgetContext) {
///         self.child.init(ctx);
///         self.do_the_thing(ctx);
///     }
///
///     #[UiNode]
///     fn update(&mut self, ctx: &mut WidgetContext) {
///         self.child.update(ctx);
///         self.do_the_thing(ctx);
///     }
/// }
/// ```
///
/// The above code expands to two `impl` blocks, one with the associated method and the other with
/// the [`UiNode`] implementation.
///
/// This is particularly useful for nodes that have a large amount of generic constrains, you just type then once.
///
/// [`UiNode`]: crate::UiNode
/// [`UiNodeList`]: crate::core::UiNodeList
/// [`into_iter`]: std::iter::IntoIterator::into_iter
/// [`measure`]: crate::UiNode::measure
/// [`render`]: crate::UiNode::render
///
/// <div style='display:none'>
#[doc(inline)]
pub use zero_ui_proc_macros::impl_ui_node;

/// Expands a function to a widget property module.
///
/// # Arguments
///
/// The macro attribute takes arguments that configure how the property can be used in widgets.
///
/// **Required**
///
/// The first argument is required and indicates when the property is set in relation to the other properties in a widget.
/// The valid values are: [`context`](#context), [`event`](#event), [`outer`](#outer), [`size`](#size), [`inner`](#inner) or
/// [`capture_only`](#capture_only).
///
/// **Optional**
///
/// Optional arguments can be set after the required, they use the `name = value` syntax. Currently there is only one
/// [`allowed_in_when`](#when-conditions).
///
/// # Function
///
/// The macro attribute must be set in a stand-alone function that sets the property by modifying the UI node tree.
///
/// ## Arguments and Output
///
/// The function argument and return type requirements are the same for normal properties (not `capture_only`).
///
/// ### Normal Properties
///
/// Normal properties must take at least two arguments, the first argument is the child [`UiNode`], the other argument(s)
/// are the property values. The function must return a type that implements [`UiNode`]. The first argument must support any
/// type that implements [`UiNode`], the other arguments also have type requirements depending on the [priority](#priority) or
/// [allowed-in-when](#when-integration). All of these requirements are validated at compile time.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::{property, UiNode, impl_ui_node, var::{Var, IntoVar}, context::WidgetContext};
/// #
/// struct MyNode<C, V> { child: C, value: V }
/// #[impl_ui_node(child)]
/// impl<C: UiNode, V: Var<&'static str>> UiNode for MyNode<C, V> {
///     fn init(&mut self, ctx: &mut WidgetContext) {
///         self.child.init(ctx);
///         println!("{}", self.value.get(ctx));
///     }
/// }
///
/// /// Property docs.
/// #[property(context)]
/// pub fn my_property(child: impl UiNode, value: impl IntoVar<&'static str>) -> impl UiNode {
///     MyNode { child, value: value.into_var() }
/// }
/// ```
///
/// ### `capture_only`
///
/// Capture-only properties do not modify the UI node tree, they exist only as a named bundle of arguments that widgets
/// capture to use internally. At least one argument is required. The return type must be never (`!`) and the property body must be empty.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::{property, var::IntoVar, text::Text};
/// /// Property docs.
/// #[property(capture_only)]
/// pub fn my_property(value: impl IntoVar<Text>) -> ! { }
/// ```
/// ## Limitations
///
/// There are some limitations to what kind of function can be used:
///
/// * Only standalone safe functions are supported, type methods, `extern` functions and `unsafe` are not supported.
/// * Only sized 'static types are supported.
/// * All stable generics are supported, generic bounds, impl trait and where clauses, const generics are not supported.
/// * Const functions are not supported. You need generics to support any type of UI node but generic const functions are unstable.
/// * Async functions are not supported.
/// * Only the simple argument pattern `name: T` are supported. Destructuring arguments or discard (_) are not supported.
///
/// ## Name
///
/// The property name follows some conventions that are enforced at compile time.
///
/// * `on_` prefix: Can only be used for `event` or `capture_only` properties and must take only a single event handler value.
/// * `is_` prefix: Can only take a single [`StateVar`] value.
///
/// # Priority
///
/// Except for `capture_only` the other configurations indicate the priority that the property must be applied to form a widget.
///
/// ## `context`
///
/// The property is applied after all other so that they can setup information associated with the widget that the other properties
/// can use. Context variables and widget state use this priority.
///
/// You can easily implement this properties using [`with_context_var`] and [`set_widget_state`].
///
/// ## `event`
///
/// Event properties are the next priority, they are set after all others except `context`, this way events can be configured by the
/// widget context properties but also have access to the widget visual they contain.
///
/// It is strongly encouraged that the event handler signature matches the one from [`on_event`].
///
/// ## `outer`
///
/// Properties that shape the visual outside of the widget, a `margin` property is an example.
///
/// ## `size`
///
/// Properties that set the widget visual size. Most widgets are sized automatically by their content, if the size is configured
/// by a user value the property has this priority.
///
/// ## `inner`
///
/// Properties that are set first, so they end-up inside of all other widget properties. Most of the properties that render use this priority.
///
/// # `when` Integration
///
/// Most properties are expected to work in widget `when` blocks, this is controlled by the optional argument `allowed_in_when`. By default all
/// properties that don't have the `on_` prefix are allowed. This can be overridden by setting `allowed_in_when = <bool>`.
///
/// If a property is `allowed_in_when` all arguments must be [`impl IntoVar<T>`]. This is validated during compile time, if you see
/// `allowed_in_when_property_requires_IntoVar_members` in a error message you need to change the type or disable `allowed_in_when`.
///
/// ## State Probing
///
/// Properties with the `is_` prefix are special, they output information about the widget instead of shaping it. They are automatically set
/// to a new probing variable when used in an widget when condition expression.
///
/// # Default
///
/// TODO
///
/// [`UiNode`]: crate::UiNode
/// [`StateVar`]: crate::var::StateVar
/// [`with_context_var`]: crate::properties::with_context_var
/// [`set_widget_state`]: crate::properties::set_widget_state
/// [`on_event`]: crate::event::on_event
/// [`impl IntoVar<T>`]: crate::var::IntoVar
///
/// <div style='display:none'>
#[doc(inline)]
pub use zero_ui_proc_macros::property;

/// Expands a module to a widget module and macro.
///
/// You can add any valid module item to a widget module, the widget attribute adds two pseudo-macros
/// [`inherit!`](#inherit) and [`properties!`](#properties), it also constrains functions named [`new_child`](#fn-new_child)
/// and [`new`](#fn-new).
///
/// After expansion the only visible change to the module is in the documentation appended, the module is still usable
/// as a namespace for any item you wish to add.
///
/// ```
/// # fn main() { }
/// # pub mod zero_ui { pub mod prelude { pub mod new_widget { pub use zero_ui_core::*; } } }
/// use zero_ui::prelude::new_widget::*;
///
/// #[widget($crate::foo)]
/// pub mod foo {
///     use super::*;
///     
///     // ..
/// }
/// ```
///
/// The widget macro takes one argument, a path to the widget module from [`$crate`].
/// This is a temporary requirement that will be removed when macros-by-example can reference the `self` module.
///
/// # Properties
///
/// Widgets are a *tree-rope* of [Ui nodes](zero_ui_core::UiNode), most of the nodes are defined and configured using
/// properties. Properties are defined using the `properties! { .. }` pseudo-macro. Multiple `properties!` items can be
/// used, they are merged during the widget compilation.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::{*, var::*};
/// # pub mod zero_ui{ pub mod properties {
/// #   use zero_ui_core::{*, var::*};
/// #   #[property(outer)]
/// #   pub fn margin(child: impl UiNode, m: impl IntoVar<u32>) -> impl UiNode { child }
/// # } }
/// #[widget($crate::foo)]
/// pub mod foo {
/// #   use super::*;
///     use zero_ui::properties::*;
///
///     properties! {
///         /// Margin applied by default.
///         margin = 10;
///     }
/// }
/// ```
///
/// ## Property Name
///
/// Only a property of each name can exist in a widget, during the widget instantiation the user can
/// set these properties by their name without needing to import the property.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::{widget, property, UiNode, var::IntoVar};
/// # #[property(context)]
/// # pub fn foo(child: impl UiNode, value: impl IntoVar<bool>) -> impl UiNode { child }
/// # #[widget($crate::bar)]
/// # pub mod bar {
/// #   use super::*;
/// properties! {
///     /// Foo docs in this widget.
///     foo;
/// }
/// # }
/// ```
///
/// You can also use the full path to a property in place, in this case the property name is the last ident in the path.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::widget;
/// # pub mod zero_ui { pub mod properties {
/// #   use zero_ui_core::{*, var::*};
/// #   #[property(outer)]
/// #   pub fn margin(child: impl UiNode, m: impl IntoVar<u32>) -> impl UiNode { child }
/// # } }
/// # #[widget($crate::foo)]
/// # pub mod foo {
/// # use super::*;
/// properties! {
///     /// Margin docs in this widget.
///     zero_ui::properties::margin;
/// }
/// # }
/// ```
///
/// And finally you can give a property a new name in place, you can use this to allow the same underlying property multiple times.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::widget;
/// # pub mod zero_ui { pub mod properties {
/// #   use zero_ui_core::{*, var::*};
/// #   #[property(outer)]
/// #   pub fn margin(child: impl UiNode, m: impl IntoVar<u32>) -> impl UiNode { child }
/// # } }
/// # #[widget($crate::foo)]
/// # pub mod foo {
/// # use super::*;
/// properties! {
///     /// Foo docs.
///     zero_ui::properties::margin as foo;
///     /// Bar docs.
///     zero_ui::properties::margin as bar;
/// }
/// # }
/// ```
///
/// ## Default Values
///
/// Properties without value are not applied unless the user sets then during instantiation. You can give a property
/// a default value so that it is always applied.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::{*, var::*};
/// # #[property(outer)]
/// # pub fn foo(child: impl UiNode, m: impl IntoVar<u32>) -> impl UiNode { child }
/// # #[widget($crate::bar)]
/// # pub mod bar {
/// #   use super::*;
/// properties! {
///     /// Foo, default value `10`.
///     foo = 10;
/// }
/// # }
/// ```
///
/// Note that the property can be removed during instantiation by using [`remove`](#remove).
///
/// ## Required
///
/// You can mark a property as *required*, meaning, the property must have a value during the widget instantiation,
/// and the property cannot be unset or removed. To mark the property use the pseudo-attribute `#[required]`.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::{*, var::*};
/// # #[property(outer)]
/// # pub fn bar(child: impl UiNode, m: impl IntoVar<u32>) -> impl UiNode { child }
/// # #[widget($crate::foo)]
/// # pub mod foo {
/// #   use super::*;
/// properties! {
///     #[required]
///     bar;
/// }
/// # }
/// ```
///
/// In the example above the required property must be set during the widget instantiation or a compile error is generated.
/// If another widget inherits from this one is also cannot remove the required property.
///
/// You can also give the required property a default value:
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::{*, var::*};
/// # #[property(outer)]
/// # pub fn bar(child: impl UiNode, m: impl IntoVar<u32>) -> impl UiNode { child }
/// # #[widget($crate::foo)]
/// # pub mod foo {
/// #   use super::*;
/// properties! {
///     #[required]
///     bar = 42;
/// }
/// # }
/// ```
///
/// In this case the property does not need to be set during instantiation, but it cannot be unset.
///
/// Note that captured properties are also marked required without the need for the pseudo-attribute.
///
/// ## Remove
///
/// Removes an [inherited](#inherit) property from the widget.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::{*, var::*, units::Alignment};
/// # #[property(outer)]
/// # pub fn content_align(child: impl UiNode, align: impl IntoVar<Alignment>) -> impl UiNode {
/// #   child
/// # }
/// # pub mod zero_ui { pub mod widgets {
/// # #[zero_ui_core::widget($crate::zero_ui::widgets::container)]
/// # pub mod container {
/// #   properties! { crate::content_align = crate::Alignment::CENTER; }
/// # }
/// # } }
/// # #[widget($crate::foo)]
/// # pub mod foo {
/// #    use super::zero_ui;
/// #    inherit!(zero_ui::widgets::container);
/// #
/// properties! {
///     remove { content_align }
/// }
/// # }
/// ```
///
/// ## Property Capture
///
/// The two [initialization functions](#initialization-functions) can *capture* a property.
/// When a property is captured it is not set by the property implementation, the property value is redirected to
/// the function and can be used in any way inside, some properties are [capture-only](zero_ui_core::property#capture_only),
/// meaning they don't have an implementation and must be captured.
///
/// ### Declare For Capture
///
/// You can declare a capture-only property in place:
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::widget;
/// # #[widget($crate::foo)]
/// # pub mod foo {
/// #    use zero_ui_core::var::*;
/// #    use zero_ui_core::UiNode;
/// #    use zero_ui_core::text::formatx;
/// #    fn text<T>(t: T) -> impl UiNode { zero_ui_core::NilUiNode }
/// #
/// properties! {
///     /// Capture-only property `foo` with default value `false`.
///     foo(impl IntoVar<bool>) = false;
/// }
///
/// fn new_child(foo: impl IntoVar<bool>) -> impl UiNode {
///     let label = foo.into_var().map(|f|formatx!("foo: {:?}", f));
///     text(label)
/// }
/// # }
/// ```
///
/// A property declared like this must be captured by the widget that is declaring it, a compile error is generated if it isn't.
///
/// You can set the property [`allowed_in_when`] value using the pseudo-attribute
/// `#[allowed_in_when = <bool>]`.
///
/// ### Captures Are Required
///
/// Captured properties are marked as [required](#required) in the widgets that declare then, there is no need to explicitly
/// annotate then with `#[required]`, for widget instance users it behaves exactly like a required property.
///
/// If the property is not explicitly marked however, widget inheritors can *remove* the property by declaring new
/// initialization functions that no longer capture the property. If it **is** marked explicitly then in must be captured
/// by inheritors, even if the source property was not `capture_only`.
///
/// ## Property Order
///
/// When a widget is initialized properties are set according with their [priority](zero_ui_core::property#priority) followed
/// by their declaration position. You can place a property in a [`child`](#child) block to have if be set before other properties.
///
/// The property value is initialized by the order the properties are declared, all [`child`](#child) property values are initialized first.
///
/// ### `child`
///
/// Widgets have two *groups* of properties, one is understood as applying to the widget, the other as applying to the [*child*](#fn-new_child).
/// To define a property in the second group, you can use a `child { .. }` block inside `properties! { }`.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::{*, var::*};
/// # #[property(outer)]
/// # pub fn margin(child: impl UiNode, m: impl IntoVar<u32>) -> impl UiNode { child }
/// # #[widget($crate::foo)]
/// # pub mod foo {
/// # use super::margin;
/// properties! {
///     child {
///         /// Spacing around the content.
///         margin as padding = 10;
///     }
/// }
/// # }
/// ```
///
/// ## When
///
/// Some widget properties need different values depending on widget state. You can manually implement this
/// using variable [mapping] and [merging] but a better way is to use the `when` block.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::{*, color::*, var::*};
/// # #[property(inner)]
/// # pub fn background_color(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode { child }
/// # #[property(context)]
/// # pub fn is_hovered(child: impl UiNode, state: StateVar) -> impl UiNode { child }
/// # #[property(context)]
/// # pub fn is_pressed(child: impl UiNode, state: StateVar) -> impl UiNode { child }
/// # #[widget($crate::foo)]
/// # pub mod foo {
/// # use super::*;
/// properties! {
///     background_color = colors::RED;
///
///     when self.is_hovered {
///         background_color = colors::BLUE;
///     }
///     when self.is_pressed {
///         background_color = colors::GREEN;
///     }
/// }
/// # }
/// ```
///
/// When blocks can be declared inside the `properties!` pseudo-macro, they take an expression followed by a block of
/// property assigns. You can reference widget properties in the expression by using the `self.` prefix.
///
/// In the example above the value of `background_color` will change depending on the interaction with the pointer, if it
/// is over the widget the background changes to blue, if it is pressed the background changes to green. Subsequent *whens* that
/// affect the same property have higher priority the previous whens, so when the pointer is over the widget and pressed the last
/// *when* (pressed color) is applied.
///
/// ### When Expression
///
/// The when expression is a boolean similar to the `if` expression, the difference in that it can reference [variables]
/// from properties or other sources, and when these variables updates the expression result updates.
///
/// #### Reference Property
///
/// Use `self.<property>` to reference to an widget property, the value resolves to the variable value of the first member of the property,
/// if the property has a default value it does not need to be defined in the widget before usage.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::{property, widget, UiNode, color::*, var::*};
/// # pub mod zero_ui { pub mod prelude { pub mod new_widget {
/// #   use crate::*;
/// #   #[property(inner)]
/// #   pub fn background_color(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode { child }
/// #   #[property(context)]
/// #   pub fn is_pressed(child: impl UiNode, state: StateVar) -> impl UiNode { child }
/// # } } }
/// #[property(context)]
/// pub fn foo(
///     child: impl UiNode,
///     member_a: impl IntoVar<bool>,
///     member_b: impl IntoVar<u32>
/// ) -> impl UiNode {
///     // ..
/// #   let _ = member_a;
/// #   let _ = member_b;
/// #   child
/// }
///
/// #[widget($crate::bar)]
/// pub mod bar {
/// #   use crate::zero_ui;
/// #   use zero_ui_core::color::colors;
///     use zero_ui::prelude::new_widget::*;
///
///     properties! {
///         background_color = colors::BLACK;
///         super::foo = true, 32;
///
///         when self.foo {
///             background_color = colors::RED;
///         }
///
///         when self.is_pressed {
///             background_color = colors::BLUE;
///         }
///     }
/// }
/// ```
///
/// In the example above `self.foo` is referencing the `member_a` variable value, note that `foo` was
/// defined in the widget first. [State variables] have a default value so
/// `is_pressed` can be used without defining it first in the widget.
///
/// #### Reference Property Member
///
/// A property reference automatically uses the first member, you can reference other members by name or by index.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::{property, widget, UiNode, var::IntoVar, color::*};
/// # #[property(context)]
/// # pub fn background_color(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode { child }
/// # #[property(context)]
/// # pub fn foo(child: impl UiNode, member_a: impl IntoVar<bool>, member_b: impl IntoVar<u32>) -> impl UiNode {  
/// #   let _ = member_a;
/// #   let _ = member_b;
/// #   child
/// # }
///
/// # #[widget($crate::bar)]
/// # pub mod bar {
/// #    use super::{background_color, colors};
/// properties! {
///     background_color = colors::BLACK;
///     super::foo = true, 32;
///
///     when self.foo.member_b == 32 {
///         background_color = colors::RED;
///     }
/// }
/// # }
/// ```
///
/// In the example above `self.foo.member_b` is referencing the `member_b` variable value. Alternatively you can also use
/// tuple indexing, `self.foo.1` also references the `member_b` variable value.
///
/// #### Reference Other Items
///
/// Widget when expressions can reference any other `'static` item, not just properties. If the item is a variable and you want
/// the expression to update when a variable update quote it with `#{<var>}`.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::{property, widget, UiNode, var::{IntoVar, context_var}, color::{Rgba, colors}};
/// # #[property(inner)]
/// # pub fn background_color(child: impl UiNode, c: impl IntoVar<Rgba>) -> impl UiNode { child }
/// # #[widget($crate::bar)]
/// # pub mod bar {
/// #   use super::*;
/// static ST_VALUE: bool = true;
///
/// context_var! { pub struct FooVar: bool = true; }
///
/// fn bar() -> bool { true }
///
/// properties! {
///     background_color = colors::BLACK;
///
///     when ST_VALUE && *#{FooVar::new()} && bar() {
///         background_color = colors::RED;
///     }
/// }
/// # }
/// ```
///
/// In the example above a static value `ST_VALUE`, a context var `FooVar` and a function `bar` are used in the expression. The expression
/// is (re)evaluated when the context var updates, `FooVar::var()` is evaluated only once during initialization.
///
/// ### Default States
///
/// Properties need to be assigned in a widget to participate in `when` blocks, this is because the generated code needs
/// to observe changes caused by the property, in the condition expression, or set the property to a default value when no
/// condition is active, assigned in when.
///
/// If the property has a default value and is not manually set in the widget it is set to the default value automatically.
///
/// Properties added automatically show in the widget documentation like manual properties, the widget user can see and set
/// then manually.
///
/// Currently only state properties have a default value, this will probably change in the future.
///
/// ### Auto-Disabling
///
/// It is not an error to use a property without default value (manual or auto) in a widget `when` block. If such a property is used
/// in the condition expression the `when` block is only used during initialization if the user sets the property.
///
/// If such a property is assigned in a `when` block it is also only used if it is set during initialization. In this case other
/// properties set in the same `when` block still use it.
///
/// You can use this to setup custom widget effects that are only activated if the widget instance actually uses a property.
///
/// # Initialization Functions
///
/// Widgets are a *tree-rope* of [Ui nodes](zero_ui_core::UiNode), the two initialization functions define the
/// inner ([`new_child`](#fn-new_child)) and outer ([`new`](#fn-new)) boundary of the widget.
///
/// The functions can *capture* properties by having an input of the same name as a widget property.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::{*, var::*, units::SideOffsets};
/// # #[property(outer)]
/// # pub fn margin(child: impl UiNode, m: impl IntoVar<SideOffsets>) -> impl UiNode { child }
/// # pub mod zero_ui { pub mod core {
/// #    pub use zero_ui_core::{NilUiNode, units, var}; }
/// #    pub mod properties { pub use crate::margin; }
/// # }
/// #[widget($crate::foo)]
/// pub mod foo {
/// #   use super::zero_ui;
///     use zero_ui::core::{NilUiNode, units::SideOffsets, var::IntoVar};
///     use zero_ui::properties::margin;
///
///     properties! {
///         margin = 10;
///     }
///
///     fn new_child(margin: impl IntoVar<SideOffsets>) -> NilUiNode {
///         // .. do something with margin.
///         NilUiNode
///     }
/// }
/// ```
///
/// In the example above the `margin` property is not applied during initialization,
/// its value is redirected the the `new_child` function. The input type must match the captured property type,
/// if the property has more then one member the input type is a tuple of the property types.
///
/// Initialization functions are not required, a the new widget inherits from another the functions from the other
/// widget are used, if not a default implementation is provided. The functions don't need to be public either, only
/// make then public is there is an utility in calling then manually.
///
/// The functions are identified by name and have extra constrains that are validated during compile time. In general
/// they cannot be `unsafe`, `async` nor `extern`, they also cannot declare lifetimes nor `const` generics.
///
/// ## `fn new_child`
///
/// The `new_child` initialization function defines the inner most node of the widget, it must output a type that implements
/// [`UiNode`].
///
/// The [default `new_child` function] does not capture any property and simply outputs
/// the [`NilUiNode`] value.
///
/// ## `fn new`
///
/// The `new` initialization function defines the outer most type of the widget, if must take at least one input that is a generic
/// that allows any [`UiNode`], although not required you probably want to capture the
/// implicit [`id`] property.
///
/// The output can be any type, if you want the widget to be compatible with most layout slots the type must implement
/// [`Widget`] and it is recommended that you use the [default new function]
/// to generate the widget.
///
/// The [default new function] captures the [`id`] property and returns a [`Widget`] node that establishes a widget context.
///
/// # `inherit!`
///
/// Widgets can inherit from one other widget and one or more other mix-ins using the pseudo-macro `inherit!(widget::path);`.
/// An inherit is like an import/reexport of properties and initialization functions.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::widget;
/// # pub mod zero_ui { pub mod widgets {
/// # use zero_ui_core::widget;
/// # #[widget($crate::zero_ui::widgets::container)]
/// # pub mod container { }
/// # } }
/// #[widget($crate::foo)]
/// pub mod foo {
/// #   use super::*;
///     inherit!(zero_ui::widgets::container);
///
///     // ..
/// }
/// ```
///
/// In the example above, the new widget `foo` inherits all the properties and
/// initialization functions of `container`.
///
/// ## Override
///
/// Subsequent inherits override properties with the same name as previously inherited. Properties
/// and functions declared in the new widget override inherited items.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::{widget, UiNode, widget_mixin, property};
/// # pub mod zero_ui { pub mod properties {
/// # #[zero_ui_core::property(outer)]
/// # pub fn margin(child: impl zero_ui_core::UiNode, margin: impl zero_ui_core::var::IntoVar<u32>) -> impl zero_ui_core::UiNode { child }
/// # } }
/// #[widget_mixin($crate::foo)]
/// pub mod foo {
/// #   use crate::zero_ui;
///     properties! {
///         zero_ui::properties::margin = 10;
///     }
/// }
///
/// #[widget_mixin($crate::bar)]
/// pub mod bar {
/// #   use crate::zero_ui;
///     properties! {
///         zero_ui::properties::margin = 20;
///     }
/// }
///
/// #[widget($crate::foo_bar)]
/// pub mod foo_bar {
/// #   use super::UiNode;
///     inherit!(super::foo);
///     inherit!(super::bar);
///
///     fn new_child() -> impl UiNode {
/// #       fn text(str: &str) -> impl UiNode { zero_ui_core::NilUiNode }
///         text("Bar!")
///     }
/// }
/// ```
///
/// In the example above `foo_bar` has a property named `margin` with default value `20`, and its child
/// is a text widget that prints `"Bar!"`.
///
/// ## Implicit
///
/// Every widget that does not inherit from another widget automatically inherits from
/// [`implicit_base`] before all other inherits.
///
/// ```
/// # fn main() { }
/// # use zero_ui_core::widget;
/// #[widget($crate::not_empty)]
/// pub mod not_empty { }
/// ```
///
/// In the example above `not_empty` contains the properties and new functions defined in the
/// [`implicit_base`].
///
/// [`$crate`]: https://doc.rust-lang.org/reference/macros-by-example.html#metavariables
/// [`implicit_base`]: mod@crate::widget_base::implicit_base
/// [`Widget`]: crate::Widget
/// [`id`]: mod@crate::widget_base::implicit_base#wp-id
/// [`UiNode`]: crate::UiNode
/// [`NilUiNode`]: crate::NilUiNode
/// [default new function]: crate::widget_base::implicit_base::new
/// [default `new_child` function]: crate::widget_base::implicit_base::new_child
/// [State variables]: crate::property#state-probing
/// [variables]: crate::var::Var
/// [mapping]: crate::var::Var::map
/// [merging]: crate::var::merge_var
/// [`allowed_in_when`]: crate::property#when-integration
///
/// <div style='display:none'>
pub use zero_ui_proc_macros::widget;

/// Expands a module to a widget mix-in module.
///
/// Widget mix-ins can only be inherited by other widgets and mix-ins, they cannot be instantiated.
///
/// See the [`#[widget(..)]`][#widget] documentation for how to declare, the only difference
/// from a full widget is that you can only inherit other mix-ins and cannot declare
/// the `new_child` and `new` functions.
///
/// [#widget]: macro@widget
///
/// <div style='display:none'>
pub use zero_ui_proc_macros::widget_mixin;

pub use crate_util::IdNameError;

mod tests;

mod private {
    // https://rust-lang.github.io/api-guidelines/future-proofing.html#sealed-traits-protect-against-downstream-implementations-c-sealed
    pub trait Sealed {}
}
