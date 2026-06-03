use crate::ast::types::{PrimitiveType, Type, TypeCollection};
use crate::ast::Lit;
use crate::diagnostic::{DiagnosticConsumer, LexionDiagnosticError};
use crate::generators::tac::instructions::{
    AssignmentInstruction, CodeLocation, ConditionalJumpInstruction, ControlFlowGraph,
    FunctionCallInstruction, FunctionRange, Instruction, InstructionInstance, Operand,
};
use crate::generators::x86::calling_convention::{CallingConvention, Location};
use crate::generators::x86::{AbiLocationRole, AssignedLivenessInterval, StackOffset, X86Target};
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
            lines.extend(self.emit_function(*range, options));
        }
        X86Assembly::new(lines.join("\n"))
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
        self.emit_function_parameter_moves(&mut lines, range, &frame);

        let mut emitted_return = false;
        let mut pending_param_count = 0;
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
                if self.emit_instruction(
                    &mut lines,
                    &frame,
                    location,
                    &mut pending_param_count,
                    &inst.instruction,
                ) {
                    emitted_return = true;
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

    fn symbol_span(&self, name: &str) -> Option<SourceSpan> {
        self.symbol_entry(name).map(|entry| entry.span)
    }

    fn symbol_entry(&self, name: &str) -> Option<&SymbolTableEntry> {
        self.symbols
            .graph
            .node_weights()
            .flat_map(|table| table.entries.iter())
            .find(|entry| entry.name == name)
    }

    fn emit_instruction(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        location: CodeLocation,
        pending_param_count: &mut usize,
        instruction: &Instruction,
    ) -> bool {
        match instruction {
            Instruction::Assignment(inst) => {
                self.emit_assignment(lines, frame, location, inst);
                false
            }
            Instruction::Copy(inst) => {
                let register = frame
                    .operand_location(location, &inst.dst)
                    .and_then(|location| location.register())
                    .unwrap_or(Register::RAX);
                load_operand(lines, frame, location, &inst.src, register);
                store_operand(lines, frame, location, &inst.dst, register);
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
                    load_operand(lines, frame, location, value, Register::RAX);
                }
                emit_epilogue(lines, frame);
                true
            }
            Instruction::Function(_) | Instruction::EndFunction(_) | Instruction::Extern(_) => {
                false
            }
            Instruction::Parameter(inst) => {
                self.emit_parameter(lines, frame, location, &inst.param);
                *pending_param_count += 1;
                false
            }
            Instruction::FunctionCall(inst) => {
                self.emit_function_call(lines, frame, location, *pending_param_count, inst);
                *pending_param_count = 0;
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
            for node in self.cfg.function_nodes(range) {
                for inst in &self.cfg[node].instructions {
                    if let Some(message) = self.unsupported_message(&inst.instruction) {
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

    fn unsupported_message(&self, instruction: &Instruction) -> Option<String> {
        match instruction {
            Instruction::Assignment(inst) => self
                .unsupported_assignment_operator_message(inst)
                .or_else(|| self.unsupported_operand_message(&inst.target))
                .or_else(|| {
                    inst.left
                        .as_ref()
                        .and_then(|left| self.unsupported_operand_message(left))
                })
                .or_else(|| self.unsupported_operand_message(&inst.right)),
            Instruction::FunctionCall(inst) => inst
                .return_target
                .as_ref()
                .and_then(|target| self.unsupported_operand_message(target)),
            Instruction::Extern(inst) => Some(format!(
                "x86 backend does not support extern declarations yet: {}",
                inst.label
            )),
            Instruction::Copy(inst) => self
                .unsupported_operand_message(&inst.dst)
                .or_else(|| self.unsupported_operand_message(&inst.src)),
            Instruction::ConditionalJump(inst) => inst
                .left
                .as_ref()
                .and_then(|left| self.unsupported_operand_message(left))
                .or_else(|| self.unsupported_operand_message(&inst.right)),
            Instruction::Parameter(inst) => self.unsupported_operand_message(&inst.param),
            Instruction::Return(inst) => inst
                .value
                .as_ref()
                .and_then(|value| self.unsupported_operand_message(value)),
            Instruction::Function(inst) => self.unsupported_function_signature_message(&inst.label),
            Instruction::Jump(_) | Instruction::EndFunction(_) => None,
        }
    }

    fn unsupported_assignment_operator_message(
        &self,
        inst: &AssignmentInstruction,
    ) -> Option<String> {
        if inst.left.is_none() && inst.operator == operators::ADDRESS_OF {
            return Some(String::from(
                "x86 backend does not support address-taking yet",
            ));
        }
        if inst.left.is_none() && inst.operator == operators::DEREFERENCE {
            return Some(String::from(
                "x86 backend does not support pointer dereference yet",
            ));
        }
        (!assignment_supported(inst)).then(|| {
            format!(
                "x86 backend does not support `{}` assignments yet",
                inst.operator
            )
        })
    }

    fn unsupported_operand_message(&self, operand: &Operand) -> Option<String> {
        match operand {
            Operand::Literal(Lit::Float(_)) => Some(String::from(
                "x86 backend does not support floating-point values yet: f32",
            )),
            Operand::Literal(Lit::String(_)) => Some(String::from(
                "x86 backend does not support string values yet: &str",
            )),
            Operand::Variable(name) => self
                .symbol_entry(name)
                .and_then(|entry| entry.var_type)
                .and_then(|ty| self.unsupported_type_message(ty)),
            Operand::Temporary(label) => {
                let name = label.to_string();
                self.symbol_entry(&name)
                    .and_then(|entry| entry.var_type)
                    .and_then(|ty| self.unsupported_type_message(ty))
            }
            Operand::Literal(_) | Operand::Label(_) | Operand::Placeholder => None,
        }
    }

    fn unsupported_function_signature_message(&self, function: &str) -> Option<String> {
        let signature = self.function_signature(function)?;
        signature
            .params
            .iter()
            .find_map(|ty| self.unsupported_type_message(*ty))
            .or_else(|| self.unsupported_type_message(signature.return_type))
    }

    fn unsupported_type_message(&self, ty: Index) -> Option<String> {
        let ty = self.types.canonicalize(ty);
        let name = self.types.to_string_index(ty);
        match self.types.get(ty)? {
            Type::PrimitiveType(PrimitiveType::F32) => Some(format!(
                "x86 backend does not support floating-point values yet: {name}"
            )),
            Type::PrimitiveType(PrimitiveType::STR) => Some(format!(
                "x86 backend does not support string values yet: {name}"
            )),
            Type::TupleType(tuple) if !tuple.types.is_empty() => Some(format!(
                "x86 backend does not support tuple aggregate values yet: {name}"
            )),
            Type::StructType(_) => Some(format!(
                "x86 backend does not support struct aggregate values yet: {name}"
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
            Type::RefType(_) => Some(format!(
                "x86 backend does not support reference or pointer values yet: {name}"
            )),
            Type::FunctionType(_) => Some(format!(
                "x86 backend does not support function pointer values yet: {name}"
            )),
            Type::TupleType(_) | Type::TypeDefType(_) | Type::PrimitiveType(_) | Type::Unknown => {
                None
            }
        }
    }

    fn function_signature(&self, function: &str) -> Option<&crate::ast::types::FunctionType> {
        self.symbol_entry(function)
            .and_then(|entry| entry.var_type)
            .and_then(|ty| self.types.get(self.types.canonicalize(ty)))
            .and_then(|ty| match ty {
                Type::FunctionType(signature) => Some(signature),
                _ => None,
            })
    }

    fn emit_assignment(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        location: CodeLocation,
        inst: &AssignmentInstruction,
    ) {
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
            (None, _) => {
                load_operand(lines, frame, location, &inst.right, target_register);
            }
            (Some(left), operators::PLUS) => {
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
            (Some(left), operators::EQUALS | operators::NOT_EQUALS) => {
                self.emit_compare(lines, frame, location, left, &inst.right, inst.operator);
            }
            (Some(left), operators::LESS | operators::LESS_EQUALS) => {
                self.emit_compare(lines, frame, location, left, &inst.right, inst.operator);
            }
            (Some(left), operators::GREATER | operators::GREATER_EQUALS) => {
                self.emit_compare(lines, frame, location, left, &inst.right, inst.operator);
            }
            (Some(left), operators::LOGICAL_AND) => {
                load_operand(lines, frame, location, left, target_register);
                lines.push(format!(
                    "  and {}, {}",
                    register_name_32(target_register),
                    operand_value(frame, location, &inst.right)
                ));
            }
            (Some(left), operators::LOGICAL_OR) => {
                load_operand(lines, frame, location, left, target_register);
                lines.push(format!(
                    "  or {}, {}",
                    register_name_32(target_register),
                    operand_value(frame, location, &inst.right)
                ));
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
            | (Some(_), operators::GREATER | operators::GREATER_EQUALS) => Register::RAX,
            _ => target_register,
        };
        store_operand(lines, frame, location, &inst.target, result_register);
    }

    fn emit_compare(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        location: CodeLocation,
        left: &Operand,
        right: &Operand,
        operator: &str,
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
        load_operand(lines, frame, location, left, Register::RAX);
        lines.push(format!(
            "  cmp eax, {}",
            operand_value(frame, location, right)
        ));
        lines.push(format!("  {setcc} al"));
        lines.push(String::from("  movzx eax, al"));
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
        location: CodeLocation,
        pending_param_count: usize,
        inst: &FunctionCallInstruction,
    ) {
        let arg_locations = self
            .function_signature(&inst.function)
            .map(|signature| {
                self.target
                    .calling_convention()
                    .assign_args(self.types, 0, signature)
            })
            .unwrap_or_default();
        let register_args = arg_locations
            .iter()
            .take(pending_param_count)
            .filter_map(outgoing_register)
            .collect::<Vec<_>>();
        for register in register_args {
            lines.push(format!("  pop {}", register_name(register)));
        }

        let stack_arg_count = arg_locations
            .iter()
            .take(pending_param_count)
            .filter(|abi_location| matches!(abi_location, Location::Stack(_)))
            .count();
        let stack_padding = frame.call_stack_padding(
            stack_arg_count,
            self.target.calling_convention().stack_alignment(),
        );
        emit_call_stack_padding(lines, stack_arg_count, stack_padding);
        lines.push(format!("  call {}", inst.function));
        let stack_cleanup = stack_arg_count * STACK_ARG_SLOT_BYTES + stack_padding;
        if stack_cleanup > 0 {
            lines.push(format!("  add rsp, {stack_cleanup}"));
        }
        if let Some(return_target) = &inst.return_target {
            store_operand(lines, frame, location, return_target, Register::RAX);
        }
    }

    fn emit_parameter(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        location: CodeLocation,
        operand: &Operand,
    ) {
        load_operand(lines, frame, location, operand, Register::RAX);
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
            let stack_size = align_to(
                spill_count * 4,
                self.target.calling_convention().stack_alignment(),
            );
            FrameLayout {
                allocations: Some(allocations),
                fallback_slots: BTreeMap::new(),
                saved_registers,
                stack_size,
            }
        } else {
            let fallback_slots = self.stack_slots(range);
            let stack_size = align_to(
                fallback_slots.len() * 4,
                self.target.calling_convention().stack_alignment(),
            );
            FrameLayout {
                allocations: None,
                fallback_slots,
                saved_registers: Vec::new(),
                stack_size,
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
                let Some(source) = incoming_location(constraint.abi_location()) else {
                    continue;
                };
                let Some(destination) = frame.frame_location(assigned.location()) else {
                    continue;
                };
                move_location(lines, source, destination, Register::RAX);
            }
        }
    }

    fn stack_slots(&self, range: FunctionRange) -> BTreeMap<String, usize> {
        let mut names = BTreeSet::new();
        for node in self.cfg.function_nodes(&range) {
            for inst in &self.cfg[node].instructions {
                collect_instruction_operands(&inst.instruction, &mut names);
            }
        }
        names
            .into_iter()
            .enumerate()
            .map(|(idx, name)| (name, (idx + 1) * 4))
            .collect()
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
            operators::UNARY_PLUS | operators::UNARY_MINUS | operators::LOGICAL_NOT
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
    saved_registers: Vec<Register>,
    stack_size: usize,
}

impl<'a> FrameLayout<'a> {
    fn operand_location(
        &self,
        location: CodeLocation,
        operand: &Operand,
    ) -> Option<AssemblyLocation> {
        let name = operand_name(operand)?;
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
            Location::Indirect { .. } | Location::Pair { .. } => None,
        }
    }

    fn frame_stack_offset(&self, offset: StackOffset) -> usize {
        self.saved_registers.len() * 8 + (offset.0 + 1) * 4
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
    );
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

fn move_location(
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
                "  mov {}, {}",
                register_name_32(dst),
                register_name_32(src)
            ));
        }
        (AssemblyLocation::Register(src), dst) => {
            lines.push(format!(
                "  mov {}, {}",
                assembly_operand(dst),
                register_name_32(src)
            ));
        }
        (src, AssemblyLocation::Register(dst)) => {
            lines.push(format!(
                "  mov {}, {}",
                register_name_32(dst),
                assembly_operand(src)
            ));
        }
        (src, dst) => {
            lines.push(format!(
                "  mov {}, {}",
                register_name_32(scratch),
                assembly_operand(src)
            ));
            lines.push(format!(
                "  mov {}, {}",
                assembly_operand(dst),
                register_name_32(scratch)
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

fn incoming_location(location: &Location) -> Option<AssemblyLocation> {
    match location {
        Location::Register(register) => Some(AssemblyLocation::Register(*register)),
        Location::Stack(offset) => Some(AssemblyLocation::IncomingStack {
            offset: 16 + offset.0 * 8,
        }),
        Location::RegisterAndStack(register, _) => Some(AssemblyLocation::Register(*register)),
        Location::Indirect { .. } | Location::Pair { .. } => None,
    }
}

fn outgoing_register(location: &Location) -> Option<Register> {
    match location {
        Location::Register(register) | Location::RegisterAndStack(register, _) => Some(*register),
        Location::Stack(_) | Location::Indirect { .. } | Location::Pair { .. } => None,
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
    format!("{:?}", register.full_register()).to_ascii_lowercase()
}

fn register_name_32(register: Register) -> String {
    format!("{:?}", register.full_register32()).to_ascii_lowercase()
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
