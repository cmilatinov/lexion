use std::sync::Arc;

use generational_arena::Index;

use lexion_lib::miette::{NamedSource, SourceSpan};
use lexion_lib::petgraph::graph::NodeIndex;

use crate::ast::types::{FunctionType, Type, TypeCollection};
use crate::ast::visitor::{
    AstNode, AstNodeMut, AstVisitor, AstVisitorAction, NodeType, TraversalType,
};
use crate::ast::{
    Ast, BlockExpr, CallExpr, Expr, ExprStmt, FuncDeclStmt, IdentExpr, IfExpr, IndexExpr, Lit,
    LitExpr, MemberExpr, OperatorExpr, ReturnStmt, Sourced, SourcedExpr, Stmt, StructDeclStmt,
    TypedExpr, VarDecl, VarDeclStmt, WhileStmt,
};
use crate::diagnostic::{DiagnosticConsumer, LexionDiagnosticError};
use crate::operators;
use crate::pipeline::PipelineStage;
use crate::symbol_table::SymbolTableGraph;
use crate::type_checker::operator_table::OperatorTable;

pub struct TypeChecker<'a> {
    src: NamedSource<Arc<String>>,
    table: &'a mut SymbolTableGraph,
    types: &'a mut TypeCollection,
    operators: OperatorTable,
    current_scope: NodeIndex,
    block_scope_counts: Vec<usize>,
}

impl<'a> TypeChecker<'a> {
    fn tc(
        &mut self,
        diag: &mut dyn DiagnosticConsumer,
        expr: &mut SourcedExpr,
        expected: Option<Index>,
    ) -> Option<Index> {
        if let Some(ty) = self.expr(diag, expr) {
            if let Some(expected) = expected {
                if !self.types.eq(ty, expected) {
                    self.expect(diag, expr.span, ty, expected);
                }
            }
            return Some(ty);
        }
        None
    }

    fn expect(
        &self,
        diag: &mut dyn DiagnosticConsumer,
        span: SourceSpan,
        ty: Index,
        expected: Index,
    ) {
        diag.error(LexionDiagnosticError {
            src: self.src.clone(),
            span,
            message: format!(
                "expected type '{}', instead got '{}'",
                self.types.to_string_index(expected),
                self.types.to_string_index(ty)
            ),
        });
    }

