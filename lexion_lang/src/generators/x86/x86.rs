use crate::ast::types::TypeCollection;
use crate::diagnostic::DiagnosticConsumer;
use crate::generators::tac::instructions::ControlFlowGraph;
use crate::pipeline::PipelineStage;
use crate::symbol_table::SymbolTable;
use iced_x86::code_asm::*;

pub struct CodeGeneratorX86<'a> {
    cfg: &'a ControlFlowGraph,
    types: &'a TypeCollection,
    symbols: &'a SymbolTable,
    assembler: CodeAssembler,
}

impl<'a> PipelineStage for CodeGeneratorX86<'a> {
    type Input = (&'a ControlFlowGraph, &'a TypeCollection, &'a SymbolTable);
    type Options = ();
    type Output = ();

    fn new((cfg, types, symbols): Self::Input) -> Self {
        Self {
            cfg,
            types,
            symbols,
            assembler: CodeAssembler::new(64).unwrap(),
        }
    }

    fn exec(self, diag: &mut dyn DiagnosticConsumer, opts: Self::Options) -> Option<Self::Output> {
        None
    }
}
