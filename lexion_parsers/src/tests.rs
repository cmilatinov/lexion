use lexion_lib::grammar::{Grammar, GrammarRule};
use lexion_lib::parsers::{GrammarParserLR, GrammarParserSLR1};
use prettytable::{format, Table};
use lexion_lib::tokenizer::tokens::EPSILON;
use crate::grm::ParserGRM;

#[cfg(test)]
#[test]
pub fn test_grm_parser() {
    let parser = ParserGRM::new();
    let str = include_str!("../../grammars/calyx.grm");
    let rules = parser.parse_from_string(str).unwrap();
    println!("{:?}", rules);
    let rules = rules.into_iter().map(|r| GrammarRule {
        left: r.left.clone(),
        right: if r.right.is_empty() {
            vec![EPSILON.into()]
        } else if Grammar::is_terminal(&r.left) {
            vec![String::from(&r.right[0][1..r.right[0].len() - 1])]
        } else { r.right }
    }).collect::<Vec<_>>();
    let grammar = Grammar::from_rules(rules);
    let parser = GrammarParserSLR1::from_grammar(&grammar);
    let mut table = Table::new();
    table.set_format(*format::consts::FORMAT_BOX_CHARS);
    parser.table.to_prettytable(&mut table);
    println!("{}", table);

    let mut table = Table::new();
    table.set_format(*format::consts::FORMAT_BOX_CHARS);
    let result = parser.parse_from_string_trace(&grammar, "extern fn test (  ) { }", Some(&mut table));
    match result {
        Ok(derivation) => {
            println!("{}", derivation.to_string());
        },
        Err(err) => {
            println!("{}", err);
        }
    }
    println!("{}", table);
    println!("{}", grammar.to_jsmachine_string());
}