    fn expr(&mut self, diag: &mut dyn DiagnosticConsumer, expr: &mut SourcedExpr) -> Option<Index> {
        let ty = match expr {
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::BlockExpr(expr),
                        ..
                    },
                ..
            } => self.block(diag, expr),
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::IfExpr(expr),
                        ..
                    },
                ..
            } => self.if_(diag, expr),
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::OperatorExpr(expr),
                        ..
                    },
                span,
            } => self.operator(diag, expr, *span),
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::MemberExpr(expr),
                        ..
                    },
                span,
            } => self.member(diag, expr, *span),
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::IndexExpr(expr),
                        ..
                    },
                span,
            } => self.index(diag, expr, *span),
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::CallExpr(expr),
                        ..
                    },
                span,
            } => self.call(diag, expr, *span),
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::IdentExpr(expr),
                        ..
                    },
                span,
            } => self.ident(diag, expr, *span),
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::LitExpr(expr),
                        ..
                    },
                ..
            } => self.lit(expr),
        };
        if let Some(ty) = ty {
            expr.value.ty = ty;
        }
        ty
    }

    fn block(&mut self, diag: &mut dyn DiagnosticConsumer, expr: &mut BlockExpr) -> Option<Index> {
        if let Some(expr) = &mut expr.expr {
            self.tc(diag, expr, None)
        } else {
            Some(self.types.unit())
        }
    }

    fn if_(&mut self, diag: &mut dyn DiagnosticConsumer, expr: &mut IfExpr) -> Option<Index> {
        self.tc(diag, &mut expr.condition, Some(self.types.bool()))?;
        let then = self.tc(
            diag,
            &mut expr.then,
            if expr.else_.is_some() {
                None
            } else {
                Some(self.types.unit())
            },
        );
        if let Some(else_) = &mut expr.else_ {
            self.tc(diag, else_, Some(then.unwrap_or(self.types.unknown())))
        } else {
            then
        }
    }

    fn operator(
        &mut self,
        diag: &mut dyn DiagnosticConsumer,
        expr: &mut OperatorExpr,
        span: SourceSpan,
    ) -> Option<Index> {
        let types = expr
            .args
            .iter_mut()
            .map(|ty| self.expr(diag, ty))
            .collect::<Vec<_>>();
        if types.iter().any(|ty| ty.is_none()) {
            return None;
        }
        let types = types.into_iter().map(|ty| ty.unwrap()).collect::<Vec<_>>();
        match self
            .operators
            .candidate_definitions(expr.operator, types.as_slice(), self.types)
        {
            Ok(defs) => {
                if let Some(def) = defs.into_iter().find(|d| d.params.eq(&types)) {
                    match expr {
                        OperatorExpr { operator, .. } if (*operator).eq("=") => {
                            if self.assign(diag, expr) {
                                Some(def.return_type)
                            } else {
                                None
                            }
                        }
                        _ => Some(def.return_type),
                    }
                } else {
                    diag.error(LexionDiagnosticError {
                        src: self.src.clone(),
                        span,
                        message: format!(
                            "no matching definition for operator '{}' with operands [{}]",
                            expr.operator,
                            types
                                .iter()
                                .map(|ty| self.types.to_string_index(*ty))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    });
                    None
                }
            }
            Err(err) => {
                diag.error(err);
                None
            }
        }
    }

    fn member(
        &mut self,
        diag: &mut dyn DiagnosticConsumer,
        expr: &mut MemberExpr,
        span: SourceSpan,
    ) -> Option<Index> {
        let mut ty_idx = self.expr(diag, &mut expr.expr)?;

        ty_idx = self.types.canonicalize(ty_idx);
        ty_idx = self.types.dereference_all(ty_idx);
        ty_idx = self.types.canonicalize(ty_idx);
        let ty = &self.types[ty_idx];
        match ty {
            Type::StructType(ty) => {
                if let Some(member) = ty.members.iter().find(|m| m.name == expr.ident.as_str()) {
                    return Some(member.ty);
                }
            }
            Type::TupleType(ty) => {
                if let Ok(idx) = expr.ident.parse::<usize>() {
                    if let Some(ty) = ty.types.get(idx) {
                        return Some(*ty);
                    }
                }
            }
            _ => {}
        }

        diag.error(LexionDiagnosticError {
            src: self.src.clone(),
            span,
            message: format!(
                "type '{}' has no member '{}'",
                self.types.to_string_index(ty_idx),
                expr.ident
            ),
        });

        None
    }

    #[allow(unused)]
    fn index(
        &mut self,
        diag: &mut dyn DiagnosticConsumer,
        expr: &mut IndexExpr,
        span: SourceSpan,
    ) -> Option<Index> {
        // TODO
        None
    }

    fn call(
        &mut self,
        diag: &mut dyn DiagnosticConsumer,
        expr: &mut CallExpr,
        span: SourceSpan,
    ) -> Option<Index> {
        let Some(fty_idx) = self.tc(diag, &mut expr.expr, None) else {
            diag.error(LexionDiagnosticError {
                src: self.src.clone(),
                span: expr.expr.span,
                message: String::from("unknown expression type"),
            });
            return None;
        };
        let Some(fty) = self.types.get(fty_idx).and_then(|ty| match ty {
            Type::FunctionType(ty) => Some(ty.clone()),
            _ => None,
        }) else {
            diag.error(LexionDiagnosticError {
                src: self.src.clone(),
                span,
                message: format!(
                    "illegal function call on non-callable type '{}'",
                    self.types.to_string_index(fty_idx)
                ),
            });
            return None;
        };
        if !fty.is_vararg && fty.params.len() != expr.args.len() {
            diag.error(LexionDiagnosticError {
                src: self.src.clone(),
                span,
                message: format!(
                    "function of type '{}' called with {} argument(s), but expected {}",
                    self.types.to_string_index(fty_idx),
                    expr.args.len(),
                    fty.params.len()
                ),
            });
            return None;
        } else if fty.is_vararg && fty.params.len() > expr.args.len() {
            diag.error(LexionDiagnosticError {
                src: self.src.clone(),
                span,
                message: format!(
                    "function of type '{}' called with {} argument(s), but expected at least {}",
                    self.types.to_string_index(fty_idx),
                    expr.args.len(),
                    fty.params.len()
                ),
            });
            return None;
        }
        for (arg, ty) in std::iter::zip(
            expr.args.iter_mut(),
            fty.params
                .into_iter()
                .map(Some)
                .chain(std::iter::repeat(None)),
        ) {
            self.tc(diag, arg, ty)?;
        }
        Some(fty.return_type)
    }

    fn ident(
        &mut self,
        diag: &mut dyn DiagnosticConsumer,
        expr: &IdentExpr,
        span: SourceSpan,
    ) -> Option<Index> {
        if let Some((_, _, entry)) = self.table.lookup(self.current_scope, expr.ident.as_str()) {
            entry.var_type
        } else {
            diag.error(LexionDiagnosticError {
                src: self.src.clone(),
                span,
                message: format!("undeclared identifier '{}'", expr.ident),
            });
            None
        }
    }

    fn lit(&mut self, expr: &LitExpr) -> Option<Index> {
        Some(match &expr.lit {
            Lit::Integer(_) => self.types.i32(),
            Lit::Float(_) => self.types.f32(),
            Lit::String(_) => self.types.str_ref(),
            Lit::Boolean(_) => self.types.bool(),
        })
    }

    fn assign(&mut self, diag: &mut dyn DiagnosticConsumer, expr: &OperatorExpr) -> bool {
        let left = &expr.args[0];
        if !Self::is_assignable(left) {
            diag.error(LexionDiagnosticError {
                src: self.src.clone(),
                span: left.span,
                message: String::from("lvalue required as left operand of assignment"),
            });
            false
        } else {
            true
        }
    }

    fn is_assignable(expr: &SourcedExpr) -> bool {
        let mut result = true;
        AstVisitor::new().visit_expr(expr, NodeType::Root, &mut |ty, node, _| match (ty, node) {
            (
                TraversalType::Postorder,
                AstNode::Expr(Sourced {
                    value:
                        TypedExpr {
                            expr: Expr::IdentExpr(_) | Expr::MemberExpr(_) | Expr::IndexExpr(_),
                            ..
                        },
                    ..
                }),
            ) => AstVisitorAction::Continue,
            (
                TraversalType::Postorder,
                AstNode::Expr(Sourced {
                    value:
                        TypedExpr {
                            expr: Expr::OperatorExpr(OperatorExpr { operator, args }),
                            ..
                        },
                    ..
                }),
            ) if ((*operator).eq(operators::DEREFERENCE)
                || (*operator).eq(operators::ADDRESS_OF))
                && args.len() == 1 =>
            {
                AstVisitorAction::Continue
            }
            (TraversalType::Preorder, _) => AstVisitorAction::Continue,
            _ => {
                if let AstNode::Expr(expr) = node {
                    println!("{expr:?}");
                }
                result = false;
                AstVisitorAction::Terminate
            }
        });
        result
    }

    fn init_operators(&mut self) {
        let i32 = self.types.i32();
        let i32_ref = self.types.reference(i32);
        let u32 = self.types.u32();
        let u32_ref = self.types.reference(u32);
        let f32 = self.types.f32();
        let f32_ref = self.types.reference(f32);
        let bool = self.types.bool();

        self.operators.add_definition_multiple(
            &["--", "++"],
            &[
                // Prefix operators
                FunctionType {
                    params: vec![u32_ref],
                    return_type: u32,
                    is_vararg: false,
                },
                FunctionType {
                    params: vec![i32_ref],
                    return_type: i32,
                    is_vararg: false,
                },
                FunctionType {
                    params: vec![f32_ref],
                    return_type: f32,
                    is_vararg: false,
                },
                // Postfix w/ dummy int parameter
                FunctionType {
                    params: vec![u32_ref, u32],
                    return_type: u32,
                    is_vararg: false,
                },
                FunctionType {
                    params: vec![i32_ref, i32],
                    return_type: i32,
                    is_vararg: false,
                },
                FunctionType {
                    params: vec![f32_ref, u32],
                    return_type: f32,
                    is_vararg: false,
                },
            ],
        );

        // Unary plus / minus operators
        self.operators.add_definition_multiple(
            &["+", "-"],
            &[
                FunctionType {
                    params: vec![u32],
                    return_type: u32,
                    is_vararg: false,
                },
                FunctionType {
                    params: vec![i32],
                    return_type: i32,
                    is_vararg: false,
                },
                FunctionType {
                    params: vec![f32],
                    return_type: f32,
                    is_vararg: false,
                },
            ],
        );

        // Logical Not operator
        self.operators.add_definition(
            String::from("!"),
            FunctionType {
                params: vec![bool],
                return_type: bool,
                is_vararg: false,
            },
        );

        // Bitwise Not operator
        self.operators.add_definition_multiple(
            &["~"],
            &[
                FunctionType {
                    params: vec![u32],
                    return_type: u32,
                    is_vararg: false,
                },
                FunctionType {
                    params: vec![i32],
                    return_type: i32,
                    is_vararg: false,
                },
            ],
        );

        // Arithmetic operators
        self.operators.add_definition_multiple(
            &["*", "/", "%", "+", "-"],
            &[
                FunctionType {
                    params: vec![u32, u32],
                    return_type: u32,
                    is_vararg: false,
                },
                FunctionType {
                    params: vec![i32, i32],
                    return_type: i32,
                    is_vararg: false,
                },
                FunctionType {
                    params: vec![f32, f32],
                    return_type: f32,
                    is_vararg: false,
                },
            ],
        );

        // Shift & Bitwise operators
        self.operators.add_definition_multiple(
            &["<<", ">>", "&", "|", "^"],
            &[
                FunctionType {
                    params: vec![u32, u32],
                    return_type: u32,
                    is_vararg: false,
                },
                FunctionType {
                    params: vec![i32, i32],
                    return_type: i32,
                    is_vararg: false,
                },
            ],
        );

        // Relational and equality operators
        self.operators.add_definition_multiple(
            &["<", ">", "<=", ">=", "==", "!="],
            &[
                FunctionType {
                    params: vec![u32, u32],
                    return_type: bool,
                    is_vararg: false,
                },
                FunctionType {
                    params: vec![i32, i32],
                    return_type: bool,
                    is_vararg: false,
                },
                FunctionType {
                    params: vec![f32, f32],
                    return_type: bool,
                    is_vararg: false,
                },
            ],
        );

        // Logical operators
        self.operators.add_definition_multiple(
            &["&&", "||"],
            &[FunctionType {
                params: vec![bool, bool],
                return_type: bool,
                is_vararg: false,
            }],
        );

        self.operators.add_rule(
            String::from("="),
            Box::new(|_, list| {
                let left = list[0];
                Ok(vec![FunctionType {
                    params: vec![left, left],
                    return_type: left,
                    is_vararg: false,
                }])
            }),
        );

        self.operators.add_rule(
            String::from("&"),
            Box::new(|types, list| {
                let expr = list[0];
                Ok(vec![FunctionType {
                    params: vec![expr],
                    return_type: types.reference(expr),
                    is_vararg: false,
                }])
            }),
        );

        self.operators.add_rule(
            String::from("*"),
            Box::new(|types, list| {
                let expr = list[0];
                if let Some(inner) = types.dereference(expr) {
                    Ok(vec![FunctionType {
                        params: vec![expr],
                        return_type: inner,
                        is_vararg: false,
                    }])
                } else {
                    Ok(vec![])
                }
            }),
        );

        self.operators.add_rule(
            String::from("?:"),
            Box::new(move |_, list| {
                let true_expr = list[1];
                Ok(vec![FunctionType {
                    params: vec![bool, true_expr, true_expr],
                    return_type: true_expr,
                    is_vararg: false,
                }])
            }),
        )
    }
}

