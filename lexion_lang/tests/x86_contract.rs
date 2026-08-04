use generational_arena::Index;
use iced_x86::Register;
use lexion_lang::ast::types::{
    FunctionType, StructMember, StructType, TupleType, Type, TypeCollection, TypeDefType,
};
use lexion_lang::generators::x86::{
    Bitness, CMemoryLayoutBuilder, CallingConvention, Location, SystemV64, X86Target,
};

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
fn system_v64_assigns_no_storage_for_unit_returns() {
    let types = TypeCollection::default();
    let signature = FunctionType {
        params: vec![],
        return_type: types.unit(),
        is_vararg: false,
    };

    assert!(SystemV64.assign_ret(&types, &signature).is_none());
}

#[test]
fn system_v64_unit_arguments_do_not_consume_registers() {
    let types = TypeCollection::default();
    let signature = FunctionType {
        params: vec![types.unit(), types.i32(), types.unit(), types.i32()],
        return_type: types.unit(),
        is_vararg: false,
    };

    let locations = SystemV64.assign_args(&types, 0, &signature);

    assert!(matches!(locations[0], Location::NoStorage));
    assert_eq!(locations[1].register(), Some(Register::RDI));
    assert!(matches!(locations[2], Location::NoStorage));
    assert_eq!(locations[3].register(), Some(Register::RSI));
}

#[test]
fn aggregate_layout_records_member_offsets_and_padding() {
    let mut types = TypeCollection::default();
    let bool_ty = types.bool();
    let i32_ty = types.i32();
    let char_ty = types.char();
    let tuple_ty = types.insert(&Type::TupleType(TupleType {
        types: vec![bool_ty, i32_ty, char_ty],
    }));
    let struct_ty = types.insert(&Type::StructType(StructType {
        ident: String::from("Packed"),
        members: vec![
            StructMember {
                name: String::from("flag"),
                ty: bool_ty,
            },
            StructMember {
                name: String::from("value"),
                ty: i32_ty,
            },
            StructMember {
                name: String::from("tag"),
                ty: char_ty,
            },
        ],
    }));

    types.compute_memory_layouts::<CMemoryLayoutBuilder>(Bitness::_64);

    assert_aggregate_layout(&types, tuple_ty);
    assert_aggregate_layout(&types, struct_ty);
}

#[test]
fn system_v64_classifies_small_aggregates_like_integer_register_values() {
    let mut types = TypeCollection::default();
    let i32_ty = types.i32();
    let pair_ty = types.insert(&Type::TupleType(TupleType {
        types: vec![i32_ty, i32_ty],
    }));
    types.compute_memory_layouts::<CMemoryLayoutBuilder>(Bitness::_64);
    let signature = FunctionType {
        params: vec![pair_ty],
        return_type: pair_ty,
        is_vararg: false,
    };

    assert_eq!(
        SystemV64.assign_args(&types, 0, &signature)[0].register(),
        Some(Register::RDI)
    );
    assert_eq!(
        SystemV64
            .assign_ret(&types, &signature)
            .and_then(|loc| loc.register()),
        Some(Register::RAX)
    );
}

#[test]
fn system_v64_classifies_two_word_aggregates_as_register_pairs() {
    let mut types = TypeCollection::default();
    let i32_ty = types.i32();
    let quad_ty = types.insert(&Type::TupleType(TupleType {
        types: vec![i32_ty, i32_ty, i32_ty, i32_ty],
    }));
    types.compute_memory_layouts::<CMemoryLayoutBuilder>(Bitness::_64);
    let signature = FunctionType {
        params: vec![quad_ty],
        return_type: quad_ty,
        is_vararg: false,
    };

    assert_register_pair(
        &SystemV64.assign_args(&types, 0, &signature)[0],
        Register::RDI,
        Register::RSI,
    );
    assert_register_pair(
        &SystemV64.assign_ret(&types, &signature).unwrap(),
        Register::RAX,
        Register::RDX,
    );
}

#[test]
fn system_v64_classifies_stack_and_indirect_aggregate_locations() {
    let mut types = TypeCollection::default();
    let i32_ty = types.i32();
    let pair_ty = types.insert(&Type::TupleType(TupleType {
        types: vec![i32_ty, i32_ty],
    }));
    let large_ty = types.insert(&Type::TupleType(TupleType {
        types: vec![i32_ty, i32_ty, i32_ty, i32_ty, i32_ty],
    }));
    types.compute_memory_layouts::<CMemoryLayoutBuilder>(Bitness::_64);
    let signature = FunctionType {
        params: vec![i32_ty, i32_ty, i32_ty, i32_ty, i32_ty, i32_ty, pair_ty],
        return_type: large_ty,
        is_vararg: false,
    };

    let locations = SystemV64.assign_args(&types, 0, &signature);

    assert_eq!(locations[0].register(), Some(Register::RSI));
    assert_stack(&locations[5], 0);
    assert_stack(&locations[6], 1);
    assert_indirect_return(&SystemV64.assign_ret(&types, &signature).unwrap(), 20);
}

