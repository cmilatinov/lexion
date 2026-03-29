use derived_deref::{Deref, DerefMut};
use enumflags2::{bitflags, BitFlag, BitFlags};
use lexion_lib::itertools::Itertools;
use std::fmt::Display;
use std::str::FromStr;
use thiserror::Error;

pub mod ast;
pub mod compiler;
pub mod diagnostic;
pub mod generators;
pub mod operators;
pub mod parser;
pub mod pipeline;
pub mod symbol_table;
pub mod type_checker;

#[bitflags]
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dump {
    ParseTable,
    ParseTrace,
    Grammar,
    AbstractSyntaxTree,
    Symbols,
    Types,
    IntermediateRepresentation,
    ControlFlowGraph,
}

impl Dump {
    const PARSE_TABLE: &'static str = "parse_table";
    const PARSE_TRACE: &'static str = "parse_trace";
    const GRAMMAR: &'static str = "grammar";
    const AST: &'static str = "ast";
    const SYMBOLS: &'static str = "symbols";
    const TYPES: &'static str = "types";
    const IR: &'static str = "ir";
    const CFG: &'static str = "cfg";
    const ALL: &'static str = "all";

    pub fn dump_options() -> impl Iterator<Item = (&'static str, BitFlags<Dump>)> {
        [
            (Self::PARSE_TABLE, Dump::ParseTable.into()),
            (Self::PARSE_TRACE, Dump::ParseTrace.into()),
            (Self::GRAMMAR, Dump::Grammar.into()),
            (Self::AST, Dump::AbstractSyntaxTree.into()),
            (Self::SYMBOLS, Dump::Symbols.into()),
            (Self::TYPES, Dump::Types.into()),
            (Self::IR, Dump::IntermediateRepresentation.into()),
            (Self::CFG, Dump::ControlFlowGraph.into()),
            (Self::ALL, Dump::all()),
        ]
        .into_iter()
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Dump::ParseTable => Self::PARSE_TABLE,
            Dump::ParseTrace => Self::PARSE_TRACE,
            Dump::Grammar => Self::GRAMMAR,
            Dump::AbstractSyntaxTree => Self::AST,
            Dump::Symbols => Self::SYMBOLS,
            Dump::Types => Self::TYPES,
            Dump::IntermediateRepresentation => Self::IR,
            Dump::ControlFlowGraph => Self::CFG,
        }
    }
}

#[derive(Debug, Clone, Copy, Deref, DerefMut)]
pub struct DumpFlags(BitFlags<Dump>);

impl From<Dump> for DumpFlags {
    fn from(value: Dump) -> Self {
        Self(BitFlags::from_flag(value))
    }
}

impl From<BitFlags<Dump>> for DumpFlags {
    fn from(value: BitFlags<Dump>) -> Self {
        Self(value)
    }
}

impl FromStr for DumpFlags {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut flags = Dump::empty();

        for part in s.split(",") {
            let part = part.trim().to_lowercase();
            let Some((_, f)) = Dump::dump_options().find(|&(flag, _)| flag == part) else {
                return Err(format!("invalid dump flag: {part}"));
            };
            flags |= f;
        }

        Ok(flags.into())
    }
}

impl Display for DumpFlags {
    fn fmt(&self, f1: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = if self.is_all() {
            "all".to_string()
        } else {
            self.iter().map(|f| f.as_str()).join(",")
        };
        write!(f1, "{str}")
    }
}

#[derive(Debug, Error)]
pub enum CompilationError {
    #[error("{0}")]
    IO(std::io::Error),
    #[error("failed to compile source code")]
    CompilationFailed,
}
