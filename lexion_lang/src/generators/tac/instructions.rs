use crate::ast::Lit;
use crate::generators::label::Label;
use derived_deref::{Deref, DerefMut};
use enum_dispatch::enum_dispatch;
use lexion_lib::itertools::Itertools;
use lexion_lib::petgraph::graph::NodeIndex;
use lexion_lib::petgraph::Graph;
use lexion_lib::tabled::builder::Builder;
use lexion_lib::tabled::settings::Style;
use lexion_lib::tabled::Table;
use std::collections::HashSet;
use std::fmt::{Debug, Display, Formatter};

#[derive(Debug, Clone)]
pub enum Operand {
    Variable(String),
    Temporary(Label),
    Label(String),
    Literal(Lit),
    Placeholder,
}

impl Operand {
    pub fn is_temporary(&self) -> bool {
        matches!(self, Operand::Temporary(_))
    }

    pub fn is_literal(&self) -> bool {
        matches!(self, Operand::Literal(_))
    }

    pub fn iter(&self) -> impl Iterator<Item = String> {
        if self.is_literal() {
            None.into_iter()
        } else {
            Some(self.to_string()).into_iter()
        }
    }
}

impl Display for Operand {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Operand::Variable(inner) => write!(f, "{inner}"),
            Operand::Label(inner) => write!(f, "{inner}"),
            Operand::Temporary(inner) => write!(f, "{inner}"),
            Operand::Literal(inner) => write!(f, "{inner}"),
            Operand::Placeholder => write!(f, "_"),
        }
    }
}

pub struct AssignmentInstruction {
    pub target: Operand,
    pub left: Option<Operand>,
    pub operator: &'static str,
    pub right: Operand,
}

impl Display for AssignmentInstruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} = {}{} {}",
            self.target,
            if let Some(left) = &self.left {
                format!("{left} ")
            } else {
                String::from("")
            },
            self.operator,
            self.right
        )
    }
}

impl BaseInstruction for AssignmentInstruction {
    fn variables_read(&self) -> HashSet<String> {
        if let Some(left) = &self.left {
            HashSet::from_iter(left.iter().chain(self.right.iter()))
        } else {
            HashSet::from_iter(self.right.iter())
        }
    }
    fn variables_written(&self) -> HashSet<String> {
        HashSet::from_iter([self.target.to_string()])
    }
}

pub struct CopyInstruction {
    pub src: Operand,
    pub dst: Operand,
}

impl Display for CopyInstruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} = {}", self.dst, self.src)
    }
}

impl BaseInstruction for CopyInstruction {
    fn variables_read(&self) -> HashSet<String> {
        HashSet::from_iter(self.src.iter())
    }
    fn variables_written(&self) -> HashSet<String> {
        HashSet::from_iter([self.dst.to_string()])
    }
}

pub struct ConditionalJumpInstruction {
    pub left: Option<Operand>,
    pub operator: &'static str,
    pub right: Operand,
    pub target: Operand,
}

impl Display for ConditionalJumpInstruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "if {}{} {} then goto {}",
            if let Some(left) = &self.left {
                format!("{left} ")
            } else {
                String::from("")
            },
            self.operator,
            self.right,
            self.target,
        )
    }
}

impl BaseInstruction for ConditionalJumpInstruction {
    fn variables_read(&self) -> HashSet<String> {
        HashSet::from_iter(
            self.right
                .iter()
                .chain(self.left.iter().flat_map(|src| src.iter())),
        )
    }
}

pub struct JumpInstruction {
    pub target: Operand,
}

impl Display for JumpInstruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "goto {}", self.target)
    }
}

impl BaseInstruction for JumpInstruction {}

pub struct ParameterInstruction {
    pub param: Operand,
}

impl Display for ParameterInstruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "param {}", self.param)
    }
}

impl BaseInstruction for ParameterInstruction {
    fn variables_read(&self) -> HashSet<String> {
        HashSet::from_iter(self.param.iter())
    }
}

