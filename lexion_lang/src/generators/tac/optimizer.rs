use super::tac::analyze_liveness;
use crate::ast::Lit;
use crate::diagnostic::DiagnosticConsumer;
use crate::generators::tac::instructions::{
    AssignmentInstruction, BaseInstruction, ConditionalJumpInstruction, ControlFlowGraph,
    CopyInstruction, Instruction, InstructionBlock, JumpInstruction, Operand, ReturnInstruction,
};
use crate::operators;
use crate::pipeline::PipelineStage;
use lexion_lib::petgraph::graph::NodeIndex;
use lexion_lib::petgraph::visit::EdgeRef;
use lexion_lib::petgraph::Direction;
use std::collections::{HashMap, HashSet, VecDeque};

const DEFAULT_TARGET_WORD_BITS: u32 = 32;
const OPTIMIZER_ITERATIONS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TacOptimizerOptions {
    pub constant_folding: bool,
    pub value_propagation: bool,
    pub common_subexpression_elimination: bool,
    pub branch_simplification: bool,
    pub cfg_cleanup: bool,
    pub dead_code_elimination: bool,
    pub target_word_bits: u32,
}

impl TacOptimizerOptions {
    pub fn none() -> Self {
        Self {
            constant_folding: false,
            value_propagation: false,
            common_subexpression_elimination: false,
            branch_simplification: false,
            cfg_cleanup: false,
            dead_code_elimination: false,
            target_word_bits: DEFAULT_TARGET_WORD_BITS,
        }
    }

    pub fn for_target_word_bits(bits: u32) -> Self {
        Self {
            target_word_bits: bits,
            ..Self::default()
        }
    }
}

impl Default for TacOptimizerOptions {
    fn default() -> Self {
        Self {
            constant_folding: true,
            value_propagation: true,
            common_subexpression_elimination: true,
            branch_simplification: true,
            cfg_cleanup: true,
            dead_code_elimination: true,
            target_word_bits: DEFAULT_TARGET_WORD_BITS,
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
        for _ in 0..OPTIMIZER_ITERATIONS {
            let mut changed = false;
            if opts.constant_folding {
                changed |= self.fold_constants(opts.target_word_bits);
            }
            if opts.value_propagation {
                changed |= self.propagate_values(opts.target_word_bits);
                changed |= self.simplify_returns();
            }
            if opts.common_subexpression_elimination {
                changed |= self.eliminate_common_subexpressions();
            }
            if opts.constant_folding {
                changed |= self.fold_constants(opts.target_word_bits);
            }
            if opts.branch_simplification {
                changed |= self.simplify_branches(opts.target_word_bits);
            }
            if opts.cfg_cleanup {
                changed |= self.cleanup_cfg();
            }
            if opts.dead_code_elimination {
                changed |= self.eliminate_dead_code();
            }
            if !changed {
                break;
            }
        }
        Some(self.cfg)
    }
}

impl CodeOptimizerTac {
    fn fold_constants(&mut self, target_word_bits: u32) -> bool {
        let mut changed = false;
        for block in self.cfg.node_weights_mut() {
            for inst in &mut block.instructions {
                let Instruction::Assignment(assignment) = &inst.instruction else {
                    continue;
                };
                let Some(folded) = fold_assignment(assignment, target_word_bits) else {
                    continue;
                };
                inst.instruction = Instruction::Copy(CopyInstruction {
                    dst: assignment.target.clone(),
                    src: Operand::Literal(folded),
                });
                changed = true;
            }
        }
        changed
    }

    fn propagate_values(&mut self, target_word_bits: u32) -> bool {
        let mut changed = false;
        for block in self.cfg.node_weights_mut() {
            let mut values = KnownValues::default();
            for inst in &mut block.instructions {
                changed |= substitute_instruction_reads(&mut inst.instruction, &values);
                changed |= simplify_instruction(&mut inst.instruction, target_word_bits);

                if matches!(inst.instruction, Instruction::FunctionCall(_)) {
                    values.clear();
                } else {
                    let written = inst.instruction.variables_written();
                    values.invalidate_writes(&written);
                    values.record(&inst.instruction);
                }
            }
        }
        changed
    }

