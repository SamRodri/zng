/*
    In-place capture_only Declaration
*/ 
properties! {
    /// Unnamed now
    foo: impl IntoVar<u32> = 10;
    fuz: impl IntoVar<u32>, impl IntoVar<32> = 10, 10;

    /// Named now
    bar: {
        a: u32,
        b: u32
    } = 10, 20;

    /// New unnamed #a
    foo: (impl IntoVar<u32>) = 10;
    fuz: (impl IntoVar<u32>, impl IntoVar<u32>) = 10, 20;
    // named stays the same.

    /// New unnamed #b
    foo(impl IntoVar<u32>) = 10;
    fuz(impl IntoVar<u32>, impl IntoVar<u32>) = 10, 20;
    /// New named #b
    bar {
        a: u32,
        b: u32
    } = 10, 20;

    /// New named #c
    fuz(a: impl IntoVar<u32>, b: impl IntoVar<u32>) = 10, 20;

    /// New unnamed #d
    foo: { impl IntoVar<u32> } = 10;
    foo: { impl IntoVar<u32>, impl IntoVar<u32> } = 10, 20;

    /// New Radical #a
    fn new_child(
        /// Capture Property
        fuz: (impl IntoVar<u32>, impl IntoVar<32>)
    ) -> imp UiNode {
        !
    }
    fn new_child(
        /// Capture Property
        fuz: (a: impl IntoVar<u32>, b: impl IntoVar<32>)
    ) -> imp UiNode {
        !
    }
}

/*
    Property Default Value
*/

// New #a
// 
// # Pros
//
// * Its the syntax proposed in a pre-RFC for default parameters (https://internals.rust-lang.org/t/pre-rfc-named-arguments/3831)
// 
// * Cons
//
// * Same parsing problem we are trying to avoid in in-place capture_only parsing.
// * rust-analyzer does not like this syntax, this sample is inside a macro to avoid an error and this file is not even linked.
macro_rules! _t { () => {

#[property(context)]
pub fn foo(child: impl UiNode, a: impl IntoVar<u32> = 10, b: impl IntoVar<u32> = 20) -> impl UiNode {
    child
}

}}

// New #b
#[property(context, default {
    b: 10,
    a: 10,
})]
pub fn foo(child: impl UiNode, a: impl IntoVar<u32>, b: impl IntoVar<u32>) -> impl UiNode {
    child
}
#[property(context, default(10, 20))]
pub fn bar(child: impl UiNode, a: impl IntoVar<u32>, b: impl IntoVar<u32>) -> impl UiNode {
    child
}

// New #c
#[property(context, default = {
    b: 10,
    a: 10,
})]
pub fn foo(child: impl UiNode, a: impl IntoVar<u32>, b: impl IntoVar<u32>) -> impl UiNode {
    child
}
#[property(context, default = 10, 20)]
pub fn bar(child: impl UiNode, a: impl IntoVar<u32>, b: impl IntoVar<u32>) -> impl UiNode {
    child
}

/*
    Required + Default Value
*/

// New #a
//
// # Pros
//
// * It looks OK.
//
// # Cons
//
// * This makes required! into a pseudo-macro, excluding a potential real macro `required`.
// * `required!;` still needs to be supported? It gets confusing, looks like two different things with the same name.
//   - We could call one `require!;` and the other `required!(T)`? No still confusing.
// * Users may tray to use `required!()` and then `unset!()`.
properties! {
    content = required!(NilUiNode);

    foo = required!;// <- still valid
}
properties! {
    content = required! {
        NilUiNode 
    };
}

// New #b
//
// # Pros
//
// * Not a pseudo-macro, no ambiguity, it is always `required!`.
// * Could be extended for whatever syntax we will use to implement animation.
// 
// # Cons
//
// * More complicated parsing, need to look ahead buffer each arg.
//
// # Questions
//
// * How does this work with named args?
properties! {
    content = required!, NewUiNode //, second_arg, etc.
}
// OR
properties! {
    content = NewUiNode, required!;
}

// New #c
//
// # Pros
//
// * Looks really good.
// * Keeps the value tokens for values only, no "special values".
//
// # Cons
//
// * Requires designing a new way to `unset!` OR it makes unset/remove be the only "special value".
properties! {
    #[required]
    content = NewUiNode;

    #[required]
    foo;
}

// 
properties! {
    remove { //renamed unset to remove
        inherited_property1;
        inherited_property2
    }
}