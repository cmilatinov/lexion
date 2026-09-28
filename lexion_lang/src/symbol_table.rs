use crate::ast::types::{FunctionType, StructMember, StructType, Type, TypeCollection};
use crate::ast::visitor::{AstNode, AstVisitor, AstVisitorAction, TraversalType};
use crate::ast::{Ast, Expr, FuncDeclStmt, Sourced, Stmt, StructDeclStmt, TypedExpr, VarDeclStmt};
use crate::diagnostic::{DiagnosticConsumer, LexionDiagnosticError, LexionDiagnosticWarn};
use crate::generators::x86::MemoryLayout;
use crate::pipeline::PipelineStage;
use enumflags2::bitflags;
use generational_arena::Index;
use lexion_lib::miette::{NamedSource, SourceSpan};
use lexion_lib::petgraph::graph::NodeIndex;
use lexion_lib::petgraph::prelude::Bfs;
use lexion_lib::petgraph::visit::{Reversed, Walker};
use lexion_lib::petgraph::Graph;
use lexion_lib::tabled::builder::Builder;
use lexion_lib::tabled::settings::Style;
use lexion_lib::tabled::Table;
use std::fmt::{Debug, Display, Formatter};
use std::sync::Arc;

#[bitflags]
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum Properties {
    Parameter,
    Borrowed,
}

pub struct VariableMeta {}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
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
    pub layout: Option<MemoryLayout>,
}

#[derive(Default)]
pub struct SymbolTable {
    pub name: String,
    pub entries: Vec<SymbolTableEntry>,
}

impl Debug for SymbolTable {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.name)?;
        for entry in self.entries.iter() {
            writeln!(f, "{}: {}", entry.name, entry.ty)?;
        }
        Ok(())
    }
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
    pub fn parent_scope(&self, scope: NodeIndex) -> Option<NodeIndex> {
        self.graph.neighbors(scope).next()
    }

    pub fn parent_entry(&self, scope: NodeIndex) -> Option<(NodeIndex, usize, &SymbolTableEntry)> {
        let scope_name = self.graph.node_weight(scope)?.name.clone();
        let parent_scope = self.parent_scope(scope)?;
        self.lookup(parent_scope, scope_name.as_str())
    }

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

    /// Looks up a symbol within a function and its nested scopes without
    /// considering symbols from other functions.
    pub fn lookup_function(
        &self,
        function: &str,
        identifier: &str,
    ) -> Option<(NodeIndex, usize, &SymbolTableEntry)> {
        let mut scopes = vec![self.function_scope(function)?];
        while let Some(scope) = scopes.pop() {
            let table = self.graph.node_weight(scope)?;
            if let Ok(index) = table
                .entries
                .as_slice()
                .binary_search_by(|probe| probe.name.as_str().cmp(identifier))
            {
                return Some((scope, index, &table.entries[index]));
            }
            scopes.extend(table.entries.iter().filter_map(|entry| {
                (entry.ty == SymbolTableEntryType::Scope)
                    .then_some(entry.table)
                    .flatten()
            }));
        }
        None
    }

    /// Finds a function declaration in any lexical scope.
    pub fn lookup_function_entry(
        &self,
        function: &str,
    ) -> Option<(NodeIndex, usize, &SymbolTableEntry)> {
        let graph = Reversed(&self.graph);
        Bfs::new(&graph, self.root).iter(&graph).find_map(|scope| {
            self.graph.node_weight(scope).and_then(|table| {
                table
                    .entries
                    .iter()
                    .position(|entry| {
                        entry.ty == SymbolTableEntryType::Function && entry.name == function
                    })
                    .map(|index| (scope, index, &table.entries[index]))
            })
        })
    }

    fn function_scope(&self, function: &str) -> Option<NodeIndex> {
        self.lookup_function_entry(function)
            .and_then(|(_, _, entry)| entry.table)
    }

    pub fn table(&self, node: NodeIndex, types: Option<&TypeCollection>) -> Option<Table> {
        let fmt_type = |index: &Option<Index>| match (&types, index) {
            (None, _) | (_, None) => String::from(""),
            (Some(types), Some(index)) => match types.get(*index) {
                None => String::from(""),
                Some(ty) => types.to_string_type(ty).into_owned(),
            },
        };
        if let Some(t) = self.graph.node_weight(node) {
            let mut builder = Builder::new();
            for entry in &t.entries {
                if let Some(child_table) = entry.table.and_then(|n| self.table(n, types)) {
                    builder.push_record([
                        format!("{:?}", entry.ty),
                        entry.name.clone(),
                        fmt_type(&entry.var_type),
                        child_table.to_string(),
                    ]);
                } else {
                    builder.push_record([
                        format!("{:?}", entry.ty),
                        entry.name.clone(),
                        fmt_type(&entry.var_type),
                        String::from("N/A"),
                    ]);
                }
            }
            let mut table = builder.build();
            table.with(Style::modern());
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
            Some(table) => write!(f, "{table}"),
            None => write!(f, ""),
        }
    }
}

