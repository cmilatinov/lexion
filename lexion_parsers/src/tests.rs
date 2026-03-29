use crate::grm::ParserGRM;
use lexion_lib::grammar::serialize::GrammarData;
use lexion_lib::grammar::{Grammar, GrammarRule};
use lexion_lib::parsers::{GrammarParserLR, GrammarParserSLR1};
use lexion_lib::tokenizer::tokens::EPSILON;
use lexion_lib::Parser;
use std::sync::Arc;
use tabled::builder::Builder;
use tabled::settings::Style;

#[cfg(test)]
#[test]
pub fn test_grm_parser() {
    let mut parser = ParserGRM::new();
    let str = include_str!("../../grammars/expression.grm");
    let src = Arc::new(str.into());
    let GrammarData { rules, .. } = parser.parse_from_string(src).unwrap();
    let rules = rules
        .into_iter()
        .map(|r| GrammarRule {
            left: r.left.clone(),
            right: if r.right.is_empty() {
                vec![EPSILON.into()]
            } else {
                r.right
            },
        })
        .collect::<Vec<_>>();
    let grammar = Grammar::from_rules(rules);
    let parser = GrammarParserSLR1::from_grammar(&grammar);

    let mut builder = Builder::new();
    let result = parser.parse_from_string_trace(
        &grammar,
        Arc::new("a = a + abc(a,b,c)".into()),
        Some(&mut builder),
    );
    match result {
        Ok(derivation) => {
            println!("{derivation}");
        }
        Err(err) => {
            println!("{err}");
        }
    }
    let mut table = builder.build();
    table.with(Style::modern());
    println!("{table}");
    println!("{}", grammar.to_jsmachine_string());
}
