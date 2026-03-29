use std::borrow::Cow;
use std::sync::Arc;

use itertools::Itertools;
use petgraph::graph::NodeIndex;
use petgraph::Graph;
use tabled::builder::Builder;

use crate::error::{ParseError, SyntaxError};
use crate::grammar::{Derivation, DerivationNode, Grammar};
use crate::parsers::{ParseTableAction, ParseTableLR};
use crate::tokenizer::tokens::{EOF, EPSILON};
use crate::tokenizer::{TokenInstance, Tokenizer};

pub type DerivationResult = Result<Derivation, ParseError>;

pub trait GrammarParserLR {
    fn get_parse_table(&self) -> &ParseTableLR;

    fn parse_from_file(&self, grammar: &Grammar, file: &'static str) -> DerivationResult {
        let tokenizer = Tokenizer::from_file(file, grammar.get_token_types())?;
        self.parse(grammar, tokenizer)
    }

    fn parse_from_file_trace(
        &self,
        grammar: &Grammar,
        file: &'static str,
        trace: Option<&mut Builder>,
    ) -> DerivationResult {
        let tokenizer = Tokenizer::from_file(file, grammar.get_token_types())?;
        self.parse_trace(grammar, tokenizer, trace)
    }

    fn parse_from_string(&self, grammar: &Grammar, string: Arc<String>) -> DerivationResult {
        let tokenizer = Tokenizer::from_string(string, grammar.get_token_types());
        self.parse(grammar, tokenizer)
    }

    fn parse_from_string_trace(
        &self,
        grammar: &Grammar,
        string: Arc<String>,
        trace: Option<&mut Builder>,
    ) -> DerivationResult {
        let tokenizer = Tokenizer::from_string(string, grammar.get_token_types());
        self.parse_trace(grammar, tokenizer, trace)
    }

    fn parse(&self, grammar: &Grammar, tokenizer: Tokenizer) -> DerivationResult {
        self.parse_trace(grammar, tokenizer, None)
    }

    fn parse_trace(
        &self,
        grammar: &Grammar,
        mut tokenizer: Tokenizer,
        mut trace: Option<&mut Builder>,
    ) -> DerivationResult {
        enum StackItem {
            State(usize),
            Node(NodeIndex),
        }

        let mut graph: Graph<DerivationNode, usize> = Graph::new();
        let table = self.get_parse_table();
        let mut stack: Vec<StackItem> = vec![StackItem::State(0)];
        let mut lookahead = tokenizer.next_token()?;
        let mut step = 0;

        if let Some(trace) = trace.as_mut() {
            trace.push_record(["Step", "Stack", "Lookahead", "Action"]);
        }

        while !stack.is_empty() {
            step += 1;
            let element = &stack[stack.len() - 1];
            let action = match element {
                StackItem::State(i) => table.get_action(*i, &lookahead.token),
                StackItem::Node(id) => {
                    let node = graph.node_weight(*id).unwrap();
                    if let StackItem::State(i) = stack
                        .iter()
                        .rev()
                        .find(|i| matches!(i, StackItem::State(_)))
                        .unwrap()
                    {
                        table.get_action(*i, &node.get_rule(grammar).left)
                    } else {
                        Cow::Owned(ParseTableAction::Reject)
                    }
                }
            };
            if let Some(trace) = trace.as_mut() {
                trace.push_record([
                    step.to_string(),
                    format!(
                        "[{}]",
                        stack
                            .iter()
                            .map(|v| match v {
                                StackItem::State(s) => s.to_string(),
                                StackItem::Node(id) => {
                                    let node = graph.node_weight(*id).unwrap();
                                    node.token.value.clone()
                                }
                            })
                            .intersperse(String::from(", "))
                            .collect::<String>()
                    ),
                    lookahead.token.clone(),
                    action.to_string(),
                ]);
            }
            match action.as_ref() {
                ParseTableAction::Conflict(_) => {
                    return Err(SyntaxError {
                        src: tokenizer.source(),
                        span: lookahead.span,
                        message: format!("conflicting action '{}' in parse table", action.as_ref()),
                    }
                    .into());
                }
                ParseTableAction::Accept => {
                    if let StackItem::Node(root, ..) = stack[stack.len() - 2] {
                        return Ok(Derivation { graph, root });
                    }
                }
                ParseTableAction::Goto(state) => {
                    stack.push(StackItem::State(*state));
                }
                ParseTableAction::Shift(state) => {
                    let id = graph.add_node(DerivationNode::from_token(lookahead.clone()));
                    stack.push(StackItem::Node(id));
                    stack.push(StackItem::State(*state));
                    lookahead = tokenizer.next_token()?;
                }
                ParseTableAction::Reduce(rule_index) => {
                    let rule = grammar.get_rule(*rule_index);
                    let num_children = if rule.right == vec![String::from(EPSILON)] {
                        0
                    } else {
                        rule.right.len()
                    };
                    let num_right = num_children * 2;
                    let node_id = graph.add_node(DerivationNode::from(
                        TokenInstance::from(&rule.left, &rule.left, lookahead.span),
                        *rule_index,
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
                        src: tokenizer.source(),
                        span: lookahead.span,
                        message: if &*lookahead.value == EOF {
                            String::from("unexpected end of input")
                        } else {
                            format!("unexpected token '{}'", lookahead.value)
                        },
                    }
                    .into());
                }
            };
        }

        Err(SyntaxError {
            src: tokenizer.source(),
            span: tokenizer.cursor_offset().into(),
            message: String::from("unexpected end of input"),
        }
        .into())
    }
}
