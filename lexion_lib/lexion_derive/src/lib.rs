use crate::serialize::{
    GrammarData, ParseTableAction, ParseTableOverrideData, ReductionData, RuleData,
};
use darling::FromDeriveInput;
use fq::*;
use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::quote;
use regex::Regex;
use std::collections::HashSet;
use std::fs::File;
use syn::{DeriveInput, LitInt, LitStr, Type};

mod fq;
mod serialize;

#[derive(FromDeriveInput)]
#[darling(attributes(grammar))]
struct ParserOptions {
    ident: Ident,
    path: String,
}

#[proc_macro_derive(Parser, attributes(grammar))]
pub fn derive_parser(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse_macro_input!(input);
    let opts = ParserOptions::from_derive_input(&input).expect("Invalid attributes for Parser");

    let struct_name = &opts.ident;

    let file = File::open(opts.path).expect("Failed to open grammar file");
    let json: GrammarData = serde_json::from_reader(file).unwrap();

    let (start_symbol, parse_result) = symbol_result_impl(&json.rules);
    let grammar = grammar_impl(&json);
    let methods = methods_impl(&json.rules);

    quote! {
        use #FQGrammarParserLR;
        use #FQEdgeRef;

        #grammar

        impl #struct_name {
            pub const GRAMMAR: &'static GRAMMAR = &GRAMMAR;
            pub const PARSER: &'static PARSER = &PARSER;

            #methods

            pub fn transform(&mut self, derivation: &#FQDerivation) -> #parse_result {
                self.#start_symbol(&derivation.graph, derivation.root)
            }
        }

        impl #FQParser for #struct_name {
            type Result = #parse_result;

            fn token_types() -> &'static [#FQTokenType] {
                &GRAMMAR.get_token_types()
            }

            fn parse_trace(
                &mut self,
                tokenizer: #FQTokenizer,
                trace: #FQOption<&mut #FQTableBuilder>
            ) -> #FQResult<Self::Result, #FQParseError> {
                let derivation =
                    PARSER.parse_trace(&GRAMMAR, tokenizer, trace)?;
                Ok(self.transform(&derivation))
            }

        }
    }
    .into()
}

fn symbol_result_impl(rules: &[RuleData]) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let first_rule = &rules[0];
    let ident = Ident::new(first_rule.left.as_str(), Span::call_site());
    let start_symbol = quote! { #ident };

    let reduction = first_rule
        .reduction
        .as_ref()
        .unwrap_or_else(|| panic!("Missing '{}' rule reduction", first_rule.left));
    let ty: Type = reduction_ty(reduction);
    let parse_result = quote! { #ty };

    (start_symbol, parse_result)
}

fn grammar_impl(json: &GrammarData) -> proc_macro2::TokenStream {
    let rules = rules_impl(&json.rules);
    let overrides = overrides_impl(&json.overrides);
    quote! {
        #FQLazyStatic {
            pub static ref GRAMMAR: #FQGrammar = #FQGrammar::from_rules(vec![#rules]);
            pub static ref PARSER: #FQGrammarParserSLR1 = {
                let mut parser = #FQGrammarParserSLR1::from_grammar(&GRAMMAR);
                parser.table.apply_conflict_resolutions([
                    #overrides
                ].iter());
                parser
            };
        }
    }
}

fn rules_impl(rules: &[RuleData]) -> proc_macro2::TokenStream {
    let mut tokens = quote! {};
    for rule in rules {
        let left = &rule.left;
        let right: proc_macro2::TokenStream = rule
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
    tokens
}

fn overrides_impl(overrides: &Option<Vec<ParseTableOverrideData>>) -> proc_macro2::TokenStream {
    let mut tokens = quote!();
    for table_override in overrides.iter().flat_map(|vec| vec.iter()) {
        let state = &table_override.state;
        let symbol = &table_override.symbol;
        let action: FQParseTableActionInstance = table_override
            .action
            .parse::<ParseTableAction>()
            .unwrap_or_else(|_| panic!("Failed to parse override '{}'", table_override.action))
            .into();
        tokens = quote! {
            #tokens
            #FQParseTableOverride {
                state: #state,
                symbol: #symbol,
                action: #action,
            },
        }
    }
    tokens
}

fn reduction_ty(reduction: &ReductionData) -> Type {
    syn::parse_str(reduction.ty.as_str())
        .unwrap_or_else(|_| panic!("Failed to parse '{}' reduction type", reduction.ty))
}

fn methods_impl(rules: &[RuleData]) -> proc_macro2::TokenStream {
    let mut methods = quote!();
    let non_terminals: HashSet<String> = rules
        .iter()
        .filter(|r| !r.left.starts_with("'"))
        .map(|r| r.left.clone())
        .collect();
    let arg_regex = Regex::new(r"\$([0-9]+)").unwrap();
    for nt in non_terminals.iter() {
        let mut nr_rule_cases = quote! {};
        let mut return_type = quote! {};
        for (i, r) in rules.iter().enumerate().filter(|(_, r)| r.left == *nt) {
            let mut code = quote! {};
            if let Some(reduction) = r.reduction.as_ref() {
                let ty = reduction_ty(reduction);
                return_type = quote! { -> #ty };
                let code_str = arg_regex
                    .replace_all(reduction.code.as_str(), "_${1}")
                    .replace("$$", "_ret");
                if !code_str.is_empty() {
                    code = syn::parse_str(code_str.as_str()).unwrap();
                    code = quote! {
                    let mut _ret;
                        #code
                        _ret
                    };
                }
            }
            let index = LitInt::new(format!("{}", i + 1).as_str(), Span::call_site());
            let args_list: Vec<usize> = (1..=r.right.len())
                .filter(|i| r.right[*i - 1].as_str() != "ε")
                .collect();
            let node_args = args_list
                .iter()
                .map(|i| format!("_n{i}"))
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
                        r.right[i - 1].to_string(),
                        format!("_{i}"),
                        format!("_n{i}"),
                    )
                })
                .fold(quote! {}, |acc, (s, i, n)| {
                    let ident = Ident::new(s.as_str(), Span::call_site());
                    let ident1 = Ident::new(i.as_str(), Span::call_site());
                    let ident2 = Ident::new(n.as_str(), Span::call_site());
                    quote! {
                        #acc
                        let mut #ident1 = self.#ident(graph, *#ident2);
                    }
                });
            let t_args = args2
                .into_iter()
                .map(|i| (format!("_{i}"), format!("_n{i}")))
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
            fn #nt_ident(&mut self, graph: &#FQGraph<#FQDerivationNode, usize>, node_id: #FQNodeIndex) #return_type {
                let node = graph.node_weight(node_id).unwrap();
                let mut children = graph.edges(node_id).collect::<Vec<_>>();
                children.sort_by_key(|e| *e.weight());
                let children: Vec<#FQNodeIndex> = children.into_iter().map(|e| e.target()).collect();
                #nr_rule_cases
            }
        });
    }
    methods
}
