use std::collections::HashSet;
use std::sync::Arc;

use generational_arena::Index;

use lexion_lib::miette::{NamedSource, SourceSpan};
use lexion_lib::petgraph::graph::NodeIndex;

use crate::ast::types::{FunctionType, PrimitiveType, TupleType, Type, TypeCollection};
use crate::ast::visitor::{AstNodeMut, AstVisitor, AstVisitorAction, TraversalType};
use crate::ast::{
    Ast, BlockExpr, CallExpr, CastExpr, Expr, ExprStmt, FuncDeclStmt, IdentExpr, IfExpr, IndexExpr,
    Lit, LitExpr, MemberExpr, OperatorExpr, ReturnStmt, Sourced, SourcedExpr, Stmt, StructDeclStmt,
    StructExpr, TupleExpr, TypedExpr, VarDecl, VarDeclStmt, WhileStmt,
};
use crate::diagnostic::{DiagnosticConsumer, LexionDiagnosticError};
use crate::operators;
use crate::pipeline::PipelineStage;
use crate::symbol_table::{SymbolTableEntryType, SymbolTableGraph};
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
                        expr: Expr::CastExpr(expr),
                        ..
                    },
                span,
            } => self.cast(diag, expr, *span),
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
                        expr: Expr::StructExpr(expr),
                        ..
                    },
                span,
            } => self.struct_(diag, expr, *span),
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::TupleExpr(expr),
                        ..
                    },
                ..
            } => self.tuple(diag, expr),
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
        if let Some(else_) = &mut expr.else_ {
            let then = self.tc(diag, &mut expr.then, None);
            self.tc(diag, else_, Some(then.unwrap_or(self.types.unknown())))
        } else {
            self.tc(diag, &mut expr.then, Some(self.types.unit()));
            Some(self.types.unit())
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
        if expr.operator == operators::ASSIGN && !self.assign(diag, expr) {
            return None;
        }
        match self
            .operators
            .candidate_definitions(expr.operator, types.as_slice(), self.types)
        {
            Ok(defs) => {
                if let Some(def) = defs.into_iter().find(|d| d.params.eq(&types)) {
                    Some(def.return_type)
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

    fn index(
        &mut self,
        diag: &mut dyn DiagnosticConsumer,
        expr: &mut IndexExpr,
        span: SourceSpan,
    ) -> Option<Index> {
        let base_ty = self.expr(diag, &mut expr.expr)?;
        let index_ty = self.expr(diag, &mut expr.index)?;
        if !self.is_integer_index(index_ty) {
            diag.error(LexionDiagnosticError {
                src: self.src.clone(),
                span: expr.index.span,
                message: format!(
                    "index expression must be an integer, instead got '{}'",
                    self.types.to_string_index(index_ty)
                ),
            });
            return None;
        }

        let base_ty = self.types.canonicalize(base_ty);
        let indexed_ty = self.types.dereference_all(base_ty);
        match self.types.get(indexed_ty) {
            Some(Type::PrimitiveType(PrimitiveType::STR)) => Some(self.types.char()),
            _ => {
                diag.error(LexionDiagnosticError {
                    src: self.src.clone(),
                    span,
                    message: format!(
                        "type '{}' cannot be indexed",
                        self.types.to_string_index(base_ty)
                    ),
                });
                None
            }
        }
    }

    fn cast(
        &mut self,
        diag: &mut dyn DiagnosticConsumer,
        expr: &mut CastExpr,
        span: SourceSpan,
    ) -> Option<Index> {
        let from_ty = self.expr(diag, &mut expr.expr)?;
        let Some(to_ty) = self.types.insert_ast_type(&expr.ty.value) else {
            diag.error(LexionDiagnosticError {
                src: self.src.clone(),
                span: expr.ty.span,
                message: String::from("unknown cast target type"),
            });
            return None;
        };

        if self.types.eq(from_ty, to_ty)
            || (self.is_scalar_cast_type(from_ty) && self.is_scalar_cast_type(to_ty))
            || (self.is_reference_type(from_ty) && self.is_reference_type(to_ty))
        {
            Some(to_ty)
        } else {
            diag.error(LexionDiagnosticError {
                src: self.src.clone(),
                span,
                message: format!(
                    "cannot cast type '{}' to '{}'",
                    self.types.to_string_index(from_ty),
                    self.types.to_string_index(to_ty)
                ),
            });
            None
        }
    }

    fn is_integer_index(&self, ty: Index) -> bool {
        matches!(
            self.types.get(self.types.canonicalize(ty)),
            Some(Type::PrimitiveType(PrimitiveType::I32 | PrimitiveType::U32))
        )
    }

    fn is_scalar_cast_type(&self, ty: Index) -> bool {
        matches!(
            self.types.get(self.types.canonicalize(ty)),
            Some(Type::PrimitiveType(
                PrimitiveType::U32
                    | PrimitiveType::I32
                    | PrimitiveType::F32
                    | PrimitiveType::BOOL
                    | PrimitiveType::CHAR
            ))
        )
    }

    fn is_reference_type(&self, ty: Index) -> bool {
        matches!(
            self.types.get(self.types.canonicalize(ty)),
            Some(Type::RefType(_))
        )
    }

    fn call(
        &mut self,
        diag: &mut dyn DiagnosticConsumer,
        expr: &mut CallExpr,
        span: SourceSpan,
    ) -> Option<Index> {
        let fty_idx = self.tc(diag, &mut expr.expr, None)?;
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

    fn struct_(
        &mut self,
        diag: &mut dyn DiagnosticConsumer,
        expr: &mut StructExpr,
        span: SourceSpan,
    ) -> Option<Index> {
        let struct_ty = self
            .table
            .lookup(self.current_scope, expr.name.value.as_str())
            .and_then(|(_, _, entry)| {
                (entry.ty == SymbolTableEntryType::Struct)
                    .then_some(entry.var_type)
                    .flatten()
            });
        let struct_ = struct_ty.and_then(|ty| match self.types.get(self.types.canonicalize(ty)) {
            Some(Type::StructType(struct_)) => Some(struct_.clone()),
            _ => None,
        });
        let Some((struct_ty, struct_)) = struct_ty.zip(struct_) else {
            diag.error(LexionDiagnosticError {
                src: self.src.clone(),
                span: expr.name.span,
                message: format!("unknown struct '{}'", expr.name.value),
            });
            for field in &mut expr.fields {
                self.tc(diag, &mut field.value.expr, None);
            }
            return None;
        };

        let mut valid = true;
        let mut seen = HashSet::new();
        for field in &mut expr.fields {
            let name = field.value.name.value.as_str();
            if !seen.insert(name.to_owned()) {
                diag.error(LexionDiagnosticError {
                    src: self.src.clone(),
                    span: field.value.name.span,
                    message: format!(
                        "duplicate field '{}' in struct literal '{}'",
                        name, expr.name.value
                    ),
                });
                valid = false;
            }

            let Some(member) = struct_.members.iter().find(|member| member.name == name) else {
                diag.error(LexionDiagnosticError {
                    src: self.src.clone(),
                    span: field.value.name.span,
                    message: format!("struct '{}' has no field '{}'", expr.name.value, name),
                });
                self.tc(diag, &mut field.value.expr, None);
                valid = false;
                continue;
            };

            match self.expr(diag, &mut field.value.expr) {
                Some(ty) if !self.types.eq(ty, member.ty) => {
                    diag.error(LexionDiagnosticError {
                        src: self.src.clone(),
                        span: field.value.expr.span,
                        message: format!(
                            "field '{}' of struct '{}' expects type '{}', instead got '{}'",
                            name,
                            expr.name.value,
                            self.types.to_string_index(member.ty),
                            self.types.to_string_index(ty)
                        ),
                    });
                    valid = false;
                }
                None => valid = false,
                Some(_) => {}
            }
        }

        for member in &struct_.members {
            if !seen.contains(member.name.as_str()) {
                diag.error(LexionDiagnosticError {
                    src: self.src.clone(),
                    span,
                    message: format!(
                        "missing field '{}' in struct literal '{}'",
                        member.name, expr.name.value
                    ),
                });
                valid = false;
            }
        }

        valid.then_some(struct_ty)
    }

    fn tuple(&mut self, diag: &mut dyn DiagnosticConsumer, expr: &mut TupleExpr) -> Option<Index> {
        let types = expr
            .values
            .iter_mut()
            .map(|value| self.tc(diag, value, None))
            .collect::<Option<Vec<_>>>()?;
        Some(self.types.insert(&Type::TupleType(TupleType { types })))
    }

    fn ident(
        &mut self,
        diag: &mut dyn DiagnosticConsumer,
        expr: &IdentExpr,
        span: SourceSpan,
    ) -> Option<Index> {
        if let Some((_, _, entry)) = self.table.lookup(self.current_scope, expr.ident.as_str()) {
            if entry.ty == SymbolTableEntryType::Struct {
                diag.error(LexionDiagnosticError {
                    src: self.src.clone(),
                    span,
                    message: format!(
                        "struct declaration '{}' is not a value; use a named struct literal",
                        expr.ident
                    ),
                });
                None
            } else {
                entry.var_type
            }
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
        if Self::is_identifier_lvalue(left) || Self::is_place_expression(left) {
            true
        } else {
            diag.error(LexionDiagnosticError {
                src: self.src.clone(),
                span: left.span,
                message: String::from("lvalue required as left operand of assignment"),
            });
            false
        }
    }

    fn is_identifier_lvalue(expr: &SourcedExpr) -> bool {
        matches!(
            expr,
            Sourced {
                value: TypedExpr {
                    expr: Expr::IdentExpr(_),
                    ..
                },
                ..
            }
        )
    }

    fn is_place_expression(expr: &SourcedExpr) -> bool {
        match expr {
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::MemberExpr(_) | Expr::IndexExpr(_),
                        ..
                    },
                ..
            } => true,
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::OperatorExpr(OperatorExpr { operator, args }),
                        ..
                    },
                ..
            } => (*operator).eq(operators::DEREFERENCE) && args.len() == 1,
            _ => false,
        }
    }

    fn init_operators(&mut self) {
        let i32 = self.types.i32();
        let u32 = self.types.u32();
        let f32 = self.types.f32();
        let bool = self.types.bool();
        let char = self.types.char();

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
        self.operators.add_definition_multiple(
            &["==", "!="],
            &[FunctionType {
                params: vec![char, char],
                return_type: bool,
                is_vararg: false,
            }],
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
        let mut scope = self.current_scope;
        let mut fn_ret_ty = None;
        while let Some((parent_scope, _, entry)) = self.table.parent_entry(scope) {
            if let Some(parent_ty) = entry.var_type {
                if let Some(Type::FunctionType(FunctionType { return_type, .. })) =
                    self.types.get(parent_ty)
                {
                    fn_ret_ty = Some(*return_type);
                    break;
                }
            }
            scope = parent_scope;
        }
        let Some(fn_ret_ty) = fn_ret_ty else {
            diag.error(LexionDiagnosticError {
                src: self.src.clone(),
                span,
                message: String::from("return statement outside of function"),
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

    fn begin_struct_stmt(&mut self, _stmt: &StructDeclStmt) {}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn type_checker_with_operators<'a>(
        table: &'a mut SymbolTableGraph,
        types: &'a mut TypeCollection,
    ) -> TypeChecker<'a> {
        TypeChecker::new((
            NamedSource::new("<test>", Arc::new(String::new())),
            table,
            types,
        ))
    }

    #[test]
    fn increment_and_decrement_are_not_language_operators() {
        let mut table = SymbolTableGraph::default();
        let mut types = TypeCollection::default();
        let tc = type_checker_with_operators(&mut table, &mut types);
        let i32_ref = tc.types.reference(tc.types.i32());

        assert!(tc
            .operators
            .candidate_definitions("++", &[i32_ref], tc.types)
            .unwrap()
            .is_empty());
        assert!(tc
            .operators
            .candidate_definitions("--", &[i32_ref], tc.types)
            .unwrap()
            .is_empty());
    }
}
