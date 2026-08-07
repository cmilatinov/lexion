use crate::ast::types::{FunctionType, PrimitiveType, Type, TypeCollection};
use crate::ast::Lit;
use crate::diagnostic::{DiagnosticConsumer, LexionDiagnosticError};
use crate::generators::tac::instructions::{
    AssignmentInstruction, BaseInstruction, BorrowInstruction, CodeLocation,
    ConditionalJumpInstruction, ControlFlowGraph, FunctionCallInstruction, FunctionCallTarget,
    FunctionRange, Instruction, InstructionInstance, LoadInstruction, Operand, Place,
    StoreInstruction,
};
use crate::generators::x86::calling_convention::{CallingConvention, Location};
use crate::generators::x86::{
    AbiLocationRole, AssignedLivenessInterval, Bitness, SizeAlign, StackOffset, X86Target,
};
use crate::operators;
use crate::pipeline::PipelineStage;
use crate::symbol_table::{SymbolTableEntry, SymbolTableGraph};
use generational_arena::Index;
use iced_x86::Register;
use lexion_lib::miette::{NamedSource, SourceSpan};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::sync::Arc;

const STACK_ARG_SLOT_BYTES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X86Assembly {
    text: String,
}

impl X86Assembly {
    pub fn new(text: String) -> Self {
        Self { text }
    }

    pub fn as_str(&self) -> &str {
        self.text.as_str()
    }
}

impl Display for X86Assembly {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.text.as_str())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct X86EmitOptions<'a> {
    pub emit_source_comments: bool,
    pub source: Option<&'a str>,
    pub diagnostic_source: Option<&'a NamedSource<Arc<String>>>,
}

impl<'a> X86EmitOptions<'a> {
    pub fn with_source_comments(source: &'a str) -> Self {
        Self {
            emit_source_comments: true,
            source: Some(source),
            diagnostic_source: None,
        }
    }

    pub fn with_source_comments_and_diagnostics(
        source: &'a str,
        diagnostic_source: &'a NamedSource<Arc<String>>,
    ) -> Self {
        Self {
            emit_source_comments: true,
            source: Some(source),
            diagnostic_source: Some(diagnostic_source),
        }
    }
}

pub struct CodeGeneratorX86<'a> {
    cfg: &'a ControlFlowGraph,
    types: &'a TypeCollection,
    symbols: &'a SymbolTableGraph,
    target: X86Target,
    allocations: Option<&'a AllocationMap>,
}

impl<'a> CodeGeneratorX86<'a> {
    pub fn with_allocations(mut self, allocations: &'a AllocationMap) -> Self {
        self.allocations = Some(allocations);
        self
    }

    fn emit(&self, options: X86EmitOptions<'_>) -> X86Assembly {
        let mut lines = vec![
            String::from(".intel_syntax noprefix"),
            String::from(".text"),
        ];
        for range in &self.cfg.functions {
            if let Some(label) = self.extern_label(*range) {
                lines.push(format!(".extern {label}"));
            } else {
                lines.extend(self.emit_function(*range, options));
            }
        }
        X86Assembly::new(lines.join("\n"))
    }

    fn extern_label(&self, range: FunctionRange) -> Option<&str> {
        self.cfg[range.start]
            .instructions
            .first()
            .and_then(|inst| match &inst.instruction {
                Instruction::Extern(external) => Some(external.label.as_str()),
                _ => None,
            })
    }

    fn emit_function(&self, range: FunctionRange, options: X86EmitOptions<'_>) -> Vec<String> {
        let name = self.cfg[range.start].label.clone();
        let frame = self.frame_layout(range);
        let mut source_line_range = None;
        let mut lines = vec![format!(".global {name}"), format!("{name}:")];
        self.emit_symbol_source_comments(&mut lines, options, &name, &mut source_line_range);
        lines.extend([String::from("  push rbp"), String::from("  mov rbp, rsp")]);
        for register in &frame.saved_registers {
            lines.push(format!("  push {}", register_name(*register)));
        }
        if frame.stack_size > 0 {
            lines.push(format!("  sub rsp, {}", frame.stack_size));
        }
        if let Some(offset) = frame.indirect_return_slot {
            lines.push(format!("  mov QWORD PTR [rbp-{offset}], rdi"));
        }
        self.emit_function_parameter_moves(&mut lines, range, &frame);

        let mut emitted_return = false;
        let mut pending_param_count = 0;
        let mut staged_indirect_target = false;
        for node in self.cfg.function_nodes(&range) {
            let block = &self.cfg[node];
            if node != range.start {
                lines.push(format!("{}:", block.label));
            }
            for (instruction_index, inst) in block.instructions.iter().enumerate() {
                self.emit_instruction_source_comments(
                    &mut lines,
                    options,
                    inst,
                    &mut source_line_range,
                );
                let location = CodeLocation::new(node, instruction_index);
                if matches!(inst.instruction, Instruction::Parameter(_)) && pending_param_count == 0
                {
                    let next_instruction = block.instructions[instruction_index..]
                        .iter()
                        .find(|inst| !matches!(inst.instruction, Instruction::Parameter(_)))
                        .map(|inst| &inst.instruction);
                    if let Some(Instruction::FunctionCall(FunctionCallInstruction {
                        target: FunctionCallTarget::Indirect(target),
                        ..
                    })) = next_instruction
                    {
                        // Keep the target below the staged arguments without reserving an
                        // allocatable register. Exchange restores a pending RAX argument.
                        lines.push(String::from("  push rax"));
                        load_function_operand(&mut lines, &frame, location, target, Register::RAX);
                        lines.push(String::from("  xchg QWORD PTR [rsp], rax"));
                        staged_indirect_target = true;
                    }
                }
                if self.emit_instruction(
                    &mut lines,
                    &frame,
                    name.as_str(),
                    &mut InstructionEmission {
                        location,
                        pending_param_count: &mut pending_param_count,
                        staged_indirect_target,
                    },
                    &inst.instruction,
                ) {
                    emitted_return = true;
                }
                if matches!(inst.instruction, Instruction::FunctionCall(_)) {
                    staged_indirect_target = false;
                }
            }
        }

        if !emitted_return {
            emit_epilogue(&mut lines, &frame);
        }
        lines
    }

    fn emit_instruction_source_comments(
        &self,
        lines: &mut Vec<String>,
        options: X86EmitOptions<'_>,
        instruction: &InstructionInstance,
        previous_range: &mut Option<SourceLineRange>,
    ) {
        let span = instruction
            .source_span
            .or_else(|| self.instruction_source_span(&instruction.instruction));
        self.emit_source_comments_for_span(lines, options, span, previous_range);
    }

    fn emit_symbol_source_comments(
        &self,
        lines: &mut Vec<String>,
        options: X86EmitOptions<'_>,
        name: &str,
        previous_range: &mut Option<SourceLineRange>,
    ) {
        let span = self.symbol_span(name);
        self.emit_source_comments_for_span(lines, options, span, previous_range);
    }

