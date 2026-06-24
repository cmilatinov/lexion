use iced_x86::Register;
use lexion_lang::ast::types::{FunctionType, TypeCollection};
use lexion_lang::generators::x86::{CallingConvention, Location, SystemV64, X86Target};

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
fn system_v64_assigns_f32_args_and_return_xmm_registers() {
    let types = TypeCollection::default();
    let signature = FunctionType {
        params: vec![types.f32(), types.i32(), types.f32()],
        return_type: types.f32(),
        is_vararg: false,
    };

    let locations = SystemV64.assign_args(&types, 0, &signature);
    let registers = locations.iter().map(Location::register).collect::<Vec<_>>();

    assert_eq!(
        registers,
        vec![
            Some(Register::XMM0),
            Some(Register::RDI),
            Some(Register::XMM1)
        ]
    );
    assert_eq!(
        SystemV64
            .assign_ret(&types, &signature)
            .and_then(|loc| loc.register()),
        Some(Register::XMM0)
    );
}

#[test]
fn system_v64_spills_f32_args_after_xmm_registers_are_exhausted() {
    let types = TypeCollection::default();
    let f32_ty = types.f32();
    let signature = FunctionType {
        params: vec![f32_ty; 9],
        return_type: types.i32(),
        is_vararg: false,
    };

    let locations = SystemV64.assign_args(&types, 0, &signature);

    assert_eq!(locations[0].register(), Some(Register::XMM0));
    assert_eq!(locations[7].register(), Some(Register::XMM7));
    match &locations[8] {
        Location::Stack(offset) => assert_eq!(offset.0, 0),
        other => panic!("expected stack location for ninth f32 arg, got {other:?}"),
    }
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
