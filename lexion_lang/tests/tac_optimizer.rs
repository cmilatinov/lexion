use lexion_lang::ast::Lit;
use lexion_lang::diagnostic::LexionDiagnosticList;
use lexion_lang::generators::tac::instructions::{
    AssignmentInstruction, ConditionalJumpInstruction, ControlFlowGraph, CopyInstruction,
    FunctionCallInstruction, Instruction, InstructionInstance, JumpInstruction, LiveSets, Operand,
    ReturnInstruction,
};
use lexion_lang::generators::tac::{
    analyze_liveness, CodeGeneratorTac, CodeOptimizerTac, TacOptimizerOptions,
};
use lexion_lang::operators;
use lexion_lang::parser::ParserLexion;
use lexion_lang::pipeline::PipelineStage;
use lexion_lang::symbol_table::SymbolTableGenerator;
use lexion_lang::type_checker::TypeChecker;
use lexion_lib::miette::NamedSource;
use lexion_lib::petgraph::visit::EdgeRef;
use std::sync::Arc;

fn compile_raw_cfg(fixture: &str) -> ControlFlowGraph {
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

    CodeGeneratorTac::new((&ast, &mut symbols, &types))
        .exec(&mut diagnostics, ())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)))
        .0
}

fn optimize_cfg(fixture: &str, options: TacOptimizerOptions) -> ControlFlowGraph {
    let mut diagnostics = LexionDiagnosticList::default();
    CodeOptimizerTac::new(compile_raw_cfg(fixture))
        .exec(&mut diagnostics, options)
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)))
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

fn optimized_snapshot(cfg: &ControlFlowGraph) -> String {
    format!("tac:\n{}\n\ncfg:\n{}", tac_snapshot(cfg), cfg_snapshot(cfg))
}

fn instruction(instruction: Instruction) -> InstructionInstance {
    InstructionInstance {
        instruction,
        source_span: None,
        live: LiveSets::default(),
    }
}

fn jump_chain_cfg() -> ControlFlowGraph {
    let mut cfg = ControlFlowGraph::new();
    let start = cfg.block(String::from("main"), true);
    cfg[start]
        .instructions
        .push(instruction(Instruction::Jump(JumpInstruction {
            target: Operand::Label(String::from("middle")),
        })));
    let middle = cfg.block(String::from("middle"), false);
    cfg[middle]
        .instructions
        .push(instruction(Instruction::Jump(JumpInstruction {
            target: Operand::Label(String::from("end")),
        })));
    let end = cfg.block(String::from("end"), false);
    cfg[end]
        .instructions
        .push(instruction(Instruction::Return(ReturnInstruction {
            value: Some(Operand::Literal(lexion_lang::ast::Lit::Integer(0))),
        })));
    cfg.link(start, middle);
    cfg.link(middle, end);
    cfg.end_function();
    cfg
}

fn redundant_return_copy_cfg() -> ControlFlowGraph {
    let mut cfg = ControlFlowGraph::new();
    let temp = sample_temporary_operand();
    let main = cfg.block(String::from("main"), true);
    cfg[main]
        .instructions
        .push(instruction(Instruction::Copy(CopyInstruction {
            dst: temp.clone(),
            src: Operand::Variable(String::from("value")),
        })));
    cfg[main]
        .instructions
        .push(instruction(Instruction::Return(ReturnInstruction {
            value: Some(temp),
        })));
    cfg.end_function();
    cfg
}

fn sample_temporary_operand() -> Operand {
    let cfg = compile_raw_cfg("backend/tac_constant_folding.lex");
    cfg.node_weights()
        .flat_map(|block| block.instructions.iter())
        .find_map(|inst| match &inst.instruction {
            Instruction::Assignment(assignment) if assignment.target.is_temporary() => {
                Some(assignment.target.clone())
            }
            _ => None,
        })
        .expect("fixture should contain a temporary")
}

fn target_word_overflow_cfg() -> ControlFlowGraph {
    let mut cfg = ControlFlowGraph::new();
    let start = cfg.block(String::from("main"), true);
    cfg[start]
        .instructions
        .push(instruction(Instruction::Assignment(
            AssignmentInstruction {
                target: Operand::Variable(String::from("value")),
                left: Some(Operand::Literal(Lit::Integer(i32::MAX as isize))),
                operator: operators::PLUS,
                right: Operand::Literal(Lit::Integer(1)),
            },
        )));
    cfg.end_function();
    cfg
}

