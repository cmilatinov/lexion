use lexion_lib::grammar::serialize::{Reduction, Rule};
use lexion_derive::impl_parser_from_json;

impl_parser_from_json!(ParserGRM, "grammars/grm.json");
