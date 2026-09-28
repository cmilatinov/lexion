use lexion_lang::diagnostic::LexionDiagnosticList;
use lexion_lang::generators::tac::instructions::ControlFlowGraph;
use lexion_lang::generators::tac::CodeGeneratorTac;
use lexion_lang::parser::ParserLexion;
use lexion_lang::pipeline::PipelineStage;
use lexion_lang::symbol_table::SymbolTableGenerator;
use lexion_lang::type_checker::TypeChecker;
use lexion_lib::miette::{NamedSource, SourceSpan};
use std::sync::Arc;

struct BackendOutput {
    cfg: ControlFlowGraph,
    source: Arc<String>,
}

fn compile_backend(fixture: &str) -> BackendOutput {
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
    BackendOutput {
        cfg,
        source: source_code,
    }
}

fn diagnostics_string(diagnostics: &LexionDiagnosticList) -> String {
    diagnostics
        .list
        .iter()
        .map(|diag| diag.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn backend_span_snapshot(output: &BackendOutput) -> String {
    output
        .cfg
        .node_indices()
        .map(|node| {
            let block = &output.cfg[node];
            let instructions = block
                .instructions
                .iter()
                .map(|inst| {
                    let span = inst
                        .source_span
                        .map(|span| format_span(output.source.as_str(), span))
                        .unwrap_or_else(|| String::from("<none>"));
                    format!("  {} => {}", inst.instruction, span)
                })
                .collect::<Vec<_>>()
                .join("\n");
            if instructions.is_empty() {
                format!("{}:", block.label)
            } else {
                format!("{}:\n{}", block.label, instructions)
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn format_span(source: &str, span: SourceSpan) -> String {
    let start = span.offset();
    let end = start.saturating_add(span.len().max(1)).min(source.len());
    let (start_line, start_col) = line_column(source, start);
    let (end_line, end_col) = line_column(source, end);
    let excerpt = source[start..end].replace('\r', "\\r").replace('\n', "\\n");
    format!(
        "{}:{}..{}:{} `{}`",
        start_line + 1,
        start_col + 1,
        end_line + 1,
        end_col + 1,
        excerpt
    )
}

fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let mut line_start = 0;
    let mut line = 0;
    for (idx, byte) in source.bytes().enumerate() {
        if idx >= offset {
            break;
        }
        if byte == b'\n' {
            line += 1;
            line_start = idx + 1;
        }
    }
    (line, offset.saturating_sub(line_start))
}

#[test]
fn backend_tac_instruction_spans() {
    let output = compile_backend("backend/branch_loop_call.lex");

    insta::assert_snapshot!(backend_span_snapshot(&output));
}
