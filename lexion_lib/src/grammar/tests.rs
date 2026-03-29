use crate::grammar::{Grammar, GrammarRule};
use crate::tokenizer::tokens::*;
use std::collections::HashSet;

/// E -> E '+' T
/// E-> T
/// T -> 'num'
fn simple_grammar() -> Grammar {
    Grammar::from_rules(vec![
        GrammarRule {
            left: "E".into(),
            right: vec!["E".into(), "'+'".into(), "T".into()],
        },
        GrammarRule {
            left: "E".into(),
            right: vec!["T".into()],
        },
        GrammarRule {
            left: "T".into(),
            right: vec!["'num'".into()],
        },
    ])
}

/// S -> A B
/// A -> 'a'
/// A -> ε
/// B -> 'b'
fn epsilon_grammar() -> Grammar {
    Grammar::from_rules(vec![
        GrammarRule {
            left: "S".into(),
            right: vec!["A".into(), "B".into()],
        },
        GrammarRule {
            left: "A".into(),
            right: vec!["'a'".into()],
        },
        GrammarRule {
            left: "A".into(),
            right: vec![EPSILON.into()],
        },
        GrammarRule {
            left: "B".into(),
            right: vec!["'b'".into()],
        },
    ])
}

#[test]
fn test_is_terminal() {
    assert!(Grammar::is_terminal("'+'"));
    assert!(Grammar::is_terminal("'num'"));
    assert!(Grammar::is_terminal(EOF));
    assert!(Grammar::is_terminal(EPSILON));
    assert!(!Grammar::is_terminal("E"));
    assert!(!Grammar::is_terminal("Expr"));
}

#[test]
fn test_is_non_terminal() {
    assert!(Grammar::is_non_terminal("E"));
    assert!(Grammar::is_non_terminal("Expr"));
    assert!(!Grammar::is_non_terminal("'+'"));
    assert!(!Grammar::is_non_terminal(EOF));
}

#[test]
fn test_grammar_from_rules() {
    let grammar = simple_grammar();
    assert_eq!(grammar.get_rules().len(), 4);
    assert_eq!(
        grammar.get_terminals(),
        &HashSet::from_iter([EOF.into(), "'+'".into(), "'num'".into()])
    );
}

#[test]
fn test_first_set_simple() {
    let grammar = simple_grammar();
    let first_e = grammar.first_of("E").unwrap();
    assert_eq!(first_e, &HashSet::from_iter(["'num'".into()]));
}

#[test]
fn test_first_set_with_epsilon() {
    let grammar = epsilon_grammar();
    let first_s = grammar.first_of("S").unwrap();
    assert_eq!(first_s, &HashSet::from_iter(["'a'".into(), "'b'".into()]));
}

#[test]
fn test_nullable_detection() {
    let grammar = epsilon_grammar();
    assert!(grammar.is_nullable("A"));
    assert!(!grammar.is_nullable("B"));
    assert!(!grammar.is_nullable("S"));
}

#[test]
fn test_nullable_sequence() {
    let grammar = epsilon_grammar();
    assert!(grammar.is_nullable_sequence(&[EPSILON.into()]));
    assert!(grammar.is_nullable_sequence(&["A".into()]));
    assert!(!grammar.is_nullable_sequence(&["B".into()]));
    assert!(!grammar.is_nullable_sequence(&["A".into(), "B".into()]));
}

#[test]
fn test_follow_set() {
    let grammar = simple_grammar();
    let follow_t = grammar.follow_of("T").unwrap();
    assert!(follow_t.contains("'+'") || follow_t.contains(EOF));
}

#[test]
fn test_get_rule() {
    let grammar = simple_grammar();
    let rule = grammar.get_rule(1);
    assert_eq!(rule.left, "E");
}

#[test]
fn test_start_symbol() {
    let grammar = simple_grammar();
    let start = grammar.get_start_symbol();
    assert_eq!(start, "E");
}
