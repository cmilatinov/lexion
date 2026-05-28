use crate::grm::ParserGRM;
use lexion_lib::grammar::serialize::{GrammarData, RuleData};
use lexion_lib::grammar::{Grammar, GrammarRule};
use lexion_lib::parsers::{GrammarParserLR, GrammarParserSLR1};
use lexion_lib::tokenizer::tokens::EPSILON;
use lexion_lib::Parser;
use std::path::Path;
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

#[test]
fn lexion_json_matches_grammar_source() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let grm_path = manifest_dir.join("../lexion_lang/grammar/lexion.grm");
    let json_path = manifest_dir.join("../lexion_lang/grammar/lexion.json");

    let mut parser = ParserGRM::new();
    let mut generated = parser
        .parse_from_file_trace(grm_path.to_str().unwrap(), None)
        .expect("failed to parse lexion.grm");
    generated.rules = generated
        .rules
        .into_iter()
        .map(|rule| RuleData {
            left: rule.left,
            right: if rule.right.is_empty() {
                vec![EPSILON.into()]
            } else {
                rule.right
            },
            reduction: rule.reduction.map(|mut reduction| {
                reduction.code = reduction.code.replace("\r\n", "\n");
                reduction
            }),
        })
        .collect();

    let checked_in: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(json_path).expect("failed to read lexion.json"),
    )
    .expect("failed to parse lexion.json");
    let generated =
        serde_json::to_value(generated).expect("failed to serialize generated grammar data");

    assert_eq!(
        generated, checked_in,
        "lexion_lang/grammar/lexion.json is out of sync with lexion.grm"
    );
}
