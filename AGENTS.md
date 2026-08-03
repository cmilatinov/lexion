# Codex Guidance

Use the steering files in this repository as standing project instructions:

- [.steering/instructions.md](.steering/instructions.md) - general repository workflow, validation defaults, and task discipline.
- [.steering/code-review.md](.steering/code-review.md) - senior-developer review checklist, severity levels, and finding format.
- [.steering/architecture.md](.steering/architecture.md) - workspace structure, compiler pipeline, design patterns, and testing strategy.
- [.steering/conventions.md](.steering/conventions.md) - Rust coding conventions, diagnostics, parser grammar style, and validation tools.
- [.steering/parsers.md](.steering/parsers.md) - parser concepts, algorithms, grammar workflow, conflicts, and project mapping.
- [.steering/compilers.md](.steering/compilers.md) - compiler stages, AST production, semantic validation, IR, CFG, liveness, and backend notes.

When making repository changes, follow the branch, commit, and pull request rules in `.steering/instructions.md`. Commit titles must start with exactly one of `refactor:`, `feat:`, `fix:`, `perf:`, `chore:`, or `docs:`.

When editing steering docs, keep every `.steering/*.md` file under 200 lines.

After a branch has been pushed or a PR has been opened, make review updates as normal follow-up commits on the same branch. Only rewrite published history for an intentional base-sync rebase or when the user explicitly asks; use `--force-with-lease` for a rebase push.

When asked what is or is not implemented, or asked to fetch or pick the next highest-priority task, consult the GitHub Project `Lexion` and its linked repository issues using the available `github-projects-workflow` and `github-issues-workflow` skills. Use merged code and pull requests to verify implementation state.

Do not create Engineering Tasks for documentation- or process-only changes unless explicitly requested. Those pull requests should omit the `Engineering Tasks` section.

When the user says a PR was merged and asks to continue or move on, sync `main`, create a fresh work branch, select the highest-priority unblocked Engineering Task from the active Epic, implement it, validate it, push it, and open a PR.

For parser or grammar changes, check `.steering/parsers.md` and run the language grammar conflict test. For compiler pipeline changes, check `.steering/compilers.md` and add focused fixtures or snapshots around the affected stage.
