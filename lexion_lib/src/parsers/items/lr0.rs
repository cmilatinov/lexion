use std::collections::{BTreeSet};
use crate::grammar::{Grammar, GrammarRule};
use crate::parsers::items::{ClosurableItem, LRItem};
use crate::tokenizer::tokens::EPSILON;

#[derive(Eq, PartialEq, Hash, Copy, Clone, PartialOrd, Ord)]
pub struct LR0Item {
    pub rule_index: usize,
    pub dot_index: usize
}

impl LR0Item {
    pub fn new(rule_index: usize, dot_index: usize) -> Self {
        LR0Item { rule_index, dot_index }
    }
}

impl LRItem for LR0Item {
    fn get_dot_index(&self) -> usize {
        self.dot_index
    }

    fn get_rule_index(&self) -> usize {
        self.rule_index
    }

    fn get_rule<'a>(&'a self, grammar: &'a Grammar) -> &GrammarRule {
        &grammar.get_rules()[self.rule_index]
    }

    fn is_final(&self, grammar: &Grammar) -> bool {
        let rule = self.get_rule(grammar);
        self.dot_index == rule.right.len() || rule.right == vec![String::from(EPSILON)]
    }

    fn is_accept(&self, grammar: &Grammar) -> bool {
        let rule = self.get_rule(grammar);
        rule.left == grammar.get_augmented_start_symbol() &&
            rule.right == vec![grammar.get_start_symbol()] &&
            self.dot_index == 1
    }

    fn to_string(&self, grammar: &Grammar) -> String {
        let rule = self.get_rule(grammar);
        let mut tokens = rule.right.clone();
        tokens.insert(self.dot_index, String::from("•"));
        format!("{} -> {}", rule.left, tokens.join(" "))
    }
}

impl ClosurableItem<LR0Item> for LR0Item {
    fn goto(grammar: &Grammar, items: &BTreeSet<LR0Item>, symbol: &str) -> BTreeSet<LR0Item> {
        items.iter()
            .filter(|i| {
                let rule = i.get_rule(grammar);
                i.dot_index < rule.right.len() &&
                    rule.right[i.dot_index] == symbol
            })
            .map(|i| LR0Item { rule_index: i.rule_index, dot_index: i.dot_index + 1 })
            .collect()
    }

    fn closure(grammar: &Grammar, items: &mut BTreeSet<LR0Item>) {
        let rules = grammar.get_rules();
        let mut additions: Vec<LR0Item> = Vec::new();
        loop {
            for item in items.iter() {
                if item.is_final(grammar) { continue }
                let rule = item.get_rule(grammar);
                for (rule_index, _r1) in rules.iter()
                    .enumerate()
                    .filter(|(_, r)| r.left == rule.right[item.dot_index]) {
                    additions.push(LR0Item { rule_index, dot_index: 0 });
                }
            }
            let prev_size = items.len();
            items.extend(additions.drain(0..));
            if prev_size == items.len() {
                break;
            }
        }
    }
}