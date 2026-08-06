use crate::ast::types::{FunctionType, PrimitiveType, Type, TypeCollection};
use crate::ast::Lit;
use crate::diagnostic::{DiagnosticConsumer, LexionDiagnosticError};
use crate::generators::tac::instructions::{
    AssignmentInstruction, ConditionalJumpInstruction, ControlFlowGraph, FunctionCallInstruction,
    FunctionRange, Instruction, Operand, Place,
};
use crate::generators::x86::calling_convention::{CallingConvention, Location};
use crate::generators::x86::{Bitness, SizeAlign, X86Target};
use crate::operators;
use crate::pipeline::PipelineStage;
use crate::symbol_table::{SymbolTableEntry, SymbolTableGraph};
use generational_arena::Index;
use iced_x86::code_asm::*;
use iced_x86::{BlockEncoderOptions, IcedError, Register};
use lexion_lib::miette::{NamedSource, SourceSpan};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

const STACK_ARG_SLOT_BYTES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShiftKind {
    Left,
    SignedRight,
    UnsignedRight,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X86MachineCode {
    bytes: Vec<u8>,
    symbols: BTreeMap<String, usize>,
}

#[derive(Clone, Copy)]
struct MachineFunctionContext<'a> {
    name: &'a str,
    return_register: Register,
    indirect_return_slot: Option<usize>,
}

