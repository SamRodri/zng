use std::collections::HashSet;

use proc_macro2::TokenStream;
use syn::*;

use crate::util::Errors;

pub fn expand(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as Input);
    let message_id = input.message_id.value();
    let message = input.message.value();

    let mut errors = Errors::default();

    let span = input.message_id.span();
    if message_id.is_empty() {
        errors.push("message_id cannot be empty", span);
    } else if message_id.chars().any(|c| c.is_whitespace()) {
        errors.push("message_id cannot be contain spaces", span);
    } else if message_id.starts_with('-') {
        errors.push("message id cannot start with `-`", span);
    } else if message_id.starts_with('#') {
        errors.push("message id cannot start with `#`", span);
    } else if message_id.starts_with('.') {
        errors.push("message id cannot start with `.`", span);
    } else if let Some((id, attribute)) = message_id.split_once('.') {
        if let Err((_, e)) = fluent_syntax::parser::parse_runtime(format!("{id} = \n .{attribute} = m")) {
            for e in e {
                errors.push(e, span);
            }
        }
    } else if let Err((_, e)) = fluent_syntax::parser::parse_runtime(format!("{message_id} = m")) {
        for e in e {
            errors.push(e, span);
        }
    }

    let fluent_msg = format!("id = {message}");
    let mut variables = HashSet::new();

    if message.is_empty() {
        errors.push("message cannot be empty", input.message.span());
    } else {
        match fluent_syntax::parser::parse_runtime(fluent_msg.as_str()) {
            Ok(ast) => {
                let span = input.message.span();
                if ast.body.len() > 1 {
                    match &ast.body[1] {
                        fluent_syntax::ast::Entry::Message(m) => {
                            errors.push(format!("unescaped fluent message `{}..`", m.id.name), span);
                        }
                        fluent_syntax::ast::Entry::Term(t) => {
                            errors.push(format!("unescaped fluent term `-{}..`", t.id.name), span);
                        }
                        fluent_syntax::ast::Entry::Comment(_c)
                        | fluent_syntax::ast::Entry::GroupComment(_c)
                        | fluent_syntax::ast::Entry::ResourceComment(_c) => {
                            errors.push("unescaped fluent comment `#..`", span);
                        }
                        fluent_syntax::ast::Entry::Junk { content } => {
                            errors.push(format!("unexpected `{content}`"), span);
                        }
                    }
                } else {
                    match &ast.body[0] {
                        fluent_syntax::ast::Entry::Message(m) => {
                            if m.id.name != "id" {
                                non_user_error!("")
                            }
                            if m.comment.is_some() {
                                non_user_error!("")
                            }

                            if let Some(m) = &m.value {
                                collect_vars_pattern(&mut errors, &mut variables, m);
                            }
                            if !m.attributes.is_empty() {
                                errors.push(format!("unescaped fluent attribute `.{}..`", m.attributes[0].id.name), span);
                            }
                        }
                        fluent_syntax::ast::Entry::Term(t) => {
                            errors.push(format!("unescaped fluent term `-{}..`", t.id.name), span);
                        }
                        fluent_syntax::ast::Entry::Comment(_c)
                        | fluent_syntax::ast::Entry::GroupComment(_c)
                        | fluent_syntax::ast::Entry::ResourceComment(_c) => {
                            errors.push("unescaped fluent comment `#..`", span);
                        }
                        fluent_syntax::ast::Entry::Junk { content } => {
                            errors.push(format!("unexpected `{content}`"), span);
                        }
                    }
                }
            }
            Err((_, e)) => {
                for e in e {
                    errors.push(e, input.message.span());
                }
            }
        }
    }

    if errors.is_empty() {
        let l10n_path = &input.l10n_path;
        let message_id = &input.message_id;
        let message = &input.message;

        let mut build = quote! {
            #l10n_path.l10n_message(#message_id, #message)
        };
        for var in variables {
            let var_ident = ident!("{}", var);
            build.extend(quote! {
                .l10n_arg(#var, #var_ident)
            });
        }
        build.extend(quote! {
            .build()
        });

        build.into()
    } else {
        quote! {
            #errors
        }
        .into()
    }
}

fn collect_vars_pattern<'s>(errors: &mut Errors, vars: &mut HashSet<&'s str>, pattern: &fluent_syntax::ast::Pattern<&'s str>) {
    for el in &pattern.elements {
        match el {
            fluent_syntax::ast::PatternElement::TextElement { .. } => continue,
            fluent_syntax::ast::PatternElement::Placeable { expression } => collect_vars_expr(errors, vars, expression),
        }
    }
}
fn collect_vars_expr<'s>(errors: &mut Errors, vars: &mut HashSet<&'s str>, expression: &fluent_syntax::ast::Expression<&'s str>) {
    match expression {
        fluent_syntax::ast::Expression::Select { selector, variants } => {
            collect_vars_inline_expr(errors, vars, selector);
            for v in variants {
                collect_vars_pattern(errors, vars, &v.value);
            }
        }
        fluent_syntax::ast::Expression::Inline(expr) => collect_vars_inline_expr(errors, vars, expr),
    }
}
fn collect_vars_inline_expr<'s>(errors: &mut Errors, vars: &mut HashSet<&'s str>, inline: &fluent_syntax::ast::InlineExpression<&'s str>) {
    match inline {
        fluent_syntax::ast::InlineExpression::FunctionReference { arguments, .. } => {
            for arg in &arguments.positional {
                collect_vars_inline_expr(errors, vars, arg);
            }
            for arg in &arguments.named {
                collect_vars_inline_expr(errors, vars, &arg.value);
            }
        }
        fluent_syntax::ast::InlineExpression::VariableReference { id } => {
            vars.insert(id.name);
        }
        fluent_syntax::ast::InlineExpression::Placeable { expression } => collect_vars_expr(errors, vars, expression),
        _ => {}
    }
}

struct Input {
    l10n_path: TokenStream,
    message_id: LitStr,
    message: LitStr,
}
impl parse::Parse for Input {
    fn parse(input: parse::ParseStream) -> Result<Self> {
        Ok(Input {
            l10n_path: non_user_braced!(input, "l10n_path").parse().unwrap(),
            message_id: non_user_braced!(input, "message_id").parse()?,
            message: non_user_braced!(input, "message").parse()?,
        })
    }
}
