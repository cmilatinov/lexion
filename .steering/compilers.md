---
inclusion: auto
---

# Compiler Knowledge Base

This file records compiler concepts used in `lexion` and how they apply to the current project.

## Compiler Stages

A typical compiler pipeline contains:

1. Lexing: Convert source text to tokens.
2. Parsing: Convert token streams to a parse tree or AST.
3. AST construction: Produce a semantic tree that omits grammar-only details.
4. Symbol resolution: Build scopes and connect identifiers to declarations.
5. Type checking: Validate expression and declaration types.
6. Semantic validation: Enforce language rules not captured by syntax or types.
7. IR generation: Lower AST to an intermediate representation.
8. CFG construction: Represent possible control-flow paths.
9. Dataflow analysis: Compute facts such as liveness.
10. Register allocation: Assign variables and temporaries to registers or stack slots.
11. Machine code or assembly generation: Emit target-specific output.

`lexion_lang::compiler::LexionCompiler` currently implements parse, symbol generation, type checking, TAC/CFG generation, liveness analysis, and register assignment. x86 emission is present as a developing backend area.

## AST Production

An AST should model language meaning rather than every grammar production. In `lexion_lang`, AST nodes are source-backed where diagnostics need spans. Expressions store type indexes after checking.

When adding syntax:

- Update grammar rules.
- Update parser-generated AST construction or derive mappings.
- Add AST enum/struct variants.
- Update visitors.
- Update type checking and IR generation if the construct has semantics.
- Add success and error fixtures.

## Symbol Tables

Symbol tables answer "what declaration does this name refer to here?"

`SymbolTableGraph` models scopes as graph nodes. Entries include kind, name, optional child table, source span, optional type, and optional layout. Entries are sorted for binary-search lookup.

Important rules:

- Functions, blocks, and structs can introduce scopes.
- Duplicate identifiers in the same scope are errors.
- Shadowing outer scope names is currently a warning.
- Scope traversal must enter and exit symmetrically.
- Later stages should use symbol lookup rather than rescanning AST declarations.

## Type Checking

Type checking verifies that expressions and statements are semantically valid.

Current patterns:

- Types are interned in `TypeCollection`.
- Operators are described by `OperatorTable` definitions or dynamic rules.
- Expressions are checked recursively, then their inferred type is written to the AST.
- Expected types are passed into checks for conditions, returns, variable initializers, and call arguments.
- Errors are emitted through `DiagnosticConsumer`.

When extending the type checker:

- Add operator definitions for new built-ins.
- Canonicalize aliases or references before member/index operations when appropriate.
- Keep lvalue checks separate from type equality.
- Prefer emitting a diagnostic and returning `None` over panicking.

## Semantic Validation

Semantic validation covers rules that may not be pure type rules, such as:

- `return` must appear inside a function.
- Assignment left side must be assignable.
- Function calls must target callable values.
- Non-vararg calls must match arity exactly.
- Vararg calls must provide at least the fixed parameters.
- Struct member access must target existing members.
- While and if conditions must be boolean.

Some semantic checks currently live in `TypeChecker`. If semantic validation grows, consider splitting a dedicated stage only when it reduces complexity.

## Intermediate Representation

IR gives later compiler stages a simpler, regular form than the AST. `lexion` uses three-address code in `generators/tac`.

Key concepts:

- Operand: variable, temporary, literal, label, or placeholder.
- Instruction: assignment, copy, parameter, call, return, jump, conditional jump, function markers, extern markers.
- Basic block: ordered instruction list with liveness metadata.
- Control-flow graph: directed graph of blocks.
- Function range: span of CFG locations belonging to a function.

IR generation should preserve evaluation order and create explicit control flow for branches and loops.

## Control-Flow Graphs

A CFG node is a basic block, and edges represent possible execution transitions.

For correctness:

- Conditional branches need edges to both possible successors.
- Loops need back edges and exits.
- Function boundaries need clear entry and end markers.
- Labels and jump instructions should agree with graph edges.

When adding control-flow constructs, verify the graph shape with dump output and tests.

## Liveness Analysis

Liveness asks whether a variable's current value may be read later.

The current TAC stage computes:

- Variables read and written per instruction.
- Block-level use and def sets.
- Block input and output sets by fixed-point propagation.
- Per-instruction input and output sets.
- Liveness intervals used by register allocation.

Dataflow changes should be tested with branching and loop cases, not only straight-line code.

## Register Allocation

`LinearRegisterAllocator` performs linear-scan allocation per function:

1. Sort liveness intervals by start location.
2. Expire active intervals that ended before the current interval starts.
3. Assign an available register when possible.
4. Spill either the current interval or the active interval with the farthest end.
5. Reset allocator state for the next function.

Allocation correctness depends on stable interval ordering, accurate interval end points, and unique stack slots for spills.

## x86 Backend Direction

The x86 modules define memory layout, calling convention abstractions, and System V support. Keep target-specific decisions in `generators/x86` and keep TAC target-independent where practical.

Before emitting real machine code or assembly, make these contracts explicit:

- Which ABI is targeted.
- Which registers are caller-saved and callee-saved.
- How parameters and return values are passed.
- How stack alignment is maintained.
- How spills and temporaries map to stack locations.

## Diagnostics And Failure Behavior

Compiler stages should stop cleanly when a previous stage failed. Avoid cascading panics after a diagnostic has already identified the user error. When possible, collect multiple diagnostics within a stage before returning `None`.

## Extension Checklist

For a new language feature:

1. Add grammar syntax and regenerate grammar data if needed.
2. Add parser and AST support.
3. Add visitor coverage.
4. Add symbol table entries or scope behavior if needed.
5. Add type checking and semantic validation.
6. Add TAC and CFG lowering.
7. Add backend/register allocation support if the feature reaches codegen.
8. Add compile-ok and compile-error fixtures.
9. Run parser conflict checks and focused compiler tests.
