//! Deterministic, deliberately small bytecode for embedding Lexion callbacks.
use crate::ast::{Expr, FuncDeclStmt, Lit, Sourced, SourcedExpr, Stmt};
use crate::parser::ParserLexion;
use lexion_lib::miette::SourceSpan;
use lexion_lib::Parser;
use std::collections::BTreeMap;
use std::sync::Arc;

pub const BYTECODE_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BytecodeValue {
    Unit,
    I32(i32),
    Bool(bool),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytecodeError {
    pub message: String,
    pub span: SourceSpan,
}
impl std::fmt::Display for BytecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for BytecodeError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytecodeProgram {
    version: u16,
    callbacks: BTreeMap<String, Callback>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Callback {
    code: Vec<Instruction>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum Instruction {
    Push(BytecodeValue),
    Call {
        name: String,
        arguments: usize,
        span: SourceSpan,
    },
    Pop,
}

impl BytecodeProgram {
    pub fn compile(source: impl Into<String>) -> Result<Self, BytecodeError> {
        let source = Arc::new(source.into());
        let mut parser = ParserLexion::new();
        let ast = parser
            .parse_from_string(source)
            .map_err(|error| BytecodeError {
                message: error.to_string(),
                span: SourceSpan::from(0),
            })?;
        let mut callbacks = BTreeMap::new();
        for statement in ast {
            let Sourced {
                value: Stmt::FuncDeclStmt(function),
                span,
            } = statement
            else {
                continue;
            };
            if !function.is_callback {
                continue;
            }
            let callback = compile_callback(&function, span)?;
            if callbacks
                .insert(function.name.value.clone(), callback)
                .is_some()
            {
                return Err(BytecodeError {
                    message: format!("duplicate callback `{}`", function.name.value),
                    span: function.name.span,
                });
            }
        }
        Ok(Self {
            version: BYTECODE_VERSION,
            callbacks,
        })
    }
    pub fn version(&self) -> u16 {
        self.version
    }
    pub fn callback_names(&self) -> impl Iterator<Item = &str> {
        self.callbacks.keys().map(String::as_str)
    }
    pub fn invoke(&self, name: &str, host: &mut dyn BytecodeHost) -> Result<(), BytecodeError> {
        let callback = self.callbacks.get(name).ok_or_else(|| BytecodeError {
            message: format!("unknown callback `{name}`"),
            span: SourceSpan::from(0),
        })?;
        let mut stack = Vec::new();
        for instruction in &callback.code {
            match instruction {
                Instruction::Push(value) => stack.push(value.clone()),
                Instruction::Pop => {
                    stack.pop();
                }
                Instruction::Call {
                    name,
                    arguments,
                    span,
                } => {
                    if stack.len() < *arguments {
                        return Err(BytecodeError {
                            message: "invalid bytecode stack state".into(),
                            span: *span,
                        });
                    }
                    let args = stack.split_off(stack.len() - arguments);
                    let value = host.call(name, &args).map_err(|message| BytecodeError {
                        message,
                        span: *span,
                    })?;
                    stack.push(value);
                }
            }
        }
        Ok(())
    }
}
pub trait BytecodeHost {
    fn call(
        &mut self,
        operation: &str,
        arguments: &[BytecodeValue],
    ) -> Result<BytecodeValue, String>;
}

fn compile_callback(function: &FuncDeclStmt, span: SourceSpan) -> Result<Callback, BytecodeError> {
    if !function.params.is_empty() || function.is_vararg {
        return Err(BytecodeError {
            message: "bytecode callbacks must not take parameters".into(),
            span: function.name.span,
        });
    }
    let is_unit = function.ty.as_ref().map_or(
        true,
        |ty| matches!(&ty.value, crate::ast::Type::Tuple(tuple) if tuple.types.is_empty()),
    );
    if !is_unit {
        return Err(BytecodeError {
            message: "bytecode callbacks must return `()`".into(),
            span: function.ty.as_ref().unwrap().span,
        });
    }
    let body = function.body.as_ref().ok_or_else(|| BytecodeError {
        message: "bytecode callbacks need a body".into(),
        span,
    })?;
    let Expr::BlockExpr(block) = &body.expr else {
        unreachable!()
    };
    let mut code = Vec::new();
    for statement in &block.stmts {
        let Stmt::ExprStmt(expr) = &statement.value else {
            return Err(BytecodeError {
                message: "only host calls are supported in bytecode callbacks".into(),
                span: statement.span,
            });
        };
        compile_call(&expr.expr, &mut code)?;
        code.push(Instruction::Pop);
    }
    if block.expr.is_some() {
        return Err(BytecodeError {
            message: "bytecode callbacks cannot return a value".into(),
            span: body.span,
        });
    }
    Ok(Callback { code })
}
fn compile_call(expr: &SourcedExpr, code: &mut Vec<Instruction>) -> Result<(), BytecodeError> {
    let Expr::CallExpr(call) = &expr.expr else {
        return Err(BytecodeError {
            message: "bytecode callbacks may only call host operations".into(),
            span: expr.span,
        });
    };
    let Expr::IdentExpr(target) = &call.expr.expr else {
        return Err(BytecodeError {
            message: "host operation must be named".into(),
            span: call.expr.span,
        });
    };
    for argument in &call.args {
        let Expr::LitExpr(literal) = &argument.expr else {
            return Err(BytecodeError {
                message: "bytecode host arguments must be literals".into(),
                span: argument.span,
            });
        };
        let value = match &literal.lit {
            Lit::Integer(value) => {
                BytecodeValue::I32((*value).try_into().map_err(|_| BytecodeError {
                    message: "integer does not fit in i32".into(),
                    span: argument.span,
                })?)
            }
            Lit::Boolean(value) => BytecodeValue::Bool(*value),
            Lit::String(value) => BytecodeValue::String(value.clone()),
            Lit::Float(_) => {
                return Err(BytecodeError {
                    message: "bytecode does not support floating-point host arguments".into(),
                    span: argument.span,
                })
            }
        };
        code.push(Instruction::Push(value));
    }
    code.push(Instruction::Call {
        name: target.ident.clone(),
        arguments: call.args.len(),
        span: expr.span,
    });
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Recorder(Vec<BytecodeValue>);
    impl BytecodeHost for Recorder {
        fn call(
            &mut self,
            name: &str,
            arguments: &[BytecodeValue],
        ) -> Result<BytecodeValue, String> {
            if name != "record" {
                return Err(format!("unknown host operation `{name}`"));
            }
            self.0.extend_from_slice(arguments);
            Ok(BytecodeValue::Unit)
        }
    }
    #[test]
    fn invokes_a_keyword_marked_callback() {
        let program = BytecodeProgram::compile("callback fn tick() -> () { record(7); }").unwrap();
        let mut host = Recorder(vec![]);
        program.invoke("tick", &mut host).unwrap();
        assert_eq!(host.0, vec![BytecodeValue::I32(7)]);
        assert_eq!(program.version(), BYTECODE_VERSION);
    }
}