    fn eliminate_common_subexpressions(&mut self) -> bool {
        let mut changed = false;
        for block in self.cfg.node_weights_mut() {
            let mut expressions = HashMap::<ExpressionKey, Operand>::new();
            for inst in &mut block.instructions {
                if matches!(inst.instruction, Instruction::FunctionCall(_)) {
                    expressions.clear();
                }

                let replacement = match &inst.instruction {
                    Instruction::Assignment(assignment) => expression_key(assignment)
                        .and_then(|key| expressions.get(&key).cloned())
                        .map(|src| (assignment.target.clone(), src)),
                    _ => None,
                };

                if let Some((dst, src)) = replacement {
                    inst.instruction = Instruction::Copy(CopyInstruction { dst, src });
                    changed = true;
                }

                let written = inst.instruction.variables_written();
                invalidate_expressions(&mut expressions, &written);

                if let Instruction::Assignment(assignment) = &inst.instruction {
                    if let Some(key) = expression_key(assignment) {
                        expressions.insert(key, assignment.target.clone());
                    }
                }
            }
        }
        changed
    }

    fn simplify_returns(&mut self) -> bool {
        let mut changed = false;
        for block in self.cfg.node_weights_mut() {
            for index in 1..block.instructions.len() {
                let Some(return_value) = return_value_name(&block.instructions[index].instruction)
                else {
                    continue;
                };
                let Instruction::Copy(copy) = &block.instructions[index - 1].instruction else {
                    continue;
                };
                if operand_name(&copy.dst).as_deref() != Some(return_value.as_str()) {
                    continue;
                }
                block.instructions[index].instruction = Instruction::Return(ReturnInstruction {
                    value: Some(copy.src.clone()),
                });
                changed = true;
            }
        }
        changed
    }

    fn simplify_branches(&mut self, target_word_bits: u32) -> bool {
        let labels = label_nodes(&self.cfg);
        let mut changed = false;
        changed |= self.rewrite_jump_chains(&labels);
        changed |= self.simplify_constant_conditional_jumps(&labels, target_word_bits);
        changed |= self.invert_fallthrough_conditional_jumps();
        changed |= self.remove_redundant_fallthrough_jumps();
        changed
    }

