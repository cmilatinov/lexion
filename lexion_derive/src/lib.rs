use proc_macro::TokenStream;
use std::collections::HashSet;
use std::fs::File;
use proc_macro2::{Ident, Span};
use quote::quote;
use syn::{LitInt, LitStr, parse_macro_input, Token, Type};
use serde::{Deserialize};
use syn::parse::{Parse, ParseStream};
use regex::Regex;

#[derive(Deserialize)]
struct Reduction {
    ty: String,
    code: String,
}

#[derive(Deserialize)]
struct GrammarRule {
    left: String,
    right: Vec<String>,
    reduction: Option<Reduction>,
}

#[derive(Deserialize)]
struct GrammarJSON {
    definitions: String,
    rules: Vec<GrammarRule>,
}

struct ImplParserInput {
    typename: Type,
    filename: LitStr,
}

impl Parse for ImplParserInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let typename = input.parse()?;
        input.parse::<Token![,]>()?;
        let filename = input.parse()?;
        Ok(Self {
            typename,
            filename,
        })
    }
}

#[proc_macro]
pub fn impl_parser_from_json(stream: TokenStream) -> TokenStream {
    let input = parse_macro_input!(stream as ImplParserInput);
    let typename = &input.typename;
    let file = File::open(input.filename.value()).unwrap();
    let json: GrammarJSON = serde_json::from_reader(file).unwrap();
    let rules = &json.rules;
    let definitions: proc_macro2::TokenStream = syn::parse_str(json.definitions.as_str()).unwrap();
    let mut tokens = quote! {};
    let mut methods = quote! {};
    let mut start_symbol = quote! {};
    let mut parse_result = quote! {};
    for (i, r) in rules.iter().enumerate() {
        if i == 0 {
            let ident = Ident::new(r.left.as_str(), Span::call_site());
            start_symbol = quote! { #ident };
            if let Some(reduction) = r.reduction.as_ref() {
                let ty: Type = syn::parse_str(reduction.ty.as_str()).unwrap();
                parse_result = quote! { #ty };
            }
        }
        let left = &r.left;
        let right: proc_macro2::TokenStream = r.right.iter().map(|s| {
            let s = LitStr::new(s.as_str(), Span::call_site());
            quote! { String::from(#s), }
        })
            .collect();
        tokens.extend(quote! {
            GrammarRule {
                left: String::from(#left),
                right: vec![ #right ]
            },
        });
    }
    let non_terminals: HashSet<String> = rules.iter()
        .filter(|r| !r.left.starts_with("'"))
        .map(|r| r.left.clone())
        .collect();
    for nt in non_terminals.iter() {
        let mut nr_rule_cases = quote! {};
        let mut return_type = quote! {};
        for (i, r) in rules.iter().enumerate().filter(|(i, r)| r.left == *nt) {
            let mut code = quote! {};
            if let Some(reduction) = r.reduction.as_ref() {
                let arg_regex = Regex::new(r"\$([0-9]+)").unwrap();
                let code_str = arg_regex
                    .replace_all(reduction.code.as_str(), "_${1}")
                    .replace("$$", "_ret");
                let ty: Type = syn::parse_str(reduction.ty.as_str()).unwrap();
                return_type = quote! { -> #ty };
                code = syn::parse_str(code_str.as_str()).unwrap();
            }
            let index = LitInt::new(format!("{}", i + 1).as_str(), Span::call_site());
            let args_list: Vec<usize> = (1..=r.right.len())
                .filter(|i| r.right[*i - 1].as_str() != "ε")
                .collect();
            let node_args = args_list.iter()
                .map(|i| format!("_n{}", i))
                .fold(quote! {}, |acc, i| {
                    let ident = Ident::new(i.as_str(), Span::call_site());
                    quote! { #acc #ident, }
                });
            let (args1, args2): (Vec<usize>, Vec<usize>) = args_list
                .into_iter()
                .filter(|i| r.right[*i - 1].as_str() != "ε")
                .partition(|i| !r.right[*i - 1].starts_with("'"));
            let nt_args = args1
                .into_iter()
                .map(|i| (
                    format!("{}", r.right[i - 1]),
                    format!("_{}", i),
                    format!("_n{}", i)
                ))
                .fold(quote! {}, |acc, (s, i, n)| {
                    let ident = Ident::new(s.as_str(), Span::call_site());
                    let ident1 = Ident::new(i.as_str(), Span::call_site());
                    let ident2 = Ident::new(n.as_str(), Span::call_site());
                    quote! {
                        #acc
                        let mut #ident1 = self.#ident(*#ident2, arena);
                    }
                });
            let t_args = args2
                .into_iter()
                .map(|i| (
                    format!("_{}", i),
                    format!("_n{}", i)
                ))
                .fold(quote! {}, |acc, (i, n)| {
                    let ident1 = Ident::new(i.as_str(), Span::call_site());
                    let ident2 = Ident::new(n.as_str(), Span::call_site());
                    quote! {
                        #acc
                        let mut #ident1 = arena.get(*#ident2).unwrap().get().token.clone();
                    }
                });
            nr_rule_cases.extend(quote! {
                #index => {
                    if let [#node_args] = &children[..] {
                        #nt_args
                        #t_args
                        let mut _ret;
                        #code
                        _ret
                    } else {
                        unreachable!()
                    }
                },
            });
        }
        nr_rule_cases = quote! {
            match node.rule_index {
                #nr_rule_cases
                _ => unreachable!()
            }
        };
        let nt_ident = Ident::new(nt.as_str(), Span::call_site());
        methods.extend(quote! {
            fn #nt_ident(&self, node_id: NodeId, arena: &Arena<DerivationNode>) #return_type {
                let node = arena.get(node_id).unwrap().get();
                let children: Vec<NodeId> = node_id.children(arena).collect();
                #nr_rule_cases
            }
        });
    }
    TokenStream::from(quote! {
        #definitions

        pub struct #typename {
            grammar: Grammar,
            grammar_parser: GrammarParserSLR1,
        }

        impl #typename {
            pub fn new() -> Self {
                let grammar = Grammar::from_rules(vec![#tokens]);
                let grammar_parser = GrammarParserSLR1::from_grammar(&grammar);
                Self {
                    grammar,
                    grammar_parser
                }
            }
        }

        impl #typename {
            pub fn parse_from_string(&self, string: &str) -> std::result::Result<#parse_result, lexion_lib::error::SyntaxError> {
                let Derivation(root_id, arena) = self.grammar_parser.parse_from_string(&self.grammar, string)?;
                Ok(self.#start_symbol(root_id, &arena))
            }
        }

        impl #typename {
            #methods
        }
    })
}