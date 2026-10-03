# Lexion language

This glossary names the concepts shared by the compiler, scripting runtime, native target, and Rust host. The [implicit reference contract](docs/language/implicit-references.md) defines their behavior.

## Language

**Value**: A scalar, immutable descriptor, tuple, or enum whose assignment copies its outer contents; any contained object references continue to alias.

**Reference value**: An identity-bearing struct or mutable collection whose assignment aliases the same object.

**Access qualifier**: The permission carried by a typed view of a reference value: mutable or read-only.
_Avoid_: Ownership marker, exclusive borrow.

**Binding**: A local name or parameter slot that holds a value or a reference to an object.

**Exported struct**: An ordinary module declaration that Rust can select as a data root or instance state type.
_Avoid_: Behavior module, data module, state keyword.

**Data root**: A host-selected exported struct instance whose reachable object graph is globally immutable.

**Instance state**: A host-selected exported struct instance owned by one running behavior instance.

**Host handle**: An opaque, generation-checked identifier for a host-owned resource.
