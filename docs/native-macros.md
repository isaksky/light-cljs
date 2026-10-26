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

### Bootstrap macro audit (light-cljs-509, 2026-10-26)

The native branch of `compiler_bootstrap::build_in` keeps an empty bootstrap
function table; `MacroHost::expand_core_javascript` reports `unsupported
operation: bootstrap macro NAME` when a call reaches that table. Namespace
builds first try `native_core_macros`, with the same form construction,
generated names and occurrence provenance as the JavaScript definitions.
The additional bootstrap form builders are selected only for the native
backend. Legacy backends keep expanding these calls in their bootstrap engine,
including its environment conversion, resource charging and macro-free unit
classification.

The audit found 21 bootstrap names without Rust expansion: `as->`, `doto`,
`js-fn?`, `while`, `dotimes`, `memfn`, `amap`, `areduce`, `..`, `lazy-cat`,
`comment`, `with-in-str`, `js-debugger`, `js-comment`, `locking`, `time`,
`unchecked-max`, `unchecked-min`, `hash-map`, `selector` and `simple-benchmark`.
The first 19 now expand in Rust. `cond->`, `cond->>`, `some->` and `some->>`
already did; they are included in the differential table as well.

`native_core_macros::tests::bootstrap_macros_expand_as_their_quickjs_definitions`
compares the added form builders with QuickJS in one table, including metadata,
generated names, source spans and origins. `bootstrap_namespace_macros_match_quickjs`
in `tests/bbp_macros.rs` compiles namespace source with both backends and runs
the emitted JavaScript in Node. Its checked-in `as->` fixture covers repeated
uses, effects exactly once, zero steps, destructuring, nested threading and
shadowing, including an `as->` in a source macro body. The error fixture checks
the exact original step location against QuickJS.

The remaining unimplemented bootstrap algorithms are `selector` (ordered
map construction, metadata and selector destructuring) and `simple-benchmark`
(option maps and legacy `pr-str` of arbitrary forms). These require more than
the direct form builders added here. The existing fallback boundaries also
remain: unscoped text entry points, malformed calls, bridge/depth limits and
valid calls the Rust path deliberately declines, such as large `js-obj` maps
or `defmulti` metadata merges. On the native backend a declined call still
fails explicitly through the empty bootstrap table. Expanding a wrapper such
as `with-in-str` or `lazy-cat` does not add runtime support for operations its
body calls.

### Penpot reproduction (light-cljs-507/509, 2026-10-26)

On macOS arm64, Rust 1.99.1, the native transducer fixtures produce the same
application JavaScript as QuickJS (`--no-default-features --features
bbp-macros,quickjs --test bbp_macros`). The table covers grouping, completion
flushing, nil and empty inputs, repeated and interleaved reductions, callback
and callable-getter effect order, reduced termination and constructor/reducer
arities. The `into` target table checks vector, map entry, list, nil, map and
set targets, including reducer initialization and variadic calls for noneditable
targets; sorted map/set rows establish the legacy contract and check the native
unsupported error. A published composed factory is consumed at one, two and
four slots under seeded scheduling and fixed one-job admission. Cold helper
demands suspend with buffered input, during final-group flushing and during
completion, retaining exact callback effects and independent interleaved
reducers. A reduced cuerdas namespace keeps `make-concat-xform` and
`unsafe-concat` verbatim (its external string-repeat helper is reduced to
JavaScript string repetition).

Rebuild the native-only compiler before running the port; feature-variant test
builds overwrite the same CLI binary. From the repository root, with the existing
Penpot port at `research/penpot-light`:

```sh
CARGO_BUILD_JOBS=3 cargo build --locked --no-default-features --features bbp-macros
target/debug/light-cljs --compile --esm --entry app.render \
  --source-root research/penpot-light/frontend/src \
  --source-root research/penpot-light/common/src \
  --source-root research/penpot-light/vendor \
  --source-root research/penpot-light/generated \
  --source-root research/penpot-light/shims \
  --optimize none --jobs 1 --no-cache --stats \
  --macro-expansions 100000000 --macro-nodes 4000000000 \
  --macro-timeout-ms 3600000 --macro-memory-limit 4000000000 \
  --macro-poll-budget 1000000000000
```

