use lexion_lang::compiler::{EmitTarget, LexionCompiler, LexionCompilerOptions};
use lexion_lib::miette::NamedSource;
use std::sync::Arc;

#[test]
fn compiler_emit_asm_returns_x86_assembly() {
    let source_path = "tests/fixtures/backend/x86_return_arithmetic.lex";
    let source_code = Arc::new(std::fs::read_to_string(source_path).unwrap());
    let source = NamedSource::new(source_path, source_code);
    let output = LexionCompiler::new(LexionCompilerOptions {
        emit: EmitTarget::X86Assembly,
        ..LexionCompilerOptions::default()
    })
    .exec(source)
    .unwrap();

    let assembly = output.assembly.expect("expected assembly output");
    insta::assert_snapshot!(assembly.as_str());
}

#[test]
fn compiler_emit_asm_returns_x86_function_call_assembly() {
    let source_path = "tests/fixtures/backend/x86_function_call.lex";
    let source_code = Arc::new(std::fs::read_to_string(source_path).unwrap());
    let source = NamedSource::new(source_path, source_code);
    let output = LexionCompiler::new(LexionCompilerOptions {
        emit: EmitTarget::X86Assembly,
        ..LexionCompilerOptions::default()
    })
    .exec(source)
    .unwrap();

    let assembly = output.assembly.expect("expected assembly output");
    insta::assert_snapshot!(assembly.as_str());
}

#[test]
fn compiler_emit_asm_returns_stack_aggregate_argument_assembly() {
    let source_path = "tests/fixtures/backend/x86_stack_aggregate_arguments.lex";
    let source_code = Arc::new(std::fs::read_to_string(source_path).unwrap());
    let source = NamedSource::new(source_path, source_code);
    let output = LexionCompiler::new(LexionCompilerOptions {
        emit: EmitTarget::X86Assembly,
        ..LexionCompilerOptions::default()
    })
    .exec(source)
    .unwrap();

    let assembly = output.assembly.expect("expected assembly output");
    insta::assert_snapshot!(assembly.as_str());
}

#[test]
fn compiler_emit_asm_returns_indirect_aggregate_return_assembly() {
    let source_path = "tests/fixtures/backend/x86_indirect_aggregate_returns.lex";
    let source_code = Arc::new(std::fs::read_to_string(source_path).unwrap());
    let source = NamedSource::new(source_path, source_code);
    let output = LexionCompiler::new(LexionCompilerOptions {
        emit: EmitTarget::X86Assembly,
        ..LexionCompilerOptions::default()
    })
    .exec(source)
    .unwrap();

    let assembly = output.assembly.expect("expected assembly output");
    insta::assert_snapshot!(assembly.as_str());
}
#[test]
fn compiler_emit_asm_returns_function_value_assembly() {
    let source_path = "tests/fixtures/backend/x86_function_values.lex";
    let source_code = Arc::new(std::fs::read_to_string(source_path).unwrap());
    let source = NamedSource::new(source_path, source_code);
    let output = LexionCompiler::new(LexionCompilerOptions {
        emit: EmitTarget::X86Assembly,
        ..LexionCompilerOptions::default()
    })
    .exec(source)
    .unwrap();

    let assembly = output.assembly.expect("expected assembly output");
    insta::assert_snapshot!(assembly.as_str());
}
