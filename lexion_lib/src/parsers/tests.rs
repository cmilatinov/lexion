use crate::grammar::{Derivation, Grammar, GrammarRule};
use crate::parsers::items::LR0Item;
use crate::parsers::{
    GrammarParserLALR1, GrammarParserLR, GrammarParserSLR1, ParseTableAction, ParseTableOverride,
};
use crate::tokenizer::tokens::*;
use petgraph::visit::Dfs;
use std::collections::HashSet;
use std::sync::Arc;
use tabled::builder::Builder;
use tabled::settings::{Alignment, Style};

fn leaf_values(derivation: &Derivation) -> Vec<String> {
    let mut dfs = Dfs::new(&derivation.graph, derivation.root);
    let mut leaves = vec![];
    while let Some(n) = dfs.next(&derivation.graph) {
        if derivation.graph.neighbors(n).count() == 0 {
            leaves.push(derivation.graph.node_weight(n).unwrap().token.value.clone());
        }
    }
    leaves
}

fn root_symbol<'a>(derivation: &'a Derivation, grammar: &'a Grammar) -> &'a str {
    &derivation
        .graph
        .node_weight(derivation.root)
        .unwrap()
        .get_rule(grammar)
        .left
}

/// E -> E '+' T
/// E -> T
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
        GrammarRule {
            left: "'num'".into(),
            right: vec![r"\d+".into()],
        },
    ])
}

/// S -> 'a'
fn trivial_grammar() -> Grammar {
    Grammar::from_rules(vec![GrammarRule {
        left: "S".into(),
        right: vec!["'a'".into()],
    }])
}

/// Program -> List
/// List -> List Item
/// List -> epsilon
/// Item -> 'a'
fn nullable_list_grammar() -> Grammar {
    Grammar::from_rules(vec![
        GrammarRule {
            left: "Program".into(),
            right: vec!["List".into()],
        },
        GrammarRule {
            left: "List".into(),
            right: vec!["List".into(), "Item".into()],
        },
        GrammarRule {
            left: "List".into(),
            right: vec![EPSILON.into()],
        },
        GrammarRule {
            left: "Item".into(),
            right: vec!["'a'".into()],
        },
    ])
}

/// LR(1), but not SLR(1).
fn lr1_not_slr_grammar() -> Grammar {
    Grammar::from_rules(vec![
        GrammarRule {
            left: "S".into(),
            right: vec!["L".into(), "'='".into(), "R".into()],
        },
        GrammarRule {
            left: "S".into(),
            right: vec!["R".into()],
        },
        GrammarRule {
            left: "L".into(),
            right: vec!["'*'".into(), "R".into()],
        },
        GrammarRule {
            left: "L".into(),
            right: vec!["'id'".into()],
        },
        GrammarRule {
            left: "R".into(),
            right: vec!["L".into()],
        },
    ])
}

/// LR(1), but not LALR(1).
fn lr1_not_lalr_grammar() -> Grammar {
    Grammar::from_rules(vec![
        GrammarRule {
            left: "S".into(),
            right: vec!["'a'".into(), "A".into(), "'d'".into()],
        },
        GrammarRule {
            left: "S".into(),
            right: vec!["'b'".into(), "A".into(), "'e'".into()],
        },
        GrammarRule {
            left: "S".into(),
            right: vec!["'a'".into(), "B".into(), "'e'".into()],
        },
        GrammarRule {
            left: "S".into(),
            right: vec!["'b'".into(), "B".into(), "'d'".into()],
        },
        GrammarRule {
            left: "A".into(),
            right: vec!["'c'".into()],
        },
        GrammarRule {
            left: "B".into(),
            right: vec!["'c'".into()],
        },
    ])
}

// --- Table test helpers ---

/// Canonicalize state numbers by BFS through shift/goto edges from state 0.
/// Returns a mapping old_state -> new_state.
fn canonicalize_states(
    table: &crate::parsers::ParseTableLR,
) -> std::collections::HashMap<usize, usize> {
    let mut mapping = std::collections::HashMap::new();
    let mut queue = std::collections::VecDeque::from([0usize]);
    let mut counter = 0usize;
    while let Some(state) = queue.pop_front() {
        if mapping.contains_key(&state) {
            continue;
        }
        mapping.insert(state, counter);
        counter += 1;
        // Follow shift/goto edges in deterministic (sorted) order
        let mut nexts: Vec<usize> = table
            .entries()
            .filter_map(|(s, _, a)| {
                if s != state {
                    return None;
                }
                match a {
                    ParseTableAction::Shift(n) | ParseTableAction::Goto(n) => Some(*n),
                    _ => None,
                }
            })
            .collect();
        nexts.sort();
        nexts.dedup();
        queue.extend(nexts);
    }
    mapping
}

