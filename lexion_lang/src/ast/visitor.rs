use crate::ast::{
    AST, BlockStmt, CallExpr, Expr, ExprStmt, FuncDeclStmt, MemberExpr, OperatorExpr,
    Sourced, SourcedExpr, SourcedStmt, Stmt,
};

#[derive(Default)]
pub struct ASTVisitor;

#[derive(Clone, Copy)]
pub enum ASTNode<'a> {
    Stmt(&'a SourcedStmt),
    Expr(&'a SourcedExpr),
}

pub enum TraversalType {
    Preorder,
    Postorder,
}

impl ASTVisitor {
    pub fn visit<F: FnMut(TraversalType, ASTNode)>(&self, ast: &AST, mut visitor: F) {
        for stmt in ast.iter() {
            self.visit_stmt(stmt, &mut visitor);
        }
    }

    pub fn visit_stmt<F: FnMut(TraversalType, ASTNode)>(
        &self,
        stmt: &SourcedStmt,
        visitor: &mut F,
    ) {
        let node = ASTNode::Stmt(stmt);
        visitor(TraversalType::Preorder, node);
        match stmt {
            Sourced {
                value: Stmt::FuncDeclStmt(FuncDeclStmt { body, .. }),
                ..
            } => {
                for stmt in body.iter() {
                    for stmt in stmt.stmts.iter() {
                        self.visit_stmt(stmt, visitor);
                    }
                }
            }
            Sourced {
                value: Stmt::BlockStmt(BlockStmt { stmts, .. }),
                ..
            } => {
                for stmt in stmts.iter() {
                    self.visit_stmt(stmt, visitor);
                }
            }
            Sourced {
                value: Stmt::ExprStmt(ExprStmt { expr }),
                ..
            } => {
                self.visit_expr(expr, visitor);
            }
            _ => {}
        }
        visitor(TraversalType::Postorder, node);
    }

    pub fn visit_expr<F: FnMut(TraversalType, ASTNode)>(
        &self,
        expr: &SourcedExpr,
        visitor: &mut F,
    ) {
        let node = ASTNode::Expr(expr);
        visitor(TraversalType::Preorder, node);
        match expr {
            Sourced {
                value: Expr::OperatorExpr(OperatorExpr { args, .. }),
                ..
            } => {
                for arg in args {
                    self.visit_expr(arg, visitor);
                }
            }
            Sourced {
                value: Expr::MemberExpr(MemberExpr { expr, .. }),
                ..
            } => {
                self.visit_expr(&expr, visitor);
            }
            Sourced {
                value: Expr::CallExpr(CallExpr { expr, args, .. }),
                ..
            } => {
                self.visit_expr(expr, visitor);
                for expr in args {
                    self.visit_expr(expr, visitor);
                }
            }
            _ => {}
        }
        visitor(TraversalType::Postorder, node);
    }
}
