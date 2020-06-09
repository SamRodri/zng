use crate::util;
use proc_macro2::TokenStream;
use syn::{parse::*, punctuated::Punctuated, *};

/// `widget!` recursive inheritance.
/// To include tokens from each inherited widget internal:
/// 1 - We need to call each inherited widget macro special (=> inherit) branch
///     this branch includes the internal tokens of that widget plus our macro declaration
/// 2 - All inside the next inherited macro, recursively.
/// 3 - When there is no more widgets to inherit we go to widget_stage3.
/// 0 - widget_stage1 already called the first inherit, like the code we generated here.
pub fn expand(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let WidgetInheriting {
        stage3_name,
        mut inherits,
        rest,
    } = parse_macro_input!(input as WidgetInheriting);

    if inherits.is_empty() {
        // go to widget_stage3.
        let r = quote! {
            #stage3_name! {
                #rest
            }
        };
        r.into()
    } else {
        // recursive to widget_stage2 again.
        let next_inherit = inherits.pop().unwrap().into_value();

        let r = quote! {
            #next_inherit! {
                -> inherit {
                    #stage3_name;
                    #next_inherit;
                    #inherits
                }
                #rest
            }
        };
        r.into()
    }
}

struct WidgetInheriting {
    stage3_name: Ident,
    inherits: Punctuated<Path, Token![+]>,
    rest: TokenStream,
}

impl Parse for WidgetInheriting {
    fn parse(input: ParseStream) -> Result<Self> {
        input.parse::<Token![=>]>().unwrap_or_else(|e| non_user_error!(e));

        let inner = util::non_user_braced(input);

        let stage3_name = inner.parse().unwrap_or_else(|e| non_user_error!(e));
        inner.parse::<Token![;]>().unwrap_or_else(|e| non_user_error!(e));

        let inherits = Punctuated::parse_terminated(&inner).unwrap_or_else(|e| non_user_error!(e));

        let rest = input.parse().unwrap_or_else(|e| non_user_error!(e));
        Ok(WidgetInheriting {
            stage3_name,
            inherits,
            rest,
        })
    }
}
