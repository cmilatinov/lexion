use crate::ast::{
    AST, BinaryExpr, BlockStmt, CallExpr, Expr, ExprStmt, FuncDeclStmt, Stmt, TernaryExpr,
    UnaryExpr,
};

#[derive(Default)]
pub struct ASTVisitor;

#[derive(Clone, Copy)]
pub enum ASTNode<'a> {
    Stmt(&'a Stmt),
    Expr(&'a Expr),
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

    pub fn visit_stmt<F: FnMut(TraversalType, ASTNode)>(&self, stmt: &Stmt, visitor: &mut F) {
        let node = ASTNode::Stmt(stmt);
        visitor(TraversalType::Preorder, node);
        match stmt {
            Stmt::FuncDeclStmt(FuncDeclStmt { body, .. }) => {
                for stmt in body.iter() {
                    for stmt in stmt.stmts.iter() {
                        self.visit_stmt(stmt, visitor);
                    }
                }
            }
            Stmt::BlockStmt(BlockStmt { stmts, .. }) => {
                for stmt in stmts.iter() {
                    self.visit_stmt(stmt, visitor);
                }
            }
            Stmt::ExprStmt(ExprStmt { expr }) => {
                self.visit_expr(expr, visitor);
            }
            _ => {}
        }
        visitor(TraversalType::Postorder, node);
    }

    pub fn visit_expr<F: FnMut(TraversalType, ASTNode)>(&self, expr: &Expr, visitor: &mut F) {
        let node = ASTNode::Expr(expr);
        visitor(TraversalType::Preorder, node);
        match expr {
            Expr::UnaryExpr(UnaryExpr { expr, .. }) => {
                self.visit_expr(expr, visitor);
            }
            Expr::BinaryExpr(BinaryExpr { left, right, .. }) => {
                self.visit_expr(left, visitor);
                self.visit_expr(right, visitor);
            }
            Expr::TernaryExpr(TernaryExpr {
                first,
                second,
                third,
                ..
            }) => {
                self.visit_expr(first, visitor);
                self.visit_expr(second, visitor);
                self.visit_expr(third, visitor);
            }
            Expr::CallExpr(CallExpr { expr, args }) => {
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
