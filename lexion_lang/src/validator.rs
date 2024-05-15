use crate::ast::Stmt;

pub struct Validator {}

impl Validator {
    pub fn new() -> Self {
        Self {}
    }

    pub fn validate(&self, ast: &Vec<Stmt>) {}
}
