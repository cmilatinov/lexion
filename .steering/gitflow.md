---
inclusion: auto
---

# Gitflow And Pull Requests

## Branches

- The default branch is `staging` unless a repo-local instruction says otherwise.
- Never start work from `main` unless the user explicitly says to.
- Never commit directly to `staging` or `main`.
- Always create a separate work branch for changes.
- Use Gitflow-style branch names.
- Branch names should use one of these prefixes: `fix/`, `feature/`, `chore/`, `docs/`, `refactor/`, `perf/`.
- Prefer branches that start from current `staging` and PRs that can merge independently.
- Avoid stacked PR chains unless an unmerged dependency is required; when stacking is unavoidable, keep the dependency explicit and narrow.
- Use a dedicated git worktree for repository changes when practical. Create the worktree from the intended base branch and keep each worktree scoped to one branch or PR.
- If there are uncommitted changes in another worktree, leave them untouched and do the new work in a separate worktree instead of stashing or switching branches in place.

## Commits

- Use conventional commit prefixes that match the work type.
- Supported prefixes include `fix:`, `feat:`, `chore:`, `docs:`, `refactor:`, and `perf:`.
- Do not add agent-identifying tags such as `[codex]`, `[claude]`, `[kiro]`, `[ai]`, or similar to commit messages.

Format: `<prefix> <concise description in imperative mood>`

Prefix meanings:

- `fix:` - bug fix
- `feat:` - new feature or capability
- `chore:` - maintenance, dependencies, tooling, CI
- `docs:` - documentation only changes
- `refactor:` - code restructuring with no behavior change
- `perf:` - performance improvement

Examples:
- `refactor: split LALR lookahead propagation helpers`
- `feat: add semantic validation for struct member access`
- `fix: preserve EOF span in parser diagnostics`
- `perf: reduce parse table action cloning`
- `chore: update Rust dependencies`
- `docs: add compiler pipeline steering notes`

## Pull Requests

- Open pull requests against `staging` by default.
- Only target another branch when explicitly instructed.
- Always create a pull request for completed work.
- Never merge your own pull request.
- Wait for human review and merge.
- Pull requests should be ready for review, not drafts, unless the user requests a draft.
- Use conventional commit prefixes in pull request titles, matching the commit style.
- Supported pull request title prefixes are the same as commit prefixes: `fix:`, `feat:`, `chore:`, `docs:`, `refactor:`, and `perf:`.
- Do not add agent-identifying tags such as `[codex]`, `[claude]`, `[kiro]`, `[ai]`, or similar to pull request titles.
- PR descriptions must include:
  - `Summary`: summary plus motivation, 2 sentences maximum.
  - `Design Decisions`: architectural or design decisions made, if applicable.
  - `Tests Added`: new tests added, or `None` with a brief reason.
  - `Validation Performed`: validation run, always as the final section.
- When modifying or fixing an existing PR, update the PR title if needed and update the existing explanatory PR comment or description. Do not create a new comment for the explanation.

## Published Branches

- After a branch has been pushed or a PR has been opened, make review updates as normal follow-up commits on the same branch.
- Do not amend, rebase, or force-push a published branch unless the user explicitly asks for history rewriting.
