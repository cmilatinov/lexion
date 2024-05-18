pub mod ast;
pub mod diagnostic;
pub mod parser;
pub mod pipeline;
pub mod symbol_table;
pub mod type_checker;
pub mod validator;

#[cfg(test)]
mod tests {
    use lexion_lib::prettytable::{format, Table};

    use crate::diagnostic::DiagnosticPrinterStdout;
    use crate::parser::ParserLexion;
    use crate::pipeline::PipelineStage;
    use crate::symbol_table::SymbolTableGenerator;

    #[test]
    fn test_parse() {
        let parser = ParserLexion::new();
        let mut trace = Table::new();
        trace.set_format(*format::consts::FORMAT_BOX_CHARS);
        let result =
            parser.parse_from_string_trace(include_str!("../tests/function.txt"), Some(&mut trace));
        trace.printstd();
        match result {
            Ok(ast) => {
                if let Some((graph, types)) =
                    SymbolTableGenerator::default().exec(&mut DiagnosticPrinterStdout, &ast)
                {
                    if let Some(table) = graph.table(graph.root, Some(&types)) {
                        println!("{}", table);
                    }
                }
            }
            Err(err) => println!("{}", err),
        };
    }
}
