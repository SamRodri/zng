//! zero-ui proc-macros.
//!
//! Documentation of macros are done at the final reexport place in the main crate.

extern crate proc_macro;

#[macro_use]
extern crate quote;

use proc_macro::TokenStream;

#[macro_use]
mod util;

mod derive_service;
pub(crate) mod expr_var;
mod hex_color;
mod when_var;

mod impl_ui_node;
pub(crate) mod property;

mod widget_0_attr;
mod widget_1_inherit;
mod widget_2_declare;

mod widget_new2;

pub(crate) mod widget_new;
mod widget_stage1;
mod widget_stage2;
pub(crate) mod widget_stage3;

#[proc_macro_attribute]
pub fn impl_ui_node(args: TokenStream, input: TokenStream) -> TokenStream {
    impl_ui_node::gen_impl_ui_node(args, input)
}

#[proc_macro_attribute]
pub fn property(args: TokenStream, input: TokenStream) -> TokenStream {
    property::expand(args, input)
}

#[proc_macro]
pub fn widget(input: TokenStream) -> TokenStream {
    widget_stage1::expand(false, input)
}

#[proc_macro]
pub fn widget_mixin(input: TokenStream) -> TokenStream {
    widget_stage1::expand(true, input)
}

// Recursive include inherited tokens. Called by the expansion of widget_state1 and widget_stage2.
#[proc_macro]
pub fn widget_stage2(input: TokenStream) -> TokenStream {
    widget_stage2::expand(input)
}

// Final widget or mix-in expansion. Called by the final expansion of widget_stage2.
#[proc_macro]
pub fn widget_stage3(input: TokenStream) -> TokenStream {
    widget_stage3::expand(input)
}

// Instantiate widgets. Called by widget macros generated by [`widget!`](widget).
#[proc_macro]
pub fn widget_new(input: TokenStream) -> TokenStream {
    widget_new::expand(input)
}

#[proc_macro]
pub fn hex_color(input: TokenStream) -> TokenStream {
    hex_color::expand(input)
}

#[proc_macro_derive(AppService)]
pub fn derive_app_service(item: TokenStream) -> TokenStream {
    derive_service::derive(item, ident!("AppService"))
}

#[proc_macro_derive(WindowService)]
pub fn derive_window_service(item: TokenStream) -> TokenStream {
    derive_service::derive(item, ident!("WindowService"))
}

#[proc_macro_attribute]
pub fn widget2(args: TokenStream, input: TokenStream) -> TokenStream {
    widget_0_attr::expand(false, args, input)
}

#[proc_macro_attribute]
pub fn widget_mixin2(args: TokenStream, input: TokenStream) -> TokenStream {
    widget_0_attr::expand(true, args, input)
}

#[proc_macro]
pub fn widget_inherit(input: TokenStream) -> TokenStream {
    widget_1_inherit::expand(input)
}

#[proc_macro]
pub fn widget_declare(input: TokenStream) -> TokenStream {
    widget_2_declare::expand(input)
}

#[proc_macro]
pub fn widget_new2(input: TokenStream) -> TokenStream {
    widget_new2::expand(input)
}

#[proc_macro]
pub fn expr_var(input: TokenStream) -> TokenStream {
    expr_var::expand(input)
}

#[proc_macro]
pub fn when_var(input: TokenStream) -> TokenStream {
    when_var::expand(input)
}
