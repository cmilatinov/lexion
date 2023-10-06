use std::collections::HashSet;
use crate::grammar::Grammar;
use crate::parsers::items::{CanonicalCollectionGraph, LR0Item, LRItem};
use crate::parsers::{GrammarParserLR, ParseTableLR};

pub struct GrammarParserSLR1 {
    collection: CanonicalCollectionGraph<LR0Item>,
    table: ParseTableLR
}

impl GrammarParserLR for GrammarParserSLR1 {
    fn get_parse_table(&self) -> &ParseTableLR {
        &self.table
    }
}

impl GrammarParserSLR1 {
    pub fn from_grammar(grammar: &Grammar) -> Self {
        let collection = CanonicalCollectionGraph::new(grammar, LR0Item::new(0, 0));
        let empty = HashSet::new();
        let table = ParseTableLR::from_collection(
            grammar,
            &collection,
            |i,_,_| {
                let rule = i.get_rule(grammar);
                grammar.follow_of(&*rule.left).unwrap_or(&empty)
            }
        );
        Self { collection, table }
    }
}