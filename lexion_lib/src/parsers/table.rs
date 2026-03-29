use crate::grammar::Grammar;
use crate::parsers::items::{CanonicalCollectionGraph, ClosurableItem, GraphState, LRItem};
use crate::tokenizer::tokens::EOF;
use itertools::Itertools;
use petgraph::prelude::EdgeRef;
use petgraph::visit::{IntoEdges, Walker};

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::hash::Hash;
use std::str::FromStr;
use tabled::builder::Builder;
use tabled::settings::object::Cell;
use tabled::settings::themes::BorderCorrection;
use tabled::settings::{Alignment, Color, Span, Style};
use tabled::Table;

#[derive(Clone, Serialize, Deserialize)]
pub enum ParseTableAction {
    Shift(usize),
    Goto(usize),
    Reduce(usize),
    Accept,
    Reject,
    Conflict(Vec<ParseTableAction>),
}

impl ParseTableAction {
    pub fn is_shift(&self) -> bool {
        matches!(self, ParseTableAction::Shift(_))
    }

    pub fn is_reduce(&self) -> bool {
        matches!(self, ParseTableAction::Reduce(_))
    }
}

#[derive(Clone)]
pub struct ParseTableOverride {
    pub state: usize,
    pub symbol: &'static str,
    pub action: ParseTableAction,
}

impl FromStr for ParseTableAction {
    type Err = ();

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        if let Some(stripped) = string.strip_prefix("r") {
            let state: usize = stripped.parse().map_err(|_| {})?;
            Ok(ParseTableAction::Reduce(state))
        } else if let Some(stripped) = string.strip_prefix("s") {
            let state: usize = stripped.parse().map_err(|_| {})?;
            Ok(ParseTableAction::Shift(state))
        } else if string == "acc" {
            Ok(ParseTableAction::Accept)
        } else if string == "rej" {
            Ok(ParseTableAction::Reject)
        } else if let Ok(state) = string.parse().map_err(|_| {}) {
            Ok(ParseTableAction::Goto(state))
        } else {
            Err(())
        }
    }
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
            match self {
                ParseTableAction::Shift(i) => format!("s{i}"),
                ParseTableAction::Goto(i) => format!("{i}"),
                ParseTableAction::Reduce(i) => format!("r{i}"),
                ParseTableAction::Accept => String::from("acc"),
                ParseTableAction::Reject => String::from(""),
                ParseTableAction::Conflict(actions) => actions.iter().join(" / "),
            }
        )
    }
}

impl ParseTableLR {
    pub fn from_collection<
        'a,
        T: Eq + Hash + LRItem + Clone + ClosurableItem<T>,
        F: Fn(&T, &GraphState<T>, usize) -> &'a HashSet<String>,
    >(
        grammar: &Grammar,
        collection: &CanonicalCollectionGraph<T>,
        reduce_terminal_set_fn: F,
    ) -> ParseTableLR {
        let mut table = ParseTableLR {
            table: HashMap::new(),
            num_states: collection.node_count(),
        };
        for index in collection.node_indices() {
            let state = &collection[index];
            if state.is_accept(grammar) {
                // Accept
                table.insert_entry(index.index(), EOF, ParseTableAction::Accept);
            } else if state.is_final(grammar) {
                // Reduce
                for final_item in state.get_items().iter().filter(|i| i.is_final(grammar)) {
                    for terminal in reduce_terminal_set_fn(final_item, state, index.index()).iter()
                    {
                        table.insert_entry(
                            index.index(),
                            terminal,
                            ParseTableAction::Reduce(final_item.get_rule_index()),
                        );
                    }
                }
            }

            // Shift / Goto
            for edge in collection.edges(index) {
                let weight = edge.weight();
                table.insert_entry(
                    index.index(),
                    &weight.symbol,
                    if Grammar::is_terminal(&weight.symbol) {
                        ParseTableAction::Shift(edge.target().index())
                    } else {
                        ParseTableAction::Goto(edge.target().index())
                    },
                );
            }
        }
        table
    }
}

