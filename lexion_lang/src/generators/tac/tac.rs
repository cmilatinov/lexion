use crate::ast::types::TypeCollection;
use crate::ast::visitor::{AstNode, AstVisitor, AstVisitorAction, NodeType, TraversalType};
use crate::ast::{
    Ast, BlockExpr, CallExpr, Expr, ExprStmt, FuncDeclStmt, IdentExpr, Lit, LitExpr, Sourced,
    SourcedExpr, Stmt, TypedExpr, VarDeclStmt, WhileStmt,
};
use crate::diagnostic::DiagnosticConsumer;
use crate::generators::label::{Label, LabelGenerator};
use crate::generators::tac::instructions::{
    AssignmentInstruction, BaseInstruction, CodeLocation, CodeSpan, ConditionalJumpInstruction,
    ControlFlowGraph, CopyInstruction, EndFunctionInstruction, ExternInstruction,
    FunctionCallInstruction, FunctionInstruction, FunctionRange, Instruction, InstructionBlock,
    InstructionInstance, JumpInstruction, LivenessInterval, Operand, ParameterInstruction,
    ReturnInstruction,
};
use crate::operators;
use crate::pipeline::PipelineStage;
use crate::symbol_table::{SymbolTableEntry, SymbolTableEntryType, SymbolTableGraph};
use generational_arena::Index;
use lexion_lib::miette::SourceSpan;
use lexion_lib::petgraph::prelude::NodeIndex;
use lexion_lib::petgraph::visit::{Dfs, Reversed, Walker};
use lexion_lib::petgraph::Direction;
use std::collections::{HashMap, HashSet, VecDeque};

struct LabelGenerators {
    temp: LabelGenerator,
    loop_start: LabelGenerator,
    loop_end: LabelGenerator,
    cond_then: LabelGenerator,
    cond_else: LabelGenerator,
    cond_end: LabelGenerator,
}

struct PartialLoop {
    jump_instruction: CodeLocation,
    start_label: Label,
}

pub struct CodeGeneratorTac<'a> {
    cfg: ControlFlowGraph,
    current_block: Option<NodeIndex>,
    scope: NodeIndex,
    labels: LabelGenerators,
    ast: &'a Ast,
    types: &'a TypeCollection,
    symbols: &'a mut SymbolTableGraph,
    loop_stack: Vec<PartialLoop>,
}

impl<'a> CodeGeneratorTac<'a> {
    fn current_block_mut(&mut self) -> Option<&mut InstructionBlock> {
        self.current_block
            .and_then(|idx| self.cfg.node_weight_mut(idx))
    }

    fn block(
        &mut self,
        label: String,
        link_to_previous: bool,
        is_function_entry: bool,
    ) -> NodeIndex {
        let new_block_idx = self.cfg.block(label, is_function_entry);
        if link_to_previous {
            if let Some(current_block_idx) = self.current_block {
                self.cfg.link(current_block_idx, new_block_idx);
            }
        }
        self.current_block = Some(new_block_idx);
        new_block_idx
    }

    fn instruction(&mut self, instruction: InstructionInstance) -> CodeLocation {
        let block = self.current_block_mut().unwrap();
        block.instructions.push(instruction);
        let instruction = block.instructions.len() - 1;
        CodeLocation::new(self.current_block.unwrap(), instruction)
    }

