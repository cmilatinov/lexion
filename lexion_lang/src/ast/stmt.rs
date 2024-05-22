use crate::ast::{SourcedExpr, Type};
use crate::ast::sourced::Sourced;

pub type SourcedStmt = Sourced<Stmt>;

#[derive(Debug)]
pub enum Stmt {
    FuncDeclStmt(FuncDeclStmt),
    BlockStmt(BlockStmt),
    VarDeclStmt(VarDeclStmt),
    ExprStmt(ExprStmt),
}

#[derive(Debug)]
pub struct FuncDeclStmt {
    pub name: Sourced<String>,
    pub params: Vec<Sourced<Param>>,
    pub ty: Option<Sourced<Type>>,
    pub body: Option<Sourced<BlockStmt>>,
}

#[derive(Debug)]
pub struct Param {
    pub name: Sourced<String>,
    pub ty: Sourced<Type>,
}

#[derive(Debug)]
pub struct BlockStmt {
    pub stmts: Vec<SourcedStmt>,
}

#[derive(Debug)]
pub struct ExprStmt {
    pub expr: Box<SourcedExpr>,
}

#[derive(Debug)]
pub struct VarDeclStmt {
    pub decls: Vec<Sourced<VarDecl>>,
}

#[derive(Debug)]
pub struct VarDecl {
    pub name: Sourced<String>,
    pub ty: Option<Sourced<Type>>,
    pub init: Option<Box<SourcedExpr>>,
}
