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
   - This file for branch, commit, and PR rules.
   - `.steering/architecture.md` for crate boundaries and pipeline ownership.
   - `.steering/conventions.md` for local Rust and test conventions.
3. Locate the smallest relevant code surface.
4. Make focused edits.
5. Run formatting and validation appropriate to the change.
6. Summarize what changed, what was validated, and any known residual risk.

## Git And Pull Request Workflow

- `main` is the default integration branch and pull request base. Never commit directly to `main`.
- Create a separate work branch with one of: `fix/`, `feature/`, `chore/`, `docs/`, `refactor/`, or `perf/`.
- When work depends on an unmerged feature, branch from that feature and target its pull request. Use a dedicated worktree when practical; leave unrelated changes in other worktrees untouched.
- Use conventional commit and pull request title prefixes: `fix:`, `feat:`, `chore:`, `docs:`, `refactor:`, or `perf:`. Do not use agent-identifying tags.
- Open review-ready pull requests against `main` unless explicitly instructed otherwise. Never merge your own pull request.
- Non-documentation pull requests must reference one to three related GitHub issues in an `Engineering Tasks` section. Documentation- and process-only pull requests omit that section unless an issue explicitly tracks the work.
- PR descriptions include `Summary`, optional `Design Decisions`, applicable task references, `Tests Added`, and final `Validation Performed` sections.
- After publication, make review updates as follow-up commits. Rewrite published history only for an intentional base-sync rebase or when explicitly requested, and use `--force-with-lease` for a rebase push.

## Steering File Maintenance

- Keep every `.steering/*.md` file under 200 lines. If a steering file approaches the limit, tighten wording or split only when the new file has a clear, distinct purpose.

## Validation Defaults

- Run `cargo fmt --all -- --check` for Rust changes, or `cargo fmt --all` when preparing a commit.
- Before committing, run `cargo clippy --workspace --all-targets -- -D warnings` for Rust, parser, compiler, grammar, or test changes and fix all warnings.
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

- Use the GitHub Project `Lexion` as the planning source of truth and repository issues as the Engineering Task requirements.
- Use the available `github-projects-workflow` and `github-issues-workflow` skills for task selection, status updates, issue creation, and PR references.
- Do not create Engineering Tasks for documentation- or process-only changes unless explicitly requested.
- Keep one PR to one coherent parser, compiler, grammar, or documentation change.
- Do not mix unrelated cleanup with behavior fixes.
- Keep linked Project items `In Progress` through review. Mark them `Done` only after merge and only when the issue's acceptance criteria are satisfied.
