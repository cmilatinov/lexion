use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter, Result};
use std::fs::File;
use itertools::Itertools;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use crate::tokenizer::*;
use crate::tokenizer::tokens::*;

lazy_static! {
    pub static ref TERMINAL: Regex = Regex::new(r"^'.*'$").unwrap();
}

#[derive(Deserialize, Serialize)]
pub struct GrammarRule {
    pub left: String,
    pub right: Vec<String>
}

impl PartialEq for GrammarRule {
    fn eq(&self, other: &Self) -> bool {
        self.left == other.left && self.right == other.right
    }
}

impl Eq for GrammarRule {}

type StringSet = HashSet<String>;
type StringSetMap = HashMap<String, HashSet<String>>;

pub struct Grammar {
    rules: Vec<GrammarRule>,
    terminal_rules: Vec<GrammarRule>,
    start_symbol: String,
    symbols: StringSet,
    non_terminals: StringSet,
    terminals: StringSet,
    first_sets: StringSetMap,
    follow_sets: StringSetMap,
    nullable_non_terminals: StringSet
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
                .intersperse(String::from("\n"))
                .collect::<String>(),
            self.non_terminals.iter()
                .map(|t| format!("    {}", t))
                .intersperse(String::from("\n"))
                .collect::<String>(),
            self.get_token_types().iter()
                .filter(|t| !t.name.is_empty())
                .map(|t| format!("    {}", t))
                .intersperse(String::from("\n"))
                .collect::<String>()
        )
    }
}

impl GrammarRule {
    pub fn to_jsmachine_string(&self) -> String {
        format!(
            "{} -> {}",
            self.left,
            self.right.iter()
                .map(|s| Grammar::stringify_jsmachine(s))
                .intersperse(String::from(" "))
                .collect::<String>()
        )
    }
}

impl Grammar {
    pub fn from_json_file(file: &str) -> serde_json::Result<Self> {
        let rules: Vec<GrammarRule> = serde_json::from_reader(File::open(file).unwrap())?;
        Ok(Grammar::from_rules(rules))
    }

    pub fn from_rules(rules: Vec<GrammarRule>) -> Self {
        let (terminal_rules, rules) = rules.into_iter()
            .partition(|r| Grammar::is_terminal(&r.left));
        let mut grammar = Grammar {
            rules,
            terminal_rules,
            start_symbol: String::from(""),
            symbols: HashSet::new(),
            terminals: HashSet::new(),
            non_terminals: HashSet::new(),
            first_sets: HashMap::new(),
            follow_sets: HashMap::new(),
            nullable_non_terminals: HashSet::new()
        };
        grammar.build_symbols();
        grammar.build_props();
        grammar.build_first_sets();
        grammar.build_follow_sets();
        grammar
    }
}

impl Grammar {
    pub fn is_terminal(symbol: &str) -> bool {
        symbol == EOF || symbol == EPSILON || TERMINAL.is_match(symbol)
    }

    pub fn is_non_terminal(symbol: &str) -> bool {
        !Grammar::is_terminal(symbol)
    }

    pub fn stringify(symbol: &str) -> String {
        if symbol == EOF || symbol == EPSILON || !Grammar::is_terminal(symbol) {
            return String::from(symbol);
        }
        String::from(&symbol[1..symbol.len() - 1])
    }

    pub fn stringify_jsmachine(symbol: &str) -> String {
        if symbol == "'->'" {
            return String::from("-=>");
        } else if symbol == EPSILON {
            return String::from("''");
        }
        return Grammar::stringify(symbol)
    }
}

impl Grammar {
    fn build_symbols(&mut self) {
        for rule in self.rules.iter() {
            if self.start_symbol.is_empty() {
                self.start_symbol = rule.left.clone();
            }
            self.non_terminals.insert(rule.left.clone());
            for symbol in &rule.right {
                if symbol == EPSILON { continue; }
                if Grammar::is_terminal(&symbol) {
                    self.terminals.insert(symbol.clone());
                } else {
                    self.non_terminals.insert(symbol.clone());
                }
            }
        }
        self.symbols.extend(self.terminals.iter().cloned());
        self.symbols.extend(self.non_terminals.iter().cloned());
        self.symbols.insert(String::from(EPSILON));
        self.symbols.insert(String::from(EOF));
        self.terminals.insert(String::from(EOF));
        self.rules.insert(
            0,
            GrammarRule {
                left: self.get_augmented_start_symbol(),
                right: vec![self.start_symbol.clone()]
            },
        );
    }

