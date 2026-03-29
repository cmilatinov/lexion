use crate::error::ParseError;
use crate::tokenizer::{TokenType, Tokenizer};
use std::fs::File;
use std::sync::Arc;
use tabled::builder::Builder;

pub use lexion_derive::Parser;

pub trait Parser {
    type Result;

    fn token_types() -> &'static [TokenType];

    fn parse_from_string(&mut self, source: Arc<String>) -> Result<Self::Result, ParseError> {
        self.parse_from_string_trace(source, None)
    }

    fn parse_from_string_trace(
        &mut self,
        source: Arc<String>,
        trace: Option<&mut Builder>,
    ) -> Result<Self::Result, ParseError> {
        self.parse_trace(Tokenizer::from_string(source, Self::token_types()), trace)
    }

    fn parse_from_file_trace(
        &mut self,
        path: &str,
        trace: Option<&mut Builder>,
    ) -> Result<Self::Result, ParseError> {
        let file = File::open(path)?;
        self.parse_trace(
            Tokenizer::from_reader(path, file, Self::token_types())?,
            trace,
        )
    }

    fn parse(&mut self, tokenizer: Tokenizer) -> Result<Self::Result, ParseError> {
        self.parse_trace(tokenizer, None)
    }

    fn parse_trace(
        &mut self,
        tokenizer: Tokenizer,
        trace: Option<&mut Builder>,
    ) -> Result<Self::Result, ParseError>;
}
