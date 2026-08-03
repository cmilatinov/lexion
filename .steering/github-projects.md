---
inclusion: auto
---

# GitHub Project Planning

Use the private GitHub Project [Lexion](https://github.com/users/cmilatinov/projects/2) as the source of truth for planned work, priority, grouping, and progress. Repository issues are the source of truth for executable Engineering Task requirements. Merged code and pull requests are the source of truth for implementation state.

Documentation- and process-only changes do not require an Engineering Task unless the user explicitly requests one.

## Planning Model

- An Epic is a Project-only draft item describing a larger outcome. Do not implement directly from an Epic.
- An Engineering Task is a repository issue with one independently valuable, testable outcome. It normally maps to one pull request.
- Add Engineering Tasks to the `Lexion` Project and use the `Epic` field to connect them to the active outcome.
- Use the live Project fields rather than assuming stale values. The expected fields are `Status`, `Priority`, `Size`, and `Epic`.
- Use `Todo`, `In Progress`, and `Done` for status; `P1` through `P4` for priority; and `S`, `M`, or `L` for size.

Inspect the live plan with:

```sh
gh project item-list 2 --owner cmilatinov --limit 100 --format json
gh project field-list 2 --owner cmilatinov --format json
```

## Task Selection

When choosing work autonomously:

1. Continue a relevant unblocked `In Progress` Engineering Task before starting another task.
2. Otherwise select an unblocked `Todo` task from an active Epic, preferring `P1`, then `P2`, `P3`, and `P4`.
3. Break priority ties by dependency order, then by the task that most directly advances the Epic and compiler correctness.
4. Confirm the issue has a bounded goal and testable `Done when` criteria. Refine or split an oversized task before implementation.
5. Use the named issue when the user selects work explicitly, even when another item has higher priority.

Group tasks only when they share the same compiler stage, parser algorithm, files, or fixtures and form one coherent review. A pull request should address at most three related Engineering Tasks.

## Autonomous Execution

- Treat the issue goal, `Done when` criteria, repository steering, and existing code patterns as sufficient direction for routine implementation decisions.
- Work independently through investigation, implementation, tests, commit, push, and a review-ready pull request. Do not ask the user to choose routine technical details that the repository can answer.
- Ask only when requirements conflict, a choice materially changes user-visible language behavior, required access is missing, or an irreversible external action needs approval.
- Never merge your own pull request. Human review and merge are the validation gate.

## Engineering Task Standard

Use this issue structure:

```md
## Goal
<!-- One outcome in one sentence. -->

## Why
<!-- Problem or user impact. -->

## Done when
- [ ] Testable acceptance criterion

## Context
<!-- Dependencies, constraints, or relevant links. Delete if empty. -->
```

- Search open and closed issues before creating a new Engineering Task.
- Do not create an issue for documentation- or process-only work unless explicitly requested.
- Add newly created non-documentation tasks to the Project with appropriate field values.
- Record dependencies and follow-up work in issues rather than only in chat.

## Progress And Pull Requests

- Move an Engineering Task to `In Progress` when implementation starts and keep it there through review.
- If work is blocked, partial, or split, comment on the issue with completed work, remaining work, and the blocker or follow-up issue.
- Use an `Engineering Tasks` section in non-documentation PRs. Use `Closes #<number>` only when the PR fully satisfies the issue; use a plain issue reference for partial work.
- Omit the `Engineering Tasks` section from documentation- and process-only PRs that have no issue.
- Include an optional `Design Decisions` section when the change makes a meaningful architectural choice.
- Mention newly created issues in the PR body so planning history remains connected to implementation.

## Merge Housekeeping

- Check whether relevant pull requests merged before selecting new work.
- After merge, verify every linked issue's `Done when` criteria against the merged result.
- Close the issue and move its Project item to `Done` only when all criteria are satisfied.
- For a partial merge, leave the issue open and `In Progress`, then comment with the completed slice and remaining work.
- Mark an Epic `Done` only after all of its Engineering Tasks are complete.
