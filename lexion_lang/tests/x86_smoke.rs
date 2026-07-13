use lexion_lang::diagnostic::LexionDiagnosticList;
use lexion_lang::generators::tac::CodeGeneratorTac;
use lexion_lang::generators::x86::{
    AbiRegisterAllocator, CodeGeneratorX86, X86EmitOptions, X86Target,
};
use lexion_lang::parser::ParserLexion;
use lexion_lang::pipeline::PipelineStage;
use lexion_lang::symbol_table::SymbolTableGenerator;
use lexion_lang::type_checker::TypeChecker;
use lexion_lib::miette::{GraphicalReportHandler, GraphicalTheme, NamedSource};
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
    TypeChecker::new((source.clone(), &mut symbols, &mut types))
        .exec(&mut diagnostics, &mut ast)
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));

    let (cfg, intervals) = CodeGeneratorTac::new((&ast, &mut symbols, &types))
        .exec(&mut diagnostics, ())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));
    let assigned = AbiRegisterAllocator::new((&cfg, &types, &symbols, X86Target::system_v64()))
        .exec(&mut diagnostics, intervals)
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));
    CodeGeneratorX86::new((&cfg, &types, &symbols))
        .with_allocations(&assigned)
        .exec(
            &mut diagnostics,
            X86EmitOptions::with_source_comments_and_diagnostics(source_code.as_ref(), &source),
        )
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)))
        .to_string()
}

fn compile_x86_error(fixture: &str) -> Vec<String> {
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
    TypeChecker::new((source.clone(), &mut symbols, &mut types))
        .exec(&mut diagnostics, &mut ast)
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));

    let (cfg, intervals) = CodeGeneratorTac::new((&ast, &mut symbols, &types))
        .exec(&mut diagnostics, ())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));
    let assigned = AbiRegisterAllocator::new((&cfg, &types, &symbols, X86Target::system_v64()))
        .exec(&mut diagnostics, intervals)
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));
    let output = CodeGeneratorX86::new((&cfg, &types, &symbols))
        .with_allocations(&assigned)
        .exec(
            &mut diagnostics,
            X86EmitOptions::with_source_comments_and_diagnostics(source_code.as_ref(), &source),
        );

    assert!(output.is_none(), "expected x86 backend to reject fixture");
    render_diagnostics(&diagnostics)
}

