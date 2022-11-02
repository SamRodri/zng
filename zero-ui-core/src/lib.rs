#![warn(unused_extern_crates)]
// examples of `widget! { .. }` and `#[property(..)]` need to be declared
// outside the main function, because they generate a `mod` with `use super::*;`
// that does not import `use` clauses declared inside the parent function.
#![allow(clippy::needless_doctest_main)]
#![allow(unstable_name_collisions)]
#![warn(missing_docs)]
#![cfg_attr(doc_nightly, feature(doc_cfg))]
#![cfg_attr(doc_nightly, feature(doc_notable_trait))]
#![recursion_limit = "256"]
// suppress nag about very simple boxed closure signatures.
#![allow(clippy::type_complexity)]

//! Core infrastructure required for creating components and running an app.

#[macro_use]
extern crate bitflags;

// to make the proc-macro $crate substitute work in doc-tests.
#[doc(hidden)]
#[allow(unused_extern_crates)]
extern crate self as zero_ui_core;

#[macro_use]
mod crate_util;

#[cfg(any(test, feature = "test_util"))]
pub use crate_util::test_log;

#[doc(hidden)]
pub use paste::paste;

#[macro_use]
pub mod handler;

pub mod app;
pub mod border;
pub mod color;
pub mod config;
pub mod context;
pub mod event;
pub mod focus;
pub mod gesture;
pub mod gradient;
pub mod image;
pub mod inspector;
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
pub mod widget_builder;
pub mod widget_info;
pub mod widget_instance;
pub mod window;

// proc-macros used internally during widget creation.
#[doc(hidden)]
pub use zero_ui_proc_macros::widget_new;

