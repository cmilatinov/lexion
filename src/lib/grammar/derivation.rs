use std::fmt::{Display, Formatter, Result};
use crate::lib::error::SyntaxError;
use crate::lib::grammar::{Grammar, GrammarRule};
use crate::lib::tokenizer::TokenInstance;

#[derive(Clone)]
pub struct DerivationNode {
    pub token: TokenInstance,
    pub children: Vec<Box<DerivationNode>>,
    pub rule_index: usize
}

pub type Derivation = std::result::Result<Box<DerivationNode>, SyntaxError>;

impl Display for DerivationNode {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(
            f, "[{}] {} {}",
            Grammar::stringify(&*self.token.token),
            self.token.loc,
            self.token.value
        )
    }
}

impl DerivationNode {
    pub fn new() -> Self {
        Self {
            token: TokenInstance::new(),
            children: Vec::new(),
            rule_index: 0
        }
    }

    pub fn from_token(token: TokenInstance) -> Self {
        Self {
            token,
            children: Vec::new(),
            rule_index: 0
        }
    }

    pub fn from(
        token: TokenInstance,
        children: Vec<Box<DerivationNode>>,
        rule_index: usize
    ) -> Self {
        Self {
            token,
            children,
            rule_index
        }
    }

    pub fn get_rule<'a>(&'a self, grammar: &'a Grammar) -> &GrammarRule {
        &grammar.get_rules()[self.rule_index]
    }
}
