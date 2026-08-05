use crate::ast::types::{FunctionType, TypeCollection, TypeKind};
use crate::generators::x86::calling_convention::{CallingConvention, Location, StackOffset};
use crate::generators::x86::{Bitness, SizeAlign};
use iced_x86::Register;

pub struct SystemV64;

const STACK_SLOT_BYTES: usize = 8;

impl CallingConvention for SystemV64 {
    fn assign_args(
        &self,
        types: &TypeCollection,
        stack_start: usize,
        signature: &FunctionType,
    ) -> Vec<Location> {
        use Register::*;

        const INTEGER_REGISTERS: [Register; 6] = [RDI, RSI, RDX, RCX, R8, R9];
        const FLOAT_REGISTERS: [Register; 8] = [XMM0, XMM1, XMM2, XMM3, XMM4, XMM5, XMM6, XMM7];

        let mut float_arg_idx = 0;
        let return_size = types.size_align(signature.return_type, Bitness::_64).size;
        let mut int_arg_idx = usize::from(
            matches!(types.kind(signature.return_type), TypeKind::Memory) && return_size > 16,
        );
        let mut stack_offset = stack_start;
        let mut result = Vec::new();

        for &arg_ty in &signature.params {
            let kind = types.kind(arg_ty);
            let SizeAlign { size, align } = types.size_align(arg_ty, Bitness::_64);
            if size == 0 {
                result.push(Location::NoStorage);
                continue;
            }
            match kind {
                TypeKind::Float | TypeKind::Vector | TypeKind::Double => {
                    if float_arg_idx < FLOAT_REGISTERS.len() {
                        result.push(Location::Register(FLOAT_REGISTERS[float_arg_idx]));
                        float_arg_idx += 1;
                    } else {
                        result.push(Location::Stack(StackOffset(stack_offset)));
                        stack_offset += 1;
                    }
                }
                TypeKind::Integer | TypeKind::Memory => {
                    if size <= 8 {
                        if int_arg_idx < INTEGER_REGISTERS.len() {
                            let reg = INTEGER_REGISTERS[int_arg_idx];
                            int_arg_idx += 1;
                            result.push(Location::Register(reg));
                        } else {
                            stack_offset = align_stack_offset(stack_offset, align.value());
                            let offset = StackOffset(stack_offset);
                            stack_offset += 1;
                            result.push(Location::Stack(offset))
                        }
                    } else if size <= 16 {
                        if int_arg_idx + 1 < INTEGER_REGISTERS.len() {
                            let low = INTEGER_REGISTERS[int_arg_idx];
                            let high = INTEGER_REGISTERS[int_arg_idx + 1];
                            int_arg_idx += 2;
                            result.push(Location::Pair {
                                low: Location::Register(low).into(),
                                high: Location::Register(high).into(),
                            });
                        } else {
                            stack_offset = align_stack_offset(stack_offset, align.value());
                            result.push(Location::Pair {
                                low: Location::Stack(StackOffset(stack_offset)).into(),
                                high: Location::Stack(StackOffset(stack_offset + 1)).into(),
                            });
                            stack_offset += 2;
                        }
                    } else {
                        stack_offset = align_stack_offset(stack_offset, align.value());
                        result.push(Location::Stack(StackOffset(stack_offset)));
                        stack_offset += stack_slots(size);
                    }
                }
                TypeKind::Unknown => {
                    stack_offset = align_stack_offset(stack_offset, align.value());
                    result.push(Location::Stack(StackOffset(stack_offset)));
                    stack_offset += stack_slots(size);
                }
            }
        }

        result
    }

    fn assign_ret(&self, types: &TypeCollection, signature: &FunctionType) -> Option<Location> {
        let kind = types.kind(signature.return_type);
        let SizeAlign { size, .. } = types.size_align(signature.return_type, Bitness::_64);
        if size == 0 {
            return None;
        }
        match kind {
            TypeKind::Float | TypeKind::Vector | TypeKind::Double => {
                Some(Location::Register(Register::XMM0))
            }
            TypeKind::Integer | TypeKind::Memory => Some(if size <= 8 {
                Location::Register(Register::RAX)
            } else if size <= 16 {
                Location::Pair {
                    low: Location::Register(Register::RAX).into(),
                    high: Location::Register(Register::RDX).into(),
                }
            } else {
                Location::Indirect {
                    address_register: Register::RDI,
                    result_register: Register::RAX,
                    size,
                }
            }),
            TypeKind::Unknown => None,
        }
    }

    fn callee_saved(&self) -> &'static [Register] {
        &[
            Register::RBX,
            Register::RBP,
            Register::R12,
            Register::R13,
            Register::R14,
            Register::R15,
        ]
    }

    fn caller_saved(&self) -> &'static [Register] {
        &[
            Register::RAX,
            Register::RDI,
            Register::RSI,
            Register::RDX,
            Register::RCX,
            Register::R8,
            Register::R9,
            Register::R10,
            Register::R11,
        ]
    }

    fn call_clobbered(&self) -> &'static [Register] {
        &[
            Register::RAX,
            Register::RDI,
            Register::RSI,
            Register::RDX,
            Register::RCX,
            Register::R8,
            Register::R9,
            Register::R10,
            Register::R11,
            Register::XMM0,
            Register::XMM1,
            Register::XMM2,
            Register::XMM3,
            Register::XMM4,
            Register::XMM5,
            Register::XMM6,
            Register::XMM7,
            Register::XMM8,
            Register::XMM9,
            Register::XMM10,
            Register::XMM11,
            Register::XMM12,
            Register::XMM13,
            Register::XMM14,
            Register::XMM15,
        ]
    }

    fn stack_alignment(&self) -> usize {
        16
    }

    fn fixed_stack_bytes(&self) -> usize {
        0
    }
}

fn stack_slots(size: usize) -> usize {
    size.div_ceil(STACK_SLOT_BYTES).max(1)
}

fn align_stack_offset(offset: usize, align: usize) -> usize {
    let slots = align.div_ceil(STACK_SLOT_BYTES).max(1);
    offset.div_ceil(slots) * slots
}
