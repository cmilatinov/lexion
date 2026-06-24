use generational_arena::Index;
use iced_x86::Register;
use lexion_lang::ast::types::{FunctionType, Type, TypeCollection};
use lexion_lang::generators::x86::{Bitness, CallingConvention, Location, SystemV64, X86Target};

#[test]
fn x86_target_uses_isolated_calling_convention() {
    let target = X86Target::system_v64();

    assert_eq!(
        target.calling_convention().callee_saved(),
        SystemV64.callee_saved()
    );
    assert_eq!(target.calling_convention().stack_alignment(), 16);
    assert_eq!(target.calling_convention().fixed_stack_bytes(), 0);
}

#[test]
fn system_v64_assigns_integer_args_and_return_registers() {
    let types = TypeCollection::default();
    let signature = FunctionType {
        params: vec![types.i32(), types.bool(), types.i32()],
        return_type: types.i32(),
        is_vararg: false,
    };

    let locations = SystemV64.assign_args(&types, 0, &signature);
    let registers = locations.iter().map(Location::register).collect::<Vec<_>>();

    assert_eq!(
        registers,
        vec![
            Some(Register::RDI),
            Some(Register::RSI),
            Some(Register::RDX)
        ]
    );
    assert_eq!(
        SystemV64
            .assign_ret(&types, &signature)
            .and_then(|loc| loc.register()),
        Some(Register::RAX)
    );
}

#[test]
fn system_v64_marks_exact_call_clobbered_registers() {
    let expected = [
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
    ];

    assert_eq!(SystemV64.call_clobbered(), expected.as_slice());
}

#[test]
fn backend_value_layouts_define_scalar_and_pointer_storage() {
    let mut types = TypeCollection::default();
    let i32_ref = types.reference(types.i32());
    let function = types.insert(&Type::FunctionType(FunctionType {
        params: vec![types.i32()],
        return_type: types.i32(),
        is_vararg: false,
    }));

    assert_size_align(&types, types.i32(), 4, 4);
    assert_size_align(&types, types.u32(), 4, 4);
    assert_size_align(&types, types.f32(), 4, 4);
    assert_size_align(&types, types.bool(), 1, 1);
    assert_size_align(&types, types.char(), 1, 1);
    assert_size_align(&types, i32_ref, 8, 8);
    assert_size_align(&types, function, 8, 8);
}

#[test]
fn backend_value_layouts_distinguish_str_from_str_reference() {
    let mut types = TypeCollection::default();
    let str_ref = types.str_ref();

    assert_size_align(&types, types.str(), 0, 1);
    assert_size_align(&types, str_ref, 16, 8);
}

fn assert_size_align(
    types: &TypeCollection,
    ty: Index,
    expected_size: usize,
    expected_align: usize,
) {
    let size_align = types.size_align(ty, Bitness::_64);

    assert_eq!(size_align.size, expected_size);
    assert_eq!(size_align.align.value(), expected_align);
}
