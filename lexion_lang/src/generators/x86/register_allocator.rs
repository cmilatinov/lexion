use crate::ast::types::{FunctionType, Type, TypeCollection, TypeKind};
use crate::diagnostic::DiagnosticConsumer;
use crate::generators::tac::instructions::{
    CodeLocation, ControlFlowGraph, FunctionCallInstruction, FunctionRange, Instruction,
    LivenessInterval, Operand,
};
use crate::generators::x86::calling_convention::{CallingConvention, Location, StackOffset};
use crate::generators::x86::{SystemV64, X86Target};
use crate::pipeline::PipelineStage;
use crate::symbol_table::SymbolTableGraph;
use iced_x86::Register;
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone)]
pub struct AssignedLivenessInterval {
    interval: LivenessInterval,
    location: Location,
    constraints: Vec<AbiLocationConstraint>,
}

impl AssignedLivenessInterval {
    pub fn interval(&self) -> &LivenessInterval {
        &self.interval
    }

    pub fn location(&self) -> &Location {
        &self.location
    }

    pub fn constraints(&self) -> &[AbiLocationConstraint] {
        &self.constraints
    }
}

#[derive(Debug, Clone)]
pub struct AbiLocationConstraint {
    location: CodeLocation,
    role: AbiLocationRole,
    abi_location: Location,
}

impl AbiLocationConstraint {
    pub fn location(&self) -> CodeLocation {
        self.location
    }

    pub fn role(&self) -> &AbiLocationRole {
        &self.role
    }