impl<'a> TypeChecker<'a> {
    fn begin_func_decl(&mut self, decl: &mut FuncDeclStmt) {
        if let Some((_, _, entry)) = self.table.lookup(self.current_scope, decl.name.as_str()) {
            self.current_scope = entry.table.unwrap();
        }
        self.block_scope_counts.push(0);
    }

    fn begin_block(&mut self) {
        let name = if let Some(table) = self.table.graph.node_weight(self.current_scope) {
            format!(
                "{}.{}",
                table.name,
                self.block_scope_counts
                    .iter()
                    .last()
                    .cloned()
                    .unwrap_or(0usize)
            )
        } else {
            String::from("")
        };
        if let Some((_, _, entry)) = self.table.lookup(self.current_scope, name.as_str()) {
            self.current_scope = entry.table.unwrap();
        }
        if let Some(block_scope_count) = self.block_scope_counts.iter_mut().last() {
            *block_scope_count += 1;
        }
        self.block_scope_counts.push(0);
    }

    fn begin_expr(&mut self, diag: &mut dyn DiagnosticConsumer, expr: &mut Box<SourcedExpr>) {
        self.tc(diag, expr.as_mut(), None);
    }

    fn begin_var_decl(&mut self, diag: &mut dyn DiagnosticConsumer, decl: &mut VarDecl) {
        let mut ty = decl
            .ty
            .as_ref()
            .and_then(|ty| self.types.insert_ast_type(&ty.value));
        if let Some(entry) = self
            .table
            .lookup_mut(self.current_scope, decl.name.value.as_str())
        {
            entry.var_type = ty;
        }
        if let Some(init) = &mut decl.init {
            ty = self.tc(diag, init, ty);
        }
        if let Some(entry) = self
            .table
            .lookup_mut(self.current_scope, decl.name.value.as_str())
        {
            if entry.var_type.is_none() {
                entry.var_type = ty;
            }
        }
    }

