use lexion_lib::grammar::serialize::{
    GrammarData as Grammar, ParseTableOverrideData as ParseTableOverride,
    ReductionData as Reduction, RuleData as Rule,
};
use lexion_lib::Parser;

#[derive(Parser)]
#[grammar(path = "grammars/grm.json")]
pub struct ParserGRM;

impl Default for ParserGRM {
    fn default() -> Self {
        Self::new()
    }
}

impl ParserGRM {
    pub fn new() -> Self {
        Self
    }
}
