use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;

use crate::lib::tokenizer::*;
use crate::lib::tokenizer::tokens::*;

type Result<T> = std::result::Result<T, ParseError>;

pub struct Tokenizer {
    file: PathBuf,
    string: String,
    cursor: RefCell<usize>,
    token_types: Vec<TokenType>,
}

impl Tokenizer {
    pub fn from_string(input: &str, token_types: Vec<TokenType>) -> Tokenizer {
        Tokenizer {
            file: PathBuf::from("inline"),
            string: input.parse().unwrap(),
            cursor: RefCell::new(0),
            token_types,
        }
    }

    pub fn from_file(file: &str, token_types: Vec<TokenType>) -> Tokenizer {
        Tokenizer {
            file: PathBuf::from(file),
            string: fs::read_to_string(file).unwrap(),
            cursor: RefCell::new(0),
            token_types,
        }
    }
}

impl Tokenizer {
    pub fn has_next(&self) -> bool {
        self.get_cursor_pos() < self.string.len()
    }

    pub fn next(&self) -> Result<TokenInstance> {
        let loc = self.get_cursor_loc();
        if !self.has_next() {
            return Ok(TokenInstance {
                token: String::from(EOF),
                value: String::from(EOF),
                loc,
            });
        }

        let (s, t) = self.match_next()?;
        *self.cursor.borrow_mut() += s.len();
        if t.name.is_empty() {
            return self.next();
        }

        Ok(TokenInstance {
            token: t.name.clone(),
            value: String::from(s),
            loc,
        })
    }

    pub fn match_next(&self) -> Result<(&str, &TokenType)> {
        let substring = &self.string[self.get_cursor_pos()..];
        let mut longest_match: Option<(&str, &TokenType)> = None;
        for token in self.token_types.iter() {
            let regex_match = match token.regex.find(substring) {
                Some(m) => m.as_str(),
                None => continue
            };

            if longest_match.is_none() || regex_match.len() > longest_match.unwrap().0.len() {
                longest_match = Some((regex_match, &token));
            }
        }

        if longest_match.is_none() {
            let loc = self.get_cursor_loc();
            let unexpected = match UNEXPECTED.find(substring) {
                Some(m) => m.as_str(),
                None => ""
            };
            return Err(ParseError {
                loc,
                message: format!("unexpected token '{}'", unexpected),
            });
        }

        Ok(longest_match.unwrap())
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
        *self.cursor.borrow()
    }
}