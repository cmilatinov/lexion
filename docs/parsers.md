---
inclusion: auto
---

# Parser Knowledge Base

This file records parser concepts used in `lexion` and how they map to the codebase.

## Core Terms

- Token: A classified slice of source text. In `lexion_lib`, token behavior is driven by `TokenType` regexes and emitted as token instances with spans.
- Terminal: A grammar symbol matched directly by the tokenizer, represented as quoted grammar strings or EOF.
- Non-terminal: A grammar symbol expanded by productions.
- Production rule: A left-hand non-terminal and a right-hand symbol sequence.
- Epsilon: The empty production, represented by the shared epsilon token constant.
- EOF: End-of-file lookahead, required for accept actions and FOLLOW sets.
- Derivation tree: Parse result graph showing which productions matched the token stream.
- Lookahead: The token used to choose parser actions without consuming arbitrary future input.

## Grammar Construction

`Grammar::from_rules` builds the internal grammar model:

1. Separates terminal regex rules from grammar productions.
2. Finds terminals and non-terminals.
3. Inserts an augmented start production.
4. Computes nullable non-terminals.
5. Computes FIRST sets.
6. Computes FOLLOW sets.
7. Builds tokenizer token types.

Token precedence depends on terminal ordering. If two token regexes can match the same prefix, verify the generated token type order.

## FIRST, FOLLOW, And Nullable

- Nullable means a symbol or symbol sequence can derive epsilon.
- FIRST(X) is the set of terminals that can begin strings derived from X.
- FOLLOW(A) is the set of terminals that can appear immediately after non-terminal A.
- SLR(1) uses FOLLOW(left-hand side) to place reduce actions.
- LALR(1) computes more precise reduce lookaheads by propagating read/follow relations over transitions.

When FIRST/FOLLOW logic changes, test epsilon productions, recursive rules, and nested nullable sequences.

## LR Parsing Model

LR parsers read input left-to-right and build a rightmost derivation in reverse. The runtime stack alternates parser states and derivation nodes.

Core actions:

- `Shift(state)` - consume one token, push a derivation node, and transition to `state`.
- `Reduce(rule)` - pop nodes matching the rule RHS, create a node for the LHS, then use a goto action.
- `Goto(state)` - transition after a reduction on a non-terminal.
- `Accept` - finish successfully when the augmented start production has matched and EOF is valid.
- `Reject` - fail with a syntax diagnostic.
- `Conflict(actions)` - parser table construction found competing actions for the same state and symbol.

The shared parse loop lives in `GrammarParserLR::parse_trace`.

## LR(0), SLR(1), And LALR(1)

- LR(0) items are productions with a dot position and no lookahead.
- A closure adds productions expected after a dot before a non-terminal.
- Goto advances the dot over a symbol and creates a transition to another state.
- The canonical collection graph is the state machine built from repeated closure and goto.
- SLR(1) starts with LR(0) states and uses grammar FOLLOW sets for reduce lookahead.
- LALR(1) keeps LR(0)-sized states but computes lookahead sets close to LR(1) precision.

In this repo:

- `items/lr0.rs` and `items/lr.rs` define LR item behavior.
- `items/graph.rs` builds canonical collection graphs.
- `slr1.rs` constructs SLR(1) tables from LR(0) item collections and FOLLOW sets.
- `lalr1.rs` computes transition read/follow/include relations and builds LALR(1) reduce lookaheads.
- `table.rs` owns parse table entries and action conflict representation.

## Parse Table Conflicts

A conflict means the parser cannot uniquely decide an action for a state/lookahead pair.

Common causes:

- Shift/reduce ambiguity from expression grammar without precedence or associativity.
- Reduce/reduce ambiguity from overlapping productions.
- Optional constructs that share prefixes.
- Nullable productions that make FOLLOW sets too broad.

Resolution workflow:

1. Print the conflicting state, symbol, and actions.
2. Inspect the relevant grammar rules and item set.
3. Prefer grammar refactoring over action-order hacks.
4. Add a fixture that exercises the ambiguous syntax.
5. Re-run `print_grammar_conflicts`.

## `.grm` Grammar Files

`lexion_parsers` parses editable `.grm` grammar files into `GrammarData`. That data can be serialized to JSON for generated parser code. Keep `.grm` and `.json` synchronized when the workflow expects checked-in generated grammar data.

## Testing Parser Changes

- Use tiny grammars for algorithm tests.
- Assert both successful derivation shape and rejection behavior.
- Snapshot parse tables only after canonicalizing state numbers.
- Test EOF and empty input explicitly.
- Include at least one conflict check for language grammar changes.

## Debugging Tools

- `parse_table` dump: inspect shift/reduce/goto actions.
- `parse_trace` dump: inspect stack, lookahead, and action sequence.
- `grammar` dump: inspect grammar in a machine-friendly textual form.
- Derivation display: inspect parse tree shape.

Prefer these artifacts over adding permanent debug prints.
