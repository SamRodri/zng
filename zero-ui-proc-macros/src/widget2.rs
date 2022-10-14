use std::mem;

use proc_macro2::{TokenStream, TokenTree};
use quote::{quote, ToTokens};
use syn::{parse::Parse, spanned::Spanned, *};

use crate::{
    util::{self, parse_outer_attrs, ErrorRecoverable, Errors},
    widget_util::{self, WgtProperty, WgtWhen},
};

pub fn expand(args: proc_macro::TokenStream, input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    // the widget mod declaration.
    let mod_ = parse_macro_input!(input as ItemMod);

    if mod_.content.is_none() {
        let mut r = syn::Error::new(mod_.semi.span(), "only modules with inline content are supported")
            .to_compile_error()
            .to_token_stream();

        mod_.to_tokens(&mut r);

        return r.into();
    }

    let (mod_braces, items) = mod_.content.unwrap();

    // accumulate the most errors as possible before returning.
    let mut errors = Errors::default();

    let crate_core = util::crate_core();

    let vis = mod_.vis;
    let ident = mod_.ident;
    let mod_token = mod_.mod_token;
    let attrs = mod_.attrs;

    // a `$crate` path to the widget module.
    let mod_path = match syn::parse::<ArgPath>(args) {
        Ok(a) => a.path,
        Err(e) => {
            errors.push_syn(e);
            quote! { $crate::missing_widget_path}
        }
    };

    let WidgetItems {
        uses,
        inherits,
        properties,
        intrinsic_fn,
        build_fn,
        others,
    } = WidgetItems::new(items, &mut errors);

    let mut intrinsic = quote!();

    for Inherit { attrs, path } in &inherits {
        intrinsic.extend(quote_spanned! {path.span()=>
            #(#attrs)*
            #path::__intrinsic__(__wgt__);
        });
    }

    if let Some(int) = &intrinsic_fn {
        intrinsic.extend(quote_spanned! {int.span()=>
            self::intrinsic(__wgt__);
        })
    }

    for prop in properties.iter().flat_map(|i| i.properties.iter()) {
        if prop.has_default() {
            let args = prop.args_new(quote!(#crate_core::property));
            intrinsic.extend(quote! {
                __wgt__.insert_property(#crate_core::property::Importance::WIDGET, #args);
            });
        } else if prop.is_unset() {
            let id = prop.property_id();
            intrinsic.extend(quote! {
                __wgt__.insert_unset(#crate_core::property::Importance::WIDGET, #id);
            });
        } else {
            errors.push(format!("missing property `{}` value(s)", prop.ident()), prop.path.span());
        }
    }

    let build = if let Some(build) = &build_fn {
        let out = &build.sig.output;
        let ident = &build.sig.ident;
        quote_spanned! {build.span()=>
            #[doc(hidden)]
            pub fn __build__(__wgt__: #crate_core::property::WidgetBuilder) #out {
                self::#ident(__wgt__)
            }
        }
    } else if let Some(inh) = inherits.last() {
        let path = &inh.path;
        quote! {
            #[doc(hidden)]
            pub use #path::__build__;
        }
    } else {
        errors.push(
            "missing `build(WidgetBuilder) -> T` function, must be provided or inherited",
            ident.span(),
        );
        quote! {
            #[doc(hidden)]
            pub fn __build__(_: #crate_core::property::WidgetBuilder) -> #crate_core::NilUiNode {
                #crate_core::NilUiNode
            }
        }
    };

    let mut inherit_export = quote!();

    for Inherit { attrs, path } in inherits {
        inherit_export.extend(quote_spanned! {path.span()=>
            #(#attrs)*
            pub use #path::*;
        });
    }

    let macro_ident = ident_spanned!(mod_path.span()=> "__wgt_{}__", mod_path_slug(mod_path.to_string()));

    let mod_items = quote! {
        // custom items
        #(#others)*

        // use items (after custom items in case of custom macro_rules re-export)
        #(#uses)*

        #inherit_export

        #intrinsic_fn

        #[doc(hidden)]
        pub fn __intrinsic__(__wgt__: &mut #crate_core::property::WidgetBuilder) {
            #intrinsic
        }

        #build_fn
        #build

        #[doc(hidden)]
        pub mod __core__ {
            pub use #crate_core::{widget_new2, property};
        }
    };

    let mut mod_block = quote!();
    mod_braces.surround(&mut mod_block, |t| t.extend(mod_items));

    let r = quote! {
        #(#attrs)*
        #vis #mod_token #ident #mod_block

        #[doc(hidden)]
        #[macro_export]
        macro_rules! #macro_ident {
            ($($tt:tt)*) => {
                #mod_path::__core__::widget_new2! {
                    widget { #mod_path }
                    instance {
                        $($tt)*
                    }
                }
            };
        }
        #[doc(hidden)]
        #vis use #macro_ident as #ident;

        #errors
    };
    r.into()
}

struct ArgPath {
    path: TokenStream,
}
impl Parse for ArgPath {
    fn parse(input: parse::ParseStream) -> syn::Result<Self> {
        let fork = input.fork();
        match (fork.parse::<Token![$]>(), fork.parse::<syn::Path>()) {
            (Ok(_), Ok(p)) => {
                if fork.is_empty() {
                    if p.segments[0].ident == "crate" {
                        Ok(ArgPath {
                            path: input.parse().unwrap(),
                        })
                    } else {
                        Err(syn::Error::new(p.segments[0].ident.span(), "expected `crate`"))
                    }
                } else {
                    Err(syn::Error::new(fork.span(), "unexpected token"))
                }
            }
            (Ok(_), Err(e)) => {
                if !util::span_is_call_site(e.span()) {
                    Err(e)
                } else {
                    Err(syn::Error::new(util::last_span(input.parse().unwrap()), e.to_string()))
                }
            }
            _ => Err(syn::Error::new(
                input.span(),
                "expected a macro_rules `$crate` path to this widget mod",
            )),
        }
    }
}

struct WidgetItems {
    uses: Vec<ItemUse>,
    inherits: Vec<Inherit>,
    properties: Vec<Properties>,
    intrinsic_fn: Option<ItemFn>,
    build_fn: Option<ItemFn>,
    others: Vec<Item>,
}
impl WidgetItems {
    fn new(items: Vec<Item>, errors: &mut Errors) -> Self {
        let mut uses = vec![];
        let mut inherits = vec![];
        let mut properties = vec![];
        let mut intrinsic_fn = None;
        let mut build_fn = None;
        let mut others = vec![];

        for item in items {
            match item {
                Item::Use(use_) => {
                    uses.push(use_);
                }
                // match properties!
                Item::Macro(ItemMacro { mac, ident: None, .. }) if mac.path.get_ident().map(|i| i == "properties").unwrap_or(false) => {
                    match syn::parse2::<Properties>(mac.tokens) {
                        Ok(mut p) => {
                            errors.extend(mem::take(&mut p.errors));
                            properties.push(p)
                        }
                        Err(e) => errors.push_syn(e),
                    }
                }
                // match inherit!
                Item::Macro(ItemMacro {
                    mac, attrs, ident: None, ..
                }) if mac.path.get_ident().map(|i| i == "inherit").unwrap_or(false) => match parse2::<Inherit>(mac.tokens) {
                    Ok(mut ps) => {
                        ps.attrs.extend(attrs);
                        inherits.push(ps)
                    }
                    Err(e) => errors.push_syn(e),
                },

                // match fn intrinsic(..)
                Item::Fn(fn_) if fn_.sig.ident == "intrinsic" => {
                    intrinsic_fn = Some(fn_);
                }
                // match fn build(..)
                Item::Fn(fn_) if fn_.sig.ident == "build" => {
                    build_fn = Some(fn_);
                }
                // other user items.
                item => others.push(item),
            }
        }

        WidgetItems {
            uses,
            inherits,
            properties,
            intrinsic_fn,
            build_fn,
            others,
        }
    }
}

struct Inherit {
    attrs: Vec<Attribute>,
    path: Path,
}
impl Parse for Inherit {
    fn parse(input: parse::ParseStream) -> syn::Result<Self> {
        Ok(Inherit {
            attrs: vec![],
            path: input.parse()?,
        })
    }
}

struct Properties {
    errors: Errors,
    properties: Vec<WgtProperty>,
    whens: Vec<WgtWhen>,
}
impl Parse for Properties {
    fn parse(input: parse::ParseStream) -> Result<Self> {
        let mut errors = Errors::default();
        let mut properties = vec![];
        let mut whens = vec![];

        while !input.is_empty() {
            let attrs = parse_outer_attrs(input, &mut errors);

            if input.peek(widget_util::keyword::when) {
                if let Some(mut when) = WgtWhen::parse(input, &mut errors) {
                    when.attrs = attrs;
                    whens.push(when);
                }
            } else if input.peek(Ident) || input.peek(Token![crate]) || input.peek(Token![super]) || input.peek(Token![self]) {
                // peek ident or path (including keywords because of super:: and self::). {
                match input.parse::<WgtProperty>() {
                    Ok(mut p) => {
                        p.attrs = attrs;
                        if !input.is_empty() && p.semi.is_none() {
                            errors.push("expected `;`", input.span());
                            while !(input.is_empty()
                                || input.peek(Ident)
                                || input.peek(Token![crate])
                                || input.peek(Token![super])
                                || input.peek(Token![self])
                                || input.peek(Token![#]) && input.peek(token::Bracket))
                            {
                                // skip to next value item.
                                let _ = input.parse::<TokenTree>();
                            }
                        }
                        properties.push(p);
                    }
                    Err(e) => {
                        let (recoverable, e) = e.recoverable();
                        if recoverable {
                            errors.push_syn(e);
                        } else {
                            return Err(e);
                        }
                    }
                }
            } else {
                errors.push("expected `when`, `child`, `remove` or a property declaration", input.span());

                // suppress the "unexpected token" error from syn parse.
                let _ = input.parse::<TokenStream>();
            }
        }

        Ok(Properties {
            errors,
            properties,
            whens,
        })
    }
}

fn mod_path_slug(path: String) -> String {
    path.replace("crate", "").replace(':', "").replace('$', "").trim().replace(' ', "_")
}

/*
    NEW
*/

pub fn expand_new(args: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let NewArgs { widget, instance } = parse_macro_input!(args as NewArgs);

    let mut errors = Errors::default();

    let call_site = instance.span();
    let instance = match syn::parse2::<Properties>(instance) {
        Ok(p) => p,
        Err(e) => {
            errors.push_syn(e);
            Properties {
                errors: Errors::default(),
                properties: vec![],
                whens: vec![],
            }
        }
    };
    errors.extend(instance.errors);

    let widget = util::set_stream_span(widget, call_site);

    let mut instance_stmts = quote!();
    for p in &instance.properties {
        if p.is_unset() {
            let id = p.property_id();
            instance_stmts.extend(quote! {
                __wgt__.insert_unset(#widget::__core__::property::Importance::INSTANCE, #id);
            });
        } else {
            let args = p.args_new(quote!(#widget::__core__::property));
            instance_stmts.extend(quote_spanned! {call_site=>
                __wgt__.insert_property(#widget::__core__::property::Importance::INSTANCE, #args);
            });
        }
    }

    let r = quote_spanned! {call_site=>
        {
            #[allow(unused_imports)]
            use #widget::*;

            let mut __wgt__ = __core__::property::WidgetBuilder::new();
            #widget::__intrinsic__(&mut __wgt__);

            #instance_stmts

            #widget::__build__(__wgt__)
        }
    };
    r.into()
}

struct NewArgs {
    widget: TokenStream,
    instance: TokenStream,
}
impl Parse for NewArgs {
    fn parse(input: parse::ParseStream) -> Result<Self> {
        Ok(NewArgs {
            widget: non_user_braced!(input, "widget").parse().unwrap(),
            instance: non_user_braced!(input, "instance").parse().unwrap(),
        })
    }
}
