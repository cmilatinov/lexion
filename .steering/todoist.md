---
inclusion: auto
---

# Todoist Planning

Use the Todoist `Lexion` project as the source of truth for planned work, scope selection, progress tracking, and PR references.

## Board Structure

Use the actual `Lexion` board sections if they already exist. If the board is being created or reorganized, prefer these sections:

- `Parser and Grammar` - grammar syntax, tokenizer rules, FIRST/FOLLOW, parse tables, conflicts, derivation trees, and `.grm` parsing.
- `Language Semantics` - AST shape, symbol tables, type checking, semantic validation, diagnostics, and language fixtures.
- `Compiler Backend` - TAC, CFG, liveness analysis, register allocation, calling conventions, and x86 lowering.
- `Diagnostics and UX` - CLI behavior, miette output, source spans, dump flags, and debugging artifacts.
- `Testing and Fixtures` - compile fixtures, error snapshots, parser snapshots, and coverage gaps.
- `Codebase Improvements` - maintenance and refactors that reduce risk or unblock future work.
- `Docs and Process` - steering files, repo workflow, README updates, and architecture notes.

## Priority Interpretation

Todoist priority maps to implementation priority as follows:

- `P1` - highest priority; select first unless the user directs otherwise or a dependency is missing.
- `P2` - important near-term work.
- `P3` - useful but normally below active milestone work.
- `P4` - backlog, polish, or later-stage work unless directly connected to the current task.

When choosing work autonomously, inspect the relevant section and prefer the highest Todoist priority item. If multiple items share the same priority, choose the one that best advances parser/compiler correctness and can be completed cleanly in one PR.

## Current Product Direction

The repo is a parser/compiler workspace. Prefer work that advances this sequence unless the user selects a different area:

1. Keep the grammar conflict-free and deterministic.
2. Stabilize parser infrastructure and derivation output.
3. Improve AST production and visitor coverage.
4. Strengthen symbol table construction and semantic validation.
5. Expand type checking and enable ignored error tests when implemented.
6. Improve TAC/CFG generation and liveness analysis.
7. Finish register assignment and later x86 emission.
8. Improve CLI diagnostics and dump artifacts for debugging.

## Implementation Selection Rules

- For user-selected work, use the named Todoist task even if it is not the highest priority.
- For general "next task" requests, start with `Codebase Improvements` only when the user is asking for maintenance/refactor work; otherwise prefer the parser/compiler milestone path above.
- Group tasks only when they share the same files, compiler stage, parser algorithm, or test fixture set and can be reviewed as one coherent change.
- A PR should cover at most three Todoist items. If more than three tasks are involved, split the work into separate PRs unless the user explicitly approves a larger scope.
- Do not mix unrelated parser, compiler backend, diagnostics, and cleanup tasks in the same PR.
- If a task is too broad, implement a clearly reviewable slice and leave the Todoist item open unless the completed slice satisfies its acceptance criteria.
- Use task descriptions and acceptance notes as requirements; do not silently narrow scope below the task's stated goal.

## Progress Cross-Reference Rules

- Before starting user-directed implementation, find the matching `Lexion` Todoist task. If none exists and a PR will be opened, create one in the most appropriate section.
- Add a Todoist comment when a task is partially completed, blocked, or split into follow-up work.
- In commit and PR summaries, reference the Todoist task title and, when available, task id.
- When the implementation reveals follow-up work, create or comment on Todoist instead of burying the information only in chat.
- Keep task status aligned with code state. Do not close a task until the relevant branch is merged and the acceptance criteria are satisfied.

## PR Reference Rules

- Every PR must reference at least one Todoist task from the `Lexion` project.
- Every PR should reference at most three Todoist tasks, and those tasks must be related by feature, compiler stage, parser algorithm, or implementation scope.
- Reference tasks by Todoist task title and, when available, task id in the PR body.
- Include a concise "Architectural Decisions" section in the PR body when the code makes or changes a relevant design decision. Keep it brief and omit the section if there is no meaningful architecture impact.
- If no existing Todoist item matches the PR changes, create a new task in the most appropriate `Lexion` section before opening the PR.
- For maintenance-only changes, use `Codebase Improvements` unless another section is clearly more specific.
- For repo/process/steering changes, create or use a `Codebase Improvements` or `Docs and Process` task.
- Mention created tasks explicitly in the PR body so the planning history remains connected to the code change.

## Merged PR Housekeeping

- When working on the project, check whether relevant PRs have been merged.
- For merged PRs, update or close the corresponding Todoist tasks referenced by the PR.
- Close a Todoist task only when the merged PR satisfies the task's stated goal or acceptance criteria.
- If the PR only completes part of a larger task, leave the task open and add a Todoist comment summarizing the merged slice and remaining work.
