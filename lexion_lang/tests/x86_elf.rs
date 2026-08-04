use iced_x86::{Decoder, DecoderOptions, Formatter, IntelFormatter};
use lexion_lang::compiler::{EmitTarget, LexionCompiler, LexionCompilerOptions};
use lexion_lang::diagnostic::LexionDiagnosticList;
use lexion_lang::generators::tac::CodeGeneratorTac;
use lexion_lang::generators::x86::{
    Bitness, CMemoryLayoutBuilder, CodeGeneratorX86Elf, X86ElfExecutable, X86ElfOptions,
};
use lexion_lang::parser::ParserLexion;
use lexion_lang::pipeline::PipelineStage;
use lexion_lang::symbol_table::SymbolTableGenerator;
use lexion_lang::type_checker::TypeChecker;
use lexion_lib::miette::NamedSource;
use std::sync::Arc;

fn compile_elf(fixture: &str) -> X86ElfExecutable {
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
    CodeGeneratorX86Elf::new((&cfg, &types, &symbols))
        .exec(&mut diagnostics, X86ElfOptions::default())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)))
}

fn compile_elf_error(fixture: &str) -> Vec<String> {
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
    let output = CodeGeneratorX86Elf::new((&cfg, &types, &symbols))
        .exec(&mut diagnostics, X86ElfOptions::default());

    assert!(
        output.is_none(),
        "expected x86 ELF backend to reject fixture"
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

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn disassemble(bytes: &[u8], ip: u64) -> String {
    let mut decoder = Decoder::with_ip(64, bytes, ip, DecoderOptions::NONE);
    let mut formatter = IntelFormatter::new();
    let mut lines = Vec::new();
    while decoder.can_decode() {
        let instruction = decoder.decode();
        let mut formatted = String::new();
        formatter.format(&instruction, &mut formatted);
        lines.push(format!("{:016X}: {formatted}", instruction.ip()));
    }
    lines.join("\n")
}

#[test]
fn x86_elf_executable_has_expected_header() {
    let executable = compile_elf("backend/x86_return_arithmetic.lex");
    let bytes = executable.as_bytes();
    let program_header = 64;

    assert_eq!(&bytes[0..4], b"\x7FELF");
    assert_eq!(bytes[4], 2);
    assert_eq!(bytes[5], 1);
    assert_eq!(read_u16(bytes, 16), 2);
    assert_eq!(read_u16(bytes, 18), 62);
    assert_eq!(read_u64(bytes, 24), executable.entry_point());
    assert_eq!(read_u64(bytes, 32), 64);
    assert_eq!(read_u16(bytes, 52), 64);
    assert_eq!(read_u16(bytes, 54), 56);
    assert_eq!(read_u16(bytes, 56), 1);

    assert_eq!(read_u32(bytes, program_header), 1);
    assert_eq!(read_u32(bytes, program_header + 4), 5);
    assert_eq!(read_u64(bytes, program_header + 8), 0);
    assert_eq!(read_u64(bytes, program_header + 16), 0x400000);
    assert_eq!(read_u64(bytes, program_header + 32), bytes.len() as u64);
    assert_eq!(read_u64(bytes, program_header + 40), bytes.len() as u64);
    assert_eq!(read_u64(bytes, program_header + 48), 0x1000);

    assert_eq!(executable.entry_point(), 0x401000);
    assert_eq!(executable.text_offset(), 0x1000);
    assert_eq!(
        executable.symbols().get("main"),
        Some(&(executable.entry_point() + executable.runtime_size() as u64))
    );
}

#[test]
fn x86_elf_executable_has_runtime_entry() {
    let executable = compile_elf("backend/x86_return_arithmetic.lex");
    let runtime_start = executable.text_offset();
    let runtime_end = runtime_start + executable.runtime_size();
    let runtime = &executable.as_bytes()[runtime_start..runtime_end];

    insta::assert_snapshot!(disassemble(runtime, executable.entry_point()));
}

#[test]
fn compiler_emits_x86_elf_executable_output() {
    let path = "tests/fixtures/backend/x86_exit_status.lex";
    let source_code = Arc::new(std::fs::read_to_string(path).expect("fixture not found"));
    let source = NamedSource::new(path, source_code);
    let options = LexionCompilerOptions {
        emit: EmitTarget::X86Elf64,
        ..LexionCompilerOptions::default()
    };
    let output = LexionCompiler::new(options)
        .exec(source)
        .expect("compiler should emit x86 elf output");
    let executable = output.executable.expect("missing executable output");

    assert!(output.diagnostics.is_empty());
    assert_eq!(&executable.as_bytes()[0..4], b"\x7FELF");
    assert_eq!(
        read_u64(executable.as_bytes(), 24),
        executable.entry_point()
    );
    assert!(executable.symbols().contains_key("main"));
}

#[test]
fn compiler_emits_x86_elf_for_unfolded_target_word_overflow() {
    let source_code = Arc::new(String::from(
        "fn main() -> i32 {\n    return 2147483647 + 1;\n}\n",
    ));
    let source = NamedSource::new("target_word_overflow.lex", source_code);
    let options = LexionCompilerOptions {
        emit: EmitTarget::X86Elf64,
        ..LexionCompilerOptions::default()
    };
    let output = LexionCompiler::new(options)
        .exec(source)
        .expect("compiler should emit x86 elf output");

    assert!(output.diagnostics.is_empty());
    assert!(output.executable.is_some());
}

#[test]
fn x86_elf_executable_supports_stack_arguments() {
    let executable = compile_elf("backend/x86_stack_arguments.lex");
    let code_start = executable.text_offset() + executable.runtime_size();
    let code = &executable.as_bytes()[code_start..];

    assert!(executable.symbols().contains_key("combine"));
    assert!(executable.symbols().contains_key("main"));
    insta::assert_snapshot!(disassemble(
        code,
        executable.entry_point() + executable.runtime_size() as u64
    ));
}

#[test]
fn x86_elf_executable_supports_f32_function_calls() {
    let executable = compile_elf("backend/x86_f32_function_calls.lex");
    let code_start = executable.text_offset() + executable.runtime_size();
    let code = &executable.as_bytes()[code_start..];

    assert!(executable.symbols().contains_key("scale"));
    assert!(executable.symbols().contains_key("sum9"));
    insta::assert_snapshot!(disassemble(
        code,
        executable.entry_point() + executable.runtime_size() as u64
    ));
}

#[test]
fn x86_elf_reports_unsupported_extern_calls() {
    insta::assert_snapshot!(compile_elf_error("backend/x86_unsupported_extern_call.lex").join("\n"));
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn run_executable_fixture(fixture: &str) -> Option<i32> {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let executable = compile_elf(fixture);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "lexion-x86-elf-{}-{}",
        std::process::id(),
        fixture.replace(['/', '\\', '.'], "-")
    ));
    std::fs::write(&path, executable.as_bytes()).expect("failed to write executable");
    let mut permissions = std::fs::metadata(&path)
        .expect("failed to stat executable")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("failed to chmod executable");

    let status = Command::new(&path)
        .status()
        .expect("failed to run executable");
    let _ = std::fs::remove_file(&path);

    status.code()
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn x86_elf_executable_runs_on_linux_x86_64() {
    assert_eq!(
        run_executable_fixture("backend/x86_exit_status.lex"),
        Some(7)
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn x86_elf_function_call_executable_runs_on_linux_x86_64() {
    assert_eq!(
        run_executable_fixture("backend/x86_function_call.lex"),
        Some(9)
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn x86_elf_local_reference_executable_runs_on_linux_x86_64() {
    assert_eq!(
        run_executable_fixture("backend/x86_reference_dereference.lex"),
        Some(2)
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn x86_elf_narrow_reference_executable_runs_on_linux_x86_64() {
    assert_eq!(
        run_executable_fixture("backend/x86_narrow_reference_dereference.lex"),
        Some(0)
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn x86_elf_unit_arguments_run_on_linux_x86_64() {
    assert_eq!(
        run_executable_fixture("backend/x86_unit_arguments.lex"),
        Some(7)
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn x86_elf_f32_function_call_executable_runs_on_linux_x86_64() {
    assert_eq!(
        run_executable_fixture("backend/x86_f32_function_calls.lex"),
        Some(0)
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn x86_elf_reference_call_executable_runs_on_linux_x86_64() {
    assert_eq!(
        run_executable_fixture("backend/x86_reference_call.lex"),
        Some(5)
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn x86_elf_reference_stack_arguments_run_on_linux_x86_64() {
    assert_eq!(
        run_executable_fixture("backend/x86_reference_stack_arguments.lex"),
        Some(9)
    );
}

#[test]
fn x86_elf_executable_supports_local_aggregate_values() {
    let executable = compile_elf("backend/x86_local_aggregates.lex");

    assert!(executable.symbols().contains_key("main"));
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn x86_elf_local_aggregate_values_run_on_linux_x86_64() {
    assert_eq!(
        run_executable_fixture("backend/x86_local_aggregates.lex"),
        Some(16)
    );
}
