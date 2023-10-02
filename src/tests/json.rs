use compiler_derive::impl_parser_json;
use crate as compiler;

#[test]
pub fn test_parser_json() {
    let a = impl_parser_json!("grammars/grm.json");
}