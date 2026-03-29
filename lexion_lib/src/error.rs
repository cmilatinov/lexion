use std::fmt::{Display, Formatter};
use std::io::Error;
use std::sync::Arc;

use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;

#[derive(Debug, Clone, Error, Diagnostic)]
#[error("{message}")]
#[diagnostic()]
pub struct SyntaxError {
    #[source_code]
    pub src: NamedSource<Arc<String>>,
    #[label("here")]
    pub span: SourceSpan,
    pub message: String,
}

#[derive(Debug)]
pub enum ParseError {
    Syntax(SyntaxError),
    Io(Error),
}

impl From<SyntaxError> for ParseError {
    fn from(value: SyntaxError) -> Self {
        Self::Syntax(value)
    }
}

impl From<Error> for ParseError {
    fn from(value: Error) -> Self {
        Self::Io(value)
    }
}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Syntax(e) => e.fmt(f),
            ParseError::Io(e) => e.fmt(f),
        }
    }
}