fn propagation_algebra_cfg() -> ControlFlowGraph {
    let mut cfg = ControlFlowGraph::new();
    let start = cfg.block(String::from("main"), true);
    cfg[start]
        .instructions
        .push(instruction(Instruction::Copy(CopyInstruction {
            dst: Operand::Variable(String::from("a")),
            src: Operand::Literal(Lit::Integer(2)),
        })));
    cfg[start]
        .instructions
        .push(instruction(Instruction::Copy(CopyInstruction {
            dst: Operand::Variable(String::from("b")),
            src: Operand::Variable(String::from("a")),
        })));
    cfg[start]
        .instructions
        .push(instruction(Instruction::Assignment(
            AssignmentInstruction {
                target: Operand::Variable(String::from("c")),
                left: Some(Operand::Variable(String::from("b"))),
                operator: operators::PLUS,
                right: Operand::Literal(Lit::Integer(0)),
            },
        )));
    cfg[start]
        .instructions
        .push(instruction(Instruction::Assignment(
            AssignmentInstruction {
                target: Operand::Variable(String::from("d")),
                left: Some(Operand::Variable(String::from("c"))),
                operator: operators::MULTIPLY,
                right: Operand::Literal(Lit::Integer(1)),
            },
        )));
    cfg[start]
        .instructions
        .push(instruction(Instruction::Assignment(
            AssignmentInstruction {
                target: Operand::Variable(String::from("e")),
                left: Some(Operand::Variable(String::from("d"))),
                operator: operators::MULTIPLY,
                right: Operand::Literal(Lit::Integer(0)),
            },
        )));
    cfg[start]
        .instructions
        .push(instruction(Instruction::Return(ReturnInstruction {
            value: Some(Operand::Variable(String::from("e"))),
        })));
    cfg.end_function();
    cfg
}

fn branch_inversion_cfg() -> ControlFlowGraph {
    let mut cfg = ControlFlowGraph::new();
    let start = cfg.block(String::from("main"), true);
    cfg[start]
        .instructions
        .push(instruction(Instruction::ConditionalJump(
            ConditionalJumpInstruction {
                left: Some(Operand::Literal(Lit::Boolean(false))),
                operator: operators::EQUALS,
                right: Operand::Variable(String::from("flag")),
                target: Operand::Label(String::from("then")),
            },
        )));
    cfg[start]
        .instructions
        .push(instruction(Instruction::Jump(JumpInstruction {
            target: Operand::Label(String::from("else")),
        })));
    let then = cfg.block(String::from("then"), false);
    cfg[then]
        .instructions
        .push(instruction(Instruction::Return(ReturnInstruction {
            value: Some(Operand::Literal(Lit::Integer(1))),
        })));
    let else_ = cfg.block(String::from("else"), false);
    cfg[else_]
        .instructions
        .push(instruction(Instruction::Return(ReturnInstruction {
            value: Some(Operand::Literal(Lit::Integer(2))),
        })));
    cfg.link(start, then);
    cfg.link(start, else_);
    cfg.end_function();
    cfg
}

fn call_branch_barrier_cfg() -> ControlFlowGraph {
    let mut cfg = ControlFlowGraph::new();
    let start = cfg.block(String::from("main"), true);
    cfg[start]
        .instructions
        .push(instruction(Instruction::Copy(CopyInstruction {
            dst: Operand::Variable(String::from("flag")),
            src: Operand::Literal(Lit::Boolean(true)),
        })));
    cfg[start]
        .instructions
        .push(instruction(Instruction::FunctionCall(
            FunctionCallInstruction {
                function: String::from("touch"),
                function_type: None,
                is_direct_function: true,
                return_target: None,
            },
        )));
    cfg[start]
        .instructions
        .push(instruction(Instruction::ConditionalJump(
            ConditionalJumpInstruction {
                left: Some(Operand::Literal(Lit::Boolean(false))),
                operator: operators::EQUALS,
                right: Operand::Variable(String::from("flag")),
                target: Operand::Label(String::from("else")),
            },
        )));
    let then = cfg.block(String::from("then"), false);
    cfg[then]
        .instructions
        .push(instruction(Instruction::Return(ReturnInstruction {
            value: Some(Operand::Literal(Lit::Integer(1))),
        })));
    let else_ = cfg.block(String::from("else"), false);
    cfg[else_]
        .instructions
        .push(instruction(Instruction::Return(ReturnInstruction {
            value: Some(Operand::Literal(Lit::Integer(2))),
        })));
    cfg.link(start, then);
    cfg.link(start, else_);
    cfg.end_function();
    cfg
}

#[test]
fn tac_optimizer_noop_preserves_tac_and_cfg() {
    let raw = compile_raw_cfg("backend/branch_loop_call.lex");
    let noop = optimize_cfg("backend/branch_loop_call.lex", TacOptimizerOptions::none());

    assert_eq!(tac_snapshot(&raw), tac_snapshot(&noop));
    assert_eq!(cfg_snapshot(&raw), cfg_snapshot(&noop));
    insta::assert_snapshot!(optimized_snapshot(&noop));
}

