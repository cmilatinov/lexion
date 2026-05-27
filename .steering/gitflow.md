---
inclusion: auto
---

# Git Workflow - Gitflow

Always use the Gitflow branching model for this project.

## Branches

- `main` - production-ready code. Only receives merges from `staging` release branches or `hotfix/*` branches.
- `staging` - integration branch. All non-hotfix branches merge here. This is the default working branch.
- `feature/*` - new features. Branch from `staging`, merge back into `staging`.
- `fix/*` - bug fixes. Branch from `staging`, merge back into `staging`.
- `refactor/*` - behavior-preserving restructuring. Branch from `staging`, merge back into `staging`.
- `docs/*` - documentation-only changes. Branch from `staging`, merge back into `staging`.
- `chore/*` - maintenance, dependencies, tooling, and cleanup. Branch from `staging`, merge back into `staging`.
- `hotfix/*` - urgent production fixes. Branch from `main`, merge into both `main` and `staging`.
- `release/*` - release prep. Branch from `staging`, merge into `main` and back into `staging`.

## Commit Title Prefixes

All commit titles must start with exactly one of these prefixes:

- `refactor:` - code restructuring with no behavior change
- `feat:` - new feature or capability
- `fix:` - bug fix
- `perf:` - performance improvement
- `chore:` - maintenance, dependencies, tooling, CI
- `docs:` - documentation only changes

Format: `<prefix> <concise description in imperative mood>`

Examples:

- `refactor: split LALR lookahead propagation helpers`
- `feat: add semantic validation for struct member access`
- `fix: preserve EOF span in parser diagnostics`
- `perf: reduce parse table action cloning`
- `chore: update Rust dependencies`
- `docs: add compiler pipeline steering notes`

## Rules

- Never commit directly to `main`.
- Always create or use a Gitflow branch for changes: `feature/*`, `fix/*`, `refactor/*`, `docs/*`, `chore/*`, `hotfix/*`, or `release/*`.
- Always use a dedicated git worktree for repository changes. Create the worktree from the intended base branch and keep each worktree scoped to one branch or PR.
- If there are uncommitted changes in another worktree, leave them untouched and do the new work in a separate worktree instead of stashing or switching branches in place.
- All non-hotfix work targets `staging`.
- Keep commits atomic: one logical parser, compiler, grammar, test, or docs change per commit.
- Use feature/fix/refactor/docs/chore branches for multi-commit work.
- After a branch has been pushed or a PR has been opened, make review updates as normal follow-up commits on the same branch.
- Do not amend, rebase, or force-push a published branch unless the user explicitly asks for history rewriting.

## Pull Requests

- Every PR must reference at least one Todoist task from the `Lexion` project.
- Every PR should reference at most three Todoist tasks, and the referenced tasks must be related by feature, compiler stage, parser algorithm, or implementation scope.
- Include a concise "Architectural Decisions" section in the PR body for relevant design choices. Omit it when the change has no meaningful architecture impact.
- If no existing Todoist task matches the changes, create a new task in the appropriate `Lexion` section before opening the PR, then reference that new task in the PR body.
- For maintenance-only, repo process, or steering changes, use or create a task in `Codebase Improvements` unless another section is clearly more specific.
- Mention tests added and the validation run in the PR body.
- When relevant PRs are merged, update or close the corresponding Todoist tasks. Close tasks only when the merged PR satisfies their stated goal or acceptance criteria.
