use crate::ast::sourced::Sourced;
use crate::ast::SourcedStmt;
use generational_arena::Index;
use std::fmt::{Display, Formatter};
use strum_macros::AsRefStr;

pub type SourcedExpr = Sourced<TypedExpr>;

#[derive(Debug)]
pub struct TypedExpr {
    pub ty: Index,
    pub expr: Expr,
}

impl From<Expr> for TypedExpr {
    fn from(expr: Expr) -> Self {
        Self {
            ty: Index::from_raw_parts(0, 0),
            expr,
        }
    }
}

#[derive(Debug, AsRefStr)]
pub enum Expr {
    BlockExpr(BlockExpr),
    IfExpr(IfExpr),
    OperatorExpr(OperatorExpr),
    MemberExpr(MemberExpr),
    IndexExpr(IndexExpr),
    CallExpr(CallExpr),
    IdentExpr(IdentExpr),
    LitExpr(LitExpr),
}

#[derive(Debug)]
pub struct BlockExpr {
    pub stmts: Vec<SourcedStmt>,
    pub expr: Option<Box<SourcedExpr>>,
}

#[derive(Debug)]
pub struct IfExpr {
    pub condition: Box<SourcedExpr>,
    pub then: Box<SourcedExpr>,
    pub else_: Option<Box<SourcedExpr>>,
}

#[derive(Debug)]
pub struct OperatorExpr {
    pub operator: &'static str,
    pub args: Vec<SourcedExpr>,
}

#[derive(Debug)]
pub struct MemberExpr {
    pub expr: Box<SourcedExpr>,
    pub ident: String,
}

#[derive(Debug)]
pub struct IndexExpr {
    pub expr: Box<SourcedExpr>,
    pub index: Box<SourcedExpr>,
}

#[derive(Debug)]
pub struct CallExpr {
    pub expr: Box<SourcedExpr>,
    pub args: Vec<SourcedExpr>,
}

#[derive(Debug)]
pub struct IdentExpr {
    pub ident: String,
}

#[derive(Debug)]
pub struct LitExpr {
    pub lit: Lit,
}

#[derive(Debug, Clone)]
pub enum Lit {
    Integer(isize),
    Float(f64),
    String(String),
    Boolean(bool),
}

impl Display for Lit {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Lit::Integer(value) => write!(f, "{value}"),
            Lit::Float(value) => write!(f, "{value}"),
            Lit::String(value) => write!(f, "{value}"),
            Lit::Boolean(value) => write!(f, "{value}"),
        }
    }
}

#[derive(Debug)]
pub enum Type {
    Path(PathType),
    Reference(ReferenceType),
    Tuple(TupleType),
}

#[derive(Debug)]
pub struct Path {
    pub segments: Vec<Sourced<String>>,
}

#[derive(Debug)]
pub struct PathType {
    pub path: Path,
}

#[derive(Debug)]
pub struct ReferenceType {
    pub to: Box<Sourced<Type>>,
}

#[derive(Debug)]
pub struct TupleType {
    pub types: Vec<Sourced<Type>>,
}
