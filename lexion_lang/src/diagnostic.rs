use std::fmt::Debug;
use std::sync::Arc;

use miette::{Diagnostic, NamedSource, SourceSpan};

use lexion_lib::{miette, thiserror};
use lexion_lib::error::SyntaxError;
use lexion_lib::miette::Report;
use lexion_lib::thiserror::Error;

#[derive(Debug, Error, Diagnostic)]
#[error("{message}")]
#[diagnostic(severity(error))]
pub struct LexionDiagnosticError {
    #[source_code]
    pub src: NamedSource<Arc<String>>,
    #[label("here")]
    pub span: SourceSpan,
    pub message: String,
}

#[derive(Debug, Error, Diagnostic)]
#[error("{message}")]
#[diagnostic(severity(warn))]
pub struct LexionDiagnosticWarn {
    #[source_code]
    pub src: NamedSource<Arc<String>>,
    #[label("here")]
    pub span: SourceSpan,
    pub message: String,
}

#[derive(Debug, Error, Diagnostic)]
#[error("{message}")]
#[diagnostic(severity(info))]
pub struct LexionDiagnosticInfo {
    #[source_code]
    pub src: NamedSource<Arc<String>>,
    #[label("here")]
    pub span: SourceSpan,
    pub message: String,
}

#[derive(Debug, Error, Diagnostic)]
pub enum LexionDiagnostic {
    #[error(transparent)]
    #[diagnostic(transparent)]
    Error(#[from] LexionDiagnosticError),
    #[error(transparent)]
    #[diagnostic(transparent)]
    Warn(#[from] LexionDiagnosticWarn),
    #[error(transparent)]
    #[diagnostic(transparent)]
    Info(#[from] LexionDiagnosticInfo),
}

impl From<SyntaxError> for LexionDiagnostic {
    fn from(value: SyntaxError) -> Self {
        Self::Error(LexionDiagnosticError {
            src: value.src,
            span: value.span,
            message: value.message,
        })
    }
}

#[derive(Debug, Default, Error, Diagnostic)]
#[error("Compilation errors:")]
#[diagnostic()]
pub struct LexionDiagnosticList {
    #[related]
    pub list: Vec<LexionDiagnostic>,
}

impl LexionDiagnosticList {
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

pub trait DiagnosticConsumer {
    fn emit(&mut self, diagnostic: LexionDiagnostic);

    fn info(&mut self, diagnostic: LexionDiagnosticInfo) {
        self.emit(LexionDiagnostic::Info(diagnostic));
    }

    fn warn(&mut self, diagnostic: LexionDiagnosticWarn) {
        self.emit(LexionDiagnostic::Warn(diagnostic));
    }

    fn error(&mut self, diagnostic: LexionDiagnosticError) {
        self.emit(LexionDiagnostic::Error(diagnostic));
    }
}

impl DiagnosticConsumer for LexionDiagnosticList {
    fn emit(&mut self, diagnostic: LexionDiagnostic) {
        self.list.push(diagnostic);
    }

    fn info(&mut self, diagnostic: LexionDiagnosticInfo) {
        self.emit(LexionDiagnostic::Info(diagnostic));
    }

    fn warn(&mut self, diagnostic: LexionDiagnosticWarn) {
        self.emit(LexionDiagnostic::Warn(diagnostic));
    }

    fn error(&mut self, diagnostic: LexionDiagnosticError) {
        self.emit(LexionDiagnostic::Error(diagnostic));
    }
}

#[derive(Default)]
pub struct DiagnosticPrinterStdout;

impl DiagnosticConsumer for DiagnosticPrinterStdout {
    fn emit(&mut self, diagnostic: LexionDiagnostic) {
        eprintln!("{:?}", Report::new(diagnostic));
    }
}
