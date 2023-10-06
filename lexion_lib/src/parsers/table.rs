use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::hash::Hash;
use prettytable::*;
use prettytable::color::{BRIGHT_BLUE, BRIGHT_MAGENTA, CYAN, GREEN};
use crate::grammar::Grammar;
use crate::parsers::items::{CanonicalCollectionGraph, ClosurableItem, GraphState, LRItem};
use crate::tokenizer::tokens::EOF;

#[derive(Clone)]
pub enum ParseTableAction {
    Shift(usize),
    Goto(usize),
    Reduce(usize),
    Accept,
    Reject,
}

pub struct ParseTableLR {
    table: HashMap<String, HashMap<usize, ParseTableAction>>,
    num_states: usize,
}

impl Display for ParseTableAction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match *self {
                ParseTableAction::Shift(i) => format!("s{}", i),
                ParseTableAction::Goto(i) => format!("{}", i),
                ParseTableAction::Reduce(i) => format!("r{}", i),
                ParseTableAction::Accept => String::from("acc"),
                ParseTableAction::Reject => String::from("")
            }
        )
    }
}

impl ParseTableLR {
    pub fn from_collection<'a, T: Eq + Hash + LRItem + Clone + ClosurableItem<T>, F: Fn(&T, &GraphState<T>, usize) -> &'a HashSet<String>>(
        grammar: &Grammar,
        collection: &CanonicalCollectionGraph<T>,
        reduce_terminal_set_fn: F,
    ) -> ParseTableLR {
        let mut table = ParseTableLR {
            table: HashMap::new(),
            num_states: collection.states.len(),
        };
        for (i, state) in collection.states.iter() {
            if state.is_accept(grammar) {
                // Accept
                table.insert_entry(*i, EOF, ParseTableAction::Accept);
            } else if state.is_final(grammar) {
                // Reduce
                for final_item in state.get_items().iter()
                    .filter(|i| i.is_final(grammar)) {
                    for terminal in reduce_terminal_set_fn(&final_item, state, *i)
                        .iter() {
                        table.insert_entry(
                            *i,
                            &*terminal,
                            ParseTableAction::Reduce(final_item.get_rule_index()),
                        );
                    }
                }
            }

            // Shift / Goto
            for edge in collection.edges.get(i)
                .unwrap_or(&Vec::new())
                .iter() {
                table.insert_entry(
                    *i, &*edge.symbol,
                    if Grammar::is_terminal(&*edge.symbol) {
                        ParseTableAction::Shift(edge.to)
                    } else {
                        ParseTableAction::Goto(edge.to)
                    },
                );
            }
        }
        table
    }
}

impl ParseTableLR {
    pub fn insert_entry(&mut self, state_index: usize, symbol: &str, action: ParseTableAction) {
        match self.table.get_mut(symbol) {
            Some(v) => { v.insert(state_index, action); }
            None => {
                self.table.insert(
                    String::from(symbol),
                    HashMap::from([(state_index, action)]),
                );
            }
        };
    }

    pub fn get_action(&self, state_index: usize, symbol: &str) -> ParseTableAction {
        match self.table.get(symbol) {
            Some(v) => match v.get(&state_index) {
                Some(a) => a.clone(),
                None => ParseTableAction::Reject
            },
            None => ParseTableAction::Reject
        }
    }

    pub fn to_prettytable(&self, table: &mut Table) {
        let mut symbols: Vec<String> = self.table.iter()
            .enumerate()
            .map(|(_, (k, _))| k.clone())
            .collect();
        symbols.sort_by(|a, b| {
            let na = if a == EOF { 1 }
            else if Grammar::is_terminal(a) { 0 }
            else { 2 };
            let nb = if b == EOF { 1 }
            else if Grammar::is_terminal(b) { 0 }
            else { 2 };
            let diff = na - nb;
            if diff < 0 { Ordering::Less }
            else if diff == 0 { Ordering::Equal }
            else { Ordering::Greater }
        });
        let num_terminals = symbols.iter()
            .filter(|s| Grammar::is_terminal(s))
            .count();
        let num_non_terminals = symbols.len() - num_terminals;
        let mut titles: Vec<Cell> = symbols.iter()
            .map(|s| Cell::new(s).style_spec("Fcbc"))
            .collect();
        titles.insert(0, Cell::new("State").style_spec("Fybc"));
        table.set_titles(Row::new(vec![
            Cell::new("\\").style_spec("Fybc"),
            Cell::new("Action").style_spec(&*format!("H{}Fybc", num_terminals)),
            Cell::new("Goto").style_spec(&*format!("H{}Fybc", num_non_terminals))
        ]));
        table.add_row(Row::new(titles));
        for state_index in 0..self.num_states {
            let mut actions = vec![Cell::new(""); symbols.len() + 1];
            actions[0] = Cell::new(&*format!("{}", state_index)).style_spec("Fcbc");
            for (symbol_index, symbol) in symbols.iter().enumerate() {
                let action = self.get_action(state_index, &**symbol);
                actions[symbol_index + 1] = Cell::new(action.to_string().as_str())
                    .style_spec("c");
                match action {
                    ParseTableAction::Accept => actions[symbol_index + 1]
                        .style(Attr::ForegroundColor(GREEN)),
                    ParseTableAction::Shift(_) => actions[symbol_index + 1]
                        .style(Attr::ForegroundColor(BRIGHT_BLUE)),
                    ParseTableAction::Reduce(_) => actions[symbol_index + 1]
                        .style(Attr::ForegroundColor(BRIGHT_MAGENTA)),
                    ParseTableAction::Goto(_) => actions[symbol_index + 1]
                        .style(Attr::ForegroundColor(CYAN)),
                    _ => {}
                }
            }
            table.add_row(Row::new(actions));
        }
    }
}