    fn begin_return_stmt(
        &mut self,
        diag: &mut dyn DiagnosticConsumer,
        stmt: &mut ReturnStmt,
        span: SourceSpan,
    ) {
        let Some(parent_ty) = self
            .table
            .parent_entry(self.current_scope)
            .and_then(|(_, _, entry)| entry.var_type)
        else {
            diag.error(LexionDiagnosticError {
                src: self.src.clone(),
                span,
                message: String::from("return statement outside of function"),
            });
            return;
        };
        let Some(fn_ret_ty) = self.types.get(parent_ty).and_then(|ty| {
            if let Type::FunctionType(FunctionType {
                return_type: fn_ret_ty,
                ..
            }) = ty
            {
                Some(*fn_ret_ty)
            } else {
                None
            }
        }) else {
            diag.error(LexionDiagnosticError {
                src: self.src.clone(),
                span,
                message: String::from("expected return statement to be within a function"),
            });
            return;
        };
        if let Some(expr) = &mut stmt.expr {
            self.tc(diag, expr, Some(fn_ret_ty));
        } else {
            let unit = self.types.unit();
            self.expect(diag, span, unit, fn_ret_ty);
        }
    }

    fn begin_struct_stmt(&mut self, stmt: &StructDeclStmt) {}

    fn begin_while_stmt(&mut self, diag: &mut dyn DiagnosticConsumer, stmt: &mut WhileStmt) {
        self.tc(diag, &mut stmt.condition, Some(self.types.bool()));
        stmt.body.ty = self.types.unit();
    }