    fn build_props(&mut self) {
        self.build_props_nullable();
    }

    fn build_props_nullable(&mut self) {
        for rule in self.rules.iter()
            .filter(|r| r.right.len() == 1 && r.right[0] == EPSILON) {
            self.nullable_non_terminals.insert(rule.left.clone());
        }

        let mut prev_size = 0;
        while self.nullable_non_terminals.len() != prev_size {
            prev_size = self.nullable_non_terminals.len();
            for rule in self.rules[1..].iter() {
                if rule.right.iter()
                    .any(|s| self.nullable_non_terminals.contains(s)) {
                    self.nullable_non_terminals.insert(rule.left.clone());
                }
            }
        }
    }

    fn build_first_sets(&mut self) {
        let mut first_rules: HashMap<String, Vec<&GrammarRule>> = HashMap::new();
        for symbol in self.symbols.iter() {
            self.first_sets.insert(symbol.clone(), HashSet::new());
            first_rules.insert(
                symbol.clone(),
                self.rules.iter()
                    .filter(|r| r.left == *symbol)
                    .collect(),
            );
        }

        let first_of = |first_sets: &StringSetMap, s: &String| -> StringSet {
            // 1. If x is a terminal, then FIRST(x) = { ‘x’ }
            //
            // 2. If x-> Є, is a production rule, then add Є to FIRST(x).
            //
            // 3. If X->Y1 Y2 Y3….Yn is a production,
            //
            //      a) FIRST(X) = FIRST(Y1)
            //      b) If FIRST(Y1) contains Є then FIRST(X) = { FIRST(Y1) – Є } U { FIRST(Y2) }
            //      c) If FIRST (Yi) contains Є for all i = 1 to n, then add Є to FIRST(X).

            // Terminal symbol
            if Grammar::is_terminal(s) {
                return vec![s.clone()].into_iter().collect();
            }

            let mut first_set = HashSet::new();

            // Iterate through every rule that has the symbol in the left-hand side
            for rule in first_rules.get(s).unwrap().iter() {
                // Loop over the RHS symbols of the rule
                // While ε is in their first set
                let mut index = 0;
                let mut first: &StringSet;
                loop {
                    first = first_sets.get(&*rule.right[index]).unwrap();
                    first_set.extend(
                        first.iter()
                            .filter(|s| s.as_str() != EPSILON)
                            .cloned()
                    );
                    if first.contains(EPSILON) {
                        index += 1;
                    }
                    if !first.contains(EPSILON) || index >= rule.right.len() {
                        break;
                    }
                }

                // If all RHS symbols have ε in their first sets
                // Add ε to firstOf(symbol)
                // If symbol => ε exists, add ε to firstOf(symbol)
                if index == rule.right.len() || rule.right == vec![String::from(EPSILON)] {
                    first_set.insert(String::from(EPSILON));
                }
            }

            return first_set;
        };

        let mut iterate = true;
        while iterate {
            iterate = false;
            for s in self.symbols.iter() {
                let prev_size = self.first_sets.get(s.as_str()).unwrap().len();
                let set = first_of(&self.first_sets, s);
                if set.len() > prev_size {
                    iterate = true;
                }
                *self.first_sets.get_mut(s.as_str()).unwrap() = set;
            }
        }
    }

