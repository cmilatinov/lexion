mod common;

use lexion_lang::diagnostic::{LexionDiagnostic, LexionDiagnosticError};
use lexion_lib::miette::{NamedSource, SourceSpan};
use std::path::Path;
use std::sync::Arc;

#[test]
fn miette_snapshot_rendering_is_stable() {
    let path = Path::new("tests")
        .join("fixtures")
        .join("errors")
        .join("rendering")
        .join("crlf.lex");
    let source_text = String::from("fn main() {\r\n    let value: i32 = true;\r\n}\r\n");
    let span = SourceSpan::new(
        source_text.find("true").expect("span target exists").into(),
        4,
    );
    let source = Arc::new(source_text);
    let diagnostic = LexionDiagnostic::Error(LexionDiagnosticError {
        src: NamedSource::new(common::fixture_source_name(&path), source),
        span,
        message: String::from("expected type 'i32', instead got 'bool'"),
    });

    insta::assert_snapshot!(common::render_diagnostic_for_snapshot(&diagnostic));
}
