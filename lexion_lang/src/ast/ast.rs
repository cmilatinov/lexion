use crate::ast::visitor::{AstNode, AstVisitor, AstVisitorAction, NodeType, TraversalType};
use crate::ast::SourcedStmt;
use std::fmt::{Display, Formatter};

pub struct AstView<'a>(&'a Vec<SourcedStmt>);

impl<'a> AstView<'a> {
    pub fn new(ast: &'a Vec<SourcedStmt>) -> Self {
        Self(ast)
    }
}

impl<'a> Display for AstView<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "[Program]")?;
        let mut stack = Vec::new();
        let mut err = Ok(());
        AstVisitor::new().visit(&self.0, |ty, node, node_ty| {
            if ty == TraversalType::Preorder {
                let name = match node {
                    AstNode::Stmt(stmt) => stmt.value.as_ref(),
                    AstNode::Expr(expr) => expr.value.expr.as_ref(),
                };
                let str = match node_ty {
                    NodeType::Root => "",
                    NodeType::Child => "├─",
                    NodeType::LastChild => "└─",
                };
                let result = writeln!(f, "{}{}[{}]", stack.join(""), str, name);
                if result.is_err() {
                    err = result;
                    return AstVisitorAction::Terminate;
                }
                stack.push(match node_ty {
                    NodeType::Child => "│ ",
                    _ => "  ",
                });
            } else if ty == TraversalType::Postorder {
                stack.pop();
            }
            AstVisitorAction::Continue
        });
        err
    }
}
