use crate::grammar::Grammar;
use crate::parsers::items::{CanonicalCollectionGraph, LR0Item};
use crate::parsers::{GrammarParserLR, ParseTableLR};

pub struct GrammarParserLR0 {
    #[allow(dead_code)]
    collection: CanonicalCollectionGraph<LR0Item>,
    table: ParseTableLR,
}

impl GrammarParserLR for GrammarParserLR0 {
    fn get_parse_table(&self) -> &ParseTableLR {
        &self.table
    }
}

impl GrammarParserLR0 {
    pub fn from_grammar(grammar: &Grammar) -> Self {
        let collection = CanonicalCollectionGraph::new(grammar, LR0Item::new(0, 0));
        let table =
            ParseTableLR::from_collection(grammar, &collection, |_, _, _| grammar.get_terminals());
        Self { collection, table }
    }
}
