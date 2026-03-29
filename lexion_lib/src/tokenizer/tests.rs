use crate::tokenizer::tokens::*;
use crate::tokenizer::{TokenType, Tokenizer};
use regex::Regex;
use std::sync::Arc;

fn test_token_types() -> Vec<TokenType> {
    vec![
        TokenType {
            name: "num".into(),
            regex: Regex::new(r"^\d+").unwrap(),
        },
        TokenType {
            name: "+".into(),
            regex: Regex::new(r"^\+").unwrap(),
        },
        TokenType {
            name: "".into(),
            regex: Regex::new(r"^\s+").unwrap(),
        },
    ]
}

#[test]
fn test_tokenize_simple() {
    let input = Arc::new("123".to_string());
    let types = test_token_types();
    let mut tokenizer = Tokenizer::from_string(input, &types);

    let token = tokenizer.next_token().unwrap();
    assert_eq!(token.token, "num");
    assert_eq!(token.value, "123");
}

#[test]
fn test_tokenize_multiple() {
    let input = Arc::new("1 + 2".to_string());
    let types = test_token_types();
    let mut tokenizer = Tokenizer::from_string(input, &types);

    let t1 = tokenizer.next_token().unwrap();
    assert_eq!(t1.token, "num");
    assert_eq!(t1.value, "1");

    let t2 = tokenizer.next_token().unwrap();
    assert_eq!(t2.token, "+");

    let t3 = tokenizer.next_token().unwrap();
    assert_eq!(t3.token, "num");
    assert_eq!(t3.value, "2");
}

#[test]
fn test_skip_whitespace() {
    let input = Arc::new("1   2".to_string());
    let types = test_token_types();
    let mut tokenizer = Tokenizer::from_string(input, &types);

    let t1 = tokenizer.next_token().unwrap();
    assert_eq!(t1.value, "1");

    let t2 = tokenizer.next_token().unwrap();
    assert_eq!(t2.value, "2");
}

#[test]
fn test_eof_token() {
    let input = Arc::new("1".to_string());
    let types = test_token_types();
    let mut tokenizer = Tokenizer::from_string(input, &types);

    tokenizer.next_token().unwrap();
    let eof = tokenizer.next_token().unwrap();
    assert_eq!(eof.token, EOF);
}

#[test]
fn test_has_next() {
    let input = Arc::new("1".to_string());
    let types = test_token_types();
    let mut tokenizer = Tokenizer::from_string(input, &types);

    assert!(tokenizer.has_next());
    tokenizer.next_token().unwrap();
    assert!(!tokenizer.has_next());
}

#[test]
fn test_longest_match() {
    let types = vec![
        TokenType {
            name: "if".into(),
            regex: Regex::new(r"if").unwrap(),
        },
        TokenType {
            name: "ident".into(),
            regex: Regex::new(r"[a-z]+").unwrap(),
        },
    ];
    let input = Arc::new("if".to_string());
    let mut tokenizer = Tokenizer::from_string(input, &types);

    let token = tokenizer.next_token().unwrap();
    assert_eq!(token.token, "if");
}

#[test]
fn test_invalid_token() {
    let input = Arc::new("@".to_string());
    let types = test_token_types();
    let mut tokenizer = Tokenizer::from_string(input, &types);

    let result = tokenizer.next_token();
    assert!(result.is_err());
}

#[test]
fn test_empty_input() {
    let input = Arc::new("".to_string());
    let types = test_token_types();
    let mut tokenizer = Tokenizer::from_string(input, &types);

    let token = tokenizer.next_token().unwrap();
    assert_eq!(token.token, EOF);
}