#[test]
fn tac_optimizer_folds_constant_expressions() {
    let mut optimized = optimize_cfg(
        "backend/tac_constant_folding.lex",
        TacOptimizerOptions::default(),
    );
    let _ = analyze_liveness(&mut optimized);

    insta::assert_snapshot!(optimized_snapshot(&optimized));
}

#[test]
fn tac_optimizer_simplifies_constant_branches() {
    let optimized = optimize_cfg(
        "backend/tac_constant_branch.lex",
        TacOptimizerOptions::default(),
    );

    insta::assert_snapshot!(optimized_snapshot(&optimized));
}

#[test]
fn tac_optimizer_simplifies_jump_chains() {
    let mut diagnostics = LexionDiagnosticList::default();
    let optimized = CodeOptimizerTac::new(jump_chain_cfg())
        .exec(&mut diagnostics, TacOptimizerOptions::default())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));

    insta::assert_snapshot!(optimized_snapshot(&optimized));
}

#[test]
fn tac_optimizer_propagates_values_and_simplifies_algebra() {
    let mut diagnostics = LexionDiagnosticList::default();
    let optimized = CodeOptimizerTac::new(propagation_algebra_cfg())
        .exec(&mut diagnostics, TacOptimizerOptions::default())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));

    insta::assert_snapshot!(optimized_snapshot(&optimized));
}

#[test]
fn tac_optimizer_eliminates_common_subexpressions() {
    let optimized = optimize_cfg(
        "backend/tac_common_subexpression.lex",
        TacOptimizerOptions::default(),
    );

    insta::assert_snapshot!(optimized_snapshot(&optimized));
}

#[test]
fn tac_optimizer_eliminates_dead_temporaries() {
    let optimized = optimize_cfg("backend/tac_dead_temp.lex", TacOptimizerOptions::default());

    insta::assert_snapshot!(optimized_snapshot(&optimized));
}

#[test]
fn tac_optimizer_eliminates_dead_temporaries_without_dropping_calls() {
    let optimized = optimize_cfg(
        "backend/tac_dead_temporaries.lex",
        TacOptimizerOptions::default(),
    );

    insta::assert_snapshot!(optimized_snapshot(&optimized));
}

#[test]
fn tac_optimizer_collapses_redundant_copy_before_return() {
    let mut diagnostics = LexionDiagnosticList::default();
    let optimized = CodeOptimizerTac::new(redundant_return_copy_cfg())
        .exec(&mut diagnostics, TacOptimizerOptions::default())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));

    insta::assert_snapshot!(optimized_snapshot(&optimized));
}

#[test]
fn tac_optimizer_preserves_dead_trapping_temporaries() {
    let optimized = optimize_cfg(
        "backend/tac_dead_trapping_temp.lex",
        TacOptimizerOptions::default(),
    );

    insta::assert_snapshot!(optimized_snapshot(&optimized));
}

#[test]
fn tac_optimizer_inverts_branches_to_remove_redundant_jumps() {
    let mut diagnostics = LexionDiagnosticList::default();
    let optimized = CodeOptimizerTac::new(branch_inversion_cfg())
        .exec(
            &mut diagnostics,
            TacOptimizerOptions {
                constant_folding: false,
                value_propagation: false,
                common_subexpression_elimination: false,
                cfg_cleanup: false,
                dead_code_elimination: false,
                ..TacOptimizerOptions::default()
            },
        )
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));

    insta::assert_snapshot!(optimized_snapshot(&optimized));
}

#[test]
fn tac_optimizer_keeps_branch_facts_across_calls_conservative() {
    let mut diagnostics = LexionDiagnosticList::default();
    let optimized = CodeOptimizerTac::new(call_branch_barrier_cfg())
        .exec(&mut diagnostics, TacOptimizerOptions::default())
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));

    insta::assert_snapshot!(optimized_snapshot(&optimized));
}

#[test]
fn tac_optimizer_skips_integer_folds_outside_target_word() {
    let mut diagnostics = LexionDiagnosticList::default();
    let optimized = CodeOptimizerTac::new(target_word_overflow_cfg())
        .exec(
            &mut diagnostics,
            TacOptimizerOptions::for_target_word_bits(32),
        )
        .unwrap_or_else(|| panic!("{}", diagnostics_string(&diagnostics)));
    let block = optimized.node_indices().next().unwrap();
    let instruction = &optimized[block].instructions[0].instruction;

    assert_eq!(instruction.to_string(), "value = 2147483647 + 1");
    assert!(matches!(instruction, Instruction::Assignment(_)));
}
