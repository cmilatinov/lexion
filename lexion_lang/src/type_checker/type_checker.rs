use std::sync::Arc;

use generational_arena::Index;

use lexion_lib::miette::{NamedSource, SourceSpan};
use lexion_lib::petgraph::graph::NodeIndex;

use crate::ast::{
    ASTNode, ASTVisitor, CallExpr, Expr, ExprStmt, FunctionType, IdentExpr, IndexExpr, Lit,
    LitExpr, MemberExpr, OperatorExpr, Sourced, SourcedExpr, Stmt, TraversalType, TypeCollection,
    VarDeclStmt, AST, TYPE_BOOL, TYPE_F32, TYPE_STR, TYPE_U32, TYPE_UNIT,
};
use crate::diagnostic::{DiagnosticConsumer, LexionDiagnosticError};
use crate::pipeline::PipelineStage;
use crate::symbol_table::SymbolTableGraph;
use crate::type_checker::operator_table::OperatorTable;

pub struct TypeChecker<'a> {
    src: NamedSource<Arc<String>>,
    ast: &'a AST,
    table: &'a mut SymbolTableGraph,
    types: &'a mut TypeCollection,
    operators: OperatorTable,
    current_scope: NodeIndex,
    block_scope_count: usize,
}

impl<'a> TypeChecker<'a> {
    fn tc(
        &mut self,
        diag: &mut dyn DiagnosticConsumer,
        expr: &SourcedExpr,
        expected: Option<Index>,
    ) -> Option<Index> {
        if let Some(ty) = self.expr(diag, expr) {
            if let Some(expected) = expected {
                if !self.types.eq(ty, expected) {
                    diag.error(LexionDiagnosticError {
                        src: self.src.clone(),
                        span: expr.span,
                        message: format!(
                            "expected type '{}', instead got '{}'",
                            self.types.to_string_index(expected),
                            self.types.to_string_index(ty)
                        ),
                    });
                }
            }
            return Some(ty);
        }
        None
    }

    fn expr(&mut self, diag: &mut dyn DiagnosticConsumer, expr: &SourcedExpr) -> Option<Index> {
        match expr {
            Sourced {
                value: Expr::NoneExpr,
                ..
            } => Some(self.types.insert(&TYPE_UNIT)),
            Sourced {
                value: Expr::LitExpr(expr),
                ..
            } => self.lit(expr),
            Sourced {
                value: Expr::OperatorExpr(expr),
                span,
            } => self.operator(diag, expr, *span),
            Sourced {
                value: Expr::MemberExpr(expr),
                span,
            } => self.member(diag, expr, *span),
            Sourced {
                value: Expr::IndexExpr(expr),
                span,
            } => self.index(diag, expr, *span),
            Sourced {
                value: Expr::CallExpr(expr),
                span,
            } => self.call(diag, expr, *span),
            Sourced {
                value: Expr::IdentExpr(expr),
                span,
            } => self.ident(diag, expr, *span),
        }
    }

