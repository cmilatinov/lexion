use crate::ast::types::{FunctionType, PrimitiveType, Type, TypeCollection};
use crate::ast::Lit;
use crate::diagnostic::{DiagnosticConsumer, LexionDiagnosticError};
use crate::generators::tac::instructions::{
    AssignmentInstruction, BaseInstruction, BorrowInstruction, CodeLocation,
    ConditionalJumpInstruction, ControlFlowGraph, FunctionCallInstruction, FunctionRange,
    Instruction, InstructionInstance, LoadInstruction, Operand, Place, StoreInstruction,
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
                    name.as_str(),
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

    fn emit_instruction(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        function: &str,
        location: CodeLocation,
        pending_param_count: &mut usize,
        instruction: &Instruction,
    ) -> bool {
        match instruction {
            Instruction::Borrow(inst) => {
                self.emit_borrow(lines, frame, location, inst);
                false
            }
            Instruction::Load(inst) => {
                self.emit_load(lines, frame, location, inst);
                false
            }
            Instruction::Store(inst) => {
                self.emit_store(lines, frame, location, inst);
                false
            }
            Instruction::Assignment(inst) => {
                self.emit_assignment(lines, frame, function, location, inst);
                false
            }
            Instruction::Copy(inst) => {
                if self.operand_is_reference(function, &inst.src)
                    || self.operand_is_reference(function, &inst.dst)
                {
                    load_reference_operand(lines, frame, location, &inst.src, Register::RAX);
                    store_reference_operand(lines, frame, location, &inst.dst, Register::RAX);
                } else {
                    let register = frame
                        .operand_location(location, &inst.dst)
                        .and_then(|location| location.register())
                        .unwrap_or(Register::RAX);
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
                .unsupported_borrow_message(&inst.place)
                .or_else(|| self.unsupported_operand_message(function, &inst.target)),
            Instruction::Load(inst) => self
                .unsupported_load_message(&inst.place)
                .or_else(|| self.unsupported_place_operand_message(function, &inst.place))
                .or_else(|| self.unsupported_operand_message(function, &inst.target)),
            Instruction::Store(inst) => self
                .unsupported_store_message(&inst.place)
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
            Instruction::FunctionCall(inst) => self
                .unsupported_call_target_message(inst)
                .or_else(|| self.unsupported_call_signature_message(inst))
                .or_else(|| {
                    inst.return_target
                        .as_ref()
                        .and_then(|target| self.unsupported_operand_message(function, target))
                }),
            Instruction::Extern(_) => None,
            Instruction::Copy(inst) => self
                .unsupported_operand_message(function, &inst.dst)
                .or_else(|| self.unsupported_operand_message(function, &inst.src)),
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
            Instruction::Parameter(inst) => self.unsupported_operand_message(function, &inst.param),
            Instruction::Return(inst) => inst
                .value
                .as_ref()
                .and_then(|value| self.unsupported_operand_message(function, value)),
            Instruction::Function(inst) => self.unsupported_function_signature_message(&inst.label),
            Instruction::Jump(_) | Instruction::EndFunction(_) => None,
        }
    }

    fn unsupported_borrow_message(&self, place: &Place) -> Option<String> {
        match place {
            Place::Direct(value) if operand_name(value).is_some() => None,
            Place::Direct(_) => Some(String::from("x86 backend can only borrow stored values")),
            Place::Member { .. } | Place::Index { .. } | Place::Dereference(_) => Some(
                String::from("x86 backend does not support references to projected places yet"),
            ),
        }
    }

    fn emit_borrow(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        location: CodeLocation,
        inst: &BorrowInstruction,
    ) {
        let Place::Direct(value) = &inst.place else {
            unreachable!("unsupported borrow places are diagnosed before emission")
        };
        let Some(AssemblyLocation::FrameStack { offset }) = frame.operand_location(location, value)
        else {
            unreachable!("borrowed values must have stable frame locations")
        };
        lines.push(format!("  lea rax, [rbp-{offset}]"));
        store_reference_operand(lines, frame, location, &inst.target, Register::RAX);
    }

    fn emit_load(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        location: CodeLocation,
        inst: &LoadInstruction,
    ) {
        match &inst.place {
            Place::Direct(value) => load_operand(lines, frame, location, value, Register::RAX),
            Place::Dereference(reference) => {
                load_reference_operand(lines, frame, location, reference, Register::RAX);
                lines.push(String::from("  mov eax, DWORD PTR [rax]"));
            }
            Place::Member { .. } | Place::Index { .. } => {
                unreachable!("unsupported load places are diagnosed before emission")
            }
        }
        store_operand(lines, frame, location, &inst.target, Register::RAX);
    }

    fn emit_store(
        &self,
        lines: &mut Vec<String>,
        frame: &FrameLayout<'_>,
        location: CodeLocation,
        inst: &StoreInstruction,
    ) {
        match &inst.place {
            Place::Direct(target) => {
                load_operand(lines, frame, location, &inst.value, Register::RAX);
                store_operand(lines, frame, location, target, Register::RAX);
            }
            Place::Dereference(reference) => {
                load_operand(lines, frame, location, &inst.value, Register::RCX);
                load_reference_operand(lines, frame, location, reference, Register::RAX);
                lines.push(String::from("  mov DWORD PTR [rax], ecx"));
            }
            Place::Member { .. } | Place::Index { .. } => {
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

    fn unsupported_load_message(&self, place: &Place) -> Option<String> {
        match place {
            Place::Member { .. } => Some(String::from(
                "x86 backend does not support member access yet",
            )),
            Place::Index { .. } => Some(String::from(
                "x86 backend does not support indexed access yet",
            )),
            Place::Dereference(_) | Place::Direct(_) => None,
        }
    }

    fn unsupported_store_message(&self, place: &Place) -> Option<String> {
        match place {
            Place::Member { .. } => Some(String::from(
                "x86 backend does not support stores through member access yet",
            )),
            Place::Index { .. } => Some(String::from(
                "x86 backend does not support stores through indexed access yet",
            )),
            Place::Dereference(_) | Place::Direct(_) => None,
        }
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

    fn unsupported_reference_abi_message(&self, ty: Index) -> Option<String> {
        let ty = self.types.canonicalize(ty);
        if matches!(self.types.get(ty), Some(Type::RefType(_)))
            && self.unsupported_type_message(ty).is_none()
        {
            return Some(format!(
                "x86 backend does not support reference parameters or returns yet: {}",
                self.types.to_string_index(ty)
            ));
        }
        None
    }

    fn unsupported_assignment_operator_message(
        &self,
        function: &str,
        inst: &AssignmentInstruction,
    ) -> Option<String> {
        if inst.left.is_none() && inst.operator == operators::TYPE_CAST {
            return self.unsupported_cast_message(function, inst);
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
            Operand::Literal(Lit::Float(_)) => Some(String::from(
                "x86 backend does not support floating-point values yet: f32",
            )),
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
        signature
            .params
            .iter()
            .find_map(|ty| {
                self.unsupported_type_message(*ty)
                    .or_else(|| self.unsupported_reference_abi_message(*ty))
            })
            .or_else(|| {
                self.unsupported_type_message(signature.return_type)
                    .or_else(|| self.unsupported_reference_abi_message(signature.return_type))
            })
    }

    fn unsupported_call_signature_message(&self, inst: &FunctionCallInstruction) -> Option<String> {
        let signature = self.function_call_signature(inst)?;
        if signature.is_vararg {
            return Some(format!(
                "x86 backend does not support calls to vararg functions yet: {}",
                inst.function
            ));
        }
        signature
            .params
            .iter()
            .find_map(|ty| self.unsupported_reference_abi_message(*ty))
            .or_else(|| self.unsupported_reference_abi_message(signature.return_type))
    }

    fn unsupported_call_target_message(&self, inst: &FunctionCallInstruction) -> Option<String> {
        (!inst.is_direct_function).then(|| {
            format!(
                "x86 backend does not support indirect calls through function values yet: {}",
                inst.function
            )
        })
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
                "x86 backend does not support tuple values yet: {name}"
            )),
            Type::StructType(_) => Some(format!(
                "x86 backend does not support struct values yet: {name}"
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
            Type::FunctionType(_) => Some(format!(
                "x86 backend does not support function values yet: {name}"
            )),
            Type::TupleType(_) | Type::TypeDefType(_) | Type::PrimitiveType(_) | Type::Unknown => {
                None
            }
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
        location: CodeLocation,
        pending_param_count: usize,
        inst: &FunctionCallInstruction,
    ) {
        let arg_locations = self
            .function_call_signature(inst)
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
            let spill_bytes = spill_count * 4;
            let mut home_bytes = align_to(spill_bytes, 8);
            let home_slots = self
                .home_slot_names(range)
                .into_iter()
                .map(|name| {
                    home_bytes += 8;
                    (
                        name,
                        saved_registers.len() * STACK_ARG_SLOT_BYTES + home_bytes,
                    )
                })
                .collect();
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
            }
        } else {
            let fallback_slots = self.stack_slots(range);
            let stack_size = align_to(
                fallback_slots.values().copied().max().unwrap_or(0),
                self.target.calling_convention().stack_alignment(),
            );
            FrameLayout {
                allocations: None,
                fallback_slots,
                home_slots: BTreeMap::new(),
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
                let size = if self.symbol_is_reference(function, &name) {
                    8
                } else {
                    4
                };
                offset = align_to(offset, size);
                offset += size;
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
                    if self.symbol_is_reference(function, &name) {
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

fn is_bool_type(types: &TypeCollection, ty: Index) -> bool {
    matches!(
        types.get(types.canonicalize(ty)),
        Some(Type::PrimitiveType(PrimitiveType::BOOL))
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
}

impl<'a> FrameLayout<'a> {
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
            Location::Indirect { .. } | Location::Pair { .. } => None,
        }
    }

    fn frame_stack_offset(&self, offset: StackOffset) -> usize {
        self.saved_registers.len() * 8 + (offset.0 + 1) * 4
    }

    fn register_occupied(&self, location: CodeLocation, register: Register) -> bool {
        self.allocations.is_some_and(|allocations| {
            allocations.iter().any(|assigned| {
                assigned.interval().span.start <= location
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
