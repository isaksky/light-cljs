# Native form macros (experimental)

Build and run the bounded big-boy-pants-js backend without linking V8 or
QuickJS:

```sh
cargo test --locked --no-default-features --features bbp-macros --test bbp_macros
cargo test --locked --no-default-features --features bbp-macros --lib native_source_macro_preserves_datum_identity
cargo build --locked --no-default-features --features bbp-macros
cargo tree --locked --no-default-features --features bbp-macros -e normal
```

The integration suite compiles real source macros with the ordinary analyzer and
emitter, checks their applications in Node, and checks native occurrence reuse,
zero repeated imports, incremental environment imports, metadata, syntax quote,
`&form`, `&env`, and error provenance. Node is needed only to run emitted
applications. Neither legacy engine appears in the last command's dependency
list. Emitted application JavaScript still uses the conventional runtime.

For a CLI smoke test (prints `40`):

```sh
cargo run --locked --no-default-features --features bbp-macros -- --compile --text '(defmacro maximum [x] (js/Math.max x 2)) (js/console.log (maximum 40))' | node
```

Namespace builds use the same feature flags with
`--compile --entry app --source-root PATH --jobs 1 --stats`. The compiler emits
ordinary application JavaScript on stdout and bridge statistics on stderr.
Per-macro `macro.gc.*` statistics also report collections, total pause nanoseconds,
the runtime's maximum pause so far, and the live-object count at its last collection.
Pauses exclude rendezvous waits, payload destruction and observer delivery.

`bbp-macros` adds an optional workspace dependency. In a build containing a
legacy engine, call `MacroHost::use_native_macros()` to select it explicitly.
When it is the only compiled engine it serves clean compilations automatically.
The production default remains V8 with QuickJS for interactive macro sessions.
QuickJS-only builds now use `--no-default-features --features quickjs`; plain
`--no-default-features` embeds no engine. Interactive sessions and compiler data
readers do not use this backend.

A compilation retains one shared runtime and aggregate execution control. Each
task has private function handles and mutable JavaScript state. Native forms and
publishable closures cross rooted dependency boundaries; mutable Maps remain
task-owned. Commands do not serialize forms, invoke a reader, or replay effects.
Input nodes are memoized across the runtime's tasks by compiler Datum
identity, with the Datum retained to prevent address reuse. Each imported node
has an occurrence token; export checks the exact occurrence object and returns
its original Datum, including exact numeric/character representation and source
provenance. Newly constructed native nodes alone allocate new Datum nodes.
An opaque heap-and-generation identity table preserves the same reuse for raw
native metadata maps, whose engine contract does not wrap metadata in occurrences.
Scalar values still use distinct occurrence wrappers, even when their values agree.

The `&env` envelope remains a persistent native map. A changed slot is associated
into its previous version; unchanged namespace/reader subtrees use their existing
rooted imports. Every macro call records imported, exported and reused nodes plus
bridge nanoseconds. `MacroHost::native_macro_calls()` exposes those per-call
records; namespace compilation's `--stats` prints `macro.bridge.N.*` lines. The
cache identity distinguishes this backend and its native-form ABI, hashes the
engine source, and includes preloaded helper source.

## Supported boundary

This is a finite-form integration, not backend parity. Lists, vectors, maps,
sets, keywords, symbols, scalar occurrences, metadata, construction, lookup,
basic finite sequence operations, equality, hashing, arithmetic
addition/subtraction and native truth tests have first-party implementations
using native form intrinsics. `seq` returns native lists for vectors, map
entries (two-element vectors), set members and strings (UTF-16 code units),
preserves nonempty lists, and returns nil for nil or empty supported inputs.
Other input kinds report an unsupported-operation error.

### Native core lowering

