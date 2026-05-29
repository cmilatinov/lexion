use crate::ast::types::TypeCollection;
use crate::ast::Lit;
use crate::diagnostic::{DiagnosticConsumer, LexionDiagnosticError};
use crate::generators::tac::instructions::{
    AssignmentInstruction, ConditionalJumpInstruction, ControlFlowGraph, FunctionRange,
    Instruction, Operand,
};
use crate::operators;
use crate::pipeline::PipelineStage;
use crate::symbol_table::SymbolTableGraph;
use iced_x86::code_asm::*;
use iced_x86::{BlockEncoderOptions, IcedError};
use lexion_lib::miette::{NamedSource, SourceSpan};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X86MachineCode {
    bytes: Vec<u8>,
    symbols: BTreeMap<String, usize>,
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
    _types: &'a TypeCollection,
    _symbols: &'a SymbolTableGraph,
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
        let slots = self.stack_slots(range);
        let stack_size = align_to(slots.len() * 4, 16);
        self.set_block_label(assembler, labels, self.cfg[range.start].label.as_str())?;
        assembler.push(rbp)?;
        assembler.mov(rbp, rsp)?;
        if stack_size > 0 {
            assembler.sub(rsp, stack_size as i32)?;
        }

        let mut emitted_return = false;
        for node in self.cfg.function_nodes(&range) {
            let block = &self.cfg[node];
            if node != range.start {
                self.set_block_label(assembler, labels, block.label.as_str())?;
            }
            for inst in &block.instructions {
                if self.emit_instruction(assembler, labels, &slots, &inst.instruction)? {
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
        instruction: &Instruction,
    ) -> Result<bool, IcedError> {
        match instruction {
            Instruction::Assignment(inst) => {
                self.emit_assignment(assembler, slots, inst)?;
                Ok(false)
            }
            Instruction::Copy(inst) => {
                load_operand(assembler, slots, &inst.src, eax)?;
                store_operand(assembler, slots, &inst.dst, eax)?;
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
                    load_operand(assembler, slots, value, eax)?;
                }
                emit_epilogue(assembler)?;
                Ok(true)
            }
            Instruction::Function(_) | Instruction::EndFunction(_) | Instruction::Extern(_) => {
                Ok(false)
            }
            Instruction::Parameter(_) | Instruction::FunctionCall(_) => {
                unreachable!("unsupported x86 instructions are diagnosed before emission")
            }
        }
    }

    fn emit_assignment(
        &self,
        assembler: &mut CodeAssembler,
        slots: &BTreeMap<String, usize>,
        inst: &AssignmentInstruction,
    ) -> Result<(), IcedError> {
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
            (Some(left), operators::LOGICAL_AND) => {
                load_operand(assembler, slots, left, eax)?;
                and_operand(assembler, slots, &inst.right)?;
            }
            (Some(left), operators::LOGICAL_OR) => {
                load_operand(assembler, slots, left, eax)?;
                or_operand(assembler, slots, &inst.right)?;
            }
            (Some(_), _) => {
                unreachable!("unsupported x86 assignment operators are diagnosed before emission")
            }
        }
        store_operand(assembler, slots, &inst.target, eax)
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

    fn validate_supported(
        &self,
        diag: &mut dyn DiagnosticConsumer,
        options: X86MachineCodeOptions,
    ) -> bool {
        let mut valid = true;
        for range in &self.cfg.functions {
            for node in self.cfg.function_nodes(range) {
                for inst in &self.cfg[node].instructions {
                    if let Some(message) = unsupported_message(&inst.instruction) {
                        valid = false;
                        diag.error(machine_code_error(options, inst.source_span, message));
                    }
                }
            }
        }
        valid
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
            _types: types,
            _symbols: symbols,
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
    *labels
        .get(label.as_str())
        .unwrap_or_else(|| panic!("missing block label `{label}`"))
}

fn stack_value(slots: &BTreeMap<String, usize>, operand: &Operand) -> AsmMemoryOperand {
    let name = match operand {
        Operand::Variable(name) => name.clone(),
        Operand::Temporary(label) => label.to_string(),
        _ => unreachable!("operand does not have a stack slot"),
    };
    let offset = slots
        .get(name.as_str())
        .unwrap_or_else(|| panic!("missing stack slot for {name}"));
    dword_ptr(rbp - *offset as i32)
}

fn literal_value(operand: &Operand) -> i32 {
    match operand {
        Operand::Literal(Lit::Integer(value)) => (*value)
            .try_into()
            .expect("x86 machine-code emitter only supports i32 integer literals"),
        Operand::Literal(Lit::Boolean(value)) => i32::from(*value),
        Operand::Literal(_) => 0,
        _ => unreachable!(),
    }
}

fn unsupported_message(instruction: &Instruction) -> Option<String> {
    match instruction {
        Instruction::Assignment(inst) => (!assignment_supported(inst)).then(|| {
            format!(
                "x86 machine-code backend does not support `{}` assignments yet",
                inst.operator
            )
        }),
        Instruction::FunctionCall(inst) => Some(format!(
            "x86 machine-code backend does not support function calls yet: {}",
            inst.function
        )),
        Instruction::Extern(inst) => Some(format!(
            "x86 machine-code backend does not support extern declarations yet: {}",
            inst.label
        )),
        Instruction::Copy(_)
        | Instruction::ConditionalJump(_)
        | Instruction::Jump(_)
        | Instruction::Parameter(_)
        | Instruction::Return(_)
        | Instruction::Function(_)
        | Instruction::EndFunction(_) => None,
    }
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
        Instruction::Jump(_)
        | Instruction::FunctionCall(_)
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
