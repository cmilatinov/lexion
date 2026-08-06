use iced_x86::Register;
use lexion_lang::diagnostic::LexionDiagnosticList;
use lexion_lang::generators::tac::CodeGeneratorTac;
use lexion_lang::generators::x86::{
    AbiRegisterAllocator, Bitness, CMemoryLayoutBuilder, CodeGeneratorX86, LinearRegisterAllocator,
    X86EmitOptions, X86Target,
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
    types.compute_memory_layouts::<CMemoryLayoutBuilder>(Bitness::_64);

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

fn compile_x86_with_registers(fixture: &str, registers: Vec<Register>) -> String {
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
    types.compute_memory_layouts::<CMemoryLayoutBuilder>(Bitness::_64);

    let (cfg, intervals) = CodeGeneratorTac::new((&ast, &mut symbols, &types))
        .exec(&mut diagnostics, ())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));
    let assigned = LinearRegisterAllocator::new((&cfg, registers))
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
    types.compute_memory_layouts::<CMemoryLayoutBuilder>(Bitness::_64);

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
fn x86_smoke_f32_function_calls() {
    let assembly = compile_x86("backend/x86_f32_function_calls.lex");

    for instruction in [
        "mov DWORD PTR [rsp], 0x3F800000",
        "movss DWORD PTR [rbp-",
        "movss xmm15, DWORD PTR [rbp+16]",
        "movss xmm0, DWORD PTR [rsp]",
        "movss xmm7, DWORD PTR [rsp+56]",
        "mov rdi, QWORD PTR [rsp+72]",
        "sub rsp, 8\n  mov rax, QWORD PTR [rsp+8]\n  mov QWORD PTR [rsp], rax\n  call sum9\n  add rsp, 16",
        "mov QWORD PTR [rsp], rax",
        "call scale",
        "call sum9",
        "call f32_tail",
        "add rsp, 96",
    ] {
        assert!(
            assembly.contains(instruction),
            "missing expected f32 ABI instruction `{instruction}`:\n{assembly}"
        );
    }
}

