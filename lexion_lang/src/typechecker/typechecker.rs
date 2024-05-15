use crate::ast::{Expr, Lit, LitExpr, Type};
use crate::typechecker::TypeError;

pub struct Typechecker;

type Result<T> = std::result::Result<T, TypeError>;

impl Typechecker {
    pub fn tc(&self, expr: &Expr) -> Result<Type> {
        match expr {
            Expr::LitExpr(lit_expr) => self.lit(lit_expr),
            _ => Ok(Type::Struct {
                ident: "void".into(),
                ref_count: 0,
            }),
        }
    }

    fn lit(&self, lit_expr: &LitExpr) -> Result<Type> {
        match &lit_expr.lit {
            Lit::Integer(_) => Ok(Type::Struct {
                ident: "u32".into(),
                ref_count: 0,
            }),
            Lit::Float(_) => Ok(Type::Struct {
                ident: "f32".into(),
                ref_count: 0,
            }),
            Lit::String(_) => Ok(Type::Struct {
                ident: "str".into(),
                ref_count: 1,
            }),
            Lit::Boolean(_) => Ok(Type::Struct {
                ident: "bool".into(),
                ref_count: 0,
            }),
        }
    }
}
