use crate::ast::types::TypeCollection;
use crate::ast::Lit;
use crate::diagnostic::{DiagnosticConsumer, LexionDiagnosticError};
use crate::generators::tac::instructions::{
    AssignmentInstruction, ConditionalJumpInstruction, ControlFlowGraph, FunctionRange,
    Instruction, InstructionInstance, Operand,
};
use crate::generators::x86::X86Target;
use crate::operators;
use crate::pipeline::PipelineStage;
use crate::symbol_table::SymbolTableGraph;
use lexion_lib::miette::{NamedSource, SourceSpan};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::sync::Arc;

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
    _types: &'a TypeCollection,
    symbols: &'a SymbolTableGraph,
    _target: X86Target,
}

impl<'a> CodeGeneratorX86<'a> {
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
        let slots = self.stack_slots(range);
        let stack_size = align_to(slots.len() * 4, 16);
        let mut source_line_range = None;
        let mut lines = vec![format!(".global {name}"), format!("{name}:")];
        self.emit_symbol_source_comments(&mut lines, options, &name, &mut source_line_range);
        lines.extend([String::from("  push rbp"), String::from("  mov rbp, rsp")]);
        if stack_size > 0 {
            lines.push(format!("  sub rsp, {stack_size}"));
        }

        let mut emitted_return = false;
        for node in self.cfg.function_nodes(&range) {
            let block = &self.cfg[node];
            if node != range.start {
                lines.push(format!("{}:", block.label));
            }
            for inst in &block.instructions {
                self.emit_instruction_source_comments(
                    &mut lines,
                    options,
                    inst,
                    &mut source_line_range,
                );
                if self.emit_instruction(&mut lines, &slots, &inst.instruction) {
                    emitted_return = true;
                }
            }
        }

        if !emitted_return {
            emit_epilogue(&mut lines);
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
        self.symbols
            .graph
            .node_weights()
            .flat_map(|table| table.entries.iter())
            .find(|entry| entry.name == name)
            .map(|entry| entry.span)
    }

