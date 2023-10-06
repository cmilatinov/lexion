use std::collections::{BTreeSet, HashMap, HashSet};
use std::hash::Hash;
use bimap::BiHashMap;
use itertools::Itertools;
use crate::grammar::Grammar;
use crate::parsers::items::{ClosurableItem, LRItem};

#[derive(Eq, PartialEq, Hash, Clone)]
pub struct GraphState<T: Eq + Hash + LRItem + Clone> {
    items: BTreeSet<T>,
    is_closured: bool,
}

pub struct GraphEdge {
    pub from: usize,
    pub to: usize,
    pub symbol: String,
}

pub struct CanonicalCollectionGraph<T: Eq + Hash + LRItem + Clone> {
    pub states: BiHashMap<usize, GraphState<T>>,
    pub edges: HashMap<usize, Vec<GraphEdge>>,
}

impl<T: Eq + Hash + LRItem + Clone + ClosurableItem<T>> GraphState<T> {
    pub fn new(items: BTreeSet<T>) -> GraphState<T> {
        GraphState {
            items,
            is_closured: false,
        }
    }

    pub fn closure(&mut self, grammar: &Grammar) {
        if self.is_closured { return; }
        T::closure(grammar, &mut self.items);
        self.is_closured = true;
    }

    pub fn goto(&self, grammar: &Grammar, symbol: &str) -> BTreeSet<T> {
        T::goto(grammar, &self.items, symbol)
    }

    pub fn is_closured(&self) -> bool {
        self.is_closured
    }

    pub fn is_final(&self, grammar: &Grammar) -> bool {
        self.items.iter().any(|i| i.is_final(grammar))
    }

    pub fn is_accept(&self, grammar: &Grammar) -> bool {
        self.items.iter().any(|i| i.is_accept(grammar))
    }

    pub fn get_items(&self) -> &BTreeSet<T> {
        &self.items
    }

    pub fn to_string(&self, grammar: &Grammar) -> String {
        self.items.iter()
            .map(|i| i.to_string(&grammar))
            .fold(String::new(), |a, b| a + &*b + "\n")
    }
}

impl<T: Eq + Ord + Hash + LRItem + Clone + ClosurableItem<T>> CanonicalCollectionGraph<T> {
    pub fn new(grammar: &Grammar, initial_item: T) -> CanonicalCollectionGraph<T> {
        type StackItem<T> = (BTreeSet<T>, Option<GraphState<T>>, String);
        let mut item_stack: Vec<StackItem<T>> = vec![(BTreeSet::from([initial_item]), None, String::from(""))];
        let mut states: BiHashMap<usize, GraphState<T>> = BiHashMap::new();
        let mut edges: HashMap<usize, Vec<GraphEdge>> = HashMap::new();
        let mut state_index = 0;

        while item_stack.len() > 0 {
            let (item_set, prev_state, trans_symbol) = item_stack.pop().unwrap();
            let mut state = GraphState::new(item_set);
            state.closure(grammar);

            let existing_state = states.get_by_right(&state);
            if existing_state.is_none() {
                states.insert(state_index, state.clone());
                state_index += 1;
                item_stack.extend(
                    state.items.iter()
                        .filter(|i| !i.is_final(grammar))
                        .map(|i| i.get_rule(grammar).right[i.get_dot_index()].clone())
                        .collect::<HashSet<String>>()
                        .iter()
                        .map(|s| (state.goto(grammar, &*s), Some(state.clone()), s.clone()))
                );
            }

            if prev_state.is_some() {
                let from = *states.get_by_right(&prev_state.unwrap()).unwrap();
                let to = *states.get_by_right(&state).unwrap();
                let edge = GraphEdge { from, to, symbol: trans_symbol.clone() };
                match edges.get_mut(&from) {
                    Some(v) => { v.push(edge); }
                    None => { edges.insert(from, vec![edge]); }
                }
            }
        }

        CanonicalCollectionGraph {
            states,
            edges,
        }
    }

    pub fn to_string(&self, grammar: &Grammar) -> String {
        let mut states = self.states.iter()
            .map(|(i, s)| {
                (*i, format!(
                    "{}{}:\n{}",
                    i,
                    if s.is_accept(grammar) { "**" }
                    else if s.is_final(grammar) { "*" }
                    else { "" },
                    s.items.iter()
                        .map(|i| i.to_string(grammar))
                        .intersperse(String::from("\n"))
                        .collect::<String>()
                ))
            })
            .collect::<Vec<(usize, String)>>();
        states.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        states.iter().fold(String::new(), |a, b| a + &*b.1 + "\n\n")
    }
}