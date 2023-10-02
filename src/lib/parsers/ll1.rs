use std::collections::{HashMap, HashSet};
use crate::lib::grammar::{Grammar};
use crate::lib::tokenizer::tokens::{EOF, EPSILON};

type ParseTableLL1 = HashMap<String, HashMap<String, usize>>;

pub struct GrammarParserLL1 {
    is_ll1: bool,
    parse_table: ParseTableLL1
}

impl GrammarParserLL1 {
    pub fn from_grammar(grammar: &Grammar) -> GrammarParserLL1 {
        let mut parser = GrammarParserLL1 {
            is_ll1: true,
            parse_table: HashMap::new()
        };
        parser.build_parse_table(grammar );
        parser
    }
}

impl GrammarParserLL1 {
    fn build_parse_table(&mut self, grammar: &Grammar) {
        for (i, rule) in grammar.get_rules().iter().enumerate() {
            let mut symbol_set: HashSet<String> = grammar.first_of(&*rule.right[0]).unwrap()
                .iter()
                .cloned()
                .collect();
            symbol_set.remove(EPSILON);
            if rule.right == vec![String::from(EOF)] {
                symbol_set.extend(
                    grammar.follow_of(&*rule.left).unwrap().iter().cloned()
                );
            }
            for terminal in grammar.first_of(&*rule.right[0]).unwrap().iter() {
                match self.parse_table.get_mut(&*rule.left) {
                    Some(m) => {
                        m.insert(terminal.clone(), i);
                    },
                    None => {
                        let mut map = HashMap::new();
                        let prev = map.insert(terminal.clone(), i);
                        if prev.is_some() {
                            self.is_ll1 = false;
                        }
                        self.parse_table.insert(rule.left.clone(), map);
                    }
                }
            }
        }
    }

    pub fn is_ll1(&self) -> bool {
        self.is_ll1
    }
}