pub struct FunctionCallInstruction {
    pub function: String,
    pub return_target: Option<Operand>,
}

impl Display for FunctionCallInstruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}call {}",
            if let Some(return_target) = &self.return_target {
                format!("{return_target} = ")
            } else {
                String::from("")
            },
            self.function
        )
    }
}

impl BaseInstruction for FunctionCallInstruction {
    fn variables_written(&self) -> HashSet<String> {
        HashSet::from_iter(self.return_target.iter().map(|target| target.to_string()))
    }
}

pub struct ReturnInstruction {
    pub value: Option<Operand>,
}

impl Display for ReturnInstruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.value
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or(String::from(""))
        )
    }
}

impl BaseInstruction for ReturnInstruction {
    fn variables_read(&self) -> HashSet<String> {
        HashSet::from_iter(self.value.iter().flat_map(|src| src.iter()))
    }
}

pub struct FunctionInstruction {
    pub label: String,
}

impl Display for FunctionInstruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "fn {}", self.label)
    }
}

impl BaseInstruction for FunctionInstruction {}

pub struct EndFunctionInstruction {
    pub label: String,
}

impl Display for EndFunctionInstruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "endfn {}", self.label)
    }
}

impl BaseInstruction for EndFunctionInstruction {}

pub struct ExternInstruction {
    pub label: String,
}

impl Display for ExternInstruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "extern {}", self.label)
    }
}

impl BaseInstruction for ExternInstruction {}

#[enum_dispatch(BaseInstruction)]
pub enum Instruction {
    Assignment(AssignmentInstruction),
    Copy(CopyInstruction),
    ConditionalJump(ConditionalJumpInstruction),
    Jump(JumpInstruction),
    Parameter(ParameterInstruction),
    FunctionCall(FunctionCallInstruction),
    Return(ReturnInstruction),
    Function(FunctionInstruction),
    EndFunction(EndFunctionInstruction),
    Extern(ExternInstruction),
}

impl Display for Instruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Instruction::Assignment(assignment) => assignment.fmt(f),
            Instruction::Copy(copy) => copy.fmt(f),
            Instruction::ConditionalJump(conditional_jump) => conditional_jump.fmt(f),
            Instruction::Jump(jump) => jump.fmt(f),
            Instruction::Parameter(parameter) => parameter.fmt(f),
            Instruction::FunctionCall(function_call) => function_call.fmt(f),
            Instruction::Return(return_value) => return_value.fmt(f),
            Instruction::Function(function) => function.fmt(f),
            Instruction::EndFunction(end_function) => end_function.fmt(f),
            Instruction::Extern(external) => external.fmt(f),
        }
    }
}

#[enum_dispatch]
pub trait BaseInstruction {
    fn variables_read(&self) -> HashSet<String> {
        Default::default()
    }
    fn variables_written(&self) -> HashSet<String> {
        Default::default()
    }
}

#[derive(Default)]
pub struct LiveSets {
    pub input: HashSet<String>,
    pub output: HashSet<String>,
    pub read: HashSet<String>,
    pub written: HashSet<String>,
}

impl Display for LiveSets {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "[in = {{ ")?;
        write!(f, "{}", self.input.iter().map(|s| s.as_str()).join(", "))?;
        write!(f, " }}]")
    }
}

pub struct InstructionInstance {
    pub instruction: Instruction,
    pub live: LiveSets,
}

impl Display for InstructionInstance {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}     {}", self.instruction, self.live)
    }
}

impl Debug for InstructionInstance {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.instruction)
    }
}

pub struct InstructionBlock {
    pub label: String,
    pub instructions: Vec<InstructionInstance>,
    pub live: LiveSets,
}

impl Display for InstructionBlock {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:     {}", self.label, self.live)?;
        for instruction in &self.instructions {
            write!(f, "\n{instruction}")?;
        }
        Ok(())
    }
}

