# Implicit references and access qualifiers

Status: language contract for [#123](https://github.com/cmilatinov/lexion/issues/123). Syntax and checking belong to #124; VM execution and native migration belong to #125. Examples below specify required behavior and are not claims about the current compiler.

This contract applies to scripting and to already supported native language forms. The scripting VM and x86 backend may support different new features, but any form accepted by both has the same copy, alias, and access behavior. Source diagnostics identify the expression or declaration that violates the contract.

## Representation and copying

| Type | Assignment, argument, and return | Identity and mutation |
| --- | --- | --- |
| `i32`, `u32`, `f32`, `bool`, `char`, `()` | Copy the value. | No shared identity. |
| `str` | Copy an immutable string descriptor; bytes may be shared. | No string mutation or observable byte identity. |
| Ordinary `struct` | Copy an implicit reference to a managed object. | Aliases observe field writes; this contract introduces no identity comparison operator. |
| `Array<T>`, `Map<K,V>` | Copy an implicit reference to a managed collection. | Aliases observe element writes; map iteration remains sorted by key. |
| Tuple | Copy the tuple and each element by its own rule. | Tuple slots have no identity; reference-bearing elements retain aliases. |
| Payload enum | Copy the tag and payload by the payload type's rule. | A reference-bearing payload retains its alias. |
| `T?` | Copy the null/tag and present payload by `T`'s rule. | A present reference retains identity; narrowing is required before access. |
| Opaque host handle, including nullable handle | Copy the resource id and generation. | It does not own the resource; each use checks validity with the host. |
| Function value | Copy an immutable callable identity. | No captured environment or mutable function object in this milestone. |

`T` in a declaration names its logical type, independent of storage layout. A struct field declared `health: i32` is a scalar slot; a field declared `pet: Pet` holds an object reference. Tuple and enum containers copy their slots, so they can contain reference values without making the container itself an object. `()` is the empty value and remains the callback result type.

Assignment to a name replaces the value in that binding. Assignment to a field or collection element changes the object reached through the binding. Passing or returning a reference value keeps the same object; no implicit deep copy occurs. Each `struct` or collection constructor allocates a fresh object, even when its inputs are equal. A returned reference remains valid while reachable from a live binding, instance state, host-owned root, or another reachable object. The VM roots values during evaluation and calls and reclaims unreachable objects; the native target must provide equivalent safe lifetimes or reject an unsupported escape with a diagnostic.

`copy(x)` is the explicit deep-copy operation for a reference graph. It creates fresh struct/collection objects, preserving cycles and repeated references *within* the new graph. Scalar, string, enum, tuple, nullable, and handle slots follow the table above; a copied handle still identifies the same host resource. Copying an immutable data root creates a separate graph that can receive `mut` access; the original stays frozen. Copying is distinct from assignment. #124 defines the intrinsic's frontend shape; #125 implements it for supported runtime types. A backend that cannot copy a supported graph must report an unsupported operation, never silently alias it.

## Access and aliasing

`mut T` grants write access to the reachable object graph of a reference value. `const T` grants read-only access, and unqualified `T` means the same read-only access. A composite tuple, enum, or nullable value carries that permission into reference-bearing elements; a qualifier on a composite with no reference-bearing elements is redundant and rejected. For scalar and other pure value types, the compiler likewise rejects `mut` or `const` and suggests plain `T`. A newly constructed or deeply copied graph yields mutable access; assigning it to an unqualified or `const` view narrows that access. In a struct declaration, a field's `T` specifies the stored type; access to the field comes from the path used to reach it. A qualifier on a field declaration is unnecessary and rejected. Function parameter and return types, local type annotations, and exported callback metadata retain access qualifiers; `mut T` and `const T` are distinct signature contracts.

Bindings may be reassigned to another value of the same type, regardless of the access qualifier. Reassigning a read-only binding changes its local slot, not the referenced object. A write to `root.child.field` or `root.items[i]` requires mutable access at every reference-bearing step of the path. Reading through a mutable view may be passed to a read-only parameter. A read-only view cannot be passed to a mutable parameter, stored as mutable access, cast to it, or returned as mutable access. Type inference retains the source view's permission; it cannot invent `mut` from an unqualified alias. A conditional or match join uses the least permissive access of its alternatives. A nullable reference, when narrowed, retains its original access.

Storing a reference in a mutable object must preserve that permission: every reference that becomes reachable through the mutable path must already have mutable access. This applies to struct field initialization and replacement, collection insertion and replacement, nested tuple/enum/nullable payloads, source defaults, and host overrides before the object is published. A read-only view cannot be inserted even when another alias to the same object is mutable; the source view itself must grant access. There are no per-field or per-element permissions that a later read could recover. The frontend rejects a known read-only source at the insertion site; the VM checks host-supplied values and globally frozen objects before storing them. `copy(read_only_child)` supplies a fresh mutable object when an independent child is intended. Copying a host handle does not change its host permissions.

Multiple mutable aliases to one object are legal. They are capabilities checked by Lexion, not exclusive Rust `&mut` borrows. Writes through one alias are visible through the others in program order. The VM executes callbacks on the host's update thread and rejects same-instance synchronous reentry. Rust must not hold an exclusive borrow into an object while invoking script that could alias it. Host APIs expose controlled operations or owned values instead of raw Rust pointers, Rust references, or engine objects.

`const` is a read-only *view*, not a global freeze: another valid mutable alias may change the object. A host-loaded **data root** is different: the host marks its entire reachable mutable object graph immutable. No mutable alias may be created from it through a field, collection, nullable narrowing, return, or host operation. A deep copy of the data graph may be used as independent mutable instance state if the new binding is declared `mut`; the copied objects are outside the frozen graph. An immutable string cannot be mutated through either route.

References are managed object identities, not addresses into another object's movable fields or stack slots. A field read of reference type carries a rooted object identity; a scalar field read copies the scalar. Returning an alias into a live state graph is safe because the result keeps that graph reachable. A local object returned by reference transfers reachability to the caller. A view into a temporary host-owned borrow cannot escape the host call; the host boundary returns an owned copy or a separately validated opaque handle. No pointer arithmetic or address-of operation can bypass access checks.

## Modules, construction, and ownership

An ordinary module contains declarations and exports selected by name. `export struct EnemyState { ... }` is the proposed exported struct spelling; `export fn` exports a function. There are no behavior, data, or library module role keywords and no designated state declaration. Rust chooses an exported struct as a data root or as per-instance state and validates that selection against the compiled schema. Ambiguous, missing, inaccessible, or incompatible exports fail before constructing an instance.

Fields may have a source default (`health: i32 = 100`) or be required. A constructor or host override supplies every required field and must match the declared type; duplicate, missing, and unknown fields are errors. Defaults are evaluated as deterministic, side-effect-free constructor expressions: literals, tuples, and struct/enum/collection constructors with pure arguments. They cannot call arbitrary functions, query the host, read ambient state, or depend on another field's initialization order. The frontend checks defaults once against the schema; each construction evaluates or deep-copies them into an independent graph. Nested mutable defaults in two instances never alias. A host override replaces its selected field after type checking and before publication of the completed instance. An owned host record override is copied into a fresh VM graph before insertion into mutable instance state; a borrowed or frozen host object cannot become a writable alias. Failed construction publishes nothing.

The host owns instance lifetime and chooses when to invoke a `callback fn` with explicit typed arguments, including `state: mut EnemyState`. Callbacks return `()`. The host validates names, result, qualifiers, types, and source spans against the compiled signature before invocation. The script cannot schedule its own callback. Instance state is one host-selected exported struct graph; data roots are separately owned immutable graphs. Qualified imports expose ordinary declarations under #108 without changing these access rules.

The Rust manifest boundary accepts owned scalars, strings, immutable plain records, and nullable opaque handles. A script record sent across it is recursively copied into an owned immutable Rust value. A Rust record result becomes a fresh script object with read-only access until explicitly copied into a mutable graph. Neither side receives a VM object identity or a live borrow through this boundary. Handles carry a resource id and generation and are checked when used; copying or serializing a handle does not extend the host resource lifetime. Signature validation includes access qualifiers for script callbacks and module exports, while host operations use the manifest's existing owned-value contract and version checks.

Serialization stores the reachable instance-state graph with type/schema identity, object ids, sharing, cycles, scalar values, enum tags, nullable tags, and handle tokens where the host supplies a restoration rule. It does not serialize access capabilities as ownership or encode process pointers. Loading creates a new rooted graph, applies the same global immutability rule to data roots, and checks schema compatibility before publishing any restored instance. A handle without a valid host restoration mapping is rejected or restored as invalid and fails safely on use, per the host manifest contract. Default-compatible structural migration operates on the graph and preserves alias relationships among surviving fields.

## Required example outcomes

The snippets are contract examples for #124/#125, not current executable fixtures. `let` bindings can be reassigned; permission controls object writes. `copy` is the proposed intrinsic.

```lexion
export struct EnemyState { health: i32 = 100, position: Point = Point { x: 0, y: 0 } }
export struct Point { x: i32 = 0, y: i32 = 0 }

callback fn update(state: mut EnemyState) -> () {
    let alias: mut EnemyState = state;
    alias.health = 90;
    state.position.x = 4; // visible through alias
}
```

Two host-created `EnemyState` instances begin with independent `position` objects. Invoking `update` on one changes only that instance. `let snapshot: mut EnemyState = copy(state)` creates a separate graph; later writes through `snapshot` do not change `state`.

```lexion
fn observe(state: EnemyState) -> i32 { return state.health; }
fn bad(state: EnemyState) -> () { state.health = 1; } // error: read-only path
fn promote(state: const EnemyState) -> mut EnemyState { return state; } // error: access escalation
fn require_mut(state: mut EnemyState) -> () { state.health = 1; }
fn borrow_bad(state: EnemyState) -> () { require_mut(state); } // error: mut argument required
```

A `const EnemyState` view can be passed to `observe`; `observe` may see changes made through a separate mutable alias on a later call. A nullable `EnemyState?` must be narrowed before reading `health`; narrowing never grants mutable access. A host data root passed to `update` fails validation before callback side effects. `copy(data)` can initialize a separate mutable state graph when bound as `mut EnemyState`.

```lexion
export struct Child { value: i32 = 0 }
export struct Container { child: Child }

fn rejected(child: const Child) -> () {
    let box: mut Container = Container { child: child }; // error: read-only child in mutable field
}
fn accepted(child: const Child) -> () {
    let box: mut Container = Container { child: copy(child) };
    box.child.value = 1; // the original child is unchanged
}
```

```lexion
let a: i32 = 1;
let b: i32 = a;
a = 2;                 // b remains 1
let first: mut EnemyState = EnemyState { health: 100 };
let second: mut EnemyState = first;
second.health = 7;      // first.health is now 7
```

`&value`, `*value`, and `&T` in old reference fixtures receive migration diagnostics. `a & b` remains bitwise AND and `a * b` remains multiplication.

## Migration and conformance

The current grammar accepts explicit `&T`, unary `&`, and unary `*`; the native backend has reference, aggregate, and `&str` fixtures. #124 removes those reference forms and implements the qualifier syntax, export/default syntax, access checks, source spans, and diagnostics. Source declarations should migrate from `&Point` to `Point` or `mut Point` according to permission, from `&str` to `str`, and from `&x`/`*x` to direct reference-value use and field access. A former scalar reference parameter cannot be translated to `mut i32`: scalars remain copies. If callers need shared mutable scalar state, place it in a struct and pass `mut Cell`; unsupported native migration cases require explicit diagnostics rather than changed behavior. Infix `&` and `*` retain their existing bitwise and arithmetic meanings.

| Scenario | Expected result |
| --- | --- |
| Assign struct alias, mutate through either `mut` view | Both views read the changed field. |
| Copy struct with two fields pointing to one child | Copy has one new child shared by its fields; original child is unchanged. |
| Return a newly constructed struct or a child of live instance state | Result remains rooted and usable after the callee returns. |
| Reassign a read-only binding | Local name changes target; neither old nor new target is mutated. |
| Pass `const` or unqualified struct to `mut` parameter | Source-located type error before invocation. |
| Initialize or replace a mutable struct field with a read-only child | Source-located insertion error, even if the container is freshly constructed. |
| Insert a tuple, enum, or nullable value holding a read-only child into a mutable collection | Source-located insertion error; wrapping does not grant access. |
| Insert `copy(read_only_child)` into a mutable container | Accepted; the new child is writable and does not alias the original. |
| Narrow read-only `T?`, then mutate nested field | Source-located read-only error. |
| Read through `const` after another legal mutable alias writes | New value is visible; `const` is a view, not a snapshot. |
| Put an alias of a data root in mutable instance state, then write | Mutation is rejected because the target object is frozen. |
| Construct two instances with nested collection defaults | Their collection objects are distinct; mutations stay per instance. |
| Copy a tuple or enum containing a struct reference | The container slots copy; the contained reference aliases unless `copy` is used. |
| Pass script record to Rust, then mutate script record | Rust's received owned record stays unchanged. |
| Load a stale handle token | Safe invalid-handle result; no access to a recycled resource. |
| Compile old unary borrow/dereference or `&T` | Migration diagnostic; infix bitwise `&` and multiply `*` still work. |

| Area | Contract checks for implementing task |
| --- | --- |
| Frontend (#124) | Parse all qualifier spellings and exported defaults; preserve spans and qualifiers in signatures; reject read-only writes, escalation through insertion, invalid defaults, old reference syntax, and mutation through a nested or narrowed read-only path. |
| VM (#125, #104) | Preserve object identity for struct/collection aliases; root returned/local objects; deep copy without losing cycles or internal sharing; check host-supplied insertions and enforce global data-root freeze and mutable path checks. |
| Host bindings (#125, #102, #103) | Validate callback qualifiers and result before invocation; create independent instance graphs; copy plain records at manifest boundary; reject invalid handle generations and reentry without exposing Rust pointers. |
| Native migration (#125) | Update supported reference, aggregate, and string fixtures to direct access; preserve scalar copy and struct alias semantics for accepted code, or produce a clear unsupported-feature diagnostic. |
| Serialization (#110, #111) | Round-trip sharing, cycles, defaults, nullable/enum tags, and schema identity; migrate atomically and reject incompatible or invalid handle restoration without publishing a partial graph. |

The implementing tasks should turn the positive and negative examples into focused fixtures, runtime tests, and native regressions. Grammar changes run the repository conflict test. The same source form must not compile with different copy or access meaning between targets.
