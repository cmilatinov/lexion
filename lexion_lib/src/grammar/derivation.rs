use std::fmt::{Display, Formatter, Result};

use petgraph::Graph;
use petgraph::graph::NodeIndex;

use crate::grammar::{Grammar, GrammarRule};
use crate::tokenizer::TokenInstance;

#[derive(Debug)]
pub struct DerivationNode {
    pub token: TokenInstance,
    pub rule_index: usize,
}

pub struct Derivation(pub NodeIndex, pub Graph<DerivationNode, usize>);

impl Display for DerivationNode {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let token = Grammar::stringify(&*self.token.token);
        write!(
            f,
            "[{}]{} {}",
            token,
            if token == self.token.value {
                String::from("")
            } else {
                format!(" `{}`", self.token.value)
            },
            self.token.loc
        )
    }
}

impl Derivation {
    fn write(
        node_id: NodeIndex,
        graph: &Graph<DerivationNode, usize>,
        f: &mut Formatter<'_>,
        mut indent: String,
        last: usize,
    ) -> Result {
        write!(f, "{}", indent)?;
        if last == 0 {
            write!(f, "├─")?;
            indent += "│ ";
        } else if last == 1 {
            write!(f, "└─")?;
            indent += "  ";
        }

        write!(f, "{}\n", graph.node_weight(node_id).unwrap())?;
        let num_children = graph.neighbors(node_id).count();
        for (i, child) in graph.neighbors(node_id).enumerate() {
            Derivation::write(
                child,
                graph,
                f,
                indent.clone(),
                if i == num_children - 1 { 1 } else { 0 },
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

    pub fn from(token: TokenInstance, rule_index: usize) -> Self {
        Self { token, rule_index }
    }

    pub fn get_rule<'a>(&'a self, grammar: &'a Grammar) -> &'a GrammarRule {
        &grammar.get_rules()[self.rule_index]
    }
}
