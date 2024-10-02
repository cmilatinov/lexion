#![allow(dead_code, special_module_name, unstable_name_collisions)]

use prettytable::{format, Table};
use std::time::Instant;

use crate::grammar::Grammar;
use crate::parsers::{GrammarParserLL1, GrammarParserLR, GrammarParserLR0, GrammarParserSLR1};

mod error;
mod grammar;
mod parsers;
mod tokenizer;

const a: &'static str = "";

fn main() {
    let grammar: Grammar = Grammar::from_json_file("grammars/grm.json").unwrap();
    println!("\n{}", grammar);
    println!("\n{}\n", grammar.to_jsmachine_string());

    let mut last = Instant::now();
    let _parser_ll1 = GrammarParserLL1::from_grammar(&grammar);
    println!(
        "[LL(1)] Parser generated ({}ms)",
        last.elapsed().as_millis()
    );

    last = Instant::now();
    let parser_lr0 = GrammarParserLR0::from_grammar(&grammar);
    println!(
        "[LR(0)] Parser generated ({}ms)",
        last.elapsed().as_millis()
    );

    last = Instant::now();
    let parser_slr1 = GrammarParserSLR1::from_grammar(&grammar);
    println!(
        "[SLR(1)] Parser generated ({}ms)",
        last.elapsed().as_millis()
    );

    // last = Instant::now();
    // let parser_lalr1 = GrammarParserLALR1::from_grammar(&grammar);
    // println!("[LALR(1)] Parser generated ({}ms)", last.elapsed().as_millis());

    let mut pt = parser_slr1.get_parse_table().to_prettytable();
    pt.set_format(*format::consts::FORMAT_BOX_CHARS);
    pt.printstd();

    let mut trace = Table::new();
    trace.set_format(*format::consts::FORMAT_BOX_CHARS);
    let mut res =
        parser_lr0.parse_from_string_trace(&grammar, "A -> 'a'; B -> 'b';", Some(&mut trace));
    if let Err(e) = res {
        println!("{}", e);
    } else if let Ok(d) = res {
        println!("{d}");
    }
    trace.printstd();
}
