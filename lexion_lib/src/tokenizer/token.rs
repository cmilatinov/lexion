use std::fmt::{Display, Formatter, Result};

use regex::Regex;

use crate::tokenizer::{FileLocation, SourceLocation};

pub struct TokenType {
    pub name: String,
    pub regex: Regex,
}

#[derive(Debug, Default, Clone)]
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

impl TokenInstance {
    pub fn new() -> Self {
        Self {
            token: String::from(""),
            value: String::from(""),
            loc: SourceLocation {
                file: "inline",
                loc: FileLocation { line: 1, col: 1 },
            },
        }
    }

    pub fn from(token: &str, value: &str, loc: &SourceLocation) -> Self {
        Self {
            token: String::from(token),
            value: String::from(value),
            loc: loc.clone(),
        }
    }
}
