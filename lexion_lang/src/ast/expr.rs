use crate::ast::sourced::Sourced;

pub type SourcedExpr = Sourced<Expr>;

#[derive(Debug)]
pub enum Expr {
    NoneExpr,
    OperatorExpr(OperatorExpr),
    MemberExpr(MemberExpr),
    IndexExpr(IndexExpr),
    CallExpr(CallExpr),
    IdentExpr(IdentExpr),
    LitExpr(LitExpr),
}

#[derive(Debug)]
pub struct OperatorExpr {
    pub operator: String,
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

#[derive(Debug)]
pub enum Lit {
    Integer(isize),
    Float(f64),
    String(String),
    Boolean(bool),
}
