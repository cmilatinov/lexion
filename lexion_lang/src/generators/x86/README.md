# x86 MVP Backend

The MVP x86 backend targets 64-bit System V and keeps ABI details behind
`CallingConvention` implementations. The default target is `X86Target<SystemV64>`;
lowering code should depend on the trait, not hard-code argument or return
registers in instruction selection.

## Supported MVP

- One `main`-style function returning `i32` or unit.
- Local `i32` and `bool` values lowered through TAC temporaries or stack slots.
- Integer arithmetic: `+`, `-`, `*`, `/`, `%`.
- Integer comparisons and equality producing boolean results.
- Boolean literals and simple boolean values.
- Function prologue, epilogue, return values, and local stack slots.

## Unsupported Until Later

- Floating-point values and operations.
- Structs, tuples, strings, and other aggregate values.
- Calls, extern functions, varargs, and ABI argument passing in generated code.
- Heap allocation, projected-place references, and reference parameters or returns.
- Multi-function executable output.

Unsupported forms should fail before emission with a diagnostic rather than
silently producing placeholder instructions.