#[test]
fn x86_smoke_indexed_aggregate_stack_call() {
    insta::assert_snapshot!(compile_x86("backend/x86_indexed_aggregate_stack_call.lex"));
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
fn x86_smoke_local_aggregate_values() {
    insta::assert_snapshot!(compile_x86("backend/x86_local_aggregates.lex"));
}

#[test]
fn x86_smoke_one_eightbyte_aggregate_abi_values() {
    insta::assert_snapshot!(compile_x86("backend/x86_aggregate_abi.lex"));
}

#[test]
fn x86_smoke_one_eightbyte_aggregate_abi_padding() {
    insta::assert_snapshot!(compile_x86("backend/x86_aggregate_abi_padding.lex"));
}

#[test]
fn x86_smoke_nested_reference_aggregate_abi_values() {
    compile_x86("backend/x86_nested_reference_aggregate_abi.lex");
}

#[test]
fn x86_smoke_register_pair_aggregate_abi_values() {
    let assembly = compile_x86("backend/x86_register_pair_aggregates.lex");
    let shift_quad = assembly
        .split_once("shift_quad:\n")
        .and_then(|(_, assembly)| assembly.split_once("shift_tuple:\n"))
        .map(|(assembly, _)| assembly)
        .expect("missing shift_quad assembly");
    let high_load = shift_quad
        .find("mov rdx, QWORD PTR [rsp]")
        .expect("missing RDX return-half load");
    let low_load = shift_quad
        .find("mov rax, QWORD PTR [rsp]")
        .expect("missing RAX return-half load");
    assert!(
        high_load < low_load,
        "register-pair return must load RDX before RAX scratch clobbers it:\n{shift_quad}"
    );
    insta::assert_snapshot!(assembly);
}

#[test]
fn x86_smoke_register_pair_aggregate_abi_padding() {
    insta::assert_snapshot!(compile_x86(
        "backend/x86_register_pair_aggregate_padding.lex"
    ));
}

#[test]
fn x86_smoke_indexed_register_pair_call_arguments() {
    let assembly = compile_x86("backend/x86_indexed_register_pair_call.lex");
    assert!(
        assembly.contains("mov rdi, QWORD PTR [rsp+72]\n  mov rsi, QWORD PTR [rsp+80]"),
        "indexed call did not load the register pair from its staged slots:\n{assembly}"
    );
}

#[test]
fn x86_smoke_indirect_aggregate_returns() {
    insta::assert_snapshot!(compile_x86("backend/x86_indirect_aggregate_returns.lex"));
}

#[test]
fn x86_smoke_indexed_indirect_return_stack_pair_arguments() {
    insta::assert_snapshot!(compile_x86(
        "backend/x86_indexed_indirect_return_stack_pair_arguments.lex"
    ));
}

#[test]
fn x86_smoke_aggregate_member_values() {
    insta::assert_snapshot!(compile_x86("backend/x86_aggregate_members.lex"));
}

#[test]
fn x86_reports_unsupported_float_casts() {
    insta::assert_snapshot!(compile_x86_error("backend/x86_unsupported_float_cast.lex").join("\n"));
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
fn x86_reports_unsupported_tuple_parameters() {
    insta::assert_snapshot!(compile_x86_error("backend/x86_unsupported_tuple.lex").join("\n"));
}

#[test]
fn x86_reports_unsupported_call_tuple_arg() {
    insta::assert_snapshot!(
        compile_x86_error("backend/x86_unsupported_call_tuple_arg.lex").join("\n")
    );
}

#[test]
fn x86_reports_unsupported_indirect_float_aggregates() {
    insta::assert_snapshot!(compile_x86_error(
        "backend/x86_unsupported_indirect_float_aggregate.lex"
    )
    .join("\n"));
}

#[test]
fn x86_reports_unsupported_struct_parameters() {
    insta::assert_snapshot!(compile_x86_error("backend/x86_unsupported_struct.lex").join("\n"));
}

#[test]
fn x86_reports_unsupported_call_struct_arg() {
    insta::assert_snapshot!(
        compile_x86_error("backend/x86_unsupported_call_struct_arg.lex").join("\n")
    );
}

#[test]
fn x86_reports_unsupported_aggregate_members() {
    insta::assert_snapshot!(
        compile_x86_error("backend/x86_unsupported_aggregate_members.lex").join("\n")
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
fn x86_smoke_reference_call() {
    insta::assert_snapshot!(compile_x86("backend/x86_reference_call.lex"));
}

#[test]
fn x86_smoke_reference_stack_arguments() {
    insta::assert_snapshot!(compile_x86("backend/x86_reference_stack_arguments.lex"));
}

#[test]
fn x86_smoke_reference_dereference() {
    insta::assert_snapshot!(compile_x86("backend/x86_reference_dereference.lex"));
}

#[test]
fn x86_smoke_narrow_reference_dereference() {
    insta::assert_snapshot!(compile_x86("backend/x86_narrow_reference_dereference.lex"));
}

#[test]
fn x86_smoke_aggregate_reference_places() {
    insta::assert_snapshot!(compile_x86("backend/x86_aggregate_reference_places.lex"));
}

#[test]
fn x86_aggregate_reference_copies_preserve_allocated_rax() {
    let assembly = compile_x86("backend/x86_aggregate_reference_places.lex");

    assert!(
        assembly
            .matches("push rax\n  mov rdx, QWORD PTR [rbp-104]")
            .count()
            >= 2,
        "aggregate reference copies did not preserve an allocated RAX:\n{assembly}"
    );
}

#[test]
fn x86_smoke_projected_aggregate_references() {
    insta::assert_snapshot!(compile_x86(
        "backend/x86_projected_aggregate_references.lex"
    ));
}

#[test]
fn x86_member_borrow_preserves_live_rax() {
    insta::assert_snapshot!(compile_x86("backend/x86_member_borrow_preserves_rax.lex"));
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
fn x86_smoke_function_values() {
    insta::assert_snapshot!(compile_x86("backend/x86_function_values.lex"));
}

#[test]
fn x86_smoke_function_value_returns() {
    insta::assert_snapshot!(compile_x86("backend/x86_function_value_returns.lex"));
}

#[test]
fn x86_smoke_function_value_members() {
    insta::assert_snapshot!(compile_x86("backend/x86_function_value_members.lex"));
}

#[test]
fn x86_smoke_function_value_member_return() {
    insta::assert_snapshot!(compile_x86("backend/x86_function_value_member_return.lex"));
}

#[test]
fn x86_smoke_function_value_aggregate_abi() {
    insta::assert_snapshot!(compile_x86("backend/x86_function_value_aggregate_abi.lex"));
}

#[test]
fn x86_smoke_function_value_preserves_live_rax() {
    insta::assert_snapshot!(compile_x86_with_registers(
        "backend/x86_function_value_live_rax.lex",
        vec![Register::RAX],
    ));
}

#[test]
fn x86_smoke_function_value_stages_indirect_target_before_arguments() {
    insta::assert_snapshot!(compile_x86_with_registers(
        "backend/x86_function_value_indirect_target.lex",
        vec![Register::RAX],
    ));
}

#[test]
fn x86_smoke_function_value_dereference_store_preserves_rax_callback() {
    insta::assert_snapshot!(compile_x86_with_registers(
        "backend/x86_function_value_dereference_store.lex",
        vec![Register::RAX],
    ));
}

#[test]
fn x86_smoke_function_value_dereference_store_preserves_live_scratch_registers() {
    insta::assert_snapshot!(compile_x86_with_registers(
        "backend/x86_function_value_dereference_store_live_scratch.lex",
        vec![Register::RAX, Register::RCX],
    ));
}

#[test]
fn x86_smoke_zero_arg_indirect_aggregate_return_stages_rdi_callback() {
    insta::assert_snapshot!(compile_x86_with_registers(
        "backend/x86_zero_arg_indirect_aggregate_return.lex",
        vec![Register::RDI],
    ));
}

#[test]
fn x86_smoke_function_value_stages_indirect_target_without_clobbering_rax_or_r11() {
    insta::assert_snapshot!(compile_x86_with_registers(
        "backend/x86_function_value_indirect_target_register_conflict.lex",
        vec![Register::RAX, Register::R11],
    ));
}

#[test]
fn x86_smoke_function_value_dereference() {
    insta::assert_snapshot!(compile_x86("backend/x86_function_value_dereference.lex"));
}

#[test]
fn x86_smoke_nested_function_value_argument() {
    insta::assert_snapshot!(compile_x86("backend/x86_nested_function_value.lex"));
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
fn x86_smoke_unit_arguments() {
    insta::assert_snapshot!(compile_x86("backend/x86_unit_arguments.lex"));
}

#[test]
fn x86_smoke_zero_sized_aggregate_argument() {
    insta::assert_snapshot!(compile_x86("backend/x86_zero_sized_aggregate_argument.lex"));
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
