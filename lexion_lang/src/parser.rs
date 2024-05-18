use lexion_derive::impl_parser_from_json;
use lexion_lib::tokenizer::SourceRange;

use crate::ast::*;

impl_parser_from_json!(ParserLexion, "lexion_lang/grammar/lexion.json");
