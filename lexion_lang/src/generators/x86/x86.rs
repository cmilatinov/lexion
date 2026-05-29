use crate::ast::types::TypeCollection;
use crate::ast::Lit;
use crate::diagnostic::DiagnosticConsumer;
use crate::generators::tac::instructions::{
    AssignmentInstruction, ConditionalJumpInstruction, ControlFlowGraph, FunctionRange,
    Instruction, Operand,
};
use crate::generators::x86::X86Target;
use crate::operators;
use crate::pipeline::PipelineStage;
use crate::symbol_table::SymbolTable;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};

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
}

impl<'a> X86EmitOptions<'a> {
    pub fn with_source_comments(source: &'a str) -> Self {
        Self {
            emit_source_comments: true,
            source: Some(source),
        }
    }
}

pub struct CodeGeneratorX86<'a> {
    cfg: &'a ControlFlowGraph,
    _types: &'a TypeCollection,
    _symbols: &'a SymbolTable,
    _target: X86Target,
}

impl<'a> CodeGeneratorX86<'a> {
    fn emit(&self, options: X86EmitOptions<'_>) -> X86Assembly {
        let mut lines = Vec::new();
        if options.emit_source_comments {
            emit_source_comments(&mut lines, options.source.unwrap_or_default());
        }
        lines.extend([
            String::from(".intel_syntax noprefix"),
            String::from(".text"),
        ]);
        for range in &self.cfg.functions {
            lines.extend(self.emit_function(*range));
        }
        X86Assembly::new(lines.join("\n"))
    }

    fn emit_function(&self, range: FunctionRange) -> Vec<String> {
        let name = self.cfg[range.start].label.clone();
        let slots = self.stack_slots(range);
        let stack_size = align_to(slots.len() * 4, 16);
        let mut lines = vec![
            format!(".global {name}"),
            format!("{name}:"),
            String::from("  push rbp"),
            String::from("  mov rbp, rsp"),
        ];
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
                lines.push(String::from("  ud2"));
                false
            }
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
                lines.push(String::from("  ud2"));
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
    type Input = (&'a ControlFlowGraph, &'a TypeCollection, &'a SymbolTable);
    type Options = X86EmitOptions<'a>;
    type Output = X86Assembly;

    fn new((cfg, types, symbols): Self::Input) -> Self {
        Self {
            cfg,
            _types: types,
            _symbols: symbols,
            _target: X86Target::default(),
        }
    }

    fn exec(self, _diag: &mut dyn DiagnosticConsumer, opts: Self::Options) -> Option<Self::Output> {
        Some(self.emit(opts))
    }
}

fn emit_source_comments(lines: &mut Vec<String>, source: &str) {
    for line in source.lines() {
        if line.is_empty() {
            lines.push(String::from("#"));
        } else {
            lines.push(format!("# {line}"));
        }
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
