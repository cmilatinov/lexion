pub mod ast;
pub mod parser;
pub mod symbol_table_generator;
pub mod typechecker;
pub mod validator;

#[cfg(test)]
mod tests {
    use lexion_lib::prettytable::{format, Table};

    use crate::parser::ParserLexion;
    use crate::symbol_table_generator::SymbolTableGenerator;

    #[test]
    fn test_parse() {
        let parser = ParserLexion::new();
        let mut trace = Table::new();
        trace.set_format(*format::consts::FORMAT_BOX_CHARS);
        let result = parser.parse_from_string_trace(
            r#"fn test(a: test, b: t2) -> cringe {
                let a = 34;
                let b: &str = "1";
                let c, d;
                a = abc(a, b, c);
                b = "" + 2 - 0.1;
                c = 1.0e-32;
                d = true;
            }"#,
            Some(&mut trace),
        );
        match result {
            Ok(ast) => {
                println!("{:#?}", ast);
                SymbolTableGenerator::default().process(&ast);
                trace.printstd();
            }
            Err(err) => println!("{}", err),
        };
    }
}
