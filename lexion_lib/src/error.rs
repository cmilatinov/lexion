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
