use std::collections::{BTreeSet};
use crate::grammar::{Grammar, GrammarRule};

pub trait LRItem {
    fn get_dot_index(&self) -> usize;
    fn get_rule_index(&self) -> usize;
    fn get_rule<'a>(&'a self, grammar: &'a Grammar) -> &'a GrammarRule;
    fn is_final(&self, grammar: &Grammar) -> bool;
    fn is_accept(&self, grammar: &Grammar) -> bool;
    fn to_string(&self, grammar: &Grammar) -> String;
}

pub trait ClosurableItem<T: LRItem> {
    fn goto(grammar: &Grammar, items: &BTreeSet<T>, symbol: &str) -> BTreeSet<T>;
    fn closure(grammar: &Grammar, items: &mut BTreeSet<T>);
}

