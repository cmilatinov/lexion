use crate::ast::*;
use lexion_derive::impl_parser_from_json;

impl_parser_from_json!(ParserLexion, "lexion_lang/grammar/lexion.json");
