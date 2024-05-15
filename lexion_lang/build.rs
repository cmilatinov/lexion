use std::io::BufWriter;
use lexion_parsers::grm::ParserGRM;
use lexion_lib::grammar::serialize::{Grammar, Rule};
use lexion_lib::tokenizer::tokens::EPSILON;

fn main() {
    let parser = ParserGRM::new();
    let rules = parser.parse_from_file("./grammar/lexion.grm").unwrap().into_iter().map(|r| {
        Rule {
            left: r.left,
            right: if r.right.is_empty() { vec![String::from(EPSILON)] } else { r.right },
            reduction: r.reduction,
        }
    }).collect::<Vec<_>>();
    let grammar = Grammar { definitions: "".into(), rules };
    let result = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("./grammar/lexion.json");
    match result {
        Ok(file) => {
            let writer = BufWriter::new(file);
            serde_json::to_writer_pretty(writer, &grammar).unwrap();
        },
        Err(err) => {
            println!("cargo:warning={}", err);
        }
    }
    println!("cargo:rerun-if-changed=grammar/lexion.grm");
}
