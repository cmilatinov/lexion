use crate::ast::Type;

use super::expr::Expr;

#[derive(Debug)]
pub enum Stmt {
    FuncDeclStmt(FuncDeclStmt),
    BlockStmt(BlockStmt),
    VarDeclStmt(VarDeclStmt),
    ExprStmt(ExprStmt),
}

#[derive(Debug)]
pub struct FuncDeclStmt {
    pub name: String,
    pub params: Vec<Param>,
    pub ty: Option<Type>,
    pub body: Option<BlockStmt>,
}

#[derive(Debug)]
pub struct Param {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug)]
pub struct BlockStmt {
    pub stmts: Vec<Stmt>,
}

#[derive(Debug)]
pub struct ExprStmt {
    pub expr: Box<Expr>,
}

#[derive(Debug)]
pub struct VarDeclStmt {
    pub decls: Vec<VarDecl>,
}

#[derive(Debug)]
pub struct VarDecl {
    pub name: String,
    pub ty: Option<Type>,
    pub init: Option<Box<Expr>>,
}
