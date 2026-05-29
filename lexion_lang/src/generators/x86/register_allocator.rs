use crate::ast::types::{FunctionType, Type, TypeCollection};
use crate::diagnostic::DiagnosticConsumer;
use crate::generators::tac::instructions::{
    CodeLocation, ControlFlowGraph, FunctionRange, Instruction, LivenessInterval, Operand,
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
        intervals.sort_by_key(|i| i.span.start);

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
        intervals.sort_by_key(|interval| interval.span.start);
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
            let allowed = self.allowed_registers(crosses_call);

            if let Some(reg) = self.take_register(&allowed, &interval_constraints) {
                let assigned_interval = AssignedLivenessInterval {
                    interval,
                    location: Location::Register(reg),
                    constraints: interval_constraints,
                };
                self.insert_active(assigned_interval.clone());
                assigned.push(assigned_interval);
            } else {
                assigned.push(AssignedLivenessInterval {
                    interval,
                    location: self.next_spill_location(),
                    constraints: interval_constraints,
                });
            }
        }

        assigned
    }

    fn reset_function(&mut self) {
        self.active = Default::default();
        self.available = self.registers.iter().copied().collect();
        self.stack_offset = 0;
    }

    fn allowed_registers(&self, crosses_call: bool) -> Vec<Register> {
        if crosses_call {
            self.target
                .calling_convention()
                .callee_saved()
                .iter()
                .copied()
                .filter(|reg| *reg != Register::RBP)
                .collect()
        } else {
            self.registers.clone()
        }
    }

    fn take_register(
        &mut self,
        allowed: &[Register],
        constraints: &[AbiLocationConstraint],
    ) -> Option<Register> {
        let preferred = constraints.iter().filter_map(|constraint| {
            constraint
                .abi_location
                .register()
                .filter(|register| allowed.contains(register))
        });
        for register in preferred.chain(allowed.iter().copied()) {
            if let Some(position) = self
                .available
                .iter()
                .position(|available| *available == register)
            {
                return self.available.remove(position);
            }
        }
        None
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
                        let Some(signature) = self.function_signature(&call.function) else {
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
                                            function: call.function.clone(),
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
                                            function: call.function.clone(),
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
        let (_, _, entry) = self.symbols.lookup(self.symbols.root, function)?;
        let ty = entry.var_type?;
        match self.types.get(ty)? {
            Type::FunctionType(signature) => Some(signature),
            _ => None,
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
