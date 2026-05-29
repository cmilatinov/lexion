use lexion_lang::diagnostic::LexionDiagnosticList;
use lexion_lang::generators::tac::CodeGeneratorTac;
use lexion_lang::generators::x86::{CodeGeneratorX86, X86EmitOptions};
use lexion_lang::parser::ParserLexion;
use lexion_lang::pipeline::PipelineStage;
use lexion_lang::symbol_table::SymbolTableGenerator;
use lexion_lang::type_checker::TypeChecker;
use lexion_lib::miette::NamedSource;
use std::sync::Arc;

fn compile_x86(fixture: &str) -> String {
    let path = format!("tests/fixtures/{fixture}");
    let source_code = Arc::new(std::fs::read_to_string(&path).expect("fixture not found"));
    let source = NamedSource::new(&path, source_code.clone());
    let mut diagnostics = LexionDiagnosticList::default();

    let (mut ast, mut types, _) = ParserLexion::new()
        .exec(&mut diagnostics, source.clone())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));
    let mut symbols = SymbolTableGenerator::new((source.clone(), &ast, &mut types))
        .exec(&mut diagnostics, ())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));
    TypeChecker::new((source, &mut symbols, &mut types))
        .exec(&mut diagnostics, &mut ast)
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));

    let (cfg, _) = CodeGeneratorTac::new((&ast, &mut symbols, &types))
        .exec(&mut diagnostics, ())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));
    CodeGeneratorX86::new((&cfg, &types, &symbols))
        .exec(
            &mut diagnostics,
            X86EmitOptions::with_source_comments(source_code.as_ref()),
        )
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)))
        .to_string()
}

fn diagnostics_string(diagnostics: &LexionDiagnosticList) -> String {
    diagnostics
        .list
        .iter()
        .map(|diag| diag.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn x86_smoke_return_arithmetic() {
    insta::assert_snapshot!(compile_x86("backend/x86_return_arithmetic.lex"));
}

#[test]
fn x86_smoke_return_bool_comparison() {
    insta::assert_snapshot!(compile_x86("backend/x86_return_bool.lex"));
}
