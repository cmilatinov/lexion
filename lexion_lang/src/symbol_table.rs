use std::fmt::{Display, Formatter};
use std::sync::Arc;

use generational_arena::Index;

use lexion_lib::miette::{NamedSource, SourceSpan};
use lexion_lib::petgraph::graph::NodeIndex;
use lexion_lib::petgraph::prelude::Bfs;
use lexion_lib::petgraph::visit::Walker;
use lexion_lib::petgraph::Graph;
use lexion_lib::prettytable::{format, row, table, Table};

use crate::ast::{
    ASTNode, ASTVisitor, FuncDeclStmt, FunctionType, Sourced, Stmt, TraversalType, Type,
    TypeCollection, VarDeclStmt, AST, TYPE_UNIT,
};
use crate::diagnostic::{DiagnosticConsumer, LexionDiagnosticError, LexionDiagnosticWarn};
use crate::pipeline::PipelineStage;

#[derive(Debug, Default, Clone, Copy)]
pub enum SymbolTableEntryType {
    Global,
    Scope,
    Function,
    Parameter,
    #[default]
    LocalVar,
    Struct,
    StructMember,
    Temporary,
}

impl Display for SymbolTableEntryType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                SymbolTableEntryType::Global => "global",
                SymbolTableEntryType::Function => "function",
                SymbolTableEntryType::Scope => "scope",
                SymbolTableEntryType::Parameter => "parameter",
                SymbolTableEntryType::LocalVar => "local variable",
                SymbolTableEntryType::Struct => "struct",
                SymbolTableEntryType::StructMember => "struct member",
                SymbolTableEntryType::Temporary => "temporary",
            }
        )
    }
}

#[derive(Debug, Clone)]
pub struct SymbolTableEntry {
    pub ty: SymbolTableEntryType,
    pub name: String,
    pub table: Option<NodeIndex>,
    pub span: SourceSpan,
    pub var_type: Option<Index>,
}

#[derive(Debug, Default)]
pub struct SymbolTable {
    pub name: String,
    pub entries: Vec<SymbolTableEntry>,
}

impl SymbolTable {
    pub fn from_name(name: String) -> Self {
        Self {
            name,
            ..Default::default()
        }
    }
}

#[derive(Default)]
pub struct SymbolTableGraph {
    pub root: NodeIndex,
    pub graph: Graph<SymbolTable, ()>,
}

impl SymbolTableGraph {
    pub fn insert_entry(&mut self, node: NodeIndex, entry: SymbolTableEntry) -> Result<(), usize> {
        if let Some(table) = self.graph.node_weight_mut(node) {
            match table
                .entries
                .as_slice()
                .binary_search_by(|probe| probe.name.as_str().cmp(entry.name.as_str()))
            {
                Ok(index) => {
                    return Err(index);
                }
                Err(index) => {
                    table.entries.insert(index, entry);
                }
            }
        }
        Ok(())
    }

    pub fn lookup(
        &self,
        node: NodeIndex,
        identifier: &str,
    ) -> Option<(NodeIndex, usize, &SymbolTableEntry)> {
        std::iter::once(node)
            .chain(Bfs::new(&self.graph, node).iter(&self.graph))
            .find_map(|n| {
                self.graph.node_weight(n).and_then(|t| {
                    t.entries
                        .as_slice()
                        .binary_search_by(|probe| probe.name.as_str().cmp(identifier))
                        .ok()
                        .map(|i| (n, i, &t.entries[i]))
                })
            })
    }

    pub fn lookup_mut(
        &mut self,
        node: NodeIndex,
        identifier: &str,
    ) -> Option<&mut SymbolTableEntry> {
        if let Some((node, index, _)) = self.lookup(node, identifier) {
            if let Some(table) = self.graph.node_weight_mut(node) {
                return Some(&mut table.entries[index]);
            }
        }
        None
    }

    pub fn table(&self, node: NodeIndex, types: Option<&TypeCollection>) -> Option<Table> {
        let fmt_type = |index: &Option<Index>| match (&types, index) {
            (None, _) | (_, None) => String::from(""),
            (Some(types), Some(index)) => match types.get(*index) {
                None => String::from(""),
                Some(ty) => types.to_string(ty),
            },
        };
        if let Some(t) = self.graph.node_weight(node) {
            let mut table = table!(["Type", "Name", "Var Type", "Table"]);
            table.set_format(*format::consts::FORMAT_BOX_CHARS);
            for entry in &t.entries {
                if let Some(child_table) = entry.table.and_then(|n| self.table(n, types)) {
                    table.add_row(row![
                        format!("{:?}", entry.ty),
                        entry.name.clone(),
                        fmt_type(&entry.var_type),
                        child_table
                    ]);
                } else {
                    table.add_row(row![
                        format!("{:?}", entry.ty),
                        entry.name.clone(),
                        fmt_type(&entry.var_type),
                        "N/A"
                    ]);
                }
            }
            Some(table)
        } else {
            None
        }
    }
}

impl Display for SymbolTableGraph {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let table = self.table(self.root, None);
        match table {
            Some(table) => write!(f, "{}", table),
            None => write!(f, ""),
        }
    }
}

pub struct SymbolTableGenerator<'a> {
    ast: &'a AST,
    src: NamedSource<Arc<String>>,
    types: TypeCollection,
    table: SymbolTableGraph,
    current_scope: NodeIndex,
}

