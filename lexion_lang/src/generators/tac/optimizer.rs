use crate::ast::Lit;
use crate::diagnostic::DiagnosticConsumer;
use crate::generators::tac::instructions::{
    AssignmentInstruction, BaseInstruction, ConditionalJumpInstruction, ControlFlowGraph,
    CopyInstruction, Instruction, JumpInstruction, Operand,
};
use crate::operators;
use crate::pipeline::PipelineStage;
use lexion_lib::petgraph::graph::NodeIndex;
use lexion_lib::petgraph::visit::EdgeRef;
use lexion_lib::petgraph::Direction;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TacOptimizerOptions {
    pub constant_folding: bool,
    pub branch_simplification: bool,
}

impl TacOptimizerOptions {
    pub fn none() -> Self {
        Self {
            constant_folding: false,
            branch_simplification: false,
        }
    }
}

impl Default for TacOptimizerOptions {
    fn default() -> Self {
        Self {
            constant_folding: true,
            branch_simplification: true,
        }
    }
}

pub struct CodeOptimizerTac {
    cfg: ControlFlowGraph,
}

impl PipelineStage for CodeOptimizerTac {
    type Input = ControlFlowGraph;
    type Options = TacOptimizerOptions;
    type Output = ControlFlowGraph;

    fn new(cfg: Self::Input) -> Self {
        Self { cfg }
    }

    fn exec(
        mut self,
        _diag: &mut dyn DiagnosticConsumer,
        opts: Self::Options,
    ) -> Option<Self::Output> {
        if opts.constant_folding {
            self.fold_constants();
        }
        if opts.branch_simplification {
            self.simplify_branches();
        }
        Some(self.cfg)
    }
}

impl CodeOptimizerTac {
    fn fold_constants(&mut self) {
        for block in self.cfg.node_weights_mut() {
            for inst in &mut block.instructions {
                let Instruction::Assignment(assignment) = &inst.instruction else {
                    continue;
                };
                let Some(folded) = fold_assignment(assignment) else {
                    continue;
                };
                inst.instruction = Instruction::Copy(CopyInstruction {
                    dst: assignment.target.clone(),
                    src: Operand::Literal(folded),
                });
            }
        }
    }

    fn simplify_branches(&mut self) {
        let labels = label_nodes(&self.cfg);
        self.rewrite_jump_chains(&labels);
        self.simplify_constant_conditional_jumps(&labels);
        self.remove_redundant_fallthrough_jumps();
    }

