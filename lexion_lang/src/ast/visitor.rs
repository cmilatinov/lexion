use crate::ast::{
    Ast, BlockExpr, CallExpr, Expr, ExprStmt, FuncDeclStmt, IfExpr, IndexExpr, MemberExpr,
    OperatorExpr, ReturnStmt, Sourced, SourcedExpr, SourcedStmt, Stmt, StructDeclStmt, TypedExpr,
    VarDeclStmt, WhileStmt,
};

pub struct AstVisitor {
    if_exprs: bool,
    block_end_exprs: bool,
}

pub enum AstVisitorAction {
    Terminate,
    Continue,
}

#[derive(Clone, Copy)]
pub enum AstNode<'a> {
    Stmt(&'a SourcedStmt),
    Expr(&'a SourcedExpr),
}

pub enum AstNodeMut<'a> {
    Stmt(&'a mut SourcedStmt),
    Expr(&'a mut SourcedExpr),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TraversalType {
    Preorder,
    Postorder,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Root,
    Child,
    LastChild,
}

impl<T> From<Option<T>> for NodeType {
    fn from(value: Option<T>) -> Self {
        if value.is_none() {
            NodeType::LastChild
        } else {
            NodeType::Child
        }
    }
}

impl Default for AstVisitor {
    fn default() -> Self {
        Self::new()
    }
}

impl AstVisitor {
    pub fn new() -> Self {
        Self {
            if_exprs: true,
            block_end_exprs: true,
        }
    }

    pub fn without_ifs(mut self) -> Self {
        self.if_exprs = false;
        self
    }

    pub fn without_block_end_exprs(mut self) -> Self {
        self.block_end_exprs = false;
        self
    }
}

macro_rules! return_on_terminate {
    ($expr:expr) => {
        if let AstVisitorAction::Terminate = $expr {
            return;
        }
    };
}

impl AstVisitor {
    pub fn visit<F: FnMut(TraversalType, AstNode, NodeType) -> AstVisitorAction>(
        &self,
        ast: &Ast,
        mut visitor: F,
    ) {
        let mut iter = ast.iter().peekable();
        while let Some(stmt) = iter.next() {
            self.visit_stmt(stmt, iter.peek().into(), &mut visitor);
        }
    }

    pub fn visit_mut<F: FnMut(TraversalType, AstNodeMut, NodeType) -> AstVisitorAction>(
        &self,
        ast: &mut Ast,
        mut visitor: F,
    ) {
        let mut iter = ast.iter_mut().peekable();
        while let Some(stmt) = iter.next() {
            self.visit_stmt_mut(stmt, iter.peek().into(), &mut visitor);
        }
    }

    pub fn visit_stmt<F: FnMut(TraversalType, AstNode, NodeType) -> AstVisitorAction>(
        &self,
        stmt: &SourcedStmt,
        ty: NodeType,
        visitor: &mut F,
    ) {
        let node = AstNode::Stmt(stmt);
        return_on_terminate!(visitor(TraversalType::Preorder, node, ty));
        match stmt {
            Sourced {
                value: Stmt::FuncDeclStmt(FuncDeclStmt { body, .. }),
                ..
            } => {
                if let Some(body) = body {
                    self.visit_block_expr(body, NodeType::LastChild, false, visitor);
                }
            }
            Sourced {
                value: Stmt::VarDeclStmt(VarDeclStmt { decl }),
                ..
            } => {
                if let Some(expr) = &decl.init {
                    self.visit_expr(expr, NodeType::LastChild, visitor);
                }
            }
            Sourced {
                value: Stmt::ExprStmt(ExprStmt { expr }),
                ..
            } => {
                self.visit_expr(expr, NodeType::LastChild, visitor);
            }
            Sourced {
                value: Stmt::ReturnStmt(ReturnStmt { expr }),
                ..
            } => {
                if let Some(expr) = expr {
                    self.visit_expr(expr, NodeType::LastChild, visitor);
                }
            }
            Sourced {
                value: Stmt::StructDeclStmt(StructDeclStmt { .. }),
                ..
            } => {}
            Sourced {
                value: Stmt::WhileStmt(WhileStmt { condition, body }),
                ..
            } => {
                self.visit_expr(condition, NodeType::Child, visitor);
                self.visit_block_expr(body, NodeType::LastChild, true, visitor);
            }
        }
        return_on_terminate!(visitor(TraversalType::Postorder, node, ty));
    }

    pub fn visit_stmt_mut<F: FnMut(TraversalType, AstNodeMut, NodeType) -> AstVisitorAction>(
        &self,
        stmt: &mut SourcedStmt,
        ty: NodeType,
        visitor: &mut F,
    ) {
        return_on_terminate!(visitor(TraversalType::Preorder, AstNodeMut::Stmt(stmt), ty));
        match stmt {
            Sourced {
                value: Stmt::FuncDeclStmt(FuncDeclStmt { body, .. }),
                ..
            } => {
                if let Some(body) = body {
                    self.visit_block_expr_mut(body, NodeType::LastChild, false, visitor);
                }
            }
            Sourced {
                value: Stmt::VarDeclStmt(VarDeclStmt { decl }),
                ..
            } => {
                if let Some(expr) = &mut decl.init {
                    self.visit_expr_mut(expr, NodeType::LastChild, visitor);
                }
            }
            Sourced {
                value: Stmt::ExprStmt(ExprStmt { expr }),
                ..
            } => {
                self.visit_expr_mut(expr, NodeType::LastChild, visitor);
            }
            Sourced {
                value: Stmt::ReturnStmt(ReturnStmt { expr }),
                ..
            } => {
                if let Some(expr) = expr {
                    self.visit_expr_mut(expr, NodeType::LastChild, visitor);
                }
            }
            Sourced {
                value: Stmt::StructDeclStmt(StructDeclStmt { .. }),
                ..
            } => {}
            Sourced {
                value: Stmt::WhileStmt(WhileStmt { condition, body }),
                ..
            } => {
                self.visit_expr_mut(condition, NodeType::Child, visitor);
                self.visit_block_expr_mut(body, NodeType::LastChild, true, visitor);
            }
        }
        return_on_terminate!(visitor(
            TraversalType::Postorder,
            AstNodeMut::Stmt(stmt),
            ty
        ));
    }

    pub fn visit_block_expr<F: FnMut(TraversalType, AstNode, NodeType) -> AstVisitorAction>(
        &self,
        expr: &SourcedExpr,
        ty: NodeType,
        include_block: bool,
        visitor: &mut F,
    ) {
        let Sourced {
            value:
                TypedExpr {
                    expr:
                        Expr::BlockExpr(BlockExpr {
                            stmts,
                            expr: return_expr,
                        }),
                    ..
                },
            ..
        } = expr
        else {
            return;
        };
        if include_block {
            return_on_terminate!(visitor(TraversalType::Preorder, AstNode::Expr(expr), ty));
        }
        let has_return_expr = return_expr.is_some();
        let mut iter = stmts.iter().peekable();
        while let Some(stmt) = iter.next() {
            self.visit_stmt(
                stmt,
                if (!include_block && ty == NodeType::Child) || has_return_expr {
                    NodeType::Child
                } else {
                    iter.peek().into()
                },
                visitor,
            );
        }
        if let Some(expr) = return_expr {
            self.visit_expr(
                expr,
                if !include_block && ty == NodeType::Child {
                    NodeType::Child
                } else {
                    NodeType::LastChild
                },
                visitor,
            );
        }
        if include_block {
            return_on_terminate!(visitor(TraversalType::Postorder, AstNode::Expr(expr), ty));
        }
    }

    pub fn visit_block_expr_mut<
        F: FnMut(TraversalType, AstNodeMut, NodeType) -> AstVisitorAction,
    >(
        &self,
        expr: &mut SourcedExpr,
        ty: NodeType,
        include_block: bool,
        visitor: &mut F,
    ) {
        if !matches!(
            expr,
            Sourced {
                value: TypedExpr {
                    expr: Expr::BlockExpr(_),
                    ..
                },
                ..
            }
        ) {
            return;
        }
        if include_block {
            return_on_terminate!(visitor(TraversalType::Preorder, AstNodeMut::Expr(expr), ty));
        }
        let Sourced {
            value:
                TypedExpr {
                    expr:
                        Expr::BlockExpr(BlockExpr {
                            stmts,
                            expr: return_expr,
                        }),
                    ..
                },
            ..
        } = expr
        else {
            return;
        };
        let has_return_expr = return_expr.is_some();
        let mut iter = stmts.iter_mut().peekable();
        while let Some(stmt) = iter.next() {
            self.visit_stmt_mut(
                stmt,
                if (!include_block && ty == NodeType::Child) || has_return_expr {
                    NodeType::Child
                } else {
                    iter.peek().into()
                },
                visitor,
            );
        }
        if let Some(expr) = return_expr {
            self.visit_expr_mut(
                expr,
                if !include_block && ty == NodeType::Child {
                    NodeType::Child
                } else {
                    NodeType::LastChild
                },
                visitor,
            );
        }
        if include_block {
            return_on_terminate!(visitor(
                TraversalType::Postorder,
                AstNodeMut::Expr(expr),
                ty
            ));
        }
    }

    pub fn visit_expr<F: FnMut(TraversalType, AstNode, NodeType) -> AstVisitorAction>(
        &self,
        expr: &SourcedExpr,
        ty: NodeType,
        visitor: &mut F,
    ) {
        let node = AstNode::Expr(expr);
        return_on_terminate!(visitor(TraversalType::Preorder, node, ty));
        match expr {
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::BlockExpr(_),
                        ..
                    },
                ..
            } => {
                self.visit_block_expr(expr, ty, true, visitor);
            }
            Sourced {
                value:
                    TypedExpr {
                        expr:
                            Expr::IfExpr(IfExpr {
                                condition,
                                then,
                                else_,
                            }),
                        ..
                    },
                ..
            } => {
                if self.if_exprs {
                    self.visit_expr(condition, NodeType::Child, visitor);
                    self.visit_block_expr(
                        then,
                        if else_.is_none() {
                            NodeType::LastChild
                        } else {
                            NodeType::Child
                        },
                        true,
                        visitor,
                    );
                    if let Some(else_) = else_ {
                        self.visit_block_expr(else_, NodeType::LastChild, true, visitor);
                    }
                }
            }
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::OperatorExpr(OperatorExpr { args, .. }),
                        ..
                    },
                ..
            } => {
                let mut iter = args.iter().peekable();
                while let Some(arg) = iter.next() {
                    self.visit_expr(arg, iter.peek().into(), visitor);
                }
            }
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::MemberExpr(MemberExpr { expr, .. }),
                        ..
                    },
                ..
            } => {
                self.visit_expr(expr, NodeType::LastChild, visitor);
            }
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::IndexExpr(IndexExpr { expr, index }),
                        ..
                    },
                ..
            } => {
                self.visit_expr(expr, NodeType::Child, visitor);
                self.visit_expr(index, NodeType::LastChild, visitor);
            }
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::CallExpr(CallExpr { expr, args, .. }),
                        ..
                    },
                ..
            } => {
                let mut iter = std::iter::once(&**expr).chain(args.iter()).peekable();
                while let Some(expr) = iter.next() {
                    self.visit_expr(expr, iter.peek().into(), visitor);
                }
            }
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::IdentExpr(_) | Expr::LitExpr(_),
                        ..
                    },
                ..
            } => {}
        }
        return_on_terminate!(visitor(TraversalType::Postorder, node, ty));
    }

    pub fn visit_expr_mut<F: FnMut(TraversalType, AstNodeMut, NodeType) -> AstVisitorAction>(
        &self,
        expr: &mut SourcedExpr,
        ty: NodeType,
        visitor: &mut F,
    ) {
        return_on_terminate!(visitor(TraversalType::Preorder, AstNodeMut::Expr(expr), ty));
        match expr {
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::BlockExpr(_),
                        ..
                    },
                ..
            } => {
                self.visit_block_expr_mut(expr, ty, true, visitor);
            }
            Sourced {
                value:
                    TypedExpr {
                        expr:
                            Expr::IfExpr(IfExpr {
                                condition,
                                then,
                                else_,
                            }),
                        ..
                    },
                ..
            } => {
                if self.if_exprs {
                    self.visit_expr_mut(condition, NodeType::Child, visitor);
                    self.visit_block_expr_mut(
                        then,
                        if else_.is_none() {
                            NodeType::LastChild
                        } else {
                            NodeType::Child
                        },
                        true,
                        visitor,
                    );
                    if let Some(else_) = else_ {
                        self.visit_block_expr_mut(else_, NodeType::LastChild, true, visitor);
                    }
                }
            }
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::OperatorExpr(OperatorExpr { args, .. }),
                        ..
                    },
                ..
            } => {
                let mut iter = args.iter_mut().peekable();
                while let Some(arg) = iter.next() {
                    self.visit_expr_mut(arg, iter.peek().into(), visitor);
                }
            }
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::MemberExpr(MemberExpr { expr, .. }),
                        ..
                    },
                ..
            } => {
                self.visit_expr_mut(&mut *expr, NodeType::LastChild, visitor);
            }
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::IndexExpr(IndexExpr { expr, index }),
                        ..
                    },
                ..
            } => {
                self.visit_expr_mut(&mut *expr, NodeType::Child, visitor);
                self.visit_expr_mut(&mut *index, NodeType::LastChild, visitor);
            }
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::CallExpr(CallExpr { expr, args, .. }),
                        ..
                    },
                ..
            } => {
                let mut iter = std::iter::once(&mut **expr)
                    .chain(args.iter_mut())
                    .peekable();
                while let Some(expr) = iter.next() {
                    self.visit_expr_mut(expr, iter.peek().into(), visitor);
                }
            }
            Sourced {
                value:
                    TypedExpr {
                        expr: Expr::IdentExpr(_) | Expr::LitExpr(_),
                        ..
                    },
                ..
            } => {}
        }
        return_on_terminate!(visitor(
            TraversalType::Postorder,
            AstNodeMut::Expr(expr),
            ty
        ));
    }
}
