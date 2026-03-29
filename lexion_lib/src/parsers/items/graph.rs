use crate::grammar::Grammar;
use crate::parsers::items::{ClosurableItem, LRItem};
use derive_more::{Deref, DerefMut};
use itertools::Itertools;
use petgraph::prelude::{EdgeRef, NodeIndex};
use petgraph::Graph;
use std::collections::BTreeSet;
use std::hash::Hash;

#[derive(Eq, PartialEq, Hash, Clone)]
pub struct GraphState<T: Eq + Hash + LRItem + Clone> {
    items: BTreeSet<T>,
    is_closured: bool,
}

pub struct GraphEdge {
    pub symbol: String,
}

#[derive(Deref, DerefMut)]
pub struct CanonicalCollectionGraph<T: Eq + Hash + LRItem + Clone> {
    graph: Graph<GraphState<T>, GraphEdge>,
}

impl<T: Eq + Hash + LRItem + Clone + ClosurableItem<T>> GraphState<T> {
    pub fn new(items: BTreeSet<T>) -> GraphState<T> {
        GraphState {
            items,
            is_closured: false,
        }
    }

    pub fn closure(&mut self, grammar: &Grammar) {
        if self.is_closured {
            return;
        }
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
        self.items
            .iter()
            .map(|i| i.to_string(grammar))
            .fold(String::new(), |a, b| a + &*b + "\n")
    }
}

impl<T: Eq + Ord + Hash + LRItem + Clone + ClosurableItem<T>> CanonicalCollectionGraph<T> {
    pub fn find_existing_state(&self, state: &GraphState<T>) -> Option<NodeIndex> {
        self.graph
            .node_indices()
            .find(|idx| &self.graph[*idx] == state)
    }

    pub fn goto(&self, state_index: NodeIndex, symbol: &str) -> Option<NodeIndex> {
        self.graph
            .edges(state_index)
            .find(|edge| &edge.weight().symbol == symbol)
            .map(|edge| edge.target())
    }

    pub fn new(grammar: &Grammar, initial_item: T) -> CanonicalCollectionGraph<T> {
        type StackItem<T> = (BTreeSet<T>, Option<NodeIndex>, String);
        let mut item_stack: Vec<StackItem<T>> =
            vec![(BTreeSet::from([initial_item]), None, String::from(""))];
        let mut graph: Graph<GraphState<T>, GraphEdge> = Graph::new();

        while let Some((item_set, prev_state_index, trans_symbol)) = item_stack.pop() {
            let mut state = GraphState::new(item_set);
            state.closure(grammar);

            let state_index: NodeIndex;
            let existing_state_index = graph.node_indices().find(|idx| &graph[*idx] == &state);
            if let Some(existing_state_index) = existing_state_index {
                state_index = existing_state_index;
            } else {
                state_index = graph.add_node(state.clone());
                item_stack.extend(
                    state
                        .items
                        .iter()
                        .filter(|i| !i.is_final(grammar))
                        .map(|i| i.get_rule(grammar).right[i.get_dot_index()].clone())
                        .collect::<BTreeSet<String>>()
                        .into_iter()
                        .map(|s| (state.goto(grammar, &s), Some(state_index), s)),
                );
            }

            if let Some(prev_state_index) = prev_state_index {
                graph.add_edge(
                    prev_state_index,
                    state_index,
                    GraphEdge {
                        symbol: trans_symbol.clone(),
                    },
                );
            }
        }

        CanonicalCollectionGraph { graph }
    }

    pub fn to_string(&self, grammar: &Grammar) -> String {
        let mut states = self
            .graph
            .node_indices()
            .map(|idx| {
                let state = &self.graph[idx];
                (
                    idx.index(),
                    format!(
                        "{}{}:\n{}",
                        idx.index(),
                        if state.is_accept(grammar) {
                            "**"
                        } else if state.is_final(grammar) {
                            "*"
                        } else {
                            ""
                        },
                        state
                            .items
                            .iter()
                            .map(|i| i.to_string(grammar))
                            .intersperse(String::from("\n"))
                            .collect::<String>()
                    ),
                )
            })
            .collect::<Vec<(usize, String)>>();
        states.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        states.iter().fold(String::new(), |a, b| a + &*b.1 + "\n\n")
    }
}
