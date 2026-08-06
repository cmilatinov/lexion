use iced_x86::{Decoder, DecoderOptions, Formatter, IntelFormatter};
use lexion_lang::diagnostic::LexionDiagnosticList;
use lexion_lang::generators::tac::CodeGeneratorTac;
use lexion_lang::generators::x86::{
    Bitness, CMemoryLayoutBuilder, CodeGeneratorX86Machine, X86MachineCode, X86MachineCodeOptions,
};
use lexion_lang::parser::ParserLexion;
use lexion_lang::pipeline::PipelineStage;
use lexion_lang::symbol_table::SymbolTableGenerator;
use lexion_lang::type_checker::TypeChecker;
use lexion_lib::miette::NamedSource;
use std::{collections::BTreeMap, sync::Arc};

fn compile_machine_code(fixture: &str) -> X86MachineCode {
    let path = format!("tests/fixtures/{fixture}");
    let source_code = Arc::new(std::fs::read_to_string(&path).expect("fixture not found"));
    let source = NamedSource::new(&path, source_code);
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
    types.compute_memory_layouts::<CMemoryLayoutBuilder>(Bitness::_64);

    let (cfg, _) = CodeGeneratorTac::new((&ast, &mut symbols, &types))
        .exec(&mut diagnostics, ())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));
    CodeGeneratorX86Machine::new((&cfg, &types, &symbols))
        .exec(&mut diagnostics, X86MachineCodeOptions::default())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)))
}

fn compile_machine_code_error(fixture: &str) -> Vec<String> {
    let path = format!("tests/fixtures/{fixture}");
    let source_code = Arc::new(std::fs::read_to_string(&path).expect("fixture not found"));
    let source = NamedSource::new(&path, source_code);
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
    types.compute_memory_layouts::<CMemoryLayoutBuilder>(Bitness::_64);

    let (cfg, _) = CodeGeneratorTac::new((&ast, &mut symbols, &types))
        .exec(&mut diagnostics, ())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));
    let output = CodeGeneratorX86Machine::new((&cfg, &types, &symbols))
        .exec(&mut diagnostics, X86MachineCodeOptions::default());

    assert!(
        output.is_none(),
        "expected x86 machine-code backend to reject fixture"
    );
    diagnostics_string(&diagnostics)
        .lines()
        .map(String::from)
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