#[test]
fn system_v64_classifies_large_aggregate_args_as_stack_values() {
    let mut types = TypeCollection::default();
    let i32_ty = types.i32();
    let large_ty = types.insert(&Type::TupleType(TupleType {
        types: vec![i32_ty, i32_ty, i32_ty, i32_ty, i32_ty],
    }));
    types.compute_memory_layouts::<CMemoryLayoutBuilder>(Bitness::_64);
    let signature = FunctionType {
        params: vec![
            i32_ty, i32_ty, i32_ty, i32_ty, i32_ty, i32_ty, large_ty, i32_ty,
        ],
        return_type: types.unit(),
        is_vararg: false,
    };

    let locations = SystemV64.assign_args(&types, 0, &signature);

    assert_eq!(locations.len(), signature.params.len());
    assert_stack(&locations[6], 0);
    assert_stack(&locations[7], 3);
}

#[test]
fn system_v64_classifies_thin_and_fat_references() {
    let mut types = TypeCollection::default();
    let thin_ref = types.reference(types.i32());
    let str_ref = types.str_ref();
    let signature = FunctionType {
        params: vec![thin_ref, str_ref],
        return_type: str_ref,
        is_vararg: false,
    };

    let locations = SystemV64.assign_args(&types, 0, &signature);

    assert_eq!(locations[0].register(), Some(Register::RDI));
    assert_register_pair(&locations[1], Register::RSI, Register::RDX);
    assert_register_pair(
        &SystemV64.assign_ret(&types, &signature).unwrap(),
        Register::RAX,
        Register::RDX,
    );
}

