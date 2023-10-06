use std::fs;
use std::path::PathBuf;
use crate::error::SyntaxError;
use crate::tokenizer::*;
use crate::tokenizer::tokens::*;

type Result<T> = std::result::Result<T, SyntaxError>;

pub struct Tokenizer {
    file: PathBuf,
    string: String,
    cursor: usize,
    token_types: Vec<TokenType>,
}

impl Tokenizer {
    pub fn from_string(input: &str, token_types: Vec<TokenType>) -> Tokenizer {
        Tokenizer {
            file: PathBuf::from("inline"),
            string: input.parse().unwrap(),
            cursor: 0,
            token_types,
        }
    }

    pub fn from_file(file: &str, token_types: Vec<TokenType>) -> Tokenizer {
        Tokenizer {
            file: PathBuf::from(file),
            string: fs::read_to_string(file).unwrap(),
            cursor: 0,
            token_types,
        }
    }
}

impl Tokenizer {
    pub fn has_next(&self) -> bool {
        self.get_cursor_pos() < self.string.len()
    }

    pub fn next(&mut self) -> Result<TokenInstance> {
        let loc = self.get_cursor_loc();
        if !self.has_next() {
            return Ok(TokenInstance {
                token: String::from(EOF),
                value: String::from(EOF),
                loc,
            });
        }

        let (s, i) = self.match_next()?;
        let token: &TokenType = &self.token_types[i];
        self.cursor += s.len();
        if token.name.is_empty() {
            return self.next();
        }

        Ok(TokenInstance {
            token: token.name.clone(),
            value: s,
            loc,
        })
    }

    pub fn match_next(&self) -> Result<(String, usize)> {
        let substring = &self.string[self.cursor..];
        let mut longest_match: Option<(&str, usize)> = None;
        for (i, token) in self.token_types.iter().enumerate() {
            let regex_match = match token.regex.find(substring) {
                Some(m) => m.as_str(),
                None => continue
            };

            if longest_match.is_none() || regex_match.len() > longest_match.unwrap().0.len() {
                longest_match = Some((regex_match, i));
            }
        }

        if longest_match.is_none() {
            let loc = self.get_cursor_loc();
            let unexpected = match UNEXPECTED.find(substring) {
                Some(m) => m.as_str(),
                None => ""
            };
            return Err(SyntaxError {
                loc,
                message: format!("unexpected token '{}'", unexpected),
            });
        }

        let (s, i) = longest_match.unwrap();
        Ok((String::from(s), i))
    }

    pub fn get_cursor_loc(&self) -> SourceLocation {
        let mut line = 1;
        let mut last_line = 0;
        for (i, c) in &mut self.string[0..self.get_cursor_pos()].as_bytes().iter().enumerate() {
            if *c == b'\n' {
                line += 1;
                last_line = if i == 0 { 0 } else { i + 1 };
            }
        }
        let file = match fs::canonicalize(&self.file) {
            Ok(p) => String::from(p.to_str().unwrap()),
            Err(_) => String::from(self.file.to_str().unwrap())
        };
        SourceLocation {
            file,
            line,
            col: (self.get_cursor_pos() - last_line + 1) as i32,
        }
    }

    pub fn get_cursor_pos(&self) -> usize {
        self.cursor
    }
}