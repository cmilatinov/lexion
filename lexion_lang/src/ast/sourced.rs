use std::fmt::{Debug, Formatter};
use std::ops::{Deref, DerefMut};

use lexion_lib::miette::SourceSpan;

pub struct Sourced<T> {
    pub span: SourceSpan,
    pub value: T,
}

impl<T: Debug> Debug for Sourced<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sourced")
            .field("span", &self.span)
            .field("value", &self.value)
            .finish()
    }
}

impl<T: Clone> Clone for Sourced<T> {
    fn clone(&self) -> Self {
        Self {
            span: self.span,
            value: self.value.clone(),
        }
    }
}

impl<T: Copy> Copy for Sourced<T> {}

impl<T> Deref for Sourced<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T> DerefMut for Sourced<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<T> From<(SourceSpan, T)> for Sourced<T> {
    fn from((span, value): (SourceSpan, T)) -> Self {
        Self { span, value }
    }
}
