use std::fs;
use std::io::Read;
use std::sync::Arc;

use miette::{NamedSource, SourceOffset};

use crate::error::SyntaxError;
use crate::tokenizer::tokens::*;
use crate::tokenizer::*;

type Result<T> = std::result::Result<T, SyntaxError>;

pub struct Tokenizer<'a> {
    file: &'a str,
    string: Arc<String>,
    cursor: usize,
    token_types: &'a [TokenType],
}

impl<'a> Tokenizer<'a> {
    pub fn from_reader<R: Read>(
        name: &'a str,
        mut reader: R,
        token_types: &'a [TokenType],
    ) -> std::io::Result<Tokenizer<'a>> {
        let mut string = String::new();
        reader.read_to_string(&mut string)?;
        Ok(Self {
            file: name,
            string: Arc::new(string),
            cursor: 0,
            token_types,
        })
    }

    pub fn from_string(input: Arc<String>, token_types: &[TokenType]) -> Tokenizer {
        Tokenizer {
            file: "inline",
            string: input,
            cursor: 0,
            token_types,
        }
    }

    pub fn from_file(
        file: &'a str,
        token_types: &'a [TokenType],
    ) -> std::io::Result<Tokenizer<'a>> {
        Self::from_reader(file, fs::File::open(file)?, token_types)
    }
}

impl<'a> Tokenizer<'a> {
    pub fn has_next(&self) -> bool {
        self.cursor < self.string.len()
    }

    pub fn next_token(&mut self) -> Result<TokenInstance> {
        let offset = self.cursor_offset();
        if !self.has_next() {
            return Ok(TokenInstance {
                token: String::from(EOF),
                value: String::from(EOF),
                span: offset.into(),
            });
        }

        let (s, i) = self.match_next()?;
        let token: &TokenType = &self.token_types[i];
        self.cursor += s.len();
        if token.name.is_empty() {
            return self.next_token();
        }

        Ok(TokenInstance {
            span: (offset, s.len()).into(),
            token: token.name.clone(),
            value: s,
        })
    }

    pub fn match_next(&self) -> Result<(String, usize)> {
        let substring = &self.string[self.cursor..];
        let mut longest_match: Option<(&str, usize)> = None;
        for (i, token) in self.token_types.iter().enumerate() {
            let regex_match = match token.regex.find(substring) {
                Some(m) => m.as_str(),
                None => continue,
            };

            if longest_match.is_none() || regex_match.len() > longest_match.unwrap().0.len() {
                longest_match = Some((regex_match, i));
            }
        }

        if longest_match.is_none() {
            let offset = self.cursor_offset();
            let unexpected = match UNEXPECTED.find(substring) {
                Some(m) => m.as_str(),
                None => "",
            };
            return Err(SyntaxError {
                src: self.source(),
                span: (offset, unexpected.len()).into(),
                message: format!("unexpected token '{unexpected}'"),
            });
        }

        let (s, i) = longest_match.unwrap();
        Ok((String::from(s), i))
    }

    pub fn cursor_offset(&self) -> SourceOffset {
        self.cursor.into()
    }

    pub fn source(&self) -> NamedSource<Arc<String>> {
        NamedSource::new(self.file, self.string.clone())
    }
}
