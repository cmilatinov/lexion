use crate::ast::sourced::Sourced;
use crate::ast::{SourcedExpr, Type};
use paste::paste;
use strum_macros::AsRefStr;

pub type SourcedStmt = Sourced<Stmt>;

#[derive(Debug, AsRefStr)]
pub enum Stmt {
    FuncDeclStmt(FuncDeclStmt),
    VarDeclStmt(VarDeclStmt),
    ExprStmt(ExprStmt),
    ReturnStmt(ReturnStmt),
    StructDeclStmt(StructDeclStmt),
    WhileStmt(WhileStmt),
}

macro_rules! impl_stmt_getters {
    ($fn_ident:ident, $ident:ident) => {
        pub fn $fn_ident(&self) -> Option<&$ident> {
            if let Stmt::$ident(stmt) = self {
                Some(stmt)
            } else {
                None
            }
        }

        paste! {
            pub fn [<$fn_ident _mut>](&mut self) -> Option<&$ident> {
                if let Stmt::$ident(stmt) = self {
                    Some(stmt)
                } else {
                    None
                }
            }
        }
    };
}

impl Stmt {
    impl_stmt_getters!(func_decl, FuncDeclStmt);
    impl_stmt_getters!(var_decl, VarDeclStmt);
    impl_stmt_getters!(expr, ExprStmt);
    impl_stmt_getters!(_return, ReturnStmt);
    impl_stmt_getters!(struct_decl, StructDeclStmt);
}

#[derive(Debug)]
pub struct FuncDeclStmt {
    pub name: Sourced<String>,
    pub params: Vec<Sourced<Param>>,
    pub ty: Option<Sourced<Type>>,
    pub body: Option<SourcedExpr>,
    pub is_vararg: bool,
    pub is_extern: bool,
}

#[derive(Debug)]
pub struct Param {
    pub name: Sourced<String>,
    pub ty: Sourced<Type>,
}

#[derive(Debug)]
pub struct ReturnStmt {
    pub expr: Option<Box<SourcedExpr>>,
}

#[derive(Debug)]
pub struct ExprStmt {
    pub expr: Box<SourcedExpr>,
}

#[derive(Debug)]
pub struct VarDeclStmt {
    pub decl: Sourced<VarDecl>,
}

#[derive(Debug)]
pub struct VarDecl {
    pub name: Sourced<String>,
    pub ty: Option<Sourced<Type>>,
    pub init: Option<Box<SourcedExpr>>,
}

#[derive(Debug)]
pub struct StructDeclStmt {
    pub name: Sourced<String>,
    pub fields: Vec<Sourced<StructField>>,
}

#[derive(Debug)]
pub struct StructField {
    pub name: Sourced<String>,
    pub ty: Sourced<Type>,
}

#[derive(Debug)]
pub struct WhileStmt {
    pub condition: Box<SourcedExpr>,
    pub body: Box<SourcedExpr>,
}