The backend selects a compile-time emission mode (part of its cache identity)
in which macro units do not use the JavaScript collection implementations.
Vector, list, map and set literals, quoted keywords and symbols, and empty
collections are the engine's `__rt.formVector(…)`, `formList`, `formMap`,
`formSet`, `formKeyword` and `formSymbol` constructors. Calls of `count`,
`seq`, `sequence`, `first`, `second`, `rest`, `next`, `cons`, `concat`, `nth`,
`get` (and keyword invocation), `contains?`, `assoc`, `dissoc`, `conj` and
`disj` with one key or value, `=` of two values, `hash`, `meta`, `with-meta`,
`keyword?`, `symbol?`, `vector?`, `map?`, `set?`, `seq?`, `nil?`, `not`,
`boolean`, `empty?`, `keyword` and `symbol` with one or two arguments, and
`vector`, `list`, `hash-set`, `hash-map`, `array-map` and `concat` with any
arguments are the engine's `__rt.clj*` intrinsics (big-boy-pants-js
`src/collections/clj.rs`): Rust, nil-punning, transparent to input
occurrences. Truth tests are `__rt.cljTruth`. A `concat` of three or more
collections is one flat `__rt.cljConcatAll(__rt.formList(…))`, so a wide call
nests no deeper than a narrow one. A hash-map literal (more than eight entries)
evaluates every key before any value, as `mapFromArrays` does, through
`__rt.cljZipMap`. Literals and calls with more than 1,024 operands use the
runtime's array constructors instead of one counted intrinsic. Other arities,
core functions used as values (`(map first xs)`, `(apply keyword xs)`) and the
remaining operations call the runtime object, whose entries use the same
intrinsics (it also provides `set`, `vec`, `keys`, `vals`, `keyword`, `symbol`,
`array-map`, `hash-set` and variadic `assoc`/`dissoc`/`conj`/`disj`). A
keyword or symbol whose spelling does not split back into its namespace and
name (`(keyword nil "a/b")`, `(keyword "a/b/c")`) has no native form and fails
explicitly.
`rest` and `next` copy the remaining elements into a new list; there are no
shared seq views. Lazy `map`, `filter` and friends are not implemented
(light-cljs-479 owns lazy sequences), which still stops the generated
benchmark project.

Lowering is what lets helper values publish: a function whose body uses only
lowered operations, literals and other compiler roots captures no task-owned
runtime object, so its initializer's value is published once and every task
shares it (`native.helpers.published`), instead of initializing again in each
consuming task (`native.helpers.per-task`). A function that still reads the
runtime object (a non-lowered core function, a variadic parameter list) stays
per-task. Sorted maps and sets, transients, records, lazy sequences, ranges and
other user types remain JavaScript-only and report an unsupported-operation
error inside a macro.

Map and set elements that a macro constructs export in the engine's trie order,
which can differ from the order the legacy engines' array maps keep; inputs
returned unchanged keep their exact original form. `concat` returns a list
where the legacy engines return a lazy sequence that their bridge exports with
an explicit `nil` metadata wrapper. A set literal with non-constant members
rejects equal members with `Duplicate key`, without the member's printed form,
after every member has been evaluated. Reader-distinct constants that lower to
equal values (`#{\a "a"}`, `'#{1 1.0}`, `'{\a 1 "a" 2}`) stay distinct members
or keys in the legacy runtimes (`constantSetFromArray` keeps them positionally);
a native set or map cannot hold two equal members or keys, so such a literal in
a macro fails with `unsupported operation: native literal set with
reader-distinct equal members` (or `... map with reader-distinct equal keys`)
instead of silently losing one. Literals whose members stay distinct are native
sets and maps as usual.
Plain JavaScript functions and ordinary interop such as `Math.max` run in the same task.
Direct JavaScript calls, constructors, method calls, instance checks,
property/index access and property assignment unwrap imported scalar occurrences. For example,
`(defmacro maximum [x] (js/Math.max x 2))` accepts a numeric input. Native
collection, symbol and keyword interop at those boundaries fails with an explicit
unsupported-operation diagnostic; it does not silently convert to JS objects.
Raw `js*` templates retain their literal semantics, including assignable holes:
direct inspection there sees native form values. Use supported Clojure operations
or the engine's explicit `__rt.formValue` intrinsic to inspect them. Core operations outside the implemented
runtime report their operation name and never fall back to another engine.

The compiler's existing Rust core macro implementations supply the bootstrap.
Core macros that need the larger JavaScript bootstrap are unsupported. Variadic
and multi-arity source macros work, including syntax quote/unquote and splicing.
Macros can emit protocol/type declarations for the application; declaration
registration retains preloaded helpers and reports unsupported demand for any
other product. Executing user protocols/types inside a macro remains unsupported.
Lazy sequences, compiler literals, dynamic reader services, compiler build
services and generated-symbol services are not implemented. Native
symbol/keyword component spellings must
round-trip unambiguously, as described in the engine's native form contract.
JavaScript dynamic compilation currently limits each emitted unit to 16,384 UTF-16
code units. Engine syntax/operation errors are reported with the macro's call and
definition locations.

