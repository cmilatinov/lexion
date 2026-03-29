use crate::diagnostic::DiagnosticConsumer;
use crate::generators::tac::instructions::{
    CodeLocation, ControlFlowGraph, FunctionRange, LivenessInterval,
};
use crate::generators::x86::calling_convention::{Location, StackOffset};
use crate::pipeline::PipelineStage;
use iced_x86::Register;
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone)]
pub struct AssignedLivenessInterval {
    interval: LivenessInterval,
    location: Location,
}

pub struct LinearRegisterAllocator<'a> {
    registers: Vec<Register>,
    active: Vec<AssignedLivenessInterval>,
    available: VecDeque<Register>,
    cfg: &'a ControlFlowGraph,
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
            cfg,
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
        let spilled;
        let spilled_interval = if let Some((idx, to_spill)) = self
            .active
            .iter()
            .enumerate()
            .max_by_key(|(_, i)| i.interval.span.end)
        {
            if to_spill.interval.span.end > interval.span.end {
                spilled = self.active.remove(idx);
                let reg = spilled.location.register().unwrap();
                let new_assigned = AssignedLivenessInterval {
                    interval,
                    location: Location::Register(reg),
                };
                self.insert_active(new_assigned.clone());
                assigned.push(new_assigned);
                &spilled.interval
            } else {
                &interval
            }
        } else {
            &interval
        };

        let spill_location = Location::Stack(StackOffset(self.stack_offset));
        self.stack_offset += 1;
        let spilled_with_stack = AssignedLivenessInterval {
            interval: spilled_interval.clone(),
            location: spill_location,
        };
        assigned.push(spilled_with_stack);
    }
}