    fn assign(
        &mut self,
        target: Operand,
        operator: &'static str,
        right: Operand,
        left: Option<Operand>,
    ) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            instruction: Instruction::Assignment(AssignmentInstruction {
                target,
                operator,
                right,
                left,
            }),
        })
    }

    fn copy(&mut self, dst: Operand, src: Operand) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            instruction: Instruction::Copy(CopyInstruction { dst, src }),
        })
    }

    fn conditional_jump(
        &mut self,
        target: Operand,
        operator: &'static str,
        right: Operand,
        left: Option<Operand>,
    ) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            instruction: Instruction::ConditionalJump(ConditionalJumpInstruction {
                target,
                operator,
                right,
                left,
            }),
        })
    }

    fn jump(&mut self, target: Operand) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            instruction: Instruction::Jump(JumpInstruction { target }),
        })
    }

    fn param(&mut self, param: Operand) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            instruction: Instruction::Parameter(ParameterInstruction { param }),
        })
    }

    fn call(&mut self, function: String, return_target: Option<Operand>) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            instruction: Instruction::FunctionCall(FunctionCallInstruction {
                function,
                return_target,
            }),
        })
    }

    fn _return(&mut self, value: Option<Operand>) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            instruction: Instruction::Return(ReturnInstruction { value }),
        })
    }

    fn function(&mut self, label: String) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            instruction: Instruction::Function(FunctionInstruction { label }),
        })
    }

    fn end_function(&mut self, label: String) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            instruction: Instruction::EndFunction(EndFunctionInstruction { label }),
        })
    }

    fn extern_(&mut self, label: String) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            instruction: Instruction::Extern(ExternInstruction { label }),
        })
    }

    fn parent_scope(&mut self) {
        if let Some(parent) = self.symbols.parent_scope(self.scope) {
            self.scope = parent;
        }
    }
}

impl<'a> PipelineStage for CodeGeneratorTac<'a> {
    type Input = (&'a Ast, &'a mut SymbolTableGraph, &'a TypeCollection);
    type Options = ();
    type Output = (
        ControlFlowGraph,
        HashMap<FunctionRange, Vec<LivenessInterval>>,
    );

    fn new((ast, symbols, types): Self::Input) -> Self {
        Self {
            cfg: Default::default(),
            current_block: None,
            ast,
            labels: LabelGenerators {
                temp: LabelGenerator::new("$t", None),
                loop_start: LabelGenerator::new("$lstart_", None),
                loop_end: LabelGenerator::new("$lend_", None),
                cond_then: LabelGenerator::new("$cthen_", None),
                cond_else: LabelGenerator::new("$celse_", None),
                cond_end: LabelGenerator::new("$cend_", None),
            },
            scope: symbols.root,
            symbols,
            types,
            loop_stack: Default::default(),
        }
    }

    fn exec(
        mut self,
        _diag: &mut dyn DiagnosticConsumer,
        _: Self::Options,
    ) -> Option<Self::Output> {
        AstVisitor::new()
            .without_ifs()
            .visit(self.ast, |ty, node, _| self.traverse(ty, node));
        let mut intervals: HashMap<FunctionRange, Vec<LivenessInterval>> = Default::default();
        for interval in self.liveness_analysis() {
            if let Some(func) = self
                .cfg
                .functions
                .iter()
                .find(|f| f.contains(interval.span))
            {
                intervals.entry(*func).or_default().push(interval);
            }
        }
        Some((self.cfg, intervals))
    }
}

impl<'a> CodeGeneratorTac<'a> {
    fn traverse(&mut self, ty: TraversalType, node: AstNode<'_>) -> AstVisitorAction {
        match (ty, node) {
            (
                TraversalType::Preorder,
                AstNode::Stmt(Sourced {
                    value: Stmt::FuncDeclStmt(decl),
                    ..
                }),
            ) => self.begin_func_decl_stmt(decl),
            (
                TraversalType::Postorder,
                AstNode::Stmt(Sourced {
                    value: Stmt::FuncDeclStmt(decl),
                    ..
                }),
            ) => self.end_func_decl_stmt(decl),
            (
                TraversalType::Preorder,
                AstNode::Stmt(Sourced {
                    value: Stmt::VarDeclStmt(decl),
                    ..
                }),
            ) => self.var_decl_stmt(decl),
            (
                TraversalType::Preorder,
                AstNode::Stmt(Sourced {
                    value: Stmt::ExprStmt(stmt),
                    ..
                }),
            ) => self.expr_stmt(stmt),
            (
                TraversalType::Preorder,
                AstNode::Stmt(Sourced {
                    value: Stmt::WhileStmt(stmt),
                    ..
                }),
            ) => self.begin_while_stmt(stmt),
            (
                TraversalType::Postorder,
                AstNode::Stmt(Sourced {
                    value: Stmt::WhileStmt(_),
                    ..
                }),
            ) => self.end_while_stmt(),
            _ => {}
        };
        AstVisitorAction::Continue
    }

