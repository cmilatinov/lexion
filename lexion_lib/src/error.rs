use std::fmt::{Display, Formatter, Result};
use colored::Colorize;
use crate::tokenizer::SourceRange;

#[derive(Debug, Clone)]
pub struct SyntaxError {
    pub range: SourceRange,
    pub message: String,
}

impl Display for SyntaxError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(
            f,
            "{} {} {}",
            "[ERROR]".bold(),
            self.message,
            self.range.start().to_string()
        )
    }
}
