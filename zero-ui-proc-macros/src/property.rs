use quote::ToTokens;
use syn::parse_macro_input;

pub fn expand(args: proc_macro::TokenStream, input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let args = parse_macro_input!(args as input::MacroArgs);
    let fn_ = parse_macro_input!(input as input::PropertyFn);

    let output = analysis::generate(args, fn_);

    let tokens = output.to_token_stream();

    //println!("\n\n========================\n\n{}\n\n=================", tokens);

    tokens.into()
}

pub use analysis::Prefix;
pub use input::keyword;
pub use input::Priority;

mod input {
    use syn::{parse::*, *};

    pub mod keyword {
        syn::custom_keyword!(context);
        syn::custom_keyword!(event);
        syn::custom_keyword!(outer);
        syn::custom_keyword!(size);
        syn::custom_keyword!(inner);
        syn::custom_keyword!(capture_only);
        syn::custom_keyword!(allowed_in_when);
    }

    pub struct MacroArgs {
        pub priority: Priority,
        //", allowed_in_when: true"
        pub allowed_in_when: Option<(Token![,], keyword::allowed_in_when, Token![:], LitBool)>,
        // trailing comma
        pub comma_token: Option<Token![,]>,
    }
    impl Parse for MacroArgs {
        fn parse(input: ParseStream) -> Result<Self> {
            Ok(MacroArgs {
                priority: input.parse()?,
                allowed_in_when: {
                    if input.peek(Token![,]) {
                        Some((input.parse()?, input.parse()?, input.parse()?, input.parse()?))
                    } else {
                        None
                    }
                },
                comma_token: input.parse()?,
            })
        }
    }

    #[derive(Clone, Copy)]
    pub enum Priority {
        Context(keyword::context),
        Event(keyword::event),
        Outer(keyword::outer),
        Size(keyword::size),
        Inner(keyword::inner),
        CaptureOnly(keyword::capture_only),
    }
    impl Priority {
        pub fn is_event(self) -> bool {
            matches!(self, Priority::Event(_))
        }
        pub fn is_capture_only(self) -> bool {
            matches!(self, Priority::CaptureOnly(_))
        }
    }
    impl Parse for Priority {
        fn parse(input: ParseStream) -> Result<Self> {
            let lookahead = input.lookahead1();

            if lookahead.peek(keyword::context) {
                input.parse().map(Priority::Context)
            } else if lookahead.peek(keyword::event) {
                input.parse().map(Priority::Event)
            } else if lookahead.peek(keyword::outer) {
                input.parse().map(Priority::Outer)
            } else if lookahead.peek(keyword::size) {
                input.parse().map(Priority::Size)
            } else if lookahead.peek(keyword::inner) {
                input.parse().map(Priority::Inner)
            } else if lookahead.peek(keyword::capture_only) {
                input.parse().map(Priority::CaptureOnly)
            } else {
                Err(lookahead.error())
            }
        }
    }

    pub struct PropertyFn {
        pub attrs: Vec<Attribute>,
        pub fn_: ItemFn,
    }
    impl Parse for PropertyFn {
        fn parse(input: ParseStream) -> Result<Self> {
            Ok(PropertyFn {
                attrs: Attribute::parse_outer(input)?,
                fn_: input.parse()?,
            })
        }
    }
}

mod analysis {
    use std::collections::HashSet;

    use proc_macro2::Ident;
    use syn::{parse_quote, spanned::Spanned, visit::Visit, visit_mut::VisitMut, TypeParam};

    use crate::util::{self, crate_core, Attributes, Errors};

    use super::{input, output};

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum Prefix {
        State,
        Event,
        None,
    }
    impl Prefix {
        pub fn new(fn_ident: &Ident) -> Self {
            let ident_str = fn_ident.to_string();

            if ident_str.starts_with("is_") {
                Prefix::State
            } else if ident_str.starts_with("on_") {
                Prefix::Event
            } else {
                Prefix::None
            }
        }

        pub fn is_state(fn_ident: &Ident) -> bool {
            let ident_str = fn_ident.to_string();
            ident_str.starts_with("is_")
        }
    }

