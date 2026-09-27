---
inclusion: auto
---

# Lexion Planning

## Source Of Truth

- Repository Engineering Tasks are [Lexion GitHub Issues](https://github.com/cmilatinov/lexion/issues).
- Roadmap status, priority, size, and Epic grouping live in the [Lexion Project](https://github.com/users/cmilatinov/projects/2).
- Epics are repository issues with native subissues; use blocking links for prerequisites rather than parent membership.
- Project priority runs from `P0` (highest) through `P3` (lowest). Check live field options before editing them.

Use issues and the codebase together when reporting what is implemented. A Project status alone is not implementation evidence.

## Task Selection

- Follow a user-selected issue even when another issue has higher priority.
- For a general next-task request, choose the highest-priority unblocked issue in the active Epic or milestone.
- For maintenance or refactor requests, choose the highest-priority unblocked improvement issue instead.
- Break ties by dependency order, compiler correctness impact, and whether the outcome fits one coherent PR.
- Keep parser, compiler backend, diagnostics, and maintenance outcomes separate unless they form one reviewable slice.

## PR And Progress References

- Implementation PRs reference the matching GitHub issue; create one first if none exists.
- Keep a PR to at most three related Engineering Tasks. Documentation-only and process-only PRs need no new issue.
- Use plain issue references for PRs targeting `staging`. Update Project status as work proceeds.
- After merge, close an issue only when its acceptance criteria are satisfied; otherwise record the remaining work on the issue.