fn render_diagnostics(diagnostics: &LexionDiagnosticList) -> Vec<String> {
    diagnostics
        .list
        .iter()
        .map(|diagnostic| {
            let mut rendered = String::new();
            GraphicalReportHandler::new_themed(GraphicalTheme::none())
                .render_report(&mut rendered, diagnostic)
                .expect("failed to render diagnostic");
            rendered
        })
        .collect()
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

#[test]
fn x86_smoke_f32_arithmetic_and_comparisons() {
    insta::assert_snapshot!(compile_x86("backend/x86_f32_operations.lex"));
}

#[test]
fn x86_smoke_ternary_expression() {
    insta::assert_snapshot!(compile_x86("backend/ternary_expression.lex"));
}

#[test]
fn x86_smoke_ternary_false_expression() {
    insta::assert_snapshot!(compile_x86("backend/ternary_false_expression.lex"));
}

#[test]
fn x86_smoke_bitwise_and_shift_operators() {
    insta::assert_snapshot!(compile_x86("backend/x86_bitwise_shift.lex"));
}

#[test]
fn x86_smoke_unsigned_shift_operators() {
    insta::assert_snapshot!(compile_x86("backend/x86_unsigned_shift.lex"));
}

#[test]
fn x86_smoke_shift_preserves_allocated_registers() {
    let assembly = compile_x86("backend/x86_shift_register_preservation.lex");
    assert!(
        !assembly.contains("mov ecx, eax\n  shl eax, cl"),
        "shift count was loaded from a clobbered result register:\n{assembly}"
    );
    insta::assert_snapshot!("x86_smoke_shift_preserves_allocated_registers", assembly);
}

#[test]
fn x86_smoke_scalar_casts() {
    insta::assert_snapshot!(compile_x86("backend/x86_scalar_casts.lex"));
}

#[test]
fn x86_smoke_char_values() {
    insta::assert_snapshot!(compile_x86("backend/x86_char_values.lex"));
}

#[test]
fn x86_reports_unsupported_float_values() {
    insta::assert_snapshot!(compile_x86_error("backend/x86_unsupported_float.lex").join("\n"));
}

#[test]
fn x86_reports_unsupported_float_casts() {
    insta::assert_snapshot!(compile_x86_error("backend/x86_unsupported_float_cast.lex").join("\n"));
}

#[test]
fn x86_reports_unsupported_float_operations() {
    insta::assert_snapshot!(compile_x86_error("backend/x86_unsupported_float_ops.lex").join("\n"));
}

#[test]
fn x86_reports_unsupported_string_values() {
    insta::assert_snapshot!(compile_x86_error("backend/x86_unsupported_string.lex").join("\n"));
}

#[test]
fn x86_reports_unsupported_call_string_arg() {
    insta::assert_snapshot!(
        compile_x86_error("backend/x86_unsupported_call_string_arg.lex").join("\n")
    );
}

#[test]
fn x86_reports_unsupported_tuple_values() {
    insta::assert_snapshot!(compile_x86_error("backend/x86_unsupported_tuple.lex").join("\n"));
}

#[test]
fn x86_reports_unsupported_call_tuple_arg() {
    insta::assert_snapshot!(
        compile_x86_error("backend/x86_unsupported_call_tuple_arg.lex").join("\n")
    );
}

#[test]
fn x86_reports_unsupported_struct_values() {
    insta::assert_snapshot!(compile_x86_error("backend/x86_unsupported_struct.lex").join("\n"));
}

#[test]
fn x86_reports_unsupported_call_struct_arg() {
    insta::assert_snapshot!(
        compile_x86_error("backend/x86_unsupported_call_struct_arg.lex").join("\n")
    );
}

#[test]
fn x86_reports_unsupported_indexed_access() {
    insta::assert_snapshot!(compile_x86_error("backend/x86_unsupported_index.lex").join("\n"));
}

#[test]
fn x86_smoke_reference_borrow() {
    insta::assert_snapshot!(compile_x86("backend/x86_reference_borrow.lex"));
}

#[test]
fn x86_reports_unsupported_call_reference_arg() {
    insta::assert_snapshot!(
        compile_x86_error("backend/x86_unsupported_call_reference_arg.lex").join("\n")
    );
}

#[test]
fn x86_smoke_reference_dereference() {
    insta::assert_snapshot!(compile_x86("backend/x86_reference_dereference.lex"));
}

#[test]
fn x86_smoke_function_scoped_symbol_types() {
    insta::assert_snapshot!(compile_x86("backend/x86_function_scoped_symbols.lex"));
}

#[test]
fn x86_smoke_extern_call() {
    insta::assert_snapshot!(compile_x86("backend/x86_extern_call.lex"));
}

#[test]
fn x86_reports_unsupported_fixed_vararg_calls() {
    insta::assert_snapshot!(
        compile_x86_error("backend/x86_unsupported_vararg_fixed.lex").join("\n")
    );
}

#[test]
fn x86_reports_unsupported_zero_fixed_vararg_calls() {
    insta::assert_snapshot!(
        compile_x86_error("backend/x86_unsupported_vararg_zero_fixed.lex").join("\n")
    );
}

#[test]
fn x86_reports_unsupported_function_values() {
    insta::assert_snapshot!(
        compile_x86_error("backend/x86_unsupported_function_value.lex").join("\n")
    );
}

#[test]
fn x86_reports_unsupported_shadowed_function_value_calls() {
    insta::assert_snapshot!(compile_x86_error(
        "backend/x86_unsupported_shadowed_function_value.lex"
    )
    .join("\n"));
}

#[test]
fn x86_smoke_system_v_function_call() {
    insta::assert_snapshot!(compile_x86("backend/x86_function_call.lex"));
}

#[test]
fn x86_smoke_stack_function_call() {
    insta::assert_snapshot!(compile_x86("backend/x86_stack_function_call.lex"));
}

#[test]
fn x86_smoke_branch_loop_function_call() {
    let assembly = compile_x86("backend/branch_loop_call.lex");
    assert!(
        !assembly.contains("cmp eax, eax"),
        "conditional jump compare clobbered its condition:\n{assembly}"
    );
    insta::assert_snapshot!("x86_smoke_branch_loop_function_call", assembly);
}

#[test]
fn x86_smoke_call_preserves_live_value() {
    insta::assert_snapshot!(compile_x86("backend/x86_call_preserves_live_value.lex"));
}