pub struct SymbolTableGenerator<'a> {
    src: NamedSource<Arc<String>>,
    ast: &'a Ast,
    types: &'a mut TypeCollection,
    table: SymbolTableGraph,
    current_scope: NodeIndex,
}

impl<'a> SymbolTableGenerator<'a> {
    fn parent_scope(&mut self) {
        if let Some(parent) = self.table.parent_scope(self.current_scope) {
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

    fn create_struct(&mut self, diag: &mut dyn DiagnosticConsumer, decl: &StructDeclStmt) {
        let struct_ = self
            .table
            .graph
            .add_node(SymbolTable::from_name(decl.name.value.clone()));
        self.table.graph.add_edge(struct_, self.current_scope, ());
        let mut members = Vec::new();
        for field in &decl.fields {
            let name = &field.value.name.value;
            let var_type = self.types.insert_ast_type(&field.ty.value);
            if let Some(ty) = var_type {
                members.push(StructMember {
                    name: name.clone(),
                    ty,
                });
            }
            self.table.graph[struct_].entries.push(SymbolTableEntry {
                ty: SymbolTableEntryType::StructMember,
                name: name.clone(),
                table: None,
                span: field.value.name.span,
                var_type,
                layout: None,
            });
        }
        let var_type = self.types.insert(&Type::StructType(StructType {
            ident: decl.name.value.clone(),
            members,
        }));
        let entry = SymbolTableEntry {
            ty: SymbolTableEntryType::Struct,
            name: decl.name.value.clone(),
            table: Some(struct_),
            span: decl.name.span,
            var_type: Some(var_type),
            layout: None,
        };
        self.insert(diag, entry);
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
    type Input = (NamedSource<Arc<String>>, &'a Ast, &'a mut TypeCollection);
    type Options = ();
    type Output = SymbolTableGraph;

    fn new((src, ast, types): Self::Input) -> Self {
        Self {
            ast,
            src,
            types,
            table: Default::default(),
            current_scope: Default::default(),
        }
    }

    fn exec(mut self, diag: &mut dyn DiagnosticConsumer, _: Self::Options) -> Option<Self::Output> {
        self.create_scope(
            diag,
            SymbolTableEntry {
                ty: SymbolTableEntryType::Global,
                name: String::from("root"),
                table: None,
                span: 0.into(),
                var_type: None,
                layout: None,
            },
        );
        self.table.root = self.current_scope;
        AstVisitor::new().visit(self.ast, |ty, node, _| {
            match (ty, node) {
                (
                    TraversalType::Preorder,
                    AstNode::Stmt(Sourced {
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
                ) => 'func_decl: {
                    let param_types = params
                        .iter()
                        .map(|p| self.types.insert_ast_type(&p.ty.value))
                        .collect::<Vec<_>>();
                    let param_types = if param_types.iter().all(|opt| opt.is_some()) {
                        param_types
                            .into_iter()
                            .map(|opt| opt.unwrap())
                            .collect::<Vec<_>>()
                    } else {
                        break 'func_decl;
                    };
                    let return_type = ty
                        .as_ref()
                        .and_then(|ty| self.types.insert_ast_type(&ty.value))
                        .unwrap_or_else(|| self.types.unit());
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
                            layout: None,
                        },
                    );
                    for Sourced { value: param, .. } in params {
                        let var_type = self.types.insert_ast_type(&param.ty.value);
                        self.insert(
                            diag,
                            SymbolTableEntry {
                                ty: SymbolTableEntryType::Parameter,
                                name: param.name.value.clone(),
                                table: None,
                                span: param.name.span,
                                var_type,
                                layout: None,
                            },
                        );
                    }
                }
                (
                    TraversalType::Preorder,
                    AstNode::Stmt(Sourced {
                        value: Stmt::VarDeclStmt(VarDeclStmt { decl, .. }),
                        ..
                    }),
                ) => {
                    let var_type = decl
                        .ty
                        .as_ref()
                        .and_then(|ty| self.types.insert_ast_type(&ty.value));
                    self.insert(
                        diag,
                        SymbolTableEntry {
                            ty: SymbolTableEntryType::LocalVar,
                            name: decl.name.value.clone(),
                            span: decl.name.span,
                            table: None,
                            var_type,
                            layout: None,
                        },
                    );
                }
                (
                    TraversalType::Preorder,
                    AstNode::Expr(Sourced {
                        value:
                            TypedExpr {
                                expr: Expr::BlockExpr(_),
                                ..
                            },
                        span,
                    }),
                ) => {
                    let table = self.table.graph.node_weight(self.current_scope).unwrap();
                    self.create_scope(
                        diag,
                        SymbolTableEntry {
                            ty: SymbolTableEntryType::Scope,
                            name: format!(
                                "{}.{}",
                                table.name,
                                table
                                    .entries
                                    .iter()
                                    .filter(|e| e.ty == SymbolTableEntryType::Scope)
                                    .count()
                            ),
                            table: None,
                            span: *span,
                            var_type: None,
                            layout: None,
                        },
                    )
                }
                (
                    TraversalType::Postorder,
                    AstNode::Stmt(Sourced {
                        value: Stmt::FuncDeclStmt(_),
                        ..
                    }),
                ) => {
                    self.parent_scope();
                }
                (
                    TraversalType::Postorder,
                    AstNode::Expr(Sourced {
                        value:
                            TypedExpr {
                                expr: Expr::BlockExpr(_),
                                ..
                            },
                        ..
                    }),
                ) => {
                    self.parent_scope();
                }
                (
                    TraversalType::Preorder,
                    AstNode::Stmt(Sourced {
                        value: Stmt::StructDeclStmt(stmt),
                        ..
                    }),
                ) => {
                    self.create_struct(diag, stmt);
                }
                _ => {}
            };
            AstVisitorAction::Continue
        });
        Some(self.table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(ty: SymbolTableEntryType, name: &str, table: Option<NodeIndex>) -> SymbolTableEntry {
        SymbolTableEntry {
            ty,
            name: String::from(name),
            table,
            span: 0.into(),
            var_type: None,
            layout: None,
        }
    }

    #[test]
    fn lookup_function_finds_nested_function_scopes() {
        let mut symbols = SymbolTableGraph::default();
        let root = symbols
            .graph
            .add_node(SymbolTable::from_name(String::from("root")));
        symbols.root = root;

        let outer = symbols
            .graph
            .add_node(SymbolTable::from_name(String::from("outer")));
        symbols.graph.add_edge(outer, root, ());
        symbols
            .insert_entry(
                root,
                entry(SymbolTableEntryType::Function, "outer", Some(outer)),
            )
            .unwrap();

        let block = symbols
            .graph
            .add_node(SymbolTable::from_name(String::from("outer.0")));
        symbols.graph.add_edge(block, outer, ());
        symbols
            .insert_entry(
                outer,
                entry(SymbolTableEntryType::Scope, "outer.0", Some(block)),
            )
            .unwrap();
        symbols
            .insert_entry(
                block,
                entry(SymbolTableEntryType::LocalVar, "block_value", None),
            )
            .unwrap();

        let inner = symbols
            .graph
            .add_node(SymbolTable::from_name(String::from("inner")));
        symbols.graph.add_edge(inner, block, ());
        symbols
            .insert_entry(
                block,
                entry(SymbolTableEntryType::Function, "inner", Some(inner)),
            )
            .unwrap();
        symbols
            .insert_entry(
                inner,
                entry(SymbolTableEntryType::LocalVar, "inner_value", None),
            )
            .unwrap();

        assert_eq!(
            symbols
                .lookup_function("inner", "inner_value")
                .map(|(_, _, entry)| entry.name.as_str()),
            Some("inner_value")
        );
        assert_eq!(
            symbols
                .lookup_function_entry("inner")
                .map(|(_, _, entry)| entry.name.as_str()),
            Some("inner")
        );
        assert_eq!(
            symbols
                .lookup_function("outer", "block_value")
                .map(|(_, _, entry)| entry.name.as_str()),
            Some("block_value")
        );
        assert!(symbols.lookup_function("outer", "inner_value").is_none());
    }
}
