use crate::ast::types::TypeCollection;
use crate::ast::visitor::{AstNode, AstVisitor, AstVisitorAction, TraversalType};
use crate::ast::{Ast, AstView};
use crate::diagnostic::{
    DiagnosticConsumer, LexionDiagnosticError, LexionDiagnosticInfo, LexionDiagnosticList,
};
use crate::generators::tac::instructions::{ControlFlowGraph, FunctionRange, LivenessInterval};
use crate::generators::tac::CodeGeneratorTac;
use crate::generators::x86::{
    AbiRegisterAllocator, AssignedLivenessInterval, CodeGeneratorX86, CodeGeneratorX86Elf,
    X86Assembly, X86ElfExecutable, X86ElfOptions, X86EmitOptions, X86Target,
};
use crate::parser::ParserLexion;
use crate::pipeline::PipelineStage;
use crate::symbol_table::{SymbolTableGenerator, SymbolTableGraph};
use crate::type_checker::TypeChecker;
use crate::{Dump, DumpFlags};
use lexion_lib::miette::{NamedSource, Report};
use lexion_lib::parsers::GrammarParserLR;
use lexion_lib::petgraph::dot::Dot;
use lexion_lib::tabled::Table;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct LexionCompilerOptions {
    pub dump_dir: PathBuf,
    pub dump_flags: DumpFlags,
    pub emit: EmitTarget,
    pub emit_source_comments: bool,
}

