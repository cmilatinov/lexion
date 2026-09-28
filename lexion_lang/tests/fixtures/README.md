# Compiler Fixture Layout

Fixtures are grouped by the first compiler stage or feature family whose behavior
they primarily exercise. Keep each fixture focused on one behavior so future PRs
can add or update coverage without changing unrelated snapshots.

## Directories

- `parser/` - syntax and AST-production forms that should parse and compile.
- `semantics/` - symbol table, type checker, and language semantic coverage.
- `control_flow/` - branch, loop, return, and CFG-facing source programs.
- `backend/` - TAC, CFG, liveness, register allocation, and lowering fixtures.
- `errors/semantics/` - semantic error fixtures with diagnostic snapshots.

## Naming

- Use lowercase snake case.
- Name the expected behavior, not the task number.
- Prefer one source file per behavior, such as `if_else_returns.lex` or
  `undefined_var.lex`.

## Ignored Error Fixtures

Ignored fixtures must include a concise `#[ignore = "..."]` reason describing
the missing compiler behavior. Remove the ignore only in the PR that implements
the diagnostic and updates the snapshot.
