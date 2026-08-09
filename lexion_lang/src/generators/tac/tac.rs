use crate::ast::types::TypeCollection;
use crate::ast::visitor::{AstNode, AstVisitor, AstVisitorAction, NodeType, TraversalType};
use crate::ast::{
    Ast, BlockExpr, CallExpr, CastExpr, Expr, ExprStmt, FuncDeclStmt, IdentExpr, IndexExpr, Lit,
    LitExpr, MemberExpr, ReturnStmt, Sourced, SourcedExpr, Stmt, StructExpr, TupleExpr, TypedExpr,
    VarDeclStmt, WhileStmt,
};
use crate::diagnostic::DiagnosticConsumer;
use crate::generators::label::{Label, LabelGenerator};
use crate::generators::tac::instructions::{
    AssignmentInstruction, BaseInstruction, BorrowInstruction, CodeLocation, CodeSpan,
    ConditionalJumpInstruction, ControlFlowGraph, CopyInstruction, EndFunctionInstruction,
    ExternInstruction, FunctionCallInstruction, FunctionCallTarget, FunctionInstruction,
    FunctionRange, Instruction, InstructionBlock, InstructionInstance, JumpInstruction,
    LivenessInterval, LoadInstruction, Operand, ParameterInstruction, Place, ReturnInstruction,
    StoreInstruction,
};
use crate::operators;
use crate::pipeline::PipelineStage;
use crate::symbol_table::{SymbolTableEntry, SymbolTableEntryType, SymbolTableGraph};
use generational_arena::Index;
use lexion_lib::miette::SourceSpan;
use lexion_lib::petgraph::prelude::NodeIndex;
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

    fn block_can_fallthrough(&self, block: NodeIndex) -> bool {
        !matches!(
            self.cfg[block]
                .instructions
                .last()
                .map(|inst| &inst.instruction),
            Some(Instruction::Jump(_) | Instruction::Return(_))
        )
    }

    fn current_block_can_fallthrough(&self) -> bool {
        self.current_block
            .map(|block| self.block_can_fallthrough(block))
            .unwrap_or(false)
    }

    fn assign(
        &mut self,
        target: Operand,
        operator: &'static str,
        right: Operand,
        left: Option<Operand>,
        source_span: Option<SourceSpan>,
    ) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            source_span,
            instruction: Instruction::Assignment(AssignmentInstruction {
                target,
                operator,
                right,
                left,
            }),
        })
    }

    fn borrow(
        &mut self,
        target: Operand,
        place: Place,
        source_span: Option<SourceSpan>,
    ) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            source_span,
            instruction: Instruction::Borrow(BorrowInstruction { target, place }),
        })
    }

    fn load(
        &mut self,
        target: Operand,
        place: Place,
        source_span: Option<SourceSpan>,
    ) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            source_span,
            instruction: Instruction::Load(LoadInstruction { target, place }),
        })
    }

    fn store(
        &mut self,
        place: Place,
        value: Operand,
        source_span: Option<SourceSpan>,
    ) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            source_span,
            instruction: Instruction::Store(StoreInstruction { place, value }),
        })
    }

    fn sourced_copy(
        &mut self,
        dst: Operand,
        src: Operand,
        source_span: SourceSpan,
    ) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            source_span: Some(source_span),
            instruction: Instruction::Copy(CopyInstruction { dst, src }),
        })
    }

    fn conditional_jump(
        &mut self,
        target: Operand,
        operator: &'static str,
        right: Operand,
        left: Option<Operand>,
        source_span: Option<SourceSpan>,
    ) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            source_span,
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
            source_span: None,
            instruction: Instruction::Jump(JumpInstruction { target }),
        })
    }

    fn param(&mut self, param: Operand, source_span: Option<SourceSpan>) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            source_span,
            instruction: Instruction::Parameter(ParameterInstruction { param }),
        })
    }

    fn call(
        &mut self,
        target: FunctionCallTarget,
        function_type: Option<Index>,
        return_target: Option<Operand>,
        source_span: SourceSpan,
    ) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            source_span: Some(source_span),
            instruction: Instruction::FunctionCall(FunctionCallInstruction {
                target,
                function_type,
                return_target,
            }),
        })
    }

    fn _return(&mut self, value: Option<Operand>, source_span: SourceSpan) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            source_span: Some(source_span),
            instruction: Instruction::Return(ReturnInstruction { value }),
        })
    }

    fn function(
        &mut self,
        label: String,
        params: Vec<String>,
        source_span: SourceSpan,
    ) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            source_span: Some(source_span),
            instruction: Instruction::Function(FunctionInstruction { label, params }),
        })
    }

    fn end_function(&mut self, label: String) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            source_span: None,
            instruction: Instruction::EndFunction(EndFunctionInstruction { label }),
        })
    }

    fn extern_(&mut self, label: String, source_span: SourceSpan) -> CodeLocation {
        self.instruction(InstructionInstance {
            live: Default::default(),
            source_span: Some(source_span),
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
        let intervals = analyze_liveness(&mut self.cfg);
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
                    value: Stmt::ReturnStmt(stmt),
                    span,
                }),
            ) => self.return_stmt(stmt, *span),
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
        let Some((_, _, entry)) = self.symbols.lookup(self.scope, decl.name.value.as_str()) else {
            return;
        };
        self.scope = entry.table.unwrap_or(self.scope);
        self.labels.temp = LabelGenerator::new("$t", None);
        self.block(decl.name.value.clone(), false, true);
        if decl.body.is_some() {
            let params = decl
                .params
                .iter()
                .map(|param| param.value.name.value.clone())
                .collect();
            self.function(decl.name.value.clone(), params, decl.name.span);
        } else if decl.is_extern {
            self.extern_(decl.name.value.clone(), decl.name.span);
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
            self.sourced_copy(
                Operand::Variable(decl.decl.name.value.clone()),
                temp,
                decl.decl.span,
            );
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
            Some(Operand::Literal(Lit::Boolean(false))),
            Some(stmt.condition.span),
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

    fn return_stmt(&mut self, stmt: &ReturnStmt, span: SourceSpan) {
        let value = stmt.expr.as_ref().map(|expr| self.expr(expr));
        self._return(value, span);
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
                        expr: Expr::CastExpr(_),
                        ..
                    },
                ..
            } => self.cast_expr(expr),
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::MemberExpr(_),
                        ..
                    },
                ..
            } => self.member_expr(expr),
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::IndexExpr(_),
                        ..
                    },
                ..
            } => self.index_expr(expr),
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
                        expr: Expr::StructExpr(_),
                        ..
                    },
                ..
            } => self.struct_expr(expr),
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::TupleExpr(_),
                        ..
                    },
                ..
            } => self.tuple_expr(expr),
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
        }
    }

    fn lit_expr(&mut self, expr: &LitExpr) -> Operand {
        Operand::Literal(expr.lit.clone())
    }

    fn ident_expr(&mut self, expr: &IdentExpr) -> Operand {
        self.symbols
            .lookup(self.scope, expr.ident.as_str())
            .filter(|(_, _, entry)| entry.ty == SymbolTableEntryType::Function)
            .map(|_| Operand::Label(expr.ident.clone()))
            .unwrap_or_else(|| Operand::Variable(expr.ident.clone()))
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
        if inner.operator == operators::TERNARY {
            return self.ternary_expr(expr);
        }
        if inner.operator == operators::ASSIGN {
            return self.assignment_expr(expr);
        }
        if inner.args.len() == 1 && inner.operator == operators::BORROW {
            return self.borrow_expr(expr);
        }
        if inner.args.len() == 1 && inner.operator == operators::DEREFERENCE {
            return self.load_expr(expr);
        }
        assert!(!inner.args.is_empty() && inner.args.len() <= 2);

        if inner.args.len() == 1 {
            let right = self.expr(&inner.args[0]);
            let temp = self.alloc_temp(*ty, *span);
            self.assign(temp.clone(), inner.operator, right, None, Some(*span));
            temp
        } else if inner.args.len() == 2 {
            let left = self.expr(&inner.args[0]);
            let right = self.expr(&inner.args[1]);
            let temp = self.alloc_temp(*ty, *span);
            self.assign(temp.clone(), inner.operator, right, Some(left), Some(*span));
            temp
        } else {
            unreachable!()
        }
    }

    fn assignment_expr(&mut self, expr: &SourcedExpr) -> Operand {
        let Sourced {
            value:
                TypedExpr {
                    expr: Expr::OperatorExpr(inner),
                    ..
                },
            span,
        } = expr
        else {
            unreachable!()
        };
        assert_eq!(inner.args.len(), 2);

        let left = &inner.args[0];
        if let Expr::IdentExpr(IdentExpr { ident }) = &left.value.expr {
            let target = Operand::Variable(ident.clone());
            let right = self.expr(&inner.args[1]);
            self.sourced_copy(target.clone(), right, *span);
            return target;
        }

        let place = self.place(left);
        let right = self.expr(&inner.args[1]);
        self.store(place, right.clone(), Some(*span));
        right
    }

    fn borrow_expr(&mut self, expr: &SourcedExpr) -> Operand {
        let Sourced {
            value:
                TypedExpr {
                    expr: Expr::OperatorExpr(inner),
                    ty,
                },
            span,
        } = expr
        else {
            unreachable!()
        };
        assert_eq!(inner.args.len(), 1);

        let place = self.place(&inner.args[0]);
        let temp = self.alloc_temp(*ty, *span);
        self.borrow(temp.clone(), place, Some(*span));
        temp
    }

    fn load_expr(&mut self, expr: &SourcedExpr) -> Operand {
        let place = self.place(expr);
        let temp = self.alloc_temp(expr.value.ty, expr.span);
        self.load(temp.clone(), place, Some(expr.span));
        temp
    }

    fn place(&mut self, expr: &SourcedExpr) -> Place {
        match &expr.value.expr {
            Expr::IdentExpr(IdentExpr { ident }) => Place::Direct(Operand::Variable(ident.clone())),
            Expr::MemberExpr(MemberExpr { expr: base, ident }) => Place::Member {
                base: Box::new(self.place(base)),
                member: ident.clone(),
            },
            Expr::IndexExpr(IndexExpr { expr: base, index }) => {
                let base = Box::new(self.place(base));
                let index = self.expr(index);
                Place::Index { base, index }
            }
            Expr::OperatorExpr(inner)
                if inner.operator == operators::DEREFERENCE && inner.args.len() == 1 =>
            {
                Place::Dereference(self.expr(&inner.args[0]))
            }
            _ => Place::Direct(self.expr(expr)),
        }
    }

    fn ternary_expr(&mut self, expr: &SourcedExpr) -> Operand {
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
        assert_eq!(inner.args.len(), 3);
        let temp = if !self.types.eq(*ty, self.types.unit()) {
            Some(self.alloc_temp(*ty, *span))
        } else {
            None
        };

        let condition = self.expr(&inner.args[0]);
        let cond_jump = self.conditional_jump(
            Operand::Placeholder,
            operators::EQUALS,
            condition,
            Some(Operand::Literal(Lit::Boolean(false))),
            Some(inner.args[0].span),
        );
        let prev_block = self.current_block.unwrap();

        let label = self.labels.cond_then.next().to_string();
        self.block(label, true, false);
        let then = self.expr(&inner.args[1]);
        if let Some(temp) = &temp {
            self.sourced_copy(temp.clone(), then, inner.args[1].span);
        }
        let then_falls_through = self.current_block_can_fallthrough();
        let then_exit = self.current_block;
        let jump = then_falls_through.then(|| self.jump(Operand::Placeholder));

        let label = self.labels.cond_else.next().to_string();
        if let Instruction::ConditionalJump(jump) = cond_jump.instruction_mut(&mut self.cfg) {
            jump.target = Operand::Label(label.clone());
        }
        let else_block = self.block(label, false, false);
        self.cfg.add_edge(prev_block, else_block, ());
        let else_ = self.expr(&inner.args[2]);
        if let Some(temp) = &temp {
            self.sourced_copy(temp.clone(), else_, inner.args[2].span);
        }
        let else_falls_through = self.current_block_can_fallthrough();
        let else_exit = self.current_block;

        let label = self.labels.cond_end.next().to_string();
        if let Some(jump_location) = jump {
            if let Instruction::Jump(jump) = &mut jump_location.instruction_mut(&mut self.cfg) {
                jump.target = Operand::Label(label.clone());
            }
        }
        let next_block = self.block(label, false, false);
        if then_falls_through {
            if let Some(then_exit) = then_exit {
                self.cfg.add_edge(then_exit, next_block, ());
            }
        }
        if else_falls_through {
            if let Some(else_exit) = else_exit {
                self.cfg.add_edge(else_exit, next_block, ());
            }
        }

        temp.unwrap_or(Operand::Placeholder)
    }

    fn cast_expr(&mut self, expr: &SourcedExpr) -> Operand {
        let Sourced {
            value:
                TypedExpr {
                    ty,
                    expr: Expr::CastExpr(CastExpr { expr: inner, .. }),
                },
            span,
        } = expr
        else {
            unreachable!()
        };
        let right = self.expr(inner);
        if inner.ty == *ty {
            right
        } else {
            let temp = self.alloc_temp(*ty, *span);
            self.assign(temp.clone(), operators::TYPE_CAST, right, None, Some(*span));
            temp
        }
    }

    fn member_expr(&mut self, expr: &SourcedExpr) -> Operand {
        self.load_expr(expr)
    }

    fn index_expr(&mut self, expr: &SourcedExpr) -> Operand {
        self.load_expr(expr)
    }

    fn tuple_expr(&mut self, expr: &SourcedExpr) -> Operand {
        let TypedExpr {
            expr: Expr::TupleExpr(TupleExpr { values }),
            ty,
        } = &expr.value
        else {
            unreachable!()
        };
        let target = self.alloc_temp(*ty, expr.span);
        for (index, value) in values.iter().enumerate() {
            let value = self.expr(value);
            self.store(
                Place::Member {
                    base: Box::new(Place::Direct(target.clone())),
                    member: index.to_string(),
                },
                value,
                Some(expr.span),
            );
        }
        target
    }

    fn struct_expr(&mut self, expr: &SourcedExpr) -> Operand {
        let TypedExpr {
            expr: Expr::StructExpr(StructExpr { fields, .. }),
            ty,
        } = &expr.value
        else {
            unreachable!()
        };
        let target = self.alloc_temp(*ty, expr.span);
        for field in fields {
            let value = self.expr(&field.value.expr);
            self.store(
                Place::Member {
                    base: Box::new(Place::Direct(target.clone())),
                    member: field.value.name.value.clone(),
                },
                value,
                Some(field.value.expr.span),
            );
        }
        target
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
        let return_value = if !self.types.eq(expr.ty, self.types.unit()) {
            Some(self.alloc_temp(*ty, *span))
        } else {
            None
        };
        let function = self.expr(expr);
        let args = args
            .iter()
            .map(|arg| self.expr(arg))
            .rev()
            .collect::<Vec<_>>();
        for arg in args {
            self.param(arg, Some(*span));
        }
        let target = match function {
            Operand::Label(name) => FunctionCallTarget::Direct(name),
            operand => FunctionCallTarget::Indirect(operand),
        };
        self.call(target, Some(expr.ty), return_value.clone(), *span);
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
            Some(expr.condition.span),
        );
        let prev_block = self.current_block.unwrap();

        let label = self.labels.cond_then.next().to_string();
        self.block(label, true, false);
        let then = self.expr(&expr.then);
        if let Some(temp) = &temp {
            self.sourced_copy(temp.clone(), then, expr.then.span);
        }

        if let Some(else_) = &expr.else_ {
            let then_falls_through = self.current_block_can_fallthrough();
            let then_exit = self.current_block;
            let jump = then_falls_through.then(|| self.jump(Operand::Placeholder));

            let label = self.labels.cond_else.next().to_string();
            if let Instruction::ConditionalJump(jump) = cond_jump.instruction_mut(&mut self.cfg) {
                jump.target = Operand::Label(label.clone());
            }
            let else_block = self.block(label, false, false);
            self.cfg.add_edge(prev_block, else_block, ());
            let else_span = else_.span;
            let else_ = self.expr(else_);
            if let Some(temp) = &temp {
                self.sourced_copy(temp.clone(), else_, else_span);
            }
            let else_falls_through = self.current_block_can_fallthrough();
            let else_exit = self.current_block;

            let label = self.labels.cond_end.next().to_string();
            if let Some(jump_location) = jump {
                if let Instruction::Jump(jump) = &mut jump_location.instruction_mut(&mut self.cfg) {
                    jump.target = Operand::Label(label.clone());
                }
            }
            let next_block = self.block(label, false, false);
            if then_falls_through {
                if let Some(then_exit) = then_exit {
                    self.cfg.add_edge(then_exit, next_block, ());
                }
            }
            if else_falls_through {
                if let Some(else_exit) = else_exit {
                    self.cfg.add_edge(else_exit, next_block, ());
                }
            }
        } else {
            let then_falls_through = self.current_block_can_fallthrough();
            let then_exit = self.current_block;
            let label = self.labels.cond_end.next().to_string();
            if let Instruction::ConditionalJump(jump) = cond_jump.instruction_mut(&mut self.cfg) {
                jump.target = Operand::Label(label.clone());
            }
            let next_block = self.block(label, false, false);
            if then_falls_through {
                if let Some(then_exit) = then_exit {
                    self.cfg.add_edge(then_exit, next_block, ());
                }
            }
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

pub fn analyze_liveness(
    cfg: &mut ControlFlowGraph,
) -> HashMap<FunctionRange, Vec<LivenessInterval>> {
    let mut intervals: HashMap<FunctionRange, Vec<LivenessInterval>> = Default::default();
    for interval in (TacLivenessAnalyzer { cfg }).liveness_analysis() {
        if let Some(func) = cfg.functions.iter().find(|f| f.contains(interval.span)) {
            intervals.entry(*func).or_default().push(interval);
        }
    }
    intervals
}

struct TacLivenessAnalyzer<'a> {
    cfg: &'a mut ControlFlowGraph,
}

