use crate::ast::types::{PrimitiveType, Type, TypeCollection};
use crate::diagnostic::{DiagnosticConsumer, LexionDiagnosticError};
use crate::generators::tac::instructions::{
    ControlFlowGraph, FunctionCallInstruction, Instruction,
};
use crate::generators::x86::{CodeGeneratorX86Machine, X86MachineCode, X86MachineCodeOptions};
use crate::pipeline::PipelineStage;
use crate::symbol_table::SymbolTableGraph;
use iced_x86::code_asm::*;
use iced_x86::{BlockEncoderOptions, IcedError};
use lexion_lib::miette::{NamedSource, SourceSpan};
use std::collections::BTreeMap;
use std::sync::Arc;

const ELF_HEADER_SIZE: usize = 64;
const PROGRAM_HEADER_SIZE: usize = 56;
const LOADABLE_SEGMENT_ALIGN: u64 = 0x1000;
const DEFAULT_IMAGE_BASE: u64 = 0x400000;
const DEFAULT_TEXT_OFFSET: u64 = 0x1000;
const ENTRY_SYMBOL: &str = "main";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X86ElfExecutable {
    bytes: Vec<u8>,
    entry_point: u64,
    text_offset: usize,
    runtime_size: usize,
    data_offset: usize,
    symbols: BTreeMap<String, u64>,
}

impl X86ElfExecutable {
    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    pub fn entry_point(&self) -> u64 {
        self.entry_point
    }

    pub fn text_offset(&self) -> usize {
        self.text_offset
    }

    pub fn runtime_size(&self) -> usize {
        self.runtime_size
    }

    pub fn data_offset(&self) -> usize {
        self.data_offset
    }

    pub fn symbols(&self) -> &BTreeMap<String, u64> {
        &self.symbols
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct X86ElfOptions {
    pub image_base: u64,
    pub text_offset: u64,
}

impl Default for X86ElfOptions {
    fn default() -> Self {
        Self {
            image_base: DEFAULT_IMAGE_BASE,
            text_offset: DEFAULT_TEXT_OFFSET,
        }
    }
}

pub struct CodeGeneratorX86Elf<'a> {
    cfg: &'a ControlFlowGraph,
    types: &'a TypeCollection,
    symbols: &'a SymbolTableGraph,
}

impl<'a> CodeGeneratorX86Elf<'a> {
    fn emit(
        &self,
        diag: &mut dyn DiagnosticConsumer,
        options: X86ElfOptions,
    ) -> Option<X86ElfExecutable> {
        if !self.validate_supported(diag) {
            return None;
        }

        let min_text_offset = (ELF_HEADER_SIZE + PROGRAM_HEADER_SIZE) as u64;
        if options.text_offset < min_text_offset {
            diag.error(elf_error(format!(
                "ELF text offset must leave room for headers: {min_text_offset} bytes required"
            )));
            return None;
        }

        let entry_point = options.image_base + options.text_offset;
        let probe_runtime = match assemble_runtime(entry_point, entry_point) {
            Ok(bytes) => bytes,
            Err(err) => {
                diag.error(elf_error(format!("x86 ELF runtime emission failed: {err}")));
                return None;
            }
        };
        let code_base = entry_point + probe_runtime.len() as u64;
        let code = CodeGeneratorX86Machine::new((self.cfg, self.types, self.symbols)).exec(
            diag,
            X86MachineCodeOptions {
                base_address: code_base,
            },
        )?;
        let Some(main_offset) = code.symbols().get(ENTRY_SYMBOL) else {
            diag.error(elf_error(format!(
                "x86 ELF executable output requires a `{ENTRY_SYMBOL}` function"
            )));
            return None;
        };

        let main_address = code_base + *main_offset as u64;
        let runtime = match assemble_runtime(entry_point, main_address) {
            Ok(bytes) => bytes,
            Err(err) => {
                diag.error(elf_error(format!("x86 ELF runtime emission failed: {err}")));
                return None;
            }
        };
        if runtime.len() != probe_runtime.len() {
            diag.error(elf_error(String::from(
                "x86 ELF runtime size changed while resolving entry call",
            )));
            return None;
        }

        let text = executable_text(&runtime, &code);
        let bytes = build_elf(options, &text);
        let symbols = code
            .symbols()
            .iter()
            .map(|(name, offset)| (name.clone(), code_base + *offset as u64))
            .collect();
        Some(X86ElfExecutable {
            bytes,
            entry_point,
            text_offset: options.text_offset as usize,
            runtime_size: runtime.len(),
            data_offset: options.text_offset as usize + runtime.len() + code.data_offset(),
            symbols,
        })
    }

    fn validate_supported(&self, diag: &mut dyn DiagnosticConsumer) -> bool {
        let mut valid = true;
        if let Some(message) = self.unsupported_entry_point_return_message() {
            valid = false;
            diag.error(elf_error(message));
        }
        for block in self.cfg.node_weights() {
            for inst in &block.instructions {
                if let Some(message) = self.unsupported_message(&inst.instruction) {
                    valid = false;
                    diag.error(elf_error(message));
                }
            }
        }
        valid
    }

