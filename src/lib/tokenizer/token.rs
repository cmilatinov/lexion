use std::fmt::{Display, Formatter, Result};
use regex::Regex;

use crate::lib::tokenizer::SourceLocation;

pub struct TokenType {
    pub name: String,
    pub regex: Regex,
}

pub struct TokenInstance {
    pub token: String,
    pub value: String,
    pub loc: SourceLocation,
}

impl Display for TokenType {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "[{}] -> /{}/", self.name, self.regex)
    }
}

impl Display for TokenInstance {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{} [{}]", self.loc, self.value.replace("\n", "\\n"))
    }
}