#[test]
fn system_v64_spills_fat_references_as_two_eightbytes() {
    let mut types = TypeCollection::default();
    let i32_ty = types.i32();
    let str_ref = types.str_ref();
    let signature = FunctionType {
        params: vec![
            i32_ty, i32_ty, i32_ty, i32_ty, i32_ty, i32_ty, str_ref, i32_ty,
        ],
        return_type: types.unit(),
        is_vararg: false,
    };

    let locations = SystemV64.assign_args(&types, 0, &signature);

    assert_stack_pair(&locations[6], 0, 1);
    assert_stack(&locations[7], 2);
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
        return_type: types.unit(),
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

#[test]
fn backend_value_layouts_define_scalar_and_reference_storage() {
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
    assert_frame_size_align(&types, types.bool(), 4, 4);
    assert_frame_size_align(&types, types.char(), 4, 4);
    assert_frame_size_align(&types, i32_ref, 8, 8);
}

#[test]
fn backend_value_layouts_distinguish_str_from_str_reference() {
    let mut types = TypeCollection::default();
    let str_ref = types.str_ref();

    assert_size_align(&types, types.str(), 0, 1);
    assert_size_align(&types, str_ref, 16, 8);
}

#[test]
fn backend_value_layouts_resolve_aggregate_alias_chains() {
    let mut types = TypeCollection::default();
    let i32_ty = types.i32();
    let pair = types.insert(&Type::TupleType(TupleType {
        types: vec![i32_ty, i32_ty],
    }));
    let pair_alias = types.insert(&Type::TypeDefType(TypeDefType {
        ident: String::from("Pair"),
        ty: pair,
    }));
    let pair_alias_alias = types.insert(&Type::TypeDefType(TypeDefType {
        ident: String::from("PairAlias"),
        ty: pair_alias,
    }));
    let pair_ref = types.reference(pair_alias_alias);

    let mapped_pair = types.insert(&Type::TupleType(TupleType {
        types: vec![types.bool(), i32_ty],
    }));
    let mapped_pair_alias = types.insert(&Type::TypeDefType(TypeDefType {
        ident: String::from("MappedPair"),
        ty: mapped_pair,
    }));
    types.type_map.insert(mapped_pair, pair);

    types.compute_memory_layouts::<CMemoryLayoutBuilder>(Bitness::_64);

    assert!(matches!(
        types.kind(pair_alias_alias),
        lexion_lang::ast::types::TypeKind::Memory
    ));
    assert_size_align(&types, pair_alias_alias, 8, 4);
    assert_size_align(&types, pair_ref, 8, 8);
    assert_eq!(types.canonicalize(mapped_pair_alias), pair);
}

#[test]
fn aggregate_layout_resolves_member_aliases_and_fat_references() {
    let mut types = TypeCollection::default();
    let str_alias = types.insert(&Type::TypeDefType(TypeDefType {
        ident: String::from("Text"),
        ty: types.str(),
    }));
    let str_ref = types.reference(str_alias);
    let i32_ty = types.i32();
    let pair = types.insert(&Type::TupleType(TupleType {
        types: vec![i32_ty, i32_ty],
    }));
    let mapped_source = types.insert(&Type::TupleType(TupleType {
        types: vec![types.bool()],
    }));
    let mapped_alias = types.insert(&Type::TypeDefType(TypeDefType {
        ident: String::from("MappedMember"),
        ty: mapped_source,
    }));
    types.type_map.insert(mapped_source, pair);
    let outer = types.insert(&Type::StructType(StructType {
        ident: String::from("Outer"),
        members: vec![
            StructMember {
                name: String::from("flag"),
                ty: types.bool(),
            },
            StructMember {
                name: String::from("text"),
                ty: str_ref,
            },
            StructMember {
                name: String::from("mapped"),
                ty: mapped_alias,
            },
            StructMember {
                name: String::from("tag"),
                ty: types.char(),
            },
        ],
    }));

    types.compute_memory_layouts::<CMemoryLayoutBuilder>(Bitness::_64);

    let layout = types.memory_layout(outer).expect("missing outer layout");
    let offsets = layout
        .members()
        .iter()
        .map(|member| member.offset)
        .collect::<Vec<_>>();
    let sizes = layout
        .members()
        .iter()
        .map(|member| member.size_align.size)
        .collect::<Vec<_>>();

    assert_eq!(offsets, vec![0, 8, 24, 32]);
    assert_eq!(sizes, vec![1, 16, 8, 1]);
    assert_size_align(&types, outer, 40, 8);
    assert_frame_size_align(&types, outer, 40, 8);
    assert_size_align(&types, mapped_alias, 8, 4);
}

#[test]
fn aggregate_layouts_recompute_after_type_changes() {
    let mut types = TypeCollection::default();
    let aggregate = types.insert(&Type::TupleType(TupleType {
        types: vec![types.bool()],
    }));

    types.compute_memory_layouts::<CMemoryLayoutBuilder>(Bitness::_64);
    assert_size_align(&types, aggregate, 1, 1);

    types[aggregate] = Type::TupleType(TupleType {
        types: vec![types.i32(), types.i32()],
    });
    types.compute_memory_layouts::<CMemoryLayoutBuilder>(Bitness::_64);

    assert_size_align(&types, aggregate, 8, 4);
}

fn assert_aggregate_layout(types: &TypeCollection, ty: Index) {
    let layout = types.memory_layout(ty).expect("missing layout");
    let offsets = layout
        .members()
        .iter()
        .map(|member| member.offset)
        .collect::<Vec<_>>();
    let sizes = layout
        .members()
        .iter()
        .map(|member| member.size_align.size)
        .collect::<Vec<_>>();

    assert_eq!(offsets, vec![0, 4, 8]);
    assert_eq!(sizes, vec![1, 4, 1]);
    let size_align = types.size_align(ty, Bitness::_64);
    assert_eq!(size_align.size, 12);
    assert_eq!(size_align.align.value(), 4);
}

fn assert_register_pair(location: &Location, low: Register, high: Register) {
    match location {
        Location::Pair { low: l, high: h } => {
            assert_eq!(l.register(), Some(low));
            assert_eq!(h.register(), Some(high));
        }
        other => panic!("expected register pair, got {other:?}"),
    }
}

fn assert_stack(location: &Location, offset: usize) {
    match location {
        Location::Stack(actual) => assert_eq!(actual.0, offset),
        other => panic!("expected stack offset {offset}, got {other:?}"),
    }
}

fn assert_stack_pair(location: &Location, low: usize, high: usize) {
    match location {
        Location::Pair { low: l, high: h } => {
            assert_stack(l, low);
            assert_stack(h, high);
        }
        other => panic!("expected stack pair, got {other:?}"),
    }
}

fn assert_indirect_return(location: &Location, size: usize) {
    match location {
        Location::Indirect {
            address_register,
            result_register,
            size: actual_size,
        } => {
            assert_eq!(*address_register, Register::RDI);
            assert_eq!(*result_register, Register::RAX);
            assert_eq!(*actual_size, size);
        }
        other => panic!("expected indirect return, got {other:?}"),
    }
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

fn assert_frame_size_align(
    types: &TypeCollection,
    ty: Index,
    expected_size: usize,
    expected_align: usize,
) {
    let size_align = types.frame_size_align(ty, Bitness::_64);
    assert_eq!(size_align.size, expected_size);
    assert_eq!(size_align.align.value(), expected_align);
}
