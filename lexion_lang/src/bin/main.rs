use std::sync::Arc;

use clap::Parser;
use enumflags2::BitFlag;
use lexion_lang::compiler::{LexionCompiler, LexionCompilerOptions};
use lexion_lang::{CompilationError, Dump, DumpFlags};
use lexion_lib::miette;
use lexion_lib::miette::{NamedSource, Report};

#[derive(Parser, Debug)]
#[command(long_about = None)]
struct Args {
    filename: String,
    #[arg(long, default_value_t = Dump::empty().into())]
    dump: DumpFlags,
    #[arg(long, default_value_t = String::from("dump"))]
    dump_dir: String,
}

impl Args {
    fn split(self) -> (String, LexionCompilerOptions) {
        (
            self.filename,
            LexionCompilerOptions {
                dump_flags: self.dump,
                dump_dir: self.dump_dir.into(),
            },
        )
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
    let (filename, options) = Args::parse().split();
    let source_code =
        Arc::new(std::fs::read_to_string(filename.as_str()).map_err(CompilationError::IO)?);
    let source = NamedSource::new(filename.as_str(), source_code);
    match LexionCompiler::new(options).exec(source) {
        Ok(list) => {
            if !list.is_empty() {
                println!("{:?}", Report::new(list));
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
