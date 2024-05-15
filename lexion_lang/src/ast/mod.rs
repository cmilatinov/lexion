use std::ops::{Deref, DerefMut};

pub use expr::*;
use lexion_lib::tokenizer::SourceLocation;
pub use stmt::*;
pub use types::*;
pub use visitor::*;

mod expr;
mod stmt;
mod types;
mod visitor;

pub struct Sourced<T> {
    pub loc: SourceLocation,
    pub inner: T,
}

impl<T> Sourced<T> {
    pub fn from(loc: SourceLocation, value: T) -> Self {
        Self { loc, inner: value }
    }
}

impl<T> Deref for Sourced<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> DerefMut for Sourced<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

pub type AST = Vec<Stmt>;
