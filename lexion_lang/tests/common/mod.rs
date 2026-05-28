#![allow(dead_code)]

use enumflags2::BitFlag;
use lexion_lang::compiler::{LexionCompiler, LexionCompilerOptions};
use lexion_lang::{Dump, DumpFlags};
use lexion_lib::miette::{GraphicalReportHandler, GraphicalTheme, NamedSource};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn fixture_path(fixture: &str) -> PathBuf {
    Path::new("tests").join("fixtures").join(fixture)
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

fn compile_with_options(
    fixture: &str,
    dump_flags: DumpFlags,
    dump_dir: impl Into<PathBuf>,
) -> Result<(), Vec<String>> {
    let path = fixture_path(fixture);
    let source_code = Arc::new(std::fs::read_to_string(&path).expect("fixture not found"));
    let source_name = path.to_string_lossy().replace('\\', "/");
    let source = NamedSource::new(source_name, source_code);
    let options = LexionCompilerOptions {
        dump_flags,
        dump_dir: dump_dir.into(),
    };
    LexionCompiler::new(options)
        .exec(source)
        .map(|_| ())
        .map_err(|diag| {
            diag.list
                .into_iter()
                .map(|diagnostic| {
                    let mut rendered = String::new();
                    GraphicalReportHandler::new_themed(GraphicalTheme::none())
                        .render_report(&mut rendered, &diagnostic)
                        .expect("failed to render diagnostic");
                    rendered
                })
                .collect()
        })
}