    fn begin_func_decl_stmt(&mut self, decl: &FuncDeclStmt) {
        let Some((scope, _, _)) = self.symbols.lookup(self.scope, decl.name.value.as_str()) else {
            return;
        };
        self.scope = scope;
        self.labels.temp = LabelGenerator::new("$t", None);
        self.block(decl.name.value.clone(), false, true);
        if decl.body.is_some() {
            self.function(decl.name.value.clone());
        } else if decl.is_extern {
            self.extern_(decl.name.value.clone());
        }
    }

    fn end_func_decl_stmt(&mut self, decl: &FuncDeclStmt) {
        if decl.body.is_some() {
            self.end_function(decl.name.value.clone());
            self.cfg.end_function();
        }
        self.parent_scope();
    }

    fn var_decl_stmt(&mut self, decl: &VarDeclStmt) {
        if let Some(init) = &decl.decl.init {
            let temp = self.expr(init);
            self.copy(Operand::Variable(decl.decl.name.value.clone()), temp);
        }
    }

    fn begin_while_stmt(&mut self, stmt: &WhileStmt) {
        let start_label = self.labels.loop_start.next();
        self.block(start_label.to_string(), true, false);
        let condition = self.expr(&stmt.condition);
        let jump_instruction = self.conditional_jump(
            Operand::Placeholder,
            operators::EQUALS,
            condition,
            Some(Operand::Literal(Lit::Boolean(true))),
        );
        self.loop_stack.push(PartialLoop {
            jump_instruction,
            start_label,
        });
    }

    fn end_while_stmt(&mut self) {
        let loop_ = self.loop_stack.pop().expect("loop stack is empty");
        let end_label = self.labels.loop_end.next();
        self.jump(Operand::Label(loop_.start_label.to_string()));
        if let Instruction::ConditionalJump(inst) =
            &mut loop_.jump_instruction.instruction_mut(&mut self.cfg)
        {
            inst.target = Operand::Label(end_label.to_string());
        }
        self.cfg.add_edge(
            self.current_block.unwrap(),
            loop_.jump_instruction.block,
            (),
        );
        let end_block = self.block(end_label.to_string(), false, false);
        self.cfg
            .add_edge(loop_.jump_instruction.block, end_block, ());
    }

    fn expr_stmt(&mut self, stmt: &ExprStmt) {
        let _ = self.expr(&stmt.expr);
    }