impl X86MachineCode {
    pub fn new(bytes: Vec<u8>, symbols: BTreeMap<String, usize>) -> Self {
        Self { bytes, symbols }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    pub fn symbols(&self) -> &BTreeMap<String, usize> {
        &self.symbols
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct X86MachineCodeOptions {
    pub base_address: u64,
}

pub struct CodeGeneratorX86Machine<'a> {
    cfg: &'a ControlFlowGraph,
    types: &'a TypeCollection,
    symbols: &'a SymbolTableGraph,
    target: X86Target,
}

impl<'a> CodeGeneratorX86Machine<'a> {
    fn emit(&self, options: X86MachineCodeOptions) -> Result<X86MachineCode, IcedError> {
        let mut assembler = CodeAssembler::new(64)?;
        let mut labels = self.create_labels(&mut assembler);
        for range in &self.cfg.functions {
            self.emit_function(&mut assembler, &mut labels, *range)?;
        }
        let result = assembler.assemble_options(
            options.base_address,
            BlockEncoderOptions::RETURN_NEW_INSTRUCTION_OFFSETS,
        )?;
        let symbols = self.symbol_offsets(&labels, &result, options.base_address)?;
        Ok(X86MachineCode::new(result.inner.code_buffer, symbols))
    }

    fn create_labels(&self, assembler: &mut CodeAssembler) -> HashMap<String, CodeLabel> {
        self.cfg
            .node_weights()
            .map(|block| (block.label.clone(), assembler.create_label()))
            .collect()
    }

    fn symbol_offsets(
        &self,
        labels: &HashMap<String, CodeLabel>,
        result: &CodeAssemblerResult,
        base_address: u64,
    ) -> Result<BTreeMap<String, usize>, IcedError> {
        self.cfg
            .functions
            .iter()
            .map(|range| {
                let name = self.cfg[range.start].label.clone();
                let label = labels.get(name.as_str()).expect("missing function label");
                let ip = result.label_ip(label)?;
                Ok((name, ip.wrapping_sub(base_address) as usize))
            })
            .collect()
    }

    fn emit_function(
        &self,
        assembler: &mut CodeAssembler,
        labels: &mut HashMap<String, CodeLabel>,
        range: FunctionRange,
    ) -> Result<(), IcedError> {
        let function = self.cfg[range.start].label.as_str();
        let slots = self.stack_slots(range);
        let indirect_return_slot = self
            .function_return_indirect_size(function)
            .map(|_| slots.values().copied().max().unwrap_or(0) + STACK_ARG_SLOT_BYTES);
        let stack_size = align_to(
            indirect_return_slot
                .or_else(|| slots.values().copied().max())
                .unwrap_or(0),
            self.target.calling_convention().stack_alignment(),
        );
        let return_register = self
            .function_return_register(&self.cfg[range.start].label)
            .unwrap_or(Register::RAX);
        let context = MachineFunctionContext {
            name: function,
            return_register,
            indirect_return_slot,
        };
        self.set_block_label(assembler, labels, self.cfg[range.start].label.as_str())?;
        assembler.push(rbp)?;
        assembler.mov(rbp, rsp)?;
        if stack_size > 0 {
            assembler.sub(rsp, stack_size as i32)?;
        }
        if let Some(offset) = indirect_return_slot {
            assembler.mov(qword_ptr(rbp - offset as i32), rdi)?;
        }
        self.store_function_params(assembler, &slots, range)?;

        let mut emitted_return = false;
        let mut pending_params = Vec::new();
        for node in self.cfg.function_nodes(&range) {
            let block = &self.cfg[node];
            if node != range.start {
                self.set_block_label(assembler, labels, block.label.as_str())?;
            }
            for inst in &block.instructions {
                if self.emit_instruction(
                    assembler,
                    labels,
                    &slots,
                    context,
                    &mut pending_params,
                    &inst.instruction,
                )? {
                    emitted_return = true;
                }
            }
        }

        if !emitted_return {
            emit_epilogue(assembler)?;
        }
        Ok(())
    }

    fn set_block_label(
        &self,
        assembler: &mut CodeAssembler,
        labels: &mut HashMap<String, CodeLabel>,
        label: &str,
    ) -> Result<(), IcedError> {
        let label = labels.get_mut(label).expect("missing block label");
        assembler.set_label(label)
    }

    fn emit_instruction(
        &self,
        assembler: &mut CodeAssembler,
        labels: &HashMap<String, CodeLabel>,
        slots: &BTreeMap<String, usize>,
        context: MachineFunctionContext<'_>,
        pending_params: &mut Vec<Operand>,
        instruction: &Instruction,
    ) -> Result<bool, IcedError> {
        match instruction {
            Instruction::Borrow(inst) => {
                self.emit_borrow(assembler, slots, inst)?;
                Ok(false)
            }
            Instruction::Load(inst) => {
                self.emit_load(assembler, slots, context.name, inst)?;
                Ok(false)
            }
            Instruction::Store(inst) => {
                self.emit_store(assembler, slots, context.name, inst)?;
                Ok(false)
            }
            Instruction::Assignment(inst) => {
                self.emit_assignment(assembler, slots, context.name, inst)?;
                Ok(false)
            }
            Instruction::Copy(inst) => {
                if self.operand_is_aggregate(context.name, &inst.src) {
                    self.emit_aggregate_copy(assembler, slots, context.name, inst)?;
                } else if self.operand_is_reference(context.name, &inst.src)
                    || self.operand_is_reference(context.name, &inst.dst)
                {
                    load_reference_operand(assembler, slots, &inst.src, rax)?;
                    store_reference_operand(assembler, slots, &inst.dst, rax)?;
                } else if self.operand_is_f32(context.name, &inst.src)
                    || self.operand_is_f32(context.name, &inst.dst)
                {
                    load_float_operand(assembler, slots, &inst.src, xmm0)?;
                    store_float_operand(assembler, slots, &inst.dst, xmm0)?;
                } else {
                    load_operand(assembler, slots, &inst.src, eax)?;
                    store_operand(assembler, slots, &inst.dst, eax)?;
                }
                Ok(false)
            }
            Instruction::ConditionalJump(inst) => {
                self.emit_conditional_jump(assembler, labels, slots, inst)?;
                Ok(false)
            }
            Instruction::Jump(inst) => {
                emit_jump(assembler, labels, &inst.target)?;
                Ok(false)
            }
            Instruction::Return(inst) => {
                if let Some(value) = &inst.value {
                    if self.operand_is_aggregate(context.name, value) {
                        if let Some(size) = self.function_return_indirect_size(context.name) {
                            self.emit_indirect_aggregate_return(
                                assembler,
                                slots,
                                value,
                                size,
                                context
                                    .indirect_return_slot
                                    .expect("missing indirect return slot"),
                            )?;
                        } else if let Some((low, high)) = self.function_return_pair(context.name) {
                            self.load_aggregate_pair(
                                assembler,
                                slots,
                                context.name,
                                value,
                                low,
                                high,
                            )?;
                        } else {
                            self.load_aggregate_operand(
                                assembler,
                                slots,
                                context.name,
                                value,
                                asm_register64(context.return_register),
                            )?;
                        }
                    } else if self.operand_is_reference(context.name, value) {
                        load_reference_operand(
                            assembler,
                            slots,
                            value,
                            asm_register64(context.return_register),
                        )?;
                    } else if self.operand_is_f32(context.name, value) {
                        load_float_operand(
                            assembler,
                            slots,
                            value,
                            asm_register_xmm(context.return_register),
                        )?;
                    } else {
                        load_operand(
                            assembler,
                            slots,
                            value,
                            asm_register32(context.return_register),
                        )?;
                    }
                }
                emit_epilogue(assembler)?;
                Ok(true)
            }
            Instruction::Parameter(inst) => {
                pending_params.push(inst.param.clone());
                Ok(false)
            }
            Instruction::FunctionCall(inst) => {
                self.emit_function_call(
                    assembler,
                    labels,
                    slots,
                    context.name,
                    pending_params,
                    inst,
                )?;
                pending_params.clear();
                Ok(false)
            }
            Instruction::Function(_) | Instruction::EndFunction(_) | Instruction::Extern(_) => {
                Ok(false)
            }
        }
    }

    fn emit_assignment(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        function: &str,
        inst: &AssignmentInstruction,
    ) -> Result<(), IcedError> {
        if self.assignment_uses_f32(function, inst) {
            return self.emit_float_assignment(assembler, slots, inst);
        }
        match (inst.left.as_ref(), inst.operator) {
            (None, operators::UNARY_MINUS) => {
                load_operand(assembler, slots, &inst.right, eax)?;
                assembler.neg(eax)?;
            }
            (None, operators::LOGICAL_NOT) => {
                load_operand(assembler, slots, &inst.right, eax)?;
                assembler.cmp(eax, 0)?;
                assembler.sete(al)?;
                assembler.movzx(eax, al)?;
            }
            (None, operators::BITWISE_NOT) => {
                load_operand(assembler, slots, &inst.right, eax)?;
                assembler.not(eax)?;
            }
            (None, operators::TYPE_CAST) if self.cast_target_is_bool(function, inst) => {
                load_operand(assembler, slots, &inst.right, eax)?;
                assembler.cmp(eax, 0)?;
                assembler.setne(al)?;
                assembler.movzx(eax, al)?;
            }
            (None, _) => {
                load_operand(assembler, slots, &inst.right, eax)?;
            }
            (Some(left), operators::PLUS) => {
                load_operand(assembler, slots, left, eax)?;
                add_operand(assembler, slots, &inst.right)?;
            }
            (Some(left), operators::MINUS) => {
                load_operand(assembler, slots, left, eax)?;
                sub_operand(assembler, slots, &inst.right)?;
            }
            (Some(left), operators::MULTIPLY) => {
                load_operand(assembler, slots, left, eax)?;
                imul_operand(assembler, slots, &inst.right)?;
            }
            (Some(left), operators::DIVIDE | operators::REMAINDER) => {
                load_operand(assembler, slots, left, eax)?;
                assembler.cdq()?;
                load_operand(assembler, slots, &inst.right, ecx)?;
                assembler.idiv(ecx)?;
                if inst.operator == operators::REMAINDER {
                    assembler.mov(eax, edx)?;
                }
            }
            (Some(left), operators::EQUALS | operators::NOT_EQUALS) => {
                self.emit_compare(assembler, slots, left, &inst.right, inst.operator)?;
            }
            (Some(left), operators::LESS | operators::LESS_EQUALS) => {
                self.emit_compare(assembler, slots, left, &inst.right, inst.operator)?;
            }
            (Some(left), operators::GREATER | operators::GREATER_EQUALS) => {
                self.emit_compare(assembler, slots, left, &inst.right, inst.operator)?;
            }
            (Some(left), operators::LOGICAL_AND | operators::BITWISE_AND) => {
                load_operand(assembler, slots, left, eax)?;
                and_operand(assembler, slots, &inst.right)?;
            }
            (Some(left), operators::LOGICAL_OR | operators::BITWISE_OR) => {
                load_operand(assembler, slots, left, eax)?;
                or_operand(assembler, slots, &inst.right)?;
            }
            (Some(left), operators::BITWISE_XOR) => {
                load_operand(assembler, slots, left, eax)?;
                xor_operand(assembler, slots, &inst.right)?;
            }
            (Some(left), operators::SHIFT_LEFT | operators::SHIFT_RIGHT) => {
                load_operand(assembler, slots, left, eax)?;
                load_operand(assembler, slots, &inst.right, ecx)?;
                match self.shift_kind(function, inst) {
                    ShiftKind::Left => assembler.shl(eax, cl)?,
                    ShiftKind::SignedRight => assembler.sar(eax, cl)?,
                    ShiftKind::UnsignedRight => assembler.shr(eax, cl)?,
                }
            }
            (Some(_), _) => {
                unreachable!("unsupported x86 assignment operators are diagnosed before emission")
            }
        }
        store_operand(assembler, slots, &inst.target, eax)
    }

    fn emit_float_assignment(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        inst: &AssignmentInstruction,
    ) -> Result<(), IcedError> {
        match (inst.left.as_ref(), inst.operator) {
            (None, operators::UNARY_MINUS) => {
                load_float_operand(assembler, slots, &inst.right, xmm0)?;
                assembler.mov(eax, i32::MIN)?;
                assembler.movd(xmm1, eax)?;
                assembler.xorps(xmm0, xmm1)?;
                store_float_operand(assembler, slots, &inst.target, xmm0)
            }
            (None, _) => {
                load_float_operand(assembler, slots, &inst.right, xmm0)?;
                store_float_operand(assembler, slots, &inst.target, xmm0)
            }
            (
                Some(left),
                operators::PLUS | operators::MINUS | operators::MULTIPLY | operators::DIVIDE,
            ) => {
                load_float_operand(assembler, slots, left, xmm0)?;
                emit_float_binary(assembler, slots, &inst.right, inst.operator)?;
                store_float_operand(assembler, slots, &inst.target, xmm0)
            }
            (Some(left), operators::EQUALS | operators::NOT_EQUALS)
            | (Some(left), operators::LESS | operators::LESS_EQUALS)
            | (Some(left), operators::GREATER | operators::GREATER_EQUALS) => {
                self.emit_float_compare(assembler, slots, left, &inst.right, inst.operator)?;
                store_operand(assembler, slots, &inst.target, eax)
            }
            (Some(_), _) => {
                unreachable!("unsupported f32 assignment operators are diagnosed before emission")
            }
        }
    }

    fn cast_target_is_bool(&self, function: &str, inst: &AssignmentInstruction) -> bool {
        self.operand_type(function, &inst.target)
            .is_some_and(|ty| is_bool_type(self.types, ty))
    }

    fn emit_compare(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        left: &Operand,
        right: &Operand,
        operator: &str,
    ) -> Result<(), IcedError> {
        load_operand(assembler, slots, left, eax)?;
        cmp_operand(assembler, slots, right)?;
        match operator {
            operators::EQUALS => assembler.sete(al)?,
            operators::NOT_EQUALS => assembler.setne(al)?,
            operators::LESS => assembler.setl(al)?,
            operators::LESS_EQUALS => assembler.setle(al)?,
            operators::GREATER => assembler.setg(al)?,
            operators::GREATER_EQUALS => assembler.setge(al)?,
            _ => unreachable!(),
        }
        assembler.movzx(eax, al)
    }

    fn emit_float_compare(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        left: &Operand,
        right: &Operand,
        operator: &str,
    ) -> Result<(), IcedError> {
        load_float_operand(assembler, slots, left, xmm0)?;
        emit_float_compare_operand(assembler, slots, right)?;
        match operator {
            operators::EQUALS => {
                assembler.sete(al)?;
                assembler.setnp(cl)?;
                assembler.and(al, cl)?;
            }
            operators::NOT_EQUALS => {
                assembler.setne(al)?;
                assembler.setp(cl)?;
                assembler.or(al, cl)?;
            }
            operators::LESS => {
                assembler.setb(al)?;
                assembler.setnp(cl)?;
                assembler.and(al, cl)?;
            }
            operators::LESS_EQUALS => {
                assembler.setbe(al)?;
                assembler.setnp(cl)?;
                assembler.and(al, cl)?;
            }
            operators::GREATER => assembler.seta(al)?,
            operators::GREATER_EQUALS => assembler.setae(al)?,
            _ => unreachable!(),
        }
        assembler.movzx(eax, al)
    }

    fn emit_conditional_jump(
        &self,
        assembler: &mut CodeAssembler,
        labels: &HashMap<String, CodeLabel>,
        slots: &BTreeMap<String, usize>,
        inst: &ConditionalJumpInstruction,
    ) -> Result<(), IcedError> {
        if let Some(left) = &inst.left {
            load_operand(assembler, slots, left, eax)?;
            cmp_operand(assembler, slots, &inst.right)?;
        } else {
            load_operand(assembler, slots, &inst.right, eax)?;
            assembler.cmp(eax, 0)?;
        }
        emit_conditional_branch(assembler, labels, inst.operator, &inst.target)
    }

    fn emit_function_call(
        &self,
        assembler: &mut CodeAssembler,
        labels: &HashMap<String, CodeLabel>,
        slots: &BTreeMap<String, usize>,
        function: &str,
        pending_params: &[Operand],
        inst: &FunctionCallInstruction,
    ) -> Result<(), IcedError> {
        let signature = self
            .function_call_signature(inst)
            .expect("function call should reference a checked function signature");
        let params = pending_params.iter().rev().collect::<Vec<_>>();
        let locations = self
            .target
            .calling_convention()
            .assign_args(self.types, 0, signature);
        let indirect_return = matches!(
            self.target
                .calling_convention()
                .assign_ret(self.types, signature),
            Some(Location::Indirect { .. })
        );
        let stack_args = params
            .iter()
            .zip(locations.iter())
            .zip(signature.params.iter())
            .filter_map(|((param, location), ty)| {
                stack_offset(location).map(|offset| (*param, offset, *ty))
            })
            .collect::<Vec<_>>();
        let stack_arg_slots = stack_args
            .iter()
            .map(|(_, offset, ty)| {
                offset
                    + self
                        .types
                        .size_align(*ty, Bitness::_64)
                        .size
                        .div_ceil(STACK_ARG_SLOT_BYTES)
            })
            .max()
            .unwrap_or(0);
        let fixed_stack_bytes = self.target.calling_convention().fixed_stack_bytes();
        let stack_padding = call_stack_padding(
            stack_arg_slots,
            fixed_stack_bytes,
            self.target.calling_convention().stack_alignment(),
        );
        let reserved_stack_bytes =
            fixed_stack_bytes + stack_padding + stack_arg_slots * STACK_ARG_SLOT_BYTES;
        if reserved_stack_bytes > 0 {
            assembler.sub(rsp, reserved_stack_bytes as i32)?;
        }
        if indirect_return {
            if let Some(return_target) = &inst.return_target {
                assembler.lea(
                    rdi,
                    qword_ptr(rbp - aggregate_stack_offset(slots, return_target, 0)),
                )?;
            }
        }

        for (param, offset, ty) in &stack_args {
            if self.type_is_aggregate(*ty) {
                self.emit_aggregate_stack_argument(assembler, slots, function, param, *offset)?;
                continue;
            } else if self.type_is_reference(*ty) {
                load_reference_operand(assembler, slots, param, rax)?;
            } else if self.operand_is_f32(function, param) {
                load_float_operand(assembler, slots, param, xmm0)?;
                assembler.movd(eax, xmm0)?;
            } else {
                load_operand(assembler, slots, param, eax)?;
            }
            assembler.mov(
                qword_ptr(rsp + (*offset * STACK_ARG_SLOT_BYTES) as i32),
                rax,
            )?;
        }
        for ((param, location), ty) in params
            .iter()
            .zip(locations.iter())
            .zip(signature.params.iter())
        {
            if let Some((low, high)) = register_pair(location) {
                self.load_aggregate_pair(assembler, slots, function, param, low, high)?;
            } else if let Some(register) = outgoing_register(location) {
                if is_xmm_register(register) {
                    load_float_operand(assembler, slots, param, asm_register_xmm(register))?;
                } else if self.type_is_reference(*ty) {
                    load_reference_operand(assembler, slots, param, asm_register64(register))?;
                } else if self.type_is_aggregate(*ty) {
                    self.load_aggregate_operand(
                        assembler,
                        slots,
                        function,
                        param,
                        asm_register64(register),
                    )?;
                } else {
                    load_operand(assembler, slots, param, asm_register32(register))?;
                }
            }
        }
        assembler.call(label_name(labels, inst.function.as_str()))?;
        let stack_cleanup = reserved_stack_bytes;
        if stack_cleanup > 0 {
            assembler.add(rsp, stack_cleanup as i32)?;
        }
        if let Some(return_target) = &inst.return_target {
            let register = self
                .function_call_return_register(inst)
                .unwrap_or(Register::RAX);
            if self.operand_is_aggregate(function, return_target) {
                if indirect_return {
                    // The callee has written directly to the return target and returns it in RAX.
                } else if let Some((low, high)) = self.function_call_return_pair(inst) {
                    self.store_aggregate_pair(
                        assembler,
                        slots,
                        function,
                        return_target,
                        low,
                        high,
                    )?;
                } else {
                    self.store_aggregate_operand(
                        assembler,
                        slots,
                        function,
                        return_target,
                        asm_register64(register),
                    )?;
                }
            } else if self.operand_is_reference(function, return_target) {
                store_reference_operand(assembler, slots, return_target, asm_register64(register))?;
            } else if self.operand_is_f32(function, return_target) {
                store_float_operand(assembler, slots, return_target, asm_register_xmm(register))?;
            } else {
                store_operand(assembler, slots, return_target, asm_register32(register))?;
            }
        }
        Ok(())
    }

    fn store_function_params(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        range: FunctionRange,
    ) -> Result<(), IcedError> {
        let Some(params) = self.function_params(range) else {
            return Ok(());
        };
        let Some(signature) = self.function_signature(&self.cfg[range.start].label) else {
            return Ok(());
        };
        let locations = self
            .target
            .calling_convention()
            .assign_args(self.types, 0, signature);
        for ((param, ty), location) in params
            .iter()
            .zip(signature.params.iter())
            .zip(locations.iter())
        {
            if let Some(offset) = slots.get(param.as_str()) {
                if is_f32_type(self.types, *ty) {
                    if let Some(register) = outgoing_register(location) {
                        assembler
                            .movss(dword_ptr(rbp - *offset as i32), asm_register_xmm(register))?;
                    } else if let Some(stack_offset) = stack_offset(location) {
                        let incoming_offset = incoming_stack_arg_offset(stack_offset);
                        assembler.movss(xmm15, dword_ptr(rbp + incoming_offset as i32))?;
                        assembler.movss(dword_ptr(rbp - *offset as i32), xmm15)?;
                    }
                } else if self.type_is_aggregate(*ty) {
                    if let Some((low, high)) = register_pair(location) {
                        self.store_aggregate_pair(
                            assembler,
                            slots,
                            &self.cfg[range.start].label,
                            &Operand::Variable(param.clone()),
                            low,
                            high,
                        )?;
                    } else if let Some(register) = outgoing_register(location) {
                        self.store_aggregate_operand(
                            assembler,
                            slots,
                            &self.cfg[range.start].label,
                            &Operand::Variable(param.clone()),
                            asm_register64(register),
                        )?;
                    } else if let Some(stack_offset) = stack_offset(location) {
                        self.store_aggregate_stack_param(
                            assembler,
                            slots,
                            &self.cfg[range.start].label,
                            &Operand::Variable(param.clone()),
                            stack_offset,
                        )?;
                    }
                } else if self.type_is_reference(*ty) {
                    if let Some(register) = outgoing_register(location) {
                        assembler.mov(qword_ptr(rbp - *offset as i32), asm_register64(register))?;
                    } else if let Some(stack_offset) = stack_offset(location) {
                        let incoming_offset = incoming_stack_arg_offset(stack_offset);
                        assembler.mov(rax, qword_ptr(rbp + incoming_offset as i32))?;
                        assembler.mov(qword_ptr(rbp - *offset as i32), rax)?;
                    }
                } else if let Some(register) = outgoing_register(location) {
                    assembler.mov(dword_ptr(rbp - *offset as i32), asm_register32(register))?;
                } else if let Some(stack_offset) = stack_offset(location) {
                    let incoming_offset = incoming_stack_arg_offset(stack_offset);
                    assembler.mov(eax, dword_ptr(rbp + incoming_offset as i32))?;
                    assembler.mov(dword_ptr(rbp - *offset as i32), eax)?;
                }
            }
        }
        Ok(())
    }

    fn function_params(&self, range: FunctionRange) -> Option<&[String]> {
        self.cfg[range.start]
            .instructions
            .iter()
            .find_map(|inst| match &inst.instruction {
                Instruction::Function(inst) => Some(inst.params.as_slice()),
                _ => None,
            })
    }

    fn validate_supported(
        &self,
        diag: &mut dyn DiagnosticConsumer,
        options: X86MachineCodeOptions,
    ) -> bool {
        let mut valid = true;
        let mut reported_messages = BTreeSet::new();
        for range in &self.cfg.functions {
            let function = self.cfg[range.start].label.as_str();
            for node in self.cfg.function_nodes(range) {
                for inst in &self.cfg[node].instructions {
                    if let Some(message) = self.unsupported_message(function, &inst.instruction) {
                        valid = false;
                        if reported_messages.insert(message.clone()) {
                            let span = inst
                                .source_span
                                .or_else(|| self.instruction_source_span(&inst.instruction));
                            diag.error(machine_code_error(options, span, message));
                        }
                    }
                }
            }
        }
        valid
    }

    fn instruction_source_span(&self, instruction: &Instruction) -> Option<SourceSpan> {
        match instruction {
            Instruction::Borrow(inst) => self
                .operand_source_span(&inst.target)
                .or_else(|| self.place_source_span(&inst.place)),
            Instruction::Load(inst) => self
                .operand_source_span(&inst.target)
                .or_else(|| self.place_source_span(&inst.place)),
            Instruction::Store(inst) => self
                .place_source_span(&inst.place)
                .or_else(|| self.operand_source_span(&inst.value)),
            Instruction::Assignment(inst) => self
                .operand_source_span(&inst.target)
                .or_else(|| {
                    inst.left
                        .as_ref()
                        .and_then(|left| self.operand_source_span(left))
                })
                .or_else(|| self.operand_source_span(&inst.right)),
            Instruction::Copy(inst) => self
                .operand_source_span(&inst.dst)
                .or_else(|| self.operand_source_span(&inst.src)),
            Instruction::ConditionalJump(inst) => inst
                .left
                .as_ref()
                .and_then(|left| self.operand_source_span(left))
                .or_else(|| self.operand_source_span(&inst.right)),
            Instruction::FunctionCall(inst) => inst
                .return_target
                .as_ref()
                .and_then(|target| self.operand_source_span(target))
                .or_else(|| self.symbol_span(&inst.function)),
            Instruction::Parameter(inst) => self.operand_source_span(&inst.param),
            Instruction::Return(inst) => inst
                .value
                .as_ref()
                .and_then(|value| self.operand_source_span(value)),
            Instruction::Function(inst) => self.symbol_span(&inst.label),
            Instruction::EndFunction(_) | Instruction::Extern(_) | Instruction::Jump(_) => None,
        }
    }

    fn operand_source_span(&self, operand: &Operand) -> Option<SourceSpan> {
        match operand {
            Operand::Variable(name) | Operand::Label(name) => self.symbol_span(name),
            Operand::Temporary(label) => self.symbol_span(label.to_string().as_str()),
            Operand::Literal(_) | Operand::Placeholder => None,
        }
    }

    fn place_source_span(&self, place: &Place) -> Option<SourceSpan> {
        match place {
            Place::Direct(value) | Place::Dereference(value) => self.operand_source_span(value),
            Place::Member { base, .. } => self.place_source_span(base),
            Place::Index { base, index } => self
                .place_source_span(base)
                .or_else(|| self.operand_source_span(index)),
        }
    }

    fn symbol_span(&self, name: &str) -> Option<SourceSpan> {
        self.global_symbol_entry(name).map(|entry| entry.span)
    }

    fn global_symbol_entry(&self, name: &str) -> Option<&SymbolTableEntry> {
        self.symbols
            .lookup(self.symbols.root, name)
            .map(|(_, _, entry)| entry)
    }

    fn function_symbol_entry(&self, function: &str, name: &str) -> Option<&SymbolTableEntry> {
        self.symbols
            .lookup_function(function, name)
            .map(|(_, _, entry)| entry)
    }

    fn unsupported_message(&self, function: &str, instruction: &Instruction) -> Option<String> {
        match instruction {
            Instruction::Borrow(inst) => self
                .unsupported_borrow_message(&inst.place)
                .or_else(|| self.unsupported_operand_message(function, &inst.target)),
            Instruction::Load(inst) => self
                .unsupported_load_message(function, &inst.place)
                .or_else(|| self.unsupported_place_operand_message(function, &inst.place))
                .or_else(|| self.unsupported_operand_message(function, &inst.target)),
            Instruction::Store(inst) => self
                .unsupported_store_message(function, &inst.place)
                .or_else(|| self.unsupported_place_operand_message(function, &inst.place))
                .or_else(|| self.unsupported_operand_message(function, &inst.value)),
            Instruction::Assignment(inst) => self
                .unsupported_assignment_operator_message(function, inst)
                .or_else(|| self.unsupported_reference_operation_message(function, &inst.target))
                .or_else(|| self.unsupported_operand_message(function, &inst.target))
                .or_else(|| {
                    inst.left.as_ref().and_then(|left| {
                        self.unsupported_reference_operation_message(function, left)
                    })
                })
                .or_else(|| {
                    inst.left
                        .as_ref()
                        .and_then(|left| self.unsupported_operand_message(function, left))
                })
                .or_else(|| self.unsupported_reference_operation_message(function, &inst.right))
                .or_else(|| self.unsupported_operand_message(function, &inst.right)),
            Instruction::Extern(inst) => Some(format!(
                "x86 machine-code backend does not support extern declarations yet: {}",
                inst.label
            )),
            Instruction::Copy(inst) => self.unsupported_copy_message(function, inst),
            Instruction::ConditionalJump(inst) => inst
                .left
                .as_ref()
                .and_then(|left| self.unsupported_reference_operation_message(function, left))
                .or_else(|| {
                    inst.left
                        .as_ref()
                        .and_then(|left| self.unsupported_operand_message(function, left))
                })
                .or_else(|| self.unsupported_reference_operation_message(function, &inst.right))
                .or_else(|| self.unsupported_operand_message(function, &inst.right)),
            Instruction::Parameter(inst) => self
                .unsupported_aggregate_operand_message(function, &inst.param, None)
                .or_else(|| self.unsupported_operand_message(function, &inst.param)),
            Instruction::Return(inst) => inst.value.as_ref().and_then(|value| {
                self.unsupported_aggregate_operand_message(function, value, None)
                    .or_else(|| self.unsupported_operand_message(function, value))
            }),
            Instruction::Function(inst) => self.unsupported_function_signature_message(&inst.label),
            Instruction::FunctionCall(inst) => self
                .unsupported_call_target_message(inst)
                .or_else(|| self.unsupported_extern_call_message(inst))
                .or_else(|| self.unsupported_call_signature_message(inst)),
            Instruction::Jump(_) | Instruction::EndFunction(_) => None,
        }
    }

    fn unsupported_borrow_message(&self, place: &Place) -> Option<String> {
        match place {
            Place::Direct(value) if operand_name(value).is_some() => None,
            Place::Direct(_) => Some(String::from(
                "x86 machine-code backend can only borrow stored values",
            )),
            Place::Member { .. } | Place::Index { .. } | Place::Dereference(_) => {
                Some(String::from(
                    "x86 machine-code backend does not support references to projected places yet",
                ))
            }
        }
    }

    fn emit_borrow(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        inst: &crate::generators::tac::instructions::BorrowInstruction,
    ) -> Result<(), IcedError> {
        let Place::Direct(value) = &inst.place else {
            unreachable!("unsupported borrow places are diagnosed before emission")
        };
        assembler.lea(rax, qword_ptr(rbp - stack_slot_offset(slots, value)))?;
        store_reference_operand(assembler, slots, &inst.target, rax)
    }

    fn emit_load(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        function: &str,
        inst: &crate::generators::tac::instructions::LoadInstruction,
    ) -> Result<(), IcedError> {
        match &inst.place {
            Place::Direct(value) => load_operand(assembler, slots, value, eax)?,
            Place::Dereference(reference) => {
                load_reference_operand(assembler, slots, reference, rax)?;
                if self.reference_pointee_size(function, reference) == 1 {
                    assembler.movzx(eax, byte_ptr(rax))?;
                } else {
                    assembler.mov(eax, dword_ptr(rax))?;
                }
            }
            Place::Member { .. } => {
                let (base, offset, ty) = self.member_place(function, &inst.place).unwrap();
                let operand = aggregate_stack_value(slots, &base, offset);
                if self.type_is_aggregate(ty) {
                    let size = self.types.size_align(ty, Bitness::_64).size;
                    return emit_aggregate_region_copy(
                        assembler,
                        slots,
                        (&base, offset),
                        (&inst.target, 0),
                        size,
                    );
                }
                if self.type_is_reference(ty) {
                    assembler.mov(
                        rax,
                        qword_ptr(rbp - aggregate_stack_offset(slots, &base, offset)),
                    )?;
                    return store_reference_operand(assembler, slots, &inst.target, rax);
                }
                if is_f32_type(self.types, ty) {
                    assembler.movss(xmm0, operand)?;
                    return store_float_operand(assembler, slots, &inst.target, xmm0);
                }
                if self.types.size_align(ty, Bitness::_64).size == 1 {
                    assembler.movzx(
                        eax,
                        byte_ptr(rbp - aggregate_stack_offset(slots, &base, offset)),
                    )?;
                } else {
                    assembler.mov(eax, operand)?;
                }
            }
            Place::Index { .. } => {
                unreachable!("unsupported load places are diagnosed before emission")
            }
        }
        store_operand(assembler, slots, &inst.target, eax)
    }

    fn emit_store(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        function: &str,
        inst: &crate::generators::tac::instructions::StoreInstruction,
    ) -> Result<(), IcedError> {
        match &inst.place {
            Place::Direct(target) => {
                load_operand(assembler, slots, &inst.value, eax)?;
                store_operand(assembler, slots, target, eax)
            }
            Place::Dereference(reference) => {
                load_reference_operand(assembler, slots, reference, rax)?;
                load_operand(assembler, slots, &inst.value, ecx)?;
                if self.reference_pointee_size(function, reference) == 1 {
                    assembler.mov(byte_ptr(rax), cl)
                } else {
                    assembler.mov(dword_ptr(rax), ecx)
                }
            }
            Place::Member { .. } => {
                let (base, offset, ty) = self.member_place(function, &inst.place).unwrap();
                let operand = aggregate_stack_value(slots, &base, offset);
                if self.type_is_aggregate(ty) {
                    let size = self.types.size_align(ty, Bitness::_64).size;
                    emit_aggregate_region_copy(
                        assembler,
                        slots,
                        (&inst.value, 0),
                        (&base, offset),
                        size,
                    )
                } else if self.type_is_reference(ty) {
                    load_reference_operand(assembler, slots, &inst.value, rax)?;
                    assembler.mov(
                        qword_ptr(rbp - aggregate_stack_offset(slots, &base, offset)),
                        rax,
                    )
                } else if is_f32_type(self.types, ty) {
                    load_float_operand(assembler, slots, &inst.value, xmm0)?;
                    assembler.movss(operand, xmm0)
                } else {
                    load_operand(assembler, slots, &inst.value, eax)?;
                    if self.types.size_align(ty, Bitness::_64).size == 1 {
                        assembler.mov(
                            byte_ptr(rbp - aggregate_stack_offset(slots, &base, offset)),
                            al,
                        )
                    } else {
                        assembler.mov(operand, eax)
                    }
                }
            }
            Place::Index { .. } => {
                unreachable!("unsupported store places are diagnosed before emission")
            }
        }
    }

    fn operand_is_reference(&self, function: &str, operand: &Operand) -> bool {
        self.operand_type(function, operand)
            .is_some_and(|ty| self.type_is_reference(ty))
    }

    fn operand_is_aggregate(&self, function: &str, operand: &Operand) -> bool {
        self.operand_type(function, operand)
            .is_some_and(|ty| self.type_is_aggregate(ty))
    }

    fn type_is_aggregate(&self, ty: Index) -> bool {
        matches!(
            self.types.get(self.types.canonicalize(ty)),
            Some(Type::TupleType(tuple)) if !tuple.types.is_empty()
        ) || matches!(
            self.types.get(self.types.canonicalize(ty)),
            Some(Type::StructType(_))
        )
    }

    fn aggregate_is_integer_only(&self, ty: Index) -> bool {
        let ty = self.types.canonicalize(ty);
        match self.types.get(ty) {
            Some(Type::TupleType(tuple)) => tuple
                .types
                .iter()
                .all(|ty| self.aggregate_member_is_integer_like(*ty)),
            Some(Type::StructType(struct_)) => struct_
                .members
                .iter()
                .all(|member| self.aggregate_member_is_integer_like(member.ty)),
            _ => false,
        }
    }

    fn aggregate_member_is_integer_like(&self, ty: Index) -> bool {
        match self.types.get(self.types.canonicalize(ty)) {
            Some(
                Type::PrimitiveType(
                    PrimitiveType::BOOL
                    | PrimitiveType::CHAR
                    | PrimitiveType::I32
                    | PrimitiveType::U32,
                )
                | Type::RefType(_),
            ) => true,
            Some(Type::TupleType(tuple)) => tuple
                .types
                .iter()
                .all(|ty| self.aggregate_member_is_integer_like(*ty)),
            Some(Type::StructType(struct_)) => struct_
                .members
                .iter()
                .all(|member| self.aggregate_member_is_integer_like(member.ty)),
            _ => false,
        }
    }

    fn member_place(&self, function: &str, place: &Place) -> Option<(Operand, usize, Index)> {
        let Place::Member { base, member } = place else {
            return None;
        };
        let (base, offset, ty) = match base.as_ref() {
            Place::Direct(base) => (base.clone(), 0, self.operand_type(function, base)?),
            Place::Member { .. } => self.member_place(function, base)?,
            Place::Index { .. } | Place::Dereference(_) => return None,
        };
        let ty = self.types.canonicalize(ty);
        let member_index = match self.types.get(ty)? {
            Type::TupleType(tuple) => member
                .parse()
                .ok()
                .filter(|index| *index < tuple.types.len())?,
            Type::StructType(struct_) => struct_
                .members
                .iter()
                .position(|field| field.name == *member)?,
            _ => return None,
        };
        let member_layout = self.types.memory_layout(ty)?.members().get(member_index)?;
        let member_ty = match self.types.get(ty)? {
            Type::TupleType(tuple) => tuple.types[member_index],
            Type::StructType(struct_) => struct_.members[member_index].ty,
            _ => return None,
        };
        Some((base, offset + member_layout.offset, member_ty))
    }

    fn emit_aggregate_copy(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        function: &str,
        inst: &crate::generators::tac::instructions::CopyInstruction,
    ) -> Result<(), IcedError> {
        let Some(ty) = self.operand_type(function, &inst.src) else {
            return Ok(());
        };
        let size = self.types.size_align(ty, Bitness::_64).size;
        emit_aggregate_region_copy(assembler, slots, (&inst.src, 0), (&inst.dst, 0), size)
    }

    fn aggregate_size(&self, function: &str, operand: &Operand) -> Option<usize> {
        self.operand_type(function, operand)
            .filter(|ty| self.type_is_aggregate(*ty))
            .map(|ty| self.types.size_align(ty, Bitness::_64).size)
    }

    fn aggregate_member_ranges(
        &self,
        ty: Index,
        base_offset: usize,
        ranges: &mut Vec<(usize, usize)>,
    ) {
        let ty = self.types.canonicalize(ty);
        let members = match self.types.get(ty) {
            Some(Type::TupleType(tuple)) => tuple.types.to_vec(),
            Some(Type::StructType(struct_)) => {
                struct_.members.iter().map(|member| member.ty).collect()
            }
            _ => {
                ranges.push((base_offset, self.types.size_align(ty, Bitness::_64).size));
                return;
            }
        };
        let layout = self.types.memory_layout(ty).unwrap();
        for (member, member_layout) in members.iter().zip(layout.members()) {
            self.aggregate_member_ranges(*member, base_offset + member_layout.offset, ranges);
        }
    }

    fn emit_indirect_aggregate_return(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        operand: &Operand,
        size: usize,
        result_slot: usize,
    ) -> Result<(), IcedError> {
        assembler.mov(rax, qword_ptr(rbp - result_slot as i32))?;
        for offset in (0..size).step_by(4) {
            let width = (size - offset).min(4);
            if width == 4 {
                assembler.mov(ecx, aggregate_stack_value(slots, operand, offset))?;
                assembler.mov(dword_ptr(rax + offset as i32), ecx)?;
            } else {
                for byte in 0..width {
                    assembler.movzx(
                        ecx,
                        byte_ptr(rbp - aggregate_stack_offset(slots, operand, offset + byte)),
                    )?;
                    assembler.mov(byte_ptr(rax + (offset + byte) as i32), cl)?;
                }
            }
        }
        assembler.mov(rax, qword_ptr(rbp - result_slot as i32))
    }

    fn load_aggregate_operand(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        function: &str,
        operand: &Operand,
        register: AsmRegister64,
    ) -> Result<(), IcedError> {
        let ty = self.operand_type(function, operand).unwrap();
        let mut ranges = Vec::new();
        self.aggregate_member_ranges(ty, 0, &mut ranges);
        assembler.sub(rsp, 8)?;
        assembler.xor(rax, rax)?;
        assembler.mov(qword_ptr(rsp), rax)?;
        for (member_offset, size) in ranges {
            for offset in (0..size).step_by(4) {
                let width = (size - offset).min(4);
                if width == 4 {
                    assembler.mov(
                        eax,
                        aggregate_stack_value(slots, operand, member_offset + offset),
                    )?;
                    assembler.mov(dword_ptr(rsp + (member_offset + offset) as i32), eax)?;
                } else {
                    for byte in 0..width {
                        assembler.movzx(
                            eax,
                            byte_ptr(
                                rbp - aggregate_stack_offset(
                                    slots,
                                    operand,
                                    member_offset + offset + byte,
                                ),
                            ),
                        )?;
                        assembler
                            .mov(byte_ptr(rsp + (member_offset + offset + byte) as i32), al)?;
                    }
                }
            }
        }
        assembler.mov(register, qword_ptr(rsp))?;
        assembler.add(rsp, 8)
    }

    fn load_aggregate_pair(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        function: &str,
        operand: &Operand,
        low: Register,
        high: Register,
    ) -> Result<(), IcedError> {
        let ty = self.operand_type(function, operand).unwrap();
        let mut member_ranges = Vec::new();
        self.aggregate_member_ranges(ty, 0, &mut member_ranges);
        let low_ranges = member_ranges
            .iter()
            .filter_map(|(offset, size)| {
                let end = (*offset + *size).min(8);
                (*offset < end).then(|| (*offset, end - *offset))
            })
            .collect::<Vec<_>>();
        let high_ranges = member_ranges
            .iter()
            .filter_map(|(offset, size)| {
                let start = (*offset).max(8);
                let end = (*offset + *size).min(16);
                (start < end).then(|| (start - 8, end - start))
            })
            .collect::<Vec<_>>();

        // This materialization uses RAX as scratch, so load RDX before the RAX half.
        self.load_aggregate_operand_part(assembler, slots, operand, 8, &high_ranges, high)?;
        self.load_aggregate_operand_part(assembler, slots, operand, 0, &low_ranges, low)
    }

    fn load_aggregate_operand_part(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        operand: &Operand,
        source_offset: usize,
        member_ranges: &[(usize, usize)],
        register: Register,
    ) -> Result<(), IcedError> {
        assembler.sub(rsp, 8)?;
        assembler.xor(rax, rax)?;
        assembler.mov(qword_ptr(rsp), rax)?;
        for (member_offset, size) in member_ranges {
            for offset in (0..*size).step_by(4) {
                let width = (*size - offset).min(4);
                if width == 4 {
                    assembler.mov(
                        eax,
                        aggregate_stack_value(
                            slots,
                            operand,
                            source_offset + member_offset + offset,
                        ),
                    )?;
                    assembler.mov(dword_ptr(rsp + (member_offset + offset) as i32), eax)?;
                } else {
                    for byte in 0..width {
                        assembler.movzx(
                            eax,
                            byte_ptr(
                                rbp - aggregate_stack_offset(
                                    slots,
                                    operand,
                                    source_offset + member_offset + offset + byte,
                                ),
                            ),
                        )?;
                        assembler
                            .mov(byte_ptr(rsp + (member_offset + offset + byte) as i32), al)?;
                    }
                }
            }
        }
        assembler.mov(asm_register64(register), qword_ptr(rsp))?;
        assembler.add(rsp, 8)
    }

    fn store_aggregate_operand(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        function: &str,
        operand: &Operand,
        register: AsmRegister64,
    ) -> Result<(), IcedError> {
        let size = self.aggregate_size(function, operand).unwrap_or(8);
        assembler.push(register)?;
        for offset in (0..size).step_by(4) {
            let width = (size - offset).min(4);
            if width == 4 {
                assembler.mov(eax, dword_ptr(rsp + offset as i32))?;
                assembler.mov(aggregate_stack_value(slots, operand, offset), eax)?;
            } else {
                for byte in 0..width {
                    assembler.mov(al, byte_ptr(rsp + (offset + byte) as i32))?;
                    assembler.mov(
                        byte_ptr(rbp - aggregate_stack_offset(slots, operand, offset + byte)),
                        al,
                    )?;
                }
            }
        }
        assembler.add(rsp, 8)
    }

    fn store_aggregate_pair(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        function: &str,
        operand: &Operand,
        low: Register,
        high: Register,
    ) -> Result<(), IcedError> {
        self.store_aggregate_operand_part(assembler, slots, function, operand, (0, 8), low)?;
        let size = self.aggregate_size(function, operand).unwrap_or(8);
        self.store_aggregate_operand_part(assembler, slots, function, operand, (8, size - 8), high)
    }

    fn emit_aggregate_stack_argument(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        function: &str,
        operand: &Operand,
        stack_offset: usize,
    ) -> Result<(), IcedError> {
        let size = self.aggregate_size(function, operand).unwrap_or(0);
        for slot in 0..size.div_ceil(STACK_ARG_SLOT_BYTES) {
            assembler.mov(
                qword_ptr(rsp + ((stack_offset + slot) * STACK_ARG_SLOT_BYTES) as i32),
                0,
            )?;
        }
        for offset in (0..size).step_by(4) {
            let width = (size - offset).min(4);
            let destination = (stack_offset * STACK_ARG_SLOT_BYTES + offset) as i32;
            if width == 4 {
                assembler.mov(eax, aggregate_stack_value(slots, operand, offset))?;
                assembler.mov(dword_ptr(rsp + destination), eax)?;
            } else {
                for byte in 0..width {
                    assembler.movzx(
                        eax,
                        byte_ptr(rbp - aggregate_stack_offset(slots, operand, offset + byte)),
                    )?;
                    assembler.mov(byte_ptr(rsp + destination + byte as i32), al)?;
                }
            }
        }
        Ok(())
    }

    fn store_aggregate_stack_param(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        function: &str,
        operand: &Operand,
        stack_offset: usize,
    ) -> Result<(), IcedError> {
        let size = self.aggregate_size(function, operand).unwrap_or(0);
        let incoming = incoming_stack_arg_offset(stack_offset) as i32;
        for offset in (0..size).step_by(4) {
            let width = (size - offset).min(4);
            if width == 4 {
                assembler.mov(eax, dword_ptr(rbp + incoming + offset as i32))?;
                assembler.mov(aggregate_stack_value(slots, operand, offset), eax)?;
            } else {
                for byte in 0..width {
                    assembler.movzx(eax, byte_ptr(rbp + incoming + (offset + byte) as i32))?;
                    assembler.mov(
                        byte_ptr(rbp - aggregate_stack_offset(slots, operand, offset + byte)),
                        al,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn store_aggregate_operand_part(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        _function: &str,
        operand: &Operand,
        range: (usize, usize),
        register: Register,
    ) -> Result<(), IcedError> {
        let (offset, size) = range;
        assembler.push(asm_register64(register))?;
        for part_offset in (0..size).step_by(4) {
            let width = (size - part_offset).min(4);
            if width == 4 {
                assembler.mov(eax, dword_ptr(rsp + part_offset as i32))?;
                assembler.mov(
                    aggregate_stack_value(slots, operand, offset + part_offset),
                    eax,
                )?;
            } else {
                for byte in 0..width {
                    assembler.mov(al, byte_ptr(rsp + (part_offset + byte) as i32))?;
                    assembler.mov(
                        byte_ptr(
                            rbp - aggregate_stack_offset(
                                slots,
                                operand,
                                offset + part_offset + byte,
                            ),
                        ),
                        al,
                    )?;
                }
            }
        }
        assembler.add(rsp, 8)
    }

    fn reference_pointee_size(&self, function: &str, operand: &Operand) -> usize {
        self.operand_type(function, operand)
            .and_then(|ty| self.types.pointee_size_align(ty, Bitness::_64))
            .map(|layout| layout.size)
            .unwrap_or(4)
    }

    fn type_is_reference(&self, ty: Index) -> bool {
        matches!(
            self.types.get(self.types.canonicalize(ty)),
            Some(Type::RefType(_))
        )
    }

    fn unsupported_load_message(&self, function: &str, place: &Place) -> Option<String> {
        match place {
            Place::Member { .. } => self.unsupported_member_message(function, place),
            Place::Index { .. } => Some(String::from(
                "x86 machine-code backend does not support indexed access yet",
            )),
            Place::Dereference(_) | Place::Direct(_) => None,
        }
    }

    fn unsupported_store_message(&self, function: &str, place: &Place) -> Option<String> {
        match place {
            Place::Member { .. } => self.unsupported_member_message(function, place),
            Place::Index { .. } => Some(String::from(
                "x86 machine-code backend does not support stores through indexed access yet",
            )),
            Place::Dereference(_) | Place::Direct(_) => None,
        }
    }

    fn unsupported_member_message(&self, function: &str, place: &Place) -> Option<String> {
        let Some((_, _, ty)) = self.member_place(function, place) else {
            return Some(String::from(
                "x86 machine-code backend does not support member access through projected references yet",
            ));
        };
        let ty = self.types.canonicalize(ty);
        (!(matches!(
            self.types.get(ty),
            Some(Type::PrimitiveType(
                PrimitiveType::U32
                    | PrimitiveType::I32
                    | PrimitiveType::F32
                    | PrimitiveType::BOOL
                    | PrimitiveType::CHAR
            ))
        ) || (self.type_is_reference(ty) && self.types.size_align(ty, Bitness::_64).size == 8)
            || self.type_is_aggregate(ty)))
        .then(|| {
            format!(
                "x86 machine-code backend does not support non-scalar aggregate members yet: {}",
                self.types.to_string_index(ty)
            )
        })
    }

    fn unsupported_place_operand_message(&self, function: &str, place: &Place) -> Option<String> {
        match place {
            Place::Direct(value) | Place::Dereference(value) => {
                self.unsupported_operand_message(function, value)
            }
            Place::Member { base, .. } => self.unsupported_place_operand_message(function, base),
            Place::Index { base, index } => self
                .unsupported_place_operand_message(function, base)
                .or_else(|| self.unsupported_operand_message(function, index)),
        }
    }

    fn unsupported_assignment_operator_message(
        &self,
        function: &str,
        inst: &AssignmentInstruction,
    ) -> Option<String> {
        if inst.left.is_none() && inst.operator == operators::TYPE_CAST {
            return self.unsupported_cast_message(function, inst);
        }
        if self.assignment_uses_f32(function, inst) && !float_assignment_supported(inst) {
            return Some(format!(
                "x86 machine-code backend does not support `{}` operations on f32 values yet",
                inst.operator
            ));
        }
        (!assignment_supported(inst)).then(|| {
            format!(
                "x86 machine-code backend does not support `{}` operations yet",
                inst.operator
            )
        })
    }

    fn unsupported_operand_message(&self, function: &str, operand: &Operand) -> Option<String> {
        match operand {
            Operand::Literal(Lit::String(_)) => Some(String::from(
                "x86 machine-code backend does not support string values yet: &str",
            )),
            Operand::Variable(name) => self
                .function_symbol_entry(function, name)
                .and_then(|entry| entry.var_type)
                .and_then(|ty| self.unsupported_type_message(ty)),
            Operand::Temporary(label) => {
                let name = label.to_string();
                self.function_symbol_entry(function, &name)
                    .and_then(|entry| entry.var_type)
                    .and_then(|ty| self.unsupported_type_message(ty))
            }
            Operand::Literal(_) | Operand::Label(_) | Operand::Placeholder => None,
        }
    }

    fn unsupported_reference_operation_message(
        &self,
        function: &str,
        operand: &Operand,
    ) -> Option<String> {
        self.operand_is_reference(function, operand).then(|| {
            format!(
                "x86 machine-code backend does not support operations on reference values yet: {operand}"
            )
        })
    }

    fn unsupported_function_signature_message(&self, function: &str) -> Option<String> {
        let signature = self.function_signature(function)?;
        if signature.is_vararg {
            return Some(format!(
                "x86 machine-code backend does not support vararg function signatures yet: {function}"
            ));
        }
        let locations = self
            .target
            .calling_convention()
            .assign_args(self.types, 0, signature);
        signature
            .params
            .iter()
            .enumerate()
            .find_map(|(index, ty)| {
                self.unsupported_aggregate_type_message(*ty, locations.get(index))
                    .or_else(|| self.unsupported_type_message(*ty))
            })
            .or_else(|| {
                self.unsupported_aggregate_type_message(
                    signature.return_type,
                    self.target
                        .calling_convention()
                        .assign_ret(self.types, signature)
                        .as_ref(),
                )
            })
            .or_else(|| self.unsupported_type_message(signature.return_type))
    }

    fn unsupported_call_signature_message(&self, inst: &FunctionCallInstruction) -> Option<String> {
        let signature = self.function_call_signature(inst)?;
        if signature.is_vararg {
            return Some(format!(
                "x86 machine-code backend does not support calls to vararg functions yet: {}",
                inst.function
            ));
        }
        let locations = self
            .target
            .calling_convention()
            .assign_args(self.types, 0, signature);
        signature
            .params
            .iter()
            .enumerate()
            .find_map(|(index, ty)| {
                self.unsupported_aggregate_type_message(*ty, locations.get(index))
            })
            .or_else(|| {
                self.unsupported_aggregate_type_message(
                    signature.return_type,
                    self.target
                        .calling_convention()
                        .assign_ret(self.types, signature)
                        .as_ref(),
                )
            })
    }

    fn unsupported_extern_call_message(&self, inst: &FunctionCallInstruction) -> Option<String> {
        (inst.is_direct_function && self.is_extern_function(&inst.function)).then(|| {
            format!(
                "x86 machine-code backend does not support calls to extern functions yet: {}",
                inst.function
            )
        })
    }

    fn unsupported_call_target_message(&self, inst: &FunctionCallInstruction) -> Option<String> {
        (!inst.is_direct_function).then(|| {
            format!(
                "x86 machine-code backend does not support indirect calls through function values yet: {}",
                inst.function
            )
        })
    }

    fn is_extern_function(&self, function: &str) -> bool {
        self.cfg.node_weights().any(|block| {
            block.instructions.iter().any(|inst| {
                matches!(
                    &inst.instruction,
                    Instruction::Extern(external) if external.label == function
                )
            })
        })
    }

    fn unsupported_type_message(&self, ty: Index) -> Option<String> {
        let ty = self.types.canonicalize(ty);
        let name = self.types.to_string_index(ty);
        match self.types.get(ty)? {
            Type::PrimitiveType(PrimitiveType::STR) => Some(format!(
                "x86 machine-code backend does not support string values yet: {name}"
            )),
            Type::RefType(ref_ty)
                if matches!(
                    self.types.get(self.types.canonicalize(ref_ty.to)),
                    Some(Type::PrimitiveType(PrimitiveType::STR))
                ) =>
            {
                Some(format!(
                    "x86 machine-code backend does not support string values yet: {name}"
                ))
            }
            Type::RefType(ref_ty) if self.reference_target_supported(ref_ty.to) => None,
            Type::RefType(_) => Some(format!(
                "x86 machine-code backend does not support references to `{name}` values yet"
            )),
            Type::FunctionType(_) => Some(format!(
                "x86 machine-code backend does not support function values yet: {name}"
            )),
            Type::TupleType(_)
            | Type::StructType(_)
            | Type::TypeDefType(_)
            | Type::PrimitiveType(_)
            | Type::Unknown => None,
        }
    }

    fn reference_target_supported(&self, ty: Index) -> bool {
        matches!(
            self.types.get(self.types.canonicalize(ty)),
            Some(Type::PrimitiveType(
                PrimitiveType::BOOL | PrimitiveType::CHAR | PrimitiveType::I32 | PrimitiveType::U32
            ))
        )
    }

    fn unsupported_aggregate_type_message(
        &self,
        ty: Index,
        location: Option<&Location>,
    ) -> Option<String> {
        if !self.type_is_aggregate(ty)
            || self.aggregate_is_integer_only(ty)
                && (location.is_none()
                    || matches!(
                        location,
                        Some(
                            Location::Register(_) | Location::Stack(_) | Location::Indirect { .. }
                        )
                    )
                    || location.and_then(register_pair).is_some()
                    || location.and_then(stack_offset).is_some())
        {
            return None;
        }
        let detail = match location {
            Some(Location::Pair { .. }) if location.and_then(register_pair).is_some() => {
                "register-pair aggregate ABI values"
            }
            Some(Location::Pair { .. }) => "stack-passed aggregate ABI values",
            Some(Location::Stack(_) | Location::RegisterAndStack(_, _)) => {
                "stack-passed aggregate ABI values"
            }
            Some(Location::Indirect { .. }) => "indirect aggregate ABI values",
            _ if !self.aggregate_is_integer_only(ty) => "non-integer aggregate ABI values",
            _ => "aggregate ABI values",
        };
        Some(format!(
            "x86 machine-code backend does not support {detail} yet: {}",
            self.types.to_string_index(ty)
        ))
    }

    fn unsupported_aggregate_operand_message(
        &self,
        function: &str,
        operand: &Operand,
        location: Option<&Location>,
    ) -> Option<String> {
        self.operand_type(function, operand)
            .and_then(|ty| self.unsupported_aggregate_type_message(ty, location))
    }

    fn unsupported_copy_message(
        &self,
        function: &str,
        inst: &crate::generators::tac::instructions::CopyInstruction,
    ) -> Option<String> {
        if self.operand_is_aggregate(function, &inst.src)
            || self.operand_is_aggregate(function, &inst.dst)
        {
            let source = self.operand_type(function, &inst.src)?;
            let destination = self.operand_type(function, &inst.dst)?;
            if self.types.eq(source, destination) && self.type_is_aggregate(source) {
                None
            } else {
                Some(String::from("x86 machine-code backend does not support aggregate copies with mismatched types"))
            }
        } else {
            self.unsupported_operand_message(function, &inst.dst)
                .or_else(|| self.unsupported_operand_message(function, &inst.src))
        }
    }

    fn operand_is_f32(&self, function: &str, operand: &Operand) -> bool {
        self.operand_type(function, operand)
            .is_some_and(|ty| is_f32_type(self.types, ty))
    }

    fn assignment_uses_f32(&self, function: &str, inst: &AssignmentInstruction) -> bool {
        self.operand_is_f32(function, &inst.target)
            || inst
                .left
                .as_ref()
                .is_some_and(|left| self.operand_is_f32(function, left))
            || self.operand_is_f32(function, &inst.right)
    }

    fn operand_primitive_type(&self, function: &str, operand: &Operand) -> Option<PrimitiveType> {
        let name = operand_name(operand)?;
        let ty = self.function_symbol_entry(function, &name)?.var_type?;
        match self.types.get(self.types.canonicalize(ty))? {
            Type::PrimitiveType(primitive) => Some(*primitive),
            _ => None,
        }
    }

    fn shift_kind(&self, function: &str, inst: &AssignmentInstruction) -> ShiftKind {
        if inst.operator == operators::SHIFT_LEFT {
            ShiftKind::Left
        } else {
            let shifted_type = inst
                .left
                .as_ref()
                .and_then(|left| self.operand_primitive_type(function, left))
                .or_else(|| self.operand_primitive_type(function, &inst.target));
            if shifted_type == Some(PrimitiveType::U32) {
                ShiftKind::UnsignedRight
            } else {
                ShiftKind::SignedRight
            }
        }
    }

    fn stack_slots(&self, range: FunctionRange) -> BTreeMap<String, usize> {
        let function = self.cfg[range.start].label.as_str();
        let mut names = BTreeSet::new();
        for node in self.cfg.function_nodes(&range) {
            for inst in &self.cfg[node].instructions {
                collect_instruction_operands(&inst.instruction, &mut names);
            }
        }
        let mut offset = 0;
        names
            .into_iter()
            .map(|name| {
                let layout = self.symbol_frame_size_align(function, &name);
                offset = layout.align.align(offset);
                offset += layout.size;
                (name, offset)
            })
            .collect()
    }

    fn symbol_frame_size_align(&self, function: &str, name: &str) -> SizeAlign {
        self.function_symbol_entry(function, name)
            .and_then(|entry| entry.var_type)
            .map(|ty| self.types.frame_size_align(ty, Bitness::_64))
            .filter(|layout| layout.size > 0)
            .unwrap_or_else(|| SizeAlign::from_size(4))
    }

    fn unsupported_cast_message(
        &self,
        function: &str,
        inst: &AssignmentInstruction,
    ) -> Option<String> {
        let Some(source_ty) = self.operand_type(function, &inst.right) else {
            return Some(String::from(
                "x86 machine-code backend does not support casts from unknown values yet",
            ));
        };
        let Some(target_ty) = self.operand_type(function, &inst.target) else {
            return Some(String::from(
                "x86 machine-code backend does not support casts to unknown values yet",
            ));
        };
        if is_integer_bool_scalar(self.types, source_ty)
            && is_integer_bool_scalar(self.types, target_ty)
        {
            None
        } else {
            Some(format!(
                "x86 machine-code backend does not support casts from `{}` to `{}` yet",
                self.types.to_string_index(source_ty),
                self.types.to_string_index(target_ty)
            ))
        }
    }

    fn operand_type(&self, function: &str, operand: &Operand) -> Option<Index> {
        match operand {
            Operand::Literal(Lit::Integer(_)) => Some(self.types.i32()),
            Operand::Literal(Lit::Boolean(_)) => Some(self.types.bool()),
            Operand::Literal(Lit::Float(_)) => Some(self.types.f32()),
            Operand::Variable(name) => self
                .function_symbol_entry(function, name)
                .and_then(|entry| entry.var_type),
            Operand::Temporary(label) => {
                let name = label.to_string();
                self.function_symbol_entry(function, &name)
                    .and_then(|entry| entry.var_type)
            }
            Operand::Literal(Lit::String(_)) | Operand::Label(_) | Operand::Placeholder => None,
        }
    }

    fn function_signature(&self, function: &str) -> Option<&FunctionType> {
        self.symbols
            .lookup_function_entry(function)
            .map(|(_, _, entry)| entry)
            .and_then(|entry| entry.var_type)
            .and_then(|ty| self.types.get(self.types.canonicalize(ty)))
            .and_then(|ty| match ty {
                Type::FunctionType(signature) => Some(signature),
                _ => None,
            })
    }

    fn function_call_signature(&self, inst: &FunctionCallInstruction) -> Option<&FunctionType> {
        inst.function_type
            .and_then(|ty| self.types.get(self.types.canonicalize(ty)))
            .and_then(|ty| match ty {
                Type::FunctionType(signature) => Some(signature),
                _ => None,
            })
    }

    fn function_return_register(&self, function: &str) -> Option<Register> {
        let signature = self.function_signature(function)?;
        self.target
            .calling_convention()
            .assign_ret(self.types, signature)
            .as_ref()
            .and_then(outgoing_register)
    }

    fn function_return_pair(&self, function: &str) -> Option<(Register, Register)> {
        let signature = self.function_signature(function)?;
        self.target
            .calling_convention()
            .assign_ret(self.types, signature)
            .as_ref()
            .and_then(register_pair)
    }

    fn function_return_indirect_size(&self, function: &str) -> Option<usize> {
        let signature = self.function_signature(function)?;
        match self
            .target
            .calling_convention()
            .assign_ret(self.types, signature)
        {
            Some(Location::Indirect { size, .. }) => Some(size),
            _ => None,
        }
    }

    fn function_call_return_register(&self, inst: &FunctionCallInstruction) -> Option<Register> {
        let signature = self.function_call_signature(inst)?;
        self.target
            .calling_convention()
            .assign_ret(self.types, signature)
            .as_ref()
            .and_then(outgoing_register)
    }

    fn function_call_return_pair(
        &self,
        inst: &FunctionCallInstruction,
    ) -> Option<(Register, Register)> {
        let signature = self.function_call_signature(inst)?;
        self.target
            .calling_convention()
            .assign_ret(self.types, signature)
            .as_ref()
            .and_then(register_pair)
    }
}

impl<'a> PipelineStage for CodeGeneratorX86Machine<'a> {
    type Input = (
        &'a ControlFlowGraph,
        &'a TypeCollection,
        &'a SymbolTableGraph,
    );
    type Options = X86MachineCodeOptions;
    type Output = X86MachineCode;

    fn new((cfg, types, symbols): Self::Input) -> Self {
        Self {
            cfg,
            types,
            symbols,
            target: X86Target::default(),
        }
    }

    fn exec(self, diag: &mut dyn DiagnosticConsumer, opts: Self::Options) -> Option<Self::Output> {
        if !self.validate_supported(diag, opts) {
            return None;
        }
        match self.emit(opts) {
            Ok(code) => Some(code),
            Err(err) => {
                diag.error(machine_code_error(
                    opts,
                    None,
                    format!("x86 machine-code emission failed: {err}"),
                ));
                None
            }
        }
    }
}

fn emit_epilogue(assembler: &mut CodeAssembler) -> Result<(), IcedError> {
    assembler.mov(rsp, rbp)?;
    assembler.pop(rbp)?;
    assembler.ret()
}

fn load_operand(
    assembler: &mut CodeAssembler,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
    register: AsmRegister32,
) -> Result<(), IcedError> {
    match operand {
        Operand::Literal(_) => assembler.mov(register, literal_value(operand)),
        Operand::Variable(_) | Operand::Temporary(_) => {
            assembler.mov(register, stack_value(slots, operand))
        }
        Operand::Placeholder => assembler.xor(register, register),
        Operand::Label(_) => unreachable!("labels are not valid i32 values"),
    }
}

fn load_float_operand(
    assembler: &mut CodeAssembler,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
    register: AsmRegisterXmm,
) -> Result<(), IcedError> {
    match operand {
        Operand::Literal(Lit::Float(value)) => {
            assembler.mov(eax, float_literal_bits(*value) as i32)?;
            assembler.movd(register, eax)
        }
        Operand::Variable(_) | Operand::Temporary(_) => {
            assembler.movss(register, stack_value(slots, operand))
        }
        Operand::Placeholder => assembler.xorps(register, register),
        Operand::Literal(_) | Operand::Label(_) => {
            unreachable!("f32 values must be literals, variables, or temporaries")
        }
    }
}

fn store_operand(
    assembler: &mut CodeAssembler,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
    register: AsmRegister32,
) -> Result<(), IcedError> {
    if matches!(operand, Operand::Variable(_) | Operand::Temporary(_)) {
        assembler.mov(stack_value(slots, operand), register)?;
    }
    Ok(())
}

fn store_float_operand(
    assembler: &mut CodeAssembler,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
    register: AsmRegisterXmm,
) -> Result<(), IcedError> {
    if matches!(operand, Operand::Variable(_) | Operand::Temporary(_)) {
        assembler.movss(stack_value(slots, operand), register)?;
    }
    Ok(())
}

fn load_reference_operand(
    assembler: &mut CodeAssembler,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
    register: AsmRegister64,
) -> Result<(), IcedError> {
    match operand {
        Operand::Variable(_) | Operand::Temporary(_) => {
            assembler.mov(register, reference_stack_value(slots, operand))
        }
        Operand::Placeholder => assembler.xor(register, register),
        Operand::Literal(_) | Operand::Label(_) => {
            unreachable!("references must be stored in variables or temporaries: {operand:?}")
        }
    }
}

fn store_reference_operand(
    assembler: &mut CodeAssembler,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
    register: AsmRegister64,
) -> Result<(), IcedError> {
    if matches!(operand, Operand::Variable(_) | Operand::Temporary(_)) {
        assembler.mov(reference_stack_value(slots, operand), register)?;
    }
    Ok(())
}

fn add_operand(
    assembler: &mut CodeAssembler,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
) -> Result<(), IcedError> {
    match operand {
        Operand::Literal(_) => assembler.add(eax, literal_value(operand)),
        Operand::Variable(_) | Operand::Temporary(_) => {
            assembler.add(eax, stack_value(slots, operand))
        }
        Operand::Placeholder => assembler.add(eax, 0),
        Operand::Label(_) => unreachable!("labels are not valid i32 values"),
    }
}

fn sub_operand(
    assembler: &mut CodeAssembler,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
) -> Result<(), IcedError> {
    match operand {
        Operand::Literal(_) => assembler.sub(eax, literal_value(operand)),
        Operand::Variable(_) | Operand::Temporary(_) => {
            assembler.sub(eax, stack_value(slots, operand))
        }
        Operand::Placeholder => assembler.sub(eax, 0),
        Operand::Label(_) => unreachable!("labels are not valid i32 values"),
    }
}

fn imul_operand(
    assembler: &mut CodeAssembler,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
) -> Result<(), IcedError> {
    match operand {
        Operand::Literal(_) => assembler.imul_3(eax, eax, literal_value(operand)),
        Operand::Variable(_) | Operand::Temporary(_) => {
            assembler.imul_2(eax, stack_value(slots, operand))
        }
        Operand::Placeholder => assembler.imul_3(eax, eax, 0),
        Operand::Label(_) => unreachable!("labels are not valid i32 values"),
    }
}

fn cmp_operand(
    assembler: &mut CodeAssembler,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
) -> Result<(), IcedError> {
    match operand {
        Operand::Literal(_) => assembler.cmp(eax, literal_value(operand)),
        Operand::Variable(_) | Operand::Temporary(_) => {
            assembler.cmp(eax, stack_value(slots, operand))
        }
        Operand::Placeholder => assembler.cmp(eax, 0),
        Operand::Label(_) => unreachable!("labels are not valid i32 values"),
    }
}

fn emit_float_binary(
    assembler: &mut CodeAssembler,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
    operator: &str,
) -> Result<(), IcedError> {
    match operand {
        Operand::Literal(Lit::Float(_)) => {
            load_float_operand(assembler, slots, operand, xmm1)?;
            emit_float_binary_register(assembler, operator)
        }
        Operand::Variable(_) | Operand::Temporary(_) => {
            emit_float_binary_memory(assembler, stack_value(slots, operand), operator)
        }
        Operand::Placeholder => {
            assembler.xorps(xmm1, xmm1)?;
            emit_float_binary_register(assembler, operator)
        }
        Operand::Literal(_) | Operand::Label(_) => {
            unreachable!("f32 values must be literals, variables, or temporaries")
        }
    }
}

fn emit_float_binary_register(
    assembler: &mut CodeAssembler,
    operator: &str,
) -> Result<(), IcedError> {
    match operator {
        operators::PLUS => assembler.addss(xmm0, xmm1),
        operators::MINUS => assembler.subss(xmm0, xmm1),
        operators::MULTIPLY => assembler.mulss(xmm0, xmm1),
        operators::DIVIDE => assembler.divss(xmm0, xmm1),
        _ => unreachable!(),
    }
}

fn emit_float_binary_memory(
    assembler: &mut CodeAssembler,
    operand: AsmMemoryOperand,
    operator: &str,
) -> Result<(), IcedError> {
    match operator {
        operators::PLUS => assembler.addss(xmm0, operand),
        operators::MINUS => assembler.subss(xmm0, operand),
        operators::MULTIPLY => assembler.mulss(xmm0, operand),
        operators::DIVIDE => assembler.divss(xmm0, operand),
        _ => unreachable!(),
    }
}

fn emit_float_compare_operand(
    assembler: &mut CodeAssembler,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
) -> Result<(), IcedError> {
    match operand {
        Operand::Literal(Lit::Float(_)) => {
            load_float_operand(assembler, slots, operand, xmm1)?;
            assembler.ucomiss(xmm0, xmm1)
        }
        Operand::Variable(_) | Operand::Temporary(_) => {
            assembler.ucomiss(xmm0, stack_value(slots, operand))
        }
        Operand::Placeholder => {
            assembler.xorps(xmm1, xmm1)?;
            assembler.ucomiss(xmm0, xmm1)
        }
        Operand::Literal(_) | Operand::Label(_) => {
            unreachable!("f32 values must be literals, variables, or temporaries")
        }
    }
}

fn and_operand(
    assembler: &mut CodeAssembler,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
) -> Result<(), IcedError> {
    match operand {
        Operand::Literal(_) => assembler.and(eax, literal_value(operand)),
        Operand::Variable(_) | Operand::Temporary(_) => {
            assembler.and(eax, stack_value(slots, operand))
        }
        Operand::Placeholder => assembler.and(eax, 0),
        Operand::Label(_) => unreachable!("labels are not valid i32 values"),
    }
}

fn or_operand(
    assembler: &mut CodeAssembler,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
) -> Result<(), IcedError> {
    match operand {
        Operand::Literal(_) => assembler.or(eax, literal_value(operand)),
        Operand::Variable(_) | Operand::Temporary(_) => {
            assembler.or(eax, stack_value(slots, operand))
        }
        Operand::Placeholder => assembler.or(eax, 0),
        Operand::Label(_) => unreachable!("labels are not valid i32 values"),
    }
}

fn xor_operand(
    assembler: &mut CodeAssembler,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
) -> Result<(), IcedError> {
    match operand {
        Operand::Literal(_) => assembler.xor(eax, literal_value(operand)),
        Operand::Variable(_) | Operand::Temporary(_) => {
            assembler.xor(eax, stack_value(slots, operand))
        }
        Operand::Placeholder => assembler.xor(eax, 0),
        Operand::Label(_) => unreachable!("labels are not valid i32 values"),
    }
}

fn emit_jump(
    assembler: &mut CodeAssembler,
    labels: &HashMap<String, CodeLabel>,
    target: &Operand,
) -> Result<(), IcedError> {
    let label = label_operand(labels, target);
    assembler.jmp(label)
}

fn emit_conditional_branch(
    assembler: &mut CodeAssembler,
    labels: &HashMap<String, CodeLabel>,
    operator: &str,
    target: &Operand,
) -> Result<(), IcedError> {
    let label = label_operand(labels, target);
    match operator {
        operators::EQUALS => assembler.je(label),
        operators::NOT_EQUALS => assembler.jne(label),
        operators::LESS => assembler.jl(label),
        operators::LESS_EQUALS => assembler.jle(label),
        operators::GREATER => assembler.jg(label),
        operators::GREATER_EQUALS => assembler.jge(label),
        _ => assembler.jne(label),
    }
}

fn label_operand(labels: &HashMap<String, CodeLabel>, operand: &Operand) -> CodeLabel {
    let Operand::Label(label) = operand else {
        unreachable!("branch target must be a label")
    };
    label_name(labels, label.as_str())
}

fn label_name(labels: &HashMap<String, CodeLabel>, label: &str) -> CodeLabel {
    *labels
        .get(label)
        .unwrap_or_else(|| panic!("missing block label `{label}`"))
}

fn outgoing_register(location: &Location) -> Option<Register> {
    match location {
        Location::Register(register) | Location::RegisterAndStack(register, _) => Some(*register),
        Location::NoStorage
        | Location::Stack(_)
        | Location::Indirect { .. }
        | Location::Pair { .. } => None,
    }
}

fn register_pair(location: &Location) -> Option<(Register, Register)> {
    let Location::Pair { low, high } = location else {
        return None;
    };
    Some((outgoing_register(low)?, outgoing_register(high)?))
}

fn stack_offset(location: &Location) -> Option<usize> {
    match location {
        Location::Stack(offset) => Some(offset.0),
        Location::RegisterAndStack(_, offset) => Some(offset.0),
        Location::Pair { low, .. } => stack_offset(low),
        Location::NoStorage | Location::Register(_) | Location::Indirect { .. } => None,
    }
}

fn incoming_stack_arg_offset(stack_offset: usize) -> usize {
    2 * STACK_ARG_SLOT_BYTES + stack_offset * STACK_ARG_SLOT_BYTES
}

fn call_stack_padding(
    stack_arg_count: usize,
    fixed_stack_bytes: usize,
    stack_alignment: usize,
) -> usize {
    let outgoing_stack_bytes = stack_arg_count * STACK_ARG_SLOT_BYTES + fixed_stack_bytes;
    let remainder = outgoing_stack_bytes % stack_alignment;
    if remainder == 0 {
        0
    } else {
        stack_alignment - remainder
    }
}

fn asm_register32(register: Register) -> AsmRegister32 {
    match register.full_register() {
        Register::RAX => eax,
        Register::RBX => ebx,
        Register::RCX => ecx,
        Register::RDX => edx,
        Register::RSI => esi,
        Register::RDI => edi,
        Register::RBP => ebp,
        Register::RSP => esp,
        Register::R8 => r8d,
        Register::R9 => r9d,
        Register::R10 => r10d,
        Register::R11 => r11d,
        Register::R12 => r12d,
        Register::R13 => r13d,
        Register::R14 => r14d,
        Register::R15 => r15d,
        _ => unreachable!("x86 machine-code emitter only supports general-purpose registers"),
    }
}

fn asm_register_xmm(register: Register) -> AsmRegisterXmm {
    match register {
        Register::XMM0 => xmm0,
        Register::XMM1 => xmm1,
        Register::XMM2 => xmm2,
        Register::XMM3 => xmm3,
        Register::XMM4 => xmm4,
        Register::XMM5 => xmm5,
        Register::XMM6 => xmm6,
        Register::XMM7 => xmm7,
        Register::XMM8 => xmm8,
        Register::XMM9 => xmm9,
        Register::XMM10 => xmm10,
        Register::XMM11 => xmm11,
        Register::XMM12 => xmm12,
        Register::XMM13 => xmm13,
        Register::XMM14 => xmm14,
        Register::XMM15 => xmm15,
        _ => unreachable!("x86 machine-code emitter expected an XMM register"),
    }
}

fn is_xmm_register(register: Register) -> bool {
    matches!(
        register,
        Register::XMM0
            | Register::XMM1
            | Register::XMM2
            | Register::XMM3
            | Register::XMM4
            | Register::XMM5
            | Register::XMM6
            | Register::XMM7
            | Register::XMM8
            | Register::XMM9
            | Register::XMM10
            | Register::XMM11
            | Register::XMM12
            | Register::XMM13
            | Register::XMM14
            | Register::XMM15
    )
}

fn asm_register64(register: Register) -> AsmRegister64 {
    match register.full_register() {
        Register::RAX => rax,
        Register::RBX => rbx,
        Register::RCX => rcx,
        Register::RDX => rdx,
        Register::RSI => rsi,
        Register::RDI => rdi,
        Register::RBP => rbp,
        Register::RSP => rsp,
        Register::R8 => r8,
        Register::R9 => r9,
        Register::R10 => r10,
        Register::R11 => r11,
        Register::R12 => r12,
        Register::R13 => r13,
        Register::R14 => r14,
        Register::R15 => r15,
        _ => unreachable!("x86 machine-code emitter only supports general-purpose registers"),
    }
}

fn stack_value(slots: &BTreeMap<String, usize>, operand: &Operand) -> AsmMemoryOperand {
    dword_ptr(rbp - stack_slot_offset(slots, operand))
}

fn aggregate_stack_value(
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
    member_offset: usize,
) -> AsmMemoryOperand {
    dword_ptr(rbp - aggregate_stack_offset(slots, operand, member_offset))
}

fn aggregate_stack_offset(
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
    member_offset: usize,
) -> i32 {
    stack_slot_offset(slots, operand) - member_offset as i32
}

fn emit_aggregate_region_copy(
    assembler: &mut CodeAssembler,
    slots: &BTreeMap<String, usize>,
    (source, source_offset): (&Operand, usize),
    (destination, destination_offset): (&Operand, usize),
    size: usize,
) -> Result<(), IcedError> {
    for offset in (0..size).step_by(4) {
        let width = (size - offset).min(4);
        if width == 4 {
            assembler.mov(
                eax,
                aggregate_stack_value(slots, source, source_offset + offset),
            )?;
            assembler.mov(
                aggregate_stack_value(slots, destination, destination_offset + offset),
                eax,
            )?;
        } else {
            for byte in 0..width {
                assembler.movzx(
                    eax,
                    byte_ptr(
                        rbp - aggregate_stack_offset(slots, source, source_offset + offset + byte),
                    ),
                )?;
                assembler.mov(
                    byte_ptr(
                        rbp - aggregate_stack_offset(
                            slots,
                            destination,
                            destination_offset + offset + byte,
                        ),
                    ),
                    al,
                )?;
            }
        }
    }
    Ok(())
}

fn reference_stack_value(slots: &BTreeMap<String, usize>, operand: &Operand) -> AsmMemoryOperand {
    qword_ptr(rbp - stack_slot_offset(slots, operand))
}

fn stack_slot_offset(slots: &BTreeMap<String, usize>, operand: &Operand) -> i32 {
    let name = match operand {
        Operand::Variable(name) => name.clone(),
        Operand::Temporary(label) => label.to_string(),
        _ => unreachable!("operand does not have a stack slot"),
    };
    let offset = slots
        .get(name.as_str())
        .unwrap_or_else(|| panic!("missing stack slot for {name}"));
    *offset as i32
}

fn operand_name(operand: &Operand) -> Option<String> {
    match operand {
        Operand::Variable(name) => Some(name.clone()),
        Operand::Temporary(label) => Some(label.to_string()),
        Operand::Literal(_) | Operand::Label(_) | Operand::Placeholder => None,
    }
}

fn literal_value(operand: &Operand) -> i32 {
    match operand {
        Operand::Literal(Lit::Integer(value)) => (*value)
            .try_into()
            .expect("x86 machine-code emitter only supports i32 integer literals"),
        Operand::Literal(Lit::Boolean(value)) => i32::from(*value),
        Operand::Literal(_) => {
            unreachable!(
                "unsupported x86 machine-code literal values are diagnosed before emission"
            )
        }
        _ => unreachable!(),
    }
}

fn float_literal_bits(value: f64) -> u32 {
    (value as f32).to_bits()
}

fn assignment_supported(inst: &AssignmentInstruction) -> bool {
    matches!(
        (inst.left.as_ref(), inst.operator),
        (
            None,
            operators::UNARY_PLUS
                | operators::UNARY_MINUS
                | operators::LOGICAL_NOT
                | operators::BITWISE_NOT
                | operators::TYPE_CAST
        ) | (Some(_), operators::PLUS)
            | (Some(_), operators::MINUS)
            | (Some(_), operators::MULTIPLY)
            | (Some(_), operators::DIVIDE)
            | (Some(_), operators::REMAINDER)
            | (Some(_), operators::EQUALS)
            | (Some(_), operators::NOT_EQUALS)
            | (Some(_), operators::LESS)
            | (Some(_), operators::LESS_EQUALS)
            | (Some(_), operators::GREATER)
            | (Some(_), operators::GREATER_EQUALS)
            | (Some(_), operators::LOGICAL_AND)
            | (Some(_), operators::LOGICAL_OR)
            | (Some(_), operators::BITWISE_AND)
            | (Some(_), operators::BITWISE_OR)
            | (Some(_), operators::BITWISE_XOR)
            | (Some(_), operators::SHIFT_LEFT)
            | (Some(_), operators::SHIFT_RIGHT)
    )
}

fn float_assignment_supported(inst: &AssignmentInstruction) -> bool {
    matches!(
        (inst.left.as_ref(), inst.operator),
        (None, operators::UNARY_PLUS | operators::UNARY_MINUS)
            | (Some(_), operators::PLUS)
            | (Some(_), operators::MINUS)
            | (Some(_), operators::MULTIPLY)
            | (Some(_), operators::DIVIDE)
            | (Some(_), operators::EQUALS)
            | (Some(_), operators::NOT_EQUALS)
            | (Some(_), operators::LESS)
            | (Some(_), operators::LESS_EQUALS)
            | (Some(_), operators::GREATER)
            | (Some(_), operators::GREATER_EQUALS)
    )
}

fn is_bool_type(types: &TypeCollection, ty: Index) -> bool {
    matches!(
        types.get(types.canonicalize(ty)),
        Some(Type::PrimitiveType(PrimitiveType::BOOL))
    )
}

fn is_f32_type(types: &TypeCollection, ty: Index) -> bool {
    matches!(
        types.get(types.canonicalize(ty)),
        Some(Type::PrimitiveType(PrimitiveType::F32))
    )
}

fn is_integer_bool_scalar(types: &TypeCollection, ty: Index) -> bool {
    matches!(
        types.get(types.canonicalize(ty)),
        Some(Type::PrimitiveType(
            PrimitiveType::BOOL | PrimitiveType::I32 | PrimitiveType::U32
        ))
    )
}

fn collect_instruction_operands(instruction: &Instruction, names: &mut BTreeSet<String>) {
    match instruction {
        Instruction::Borrow(inst) => {
            collect_operand(&inst.target, names);
            collect_place_operands(&inst.place, names);
        }
        Instruction::Load(inst) => {
            collect_operand(&inst.target, names);
            collect_place_operands(&inst.place, names);
        }
        Instruction::Store(inst) => {
            collect_place_operands(&inst.place, names);
            collect_operand(&inst.value, names);
        }
        Instruction::Assignment(inst) => {
            collect_operand(&inst.target, names);
            if let Some(left) = &inst.left {
                collect_operand(left, names);
            }
            collect_operand(&inst.right, names);
        }
        Instruction::Copy(inst) => {
            collect_operand(&inst.src, names);
            collect_operand(&inst.dst, names);
        }
        Instruction::ConditionalJump(inst) => {
            if let Some(left) = &inst.left {
                collect_operand(left, names);
            }
            collect_operand(&inst.right, names);
        }
        Instruction::Parameter(inst) => collect_operand(&inst.param, names),
        Instruction::Return(inst) => {
            if let Some(value) = &inst.value {
                collect_operand(value, names);
            }
        }
        Instruction::FunctionCall(inst) => {
            if let Some(target) = &inst.return_target {
                collect_operand(target, names);
            }
        }
        Instruction::Jump(_)
        | Instruction::Function(_)
        | Instruction::EndFunction(_)
        | Instruction::Extern(_) => {}
    }
}

fn collect_place_operands(place: &Place, names: &mut BTreeSet<String>) {
    match place {
        Place::Direct(value) | Place::Dereference(value) => collect_operand(value, names),
        Place::Member { base, .. } => collect_place_operands(base, names),
        Place::Index { base, index } => {
            collect_place_operands(base, names);
            collect_operand(index, names);
        }
    }
}

fn collect_operand(operand: &Operand, names: &mut BTreeSet<String>) {
    match operand {
        Operand::Variable(name) => {
            names.insert(name.clone());
        }
        Operand::Temporary(label) => {
            names.insert(label.to_string());
        }
        Operand::Literal(_) | Operand::Label(_) | Operand::Placeholder => {}
    }
}

fn machine_code_error(
    _options: X86MachineCodeOptions,
    span: Option<SourceSpan>,
    message: String,
) -> LexionDiagnosticError {
    LexionDiagnosticError {
        src: NamedSource::new("<x86 machine code>", Arc::new(String::new())),
        span: span.unwrap_or_else(|| SourceSpan::from(0)),
        message,
    }
}

fn align_to(value: usize, align: usize) -> usize {
    if value == 0 {
        0
    } else {
        value.div_ceil(align) * align
    }
}
