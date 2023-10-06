use std::fmt::{Display, Formatter, Result};
use indextree::{Arena, NodeId};
use crate::grammar::{Grammar, GrammarRule};
use crate::tokenizer::TokenInstance;

pub struct DerivationNode {
    pub token: TokenInstance,
    pub rule_index: usize,
}

pub struct Derivation(pub NodeId, pub Arena<DerivationNode>);

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

impl Derivation {
    fn write(
        node_id: NodeId,
        arena: &Arena<DerivationNode>,
        f: &mut Formatter<'_>,
        mut indent: String,
        last: usize
    ) -> Result {
        write!(f, "{}", indent)?;
        if last == 0 {
            write!(f, "├─")?;
            indent += "│ ";
        } else if last == 1 {
            write!(f, "└─")?;
            indent += "  ";
        }
        write!(f, "{}\n", arena.get(node_id).unwrap().get())?;
        let num_children = node_id.children(arena).count();
        for (i, child) in node_id.children(arena).enumerate() {
            Derivation::write(
                child,
                arena,
                f,
                indent.clone(),
                if i == num_children - 1 { 1 }
                else { 0 }
            )?;
        }
        Ok(())
    }
}

impl Display for Derivation {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        Self::write(self.0, &self.1, f, String::from(""), 2)
    }
}

impl DerivationNode {
    pub fn new() -> Self {
        Self {
            token: TokenInstance::new(),
            rule_index: 0,
        }
    }

    pub fn from_token(token: TokenInstance) -> Self {
        Self {
            token,
            rule_index: 0,
        }
    }

    pub fn from(
        token: TokenInstance,
        rule_index: usize,
    ) -> Self {
        Self {
            token,
            rule_index,
        }
    }

    pub fn get_rule<'a>(&'a self, grammar: &'a Grammar) -> &'a GrammarRule {
        &grammar.get_rules()[self.rule_index]
    }
}

