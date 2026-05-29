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
fn system_v64_marks_integer_arg_registers_call_clobbered() {
    let call_clobbered = SystemV64.call_clobbered();

    for register in [
        Register::RAX,
        Register::RDI,
        Register::RSI,
        Register::RDX,
        Register::RCX,
        Register::R8,
        Register::R9,
        Register::R10,
        Register::R11,
    ] {
        assert!(
            call_clobbered.contains(&register),
            "{register:?} must be treated as call-clobbered"
        );
    }
}
