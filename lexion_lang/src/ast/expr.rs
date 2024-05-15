#[derive(Debug)]
pub enum Expr {
    NoneExpr,
    UnaryExpr(UnaryExpr),
    BinaryExpr(BinaryExpr),
    TernaryExpr(TernaryExpr),
    CallExpr(CallExpr),
    IdentExpr(IdentExpr),
    LitExpr(LitExpr),
}

#[derive(Debug)]
pub struct UnaryExpr {
    pub operator: String,
    pub expr: Box<Expr>,
}

#[derive(Debug)]
pub struct BinaryExpr {
    pub operator: String,
    pub left: Box<Expr>,
    pub right: Box<Expr>,
}

#[derive(Debug)]
pub struct TernaryExpr {
    pub operator: String,
    pub first: Box<Expr>,
    pub second: Box<Expr>,
    pub third: Box<Expr>,
}

#[derive(Debug)]
pub struct CallExpr {
    pub expr: Box<Expr>,
    pub args: Vec<Expr>,
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