impl Debug for InstructionBlock {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:", self.label)?;
        for instruction in &self.instructions {
            write!(f, "\n{instruction:?}")?;
        }
        Ok(())
    }
}

impl InstructionBlock {
    pub fn new(label: String) -> Self {
        Self {
            label,
            instructions: Default::default(),
            live: Default::default(),
        }
    }

    pub fn table(&self) -> Table {
        let mut builder = Builder::new();
        builder.push_record(["Instruction", "Read", "Written", "Live In", "Live Out"]);
        for inst in &self.instructions {
            builder.push_record([
                inst.instruction.to_string(),
                format!("{:?}", inst.live.read),
                format!("{:?}", inst.live.written),
                format!("{:?}", inst.live.input),
                format!("{:?}", inst.live.output),
            ]);
        }
        let mut table = builder.build();
        table.with(Style::modern());
        table
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionRange {
    pub start: NodeIndex,
    pub end: NodeIndex,
}

impl FunctionRange {
    pub fn contains(&self, span: CodeSpan) -> bool {
        self.start.index() <= span.start.block.index() && span.end.block.index() <= self.end.index()
    }
}

#[derive(Deref, DerefMut)]
pub struct ControlFlowGraph {
    pub functions: Vec<FunctionRange>,
    #[target]
    pub graph: Graph<InstructionBlock, ()>,
}

impl Default for ControlFlowGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlFlowGraph {
    pub fn new() -> Self {
        Self {
            functions: Default::default(),
            graph: Default::default(),
        }
    }

    pub fn block(&mut self, label: String, is_function_entry: bool) -> NodeIndex {
        let block = self.graph.add_node(InstructionBlock::new(label));
        if is_function_entry {
            if let Some(last) = self.functions.last_mut() {
                last.end = NodeIndex::new(self.graph.node_count() - 2);
            }
            self.functions.push(FunctionRange {
                start: block,
                end: block,
            });
        } else if let Some(last) = self.functions.last_mut() {
            last.end = block;
        }
        block
    }

    pub fn end_function(&mut self) {
        let Some(last) = self.functions.last_mut() else {
            return;
        };
        last.end = NodeIndex::new(self.graph.node_count() - 1);
    }

    pub fn link(&mut self, start_block: NodeIndex, end_block: NodeIndex) {
        self.graph.add_edge(start_block, end_block, ());
    }

    pub fn function_nodes(&self, func: &FunctionRange) -> impl Iterator<Item = NodeIndex> {
        (func.start.index()..=func.end.index()).map(NodeIndex::new)
    }
}

#[derive(Debug, Clone, Copy, Ord, PartialOrd, Eq, PartialEq)]
pub struct CodeLocation {
    pub block: NodeIndex,
    pub instruction: usize,
}

impl CodeLocation {
    pub fn new(block: NodeIndex, instruction: usize) -> Self {
        Self { block, instruction }
    }

    pub fn instruction<'a>(&self, cfg: &'a ControlFlowGraph) -> &'a Instruction {
        &cfg[self.block].instructions[self.instruction].instruction
    }

    pub fn instruction_mut<'a>(&self, cfg: &'a mut ControlFlowGraph) -> &'a mut Instruction {
        &mut cfg[self.block].instructions[self.instruction].instruction
    }
}

#[derive(Debug, Clone, Copy, Ord, PartialOrd, Eq, PartialEq)]
pub struct CodeSpan {
    pub start: CodeLocation,
    pub end: CodeLocation,
}

impl CodeSpan {
    pub fn new(start: CodeLocation, end: CodeLocation) -> Self {
        Self { start, end }
    }

    pub fn from_location(loc: CodeLocation) -> Self {
        Self {
            start: loc,
            end: loc,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LivenessInterval {
    pub variable: String,
    pub span: CodeSpan,
    pub uses: Vec<CodeLocation>,
}