    fn expr(&mut self, expr: &SourcedExpr) -> Operand {
        match expr {
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::LitExpr(expr),
                        ..
                    },
                ..
            } => self.lit_expr(expr),
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::IdentExpr(expr),
                        ..
                    },
                ..
            } => self.ident_expr(expr),
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::OperatorExpr(_),
                        ..
                    },
                ..
            } => self.operator_expr(expr),
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::CallExpr(_),
                        ..
                    },
                ..
            } => self.call_expr(expr).unwrap_or(Operand::Placeholder),
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::IfExpr(_),
                        ..
                    },
                ..
            } => self.if_expr(expr).unwrap_or(Operand::Placeholder),
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::BlockExpr(_),
                        ..
                    },
                ..
            } => self.block_expr(expr).unwrap_or(Operand::Placeholder),
            _ => Operand::Placeholder,
        }
    }

    fn lit_expr(&mut self, expr: &LitExpr) -> Operand {
        Operand::Literal(expr.lit.clone())
    }

    fn ident_expr(&mut self, expr: &IdentExpr) -> Operand {
        Operand::Variable(expr.ident.clone())
    }

    fn operator_expr(&mut self, expr: &SourcedExpr) -> Operand {
        let Sourced {
            value:
                TypedExpr {
                    expr: Expr::OperatorExpr(inner),
                    ty,
                },
            span,
            ..
        } = expr
        else {
            unreachable!()
        };
        assert!(!inner.args.is_empty() && inner.args.len() <= 2);

        if inner.args.len() == 1 {
            let right = self.expr(&inner.args[0]);
            let temp = self.alloc_temp(*ty, *span);
            self.assign(temp.clone(), inner.operator, right, None);
            temp
        } else if inner.args.len() == 2 {
            let left = self.expr(&inner.args[0]);
            let right = self.expr(&inner.args[1]);
            if inner.operator == "=" {
                self.copy(left.clone(), right);
                left
            } else {
                let temp = self.alloc_temp(*ty, *span);
                self.assign(temp.clone(), inner.operator, right, Some(left));
                temp
            }
        } else {
            unreachable!()
        }
    }

    fn call_expr(&mut self, expr: &SourcedExpr) -> Option<Operand> {
        let Sourced {
            value:
                TypedExpr {
                    expr: Expr::CallExpr(CallExpr { expr, args }),
                    ty,
                },
            span,
            ..
        } = expr
        else {
            return None;
        };
        let Sourced {
            value:
                TypedExpr {
                    expr: Expr::IdentExpr(ident),
                    ..
                },
            ..
        } = expr.as_ref()
        else {
            return None;
        };
        let return_value = if !self.types.eq(expr.ty, self.types.unit()) {
            Some(self.alloc_temp(*ty, *span))
        } else {
            None
        };
        let args = args
            .iter()
            .map(|arg| self.expr(arg))
            .rev()
            .collect::<Vec<_>>();
        for arg in args {
            self.param(arg);
        }
        self.call(ident.ident.clone(), return_value.clone());
        return_value
    }

    fn if_expr(&mut self, expr: &SourcedExpr) -> Option<Operand> {
        let Sourced {
            value:
                TypedExpr {
                    expr: Expr::IfExpr(expr),
                    ty,
                },
            span,
        } = expr
        else {
            return None;
        };
        let temp = if !self.types.eq(*ty, self.types.unit()) {
            Some(self.alloc_temp(*ty, *span))
        } else {
            None
        };

        let condition = self.expr(&expr.condition);
        let cond_jump = self.conditional_jump(
            Operand::Placeholder,
            operators::EQUALS,
            condition,
            Some(Operand::Literal(Lit::Boolean(false))),
        );
        let prev_block = self.current_block.unwrap();

        let label = self.labels.cond_then.next().to_string();
        let then_block = self.block(label, true, false);
        let then = self.expr(&expr.then);
        if let Some(temp) = &temp {
            self.copy(temp.clone(), then);
        }

        if let Some(else_) = &expr.else_ {
            let jump = self.jump(Operand::Placeholder);

            let label = self.labels.cond_else.next().to_string();
            if let Instruction::ConditionalJump(jump) = cond_jump.instruction_mut(&mut self.cfg) {
                jump.target = Operand::Label(label.clone());
            }
            let else_block = self.block(label, false, false);
            self.cfg.add_edge(prev_block, else_block, ());
            let else_ = self.expr(else_);
            if let Some(temp) = &temp {
                self.copy(temp.clone(), else_);
            }

            let label = self.labels.cond_end.next().to_string();
            if let Instruction::Jump(jump) = &mut jump.instruction_mut(&mut self.cfg) {
                jump.target = Operand::Label(label.clone());
            }
            let next_block = self.block(label, false, false);
            self.cfg.add_edge(then_block, next_block, ());
            self.cfg.add_edge(else_block, next_block, ());
        } else {
            let label = self.labels.cond_end.next().to_string();
            if let Instruction::ConditionalJump(jump) = cond_jump.instruction_mut(&mut self.cfg) {
                jump.target = Operand::Label(label.clone());
            }
            let next_block = self.block(label, true, false);
            self.cfg.add_edge(prev_block, next_block, ());
        }

        temp
    }

    fn block_expr(&mut self, expr: &SourcedExpr) -> Option<Operand> {
        AstVisitor::new()
            .without_ifs()
            .without_block_end_exprs()
            .visit_block_expr(expr, NodeType::Root, false, &mut |ty, node, _| {
                self.traverse(ty, node)
            });
        let Sourced {
            value:
                TypedExpr {
                    expr:
                        Expr::BlockExpr(BlockExpr {
                            expr: Some(expr), ..
                        }),
                    ..
                },
            ..
        } = expr
        else {
            return None;
        };
        Some(self.expr(expr))
    }
}

impl<'a> CodeGeneratorTac<'a> {
    fn alloc_temp(&mut self, ty: Index, span: SourceSpan) -> Operand {
        let temp = self.labels.temp.next();
        let _ = self.symbols.insert_entry(
            self.scope,
            SymbolTableEntry {
                ty: SymbolTableEntryType::Temporary,
                name: temp.to_string(),
                table: None,
                span,
                var_type: Some(ty),
                layout: None,
            },
        );
        Operand::Temporary(temp)
    }
}

