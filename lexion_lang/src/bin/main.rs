use std::sync::Arc;

use clap::{Parser, ValueEnum};
use enumflags2::BitFlag;
use lexion_lang::compiler::{EmitTarget, LexionCompiler, LexionCompilerOptions};
use lexion_lang::{CompilationError, Dump, DumpFlags};
use lexion_lib::miette;
use lexion_lib::miette::{NamedSource, Report};
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(long_about = None)]
struct Args {
    filename: String,
    #[arg(long, default_value_t = Dump::empty().into())]
    dump: DumpFlags,
    #[arg(long, default_value_t = String::from("dump"))]
    dump_dir: String,
    #[arg(long, value_enum, default_value_t = EmitArg::Check)]
    emit: EmitArg,
    #[arg(long)]
    output: Option<PathBuf>,
}

impl Args {
    fn split(self) -> (String, LexionCompilerOptions, Option<PathBuf>) {
        let output = self.output;
        (
            self.filename,
            LexionCompilerOptions {
                dump_flags: self.dump,
                dump_dir: self.dump_dir.into(),
                emit: self.emit.into(),
                ..LexionCompilerOptions::default()
            },
            output,
        )
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum EmitArg {
    Check,
    Asm,
    Elf64,
}

impl From<EmitArg> for EmitTarget {
    fn from(value: EmitArg) -> Self {
        match value {
            EmitArg::Check => EmitTarget::Check,
            EmitArg::Asm => EmitTarget::X86Assembly,
            EmitArg::Elf64 => EmitTarget::X86Elf64,
        }
    }
}

fn main() -> Result<(), CompilationError> {
    miette::set_hook(Box::new(|_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .force_graphical(true)
                .terminal_links(true)
                .context_lines(2)
                .color(true)
                .unicode(true)
                .break_words(true)
                .build(),
        )
    }))
    .expect("failed to initialize logging hook");
    let (filename, options, output_path) = Args::parse().split();
    let source_code =
        Arc::new(std::fs::read_to_string(filename.as_str()).map_err(CompilationError::IO)?);
    let source = NamedSource::new(filename.as_str(), source_code);
    match LexionCompiler::new(options).exec(source) {
        Ok(output) => {
            if !output.diagnostics.is_empty() {
                println!("{:?}", Report::new(output.diagnostics));
            }
            if let Some(assembly) = output.assembly {
                match output_path.as_ref() {
                    Some(path) => write_output(path, assembly.as_str())?,
                    None => println!("{assembly}"),
                }
            }
            if let Some(executable) = output.executable {
                let Some(path) = output_path.as_ref() else {
                    return Err(CompilationError::OutputRequired);
                };
                write_output(path, executable.as_bytes())?;
            }
            Ok(())
        }
        Err(list) => {
            if !list.is_empty() {
                println!("{:?}", Report::new(list));
            }
            Err(CompilationError::CompilationFailed)
        }
    }
}

fn write_output(path: &Path, content: impl AsRef<[u8]>) -> Result<(), CompilationError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(CompilationError::IO)?;
    }
    std::fs::write(path, content).map_err(CompilationError::IO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_assembly_emit_mode() {
        let (_, options, output) = Args::parse_from([
            "lexion",
            "--emit",
            "asm",
            "--output",
            "target/test-dumps/out.s",
            "tests/fixtures/backend/x86_return_arithmetic.lex",
        ])
        .split();

        assert_eq!(options.emit, EmitTarget::X86Assembly);
        assert_eq!(output, Some(PathBuf::from("target/test-dumps/out.s")));
    }

    #[test]
    fn parses_elf64_emit_mode() {
        let (_, options, output) = Args::parse_from([
            "lexion",
            "--emit",
            "elf64",
            "--output",
            "target/test-dumps/out",
            "tests/fixtures/backend/x86_exit_status.lex",
        ])
        .split();

        assert_eq!(options.emit, EmitTarget::X86Elf64);
        assert_eq!(output, Some(PathBuf::from("target/test-dumps/out")));
    }
}
