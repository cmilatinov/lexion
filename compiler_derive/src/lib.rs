use proc_macro::TokenStream;
use std::fs::File;
use proc_macro2::Span;
use quote::quote;
use syn::LitStr;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GrammarRule {
    left: String,
    right: Vec<String>,
    reduction: Option<String>
}

#[proc_macro]
pub fn impl_parser_json(input: TokenStream) -> TokenStream {
    let filename: LitStr = syn::parse(input).unwrap();
    let file = File::open(filename.value()).unwrap();
    let rules: Vec<GrammarRule> = serde_json::from_reader(file).unwrap();
    let mut tokens = quote!{};
    for r in rules.iter() {
        let left = &r.left;
        let right: proc_macro2::TokenStream = r.right.iter().map(|s| {
            let s = LitStr::new(s.as_str(), Span::call_site());
            quote! { #s, }
        })
            .collect();
        let reduction = if let Some(reduction) = r.reduction.as_ref() { reduction.as_str() } else { "|_| json!({})" };
        let reduction_tokens: proc_macro2::TokenStream = reduction.parse().unwrap();
        tokens.extend(quote! {
            GrammarRule {
                left: #left,
                right: vec![ #right ],
                reduction: Box::new(#reduction_tokens)
            },
        })
    }
    TokenStream::from(quote! {
        {
            use compiler::lib::grammar::{Grammar, GrammarRule};
            use serde_json::json;
            vec![ #tokens ]
        }
    })
}

#[proc_macro]
pub fn impl_parser_grm(input: TokenStream) -> TokenStream {
    TokenStream::from(quote! {})
}