fn machine_snapshot(code: &X86MachineCode) -> String {
    format!(
        "symbols:\n{}\n\nbytes:\n{}\n\ndisassembly:\n{}",
        code.symbols()
            .iter()
            .map(|(name, offset)| format!("{name}=0x{offset:04X}"))
            .collect::<Vec<_>>()
            .join("\n"),
        hex_bytes(code.as_bytes()),
        disassemble(code.as_bytes(), code.symbols())
    )
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn disassemble(bytes: &[u8], symbols: &BTreeMap<String, usize>) -> String {
    let symbols_by_offset = symbols
        .iter()
        .map(|(name, offset)| (*offset, name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut decoder = Decoder::with_ip(64, bytes, 0, DecoderOptions::NONE);
    let mut formatter = IntelFormatter::new();
    let mut lines = Vec::new();
    while decoder.can_decode() {
        let instruction = decoder.decode();
        let mut formatted = String::new();
        formatter.format(&instruction, &mut formatted);
        if let Some(symbol) = symbols_by_offset.get(&(instruction.ip() as usize)) {
            lines.push(format!("{symbol}:"));
        }
        if formatted.starts_with("call ") {
            if let Some(symbol) =
                symbols_by_offset.get(&(instruction.near_branch_target() as usize))
            {
                formatted = format!("{formatted} <{symbol}>");
            }
        }
        lines.push(format!("{:04X}: {formatted}", instruction.ip()));
    }
    lines.join("\n")
}

#[test]
fn x86_machine_code_return_arithmetic() {
    let code = compile_machine_code("backend/x86_return_arithmetic.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_return_bool_comparison() {
    let code = compile_machine_code("backend/x86_return_bool.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_f32_arithmetic_and_comparisons() {
    let code = compile_machine_code("backend/x86_f32_operations.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_f32_function_calls() {
    let code = compile_machine_code("backend/x86_f32_function_calls.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_ternary_expression() {
    let code = compile_machine_code("backend/ternary_expression.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_ternary_false_expression() {
    let code = compile_machine_code("backend/ternary_false_expression.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_bitwise_and_shift_operators() {
    let code = compile_machine_code("backend/x86_bitwise_shift.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_unsigned_shift_operators() {
    let code = compile_machine_code("backend/x86_unsigned_shift.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_scalar_casts() {
    let code = compile_machine_code("backend/x86_scalar_casts.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_char_values() {
    let code = compile_machine_code("backend/x86_char_values.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_local_aggregate_values() {
    let code = compile_machine_code("backend/x86_local_aggregates.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_one_eightbyte_aggregate_abi_values() {
    insta::assert_snapshot!(machine_snapshot(&compile_machine_code(
        "backend/x86_aggregate_abi.lex"
    )));
}

#[test]
fn x86_machine_code_one_eightbyte_aggregate_abi_padding() {
    insta::assert_snapshot!(machine_snapshot(&compile_machine_code(
        "backend/x86_aggregate_abi_padding.lex"
    )));
}

#[test]
fn x86_machine_code_nested_reference_aggregate_abi_values() {
    compile_machine_code("backend/x86_nested_reference_aggregate_abi.lex");
}

#[test]
fn x86_machine_code_register_pair_aggregate_abi_values() {
    let snapshot = machine_snapshot(&compile_machine_code(
        "backend/x86_register_pair_aggregates.lex",
    ));
    let shift_quad = snapshot
        .split_once("shift_quad:\n")
        .and_then(|(_, disassembly)| disassembly.split_once("shift_tuple:\n"))
        .map(|(disassembly, _)| disassembly)
        .expect("missing shift_quad disassembly");
    let high_load = shift_quad
        .find("mov rdx,[rsp]")
        .expect("missing RDX return-half load");
    let low_load = shift_quad
        .find("mov rax,[rsp]")
        .expect("missing RAX return-half load");
    assert!(
        high_load < low_load,
        "register-pair return must load RDX before RAX scratch clobbers it:\n{shift_quad}"
    );
    insta::assert_snapshot!(snapshot);
}

#[test]
fn x86_machine_code_indexed_register_pair_call_arguments() {
    compile_machine_code("backend/x86_indexed_register_pair_call.lex");
}

#[test]
fn x86_machine_code_aggregate_member_values() {
    let code = compile_machine_code("backend/x86_aggregate_members.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_if_else_returns() {
    let code = compile_machine_code("backend/x86_if_expression.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_system_v_function_call() {
    let code = compile_machine_code("backend/x86_function_call.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_stack_arguments() {
    let code = compile_machine_code("backend/x86_stack_arguments.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_unit_arguments() {
    let code = compile_machine_code("backend/x86_unit_arguments.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_reports_unsupported_string_values() {
    insta::assert_snapshot!(
        compile_machine_code_error("backend/x86_unsupported_string.lex").join("\n")
    );
}

#[test]
fn x86_machine_reports_unsupported_call_string_arg() {
    insta::assert_snapshot!(compile_machine_code_error(
        "backend/x86_unsupported_call_string_arg.lex"
    )
    .join("\n"));
}

#[test]
fn x86_machine_reports_unsupported_call_tuple_arg() {
    insta::assert_snapshot!(compile_machine_code_error(
        "backend/x86_unsupported_call_tuple_arg.lex"
    )
    .join("\n"));
}

#[test]
fn x86_machine_reports_unsupported_stack_and_indirect_aggregates() {
    insta::assert_snapshot!(compile_machine_code_error(
        "backend/x86_unsupported_stack_and_indirect_aggregates.lex"
    )
    .join("\n"));
}

#[test]
fn x86_machine_reports_unsupported_call_struct_arg() {
    insta::assert_snapshot!(compile_machine_code_error(
        "backend/x86_unsupported_call_struct_arg.lex"
    )
    .join("\n"));
}

#[test]
fn x86_machine_reports_unsupported_aggregate_members() {
    insta::assert_snapshot!(compile_machine_code_error(
        "backend/x86_unsupported_aggregate_members.lex"
    )
    .join("\n"));
}

#[test]
fn x86_machine_code_reference_call() {
    let code = compile_machine_code("backend/x86_reference_call.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_reference_stack_arguments() {
    let code = compile_machine_code("backend/x86_reference_stack_arguments.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_reference_borrow() {
    let code = compile_machine_code("backend/x86_reference_borrow.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_reference_dereference() {
    let code = compile_machine_code("backend/x86_reference_dereference.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_narrow_reference_dereference() {
    let code = compile_machine_code("backend/x86_narrow_reference_dereference.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_code_function_scoped_symbol_types() {
    let code = compile_machine_code("backend/x86_function_scoped_symbols.lex");

    insta::assert_snapshot!(machine_snapshot(&code));
}

#[test]
fn x86_machine_reports_unsupported_indexed_access() {
    insta::assert_snapshot!(
        compile_machine_code_error("backend/x86_unsupported_index.lex").join("\n")
    );
}

#[test]
fn x86_machine_reports_unsupported_extern_calls() {
    insta::assert_snapshot!(
        compile_machine_code_error("backend/x86_unsupported_extern_call.lex").join("\n")
    );
}

#[test]
fn x86_machine_reports_unsupported_vararg_call() {
    insta::assert_snapshot!(
        compile_machine_code_error("backend/x86_unsupported_vararg_call.lex").join("\n")
    );
}

#[test]
fn x86_machine_reports_unsupported_fixed_vararg_calls() {
    insta::assert_snapshot!(
        compile_machine_code_error("backend/x86_unsupported_vararg_fixed.lex").join("\n")
    );
}

#[test]
fn x86_machine_reports_unsupported_zero_fixed_vararg_calls() {
    insta::assert_snapshot!(compile_machine_code_error(
        "backend/x86_unsupported_vararg_zero_fixed.lex"
    )
    .join("\n"));
}

#[test]
fn x86_machine_reports_unsupported_function_values() {
    insta::assert_snapshot!(compile_machine_code_error(
        "backend/x86_unsupported_function_value.lex"
    )
    .join("\n"));
}

#[test]
fn x86_machine_reports_unsupported_shadowed_function_value_calls() {
    insta::assert_snapshot!(compile_machine_code_error(
        "backend/x86_unsupported_shadowed_function_value.lex"
    )
    .join("\n"));
}
