use crate::ast::types::TypeCollection;
use crate::ast::visitor::{AstNode, AstVisitor, AstVisitorAction, TraversalType};
use crate::ast::{Ast, AstView};
use crate::diagnostic::{DiagnosticConsumer, LexionDiagnosticInfo, LexionDiagnosticList};
use crate::generators::tac::instructions::{ControlFlowGraph, FunctionRange, LivenessInterval};
use crate::generators::tac::CodeGeneratorTac;
use crate::generators::x86::{AssignedLivenessInterval, LinearRegisterAllocator};
use crate::parser::ParserLexion;
use crate::pipeline::PipelineStage;
use crate::symbol_table::{SymbolTableGenerator, SymbolTableGraph};
use crate::type_checker::TypeChecker;
use crate::{Dump, DumpFlags};
use iced_x86::Register;
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
    ) -> Result<LexionDiagnosticList, LexionDiagnosticList> {
        let mut diagnostics = LexionDiagnosticList::default();
        let Some((mut ast, mut types, trace)) = self.parse_source(&mut diagnostics, source.clone())
        else {
            return Err(diagnostics);
        };

        let Some(mut symbols) =
            self.generate_symbols(&mut diagnostics, source.clone(), &ast, &mut types, &trace)
        else {
            return Err(diagnostics);
        };

        let Some(_) = self.type_check(
            &mut diagnostics,
            source.clone(),
            &mut ast,
            &mut symbols,
            &mut types,
        ) else {
            return Err(diagnostics);
        };

        let Some((cfg, intervals)) =
            self.generate_ir(&mut diagnostics, source, &ast, &mut symbols, &types)
        else {
            return Err(diagnostics);
        };

        let Some(assigned) = self.assign_registers(&mut diagnostics, &cfg, intervals) else {
            return Err(diagnostics);
        };

        println!("{assigned:#?}");

        Ok(diagnostics)
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
        println!("{path:?}");
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        file.write_all(content.as_ref())?;
        Ok(())
    }

    fn parse_source(
        &self,
        diagnostics: &mut LexionDiagnosticList,
        source: NamedSource<Arc<String>>,
    ) -> Option<(Ast, TypeCollection, Table)> {
        let parser = ParserLexion::new();

        if self.options.dump_flags.contains(Dump::ParseTable) {
            let table: Table = ParserLexion::PARSER.get_parse_table().to_table();
            self.dump_file("parse_table.table", table.to_string())
                .unwrap();
        }

        if self.options.dump_flags.contains(Dump::Grammar) {
            self.dump_file(
                "grammar.jsmachine",
                ParserLexion::GRAMMAR.to_jsmachine_string(),
            )
            .unwrap();
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
            self.dump_file("parse_trace.table", trace.to_string())
                .unwrap();
        }

        if self.options.dump_flags.contains(Dump::AbstractSyntaxTree) {
            self.dump_file("ast.tree", AstView::new(ast).to_string())
                .unwrap();
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
                self.dump_file("symbols.table", table.to_string()).unwrap();
            }
            self.dump_file("symbols.dot", format!("{:?}", Dot::new(&symbols.graph)))
                .unwrap();
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
            self.dump_file("types.list", Report::new(type_list).to_string())
                .unwrap();
        }

        CodeGeneratorTac::new((ast, symbols, types)).exec(diagnostics, ())
    }

    fn assign_registers(
        &self,
        diagnostics: &mut LexionDiagnosticList,
        cfg: &ControlFlowGraph,
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
            self.dump_file("ir.table", ir).unwrap();
            self.dump_file("ir.dot", format!("{:?}", Dot::new(&cfg.graph)))
                .unwrap();
        }
        // SystemV64::callee_saved();
        LinearRegisterAllocator::new((
            cfg,
            Vec::from_iter([
                Register::RAX,
                Register::RCX,
                Register::RDX,
                Register::RSI,
                Register::RDI,
                Register::R8,
                Register::R9,
                Register::R10,
                Register::R11,
            ]),
        ))
        .exec(diagnostics, intervals)
    }
}