    fn emit_source_comments_for_span(
        &self,
        lines: &mut Vec<String>,
        options: X86EmitOptions<'_>,
        span: Option<SourceSpan>,
        previous_range: &mut Option<SourceLineRange>,
    ) {
        let Some(source) = options.source.filter(|_| options.emit_source_comments) else {
            return;
        };
        let Some(range) = span.and_then(|span| source_line_range(source, span)) else {
            return;
        };
        if Some(range) == *previous_range {
            return;
        }
        lines.extend(source_comments_for_range(source, range));
        *previous_range = Some(range);
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
                .or_else(|| {
                    inst.target
                        .direct_name()
                        .and_then(|name| self.symbol_span(name))
                }),
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
            Operand::Label(name) => self
                .function_symbol_entry(function, name)
                .or_else(|| self.global_symbol_entry(name))
                .and_then(|entry| entry.var_type),
            Operand::Literal(Lit::String(_)) | Operand::Placeholder => None,
        }
    }

    fn emit_instruction(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        function: &str,
        emission: &mut InstructionEmission<'_>,
        instruction: &Instruction,
    ) -> bool {
        let location = emission.location;
        match instruction {
            Instruction::Borrow(inst) => {
                self.emit_borrow(lines, frame, function, location, inst);
                false
            }
            Instruction::Load(inst) => {
                self.emit_load(lines, frame, function, location, inst);
                false
            }
            Instruction::Store(inst) => {
                self.emit_store(lines, frame, function, location, inst);
                false
            }
            Instruction::Assignment(inst) => {
                self.emit_assignment(lines, frame, function, location, inst);
                false
            }
            Instruction::Copy(inst) => {
                let Some(destination) = frame.operand_location(location, &inst.dst) else {
                    return false;
                };
                if self.operand_is_aggregate(function, &inst.src) {
                    self.emit_aggregate_copy(lines, frame, function, location, inst);
                } else if self.operand_is_function(function, &inst.src)
                    || self.operand_is_function(function, &inst.dst)
                {
                    let register = destination.register().unwrap_or(Register::RAX);
                    let preserved = if destination.register().is_none() {
                        preserve_register(lines, frame, location, register)
                    } else {
                        false
                    };
                    load_function_operand(lines, frame, location, &inst.src, register);
                    store_reference_operand(lines, frame, location, &inst.dst, register);
                    restore_register(lines, register, preserved);
                } else if self.operand_is_reference(function, &inst.src)
                    || self.operand_is_reference(function, &inst.dst)
                {
                    load_reference_operand(lines, frame, location, &inst.src, Register::RAX);
                    store_reference_operand(lines, frame, location, &inst.dst, Register::RAX);
                } else if self.operand_is_f32(function, &inst.src)
                    || self.operand_is_f32(function, &inst.dst)
                {
                    load_float_operand(lines, frame, location, &inst.src, Register::XMM0);
                    store_float_operand(lines, frame, location, &inst.dst, Register::XMM0);
                } else {
                    let register = destination.register().unwrap_or(Register::RAX);
                    load_operand(lines, frame, location, &inst.src, register);
                    store_operand(lines, frame, location, &inst.dst, register);
                }
                false
            }
            Instruction::ConditionalJump(inst) => {
                self.emit_conditional_jump(lines, frame, location, inst);
                false
            }
            Instruction::Jump(inst) => {
                lines.push(format!("  jmp {}", inst.target));
                false
            }
            Instruction::Return(inst) => {
                if let Some(value) = &inst.value {
                    let return_register = self
                        .function_return_register(function)
                        .unwrap_or(Register::RAX);
                    if self.operand_is_aggregate(function, value) {
                        if let Some(size) = self.function_return_indirect_size(function) {
                            self.emit_indirect_aggregate_return(
                                lines, frame, location, value, size,
                            );
                        } else if let Some((low, high)) = self.function_return_pair(function) {
                            self.load_aggregate_pair(
                                lines,
                                frame,
                                function,
                                location,
                                value,
                                (low, high),
                            );
                        } else {
                            self.load_aggregate_operand(
                                lines,
                                frame,
                                function,
                                location,
                                value,
                                return_register,
                            );
                        }
                    } else if self.function_returns_function(function) {
                        load_function_operand(lines, frame, location, value, return_register);
                    } else if self.operand_is_reference(function, value) {
                        load_reference_operand(lines, frame, location, value, return_register);
                    } else if self.operand_is_f32(function, value) {
                        load_float_operand(lines, frame, location, value, return_register);
                    } else {
                        load_operand(lines, frame, location, value, return_register);
                    }
                }
                emit_epilogue(lines, frame);
                true
            }
            Instruction::Function(_) | Instruction::EndFunction(_) | Instruction::Extern(_) => {
                false
            }
            Instruction::Parameter(inst) => {
                self.emit_parameter(lines, frame, function, location, &inst.param);
                *emission.pending_param_count += 1;
                false
            }
            Instruction::FunctionCall(inst) => {
                self.emit_function_call(lines, frame, function, emission, inst);
                *emission.pending_param_count = 0;
                false
            }
        }
    }

    fn validate_supported(
        &self,
        diag: &mut dyn DiagnosticConsumer,
        options: X86EmitOptions<'_>,
    ) -> bool {
        let mut valid = true;
        for range in &self.cfg.functions {
            let function = self.cfg[range.start].label.as_str();
            for node in self.cfg.function_nodes(range) {
                for inst in &self.cfg[node].instructions {
                    if let Some(message) = self.unsupported_message(function, &inst.instruction) {
                        valid = false;
                        let span = inst
                            .source_span
                            .or_else(|| self.instruction_source_span(&inst.instruction))
                            .unwrap_or_else(|| SourceSpan::from(0));
                        diag.error(LexionDiagnosticError {
                            src: diagnostic_source(options),
                            span,
                            message,
                        });
                    }
                }
            }
        }
        valid
    }

    fn unsupported_message(&self, function: &str, instruction: &Instruction) -> Option<String> {
        match instruction {
            Instruction::Borrow(inst) => self
                .unsupported_borrow_message(function, &inst.place)
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
            Instruction::FunctionCall(inst) => {
                self.unsupported_call_signature_message(inst).or_else(|| {
                    inst.return_target
                        .as_ref()
                        .and_then(|target| self.unsupported_operand_message(function, target))
                })
            }
            Instruction::Extern(_) => None,
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
            Instruction::Jump(_) | Instruction::EndFunction(_) => None,
        }
    }

    fn unsupported_borrow_message(&self, function: &str, place: &Place) -> Option<String> {
        match place {
            Place::Direct(value) if operand_name(value).is_some() => None,
            Place::Direct(_) => Some(String::from("x86 backend can only borrow stored values")),
            Place::Member { .. } if self.member_place(function, place).is_some() => None,
            Place::Member { .. } => Some(String::from(
                "x86 backend does not support references through projected places yet",
            )),
            Place::Index { .. } => Some(String::from(
                "x86 backend does not support references to indexed places yet",
            )),
            Place::Dereference(_) => Some(String::from(
                "x86 backend does not support references to dereferenced places yet",
            )),
        }
    }

    fn emit_borrow(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        function: &str,
        location: CodeLocation,
        inst: &BorrowInstruction,
    ) {
        let operand = match &inst.place {
            Place::Direct(value) => {
                let Some(AssemblyLocation::FrameStack { offset }) =
                    frame.operand_location(location, value)
                else {
                    unreachable!("borrowed values must have stable frame locations")
                };
                format!("[rbp-{offset}]")
            }
            Place::Member { .. } => {
                let (base, offset, _) = self.member_place(function, &inst.place).unwrap();
                aggregate_member_operand(frame, location, &base, offset)
                    .expect("borrowed aggregate members must have stable frame locations")
            }
            Place::Index { .. } | Place::Dereference(_) => {
                unreachable!("unsupported borrow places are diagnosed before emission")
            }
        };
        let preserved_rax = operand_register(frame, location, &inst.target) != Some(Register::RAX)
            && preserve_register(lines, frame, location, Register::RAX);
        lines.push(format!("  lea rax, {operand}"));
        store_reference_operand(lines, frame, location, &inst.target, Register::RAX);
        restore_register(lines, Register::RAX, preserved_rax);
    }

    fn emit_load(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        function: &str,
        location: CodeLocation,
        inst: &LoadInstruction,
    ) {
        let allocated_target_register = operand_register(frame, location, &inst.target);
        let target_register = allocated_target_register.unwrap_or(Register::RAX);
        let mut preserved_target_register = false;
        match &inst.place {
            Place::Direct(value) => {
                if allocated_target_register.is_none() {
                    preserved_target_register =
                        preserve_register(lines, frame, location, target_register);
                }
                load_operand(lines, frame, location, value, target_register);
            }
            Place::Dereference(reference) => {
                if let Some(size) = self.reference_pointee_aggregate_size(function, reference) {
                    let Some(destination) =
                        aggregate_member_operand(frame, location, &inst.target, 0)
                    else {
                        return;
                    };
                    let preserved_rax = preserve_register(lines, frame, location, Register::RAX);
                    let preserved_rdx = preserve_register(lines, frame, location, Register::RDX);
                    load_reference_operand(lines, frame, location, reference, Register::RDX);
                    emit_memory_copy(lines, "[rdx]", &destination, size);
                    restore_register(lines, Register::RDX, preserved_rdx);
                    restore_register(lines, Register::RAX, preserved_rax);
                    return;
                }
                if allocated_target_register.is_none() {
                    preserved_target_register =
                        preserve_register(lines, frame, location, target_register);
                }
                load_reference_operand(lines, frame, location, reference, target_register);
                if self.reference_pointee_is_function(function, reference) {
                    lines.push(format!(
                        "  mov {}, QWORD PTR [{}]",
                        register_name(target_register),
                        register_name(target_register)
                    ));
                    store_reference_operand(lines, frame, location, &inst.target, target_register);
                    restore_register(lines, target_register, preserved_target_register);
                    return;
                } else if self.reference_pointee_size(function, reference) == 1 {
                    lines.push(format!(
                        "  movzx {}, BYTE PTR [{}]",
                        register_name_32(target_register),
                        register_name(target_register)
                    ));
                } else {
                    lines.push(format!(
                        "  mov {}, DWORD PTR [{}]",
                        register_name_32(target_register),
                        register_name(target_register)
                    ));
                }
            }
            Place::Member { .. } => {
                let (base, offset, ty) = self.member_place(function, &inst.place).unwrap();
                let operand = aggregate_member_operand(frame, location, &base, offset).unwrap();
                if self.type_is_aggregate(ty) {
                    let target =
                        aggregate_member_operand(frame, location, &inst.target, 0).unwrap();
                    let size = self.types.size_align(ty, Bitness::_64).size;
                    let preserved = preserve_register(lines, frame, location, Register::RAX);
                    emit_memory_copy(lines, &operand, &target, size);
                    restore_register(lines, Register::RAX, preserved);
                    return;
                }
                if self.type_is_reference(ty) || self.type_is_function(ty) {
                    let target_register = allocated_target_register.unwrap_or(Register::RAX);
                    let preserved = allocated_target_register.is_none()
                        && preserve_register(lines, frame, location, target_register);
                    lines.push(format!(
                        "  mov {}, QWORD PTR {operand}",
                        register_name(target_register)
                    ));
                    store_reference_operand(lines, frame, location, &inst.target, target_register);
                    restore_register(lines, target_register, preserved);
                    return;
                }
                if is_f32_type(self.types, ty) {
                    lines.push(format!("  movss xmm0, DWORD PTR {operand}"));
                    store_float_operand(lines, frame, location, &inst.target, Register::XMM0);
                    return;
                }
                if allocated_target_register.is_none() {
                    preserved_target_register =
                        preserve_register(lines, frame, location, target_register);
                }
                if self.types.size_align(ty, Bitness::_64).size == 1 {
                    lines.push(format!(
                        "  movzx {}, BYTE PTR {operand}",
                        register_name_32(target_register)
                    ));
                } else {
                    lines.push(format!(
                        "  mov {}, DWORD PTR {operand}",
                        register_name_32(target_register)
                    ));
                }
            }
            Place::Index { .. } => {
                unreachable!("unsupported load places are diagnosed before emission")
            }
        }
        store_operand(lines, frame, location, &inst.target, target_register);
        restore_register(lines, target_register, preserved_target_register);
    }

    fn emit_store(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        function: &str,
        location: CodeLocation,
        inst: &StoreInstruction,
    ) {
        match &inst.place {
            Place::Direct(target) => {
                load_operand(lines, frame, location, &inst.value, Register::RAX);
                store_operand(lines, frame, location, target, Register::RAX);
            }
            Place::Dereference(reference) => {
                if let Some(size) = self.reference_pointee_aggregate_size(function, reference) {
                    let Some(source) = aggregate_member_operand(frame, location, &inst.value, 0)
                    else {
                        return;
                    };
                    let preserved_rax = preserve_register(lines, frame, location, Register::RAX);
                    let preserved_rdx = preserve_register(lines, frame, location, Register::RDX);
                    load_reference_operand(lines, frame, location, reference, Register::RDX);
                    emit_memory_copy(lines, &source, "[rdx]", size);
                    restore_register(lines, Register::RDX, preserved_rdx);
                    restore_register(lines, Register::RAX, preserved_rax);
                    return;
                }
                if self.reference_pointee_is_function(function, reference) {
                    let reference_register = operand_register(frame, location, reference);
                    let value_register = operand_register(frame, location, &inst.value);
                    let preserve_rax = reference_register != Some(Register::RAX)
                        && value_register != Some(Register::RAX)
                        && preserve_register(lines, frame, location, Register::RAX);
                    let preserve_rcx = reference_register != Some(Register::RCX)
                        && value_register != Some(Register::RCX)
                        && preserve_register(lines, frame, location, Register::RCX);
                    if reference_register == Some(Register::RCX)
                        && value_register == Some(Register::RAX)
                    {
                        // Keep the callback while RAX is used to materialize the reference.
                        lines.push(String::from("  push rax"));
                        load_reference_operand(lines, frame, location, reference, Register::RAX);
                        lines.push(String::from("  pop rcx"));
                    } else if reference_register == Some(Register::RCX)
                        && value_register != Some(Register::RCX)
                    {
                        load_reference_operand(lines, frame, location, reference, Register::RAX);
                        load_function_operand(lines, frame, location, &inst.value, Register::RCX);
                    } else {
                        load_function_operand(lines, frame, location, &inst.value, Register::RCX);
                        load_reference_operand(lines, frame, location, reference, Register::RAX);
                    }
                    lines.push(String::from("  mov QWORD PTR [rax], rcx"));
                    restore_register(lines, Register::RCX, preserve_rcx);
                    restore_register(lines, Register::RAX, preserve_rax);
                } else {
                    load_operand(lines, frame, location, &inst.value, Register::RCX);
                    load_reference_operand(lines, frame, location, reference, Register::RAX);
                    if self.reference_pointee_size(function, reference) == 1 {
                        lines.push(String::from("  mov BYTE PTR [rax], cl"));
                    } else {
                        lines.push(String::from("  mov DWORD PTR [rax], ecx"));
                    }
                }
            }
            Place::Member { .. } => {
                let (base, offset, ty) = self.member_place(function, &inst.place).unwrap();
                let operand = aggregate_member_operand(frame, location, &base, offset).unwrap();
                if self.type_is_aggregate(ty) {
                    let source = aggregate_member_operand(frame, location, &inst.value, 0).unwrap();
                    let size = self.types.size_align(ty, Bitness::_64).size;
                    let preserved = preserve_register(lines, frame, location, Register::RAX);
                    emit_memory_copy(lines, &source, &operand, size);
                    restore_register(lines, Register::RAX, preserved);
                } else if self.type_is_reference(ty) || self.type_is_function(ty) {
                    let preserved = preserve_register(lines, frame, location, Register::RAX);
                    if self.type_is_function(ty) {
                        load_function_operand(lines, frame, location, &inst.value, Register::RAX);
                    } else {
                        load_reference_operand(lines, frame, location, &inst.value, Register::RAX);
                    }
                    lines.push(format!("  mov QWORD PTR {operand}, rax"));
                    restore_register(lines, Register::RAX, preserved);
                } else if is_f32_type(self.types, ty) {
                    load_float_operand(lines, frame, location, &inst.value, Register::XMM0);
                    lines.push(format!("  movss DWORD PTR {operand}, xmm0"));
                } else {
                    let allocated_value_register = operand_register(frame, location, &inst.value);
                    let value_register = allocated_value_register.unwrap_or(Register::RAX);
                    let preserved = if allocated_value_register.is_none() {
                        preserve_register(lines, frame, location, Register::RAX)
                    } else {
                        false
                    };
                    load_operand(lines, frame, location, &inst.value, value_register);
                    if self.types.size_align(ty, Bitness::_64).size == 1 {
                        lines.push(format!(
                            "  mov BYTE PTR {operand}, {}",
                            register_name_8(value_register)
                        ));
                    } else {
                        lines.push(format!(
                            "  mov DWORD PTR {operand}, {}",
                            register_name_32(value_register)
                        ));
                    }
                    restore_register(lines, Register::RAX, preserved);
                }
            }
            Place::Index { .. } => {
                unreachable!("unsupported store places are diagnosed before emission")
            }
        }
    }

    fn operand_is_reference(&self, function: &str, operand: &Operand) -> bool {
        self.operand_type(function, operand).is_some_and(|ty| {
            matches!(
                self.types.get(self.types.canonicalize(ty)),
                Some(Type::RefType(_))
            )
        })
    }

    fn operand_is_function(&self, function: &str, operand: &Operand) -> bool {
        self.operand_type(function, operand)
            .is_some_and(|ty| self.type_is_function(ty))
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
                | Type::RefType(_)
                | Type::FunctionType(_),
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

    fn type_is_reference(&self, ty: Index) -> bool {
        matches!(
            self.types.get(self.types.canonicalize(ty)),
            Some(Type::RefType(_))
        )
    }

    fn type_is_function(&self, ty: Index) -> bool {
        matches!(
            self.types.get(self.types.canonicalize(ty)),
            Some(Type::FunctionType(_))
        )
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
        let layout = self.types.memory_layout(ty)?;
        let member_layout = layout.members().get(member_index)?;
        let member_ty = match self.types.get(ty)? {
            Type::TupleType(tuple) => tuple.types[member_index],
            Type::StructType(struct_) => struct_.members[member_index].ty,
            _ => return None,
        };
        Some((base, offset + member_layout.offset, member_ty))
    }

    fn emit_aggregate_copy(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        function: &str,
        location: CodeLocation,
        inst: &crate::generators::tac::instructions::CopyInstruction,
    ) {
        let Some(ty) = self.operand_type(function, &inst.src) else {
            return;
        };
        let size = self.types.size_align(ty, Bitness::_64).size;
        let Some(source) = aggregate_member_operand(frame, location, &inst.src, 0) else {
            return;
        };
        let Some(destination) = aggregate_member_operand(frame, location, &inst.dst, 0) else {
            return;
        };
        let preserved = preserve_register(lines, frame, location, Register::RAX);
        emit_memory_copy(lines, &source, &destination, size);
        restore_register(lines, Register::RAX, preserved);
    }

    fn reference_pointee_size(&self, function: &str, operand: &Operand) -> usize {
        self.operand_type(function, operand)
            .and_then(|ty| self.types.pointee_size_align(ty, Bitness::_64))
            .map(|layout| layout.size)
            .unwrap_or(4)
    }

    fn reference_pointee_is_function(&self, function: &str, operand: &Operand) -> bool {
        self.operand_type(function, operand)
            .and_then(|ty| match self.types.get(self.types.canonicalize(ty)) {
                Some(Type::RefType(ref_ty)) => Some(ref_ty.to),
                _ => None,
            })
            .is_some_and(|ty| self.type_is_function(ty))
    }

    fn reference_pointee_aggregate_size(&self, function: &str, operand: &Operand) -> Option<usize> {
        let ty = self.operand_type(function, operand)?;
        let Type::RefType(reference) = self.types.get(self.types.canonicalize(ty))? else {
            return None;
        };
        self.type_is_aggregate(reference.to)
            .then(|| self.types.size_align(reference.to, Bitness::_64).size)
    }

    fn unsupported_load_message(&self, function: &str, place: &Place) -> Option<String> {
        match place {
            Place::Member { .. } => self.unsupported_member_message(function, place),
            Place::Index { .. } => Some(String::from(
                "x86 backend does not support indexed access yet",
            )),
            Place::Dereference(_) | Place::Direct(_) => None,
        }
    }

    fn unsupported_store_message(&self, function: &str, place: &Place) -> Option<String> {
        match place {
            Place::Member { .. } => self.unsupported_member_message(function, place),
            Place::Index { .. } => Some(String::from(
                "x86 backend does not support stores through indexed access yet",
            )),
            Place::Dereference(_) | Place::Direct(_) => None,
        }
    }

    fn unsupported_member_message(&self, function: &str, place: &Place) -> Option<String> {
        let Some((_, _, ty)) = self.member_place(function, place) else {
            return Some(String::from(
                "x86 backend does not support member access through projected references yet",
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
        ) || ((self.type_is_reference(ty) || self.type_is_function(ty))
            && self.types.size_align(ty, Bitness::_64).size == 8)
            || self.type_is_aggregate(ty)))
        .then(|| {
            format!(
                "x86 backend does not support non-scalar aggregate members yet: {}",
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
                "x86 backend does not support `{}` operations on f32 values yet",
                inst.operator
            ));
        }
        (!assignment_supported(inst)).then(|| {
            format!(
                "x86 backend does not support `{}` operations yet",
                inst.operator
            )
        })
    }

    fn unsupported_cast_message(
        &self,
        function: &str,
        inst: &AssignmentInstruction,
    ) -> Option<String> {
        let Some(source_ty) = self.operand_type(function, &inst.right) else {
            return Some(String::from(
                "x86 backend does not support casts from unknown values yet",
            ));
        };
        let Some(target_ty) = self.operand_type(function, &inst.target) else {
            return Some(String::from(
                "x86 backend does not support casts to unknown values yet",
            ));
        };
        if is_integer_bool_scalar(self.types, source_ty)
            && is_integer_bool_scalar(self.types, target_ty)
        {
            None
        } else {
            Some(format!(
                "x86 backend does not support casts from `{}` to `{}` yet",
                self.types.to_string_index(source_ty),
                self.types.to_string_index(target_ty)
            ))
        }
    }

    fn unsupported_operand_message(&self, function: &str, operand: &Operand) -> Option<String> {
        match operand {
            Operand::Literal(Lit::String(_)) => Some(String::from(
                "x86 backend does not support string values yet: &str",
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
            format!("x86 backend does not support operations on reference values yet: {operand}")
        })
    }

    fn unsupported_function_signature_message(&self, function: &str) -> Option<String> {
        let signature = self.function_signature(function)?;
        if signature.is_vararg {
            return Some(format!(
                "x86 backend does not support vararg function signatures yet: {function}"
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
                "x86 backend does not support calls to vararg functions yet: {}",
                inst.target
            ));
        }
        signature
            .params
            .iter()
            .enumerate()
            .find_map(|(index, ty)| {
                self.unsupported_aggregate_type_message(
                    *ty,
                    self.target
                        .calling_convention()
                        .assign_args(self.types, 0, signature)
                        .get(index),
                )
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

    fn unsupported_type_message(&self, ty: Index) -> Option<String> {
        let ty = self.types.canonicalize(ty);
        let name = self.types.to_string_index(ty);
        match self.types.get(ty)? {
            Type::PrimitiveType(PrimitiveType::STR) => Some(format!(
                "x86 backend does not support string values yet: {name}"
            )),
            Type::RefType(ref_ty)
                if matches!(
                    self.types.get(self.types.canonicalize(ref_ty.to)),
                    Some(Type::PrimitiveType(PrimitiveType::STR))
                ) =>
            {
                Some(format!(
                    "x86 backend does not support string values yet: {name}"
                ))
            }
            Type::RefType(ref_ty) if self.reference_target_supported(ref_ty.to) => None,
            Type::RefType(_) => Some(format!(
                "x86 backend does not support references to `{name}` values yet"
            )),
            Type::FunctionType(_) => None,
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
            Some(
                Type::PrimitiveType(
                    PrimitiveType::BOOL
                        | PrimitiveType::CHAR
                        | PrimitiveType::I32
                        | PrimitiveType::U32
                ) | Type::FunctionType(_)
            )
        ) || (self.type_is_aggregate(ty) && self.aggregate_is_integer_only(ty))
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
                            Location::NoStorage
                                | Location::Register(_)
                                | Location::Stack(_)
                                | Location::Indirect { .. }
                        )
                    )
                    || location.and_then(register_pair).is_some()
                    || location.and_then(stack_location_offset).is_some())
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
            "x86 backend does not support {detail} yet: {}",
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
                Some(String::from(
                    "x86 backend does not support aggregate copies with mismatched types",
                ))
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

    fn function_call_signature(&self, inst: &FunctionCallInstruction) -> Option<&FunctionType> {
        inst.function_type
            .and_then(|ty| self.types.get(self.types.canonicalize(ty)))
            .and_then(|ty| match ty {
                Type::FunctionType(signature) => Some(signature),
                _ => None,
            })
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

    fn function_return_register(&self, function: &str) -> Option<Register> {
        let signature = self.function_signature(function)?;
        self.target
            .calling_convention()
            .assign_ret(self.types, signature)
            .as_ref()
            .and_then(outgoing_register)
    }

    fn function_returns_function(&self, function: &str) -> bool {
        self.function_signature(function)
            .is_some_and(|signature| self.type_is_function(signature.return_type))
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

    fn function_call_indirect_return(&self, inst: &FunctionCallInstruction) -> bool {
        self.function_call_signature(inst).is_some_and(|signature| {
            matches!(
                self.target
                    .calling_convention()
                    .assign_ret(self.types, signature),
                Some(Location::Indirect { .. })
            )
        })
    }

    fn operand_primitive_type(&self, function: &str, operand: &Operand) -> Option<PrimitiveType> {
        let name = operand_name(operand)?;
        let ty = self.function_symbol_entry(function, &name)?.var_type?;
        match self.types.get(self.types.canonicalize(ty))? {
            Type::PrimitiveType(primitive) => Some(*primitive),
            _ => None,
        }
    }

    fn shift_mnemonic(&self, function: &str, inst: &AssignmentInstruction) -> &'static str {
        if inst.operator == operators::SHIFT_LEFT {
            return "shl";
        }
        let shifted_type = inst
            .left
            .as_ref()
            .and_then(|left| self.operand_primitive_type(function, left))
            .or_else(|| self.operand_primitive_type(function, &inst.target));
        if shifted_type == Some(PrimitiveType::U32) {
            "shr"
        } else {
            "sar"
        }
    }

    fn emit_assignment(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        function: &str,
        location: CodeLocation,
        inst: &AssignmentInstruction,
    ) {
        if self.assignment_uses_f32(function, inst) {
            self.emit_float_assignment(lines, frame, location, inst);
            return;
        }
        let target_register = frame
            .operand_location(location, &inst.target)
            .and_then(|location| location.register())
            .unwrap_or(Register::RAX);
        match (inst.left.as_ref(), inst.operator) {
            (None, operators::UNARY_MINUS) => {
                load_operand(lines, frame, location, &inst.right, target_register);
                lines.push(format!("  neg {}", register_name_32(target_register)));
            }
            (None, operators::LOGICAL_NOT) => {
                load_operand(lines, frame, location, &inst.right, Register::RAX);
                lines.push(String::from("  cmp eax, 0"));
                lines.push(String::from("  sete al"));
                lines.push(String::from("  movzx eax, al"));
            }
            (None, operators::BITWISE_NOT) => {
                load_operand(lines, frame, location, &inst.right, target_register);
                lines.push(format!("  not {}", register_name_32(target_register)));
            }
            (None, operators::TYPE_CAST) if self.cast_target_is_bool(function, inst) => {
                load_operand(lines, frame, location, &inst.right, Register::RAX);
                lines.push(String::from("  cmp eax, 0"));
                lines.push(String::from("  setne al"));
                lines.push(String::from("  movzx eax, al"));
            }
            (None, _) => {
                load_operand(lines, frame, location, &inst.right, target_register);
            }
            (Some(left), operators::PLUS) => {
                if frame
                    .operand_location(location, &inst.right)
                    .and_then(AssemblyLocation::register)
                    == Some(target_register)
                {
                    // Preserve a call result before loading the left side into its register.
                    lines.push(format!("  push {}", register_name(target_register)));
                    load_operand(lines, frame, location, left, target_register);
                    lines.push(format!(
                        "  add {}, DWORD PTR [rsp]",
                        register_name_32(target_register)
                    ));
                    lines.push(String::from("  add rsp, 8"));
                    store_operand(lines, frame, location, &inst.target, target_register);
                    return;
                }
                load_operand(lines, frame, location, left, target_register);
                lines.push(format!(
                    "  add {}, {}",
                    register_name_32(target_register),
                    operand_value(frame, location, &inst.right)
                ));
            }
            (Some(left), operators::MINUS) => {
                load_operand(lines, frame, location, left, target_register);
                lines.push(format!(
                    "  sub {}, {}",
                    register_name_32(target_register),
                    operand_value(frame, location, &inst.right)
                ));
            }
            (Some(left), operators::MULTIPLY) => {
                load_operand(lines, frame, location, left, target_register);
                lines.push(format!(
                    "  imul {}, {}",
                    register_name_32(target_register),
                    operand_value(frame, location, &inst.right)
                ));
            }
            (Some(left), operators::DIVIDE | operators::REMAINDER) => {
                load_operand(lines, frame, location, left, Register::RAX);
                lines.push(String::from("  cdq"));
                load_operand(lines, frame, location, &inst.right, Register::RCX);
                lines.push(String::from("  idiv ecx"));
                if inst.operator == operators::REMAINDER {
                    lines.push(String::from("  mov eax, edx"));
                }
            }
            (Some(left), operators::EQUALS | operators::NOT_EQUALS) => Self::emit_compare(
                lines,
                frame,
                location,
                left,
                &inst.right,
                inst.operator,
                target_register,
            ),
            (Some(left), operators::LESS | operators::LESS_EQUALS) => Self::emit_compare(
                lines,
                frame,
                location,
                left,
                &inst.right,
                inst.operator,
                target_register,
            ),
            (Some(left), operators::GREATER | operators::GREATER_EQUALS) => Self::emit_compare(
                lines,
                frame,
                location,
                left,
                &inst.right,
                inst.operator,
                target_register,
            ),
            (Some(left), operators::LOGICAL_AND | operators::BITWISE_AND) => {
                load_operand(lines, frame, location, left, target_register);
                lines.push(format!(
                    "  and {}, {}",
                    register_name_32(target_register),
                    operand_value(frame, location, &inst.right)
                ));
            }
            (Some(left), operators::LOGICAL_OR | operators::BITWISE_OR) => {
                load_operand(lines, frame, location, left, target_register);
                lines.push(format!(
                    "  or {}, {}",
                    register_name_32(target_register),
                    operand_value(frame, location, &inst.right)
                ));
            }
            (Some(left), operators::BITWISE_XOR) => {
                load_operand(lines, frame, location, left, target_register);
                lines.push(format!(
                    "  xor {}, {}",
                    register_name_32(target_register),
                    operand_value(frame, location, &inst.right)
                ));
            }
            (Some(left), operators::SHIFT_LEFT | operators::SHIFT_RIGHT) => {
                let mnemonic = self.shift_mnemonic(function, inst);
                if target_register == Register::RCX {
                    emit_shift_to_rcx_target(lines, frame, location, left, &inst.right, mnemonic);
                    return;
                }
                emit_shift(
                    lines,
                    frame,
                    location,
                    left,
                    &inst.right,
                    target_register,
                    mnemonic,
                );
            }
            (Some(_), _) => {
                unreachable!("unsupported x86 assignment operators are diagnosed before emission")
            }
        }
        let result_register = match (inst.left.as_ref(), inst.operator) {
            (None, operators::LOGICAL_NOT)
            | (Some(_), operators::DIVIDE | operators::REMAINDER)
            | (Some(_), operators::EQUALS | operators::NOT_EQUALS)
            | (Some(_), operators::LESS | operators::LESS_EQUALS)
            | (Some(_), operators::GREATER | operators::GREATER_EQUALS) => target_register,
            (None, operators::TYPE_CAST) if self.cast_target_is_bool(function, inst) => {
                Register::RAX
            }
            _ => target_register,
        };
        store_operand(lines, frame, location, &inst.target, result_register);
    }

    fn emit_float_assignment(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        location: CodeLocation,
        inst: &AssignmentInstruction,
    ) {
        match (inst.left.as_ref(), inst.operator) {
            (None, operators::UNARY_MINUS) => {
                load_float_operand(lines, frame, location, &inst.right, Register::XMM0);
                lines.push(String::from("  mov eax, 0x80000000"));
                lines.push(String::from("  movd xmm1, eax"));
                lines.push(String::from("  xorps xmm0, xmm1"));
                store_float_operand(lines, frame, location, &inst.target, Register::XMM0);
            }
            (None, _) => {
                load_float_operand(lines, frame, location, &inst.right, Register::XMM0);
                store_float_operand(lines, frame, location, &inst.target, Register::XMM0);
            }
            (
                Some(left),
                operators::PLUS | operators::MINUS | operators::MULTIPLY | operators::DIVIDE,
            ) => {
                load_float_operand(lines, frame, location, left, Register::XMM0);
                let right =
                    float_operand_value(lines, frame, location, &inst.right, Register::XMM1);
                let mnemonic = match inst.operator {
                    operators::PLUS => "addss",
                    operators::MINUS => "subss",
                    operators::MULTIPLY => "mulss",
                    operators::DIVIDE => "divss",
                    _ => unreachable!(),
                };
                lines.push(format!("  {mnemonic} xmm0, {right}"));
                store_float_operand(lines, frame, location, &inst.target, Register::XMM0);
            }
            (Some(left), operators::EQUALS | operators::NOT_EQUALS)
            | (Some(left), operators::LESS | operators::LESS_EQUALS)
            | (Some(left), operators::GREATER | operators::GREATER_EQUALS) => {
                let result_register = frame
                    .operand_location(location, &inst.target)
                    .and_then(|location| location.register())
                    .unwrap_or(Register::RAX);
                emit_float_compare(
                    lines,
                    frame,
                    location,
                    left,
                    &inst.right,
                    inst.operator,
                    result_register,
                );
                store_operand(lines, frame, location, &inst.target, result_register);
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
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        location: CodeLocation,
        left: &Operand,
        right: &Operand,
        operator: &str,
        result_register: Register,
    ) {
        let setcc = match operator {
            operators::EQUALS => "sete",
            operators::NOT_EQUALS => "setne",
            operators::LESS => "setl",
            operators::LESS_EQUALS => "setle",
            operators::GREATER => "setg",
            operators::GREATER_EQUALS => "setge",
            _ => unreachable!(),
        };
        load_operand(lines, frame, location, left, result_register);
        lines.push(format!(
            "  cmp {}, {}",
            register_name_32(result_register),
            operand_value(frame, location, right)
        ));
        lines.push(format!("  {setcc} {}", register_name_8(result_register)));
        lines.push(format!(
            "  movzx {}, {}",
            register_name_32(result_register),
            register_name_8(result_register)
        ));
    }

    fn emit_conditional_jump(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        location: CodeLocation,
        inst: &ConditionalJumpInstruction,
    ) {
        let operator = if let Some(left) = &inst.left {
            emit_conditional_compare(lines, frame, location, left, inst.operator, &inst.right)
        } else {
            load_operand(lines, frame, location, &inst.right, Register::RAX);
            lines.push(String::from("  cmp eax, 0"));
            inst.operator
        };
        lines.push(format!("  {} {}", jump_for(operator), inst.target));
    }

    fn emit_function_call(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        function: &str,
        emission: &InstructionEmission<'_>,
        inst: &FunctionCallInstruction,
    ) {
        let location = emission.location;
        let indirect_return = self.function_call_indirect_return(inst);
        let mut staged_indirect_target = emission.staged_indirect_target;
        if !staged_indirect_target && indirect_return {
            if let FunctionCallTarget::Indirect(target) = &inst.target {
                // Zero-argument calls have no ParameterInstruction at which to stage a target.
                lines.push(String::from("  push rax"));
                load_function_operand(lines, frame, location, target, Register::RAX);
                lines.push(String::from("  xchg QWORD PTR [rsp], rax"));
                staged_indirect_target = true;
            }
        }
        let signature = self
            .function_call_signature(inst)
            .expect("function call should reference a checked function signature");
        let arg_locations = self
            .target
            .calling_convention()
            .assign_args(self.types, 0, signature);
        let arg_locations = &arg_locations[..*emission.pending_param_count];
        let arg_types = &signature.params[..*emission.pending_param_count];
        let stack_arg_count = arg_types
            .iter()
            .zip(arg_locations.iter())
            .filter_map(|(ty, location)| {
                stack_location_offset(location).map(|offset| {
                    offset
                        + self
                            .types
                            .size_align(*ty, Bitness::_64)
                            .size
                            .div_ceil(STACK_ARG_SLOT_BYTES)
                })
            })
            .max()
            .unwrap_or(0);
        let indexed_arguments = non_stack_argument_follows_stack_argument(arg_locations);
        let argument_slot_counts = arg_types
            .iter()
            .zip(arg_locations.iter())
            .map(|(ty, location)| self.staged_argument_slot_count(*ty, location))
            .collect::<Vec<_>>();
        let staged_arg_count =
            usize::from(indexed_arguments) * argument_slot_counts.iter().sum::<usize>();
        let stack_padding = frame.call_stack_padding(
            stack_arg_count + staged_arg_count + usize::from(staged_indirect_target),
            self.target.calling_convention().stack_alignment(),
        );
        if indirect_return {
            if let Some(destination) = inst
                .return_target
                .as_ref()
                .and_then(|target| aggregate_member_operand(frame, location, target, 0))
            {
                lines.push(format!("  lea rdi, {destination}"));
            }
        }
        if indexed_arguments {
            emit_indexed_call_arguments(
                lines,
                arg_locations,
                &argument_slot_counts,
                stack_arg_count,
                stack_padding,
            );
        } else {
            for abi_location in arg_locations {
                if matches!(abi_location, Location::NoStorage) {
                    continue;
                }
                if let Some((low, high)) = register_pair(abi_location) {
                    lines.push(format!("  pop {}", register_name(low)));
                    lines.push(format!("  pop {}", register_name(high)));
                } else if let Some(register) = outgoing_register(abi_location) {
                    if is_xmm_register(register) {
                        lines.push(format!(
                            "  movss {}, DWORD PTR [rsp]",
                            register_name(register)
                        ));
                        lines.push(String::from("  add rsp, 8"));
                    } else {
                        lines.push(format!("  pop {}", register_name(register)));
                    }
                }
            }
            emit_call_stack_padding(lines, stack_arg_count, stack_padding);
        }
        let stack_cleanup = stack_arg_count * STACK_ARG_SLOT_BYTES
            + stack_padding
            + staged_arg_count * STACK_ARG_SLOT_BYTES;
        match &inst.target {
            FunctionCallTarget::Direct(name) => lines.push(format!("  call {name}")),
            FunctionCallTarget::Indirect(target) => {
                if staged_indirect_target {
                    lines.push(format!("  call QWORD PTR {}", rsp_slot(stack_cleanup)));
                } else {
                    load_function_operand(lines, frame, location, target, Register::RAX);
                    lines.push(String::from("  call rax"));
                }
            }
        }
        if stack_cleanup > 0 {
            lines.push(format!("  add rsp, {stack_cleanup}"));
        }
        if staged_indirect_target {
            lines.push(String::from("  add rsp, 8"));
        }
        if let Some(return_target) = &inst.return_target {
            let return_register = self
                .function_call_return_register(inst)
                .unwrap_or(Register::RAX);
            if self.operand_is_aggregate(function, return_target) {
                if indirect_return {
                    // The callee has written directly to the return target and returns it in RAX.
                } else if let Some((low, high)) = self.function_call_return_pair(inst) {
                    self.store_aggregate_pair(
                        lines,
                        frame,
                        function,
                        location,
                        return_target,
                        (low, high),
                    );
                } else {
                    self.store_aggregate_from_register(
                        lines,
                        frame,
                        function,
                        location,
                        return_target,
                        return_register,
                    );
                }
            } else if self.operand_is_reference(function, return_target)
                || self.operand_is_function(function, return_target)
            {
                store_reference_operand(lines, frame, location, return_target, return_register);
            } else if self.operand_is_f32(function, return_target) {
                store_float_operand(lines, frame, location, return_target, return_register);
            } else {
                store_operand(lines, frame, location, return_target, return_register);
            }
        }
    }

    fn staged_argument_slot_count(&self, ty: Index, location: &Location) -> usize {
        if self.type_is_aggregate(ty) {
            self.types
                .size_align(ty, Bitness::_64)
                .size
                .div_ceil(STACK_ARG_SLOT_BYTES)
        } else {
            staged_argument_slot_count(location)
        }
    }

    fn emit_parameter(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        function: &str,
        location: CodeLocation,
        operand: &Operand,
    ) {
        if self.operand_is_aggregate(function, operand) {
            let Some(size) = self.aggregate_size(function, operand) else {
                return;
            };
            let Some(source) = aggregate_member_operand(frame, location, operand, 0) else {
                return;
            };
            let slots = size.div_ceil(STACK_ARG_SLOT_BYTES);
            lines.push(format!("  sub rsp, {}", slots * STACK_ARG_SLOT_BYTES));
            for slot in 0..slots {
                lines.push(format!(
                    "  mov QWORD PTR {}, 0",
                    rsp_slot(slot * STACK_ARG_SLOT_BYTES)
                ));
            }
            emit_memory_copy(lines, &source, "[rsp]", size);
            return;
        } else if self.operand_is_f32(function, operand) {
            stage_float_parameter(lines, frame, location, operand);
            return;
        }
        if self.operand_is_reference(function, operand) {
            load_reference_operand(lines, frame, location, operand, Register::RAX);
        } else if self.operand_is_function(function, operand) {
            load_function_operand(lines, frame, location, operand, Register::RAX);
        } else {
            load_operand(lines, frame, location, operand, Register::RAX);
        }
        lines.push(String::from("  push rax"));
    }

    fn frame_layout(&self, range: FunctionRange) -> FrameLayout<'a> {
        if let Some(allocations) = self
            .allocations
            .and_then(|allocations| allocations.get(&range))
        {
            let saved_registers = self.saved_registers(allocations);
            let spill_count = allocations
                .iter()
                .filter_map(|assigned| assigned.location().stack_offset())
                .map(|offset| offset.0 + 1)
                .max()
                .unwrap_or(0);
            let spill_bytes = spill_count * STACK_ARG_SLOT_BYTES;
            let mut home_bytes = align_to(spill_bytes, 8);
            let function = self.cfg[range.start].label.as_str();
            let home_slots = self
                .home_slot_names(range)
                .into_iter()
                .map(|name| {
                    let layout = self.symbol_frame_size_align(function, &name);
                    home_bytes = layout.align.align(home_bytes);
                    home_bytes += layout.size;
                    (
                        name,
                        saved_registers.len() * STACK_ARG_SLOT_BYTES + home_bytes,
                    )
                })
                .collect();
            let indirect_return_slot = self.function_return_indirect_size(function).map(|_| {
                home_bytes += STACK_ARG_SLOT_BYTES;
                saved_registers.len() * STACK_ARG_SLOT_BYTES + home_bytes
            });
            let stack_size = align_to(
                home_bytes,
                self.target.calling_convention().stack_alignment(),
            );
            FrameLayout {
                allocations: Some(allocations),
                fallback_slots: BTreeMap::new(),
                home_slots,
                saved_registers,
                stack_size,
                indirect_return_slot,
            }
        } else {
            let fallback_slots = self.stack_slots(range);
            let indirect_return_slot = self
                .function_return_indirect_size(self.cfg[range.start].label.as_str())
                .map(|_| {
                    fallback_slots.values().copied().max().unwrap_or(0) + STACK_ARG_SLOT_BYTES
                });
            let stack_size = align_to(
                indirect_return_slot
                    .or_else(|| fallback_slots.values().copied().max())
                    .unwrap_or(0),
                self.target.calling_convention().stack_alignment(),
            );
            FrameLayout {
                allocations: None,
                fallback_slots,
                home_slots: BTreeMap::new(),
                saved_registers: Vec::new(),
                stack_size,
                indirect_return_slot,
            }
        }
    }

    fn saved_registers(&self, allocations: &[AssignedLivenessInterval]) -> Vec<Register> {
        let callee_saved = self.target.calling_convention().callee_saved();
        let mut registers = allocations
            .iter()
            .filter_map(|assigned| assigned.location().register())
            .filter(|register| *register != Register::RBP && callee_saved.contains(register))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        registers.sort_by_key(|register| register.number());
        registers
    }

    fn emit_function_parameter_moves(
        &self,
        lines: &mut Vec<String>,
        range: FunctionRange,
        frame: &FrameLayout<'_>,
    ) {
        let Some(allocations) = frame.allocations else {
            return;
        };
        for assigned in allocations {
            for constraint in assigned.constraints() {
                if constraint.location() != CodeLocation::new(range.start, 0)
                    || !matches!(constraint.role(), AbiLocationRole::FunctionParameter { .. })
                {
                    continue;
                }
                let parameter = Operand::Variable(assigned.interval().variable.clone());
                let Some(destination) = frame.operand_location(constraint.location(), &parameter)
                else {
                    continue;
                };
                if self.symbol_is_aggregate(
                    &self.cfg[range.start].label,
                    &assigned.interval().variable,
                ) {
                    if let (Some((low, high)), Some(size), Some(destination)) = (
                        register_pair(constraint.abi_location()),
                        self.aggregate_size(&self.cfg[range.start].label, &parameter),
                        aggregate_member_operand(frame, constraint.location(), &parameter, 0),
                    ) {
                        store_aggregate_register(lines, low, &destination, 8);
                        store_aggregate_register(
                            lines,
                            high,
                            &offset_assembly_operand(&destination, 8),
                            size - 8,
                        );
                        continue;
                    }
                    if let (Some(size), Some(offset), Some(destination)) = (
                        self.aggregate_size(&self.cfg[range.start].label, &parameter),
                        stack_location_offset(constraint.abi_location()),
                        aggregate_member_operand(frame, constraint.location(), &parameter, 0),
                    ) {
                        emit_memory_copy(
                            lines,
                            &format!("[rbp+{}]", 16 + offset * STACK_ARG_SLOT_BYTES),
                            &destination,
                            size,
                        );
                        continue;
                    }
                }
                let Some(source) = incoming_location(constraint.abi_location()) else {
                    continue;
                };
                if self.symbol_is_f32(&self.cfg[range.start].label, &assigned.interval().variable) {
                    move_float_location(lines, source, destination, Register::XMM15);
                } else if self.symbol_is_aggregate(
                    &self.cfg[range.start].label,
                    &assigned.interval().variable,
                ) {
                    let parameter = Operand::Variable(assigned.interval().variable.clone());
                    let size = self.aggregate_size(&self.cfg[range.start].label, &parameter);
                    if let (Some(size), AssemblyLocation::Register(source)) = (size, source) {
                        if let Some(destination) =
                            aggregate_member_operand(frame, constraint.location(), &parameter, 0)
                        {
                            store_aggregate_register(lines, source, &destination, size);
                        }
                    } else if let (Some(size), Some(destination)) = (
                        size,
                        aggregate_member_operand(frame, constraint.location(), &parameter, 0),
                    ) {
                        emit_memory_copy(
                            lines,
                            &assembly_operand(source).replace("DWORD PTR ", ""),
                            &destination,
                            size,
                        );
                    }
                } else if self.symbol_is_reference(
                    &self.cfg[range.start].label,
                    &assigned.interval().variable,
                ) || self
                    .symbol_is_function(&self.cfg[range.start].label, &assigned.interval().variable)
                {
                    let Some(destination) = frame.variable_location(
                        assigned.interval().variable.as_str(),
                        assigned.location(),
                    ) else {
                        continue;
                    };
                    move_location(lines, source, destination, Register::RAX, MoveWidth::Bits64);
                } else {
                    move_location(lines, source, destination, Register::RAX, MoveWidth::Bits32);
                }
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

    fn home_slot_names(&self, range: FunctionRange) -> BTreeSet<String> {
        let function = self.cfg[range.start].label.as_str();
        let mut names = BTreeSet::new();
        for node in self.cfg.function_nodes(&range) {
            for inst in &self.cfg[node].instructions {
                if let Instruction::Borrow(BorrowInstruction {
                    place: Place::Direct(value),
                    ..
                }) = &inst.instruction
                {
                    if let Some(name) = operand_name(value) {
                        names.insert(name);
                    }
                }
                for name in inst
                    .instruction
                    .variables_read()
                    .into_iter()
                    .chain(inst.instruction.variables_written())
                {
                    if self.symbol_is_reference(function, &name)
                        || self.symbol_is_f32(function, &name)
                        || self.symbol_is_aggregate(function, &name)
                    {
                        names.insert(name);
                    }
                }
            }
        }
        names
    }

    fn symbol_is_reference(&self, function: &str, name: &str) -> bool {
        self.function_symbol_entry(function, name)
            .and_then(|entry| entry.var_type)
            .is_some_and(|ty| {
                matches!(
                    self.types.get(self.types.canonicalize(ty)),
                    Some(Type::RefType(_))
                )
            })
    }

    fn symbol_is_function(&self, function: &str, name: &str) -> bool {
        self.function_symbol_entry(function, name)
            .and_then(|entry| entry.var_type)
            .is_some_and(|ty| self.type_is_function(ty))
    }

    fn symbol_frame_size_align(&self, function: &str, name: &str) -> SizeAlign {
        self.function_symbol_entry(function, name)
            .and_then(|entry| entry.var_type)
            .map(|ty| self.types.frame_size_align(ty, Bitness::_64))
            .filter(|layout| layout.size > 0)
            .unwrap_or_else(|| SizeAlign::from_size(4))
    }

    fn symbol_is_f32(&self, function: &str, name: &str) -> bool {
        self.function_symbol_entry(function, name)
            .and_then(|entry| entry.var_type)
            .is_some_and(|ty| is_f32_type(self.types, ty))
    }

    fn symbol_is_aggregate(&self, function: &str, name: &str) -> bool {
        self.function_symbol_entry(function, name)
            .and_then(|entry| entry.var_type)
            .is_some_and(|ty| self.type_is_aggregate(ty))
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

    fn load_aggregate_operand(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        function: &str,
        location: CodeLocation,
        operand: &Operand,
        register: Register,
    ) {
        let Some(source) = aggregate_member_operand(frame, location, operand, 0) else {
            return;
        };
        let Some(ty) = self.operand_type(function, operand) else {
            return;
        };
        let mut ranges = Vec::new();
        self.aggregate_member_ranges(ty, 0, &mut ranges);
        load_aggregate_register(lines, &source, register, &ranges);
    }

    fn store_aggregate_from_register(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        function: &str,
        location: CodeLocation,
        operand: &Operand,
        register: Register,
    ) {
        let Some(destination) = aggregate_member_operand(frame, location, operand, 0) else {
            return;
        };
        let Some(size) = self.aggregate_size(function, operand) else {
            return;
        };
        store_aggregate_register(lines, register, &destination, size);
    }

    fn emit_indirect_aggregate_return(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        location: CodeLocation,
        operand: &Operand,
        size: usize,
    ) {
        let Some(source) = aggregate_member_operand(frame, location, operand, 0) else {
            return;
        };
        let Some(result_slot) = frame.indirect_return_slot else {
            return;
        };
        lines.push(format!("  mov rax, QWORD PTR [rbp-{result_slot}]"));
        for offset in (0..size).step_by(4) {
            let width = (size - offset).min(4);
            let source = offset_assembly_operand(&source, offset);
            if width == 4 {
                lines.push(format!("  mov ecx, DWORD PTR {source}"));
                lines.push(format!("  mov DWORD PTR [rax+{offset}], ecx"));
            } else {
                for byte in 0..width {
                    let source = offset_assembly_operand(&source, byte);
                    lines.push(format!("  mov cl, BYTE PTR {source}"));
                    lines.push(format!("  mov BYTE PTR [rax+{}], cl", offset + byte));
                }
            }
        }
        lines.push(format!("  mov rax, QWORD PTR [rbp-{result_slot}]"));
    }

    fn load_aggregate_pair(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        function: &str,
        location: CodeLocation,
        operand: &Operand,
        registers: (Register, Register),
    ) {
        let (low, high) = registers;
        let Some(source) = aggregate_member_operand(frame, location, operand, 0) else {
            return;
        };
        let Some(ty) = self.operand_type(function, operand) else {
            return;
        };
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
        load_aggregate_register(
            lines,
            &offset_assembly_operand(&source, 8),
            high,
            &high_ranges,
        );
        load_aggregate_register(lines, &source, low, &low_ranges);
    }

    fn store_aggregate_pair(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        function: &str,
        location: CodeLocation,
        operand: &Operand,
        registers: (Register, Register),
    ) {
        let (low, high) = registers;
        let Some(destination) = aggregate_member_operand(frame, location, operand, 0) else {
            return;
        };
        let Some(size) = self.aggregate_size(function, operand) else {
            return;
        };
        store_aggregate_register(lines, low, &destination, 8);
        store_aggregate_register(
            lines,
            high,
            &offset_assembly_operand(&destination, 8),
            size - 8,
        );
    }
}

impl<'a> PipelineStage for CodeGeneratorX86<'a> {
    type Input = (
        &'a ControlFlowGraph,
        &'a TypeCollection,
        &'a SymbolTableGraph,
    );
    type Options = X86EmitOptions<'a>;
    type Output = X86Assembly;

    fn new((cfg, types, symbols): Self::Input) -> Self {
        Self {
            cfg,
            types,
            symbols,
            target: X86Target::default(),
            allocations: None,
        }
    }

    fn exec(self, diag: &mut dyn DiagnosticConsumer, opts: Self::Options) -> Option<Self::Output> {
        if !self.validate_supported(diag, opts) {
            return None;
        }
        Some(self.emit(opts))
    }
}

fn diagnostic_source(options: X86EmitOptions<'_>) -> NamedSource<Arc<String>> {
    options
        .diagnostic_source
        .cloned()
        .unwrap_or_else(|| NamedSource::new("<x86 backend>", Arc::new(String::new())))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceLineRange {
    start: usize,
    end: usize,
}

fn source_line_range(source: &str, span: SourceSpan) -> Option<SourceLineRange> {
    let start = span.offset();
    let end = start.saturating_add(span.len().max(1));
    if start >= source.len() {
        return None;
    }

    let mut cursor = 0;
    let mut start_line = None;
    let mut end_line = None;
    for (line_index, raw_line) in source.split_inclusive('\n').enumerate() {
        let line_end = cursor + raw_line.len();
        if line_end > start && start_line.is_none() {
            start_line = Some(line_index);
        }
        if line_end >= end && end_line.is_none() {
            end_line = Some(line_index);
        }
        cursor = line_end;
    }

    start_line.map(|start| SourceLineRange {
        start,
        end: end_line.unwrap_or(start),
    })
}

fn source_comments_for_range(source: &str, range: SourceLineRange) -> Vec<String> {
    source
        .split_inclusive('\n')
        .enumerate()
        .filter_map(|(line_index, raw_line)| {
            if line_index < range.start || range.end < line_index {
                return None;
            }
            let line = raw_line.trim_end_matches(&['\r', '\n'][..]);
            Some(source_comment(line))
        })
        .collect()
}

fn source_comment(line: &str) -> String {
    if line.is_empty() {
        String::from("#")
    } else {
        format!("# {line}")
    }
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
            if let FunctionCallTarget::Indirect(target) = &inst.target {
                collect_operand(target, names);
            }
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

fn emit_epilogue(lines: &mut Vec<String>, frame: &FrameLayout<'_>) {
    if frame.stack_size > 0 {
        lines.push(format!("  add rsp, {}", frame.stack_size));
    }
    for register in frame.saved_registers.iter().rev() {
        lines.push(format!("  pop {}", register_name(*register)));
    }
    lines.push(String::from("  pop rbp"));
    lines.push(String::from("  ret"));
}

pub type AllocationMap = std::collections::HashMap<FunctionRange, Vec<AssignedLivenessInterval>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssemblyLocation {
    Register(Register),
    FrameStack { offset: usize },
    IncomingStack { offset: usize },
}

impl AssemblyLocation {
    fn register(self) -> Option<Register> {
        match self {
            AssemblyLocation::Register(register) => Some(register),
            AssemblyLocation::FrameStack { .. } | AssemblyLocation::IncomingStack { .. } => None,
        }
    }
}

struct FrameLayout<'a> {
    allocations: Option<&'a [AssignedLivenessInterval]>,
    fallback_slots: BTreeMap<String, usize>,
    home_slots: BTreeMap<String, usize>,
    saved_registers: Vec<Register>,
    stack_size: usize,
    indirect_return_slot: Option<usize>,
}

struct InstructionEmission<'a> {
    location: CodeLocation,
    pending_param_count: &'a mut usize,
    staged_indirect_target: bool,
}

impl<'a> FrameLayout<'a> {
    fn variable_location(&self, name: &str, allocated: &Location) -> Option<AssemblyLocation> {
        self.home_slots
            .get(name)
            .map(|offset| AssemblyLocation::FrameStack { offset: *offset })
            .or_else(|| self.frame_location(allocated))
    }

    fn operand_location(
        &self,
        location: CodeLocation,
        operand: &Operand,
    ) -> Option<AssemblyLocation> {
        let name = operand_name(operand)?;
        if let Some(offset) = self.home_slots.get(name.as_str()) {
            return Some(AssemblyLocation::FrameStack { offset: *offset });
        }
        if let Some(allocations) = self.allocations {
            return allocations
                .iter()
                .find(|assigned| {
                    assigned.interval().variable == name
                        && assigned.interval().span.start <= location
                        && location < assigned.interval().span.end
                })
                .and_then(|assigned| self.frame_location(assigned.location()));
        }
        self.fallback_slots
            .get(name.as_str())
            .map(|offset| AssemblyLocation::FrameStack { offset: *offset })
    }

    fn frame_location(&self, location: &Location) -> Option<AssemblyLocation> {
        match location {
            Location::Register(register) => Some(AssemblyLocation::Register(*register)),
            Location::Stack(offset) => Some(AssemblyLocation::FrameStack {
                offset: self.frame_stack_offset(*offset),
            }),
            Location::RegisterAndStack(register, _) => Some(AssemblyLocation::Register(*register)),
            Location::NoStorage | Location::Indirect { .. } | Location::Pair { .. } => None,
        }
    }

    fn frame_stack_offset(&self, offset: StackOffset) -> usize {
        self.saved_registers.len() * STACK_ARG_SLOT_BYTES + (offset.0 + 1) * STACK_ARG_SLOT_BYTES
    }

    fn register_occupied(&self, location: CodeLocation, register: Register) -> bool {
        self.allocations.is_some_and(|allocations| {
            allocations.iter().any(|assigned| {
                !self
                    .home_slots
                    .contains_key(assigned.interval().variable.as_str())
                    && assigned.interval().span.start <= location
                    && location < assigned.interval().span.end
                    && self.frame_location(assigned.location())
                        == Some(AssemblyLocation::Register(register))
            })
        })
    }

    fn call_stack_padding(&self, stack_arg_count: usize, stack_alignment: usize) -> usize {
        let base_offset = (self.saved_registers.len() * STACK_ARG_SLOT_BYTES) % stack_alignment;
        let outgoing_offset = stack_arg_count * STACK_ARG_SLOT_BYTES;
        let remainder = (base_offset + outgoing_offset) % stack_alignment;
        if remainder == 0 {
            0
        } else {
            stack_alignment - remainder
        }
    }
}

fn emit_shift(
    lines: &mut Vec<String>,
    frame: &FrameLayout<'_>,
    location: CodeLocation,
    left: &Operand,
    right: &Operand,
    result_register: Register,
    mnemonic: &str,
) {
    load_operand(lines, frame, location, left, result_register);
    let preserve_rcx = frame.register_occupied(location, Register::RCX);
    if preserve_rcx {
        lines.push(String::from("  push rcx"));
    }
    load_operand(lines, frame, location, right, Register::RCX);
    lines.push(format!(
        "  {mnemonic} {}, cl",
        register_name_32(result_register)
    ));
    if preserve_rcx {
        lines.push(String::from("  pop rcx"));
    }
}

fn emit_shift_to_rcx_target(
    lines: &mut Vec<String>,
    frame: &FrameLayout<'_>,
    location: CodeLocation,
    left: &Operand,
    right: &Operand,
    mnemonic: &str,
) {
    lines.push(String::from("  push rax"));
    if operand_register(frame, location, right) == Some(Register::RAX) {
        lines.push(String::from("  mov ecx, DWORD PTR [rsp]"));
        load_operand(lines, frame, location, left, Register::RAX);
    } else {
        load_operand(lines, frame, location, left, Register::RAX);
        load_operand(lines, frame, location, right, Register::RCX);
    }
    lines.push(format!("  {mnemonic} eax, cl"));
    lines.push(String::from("  mov ecx, eax"));
    lines.push(String::from("  pop rax"));
}

fn operand_register(
    frame: &FrameLayout<'_>,
    location: CodeLocation,
    operand: &Operand,
) -> Option<Register> {
    frame
        .operand_location(location, operand)
        .and_then(|location| location.register())
}

fn preserve_register(
    lines: &mut Vec<String>,
    frame: &FrameLayout<'_>,
    location: CodeLocation,
    register: Register,
) -> bool {
    let preserve = frame.register_occupied(location, register);
    if preserve {
        lines.push(format!("  push {}", register_name(register)));
    }
    preserve
}

fn restore_register(lines: &mut Vec<String>, register: Register, restore: bool) {
    if restore {
        lines.push(format!("  pop {}", register_name(register)));
    }
}

fn load_operand(
    lines: &mut Vec<String>,
    frame: &FrameLayout<'_>,
    location: CodeLocation,
    operand: &Operand,
    register: Register,
) {
    match operand {
        Operand::Literal(_) => lines.push(format!(
            "  mov {}, {}",
            register_name_32(register),
            literal_value(operand)
        )),
        Operand::Variable(_) | Operand::Temporary(_) => {
            if frame
                .operand_location(location, operand)
                .is_some_and(|source| source == AssemblyLocation::Register(register))
            {
                return;
            }
            lines.push(format!(
                "  mov {}, {}",
                register_name_32(register),
                operand_value(frame, location, operand)
            ));
        }
        Operand::Placeholder => lines.push(format!(
            "  xor {}, {}",
            register_name_32(register),
            register_name_32(register)
        )),
        Operand::Label(_) => lines.push(format!("  lea {}, [{operand}]", register_name(register))),
    }
}

fn load_function_operand(
    lines: &mut Vec<String>,
    frame: &FrameLayout<'_>,
    location: CodeLocation,
    operand: &Operand,
    register: Register,
) {
    match operand {
        Operand::Label(name) => lines.push(format!("  lea {}, [{name}]", register_name(register))),
        Operand::Variable(_) | Operand::Temporary(_) => {
            load_reference_operand(lines, frame, location, operand, register)
        }
        Operand::Placeholder => lines.push(format!("  xor {0}, {0}", register_name(register))),
        Operand::Literal(_) => {
            unreachable!("function values must be stored or declared functions")
        }
    }
}

fn load_float_operand(
    lines: &mut Vec<String>,
    frame: &FrameLayout<'_>,
    location: CodeLocation,
    operand: &Operand,
    register: Register,
) {
    match operand {
        Operand::Literal(Lit::Float(value)) => {
            lines.push(format!("  mov eax, {}", float_literal_bits(*value)));
            lines.push(format!("  movd {}, eax", register_name(register)));
        }
        Operand::Variable(_) | Operand::Temporary(_) => {
            let Some(source) = frame.operand_location(location, operand) else {
                return;
            };
            if source == AssemblyLocation::Register(register) {
                return;
            }
            lines.push(format!(
                "  movss {}, {}",
                register_name(register),
                float_assembly_operand(source)
            ));
        }
        Operand::Placeholder => {
            lines.push(format!("  xorps {0}, {0}", register_name(register)));
        }
        Operand::Literal(_) | Operand::Label(_) => {
            unreachable!("f32 values must be literals, variables, or temporaries")
        }
    }
}

fn stage_float_parameter(
    lines: &mut Vec<String>,
    frame: &FrameLayout<'_>,
    location: CodeLocation,
    operand: &Operand,
) {
    lines.push(String::from("  sub rsp, 8"));
    match operand {
        Operand::Literal(Lit::Float(value)) => lines.push(format!(
            "  mov DWORD PTR [rsp], {}",
            float_literal_bits(*value)
        )),
        Operand::Variable(_) | Operand::Temporary(_) => {
            let Some(source) = frame.operand_location(location, operand) else {
                return;
            };
            if let AssemblyLocation::Register(register) = source {
                lines.push(format!(
                    "  movss DWORD PTR [rsp], {}",
                    register_name(register)
                ));
            } else {
                lines.push(format!("  movss xmm15, {}", float_assembly_operand(source)));
                lines.push(String::from("  movss DWORD PTR [rsp], xmm15"));
            }
        }
        Operand::Placeholder => lines.push(String::from("  mov DWORD PTR [rsp], 0")),
        Operand::Literal(_) | Operand::Label(_) => {
            unreachable!("f32 values must be literals, variables, or temporaries")
        }
    }
}

fn store_operand(
    lines: &mut Vec<String>,
    frame: &FrameLayout<'_>,
    location: CodeLocation,
    operand: &Operand,
    register: Register,
) {
    let Some(destination) = frame.operand_location(location, operand) else {
        return;
    };
    move_location(
        lines,
        AssemblyLocation::Register(register),
        destination,
        Register::RAX,
        MoveWidth::Bits32,
    );
}

fn store_float_operand(
    lines: &mut Vec<String>,
    frame: &FrameLayout<'_>,
    location: CodeLocation,
    operand: &Operand,
    register: Register,
) {
    let Some(destination) = frame.operand_location(location, operand) else {
        return;
    };
    if destination == AssemblyLocation::Register(register) {
        return;
    }
    lines.push(format!(
        "  movss {}, {}",
        float_assembly_operand(destination),
        register_name(register)
    ));
}

fn load_reference_operand(
    lines: &mut Vec<String>,
    frame: &FrameLayout<'_>,
    location: CodeLocation,
    operand: &Operand,
    register: Register,
) {
    let Some(source) = frame.operand_location(location, operand) else {
        return;
    };
    if source == AssemblyLocation::Register(register) {
        return;
    }
    lines.push(format!(
        "  mov {}, {}",
        register_name(register),
        assembly_operand_64(source)
    ));
}

fn store_reference_operand(
    lines: &mut Vec<String>,
    frame: &FrameLayout<'_>,
    location: CodeLocation,
    operand: &Operand,
    register: Register,
) {
    let Some(destination) = frame.operand_location(location, operand) else {
        return;
    };
    if destination == AssemblyLocation::Register(register) {
        return;
    }
    lines.push(format!(
        "  mov {}, {}",
        assembly_operand_64(destination),
        register_name(register)
    ));
}

fn operand_value(frame: &FrameLayout<'_>, location: CodeLocation, operand: &Operand) -> String {
    match operand {
        Operand::Literal(_) => literal_value(operand),
        Operand::Variable(_) | Operand::Temporary(_) => frame
            .operand_location(location, operand)
            .map(assembly_operand)
            .unwrap_or_else(|| String::from("0")),
        Operand::Label(label) => label.clone(),
        Operand::Placeholder => String::from("0"),
    }
}

fn aggregate_member_operand(
    frame: &FrameLayout<'_>,
    location: CodeLocation,
    operand: &Operand,
    member_offset: usize,
) -> Option<String> {
    let AssemblyLocation::FrameStack { offset } = frame.operand_location(location, operand)? else {
        return None;
    };
    let displacement = offset.checked_sub(member_offset)?;
    Some(if displacement == 0 {
        String::from("[rbp]")
    } else {
        format!("[rbp-{displacement}]")
    })
}

fn offset_assembly_operand(operand: &str, offset: usize) -> String {
    if offset == 0 {
        return operand.to_string();
    }
    let inner = operand.trim_start_matches('[').trim_end_matches(']');
    format!("[{inner}+{offset}]")
}

fn emit_memory_copy(lines: &mut Vec<String>, source: &str, destination: &str, size: usize) {
    for offset in (0..size).step_by(4) {
        let width = (size - offset).min(4);
        let source = offset_assembly_operand(source, offset);
        let destination = offset_assembly_operand(destination, offset);
        if width == 4 {
            lines.push(format!("  mov eax, DWORD PTR {source}"));
            lines.push(format!("  mov DWORD PTR {destination}, eax"));
        } else {
            for byte in 0..width {
                let source = offset_assembly_operand(&source, byte);
                let destination = offset_assembly_operand(&destination, byte);
                lines.push(format!("  mov al, BYTE PTR {source}"));
                lines.push(format!("  mov BYTE PTR {destination}, al"));
            }
        }
    }
}

/// Materialize initialized aggregate members while leaving ABI padding zeroed.
fn load_aggregate_register(
    lines: &mut Vec<String>,
    source: &str,
    register: Register,
    member_ranges: &[(usize, usize)],
) {
    lines.push(String::from("  sub rsp, 8"));
    lines.push(String::from("  mov QWORD PTR [rsp], 0"));
    for (offset, size) in member_ranges {
        emit_memory_copy(
            lines,
            &offset_assembly_operand(source, *offset),
            &offset_assembly_operand("[rsp]", *offset),
            *size,
        );
    }
    lines.push(format!(
        "  mov {}, QWORD PTR [rsp]",
        register_name(register)
    ));
    lines.push(String::from("  add rsp, 8"));
}

/// Store only the aggregate's initialized bytes so adjacent frame values remain intact.
fn store_aggregate_register(
    lines: &mut Vec<String>,
    register: Register,
    destination: &str,
    size: usize,
) {
    lines.push(format!("  push {}", register_name(register)));
    emit_memory_copy(lines, "[rsp]", destination, size);
    lines.push(String::from("  add rsp, 8"));
}

fn float_operand_value(
    lines: &mut Vec<String>,
    frame: &FrameLayout<'_>,
    location: CodeLocation,
    operand: &Operand,
    literal_register: Register,
) -> String {
    match operand {
        Operand::Literal(Lit::Float(_)) => {
            load_float_operand(lines, frame, location, operand, literal_register);
            register_name(literal_register)
        }
        Operand::Variable(_) | Operand::Temporary(_) => frame
            .operand_location(location, operand)
            .map(float_assembly_operand)
            .unwrap_or_else(|| String::from("xmm0")),
        Operand::Placeholder => String::from("xmm0"),
        Operand::Literal(_) | Operand::Label(_) => {
            unreachable!("f32 values must be literals, variables, or temporaries")
        }
    }
}

fn emit_call_stack_padding(lines: &mut Vec<String>, stack_arg_count: usize, stack_padding: usize) {
    if stack_padding == 0 {
        return;
    }

    lines.push(format!("  sub rsp, {stack_padding}"));
    for index in 0..stack_arg_count {
        let source = stack_padding + index * STACK_ARG_SLOT_BYTES;
        let destination = index * STACK_ARG_SLOT_BYTES;
        lines.push(format!("  mov rax, QWORD PTR {}", rsp_slot(source)));
        lines.push(format!("  mov QWORD PTR {}, rax", rsp_slot(destination)));
    }
}

fn non_stack_argument_follows_stack_argument(arg_locations: &[Location]) -> bool {
    let mut saw_stack_argument = false;
    arg_locations.iter().any(|location| {
        if stack_argument_offset(location).is_some() {
            saw_stack_argument = true;
            return false;
        }
        saw_stack_argument
    })
}

fn emit_indexed_call_arguments(
    lines: &mut Vec<String>,
    arg_locations: &[Location],
    argument_slot_counts: &[usize],
    stack_arg_count: usize,
    stack_padding: usize,
) {
    for (index, location) in arg_locations.iter().enumerate() {
        let source_offset =
            argument_slot_counts[..index].iter().sum::<usize>() * STACK_ARG_SLOT_BYTES;
        if let Some((low, high)) = register_pair(location) {
            let low_source = rsp_slot(source_offset);
            let high_source = rsp_slot(source_offset + STACK_ARG_SLOT_BYTES);
            lines.push(format!(
                "  mov {}, QWORD PTR {low_source}",
                register_name(low)
            ));
            lines.push(format!(
                "  mov {}, QWORD PTR {high_source}",
                register_name(high)
            ));
            continue;
        }
        let Some(register) = outgoing_register(location) else {
            continue;
        };
        let source = rsp_slot(source_offset);
        if is_xmm_register(register) {
            lines.push(format!(
                "  movss {}, DWORD PTR {source}",
                register_name(register)
            ));
        } else {
            lines.push(format!(
                "  mov {}, QWORD PTR {source}",
                register_name(register)
            ));
        }
    }

    let outgoing_bytes = stack_arg_count * STACK_ARG_SLOT_BYTES + stack_padding;
    if outgoing_bytes == 0 {
        return;
    }
    lines.push(format!("  sub rsp, {outgoing_bytes}"));
    for (index, location) in arg_locations.iter().enumerate() {
        let Some(offset) = stack_location_offset(location) else {
            continue;
        };
        let source_offset =
            argument_slot_counts[..index].iter().sum::<usize>() * STACK_ARG_SLOT_BYTES;
        for slot in 0..argument_slot_counts[index] {
            let source = rsp_slot(outgoing_bytes + source_offset + slot * STACK_ARG_SLOT_BYTES);
            let destination = rsp_slot((offset + slot) * STACK_ARG_SLOT_BYTES);
            lines.push(format!("  mov rax, QWORD PTR {source}"));
            lines.push(format!("  mov QWORD PTR {destination}, rax"));
        }
    }
}

fn staged_argument_slot_count(location: &Location) -> usize {
    if register_pair(location).is_some() {
        2
    } else {
        1
    }
}

fn rsp_slot(offset: usize) -> String {
    if offset == 0 {
        String::from("[rsp]")
    } else {
        format!("[rsp+{offset}]")
    }
}

fn emit_conditional_compare(
    lines: &mut Vec<String>,
    frame: &FrameLayout<'_>,
    location: CodeLocation,
    left: &Operand,
    operator: &'static str,
    right: &Operand,
) -> &'static str {
    if is_immediate_compare_operand(left) && is_addressable_compare_operand(right) {
        lines.push(format!(
            "  cmp {}, {}",
            operand_value(frame, location, right),
            operand_value(frame, location, left)
        ));
        return swapped_comparison_operator(operator);
    }

    load_operand(lines, frame, location, left, Register::RAX);
    lines.push(format!(
        "  cmp eax, {}",
        operand_value(frame, location, right)
    ));
    operator
}

fn emit_float_compare(
    lines: &mut Vec<String>,
    frame: &FrameLayout<'_>,
    location: CodeLocation,
    left: &Operand,
    right: &Operand,
    operator: &str,
    result_register: Register,
) {
    load_float_operand(lines, frame, location, left, Register::XMM0);
    let right = float_operand_value(lines, frame, location, right, Register::XMM1);
    lines.push(format!("  ucomiss xmm0, {right}"));

    let result = register_name_8(result_register);
    let guard_register = if result_register == Register::RCX {
        Register::RAX
    } else {
        Register::RCX
    };
    let guard = register_name_8(guard_register);
    let preserve_guard = frame.register_occupied(location, guard_register);
    if preserve_guard {
        lines.push(format!("  push {}", register_name(guard_register)));
    }
    match operator {
        operators::EQUALS => {
            lines.push(format!("  sete {result}"));
            lines.push(format!("  setnp {guard}"));
            lines.push(format!("  and {result}, {guard}"));
        }
        operators::NOT_EQUALS => {
            lines.push(format!("  setne {result}"));
            lines.push(format!("  setp {guard}"));
            lines.push(format!("  or {result}, {guard}"));
        }
        operators::LESS => {
            lines.push(format!("  setb {result}"));
            lines.push(format!("  setnp {guard}"));
            lines.push(format!("  and {result}, {guard}"));
        }
        operators::LESS_EQUALS => {
            lines.push(format!("  setbe {result}"));
            lines.push(format!("  setnp {guard}"));
            lines.push(format!("  and {result}, {guard}"));
        }
        operators::GREATER => lines.push(format!("  seta {result}")),
        operators::GREATER_EQUALS => lines.push(format!("  setae {result}")),
        _ => unreachable!(),
    }
    lines.push(format!(
        "  movzx {}, {result}",
        register_name_32(result_register)
    ));
    if preserve_guard {
        lines.push(format!("  pop {}", register_name(guard_register)));
    }
}

fn is_immediate_compare_operand(operand: &Operand) -> bool {
    matches!(operand, Operand::Literal(_) | Operand::Placeholder)
}

fn is_addressable_compare_operand(operand: &Operand) -> bool {
    matches!(operand, Operand::Variable(_) | Operand::Temporary(_))
}

fn swapped_comparison_operator(operator: &'static str) -> &'static str {
    match operator {
        operators::LESS => operators::GREATER,
        operators::LESS_EQUALS => operators::GREATER_EQUALS,
        operators::GREATER => operators::LESS,
        operators::GREATER_EQUALS => operators::LESS_EQUALS,
        operators::EQUALS | operators::NOT_EQUALS => operator,
        _ => operator,
    }
}

#[derive(Clone, Copy)]
enum MoveWidth {
    Bits32,
    Bits64,
}

impl MoveWidth {
    fn register_name(self, register: Register) -> String {
        match self {
            MoveWidth::Bits32 => register_name_32(register),
            MoveWidth::Bits64 => register_name(register),
        }
    }

    fn assembly_operand(self, location: AssemblyLocation) -> String {
        match self {
            MoveWidth::Bits32 => assembly_operand(location),
            MoveWidth::Bits64 => assembly_operand_64(location),
        }
    }
}

fn move_location(
    lines: &mut Vec<String>,
    source: AssemblyLocation,
    destination: AssemblyLocation,
    scratch: Register,
    width: MoveWidth,
) {
    if source == destination {
        return;
    }
    match (source, destination) {
        (AssemblyLocation::Register(src), AssemblyLocation::Register(dst)) => {
            lines.push(format!(
                "  mov {}, {}",
                width.register_name(dst),
                width.register_name(src)
            ));
        }
        (AssemblyLocation::Register(src), dst) => {
            lines.push(format!(
                "  mov {}, {}",
                width.assembly_operand(dst),
                width.register_name(src)
            ));
        }
        (src, AssemblyLocation::Register(dst)) => {
            lines.push(format!(
                "  mov {}, {}",
                width.register_name(dst),
                width.assembly_operand(src)
            ));
        }
        (src, dst) => {
            lines.push(format!(
                "  mov {}, {}",
                width.register_name(scratch),
                width.assembly_operand(src)
            ));
            lines.push(format!(
                "  mov {}, {}",
                width.assembly_operand(dst),
                width.register_name(scratch)
            ));
        }
    }
}

fn move_float_location(
    lines: &mut Vec<String>,
    source: AssemblyLocation,
    destination: AssemblyLocation,
    scratch: Register,
) {
    if source == destination {
        return;
    }
    match (source, destination) {
        (AssemblyLocation::Register(src), AssemblyLocation::Register(dst)) => {
            lines.push(format!(
                "  movss {}, {}",
                register_name(dst),
                register_name(src)
            ));
        }
        (AssemblyLocation::Register(src), dst) => {
            lines.push(format!(
                "  movss {}, {}",
                float_assembly_operand(dst),
                register_name(src)
            ));
        }
        (src, AssemblyLocation::Register(dst)) => {
            lines.push(format!(
                "  movss {}, {}",
                register_name(dst),
                float_assembly_operand(src)
            ));
        }
        (src, dst) => {
            lines.push(format!(
                "  movss {}, {}",
                register_name(scratch),
                float_assembly_operand(src)
            ));
            lines.push(format!(
                "  movss {}, {}",
                float_assembly_operand(dst),
                register_name(scratch)
            ));
        }
    }
}

fn assembly_operand(location: AssemblyLocation) -> String {
    match location {
        AssemblyLocation::Register(register) => register_name_32(register),
        AssemblyLocation::FrameStack { offset } => format!("DWORD PTR [rbp-{offset}]"),
        AssemblyLocation::IncomingStack { offset } => format!("DWORD PTR [rbp+{offset}]"),
    }
}

fn float_assembly_operand(location: AssemblyLocation) -> String {
    match location {
        AssemblyLocation::Register(register) => register_name(register),
        AssemblyLocation::FrameStack { offset } => format!("DWORD PTR [rbp-{offset}]"),
        AssemblyLocation::IncomingStack { offset } => format!("DWORD PTR [rbp+{offset}]"),
    }
}

fn assembly_operand_64(location: AssemblyLocation) -> String {
    match location {
        AssemblyLocation::Register(register) => register_name(register),
        AssemblyLocation::FrameStack { offset } => format!("QWORD PTR [rbp-{offset}]"),
        AssemblyLocation::IncomingStack { offset } => format!("QWORD PTR [rbp+{offset}]"),
    }
}

fn incoming_location(location: &Location) -> Option<AssemblyLocation> {
    match location {
        Location::Register(register) => Some(AssemblyLocation::Register(*register)),
        Location::Stack(offset) => Some(AssemblyLocation::IncomingStack {
            offset: 16 + offset.0 * 8,
        }),
        Location::RegisterAndStack(register, _) => Some(AssemblyLocation::Register(*register)),
        Location::NoStorage | Location::Indirect { .. } | Location::Pair { .. } => None,
    }
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

fn stack_location_offset(location: &Location) -> Option<usize> {
    match location {
        Location::Stack(offset) => Some(offset.0),
        Location::RegisterAndStack(_, offset) => Some(offset.0),
        Location::Pair { low, .. } => stack_location_offset(low),
        Location::NoStorage | Location::Register(_) | Location::Indirect { .. } => None,
    }
}

fn stack_argument_offset(location: &Location) -> Option<usize> {
    match location {
        Location::Stack(_) | Location::Pair { .. } => stack_location_offset(location),
        Location::NoStorage
        | Location::Register(_)
        | Location::RegisterAndStack(_, _)
        | Location::Indirect { .. } => None,
    }
}

fn operand_name(operand: &Operand) -> Option<String> {
    match operand {
        Operand::Variable(name) => Some(name.clone()),
        Operand::Temporary(label) => Some(label.to_string()),
        Operand::Literal(_) | Operand::Label(_) | Operand::Placeholder => None,
    }
}

fn register_name(register: Register) -> String {
    if is_xmm_register(register) {
        return format!("{register:?}").to_ascii_lowercase();
    }
    format!("{:?}", register.full_register()).to_ascii_lowercase()
}

fn register_name_32(register: Register) -> String {
    format!("{:?}", register.full_register32()).to_ascii_lowercase()
}

fn register_name_8(register: Register) -> &'static str {
    match register.full_register() {
        Register::RAX => "al",
        Register::RBX => "bl",
        Register::RCX => "cl",
        Register::RDX => "dl",
        Register::RSI => "sil",
        Register::RDI => "dil",
        Register::RBP => "bpl",
        Register::RSP => "spl",
        Register::R8 => "r8b",
        Register::R9 => "r9b",
        Register::R10 => "r10b",
        Register::R11 => "r11b",
        Register::R12 => "r12b",
        Register::R13 => "r13b",
        Register::R14 => "r14b",
        Register::R15 => "r15b",
        _ => unreachable!("x86 text emitter only allocates general-purpose registers"),
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

fn literal_value(operand: &Operand) -> String {
    match operand {
        Operand::Literal(Lit::Integer(value)) => value.to_string(),
        Operand::Literal(Lit::Boolean(value)) => {
            if *value {
                String::from("1")
            } else {
                String::from("0")
            }
        }
        Operand::Literal(_) => String::from("0"),
        _ => unreachable!(),
    }
}

fn float_literal_bits(value: f64) -> String {
    format!("0x{:08X}", (value as f32).to_bits())
}

fn jump_for(operator: &str) -> &'static str {
    match operator {
        operators::EQUALS => "je",
        operators::NOT_EQUALS => "jne",
        operators::LESS => "jl",
        operators::LESS_EQUALS => "jle",
        operators::GREATER => "jg",
        operators::GREATER_EQUALS => "jge",
        _ => "jne",
    }
}

fn align_to(value: usize, align: usize) -> usize {
    if value == 0 {
        0
    } else {
        value.div_ceil(align) * align
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::LexionDiagnosticList;
    use crate::generators::tac::instructions::{CodeSpan, LivenessInterval};
    use crate::generators::x86::LinearRegisterAllocator;
    use std::collections::HashMap;

    fn allocated_frame(variable: &str) -> (FrameLayout<'static>, CodeLocation) {
        let mut cfg = Box::new(ControlFlowGraph::new());
        let block = cfg.block(String::from("test"), true);
        let range = cfg.functions[0];
        let location = CodeLocation::new(block, 0);
        let intervals = HashMap::from([(
            range,
            vec![LivenessInterval {
                variable: String::from(variable),
                span: CodeSpan::new(location, CodeLocation::new(block, 1)),
                uses: vec![],
            }],
        )]);
        let mut diagnostics = LexionDiagnosticList::default();
        let mut allocations = LinearRegisterAllocator::new((&cfg, vec![Register::RAX]))
            .exec(&mut diagnostics, intervals)
            .unwrap();
        let allocations = allocations.remove(&range).unwrap();

        (
            FrameLayout {
                allocations: Some(Box::leak(allocations.into_boxed_slice())),
                fallback_slots: BTreeMap::new(),
                home_slots: BTreeMap::from_iter(
                    [(String::from("value"), 16)]
                        .into_iter()
                        .chain((variable != "reference").then(|| (String::from("reference"), 8))),
                ),
                saved_registers: Vec::new(),
                stack_size: 0,
                indirect_return_slot: None,
            },
            location,
        )
    }

    #[test]
    fn aggregate_borrow_preserves_live_rax_unless_target_owns_it() {
        let mut lines = Vec::new();
        let borrow = BorrowInstruction {
            target: Operand::Variable(String::from("reference")),
            place: Place::Direct(Operand::Variable(String::from("value"))),
        };
        let generator = CodeGeneratorX86 {
            cfg: Box::leak(Box::new(ControlFlowGraph::new())),
            types: Box::leak(Box::new(TypeCollection::default())),
            symbols: Box::leak(Box::new(SymbolTableGraph::default())),
            target: X86Target::system_v64(),
            allocations: None,
        };
        let (live_frame, location) = allocated_frame("live");

        generator.emit_borrow(&mut lines, &live_frame, "test", location, &borrow);
        assert_eq!(
            lines,
            [
                "  push rax",
                "  lea rax, [rbp-16]",
                "  mov QWORD PTR [rbp-8], rax",
                "  pop rax"
            ]
        );

        lines.clear();
        let (target_frame, location) = allocated_frame("reference");
        generator.emit_borrow(&mut lines, &target_frame, "test", location, &borrow);
        assert_eq!(lines, ["  lea rax, [rbp-16]"]);
    }
}
