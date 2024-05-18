use std::fmt::{Debug, Display, Formatter};

use lexion_lib::tokenizer::SourceRange;

#[derive(Debug)]
pub enum DiagnosticType {
    Info,
    Warning,
    Error,
}

impl Display for DiagnosticType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        <Self as Debug>::fmt(self, f)
    }
}

#[derive(Debug)]
pub struct Diagnostic {
    pub ty: DiagnosticType,
    pub message: String,
    pub range: Option<SourceRange>,
}

impl Display for Diagnostic {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} [{}] {}",
            match self.range {
                None => String::new(),
                Some(range) => range.to_string(),
            },
            self.ty,
            self.message
        )
    }
}

pub trait DiagnosticConsumer {
    fn emit(&mut self, diagnostic: Diagnostic);

    fn emit_ty_msg(&mut self, ty: DiagnosticType, message: String) {
        self.emit(Diagnostic {
            ty,
            message,
            range: None,
        });
    }

    fn info(&mut self, message: String) {
        self.emit(Diagnostic {
            ty: DiagnosticType::Info,
            message,
            range: None,
        });
    }

    fn info_range(&mut self, message: String, range: SourceRange) {
        self.emit(Diagnostic {
            ty: DiagnosticType::Warning,
            message,
            range: Some(range),
        });
    }

    fn warn(&mut self, message: String) {
        self.emit(Diagnostic {
            ty: DiagnosticType::Warning,
            message,
            range: None,
        });
    }

    fn warn_range(&mut self, message: String, range: SourceRange) {
        self.emit(Diagnostic {
            ty: DiagnosticType::Warning,
            message,
            range: Some(range),
        });
    }

    fn error(&mut self, message: String) {
        self.emit(Diagnostic {
            ty: DiagnosticType::Error,
            message,
            range: None,
        });
    }

    fn error_range(&mut self, message: String, range: SourceRange) {
        self.emit(Diagnostic {
            ty: DiagnosticType::Error,
            message,
            range: Some(range),
        });
    }
}

pub struct DiagnosticPrinterStdout;

impl DiagnosticConsumer for DiagnosticPrinterStdout {
    fn emit(&mut self, diagnostic: Diagnostic) {
        println!("{}", diagnostic);
    }
}
