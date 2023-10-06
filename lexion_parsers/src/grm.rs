use indextree::{Arena, NodeId};
use lexion_lib::grammar::{Derivation, DerivationNode, Grammar, GrammarRule};
use lexion_lib::parsers::{GrammarParserLR, GrammarParserSLR1};
use lexion_derive::impl_parser_from_json;

impl_parser_from_json!(ParserGRM, "grammars/grm.json");