use crate::grammar::Grammar;
use crate::parsers::items::{CanonicalCollectionGraph, LR0Item, LRItem};
use crate::parsers::{GrammarParserLR, ParseTableLR};
use crate::tokenizer::tokens::EOF;
use petgraph::prelude::{EdgeIndex, EdgeRef, NodeIndex};
use petgraph::visit::IntoEdgesDirected;
use petgraph::Direction;
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
}

#[derive(Hash, Eq, PartialEq, Clone, Copy)]
struct Transition(EdgeIndex);

impl Transition {
    fn state_symbol(&self, collection: &CanonicalCollectionGraph<LR0Item>) -> (NodeIndex, String) {
        let (source, _target) = collection.edge_endpoints(self.0).unwrap();
        (source, collection[self.0].symbol.clone())
    }
}

impl<'a> SetConstructorLALR1<'a> {
    fn from_grammar(
        grammar: &'a Grammar,
        collection: &'a CanonicalCollectionGraph<LR0Item>,
    ) -> Self {
        Self {
            grammar,
            collection,
        }
    }

    // DirectRead(q, A)
    fn direct_read(&self, transition: Transition) -> HashSet<String> {
        let (state_index, symbol) = transition.state_symbol(self.collection);
        let Some(r) = self.collection.goto(state_index, &symbol) else {
            return HashSet::new();
        };
        self.collection
            .edges_directed(r, Direction::Outgoing)
            .filter_map(|edge| {
                if Grammar::is_terminal(&edge.weight().symbol) {
                    Some(edge.weight().symbol.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    // Reads(q, A)
    fn reads_relation(&self, transition: Transition) -> Vec<Transition> {
        let (state_index, symbol) = transition.state_symbol(self.collection);
        let Some(r) = self.collection.goto(state_index, &symbol) else {
            return vec![];
        };
        self.collection
            .edges(r)
            .filter_map(|edge| {
                let symbol = &edge.weight().symbol;
                if Grammar::is_non_terminal(symbol) && self.grammar.is_nullable(symbol) {
                    Some(Transition(edge.id()))
                } else {
                    None
                }
            })
            .collect()
    }

    // Includes(q, A)
    fn includes_relation(&self, transition: Transition) -> Vec<Transition> {
        let mut relations = Vec::new();
        let (state_index, symbol) = transition.state_symbol(self.collection);
        for rule in self.grammar.get_rules().iter().skip(1) {
            for (i, rule_symbol) in rule.right.iter().enumerate() {
                let gamma = &rule.right[i + 1..];
                if &symbol != rule_symbol || !self.grammar.is_nullable_sequence(gamma) {
                    continue;
                }
                let beta = &rule.right[0..i];
                relations.extend(
                    self.trace_backwards(state_index, beta)
                        .into_iter()
                        .filter_map(|r| {
                            self.collection.edges(r).find_map(|edge| {
                                if edge.weight().symbol == rule.left {
                                    Some(Transition(edge.id()))
                                } else {
                                    None
                                }
                            })
                        }),
                );
            }
        }
        relations
    }

    fn trace_backwards(
        &self,
        current_state_index: NodeIndex,
        symbols: &[String],
    ) -> Vec<NodeIndex> {
        if symbols.is_empty() {
            return vec![current_state_index];
        }
        let last_symbol = &symbols[symbols.len() - 1];
        let remaining_symbols = &symbols[..symbols.len() - 1];
        self.collection
            .edges_directed(current_state_index, Direction::Incoming)
            .filter_map(|edge| {
                if &edge.weight().symbol == last_symbol {
                    Some(self.trace_backwards(edge.source(), remaining_symbols))
                } else {
                    None
                }
            })
            .flatten()
            .collect()
    }

    fn follow_sets(&self) -> HashMap<Transition, HashSet<String>> {
        let mut read_sets: HashMap<Transition, HashSet<String>> = self
            .collection
            .edge_indices()
            .filter_map(|edge| {
                let transition = Transition(edge);
                if Grammar::is_non_terminal(&self.collection[edge].symbol) {
                    Some((transition, self.direct_read(transition)))
                } else {
                    None
                }
            })
            .collect();

        self.propagate(&mut read_sets, |t| self.reads_relation(t));

        let mut follow_sets = read_sets.clone();
        let first_transition = Transition(
            self.collection
                .edges(NodeIndex::new(0))
                .find(|edge| edge.weight().symbol == self.grammar.get_start_symbol())
                .map(|edge| edge.id())
                .unwrap(),
        );
        follow_sets
            .entry(first_transition)
            .or_default()
            .insert(String::from(EOF));

        self.propagate(&mut follow_sets, |t| self.includes_relation(t));

        follow_sets
    }

    fn propagate<F>(&self, sets: &mut HashMap<Transition, HashSet<String>>, relation_fn: F)
    where
        F: Fn(Transition) -> Vec<Transition>,
    {
        let mut changed = true;
        while changed {
            changed = false;
            let keys: Vec<Transition> = sets.keys().cloned().collect();
            for (transition, related_transition) in keys.into_iter().flat_map(|t| {
                relation_fn(t)
                    .into_iter()
                    .map(move |related_t| (t, related_t))
            }) {
                if let Some(related_set) = sets.get(&related_transition).cloned() {
                    let current_set = sets.get_mut(&transition).unwrap();
                    let old_size = current_set.len();
                    current_set.extend(related_set);
                    if current_set.len() > old_size {
                        changed = true;
                    }
                }
            }
        }
    }

    fn final_state_items(&'a self) -> impl Iterator<Item = (NodeIndex, &'a LR0Item)> + 'a {
        self.collection
            .node_indices()
            .filter_map(move |idx| {
                let state = &self.collection[idx];
                if !state.is_final(self.grammar) {
                    return None;
                }
                Some(state.get_items().iter().filter_map(move |item| {
                    if item.is_final(self.grammar) {
                        Some((idx, item))
                    } else {
                        None
                    }
                }))
            })
            .flatten()
    }

    fn lookahead_sets(&self) -> HashMap<(usize, LR0Item), HashSet<String>> {
        let follow_sets = self.follow_sets();
        let mut lookahead_sets = HashMap::new();
        for (index, item) in self.final_state_items() {
            let rule = item.get_rule(self.grammar);
            // Trace back through the rule's RHS to find predecessor states,
            // then look up the goto transition for rule.left from each predecessor.
            let lookaheads: HashSet<String> = self
                .trace_backwards(index, &rule.right)
                .into_iter()
                .flat_map(|pred| {
                    self.collection
                        .edges(pred)
                        .filter_map(|edge| {
                            if edge.weight().symbol == rule.left {
                                follow_sets.get(&Transition(edge.id())).cloned()
                            } else {
                                None
                            }
                        })
                        .flatten()
                        .collect::<HashSet<String>>()
                })
                .collect();
            lookahead_sets.insert((index.index(), *item), lookaheads);
        }
        lookahead_sets
    }
}

impl GrammarParserLALR1 {
    pub fn from_grammar(grammar: &Grammar) -> Self {
        let collection = CanonicalCollectionGraph::new(grammar, LR0Item::new(0, 0));
        let lookahead_sets =
            SetConstructorLALR1::from_grammar(grammar, &collection).lookahead_sets();
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

impl GrammarParserLR for GrammarParserLALR1 {
    fn get_parse_table(&self) -> &ParseTableLR {
        &self.table
    }
}
