use lexion_lang::diagnostic::LexionDiagnosticList;
use lexion_lang::generators::tac::instructions::ControlFlowGraph;
use lexion_lang::generators::tac::CodeGeneratorTac;
use lexion_lang::parser::ParserLexion;
use lexion_lang::pipeline::PipelineStage;
use lexion_lang::symbol_table::SymbolTableGenerator;
use lexion_lang::type_checker::TypeChecker;
use lexion_lib::miette::NamedSource;
use lexion_lib::petgraph::visit::EdgeRef;
use std::sync::Arc;

fn compile_cfg(fixture: &str) -> ControlFlowGraph {
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

    let (cfg, _) = CodeGeneratorTac::new((&ast, &mut symbols, &types))
        .exec(&mut diagnostics, ())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));
    cfg
}

fn diagnostics_string(diagnostics: &LexionDiagnosticList) -> String {
    diagnostics
        .list
        .iter()
        .map(|diag| diag.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn tac_snapshot(cfg: &ControlFlowGraph) -> String {
    cfg.node_indices()
        .map(|node| {
            let block = &cfg[node];
            let instructions = block
                .instructions
                .iter()
                .map(|inst| format!("  {}", inst.instruction))
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

fn cfg_snapshot(cfg: &ControlFlowGraph) -> String {
    let mut functions = cfg
        .functions
        .iter()
        .map(|range| format!("function {}..{}", range.start.index(), range.end.index()))
        .collect::<Vec<_>>();
    functions.sort();

    let mut edges = cfg
        .edge_references()
        .map(|edge| {
            let source = edge.source();
            let target = edge.target();
            format!(
                "{}({}) -> {}({})",
                source.index(),
                cfg[source].label,
                target.index(),
                cfg[target].label
            )
        })
        .collect::<Vec<_>>();
    edges.sort();

    format!("{}\n\n{}", functions.join("\n"), edges.join("\n"))
}

#[test]
fn backend_branch_loop_call_tac_snapshot() {
    let cfg = compile_cfg("backend/branch_loop_call.lex");

    insta::assert_snapshot!(tac_snapshot(&cfg));
}

#[test]
fn backend_branch_loop_call_cfg_snapshot() {
    let cfg = compile_cfg("backend/branch_loop_call.lex");

    insta::assert_snapshot!(cfg_snapshot(&cfg));
}
