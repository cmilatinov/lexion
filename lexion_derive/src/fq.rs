use proc_macro2::TokenStream;
use quote::{quote, ToTokens};

pub(crate) struct FQOption;
pub(crate) struct FQResult;

pub(crate) struct FQNodeIndex;
pub(crate) struct FQEdgeRef;
pub(crate) struct FQGraph;

pub(crate) struct FQTable;

pub(crate) struct FQDerivation;
pub(crate) struct FQDerivationNode;
pub(crate) struct FQGrammar;
pub(crate) struct FQGrammarRule;
pub(crate) struct FQGrammarParserLR;
pub(crate) struct FQGrammarParserSLR1;
pub(crate) struct FQSyntaxError;

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

impl ToTokens for FQTable {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::prettytable::Table).to_tokens(tokens)
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

impl ToTokens for FQSyntaxError {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        quote!(lexion_lib::error::SyntaxError).to_tokens(tokens)
    }
}
