use std::fmt::{Display, Formatter, Result};
use crate::lib::error::SyntaxError;
use crate::lib::grammar::{Grammar, GrammarRule};
use crate::lib::tokenizer::TokenInstance;

#[derive(Clone)]
pub struct DerivationNode {
    pub token: TokenInstance,
    pub children: Vec<Box<DerivationNode>>,
    pub rule_index: usize,
}

pub type Derivation = std::result::Result<Box<DerivationNode>, SyntaxError>;

impl Display for DerivationNode {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let token = Grammar::stringify(&*self.token.token);
        write!(
            f, "[{}]{} {}",
            token,
            if token == self.token.value { String::from("") }
            else { format!(" `{}`", self.token.value) },
            self.token.loc
        )
    }
}

impl DerivationNode {
    pub fn new() -> Self {
        Self {
            token: TokenInstance::new(),
            children: Vec::new(),
            rule_index: 0,
        }
    }

    pub fn from_token(token: TokenInstance) -> Self {
        Self {
            token,
            children: Vec::new(),
            rule_index: 0,
        }
    }

    pub fn from(
        token: TokenInstance,
        children: Vec<Box<DerivationNode>>,
        rule_index: usize,
    ) -> Self {
        Self {
            token,
            children,
            rule_index,
        }
    }

    pub fn get_rule<'a>(&'a self, grammar: &'a Grammar) -> &'a GrammarRule {
        &grammar.get_rules()[self.rule_index]
    }

    fn _print_std(node: &DerivationNode, mut indent: String, last: i32) {
        print!("{}", indent);
        if last == 0 {
            print!("├─");
            indent += "│ ";
        } else if last == 1 {
            print!("└─");
            indent += "  ";
        }
        println!("{}", node);
        for (i, child) in node.children.iter().enumerate() {
            DerivationNode::_print_std(
                child,
                indent.clone(),
                if i == node.children.len() - 1 { 1 }
                else { 0 }
            );
        }
    }

    pub fn print_std(&self) {
        DerivationNode::_print_std(self, String::from(""), 2);
    }
}
