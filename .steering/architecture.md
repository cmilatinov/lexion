---
inclusion: auto
---

# Architecture

`lexion` is a Rust workspace for grammar tooling, parser generation, and a language compiler pipeline.

## Workspace Crates

### `lexion_lib`

Core parser and grammar library.

- `tokenizer/` - token definitions, spans, token instances, and tokenizer implementation.
- `grammar/` - grammar rules, terminal/non-terminal discovery, nullable analysis, FIRST sets, FOLLOW sets, token type construction, derivation tree support, and grammar serialization.
- `parsers/` - LL(1), LR(0), SLR(1), LALR(1), LR table structures, canonical collection graph, LR items, parse table actions, and shared LR parse loop.
- `parser.rs` - public `Parser` trait and `#[derive(Parser)]` re-export.
- `lexion_derive/` - derive macro support for generated parser implementations.
- `ext/` - local extensions around dependencies such as `petgraph`.
- `error.rs` - parser and syntax error types used by tokenizer/parser flows.

### `lexion_parsers`

Grammar-file parser crate.

- `grm.rs` defines the parser for `.grm` grammar source files.
- Tests parse `grammars/expression.grm`, convert parsed rules into `Grammar`, build an SLR(1) parser, and parse sample input.
- This crate bridges editable grammar syntax to the serialized grammar data consumed by parser generation.

### `lexion_lang`

Language frontend and compiler pipeline.

- `grammar/` - `lexion.grm` source grammar and generated `lexion.json` grammar data.
- `parser.rs` - `ParserLexion`, generated from `lexion.json`, producing AST plus parse trace output.
- `ast/` - AST node definitions, sourced wrappers, type definitions, and visitor traversal.
- `diagnostic.rs` - diagnostic list, errors, warnings, and info messages.
- `symbol_table.rs` - lexical scope graph, sorted entries, duplicate/shadow checks, function/struct/local/temporary symbols.
- `type_checker/` - type checker and operator table.
- `generators/tac/` - three-address code, instruction model, control-flow graph, liveness analysis.
- `generators/x86/` - memory layout, calling convention abstractions, and linear-scan register allocation.
- `compiler.rs` - orchestrates parse, symbol generation, type checking, IR generation, and register assignment.
- `src/bin/main.rs` - CLI entry point with dump flag support and miette diagnostics.

## Compiler Pipeline

The top-level pipeline is implemented in `LexionCompiler::exec`:

1. Parse source through `ParserLexion`.
2. Generate symbol tables with `SymbolTableGenerator`.
3. Type-check and annotate AST expressions with `TypeChecker`.
4. Generate TAC and CFG with `CodeGeneratorTac`.
5. Perform liveness analysis.
6. Assign registers with `LinearRegisterAllocator`.

Each stage implements the `PipelineStage` trait from `lexion_lang/src/pipeline.rs`, with explicit `Input`, `Options`, and `Output` types. Stages report errors through `DiagnosticConsumer` and return `None` when compilation cannot continue.

## Main Design Patterns

- Grammar data is generated or loaded as structured `GrammarData`, then converted to `Grammar`.
- Parser algorithms operate over `petgraph` graphs for derivations, canonical collections, symbol tables, and CFGs.
- Source locations are carried with `Sourced<T>` and `SourceSpan`; diagnostics should preserve these spans.
- AST traversal is centralized in visitor helpers. New AST nodes must be wired into immutable and mutable visitors.
- Types are interned in `TypeCollection`; AST expressions store type indexes after checking.
- Symbol tables are graph nodes linked back to parent scopes. Entries remain sorted for binary-search lookup.
- TAC uses explicit operands, labels, instructions, and blocks before backend-specific lowering.
- Liveness and register allocation work per function range.

## Testing Strategy

- Unit tests in `lexion_lib` cover grammar/parser algorithms, parse table actions, SLR/LALR parsing, conflicts, and snapshots.
- `lexion_parsers` tests cover `.grm` parsing and grammar-to-parser integration.
- `lexion_lang` tests compile fixtures in `lexion_lang/tests/fixtures`.
- Success fixtures live under `lexion_lang/tests/fixtures/*.lex`.
- Error fixtures live under `lexion_lang/tests/fixtures/errors/*.lex` and should use snapshots when diagnostics are stable.
- Grammar changes should include conflict validation through `print_grammar_conflicts`.
- Parser table snapshots should be deterministic and canonicalized before assertion.

## Generated And Debug Artifacts

- `lexion_lang/grammar/lexion.grm` is the grammar source.
- `lexion_lang/grammar/lexion.json` is generated grammar data used by `ParserLexion`.
- `dump/`, `grammar.txt`, and `table.txt` are debug or generated artifacts. Do not update them unless the task explicitly requires it.
- Compiler dump flags can emit parse tables, parse traces, AST views, symbol tables, types, IR tables, and graph DOT output.