/// Expands an `impl` block into an [`UiNode`] trait implementation or new node declaration.
///
/// Missing [`UiNode`] methods are generated by this macro. The generation is configured in the macro arguments.
/// The arguments can be a single keyword, a pair of delegates or an entire struct declaration.
///
/// The general idea is you implement only the methods required by your node and configure this macro to generate the methods
/// that are just boilerplate UI tree propagation, and in [new node](#new-node) mode var and event handlers can be inited automatically
/// as well.
///
/// # Delegate to single `impl UiNode`
///
/// If your node contains a single child node, like most property nodes, you can configure the code
/// generator to delegate the method calls for the child node.
///
/// ```
/// # use zero_ui_core::{ui_node, widget_instance::UiNode};
/// struct MyNode<C> {
///     child: C
/// }
/// #[ui_node(
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
/// # use zero_ui_core::{ui_node, widget_instance::UiNode};
/// # struct MyNode<C> { child: C }
/// #[ui_node(child)]
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
/// # use zero_ui_core::{ui_node, widget_instance::*};
/// struct MyNode<L> {
///     children: L
/// }
/// #[ui_node(
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
/// # use zero_ui_core::{ui_node, widget_instance::*};
/// # struct MyNode<L> { children: L }
/// #[ui_node(children)]
/// impl<L: UiNodeList> UiNode for MyNode<L> { }
/// ```
///
/// The generated code simply calls the equivalent [`UiNodeList`] method in the list.
/// That is the same method name with the `_all` prefix. So `UiNode::init` maps to `UiNodeList::init_all` and so on.
///
/// ## Don't Delegate
///
/// If your node does not have any child nodes you can configure the code generator to generate empty missing methods.
///
/// ```
/// # use zero_ui_core::{ui_node, widget_instance::UiNode};
/// # struct MyNode { }
/// #[ui_node(none)]
/// impl UiNode for MyNode { }
/// ```
///
/// The generated [`measure`] and [`layout`] code returns the fill size.
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
/// The [`measure`] method is an exception to this and will not show the error, its ideal implementation
/// is one where the entire sub-tree is skipped from the the computation.
///
/// # Mixing Methods
///
/// You can use the same `impl` block to define [`UiNode`] methods and
/// associated methods by using this attribute in a `impl` block without trait. The [`UiNode`]
/// methods must be tagged with the `#[UiNode]` pseudo-attribute.
///
/// ```
/// # use zero_ui_core::{ui_node, widget_instance::*, context::*};
/// # struct MyNode { child: BoxedUiNode }
/// #[ui_node(child)]
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
///     fn update(&mut self, ctx: &mut WidgetContext, updates: &mut WidgetUpdates) {
///         self.child.update(ctx, updates);
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
/// # New Node
///
/// In all the usage seen so far you must declare the `struct` type yourself, and the generic bounds to
/// make it work in the `impl` block, and any var or event in it needs to be subscribed manually. You can
/// avoid this extra boilerplate by declaring the node `struct` as an arg for the macro.
///
/// ```
/// # use zero_ui_core::{ui_node, widget_instance::UiNode, context::*, var::*};
/// fn my_widget_node(child: impl UiNode, number: impl IntoVar<u32>) -> impl UiNode {
///     #[ui_node(struct MyNode {
///         child: impl UiNode,
///         #[var] number: impl Var<u32>,
///     })]
///     impl UiNode for MyNode {
///         fn update(&mut self, ctx: &mut WidgetContext, updates: &mut WidgetUpdates) {
///             self.child.update(ctx, updates);
///             if let Some(n) = self.number.get_new(ctx) {
///                 println!("new number: {n}");
///             }
///         }
///     }
///     MyNode {
///         child,
///         number: number.into_var(),
///     }
/// }
/// ```
///
/// In the example above the `MyNode` struct is declared with two generic params: `T_child` and `T_var`, the unimplemented
/// node methods are delegated to `child` because of the name, and the `number` var is subscribed automatically because of
/// the `#[var]` pseudo attribute.
///
/// This syntax can save a lot of typing and improve readability for nodes that have multiple generic parameters, it is ideal
/// for declaring *anonymous* nodes, like those returned by functions with return type `-> impl UiNode`.
///
/// ## Generics
///
/// You can declare named generics in the `struct`, those are copied to the implement block, you can also have members with type
/// `impl Trait`, a named generic is generated for these, the generated name is `T_member`. You can use named generics in the `impl`
/// generics the same way as you would in a function.
///
/// ## Impl Block
///
/// The impl block cannot have any generics, they are added automatically, the `UiNode for` part is optional, like in the delegating
/// mode, if you omit the trait you must annotate each node method with the `#[UiNode]` pseudo attribute.
///
/// ## Delegation
///
/// Delegation is limited to members named `child` or `children`, there is no way to declare a custom delegation in *new node*
/// mode. If no specially named member is present the `none` delegation is used.
///
/// ## Subscription
///
/// You can mark members with the `#[var]` or `#[event]` pseudo attributes to generate initialization code that subscribes the var or
/// event to the [`WidgetContext::handles`]. The init code is placed in a method with signature `fn init_handles(&mut self, &mut WidgetContext)`,
/// if you manually implement the `init` node method you must call `self.init_handles(ctx);` in it.
///
/// ## Limitations
///
/// The new node type must be private, you cannot set visibility modifiers. The struct cannot have any attribute set on it, but you can
/// have attributes in members, the `#[cfg]` attribute is copied to generated generics. The `impl Trait` auto-generics only works for
/// the entire type of a generic, you cannot declare a type `Vec<impl Debug>` for example.
///
/// The new node syntax is designed to alleviate the boilerplate of declaring nodes that are just implementation detail of properties and widgets.
///
/// [`UiNode`]: crate::UiNode
/// [`UiNodeList`]: crate::UiNodeList
/// [`measure`]: crate::UiNode::measure
/// [`layout`]: crate::UiNode::layout
/// [`render`]: crate::UiNode::render
/// [`WidgetContext::handles`]: crate::context::WidgetContext::handles
///
/// <div style='display:none'>
#[doc(inline)]
pub use zero_ui_proc_macros::ui_node;

/// Expands a function to a widget property module.
/// TODO !!:
#[doc(inline)]
pub use zero_ui_proc_macros::property;

