#![allow(dead_code)]

use enumflags2::BitFlag;
use lexion_lang::compiler::{LexionCompiler, LexionCompilerOptions};
use lexion_lang::diagnostic::LexionDiagnostic;
use lexion_lang::{Dump, DumpFlags};
use lexion_lib::miette::{GraphicalReportHandler, GraphicalTheme, NamedSource};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn fixture_path(fixture: &str) -> PathBuf {
    Path::new("tests").join("fixtures").join(fixture)
}

pub fn fixture_source_name(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub fn compile(fixture: &str) -> Result<(), Vec<String>> {
    compile_with_options(fixture, Dump::empty().into(), "target/test-dumps")
}

pub fn compile_with_dumps_to(
    fixture: &str,
    dump_dir: impl Into<PathBuf>,
) -> Result<(), Vec<String>> {
    compile_with_options(fixture, DumpFlags::from(Dump::all()), dump_dir)
}

pub fn assert_compiles(fixture: &str) {
    if let Err(errors) = compile(fixture) {
        panic!(
            "expected fixture `{fixture}` to compile, got diagnostics:\n{}",
            errors.join("\n")
        );
    }
}

pub fn assert_fails(fixture: &str) -> Vec<String> {
    compile(fixture).unwrap_err()
}

pub fn render_diagnostic_for_snapshot(diagnostic: &LexionDiagnostic) -> String {
    let mut rendered = String::new();
    snapshot_report_handler()
        .render_report(&mut rendered, diagnostic)
        .expect("failed to render diagnostic");
    normalize_snapshot_text(rendered)
}

fn snapshot_report_handler() -> GraphicalReportHandler {
    GraphicalReportHandler::new_themed(GraphicalTheme::none())
        .with_width(200)
        .with_context_lines(1)
        .with_links(false)
        .with_urls(false)
        .without_syntax_highlighting()
}

fn normalize_snapshot_text(text: String) -> String {
    text.replace("\r\n", "\n")
}

fn compile_with_options(
    fixture: &str,
    dump_flags: DumpFlags,
    dump_dir: impl Into<PathBuf>,
) -> Result<(), Vec<String>> {
    let path = fixture_path(fixture);
    let source_code = Arc::new(std::fs::read_to_string(&path).expect("fixture not found"));
    let source_name = fixture_source_name(&path);
    let source = NamedSource::new(source_name, source_code);
    let options = LexionCompilerOptions {
        dump_flags,
        dump_dir: dump_dir.into(),
        ..LexionCompilerOptions::default()
    };
    LexionCompiler::new(options)
        .exec(source)
        .map(|_| ())
        .map_err(|diag| {
            diag.list
                .into_iter()
                .map(|diagnostic| render_diagnostic_for_snapshot(&diagnostic))
                .collect()
        })
}
