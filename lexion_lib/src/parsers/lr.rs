use itertools::Itertools;
use petgraph::Graph;
use petgraph::graph::NodeIndex;
use prettytable::{Cell, row, Row, Table};

use crate::error::SyntaxError;
use crate::grammar::{Derivation, DerivationNode, Grammar};
use crate::parsers::{ParseTableAction, ParseTableLR};
use crate::tokenizer::{SourceRange, TokenInstance, Tokenizer};
use crate::tokenizer::tokens::{EOF, EPSILON};

pub type DerivationResult = Result<Derivation, SyntaxError>;

pub trait GrammarParserLR {
    fn get_parse_table(&self) -> &ParseTableLR;

    fn parse_from_file(&self, grammar: &Grammar, file: &'static str) -> DerivationResult {
        let mut tokenizer = Tokenizer::from_file(file, grammar.get_token_types());
        self.parse(grammar, &mut tokenizer)
    }

    fn parse_from_file_trace(
        &self,
        grammar: &Grammar,
        file: &'static str,
        trace: Option<&mut Table>,
    ) -> DerivationResult {
        let mut tokenizer = Tokenizer::from_file(file, grammar.get_token_types());
        self.parse_trace(grammar, &mut tokenizer, trace)
    }

    fn parse_from_string(&self, grammar: &Grammar, string: &str) -> DerivationResult {
        let mut tokenizer = Tokenizer::from_string(string, grammar.get_token_types());
        self.parse(grammar, &mut tokenizer)
    }

    fn parse_from_string_trace(
        &self,
        grammar: &Grammar,
        string: &str,
        trace: Option<&mut Table>,
    ) -> DerivationResult {
        let mut tokenizer = Tokenizer::from_string(string, grammar.get_token_types());
        self.parse_trace(grammar, &mut tokenizer, trace)
    }

    fn parse(&self, grammar: &Grammar, tokenizer: &mut Tokenizer) -> DerivationResult {
        self.parse_trace(grammar, tokenizer, None)
    }

    fn parse_trace(
        &self,
        grammar: &Grammar,
        tokenizer: &mut Tokenizer,
        mut parse_trace: Option<&mut Table>,
    ) -> DerivationResult {
        enum StackItem {
            State(usize),
            Node(NodeIndex),
        }

        let mut graph: Graph<DerivationNode, usize> = Graph::new();
        let table = self.get_parse_table();
        let mut stack: Vec<StackItem> = vec![StackItem::State(0)];
        let mut lookahead = tokenizer.next()?;
        let mut step = 0;

        if let Some(trace) = parse_trace.as_mut() {
            trace.set_titles(row![cFyb => "Step", "Stack", "Lookahead", "Action"]);
        }

        while stack.len() > 0 {
            step += 1;
            let element = &stack[stack.len() - 1];
            let action = match element {
                StackItem::State(i) => table.get_action(*i, &*lookahead.token),
                StackItem::Node(id) => {
                    let node = graph.node_weight(*id).unwrap();
                    if let StackItem::State(i) = stack
                        .iter()
                        .rev()
                        .find(|i| {
                            if let StackItem::State(_) = *i {
                                true
                            } else {
                                false
                            }
                        })
                        .unwrap()
                    {
                        table.get_action(*i, &*node.get_rule(grammar).left)
                    } else {
                        ParseTableAction::Reject
                    }
                }
            };
            if let Some(trace) = parse_trace.as_mut() {
                trace.add_row(Row::new(vec![
                    Cell::new(&*format!("{}", step)).style_spec("cFc"),
                    Cell::new(&*format!(
                        "[{}]",
                        stack
                            .iter()
                            .map(|v| match v {
                                StackItem::State(s) => format!("{}", s),
                                StackItem::Node(id) => {
                                    let node = graph.node_weight(*id).unwrap();
                                    format!("{}", node.token.value)
                                }
                            })
                            .intersperse(String::from(", "))
                            .collect::<String>()
                    )),
                    Cell::new(&*lookahead.token).style_spec("c"),
                    Cell::new(&*format!("{}", action)).style_spec("c"),
                ]));
            }
            match action {
                ParseTableAction::Accept => {
                    if let StackItem::Node(id, ..) = stack[stack.len() - 2] {
                        return Ok(Derivation(id, graph));
                    }
                }
                ParseTableAction::Goto(state) => {
                    stack.push(StackItem::State(state));
                }
                ParseTableAction::Shift(state) => {
                    let id = graph.add_node(DerivationNode::from_token(lookahead.clone()));
                    stack.push(StackItem::Node(id));
                    stack.push(StackItem::State(state));
                    lookahead = tokenizer.next()?;
                }
                ParseTableAction::Reduce(rule_index) => {
                    let rule = grammar.get_rule(rule_index);
                    let num_children = if rule.right == vec![String::from(EPSILON)] {
                        0
                    } else {
                        rule.right.len()
                    };
                    let num_right = num_children * 2;
                    let node_id = graph.add_node(DerivationNode::from(
                        TokenInstance::from(&*rule.left, &*rule.left, &lookahead.loc),
                        rule_index,
                    ));
                    for child_id in stack
                        .drain((stack.len() - num_right)..)
                        .enumerate()
                        .filter(|(i, _)| i % 2 == 0)
                        .map(|(_, v)| {
                            if let StackItem::Node(id) = v {
                                return id;
                            }
                            unreachable!()
                        })
                    {
                        graph.add_edge(node_id, child_id, graph.edges(node_id).count());
                    }
                    stack.push(StackItem::Node(node_id));
                }
                ParseTableAction::Reject => {
                    return Err(SyntaxError {
                        range: SourceRange::from_loc_len(lookahead.loc, lookahead.value.len()),
                        message: if &*lookahead.value == EOF {
                            String::from("unexpected end of input")
                        } else {
                            format!("unexpected token '{}'", lookahead.value)
                        },
                    });
                }
            };
        }

        Err(SyntaxError {
            range: tokenizer.get_cursor_loc().into(),
            message: String::from("unexpected end of input"),
        })
    }
}
