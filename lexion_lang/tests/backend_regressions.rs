use iced_x86::Register;
use lexion_lang::diagnostic::LexionDiagnosticList;
use lexion_lang::generators::tac::instructions::{
    ControlFlowGraph, FunctionRange, LivenessInterval,
};
use lexion_lang::generators::tac::CodeGeneratorTac;
use lexion_lang::generators::x86::{AssignedLivenessInterval, LinearRegisterAllocator};
use lexion_lang::parser::ParserLexion;
use lexion_lang::pipeline::PipelineStage;
use lexion_lang::symbol_table::SymbolTableGenerator;
use lexion_lang::type_checker::TypeChecker;
use lexion_lib::miette::NamedSource;
use lexion_lib::petgraph::visit::EdgeRef;
use std::collections::HashMap;
use std::sync::Arc;

struct BackendOutput {
    cfg: ControlFlowGraph,
    intervals: HashMap<FunctionRange, Vec<LivenessInterval>>,
}

fn compile_cfg(fixture: &str) -> ControlFlowGraph {
    compile_backend(fixture).cfg
}

fn compile_backend(fixture: &str) -> BackendOutput {
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

    let (cfg, intervals) = CodeGeneratorTac::new((&ast, &mut symbols, &types))
        .exec(&mut diagnostics, ())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));
    BackendOutput { cfg, intervals }
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

fn liveness_snapshot(output: &BackendOutput) -> String {
    sorted_intervals(&output.intervals)
        .into_iter()
        .map(|(range, interval)| format_interval(range, interval))
        .collect::<Vec<_>>()
        .join("\n")
}

fn allocation_snapshot(output: BackendOutput) -> String {
    let mut diagnostics = LexionDiagnosticList::default();
    let assigned = LinearRegisterAllocator::new((
        &output.cfg,
        vec![
            Register::RAX,
            Register::RCX,
            Register::RDX,
            Register::RSI,
            Register::RDI,
            Register::R8,
            Register::R9,
            Register::R10,
            Register::R11,
        ],
    ))
    .exec(&mut diagnostics, output.intervals)
    .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));

    sorted_assignments(&assigned)
        .into_iter()
        .map(|(range, assigned)| {
            format!(
                "{} -> {:?}",
                format_interval(range, assigned.interval()),
                assigned.location()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn sorted_intervals(
    intervals: &HashMap<FunctionRange, Vec<LivenessInterval>>,
) -> Vec<(FunctionRange, &LivenessInterval)> {
    let mut result = intervals
        .iter()
        .flat_map(|(range, intervals)| intervals.iter().map(|interval| (*range, interval)))
        .collect::<Vec<_>>();
    result.sort_by_key(|(range, interval)| {
        (
            range.start.index(),
            range.end.index(),
            interval.variable.clone(),
            interval.span.start.block.index(),
            interval.span.start.instruction,
            interval.span.end.block.index(),
            interval.span.end.instruction,
        )
    });
    result
}

fn sorted_assignments(
    assigned: &HashMap<FunctionRange, Vec<AssignedLivenessInterval>>,
) -> Vec<(FunctionRange, &AssignedLivenessInterval)> {
    let mut result = assigned
        .iter()
        .flat_map(|(range, assigned)| assigned.iter().map(|assigned| (*range, assigned)))
        .collect::<Vec<_>>();
    result.sort_by_key(|(range, assigned)| {
        let interval = assigned.interval();
        (
            range.start.index(),
            range.end.index(),
            interval.variable.clone(),
            interval.span.start.block.index(),
            interval.span.start.instruction,
            interval.span.end.block.index(),
            interval.span.end.instruction,
            format!("{:?}", assigned.location()),
        )
    });
    result
}

fn format_interval(range: FunctionRange, interval: &LivenessInterval) -> String {
    let uses = interval
        .uses
        .iter()
        .map(|loc| format!("{}:{}", loc.block.index(), loc.instruction))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "function {}..{} {} [{}:{}..{}:{}] uses [{}]",
        range.start.index(),
        range.end.index(),
        interval.variable,
        interval.span.start.block.index(),
        interval.span.start.instruction,
        interval.span.end.block.index(),
        interval.span.end.instruction,
        uses
    )
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

#[test]
fn backend_branch_loop_call_liveness_snapshot() {
    let output = compile_backend("backend/branch_loop_call.lex");

    insta::assert_snapshot!(liveness_snapshot(&output));
}

#[test]
fn backend_branch_loop_call_register_allocation_snapshot() {
    let output = compile_backend("backend/branch_loop_call.lex");

    insta::assert_snapshot!(allocation_snapshot(output));
}