    pub fn abi_location(&self) -> &Location {
        &self.abi_location
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbiLocationRole {
    FunctionParameter { index: usize },
    CallArgument { function: String, index: usize },
    CallReturn { function: String },
    ReturnValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HardRegisterConstraint {
    None,
    Register(Register),
    Conflict,
}

pub struct LinearRegisterAllocator<'a> {
    registers: Vec<Register>,
    active: Vec<AssignedLivenessInterval>,
    available: VecDeque<Register>,
    _cfg: &'a ControlFlowGraph,
    stack_offset: usize,
}

impl<'a> PipelineStage for LinearRegisterAllocator<'a> {
    type Input = (&'a ControlFlowGraph, Vec<Register>);
    type Options = HashMap<FunctionRange, Vec<LivenessInterval>>;
    type Output = HashMap<FunctionRange, Vec<AssignedLivenessInterval>>;

    fn new((cfg, registers): Self::Input) -> Self {
        Self {
            available: VecDeque::from_iter(registers.iter().cloned()),
            registers,
            active: Default::default(),
            _cfg: cfg,
            stack_offset: 0,
        }
    }

    fn exec(
        mut self,
        _diag: &mut dyn DiagnosticConsumer,
        intervals: Self::Options,
    ) -> Option<Self::Output> {
        Some(
            intervals
                .into_iter()
                .map(|(func, intervals)| {
                    let result = (func, self.linear_scan(intervals));
                    self.active = Default::default();
                    self.available = self.registers.iter().cloned().collect();
                    self.stack_offset = 0;
                    result
                })
                .collect(),
        )
    }
}

impl<'a> LinearRegisterAllocator<'a> {
    fn linear_scan(
        &mut self,
        mut intervals: Vec<LivenessInterval>,
    ) -> Vec<AssignedLivenessInterval> {
        sort_intervals(&mut intervals);

        let mut assigned = Vec::new();

        for interval in intervals {
            let start = interval.span.start;
            self.expire_old_intervals(start);

            if let Some(reg) = self.available.pop_front() {
                let assigned_interval = AssignedLivenessInterval {
                    interval,
                    location: Location::Register(reg),
                    constraints: Vec::new(),
                };
                self.insert_active(assigned_interval.clone());
                assigned.push(assigned_interval);
            } else {
                self.spill(interval, &mut assigned);
            }
        }

        assigned
    }

    fn insert_active(&mut self, assigned: AssignedLivenessInterval) {
        let insert_idx = self
            .active
            .binary_search_by_key(&assigned.interval.span.start, |i| i.interval.span.start)
            .unwrap_or_else(|idx| idx);
        self.active.insert(insert_idx, assigned);
    }

    fn expire_old_intervals(&mut self, start: CodeLocation) {
        self.active.retain(|active| {
            if active.interval.span.end > start {
                true
            } else {
                if let Location::Register(reg) = active.location {
                    self.available.push_front(reg);
                }
                false
            }
        });
    }

    fn spill(&mut self, interval: LivenessInterval, assigned: &mut Vec<AssignedLivenessInterval>) {
        if let Some((idx, spill_end)) = self
            .active
            .iter()
            .enumerate()
            .max_by_key(|(_, i)| i.interval.span.end)
            .map(|(idx, interval)| (idx, interval.interval.span.end))
        {
            if spill_end > interval.span.end {
                let spilled = self.active.remove(idx);
                let reg = spilled.location.register().unwrap();
                let spill_location = self.next_spill_location();
                if let Some(assigned_spill) = assigned
                    .iter_mut()
                    .find(|assigned| Self::same_interval(assigned.interval(), &spilled.interval))
                {
                    assigned_spill.location = spill_location;
                } else {
                    assigned.push(AssignedLivenessInterval {
                        interval: spilled.interval,
                        location: spill_location,
                        constraints: spilled.constraints,
                    });
                }

                let new_assigned = AssignedLivenessInterval {
                    interval,
                    location: Location::Register(reg),
                    constraints: Vec::new(),
                };
                self.insert_active(new_assigned.clone());
                assigned.push(new_assigned);
                return;
            }
        }

        assigned.push(AssignedLivenessInterval {
            interval,
            location: self.next_spill_location(),
            constraints: Vec::new(),
        });
    }

    fn next_spill_location(&mut self) -> Location {
        let location = Location::Stack(StackOffset(self.stack_offset));
        self.stack_offset += 1;
        location
    }

    fn same_interval(left: &LivenessInterval, right: &LivenessInterval) -> bool {
        left.variable == right.variable && left.span == right.span
    }
}

pub struct AbiRegisterAllocator<'a, C = SystemV64> {
    cfg: &'a ControlFlowGraph,
    types: &'a TypeCollection,
    symbols: &'a SymbolTableGraph,
    target: X86Target<C>,
    active: Vec<AssignedLivenessInterval>,
    available: VecDeque<Register>,
    registers: Vec<Register>,
    stack_offset: usize,
}

impl<'a> PipelineStage for AbiRegisterAllocator<'a, SystemV64> {
    type Input = (
        &'a ControlFlowGraph,
        &'a TypeCollection,
        &'a SymbolTableGraph,
        X86Target<SystemV64>,
    );
    type Options = HashMap<FunctionRange, Vec<LivenessInterval>>;
    type Output = HashMap<FunctionRange, Vec<AssignedLivenessInterval>>;

    fn new((cfg, types, symbols, target): Self::Input) -> Self {
        let mut registers = Vec::new();
        registers.extend(target.calling_convention().caller_saved().iter().copied());
        registers.extend(
            target
                .calling_convention()
                .callee_saved()
                .iter()
                .copied()
                .filter(|reg| *reg != Register::RBP),
        );
        registers.extend(
            target
                .calling_convention()
                .call_clobbered()
                .iter()
                .copied()
                .filter(is_xmm_register),
        );
        Self {
            cfg,
            types,
            symbols,
            target,
            active: Default::default(),
            available: registers.iter().copied().collect(),
            registers,
            stack_offset: 0,
        }
    }

    fn exec(
        mut self,
        _diag: &mut dyn DiagnosticConsumer,
        intervals: Self::Options,
    ) -> Option<Self::Output> {
        Some(
            intervals
                .into_iter()
                .map(|(func, intervals)| {
                    let result = (func, self.linear_scan_function(func, intervals));
                    self.reset_function();
                    result
                })
                .collect(),
        )
    }
}

impl<'a, C: CallingConvention> AbiRegisterAllocator<'a, C> {
    fn linear_scan_function(
        &mut self,
        range: FunctionRange,
        mut intervals: Vec<LivenessInterval>,
    ) -> Vec<AssignedLivenessInterval> {
        sort_intervals(&mut intervals);
        let call_locations = self.call_locations(range);
        let constraints = self.location_constraints(range);
        let mut assigned = Vec::new();

        for interval in intervals {
            let start = interval.span.start;
            self.expire_old_intervals(start);

            let interval_constraints = constraints
                .get(interval.variable.as_str())
                .cloned()
                .unwrap_or_default();
            let crosses_call = call_locations
                .iter()
                .any(|call| interval.span.start < *call && *call < interval.span.end);
            let allowed = self.allowed_registers(range, &interval, crosses_call);

            let location = self.allocate_location(&allowed, &interval_constraints, &mut assigned);
            let assigned_interval = AssignedLivenessInterval {
                interval,
                location,
                constraints: interval_constraints,
            };
            if assigned_interval.location.register().is_some() {
                self.insert_active(assigned_interval.clone());
            }
            assigned.push(assigned_interval);
        }

        assigned
    }

    fn reset_function(&mut self) {
        self.active = Default::default();
        self.available = self.registers.iter().copied().collect();
        self.stack_offset = 0;
    }

    fn allowed_registers(
        &self,
        range: FunctionRange,
        interval: &LivenessInterval,
        crosses_call: bool,
    ) -> Vec<Register> {
        self.registers_for_kind(self.interval_type(range, interval), crosses_call)
    }

    fn registers_for_kind(&self, kind: Option<TypeKind>, crosses_call: bool) -> Vec<Register> {
        if matches!(
            kind,
            Some(TypeKind::Float | TypeKind::Double | TypeKind::Vector)
        ) {
            return if crosses_call {
                Vec::new()
            } else {
                self.registers
                    .iter()
                    .copied()
                    .filter(is_xmm_register)
                    .collect()
            };
        }
        if crosses_call {
            self.target
                .calling_convention()
                .callee_saved()
                .iter()
                .copied()
                .filter(|reg| *reg != Register::RBP)
                .collect()
        } else {
            self.registers
                .iter()
                .copied()
                .filter(|register| !is_xmm_register(register))
                .collect()
        }
    }

    fn interval_type(&self, range: FunctionRange, interval: &LivenessInterval) -> Option<TypeKind> {
        self.symbols
            .lookup_function(&self.cfg[range.start].label, interval.variable.as_str())
            .and_then(|(_, _, entry)| entry.var_type)
            .map(|ty| self.types.kind(ty))
    }

    fn allocate_location(
        &mut self,
        allowed: &[Register],
        constraints: &[AbiLocationConstraint],
        assigned: &mut Vec<AssignedLivenessInterval>,
    ) -> Location {
        match hard_register_constraint(constraints) {
            HardRegisterConstraint::Register(register) => {
                if !allowed.contains(&register) {
                    return self.next_spill_location();
                }
                if self.take_available_register(register).is_some()
                    || self.spill_active_for_register(register, assigned)
                {
                    Location::Register(register)
                } else {
                    self.next_spill_location()
                }
            }
            HardRegisterConstraint::Conflict => self.next_spill_location(),
            HardRegisterConstraint::None => self
                .take_next_allowed_register(allowed)
                .map(Location::Register)
                .unwrap_or_else(|| self.next_spill_location()),
        }
    }

    fn take_next_allowed_register(&mut self, allowed: &[Register]) -> Option<Register> {
        for register in allowed {
            if let Some(register) = self.take_available_register(*register) {
                return Some(register);
            }
        }
        None
    }

    fn take_available_register(&mut self, register: Register) -> Option<Register> {
        self.available
            .iter()
            .position(|available| *available == register)
            .and_then(|position| self.available.remove(position))
    }

    fn spill_active_for_register(
        &mut self,
        register: Register,
        assigned: &mut Vec<AssignedLivenessInterval>,
    ) -> bool {
        let Some(position) = self
            .active
            .iter()
            .position(|active| active.location.register() == Some(register))
        else {
            return false;
        };
        if active_requires_register(&self.active[position], register) {
            return false;
        }

        let spilled = self.active.remove(position);
        let spill_location = self.next_spill_location();
        if let Some(assigned_spill) = assigned.iter_mut().find(|assigned| {
            LinearRegisterAllocator::same_interval(assigned.interval(), &spilled.interval)
        }) {
            assigned_spill.location = spill_location;
        } else {
            assigned.push(AssignedLivenessInterval {
                interval: spilled.interval,
                location: spill_location,
                constraints: spilled.constraints,
            });
        }
        true
    }

    fn insert_active(&mut self, assigned: AssignedLivenessInterval) {
        let insert_idx = self
            .active
            .binary_search_by_key(&assigned.interval.span.start, |i| i.interval.span.start)
            .unwrap_or_else(|idx| idx);
        self.active.insert(insert_idx, assigned);
    }

    fn expire_old_intervals(&mut self, start: CodeLocation) {
        self.active.retain(|active| {
            if active.interval.span.end > start {
                true
            } else {
                if let Location::Register(reg) = active.location {
                    self.available.push_back(reg);
                }
                false
            }
        });
    }

    fn next_spill_location(&mut self) -> Location {
        let location = Location::Stack(StackOffset(self.stack_offset));
        self.stack_offset += 1;
        location
    }

    fn call_locations(&self, range: FunctionRange) -> Vec<CodeLocation> {
        self.cfg
            .function_nodes(&range)
            .flat_map(|block| {
                self.cfg[block].instructions.iter().enumerate().filter_map(
                    move |(instruction, instance)| {
                        matches!(instance.instruction, Instruction::FunctionCall(_))
                            .then_some(CodeLocation::new(block, instruction))
                    },
                )
            })
            .collect()
    }

    fn location_constraints(
        &self,
        range: FunctionRange,
    ) -> HashMap<String, Vec<AbiLocationConstraint>> {
        let mut constraints = HashMap::new();
        self.add_function_parameter_constraints(range, &mut constraints);
        self.add_call_constraints(range, &mut constraints);
        self.add_return_constraints(range, &mut constraints);
        constraints
    }

    fn add_function_parameter_constraints(
        &self,
        range: FunctionRange,
        constraints: &mut HashMap<String, Vec<AbiLocationConstraint>>,
    ) {
        let Some(signature) = self.function_signature(&self.cfg[range.start].label) else {
            return;
        };
        let locations = self
            .target
            .calling_convention()
            .assign_args(self.types, 0, signature);
        let Some(Instruction::Function(function)) = self.cfg[range.start]
            .instructions
            .first()
            .map(|instance| &instance.instruction)
        else {
            return;
        };

        for (index, (name, abi_location)) in function.params.iter().zip(locations).enumerate() {
            constraints
                .entry(name.clone())
                .or_default()
                .push(AbiLocationConstraint {
                    location: CodeLocation::new(range.start, 0),
                    role: AbiLocationRole::FunctionParameter { index },
                    abi_location,
                });
        }
    }

    fn add_call_constraints(
        &self,
        range: FunctionRange,
        constraints: &mut HashMap<String, Vec<AbiLocationConstraint>>,
    ) {
        for block in self.cfg.function_nodes(&range) {
            let mut pending_params = Vec::new();
            for (instruction_index, instance) in self.cfg[block].instructions.iter().enumerate() {
                match &instance.instruction {
                    Instruction::Parameter(param) => {
                        pending_params.push((
                            CodeLocation::new(block, instruction_index),
                            param.param.clone(),
                        ));
                    }
                    Instruction::FunctionCall(call) => {
                        let Some(signature) = self.function_call_signature(call) else {
                            pending_params.clear();
                            continue;
                        };
                        let locations = self
                            .target
                            .calling_convention()
                            .assign_args(self.types, 0, signature);
                        pending_params.reverse();
                        for (index, ((location, operand), abi_location)) in
                            pending_params.iter().zip(locations).enumerate()
                        {
                            if let Some(name) = operand_name(operand) {
                                constraints
                                    .entry(name)
                                    .or_default()
                                    .push(AbiLocationConstraint {
                                        location: *location,
                                        role: AbiLocationRole::CallArgument {
                                            function: call.target.to_string(),
                                            index,
                                        },
                                        abi_location,
                                    });
                            }
                        }

                        if let Some(return_target) = &call.return_target {
                            if let (Some(name), Some(abi_location)) = (
                                operand_name(return_target),
                                self.target
                                    .calling_convention()
                                    .assign_ret(self.types, signature),
                            ) {
                                constraints
                                    .entry(name)
                                    .or_default()
                                    .push(AbiLocationConstraint {
                                        location: CodeLocation::new(block, instruction_index),
                                        role: AbiLocationRole::CallReturn {
                                            function: call.target.to_string(),
                                        },
                                        abi_location,
                                    });
                            }
                        }
                        pending_params.clear();
                    }
                    _ => pending_params.clear(),
                }
            }
        }
    }

    fn add_return_constraints(
        &self,
        range: FunctionRange,
        constraints: &mut HashMap<String, Vec<AbiLocationConstraint>>,
    ) {
        let Some(signature) = self.function_signature(&self.cfg[range.start].label) else {
            return;
        };
        let Some(abi_location) = self
            .target
            .calling_convention()
            .assign_ret(self.types, signature)
        else {
            return;
        };

        for block in self.cfg.function_nodes(&range) {
            for (instruction_index, instance) in self.cfg[block].instructions.iter().enumerate() {
                let Instruction::Return(return_) = &instance.instruction else {
                    continue;
                };
                let Some(value) = return_.value.as_ref().and_then(operand_name) else {
                    continue;
                };
                constraints
                    .entry(value)
                    .or_default()
                    .push(AbiLocationConstraint {
                        location: CodeLocation::new(block, instruction_index),
                        role: AbiLocationRole::ReturnValue,
                        abi_location: abi_location.clone(),
                    });
            }
        }
    }

    fn function_signature(&self, function: &str) -> Option<&FunctionType> {
        let (_, _, entry) = self.symbols.lookup_function_entry(function)?;
        let ty = entry.var_type?;
        match self.types.get(self.types.canonicalize(ty))? {
            Type::FunctionType(signature) => Some(signature),
            _ => None,
        }
    }

    fn function_call_signature(&self, inst: &FunctionCallInstruction) -> Option<&FunctionType> {
        let ty = inst.function_type?;
        match self.types.get(self.types.canonicalize(ty))? {
            Type::FunctionType(signature) => Some(signature),
            _ => None,
        }
    }
}

fn sort_intervals(intervals: &mut [LivenessInterval]) {
    intervals.sort_by(|left, right| {
        left.span
            .start
            .cmp(&right.span.start)
            .then_with(|| left.variable.cmp(&right.variable))
            .then_with(|| left.span.end.cmp(&right.span.end))
    });
}

fn active_requires_register(assigned: &AssignedLivenessInterval, register: Register) -> bool {
    matches!(
        hard_register_constraint(assigned.constraints()),
        HardRegisterConstraint::Register(required) if required == register
    )
}

fn hard_register_constraint(constraints: &[AbiLocationConstraint]) -> HardRegisterConstraint {
    // A pair needs two registers atomically; scalar allocation keeps it in its home.
    if constraints
        .iter()
        .any(|constraint| matches!(constraint.abi_location(), Location::Pair { .. }))
    {
        return HardRegisterConstraint::Conflict;
    }
    let mut required = None;
    for register in constraints
        .iter()
        .filter_map(|constraint| constraint.abi_location.register())
    {
        match required {
            Some(existing) if existing != register => return HardRegisterConstraint::Conflict,
            Some(_) => {}
            None => required = Some(register),
        }
    }

    required.map_or(
        HardRegisterConstraint::None,
        HardRegisterConstraint::Register,
    )
}

fn operand_name(operand: &Operand) -> Option<String> {
    match operand {
        Operand::Variable(name) => Some(name.clone()),
        Operand::Temporary(label) => Some(label.to_string()),
        Operand::Literal(_) | Operand::Label(_) | Operand::Placeholder => None,
    }
}

fn is_xmm_register(register: &Register) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generators::tac::instructions::{CodeSpan, ControlFlowGraph};
    use crate::{ast::types::TypeCollection, symbol_table::SymbolTableGraph};
    use lexion_lib::petgraph::graph::NodeIndex;

    #[test]
    fn hard_register_constraint_spills_unconstrained_active_interval() {
        let cfg = ControlFlowGraph::new();
        let types = TypeCollection::default();
        let symbols = SymbolTableGraph::default();
        let mut allocator =
            AbiRegisterAllocator::new((&cfg, &types, &symbols, X86Target::system_v64()));
        allocator.available.clear();

        let active = AssignedLivenessInterval {
            interval: interval("live", 0, 3),
            location: Location::Register(Register::RAX),
            constraints: Vec::new(),
        };
        allocator.insert_active(active.clone());
        let mut assigned = vec![active];

        let location = allocator.allocate_location(
            &[Register::RAX],
            &[register_constraint(Register::RAX)],
            &mut assigned,
        );

        assert_register(&location, Register::RAX);
        assert_stack(assigned[0].location(), 0);
    }

    #[test]
    fn conflicting_hard_register_constraints_spill_interval() {
        let cfg = ControlFlowGraph::new();
        let types = TypeCollection::default();
        let symbols = SymbolTableGraph::default();
        let mut allocator =
            AbiRegisterAllocator::new((&cfg, &types, &symbols, X86Target::system_v64()));

        let location = allocator.allocate_location(
            &[Register::RAX, Register::RDI],
            &[
                register_constraint(Register::RAX),
                register_constraint(Register::RDI),
            ],
            &mut Vec::new(),
        );

        assert_stack(&location, 0);
    }

    #[test]
    fn register_pair_constraint_spills_instead_of_using_a_scalar_register() {
        let cfg = ControlFlowGraph::new();
        let types = TypeCollection::default();
        let symbols = SymbolTableGraph::default();
        let mut allocator =
            AbiRegisterAllocator::new((&cfg, &types, &symbols, X86Target::system_v64()));

        let location = allocator.allocate_location(
            &[Register::RAX, Register::RDX],
            &[pair_constraint(Register::RAX, Register::RDX)],
            &mut Vec::new(),
        );

        assert_stack(&location, 0);
    }

    #[test]
    fn caller_saved_hard_register_constraint_spills_when_disallowed() {
        let cfg = ControlFlowGraph::new();
        let types = TypeCollection::default();
        let symbols = SymbolTableGraph::default();
        let mut allocator =
            AbiRegisterAllocator::new((&cfg, &types, &symbols, X86Target::system_v64()));

        let location = allocator.allocate_location(
            &[Register::RBX, Register::R12],
            &[register_constraint(Register::RDI)],
            &mut Vec::new(),
        );

        assert_stack(&location, 0);
    }

    #[test]
    fn system_v_allocator_includes_xmm_registers() {
        let cfg = ControlFlowGraph::new();
        let types = TypeCollection::default();
        let symbols = SymbolTableGraph::default();
        let allocator =
            AbiRegisterAllocator::new((&cfg, &types, &symbols, X86Target::system_v64()));

        assert!(allocator.registers.contains(&Register::XMM0));
        assert!(allocator.registers.contains(&Register::XMM15));
    }

    #[test]
    fn float_values_use_xmm_registers_and_spill_across_calls() {
        let cfg = ControlFlowGraph::new();
        let types = TypeCollection::default();
        let symbols = SymbolTableGraph::default();
        let allocator =
            AbiRegisterAllocator::new((&cfg, &types, &symbols, X86Target::system_v64()));

        let registers = allocator.registers_for_kind(Some(TypeKind::Float), false);
        assert!(registers.contains(&Register::XMM0));
        assert!(registers.iter().all(is_xmm_register));
        assert!(allocator
            .registers_for_kind(Some(TypeKind::Float), true)
            .is_empty());

        let registers = allocator.registers_for_kind(Some(TypeKind::Integer), false);
        assert!(registers.contains(&Register::RAX));
        assert!(registers.iter().all(|register| !is_xmm_register(register)));
    }

    fn interval(name: &str, start: usize, end: usize) -> LivenessInterval {
        LivenessInterval {
            variable: String::from(name),
            span: CodeSpan::new(location(start), location(end)),
            uses: Vec::new(),
        }
    }

    fn register_constraint(register: Register) -> AbiLocationConstraint {
        AbiLocationConstraint {
            location: location(0),
            role: AbiLocationRole::ReturnValue,
            abi_location: Location::Register(register),
        }
    }

    fn pair_constraint(low: Register, high: Register) -> AbiLocationConstraint {
        AbiLocationConstraint {
            location: location(0),
            role: AbiLocationRole::ReturnValue,
            abi_location: Location::Pair {
                low: Box::new(Location::Register(low)),
                high: Box::new(Location::Register(high)),
            },
        }
    }

    fn location(instruction: usize) -> CodeLocation {
        CodeLocation::new(NodeIndex::new(0), instruction)
    }

    fn assert_register(location: &Location, register: Register) {
        match location {
            Location::Register(actual) => assert_eq!(*actual, register),
            other => panic!("expected register {register:?}, got {other:?}"),
        }
    }

    fn assert_stack(location: &Location, offset: usize) {
        match location {
            Location::Stack(actual) => assert_eq!(actual.0, offset),
            other => panic!("expected stack offset {offset}, got {other:?}"),
        }
    }
}
