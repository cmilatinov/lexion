#![allow(dead_code, special_module_name)]

use colored::Colorize;

use lib::tokenizer::tokens::*;
use lib::tokenizer::*;
use crate::lib::grammar::{Grammar, GrammarRule};
use crate::lib::parsers::GrammarParserLL1;

mod lib;

fn main() {
    let tokenizer = Tokenizer::from_file(
        "./tests/source.txt",
        vec![
            TokenType { name: String::from(""), regex: WHITESPACE.clone() },
            TokenType { name: String::from(""), regex: SINGLE_LINE_COMMENT.clone() },
            TokenType { name: String::from(""), regex: MULTI_LINE_COMMENT.clone() },
            TokenType { name: String::from("int"), regex: INTEGER.clone() },
            TokenType { name: String::from("float"), regex: FLOAT.clone() },
            TokenType { name: String::from("bool"), regex: BOOLEAN.clone() },
            TokenType { name: String::from("string"), regex: SINGLE_QUOTED_STRING.clone() },
            TokenType { name: String::from("string"), regex: DOUBLE_QUOTED_STRING.clone() },
        ],
    );
    while tokenizer.has_next() {
        match tokenizer.next() {
            Ok(token) => {
                println!("{}", token);
            }
            Err(e) => {
                eprintln!("{}", e.to_string().red());
                break;
            }
        }
    }

    let grammar: Grammar = Grammar::from_rules(vec![
        GrammarRule {
            left: String::from("S"),
            right: vec![
                String::from("A"),
                String::from("A")
            ]
        },
        GrammarRule {
            left: String::from("A"),
            right: vec![
                String::from("'a'"),
                String::from("A")
            ]
        },
        GrammarRule {
            left: String::from("A"),
            right: vec![
                String::from("'b'")
            ]
        },
        GrammarRule {
            left: String::from("'b'"),
            right: vec![
                String::from("abcasldk")
            ]
        }
    ]);
    println!("\n{}", grammar);

    let parser = GrammarParserLL1::from_grammar(&grammar);
    println!("LL(1) - {}", parser.is_ll1());
}
