mod common;

fn grammar_conflicts(
    grammar: &lexion_lib::grammar::Grammar,
    table: &lexion_lib::parsers::ParseTableLR,
) -> Vec<String> {
    use lexion_lib::parsers::ParseTableAction;

    let mut conflicts: Vec<String> = table
        .entries()
        .filter_map(|(state, symbol, action)| {
            if let ParseTableAction::Conflict(actions) = action {
                let descriptions: Vec<String> = actions
                    .iter()
                    .map(|a| match a {
                        ParseTableAction::Shift(s) => format!("shift(s{s})"),
                        ParseTableAction::Reduce(r) => {
                            let rule = grammar.get_rule(*r);
                            format!("reduce({} -> {})", rule.left, rule.right.join(" "))
                        }
                        _ => a.to_string(),
                    })
                    .collect();
                Some(format!(
                    "state {state}, on '{symbol}': {}",
                    descriptions.join(" | ")
                ))
            } else {
                None
            }
        })
        .collect();
    conflicts.sort();
    conflicts
}

#[test]
fn test_variables() {
    assert!(common::compile("variables.lex").is_ok());
}

#[test]
fn test_functions() {
    assert!(common::compile("functions.lex").is_ok());
}

#[test]
fn test_control_flow() {
    assert!(common::compile("control_flow.lex").is_ok());
}

#[test]
fn test_expression_forms() {
    assert!(common::compile("expression_forms.lex").is_ok());
}

#[test]
fn test_cast_expression_parses() {
    use lexion_lang::parser::ParserLexion;
    use lexion_lib::Parser;
    use std::sync::Arc;

    let mut parser = ParserLexion::new();
    assert!(parser
        .parse_from_string(Arc::new("fn main() -> i32 { return -1 as i32; }".into()))
        .is_ok());
}

#[test]
fn test_if_else_expr_statement_semicolon_optional() {
    use lexion_lang::parser::ParserLexion;
    use lexion_lib::Parser;
    use std::sync::Arc;

    let mut parser = ParserLexion::new();
    assert!(parser
        .parse_from_string(Arc::new(
            "fn main() {
                if true { let y = 1; } else { let y = 0; }
                let z = { if true { 1 } else { 0 } };
            }"
            .into()
        ))
        .is_ok());
}

#[test]
fn test_keyword_identifier_terminal_precedence() {
    use lexion_lang::parser::ParserLexion;
    use lexion_lib::tokenizer::Tokenizer;
    use lexion_lib::Parser;
    use std::sync::Arc;

    fn first_token(source: &str) -> String {
        let mut tokenizer =
            Tokenizer::from_string(Arc::new(source.into()), ParserLexion::token_types());
        tokenizer.next_token().unwrap().token
    }

    assert_eq!(first_token("let"), "'let'");
    assert_eq!(first_token("letdown"), "'ident'");
    assert_eq!(first_token("true"), "'bool_literal'");
    assert_eq!(first_token("true_value"), "'ident'");
    assert_eq!(first_token("as"), "'as'");
    assert_eq!(first_token("assert"), "'ident'");
    assert_eq!(first_token("->"), "'->'");
    assert_eq!(first_token("=="), "'eq_op'");
    assert_eq!(first_token(">="), "'rel_op'");
    assert_eq!(first_token(">>"), "'shift_op'");
    assert_eq!(first_token(", ..."), "'vararg_literal'");
}

#[test]
fn test_structs() {
    assert!(common::compile("structs.lex").is_ok());
}

#[test]
fn print_grammar_conflicts() {
    use lexion_lang::parser::ParserLexion;
    use lexion_lib::parsers::GrammarParserLR;

    let grammar = &ParserLexion::GRAMMAR;
    let table = ParserLexion::PARSER.get_parse_table();

    let conflicts = grammar_conflicts(grammar, table);
    println!("\n=== CONFLICTS ({}) ===", conflicts.len());
    for c in &conflicts {
        println!("{c}");
    }
    assert!(
        conflicts.is_empty(),
        "{} conflict(s) found",
        conflicts.len()
    );
}

#[test]
fn print_raw_grammar_conflicts() {
    use lexion_lang::parser::ParserLexion;
    use lexion_lib::parsers::{GrammarParserLALR1, GrammarParserLR};

    let grammar = &ParserLexion::GRAMMAR;
    let parser = GrammarParserLALR1::from_grammar(grammar);
    let conflicts = grammar_conflicts(grammar, parser.get_parse_table());

    println!("\n=== RAW CONFLICTS ({}) ===", conflicts.len());
    for c in &conflicts {
        println!("{c}");
    }
    assert!(
        conflicts.is_empty(),
        "{} raw conflict(s) found",
        conflicts.len()
    );
}