    fn rewrite_jump_chains(&mut self, labels: &HashMap<String, NodeIndex>) -> bool {
        let mut changed = false;
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
                changed = true;
            }
        }
        changed
    }

    fn simplify_constant_conditional_jumps(
        &mut self,
        labels: &HashMap<String, NodeIndex>,
        target_word_bits: u32,
    ) -> bool {
        let mut changed = false;
        let nodes = self.cfg.node_indices().collect::<Vec<_>>();
        for node in nodes {
            let mut constants = HashMap::new();
            let mut replacements = Vec::new();
            for (index, inst) in self.cfg[node].instructions.iter().enumerate() {
                match &inst.instruction {
                    Instruction::FunctionCall(_) => {
                        constants.clear();
                    }
                    Instruction::ConditionalJump(jump) => {
                        if let Some(taken) = evaluate_condition(jump, &constants, target_word_bits)
                        {
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
                changed = true;
            }
        }
        changed
    }

    fn invert_fallthrough_conditional_jumps(&mut self) -> bool {
        let mut changed = false;
        for range in self.cfg.functions.clone() {
            let nodes = self.cfg.function_nodes(&range).collect::<Vec<_>>();
            for window in nodes.windows(2) {
                let [node, next] = window else {
                    continue;
                };
                let next_label = self.cfg[*next].label.clone();
                if self.cfg[*node].instructions.len() < 2 {
                    continue;
                }
                let jump_index = self.cfg[*node].instructions.len() - 1;
                let cond_index = jump_index - 1;
                let Some((operator, target, fallthrough_target)) = self.cfg[*node]
                    .instructions
                    .get(cond_index)
                    .and_then(|conditional| {
                        self.cfg[*node]
                            .instructions
                            .get(jump_index)
                            .and_then(|jump| match (&conditional.instruction, &jump.instruction) {
                                (
                                    Instruction::ConditionalJump(conditional),
                                    Instruction::Jump(jump),
                                ) => Some((
                                    conditional.operator,
                                    conditional.target.clone(),
                                    jump.target.clone(),
                                )),
                                _ => None,
                            })
                    })
                else {
                    continue;
                };
                let Some(inverted) = invert_operator(operator) else {
                    continue;
                };
                if label_operand(&target) != Some(next_label.as_str()) {
                    continue;
                }
                let Instruction::ConditionalJump(jump) =
                    &mut self.cfg[*node].instructions[cond_index].instruction
                else {
                    continue;
                };
                jump.operator = inverted;
                jump.target = fallthrough_target;
                self.cfg[*node].instructions.remove(jump_index);
                changed = true;
            }
        }
        changed
    }

    fn remove_redundant_fallthrough_jumps(&mut self) -> bool {
        let mut changed = false;
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
                    changed = true;
                }
            }
        }
        changed
    }

    fn cleanup_cfg(&mut self) -> bool {
        let included = self.reachable_nodes();
        let original_node_count = self.cfg.node_count();
        let mut new_cfg = ControlFlowGraph::new();
        let mut node_map = HashMap::new();
        let mut changed = included.len() != original_node_count;

        for range in self.cfg.functions.clone() {
            let nodes = self
                .cfg
                .function_nodes(&range)
                .filter(|node| included.contains(node))
                .collect::<Vec<_>>();
            if nodes.is_empty() {
                continue;
            }

            let mut current_new = None;
            let mut previous_old = None;
            for old_node in nodes {
                if let (Some(prev_old), Some(new_node)) = (previous_old, current_new) {
                    if can_merge_blocks(&self.cfg, &included, prev_old, old_node) {
                        let next_label = self.cfg[old_node].label.clone();
                        remove_trailing_jump_to(&mut new_cfg[new_node], next_label.as_str());
                        let mut block = clean_block_clone(&self.cfg[old_node]);
                        new_cfg
                            .node_weight_mut(new_node)
                            .expect("new CFG node should exist")
                            .instructions
                            .append(&mut block.instructions);
                        node_map.insert(old_node, new_node);
                        previous_old = Some(old_node);
                        changed = true;
                        continue;
                    }
                }

                let is_function_entry = current_new.is_none();
                let new_node = new_cfg.block(self.cfg[old_node].label.clone(), is_function_entry);
                new_cfg[new_node] = clean_block_clone(&self.cfg[old_node]);
                node_map.insert(old_node, new_node);
                current_new = Some(new_node);
                previous_old = Some(old_node);
            }
            new_cfg.end_function();
        }

        for edge in self.cfg.edge_references() {
            let Some(source) = node_map.get(&edge.source()).copied() else {
                continue;
            };
            let Some(target) = node_map.get(&edge.target()).copied() else {
                continue;
            };
            if source != target {
                ensure_edge(&mut new_cfg, source, target);
            }
        }

        if changed {
            self.cfg = new_cfg;
        }
        changed
    }

    fn reachable_nodes(&self) -> HashSet<NodeIndex> {
        let mut reachable = HashSet::new();
        for range in &self.cfg.functions {
            let mut worklist = VecDeque::from([range.start]);
            while let Some(node) = worklist.pop_front() {
                if !range_contains_node(range, node) || !reachable.insert(node) {
                    continue;
                }
                for successor in self.cfg.neighbors(node) {
                    worklist.push_back(successor);
                }
            }
        }
        reachable
    }

    fn eliminate_dead_code(&mut self) -> bool {
        analyze_liveness(&mut self.cfg);
        let mut changed = false;
        for block in self.cfg.node_weights_mut() {
            block.instructions.retain(|inst| {
                if is_redundant_copy(&inst.instruction) {
                    changed = true;
                    return false;
                }
                if is_dead_temporary_write(&inst.instruction, &inst.live.output) {
                    changed = true;
                    return false;
                }
                true
            });
        }
        changed
    }
}

#[derive(Default)]
struct KnownValues {
    constants: HashMap<String, Lit>,
    copies: HashMap<String, Operand>,
}

impl KnownValues {
    fn clear(&mut self) {
        self.constants.clear();
        self.copies.clear();
    }

    fn resolve(&self, operand: &Operand) -> Operand {
        let mut current = operand.clone();
        let mut seen = HashSet::new();
        while let Some(name) = operand_name(&current) {
            if !seen.insert(name.clone()) {
                break;
            }
            if let Some(lit) = self.constants.get(name.as_str()) {
                return Operand::Literal(lit.clone());
            }
            let Some(copy) = self.copies.get(name.as_str()) else {
                break;
            };
            current = copy.clone();
        }
        current
    }

