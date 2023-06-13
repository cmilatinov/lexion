use std::fmt::{Display, Formatter, Result};

#[derive(Clone)]
pub struct SourceLocation {
    pub file: String,
    pub line: i32,
    pub col: i32,
}

impl Display for SourceLocation {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(
            f,
            "(file:///{}:{}:{})",
            self.file,
            self.line,
            self.col
        )
    }
}
