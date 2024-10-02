use crate::grammar::Grammar;
use crate::parsers::items::{CanonicalCollectionGraph, LR0Item, LRItem};
use crate::parsers::{GrammarParserLR, ParseTableAction, ParseTableLR, ParseTableOverride};
use std::collections::HashSet;

pub struct GrammarParserSLR1 {
    pub collection: CanonicalCollectionGraph<LR0Item>,
    pub table: ParseTableLR,
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
        let mut table = ParseTableLR::from_collection(grammar, &collection, |i, _, _| {
            let rule = i.get_rule(grammar);
            grammar.follow_of(&*rule.left).unwrap_or(&empty)
        });
        table.apply_conflict_resolutions(vec![ParseTableOverride {
            state: 114,
            symbol: "','",
            action: ParseTableAction::Reduce(96),
        }]);
        Self { collection, table }
    }
}