impl<'a> CodeGeneratorTac<'a> {
    fn liveness_read_written(&mut self) {
        for block in self.cfg.node_weights_mut() {
            let mut written_so_far = HashSet::new();
            let mut block_use = HashSet::new();
            let mut block_def = HashSet::new();
            for inst in block.instructions.iter_mut() {
                inst.live.read = inst.instruction.variables_read();
                inst.live.written = inst.instruction.variables_written();
                for var in inst.live.read.iter() {
                    if !written_so_far.contains(var) {
                        block_use.insert(var.clone());
                    }
                }
                for var in inst.live.written.iter() {
                    written_so_far.insert(var.clone());
                    block_def.insert(var.clone());
                }
            }
            block.live.read = block_use;
            block.live.written = block_def;
        }
    }

    fn liveness_per_instruction(&mut self) {
        for block in self.cfg.node_weights_mut() {
            let mut output = block.live.output.clone();
            for inst in block.instructions.iter_mut().rev() {
                inst.live.output = output;
                let diff = inst
                    .live
                    .output
                    .difference(&inst.live.written)
                    .cloned()
                    .collect();
                output = inst.live.read.union(&diff).cloned().collect();
                inst.live.input = output.clone();
            }
        }
    }

    fn liveness_analysis(&mut self) -> Vec<LivenessInterval> {
        self.liveness_read_written();

        let exit_block = self.current_block.unwrap();
        let reversed_blocks = Reversed(&self.cfg.graph);

        let mut worklist = Dfs::new(&reversed_blocks, exit_block)
            .iter(&reversed_blocks)
            .collect::<VecDeque<_>>();
        let mut in_worklist = worklist.iter().cloned().collect::<HashSet<_>>();

        while let Some(block_idx) = worklist.pop_front() {
            in_worklist.remove(&block_idx);

            let old_input = self.cfg[block_idx].live.input.clone();
            let old_output = self.cfg[block_idx].live.output.clone();

            let output = self
                .cfg
                .neighbors(block_idx)
                .flat_map(|block| self.cfg[block].live.input.iter().cloned())
                .collect::<HashSet<_>>();

            let diff = output
                .difference(&self.cfg[block_idx].live.written)
                .cloned()
                .collect();
            let input = self.cfg[block_idx]
                .live
                .read
                .union(&diff)
                .cloned()
                .collect();

            if old_input != input || old_output != output {
                for pred in self.cfg.neighbors_directed(block_idx, Direction::Incoming) {
                    if !in_worklist.contains(&pred) {
                        worklist.push_back(pred);
                        in_worklist.insert(pred);
                    }
                }
            }

            self.cfg[block_idx].live.input = input;
            self.cfg[block_idx].live.output = output;
        }

        self.liveness_per_instruction();
        self.liveness_intervals()
    }

    pub fn liveness_intervals(&self) -> Vec<LivenessInterval> {
        let mut result: Vec<LivenessInterval> = Default::default();
        let mut active_intervals: HashMap<String, LivenessInterval> = HashMap::new();
        for node in self.cfg.node_indices() {
            let block = &self.cfg[node];
            for (inst_idx, inst) in block.instructions.iter().enumerate() {
                let loc = CodeLocation::new(node, inst_idx);
                for live_var in &inst.live.input {
                    active_intervals
                        .entry(live_var.clone())
                        .or_insert_with(|| LivenessInterval {
                            variable: live_var.clone(),
                            span: CodeSpan::from_location(loc),
                            uses: Vec::new(),
                        });
                }
                for read_var in &inst.live.read {
                    if let Some(interval) = active_intervals.get_mut(read_var) {
                        interval.uses.push(loc);
                    }
                }
                let (mut still_live, now_dead) = active_intervals
                    .drain()
                    .partition::<HashMap<_, _>, _>(|(v, _)| inst.live.output.contains(v));
                for (_, interval) in still_live.iter_mut() {
                    interval.span.end = loc;
                }
                active_intervals = still_live;
                for (_, mut interval) in now_dead {
                    interval.span.end = CodeLocation::new(node, inst_idx + 1);
                    result.push(interval);
                }
            }
        }
        result
    }
}
