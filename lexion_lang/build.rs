use lexion_lib::grammar::serialize::RuleData;
use lexion_lib::tabled::builder::Builder;
use lexion_lib::tokenizer::tokens::EPSILON;
use lexion_lib::Parser;
use lexion_parsers::grm::ParserGRM;
use std::io::BufWriter;

fn main() {
    let mut parser = ParserGRM::new();
    let mut trace = Builder::new();
    let data = parser.parse_from_file_trace("./grammar/lexion.grm", Some(&mut trace));
    if let Err(err) = data {
        let table = trace.build();
        panic!("{err:?}\n{table}");
    }
    let mut data = data.unwrap();
    data.rules = data
        .rules
        .into_iter()
        .map(|r| RuleData {
            left: r.left,
            right: if r.right.is_empty() {
                vec![String::from(EPSILON)]
            } else {
                r.right
            },
            reduction: r.reduction,
        })
        .collect::<Vec<_>>();
    let result = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("./grammar/lexion.json");
    match result {
        Ok(file) => {
            let writer = BufWriter::new(file);
            serde_json::to_writer_pretty(writer, &data).unwrap();
        }
        Err(err) => {
            println!("cargo:warning={err}");
        }
    }
    println!("cargo:rerun-if-changed=grammar/lexion.grm");
}
