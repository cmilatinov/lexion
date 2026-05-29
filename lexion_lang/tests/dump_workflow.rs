use lexion_lang::compiler::{LexionCompiler, LexionCompilerOptions};
use lexion_lang::DumpFlags;
use lexion_lib::miette::NamedSource;
use std::sync::Arc;

#[test]
fn dump_flags_display_in_stable_order() {
    let flags: DumpFlags = "ir, ast, cfg".parse().unwrap();

    assert_eq!(flags.to_string(), "ast,ir,cfg");
}

#[test]
fn dump_directory_is_predictable_for_repeated_runs() {
    let dump_dir = std::path::Path::new("target/test-dumps/predictable_dump_workflow");
    if dump_dir.exists() {
        std::fs::remove_dir_all(dump_dir).unwrap();
    }
    std::fs::create_dir_all(dump_dir).unwrap();
    std::fs::write(dump_dir.join("ast.tree"), "stale").unwrap();

    let source_path = "tests/fixtures/semantics/variables.lex";
    let source_code = Arc::new(std::fs::read_to_string(source_path).unwrap());
    let source = NamedSource::new(source_path, source_code);
    let options = LexionCompilerOptions {
        dump_flags: "ast,ir".parse().unwrap(),
        dump_dir: dump_dir.into(),
        ..LexionCompilerOptions::default()
    };

    LexionCompiler::new(options).exec(source).unwrap();

    let ast = std::fs::read_to_string(dump_dir.join("ast.tree")).unwrap();
    assert_ne!(ast, "stale");
    assert!(dump_dir.join("ir.table").is_file());
    assert!(dump_dir.join("ir.dot").is_file());
    assert!(!dump_dir.join("parse_trace.table").exists());
}

#[test]
fn dump_write_failures_are_reported_as_diagnostics() {
    let dump_dir = std::path::Path::new("target/test-dumps/dump_path_is_file");
    if dump_dir.is_dir() {
        std::fs::remove_dir_all(dump_dir).unwrap();
    } else if dump_dir.exists() {
        std::fs::remove_file(dump_dir).unwrap();
    }
    std::fs::create_dir_all(dump_dir.parent().unwrap()).unwrap();
    std::fs::write(dump_dir, "not a directory").unwrap();

    let source_path = "tests/fixtures/semantics/variables.lex";
    let source_code = Arc::new(std::fs::read_to_string(source_path).unwrap());
    let source = NamedSource::new(source_path, source_code);
    let options = LexionCompilerOptions {
        dump_flags: "ast".parse().unwrap(),
        dump_dir: dump_dir.into(),
    };

    let diagnostics = LexionCompiler::new(options).exec(source).unwrap_err();
    let messages = diagnostics
        .list
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert!(
        messages
            .iter()
            .any(|message| message.contains("failed to write dump file `ast.tree`")),
        "expected dump write diagnostic, got {messages:#?}"
    );
}
