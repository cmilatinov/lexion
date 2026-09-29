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
- `f32` literals, copies, unary negation, arithmetic, and comparisons lowered with scalar SSE instructions.
- System V `f32` parameters, register arguments, and returns lowered through XMM0-XMM7; later arguments use stack slots.
- Thin scalar references, including local borrow/dereference and System V parameters and returns.
- Function prologue, epilogue, return values, and local stack slots.

## Value Storage Contract

- `i32`, `u32`, and `f32` use 4-byte values aligned to 4 bytes. `bool` and `char` use 1-byte memory values, while standalone execution-frame homes reserve 4 bytes.
- Function values and references other than `&str` are one pointer-sized word. A reference to an aggregate is still a thin pointer, not inline aggregate storage.
- `&str` is two consecutive pointer-sized words aligned to one word: the data pointer is the lower-addressed word at offset 0 and the length is the higher-addressed word at offset 8.
- Tuple and struct members use declaration order with C-style alignment and trailing padding. Aggregates are inline memory values; future copies must transfer the complete layout and projections must use recorded member offsets.
- Unit and empty aggregates occupy no bytes and have no return location. Raw `str` and unknown types have no standalone storage.
- `StackOffset` counts eightbyte slots for ABI arguments and register spills. Frame homes start at their required alignment and reserve their complete frame layout.
- The current aggregate ABI classifier is intentionally limited to integer-style one- and two-eightbyte values plus stack or indirect memory values. Generated aggregate calls remain rejected; float and mixed-class aggregate lowering must extend the classifier before enabling those calls.

The compiler computes aggregate layouts after type checking. Aggregate, string, and projected-place lowering remains diagnostic-only until the corresponding backend tasks implement operations over this contract.

## Unsupported Until Later

- Structs, tuples, strings, and other aggregate values.
- Extern calls, varargs, and aggregate ABI argument passing in generated code.
- Heap allocation and projected-place references.
- Multi-function executable output.

Unsupported forms should fail before emission with a diagnostic rather than
silently producing placeholder instructions.