    fn operator(
        &mut self,
        diag: &mut dyn DiagnosticConsumer,
        expr: &OperatorExpr,
        span: SourceSpan,
    ) -> Option<Index> {
        let types = expr
            .args
            .iter()
            .map(|ty| self.expr(diag, ty))
            .collect::<Vec<_>>();
        if types.iter().any(|ty| ty.is_none()) {
            return None;
        }
        let types = types.into_iter().map(|ty| ty.unwrap()).collect::<Vec<_>>();
        match self.operators.candidate_definitions(
            expr.operator.as_str(),
            types.as_slice(),
            self.types,
        ) {
            Ok(defs) => {
                if let Some(def) = defs.into_iter().find(|d| d.params.eq(&types)) {
                    match expr {
                        OperatorExpr { operator, .. } if operator.as_str().eq("=") => {
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
        expr: &MemberExpr,
        span: SourceSpan,
    ) -> Option<Index> {
        // TODO
        None
    }

    fn index(
        &mut self,
        diag: &mut dyn DiagnosticConsumer,
        expr: &IndexExpr,
        span: SourceSpan,
    ) -> Option<Index> {
        // TODO
        None
    }

    fn call(
        &mut self,
        diag: &mut dyn DiagnosticConsumer,
        expr: &CallExpr,
        span: SourceSpan,
    ) -> Option<Index> {
        // TODO
        None
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
                message: format!("identifier '{}' not found ", expr.ident),
            });
            None
        }
    }

    fn lit(&mut self, expr: &LitExpr) -> Option<Index> {
        Some(match &expr.lit {
            Lit::Integer(_) => self.types.insert(&TYPE_U32),
            Lit::Float(_) => self.types.insert(&TYPE_F32),
            Lit::String(_) => self.types.insert(&TYPE_STR),
            Lit::Boolean(_) => self.types.insert(&TYPE_BOOL),
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
        ASTVisitor::default().visit_expr(expr, &mut |ty, node| {
            if !result {
                return;
            }
            match (ty, node) {
                (
                    TraversalType::Postorder,
                    ASTNode::Expr(Sourced {
                        value: Expr::IdentExpr(_) | Expr::MemberExpr(_) | Expr::IndexExpr(_),
                        ..
                    }),
                ) => {}
                (
                    TraversalType::Postorder,
                    ASTNode::Expr(Sourced {
                        value: Expr::OperatorExpr(OperatorExpr { operator, args }),
                        ..
                    }),
                ) if (operator.as_str().eq("*") || operator.as_str().eq("&"))
                    && args.len() == 1 => {}
                (TraversalType::Preorder, _) => {}
                _ => {
                    match node {
                        ASTNode::Expr(expr) => println!("{:?}", expr),
                        _ => {}
                    };
                    result = false;
                }
            }
        });
        result
    }

    fn init_operators(&mut self) {
        let u32 = self.types.insert(&TYPE_U32);
        let u32_ref = self.types.reference(u32);
        let f32 = self.types.insert(&TYPE_F32);
        let f32_ref = self.types.reference(f32);
        let bool = self.types.insert(&TYPE_BOOL);

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
        self.operators.add_definition(
            String::from("~"),
            FunctionType {
                params: vec![u32],
                return_type: u32,
                is_vararg: false,
            },
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
                    params: vec![f32, f32],
                    return_type: f32,
                    is_vararg: false,
                },
            ],
        );

        // Shift & Bitwise operators
        self.operators.add_definition_multiple(
            &["<<", ">>", "&", "|", "^"],
            &[FunctionType {
                params: vec![u32, u32],
                return_type: u32,
                is_vararg: false,
            }],
        );

        // Relational and equality operators
        self.operators.add_definition_multiple(
            &["<", ">", "<=", ">=", "==", "!="],
            &[
                FunctionType {
                    params: vec![u32, u32],
                    return_type: u32,
                    is_vararg: false,
                },
                FunctionType {
                    params: vec![f32, f32],
                    return_type: f32,
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

impl<'a> PipelineStage for TypeChecker<'a> {
    type Input = (
        NamedSource<Arc<String>>,
        &'a AST,
        &'a mut SymbolTableGraph,
        &'a mut TypeCollection,
    );
    type Output = ();

    fn new((src, ast, table, types): Self::Input) -> Self {
        let mut tc = Self {
            src,
            ast,
            table,
            types,
            operators: Default::default(),
            current_scope: Default::default(),
            block_scope_count: 0,
        };
        tc.init_operators();
        tc
    }

    fn exec(mut self, diag: &mut dyn DiagnosticConsumer) -> Option<Self::Output> {
        ASTVisitor::default().visit(self.ast, |ty, node| match (ty, node) {
            (
                TraversalType::Preorder,
                ASTNode::Stmt(Sourced {
                    value: Stmt::FuncDeclStmt(decl),
                    ..
                }),
            ) => {
                if let Some((_, _, entry)) =
                    self.table.lookup(self.current_scope, decl.name.as_str())
                {
                    self.current_scope = entry.table.unwrap();
                }
            }
            (
                TraversalType::Preorder,
                ASTNode::Stmt(Sourced {
                    value: Stmt::BlockStmt(_),
                    ..
                }),
            ) => {
                let name = if let Some(table) = self.table.graph.node_weight(self.current_scope) {
                    format!("{}.{}", table.name, self.block_scope_count)
                } else {
                    String::from("")
                };
                if let Some((_, _, entry)) = self.table.lookup(self.current_scope, name.as_str()) {
                    self.current_scope = entry.table.unwrap();
                }
                self.block_scope_count += 1;
            }
            (
                TraversalType::Preorder,
                ASTNode::Stmt(Sourced {
                    value: Stmt::ExprStmt(ExprStmt { expr }),
                    ..
                }),
            ) => {
                self.tc(diag, &expr, None);
            }
            (
                TraversalType::Preorder,
                ASTNode::Stmt(Sourced {
                    value: Stmt::VarDeclStmt(VarDeclStmt { decls, .. }),
                    ..
                }),
            ) => {
                for Sourced { value: decl, .. } in decls {
                    let mut ty = decl.ty.as_ref().map(|ty| self.types.insert(&ty.value));
                    if let Some(entry) = self
                        .table
                        .lookup_mut(self.current_scope, decl.name.value.as_str())
                    {
                        entry.var_type = ty;
                    }
                    if let Some(init) = &decl.init {
                        ty = self.tc(diag, &init, ty);
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
            }
            (
                TraversalType::Postorder,
                ASTNode::Stmt(Sourced {
                    value: Stmt::FuncDeclStmt(_) | Stmt::BlockStmt(_),
                    ..
                }),
            ) => {
                if let Some(parent) = self.table.graph.neighbors(self.current_scope).next() {
                    self.current_scope = parent;
                }
                self.block_scope_count = 0;
            }
            _ => {}
        });
        Some(())
    }
}