    fn build_follow_sets(&mut self) {
        let mut follow_rules: HashMap<String, Vec<&GrammarRule>> = HashMap::new();
        for symbol in self.non_terminals.iter() {
            self.follow_sets.insert(symbol.clone(), HashSet::new());
            follow_rules.insert(
                symbol.clone(),
                self.rules.iter()
                    .filter(|r| r.right.contains(symbol))
                    .collect(),
            );
        }
        self.follow_sets.get_mut(&*self.start_symbol).unwrap().insert(String::from(EOF));

        let follow_of = |first_sets: &StringSetMap, follow_sets: &StringSetMap, s: &String| -> StringSet {
            // 1. FOLLOW(S) = { $ }   // where S is the starting Non-Terminal
            //
            // 2. If A -> pBq is a production, where p, B and q are any grammar symbols,
            //    then everything in FIRST(q)  except Є is in FOLLOW(B).
            //
            // 3. If A-> pB is a production, then everything in FOLLOW(A) is in FOLLOW(B).
            //
            // 4. If A-> pBq is a production and FIRST(q) contains Є,
            //    then FOLLOW(B) contains { FIRST(q) – Є } U FOLLOW(A)
            let mut follow_set: StringSet = HashSet::new();

            // Find productions where symbol is in RHS
            for rule in follow_rules.get(s).unwrap().iter() {
                // Symbol may occur multiple times in RHS
                // We need to loop over each occurrence
                for (i, _) in rule.right.iter().enumerate().filter(|(_, v)| *v == s) {
                    // Loop over the RHS symbols occurring after symbol
                    // While ε is in their first set
                    let mut index = i + 1;
                    let mut first: &StringSet;
                    loop {
                        // We've hit the end of the RHS of the rule
                        // So everything in FOLLOW(LHS) is also in FOLLOW(symbol)
                        if index == rule.right.len() {
                            follow_set.extend(
                                follow_sets.get(&*rule.left)
                                    // .unwrap()
                                    .unwrap_or(&HashSet::new())
                                    .iter()
                                    .cloned()
                            );
                            break;
                        }
                        first = first_sets.get(&*rule.right[index]).unwrap();
                        follow_set.extend(
                            first.iter()
                                .filter(|s| s.as_str() != EPSILON)
                                .cloned()
                        );
                        if first.contains(EPSILON) {
                            index += 1;
                        } else {
                            break;
                        }
                    }
                }
            }

            // Add $ to FOLLOW(S)
            if *s == self.start_symbol {
                follow_set.insert(String::from(EOF));
            }

            // Remove epsilon and return the computed follow set
            follow_set.remove(EPSILON);
            follow_set
        };

        let mut iterate = true;
        while iterate {
            iterate = false;
            for s in self.non_terminals.iter() {
                let prev_size = self.follow_sets.get(s.as_str()).unwrap().len();
                let set = follow_of(&self.first_sets, &self.follow_sets, s);
                if set.len() > prev_size {
                    iterate = true;
                }
                *self.follow_sets.get_mut(s.as_str()).unwrap() = set;
            }
        }
    }

    pub fn first_of(&self, symbol: &str) -> Option<&StringSet> {
        self.first_sets.get(symbol)
    }

    pub fn follow_of(&self, symbol: &str) -> Option<&StringSet> {
        self.follow_sets.get(symbol)
    }

    pub fn is_nullable(&self, symbol: &str) -> bool {
        self.nullable_non_terminals.contains(symbol)
    }

    pub fn is_nullable_sequence(&self, sequence: &[String]) -> bool {
        sequence.iter().all(|s| self.is_nullable(s))
    }

    pub fn get_rule(&self, rule_index: usize) -> &GrammarRule {
        &self.rules[rule_index]
    }

    pub fn get_rules(&self) -> &Vec<GrammarRule> {
        &self.rules
    }

    pub fn get_token_types(&self) -> Vec<TokenType> {
        let mut token_types = vec![
            TokenType { name: String::from(""), regex: WHITESPACE.clone() },
            TokenType { name: String::from(""), regex: SINGLE_LINE_COMMENT.clone() },
            TokenType { name: String::from(""), regex: MULTI_LINE_COMMENT.clone() },
        ];
        token_types.extend(
            self.terminals.iter()
                .filter(|t| &**t != EOF)
                .sorted_by_key(|t| {
                    let rule = self.terminal_rules.iter().find(|r| r.left == **t);
                    if let Some(_) = rule { 1 } else { 0 }
                })
                .map(|t| {
                    let rule = self.terminal_rules.iter().find(|r| r.left == *t);
                    let regex = match rule {
                        Some(r) => format!("^{}", r.right[0]),
                        None => format!("^{}", regex::escape(&*Grammar::stringify(t)))
                    };
                    TokenType {
                        name: String::from(t),
                        regex: Regex::new(&*regex).unwrap(),
                    }
                })
        );
        token_types
    }

    pub fn get_start_symbol(&self) -> String {
        self.start_symbol.clone()
    }

    pub fn get_terminals(&self) -> &StringSet {
        &self.terminals
    }

    pub fn get_augmented_start_symbol(&self) -> String {
        format!("{}'", self.start_symbol)
    }

    pub fn to_jsmachine_string(&self) -> String {
        format!(
            "{}",
            self.rules.iter()
                .map(|r| r.to_jsmachine_string())
                .intersperse(String::from("\n"))
                .collect::<String>()
        )
    }
}
