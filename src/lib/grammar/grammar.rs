use std::collections::HashSet;
use std::fmt::{Display, Formatter, Result};
use lazy_static::lazy_static;
use regex::Regex;
use crate::lib::tokenizer::*;
use crate::lib::tokenizer::tokens::*;

lazy_static! {
    pub static ref TERMINAL: Regex = Regex::new(r"^'.*'$").unwrap();
}

#[derive(Clone)]
pub struct GrammarRule {
    pub left: String,
    pub right: Vec<String>,
}

pub struct Grammar {
    pub start_symbol: String,
    pub rules: Vec<GrammarRule>,
    pub terminal_rules: Vec<GrammarRule>,
    pub non_terminals: HashSet<String>,
    pub terminals: HashSet<String>,
}

impl Display for GrammarRule {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{} -> {}", self.left, self.right.join(" "))
    }
}

impl Display for Grammar {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(
            f,
            concat!(
            "Grammar {{\n",
            "  {}\n",
            "  Rules {{\n",
            "{}\n",
            "  }}\n",
            "  Non-terminals {{\n",
            "{}\n",
            "  }}\n",
            "  Tokens {{\n",
            "{}\n",
            "  }}\n",
            "}}"
            ),
            self.start_symbol,
            self.rules.iter()
                .map(|r| format!("    {}", r))
                .collect::<Vec<String>>()
                .join("\n"),
            self.non_terminals.iter()
                .map(|t| format!("    {}", t))
                .collect::<Vec<String>>()
                .join("\n"),
            self.get_token_types().iter()
                .filter(|t| !t.name.is_empty())
                .map(|t| format!("    {}", t))
                .collect::<Vec<String>>()
                .join("\n")
        )
    }
}

impl Grammar {
    pub fn from_rules(rules: Vec<GrammarRule>) -> Grammar {
        type StringSet = HashSet<String>;

        let mut start_symbol: String = String::from("");
        let mut terminals: StringSet = HashSet::new();
        let mut non_terminals: StringSet = HashSet::new();
        let mut grammar_rules: Vec<GrammarRule> = Vec::new();
        let mut terminal_rules: Vec<GrammarRule> = Vec::new();
        let mut iter = rules.into_iter().fuse();
        let mut elem = iter.next();

        while elem.is_some() {
            let rule = elem.unwrap();
            if Grammar::is_terminal(&rule.left) {
                terminal_rules.push(rule);
                elem = iter.next();
                continue;
            }

            if start_symbol.is_empty() {
                start_symbol = rule.left.clone();
            }

            non_terminals.insert(rule.left.clone());
            for symbol in &rule.right {
                if Grammar::is_terminal(&symbol) {
                    terminals.insert(symbol.clone());
                } else {
                    non_terminals.insert(symbol.clone());
                }
            }

            grammar_rules.push(rule);
            elem = iter.next();
        }

        Grammar {
            start_symbol,
            rules: grammar_rules,
            terminal_rules,
            terminals,
            non_terminals,
        }
    }
}

impl Grammar {
    pub fn is_terminal(symbol: &String) -> bool {
        symbol == EOF || symbol == EPSILON || TERMINAL.is_match(symbol.as_str())
    }

    pub fn is_non_terminal(symbol: &String) -> bool {
        !Grammar::is_terminal(symbol)
    }

    pub fn stringify(symbol: &String) -> String {
        if symbol == EOF || symbol == EPSILON || !Grammar::is_terminal(symbol) {
            return symbol.clone();
        }
        String::from(&symbol.as_str()[1..symbol.len() - 1])
    }
}

impl Grammar {
    pub fn get_token_types(&self) -> Vec<TokenType> {
        let mut token_types = vec![
            TokenType { name: String::from(""), regex: WHITESPACE.clone() },
            TokenType { name: String::from(""), regex: SINGLE_LINE_COMMENT.clone() },
            TokenType { name: String::from(""), regex: MULTI_LINE_COMMENT.clone() },
        ];
        token_types.extend(
            self.terminals.iter()
                .map(|t| {
                    let rule = self.terminal_rules.iter().find(|r| r.left == *t);
                    let regex = match rule {
                        Some(r) => r.right[0].clone(),
                        None => Grammar::stringify(t)
                    };
                    TokenType {
                        name: String::from(t),
                        regex: Regex::new(format!("^{}", regex).as_str()).unwrap(),
                    }
                })
        );
        token_types
    }
}