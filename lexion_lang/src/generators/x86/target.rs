use crate::generators::x86::SystemV64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X86MvpUnsupported {
    FloatingPoint,
    Aggregates,
    Pointers,
    FunctionCalls,
    Externs,
    Varargs,
}

pub struct X86Target<C = SystemV64> {
    calling_convention: C,
}

impl Default for X86Target<SystemV64> {
    fn default() -> Self {
        Self::system_v64()
    }
}

impl X86Target<SystemV64> {
    pub fn system_v64() -> Self {
        Self {
            calling_convention: SystemV64,
        }
    }
}

impl<C> X86Target<C> {
    pub fn new(calling_convention: C) -> Self {
        Self { calling_convention }
    }

    pub fn calling_convention(&self) -> &C {
        &self.calling_convention
    }
}
