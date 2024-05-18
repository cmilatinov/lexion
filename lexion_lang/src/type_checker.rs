use lexion_lib::petgraph::graph::NodeIndex;

use crate::ast::{
    AST, ASTNode, ASTVisitor, Expr, Lit, LitExpr, Stmt, TraversalType, Type, TYPE_BOOL, TYPE_F32,
    TYPE_STR, TYPE_U32, TYPE_VOID,
};
use crate::diagnostic::DiagnosticConsumer;
use crate::pipeline::PipelineStage;
use crate::symbol_table::SymbolTableGraph;

#[derive(Default)]
pub struct TypeChecker {
    current_scope: NodeIndex,
}

impl TypeChecker {
    fn tc(&self, diag: &mut dyn DiagnosticConsumer, expr: &Expr) -> Option<Type> {
        match expr {
            Expr::LitExpr(lit_expr) => self.lit(lit_expr),
            _ => Some(TYPE_VOID.clone()),
        }
    }

    fn lit(&self, lit_expr: &LitExpr) -> Option<Type> {
        match &lit_expr.lit {
            Lit::Integer(_) => Some(TYPE_U32.clone()),
            Lit::Float(_) => Some(TYPE_F32.clone()),
            Lit::String(_) => Some(TYPE_STR.clone()),
            Lit::Boolean(_) => Some(TYPE_BOOL.clone()),
        }
    }

    fn expr(&self) {}
}

impl PipelineStage for TypeChecker {
    type Input = (AST, SymbolTableGraph);
    type Output = ();

    fn exec(
        mut self,
        diag: &mut dyn DiagnosticConsumer,
        (ast, table): &Self::Input,
    ) -> Option<Self::Output> {
        ASTVisitor::default().visit(ast, |ty, node| match (ty, node) {
            (TraversalType::Preorder, ASTNode::Stmt(Stmt::FuncDeclStmt(decl))) => {
                if let Some((node, _, _)) = table.lookup(self.current_scope, decl.name.as_str()) {
                    self.current_scope = node;
                }
            }
            (
                TraversalType::Postorder,
                ASTNode::Stmt(Stmt::FuncDeclStmt(_)) | ASTNode::Stmt(Stmt::BlockStmt(_)),
            ) => {
                if let Some(parent) = table.graph.neighbors(self.current_scope).next() {
                    self.current_scope = parent;
                }
            }
            _ => {}
        });
        Some(())
    }
}