/// Serialize a parse table to a canonical, deterministic string for snapshot comparison.
fn table_canonical_string(table: &crate::parsers::ParseTableLR) -> String {
    let mapping = canonicalize_states(table);
    let remap = |s: usize| mapping.get(&s).copied().unwrap_or(s);
    let remap_action = |a: &ParseTableAction| -> String {
        match a {
            ParseTableAction::Shift(n) => format!("s{}", remap(*n)),
            ParseTableAction::Goto(n) => format!("g{}", remap(*n)),
            ParseTableAction::Reduce(r) => format!("r{r}"),
            ParseTableAction::Accept => "acc".into(),
            ParseTableAction::Reject => "rej".into(),
            ParseTableAction::Conflict(_) => "conflict".into(),
        }
    };
    let mut entries: Vec<String> = table
        .entries()
        .map(|(state, symbol, action)| {
            format!("({},{},{})", remap(state), symbol, remap_action(action))
        })
        .collect();
    entries.sort();
    entries.join("\n")
}

fn has_conflicts(table: &crate::parsers::ParseTableLR) -> bool {
    table
        .entries()
        .any(|(_, _, a)| matches!(a, ParseTableAction::Conflict(_)))
}

fn conflict_actions(
    table: &crate::parsers::ParseTableLR,
) -> Vec<(usize, String, Vec<ParseTableAction>)> {
    table
        .entries()
        .filter_map(|(state, symbol, action)| {
            if let ParseTableAction::Conflict(actions) = action {
                Some((state, symbol.to_string(), actions.clone()))
            } else {
                None
            }
        })
        .collect()
}

// --- ParseTableAction ---

#[test]
fn test_action_display() {
    assert_eq!(ParseTableAction::Shift(3).to_string(), "s3");
    assert_eq!(ParseTableAction::Reduce(1).to_string(), "r1");
    assert_eq!(ParseTableAction::Goto(2).to_string(), "2");
    assert_eq!(ParseTableAction::Accept.to_string(), "acc");
    assert_eq!(ParseTableAction::Reject.to_string(), "");
}

#[test]
fn test_action_is_shift_reduce() {
    assert!(ParseTableAction::Shift(0).is_shift());
    assert!(!ParseTableAction::Reduce(0).is_shift());
    assert!(ParseTableAction::Reduce(0).is_reduce());
    assert!(!ParseTableAction::Shift(0).is_reduce());
}

#[test]
fn test_action_from_str() {
    use std::str::FromStr;
    assert!(matches!(
        ParseTableAction::from_str("s2").unwrap(),
        ParseTableAction::Shift(2)
    ));
    assert!(matches!(
        ParseTableAction::from_str("r1").unwrap(),
        ParseTableAction::Reduce(1)
    ));
    assert!(matches!(
        ParseTableAction::from_str("acc").unwrap(),
        ParseTableAction::Accept
    ));
    assert!(matches!(
        ParseTableAction::from_str("3").unwrap(),
        ParseTableAction::Goto(3)
    ));
    assert!(ParseTableAction::from_str("???").is_err());
}

// --- SLR(1) ---

#[test]
fn test_slr1_builds_from_grammar() {
    let grammar = simple_grammar();
    let parser = GrammarParserSLR1::from_grammar(&grammar);
    // Accept action must exist for EOF in some state
    let table = parser.get_parse_table();
    let has_accept =
        (0..20).any(|s| matches!(table.get_action(s, EOF).as_ref(), ParseTableAction::Accept));
    assert!(has_accept);
}

#[test]
fn test_slr1_parses_valid_input() {
    let grammar = simple_grammar();
    let parser = GrammarParserSLR1::from_grammar(&grammar);
    let derivation = parser
        .parse_from_string(&grammar, Arc::new("1".into()))
        .unwrap();
    assert_eq!(root_symbol(&derivation, &grammar), "E");
    assert_eq!(leaf_values(&derivation), vec!["1"]);
}

#[test]
fn test_slr1_parses_addition() {
    let grammar = simple_grammar();
    let parser = GrammarParserSLR1::from_grammar(&grammar);
    let derivation = parser
        .parse_from_string(&grammar, Arc::new("1 + 2".into()))
        .unwrap();
    assert_eq!(root_symbol(&derivation, &grammar), "E");
    assert_eq!(leaf_values(&derivation), vec!["1", "+", "2"]);
}

#[test]
fn test_slr1_rejects_invalid_input() {
    let grammar = simple_grammar();
    let parser = GrammarParserSLR1::from_grammar(&grammar);
    let result = parser.parse_from_string(&grammar, Arc::new("+".into()));
    assert!(result.is_err());
}