impl ParseTableLR {
    pub fn apply_conflict_resolutions<'a>(
        &'a mut self,
        overrides: impl Iterator<Item = &'a ParseTableOverride>,
    ) {
        // Resolve S/R or R/R conflicts
        for o in overrides {
            self.insert_entry(o.state, o.symbol, o.action.clone());
        }
    }

    pub fn insert_entry(&mut self, state_index: usize, symbol: &str, action: ParseTableAction) {
        match self.table.get_mut(symbol) {
            Some(v) => {
                if let Some(old_action) = v.remove(&state_index) {
                    if let ParseTableAction::Conflict(mut actions) = old_action {
                        actions.push(action);
                        v.insert(state_index, ParseTableAction::Conflict(actions));
                    } else {
                        let new_action = ParseTableAction::Conflict(vec![old_action, action]);
                        println!("{state_index} {symbol} -> {new_action}");
                        v.insert(state_index, new_action);
                    }
                } else {
                    v.insert(state_index, action);
                }
            }
            None => {
                self.table
                    .insert(String::from(symbol), HashMap::from([(state_index, action)]));
            }
        };
    }

    pub fn entries(&self) -> impl Iterator<Item = (usize, &str, &ParseTableAction)> {
        self.table
            .iter()
            .flat_map(|(symbol, states)| states.iter().map(move |(s, a)| (*s, symbol.as_str(), a)))
    }

    pub fn get_action(&self, state_index: usize, symbol: &str) -> Cow<ParseTableAction> {
        match self.table.get(symbol) {
            Some(v) => match v.get(&state_index) {
                Some(ParseTableAction::Conflict(actions)) => {
                    Cow::Borrowed(actions.last().expect("conflict action with empty list"))
                }
                Some(a) => Cow::Borrowed(a),
                None => Cow::Owned(ParseTableAction::Reject),
            },
            None => Cow::Owned(ParseTableAction::Reject),
        }
    }

    pub fn to_table(&self) -> Table {
        let mut symbols: Vec<String> = self.table.keys().cloned().collect();
        symbols.sort_by(|a, b| {
            let rank = |s: &str| {
                if s == EOF {
                    1
                } else if Grammar::is_terminal(s) {
                    0
                } else {
                    2
                }
            };
            rank(a).cmp(&rank(b))
        });

        let num_terminals = symbols.iter().filter(|s| Grammar::is_terminal(s)).count();
        let num_non_terminals = symbols
            .iter()
            .filter(|s| Grammar::is_non_terminal(s))
            .count();
        let mut builder = Builder::default();

        let header_iter: Vec<String> = std::iter::once(String::from("\\"))
            .chain(symbols.iter().cloned())
            .collect();
        let mut span_row = vec![String::new(); symbols.len() + 1];
        span_row[1] = String::from("Action");
        span_row[1 + num_terminals] = String::from("Goto");
        builder.push_record(span_row);
        builder.push_record(header_iter);

        // data rows
        let mut cell_colors: Vec<(usize, usize, Color)> = vec![];
        for state_index in 0..self.num_states {
            let mut row: Vec<String> = vec![state_index.to_string()];
            for (col, symbol) in symbols.iter().enumerate() {
                let action = self.get_action(state_index, symbol);
                row.push(action.to_string());
                let color = match action.as_ref() {
                    ParseTableAction::Accept => Some(Color::FG_GREEN),
                    ParseTableAction::Shift(_) => Some(Color::FG_BRIGHT_BLUE),
                    ParseTableAction::Reduce(_) => Some(Color::FG_BRIGHT_MAGENTA),
                    ParseTableAction::Goto(_) => Some(Color::FG_CYAN),
                    ParseTableAction::Conflict(_) => Some(Color::FG_RED),
                    _ => None,
                };
                if let Some(c) = color {
                    cell_colors.push((state_index, col + 1, c));
                }
            }
            builder.push_record(row);
        }

        let mut table = builder.build();
        table.with(Style::modern());
        table.with(Alignment::center());
        table.modify((0, 1), Span::column(num_terminals as isize));
        table.modify(
            (0, 1 + num_terminals),
            Span::column(num_non_terminals as isize),
        );
        table.with(BorderCorrection::span());
        for (row, col, color) in cell_colors {
            table.modify(Cell::new(row + 2, col), color);
        }
        table
    }
}
