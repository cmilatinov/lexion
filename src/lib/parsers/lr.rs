use itertools::Itertools;
use prettytable::{Cell, Row, Table};
use crate::lib::error::SyntaxError;
use crate::lib::grammar::{Derivation, DerivationNode, Grammar};
use crate::lib::parsers::{ParseTableAction, ParseTableLR};
use crate::lib::tokenizer::{TokenInstance, Tokenizer};
use crate::lib::tokenizer::tokens::{EOF, EPSILON};

pub trait GrammarParserLR {
    fn get_parse_table(&self) -> &ParseTableLR;

    fn parse_from_file(
        &self,
        grammar: &Grammar,
        file: &str,
    ) -> Derivation {
        let mut tokenizer = Tokenizer::from_file(file, grammar.get_token_types());
        self.parse(grammar, &mut tokenizer)
    }

    fn parse_from_file_trace(
        &self,
        grammar: &Grammar,
        file: &str,
        trace: Option<&mut Table>
    ) -> Derivation {
        let mut tokenizer = Tokenizer::from_file(file, grammar.get_token_types());
        self.parse_trace(grammar, &mut tokenizer, trace)
    }

    fn parse_from_string(
        &self,
        grammar: &Grammar,
        string: &str,
    ) -> Derivation {
        let mut tokenizer = Tokenizer::from_string(string, grammar.get_token_types());
        self.parse(grammar, &mut tokenizer)
    }

    fn parse_from_string_trace(
        &self,
        grammar: &Grammar,
        string: &str,
        trace: Option<&mut Table>
    ) -> Derivation {
        let mut tokenizer = Tokenizer::from_string(string, grammar.get_token_types());
        self.parse_trace(grammar, &mut tokenizer, trace)
    }

    fn parse(
        &self,
        grammar: &Grammar,
        tokenizer: &mut Tokenizer,
    ) -> Derivation {
        self.parse_trace(grammar, tokenizer, None)
    }

    fn parse_trace(
        &self,
        grammar: &Grammar,
        tokenizer: &mut Tokenizer,
        mut parse_trace: Option<&mut Table>,
    ) -> Derivation {
        enum StackItem {
            State(usize),
            Node(Box<DerivationNode>),
        }

        let table = self.get_parse_table();
        let mut stack: Vec<StackItem> = vec![StackItem::State(0)];
        let mut lookahead = tokenizer.next()?;
        let mut step = 0;

        while stack.len() > 0 {
            step += 1;
            let element = &stack[stack.len() - 1];
            let action = match element {
                StackItem::State(i) => table.get_action(*i, &*lookahead.token),
                StackItem::Node(node) => {
                    if let StackItem::State(i) = stack.iter()
                        .rev()
                        .find(|i| if let StackItem::State(_) = *i { true } else { false })
                        .unwrap() {
                        table.get_action(*i, &*node.get_rule(grammar).left)
                    } else { ParseTableAction::Reject }
                }
            };
            if parse_trace.is_some() {
                let trace = &mut **parse_trace.as_mut().unwrap();
                trace.add_row(
                    Row::new(vec![
                        Cell::new(&*format!("{}", step)).style_spec("cFc"),
                        Cell::new(&*format!(
                            "[{}]",
                            stack.iter()
                                .map(|v| match v {
                                    StackItem::State(s) => format!("{}", s),
                                    StackItem::Node(node) => format!("{}", node.token.value)
                                })
                                .intersperse(String::from(", "))
                                .collect::<String>())),
                        Cell::new(&*lookahead.token).style_spec("c"),
                        Cell::new(&*format!("{}", action)).style_spec("c"),
                    ])
                );
            }
            match action {
                ParseTableAction::Accept => {
                    if let StackItem::Node(node) = &stack[stack.len() - 2] {
                        return Ok(node.clone());
                    }
                }
                ParseTableAction::Goto(state) => {
                    stack.push(StackItem::State(state));
                }
                ParseTableAction::Shift(state) => {
                    stack.push(StackItem::Node(Box::new(DerivationNode::from_token(lookahead.clone()))));
                    stack.push(StackItem::State(state));
                    lookahead = tokenizer.next()?;
                }
                ParseTableAction::Reduce(rule_index) => {
                    let rule = grammar.get_rule(rule_index);
                    let mut num_right =
                        if rule.right == vec![String::from(EPSILON)] { 0 } else { rule.right.len() };
                    num_right *= 2;
                    let children: Vec<Box<DerivationNode>> = stack.drain((stack.len() - num_right)..)
                        .enumerate()
                        .filter(|(i, _)| i % 2 == 0)
                        .map(|(_, v)| {
                            if let StackItem::Node(node) = v {
                                return node;
                            }
                            Box::new(DerivationNode::new())
                        })
                        .collect();
                    stack.push(StackItem::Node(
                        Box::new(DerivationNode::from(
                            TokenInstance::from(
                                &*rule.left,
                                &*rule.left,
                                if !children.is_empty() { &children[0].token.loc } else { &lookahead.loc },
                            ),
                            children,
                            rule_index,
                        ))
                    ));
                }
                ParseTableAction::Reject => {
                    return Err(SyntaxError {
                        loc: lookahead.loc,
                        message:
                            if &*lookahead.value == EOF { String::from("unexpected end of input") }
                            else { format!("unexpected token '{}'", lookahead.value) },
                    });
                }
            };
        };

        Err(SyntaxError {
            loc: tokenizer.get_cursor_loc(),
            message: String::from(""),
        })
    }
}