Ordinary source helpers are compiled on demand. A macro parks at a dependency
read while the compiler analyzes the helper using separate task state on the
same heap. Its factory is compiled once into shared executable code. Binding,
factory and initializer outcomes have distinct result identities. Ready reads
are recorded too. The first initializer runs once, then its value is checked
for publication: immutable results are shared; values reaching task-owned
mutation initialize once in each consuming task. The first task keeps its
already initialized value in that case. `macro.helpers.N.*` reports actual
compiled factories, published values, per-task initializations and waits.
Namespace builds also report aggregate `native.helpers.*`, `native.tasks.steps`
and `native.tasks.migrations` counts, including worker activity.

Embedders can also explicitly preload a self-contained helper expression:

```rust
host.preload_native_helper("cljs.user/helper", "(fn [x] (list '+ x 2))")?;
// Source declares the binding, then defines and calls the macro:
// (declare helper)
// (defmacro add-two [x] (helper x))
// (add-two 40)
```

Preload configuration is installed anew in each session. Declaration-bundle
products such as protocols still report unsupported demand. Namespace builds
support `--jobs 1`, `2` and higher through the existing compiler pool. A waiting
worker helps compiler jobs proven macro-free with fresh, bounded nested state.
Jobs that may demand other analyses remain for ordinary worker claims, avoiding
artificial waits on an analyzer frame suspended lower in the same Rust stack.
After a helper
wait, owned VM steps may resume on another worker; analyzer callbacks remain on
their owning thread. The pool test forces two consumers to park, one helper to
compile, unrelated analysis during the wait, and a continuation to migrate without
replaying effects. Namespace output is compared at one, two and four slots.
Evaluator tests also collect while both consumers are parked and check shared
input identity and `finally` execution.

Consumer cancellation withdraws that task's waits without cancelling a shared
producer or another consumer. Build cancellation stops the shared execution
control. Both unwind saved frames, and a failed task's worker view is retired.

Bridge depth/node limits and the evaluator's cancellation, poll budget and
wall-clock deadline bound work. The session keeps imported nodes until teardown,
so repeated imports trade memory retention for stable identity. The runtime
automatically collects from allocation pressure.

### Memory limit

`--macro-memory-limit N` (default 64 MiB; `:macro-memory-limit` in a build) is
the native runtime's one aggregate accounted-byte budget, shared by every task
and worker of the compilation. `BbpRuntime::new` is the one place it reaches
the engine: `heap_policy` maps it to `HeapPolicy::hard_limit_bytes` and builds
the runtime with `Runtime::try_with_heap_policy`. Evaluators spawned later
share that runtime and cannot reset its budget.

What counts is the engine's managed heap (big-boy-pants-js
`docs/architecture.md`, "Byte accounting"): the booted realm (about 1.15 MB
accounted before anything runs; with the evaluator's boot code a macro task is
roughly another 1.5 MB), objects, strings, native forms, saved continuations
and retained native scratch, including garbage until a collection sweeps it.
Executable code, the compiler's own data, Rust stacks and allocator overhead
are outside it, so it is not a process RSS cap. A limit too small for the realm
fails when the runtime is built, with the diagnostic "macro memory limit of N
bytes cannot hold the native macro runtime's realm".

Allocation pressure under the limit triggers collections automatically. An
allocation that would still exceed it fails with the engine's catchable
resource error, the primitive string `heap byte limit exceeded`: a macro that
does not catch it fails its expansion with that message at the call site, and
one that catches it and releases its data continues. The collection a denial
requests runs before the handler releases anything, so the evaluator stores a
command's value only after its next command arrives (nothing allocates between
a macro's return and its task's park), and opens host scopes for imports and
requests only after collecting while `Runtime::byte_starved` reports a denial
not yet relieved. A producer's or initializer's failure message is charged
when it fits and otherwise rooted uncharged, so waiting consumers always
receive their outcome.

Namespace stats report the runtime-wide accounting after each native call as
`macro.gc.N.managed-bytes`, `peak-managed-bytes`, `live-bytes` (after the last
collection), `admitted-bytes` (volume admitted during the call, by every
task) and `byte-limit`, next to the existing collection, pause and live-object
counters.
Legacy-only unit tests still require the `quickjs` feature; the dedicated
`bbp_macros` integration target above runs without it.
