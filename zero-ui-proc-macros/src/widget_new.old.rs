use crate::util;
use crate::widget::{self, PropertyAssign, WhenBlock};
use proc_macro2::{Span, TokenStream};
use spanned::Spanned;
use std::collections::{hash_map, HashMap, HashSet};
use std::rc::Rc;
use syn::punctuated::Punctuated;
use syn::{parse::*, *};
use visit_mut::VisitMut;
use widget::{PropertyValue, WhenConditionVisitor, WidgetItemTarget};

pub mod keyword {
    syn::custom_keyword!(m);
    syn::custom_keyword!(c);
    syn::custom_keyword!(s);
    syn::custom_keyword!(w);
    syn::custom_keyword!(n);
    syn::custom_keyword!(i);

    syn::custom_keyword!(r);
    syn::custom_keyword!(l);
    syn::custom_keyword!(d);
}

/// `widget_new!` implementation
#[allow(clippy::cognitive_complexity)]
pub fn expand_widget_new(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as WidgetNewInput);
    let widget_name = input.ident;
    let crate_ = util::zero_ui_crate_ident();

    // map properties for collection into a HashMap.
    let map_props = |bdb: BuiltDefaultBlock, target| bdb.properties.into_iter().map(move |p| (p.ident, (target, p.kind)));

    // map of metadata of properties defined by the widget.
    let mut known_props: HashMap<_, _> = map_props(input.default_child, WidgetItemTarget::Child)
        .chain(map_props(input.default_self, WidgetItemTarget::Self_))
        .collect();

    let mut props_in_widget: HashSet<_> = known_props.keys().cloned().collect();

    // declarations of property arguments in the user written order.
    let mut let_args = Vec::with_capacity(input.input.sets.len());
    let mut let_default_args = vec![];
    // metadata about let_args and let_default_args.
    let mut setted_props = Vec::with_capacity(let_args.capacity());

    // collects property assigns from the user.
    for set in input.input.sets {
        let name = ident! {"{}_args", set.ident};
        let prop = set.ident;
        let target;
        let prop_prefix;
        let required;
        let in_widget;
        if let Some((tgt, kind)) = known_props.remove(&prop) {
            target = tgt;
            prop_prefix = quote! { #widget_name::ps:: };
            required = kind == BuiltPropertyKind::Required;
            in_widget = true;
        } else {
            target = WidgetItemTarget::Self_;
            prop_prefix = quote! {};
            required = false;
            in_widget = false;
        }
        match set.value {
            PropertyValue::Fields(fields) => let_args.push(quote! {
                let #name = #prop_prefix#prop::NamedArgs{
                    _phantom: std::marker::PhantomData,
                    #fields
                };
            }),
            PropertyValue::Args(args) => let_args.push(quote! {
                let #name = #prop_prefix#prop::args(#args);
            }),
            PropertyValue::Unset => {
                if required {
                    abort! {prop.span(), "cannot unset required property `{}`", prop}
                }
            }
        }

        setted_props.push((prop, target, in_widget));
    }
    // collects property assigns from default widget properties.
    for (prop, (target, kind)) in known_props {
        let name = ident!("{}_args", prop);
        match kind {
            BuiltPropertyKind::Required => {
                abort_call_site!("missing required property `{}`", prop);
            }
            BuiltPropertyKind::Default => {
                let_default_args.push(quote! {
                    let #name = #widget_name::df::#prop();
                });
                setted_props.push((prop, target, true));
            }
            BuiltPropertyKind::Local => {}
        }
    }

    let mut let_when_vars = Vec::with_capacity(input.whens.whens.len());
    let mut prop_wns_map = HashMap::new();

    // collect when var declarations
    for (i, when) in input.whens.whens.into_iter().enumerate() {
        let wn = ident!("w{}", i);
        let when_args: Vec<_> = when.args.iter().map(|p| ident!("{}_args", p)).collect();
        let_when_vars.push(quote! {let #wn = #widget_name::we::#wn(#(&#when_args),*);});

        for (prop, prop_args) in when.args.into_iter().zip(when_args) {
            props_in_widget.insert(prop.clone());

            // if property has no initial value.
            if !setted_props.iter().any(|p| p.0 == prop) {
                let_default_args.push(quote! {
                    let #prop_args = #widget_name::ps::#prop::args(#crate_::core::var::var(false));
                });
                setted_props.push((prop.clone(), WidgetItemTarget::Self_, true));
            }
        }

        let wn = Rc::new(wn);

        for prop in when.sets.into_iter() {
            let value_init = quote!(#widget_name::df::#wn::#prop());
            let full_name = quote!(#widget_name::ps::#prop);
            match prop_wns_map.entry(prop) {
                hash_map::Entry::Vacant(entry) => {
                    entry.insert((full_name, vec![(wn.clone(), value_init)]));
                }
                hash_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().1.push((wn.clone(), value_init));
                }
            }
        }
    }

    for (i, mut when) in input.input.whens.into_iter().enumerate() {
        let condition_span = when.condition.span();

        let mut visitor = WhenConditionVisitor::default();
        visitor.visit_expr_mut(&mut when.condition);

        // dedup property members.
        let property_members: HashMap<_, _> = visitor.properties.iter().map(|p| (&p.new_name, p)).collect();
        if property_members.is_empty() {
            abort!(condition_span.span(), "`when` condition must reference properties")
        }

        // dedup properties.
        let property_args: HashMap<_, _> = property_members
            .values()
            .map(|p| (&p.property, ident!("{}_args", p.property)))
            .collect();

        let mut asserts = vec![];
        for (&p, _) in property_args.iter() {
            let ns = if props_in_widget.contains(p) {
                let mut widget_name = widget_name.clone();
                widget_name.set_span(p.span());
                quote_spanned!(p.span()=> #widget_name::ps::#p)
            } else {
                quote!(#p)
            };
            asserts.push(quote_spanned!(p.span()=> use #ns::is_allowed_in_when;));
        }
        let asserts = quote!(#({#[allow(unused)]#asserts})*);

        let local_names = property_members.keys();
        let members = property_members.values().map(|p| {
            let property = &p.property;
            let property = if props_in_widget.contains(property) {
                let mut widget_name = widget_name.clone();
                widget_name.set_span(property.span());
                quote_spanned!(property.span()=> #widget_name::ps::#property)
            } else {
                quote!(#property)
            };

            match &p.member {
                Member::Named(ident) => quote_spanned!(property.span()=> #property::ArgsNamed::#ident),
                Member::Unnamed(idx) => {
                    let argi = ident_spanned!(property.span()=> "arg{}", idx.index);
                    quote_spanned!(property.span()=> #property::ArgsNumbered::#argi)
                }
            }
        });
        let arg_names = property_members.values().map(|p| &property_args[&p.property]);

        let mut init_locals = vec![];
        for ((local_name, member), args) in local_names.zip(members).zip(arg_names) {
            let mut crate_ = crate_.clone();
            crate_.set_span(local_name.span());
            init_locals.push(quote_spanned! {local_name.span()=>
                let #local_name = #crate_::core::var::IntoVar::into_var(std::clone::Clone::clone(#member(&#args)));
            })
        }
        let init_locals = quote!(#(#init_locals)*);

        let condition = when.condition;
        let return_ = if property_members.len() == 1 {
            let new_name = property_members.keys().next().unwrap();
            if !visitor.found_mult_exprs {
                // if is only a reference to a property.
                // ex.: when self.is_pressed {}
                quote_spanned!(new_name.span()=>  #[allow(clippy::let_and_return)]let r = #new_name;r)
            } else {
                quote_spanned!(condition_span=> #crate_::core::var::Var::into_map(#new_name, |#new_name|{
                    #condition
                }))
            }
        } else {
            let new_names = property_members.keys();
            let args = new_names.clone();
            quote_spanned! {condition_span=>
                merge_var!(#(#new_names, )* |#(#args),*|{
                    #condition
                })
            }
        };

        let local_wn = ident!("local_w{}", i);
        let_when_vars.push(quote! {
            let #local_wn = {
                #asserts
                #init_locals
                #return_
            };
        });

        let local_wn = Rc::new(local_wn);

        for p in when.properties.into_iter() {
            let name = p.ident;

            let full_name = if props_in_widget.contains(&name) {
                let mut widget_name = widget_name.clone();
                widget_name.set_span(name.span());
                quote_spanned!(name.span()=> #widget_name::ps::#name)
            } else {
                quote!(#name)
            };

            let init_value = match p.value {
                PropertyValue::Fields(fields) => quote! {
                    #full_name::NamedArgs{
                        _phantom: std::marker::PhantomData,
                        #fields
                    }
                },
                PropertyValue::Args(args) => quote! {
                    #full_name::args(#args)
                },
                PropertyValue::Unset => {
                    abort! {name.span(), "cannot unset in when"}
                }
            };

            match prop_wns_map.entry(name) {
                hash_map::Entry::Vacant(entry) => {
                    entry.insert((full_name, vec![(local_wn.clone(), init_value)]));
                }
                hash_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().1.push((local_wn.clone(), init_value));
                }
            }
        }
    }

    let mut prop_when_indexes = Vec::with_capacity(prop_wns_map.len());
    let mut let_switches = Vec::with_capacity(prop_when_indexes.len());

    // collect switches
    for (prop, (full_name, wns)) in prop_wns_map {
        let prop_idx = ident!("{}_index", prop);

        if wns.len() == 1 {
            let wn = &wns[0].0;
            prop_when_indexes.push(quote! {
                let #prop_idx = #crate_::core::var::Var::map(&#wn, |&#wn| if #wn { 1usize } else { 0usize });
            });
        } else {
            let wns_input = wns.iter().map(|(wn, _)| {
                if Rc::strong_count(&wn) == 1 {
                    quote!(#wn)
                } else {
                    quote!(#wn.clone())
                }
            });

            let wns_i = (1..=wns.len()).rev();
            let wns = wns.iter().map(|(w, _)| w);
            let wns_rev = wns.clone().rev();

            prop_when_indexes.push(quote! {
                let #prop_idx = #crate_::core::var::merge_var!(#(#wns_input),* , |#(&#wns),*|{
                    #(if #wns_rev { #wns_i })else*
                    else { 0usize }
                });
            });
        }

        let prop_args = ident!("{}_args", prop);
        let value_init = wns.iter().map(|(_, vi)| vi);
        let wns = wns.iter().map(|(w, _)| w);
        let let_wns = wns.clone();
        let_switches.push(quote! {
            let #prop_args = {
                #(let #let_wns = #value_init;)*
                #full_name::switch_args!(#full_name, #prop_idx, #prop_args, #(#wns),*)
            };
        });
    }

    // generate property set calls.

    #[derive(Default)]
    struct SetProps {
        context: Vec<TokenStream>,
        event: Vec<TokenStream>,
        outer: Vec<TokenStream>,
        size: Vec<TokenStream>,
        inner: Vec<TokenStream>,
    }

    let mut set_child = SetProps::default();
    let mut set_self = SetProps::default();

    for (prop, target, in_widget) in setted_props {
        let set;
        let expected_new_args;
        match target {
            WidgetItemTarget::Child => {
                set = &mut set_child;
                expected_new_args = &input.new_child;
            }
            WidgetItemTarget::Self_ => {
                set = &mut set_self;
                expected_new_args = &input.new;
            }
        }

        if !expected_new_args.iter().any(|a| a == &prop) {
            let name = ident! {"{}_args", prop};
            let props = if in_widget { quote!(#widget_name::ps::) } else { quote!() };
            set.context
                .push(quote!(#props#prop::set_args!(context, #props#prop::set_args, node, #name);));
            set.event
                .push(quote!(#props#prop::set_args!(event, #props#prop::set_args, node, #name);));
            set.outer
                .push(quote!(#props#prop::set_args!(outer, #props#prop::set_args, node, #name);));
            set.size
                .push(quote!(#props#prop::set_args!(size, #props#prop::set_args, node, #name);));
            set.inner
                .push(quote!(#props#prop::set_args!(inner, #props#prop::set_args, node, #name);));
            //NOTE: Sends ::set_args instead of doing it in the set_args! macro because of a limitation in macro_rules
        }
    }

    let SetProps {
        context: set_child_props_ctx,
        event: set_child_props_event,
        outer: set_child_props_outer,
        size: set_child_props_size,
        inner: set_child_props_inner,
    } = set_child;

    let new_child_args = input.new_child.into_iter().map(|n| ident!("{}_args", n));

    let SetProps {
        context: set_self_props_ctx,
        event: set_self_props_event,
        outer: set_self_props_outer,
        size: set_self_props_size,
        inner: set_self_props_inner,
    } = set_self;

    let new_args = input.new.into_iter().map(|n| ident!("{}_args", n));

    // widget child expression `=> {this}`
    let child = input.input.child_expr;

    // declarations of self property arguments.

    let r = quote! {{
        #(#let_default_args)*
        #(#let_args)*

        #(#let_when_vars)*
        #(#prop_when_indexes)*
        
        #(#let_switches)*

        let node = #child;

        // apply child properties
        #(#set_child_props_inner)*
        #(#set_child_props_size)*
        #(#set_child_props_outer)*
        #(#set_child_props_event)*
        #(#set_child_props_ctx)*

        let node = #widget_name::new_child(node, #(#new_child_args),*);

        // apply self properties
        #(#set_self_props_inner)*
        #(#set_self_props_size)*
        #(#set_self_props_outer)*
        #(#set_self_props_event)*
        #(#set_self_props_ctx)*

        #widget_name::new(node, #(#new_args),*)
    }};

    r.into()
}

pub struct WidgetNewInput {
    pub ident: Ident,
    pub default_child: BuiltDefaultBlock,
    pub default_self: BuiltDefaultBlock,
    pub whens: BuiltWhens,
    new_child: Punctuated<Ident, Token![,]>,
    new: Punctuated<Ident, Token![,]>,
    input: NewWidgetInput,
}

impl Parse for WidgetNewInput {
    fn parse(input: ParseStream) -> Result<Self> {
        input.parse::<keyword::m>().unwrap_or_else(|e| non_user_error!(e));
        let ident = input.parse().unwrap_or_else(|e| non_user_error!(e));

        input.parse::<keyword::c>().unwrap_or_else(|e| non_user_error!(e));
        let default_child = input.parse().unwrap_or_else(|e| non_user_error!(e));

        input.parse::<keyword::s>().unwrap_or_else(|e| non_user_error!(e));
        let default_self = input.parse().unwrap_or_else(|e| non_user_error!(e));

        input.parse::<keyword::w>().unwrap_or_else(|e| non_user_error!(e));
        let whens = input.parse().unwrap_or_else(|e| non_user_error!(e));

        input.parse::<keyword::n>().unwrap_or_else(|e| non_user_error!(e));
        let inner = util::non_user_parenthesized(input);
        let new_child = Punctuated::parse_terminated(&inner).unwrap_or_else(|e| non_user_error!(e));
        let inner = util::non_user_parenthesized(input);
        let new = Punctuated::parse_terminated(&inner).unwrap_or_else(|e| non_user_error!(e));

        input.parse::<keyword::i>().unwrap_or_else(|e| non_user_error!(e));
        let input = input.parse()?;

        Ok(WidgetNewInput {
            ident,
            default_child,
            default_self,
            whens,
            new_child,
            new,
            input,
        })
    }
}

pub struct BuiltDefaultBlock {
    pub properties: Punctuated<BuiltProperty, Token![,]>,
}

impl Parse for BuiltDefaultBlock {
    fn parse(input: ParseStream) -> Result<Self> {
        let inner;
        braced!(inner in input);
        let properties = Punctuated::parse_terminated(&inner)?;
        Ok(BuiltDefaultBlock { properties })
    }
}

pub struct BuiltWhens {
    whens: Punctuated<BuiltWhen, Token![,]>,
}

impl Parse for BuiltWhens {
    fn parse(input: ParseStream) -> Result<Self> {
        let inner;
        braced!(inner in input);
        let whens = Punctuated::parse_terminated(&inner)?;
        Ok(BuiltWhens { whens })
    }
}

struct BuiltWhen {
    args: Punctuated<Ident, Token![,]>,
    sets: Punctuated<Ident, Token![,]>,
}

impl Parse for BuiltWhen {
    fn parse(input: ParseStream) -> Result<Self> {
        let inner;
        parenthesized!(inner in input);
        let args = Punctuated::parse_terminated(&inner)?;

        let inner;
        braced!(inner in input);
        let sets = Punctuated::parse_terminated(&inner)?;

        Ok(BuiltWhen { args, sets })
    }
}

pub struct BuiltProperty {
    pub kind: BuiltPropertyKind,
    pub ident: Ident,
}

impl Parse for BuiltProperty {
    fn parse(input: ParseStream) -> Result<Self> {
        let kind = input.parse()?;
        let ident = input.parse()?;
        Ok(BuiltProperty { kind, ident })
    }
}

#[derive(PartialEq, Eq)]
pub enum BuiltPropertyKind {
    /// Required property.
    Required,
    /// Property is provided by the widget.
    Local,
    /// Property and default is provided by the widget.
    Default,
}

impl Parse for BuiltPropertyKind {
    fn parse(input: ParseStream) -> Result<Self> {
        if input.peek(keyword::d) {
            input.parse::<keyword::d>()?;
            Ok(BuiltPropertyKind::Default)
        } else if input.peek(keyword::l) {
            input.parse::<keyword::l>()?;
            Ok(BuiltPropertyKind::Local)
        } else if input.peek(keyword::r) {
            input.parse::<keyword::r>()?;
            Ok(BuiltPropertyKind::Required)
        } else {
            Err(Error::new(input.span(), "expected one of: r, d, l"))
        }
    }
}

struct NewWidgetInput {
    sets: Vec<PropertyAssign>,
    whens: Vec<WhenBlock>,
    child_expr: Expr,
}

impl Parse for NewWidgetInput {
    fn parse(input: ParseStream) -> Result<Self> {
        let inner = util::non_user_braced(input);

        let mut sets = vec![];
        let mut whens = vec![];

        while !inner.is_empty() {
            let lookahead = inner.lookahead1();

            // expect `when` at start or after `property:`
            if lookahead.peek(widget::keyword::when) {
                whens.push(inner.parse()?);
            }
            // expect `property:` only before `when` blocks.
            else if whens.is_empty() && lookahead.peek(Ident) {
                sets.push(inner.parse()?);
            }
            // expect `=>` to be the last item.
            else if lookahead.peek(Token![=>]) {
                inner.parse::<Token![=>]>()?;
                let child_expr = inner.parse()?;

                return Ok(NewWidgetInput { sets, whens, child_expr });
            } else {
                return Err(lookahead.error());
            }
        }

        // if user input is empty, use a lookahead to make an error message.
        let error = input.lookahead1();
        error.peek(Ident);
        error.peek(widget::keyword::when);
        error.peek(Token![=>]);
        Err(error.error())
    }
}