    fn unsupported_entry_point_return_message(&self) -> Option<String> {
        let return_type = self
            .symbols
            .lookup_function_entry(ENTRY_SYMBOL)
            .map(|(_, _, entry)| entry)
            .and_then(|entry| entry.var_type)
            .and_then(|ty| self.types.get(self.types.canonicalize(ty)))
            .and_then(|ty| match ty {
                Type::FunctionType(signature) => Some(signature.return_type),
                _ => None,
            })?;
        let return_type = self.types.canonicalize(return_type);
        let supported = match self.types.get(return_type) {
            Some(Type::PrimitiveType(
                PrimitiveType::I32 | PrimitiveType::U32 | PrimitiveType::BOOL | PrimitiveType::CHAR,
            )) => true,
            Some(Type::TupleType(tuple)) => tuple.types.is_empty(),
            _ => false,
        };
        (!supported).then(|| {
            format!(
                "x86 ELF executable output requires `{ENTRY_SYMBOL}` to return an integer scalar or unit, found {}",
                self.types.to_string_index(return_type)
            )
        })
    }

    fn unsupported_message(&self, instruction: &Instruction) -> Option<String> {
        match instruction {
            Instruction::Extern(inst) => Some(format!(
                "x86 ELF executable output does not support extern declarations yet: {}",
                inst.label
            )),
            Instruction::FunctionCall(inst) => self.unsupported_extern_call_message(inst),
            _ => None,
        }
    }

    fn unsupported_extern_call_message(&self, inst: &FunctionCallInstruction) -> Option<String> {
        inst.target
            .direct_name()
            .filter(|function| self.is_extern_function(function))
            .map(|function| {
            format!(
                "x86 ELF executable output does not support calls to extern functions until relocations are implemented: {function}"
            )
        })
    }

    fn is_extern_function(&self, function: &str) -> bool {
        self.cfg.node_weights().any(|block| {
            block.instructions.iter().any(|inst| {
                matches!(
                    &inst.instruction,
                    Instruction::Extern(external) if external.label == function
                )
            })
        })
    }
}

impl<'a> PipelineStage for CodeGeneratorX86Elf<'a> {
    type Input = (
        &'a ControlFlowGraph,
        &'a TypeCollection,
        &'a SymbolTableGraph,
    );
    type Options = X86ElfOptions;
    type Output = X86ElfExecutable;

    fn new((cfg, types, symbols): Self::Input) -> Self {
        Self {
            cfg,
            types,
            symbols,
        }
    }

    fn exec(self, diag: &mut dyn DiagnosticConsumer, opts: Self::Options) -> Option<Self::Output> {
        self.emit(diag, opts)
    }
}

fn assemble_runtime(entry_point: u64, main_address: u64) -> Result<Vec<u8>, IcedError> {
    let mut assembler = CodeAssembler::new(64)?;
    assembler.call(main_address)?;
    assembler.mov(edi, eax)?;
    assembler.mov(eax, 60)?;
    assembler.syscall()?;
    let result = assembler.assemble_options(entry_point, BlockEncoderOptions::NONE)?;
    Ok(result.inner.code_buffer)
}

fn executable_text(runtime: &[u8], code: &X86MachineCode) -> Vec<u8> {
    let mut text = Vec::with_capacity(runtime.len() + code.as_bytes().len());
    text.extend_from_slice(runtime);
    text.extend_from_slice(code.as_bytes());
    text
}

fn build_elf(options: X86ElfOptions, text: &[u8]) -> Vec<u8> {
    let text_offset = options.text_offset as usize;
    let file_size = text_offset + text.len();
    let mut bytes = Vec::with_capacity(file_size);
    write_elf_header(&mut bytes, options, file_size as u64);
    write_program_header(&mut bytes, options, file_size as u64);
    bytes.resize(text_offset, 0);
    bytes.extend_from_slice(text);
    bytes
}

fn write_elf_header(bytes: &mut Vec<u8>, options: X86ElfOptions, file_size: u64) {
    bytes.extend_from_slice(&[0x7F, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    push_u16(bytes, 2);
    push_u16(bytes, 62);
    push_u32(bytes, 1);
    push_u64(bytes, options.image_base + options.text_offset);
    push_u64(bytes, ELF_HEADER_SIZE as u64);
    push_u64(bytes, 0);
    push_u32(bytes, 0);
    push_u16(bytes, ELF_HEADER_SIZE as u16);
    push_u16(bytes, PROGRAM_HEADER_SIZE as u16);
    push_u16(bytes, 1);
    push_u16(bytes, 0);
    push_u16(bytes, 0);
    push_u16(bytes, 0);
    debug_assert_eq!(bytes.len(), ELF_HEADER_SIZE);
    debug_assert!(file_size >= options.text_offset);
}

fn write_program_header(bytes: &mut Vec<u8>, options: X86ElfOptions, file_size: u64) {
    push_u32(bytes, 1);
    push_u32(bytes, 5);
    push_u64(bytes, 0);
    push_u64(bytes, options.image_base);
    push_u64(bytes, options.image_base);
    push_u64(bytes, file_size);
    push_u64(bytes, file_size);
    push_u64(bytes, LOADABLE_SEGMENT_ALIGN);
    debug_assert_eq!(bytes.len(), ELF_HEADER_SIZE + PROGRAM_HEADER_SIZE);
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn elf_error(message: String) -> LexionDiagnosticError {
    LexionDiagnosticError {
        src: NamedSource::new("<x86 elf>", Arc::new(String::new())),
        span: SourceSpan::from(0),
        message,
    }
}
