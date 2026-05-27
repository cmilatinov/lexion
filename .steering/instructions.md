---
inclusion: auto
---

# Repository Workflow

These are the default operating instructions for work in `lexion`.

## Working Principles

- Treat `lexion` as a Rust compiler/parser workspace. Preserve parser correctness, diagnostic quality, and deterministic compiler output ahead of broad refactors.
- Read local code before changing behavior. Prefer the established crate boundaries and helper APIs over adding parallel implementations.
- Keep changes scoped to the requested compiler stage, parser algorithm, grammar rule, or workflow document.
- Protect existing work in the tree. If files have uncommitted changes that are not part of the current task, leave them untouched.
- Use `rg` and `rg --files` for code search.
- Do not silently broaden grammar or language behavior. Document any syntax or semantic change in tests and, when useful, in `.steering/parsers.md` or `.steering/compilers.md`.

## Standard Change Flow

1. Inspect `git status --short`.
2. Read the relevant steering files:
   - `.steering/gitflow.md` for branch, commit, and PR rules.
   - `.steering/todoist.md` when selecting work or preparing PR references.
   - `.steering/architecture.md` for crate boundaries and pipeline ownership.
   - `.steering/conventions.md` for local Rust and test conventions.
3. Locate the smallest relevant code surface.
4. Make focused edits.
5. Run formatting and validation appropriate to the change.
6. Summarize what changed, what was validated, and any known residual risk.

## Steering File Maintenance

- Keep every `.steering/*.md` file under 200 lines. If a steering file approaches the limit, tighten wording or split only when the new file has a clear, distinct purpose.

## Validation Defaults

- Run `cargo fmt --all -- --check` for Rust changes, or `cargo fmt --all` when preparing a commit.
- Run `cargo test` for broad parser/compiler changes.
- Run focused package tests when the scope is narrow:
  - `cargo test -p lexion_lib`
  - `cargo test -p lexion_parsers`
  - `cargo test -p lexion_lang`
- For grammar changes, run `cargo test -p lexion_lang print_grammar_conflicts -- --nocapture` and verify there are no conflicts unless the user explicitly accepted a known conflict.
- For compiler pipeline changes, include at least one success fixture or error fixture test that exercises the changed stage.

## Dump Artifacts

The language compiler supports dump flags such as `parse_table`, `parse_trace`, `grammar`, `ast`, `symbols`, `types`, `ir`, `cfg`, and `all`. Use these dumps to debug parser and compiler behavior, but do not commit generated files from `dump/` unless a task explicitly asks for golden artifacts.

## Task And PR Discipline

- Use Todoist as the planning source of truth when choosing the next task or opening a PR.
- Use the `Lexion` Todoist project for this repo. If the project does not exist yet, create or request it before opening a PR that needs task references.
- Keep one PR to one coherent parser, compiler, grammar, or documentation change.
- Do not mix unrelated cleanup with behavior fixes.
- When a PR is merged, close the linked Todoist task only if the merged code satisfies the task's stated goal or acceptance criteria.
