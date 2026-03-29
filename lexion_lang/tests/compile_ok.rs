mod common;

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
fn test_structs() {
    assert!(common::compile("structs.lex").is_ok());
}

#[test]
fn print_grammar_conflicts() {
    use lexion_lang::parser::ParserLexion;
    use lexion_lib::parsers::{GrammarParserLR, ParseTableAction};

    let grammar = &ParserLexion::GRAMMAR;
    let table = ParserLexion::PARSER.get_parse_table();

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
                Some(format!("state {state}, on '{symbol}': {}", descriptions.join(" | ")))
            } else {
                None
            }
        })
        .collect();

    conflicts.sort();
    println!("\n=== CONFLICTS ({}) ===", conflicts.len());
    for c in &conflicts {
        println!("{c}");
    }
    assert!(conflicts.is_empty(), "{} conflict(s) found", conflicts.len());
}