/// Expands a module to a widget module and macro.
///
/// Each widget is a module and macro pair that construct a [`WidgetBuilder`] and instantiates a custom widget type.  Widgets
/// can *inherit* from one other widget and from multiple [mix-ins](macro@widget_mixin), they can define properties with and without
/// a default value and can add build actions to the [`WidgetBuilder`] that generates intrinsic nodes that define the widget behavior.
///
/// # Attribute
///
/// The widget attribute must be placed in a `mod name { }` module declaration, only modules with inline content are supported, mods
/// with content in other files will cause a compile time error. You also cannot set the attribute from the inside `#!`, this  is a current
/// limitation of the Rust compiler.
///
/// The attribute receives one argument, it must be a macro style `$crate` path to the widget module, this is used in the generated macro
/// to find the module during instantiation. The path must be to the *public* path to the module, that is, the same path that will be used
/// to import the widget macro.
///
/// ```
/// # fn main() { }
/// use zero_ui_core::widget;
///
/// /// Minimal widget.
/// #[widget($crate::foo)]
/// pub mod foo {
///     inherit!(zero_ui_core::widget_base::base);
/// }
/// ```
///
/// Because Rust does not allow custom inner attributes you cannot have a file per widget, the main zero-ui crate works around this
/// by declaring the widget in a private `name_wgt.rs` and then re-exporting it. For example, the code above can be placed in a
/// `foo_wgt.rs` file and then re-exported in the `lib.rs` file using:
///
/// ```
/// # fn main() { }
/// mod foo_wgt;
/// #[doc(inline)]
/// pub use foo_wgt::*;
/// ```
///
/// # Inherit
///
/// Inside the widget module the `inherit!` pseudo-macro can be used *import* another widget and multiple mix-ins. All properties
/// in the other widget are imported and re-exported, the widget [include](#include) function is called before the widget's own and
/// the [build](#build) function is used if the widget does not override it.
///
/// Apart from some special cases widgets should always inherit from another, in case no specific parent is needed the widget should
/// inherit from [`widget_base::base`]. The base widget implements the minimal collaborative layout and render mechanisms that are
/// expected by properties and other widgets.
///
/// # Include
///
/// The widget module can define a function that *includes* custom build actions in the [`WidgetBuilder`] that is generated during
/// [instantiation](#instantiation).
///
/// ```
/// # fn main() { }
/// use zero_ui_core::{widget, widget_base::*, widget_builder::*};
///
/// #[widget($crate::foo)]
/// pub mod foo {
///     use super::*;
///
///     inherit!(base);
///
///     fn include(wgt: &mut WidgetBuilder) {
///         wgt.push_build_action(|wgt| {
///             // push_intrinsic, capture_var.
///         });
///     }
/// }
/// ```
///
/// The example above demonstrate the function used to [`push_build_action`]. This is the primary mechanism for widgets to define their
/// own behavior that does not depend on properties. Note that the widget inherits from [`widget_base::base`], during [instantiation](#instantiation)
/// of `foo!` the base include is called first, then the `foo!` include is called.
///
/// # Build
///
/// The widget module can define a function that *builds* the final widget instance.
///
/// ```
/// # fn main() { }
/// use zero_ui_core::{widget, widget_base, widget_builder::*, widget_instance::*};
///
/// #[widget($crate::foo)]
/// pub mod foo {
///     use super::*;
///
///     inherit!(widget_base::base);
///
///     fn build(wgt: WidgetBuilder) -> impl UiNode {
///         widget_base::nodes::widget(wgt)
///     }
/// }
/// ```
///
/// The build function takes the [`WidgetBuilder`] already loaded with includes and properties, the function can define its own
/// return type, this is the **widget type**. If the build function is not defined the inherited parent build function is used,
/// if the widget does not inherit from any other the build function is required, and a compile error is shown if it is missing.
///
/// Unlike the [include](#include) function, the widget only has one `build`, if defined it overrides the parent `build`. Most widgets
/// don't define their own build, leaving it to be inherited from [`widget_base::base`]. The base type is an opaque `impl UiNode`, normal
/// widgets must implement [`UiNode`], otherwise they cannot be used as child of other widgets, the widget outer-node also must implement
/// the widget context, to ensure that the widget is correctly placed in the UI tree. The base widget implementation is in [`widget_base::nodes::widget`],
/// you can use it directly, so even if you need to run code on build or define a custom type you don't need to start from scratch.
///
/// # Properties
///
/// Inside the widget module the `properties!` pseudo-macro can used to declare properties of the widget. The properties can
/// be assigned, renamed and exported as widget properties.
/// 
/// ```
/// # fn main() { }
/// use zero_ui_core::{*, widget_builder::*, widget_instance::*};
/// 
/// #[property(context)]
/// pub fn bar(child: impl UiNode, val: impl IntoVar<bool>) -> impl UiNode {
///   let _ = val;
///   child
/// }
///
/// #[widget($crate::foo)]
/// pub mod foo {
///     use super::*;
///
///     inherit!(widget_base::base);
///
///     properties! {
///         /// Baz property docs.
///         pub bar as baz = true;
///         // inherited property
///         enabled = false;
///     }
/// }
/// ```
/// 
/// The example above declares an widget that exports the property `baz`, it is also automatically set to `true` and it also 
/// sets the inherited [`widget_base::base`] property `enabled` to `false`.
/// 
/// The property visibility controls if it is assignable in derived widgets or during widget instantiation, in the example above
/// if `baz` was not `pub` it would be set on the widget but it does not get a `baz` property accessible from outside. Inherited
/// visibility cannot be overridden, the `enabled` property is defined as `pub` in [`widget_base::base`] so it is still `pub` in the
/// widget, even though the value was changed.
/// 
/// You can also export properties without defining a value, the default assign is not required, the property is only instantiated
/// if it is assigned in the final widget instance, but by exporting the property it is available in the widget macro by name without
/// needing a `use` import.
/// 
/// ## Unset
/// 
/// If an inherited property is assigned a value you can *unset* this value by assigning the property with the special value `unset!`.
/// 
/// ```
/// # fn main() { }
/// use zero_ui_core::{*, widget_builder::*, widget_instance::*};
/// # 
/// # #[property(context)]
/// # pub fn bar(child: impl UiNode, val: impl IntoVar<bool>) -> impl UiNode {
/// #   let _ = val;
/// #   child
/// # }
/// # 
/// # #[widget($crate::foo)]
/// # pub mod foo {
/// #     use super::*;
/// # 
/// #     inherit!(widget_base::base);
/// # 
/// #     properties! {
/// #         /// Baz property docs.
/// #         pub bar as baz = true;
/// #     }
/// # }
/// #[widget($crate::bar)]
/// pub mod bar {
///     inherit!(crate::foo);
///     
///     properties! {
///         baz = unset!;
///     }
/// }
/// ```
/// 
/// In the example above the widget `bar` inherits the `foo` widget that defines and sets the `baz` property. Instances of the
/// `bar` property will not include an instance of `baz` because it was `unset!`. Note that this does not remove the property
/// the `bar` widget still exports the `baz` property, it just does not have a default value anymore.
/// 
/// An `unset!` assign also removes all `when` assigns to the same property, this is unlike normal assigns that just override the
/// *default* value of the property, merged with the `when` assigns. 
/// 
/// ## Multiple Inputs
/// 
/// Some properties have multiple inputs, you can use a different syntax to assign each input by name or as a comma separated list. 
/// In the example below the property `anb` has two inputs `a` and `b`, they are assigned by name in the `named` property and by
/// position in the `unnamed` property. Note that the order of inputs can be swapped in the named init.
/// 
/// ```
/// # fn main() { }
/// use zero_ui_core::{*, widget_builder::*, widget_instance::*, var::*};
/// 
/// #[property(context)]
/// pub fn anb(child: impl UiNode, a: impl IntoVar<bool>, b: impl IntoVar<bool>) -> impl UiNode {
///   let _ = (a, b);
///   child
/// }
///
/// #[widget($crate::foo)]
/// pub mod foo {
///     use super::*;
///
///     inherit!(widget_base::base);
///
///     properties! {
///         pub anb as named = {
///             b: false,
///             a: true,
///         };
///         pub anb as unnamed = true, false;
///     }
/// }
/// ```
/// 
/// ## When
/// 
/// Conditional property assigns can be setup using `when` blocks. A `when` block has a `bool` expression and multiple property assigns,
/// when the expression is `true` each property has the assigned value, unless it is overridden by a later `when` block.
/// 
/// ```
/// # use zero_ui_core::{*, widget_builder::*, widget_instance::*, color::*};
/// #
/// # #[property(fill)]
/// # pub fn background_color(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode {
/// #   let _ = color;
/// #   child
/// # }
/// #
/// # #[property(layout)]
/// # pub fn is_pressed(child: impl UiNode, state: var::StateVar) -> impl UiNode {
/// #   let _ = val;
/// #   child
/// # } 
/// #
/// # #[widget($crate::foo)]
/// # pub mod foo {
/// #     use super::*;
/// # 
/// #     inherit!(widget_base::base);
/// #
/// properties! {
///     background_color = colors::RED;
/// 
///     when *#is_pressed {
///         background_color = colors::GREEN;
///     }
/// }
/// # }
/// ```
/// 
/// ### When Condition
/// 
/// The `when` block defines a condition expression, in the example above this is `*#is_pressed`. The expression can be any Rust expression
/// that results in a [`bool`] value, you can reference properties in it using the `#` token followed by the property name or path and you
/// can reference variables in it using the `#{var}` syntax. If a property or var is reference the `when` block is dynamic, updating all
/// assigned properties when the expression result changes.
/// 
/// ### Property Reference
/// 
/// The most common `when` expression reference is a property, in the example above the `is_pressed` property is instantiated for the widget
/// and it's [`StateVar`] input controls when the background is set to green. Note that a reference to the value is inserted in the expression
/// so an extra deref `*` is required. A property can also be referenced with a path, `#properties::is_pressed` also works.
/// 
/// The syntax seen so far is actually a shorthand way to reference the first input of a property, the full syntax is `#is_pressed.0` or 
/// `#is_pressed.state`. You can use the extended syntax to reference inputs of properties with out than one input, the input can be
/// reference by tuple-style index or by name. Note that if the value it self is a tuple or `struct` you need to use the extended syntax
/// to reference a member of the value, `#foo.0.0` or `#foo.0.name`. Methods have no ambiguity, `#foo.name()` is the same as `#foo.0.name()`.
/// 
/// Not all properties can be referenced in `when` conditions, only inputs of type [`StateVar`], `impl IntoVar<T>` and `impl IntoValue<T>` are
/// allowed, attempting to reference a different kind of input generates a compile error.
/// 
/// ### Variable Reference
/// 
/// Other variable can also be referenced, in a widget declaration only context variables due to placement, but in widget instances any locally
/// declared variable can be referenced. Like with properties the variable value is inserted in the expression as a reference  so you may need
/// to deref in case the var is a simple [`Copy`] value.
/// 
/// ```
/// # use zero_ui_core::{*, widget_builder::*, widget_instance::*, color::*};
/// #
/// # #[property(fill)]
/// # pub fn background_color(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode {
/// #   let _ = color;
/// #   child
/// # }
/// #
/// context_var! { 
///     pub static FOO_VAR: Vec<&'static str> = vec![];
///     pub static BAR_BAR: bool = false;
/// }
/// #
/// # #[widget($crate::foo)]
/// # pub mod foo {
/// #     use super::*;
/// # 
/// #     inherit!(widget_base::base);
/// 
/// properties! {
///     background_color = colors::RED;
/// 
///     when !*#{BAR_VAR} && #{FOO_VAR}.contains("green") {
///         background_color = colors::GREEN;
///     }
/// }
/// # }
/// ```
/// 
/// ### When Assigns
/// 
/// Inside the `when` block a list of property assigns is expected, only properties with all inputs of type `impl IntoVar<T>` can ne assigned
/// in `when` blocks, you also cannot `unset!` in when assigns. On instantiation a single instance of the property will be generated, the input
/// vars will track the when expression state and update to the value assigned in the block when it is `true`. When no block is `true` the value
/// assigned to the property outside `when` blocks is used, or the property default value. When more then one block is `true` the *last* one
/// sets the value. 
/// 
/// ### Default Values
/// 
/// A when assign can be defined by a property without setting a default value, during instantiation if the property declaration has
/// a default value it is used, or if the property was later assigned a value it is used as *default*, if it is not possible to generate
/// a default value the property is not instantiated and the when assign is not used.
/// 
/// The same apply for properties referenced in the condition expression, note that all `is_state` properties have a default value so
/// it is more rare that a default value is not available. If a condition property cannot be generated the entire when block is ignored.
///
/// # Instantiation
///
/// After the widget macro attribute expands you can still use the module like any other mod, but you can also use it like a macro that
/// accepts property inputs like the `properties!` pseudo-macro, except for the visibility control.
/// 
/// ```
/// # use zero_ui_core::{*, widget_builder::*, widget_instance::*, color::*};
/// # 
/// # #[property(context)]
/// # pub fn bar(child: impl UiNode, val: impl var::IntoVar<bool>) -> impl UiNode {
/// #   let _ = val;
/// #   child
/// # }
/// #
/// # #[property(layout)]
/// # pub fn margin(child: impl UiNode, val: impl var::IntoVar<u32>) -> impl UiNode {
/// #   let _ = val;
/// #   child
/// # }
/// #
/// # #[property(fill)]
/// # pub fn background_color(child: impl UiNode, color: impl IntoVar<Rgba>) -> impl UiNode {
/// #   let _ = color;
/// #   child
/// # }
/// #
/// # #[property(layout)]
/// # pub fn is_pressed(child: impl UiNode, state: var::StateVar) -> impl UiNode {
/// #   let _ = val;
/// #   child
/// # } 
/// #
/// # #[widget($crate::foo)]
/// # pub mod foo {
/// #     use super::*;
/// # 
/// #     inherit!(widget_base::base);
/// # 
/// #     properties! {
/// #         /// Baz property docs.
/// #         pub bar as baz = true;
/// #         // inherited property
/// #         enabled = false;
/// #     }
/// # }
/// # fn main() {
/// let wgt = foo! {
///     baz = false;
///     margin = 10;
///     
///     when *#is_pressed {
///         background_color = colors::GREEN;
///     }
/// }; 
/// # }
/// ```
/// 
/// In the example above  the `baz` property is imported from the `foo!` widget, all widget properties are imported inside the
/// widget macro call, and `foo` exported `pub bar as baz`. The value of `baz` is changed for this instance, the instance also
/// gets a new property `margin`, that was not defined in the widget.
/// 
/// Most of the features of `properties!` can be used in the widget macro, you can `unset!` properties or rename then using the `original as name`
/// syntax. You can also setup `when` conditions, as demonstrated above, the `background_color` is `GREEN` when `is_pressed`, these properties
/// also don't need to be defined in the widget before use, but if they are they are used instead of the contextual imports.
/// 
/// ## Init Shorthand 
/// 
/// The generated instantiation widget macro also support the *init shorthand* syntax, where the name of a `let` variable defines the property
/// name and value. In the example below the `margin` property is set on the widget with the value of `margin`.
/// 
/// ```
/// # macro_rules! demo {
/// # () => {
/// let margin = 10;
/// let wgt = foo! {
///     margin;
/// };
/// # };
/// # }
/// ```
/// 
/// # More Details
/// 
/// See the [`WidgetBuilder`], [`WidgetBuilding`], [`Priority`] and [`Importance`] for more details of how the parts expanded from this macro are 
/// put together to form a widget instance.
///
/// [`WidgetBuilder`]: widget_builder::WidgetBuilder
/// [`WidgetBuilding`]: widget_builder::WidgetBuilding
/// [`Priority`]: widget_builder::Priority
/// [`Importance`]: widget_builder::Importance
/// [`push_build_action`]: widget_builder::WidgetBuilder::push_build_action
/// [`UiNode`]: widget_instance::UiNode
/// [`StateVar`]: var::StateVar
#[doc(inline)]
pub use zero_ui_proc_macros::widget;

/// Expands a module to a widget mix-in module.
///
/// Widget mix-ins can only be inherited by other widgets and mix-ins, they cannot be instantiated. Widgets can only
/// inherit from one other widget, but they can inherit from many mix-ins. All mix-in names must have the `_mixin` suffix,
/// this is validated at compile time. Mix-ins represent a set of properties and build actions that adds a complex feature to an widget,
/// something that cannot be implemented as a single property.
///
/// See the [`#[widget(..)]`][#widget] documentation for how to declare, the only difference
/// from a full widget is that you can only inherit other mix-ins and cannot declare
/// the `build` function.
///
/// [#widget]: macro@widget
///
/// <div style='display:none'>

#[doc(inline)]
pub use zero_ui_proc_macros::widget_mixin;

mod tests;

mod private {
    // https://rust-lang.github.io/api-guidelines/future-proofing.html#sealed-traits-protect-against-downstream-implementations-c-sealed
    pub trait Sealed {}
}
