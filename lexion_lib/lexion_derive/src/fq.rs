use crate::serialize::ParseTableAction;
use proc_macro2::TokenStream;
use quote::{quote, ToTokens};

pub(crate) struct FQOption;
pub(crate) struct FQResult;

pub(crate) struct FQNodeIndex;
pub(crate) struct FQEdgeRef;
pub(crate) struct FQGraph;

pub(crate) struct FQTableBuilder;
pub(crate) struct FQParseTableOverride;
pub(crate) struct FQParseTableAction;
pub(crate) struct FQParseTableActionInstance {
    action: ParseTableAction,
}

pub(crate) struct FQDerivation;
pub(crate) struct FQDerivationNode;
pub(crate) struct FQGrammar;
pub(crate) struct FQGrammarRule;
pub(crate) struct FQGrammarParserLR;
pub(crate) struct FQGrammarParserSLR1;
pub(crate) struct FQParseError;
pub(crate) struct FQParser;
pub(crate) struct FQTokenizer;
pub(crate) struct FQTokenType;

pub(crate) struct FQLazyStatic;

impl ToTokens for FQOption {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(std::option::Option).to_tokens(tokens)
    }
}

impl ToTokens for FQResult {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(std::result::Result).to_tokens(tokens)
    }
}

impl ToTokens for FQNodeIndex {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::petgraph::graph::NodeIndex).to_tokens(tokens)
    }
}

impl ToTokens for FQEdgeRef {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::petgraph::visit::EdgeRef).to_tokens(tokens)
    }
}

impl ToTokens for FQGraph {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::petgraph::graph::Graph).to_tokens(tokens)
    }
}

impl ToTokens for FQTableBuilder {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::tabled::builder::Builder).to_tokens(tokens)
    }
}

impl ToTokens for FQParseTableOverride {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::parsers::ParseTableOverride).to_tokens(tokens)
    }
}

impl ToTokens for FQParseTableAction {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::parsers::ParseTableAction).to_tokens(tokens)
    }
}

impl From<ParseTableAction> for FQParseTableActionInstance {
    fn from(action: ParseTableAction) -> Self {
        Self { action }
    }
}

impl FQParseTableActionInstance {
    fn recurse_to_tokens(action: &ParseTableAction) -> TokenStream {
        match action {
            ParseTableAction::Goto(value) => quote!(#FQParseTableAction::Goto(#value)),
            ParseTableAction::Shift(value) => quote!(#FQParseTableAction::Shift(#value)),
            ParseTableAction::Reduce(value) => quote!(#FQParseTableAction::Reduce(#value)),
            ParseTableAction::Accept => quote!(#FQParseTableAction::Accept),
            ParseTableAction::Reject => quote!(#FQParseTableAction::Reject),
            ParseTableAction::Conflict(actions) => {
                let mut items = quote! {};
                for action in actions {
                    let action_tokens = Self::recurse_to_tokens(action);
                    items = quote! { #items #action_tokens , };
                }
                let tokens = quote!(#FQParseTableAction::Conflict(vec![#items]));
                panic!("{tokens:#?}");
            }
        }
    }
}

impl ToTokens for FQParseTableActionInstance {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        Self::recurse_to_tokens(&self.action).to_tokens(tokens)
    }
}

impl ToTokens for FQDerivation {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::grammar::Derivation).to_tokens(tokens)
    }
}

impl ToTokens for FQDerivationNode {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::grammar::DerivationNode).to_tokens(tokens)
    }
}

impl ToTokens for FQGrammar {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::grammar::Grammar).to_tokens(tokens)
    }
}

impl ToTokens for FQGrammarRule {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::grammar::GrammarRule).to_tokens(tokens)
    }
}

impl ToTokens for FQGrammarParserLR {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::parsers::GrammarParserLR).to_tokens(tokens)
    }
}

impl ToTokens for FQGrammarParserSLR1 {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::parsers::GrammarParserSLR1).to_tokens(tokens)
    }
}

impl ToTokens for FQParseError {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::error::ParseError).to_tokens(tokens)
    }
}

impl ToTokens for FQParser {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::Parser).to_tokens(tokens)
    }
}

impl ToTokens for FQTokenizer {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::tokenizer::Tokenizer).to_tokens(tokens)
    }
}

impl ToTokens for FQTokenType {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::tokenizer::TokenType).to_tokens(tokens)
    }
}

impl ToTokens for FQLazyStatic {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lazy_static::lazy_static!).to_tokens(tokens)
    }
}
