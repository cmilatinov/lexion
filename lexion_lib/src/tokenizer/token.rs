use std::fmt::{Display, Formatter, Result};

use miette::SourceSpan;
use regex::Regex;

pub struct TokenType {
    pub name: String,
    pub regex: Regex,
}

#[derive(Debug, Clone)]
pub struct TokenInstance {
    pub token: String,
    pub value: String,
    pub span: SourceSpan,
}

impl Default for TokenInstance {
    fn default() -> Self {
        Self {
            token: String::from(""),
            value: String::from(""),
            span: (0, 0).into(),
        }
    }
}

impl Display for TokenType {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "[{}] -> /{}/", self.name, self.regex)
    }
}

impl TokenInstance {
    pub fn from(token: &str, value: &str, span: SourceSpan) -> Self {
        Self {
            token: String::from(token),
            value: String::from(value),
            span,
        }
    }
}