    fn rewrite_jump_chains(&mut self, labels: &HashMap<String, NodeIndex>) {
        let nodes = self.cfg.node_indices().collect::<Vec<_>>();
        for node in nodes {
            let replacements = self.cfg[node]
                .instructions
                .iter()
                .enumerate()
                .filter_map(|(index, inst)| match &inst.instruction {
                    Instruction::Jump(jump) => label_operand(&jump.target).and_then(|label| {
                        resolve_jump_target(&self.cfg, labels, label)
                            .filter(|resolved| resolved != label)
                            .map(|resolved| (index, label.to_string(), resolved, true))
                    }),
                    Instruction::ConditionalJump(jump) => {
                        label_operand(&jump.target).and_then(|label| {
                            resolve_jump_target(&self.cfg, labels, label)
                                .filter(|resolved| resolved != label)
                                .map(|resolved| (index, label.to_string(), resolved, false))
                        })
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();

            for (index, old_label, new_label, unconditional) in replacements {
                match &mut self.cfg[node].instructions[index].instruction {
                    Instruction::Jump(jump) => jump.target = Operand::Label(new_label.clone()),
                    Instruction::ConditionalJump(jump) => {
                        jump.target = Operand::Label(new_label.clone())
                    }
                    _ => {}
                }
                if let Some(new_target) = labels.get(new_label.as_str()).copied() {
                    if unconditional {
                        retain_only_edge(&mut self.cfg, node, new_target);
                    } else if let Some(old_target) = labels.get(old_label.as_str()).copied() {
                        remove_edge_to(&mut self.cfg, node, old_target);
                    }
                    ensure_edge(&mut self.cfg, node, new_target);
                }
            }
        }
    }

    fn simplify_constant_conditional_jumps(&mut self, labels: &HashMap<String, NodeIndex>) {
        let nodes = self.cfg.node_indices().collect::<Vec<_>>();
        for node in nodes {
            let mut constants = HashMap::new();
            let mut replacements = Vec::new();
            for (index, inst) in self.cfg[node].instructions.iter().enumerate() {
                match &inst.instruction {
                    Instruction::ConditionalJump(jump) => {
                        if let Some(taken) = evaluate_condition(jump, &constants) {
                            replacements.push((index, jump.target.clone(), taken));
                        }
                    }
                    _ => record_constants(&mut constants, &inst.instruction),
                }
                remove_written_constants(&mut constants, &inst.instruction);
            }

            for (index, target, taken) in replacements.into_iter().rev() {
                let target_node = label_operand(&target).and_then(|label| labels.get(label));
                if taken {
                    self.cfg[node].instructions[index].instruction =
                        Instruction::Jump(JumpInstruction { target });
                    if let Some(target_node) = target_node {
                        retain_only_edge(&mut self.cfg, node, *target_node);
                    }
                } else {
                    self.cfg[node].instructions.remove(index);
                    if let Some(target_node) = target_node {
                        remove_edge_to(&mut self.cfg, node, *target_node);
                    }
                }
            }
        }
    }

    fn remove_redundant_fallthrough_jumps(&mut self) {
        for range in self.cfg.functions.clone() {
            let nodes = self.cfg.function_nodes(&range).collect::<Vec<_>>();
            for window in nodes.windows(2) {
                let [node, next] = window else {
                    continue;
                };
                let next_label = self.cfg[*next].label.clone();
                let Some(last) = self.cfg[*node].instructions.last() else {
                    continue;
                };
                let Instruction::Jump(jump) = &last.instruction else {
                    continue;
                };
                if label_operand(&jump.target) == Some(next_label.as_str()) {
                    self.cfg[*node].instructions.pop();
                }
            }
        }
    }
}

fn fold_assignment(inst: &AssignmentInstruction) -> Option<Lit> {
    match (inst.left.as_ref(), inst.operator, &inst.right) {
        (None, operators::UNARY_PLUS, Operand::Literal(Lit::Integer(value))) => {
            Some(Lit::Integer(*value))
        }
        (None, operators::UNARY_MINUS, Operand::Literal(Lit::Integer(value))) => {
            value.checked_neg().map(Lit::Integer)
        }
        (None, operators::LOGICAL_NOT, Operand::Literal(Lit::Boolean(value))) => {
            Some(Lit::Boolean(!value))
        }
        (Some(Operand::Literal(left)), operator, Operand::Literal(right)) => {
            fold_binary_literals(left, operator, right)
        }
        _ => None,
    }
}

fn fold_binary_literals(left: &Lit, operator: &str, right: &Lit) -> Option<Lit> {
    match (left, operator, right) {
        (Lit::Integer(left), operators::PLUS, Lit::Integer(right)) => {
            left.checked_add(*right).map(Lit::Integer)
        }
        (Lit::Integer(left), operators::MINUS, Lit::Integer(right)) => {
            left.checked_sub(*right).map(Lit::Integer)
        }
        (Lit::Integer(left), operators::MULTIPLY, Lit::Integer(right)) => {
            left.checked_mul(*right).map(Lit::Integer)
        }
        (Lit::Integer(left), operators::DIVIDE, Lit::Integer(right)) if *right != 0 => {
            left.checked_div(*right).map(Lit::Integer)
        }
        (Lit::Integer(left), operators::REMAINDER, Lit::Integer(right)) if *right != 0 => {
            left.checked_rem(*right).map(Lit::Integer)
        }
        (Lit::Integer(left), operator, Lit::Integer(right)) => {
            compare_integers(*left, operator, *right).map(Lit::Boolean)
        }
        (Lit::Boolean(left), operators::LOGICAL_AND, Lit::Boolean(right)) => {
            Some(Lit::Boolean(*left && *right))
        }
        (Lit::Boolean(left), operators::LOGICAL_OR, Lit::Boolean(right)) => {
            Some(Lit::Boolean(*left || *right))
        }
        (Lit::Boolean(left), operators::EQUALS, Lit::Boolean(right)) => {
            Some(Lit::Boolean(left == right))
        }
        (Lit::Boolean(left), operators::NOT_EQUALS, Lit::Boolean(right)) => {
            Some(Lit::Boolean(left != right))
        }
        _ => None,
    }
}

fn compare_integers(left: isize, operator: &str, right: isize) -> Option<bool> {
    match operator {
        operators::EQUALS => Some(left == right),
        operators::NOT_EQUALS => Some(left != right),
        operators::LESS => Some(left < right),
        operators::LESS_EQUALS => Some(left <= right),
        operators::GREATER => Some(left > right),
        operators::GREATER_EQUALS => Some(left >= right),
        _ => None,
    }
}

fn evaluate_condition(
    jump: &ConditionalJumpInstruction,
    constants: &HashMap<String, Lit>,
) -> Option<bool> {
    let right = literal_for_operand(&jump.right, constants)?;
    if let Some(left) = &jump.left {
        let left = literal_for_operand(left, constants)?;
        fold_binary_literals(left, jump.operator, right).and_then(|lit| match lit {
            Lit::Boolean(value) => Some(value),
            _ => None,
        })
    } else {
        match (jump.operator, right) {
            (operators::EQUALS, Lit::Boolean(value)) => Some(*value),
            (operators::NOT_EQUALS, Lit::Boolean(value)) => Some(!*value),
            _ => None,
        }
    }
}

fn literal_for_operand<'a>(
    operand: &'a Operand,
    constants: &'a HashMap<String, Lit>,
) -> Option<&'a Lit> {
    match operand {
        Operand::Literal(lit) => Some(lit),
        Operand::Variable(name) => constants.get(name),
        Operand::Temporary(label) => constants.get(label.to_string().as_str()),
        Operand::Label(_) | Operand::Placeholder => None,
    }
}

fn record_constants(constants: &mut HashMap<String, Lit>, instruction: &Instruction) {
    let Instruction::Copy(copy) = instruction else {
        return;
    };
    let Some(dst) = operand_name(&copy.dst) else {
        return;
    };
    if let Some(value) = literal_for_operand(&copy.src, constants).cloned() {
        constants.insert(dst, value);
    }
}

fn remove_written_constants(constants: &mut HashMap<String, Lit>, instruction: &Instruction) {
    for name in instruction.variables_written() {
        if !matches!(
            instruction,
            Instruction::Copy(copy)
                if operand_name(&copy.dst).as_deref() == Some(name.as_str())
                    && literal_for_operand(&copy.src, constants).is_some()
        ) {
            constants.remove(name.as_str());
        }
    }
}

fn operand_name(operand: &Operand) -> Option<String> {
    match operand {
        Operand::Variable(name) => Some(name.clone()),
        Operand::Temporary(label) => Some(label.to_string()),
        Operand::Literal(_) | Operand::Label(_) | Operand::Placeholder => None,
    }
}

fn label_operand(operand: &Operand) -> Option<&str> {
    match operand {
        Operand::Label(label) => Some(label.as_str()),
        _ => None,
    }
}

fn label_nodes(cfg: &ControlFlowGraph) -> HashMap<String, NodeIndex> {
    cfg.node_indices()
        .map(|node| (cfg[node].label.clone(), node))
        .collect()
}

fn resolve_jump_target(
    cfg: &ControlFlowGraph,
    labels: &HashMap<String, NodeIndex>,
    label: &str,
) -> Option<String> {
    let mut current = label.to_string();
    let mut seen = HashSet::new();
    while seen.insert(current.clone()) {
        let node = labels.get(current.as_str())?;
        let Some(next) = sole_jump_target(&cfg[*node]) else {
            return Some(current);
        };
        current = next;
    }
    Some(current)
}

fn sole_jump_target(
    block: &crate::generators::tac::instructions::InstructionBlock,
) -> Option<String> {
    let [inst] = block.instructions.as_slice() else {
        return None;
    };
    let Instruction::Jump(jump) = &inst.instruction else {
        return None;
    };
    label_operand(&jump.target).map(String::from)
}

fn retain_only_edge(cfg: &mut ControlFlowGraph, source: NodeIndex, target: NodeIndex) {
    let edges = cfg
        .edges_directed(source, Direction::Outgoing)
        .filter(|edge| edge.target() != target)
        .map(|edge| edge.id())
        .collect::<Vec<_>>();
    for edge in edges {
        cfg.remove_edge(edge);
    }
}

fn remove_edge_to(cfg: &mut ControlFlowGraph, source: NodeIndex, target: NodeIndex) {
    let edges = cfg
        .edges_directed(source, Direction::Outgoing)
        .filter(|edge| edge.target() == target)
        .map(|edge| edge.id())
        .collect::<Vec<_>>();
    for edge in edges {
        cfg.remove_edge(edge);
    }
}

fn ensure_edge(cfg: &mut ControlFlowGraph, source: NodeIndex, target: NodeIndex) {
    let exists = cfg
        .edges_directed(source, Direction::Outgoing)
        .any(|edge| edge.target() == target);
    if !exists {
        cfg.add_edge(source, target, ());
    }
}
