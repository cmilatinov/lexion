use lexion_lib::petgraph::dot::{Config, Dot};
use lexion_lib::petgraph::Graph;
use lexion_lib::petgraph::graph::NodeIndex;
use lexion_lib::petgraph::prelude::Bfs;
use lexion_lib::petgraph::visit::Walker;

use crate::ast::{AST, ASTNode, ASTVisitor, Expr, FuncDeclStmt, Stmt, VarDeclStmt};
use crate::typechecker::{SymbolTable, SymbolTableEntry};

#[derive(Default)]
pub struct SymbolTableGenerator {
    graph: Graph<SymbolTable, ()>,
    current_scope: NodeIndex,
}

impl SymbolTableGenerator {
    pub fn process(&mut self, ast: &AST) {
        self.create_scope(String::from("root"));
        ASTVisitor::default().visit(ast, |node| match node {
            ASTNode::Stmt(stmt) => match stmt {
                Stmt::FuncDeclStmt(FuncDeclStmt { name, .. }) => {
                    self.create_scope(name.clone());
                }
                Stmt::VarDeclStmt(VarDeclStmt { decls }) => {
                    for decl in decls.iter() {
                        self.insert(SymbolTableEntry {
                            name: decl.name.clone(),
                            table: None,
                            range: None,
                        });
                    }
                }
                _ => {}
            },
            ASTNode::Expr(expr) => match expr {
                Expr::NoneExpr => {}
                Expr::UnaryExpr(_) => {}
                Expr::BinaryExpr(_) => {}
                Expr::TernaryExpr(_) => {}
                Expr::CallExpr(_) => {}
                Expr::IdentExpr(_) => {}
                Expr::LitExpr(_) => {}
            },
        });
        println!(
            "{:?}",
            Dot::with_config(&self.graph, &[Config::EdgeNoLabel])
        );
    }

    fn create_scope(&mut self, name: String) {
        let scope = self.graph.add_node(SymbolTable::from_name(name.clone()));
        if scope != Default::default() {
            self.graph.add_edge(scope, self.current_scope, ());
            if let Some(table) = self.graph.node_weight_mut(self.current_scope) {
                let mut entry = SymbolTableEntry::from_name(name);
                entry.table = Some(scope);
                table.entries.push(entry);
            }
        }
        self.current_scope = scope;
    }

    fn insert(&mut self, entry: SymbolTableEntry) {
        if let Some(table) = self.graph.node_weight_mut(self.current_scope) {
            let index = table
                .entries
                .as_slice()
                .binary_search_by(|probe| probe.name.as_str().cmp(entry.name.as_str()))
                .unwrap_or_else(|value| value);
            table.entries.insert(index, entry);
        }
    }

    fn lookup(&self, identifier: &str) -> Option<&SymbolTableEntry> {
        std::iter::once(self.current_scope)
            .chain(Bfs::new(&self.graph, self.current_scope).iter(&self.graph))
            .find_map(|n| {
                self.graph.node_weight(n).and_then(|t| {
                    t.entries
                        .as_slice()
                        .binary_search_by(|probe| probe.name.as_str().cmp(identifier))
                        .ok()
                        .map(|i| &t.entries[i])
                })
            })
    }
}