    fn invalidate_writes(&mut self, written: &HashSet<String>) {
        self.constants.retain(|name, _| !written.contains(name));
        self.copies.retain(|name, operand| {
            !written.contains(name)
                && operand_name(operand).is_none_or(|source| !written.contains(&source))
        });
    }

    fn record(&mut self, instruction: &Instruction) {
        let Instruction::Copy(copy) = instruction else {
            return;
        };
        let Some(dst) = operand_name(&copy.dst) else {
            return;
        };
        match self.resolve(&copy.src) {
            Operand::Literal(lit) => {
                self.constants.insert(dst.clone(), lit);
                self.copies.remove(dst.as_str());
            }
            Operand::Variable(_) | Operand::Temporary(_) => {
                if operand_name(&copy.src).as_deref() != Some(dst.as_str()) {
                    self.copies.insert(dst.clone(), copy.src.clone());
                }
                self.constants.remove(dst.as_str());
            }
            Operand::Label(_) | Operand::Placeholder => {
                self.constants.remove(dst.as_str());
                self.copies.remove(dst.as_str());
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ExpressionKey {
    operator: &'static str,
    operands: Vec<String>,
}

fn substitute_instruction_reads(instruction: &mut Instruction, values: &KnownValues) -> bool {
    match instruction {
        Instruction::Assignment(inst) => {
            let mut changed = false;
            if let Some(left) = &mut inst.left {
                changed |= substitute_operand(left, values);
            }
            changed |= substitute_operand(&mut inst.right, values);
            changed
        }
        Instruction::Copy(inst) => substitute_operand(&mut inst.src, values),
        Instruction::ConditionalJump(inst) => {
            let mut changed = false;
            if let Some(left) = &mut inst.left {
                changed |= substitute_operand(left, values);
            }
            changed |= substitute_operand(&mut inst.right, values);
            changed
        }
        Instruction::Parameter(inst) => substitute_operand(&mut inst.param, values),
        Instruction::Return(inst) => inst
            .value
            .as_mut()
            .is_some_and(|value| substitute_operand(value, values)),
        Instruction::FunctionCall(_)
        | Instruction::Function(_)
        | Instruction::EndFunction(_)
        | Instruction::Extern(_)
        | Instruction::Jump(_) => false,
    }
}

fn substitute_operand(operand: &mut Operand, values: &KnownValues) -> bool {
    let replacement = values.resolve(operand);
    if operand_signature(operand) == operand_signature(&replacement) {
        return false;
    }
    *operand = replacement;
    true
}

fn simplify_instruction(instruction: &mut Instruction, target_word_bits: u32) -> bool {
    let Instruction::Assignment(assignment) = instruction else {
        return false;
    };
    let replacement = fold_assignment(assignment, target_word_bits)
        .map(Operand::Literal)
        .or_else(|| simplify_algebraic_assignment(assignment));
    let Some(src) = replacement else {
        return false;
    };
    *instruction = Instruction::Copy(CopyInstruction {
        dst: assignment.target.clone(),
        src,
    });
    true
}

fn simplify_algebraic_assignment(inst: &AssignmentInstruction) -> Option<Operand> {
    match (inst.left.as_ref(), inst.operator, &inst.right) {
        (None, operators::UNARY_PLUS, right) => Some(right.clone()),
        (Some(left), operators::PLUS, right) if is_integer_literal(right, 0) => Some(left.clone()),
        (Some(left), operators::PLUS, right) if is_integer_literal(left, 0) => Some(right.clone()),
        (Some(left), operators::MINUS, right) if is_integer_literal(right, 0) => Some(left.clone()),
        (Some(_), operators::MINUS, right) if same_operand(inst.left.as_ref()?, right) => {
            Some(Operand::Literal(Lit::Integer(0)))
        }
        (Some(left), operators::MULTIPLY, right) if is_integer_literal(right, 1) => {
            Some(left.clone())
        }
        (Some(left), operators::MULTIPLY, right) if is_integer_literal(left, 1) => {
            Some(right.clone())
        }
        (Some(_), operators::MULTIPLY, right) if is_integer_literal(right, 0) => {
            Some(Operand::Literal(Lit::Integer(0)))
        }
        (Some(left), operators::MULTIPLY, _) if is_integer_literal(left, 0) => {
            Some(Operand::Literal(Lit::Integer(0)))
        }
        (Some(left), operators::DIVIDE, right) if is_integer_literal(right, 1) => {
            Some(left.clone())
        }
        (Some(_), operators::REMAINDER, right) if is_integer_literal(right, 1) => {
            Some(Operand::Literal(Lit::Integer(0)))
        }
        (Some(left), operators::LOGICAL_AND, right) if is_boolean_literal(right, true) => {
            Some(left.clone())
        }
        (Some(left), operators::LOGICAL_AND, right) if is_boolean_literal(left, true) => {
            Some(right.clone())
        }
        (Some(_), operators::LOGICAL_AND, right) if is_boolean_literal(right, false) => {
            Some(Operand::Literal(Lit::Boolean(false)))
        }
        (Some(left), operators::LOGICAL_AND, _) if is_boolean_literal(left, false) => {
            Some(Operand::Literal(Lit::Boolean(false)))
        }
        (Some(left), operators::LOGICAL_OR, right) if is_boolean_literal(right, false) => {
            Some(left.clone())
        }
        (Some(left), operators::LOGICAL_OR, right) if is_boolean_literal(left, false) => {
            Some(right.clone())
        }
        (Some(_), operators::LOGICAL_OR, right) if is_boolean_literal(right, true) => {
            Some(Operand::Literal(Lit::Boolean(true)))
        }
        (Some(left), operators::LOGICAL_OR, _) if is_boolean_literal(left, true) => {
            Some(Operand::Literal(Lit::Boolean(true)))
        }
        _ => None,
    }
}

fn fold_assignment(inst: &AssignmentInstruction, target_word_bits: u32) -> Option<Lit> {
    match (inst.left.as_ref(), inst.operator, &inst.right) {
        (None, operators::UNARY_PLUS, Operand::Literal(Lit::Integer(value))) => {
            target_word_value(*value, target_word_bits).map(Lit::Integer)
        }
        (None, operators::UNARY_MINUS, Operand::Literal(Lit::Integer(value))) => value
            .checked_neg()
            .and_then(|value| target_word_value(value, target_word_bits))
            .map(Lit::Integer),
        (None, operators::LOGICAL_NOT, Operand::Literal(Lit::Boolean(value))) => {
            Some(Lit::Boolean(!value))
        }
        (Some(Operand::Literal(left)), operator, Operand::Literal(right)) => {
            fold_binary_literals(left, operator, right, target_word_bits)
        }
        _ => None,
    }
}

fn fold_binary_literals(
    left: &Lit,
    operator: &str,
    right: &Lit,
    target_word_bits: u32,
) -> Option<Lit> {
    match (left, operator, right) {
        (Lit::Integer(left), operators::PLUS, Lit::Integer(right)) => {
            fold_integer_binary(*left, *right, target_word_bits, isize::checked_add)
                .map(Lit::Integer)
        }
        (Lit::Integer(left), operators::MINUS, Lit::Integer(right)) => {
            fold_integer_binary(*left, *right, target_word_bits, isize::checked_sub)
                .map(Lit::Integer)
        }
        (Lit::Integer(left), operators::MULTIPLY, Lit::Integer(right)) => {
            fold_integer_binary(*left, *right, target_word_bits, isize::checked_mul)
                .map(Lit::Integer)
        }
        (Lit::Integer(left), operators::DIVIDE, Lit::Integer(right)) if *right != 0 => {
            fold_integer_binary(*left, *right, target_word_bits, isize::checked_div)
                .map(Lit::Integer)
        }
        (Lit::Integer(left), operators::REMAINDER, Lit::Integer(right)) if *right != 0 => {
            fold_integer_binary(*left, *right, target_word_bits, isize::checked_rem)
                .map(Lit::Integer)
        }
        (Lit::Integer(left), operator, Lit::Integer(right))
            if target_word_value(*left, target_word_bits).is_some()
                && target_word_value(*right, target_word_bits).is_some() =>
        {
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

fn fold_integer_binary(
    left: isize,
    right: isize,
    target_word_bits: u32,
    op: fn(isize, isize) -> Option<isize>,
) -> Option<isize> {
    target_word_value(left, target_word_bits)?;
    target_word_value(right, target_word_bits)?;
    op(left, right).and_then(|value| target_word_value(value, target_word_bits))
}

fn target_word_value(value: isize, target_word_bits: u32) -> Option<isize> {
    if target_word_bits == 0 {
        return None;
    }

    let bits = target_word_bits.min(isize::BITS);
    let value = value as i128;
    let (min, max) = if bits == isize::BITS {
        (isize::MIN as i128, isize::MAX as i128)
    } else {
        let limit = 1_i128 << (bits - 1);
        (-limit, limit - 1)
    };

    (min..=max).contains(&value).then_some(value as isize)
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
    target_word_bits: u32,
) -> Option<bool> {
    let right = literal_for_operand(&jump.right, constants)?;
    if let Some(left) = &jump.left {
        let left = literal_for_operand(left, constants)?;
        fold_binary_literals(left, jump.operator, right, target_word_bits).and_then(|lit| match lit
        {
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

fn expression_key(inst: &AssignmentInstruction) -> Option<ExpressionKey> {
    if operand_name(&inst.target).is_none() || !is_common_subexpression_operator(inst) {
        return None;
    }
    let mut operands = if let Some(left) = &inst.left {
        vec![operand_signature(left), operand_signature(&inst.right)]
    } else {
        vec![operand_signature(&inst.right)]
    };
    if is_commutative_operator(inst.operator) {
        operands.sort();
    }
    Some(ExpressionKey {
        operator: inst.operator,
        operands,
    })
}

fn is_common_subexpression_operator(inst: &AssignmentInstruction) -> bool {
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

fn is_commutative_operator(operator: &str) -> bool {
    matches!(
        operator,
        operators::PLUS
            | operators::MULTIPLY
            | operators::EQUALS
            | operators::NOT_EQUALS
            | operators::LOGICAL_AND
            | operators::LOGICAL_OR
    )
}

fn invalidate_expressions(
    expressions: &mut HashMap<ExpressionKey, Operand>,
    written: &HashSet<String>,
) {
    expressions.retain(|key, value| {
        let value_written = operand_name(value).is_some_and(|name| written.contains(&name));
        let input_written = key.operands.iter().any(|operand| {
            written
                .iter()
                .any(|name| operand.as_str() == named_operand_signature(name).as_str())
        });
        !value_written && !input_written
    });
}

fn clean_block_clone(block: &InstructionBlock) -> InstructionBlock {
    let mut block = block.clone();
    block.live = Default::default();
    for inst in &mut block.instructions {
        inst.live = Default::default();
    }
    block
}

fn can_merge_blocks(
    cfg: &ControlFlowGraph,
    included: &HashSet<NodeIndex>,
    prev: NodeIndex,
    next: NodeIndex,
) -> bool {
    let outgoing = cfg
        .edges_directed(prev, Direction::Outgoing)
        .filter(|edge| included.contains(&edge.target()))
        .map(|edge| edge.target())
        .collect::<Vec<_>>();
    if outgoing.as_slice() != [next] {
        return false;
    }

    let incoming = cfg
        .edges_directed(next, Direction::Incoming)
        .filter(|edge| included.contains(&edge.source()))
        .map(|edge| edge.source())
        .collect::<Vec<_>>();
    if incoming.as_slice() != [prev] {
        return false;
    }

    match cfg[prev].instructions.last().map(|inst| &inst.instruction) {
        Some(Instruction::ConditionalJump(_) | Instruction::Return(_)) => false,
        Some(Instruction::Jump(jump)) => {
            label_operand(&jump.target) == Some(cfg[next].label.as_str())
        }
        _ => true,
    }
}

fn remove_trailing_jump_to(block: &mut InstructionBlock, label: &str) -> bool {
    let Some(Instruction::Jump(jump)) = block.instructions.last().map(|inst| &inst.instruction)
    else {
        return false;
    };
    if label_operand(&jump.target) == Some(label) {
        block.instructions.pop();
        true
    } else {
        false
    }
}

fn range_contains_node(
    range: &crate::generators::tac::instructions::FunctionRange,
    node: NodeIndex,
) -> bool {
    range.start.index() <= node.index() && node.index() <= range.end.index()
}

fn is_dead_temporary_write(instruction: &Instruction, live_out: &HashSet<String>) -> bool {
    match instruction {
        Instruction::Assignment(assignment) => {
            is_dead_temporary(&assignment.target, live_out)
                && assignment_is_safe_to_eliminate(assignment)
        }
        Instruction::Copy(copy) => is_dead_temporary(&copy.dst, live_out),
        _ => false,
    }
}

fn assignment_is_safe_to_eliminate(assignment: &AssignmentInstruction) -> bool {
    matches!(
        (assignment.left.as_ref(), assignment.operator),
        (
            None,
            operators::UNARY_PLUS | operators::UNARY_MINUS | operators::LOGICAL_NOT
        ) | (Some(_), operators::PLUS)
            | (Some(_), operators::MINUS)
            | (Some(_), operators::MULTIPLY)
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

fn is_dead_temporary(operand: &Operand, live_out: &HashSet<String>) -> bool {
    matches!(operand, Operand::Temporary(_))
        && operand_name(operand).is_some_and(|name| !live_out.contains(&name))
}

fn is_redundant_copy(instruction: &Instruction) -> bool {
    let Instruction::Copy(copy) = instruction else {
        return false;
    };
    same_operand(&copy.dst, &copy.src)
}

fn return_value_name(instruction: &Instruction) -> Option<String> {
    let Instruction::Return(return_) = instruction else {
        return None;
    };
    return_.value.as_ref().and_then(operand_name)
}

fn operand_name(operand: &Operand) -> Option<String> {
    match operand {
        Operand::Variable(name) => Some(name.clone()),
        Operand::Temporary(label) => Some(label.to_string()),
        Operand::Literal(_) | Operand::Label(_) | Operand::Placeholder => None,
    }
}

fn operand_signature(operand: &Operand) -> String {
    match operand {
        Operand::Variable(name) => named_operand_signature(name),
        Operand::Temporary(label) => named_operand_signature(label.to_string().as_str()),
        Operand::Literal(lit) => format!("lit:{lit}"),
        Operand::Label(label) => format!("label:{label}"),
        Operand::Placeholder => String::from("placeholder"),
    }
}

fn named_operand_signature(name: &str) -> String {
    format!("name:{name}")
}

fn same_operand(left: &Operand, right: &Operand) -> bool {
    operand_signature(left) == operand_signature(right)
}

fn is_integer_literal(operand: &Operand, expected: isize) -> bool {
    matches!(operand, Operand::Literal(Lit::Integer(value)) if *value == expected)
}

fn is_boolean_literal(operand: &Operand, expected: bool) -> bool {
    matches!(operand, Operand::Literal(Lit::Boolean(value)) if *value == expected)
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
        let Some(next) = sole_successor_target(cfg, *node) else {
            return Some(current);
        };
        current = next;
    }
    Some(current)
}

fn sole_successor_target(cfg: &ControlFlowGraph, node: NodeIndex) -> Option<String> {
    if let Some(target) = sole_jump_target(&cfg[node]) {
        return Some(target);
    }
    if !cfg[node].instructions.is_empty() {
        return None;
    }
    let successors = cfg.neighbors(node).collect::<Vec<_>>();
    let [successor] = successors.as_slice() else {
        return None;
    };
    Some(cfg[*successor].label.clone())
}

fn sole_jump_target(block: &InstructionBlock) -> Option<String> {
    let [inst] = block.instructions.as_slice() else {
        return None;
    };
    let Instruction::Jump(jump) = &inst.instruction else {
        return None;
    };
    label_operand(&jump.target).map(String::from)
}

fn invert_operator(operator: &str) -> Option<&'static str> {
    match operator {
        operators::EQUALS => Some(operators::NOT_EQUALS),
        operators::NOT_EQUALS => Some(operators::EQUALS),
        operators::LESS => Some(operators::GREATER_EQUALS),
        operators::LESS_EQUALS => Some(operators::GREATER),
        operators::GREATER => Some(operators::LESS_EQUALS),
        operators::GREATER_EQUALS => Some(operators::LESS),
        _ => None,
    }
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
