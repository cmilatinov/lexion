use crate::grm::ParserGRM;

#[test]
pub fn test_grm_parser() {
    let parser = ParserGRM::new();
    let res = parser.parse_from_string("A -> 'a' | 'd' {{test}}; B -> 'b'; C -> 'c';");
    println!("Test");
}