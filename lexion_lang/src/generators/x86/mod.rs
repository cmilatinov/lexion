mod calling_convention;
mod elf;
mod machine_code;
mod memory_layout;
mod register_allocator;
mod target;
#[allow(clippy::module_inception)]
mod x86;

pub use calling_convention::*;
pub use elf::*;
pub use machine_code::*;
pub use memory_layout::*;
use num_enum::IntoPrimitive;
pub use register_allocator::*;
pub use target::*;
pub use x86::*;

#[derive(Debug, Clone, Copy, IntoPrimitive, PartialEq, Eq)]
#[repr(u32)]
pub enum Bitness {
    _32 = 4,
    _64 = 8,
}

impl Bitness {
    pub fn bytes(&self) -> usize {
        *self as usize
    }
}