impl<'a> SymbolTableGenerator<'a> {
    fn parent_scope(&mut self) {
        if let Some(parent) = self.table.graph.neighbors(self.current_scope).next() {
            self.current_scope = parent;
        }
    }

    fn create_scope(&mut self, diag: &mut dyn DiagnosticConsumer, mut entry: SymbolTableEntry) {
        let scope = self
            .table
            .graph
            .add_node(SymbolTable::from_name(entry.name.clone()));
        if scope != Default::default() {
            self.table.graph.add_edge(scope, self.current_scope, ());
            entry.table = Some(scope);
            self.insert(diag, entry);
        }
        self.current_scope = scope;
    }

    fn insert(&mut self, diag: &mut dyn DiagnosticConsumer, entry: SymbolTableEntry) {
        if let Some((scope, _, prev)) = self.lookup(entry.name.as_str()) {
            if scope != self.current_scope {
                diag.warn(LexionDiagnosticWarn {
                    src: self.src.clone(),
                    span: entry.span,
                    message: format!("shadowed {} '{}'", prev.ty, prev.name),
                })
            }
        }
        if let Err(index) = self.table.insert_entry(self.current_scope, entry.clone()) {
            if let Some(table) = self.table.graph.node_weight(self.current_scope) {
                diag.error(LexionDiagnosticError {
                    src: self.src.clone(),
                    span: entry.span,
                    message: format!("duplicate identifier '{}'", table.entries[index].name),
                });
            }
        }
    }

    fn lookup(&self, identifier: &str) -> Option<(NodeIndex, usize, &SymbolTableEntry)> {
        self.table.lookup(self.current_scope, identifier)
    }
}

impl<'a> PipelineStage for SymbolTableGenerator<'a> {
    type Input = (NamedSource<Arc<String>>, &'a AST);
    type Output = (SymbolTableGraph, TypeCollection);

    fn new((src, ast): Self::Input) -> Self {
        Self {
            ast,
            src,
            types: Default::default(),
            table: Default::default(),
            current_scope: Default::default(),
        }
    }
    fn exec(mut self, diag: &mut dyn DiagnosticConsumer) -> Option<Self::Output> {
        self.create_scope(
            diag,
            SymbolTableEntry {
                ty: SymbolTableEntryType::Global,
                name: String::from("root"),
                table: None,
                span: 0.into(),
                var_type: None,
            },
        );
        self.table.root = self.current_scope;
        ASTVisitor::default().visit(self.ast, |ty, node| match (ty, node) {
            (
                TraversalType::Preorder,
                ASTNode::Stmt(Sourced {
                    value:
                        Stmt::FuncDeclStmt(FuncDeclStmt {
                            ty,
                            name,
                            params,
                            is_vararg,
                            ..
                        }),
                    ..
                }),
            ) => {
                let param_types = params
                    .iter()
                    .map(|p| self.types.insert(&p.ty.value))
                    .collect::<Vec<_>>();
                let return_type = ty
                    .clone()
                    .map(|ty| self.types.insert(&ty.value))
                    .unwrap_or_else(|| self.types.insert(&TYPE_UNIT));
                let var_type = Some(self.types.insert(&Type::FunctionType(FunctionType {
                    params: param_types,
                    return_type,
                    is_vararg: *is_vararg,
                })));
                self.create_scope(
                    diag,
                    SymbolTableEntry {
                        ty: SymbolTableEntryType::Function,
                        name: name.value.clone(),
                        table: None,
                        span: name.span,
                        var_type,
                    },
                );
                for Sourced { value: param, .. } in params {
                    let var_type = Some(self.types.insert(&param.ty.value));
                    self.insert(
                        diag,
                        SymbolTableEntry {
                            ty: SymbolTableEntryType::Parameter,
                            name: param.name.value.clone(),
                            table: None,
                            span: param.name.span,
                            var_type,
                        },
                    );
                }
            }
            (
                TraversalType::Preorder,
                ASTNode::Stmt(Sourced {
                    value: Stmt::VarDeclStmt(VarDeclStmt { decls, .. }),
                    ..
                }),
            ) => {
                for decl in decls.iter() {
                    let var_type = decl.ty.as_ref().map(|ty| self.types.insert(&ty.value));
                    self.insert(
                        diag,
                        SymbolTableEntry {
                            ty: SymbolTableEntryType::LocalVar,
                            name: decl.name.value.clone(),
                            span: decl.span,
                            table: None,
                            var_type,
                        },
                    );
                }
            }
            (
                TraversalType::Preorder,
                ASTNode::Stmt(Sourced {
                    value: Stmt::BlockStmt(_),
                    span,
                }),
            ) => {
                let table = self.table.graph.node_weight(self.current_scope).unwrap();
                self.create_scope(
                    diag,
                    SymbolTableEntry {
                        ty: SymbolTableEntryType::Scope,
                        name: format!("{}.{}", table.name, table.entries.len()),
                        table: None,
                        span: *span,
                        var_type: None,
                    },
                )
            }
            (
                TraversalType::Postorder,
                ASTNode::Stmt(Sourced {
                    value: Stmt::FuncDeclStmt(_) | Stmt::BlockStmt(_),
                    ..
                }),
            ) => {
                self.parent_scope();
            }
            _ => {}
        });
        Some((self.table, self.types))
    }
}
