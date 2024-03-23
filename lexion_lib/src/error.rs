use std::fmt::{Display, Formatter, Result};
use colored::Colorize;
use crate::tokenizer::SourceLocation;

#[derive(Debug, Clone)]
pub struct SyntaxError {
    pub loc: SourceLocation,
    pub message: String,
}

impl Display for SyntaxError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(
            f,
            "{} {} {}",
            "[ERROR]".bold(),
            self.message,
            self.loc.to_string()
        )
    }
}