In an isolated worktree with `research/` left untouched, use the same five
source roots under `/Users/admin/src/light-cljs/research/penpot-light`.

After light-cljs-509, both entries pass the former cuerdas `as->` error. At
`54bdea18`, both commands exit 1, with these exact first errors (the read-only
port's canonical source paths):

`--entry app.render`:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/record.cljc:343:1: macro expansion: big-boy-pants-js: RangeError: eval source exceeds 16,384 UTF-16 code units (bytes 15154..17475)
```

`--entry app.main`:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/vendor/promesa/util.cljc:228:1: macro expansion: big-boy-pants-js: SyntaxError: nesting exceeds the limit of 256 units (bytes 5371..6033)
```

The former `core.partitionBy` error is gone: cuerdas' `(partition-by string?)`,
one-argument `filter` and `mapcat`, their `comp`, and three-argument `into` now
run natively, as do the cuerdas `as->` occurrences.

Light-cljs-510 fixes both frontend admission failures. The actual evaluated
inputs were the generated factory expressions
`rt => rt.core.macroReaderFunction((UNIT)(rt))`, 37,597 bytes for
`app.common.record/defrecord` and 8,478 bytes for `promesa.util/with-open` in
the captured failing builds. Both exceeded weighted parser nesting256:
syntax quote emits multiple `sequence`, `seq`, `concat` and `list` calls per
source level, including around generated `for` bodies. The larger factory's
initial compilation failed, and its fallback to dynamic eval masked the
nesting error with eval's size error. The declarations' source text was not
the JavaScript input. Reader registrations were unrelated.

Native macro emission now writes constructor trees of three or more levels
as shallow statements with temporary locals. Explicit worklists preserve
left-to-right evaluation of every occurrence; branches and function bodies
keep their original evaluation sites. These units then use the existing
shared-factory compilation boundary, on its 2 MiB helper stack. Parser
nesting256, the 64 KiB VM stack boundary, dynamic eval's 16,384 UTF-16-unit
cap, cancellation and aggregate heap accounting are unchanged. Application
and legacy macro emission keep their existing output.

At `10c261f0` on macOS arm64, Homebrew Rust/Cargo 1.99.1, fresh native-only
builds with the same five absolute source roots and `--jobs 1 --no-cache`
budgets above pass both former failure sites. Their next first errors are:

`--entry app.render`:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/geom/point.cljc:29:1: macro expansion: while expanding cr/defrecord: /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/geom/point.cljc:29:1: macro app.common.record/defrecord: big-boy-pants-js: Error: unsupported operation: native macro runtime core.gensym; definition /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/record.cljc:343:1 (bytes 830..856)
```

`--entry app.main`:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/logging.cljc:306:24: macro expansion: while expanding str/ffmt: /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/logging.cljc:306:24: macro cuerdas.core/ffmt: big-boy-pants-js: Error: unsupported operation: native macro runtime core.reSeq; definition /Users/admin/src/light-cljs/research/penpot-light/vendor/cuerdas/core.cljc:1056:1 (bytes 11076..11141)
```

Neither entry completes natively yet. These are runtime core-service gaps;
light-cljs-512 covers compiler-owned `gensym`.
`tests/fixtures/native-helper-demand/frontend` retains the two original
macro declarations with their MPL notices and unrelated definitions removed.
`penpot_macro_declarations_fit_the_native_frontend` builds them natively and
compares the application output with QuickJS when enabled.
`deep_native_constructor_trees_preserve_effects_and_provenance` compares
70-level trees, sibling effects, branches, exception/finally order, closures,
raw-JavaScript hygiene and input occurrence identity against QuickJS.

### Native forms

This is a finite-form integration, not backend parity. Lists, vectors, maps,
sets, keywords, symbols, scalar occurrences, metadata, construction, lookup,
basic finite sequence operations, equality, hashing, arithmetic
addition/subtraction and native truth tests have first-party implementations
using native form intrinsics. `seq` returns native lists for map entries
(two-element vectors), set members and strings (UTF-16 code units), a chunked
sequence for a vector (chunks of 32, as a vector's `ChunkedSeq` is), the
realized sequence of a lazy sequence, preserves nonempty lists and cons
sequences, and returns nil for nil or empty supported inputs. Other input
kinds report an unsupported-operation error. Lazy sequences are below ("Lazy
sequences").

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
`__rt.cljZipMap`. Literals and calls with more than 4,096 operands use the
runtime's array constructors instead of one counted intrinsic. Other arities,
core functions used as values (`(map first xs)`, `(apply keyword xs)`) and the
remaining operations call the runtime object, whose entries use the same
intrinsics (it also provides `set`, `vec`, `keys`, `vals`, `keyword`, `symbol`,
`array-map`, `hash-set` and variadic `assoc`/`dissoc`/`conj`/`disj`). A
keyword or symbol whose spelling does not split back into its namespace and
name (`(keyword nil "a/b")`, `(keyword "a/b/c")`) has no native form and fails
explicitly (`(symbol (keyword "" "x"))`, the legacy runtimes' unqualified
`/x`, too). A keyword converts to a symbol through its spelling, as in the
legacy runtimes; invalid operands throw a plain `Error` with the legacy
message.
`rest` and `next` of a vector or list return a list that shares its trie
(light-cljs-502), so walking one with `next` is linear; those of a map, set or
string copy the remaining elements into a new list; of a lazy or chunked
sequence they read through it (see "Lazy sequences"). `(lazy-seq …)`,
`realized?`, `chunked-seq?`, `chunk-first`, `chunk-rest`, `chunk-next`,
`chunk-cons`, `chunk-buffer`, `chunk-append`, `chunk`, `-nth`, `<`, `<=`, `>`
and `>=` of two values and `inc` and `dec` are lowered too, so that the helper
functions `for` and `doseq` expand into need no runtime object.

`comp`, `partial`, `juxt`, `complement`, `constantly`, `identity` and `fnil`
also have native compile-time lowering (light-cljs-506). Their returned
functions run on the task VM, including suspension on a demanded helper;
captured function lists, partial arguments and defaults are native persistent
vectors. They can therefore publish when their captured values can. The core
table supplies equivalent constructors when passed as values. `(comp)` returns
the core identity function, and `(comp f)` and `(partial f)` return `f` itself.
With no arguments, `(partial)` preserves the legacy deferred callable with an
undefined target and three undefined prefix arguments.

One-argument `map`, `filter`, `mapcat` and `partition-by` construct native
transducers (light-cljs-507), and their collection overloads remain lazy.
`comp` combines them, `into` supports zero through three arguments, and
`transduce` and `completing` supply initialization and completion. Three-argument
`into` supplies `conj` to noneditable targets (including map entry, list and
nil), retaining its zero-argument initialization and variadic arities; native
vector, map and set targets use the legacy step/completion-only adapter. Sorted
forms remain unsupported. A `partition-by` factory captures immutable inputs;
each application to a reducer allocates its own task-owned buffer and prior-key
cell. The buffer holds a persistent native vector and clears before downstream
calls, including the final completion flush. A transformed reducer with mutable
cells cannot publish; the factory can publish when its inputs can. Callbacks
execute on the task VM and can park without replaying effects. `mapcat` stops on
downstream termination before reading another flattened element or source input.

`reduced`, `reduced?`, `ensure-reduced`, `unreduced` and dereferencing a reduced
value use an opaque native form with identity equality/hash. Its payload is
traced, charged to the same heap budget and checked for publication. `reduce`
unwraps one marker and stops before advancing its input; `transduce` then calls
the transformed reducer's completion arity once. Buffered completion unwraps a
marker from its final downstream step before calling downstream completion.
Invocation of native keywords, symbols, maps, sets and vectors uses
`__rt.cljInvoke`; collection lookups retain input occurrences. Vector calls
check numeric bounds before truncating an in-range fractional index; the
intrinsics for `nth` and `get` keep their own index contracts. Map seq entries
retain an internal distinction and require exact `0` or `1` call indices.
With one argument, a rejected entry index throws `Index out of bounds`; with
two it returns the supplied fallback unchanged, including nil, undefined and
input occurrences. Entries reject other arities; ordinary vectors ignore
extra arguments. Both native call adapters preserve the argument count in
`__rt.cljInvoke`. An entry's `vec`, `assoc`, `conj` and `with-meta` results
are ordinary vectors. Ordinary JavaScript callable objects retain their `.call`
method and receiver. Emitted calls read
that method before evaluating arguments, including when its getter throws.
`fnil` replaces
nil and undefined occurrences while keeping false, zero and empty strings.
Its short calls preserve the legacy callable's variadic fallback behavior.

`zero?` and `false?` use occurrence-transparent engine predicates, directly or
as core function values; `some?` unwraps its input. `exists?` supports registered
definitions, declarations, host paths and expressions. Probing a declaration
does not demand its unavailable value; a definition is demanded and its
initializer's error propagates. Nil and false definitions exist, an undefined
one does not. The differential fixtures include the unchanged
`MUTUAL_RECURSION` source from `tests/compiler/macros.rs`.

`resolve` already has analyzer-side namespace lookup. An absent binding returns
nil, but a present one needs the Var runtime and reports
`unsupported operation: native macro runtime Var reflection`. Completing this
is larger than a namespace-lookup adapter: native Vars need identity by symbol,
invocation, dereference, equality/hash, metadata, and generation-aware deferred
`:test` metadata without demanding an initializer. `var?` is also unsupported;
the verbatim `DEFONCE_PENDING` fixture in `tests/compiler/macros.rs` therefore
still stops there. This work does not claim that fixture passes natively.

Lowering is what lets helper values publish: a function whose body uses only
lowered operations, literals and other compiler roots captures no task-owned
runtime object, so its initializer's value is published once and every task
shares it (`native.helpers.published`), instead of initializing again in each
consuming task (`native.helpers.per-task`). A function that still reads the
runtime object (a non-lowered core function, a variadic parameter list) stays
per-task. Sorted maps and sets, transients and other host types remain
JavaScript-only and report an unsupported-operation error inside a macro;
protocols, types and records, and lazy sequences, are below.

Map and set elements that a macro constructs export in the engine's trie order,
which can differ from the order the legacy engines' array maps keep; inputs
returned unchanged keep their exact original form. `concat` of collections
none of which is lazy returns a list where the legacy engines return a lazy
sequence that their bridge exports with an explicit `nil` metadata wrapper
(with a lazy or cons collection among them it is lazy and exports as theirs). A set literal with non-constant members
rejects equal members with `Duplicate key`, without the member's printed form,
after every member has been evaluated. Reader-distinct constants that lower to
equal values (`#{\a "a"}`, `'#{1 1.0}`, `'{\a 1 "a" 2}`) stay distinct members
or keys in the legacy runtimes (`constantSetFromArray` keeps them positionally);
a native set or map cannot hold two equal members or keys, so such a literal in
a macro fails with `unsupported operation: native literal set with
reader-distinct equal members` (or `... map with reader-distinct equal keys`)
instead of silently losing one. Literals whose members stay distinct are native
sets and maps as usual. Positional native literals are light-cljs-496.
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
Macros can emit protocol/type declarations for the application, and macro code
can use the compilation's protocols, types and records (see "Protocols and
types"); declaration registration keeps preloaded helpers.
Compile-time atoms (`atom`, `deref`, `swap!`, `reset!`, `compare-and-set!`,
`swap-vals!`, `reset-vals!`) are engine atoms, one per Var across tasks, whose
effects keep their `--jobs 1` order at every `--jobs` (light-cljs-480;
docs/compiler-concurrency.md, "Effect lanes (light-cljs-480)"). A `defonce`
keeps an earlier definition of its Var and fills a declaration, as in the
legacy runtimes.
Compiler literals, dynamic reader services, compiler build
services and generated-symbol services are not implemented. Native
symbol/keyword component spellings must
round-trip unambiguously, as described in the engine's native form contract.
JavaScript dynamic eval limits its input to 16,384 UTF-16 code units. Larger
compiler-generated function factories use shared compilation; native
syntax-quote constructor trees are flattened as described above. Arbitrary
JavaScript nesting still obeys the parser's bound. Engine syntax/operation
errors are reported with the macro's call and definition locations.

Native tasks skip JavaScript reader-namespace registrations before generating
their source. These tables serve the legacy runtime's reader; native macros
receive forms and reader context through the native bridge. The skipped source
therefore cannot hit the eval limit as a project's namespace count grows.
Macro cache identity and legacy reader registration behavior are unchanged.

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

A unit source of at least 512 bytes, such as a source macro's or a
helper's code, is compiled once per runtime into a published factory; every
task that evaluates it calls that factory with its own native runtime, so its
per-task state still lands in that task's globals. A source that fails to
compile, or whose factory does not publish, is evaluated by the task as
before, with the same diagnostic. A helper's producer task replays its
parent's registrations in fixed blocks of 64, so full blocks also compile
once. A producer task is not its worker's engine: registrations it takes
while compiling a helper stay pending for the worker's engine, which installs
them at its next analysis. On this backend a pool slot analyzes a unit proven
macro-free on a view without an engine task, including the fresh slot a
waiting worker's nested help makes. `--stats` reports `native.tasks.contexts`
(booted tasks) and `native.units.compiled`, `shared` and `evaluated`.
`tests/bbp_macros.rs` builds four pool fixtures at one, two, four and eight
slots under seeded scheduler perturbation and compares JavaScript or error,
source maps, warnings and compiled helpers with the one-slot build:
`LIGHT_CLJS_STRESS_SEEDS=12 cargo test --locked --no-default-features --features
bbp-macros --test bbp_macros native_pool_builds` (debug builds only).

A pool slot holds no task between jobs. After a macro unit it parks its task
context with the runtime, together with what its view knows about that task
(how many catalog registrations it installed, the source macros it evaluated
in it and the reader declarations it sent), and its next macro unit, on any
slot, borrows the idle context with the most registrations installed; a slot
boots a new one only when every context is in use. A pool therefore runs as
many worker tasks as it analyzes macro units at once, and each installs a
registration and evaluates a source macro once, not once per slot. A worker
catching up on the catalog installs consecutive root registrations in blocks
of 64 aligned to the catalog, one evaluation per block, so a full block's
factory compiles once for the runtime. `--stats` reports
`native.tasks.lent` (parked) and `native.tasks.borrowed`. Helper values that
do not publish still initialize once in each task that consumes them. Every
helper value in the pool fixtures publishes, so their helper initializations
equal the `--jobs 1` count at every slot count.

A pool worker whose task waits for a result another task produces parks
it: the task's next step is registered with the runtime, and the completion
that makes the task runnable queues that step on the pool from the wakeups
`Runtime::complete` returns, where any worker, the waiting one included while
it helps, runs it; the waiting worker continues with its outcome
(`native.tasks.woken`). The worker's Rust analysis frames stay on its stack
meanwhile, so it frees its thread for other jobs only by helping. A task
whose result arrived before it parked, or that runs outside a pool, polls as
before.

Consumer cancellation withdraws that task's waits without cancelling a shared
producer or another consumer. Build cancellation stops the shared execution
control. Both unwind saved frames, and a failed task's worker view is retired.

Bridge depth/node limits and the evaluator's cancellation, poll budget and
wall-clock deadline bound work. The session keeps imported nodes until teardown,
so repeated imports trade memory retention for stable identity. The runtime
automatically collects from allocation pressure.

### Protocols and types

Macro code can call the compilation's protocol methods and construct its
`deftype`s and `defrecord`s (light-cljs-478). A declaration bundle (a
`defprotocol`, `deftype`, `defrecord`, `extend-type` or `extend-protocol`) is a
helper result like a root: registration names its products, and the first read
of a product, or a dispatch on one of its protocols, demands its unit with
`__rt.need` and runs it in the reading task. Its factory is compiled once and
shared.

Each protocol method's implementations are an engine dispatch table: a
SameValueZero persistent map from a dispatch key to the method, read by
`__rt.cljDispatch` in Rust, so a call reads no JavaScript property and runs no
user equality. The key of a struct is its type's identity; of a native form its
kind (`"keyword"`, `"symbol"`, `"list"`, `"vector"`, `"map"`, `"set"`); of a
primitive `"null"`, `"boolean"`, `"number"` or `"string"`; then `"function"`,
`"array"`, and `"_"` for `default`. A JavaScript object (a mutable type's
instance, an interop object) is resolved by its type, then `object`, then
`default`. `satisfies?` asks the protocol's marker table the same way.

The protocol-extension frontier is preserved: an extension's order is its
bundle's registration order, and a table holds the extensions up to the
frontier the compiler selects for the expansion (`__lightEnterCompilerFrontier`),
installing those bundles first. An expansion before a later `extend-type` sees
the earlier implementation, as in the legacy runtimes. Extensions that macro
code installs outside a bundle are visible at every frontier.

A `deftype` or `defrecord` without `^:mutable`, `^:unsynchronized-mutable`
or `^:volatile-mutable` fields (the type's layout records whether it has one;
methods or none) is a native struct type, made once with its unit's factory (so every task has the same type
identity), and its instances are native structs: immutable, publishable when
their fields are, read in methods with `__rt.structField` and constructed with
`__rt.structNew` (`(T. …)`, `->T`; a constructor call checks for a struct type
and uses `new` otherwise). A record's `__hash` is not a mutable field here: the
engine caches the hash in an atomic, idempotent slot. Records keep the
runtime's semantics: keyword lookup of fields, then the extension map; `assoc`,
`dissoc` (of a field: a plain map with the record's metadata), `conj`,
`count`, `seq` (fields in order, then extension entries), `contains?`, `keys`,
`vals`, `meta`/`with-meta`, `map?` and `record?`; `=` of the same type with
equal fields and extension entries; and the runtime's record hash. A deftype
instance compares and hashes by identity. Interop on a struct (`(.-x s)`) reads
a JavaScript view of its fields. A type with a mutable field, and a raw
`deftype*`, keeps task-owned JavaScript instances, dispatched by their type,
which do not publish. `Object` methods are methods of those instances and of a
struct's interop view, installed under the munged member name interop reads
(`foo-bar` as `foo_bar`); they are tables like protocol methods, so later
extensions (inside a macro too) replace or add them, and a call selects its
overload as the legacy `installMethods` does (one fixed overload takes any
call, else the call's arity, else the variadic one). `instance?` of a struct
type compares the value's type identity; `implements?` sees only the type's
own implementations, including `extend-type` of the native form kinds below,
never host-type or `default` extensions, which `satisfies?` also sees; a protocol
with `:extend-via-metadata` reads the receiver's metadata first. A struct of a
type the task has not declared yet (a value another task published) demands the
type's declaration at its first dispatch or interop. `(R. a b meta ext)` keeps
the record's metadata and extension map. Extensions that macro code runs
outside a bundle come after every bundle's, in the order run.

Not supported: user implementations of core protocols (`IEquiv`, `IHash`,
`ILookup`, `ISeqable`, `ICounted` …) are installed in their tables but the
native core operations (`=`, `hash`, `get`, `seq`, `count`) do not call them;
`specify!`/`extend-instance`, `extend-type` of core types other than the native
form kinds (`Keyword`, `Symbol`, `PersistentVector`, `List`, `EmptyList`,
`PersistentHashSet`, and `PersistentArrayMap` and `PersistentHashMap`, which
are one native map kind, so extending either extends every map), variadic
protocol methods, `map->R` of a record with mutable fields, `getBasis` and
other properties of a struct type, and a macro returning a struct (export fails
with an explicit error). User core-protocol implementations are
light-cljs-497. A helper analyzed on demand can itself expand source macros:
its analysis task materializes each macro declaration once from its emitted
code, with its own function handles and the helper's declaration frontier.
Macro bodies run only for their source occurrences; a parked consumer's
effects are not replayed.

### Lazy sequences

Macro code's lazy sequences are the engine's once-realized cells
(light-cljs-479; big-boy-pants-js `src/collections/lazy.rs`). `(lazy-seq …)`
is `__rt.lazySeq(thunk)`. The runtime object provides `map` (`mapSeq`, of one
or more collections), `filter`, `remove`, `keep`, `mapcat`, `map-indexed`,
`take`, `drop`, `take-while`, `drop-while`, `interleave`, `iterate`, `repeat`,
`range`, `partition` (two and three arguments), `concat`, `doall`, `dorun`,
and over sequences `reduce`, `into`, `every?`, `not-every?`, `some`, `not-any?`, `last`, `butlast`,
`reverse`, `mapv`, `filterv`, with `even?`, `odd?`, `zero?`, `pos?`, `neg?`,
`comp`, `partial`, `complement` and `constantly`. `for` and `doseq` are the
compiler's Rust expansions, over chunked sequences as in ClojureScript (a
vector's seq of more than 32 elements, a finite `range` and `chunk-cons`
sequences are chunked; lists and shorter vectors are not). The clj intrinsics read lazy sequences through: `first`, `rest`,
`next`, `seq`, `nth`, `count`, `=`, `hash`, `concat`, structural map keys and
the rest. `cons` and `conj` keep a lazy sequence as an unrealized tail, and
a `concat` involving one is lazy, realizing one collection at a time.
Equality stops at the first unequal element. Not provided:
`cycle`, `eduction`, `sequence` of a transducer, transducers other
than `map`, `filter`, `mapcat` and `partition-by`, and sorted sequences.
The text-mode compile entry points (`--text`,
`analyze_with_macros`) expand `for`, `doseq`, `when` and `lazy-cat` through the
JavaScript bootstrap, which this backend does not have; namespace builds expand
them in Rust.

A realization runs the thunk once, in the task that first needs the
sequence; a task that needs it while another task realizes it parks (it
holds no worker) until the realization ends, and then reads the value. A
thunk that throws leaves the sequence unrealized, as ClojureScript's
`LazySeq` does: the realizing expression sees the exception, every reader
that was parked on that attempt sees the same failure (the thrown value when
it is shareable, an `Error` with its message otherwise), and a later reader
runs the thunk again. A thunk that needs its own sequence while running it
fails with a catchable `RangeError` where ClojureScript recurses until the
stack overflows, and a sequence that realized itself (an infinite cycle) is a
`RangeError` where ClojureScript loops. A cycle through a helper (a thunk
that demands a helper whose initializer needs the same sequence) is not
detected: the reader waits until the build's `--macro-timeout-ms` deadline.
The compiler wakes a parked reader by polling the sequence's status (at most
20 ms after the realization ends); a scheduler that enqueues the wakeups
the engine returns (`Runtime::take_lazy_wakeups`) would not wait.

A lazy sequence publishes when its thunk does (before realization) or its
value does (after): a helper's `(def xs (map inc v))` or `(def ps (for [x v]
[x x]))` is one shared cell that every task realizes once, while a thunk
that calls a non-lowered core function captures the task's runtime object and
the helper stays per-task. A published sequence must realize a shareable
value; otherwise that realization fails with a `TypeError` naming the path,
for the realizing task and every task parked on it.

A macro's result is realized before it is exported (`__rt.cljRealizeAll`),
so an infinite sequence fails on the memory limit or the deadline rather than
at the bridge's node limit as in the legacy engines. Realized sequences export
the metadata the legacy runtime's sequences have (an explicit `nil` for a
`LazySeq` or `ChunkedCons`, none for a `Cons`, a vector's seq or an
`IntegerRange`), so expansions are the legacy compiler's. An operation over an
unrealized sequence executes again after each cell it realizes; `count` (so
`doall`), the runtime's array helpers and the result's realization realize the
rest of a chain at once and stay linear, while `=` and `hash` of an unrealized
sequence of `k` cells and `n` elements read `O(k * n)` elements. Chunked
sources realize 32 elements a cell.

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

## Seeded 200k namespace build (2026-10-26)

At `26ca6516` (light-cljs-505), the seed-42 generated project builds completely
with the native-only release compiler at `--jobs 1`. It passes the former
`macros/workload/trildorplus.cljc:6:1` failure (`RangeError: eval source exceeds
16,384 UTF-16 code units`); there is no next compiler failure. The generated
fixture has 199,129 nonblank lines, 213,984 physical lines, 9,966,430 source bytes,
and 450 worker namespaces. The generated configuration disables the compiler
cache and gives macros a 2 GiB heap budget and a one-hour deadline.

Reproduce from the repository root (destination must be absent):

```sh
bb benchmarks/generated-project/generate.bb target/generated-native-200k --seed 42 --lines 200k
CARGO_BUILD_JOBS=3 cargo build --locked --release --no-default-features --features bbp-macros --bin light-cljs
cd target/generated-native-200k
../release/light-cljs release bench --jobs 1
node run-light.mjs 42 3
bb oracle.bb 42 3
```

Both executions print `484264`. This was checked on macOS aarch64 with Homebrew
Rust/Cargo 1.99.1 and Babashka 1.12.218. It demonstrates this seeded build and
application result; the unsupported operations listed above remain boundaries
for other macro workloads.
