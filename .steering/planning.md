---
inclusion: auto
---

# Lexion Planning

## Source Of Truth

- The [Lexion GitHub Project](https://github.com/users/cmilatinov/projects/2) (owner `cmilatinov`, Project #2) is the tracking source of truth for roadmap status, priority, size, and Epic grouping.
- Repository Engineering Tasks are linked [Lexion GitHub Issues](https://github.com/cmilatinov/lexion/issues); their descriptions and acceptance criteria define the requirements.
- Epics are repository issues with native subissues; use blocking links for prerequisites rather than parent membership.
- Project priority runs from `P0` (highest) through `P3` (lowest). Check live field options before editing them.

Use issues and the codebase together when reporting what is implemented. A Project status alone is not implementation evidence.

## Task Selection

- Inspect the live Project and linked issues before recommending or selecting work; local planning drafts may be stale.
- Follow a user-selected issue even when another issue has higher priority.
- For a general next-task request, choose the highest-priority unblocked issue in the active Epic or milestone.
- Verify that the issue is open and its native blockers and stated prerequisites are satisfied. A `Ready` status or `ready-for-agent` label alone does not establish readiness.
- Check linked PRs and merged code before selecting an issue so completed work or an active implementation is accounted for.
- For maintenance or refactor requests, choose the highest-priority unblocked improvement issue instead.
- Break ties by dependency order, compiler correctness impact, and whether the outcome fits one coherent PR.
- Keep parser, compiler backend, diagnostics, and maintenance outcomes separate unless they form one reviewable slice.

## PR And Progress References

- Implementation PRs reference the matching GitHub issue; create one first if none exists.
- Add new implementation issues to the Lexion Project, using its existing fields and Epic relationships.
- Keep a PR to at most three related Engineering Tasks. Documentation-only and process-only PRs need no new issue.
- For PRs targeting the default branch `main`, use `Closes #<number>` only when the PR fully satisfies that issue's acceptance criteria; use plain references for partial work or another target branch. Human acceptance issues remain open until the required person accepts and closes them.
- Keep Project items `In Progress` through implementation and review, as specified in `.steering/instructions.md`. Record blockers or remaining work on the linked issue and align Project status with that evidence.
- After merge, close an issue only when its acceptance criteria are satisfied; otherwise record the remaining work on the issue.
