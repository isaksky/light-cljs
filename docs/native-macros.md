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
keywords, symbols, scalar occurrences, metadata, construction, lookup, basic
finite sequence operations, equality, arithmetic addition/subtraction and native
truth tests have first-party implementations using native form intrinsics.
`seq` returns native lists for vectors, map entries (two-element vectors), and
strings (UTF-16 code units), preserves nonempty lists, and returns nil for nil or
empty supported inputs. Other input kinds report an unsupported-operation error.
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
Lazy sequences, sets, compiler literals, dynamic reader services, compiler build
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
worker helps ready compiler jobs with fresh, bounded nested state. After a helper
wait, owned VM steps may resume on another worker; analyzer callbacks remain on
their owning thread. The pool test forces two consumers to park, one helper to
compile, unrelated analysis during the wait, and a continuation to migrate without
replaying effects. Namespace output is compared at one, two and four slots.
Evaluator tests also collect while both consumers are parked and check shared
input identity and `finally` execution.

Bridge depth/node limits and the evaluator's cancellation, poll budget and
wall-clock deadline bound work. The session keeps imported nodes until teardown,
so repeated imports trade memory retention for stable identity. Aggregate native
heap memory enforcement and automatic collection are not provided by this stage;
the existing `--macro-memory-limit` does not yet cap every native engine allocation.
Legacy-only unit tests still require the `quickjs` feature; the dedicated
`bbp_macros` integration target above runs without it.
