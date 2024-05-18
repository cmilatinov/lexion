use std::fmt::{Display, Formatter};

use generational_arena::Index;

use lexion_lib::petgraph::Graph;
use lexion_lib::petgraph::graph::NodeIndex;
use lexion_lib::petgraph::prelude::Bfs;
use lexion_lib::petgraph::visit::Walker;
use lexion_lib::prettytable::{format, row, table, Table};
use lexion_lib::tokenizer::SourceRange;

use crate::ast::{
    AST, ASTNode, ASTVisitor, BlockStmt, FuncDeclStmt, Stmt, TraversalType, Type,
    TYPE_VOID, TypeCollection, VarDeclStmt,
};
use crate::diagnostic::DiagnosticConsumer;
use crate::pipeline::PipelineStage;

#[derive(Debug, Default)]
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

#[derive(Debug, Default)]
pub struct SymbolTableEntry {
    pub ty: SymbolTableEntryType,
    pub name: String,
    pub table: Option<NodeIndex>,
    pub range: Option<SourceRange>,
    pub var_type: Option<Index>,
}

impl SymbolTableEntry {
    pub fn from_ty_name(ty: SymbolTableEntryType, name: String) -> Self {
        Self {
            ty,
            name,
            ..Default::default()
        }
    }
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

    pub fn table(&self, node: NodeIndex, types: Option<&TypeCollection>) -> Option<Table> {
        let fmt_type = |index: &Option<Index>| match (&types, index) {
            (None, _) | (_, None) => String::from(""),
            (Some(types), Some(index)) => match types.get(*index) {
                None => String::from(""),
                Some(ty) => ty.to_string(),
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

#[derive(Default)]
pub struct SymbolTableGenerator {
    type_collection: TypeCollection,
    table: SymbolTableGraph,
    current_scope: NodeIndex,
}

impl SymbolTableGenerator {
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
                diag.warn(format!("Shadowed {} '{}'", prev.ty, prev.name))
            }
        }
        if let Err(index) = self.table.insert_entry(self.current_scope, entry) {
            if let Some(table) = self.table.graph.node_weight(self.current_scope) {
                diag.error(format!(
                    "Multiply declared identifier '{}'",
                    table.entries[index].name
                ));
            }
        }
    }

    fn lookup(&self, identifier: &str) -> Option<(NodeIndex, usize, &SymbolTableEntry)> {
        self.table.lookup(self.current_scope, identifier)
    }
}

impl PipelineStage for SymbolTableGenerator {
    type Input = AST;
    type Output = (SymbolTableGraph, TypeCollection);
    fn exec(
        mut self,
        diag: &mut dyn DiagnosticConsumer,
        ast: &Self::Input,
    ) -> Option<Self::Output> {
        self.create_scope(
            diag,
            SymbolTableEntry {
                ty: SymbolTableEntryType::Global,
                name: String::from("root"),
                ..Default::default()
            },
        );
        self.table.root = self.current_scope;
        ASTVisitor::default().visit(ast, |ty, node| match (ty, node) {
            (
                TraversalType::Preorder,
                ASTNode::Stmt(Stmt::FuncDeclStmt(FuncDeclStmt {
                    ty,
                    name,
                    params,
                    range,
                    ..
                })),
            ) => {
                let var_type = Some(self.type_collection.insert(Type::Function {
                    params: params.iter().map(|p| p.ty.clone()).collect(),
                    return_type: Box::new(ty.clone().unwrap_or_else(|| TYPE_VOID.clone())),
                }));
                self.create_scope(
                    diag,
                    SymbolTableEntry {
                        ty: SymbolTableEntryType::Function,
                        name: name.clone(),
                        range: Some(*range),
                        var_type,
                        ..Default::default()
                    },
                );
                for param in params {
                    let var_type = Some(self.type_collection.insert(param.ty.clone()));
                    self.insert(
                        diag,
                        SymbolTableEntry {
                            ty: SymbolTableEntryType::Parameter,
                            name: param.name.clone(),
                            range: Some(param.range),
                            table: None,
                            var_type,
                        },
                    );
                }
            }
            (
                TraversalType::Preorder,
                ASTNode::Stmt(Stmt::VarDeclStmt(VarDeclStmt { decls, .. })),
            ) => {
                for decl in decls.iter() {
                    let var_type = decl
                        .ty
                        .as_ref()
                        .map(|ty| self.type_collection.insert(ty.clone()));
                    self.insert(
                        diag,
                        SymbolTableEntry {
                            ty: SymbolTableEntryType::LocalVar,
                            name: decl.name.clone(),
                            table: None,
                            range: None,
                            var_type,
                        },
                    );
                }
            }
            (TraversalType::Preorder, ASTNode::Stmt(Stmt::BlockStmt(BlockStmt { range, .. }))) => {
                let table = self.table.graph.node_weight(self.current_scope).unwrap();
                self.create_scope(
                    diag,
                    SymbolTableEntry {
                        ty: SymbolTableEntryType::Scope,
                        name: format!("{}.{}", table.name, table.entries.len()),
                        range: Some(*range),
                        ..Default::default()
                    },
                )
            }
            (
                TraversalType::Postorder,
                ASTNode::Stmt(Stmt::FuncDeclStmt(_)) | ASTNode::Stmt(Stmt::BlockStmt(_)),
            ) => {
                self.parent_scope();
            }
            _ => {}
        });
        Some((self.table, self.type_collection))
    }
}
