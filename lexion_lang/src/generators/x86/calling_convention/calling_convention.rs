use crate::ast::types::{FunctionType, TypeCollection};
use derived_deref::{Deref, DerefMut};
use iced_x86::Register;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperandSize {
    _32,
    _64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegClass {
    Gpr(Register),
    Xmm(Register),
}

impl RegClass {
    pub fn subregister(&self, size: OperandSize) -> Register {
        match (self, size) {
            (RegClass::Gpr(r), OperandSize::_32) => r.full_register32(),
            (RegClass::Gpr(r), OperandSize::_64) => r.full_register(),
            (RegClass::Xmm(r), OperandSize::_32 | OperandSize::_64) => *r,
        }
    }

    pub fn family(&self) -> Register {
        match self {
            RegClass::Gpr(r) | RegClass::Xmm(r) => r.full_register(),
        }
    }
}

#[derive(Debug, Default, Deref, DerefMut, Clone, Copy)]
pub struct StackOffset(pub usize);

#[derive(Debug, Clone)]
pub enum Location {
    Register(Register),
    Stack(StackOffset),
    RegisterAndStack(Register, StackOffset),
    Indirect {
        address_register: Register,
        size: usize,
    },
    Pair {
        low: Box<Location>,
        high: Box<Location>,
    },
}

impl Location {
    pub fn register(&self) -> Option<Register> {
        match self {
            Location::Register(reg) => Some(*reg),
            _ => None,
        }
    }

    pub fn stack_offset(&self) -> Option<StackOffset> {
        match self {
            Location::Stack(offset) => Some(*offset),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ArgClass {
    Integer,
    Float,
    Memory,
}

pub trait CallingConvention {
    /// Assign argument locations in order. Result vector length equals params.len().
    fn assign_args(
        &self,
        types: &TypeCollection,
        stack_start: usize,
        signature: &FunctionType,
    ) -> Vec<Location>;

    /// Assign return location(s). None => void.
    fn assign_ret(&self, types: &TypeCollection, signature: &FunctionType) -> Option<Location>;

    /// Registers that the callee **must** preserve across calls.
    fn callee_saved(&self) -> &'static [Register];

    /// Registers that the caller must save if needed (volatile).
    fn caller_saved(&self) -> &'static [Register];

    /// Registers that the callee must preserve across calls.
    fn call_clobbered(&self) -> &'static [Register];

    /// Required stack alignment in bytes (e.g. 16).
    fn stack_alignment(&self) -> usize;

    /// Extra per-call fixed stack area (e.g. x64 Windows shadow space)
    fn fixed_stack_bytes(&self) -> usize;
}