impl Default for LexionCompilerOptions {
    fn default() -> Self {
        Self {
            dump_dir: PathBuf::from("dump"),
            dump_flags: DumpFlags::default(),
            emit: EmitTarget::Check,
            emit_source_comments: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EmitTarget {
    #[default]
    Check,
    X86Assembly,
    X86Elf64,
}

pub struct LexionCompilerOutput {
    pub diagnostics: LexionDiagnosticList,
    pub assembly: Option<X86Assembly>,
    pub executable: Option<X86ElfExecutable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmitOutputError {
    X86AssemblyFailed,
    X86Elf64Failed,
}

struct EmitOutputInput<'a> {
    cfg: &'a ControlFlowGraph,
    types: &'a TypeCollection,
    symbols: &'a SymbolTableGraph,
    allocations: &'a HashMap<FunctionRange, Vec<AssignedLivenessInterval>>,
    source_text: &'a str,
    source: &'a NamedSource<Arc<String>>,
}

pub struct LexionCompiler {
    options: LexionCompilerOptions,
}

impl LexionCompiler {
    pub fn new(options: LexionCompilerOptions) -> Self {
        Self { options }
    }

    pub fn exec(
        &mut self,
        source: NamedSource<Arc<String>>,
    ) -> Result<LexionCompilerOutput, LexionDiagnosticList> {
        let mut diagnostics = LexionDiagnosticList::default();
        let source_text = source.inner().clone();
        let Some((mut ast, mut types, trace)) = self.parse_source(&mut diagnostics, source.clone())
        else {
            return Err(diagnostics);
        };

        let Some(mut symbols) =
            self.generate_symbols(&mut diagnostics, source.clone(), &ast, &mut types, &trace)
        else {
            return Err(diagnostics);
        };
        if diagnostics.has_errors() {
            return Err(diagnostics);
        }

        let Some(_) = self.type_check(
            &mut diagnostics,
            source.clone(),
            &mut ast,
            &mut symbols,
            &mut types,
        ) else {
            return Err(diagnostics);
        };
        if diagnostics.has_errors() {
            return Err(diagnostics);
        }

        let Some((cfg, intervals)) =
            self.generate_ir(&mut diagnostics, source.clone(), &ast, &mut symbols, &types)
        else {
            return Err(diagnostics);
        };

        let Some(assigned) =
            self.assign_registers(&mut diagnostics, &source, &cfg, &types, &symbols, intervals)
        else {
            return Err(diagnostics);
        };

        let (assembly, executable) = match self.emit_output(
            &mut diagnostics,
            EmitOutputInput {
                cfg: &cfg,
                types: &types,
                symbols: &symbols,
                allocations: &assigned,
                source_text: source_text.as_ref(),
                source: &source,
            },
        ) {
            Ok(output) => output,
            Err(EmitOutputError::X86AssemblyFailed | EmitOutputError::X86Elf64Failed) => {
                return Err(diagnostics);
            }
        };

        Ok(LexionCompilerOutput {
            diagnostics,
            assembly,
            executable,
        })
    }
}

impl LexionCompiler {
    fn dump_file(
        &self,
        name: &'static str,
        content: impl AsRef<[u8]>,
    ) -> Result<(), std::io::Error> {
        std::fs::create_dir_all(self.options.dump_dir.as_path())?;
        let path = self.options.dump_dir.join(name);
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        file.write_all(content.as_ref())?;
        Ok(())
    }

    fn dump_file_or_diagnostic(
        &self,
        diagnostics: &mut LexionDiagnosticList,
        source: &NamedSource<Arc<String>>,
        name: &'static str,
        content: impl AsRef<[u8]>,
    ) -> Option<()> {
        match self.dump_file(name, content) {
            Ok(()) => Some(()),
            Err(err) => {
                diagnostics.error(LexionDiagnosticError {
                    src: source.clone(),
                    span: lexion_lib::miette::SourceSpan::from(0),
                    message: format!("failed to write dump file `{name}`: {err}"),
                });
                None
            }
        }
    }

    fn parse_source(
        &self,
        diagnostics: &mut LexionDiagnosticList,
        source: NamedSource<Arc<String>>,
    ) -> Option<(Ast, TypeCollection, Table)> {
        let parser = ParserLexion::new();

        if self.options.dump_flags.contains(Dump::ParseTable) {
            let table: Table = ParserLexion::PARSER.get_parse_table().to_table();
            self.dump_file_or_diagnostic(
                diagnostics,
                &source,
                "parse_table.table",
                table.to_string(),
            )?;
        }

        if self.options.dump_flags.contains(Dump::Grammar) {
            self.dump_file_or_diagnostic(
                diagnostics,
                &source,
                "grammar.jsmachine",
                ParserLexion::GRAMMAR.to_jsmachine_string(),
            )?;
        }

        parser.exec(diagnostics, source.clone())
    }

    fn generate_symbols(
        &self,
        diagnostics: &mut LexionDiagnosticList,
        source: NamedSource<Arc<String>>,
        ast: &Ast,
        types: &mut TypeCollection,
        trace: &Table,
    ) -> Option<SymbolTableGraph> {
        if self.options.dump_flags.contains(Dump::ParseTrace) {
            self.dump_file_or_diagnostic(
                diagnostics,
                &source,
                "parse_trace.table",
                trace.to_string(),
            )?;
        }

        if self.options.dump_flags.contains(Dump::AbstractSyntaxTree) {
            self.dump_file_or_diagnostic(
                diagnostics,
                &source,
                "ast.tree",
                AstView::new(ast).to_string(),
            )?;
        }

        SymbolTableGenerator::new((source.clone(), ast, types)).exec(diagnostics, ())
    }

    fn type_check(
        &self,
        diagnostics: &mut LexionDiagnosticList,
        source: NamedSource<Arc<String>>,
        ast: &mut Ast,
        symbols: &mut SymbolTableGraph,
        types: &mut TypeCollection,
    ) -> Option<()> {
        if self.options.dump_flags.contains(Dump::Symbols) {
            if let Some(table) = symbols.table(symbols.root, Some(types)) {
                self.dump_file_or_diagnostic(
                    diagnostics,
                    &source,
                    "symbols.table",
                    table.to_string(),
                )?;
            }
            self.dump_file_or_diagnostic(
                diagnostics,
                &source,
                "symbols.dot",
                format!("{:?}", Dot::new(&symbols.graph)),
            )?;
        }

        TypeChecker::new((source.clone(), symbols, types)).exec(diagnostics, ast)
    }

    fn generate_ir(
        &self,
        diagnostics: &mut LexionDiagnosticList,
        source: NamedSource<Arc<String>>,
        ast: &Ast,
        symbols: &mut SymbolTableGraph,
        types: &TypeCollection,
    ) -> Option<(
        ControlFlowGraph,
        HashMap<FunctionRange, Vec<LivenessInterval>>,
    )> {
        if self.options.dump_flags.contains(Dump::Types) {
            let mut type_list = LexionDiagnosticList::default();
            AstVisitor::new().visit(ast, |ty, node, _| {
                if let (TraversalType::Postorder, AstNode::Expr(expr)) = (ty, node) {
                    type_list.info(LexionDiagnosticInfo {
                        src: source.clone(),
                        span: expr.span,
                        message: types.to_string_index(expr.ty).into_owned(),
                    });
                }
                AstVisitorAction::Continue
            });
            self.dump_file_or_diagnostic(
                diagnostics,
                &source,
                "types.list",
                Report::new(type_list).to_string(),
            )?;
        }

        CodeGeneratorTac::new((ast, symbols, types)).exec(diagnostics, ())
    }

    fn assign_registers(
        &self,
        diagnostics: &mut LexionDiagnosticList,
        source: &NamedSource<Arc<String>>,
        cfg: &ControlFlowGraph,
        types: &TypeCollection,
        symbols: &SymbolTableGraph,
        intervals: HashMap<FunctionRange, Vec<LivenessInterval>>,
    ) -> Option<HashMap<FunctionRange, Vec<AssignedLivenessInterval>>> {
        if self
            .options
            .dump_flags
            .contains(Dump::IntermediateRepresentation)
        {
            let mut ir = String::with_capacity(4096);
            for block in cfg.node_weights() {
                ir.push_str(&block.table().to_string());
                ir.push('\n');
            }
            self.dump_file_or_diagnostic(diagnostics, source, "ir.table", ir)?;
            self.dump_file_or_diagnostic(
                diagnostics,
                source,
                "ir.dot",
                format!("{:?}", Dot::new(&cfg.graph)),
            )?;
        }
        AbiRegisterAllocator::new((cfg, types, symbols, X86Target::system_v64()))
            .exec(diagnostics, intervals)
    }

    fn emit_output(
        &self,
        diagnostics: &mut LexionDiagnosticList,
        input: EmitOutputInput<'_>,
    ) -> Result<(Option<X86Assembly>, Option<X86ElfExecutable>), EmitOutputError> {
        match self.options.emit {
            EmitTarget::Check => Ok((None, None)),
            EmitTarget::X86Assembly => {
                CodeGeneratorX86::new((input.cfg, input.types, input.symbols))
                    .with_allocations(input.allocations)
                    .exec(
                        diagnostics,
                        X86EmitOptions {
                            emit_source_comments: self.options.emit_source_comments,
                            source: Some(input.source_text),
                            diagnostic_source: Some(input.source),
                        },
                    )
                    .map(|assembly| (Some(assembly), None))
                    .ok_or(EmitOutputError::X86AssemblyFailed)
            }
            EmitTarget::X86Elf64 => {
                CodeGeneratorX86Elf::new((input.cfg, input.types, input.symbols))
                    .exec(diagnostics, X86ElfOptions::default())
                    .map(|executable| (None, Some(executable)))
                    .ok_or(EmitOutputError::X86Elf64Failed)
            }
        }
    }
}
