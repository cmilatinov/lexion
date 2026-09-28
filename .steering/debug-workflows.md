# Debug Workflows

Use these workflows when investigating compiler stages or preparing regression
fixtures.

## Parser And Grammar

- Run grammar conflict checks with
  `cargo test -p lexion_lang grammar_conflicts -- --nocapture`.
- Use `--dump parse_table,grammar,parse_trace` to inspect parser inputs and
  parse traces in a chosen dump directory.
- Keep `lexion.grm` and `lexion.json` synchronized before expanding parser
  snapshots.

## Semantic Diagnostics

- Add compile-error fixtures under `lexion_lang/tests/fixtures/errors/`.
- Prefer rendered miette snapshots with color and unicode disabled so output is
  stable in CI and local terminals.
- Snapshot the narrowest span that explains the error, usually the offending
  expression or identifier.

## IR, CFG, And Liveness

- Use backend fixtures under `lexion_lang/tests/fixtures/backend/` for TAC,
  CFG, liveness, and register allocation behavior.
- Use `--dump ir,cfg,types,symbols` when a source fixture compiles but produces
  unexpected backend structure.
- Prefer one fixture per behavior and one snapshot per compiler artifact.

## x86 Backend

- The MVP x86 backend currently emits deterministic Intel-syntax assembly text.
- Use `cargo test -p lexion_lang --test x86_smoke` for simple lowering smoke
  coverage.
- Unsupported MVP features should be called out explicitly rather than lowered
  as silent placeholders.
