use std::collections::{HashMap, HashSet};
use crate::lib::grammar::{Grammar, GrammarRule};
use crate::lib::tokenizer::tokens::{EOF, EPSILON};

type ParseTableLL1<'t> = HashMap<String, HashMap<String, &'t GrammarRule>>;

pub struct GrammarParserLL1<'g> {
    is_ll1: bool,
    parse_table: ParseTableLL1<'g>
}

impl GrammarParserLL1<'_> {
    pub fn from_grammar(grammar: &Grammar) -> GrammarParserLL1 {
        let mut parser = GrammarParserLL1 {
            is_ll1: true,
            parse_table: HashMap::new()
        };
        parser.build_parse_table(grammar );
        parser
    }
}

impl<'a> GrammarParserLL1<'a> {
    fn build_parse_table(&mut self, grammar: &'a Grammar) {
        for rule in grammar.get_rules().iter() {
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
                    Some(m) => { m.insert(terminal.clone(), rule); },
                    None => {
                        let mut map = HashMap::new();
                        let prev = map.insert(terminal.clone(), rule);
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