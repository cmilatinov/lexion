use std::process::Command;

#[test]
fn cli_emit_asm_writes_assembly_output_file() {
    let output_path = std::path::Path::new("target/test-dumps/cli_emit_asm/main.s");
    if output_path.exists() {
        std::fs::remove_file(output_path).unwrap();
    }

    let status = Command::new(env!("CARGO_BIN_EXE_main"))
        .arg("tests/fixtures/backend/x86_return_arithmetic.lex")
        .arg("--emit")
        .arg("asm")
        .arg("--output")
        .arg(output_path)
        .status()
        .expect("failed to run compiler");

    assert!(status.success());
    let assembly = std::fs::read_to_string(output_path).unwrap();
    assert!(assembly.contains(".global main"));
    assert!(assembly.contains("#     return value - 4;"));
}
