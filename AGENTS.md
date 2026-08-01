# Codex Guidance

Use the steering files in this repository as standing project instructions:

- [.steering/instructions.md](.steering/instructions.md) - general repository workflow, validation defaults, and task discipline.
- [.steering/todoist.md](.steering/todoist.md) - Todoist planning, task selection, progress cross-references, and PR reference rules.
- [.steering/code-review.md](.steering/code-review.md) - senior-developer review checklist, severity levels, and finding format.
- [.steering/architecture.md](.steering/architecture.md) - workspace structure, compiler pipeline, design patterns, and testing strategy.
- [.steering/conventions.md](.steering/conventions.md) - Rust coding conventions, diagnostics, parser grammar style, and validation tools.
- [.steering/parsers.md](.steering/parsers.md) - parser concepts, algorithms, grammar workflow, conflicts, and project mapping.
- [.steering/compilers.md](.steering/compilers.md) - compiler stages, AST production, semantic validation, IR, CFG, liveness, and backend notes.

When making repository changes, follow the branch, commit, and pull request rules in `.steering/instructions.md`. Commit titles must start with exactly one of `refactor:`, `feat:`, `fix:`, `perf:`, `chore:`, or `docs:`.

When editing steering docs, keep every `.steering/*.md` file under 200 lines.

After a branch has been pushed or a PR has been opened, make review updates as normal follow-up commits on the same branch. Only rewrite published history for an intentional base-sync rebase or when the user explicitly asks; use `--force-with-lease` for a rebase push.

When asked what is or is not implemented, or asked to fetch or pick the next highest-priority task, consult the Todoist `Lexion` project first. For maintenance, process, or steering work, use `Codebase Improvements` or `Docs and Process` unless another section is clearly more specific.

Do not create Todoist tasks for documentation-only changes. Docs-only PRs should omit Todoist and `Tasks Addressed` sections.

When the user says a PR was merged and asks to continue or move on, sync `main`, create a fresh work branch, pick the next highest-priority Todoist task plus tightly connected tasks, implement, validate, push, and open a PR.

For parser or grammar changes, check `.steering/parsers.md` and run the language grammar conflict test. For compiler pipeline changes, check `.steering/compilers.md` and add focused fixtures or snapshots around the affected stage.