#[test]
fn test_slr1_rejects_empty_input() {
    let grammar = simple_grammar();
    let parser = GrammarParserSLR1::from_grammar(&grammar);
    let result = parser.parse_from_string(&grammar, Arc::new("".into()));
    assert!(result.is_err());
}

// --- LALR(1) ---

#[test]
fn test_lalr1_builds_from_grammar() {
    let grammar = simple_grammar();
    let parser = GrammarParserLALR1::from_grammar(&grammar);
    let table = parser.get_parse_table();
    let has_accept =
        (0..20).any(|s| matches!(table.get_action(s, EOF).as_ref(), ParseTableAction::Accept));
    assert!(has_accept);
}

#[test]
fn test_lalr1_parses_valid_input() {
    let grammar = simple_grammar();
    let parser = GrammarParserLALR1::from_grammar(&grammar);
    let derivation = parser
        .parse_from_string(&grammar, Arc::new("1".into()))
        .unwrap();
    assert_eq!(root_symbol(&derivation, &grammar), "E");
    assert_eq!(leaf_values(&derivation), vec!["1"]);
}

#[test]
fn test_lalr1_parses_addition() {
    let grammar = simple_grammar();
    let parser = GrammarParserLALR1::from_grammar(&grammar);
    let derivation = parser
        .parse_from_string(&grammar, Arc::new("1 + 2".into()))
        .unwrap();
    assert_eq!(root_symbol(&derivation, &grammar), "E");
    assert_eq!(leaf_values(&derivation), vec!["1", "+", "2"]);
}

#[test]
fn test_lalr1_parses_chained_addition() {
    let grammar = simple_grammar();
    let parser = GrammarParserLALR1::from_grammar(&grammar);
    let derivation = parser
        .parse_from_string(&grammar, Arc::new("1 + 2 + 3".into()))
        .unwrap();
    assert_eq!(root_symbol(&derivation, &grammar), "E");
    assert_eq!(leaf_values(&derivation), vec!["1", "+", "2", "+", "3"]);
}

#[test]
fn test_lalr1_rejects_invalid_input() {
    let grammar = simple_grammar();
    let parser = GrammarParserLALR1::from_grammar(&grammar);
    let result = parser.parse_from_string(&grammar, Arc::new("+".into()));
    assert!(result.is_err());
}

#[test]
fn test_lalr1_rejects_trailing_operator() {
    let grammar = simple_grammar();
    let parser = GrammarParserLALR1::from_grammar(&grammar);
    let result = parser.parse_from_string(&grammar, Arc::new("1 +".into()));
    assert!(result.is_err());
}

#[test]
fn test_lalr1_trivial_grammar() {
    let grammar = trivial_grammar();
    let parser = GrammarParserLALR1::from_grammar(&grammar);
    let derivation = parser
        .parse_from_string(&grammar, Arc::new("a".into()))
        .unwrap();
    assert_eq!(root_symbol(&derivation, &grammar), "S");
    assert_eq!(leaf_values(&derivation), vec!["a"]);
    assert!(parser
        .parse_from_string(&grammar, Arc::new("b".into()))
        .is_err());
}

#[test]
fn test_lalr1_lookaheads_include_epsilon_reductions() {
    let grammar = nullable_list_grammar();
    let parser = GrammarParserLALR1::from_grammar(&grammar);
    let epsilon_list_item = LR0Item::new(3, 0);
    let lookaheads: HashSet<String> = parser
        .lookahead_sets()
        .iter()
        .filter(|((_, item), _)| *item == epsilon_list_item)
        .flat_map(|(_, lookahead)| lookahead.iter().cloned())
        .collect();

    assert!(lookaheads.contains(EOF));
    assert!(lookaheads.contains("'a'"));
}

#[test]
fn test_lalr1_parses_nullable_list_grammar() {
    let grammar = nullable_list_grammar();
    let parser = GrammarParserLALR1::from_grammar(&grammar);

    let empty = parser
        .parse_from_string(&grammar, Arc::new(String::new()))
        .unwrap();
    assert_eq!(root_symbol(&empty, &grammar), "Program");

    let items = parser
        .parse_from_string(&grammar, Arc::new("a a".into()))
        .unwrap();
    assert_eq!(root_symbol(&items, &grammar), "Program");
    assert_eq!(
        leaf_values(&items)
            .into_iter()
            .filter(|value| value == "a")
            .collect::<Vec<_>>(),
        vec!["a", "a"]
    );
}

