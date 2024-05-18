use proc_macro::TokenStream;
use std::collections::HashSet;
use std::fs::File;

use proc_macro2::{Ident, Span};
use quote::quote;
use regex::Regex;
use syn::{LitInt, LitStr, parse_macro_input, Token, Type};
use syn::parse::{Parse, ParseStream};

use fq::*;
use lexion_lib::grammar::serialize::Grammar;

mod fq;

struct ImplParserInput {
    typename: Type,
    filename: LitStr,
}

impl Parse for ImplParserInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let typename = input.parse()?;
        input.parse::<Token![,]>()?;
        let filename = input.parse()?;
        Ok(Self { typename, filename })
    }
}

#[proc_macro]
pub fn impl_parser_from_json(stream: TokenStream) -> TokenStream {
    let input = parse_macro_input!(stream as ImplParserInput);
    let typename = &input.typename;
    let file = File::open(input.filename.value()).unwrap();
    let json: Grammar = serde_json::from_reader(file).unwrap();
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
        let right: proc_macro2::TokenStream = r
            .right
            .iter()
            .map(|s| {
                let s = LitStr::new(s.as_str(), Span::call_site());
                quote! { String::from(#s), }
            })
            .collect();
        tokens.extend(quote! {
            #FQGrammarRule {
                left: String::from(#left),
                right: vec![ #right ]
            },
        });
    }
    let non_terminals: HashSet<String> = rules
        .iter()
        .filter(|r| !r.left.starts_with("'"))
        .map(|r| r.left.clone())
        .collect();
    for nt in non_terminals.iter() {
        let mut nr_rule_cases = quote! {};
        let mut return_type = quote! {};
        for (i, r) in rules.iter().enumerate().filter(|(_, r)| r.left == *nt) {
            let mut code = quote! {};
            if let Some(reduction) = r.reduction.as_ref() {
                let arg_regex = Regex::new(r"\$([0-9]+)").unwrap();
                let code_str = arg_regex
                    .replace_all(reduction.code.as_str(), "_${1}")
                    .replace("$$", "_ret");
                let ty: Type = syn::parse_str(reduction.ty.as_str()).unwrap();
                return_type = quote! { -> #ty };
                code = syn::parse_str(code_str.as_str()).unwrap();
                code = quote! {
                    let mut _ret;
                    #code
                    _ret
                };
            }
            let index = LitInt::new(format!("{}", i + 1).as_str(), Span::call_site());
            let args_list: Vec<usize> = (1..=r.right.len())
                .filter(|i| r.right[*i - 1].as_str() != "ε")
                .collect();
            let node_args =
                args_list
                    .iter()
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
                .map(|i| {
                    (
                        format!("{}", r.right[i - 1]),
                        format!("_{}", i),
                        format!("_n{}", i),
                    )
                })
                .fold(quote! {}, |acc, (s, i, n)| {
                    let ident = Ident::new(s.as_str(), Span::call_site());
                    let ident1 = Ident::new(i.as_str(), Span::call_site());
                    let ident2 = Ident::new(n.as_str(), Span::call_site());
                    quote! {
                        #acc
                        let mut #ident1 = self.#ident(*#ident2, graph);
                    }
                });
            let t_args = args2
                .into_iter()
                .map(|i| (format!("_{}", i), format!("_n{}", i)))
                .fold(quote! {}, |acc, (i, n)| {
                    let ident1 = Ident::new(i.as_str(), Span::call_site());
                    let ident2 = Ident::new(n.as_str(), Span::call_site());
                    quote! {
                        #acc
                        let mut #ident1 = graph.node_weight(*#ident2).unwrap().token.clone();
                    }
                });
            nr_rule_cases.extend(quote! {
                #index => {
                    if let [#node_args] = &children[..] {
                        #nt_args
                        #t_args
                        #code
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
            fn #nt_ident(&self, node_id: #FQNodeIndex, graph: &#FQGraph<#FQDerivationNode, usize>) #return_type {
                let node = graph.node_weight(node_id).unwrap();
                let mut children = graph.edges(node_id).collect::<Vec<_>>();
                children.sort_by_key(|e| *e.weight());
                let children: Vec<#FQNodeIndex> = children.into_iter().map(|e| e.target()).collect();
                #nr_rule_cases
            }
        });
    }
    TokenStream::from(quote! {
        use #FQGrammarParserLR;
        use #FQEdgeRef;

        #definitions

        pub struct #typename {
            pub grammar: #FQGrammar,
            pub grammar_parser: #FQGrammarParserSLR1,
        }

        impl #typename {
            pub fn new() -> Self {
                let grammar = #FQGrammar::from_rules(vec![#tokens]);
                let grammar_parser = #FQGrammarParserSLR1::from_grammar(&grammar);
                Self {
                    grammar,
                    grammar_parser
                }
            }
        }

        impl #typename {
            pub fn transform(&self, derivation: &#FQDerivation) -> #parse_result {
                self.#start_symbol(derivation.0, &derivation.1)
            }

            pub fn parse_from_string(
                &self, string: &str
            ) -> #FQResult<#parse_result, #FQSyntaxError> {
                self.parse_from_string_trace(string, None)
            }

            pub fn parse_from_string_trace(
                &self, string: &str, trace: #FQOption<&mut #FQTable>
            ) -> #FQResult<#parse_result, #FQSyntaxError> {
                let derivation =
                    self.grammar_parser.parse_from_string_trace(&self.grammar, string, trace)?;
                Ok(self.transform(&derivation))
            }

            pub fn parse_from_file(
                &self, file: &'static str
            ) -> #FQResult<#parse_result, #FQSyntaxError> {
                self.parse_from_file_trace(file, None)
            }

            pub fn parse_from_file_trace(
                &self, file: &'static str, trace: #FQOption<&mut #FQTable>
            ) -> #FQResult<#parse_result, #FQSyntaxError> {
                let derivation =
                    self.grammar_parser.parse_from_file_trace(&self.grammar, file, trace)?;
                Ok(self.transform(&derivation))
            }
        }

        impl #typename {
            #methods
        }
    })
}