    fn end_block(&mut self) {
        if let Some(parent) = self.table.graph.neighbors(self.current_scope).next() {
            self.current_scope = parent;
        }
        self.block_scope_counts.pop();
    }
}

impl<'a> PipelineStage for TypeChecker<'a> {
    type Input = (
        NamedSource<Arc<String>>,
        &'a mut SymbolTableGraph,
        &'a mut TypeCollection,
    );
    type Options = &'a mut Ast;
    type Output = ();

    fn new((src, table, types): Self::Input) -> Self {
        let mut tc = Self {
            src,
            table,
            types,
            operators: Default::default(),
            current_scope: Default::default(),
            block_scope_counts: Default::default(),
        };
        tc.init_operators();
        tc
    }

    fn exec(
        mut self,
        diag: &mut dyn DiagnosticConsumer,
        ast: Self::Options,
    ) -> Option<Self::Output> {
        AstVisitor::new().visit_mut(ast, |ty, node, _| {
            match (ty, node) {
                (
                    TraversalType::Preorder,
                    AstNodeMut::Stmt(Sourced {
                        value: Stmt::FuncDeclStmt(decl),
                        ..
                    }),
                ) => self.begin_func_decl(decl),
                (
                    TraversalType::Postorder,
                    AstNodeMut::Stmt(Sourced {
                        value: Stmt::FuncDeclStmt(_),
                        ..
                    }),
                ) => self.end_block(),
                (
                    TraversalType::Preorder,
                    AstNodeMut::Expr(Sourced {
                        value:
                            TypedExpr {
                                expr: Expr::BlockExpr(_),
                                ..
                            },
                        ..
                    }),
                ) => self.begin_block(),
                (
                    TraversalType::Postorder,
                    AstNodeMut::Expr(Sourced {
                        value:
                            TypedExpr {
                                expr: Expr::BlockExpr(_),
                                ..
                            },
                        ..
                    }),
                ) => self.end_block(),
                (
                    TraversalType::Preorder,
                    AstNodeMut::Stmt(Sourced {
                        value: Stmt::ExprStmt(ExprStmt { expr }),
                        ..
                    }),
                ) => self.begin_expr(diag, expr),
                (
                    TraversalType::Preorder,
                    AstNodeMut::Stmt(Sourced {
                        value:
                            Stmt::VarDeclStmt(VarDeclStmt {
                                decl: Sourced { value: decl, .. },
                                ..
                            }),
                        ..
                    }),
                ) => self.begin_var_decl(diag, decl),
                (
                    TraversalType::Preorder,
                    AstNodeMut::Stmt(Sourced {
                        value: Stmt::StructDeclStmt(stmt),
                        ..
                    }),
                ) => self.begin_struct_stmt(stmt),
                (
                    TraversalType::Preorder,
                    AstNodeMut::Stmt(Sourced {
                        value: Stmt::ReturnStmt(stmt),
                        span,
                    }),
                ) => self.begin_return_stmt(diag, stmt, *span),
                (
                    TraversalType::Preorder,
                    AstNodeMut::Stmt(Sourced {
                        value: Stmt::WhileStmt(stmt),
                        ..
                    }),
                ) => self.begin_while_stmt(diag, stmt),
                _ => {}
            }
            AstVisitorAction::Continue
        });
        Some(())
    }
}