#[test]
fn test_lalr1_resolves_lr1_not_slr_lookaheads() {
    let grammar = lr1_not_slr_grammar();
    let slr = GrammarParserSLR1::from_grammar(&grammar);
    let lalr = GrammarParserLALR1::from_grammar(&grammar);

    assert!(has_conflicts(slr.get_parse_table()));
    assert!(!has_conflicts(lalr.get_parse_table()));
    assert!(lalr
        .parse_from_string(&grammar, Arc::new("id = id".into()))
        .is_ok());
}

#[test]
fn test_lalr1_reports_lr1_not_lalr_reduce_reduce_conflict() {
    let grammar = lr1_not_lalr_grammar();
    let parser = GrammarParserLALR1::from_grammar(&grammar);
    let conflicts = conflict_actions(parser.get_parse_table());

    assert!(conflicts.iter().any(|(_, _, actions)| {
        actions.len() >= 2
            && actions
                .iter()
                .all(|action| matches!(action, ParseTableAction::Reduce(_)))
    }));
}

// --- Derivation tree ---

#[test]
fn test_derivation_addition_tree_shape() {
    let grammar = simple_grammar();
    println!("{}", grammar.to_jsmachine_string());

    let parser = GrammarParserLALR1::from_grammar(&grammar);
    let table = parser.get_parse_table().to_table();
    println!("{table}");

    let mut builder = Builder::new();
    let derivation =
        parser.parse_from_string_trace(&grammar, Arc::new("1 + 2".into()), Some(&mut builder));
    let mut table = builder.build();
    table.with(Style::modern());
    table.with(Alignment::center());
    println!("{table}");

    let derivation = derivation.unwrap();
    println!("{derivation}");

    // Root is E with 3 children: E, '+', T
    assert_eq!(root_symbol(&derivation, &grammar), "E");
    assert_eq!(derivation.graph.neighbors(derivation.root).count(), 3);
    assert_eq!(leaf_values(&derivation), vec!["1", "+", "2"]);
}

// --- Table structure tests ---

#[test]
fn test_slr1_no_conflicts_simple_grammar() {
    let grammar = simple_grammar();
    let parser = GrammarParserSLR1::from_grammar(&grammar);
    assert!(!has_conflicts(parser.get_parse_table()));
}

#[test]
fn test_lalr1_no_conflicts_simple_grammar() {
    let grammar = simple_grammar();
    let parser = GrammarParserLALR1::from_grammar(&grammar);
    assert!(!has_conflicts(parser.get_parse_table()));
}

#[test]
fn test_slr1_lalr1_equivalent_simple_grammar() {
    let grammar = simple_grammar();
    let slr = GrammarParserSLR1::from_grammar(&grammar);
    let lalr = GrammarParserLALR1::from_grammar(&grammar);
    assert_eq!(
        table_canonical_string(slr.get_parse_table()),
        table_canonical_string(lalr.get_parse_table())
    );
}

#[test]
fn test_slr1_table_snapshot_simple_grammar() {
    let grammar = simple_grammar();
    let parser = GrammarParserSLR1::from_grammar(&grammar);
    insta::assert_snapshot!(table_canonical_string(parser.get_parse_table()));
}

#[test]
fn test_lalr1_table_snapshot_trivial_grammar() {
    let grammar = trivial_grammar();
    let parser = GrammarParserLALR1::from_grammar(&grammar);
    insta::assert_snapshot!(table_canonical_string(parser.get_parse_table()));
}

#[test]
fn test_conflict_resolution_requires_existing_conflict() {
    let grammar = lr1_not_slr_grammar();
    let mut parser = GrammarParserSLR1::from_grammar(&grammar);
    let (state, _, actions) = conflict_actions(parser.get_parse_table())
        .into_iter()
        .find(|(_, symbol, _)| symbol == "'='")
        .expect("expected SLR conflict on '='");

    let wrong_action = ParseTableOverride {
        state,
        symbol: "'='",
        action: ParseTableAction::Accept,
    };
    assert!(parser
        .get_parse_table_mut()
        .apply_conflict_resolutions([wrong_action].iter())
        .is_err());

    let override_action = ParseTableOverride {
        state,
        symbol: "'='",
        action: actions[0].clone(),
    };
    assert!(parser
        .get_parse_table_mut()
        .apply_conflict_resolutions([override_action].iter())
        .is_ok());
    assert!(!matches!(
        parser.get_parse_table().get_action(state, "'='").as_ref(),
        ParseTableAction::Conflict(_)
    ));

    let stale_override = ParseTableOverride {
        state,
        symbol: "'='",
        action: actions[0].clone(),
    };
    assert!(parser
        .get_parse_table_mut()
        .apply_conflict_resolutions([stale_override].iter())
        .is_err());
}
