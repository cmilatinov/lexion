//! Deterministic, deliberately small bytecode for embedding Lexion callbacks.
use crate::ast::{Expr, FuncDeclStmt, FunctionQualifier, Lit, Sourced, SourcedExpr, Stmt, Type};
use crate::parser::ParserLexion;
use lexion_lib::error::ParseError;
use lexion_lib::miette::SourceSpan;
use lexion_lib::Parser;
use std::collections::BTreeMap;
use std::sync::Arc;

pub const BYTECODE_VERSION: u16 = 1;
pub const DEFAULT_HOST_API_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BytecodeValue {
    Unit,
    I32(i32),
    Bool(bool),
    String(String),
}

impl BytecodeValue {
    fn value_type(&self) -> BytecodeValueType {
        match self {
            Self::Unit => BytecodeValueType::Unit,
            Self::I32(_) => BytecodeValueType::I32,
            Self::Bool(_) => BytecodeValueType::Bool,
            Self::String(_) => BytecodeValueType::String,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BytecodeValueType {
    Unit,
    I32,
    Bool,
    String,
}

impl std::fmt::Display for BytecodeValueType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Unit => "()",
            Self::I32 => "i32",
            Self::Bool => "bool",
            Self::String => "str",
        };
        write!(f, "{value}")
    }
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
pub struct BytecodeOperation {
    id: u16,
    name: String,
    arguments: Vec<BytecodeValueType>,
}

impl BytecodeOperation {
    pub fn id(&self) -> u16 {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn arguments(&self) -> &[BytecodeValueType] {
        &self.arguments
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytecodeProgram {
    version: u16,
    required_host_api_version: u16,
    operations: Vec<BytecodeOperation>,
    callbacks: BTreeMap<String, Callback>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Callback {
    code: Vec<Instruction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Instruction {
    CallHost {
        operation: u16,
        arguments: Vec<BytecodeValue>,
        span: SourceSpan,
    },
}

impl BytecodeProgram {
    pub fn compile(source: impl Into<String>) -> Result<Self, BytecodeError> {
        Self::compile_for_host_api(source, DEFAULT_HOST_API_VERSION)
    }

    pub fn compile_for_host_api(
        source: impl Into<String>,
        required_host_api_version: u16,
    ) -> Result<Self, BytecodeError> {
        let source = Arc::new(source.into());
        let mut parser = ParserLexion::new();
        let ast = parser.parse_from_string(source).map_err(|error| {
            let span = match &error {
                ParseError::Syntax(error) => error.span,
                ParseError::Io(_) => SourceSpan::from(0),
            };
            BytecodeError {
                message: error.to_string(),
                span,
            }
        })?;

        let mut host_declarations = BTreeMap::new();
        let mut callback_declarations = Vec::new();
        let mut declaration_names = BTreeMap::new();
        for statement in &ast {
            let Sourced {
                value: Stmt::FuncDeclStmt(function),
                span,
            } = statement
            else {
                return Err(BytecodeError {
                    message: "bytecode programs only support extern host declarations and callback functions".into(),
                    span: statement.span,
                });
            };
            if declaration_names
                .insert(function.name.value.as_str(), function.name.span)
                .is_some()
            {
                return Err(BytecodeError {
                    message: format!("duplicate function declaration `{}`", function.name.value),
                    span: function.name.span,
                });
            }
            match function.qualifier {
                FunctionQualifier::Extern => {
                    if host_declarations
                        .insert(function.name.value.as_str(), (function, *span))
                        .is_some()
                    {
                        return Err(BytecodeError {
                            message: format!("duplicate host operation `{}`", function.name.value),
                            span: function.name.span,
                        });
                    }
                }
                FunctionQualifier::Callback => callback_declarations.push((function, *span)),
                FunctionQualifier::None => {
                    return Err(BytecodeError {
                        message: "bytecode programs only support extern host declarations and callback functions".into(),
                        span: *span,
                    });
                }
            }
        }

        let mut operations = Vec::with_capacity(host_declarations.len());
        for (index, (name, (function, span))) in host_declarations.into_iter().enumerate() {
            let id = u16::try_from(index).map_err(|_| BytecodeError {
                message: "bytecode programs support at most 65536 host operations".into(),
                span,
            })?;
            operations.push(compile_host_operation(id, name, function, span)?);
        }
        let operations_by_name = operations
            .iter()
            .map(|operation| (operation.name.as_str(), operation))
            .collect::<BTreeMap<_, _>>();

        let mut callbacks = BTreeMap::new();
        for (function, span) in callback_declarations {
            let callback = compile_callback(function, span, &operations_by_name)?;
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
            required_host_api_version,
            operations,
            callbacks,
        })
    }

    pub fn version(&self) -> u16 {
        self.version
    }

    pub fn required_host_api_version(&self) -> u16 {
        self.required_host_api_version
    }

    pub fn callback_names(&self) -> impl Iterator<Item = &str> {
        self.callbacks.keys().map(String::as_str)
    }

    pub fn operations(&self) -> impl Iterator<Item = &BytecodeOperation> {
        self.operations.iter()
    }

    pub fn invoke(&self, name: &str, host: &mut dyn BytecodeHost) -> Result<(), BytecodeError> {
        let callback = self.callbacks.get(name).ok_or_else(|| BytecodeError {
            message: format!("unknown callback `{name}`"),
            span: SourceSpan::from(0),
        })?;
        for instruction in &callback.code {
            match instruction {
                Instruction::CallHost {
                    operation,
                    arguments,
                    span,
                } => {
                    let operation =
                        self.operations
                            .get(usize::from(*operation))
                            .ok_or_else(|| BytecodeError {
                                message: "invalid bytecode operation".into(),
                                span: *span,
                            })?;
                    host.call(operation, arguments)
                        .map_err(|message| BytecodeError {
                            message,
                            span: *span,
                        })?;
                }
            }
        }
        Ok(())
    }
}

pub trait BytecodeHost {
    fn call(
        &mut self,
        operation: &BytecodeOperation,
        arguments: &[BytecodeValue],
    ) -> Result<(), String>;
}

/// A versioned, owned-value host boundary.  It deliberately exposes neither VM
/// references nor Rust pointers to scripts.
pub struct HostManifest {
    version: u16,
    operations: BTreeMap<String, HostOperation>,
    executing: bool,
}

pub struct HostOperation {
    pub arguments: Vec<HostValueType>,
    handler: Box<dyn FnMut(&[BytecodeValue]) -> Result<(), String> + Send>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostValueType {
    Unit,
    I32,
    Bool,
    String,
}

impl HostManifest {
    pub fn new(version: u16) -> Self {
        Self {
            version,
            operations: BTreeMap::new(),
            executing: false,
        }
    }
    pub fn version(&self) -> u16 {
        self.version
    }
    pub fn register(
        &mut self,
        name: impl Into<String>,
        arguments: Vec<HostValueType>,
        handler: impl FnMut(&[BytecodeValue]) -> Result<(), String> + Send + 'static,
    ) -> Result<(), String> {
        let name = name.into();
        if self.operations.contains_key(&name) {
            return Err(format!("host operation `{name}` is already registered"));
        }
        self.operations.insert(
            name,
            HostOperation {
                arguments,
                handler: Box::new(handler),
            },
        );
        Ok(())
    }
    pub fn invoke(
        &mut self,
        program: &BytecodeProgram,
        callback: &str,
    ) -> Result<(), BytecodeError> {
        if self.executing {
            return Err(BytecodeError {
                message: "same-instance synchronous reentry is not allowed".into(),
                span: SourceSpan::from(0),
            });
        }
        if program.version != BYTECODE_VERSION {
            return Err(BytecodeError {
                message: format!("unsupported bytecode version {}", program.version),
                span: SourceSpan::from(0),
            });
        }
        if program.required_host_api_version != self.version {
            return Err(BytecodeError {
                message: format!(
                    "bytecode requires host API version {}, but manifest provides version {}",
                    program.required_host_api_version, self.version
                ),
                span: SourceSpan::from(0),
            });
        }
        for operation in program.operations() {
            let entry = self.operations.get(operation.name()).ok_or_else(|| BytecodeError {
                message: format!(
                    "host manifest v{} has no `{}` operation",
                    self.version,
                    operation.name()
                ),
                span: SourceSpan::from(0),
            })?;
            let expected = operation
                .arguments()
                .iter()
                .copied()
                .map(HostValueType::from)
                .collect::<Vec<_>>();
            if entry.arguments != expected {
                return Err(BytecodeError {
                    message: format!(
                        "host manifest v{} has an incompatible `{}` operation signature",
                        self.version,
                        operation.name()
                    ),
                    span: SourceSpan::from(0),
                });
            }
        }
        self.executing = true;
        let result = program.invoke(callback, self);
        self.executing = false;
        result
    }
}

impl BytecodeHost for HostManifest {
    fn call(
        &mut self,
        operation: &BytecodeOperation,
        arguments: &[BytecodeValue],
    ) -> Result<(), String> {
        let entry = self.operations.get_mut(operation.name()).ok_or_else(|| {
            format!(
                "host manifest v{} has no `{}` operation",
                operation.name(),
                self.version
            )
        })?;
        if entry.arguments.len() != arguments.len() {
            return Err(format!(
                "host operation `{}` expects {} argument(s), got {}",
                operation.name(),
                entry.arguments.len(),
                arguments.len()
            ));
        }
        for (expected, actual) in entry.arguments.iter().zip(arguments) {
            if !matches_type(expected, actual) {
                return Err(format!(
                    "host operation `{}` received an incompatible argument",
                    operation.name()
                ));
            }
        }
        (entry.handler)(arguments)
    }
}

fn matches_type(expected: &HostValueType, value: &BytecodeValue) -> bool {
    matches!(
        (expected, value),
        (HostValueType::Unit, BytecodeValue::Unit)
            | (HostValueType::I32, BytecodeValue::I32(_))
            | (HostValueType::Bool, BytecodeValue::Bool(_))
            | (HostValueType::String, BytecodeValue::String(_))
    )
}

impl From<BytecodeValueType> for HostValueType {
    fn from(value: BytecodeValueType) -> Self {
        match value {
            BytecodeValueType::Unit => Self::Unit,
            BytecodeValueType::I32 => Self::I32,
            BytecodeValueType::Bool => Self::Bool,
            BytecodeValueType::String => Self::String,
        }
    }
}

fn compile_host_operation(
    id: u16,
    name: &str,
    function: &FuncDeclStmt,
    span: SourceSpan,
) -> Result<BytecodeOperation, BytecodeError> {
    if function.body.is_some() {
        return Err(BytecodeError {
            message: "bytecode host operations must not have a body".into(),
            span,
        });
    }
    if function.is_vararg {
        return Err(BytecodeError {
            message: "bytecode host operations must not be variadic".into(),
            span: function.name.span,
        });
    }
    let arguments = function
        .params
        .iter()
        .map(|parameter| bytecode_parameter_type(&parameter.value.ty.value, parameter.span))
        .collect::<Result<Vec<_>, _>>()?;
    let result = function
        .ty
        .as_ref()
        .map_or(Ok(BytecodeValueType::Unit), |ty| {
            bytecode_type(&ty.value, ty.span)
        })?;
    if result != BytecodeValueType::Unit {
        return Err(BytecodeError {
            message: "bytecode host operations must return `()`".into(),
            span: function.ty.as_ref().unwrap().span,
        });
    }
    Ok(BytecodeOperation {
        id,
        name: name.into(),
        arguments,
    })
}

fn compile_callback(
    function: &FuncDeclStmt,
    span: SourceSpan,
    operations: &BTreeMap<&str, &BytecodeOperation>,
) -> Result<Callback, BytecodeError> {
    if !function.params.is_empty() || function.is_vararg {
        return Err(BytecodeError {
            message: "bytecode callbacks must not take parameters".into(),
            span: function.name.span,
        });
    }
    let result = function
        .ty
        .as_ref()
        .map_or(Ok(BytecodeValueType::Unit), |ty| {
            bytecode_type(&ty.value, ty.span)
        })?;
    if result != BytecodeValueType::Unit {
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
        code.push(compile_call(&expr.expr, operations)?);
    }
    if block.expr.is_some() {
        return Err(BytecodeError {
            message: "bytecode callbacks cannot return a value".into(),
            span: body.span,
        });
    }
    Ok(Callback { code })
}

fn compile_call(
    expr: &SourcedExpr,
    operations: &BTreeMap<&str, &BytecodeOperation>,
) -> Result<Instruction, BytecodeError> {
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
    let operation = operations
        .get(target.ident.as_str())
        .ok_or_else(|| BytecodeError {
            message: format!("unknown host operation `{}`", target.ident),
            span: call.expr.span,
        })?;
    if operation.arguments.len() != call.args.len() {
        return Err(BytecodeError {
            message: format!(
                "host operation `{}` expects {} argument(s), got {}",
                operation.name,
                operation.arguments.len(),
                call.args.len()
            ),
            span: expr.span,
        });
    }
    let mut arguments = Vec::with_capacity(call.args.len());
    for (index, argument) in call.args.iter().enumerate() {
        let value = bytecode_value(argument)?;
        let actual = value.value_type();
        let expected = operation.arguments[index];
        if actual != expected {
            return Err(BytecodeError {
                message: format!(
                    "host operation `{}` argument {} expects {}, got {}",
                    operation.name,
                    index + 1,
                    expected,
                    actual
                ),
                span: argument.span,
            });
        }
        arguments.push(value);
    }
    Ok(Instruction::CallHost {
        operation: operation.id,
        arguments,
        span: expr.span,
    })
}

fn bytecode_value(argument: &SourcedExpr) -> Result<BytecodeValue, BytecodeError> {
    match &argument.expr {
        Expr::LitExpr(literal) => match &literal.lit {
            Lit::Integer(value) => bytecode_i32(*value, argument.span),
            Lit::Boolean(value) => Ok(BytecodeValue::Bool(*value)),
            Lit::String(value) => Ok(BytecodeValue::String(decode_string_literal(
                value,
                argument.span,
            )?)),
            Lit::Float(_) => Err(BytecodeError {
                message: "bytecode does not support floating-point host arguments".into(),
                span: argument.span,
            }),
        },
        Expr::OperatorExpr(operator) if operator.operator == "-" && operator.args.len() == 1 => {
            let Expr::LitExpr(literal) = &operator.args[0].expr else {
                return Err(BytecodeError {
                    message: "bytecode host arguments must be literals".into(),
                    span: argument.span,
                });
            };
            let Lit::Integer(value) = literal.lit else {
                return Err(BytecodeError {
                    message: "bytecode host arguments must be literals".into(),
                    span: argument.span,
                });
            };
            bytecode_i32(
                value.checked_neg().ok_or_else(|| BytecodeError {
                    message: "integer does not fit in i32".into(),
                    span: argument.span,
                })?,
                argument.span,
            )
        }
        _ => Err(BytecodeError {
            message: "bytecode host arguments must be literals".into(),
            span: argument.span,
        }),
    }
}

fn bytecode_i32(value: isize, span: SourceSpan) -> Result<BytecodeValue, BytecodeError> {
    Ok(BytecodeValue::I32(value.try_into().map_err(|_| {
        BytecodeError {
            message: "integer does not fit in i32".into(),
            span,
        }
    })?))
}

fn decode_string_literal(value: &str, span: SourceSpan) -> Result<String, BytecodeError> {
    let Some(quote) = value.chars().next() else {
        return Err(BytecodeError {
            message: "invalid string literal".into(),
            span,
        });
    };
    let Some(inner) = value
        .strip_prefix(quote)
        .and_then(|value| value.strip_suffix(quote))
    else {
        return Err(BytecodeError {
            message: "invalid string literal".into(),
            span,
        });
    };
    let mut decoded = String::with_capacity(inner.len());
    let mut characters = inner.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        let escaped = characters.next().ok_or_else(|| BytecodeError {
            message: "invalid string escape".into(),
            span,
        })?;
        decoded.push(match escaped {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '0' => '\0',
            other => other,
        });
    }
    Ok(decoded)
}

fn bytecode_parameter_type(
    ty: &Type,
    span: SourceSpan,
) -> Result<BytecodeValueType, BytecodeError> {
    let value_type = bytecode_type(ty, span)?;
    if value_type == BytecodeValueType::Unit {
        return Err(BytecodeError {
            message: "bytecode host operation parameters must not use `()`".into(),
            span,
        });
    }
    Ok(value_type)
}

fn bytecode_type(ty: &Type, span: SourceSpan) -> Result<BytecodeValueType, BytecodeError> {
    let result = match ty {
        Type::Tuple(tuple) if tuple.types.is_empty() => Some(BytecodeValueType::Unit),
        Type::Path(path) if path.path.segments.len() == 1 => {
            match path.path.segments[0].value.as_str() {
                "i32" => Some(BytecodeValueType::I32),
                "bool" => Some(BytecodeValueType::Bool),
                "str" => Some(BytecodeValueType::String),
                _ => None,
            }
        }
        Type::Reference(reference) => match &reference.to.value {
            Type::Path(path)
                if path.path.segments.len() == 1 && path.path.segments[0].value == "str" =>
            {
                Some(BytecodeValueType::String)
            }
            _ => None,
        },
        _ => None,
    };
    result.ok_or_else(|| BytecodeError {
        message: "bytecode host operations only support i32, bool, str, and () values".into(),
        span,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorder(Vec<(u16, Vec<BytecodeValue>)>);

    impl BytecodeHost for Recorder {
        fn call(
            &mut self,
            operation: &BytecodeOperation,
            arguments: &[BytecodeValue],
        ) -> Result<(), String> {
            self.0.push((operation.id(), arguments.to_vec()));
            Ok(())
        }
    }

    #[test]
    fn invokes_a_validated_callback_with_precompiled_arguments() {
        let program = BytecodeProgram::compile(
            "extern fn record(value: i32); callback fn tick() -> () { record(7); record(8); }",
        )
        .unwrap();
        let mut host = Recorder::default();
        program.invoke("tick", &mut host).unwrap();
        program.invoke("tick", &mut host).unwrap();

        assert_eq!(
            host.0,
            vec![
                (0, vec![BytecodeValue::I32(7)]),
                (0, vec![BytecodeValue::I32(8)]),
                (0, vec![BytecodeValue::I32(7)]),
                (0, vec![BytecodeValue::I32(8)]),
            ]
        );
        assert_eq!(program.operations().next().unwrap().name(), "record");
        assert_eq!(program.version(), BYTECODE_VERSION);
    }
    #[test]
    fn versioned_manifest_dispatches_a_command() {
        let program = BytecodeProgram::compile(
            "extern fn record(value: i32); callback fn tick() -> () { record(7); }",
        )
        .unwrap();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let target = seen.clone();
        let mut manifest = HostManifest::new(1);
        manifest
            .register(
                "record",
                vec![HostValueType::I32],
                move |args| {
                    target.lock().unwrap().extend_from_slice(args);
                    Ok(())
                },
            )
            .unwrap();
        manifest.invoke(&program, "tick").unwrap();
        assert_eq!(*seen.lock().unwrap(), vec![BytecodeValue::I32(7)]);
    }

    #[test]
    fn rejects_unknown_or_mismatched_host_calls() {
        let unknown = BytecodeProgram::compile("callback fn tick() { record(7); }").unwrap_err();
        assert_eq!(unknown.message, "unknown host operation `record`");

        let wrong_type = BytecodeProgram::compile(
            "extern fn record(value: i32); callback fn tick() { record(true); }",
        )
        .unwrap_err();
        assert_eq!(
            wrong_type.message,
            "host operation `record` argument 1 expects i32, got bool"
        );

        let wrong_arity = BytecodeProgram::compile(
            "extern fn record(value: i32); callback fn tick() { record(7, 8); }",
        )
        .unwrap_err();
        assert_eq!(
            wrong_arity.message,
            "host operation `record` expects 1 argument(s), got 2"
        );
    }

    #[test]
    fn decodes_string_literals_and_accepts_negative_i32_arguments() {
        let program = BytecodeProgram::compile(
            "extern fn record(value: &str); extern fn offset(value: i32); callback fn tick() { record(\"line\\ntext\"); offset(-1); }",
        )
        .unwrap();
        let mut host = Recorder::default();
        program.invoke("tick", &mut host).unwrap();
        assert_eq!(
            host.0,
            vec![
                (1, vec![BytecodeValue::String("line\ntext".into())]),
                (0, vec![BytecodeValue::I32(-1)]),
            ]
        );
    }

    #[test]
    fn rejects_unsupported_top_level_code_and_host_signatures() {
        let top_level = BytecodeProgram::compile("fn helper() {}").unwrap_err();
        assert_eq!(
            top_level.message,
            "bytecode programs only support extern host declarations and callback functions"
        );

        let return_type = BytecodeProgram::compile("extern fn record() -> i32;").unwrap_err();
        assert_eq!(
            return_type.message,
            "bytecode host operations must return `()`"
        );

        let unit_parameter = BytecodeProgram::compile("extern fn record(value: ());").unwrap_err();
        assert_eq!(
            unit_parameter.message,
            "bytecode host operation parameters must not use `()`"
        );

        let duplicate_name =
            BytecodeProgram::compile("extern fn tick(); callback fn tick() { tick(); }")
                .unwrap_err();
        assert_eq!(
            duplicate_name.message,
            "duplicate function declaration `tick`"
        );
    }

    #[test]
    fn preserves_syntax_error_spans() {
        let error = BytecodeProgram::compile("callback fn tick( {").unwrap_err();
        assert_ne!(error.span, SourceSpan::from(0));
    }

    #[test]
    fn preserves_host_failures_at_the_call_span() {
        struct FailingHost;
        impl BytecodeHost for FailingHost {
            fn call(&mut self, _: &BytecodeOperation, _: &[BytecodeValue]) -> Result<(), String> {
                Err("host failed".into())
            }
        }

        let program = BytecodeProgram::compile(
            "extern fn record(value: i32); callback fn tick() { record(7); }",
        )
        .unwrap();
        let error = program.invoke("tick", &mut FailingHost).unwrap_err();
        assert_eq!(error.message, "host failed");
        assert_ne!(error.span, SourceSpan::from(0));
    }
}