    fn emit_instruction(
        &self,
        lines: &mut Vec<String>,
        slots: &BTreeMap<String, usize>,
        instruction: &Instruction,
    ) -> bool {
        match instruction {
            Instruction::Assignment(inst) => {
                self.emit_assignment(lines, slots, inst);
                false
            }
            Instruction::Copy(inst) => {
                load_operand(lines, slots, &inst.src, "eax");
                store_operand(lines, slots, &inst.dst, "eax");
                false
            }
            Instruction::ConditionalJump(inst) => {
                self.emit_conditional_jump(lines, slots, inst);
                false
            }
            Instruction::Jump(inst) => {
                lines.push(format!("  jmp {}", inst.target));
                false
            }
            Instruction::Return(inst) => {
                if let Some(value) = &inst.value {
                    load_operand(lines, slots, value, "eax");
                }
                emit_epilogue(lines);
                true
            }
            Instruction::Function(_) | Instruction::EndFunction(_) | Instruction::Extern(_) => {
                false
            }
            Instruction::Parameter(_) | Instruction::FunctionCall(_) => {
                unreachable!("unsupported x86 instructions are diagnosed before emission")
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
            Instruction::Assignment(inst) => (!assignment_supported(inst)).then(|| {
                format!(
                    "x86 backend does not support `{}` assignments yet",
                    inst.operator
                )
            }),
            Instruction::FunctionCall(inst) => Some(format!(
                "x86 backend does not support function calls yet: {}",
                inst.function
            )),
            Instruction::Extern(inst) => Some(format!(
                "x86 backend does not support extern declarations yet: {}",
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

    fn emit_assignment(
        &self,
        lines: &mut Vec<String>,
        slots: &BTreeMap<String, usize>,
        inst: &AssignmentInstruction,
    ) {
        match (inst.left.as_ref(), inst.operator) {
            (None, operators::UNARY_MINUS) => {
                load_operand(lines, slots, &inst.right, "eax");
                lines.push(String::from("  neg eax"));
            }
            (None, operators::LOGICAL_NOT) => {
                load_operand(lines, slots, &inst.right, "eax");
                lines.push(String::from("  cmp eax, 0"));
                lines.push(String::from("  sete al"));
                lines.push(String::from("  movzx eax, al"));
            }
            (None, _) => {
                load_operand(lines, slots, &inst.right, "eax");
            }
            (Some(left), operators::PLUS) => {
                load_operand(lines, slots, left, "eax");
                lines.push(format!("  add eax, {}", operand_value(slots, &inst.right)));
            }
            (Some(left), operators::MINUS) => {
                load_operand(lines, slots, left, "eax");
                lines.push(format!("  sub eax, {}", operand_value(slots, &inst.right)));
            }
            (Some(left), operators::MULTIPLY) => {
                load_operand(lines, slots, left, "eax");
                lines.push(format!("  imul eax, {}", operand_value(slots, &inst.right)));
            }
            (Some(left), operators::DIVIDE | operators::REMAINDER) => {
                load_operand(lines, slots, left, "eax");
                lines.push(String::from("  cdq"));
                load_operand(lines, slots, &inst.right, "ecx");
                lines.push(String::from("  idiv ecx"));
                if inst.operator == operators::REMAINDER {
                    lines.push(String::from("  mov eax, edx"));
                }
            }
            (Some(left), operators::EQUALS | operators::NOT_EQUALS) => {
                self.emit_compare(lines, slots, left, &inst.right, inst.operator);
            }
            (Some(left), operators::LESS | operators::LESS_EQUALS) => {
                self.emit_compare(lines, slots, left, &inst.right, inst.operator);
            }
            (Some(left), operators::GREATER | operators::GREATER_EQUALS) => {
                self.emit_compare(lines, slots, left, &inst.right, inst.operator);
            }
            (Some(left), operators::LOGICAL_AND) => {
                load_operand(lines, slots, left, "eax");
                lines.push(format!("  and eax, {}", operand_value(slots, &inst.right)));
            }
            (Some(left), operators::LOGICAL_OR) => {
                load_operand(lines, slots, left, "eax");
                lines.push(format!("  or eax, {}", operand_value(slots, &inst.right)));
            }
            (Some(_), _) => {
                unreachable!("unsupported x86 assignment operators are diagnosed before emission")
            }
        }
        store_operand(lines, slots, &inst.target, "eax");
    }

    fn emit_compare(
        &self,
        lines: &mut Vec<String>,
        slots: &BTreeMap<String, usize>,
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
        load_operand(lines, slots, left, "eax");
        lines.push(format!("  cmp eax, {}", operand_value(slots, right)));
        lines.push(format!("  {setcc} al"));
        lines.push(String::from("  movzx eax, al"));
    }

    fn emit_conditional_jump(
        &self,
        lines: &mut Vec<String>,
        slots: &BTreeMap<String, usize>,
        inst: &ConditionalJumpInstruction,
    ) {
        if let Some(left) = &inst.left {
            load_operand(lines, slots, left, "eax");
            lines.push(format!("  cmp eax, {}", operand_value(slots, &inst.right)));
        } else {
            load_operand(lines, slots, &inst.right, "eax");
            lines.push(String::from("  cmp eax, 0"));
        }
        lines.push(format!("  {} {}", jump_for(inst.operator), inst.target));
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
            _types: types,
            symbols,
            _target: X86Target::default(),
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

fn emit_epilogue(lines: &mut Vec<String>) {
    lines.push(String::from("  mov rsp, rbp"));
    lines.push(String::from("  pop rbp"));
    lines.push(String::from("  ret"));
}

fn load_operand(
    lines: &mut Vec<String>,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
    register: &str,
) {
    match operand {
        Operand::Literal(_) => lines.push(format!("  mov {register}, {}", literal_value(operand))),
        Operand::Variable(_) | Operand::Temporary(_) => lines.push(format!(
            "  mov {register}, {}",
            operand_value(slots, operand)
        )),
        Operand::Placeholder => lines.push(format!("  xor {register}, {register}")),
        Operand::Label(_) => lines.push(format!("  lea {register}, [{operand}]")),
    }
}

fn store_operand(
    lines: &mut Vec<String>,
    slots: &BTreeMap<String, usize>,
    operand: &Operand,
    register: &str,
) {
    if matches!(operand, Operand::Variable(_) | Operand::Temporary(_)) {
        lines.push(format!(
            "  mov {}, {register}",
            operand_value(slots, operand)
        ));
    }
}

fn operand_value(slots: &BTreeMap<String, usize>, operand: &Operand) -> String {
    match operand {
        Operand::Literal(_) => literal_value(operand),
        Operand::Variable(name) => stack_value(slots, name),
        Operand::Temporary(label) => stack_value(slots, label.to_string().as_str()),
        Operand::Label(label) => label.clone(),
        Operand::Placeholder => String::from("0"),
    }
}

fn stack_value(slots: &BTreeMap<String, usize>, name: &str) -> String {
    let offset = slots
        .get(name)
        .unwrap_or_else(|| panic!("missing stack slot for {name}"));
    format!("DWORD PTR [rbp-{offset}]")
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
