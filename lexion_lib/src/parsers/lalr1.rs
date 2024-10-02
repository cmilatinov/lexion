use crate::grammar::Grammar;
use crate::parsers::items::{CanonicalCollectionGraph, GraphState, LR0Item, LRItem};
use crate::parsers::ParseTableLR;
use crate::tokenizer::tokens::{EOF, EPSILON};
use std::collections::{HashMap, HashSet};

pub struct GrammarParserLALR1 {
    #[allow(dead_code)]
    collection: CanonicalCollectionGraph<LR0Item>,
    #[allow(dead_code)]
    lookahead_sets: HashMap<(usize, LR0Item), HashSet<String>>,
    table: ParseTableLR,
}

struct SetConstructorLALR1<'a> {
    grammar: &'a Grammar,
    collection: &'a CanonicalCollectionGraph<LR0Item>,
    lookaheads: HashSet<String>,
    tm: HashSet<GraphState<LR0Item>>,
    visited: HashSet<(String, GraphState<LR0Item>)>,
}

impl<'a> SetConstructorLALR1<'a> {
    fn from_grammar(
        grammar: &'a Grammar,
        collection: &'a CanonicalCollectionGraph<LR0Item>,
    ) -> Self {
        Self {
            grammar,
            collection,
            lookaheads: HashSet::new(),
            tm: HashSet::new(),
            visited: HashSet::new(),
        }
    }

    fn goto(&self, state: &GraphState<LR0Item>, symbol: &str) -> GraphState<LR0Item> {
        let mut result = GraphState::new(state.goto(self.grammar, symbol));
        result.closure(self.grammar);
        result
    }

    fn pred(
        &self,
        state: &GraphState<LR0Item>,
        sequence: &[String],
    ) -> HashSet<GraphState<LR0Item>> {
        if sequence.len() == 0 || *sequence == vec![String::from(EPSILON)] {
            return HashSet::from([state.clone()]);
        }

        let mut pred = HashSet::new();
        let last = sequence.last().unwrap();
        for (_, s) in self.collection.states.iter() {
            let goto = self.goto(s, &**last);
            if goto.eq(state) {
                pred.extend(self.pred(s, &sequence[..sequence.len() - 1]).into_iter());
            }
        }
        pred
    }

    fn trans(&mut self, state: &GraphState<LR0Item>) {
        let state_index = self.collection.states.get_by_right(state);
        if let Some(index) = state_index {
            println!("trans({})", index);
        } else {
            println!("trans(-1)");
        }
        self.tm.insert(state.clone());
        if state.is_accept(self.grammar) {
            self.lookaheads.insert(String::from(EOF));
            return;
        }
        for i in state
            .get_items()
            .iter()
            .filter(|i| i.dot_index < i.get_rule(self.grammar).right.len())
        {
            let rule = i.get_rule(self.grammar);
            let x = rule.right[i.dot_index].as_str();
            if x != EPSILON && Grammar::is_terminal(x) {
                self.lookaheads.insert(String::from(x));
            } else if self.grammar.is_nullable(x) {
                let goto = self.goto(state, x);
                if self.tm.contains(&goto) {
                    self.trans(&goto);
                }
            }
        }
    }

    fn lalr(&mut self, item: &LR0Item, state: &GraphState<LR0Item>) {
        let rule = item.get_rule(self.grammar);
        let a = &*rule.left;
        let alpha = &rule.right[0..item.dot_index];

        println!(
            "lalr([{}], {})",
            item.to_string(self.grammar),
            self.collection.states.get_by_right(state).unwrap()
        );

        let visited = self.visited.clone();
        for s in self
            .pred(state, alpha)
            .iter()
            .filter(|s| !visited.contains(&(String::from(a), (*s).clone())))
        {
            self.visited.insert((String::from(a), s.clone()));
            self.trans(&self.goto(&s, a));
            for i in s.get_items().iter().filter(|i| {
                let i_rule = i.get_rule(self.grammar);
                i_rule.right[i.dot_index] == rule.left
                    && self
                        .grammar
                        .is_nullable_sequence(&i_rule.right[i.dot_index + 1..])
            }) {
                self.lalr(i, s);
            }
        }
    }

    fn lalr1(&mut self, item: &LR0Item, state: &GraphState<LR0Item>) {
        let rule = item.get_rule(self.grammar);
        if self.grammar.get_augmented_start_symbol() == rule.left {
            return;
        }

        self.lalr(item, state);
    }
}

impl GrammarParserLALR1 {
    pub fn from_grammar(grammar: &Grammar) -> Self {
        let collection = CanonicalCollectionGraph::new(grammar, LR0Item::new(0, 0));
        let mut lookahead_sets = HashMap::new();

        for (index, state) in collection
            .states
            .iter()
            .filter(|(_, s)| s.is_final(grammar))
        {
            for item in state.get_items().iter().filter(|i| i.is_final(grammar)) {
                let mut constructor = SetConstructorLALR1::from_grammar(grammar, &collection);
                constructor.lalr1(item, state);
                lookahead_sets.insert((*index, *item), constructor.lookaheads);
            }
        }

        let empty = HashSet::new();
        let table = ParseTableLR::from_collection(grammar, &collection, |i, _, si| {
            lookahead_sets.get(&(si, *i)).unwrap_or(&empty)
        });

        Self {
            collection,
            table,
            lookahead_sets,
        }
    }
}

impl GrammarParserLALR1 {
    pub fn get_parse_table(&self) -> &ParseTableLR {
        &self.table
    }
}