impl TacLivenessAnalyzer<'_> {
    fn clear_liveness(&mut self) {
        for block in self.cfg.node_weights_mut() {
            block.live = Default::default();
            for inst in &mut block.instructions {
                inst.live = Default::default();
            }
        }
    }

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
        self.clear_liveness();
        self.liveness_read_written();

        let mut worklist = self.cfg.node_indices().collect::<VecDeque<_>>();
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

    fn liveness_intervals(&self) -> Vec<LivenessInterval> {
        let mut result: Vec<LivenessInterval> = Default::default();

        for func in &self.cfg.functions {
            let mut intervals: HashMap<String, LivenessInterval> = HashMap::new();

            for node in self.cfg.function_nodes(func) {
                let block = &self.cfg[node];
                for (inst_idx, inst) in block.instructions.iter().enumerate() {
                    let loc = CodeLocation::new(node, inst_idx);
                    let after = CodeLocation::new(node, inst_idx + 1);

                    for live_var in &inst.live.input {
                        Self::record_interval_location(&mut intervals, live_var, loc, after);
                    }
                    for live_var in &inst.live.output {
                        Self::record_interval_location(&mut intervals, live_var, loc, after);
                    }
                    for written_var in &inst.live.written {
                        if inst.live.output.contains(written_var) {
                            Self::record_interval_location(&mut intervals, written_var, loc, after);
                        }
                    }
                    for read_var in &inst.live.read {
                        Self::record_interval_location(&mut intervals, read_var, loc, after);
                        if let Some(interval) = intervals.get_mut(read_var) {
                            interval.uses.push(loc);
                        }
                    }
                }
            }

            result.extend(intervals.into_values());
        }

        result
    }

    fn record_interval_location(
        intervals: &mut HashMap<String, LivenessInterval>,
        variable: &str,
        start: CodeLocation,
        end: CodeLocation,
    ) {
        let interval = intervals
            .entry(variable.to_string())
            .or_insert_with(|| LivenessInterval {
                variable: variable.to_string(),
                span: CodeSpan::new(start, end),
                uses: Vec::new(),
            });
        interval.span.start = interval.span.start.min(start);
        interval.span.end = interval.span.end.max(end);
    }
}
