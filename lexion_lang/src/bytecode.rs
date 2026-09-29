//! Deterministic, deliberately small bytecode for embedding Lexion callbacks.
use crate::ast::{
    Expr, FuncDeclStmt, FunctionQualifier, Lit, Sourced, SourcedExpr, Stmt, StructDeclStmt, Type,
};
use crate::parser::ParserLexion;
use lexion_lib::error::ParseError;
use lexion_lib::miette::SourceSpan;
use lexion_lib::Parser;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const BYTECODE_VERSION: u16 = 1;
pub const DEFAULT_HOST_API_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BytecodeValue {
    Unit,
    I32(i32),
    Bool(bool),
    String(String),
    Record {
        fields: BTreeMap<String, BytecodeValue>,
    },
}

impl BytecodeValue {
    fn value_type(&self) -> BytecodeValueType {
        match self {
            Self::Unit => BytecodeValueType::Unit,
            Self::I32(_) => BytecodeValueType::I32,
            Self::Bool(_) => BytecodeValueType::Bool,
            Self::String(_) => BytecodeValueType::String,
            Self::Record { fields } => BytecodeValueType::Record(
                fields
                    .iter()
                    .map(|(name, value)| (name.clone(), value.value_type()))
                    .collect(),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BytecodeValueType {
    Unit,
    I32,
    Bool,
    String,
    Record(BTreeMap<String, BytecodeValueType>),
}

impl std::fmt::Display for BytecodeValueType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Unit => "()",
            Self::I32 => "i32",
            Self::Bool => "bool",
            Self::String => "str",
            Self::Record(fields) => {
                return write!(
                    f,
                    "{{{}}}",
                    fields.keys().cloned().collect::<Vec<_>>().join(", ")
                )
            }
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
    result: BytecodeValueType,
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

    pub fn result(&self) -> &BytecodeValueType {
        &self.result
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
        arguments: Vec<BytecodeOperand>,
        result: Option<String>,
        span: SourceSpan,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BytecodeOperand {
    Value(BytecodeValue),
    Local(String),
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

        let mut struct_declarations = BTreeMap::new();
        let mut host_declarations = BTreeMap::new();
        let mut callback_declarations = Vec::new();
        let mut declaration_names = BTreeMap::new();
        for statement in &ast {
            if let Stmt::StructDeclStmt(declaration) = &statement.value {
                if struct_declarations
                    .insert(declaration.name.value.as_str(), declaration)
                    .is_some()
                {
                    return Err(BytecodeError {
                        message: format!(
                            "duplicate record declaration `{}`",
                            declaration.name.value
                        ),
                        span: declaration.name.span,
                    });
                }
                continue;
            }
            let Sourced {
                value: Stmt::FuncDeclStmt(function),
                span,
            } = statement
            else {
                return Err(BytecodeError {
                    message: "bytecode programs only support record declarations, extern host declarations, and callback functions".into(),
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
            operations.push(compile_host_operation(
                id,
                name,
                function,
                span,
                &struct_declarations,
            )?);
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
        self.invoke_inner(name, host, None)
    }

    pub fn invoke_with_state(
        &self,
        name: &str,
        host: &mut dyn BytecodeHost,
        state: &mut BehaviorState,
    ) -> Result<(), BytecodeError> {
        self.invoke_inner(name, host, Some(state))
    }

    fn invoke_inner(
        &self,
        name: &str,
        host: &mut dyn BytecodeHost,
        mut state: Option<&mut BehaviorState>,
    ) -> Result<(), BytecodeError> {
        let callback = self.callbacks.get(name).ok_or_else(|| BytecodeError {
            message: format!("unknown callback `{name}`"),
            span: SourceSpan::from(0),
        })?;
        let mut locals = BTreeMap::new();
        for instruction in &callback.code {
            match instruction {
                Instruction::CallHost {
                    operation,
                    arguments,
                    result,
                    span,
                } => {
                    let operation =
                        self.operations
                            .get(usize::from(*operation))
                            .ok_or_else(|| BytecodeError {
                                message: "invalid bytecode operation".into(),
                                span: *span,
                            })?;
                    let arguments = arguments
                        .iter()
                        .map(|argument| match argument {
                            BytecodeOperand::Value(value) => Ok(value.clone()),
                            BytecodeOperand::Local(name) => {
                                locals.get(name).cloned().ok_or_else(|| BytecodeError {
                                    message: format!("unknown bytecode local `{name}`"),
                                    span: *span,
                                })
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let value = match state.as_deref_mut() {
                        Some(state) => host.call_with_state(operation, &arguments, state),
                        None => host.call(operation, &arguments),
                    }
                    .map_err(|message| BytecodeError {
                        message,
                        span: *span,
                    })?;
                    if value.value_type() != *operation.result() {
                        return Err(BytecodeError {
                            message: format!(
                                "host operation `{}` returned an incompatible value",
                                operation.name()
                            ),
                            span: *span,
                        });
                    }
                    if let Some(name) = result {
                        locals.insert(name.clone(), value);
                    }
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
    ) -> Result<BytecodeValue, String>;

    fn call_with_state(
        &mut self,
        operation: &BytecodeOperation,
        arguments: &[BytecodeValue],
        _: &mut BehaviorState,
    ) -> Result<BytecodeValue, String> {
        self.call(operation, arguments)
    }
}

/// A versioned, owned-value host boundary.  It deliberately exposes neither VM
/// references nor Rust pointers to scripts.
pub struct HostManifest {
    version: u16,
    operations: BTreeMap<String, HostOperation>,
    executing: Arc<AtomicBool>,
    dispatching: Arc<AtomicBool>,
}

pub struct HostOperation {
    pub arguments: Vec<HostValueType>,
    result: Option<HostValueType>,
    handler: HostHandler,
}

enum HostHandler {
    Stateless(Box<dyn FnMut(&[BytecodeValue]) -> Result<BytecodeValue, String> + Send>),
    Stateful(
        Box<
            dyn FnMut(&mut BehaviorState, &[BytecodeValue]) -> Result<BytecodeValue, String> + Send,
        >,
    ),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostValueType {
    I32,
    Bool,
    String,
    Record(BTreeMap<String, HostValueType>),
}

impl HostManifest {
    pub fn new(version: u16) -> Self {
        Self {
            version,
            operations: BTreeMap::new(),
            executing: Arc::new(AtomicBool::new(false)),
            dispatching: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn version(&self) -> u16 {
        self.version
    }
    pub fn register(
        &mut self,
        name: impl Into<String>,
        arguments: Vec<HostValueType>,
        mut handler: impl FnMut(&[BytecodeValue]) -> Result<(), String> + Send + 'static,
    ) -> Result<(), String> {
        let name = name.into();
        if self.operations.contains_key(&name) {
            return Err(format!("host operation `{name}` is already registered"));
        }
        self.operations.insert(
            name,
            HostOperation {
                arguments,
                result: None,
                handler: HostHandler::Stateless(Box::new(move |arguments| {
                    handler(arguments).map(|()| BytecodeValue::Unit)
                })),
            },
        );
        Ok(())
    }

    pub fn register_query(
        &mut self,
        name: impl Into<String>,
        arguments: Vec<HostValueType>,
        result: HostValueType,
        handler: impl FnMut(&[BytecodeValue]) -> Result<BytecodeValue, String> + Send + 'static,
    ) -> Result<(), String> {
        let name = name.into();
        if self.operations.contains_key(&name) {
            return Err(format!("host operation `{name}` is already registered"));
        }
        self.operations.insert(
            name,
            HostOperation {
                arguments,
                result: Some(result),
                handler: HostHandler::Stateless(Box::new(handler)),
            },
        );
        Ok(())
    }

    pub fn register_with_state(
        &mut self,
        name: impl Into<String>,
        arguments: Vec<HostValueType>,
        mut handler: impl FnMut(&mut BehaviorState, &[BytecodeValue]) -> Result<(), String>
            + Send
            + 'static,
    ) -> Result<(), String> {
        self.register_stateful_query(name, arguments, None, move |state, arguments| {
            handler(state, arguments).map(|()| BytecodeValue::Unit)
        })
    }

    fn register_stateful_query(
        &mut self,
        name: impl Into<String>,
        arguments: Vec<HostValueType>,
        result: Option<HostValueType>,
        handler: impl FnMut(&mut BehaviorState, &[BytecodeValue]) -> Result<BytecodeValue, String>
            + Send
            + 'static,
    ) -> Result<(), String> {
        let name = name.into();
        if self.operations.contains_key(&name) {
            return Err(format!("host operation `{name}` is already registered"));
        }
        self.operations.insert(
            name,
            HostOperation {
                arguments,
                result,
                handler: HostHandler::Stateful(Box::new(handler)),
            },
        );
        Ok(())
    }
    pub fn invoke(
        &mut self,
        program: &BytecodeProgram,
        callback: &str,
    ) -> Result<(), BytecodeError> {
        let _execution = self.begin_invoke(program)?;
        program.invoke(callback, self)
    }

    pub fn invoke_with_state(
        &mut self,
        program: &BytecodeProgram,
        callback: &str,
        state: &mut BehaviorState,
    ) -> Result<(), BytecodeError> {
        let _execution = self.begin_invoke(program)?;
        program.invoke_with_state(callback, self, state)
    }

    fn begin_invoke(&self, program: &BytecodeProgram) -> Result<ExecutionGuard, BytecodeError> {
        if self.executing.load(Ordering::Acquire) {
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
            let entry = self
                .operations
                .get(operation.name())
                .ok_or_else(|| BytecodeError {
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
                .map(|value| {
                    HostValueType::from_bytecode(value.clone()).ok_or_else(|| BytecodeError {
                        message: format!(
                            "host manifest cannot invoke `{}` with an unsupported {} argument",
                            operation.name(),
                            value
                        ),
                        span: SourceSpan::from(0),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
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
            let expected_result = match operation.result() {
                BytecodeValueType::Unit => None,
                value => Some(HostValueType::from_bytecode(value.clone()).ok_or_else(|| {
                    BytecodeError {
                        message: format!(
                            "host manifest cannot invoke `{}` with an unsupported result",
                            operation.name()
                        ),
                        span: SourceSpan::from(0),
                    }
                })?),
            };
            if entry.result != expected_result {
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
        if self.executing.swap(true, Ordering::AcqRel) {
            return Err(BytecodeError {
                message: "same-instance synchronous reentry is not allowed".into(),
                span: SourceSpan::from(0),
            });
        }
        self.dispatching.store(true, Ordering::Release);
        Ok(ExecutionGuard {
            executing: Arc::clone(&self.executing),
            dispatching: Arc::clone(&self.dispatching),
        })
    }
}

impl BytecodeHost for HostManifest {
    fn call(
        &mut self,
        operation: &BytecodeOperation,
        arguments: &[BytecodeValue],
    ) -> Result<BytecodeValue, String> {
        if !self.dispatching.load(Ordering::Acquire) {
            return Err("host manifest calls must be invoked through HostManifest::invoke".into());
        }
        let entry = self.operations.get_mut(operation.name()).ok_or_else(|| {
            format!(
                "host manifest v{} has no `{}` operation",
                operation.name(),
                self.version
            )
        })?;
        validate_host_arguments(operation, &entry.arguments, arguments)?;
        let value = match &mut entry.handler {
            HostHandler::Stateless(handler) => handler(arguments),
            HostHandler::Stateful(_) => Err(format!(
                "host operation `{}` requires behavior state",
                operation.name()
            )),
        }?;
        validate_host_result(operation, entry.result.as_ref(), &value)?;
        Ok(value)
    }

    fn call_with_state(
        &mut self,
        operation: &BytecodeOperation,
        arguments: &[BytecodeValue],
        state: &mut BehaviorState,
    ) -> Result<BytecodeValue, String> {
        if !self.dispatching.load(Ordering::Acquire) {
            return Err("host manifest calls must be invoked through HostManifest::invoke".into());
        }
        let entry = self.operations.get_mut(operation.name()).ok_or_else(|| {
            format!(
                "host manifest v{} has no `{}` operation",
                operation.name(),
                self.version
            )
        })?;
        validate_host_arguments(operation, &entry.arguments, arguments)?;
        let value = match &mut entry.handler {
            HostHandler::Stateless(handler) => handler(arguments),
            HostHandler::Stateful(handler) => handler(state, arguments),
        }?;
        validate_host_result(operation, entry.result.as_ref(), &value)?;
        Ok(value)
    }
}

fn validate_host_arguments(
    operation: &BytecodeOperation,
    expected_arguments: &[HostValueType],
    arguments: &[BytecodeValue],
) -> Result<(), String> {
    if expected_arguments.len() != arguments.len() {
        return Err(format!(
            "host operation `{}` expects {} argument(s), got {}",
            operation.name(),
            expected_arguments.len(),
            arguments.len()
        ));
    }
    for (expected, actual) in expected_arguments.iter().zip(arguments) {
        if !matches_type(expected, actual) {
            return Err(format!(
                "host operation `{}` received an incompatible argument",
                operation.name()
            ));
        }
    }
    Ok(())
}

fn matches_type(expected: &HostValueType, value: &BytecodeValue) -> bool {
    match (expected, value) {
        (HostValueType::I32, BytecodeValue::I32(_))
        | (HostValueType::Bool, BytecodeValue::Bool(_))
        | (HostValueType::String, BytecodeValue::String(_)) => true,
        (HostValueType::Record(expected), BytecodeValue::Record { fields }) => {
            expected.len() == fields.len()
                && expected.iter().all(|(name, expected)| {
                    fields
                        .get(name)
                        .is_some_and(|value| matches_type(expected, value))
                })
        }
        _ => false,
    }
}

fn validate_host_result(
    operation: &BytecodeOperation,
    expected: Option<&HostValueType>,
    value: &BytecodeValue,
) -> Result<(), String> {
    match expected {
        Some(expected) if !matches_type(expected, value) => Err(format!(
            "host operation `{}` returned an incompatible value",
            operation.name()
        )),
        None if value != &BytecodeValue::Unit => Err(format!(
            "host operation `{}` returned an incompatible value",
            operation.name()
        )),
        _ => Ok(()),
    }
}

/// Definition-time state for an entity-owned behavior module. Instances clone
/// these defaults, so no state is shared accidentally between entities.
#[derive(Debug, Clone)]
pub struct BehaviorModule {
    definition: Arc<BehaviorModuleDefinition>,
}

#[derive(Debug)]
struct BehaviorModuleDefinition {
    program: BytecodeProgram,
    defaults: BTreeMap<String, BytecodeValue>,
}

#[derive(Debug, Clone)]
pub struct BehaviorState {
    values: BTreeMap<String, BytecodeValue>,
}

impl BehaviorState {
    pub fn get(&self, name: &str) -> Option<&BytecodeValue> {
        self.values.get(name)
    }

    pub fn set(&mut self, name: &str, value: BytecodeValue) -> Result<(), String> {
        let previous = self
            .values
            .get(name)
            .ok_or_else(|| format!("unknown behavior state `{name}`"))?;
        if previous.value_type() != value.value_type() {
            return Err(format!("behavior state `{name}` has an incompatible type"));
        }
        self.values.insert(name.into(), value);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct BehaviorInstance {
    module: BehaviorModule,
    state: BehaviorState,
}

impl BehaviorModule {
    pub fn new(
        program: BytecodeProgram,
        defaults: BTreeMap<String, BytecodeValue>,
    ) -> Result<Self, BytecodeError> {
        if defaults
            .values()
            .any(|value| matches!(value, BytecodeValue::Unit))
        {
            return Err(BytecodeError {
                message: "behavior state defaults cannot be unit values".into(),
                span: SourceSpan::from(0),
            });
        }
        Ok(Self {
            definition: Arc::new(BehaviorModuleDefinition { program, defaults }),
        })
    }
    pub fn create_instance(
        &self,
        overrides: BTreeMap<String, BytecodeValue>,
    ) -> Result<BehaviorInstance, BytecodeError> {
        let mut state = self.definition.defaults.clone();
        for (name, value) in overrides {
            let default = state.get(&name).ok_or_else(|| BytecodeError {
                message: format!("unknown behavior state override `{name}`"),
                span: SourceSpan::from(0),
            })?;
            if default.value_type() != value.value_type() {
                return Err(BytecodeError {
                    message: format!("behavior state override `{name}` has an incompatible type"),
                    span: SourceSpan::from(0),
                });
            }
            state.insert(name, value);
        }
        Ok(BehaviorInstance {
            module: self.clone(),
            state: BehaviorState { values: state },
        })
    }
}

impl BehaviorInstance {
    pub fn state(&self, name: &str) -> Option<&BytecodeValue> {
        self.state.get(name)
    }
    pub fn set_state(&mut self, name: &str, value: BytecodeValue) -> Result<(), BytecodeError> {
        self.state
            .set(name, value)
            .map_err(|message| BytecodeError {
                message,
                span: SourceSpan::from(0),
            })
    }
    /// Rust owns scheduling: scripts can only run a named callback when this
    /// method is called by the host's update thread.
    pub fn invoke(
        &mut self,
        callback: &str,
        manifest: &mut HostManifest,
    ) -> Result<(), BytecodeError> {
        manifest.invoke_with_state(&self.module.definition.program, callback, &mut self.state)
    }
}

struct ExecutionGuard {
    executing: Arc<AtomicBool>,
    dispatching: Arc<AtomicBool>,
}

impl Drop for ExecutionGuard {
    fn drop(&mut self) {
        self.dispatching.store(false, Ordering::Release);
        self.executing.store(false, Ordering::Release);
    }
}

impl HostValueType {
    fn from_bytecode(value: BytecodeValueType) -> Option<Self> {
        match value {
            BytecodeValueType::Unit => None,
            BytecodeValueType::I32 => Some(Self::I32),
            BytecodeValueType::Bool => Some(Self::Bool),
            BytecodeValueType::String => Some(Self::String),
            BytecodeValueType::Record(fields) => Some(Self::Record(
                fields
                    .into_iter()
                    .map(|(name, value)| Some((name, Self::from_bytecode(value)?)))
                    .collect::<Option<BTreeMap<_, _>>>()?,
            )),
        }
    }
}

fn compile_host_operation(
    id: u16,
    name: &str,
    function: &FuncDeclStmt,
    span: SourceSpan,
    records: &BTreeMap<&str, &StructDeclStmt>,
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
        .map(|parameter| {
            bytecode_parameter_type(&parameter.value.ty.value, parameter.span, records)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let result = function
        .ty
        .as_ref()
        .map_or(Ok(BytecodeValueType::Unit), |ty| {
            bytecode_type(&ty.value, ty.span, records)
        })?;
    Ok(BytecodeOperation {
        id,
        name: name.into(),
        arguments,
        result,
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
            bytecode_type(&ty.value, ty.span, &BTreeMap::new())
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
    let mut locals = BTreeMap::new();
    for statement in &block.stmts {
        match &statement.value {
            Stmt::ExprStmt(expr) => code.push(compile_call(&expr.expr, operations, &locals, None)?),
            Stmt::VarDeclStmt(declaration) => {
                let init = declaration
                    .decl
                    .value
                    .init
                    .as_ref()
                    .ok_or_else(|| BytecodeError {
                        message: "bytecode locals must be initialized by a host query".into(),
                        span: declaration.decl.span,
                    })?;
                let name = declaration.decl.value.name.value.clone();
                let instruction = compile_call(init, operations, &locals, Some(name.clone()))?;
                let Instruction::CallHost { operation, .. } = &instruction;
                let result = operations
                    .values()
                    .find(|candidate| candidate.id == *operation)
                    .expect("compiled instruction references a known operation")
                    .result()
                    .clone();
                if result == BytecodeValueType::Unit {
                    return Err(BytecodeError {
                        message: "bytecode locals must be initialized by a host query".into(),
                        span: declaration.decl.span,
                    });
                }
                locals.insert(name, result);
                code.push(instruction);
            }
            _ => {
                return Err(BytecodeError {
                    message:
                        "only host calls and query bindings are supported in bytecode callbacks"
                            .into(),
                    span: statement.span,
                })
            }
        }
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
    locals: &BTreeMap<String, BytecodeValueType>,
    result: Option<String>,
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
        let value = bytecode_value(argument, locals)?;
        let actual = value.value_type(locals)?;
        let expected = &operation.arguments[index];
        if actual != *expected {
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
        result,
        span: expr.span,
    })
}

impl BytecodeOperand {
    fn value_type(
        &self,
        locals: &BTreeMap<String, BytecodeValueType>,
    ) -> Result<BytecodeValueType, BytecodeError> {
        match self {
            Self::Value(value) => Ok(value.value_type()),
            Self::Local(name) => locals.get(name).cloned().ok_or_else(|| BytecodeError {
                message: format!("unknown bytecode local `{name}`"),
                span: SourceSpan::from(0),
            }),
        }
    }
}

fn bytecode_value(
    argument: &SourcedExpr,
    locals: &BTreeMap<String, BytecodeValueType>,
) -> Result<BytecodeOperand, BytecodeError> {
    match &argument.expr {
        Expr::LitExpr(literal) => match &literal.lit {
            Lit::Integer(value) => bytecode_i32(*value, argument.span).map(BytecodeOperand::Value),
            Lit::Boolean(value) => Ok(BytecodeOperand::Value(BytecodeValue::Bool(*value))),
            Lit::String(value) => Ok(BytecodeOperand::Value(BytecodeValue::String(
                decode_string_literal(value, argument.span)?,
            ))),
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
            .map(BytecodeOperand::Value)
        }
        Expr::IdentExpr(ident) if locals.contains_key(&ident.ident) => {
            Ok(BytecodeOperand::Local(ident.ident.clone()))
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
    records: &BTreeMap<&str, &StructDeclStmt>,
) -> Result<BytecodeValueType, BytecodeError> {
    if matches!(ty, Type::Path(path) if path.path.segments.len() == 1 && path.path.segments[0].value == "str")
    {
        return Err(BytecodeError {
            message: "bytecode host operation string parameters must use `&str`".into(),
            span,
        });
    }
    let value_type = bytecode_type(ty, span, records)?;
    if value_type == BytecodeValueType::Unit {
        return Err(BytecodeError {
            message: "bytecode host operation parameters must not use `()`".into(),
            span,
        });
    }
    Ok(value_type)
}

fn bytecode_type(
    ty: &Type,
    span: SourceSpan,
    records: &BTreeMap<&str, &StructDeclStmt>,
) -> Result<BytecodeValueType, BytecodeError> {
    let result = match ty {
        Type::Tuple(tuple) if tuple.types.is_empty() => Some(BytecodeValueType::Unit),
        Type::Path(path) if path.path.segments.len() == 1 => {
            match path.path.segments[0].value.as_str() {
                "i32" => Some(BytecodeValueType::I32),
                "bool" => Some(BytecodeValueType::Bool),
                "str" => Some(BytecodeValueType::String),
                name => records
                    .get(name)
                    .map(|record| {
                        record
                            .fields
                            .iter()
                            .map(|field| {
                                Ok((
                                    field.value.name.value.clone(),
                                    bytecode_type(&field.value.ty.value, field.span, records)?,
                                ))
                            })
                            .collect::<Result<BTreeMap<_, _>, BytecodeError>>()
                            .map(BytecodeValueType::Record)
                    })
                    .transpose()?,
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
        ) -> Result<BytecodeValue, String> {
            self.0.push((operation.id(), arguments.to_vec()));
            Ok(BytecodeValue::Unit)
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
            .register("record", vec![HostValueType::I32], move |args| {
                target.lock().unwrap().extend_from_slice(args);
                Ok(())
            })
            .unwrap();
        manifest.invoke(&program, "tick").unwrap();
        assert_eq!(*seen.lock().unwrap(), vec![BytecodeValue::I32(7)]);
    }
    #[test]
    fn behavior_instances_keep_independent_state() {
        let program = BytecodeProgram::compile("callback fn update() -> () { }").unwrap();
        let module = BehaviorModule::new(
            program,
            BTreeMap::from([("health".into(), BytecodeValue::I32(10))]),
        )
        .unwrap();
        let mut left = module.create_instance(BTreeMap::new()).unwrap();
        let right = module
            .create_instance(BTreeMap::from([("health".into(), BytecodeValue::I32(20))]))
            .unwrap();
        assert!(Arc::ptr_eq(
            &left.module.definition,
            &right.module.definition
        ));
        left.set_state("health", BytecodeValue::I32(5)).unwrap();
        assert_eq!(left.state("health"), Some(&BytecodeValue::I32(5)));
        assert_eq!(right.state("health"), Some(&BytecodeValue::I32(20)));
    }

    #[test]
    fn behavior_callbacks_receive_their_instance_state() {
        let program =
            BytecodeProgram::compile("extern fn heal(); callback fn update() -> () { heal(); }")
                .unwrap();
        let module = BehaviorModule::new(
            program,
            BTreeMap::from([("health".into(), BytecodeValue::I32(10))]),
        )
        .unwrap();
        let mut left = module.create_instance(BTreeMap::new()).unwrap();
        let mut right = module
            .create_instance(BTreeMap::from([("health".into(), BytecodeValue::I32(20))]))
            .unwrap();
        let mut manifest = HostManifest::new(DEFAULT_HOST_API_VERSION);
        manifest
            .register_with_state("heal", vec![], |state, _| {
                let BytecodeValue::I32(health) = state.get("health").unwrap() else {
                    return Err("health must be i32".into());
                };
                state.set("health", BytecodeValue::I32(health + 1))
            })
            .unwrap();

        left.invoke("update", &mut manifest).unwrap();
        right.invoke("update", &mut manifest).unwrap();

        assert_eq!(left.state("health"), Some(&BytecodeValue::I32(11)));
        assert_eq!(right.state("health"), Some(&BytecodeValue::I32(21)));
    }

    #[test]
    fn direct_program_invocation_cannot_bypass_manifest_validation() {
        let program = BytecodeProgram::compile_for_host_api(
            "extern fn record(); callback fn tick() -> () { record(); }",
            2,
        )
        .unwrap();
        let mut manifest = HostManifest::new(1);
        manifest.register("record", vec![], |_| Ok(())).unwrap();

        let error = program.invoke("tick", &mut manifest).unwrap_err();
        assert_eq!(
            error.message,
            "host manifest calls must be invoked through HostManifest::invoke"
        );
        let error = manifest.invoke(&program, "tick").unwrap_err();
        assert_eq!(
            error.message,
            "bytecode requires host API version 2, but manifest provides version 1"
        );
    }

    #[test]
    fn direct_stateful_program_invocation_cannot_bypass_manifest_validation() {
        let program = BytecodeProgram::compile_for_host_api(
            "extern fn heal(); callback fn tick() -> () { heal(); }",
            2,
        )
        .unwrap();
        let module = BehaviorModule::new(
            program,
            BTreeMap::from([("health".into(), BytecodeValue::I32(10))]),
        )
        .unwrap();
        let mut instance = module.create_instance(BTreeMap::new()).unwrap();
        let mut manifest = HostManifest::new(1);
        manifest
            .register_with_state("heal", vec![], |state, _| {
                state.set("health", BytecodeValue::I32(0))
            })
            .unwrap();

        let error = module
            .definition
            .program
            .invoke_with_state("tick", &mut manifest, &mut instance.state)
            .unwrap_err();
        assert_eq!(
            error.message,
            "host manifest calls must be invoked through HostManifest::invoke"
        );
        assert_eq!(instance.state("health"), Some(&BytecodeValue::I32(10)));

        let error = instance.invoke("tick", &mut manifest).unwrap_err();
        assert_eq!(
            error.message,
            "bytecode requires host API version 2, but manifest provides version 1"
        );
    }

    #[test]
    fn manifest_is_reusable_after_a_caught_host_panic() {
        use std::panic::{catch_unwind, AssertUnwindSafe};
        use std::sync::atomic::AtomicUsize;

        let program =
            BytecodeProgram::compile("extern fn record(); callback fn tick() -> () { record(); }")
                .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let handler_calls = Arc::clone(&calls);
        let mut manifest = HostManifest::new(DEFAULT_HOST_API_VERSION);
        manifest
            .register("record", vec![], move |_| {
                if handler_calls.fetch_add(1, Ordering::Relaxed) == 0 {
                    panic!("host panic");
                }
                Ok(())
            })
            .unwrap();

        assert!(catch_unwind(AssertUnwindSafe(|| manifest.invoke(&program, "tick"))).is_err());
        manifest.invoke(&program, "tick").unwrap();
    }

    #[test]
    fn manifest_rejects_an_incompatible_api_version_or_signature() {
        let program = BytecodeProgram::compile_for_host_api(
            "extern fn record(value: i32); callback fn tick() -> () { record(7); }",
            2,
        )
        .unwrap();
        let mut manifest = HostManifest::new(1);
        manifest
            .register("record", vec![HostValueType::I32], |_| Ok(()))
            .unwrap();

        let error = manifest.invoke(&program, "tick").unwrap_err();
        assert_eq!(
            error.message,
            "bytecode requires host API version 2, but manifest provides version 1"
        );

        let program = BytecodeProgram::compile(
            "extern fn record(value: i32); callback fn tick() -> () { record(7); }",
        )
        .unwrap();
        let mut manifest = HostManifest::new(DEFAULT_HOST_API_VERSION);
        manifest
            .register("record", vec![HostValueType::Bool], |_| Ok(()))
            .unwrap();

        let error = manifest.invoke(&program, "tick").unwrap_err();
        assert_eq!(
            error.message,
            "host manifest v1 has an incompatible `record` operation signature"
        );
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

        let return_type = BytecodeProgram::compile("extern fn record() -> u32;").unwrap_err();
        assert_eq!(
            return_type.message,
            "bytecode host operations only support i32, bool, str, and () values"
        );

        let unit_parameter = BytecodeProgram::compile("extern fn record(value: ());").unwrap_err();
        assert_eq!(
            unit_parameter.message,
            "bytecode host operation parameters must not use `()`"
        );

        let bare_string = BytecodeProgram::compile("extern fn record(value: str);").unwrap_err();
        assert_eq!(
            bare_string.message,
            "bytecode host operation string parameters must use `&str`"
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
            fn call(
                &mut self,
                _: &BytecodeOperation,
                _: &[BytecodeValue],
            ) -> Result<BytecodeValue, String> {
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

    #[test]
    fn query_results_can_flow_to_typed_record_commands() {
        let program = BytecodeProgram::compile(
            "struct Reading { value: i32 }\n\
             extern fn query() -> Reading;\n\
             extern fn emit(reading: Reading);\n\
             callback fn tick() { let reading = query(); emit(reading); }",
        )
        .unwrap();
        let reading_type =
            HostValueType::Record([("value".into(), HostValueType::I32)].into_iter().collect());
        let seen = Arc::new(std::sync::Mutex::new(None));
        let target = Arc::clone(&seen);
        let mut manifest = HostManifest::new(DEFAULT_HOST_API_VERSION);
        manifest
            .register_query("query", vec![], reading_type.clone(), |_| {
                Ok(BytecodeValue::Record {
                    fields: [("value".into(), BytecodeValue::I32(42))]
                        .into_iter()
                        .collect(),
                })
            })
            .unwrap();
        manifest
            .register("emit", vec![reading_type], move |arguments| {
                *target.lock().unwrap() = Some(arguments[0].clone());
                Ok(())
            })
            .unwrap();

        manifest.invoke(&program, "tick").unwrap();
        assert_eq!(
            *seen.lock().unwrap(),
            Some(BytecodeValue::Record {
                fields: [("value".into(), BytecodeValue::I32(42))]
                    .into_iter()
                    .collect(),
            })
        );

        let incompatible_type = HostValueType::Record(
            [("value".into(), HostValueType::Bool)]
                .into_iter()
                .collect(),
        );
        let mut incompatible = HostManifest::new(DEFAULT_HOST_API_VERSION);
        incompatible
            .register_query("query", vec![], incompatible_type.clone(), |_| {
                Ok(BytecodeValue::Record {
                    fields: [("value".into(), BytecodeValue::Bool(true))]
                        .into_iter()
                        .collect(),
                })
            })
            .unwrap();
        incompatible
            .register("emit", vec![incompatible_type], |_| Ok(()))
            .unwrap();
        assert_eq!(
            incompatible.invoke(&program, "tick").unwrap_err().message,
            "host manifest v1 has an incompatible `emit` operation signature"
        );
    }
}
