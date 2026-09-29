---
inclusion: auto
---

# Code Review Workflow

When the user asks for a review, default to a senior-developer code review stance. Prioritize concrete bugs, regressions, security risks, parser/compiler correctness issues, performance problems, maintainability issues, and missing edge-case handling over summaries or praise.

## Review Checklist

Check for:

1. Bugs: logic errors, off-by-one mistakes, invalid state transitions, incorrect parser table actions, incorrect scope lookup, wrong type unification, stale liveness data, and register allocation mistakes.
2. Security and safety: risky filesystem access, unsafe input handling, path traversal, unchecked generated paths, panics on user-provided source, and malformed grammar handling.
3. Performance: unnecessary allocations in tokenizer/parser loops, repeated graph traversals, avoidable parse table rebuilds, excessive cloning in AST/type/IR paths, and scalability problems with larger grammars.
4. Maintainability: unclear names, excessive complexity, duplicated algorithms, brittle coupling between crates, missing tests around changed behavior, and code that violates local parser/compiler patterns.
5. Edge cases: empty input, EOF handling, epsilon productions, nullable grammar sequences, ambiguous grammars, parse conflicts, nested scopes, duplicate identifiers, varargs calls, pointer/reference operators, missing return values, empty blocks, and partial compilation failures.
6. Diagnostics: incorrect source spans, misleading error messages, panics instead of diagnostics, lost warnings, and output that makes failing fixtures hard to understand.
7. Tests: missing success fixtures, missing error snapshots, ignored tests that should now be enabled, unstable snapshot ordering, or grammar changes without conflict checks.

Be strict. It is better to surface real issues during review than after merge.

## Finding Format

For each issue, include:

- Severity: `Critical`, `High`, `Medium`, or `Low`.
- File and line number or a narrow section reference.
- What is wrong.
- Why it matters.
- How to fix it.
- A relevant clickable file link and, when useful, a short code snippet.
- A clear `Keep` / `Skip` choice for the user.

List issues one by one. Keep each finding concise and specific enough that it can become an implementation task without more investigation.

## Severity Guidance

- `Critical` - data loss, security vulnerability, crash on common user input, parser/compiler corruption that blocks all builds, or a miscompile on ordinary source.
- `High` - likely user-visible compiler bug, serious regression in grammar parsing, hard-to-debug semantic issue, or incorrect generated IR/register assignment in core paths.
- `Medium` - plausible bug, edge-case failure, test gap around meaningful behavior, diagnostic quality issue, or maintainability issue that will slow near-term work.
- `Low` - small cleanup, naming, local clarity, minor robustness improvement, or documentation mismatch.

Avoid inflating severity. If a finding is speculative, say what evidence would confirm it.

## Interaction Flow

1. Present findings first, ordered by severity.
2. For each finding, ask whether to `Keep` or `Skip`.
3. Do not implement fixes during the initial review unless the user explicitly asks for immediate fixes.
4. After the user chooses which findings to keep, make a concise fix list from only the kept findings.
5. When the user tells you to proceed, implement all necessary fixes for the kept findings.
6. After implementation, run focused validation and summarize which findings were fixed.

If no issues are found, say that clearly and mention any residual test gaps or risks.

## Lexion-Specific Review Prompts

- Grammar changes: Are FIRST/FOLLOW, nullable, SLR(1), and LALR(1) expectations still valid? Do conflict tests explain any ambiguity?
- Parser changes: Are shift, reduce, goto, accept, reject, and conflict actions handled deterministically?
- AST changes: Are spans preserved through lowering, and do visitors traverse new nodes in both immutable and mutable paths?
- Symbol table changes: Are lexical scopes entered and exited symmetrically? Are entries sorted consistently for lookup?
- Type checker changes: Are inferred types written back to AST nodes, and are errors emitted instead of panics?
- IR changes: Are CFG edges correct for branching and loops? Are temporaries inserted into the symbol table with types?
- Register allocation changes: Are intervals sorted correctly, old intervals expired, and spill locations unique per function?

## Repo Workflow

- When review fixes become a PR, use the available `github-issues-workflow` skill: reference at least one matching Engineering Task for non-documentation work, create one if none exists, and keep PR scope to at most three related issues.
- Keep review-fix commits atomic when practical.
- Do not mix unrelated cleanup into review fixes unless the user explicitly includes it.