    pub fn generate(args: input::MacroArgs, fn_: input::PropertyFn) -> output::Output {
        let input::PropertyFn { attrs, mut fn_ } = fn_;

        let mut errors = Errors::default();

        let prefix = Prefix::new(&fn_.sig.ident);
        let attrs = Attributes::new(attrs);

        // validate prefix
        let args_len = fn_.sig.inputs.len();
        let args_span = fn_.sig.inputs.span();
        if args.priority.is_capture_only() {
            match prefix {
                Prefix::State => {
                    if args_len != 1 {
                        errors.push("is_* capture_only properties must have 1 parameter, `IsStateVar`", args_span);
                    }
                }
                Prefix::Event => {
                    if args_len != 1 {
                        errors.push("on_* capture_only properties must have 1 parameter, `FnMut`", args_span);
                    }
                }
                Prefix::None => {
                    if args_len == 0 {
                        errors.push("capture_only properties must have at least 1 parameter", args_span);
                    }
                }
            }
        } else {
            match prefix {
                Prefix::State => {
                    if args_len != 2 {
                        errors.push(
                            "is_* properties functions must have 2 parameters, `UiNode` and `IsStateVar`",
                            args_span,
                        );
                    }
                }
                Prefix::Event => {
                    if args_len != 2 {
                        errors.push("on_* properties must have 2 parameters, `UiNode` and `FnMut`", args_span);
                    }
                    if !args.priority.is_event() {
                        errors.push(
                            "only `event` or `capture_only` priority properties can have the prefix `on_`",
                            fn_.sig.ident.span(),
                        )
                    }
                }
                Prefix::None => {
                    if args_len < 2 {
                        errors.push(
                            "properties must have at least 2 parameters, `UiNode` and one or more values",
                            args_span,
                        );
                    }
                }
            }
        }
        if args.priority.is_event() && prefix != Prefix::Event {
            errors.push("property marked `event` does not have prefix `on_`", fn_.sig.ident.span());
        }

        // validate return type.
        if args.priority.is_capture_only() {
            let valid = match &fn_.sig.output {
                syn::ReturnType::Default => false,
                syn::ReturnType::Type(_, t) => matches!(&**t, syn::Type::Never(_)),
            };

            if !valid {
                errors.push("capture_only properties must have return type ` -> !`", fn_.sig.output.span());
            }
        } else {
            // properties not capture_only:
            // rust will validate because we call fn_ in ArgsImpl.set(..) -> impl UiNode.
        }

        // patch signature to continue validation:
        if args.priority.is_capture_only() {
            if fn_.sig.inputs.is_empty() {
                fn_.sig.inputs.push(parse_quote!(_missing_param: ()));
            }
        } else {
            if fn_.sig.inputs.is_empty() {
                let crate_core = crate_core();
                fn_.sig.inputs.push(parse_quote!( _missing_child: impl #crate_core::UiNode ));
            }
            if fn_.sig.inputs.len() == 1 {
                fn_.sig.inputs.push(parse_quote!(_missing_param: ()));
            }
        }

        // collect normal generics.
        let mut generic_types = vec![]; // Vec<TypeParam>
        for gen in fn_.sig.generics.type_params() {
            generic_types.push(gen.clone());
        }
        // move where clauses to normal generics.
        if let Some(where_) = &fn_.sig.generics.where_clause {
            for pre in where_.predicates.iter() {
                if let syn::WherePredicate::Type(pt) = pre {
                    if let syn::Type::Path(ti) = &pt.bounded_ty {
                        if ti.qself.is_none() {
                            if let Some(t_ident) = ti.path.get_ident() {
                                // T : bounds
                                if let Some(gen) = generic_types.iter_mut().find(|t| &t.ident == t_ident) {
                                    // found T
                                    gen.bounds.extend(pt.bounds.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        if !args.priority.is_capture_only() {
            // remove generics used only in the first `child` input.
            let used = {
                let mut cleanup = CleanupGenerics::new(&generic_types);
                for input in fn_.sig.inputs.iter().skip(1) {
                    cleanup.visit_fn_arg(input);
                }

                cleanup.used
            };

            generic_types.retain(|t| used.contains(&t.ident));
        }

        // validate input patterns and collect arg_idents and arg_types and impl_types (for generic_types):
        let mut arg_idents = vec![]; // Vec<Ident>
        let mut arg_types = vec![]; // Vec<Type>

        let mut embedded_impl_types = PatchEmbededImplTrait::default();
        let mut impl_types = vec![]; // Vec<TypeParam>

        let inputs = if args.priority.is_capture_only() {
            fn_.sig.inputs.iter().skip(0)
        } else {
            fn_.sig.inputs.iter().skip(1)
        };

        let mut invalid_n = 0;
        let mut invalid_idents = move || {
            let next = ident!("_invalid{}", invalid_n);
            invalid_n += 1;
            next
        };
        for input in inputs {
            match input {
                syn::FnArg::Typed(t) => {
                    // any pat : pat
                    match &*t.pat {
                        syn::Pat::Ident(ident_pat) => {
                            if let Some(subpat) = &ident_pat.subpat {
                                // ident @ sub_pat : type
                                errors.push(
                                    "only `field: T` pattern can be property arguments, found sub-pattern",
                                    subpat.0.span(),
                                );
                                arg_idents.push(invalid_idents());
                                arg_types.push((&*t.ty).clone());
                            } else if ident_pat.ident == "self" {
                                // self : type
                                errors.push("methods cannot be property functions", ident_pat.ident.span());
                                arg_idents.push(invalid_idents());
                                arg_types.push((&*t.ty).clone());
                            } else {
                                // VALID
                                // ident: type
                                arg_idents.push(ident_pat.ident.clone());
                                arg_types.push((&*t.ty).clone());
                            }
                        }
                        invalid => {
                            // any_pat no type ascription
                            errors.push("only `field: T` pattern can be property arguments", invalid.span());
                            arg_idents.push(invalid_idents());
                            arg_types.push(parse_quote!(()));
                        }
                    }

                    // convert `impl Trait` to normal generics:
                    let last_i = arg_types.len() - 1;
                    if let syn::Type::ImplTrait(impl_) = &arg_types[last_i] {
                        // impl at the *top* level gets a readable name

                        let t_ident = ident_spanned!(impl_.span()=> "T_{}", arg_idents[last_i]);

                        // the bounds can have nested impl Traits.
                        let mut bounds = impl_.bounds.clone();
                        for bound in bounds.iter_mut() {
                            embedded_impl_types.visit_type_param_bound_mut(bound);
                        }

                        impl_types.push(parse_quote! {
                            #t_ident : #bounds
                        });
                        arg_types[last_i] = parse_quote!( #t_ident );
                    } else {
                        embedded_impl_types.visit_type_mut(&mut arg_types[last_i]);
                    }
                }

                syn::FnArg::Receiver(invalid) => {
                    // `self`
                    errors.push("methods cannot be property functions", invalid.span())
                }
            }
        }

        generic_types.extend(embedded_impl_types.types);
        generic_types.extend(impl_types);

        // convert `T:? bounds?` to `type T:? Self::?bounds?;`
        let mut to_assoc = GenericToAssocTypes {
            t_idents: generic_types.iter().map(|t| t.ident.clone()).collect(),
        };
        let mut assoc_types = vec![];
        for gen in &generic_types {
            let ident = &gen.ident;
            let mut bounds = gen.bounds.clone();
            if bounds.is_empty() {
                assoc_types.push(parse_quote! { type #ident; });
            } else {
                for bound in bounds.iter_mut() {
                    to_assoc.visit_type_param_bound_mut(bound);
                }
                assoc_types.push(parse_quote! {
                    type #ident : #bounds;
                });
            }
        }

        // convert arg types to be a return type in the Args trait methods.
        let mut arg_return_types = arg_types.clone();
        for ty in &mut arg_return_types {
            if let syn::Type::Path(tp) = ty {
                if let Some(ident) = tp.path.get_ident() {
                    if to_assoc.t_idents.contains(ident) {
                        // is one of the generic types, change to Self::T
                        *ty = parse_quote!( Self::#ident );
                        continue;
                    }
                }
            }
            to_assoc.visit_type_mut(ty);
        }

        // collect phantom type idents.
        let mut phantom_idents = to_assoc.t_idents;
        for arg_ty in &arg_types {
            if let syn::Type::Path(tp) = arg_ty {
                if let Some(ident) = tp.path.get_ident() {
                    if let Some(i) = phantom_idents.iter().position(|id| id == ident) {
                        phantom_idents.swap_remove(i);
                    }
                }
            }
        }

        // more signature validation.
        if let Some(async_) = &fn_.sig.asyncness {
            errors.push("property functions cannot be `async`", async_.span());
        }
        if let Some(unsafe_) = &fn_.sig.unsafety {
            errors.push("property functions cannot be `unsafe`", unsafe_.span());
        }
        if let Some(abi) = &fn_.sig.abi {
            errors.push("property functions cannot be `extern`", abi.span());
        }
        if let Some(lifetime) = fn_.sig.generics.lifetimes().next() {
            errors.push("property functions cannot declare lifetimes", lifetime.span());
        }
        if let Some(const_) = fn_.sig.generics.const_params().next() {
            errors.push("property functions do not support `const` generics", const_.span());
        }

        if args.priority.is_capture_only() {
            // set capture_only standard error.
            let msg = format!("property `{}` cannot be set because it is capture-only", fn_.sig.ident);
            fn_.block = parse_quote! {
                { panic!(#msg) }
            };
            // allow unused property fields.
            fn_.attrs.push(parse_quote! { #[allow(unused_variables)] });
        }

        let allowed_in_when = match args.allowed_in_when {
            Some(b) => b.3.value,
            None => match prefix {
                Prefix::State | Prefix::None => true,
                Prefix::Event => false,
            },
        };

        let macro_ident = ident!("{}_{}", fn_.sig.ident, util::uuid());

        output::Output {
            errors,
            fn_attrs: output::OutputAttributes {
                docs: attrs.docs,
                inline: attrs.inline,
                cfg: attrs.cfg.clone(),
            },
            types: output::OutputTypes {
                cfg: attrs.cfg.clone(),
                ident: fn_.sig.ident.clone(),
                generics: generic_types,
                allowed_in_when,
                phantom_idents: phantom_idents.clone(),
                arg_idents: arg_idents.clone(),
                priority: args.priority,
                arg_types,
                assoc_types,
                arg_return_types,
            },
            mod_: output::OutputMod {
                cfg: attrs.cfg.clone(),
                vis: fn_.vis.clone(),
                ident: fn_.sig.ident.clone(),
                is_capture_only: args.priority.is_capture_only(),
                macro_ident: macro_ident.clone(),
                args_ident: ident!("{}_Args", fn_.sig.ident),
                args_impl_ident: ident!("{}_ArgsImpl", fn_.sig.ident),
            },
            macro_: output::OutputMacro {
                cfg: attrs.cfg,
                macro_ident,
                export: !matches!(fn_.vis, syn::Visibility::Inherited),
                priority: args.priority,
                allowed_in_when,
                phantom_idents,
                arg_idents,
            },
            fn_,
        }
    }

    #[derive(Default)]
    struct PatchEmbededImplTrait {
        types: Vec<TypeParam>,
    }
    impl VisitMut for PatchEmbededImplTrait {
        fn visit_type_mut(&mut self, i: &mut syn::Type) {
            syn::visit_mut::visit_type_mut(self, i);

            if let syn::Type::ImplTrait(impl_trait) = i {
                let t_ident = ident!("T_impl_{}", self.types.len());
                let bounds = &impl_trait.bounds;
                self.types.push(parse_quote! {
                    #t_ident : #bounds
                });
                *i = parse_quote!(#t_ident);
            }
        }
    }

    struct GenericToAssocTypes {
        t_idents: Vec<Ident>,
    }
    impl VisitMut for GenericToAssocTypes {
        fn visit_type_mut(&mut self, i: &mut syn::Type) {
            if let syn::Type::Path(tp) = i {
                if let Some(ident) = tp.path.get_ident() {
                    if self.t_idents.contains(ident) {
                        // if
                        *i = parse_quote!( Self::#ident );
                        return;
                    }
                }
            }

            // else
            syn::visit_mut::visit_type_mut(self, i);
        }
    }

    struct CleanupGenerics<'g> {
        generics: &'g [TypeParam],
        used: HashSet<Ident>,
    }
    impl<'g> CleanupGenerics<'g> {
        fn new(generics: &'g [TypeParam]) -> Self {
            CleanupGenerics {
                used: HashSet::new(),
                generics,
            }
        }
    }
    impl<'g, 'v> Visit<'v> for CleanupGenerics<'g> {
        fn visit_type(&mut self, i: &'v syn::Type) {
            if let syn::Type::Path(tp) = i {
                if let Some(ident) = tp.path.get_ident() {
                    if let Some(gen) = self.generics.iter().find(|g| &g.ident == ident) {
                        // uses generic.
                        if self.used.insert(ident.clone()) {
                            // because of this it uses the generic bounds too.
                            for bound in gen.bounds.iter() {
                                self.visit_type_param_bound(bound);
                            }
                        }
                    }
                }
            }
            syn::visit::visit_type(self, i);
        }
    }
}

mod output {
    use proc_macro2::{Ident, TokenStream};
    use quote::ToTokens;
    use syn::{Attribute, ItemFn, TraitItemType, Type, TypeParam, Visibility};

    use crate::util::{crate_core, Errors};

    use super::input::Priority;

    pub struct Output {
        pub errors: Errors,
        pub fn_attrs: OutputAttributes,
        pub fn_: ItemFn,
        pub types: OutputTypes,
        pub macro_: OutputMacro,
        pub mod_: OutputMod,
    }
    impl ToTokens for Output {
        fn to_tokens(&self, tokens: &mut TokenStream) {
            self.errors.to_tokens(tokens);
            self.fn_attrs.to_tokens(tokens);
            self.fn_.to_tokens(tokens);
            self.types.to_tokens(tokens);
            self.macro_.to_tokens(tokens);
            self.mod_.to_tokens(tokens);
        }
    }

    pub struct OutputAttributes {
        pub docs: Vec<Attribute>,
        pub inline: Option<Attribute>,
        pub cfg: Option<Attribute>,
    }

    impl ToTokens for OutputAttributes {
        fn to_tokens(&self, tokens: &mut TokenStream) {
            for doc in &self.docs {
                doc.to_tokens(tokens);
            }
            tokens.extend(quote! {
                /// </div>
                /// <h2 id='prop_fn' class='small-section-header'>Function<a href='#prop_fn' class='anchor'></a></h2>
                /// <pre id='ffn' class='rust fn'></pre>
                /// <div class='docblock'>
                ///
                /// Each property is a function that can be called directly. 
                ///
                ///
                /// The property is ***set*** around the first input [`UiNode`](zero_ui::core::UiNode), 
                /// the other inputs are the property arguments. The function output if a new [`UiNode`](zero_ui::core::UiNode) that
                /// includes the property behavior.
            });
            doc_extend!(tokens, "<script>{}</script>", js!("property_full.js"));
            self.inline.to_tokens(tokens);
            self.cfg.to_tokens(tokens);
        }
    }

    pub struct OutputTypes {
        pub cfg: Option<Attribute>,

        pub ident: Ident,

        pub priority: Priority,
        pub allowed_in_when: bool,

        pub generics: Vec<TypeParam>,
        pub phantom_idents: Vec<Ident>,

        pub arg_idents: Vec<Ident>,
        pub arg_types: Vec<Type>,

        pub assoc_types: Vec<TraitItemType>,
        pub arg_return_types: Vec<Type>,
    }
    impl ToTokens for OutputTypes {
        fn to_tokens(&self, tokens: &mut TokenStream) {
            let OutputTypes {
                cfg,
                ident,
                generics,
                assoc_types,
                phantom_idents: phantom,
                arg_idents,
                arg_types,
                arg_return_types,
                ..
            } = self;
            let args_impl_ident = ident!("{}_ArgsImpl", self.ident);
            let args_ident = ident!("{}_Args", self.ident);
            let arg_locals: Vec<_> = arg_idents.iter().enumerate().map(|(i, id)| ident!("__{}_{}", i, id)).collect();
            let crate_core = crate_core();

            let (phantom_decl, phantom_init) = if self.phantom_idents.is_empty() {
                (TokenStream::new(), TokenStream::new())
            } else {
                (
                    quote! {
                        pub _phantom: std::marker::PhantomData<( #(#phantom),* )>,
                    },
                    quote! {
                        _phantom: std::marker::PhantomData,
                    },
                )
            };

            let (generic_decl, generic_use) = if generics.is_empty() {
                (TokenStream::new(), TokenStream::new())
            } else {
                let generic_idents = generics.iter().map(|t| &t.ident);
                (quote! { < #(#generics),* > }, quote! { < #(#generic_idents),* > })
            };

            let assoc_connect = if generics.is_empty() {
                TokenStream::new()
            } else {
                let mut co = TokenStream::new();
                for gen in assoc_types {
                    let ident = &gen.ident;
                    co.extend(quote! { type #ident = #ident; });
                }
                co
            };

            #[cfg(debug_assertions)]
            let arg_debug_vars = if self.allowed_in_when {
                quote! {
                    let arg_debug_vars = {
                        let ( #(#arg_locals),* ) = self_.unwrap_ref();
                        Box::new([
                        #(
                            #crate_core::debug::debug_var(
                                #crate_core::var::IntoVar::into_var(
                                    std::clone::Clone::clone(#arg_locals)
                                )
                            ),
                        )*
                        ])
                    };
                }
            } else {
                quote! {
                    let arg_debug_vars = Box::new([]);
                }
            };

            let set = if self.priority.is_capture_only() {
                TokenStream::new()
            } else {
                let set_ident = ident!("__{}_set", ident);
                let set_debug_ident = ident!("__{}_set_debug", ident);
                #[cfg(debug_assertions)]
                {
                    let ident_str = ident.to_string();
                    let arg_idents_str = arg_idents.iter().map(|i| i.to_string());
                    let priority = match self.priority {
                        Priority::Context(_) => quote!(Context),
                        Priority::Event(_) => quote!(Event),
                        Priority::Outer(_) => quote!(Outer),
                        Priority::Size(_) => quote!(Size),
                        Priority::Inner(_) => quote!(Inner),
                        Priority::CaptureOnly(_) => quote!(CaptureOnly),
                    };
                    quote! {
                        #[doc(hidden)]
                        #[inline]
                        pub fn #set_ident(self_: impl #args_ident, child: impl #crate_core::UiNode) -> impl #crate_core::UiNode {
                            let ( #(#arg_locals),* ) = self_.unwrap();
                            #ident(child, #( #arg_locals ),*)
                        }

                        #[doc(hidden)]
                        #[inline]
                        pub fn #set_debug_ident(
                            self_: impl #args_ident,
                            child: impl #crate_core::UiNode,
                            property_name: &'static str,
                            instance_location: #crate_core::debug::SourceLocation,
                            user_assigned: bool
                        ) -> #crate_core::debug::PropertyInfoNode {
                            #arg_debug_vars

                            fn box_fix(node: impl #crate_core::UiNode) -> Box<dyn #crate_core::UiNode> {
                                #crate_core::UiNode::boxed(node)
                            }
                            let node = box_fix(#set_ident(self_, child));

                            #crate_core::debug::PropertyInfoNode::new_v1(
                                node,
                                #crate_core::debug::PropertyPriority::#priority,
                                #ident_str,
                                #crate_core::debug::source_location!(),
                                property_name,
                                instance_location,
                                &[#( #arg_idents_str ),*],
                                arg_debug_vars,
                                user_assigned
                            )
                        }
                    }
                }

                #[cfg(not(debug_assertions))]
                quote! {
                    #[doc(hidden)]
                    #[inline]
                    pub fn #set_ident(self_: impl #args_ident, child: impl #crate_core::UiNode) -> impl #crate_core::UiNode {
                        let ( #(#arg_locals),* ) = self_.unwrap();
                        #ident(child, #( #arg_locals ),*)
                    }
                }
            };

            let cap_debug = {
                #[cfg(debug_assertions)]
                {
                    let cap_ident = ident!("__{}_captured_debug", ident);
                    let arg_idents_str = arg_idents.iter().map(|i| i.to_string());

                    quote! {
                        #[doc(hidden)]
                        #[inline]
                        pub fn #cap_ident(
                            self_: &impl #args_ident,
                            property_name: &'static str,
                            instance_location: #crate_core::debug::SourceLocation,
                            user_assigned: bool
                        ) -> #crate_core::debug::CapturedPropertyV1 {
                            #arg_debug_vars
                            #crate_core::debug::CapturedPropertyV1 {
                                property_name,
                                instance_location,
                                arg_names: &[#( #arg_idents_str ),*],
                                arg_debug_vars,
                                user_assigned,
                            }
                        }
                    }
                }
                #[cfg(not(debug_assertions))]
                TokenStream::new()
            };

            let (unwrap_ty, unwrap_expr) = if arg_return_types.len() == 1 {
                let ty = arg_return_types[0].to_token_stream();
                let single_arg = &arg_idents[0];
                let expr = quote! { self.#single_arg };
                (ty, expr)
            } else {
                (
                    quote! {
                        ( #( #arg_return_types ),* )
                    },
                    quote! {
                        ( #( self.#arg_idents ),* )
                    },
                )
            };
            let (unwrap_ty_ref, unwrap_expr_ref) = if arg_return_types.len() == 1 {
                let single_ty = &arg_return_types[0];
                let ty = quote! { &#single_ty };
                let single_arg = &arg_idents[0];
                let expr = quote! { &self.#single_arg };
                (ty, expr)
            } else {
                (quote! { ( #( &#arg_return_types ),* ) }, quote! { ( #( &self.#arg_idents ),* ) })
            };

            let named_arg_mtds: Vec<_> = arg_idents.iter().map(|a| ident!("__{}", a)).collect();
            let numbered_arg_mtds: Vec<_> = (0..arg_idents.len()).map(|a| ident!("__{}", a)).collect();

            tokens.extend(quote! {
                #cfg
                #[doc(hidden)]
                #[allow(non_camel_case_types)]
                pub struct #args_impl_ident #generic_decl {
                    #phantom_decl
                    #(pub #arg_idents: #arg_types,)*
                }

                #cfg
                #[doc(hidden)]
                #[allow(non_camel_case_types)]
                pub trait #args_ident {
                    #(#assoc_types)*

                    #(
                        fn #named_arg_mtds(&self) -> &#arg_return_types;
                        fn #numbered_arg_mtds(&self) -> &#arg_return_types;
                    )*

                    fn unwrap(self) -> #unwrap_ty;
                    fn unwrap_ref(&self) -> #unwrap_ty_ref;
                }

                #cfg
                #[allow(non_camel_case_types)]
                impl #generic_decl #args_impl_ident #generic_use {
                    #[inline]
                    pub fn new(#( #arg_idents: #arg_types ),*) -> Self {
                        Self {
                            #phantom_init
                            #(#arg_idents,)*
                        }
                    }

                    #[inline]
                    pub fn args(self) -> impl #args_ident {
                        self
                    }
                }

                #cfg
                #[allow(non_camel_case_types)]
                impl #generic_decl #args_ident for #args_impl_ident #generic_use {
                    #assoc_connect

                    #(
                        #[inline]
                        fn #named_arg_mtds(&self) -> &#arg_return_types {
                            &self.#arg_idents
                        }

                        #[inline]
                        fn #numbered_arg_mtds(&self) -> &#arg_return_types {
                            &self.#arg_idents
                        }
                    )*

                    #[inline]
                    fn unwrap(self) -> #unwrap_ty {
                        #unwrap_expr
                    }

                    #[inline]
                    fn unwrap_ref(&self) -> #unwrap_ty_ref {
                        #unwrap_expr_ref
                    }
                }

                #set
                #cap_debug
            })
        }
    }

    impl ToTokens for Priority {
        fn to_tokens(&self, tokens: &mut TokenStream) {
            match self {
                Priority::Context(kw) => kw.to_tokens(tokens),
                Priority::Event(kw) => kw.to_tokens(tokens),
                Priority::Outer(kw) => kw.to_tokens(tokens),
                Priority::Size(kw) => kw.to_tokens(tokens),
                Priority::Inner(kw) => kw.to_tokens(tokens),
                Priority::CaptureOnly(kw) => kw.to_tokens(tokens),
            }
        }
    }

    pub struct OutputMacro {
        pub cfg: Option<Attribute>,

        pub macro_ident: Ident,
        pub export: bool,

        pub phantom_idents: Vec<Ident>,

        pub priority: Priority,

        pub allowed_in_when: bool,

        pub arg_idents: Vec<Ident>,
    }
    impl ToTokens for OutputMacro {
        fn to_tokens(&self, tokens: &mut TokenStream) {
            let OutputMacro {
                cfg,
                macro_ident,
                phantom_idents: phantom,
                priority,
                arg_idents,
                ..
            } = self;

            let phantom = if phantom.is_empty() {
                TokenStream::new()
            } else {
                quote! { _phantom: std::marker::PhantomData<#(#phantom),*>, }
            };

            let set = if priority.is_capture_only() {
                TokenStream::new()
            } else {
                #[cfg(debug_assertions)]
                quote! {
                    (set #priority, $node:ident, $property_path: path, $args:ident,
                        $property_name:expr, $source_location:expr, $user_assigned:tt) => {
                            let $node = {
                                use $property_path::{set_debug as __set};
                                __set($args, $node, $property_name, $source_location, $user_assigned)
                            };
                    };
                    (set #priority, $node:expr, $property_path: path, $args:ident) => {
                        let $node = {
                            use $property_path::{set as __set};
                            __set($args, $node)
                        };
                    };
                    (set $other:ident, $($ignore:tt)+) => { };
                }
                #[cfg(not(debug_assertions))]
                quote! {
                    (set #priority, $node:expr, $property_path: path, $args:ident) => {
                        let $node = {
                            use $property_path::{set as __set};
                            __set($args, $node)
                        };
                    };
                    (set $other:ident, $($ignore:tt)+) => { };
                }
            };

            let allowed_in_when = if self.allowed_in_when {
                quote! {
                    (assert allowed_in_when=> $msg:tt) => { };
                }
            } else {
                quote! {
                    (assert allowed_in_when=> $msg:tt) => {
                        std::compile_error!{$msg}
                    };
                }
            };

            let capture_only = if !priority.is_capture_only() {
                quote! {
                    (assert !capture_only=> $msg:tt) => { };
                }
            } else {
                quote! {
                    (assert !capture_only=> $msg:tt) => {
                        std::compile_error!{$msg}
                    };
                }
            };

            let if_pub = if self.export {
                quote! {
                    (if export=> $($tt:tt)*) => {
                        $($tt)*
                    };
                }
            } else {
                quote! {
                    (if export=> $($tt:tt)*) => { };
                }
            };

            let arg_locals: Vec<_> = arg_idents.iter().enumerate().map(|(i, id)| ident!("__{}_{}", i, id)).collect();

            let switches = if arg_locals.len() == 1 {
                let arg = &arg_locals[0];
                quote! {
                    let #arg = __switch_var!($idx, $($arg_for_i),+);
                }
            } else {
                let n = (0..arg_locals.len()).map(syn::Index::from);
                quote! {
                    #(let #arg_locals = __switch_var!(std::clone::Clone::clone(&$idx), $($arg_for_i.#n),+) ;)*
                }
            };

            tokens.extend(quote! {
                #cfg
                #[doc(hidden)]
                #[macro_export]
                macro_rules! #macro_ident {
                    (named_new $property_path:path { $($fields:tt)+ }) => {
                        {
                            use $property_path::{ArgsImpl as __ArgsImpl};
                            __ArgsImpl {
                                #phantom
                                $($fields)+
                            }
                        }
                    };

                    #set

                    #allowed_in_when

                    #capture_only

                    #if_pub

                    (switch $property_path:path, $idx:ident, $($arg_for_i:ident),+) => {
                        {
                            use $property_path::{ArgsImpl as __ArgsImpl, Args as __Args, switch_var as __switch_var};
                            $(let $arg_for_i = __Args::unwrap($arg_for_i);)+
                            #switches
                            __ArgsImpl::new(#(#arg_locals),*)
                        }
                    };
                }
            })
        }
    }

    pub struct OutputMod {
        pub cfg: Option<Attribute>,
        pub vis: Visibility,
        pub is_capture_only: bool,
        pub ident: Ident,
        pub macro_ident: Ident,
        pub args_ident: Ident,
        pub args_impl_ident: Ident,
    }
    impl ToTokens for OutputMod {
        fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
            let OutputMod {
                cfg,
                vis,
                ident,
                macro_ident,
                args_ident,
                args_impl_ident,
                ..
            } = self;

            let crate_core = crate_core();

            let set_export = if self.is_capture_only {
                TokenStream::new()
            } else {
                let set_ident = ident!("__{}_set", ident);
                let set_dbg_ident = ident!("__{}_set_debug", ident);

                #[cfg(debug_assertions)]
                {
                    quote! {
                        #set_ident as set,
                        #set_dbg_ident as set_debug,
                    }
                }
                #[cfg(not(debug_assertions))]
                quote! {
                    #set_ident as set,
                }
            };

            let cap_export = {
                #[cfg(debug_assertions)]
                {
                    let cap_ident = ident!("__{}_captured_debug", ident);
                    quote! {
                        #cap_ident as captured_debug,
                    }
                }
                #[cfg(not(debug_assertions))]
                TokenStream::new()
            };

            tokens.extend(quote! {
                #cfg
                #[doc(hidden)]
                #vis mod #ident {
                    #vis use super::{
                        #ident as export,
                    };
                    pub use super::{
                        #args_impl_ident as ArgsImpl,
                        #args_ident as Args,
                        #set_export
                        #cap_export
                    };
                    pub use #macro_ident as code_gen;
                    pub use #crate_core::var::switch_var;
                }
            })
        }
    }
}
