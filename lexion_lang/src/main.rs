use std::sync::Arc;

use clap::Parser;

use lexion_lang::diagnostic::LexionDiagnosticList;
use lexion_lang::parser::ParserLexion;
use lexion_lang::pipeline::PipelineStage;
use lexion_lang::symbol_table::SymbolTableGenerator;
use lexion_lang::type_checker::TypeChecker;
use lexion_lib::miette;
use lexion_lib::miette::{NamedSource, Report};

pub struct LexionCompiler;

impl LexionCompiler {
    fn exec(args: &Args, source_code: Arc<String>) -> Result<(), ()> {
        let parser = ParserLexion::new();
        let src = NamedSource::new(args.filename.clone(), source_code.clone());
        match parser.parse_from_string(source_code) {
            Err(err) => {
                println!("{:?}", Report::new(err));
                Err(())
            }
            Ok(ast) => {
                let mut list = LexionDiagnosticList::default();
                if let Some((mut graph, mut types)) =
                    SymbolTableGenerator::new((src.clone(), &ast)).exec(&mut list)
                {
                    if let Some(table) = graph.table(graph.root, Some(&types)) {
                        if args.table {
                            println!("{}", table);
                        }
                    }
                    TypeChecker::new((src, &ast, &mut graph, &mut types)).exec(&mut list);
                }
                if !list.is_empty() {
                    println!("{:?}", Report::new(list));
                }
                Ok(())
            }
        }
    }
}

#[derive(Parser, Debug)]
#[command(long_about = None)]
struct Args {
    filename: String,
    #[arg(short, long)]
    table: bool,
}

fn main() -> Result<(), ()> {
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
    .map_err(|_| ())?;
    let args = Args::parse();
    let source_code = Arc::new(std::fs::read_to_string(args.filename.as_str()).map_err(|_| ())?);
    LexionCompiler::exec(&args, source_code)
}
