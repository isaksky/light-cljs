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
identity through weak engine handles, with the Datum retained to prevent
address reuse while its occurrence or raw native object is alive. Each imported node
has an occurrence token; export checks the exact heap-and-generation object identity and returns
its original Datum, including exact numeric/character representation and source
provenance. Newly constructed native nodes alone allocate new Datum nodes.
An opaque heap-and-generation identity table preserves the same reuse for raw
native metadata maps, whose engine contract does not wrap metadata in occurrences.
Scalar values still use distinct occurrence wrappers, even when their values agree.
After a collection, the next import prunes dead memo entries and their compiler
provenance. Surviving imported Datums keep source information and conversion
costs for their entire compiler subtree, including metadata and every positional
reader entry. Reimporting a reclaimed occurrence creates a new wrapper; any
occurrence still held by a macro, another task or a saved continuation keeps
its identity and exact original Datum. A raw metadata map can outlive its
wrapper: reimport wraps that same native map while it remains observable.

The `&env` envelope remains a persistent native map. A changed slot is associated
into its previous version; unchanged namespace/reader subtrees use their existing
rooted imports. Every macro call records imported, exported and reused nodes plus
bridge nanoseconds. `MacroHost::native_macro_calls()` exposes those per-call
records; namespace compilation's `--stats` prints `macro.bridge.N.*` lines. The
cache identity distinguishes this backend and its native-form ABI, hashes the
engine source, and includes preloaded helper source.

## Supported boundary

### Explicit compiler literal forms (light-cljs-535)

The native bridge imports and exports explicit compiler descriptors with
immutable native payloads. It preserves original Datum identity, source
occurrences, metadata and exact UTF-16 when an unchanged descriptor or payload
returns. Payloads, metadata and every positional reader entry participate in
fresh and memoized conversion node/depth limits, publication, rooting and GC.
No legacy evaluator or native-only code in `src/js_runtime.js` is involved.

| Compiler literal kind | Native bridge representation |
| --- | --- |
| `#js` array | Explicit descriptor with a positional native vector payload |
| `#js` object | Explicit descriptor with a positional native map payload |
| Regex | Explicit descriptor with an exact pattern string |
| Instant | Explicit descriptor with the reader's epoch-millisecond payload |
| UUID | Explicit descriptor with the reader's UUID string |
| Queue | Explicit descriptor with a native vector payload |
| Big integer | Number occurrence retaining the original exact reader Datum until emission |

Quoted `#js` constructors lower to native intrinsics. Known object keys keep
JavaScript own-property order and last writes. The existing shallow constructor
worklist handles deep literals; wide literals use its bounded fallback rather
than exceeding the engine's 4,096-operand limit. The parser's nesting256 and VM
stack limits are unchanged. The tests cover 60 alternating nested literals,
4,097 array elements and 2,050 object fields. Demanded helpers publish with zero
task-local helper fallback and identical emitted output at jobs 1/2/4/8.

The bounded native reader also constructs `#js` descriptors through the same
constructors, including its exposed tag-table handler. One runtime-owned native
atom retains a canonical pure builtin tag table. Its functions capture no mutable
reader adapter, so a shared `cljs.reader` tag table remains publishable and later
tasks still recognize builtin handlers by identity after the first owner is
dropped and a collection runs. The red/green evaluator regression transfers that
table between tasks and collects the owner before reading another literal. The reader
facade fixture compares exact UTF-16 and property ordering with QuickJS at jobs
1/2/4/8. Invalid reader keys and values retain canonical located diagnostics.

Descriptors remain opaque compiler values, rather than an implementation of all
JavaScript container, Date, UUID and queue operations. Regex core matching obtains
a task-owned RegExp view. Unquoted regex and instant expressions retain their
existing JavaScript evaluation; raw RegExp/Date result export remains unsupported.
JS containers and regexes retain identity equality and hashing. UUIDs compare
their lowercase contents, instants their clipped epoch values, and queues their
elements, with the same hashes as equal sequential collections. Canonical scalar
values are retained separately from the original payload occurrence, including
for a textual instant or uppercase UUID supplied through the explicit engine API.
Normalized object fields retain positional storage at every size; imported
compiler maps retain their legacy traversal view without replacing their payload.
Object predicates observe an own `constructor` field. Literal `str` uses the
represented value's conversion, including array joining and object coercion.
Saved `str` functions retain that conversion under `with-redefs`; literal
printing uses private classification and coercion independently of rebound
`object?`, `string?` and `str` vars. Textual instant descriptors select the
runtime's immutable host zone before canonicalizing local date-time payloads.
Hostless Scope construction selects and freezes UTC; explicit offsets do not
depend on the local zone. Original payloads remain unchanged.
Date's default string and time zone remain engine/host dependent.
The compiler reader still rejects metadata directly on a `#js` source literal;
metadata on surrounding forms, payloads and API-created descriptors is retained.
The EDN reader's `#inst`, `#uuid` and `#queue` value services, and code-profile
namespace resolution, remain separate unsupported boundaries. `#queue` source
syntax expands to an `into` form before macro input; an explicit queue descriptor
can also originate from `PersistentQueue.EMPTY` in a macro unit. Ordinary host
arrays and objects, including frozen objects, still produce located non-form
result errors. They are never inferred to be compiler descriptors.

The source macro literal table compares every compiler literal kind with QuickJS
as incoming syntax, constructed results and macro printing. Other regressions
cover unchanged payload identity/provenance after GC, metadata, memoized resource
limits, lazy payload realization and rejection of task-owned published payloads.
The next small Penpot service gaps, `object?`, `array?`, string `name`, `subs`,
`sequential?`, `ffirst` and `update`, also have native/QuickJS regressions for their
values, effects, metadata and exact strings.
The operation matrix also compares equality, repeated hashing, equal-key lookup
in small and hashed maps, string conversion, printer depth/readability/length,
alternate printer callbacks, predicates and invalid reader keys at jobs 1/2/4/8.
It checks descriptor regex flags, captures, repeated matching and incremental
`re-seq` tails. Date string conversion is compared with a raw Date in each engine
to avoid equating their different host-defined date formats. Malformed explicit
object keys fail with a located macro diagnostic before analyzer lowering.

At `13207bd2`, rebased onto `1888f327`, a fresh native-only debug compiler on macOS
arm64 with Homebrew Rust/Cargo 1.99.1 passes the former `rumext/v2.cljs:906:3`
literal boundary. Both documented Penpot entries were rerun with all five absolute
read-only roots, `--jobs 1 --no-cache` and every budget below. Both exit 1 with
this identical first diagnostic:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/main/ui/shapes/text/fo_text.cljs:17:1: macro expansion: while expanding mf/defc: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/main/ui/shapes/text/fo_text.cljs:17:1: macro rumext.v2/defc: big-boy-pants-js: Error: unsupported operation: native macro runtime core.isList; definition /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:334:1 (bytes 522..819)
```

This is a sequence type-fidelity boundary, tracked by light-cljs-546. Native
`FormKind::List` also represents map and string sequence views. QuickJS's `list?`
distinguishes those from lists and plain Cons values without realizing a lazy
sequence. A reduced real-source macro prints
`[true true true false false false false]` with QuickJS for empty/nonempty lists,
plain Cons, map/string/vector seqs and an unforced lazy value; native execution
fails explicitly at `core.isList`. Neither Penpot entry completes yet, and
`research/` was left unchanged.

On that final source/base, all 174 native evaluator/macro/reader/Proxy tests pass
with `LIGHT_CLJS_STRESS_SEEDS=12`, as do all 2,152 engine tests and seven doctests.
The 12 reader tests also pass with `bbp-macros,quickjs`. The complete default V8
and QuickJS-only runs report 2,131 and 2,054 passes respectively, each with the
same existing `playground_crate_checks_for_wasm32` environment failure: Cargo
assigns the excluded nested playground package to the ancestor main checkout's
workspace. Their library, source and execution checks pass. These complete runs
are reused: later emitter changes run only in native mode, and the final reader
fix and engine-only rebase do not affect their default/legacy paths.

The default-feature V8 compiler was rebuilt with `LIGHT_CLJS_V8_CACHE=require`.
For the seed-42, 200k-line project generated with
`bb benchmarks/generated-project/generate.bb target/g200k --seed 42 --lines 200k`,
ten quiet, interleaved pairs timed `release bench --no-cache` using the original
`ed4504ed` release binary and the final implementation. Median wall time was
7.055 s before and 7.085 s after (1.0043x, +0.43%); ranges were 7.00–7.25 s and
7.03–7.26 s. The difference is within run-to-run variation, with no material V8
slowdown. A pair interrupted by peer work was discarded before this comparison.

Root default/QuickJS Clippy passes strictly. Native and engine strict Clippy
retain the nine established engine diagnostics; allowing only `deprecated`,
`clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices` passes the engine
and the native library, CLI and integration targets. Engine-free library/CLI
Clippy passes strictly. Engine-free all-target lint cannot compile unchanged
benchmark support that calls legacy-only evaluator methods. Compiler and engine
format checks cover the final content.

Review fixes at `8cd5e55e`, rebased onto `730a770f`, extend the differential
matrix through saved `str` calls under `with-redefs`. All ten literal tests and
14 reader/Proxy tests pass with native and QuickJS enabled. All 177 final native
evaluator/macro/reader/Proxy tests pass with `LIGHT_CLJS_STRESS_SEEDS=12`; the
matrix compares exact emitted output at jobs 1/2/4/8. Both final native Penpot
commands repeat the exact `core.isList` diagnostic above, with every documented
budget and all five read-only roots. They take 227.39 s (`app.main`) and
221.08 s (`app.render`); the existing light-cljs-546 remains the next boundary.

The rebased engine passes all 2,158 tests and seven doctests. Those results remain
valid after the compiler-only coercion fix because engine sources, dependencies
and corpus inputs are unchanged. Fresh full V8 and QuickJS runs pass 2,131/2,132
and 2,054/2,055 tests respectively. Both fail only
`playground_crate_checks_for_wasm32`: the pinned wasm Cargo resolves the nested
playground crate into `/Users/admin/src/light-cljs/Cargo.toml`'s workspace.
The temporary read-only ClojureScript symlink uses revision
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` and is removed after verification.
These runs use macOS arm64, Homebrew Rust/Cargo 1.99.1, Node 26.12.1, three build
jobs, three native/engine test workers and eight legacy nextest slots.

Fresh default/QuickJS all-target and engine-free library/CLI Clippy pass strictly.
Engine-free all-target lint retains the same missing legacy evaluator methods in
unchanged `benches/support/levels.rs`. Strict engine Clippy retains the same nine
diagnostics; allowing only the three categories listed above passes the engine
and final native targets. Both Rust format checks pass, with no source lint
suppressions and no native-only changes to `src/js_runtime.js`.

Final integration at `c2a22059` is rebased onto `af0c13dc`. Object payloads are
validated on native import as well as export, so malformed public Datum inputs
cannot enter the identity cache and bypass fresh export checks. Numeric,
collection and qualified keys, odd map payloads and non-map payloads have
regressions. The integrated native suite passes all 179 tests with twelve stress
seeds; the combined native/QuickJS evaluator and literal selection passes all
68 tests, including exact one/two/four/eight-slot comparisons. The engine passes
2,161 tests and seven doctests. The subsequent test-only rebase adds one passing
namespace-diagnostic regression; its strengthened cached-factory binding test
passes with native-only, QuickJS and V8. Unchanged engine and legacy suite results
are reused across that rebase. The legacy runtime, emitter and dependency inputs
are unchanged by these integrations. Both final Penpot entries repeat the exact
`core.isList` diagnostic above, taking 225.93 s and 221.18 s respectively.

The final default-feature release binary was rebuilt with
`LIGHT_CLJS_V8_CACHE=require` and compared with the saved `967e2a57` release
binary on the same seed-42 200k fixture. Ten clean, interleaved pairs of
`release bench --no-cache` have median wall times of 7.565 s before and 7.480 s
after (0.9888x, -1.12%); ranges are 7.18–7.91 s and 7.31–7.76 s. One pair
interrupted by other builds/tests was excluded. Measurements wait for a quiet
window and detect interference throughout each run. There is no material V8
slowdown. The final documentation-only commit reuses these source checks.

The second literal review fixes at `9269dc8f` pass the operation differential
with both `TZ=UTC` and `TZ=America/Denver`, including rebound printing predicates
and offsetless/offset `#inst` equality and hashing at jobs 1/2/4/8. The new
engine regressions reproduce both host-selected and hostless time-zone ordering
bugs before the fix. All 179 native tests pass with twelve stress seeds, as do
all 2,164 engine tests and seven doctests. Engine strict Clippy retains the same
nine diagnostics; isolating only the established categories passes engine and
native targets. The subsequent public API comment changes no executable code.

Fresh full default V8 and QuickJS-only runs on that content pass 2,131/2,132
and 2,054/2,055 tests respectively. Both reproduce only the documented playground
workspace error. Their all-target Clippy runs pass strictly. Engine-free
library/CLI Clippy passes strictly; its all-target run cannot compile unchanged
legacy-dependent benchmark and evaluator fixtures. Both format checks pass.
Fresh native-only Penpot entries again reach the exact `core.isList` diagnostic
above: 215.00 s for `app.main`, 209.39 s for `app.render`, with jobs 1, no cache,
all five absolute read-only roots and every documented macro budget. The next
boundary remains the existing light-cljs-546.

Rebased onto `ba1bb3bd` at `7981ff45`, both TZ operation matrices still pass.
The new cached-core-write regression passes with native, V8 and QuickJS, and
the expanded twelve-seed native selection reports 194 passes and twelve
bootstrap-oracle failures. All twelve failure names and diagnostics match an
unmodified `ba1bb3bd` build; these tests require a legacy bootstrap engine.
With `bbp-macros,quickjs`, all fifteen bootstrap-oracle tests pass. Native,
default, QuickJS and engine-free library/CLI lint checks retain their results,
with only the established engine lint categories allowed for native targets.
Full engine and legacy suite results above are reused: the rebase changes
native-only paths and tests; engine sources, dependency inputs and the legacy
emission path are unchanged. Both format checks cover the rebased sources.

The final V8 gate compares the saved `30fa1ecd` default-feature release binary
with the rebuilt `7981ff45` executable content on the same seed-42, 200k-line
generated project using `release bench --no-cache`. Ten clean interleaved pairs
across quiet windows have median wall times of 7.305 s before and 7.250 s after
(0.99247x, -0.75%); ranges are 7.25–7.98 s and 7.16–7.82 s. Pairs interrupted
by other builds or tests are excluded. There is no V8 slowdown.
Both final native Penpot commands repeat the exact `core.isList` diagnostic
above, taking 181.19 s (`app.main`) and 183.45 s (`app.render`), with jobs 1,
no cache, all five absolute roots and every budget. The existing light-cljs-546
remains the next boundary. Final documentation-only commits reuse these checks.

### Key-value reduction (light-cljs-518)

Native `reduce-kv` invokes `(rf accumulator key value)` on the resumable task
VM, including cold callback helper demands. Nil and empty inputs return the
original accumulator; each callback result is tested for `reduced?`, and one
marker is unwrapped before returning without reading the next sequence item.
No transient is created and the input collection and accumulator metadata stay
intact. Explicit type implementations precede built-in `IKVReduce` and
`IMapEntry` implementations, which precede object/default extensions. Other
seqable inputs use the public `IMapEntry` key/value methods, with their
missing-method errors and callback getter/receiver evaluation order preserved.
Reduction traverses custom `ISeqable`/`ISeq` values through their protocols,
using a direct `INext` implementation when present and `ISeq/-rest` otherwise,
including the first advance of a two-argument `reduce`. Object/default
`INext` extensions do not participate in that advance. Direct `-kv-reduce`
calls require an `IKVReduce` implementation and reject nil, lists, arrays and
map entries unless an extension supplies one.

Vectors supply zero-based indices. Hash maps use legacy five-bit hash-path
order, including the nil key; array maps and positional literal maps retain
their storage order and duplicate lowered keys. Records visit declared fields
first and their extension map next, including positional extensions and
mutable record views. The existing native imported-reader bridge still keeps
source positions where the legacy bridge promotes a large input to a hash
map; the differential table pins both results explicitly.

Default-comparator `sorted-map` now constructs a persistent native map that
keeps sorted traversal through `assoc`, `dissoc`, metadata, publication and GC,
and remains noneditable. Supported comparable native keys are nil, numbers,
booleans, exact UTF-16 strings, keywords, symbols and vectors of comparable
keys. Custom sorted comparators, host-object keys and sorted sets remain
unsupported. Incomparable keys raise catchable `Error`s with the legacy
operand-specific messages, including during construction, lookup, updates
and equality; catch and finally both run. Map equality uses the destination
map's comparator and preserves the source map's value-comparison direction.
Comparison messages format native collection operands in rooted, bounded VM
steps. Traversal storage, ordered map snapshots, exact UTF-16 output and
complete UTF-8 host diagnostics are admitted against the aggregate byte
budget before allocation; string length is checked before output growth.
Lone surrogates and readable control-character escapes survive in the
JavaScript message. Host diagnostic copies and rethrows share their buffer
and its charge. A small shared collection graph cannot bypass the budget by
expanding into an exponentially larger diagnostic.
Full result realization rebuilds sorted maps containing lazy values or
metadata while preserving sorted updates, key identity and noneditability.
The source macro table compares these collection kinds, callback
effects, errors, identity and early termination against QuickJS. A reduced
rumext property-filter fixture demands cold helpers at one, two and four
slots, retaining every callback effect exactly once. Penpot's next small
service gaps, `qualified-keyword?`, `true?` and `vary-meta`, are also provided
natively.

At `076a1c21`, based on `d469a30b`, the native-only debug compiler was rebuilt
on macOS arm64 with Homebrew Rust/Cargo 1.99.1. Both Penpot entries were rerun
with the five absolute read-only roots, `--jobs 1 --no-cache` and every budget
from the reproduction below. Both pass the former `core.reduceKv` boundary
and the three small core services above, then exit 1 with this exact first
diagnostic, identical for `--entry app.render` and `--entry app.main`:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:893:4: macro expansion: while expanding fnc: /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:893:4: macro rumext.v2/fnc: big-boy-pants-js: TypeError: undefined is not a function; definition /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:320:1 (bytes 27920..28218)
```

A temporary stack diagnostic, removed after the probe, locates the failing
call in the demanded `rumext.v2.compiler/compile` helper: its `binding` of
`*handlers*` calls the absent native `globalThis.__lightSaveCompilerBinding`
before installing the new value. This is a compiler binding-service gap,
separate from core collection operations, tracked by light-cljs-520. Its
four-form reduced reproduction also fails natively, while the QuickJS host
compiles it and Node prints `{:bound true}`. Neither entry completes, and
`research/` was left untouched.

The native integration suites pass 77 macro, ten reader and two Proxy tests;
37 native evaluator unit tests also pass. The full engine run at `7054ab76`
passes 2,129 tests and seven doctests, including the byte-limit, string-limit,
yield/GC and retained-host-diagnostic regressions. The `bbp-macros,quickjs` run
passes all 333 compiler unit, 79 macro, ten reader and two Proxy tests (one
unrelated unit test is ignored).
The full native/combined suites and Penpot runs were checked at `076a1c21`.
The only subsequent production change, at `7054ab76`, increases the map
snapshot reservation to cover a full B-tree node even for one entry. Those
suite/Penpot results are reused; the expanded native/QuickJS table and full
engine suites were rerun after that correction.
Strict engine Clippy retains the nine documented baseline diagnostics;
allowing only those categories passes for the engine and the native compiler,
CLI and integration-test targets. Strict Clippy passes for the available
QuickJS-only targets and the engine-free compiler library/CLI. Both format
checks and the Node runtime suite pass. A prior broader native-only compiler
unit run passes 258 tests and fails 27 tests requiring legacy
bootstrap, compiler services or QuickJS readers. Restoring the original
`a8edeeb5` production sources temporarily reproduces all 27 failure signatures
under the same toolchain and feature flags (ignoring only thread/source IDs).
Those sources were restored to the implementation before further validation.
The final `native_` unit filter reports 12 passes and 12 bootstrap-macro
failures. All 12 test names and exact `unsupported operation: bootstrap macro`
messages match that recorded baseline; the bootstrap, macro-host, evaluator
and test sources on those failure paths remain unchanged from `a8edeeb5`.
The QuickJS compiler integration target cannot build in this worktree: eight
`include_str!` inputs require the absent `research/clojurescript` checkout.
No research symlink was created, and the unavailable default V8 suite remains
the orchestrator's check.

The other QuickJS-only compiler, CLI and integration targets report 822 passed,
52 ignored and two environment failures. `playground_crate_checks_for_wasm32`
fails before compilation because Cargo assigns the excluded playground crate
to the ancestor main checkout's workspace. The watch test
`a_failed_restart_waits_for_work` cannot execute `ps` in this sandbox
(`PermissionDenied: Operation not permitted`). Their test sources, build
script and manifests are unchanged from `a8edeeb5`; neither failure reaches
the native services changed here.

### Printing and namespace services (light-cljs-513)

Native printers cover nil, booleans, Numbers (including negative
zero and nonfinite values), exact UTF-16 strings and characters, named values,
lists, vectors, maps, sets, lazy/cons sequences, immutable and mutable types,
records, Reduced markers, shared atoms, regexes and Namespace values. The
printing table in `tests/bbp_macros.rs` compares real source macros with
QuickJS, including the existing printing-fixture rows, metadata, print limits,
readability, namespace maps, explicit options, custom printer/writer protocols,
errors and restoration after a thrown lazy printer. Plain host objects, arrays
and ordinary functions still fail explicitly. There is no legacy fallback.

The review regressions compare `str`, `pr-str` and `print-str` on immutable
and mutable records/types, both with and without `Object/toString`. Record
`str` honors that method (or the default `[object Object]`), while readable
printing uses record fields and extension maps. Mutable records also retain
print limits, metadata and `print-meta?`. The compiler supplies one JavaScript
member for every declared field, preserving collisions after munging, reserved
names and internal record slots without depending on property enumeration.
Custom printers receive a real `StringBufferWriter`; `-write` returns its
appendable buffer, including when collection `str` invokes the printer.
String helpers keep the original output
buffer even if a custom printer replaces, clears or makes the writer's `sb`
slot unreadable; subsequent writes still use the current slot, as in QuickJS.

`formPrintItems` preserves positional traversal and restores legacy five-bit
hash traversal, including nil-first traversal, independently of the native
lookup trie's hash spreading. Array-map constructors, small sets, empty map
literals, `assoc` on nil, and record extension maps retain insertion order.
Array maps constructed above eight entries also retain that order; adding a
new key promotes them, as does a ninth distinct member in a small set. Record
printing keeps declared fields first and traverses its extension map with the
same positional/hash rules. The differential table includes reader-distinct
positional literals, removal's trailing nil positions, promotion, duplicate
constructor keys and record extensions. Imported duplicate reader values retain
light-cljs-496's documented difference from the legacy input bridge below;
the duplicate-literal comparisons construct equal stored values inside macros.

The macro wrapper binds `*ns*` from the existing `&env` namespace name and
restores it on success or failure. Namespace name slots retain their original
symbol occurrences and metadata. Initializer demands publish immutable
namespace snapshots in dependency kind 7; a namespace read joins the existing
source-order lane and selects its earliest direct or inherited caller. Explicit
bindings inside initializers take precedence along that ancestry. Snapshots
also distinguish Namespace objects from explicitly bound nil or symbols. Task
migration does not consult worker thread-local namespace state. The namespace
snapshot distinguishes registry entries from explicitly constructed Namespace
values, retaining the latter's name occurrence and task-local identity without
interning it. The cache uses weak snapshot keys: observable snapshots retain
their Namespace identity, and discarded snapshots and their Namespace objects
are collectable. A GC regression checks both lifetimes. Differential coverage
includes distinct explicit objects with the same registry name and different
metadata, and a non-symbol name slot, alongside `create-ns`/`find-ns`.
The namespace fixture matches QuickJS's emitted program and diagnostic
message/provenance at
jobs 1/2/4, including seeded scheduling, a held earlier caller, nested helper
compilation, nested initializers, metadata, exceptions and task reuse. Engine
stack frames and the native error prefix are compared separately.

The next `app.main` run exposed `core.partitionAll` in the logging pipeline.
Native `partition-all` now supports its lazy two/three-argument collection
overloads and one-argument buffered transducer, including a final partial group,
repeated use and reduced termination. One-argument `remove` also supplies the
transducer that pipeline uses. Both inline factories lower to shareable
closures; core-as-value adapters have equivalent behavior. The core-service
table compares their results, errors and lazy effects against QuickJS. A real
source fixture publishes the partition/removal composition and its downstream
helper, then suspends and resumes buffered reductions at jobs 1/2/4.

Verification on macOS arm64, Homebrew Rust/Cargo 1.99.1 and Node 26.12.1:
`cargo test --locked --no-default-features --features bbp-macros --test
bbp_macros` passes all 75 tests, including the expanded printing table and
namespace scheduling fixture; all 37 native evaluator tests also pass.
With `bbp-macros,quickjs`, the complete macro
integration binary passes 77 tests and the library passes 333 (one ignored).
Both feature configurations' CLI unit test passes. These results cover the
second review fixes at `15efcf0e`: declared record member mappings, independent
output buffer capture, weak namespace snapshot keys and their regressions.
The engine's `--all-targets` run passes 2,113 tests and `--doc` passes seven;
its code is unchanged after `1d6c19c8`, so those results cover the final tree.
The small-set change preserves the embedding API's existing rejection of
unrealized structural keys, checked by the unchanged lazy-sequence regression.

The QuickJS-only library/binary and available integration targets run with
`cargo nextest run --locked --no-default-features --features quickjs`, eight
test workers and three Cargo build jobs: 822 pass, two fail and 52 remain
normally ignored. The failures retain the established environment signatures:
`cli::playground_crate_checks_for_wasm32` discovers the main checkout's
workspace, and `dev_watch::a_failed_restart_waits_for_work` cannot spawn `ps`
under this sandbox. The compiler integration target cannot build because eight
pinned `research/clojurescript` includes are absent; `research/` was left
untouched. The wider native-only library run has the same 27 unsupported
legacy bootstrap/host-service failures reproduced on untouched `26512b2f`
(258 passing here, 256 there). The second review-fix run compares all 27 failure
signatures with that baseline, removing only thread and source allocation IDs;
they match exactly. Its native-only CLI unit test also passes.

Direct JavaScript runtime checks and both formatter checks pass. QuickJS-only
and engine-free library/binary Clippy pass strictly. Strict engine Clippy
retains eight library/nine all-target diagnostics. Allowing only `deprecated`,
`clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices` passes engine
all-target and native/native-plus-QuickJS library/binary/macro-test checks.
Full compiler all-target lint remains subject to the missing includes (and
QuickJS-specific tests in the native-only configuration). Engine-free
all-target lint also reaches unchanged benchmark calls to legacy evaluator
methods absent in that configuration; its library/binary lint passes strictly.
V8 validation is reserved for the orchestrator because its machine cache is
inaccessible here.

At `15efcf0e` (repeating `3c08827e` and `848be441`), on macOS arm64,
Homebrew Rust/Cargo 1.99.1, using the Penpot reproduction below with five
absolute source roots, jobs 1, no cache and the documented budgets, both
entries pass the original printing/namespace errors
and the additional logging transducer gap. Both exit 1 at the engine Proxy
boundary already filed as light-cljs-514. The exact next errors are:

`--entry app.render`:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/geom/matrix.cljc:251:4: macro expansion: while expanding app.common.data.macros/assert!: /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/geom/matrix.cljc:251:4: macro app.common.data.macros/assert!: big-boy-pants-js: TypeError: /Users/admin/src/light-cljs/research/penpot-light/vendor/cognitect/transit.cljs:84:1: extension: unsupported Proxy trap: has; definition /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/data/macros.cljc:105:1 (bytes 6545..6573)
```

`--entry app.main`:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/config.cljs:236:3: macro expansion: while expanding app.common.data.macros/assert!: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/config.cljs:236:3: macro app.common.data.macros/assert!: big-boy-pants-js: TypeError: /Users/admin/src/light-cljs/research/penpot-light/vendor/cognitect/transit.cljs:84:1: extension: unsupported Proxy trap: has; definition /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/data/macros.cljc:105:1 (bytes 8211..8260)
```

The final `app.main` failure supersedes the earlier uncommitted-build success;
neither entry completes on this tree. No default backend, legacy runtime or
read-only research file was changed.

### Proxy presence at foreign printer extensions (light-cljs-514)

The emitted package helper checks `name in Object(registry)` before retrieving
an imported package. `MacroHost`'s opaque package registry supplies a `has` trap
that returns true. The engine previously rejected this trap during Transit
printer extension demand. Presence checks now execute the trap through traced
native continuations, including inherited Proxies, trap getters, key conversion,
dependency waits and target descriptor/extensibility invariants. Array methods,
descriptor readers, locale lists and Error cause options use the same path.

With the native printers integrated, both Penpot entries next exposed `get`
and then `apply` on the same opaque registry/function path. These traps now
use resumable native continuations too: reads preserve the original receiver
and enforce fixed-property invariants after the trap; calls create a fresh
argument array and preserve `this` through bound/Reflect forwarding and native
callbacks. The first-party semantic fixtures
`crates/big-boy-pants-js/tests/fixtures/js/proxy_has.js` and `proxy_get.js` run
unchanged in Node/V8 and the native engine. `tests/proxy_has.rs` checks exact
callback order across dependency waits, collection and migration on 64 KiB
worker stacks, including every supported descriptor reader and extensibility
callbacks during enumeration and reflection.

JSON array serialization reads the original Proxy receiver's `length` and
converts it through resumable ToLength after array classification. Transparent
Proxies preserve conversion lookup mode for ordinary and bound functions.
`proxy_builtin_reads.js` runs unchanged in Node/V8 and checks exact read order,
virtual lengths and exceptions for JSON, Array.from/concat/join/sort/flat,
spread, Object.assign and string conversion. The engine repeats the table
with 113 dependency waits, collection and migration; the promise regression
asserts that a throwing `then` read rejects with its original reason.

`tests/bbp_proxy_has.rs` compiles a source macro printing `[nil]`, which demands
the TransitUUID `IUUID`/`IPrintWithWriter` extension through the opaque package
registry. It produces `"[nil]"`; the same source without a supplied package
fails explicitly. A separate source fixture originally preserved the unsupported
native printer extension on `js/Promise`; after the reader work below adds
Promise dispatch, that rejection fixture uses `js/Date`. Printer dispatch is
not weakened and no legacy runtime is used for native expansion.

On the implementation rebased on `964782f7`, both documented Penpot entries
were rebuilt and rerun natively with the five absolute read-only source roots,
`--jobs 1 --no-cache` and every budget in the reproduction below. Both pass the
Transit extension's `has`, `get` and `apply` errors. Their next errors are:

`--entry app.render`:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/types/modifiers.cljc:275:4: macro expansion: while expanding cuerdas.core/ffmt: /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/types/modifiers.cljc:275:4: macro cuerdas.core/ffmt: big-boy-pants-js: Error: unsupported operation: native macro runtime core.readerReadString; definition /Users/admin/src/light-cljs/research/penpot-light/vendor/cuerdas/core.cljc:1056:1 (bytes 8736..8834)
```

`--entry app.main`:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/config.cljs:236:3: macro expansion: while expanding app.common.data.macros/assert!: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/config.cljs:236:3: macro app.common.data.macros/assert!: big-boy-pants-js: Error: /Users/admin/src/light-cljs/research/penpot-light/vendor/promesa/impl.cljc:380:4: extension: unsupported operation: native protocol extension of [object Function]; definition /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/data/macros.cljc:105:1 (bytes 8211..8260)
```

At this checkpoint neither Penpot entry completes: the reader service and the
Promise extension remain boundaries. Light-cljs-516 below supersedes those two
errors. The read-only research tree and default backend are unchanged.

Validation at `b438d3fa`, macOS arm64, Homebrew Rust/Cargo 1.99.1 and Node
26.12.1: a single all-target engine run passes 2,123 tests, and seven doctests
pass. Both documented Penpot commands were rerun on this implementation and
retain the exact next errors above. `cargo test --locked --no-default-features
--features bbp-macros --test bbp_macros --test bbp_proxy_has` passes 75 macro
tests and two source regressions; the native evaluator's 37 unit tests pass. With
`--features bbp-macros,quickjs`, the full library passes 333 tests (one ignored),
the CLI unit test passes, and the macro/source targets pass 77 and two tests.

The QuickJS-only library/binaries and available integration targets pass
822 tests, with 52 normally skipped and the same two environment failures:
the playground wasm crate discovers the main checkout's workspace, and
`dev_watch::a_failed_restart_waits_for_work` cannot spawn `ps` (`PermissionDenied`,
`Operation not permitted`). The compiler integration target and full compiler
all-target lint remain unavailable because eight pinned research includes are
absent; this run leaves `research/` untouched. Both format checks pass. Strict
engine Clippy retains the nine baseline diagnostics in `deprecated`,
`clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices`; allowing only
those categories passes all engine targets and native/native-plus-QuickJS
library, binary and macro/source-test lint. QuickJS-only library/binary Clippy
passes strictly.

V8 compiler validation remains with the orchestrator because its machine cache
is inaccessible in this sandbox.

### Native reader service (light-cljs-516, 2026-10-26)

Cuerdas `interpolate-ffmt` calls `cljs.reader/read-string` on a placeholder's
numeric suffix, such as `"2"` in `%2`. The reader facade now calls the shared
bounded parser with native constructors and native inspection. The `reader`
unit in `src/js_runtime.js` is the parser used by legacy engines and emitted
applications; `src/bbp_reader.js` supplies its native adapters. Native task
assembly extracts that parser and adapts its scalar occurrence and callback
boundaries, checking every source substitution. These adaptations live in
`src/bbp_evaluator.rs`; the emitted runtime is byte-for-byte identical to
`d83372e5`, avoiding application size growth. Only the parser unit is embedded
in the native task, without loading a legacy runtime or calling an evaluator
fallback. The source compiler's code reader remains
separate: the facade reads data and rejects code-only syntax.

The native service supports strings and escapes, numbers, characters, names,
collections, metadata, namespaced maps, discard forms, synchronous streams,
`:eof`, custom `:readers` and `:default` functions, including Clojure callable
values. It keeps the canonical limits of 1,048,576 input UTF-16 units,
100,000 parser nodes and 32 active depth levels. Reader error source, line,
column, offset, category and message follow the canonical parser. Reader-created
forms keep the legacy macro call-site attribution; supplied EOF values and tag
results keep their original occurrences and metadata. Invalid metadata targets
produce catchable reader errors before calling a native intrinsic.

Reader-created maps and sets retain their native persistent storage with a
reader traversal flag, so native seqs and returned compiler forms agree with
QuickJS. Persistent updates and metadata copies preserve the flag; hashed
collections stay hashed after shrinking below the array threshold or to zero,
and later additions still follow hash traversal. Fresh maps
and sets exported by the native bridge use `Scope::form_print_items`, sharing
the existing printer traversal without changing native lookup or the general
native seq contract. Coverage includes collection thresholds, nil-first hash
traversal, updates and large custom-tag results, alongside input-scope restoration.

Tag callbacks run in the task's VM and can suspend for compiler helpers without
replaying earlier effects. The facade's published initial tag table contains
shareable entry points. The builtin EDN reader services for
`#inst`, `#uuid` and `#queue` remain unsupported: these fail explicitly,
including when their builtin handlers come from another task's table. Native
`#js` values and cross-task builtin identity are now supported by the explicit
compiler literal forms above. Custom
handlers for those names can return supported native data. Code-profile
`cljs.core/read-string` and its namespace qualification remain unsupported;
the new services expose the data profiles used by `cljs.reader` and
`clojure.edn`. Native `merge` supplies the facade's option-map operations.
Legacy engines and emitted applications retain their original adapters and
builtin literals.

Native extension tables also recognize `js/Promise`, so promesa's
`IPrintWithWriter` extension installs and participates in real dispatch.
Promise-specific implementations precede object/default fallbacks and use the
existing declaration frontier and source order. Promise `Object` methods have
no native installation path and fail explicitly before modifying any extension
table. The regression compares
printing, a user protocol method, `satisfies?` and `implements?` with QuickJS;
other foreign constructors retain their explicit unsupported boundary.

`tests/bbp_reader.rs` has one differential table for forms, nested metadata,
exact emitted string units, compiler expression source positions, options,
custom and callable tags, reader conditionals' rejection, limits and errors.
Its source fixtures cover the actual reader facade and a callback that demands
a helper at jobs 1/2/4, observing its before/after effects exactly once. A reduced
Cuerdas fixture (BSD-2-Clause, `funcool/cuerdas` 2026.415, recorded in
`THIRD_PARTY_NOTICES.md`) retains `concat`, `interpolate-ffmt` and `ffmt` verbatim, with
the existing string-repeat reduction; indexed, sequential and escaped-percent
formats match QuickJS. Separate tests preserve supplied occurrence identity
and check the unsupported literal boundaries. The differential table also
covers shrinking hash collections, nil `merge` arguments, record callback
lookup/metadata/identity, occurrence-wrapped four-argument EOF flags, chunked
CR/LF lines, close errors, timestamp fields/milliseconds, and tag/default
registration return values and effects. The facade fixture checks nil, empty
and EOF-only option maps for both string and stream reading.

At `f5b0d139`, based on `d83372e5` including landed light-cljs-514, both Penpot
entries were rebuilt and run natively with the five absolute read-only source roots,
`--jobs 1 --no-cache`, `--optimize none --esm --stats` and every budget in the
reproduction below. Both pass the former reader and Promise extension errors.
Both `--entry app.render` and `--entry app.main` exit 1 with this exact next
diagnostic, tracked by light-cljs-518:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:893:4: macro expansion: while expanding fnc: /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:893:4: macro rumext.v2/fnc: big-boy-pants-js: Error: unsupported operation: native macro runtime core.reduceKv; definition /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:320:1 (bytes 27920..28218)
```

Neither Penpot entry completes, and `research/` was left untouched.

Verification on macOS arm64, Homebrew Rust/Cargo 1.99.1 and Node 26.12.1:

- Native-only nextest passes 121 targeted tests: all 75 macro integration
  tests, seven reader/Promise tests, two Proxy source tests and 37 native
  evaluator tests. This run covers `f5b0d139`.
- With `bbp-macros,quickjs`, library, binary and those integration targets pass
  420 tests with one normally ignored test, including the expanded collection
  table and its explicit `undefined` registration assertion.
- The QuickJS-only library, binaries and every available integration target
  other than `compiler` run 824 tests: 822 pass, 52 are normally skipped, and
  two retain the established environment failures. `playground_crate_checks_for_wasm32`
  discovers `/Users/admin/src/light-cljs/Cargo.toml` as its enclosing workspace;
  `a_failed_restart_waits_for_work` cannot spawn `ps` (`PermissionDenied`,
  `Operation not permitted`). Both failures reproduce alone.
- The `compiler` target cannot build its eight missing pinned research
  includes. Running its existing reader, map-reader and runtime-manifest
  modules through a temporary `rustc --test` runner under `target/` passes 43
  tests with one normally ignored test. The manifest's existing update test
  regenerates the `requires` headers; no second test implementation is added.

The full default V8 suite remains with the orchestrator because the machine
cache is inaccessible. The engine's all-target suite passes 2,123 tests and
its doctests pass seven, covering the shared traversal helper and Scope API.
Its native integration checks include the landed Proxy implementation.

Both formatter checks and the direct JavaScript runtime checks pass. Strict
engine Clippy retains the same eight library/nine all-target diagnostics in
`deprecated`, `clippy::chunks_exact_to_as_chunks` and
`clippy::byte_char_slices`. Allowing only those categories passes all engine
targets and the native/native-plus-QuickJS library, binary and macro/source
targets. QuickJS-only and engine-free library/binary Clippy pass strictly.
Full compiler all-target lint remains blocked by the eight missing includes.

The compiler feature checks use three Cargo build jobs and eight test workers,
with feature configurations run sequentially:

```sh
CARGO_BUILD_JOBS=3 cargo nextest run --locked --no-default-features \
  --features bbp-macros --lib --test bbp_macros --test bbp_reader --test bbp_proxy_has -j 8 \
  -E 'binary(bbp_macros) | binary(bbp_reader) | binary(bbp_proxy_has) | test(bbp_evaluator::tests::)'
CARGO_BUILD_JOBS=3 cargo nextest run --locked --no-default-features \
  --features bbp-macros,quickjs --lib --bins \
  --test bbp_macros --test bbp_reader --test bbp_proxy_has -j 8
CARGO_BUILD_JOBS=3 cargo nextest run --locked --no-default-features --features quickjs \
  --lib --bins --test artifact_store --test benchmark_variants --test cache_matrix \
  --test cli --test compiler_cache --test config_file --test dev_nrepl --test dev_repl \
  --test dev_server --test dev_watch --test evaluation --test execution --test frontend \
  --test provenance --test v8_cache --no-fail-fast -j 8
```

Review corrections at `38edd314` restore `src/js_runtime.js` exactly to
`d83372e5`; native occurrence/callback adaptations are applied only when
assembling the native parser. The unchanged size-ratchet test runs under
QuickJS: every ceiling passes, with ordinary `none`/`prepare` output 871 bytes
smaller than `10567070` (keyword-case 497,211 bytes; pure-defs at prepare
498,176 bytes). Manifest regeneration leaves the runtime unchanged. The existing
temporary runner under `target/` loads the original reader, map-reader,
runtime-manifest, optimization and size-ratchet modules without copying their
tests: 72 pass and one remains normally ignored. The full compiler target and
QuickJS all-target Clippy still cannot read the same eight pinned research
includes; no research files or symlinks were changed.

On the same Rust/Node versions, the final native-only selection passes 123
tests, including nine reader/Promise tests. The native-plus-QuickJS library,
binary, macro, reader and Proxy targets pass all 422 tests, with one normally
skipped test. QuickJS's available targets again
pass 822 of 824 tests, with the same playground workspace-discovery and
`ps` permission failures described above. The engine all-target suite passes
2,123 tests at `f44ee730`; the subsequent map-conj traversal change is covered
by all 29 form tests, seven Clojure collection unit tests and the expanded
native/QuickJS reader table. All seven engine doctests pass. Both Penpot
commands were rebuilt and rerun at `38edd314`, with the five absolute roots
and budgets from the Penpot reproduction below: both exit 1 with the identical
`core.reduceKv` diagnostic quoted above and tracked by light-cljs-518.

Final native/native-plus-QuickJS scoped Clippy and engine all-target Clippy pass
when allowing only the same three baseline categories above. Strict engine/native checks retain
the same eight library/nine all-target engine diagnostics. QuickJS-only and
engine-free library/binary checks pass strictly. Native-only all-target Clippy
also reaches unavailable legacy evaluator methods in the compiler benchmark
and `unused_enumerate_index` in `tests/execution.rs:1772`; those benchmark,
execution and evaluator files are unchanged from `d83372e5`. Full V8/default
validation remains with the orchestrator.

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
as shallow statements with one temporary operand array per tree. Consumed
operand slots are reused and their references released, so the number of
tree nodes does not consume the engine's local-binding limit. The array
uses the ordinary accounted heap. The callable adapter is declared once
at the unit's top level, keeping its generated function bodies out of
deeply scoped leaves. Explicit worklists preserve
left-to-right evaluation of every occurrence; branches and function bodies
keep their original evaluation sites. These units then use the existing
shared-factory compilation boundary, on its 2 MiB helper stack. Parser
nesting256, the 64 KiB VM stack boundary, dynamic eval's 16,384 UTF-16-unit
cap, cancellation and aggregate heap accounting are unchanged. Application
and legacy macro emission keep their existing output.

At `76eef6f9` on macOS arm64, Homebrew Rust/Cargo 1.99.1, fresh native-only
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

Neither entry completes natively yet. The light-cljs-512 audit below fixes
these two runtime core-service gaps and records the next boundaries.
`tests/fixtures/native-helper-demand/frontend` retains the two original
macro declarations with their MPL notices and unrelated definitions removed.
`penpot_macro_declarations_fit_the_native_frontend` builds them natively and
compares the application output with QuickJS when enabled.
`deep_native_constructor_trees_preserve_effects_and_provenance` compares
70-level trees, sibling effects, branches, exception/finally order, closures,
raw-JavaScript hygiene and input occurrence identity against QuickJS.
`wide_and_deep_native_constructor_trees_reuse_operand_slots` expands a
69,633-node tree (beyond the engine's 65,535-local bound) and a 120-level
tree, checking leaf effects and occurrence identity against QuickJS.

### Compiler-owned names and core-service audit (light-cljs-512)

Native `gensym` takes scoped numbers from `generated_names`, sharing the
counter used by Rust core-macro expansion. A VM step carries only shared
counter handles and the analyzer's capture log across worker migration;
it never consults the executing worker's thread-local scopes. JavaScript
constructs the spelling and native symbol in the accounted heap. Unscoped
allocation skips imported and previously generated names, honors cache
counter reservations, and retains its spelling table as accounted engine
strings. Prefix tests cover the default, explicit strings, nil, booleans,
numbers, keywords, qualified symbols and exact UTF-16, including a lone
surrogate. Collection printing remains a separate boundary.

A root initializer can allocate names in its caller's scope. Its gensym
effect waits on the existing source-order effect lane. When an earlier
caller demands an initializer a later worker already started, the initializer
uses the earlier caller's scope, prefix and capture log. Prefixes are rooted
native dependency results; counters and logs remain compiler-owned. Initializer
claims and helper compilation retain their existing priority inheritance,
with no effect replay. The source-macro test compares cold output and
partial cache installs at one, two and four jobs, both seeded parallel
scheduling and fixed one-job admission, exercising idle context reuse.
Fresh, nested initializer allocations also match QuickJS under three
scheduling seeds and a held-earlier-demand fixture. As with QuickJS, macros
that require skipped initializer effects to
repeat must use `^:light-cljs/no-cache` or a cache blocker.

The repeated Penpot builds and the source/numeric fixtures in
`tests/compiler/macros.rs` exposed these small runtime gaps. All are now
implemented and checked in the single `native_core_services_match_quickjs`
table in `tests/bbp_macros.rs`:

| Discovered service | Coverage |
| --- | --- |
| `str` of symbols and keywords | Qualified names and gensym prefixes |
| `reSeq`, `rePattern`, `reFind`, `reMatches`, `isRegex` | Captures, missing groups, lazy/repeated tails, empty matches, UTF-16 offsets, global regex state and errors |
| `replace`, `seqToMap` | Nil/false replacements, vector metadata, transducer calls and trailing destructuring maps |
| `interpose`, `keepIndexed` | Sequence and transducer arities, repeated reductions, callback effects and reduced termination at a separator |
| `divide`, `int`, `long`, `byte`, `short`, `float`, `double`, `quot`, `rem`, `mod`, `numericEqual` | The compiler macro fixtures' Number conversions, negative zero and infinities |
| `notEqual`, `isNumber`, `isFloat`, `isDouble`, `isInfinite` | Native scalar occurrences and ordinary values |

The identity helpers `byte`, `short`, `float` and `double` retain imported
scalar occurrences, including large integer spellings and character/string
provenance. A focused evaluator test checks exact Datum reuse. Captured
UTF-16 name reservation sets also match QuickJS for every namespace in the
gensym fixture, including macros compiled on fresh helper tasks.
The binding-hygiene regression interleaves native `gensym "logic"` bindings
with Rust `or` expansion in the same function scope, comparing emitted names
and application results against QuickJS at one, two and four jobs. The
core-service table also discards the first whole chunk of a 65-element vector
in `keep-indexed`, checking later indices, callback counts and repeated demand
against QuickJS across both remaining chunks.

`native_compiler_macro_fixtures_keep_their_legacy_results` also reuses the
source-order, nested-demand, captured-function, failed-initializer and
extension-frontier fixture constants verbatim. The next nontrivial runtime
gap in the numeric macro fixture is `core.prStr` at
`(pr-str (float 16777217))`; the earlier numeric and `keep-indexed` calls
now run natively. The full compiler integration binary additionally needs
the pinned `research/clojurescript` checkout; this work leaves `research/`
untouched.

At `266cdeae`, rebased onto `32a999a1`, with the same macOS arm64/Rust 1.99.1
native-only command, five absolute source roots, one job, no cache and budgets
above, the next first errors are:

`--entry app.render`:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/geom/matrix.cljc:251:4: macro expansion: while expanding app.common.data.macros/assert!: /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/geom/matrix.cljc:251:4: macro app.common.data.macros/assert!: big-boy-pants-js: Error: unsupported operation: native macro runtime core.prStr; definition /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/data/macros.cljc:105:1 (bytes 6545..6573)
```

`--entry app.main`:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/config.cljs:150:9: macro expansion: while expanding log/wrn: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/config.cljs:150:9: macro app.common.logging/wrn: big-boy-pants-js: Error: unsupported operation: native macro runtime core.currentNs; definition /Users/admin/src/light-cljs/research/penpot-light/common/src/app/common/logging.cljc:416:1 (bytes 5204..5289)
```

These require native printing and dynamic namespace services, rather than
another direct scalar or finite-sequence core function. Neither entry
completes natively yet.

After the rebase and binding/chunk regressions, all 71 native-only macro
integration tests and all 35 native evaluator tests pass. With
`bbp-macros,quickjs`, all 73 macro integration tests and 331 library tests
pass (one ignored). The available QuickJS-only targets run 824 tests:
822 pass, with the two established environment failures in the playground
wasm workspace check and the watch test's sandbox-denied `ps` invocation.
The `compiler` target still cannot compile its eight missing pinned research
includes. Direct JavaScript runtime tests and format checks pass. QuickJS and
engine-free library/binary Clippy pass strictly. Native and native/QuickJS
library/binary/macro Clippy pass when only the three established engine
diagnostic categories are allowed; strict lint retains eight engine-library
diagnostics. The default V8 suite is reserved for the orchestrator.

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
collections are the engine's `__rt.formVector(…)`, `formList`, `formArrayMap`,
`formLiteralMap`, `formLiteralSet`, `formMap`, `formSet`, `formKeyword` and
`formSymbol` constructors. Calls of `count`,
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
and `>=` and `inc` and `dec` are lowered too, so that the helper
functions `for` and `doseq` expand into need no runtime object.

Fixed calls of `+`, `-`, `*`, `/`, `<`, `<=`, `>` and `>=` use direct JavaScript
operators on `__rt.formValue` operands (light-cljs-501). Unary `+` and `*`
retain their operand, including nil, occurrences and signed zero. Wider calls
evaluate all operands once before coercion or comparison short-circuiting,
using function-local temporaries where available. JavaScript string, nil and
number coercion is preserved. Remaining core functions a unit uses are bound
once per factory call, so their calls avoid repeated `rt.core` property reads.
An unsupported core getter is retained separately and still throws at its
original read, before call operands run. Operations assigned with `set!` in
the unit retain their dynamic reads.

One- and two-argument dynamic calls inside native functions retain the callee,
its `.call` method and the operands in frame locals (light-cljs-501). They
avoid allocating an adapter closure and argument arrays for each call. The
compiler-reserved `__rt.call` uses the VM's existing method-call instruction.
The method is still read before the operands, including a failing getter;
custom callables keep their receiver and functions receive `null`. Native
callable collections preserve their original occurrence and optional default.
Suspended operand evaluation retains these locals in the VM frame. Wider and
top-level calls use the existing generic adapter.

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
transducers (light-cljs-507), as do `partition-all` and `remove`
(light-cljs-513), and their collection overloads remain lazy.
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

Map and set elements that a macro constructs export in positional order or the
engine's trie order for hashed storage. Hashed export/seq order can differ from
the legacy engines' hash trie; printers restore the legacy hash traversal.
Inputs returned unchanged keep their exact original form. `concat` of collections
none of which is lazy returns a list where the legacy engines return a lazy
sequence that their bridge exports with an explicit `nil` metadata wrapper
(with a lazy or cons collection among them it is lazy and exports as theirs). A set literal with non-constant members
rejects equal members with `Duplicate key`, without the member's printed form,
after every member has been evaluated. Reader-distinct constants that lower to
equal values (`#{\a "a"}`, `'#{1 1.0}`, `'{\a 1 "a" 2}`) stay distinct members
or keys in positional native sets and maps, as in the legacy literal
constructors. They count and iterate every stored position and look up the
first equal key. `assoc` replaces only its first value; `conj` keeps an existing
member or appends a new one. Appending past eight entries promotes the backing
map to hashed storage, merging equal keys. `dissoc`/`disj` retain the legacy
array-map removal behavior: all equal keys are skipped but the count decreases
by one, leaving trailing undefined positions (observed as nil by core operations).
Equality and unordered hashing use the same entry/member rules as hashed forms.
Duplicate literals retain the legacy anomalies: set equality can be directional,
and two maps with differing values at equal keys can compare unequal even with
the same entries. Hash caches do not alter those comparisons.

Native reader inputs also use positional forms, preserving their original
members, keys, metadata and occurrence identity through updates and export.
This intentionally differs from the legacy input bridges, which still collapse
equal lowered keys and members. Literal expressions evaluated inside a macro
match QuickJS; `tests/bbp_macros.rs` pins both behaviors in its differential
matrix. Positional payload nodes use the aggregate allocation budget and trace
every stored position, including entries hidden by first-match lookup.
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

Demanded source helpers support compiler binding save/install/restore services.
Each VM task owns its binding overlays; snapshots record overlay presence and
value without reading the underlying root. Nested `binding` and `with-redefs`
therefore preserve pending, initialized and failed slots, including nil, false
and undefined overlays. Emitted `finally` blocks restore overlays after throws,
and saved VM frames retain them across helper waits and task migration. Bound
`set!` uses a native compiler service rather than an engine Proxy `set` trap.
Installation joins the existing source-order effect lane before changing an
overlay: a pending shared root can capture a binding without touching an atom,
so later callers must not initialize it first. A shared initializer reserves
its dependency before waiting for the lane, allowing earlier callers to lend
their position. Once the lane opens, the earliest caller's task executes the
factory, even if a later task reserved it first. Selection rechecks each
candidate's caller ancestry, including positions lent to helper producers
after reservation; inheritance through the initializer being selected cannot
change the candidates' relative order. This retains the caller's
overlays and mutable binding values by identity, with no factory effects to
replay or task-owned objects to publish. Bindings installed inside that
initializer still override its caller's bindings. Cancelling the current owner
before task selection fails the reserved claim and wakes its consumers.
Cancelling a superseded reserver withdraws that candidate and leaves the
transferred claim and its consumers alive.
The pool regressions force a later initializer to park, check inherited lane
progress, and verify scalar and mutable bindings, nested overrides,
restoration and once-only effects. The scheduling regression uses
`LIGHT_CLJS_STRESS_SEEDS` at two/four/eight slots and holds the earlier caller
at eight slots; emitted output matches the one-slot build and QuickJS.
Source regressions compare values, failures, restoration and once-only effects
against QuickJS, with identical emitted programs at one, two, four and eight
slots. A shared-runtime regression separately checks task isolation and that
saving a pending or failed getter does not force or replace it.

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

Preparing a deferred native unit walks only the expressions reachable from its
root, including explicit Var metadata. It indexes multimethod function operands,
core writes and type and reify method owners in that walk instead of scanning
the frozen catalog prefix for each unit. Global reads keep their separate lazy
demands. Cross-unit core replacement uses the session's conservative source
index, augmented with resolved mutation targets as they are analyzed. This
preserves generated writes from earlier units even after their IR is released.
Coordinator and worker frames both publish these targets; workers reach the
shared catalog directly without needing a coordinator session handle.
Installing a cached namespace restores resolved targets from both its top-level
and deferred-body artifact tables before later factories are prepared. This
visits only the installed artifact's IR and checks cancellation along the way.
Preparation retains executable source and reader tables, without retaining an
old program or running initializers. A regression counts actual arena reads
with growing frozen prefixes, including captured reify methods, and checks
definition-time metadata and output.

A unit source of at least 512 bytes, such as a source macro's or a
helper's code, is compiled once per runtime into a published factory; every
task that evaluates it calls that factory with its own native runtime, so its
per-task state still lands in that task's globals. A source that fails to
compile, or whose factory does not publish, is evaluated by the task as
before, with the same diagnostic. Catalog registrations are immutable native
data shared by every task of the compilation (light-cljs-503). Each frontier
is a persistent vector prefix of root and bundle descriptors. A worker applies
only its newly registered suffix; a helper producer imports its parent's same
rooted vector and populates a fresh ordinary JavaScript root object. Neither
path parses or evaluates catalog registration source. Every descriptor runs
in source order through the existing root or declaration registration function;
there is no coalescing, virtual property backing, or special redefinition path.
The task owns its getters, initializer slots, declaration products and protocol
tables. Data snapshots contain none of those mutable values. Tasks root only
their current snapshot; the shared cache is weak, so unused historical
frontiers are collectable. A cache miss extends the task's own rooted prefix.
A producer task
is not its worker's engine: registrations it takes
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
registration and evaluates a source macro once, not once per slot. Catalog
imports use a native command and leave no exported value handle behind. A
producer still populates all of its ordinary properties; sharing immutable
data removes source replay rather than sharing task-owned JavaScript objects.
`--stats` reports
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
wall-clock deadline bound work. The bridge memo does not root imported nodes;
the current incremental environment envelope alone has a bridge-owned strong
root. Macros can retain earlier envelopes through ordinary engine references.
The runtime automatically collects from allocation pressure.

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

Fixed native protocol implementations take their hidden receiver explicitly;
the dispatcher calls their entry directly (light-cljs-501). The receiver stays
fixed across method `recur`, and mutable field reads and captured receivers keep
their behavior. `Object`, `IFn` and methods whose body reads its function scope
(including raw JavaScript and `this-as`) retain the JavaScript receiver ABI;
protocol installation adapts those entries once. Multi-arity and metadata
dispatch still select the same implementation at the same frontier. Metadata
functions keep their ordinary null receiver, including strict JavaScript functions.

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
instance compares and hashes by identity unless it supplies the core protocols
described below. Interop on a struct (`(.-x s)`) reads
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

Native core operations on these structs call user `IEquiv`, `IHash`, `ILookup`,
`ICounted`, `ISeqable`, `ISeq`, `INext`, `IIndexed`, `IAssociative`, `IMap`,
`ICollection`, `ISet`, `IMeta` and `IWithMeta` entries. Direct, aliased and
`apply` calls use the same task-local protocol tables and declaration frontier;
`get` and keyword/symbol invocation preserve the two- or three-argument lookup
method, including an explicit nil/false default. Records retain their built-in
operations unless their own type is extended; object/default extensions do not
replace those defaults. `count` can walk an `ISeqable` without `ICounted`, and
sequence operations and reduction can traverse user sequences. Resolution and
method calls run through rooted VM callback frames, so helper demand can suspend
and resume after GC or migration without replaying effects. Equality keeps its
identity shortcut and returns a boolean; user hashes are coerced to signed
32-bit integers, as in the legacy compiler.

`first`, `rest` and `next` select direct protocol implementations before their
sequence fallbacks. After taking `seq`, `first` and `rest` resolve the resulting
value's `ISeq` method, including Object/default extensions; native sequences
keep their own implementations. Object extensions precede default extensions
on user structs. A
declared protocol missing the requested method arity throws a missing-method
error. The count, vector conversion and nth fallbacks accept both nil and
JavaScript undefined as exhaustion. Traversal honors indexed user sequence tails
and resolves user methods exposed by lazy wrappers.

Persistent native collections keep structural equality and hashing recursively,
including their keys, members and stored values. They never invoke user
`IEquiv`/`IHash`: two separate deftype instances remain distinct keys or set
members even when `(= a b)` calls their method and returns true. A record key
retains structural record comparison and hashing even after a user override.
Consequently equality/hashing of a collection containing custom structs can
differ from the legacy compiler. Dispatch tables, dependency tables and other
engine bookkeeping also never call user equality or hashing. The differential
matrix `native_struct_core_protocol_operation_matrix_matches_legacy` covers
each operation's call shapes at 1/2/4/8 slots; engine form tests separately
assert the structural-key policy and callback rooting across suspension/GC.

Not supported: `specify!`/`extend-instance`, `extend-type` of core types other than the native
form kinds (`Keyword`, `Symbol`, `PersistentVector`, `List`, `EmptyList`,
`PersistentHashSet`, and `PersistentArrayMap` and `PersistentHashMap`, which
are one native map kind, so extending either extends every map), variadic
protocol methods, `map->R` of a record with mutable fields, `getBasis` and
other properties of a struct type, and a macro returning a struct (export fails
with an explicit error). A helper analyzed on demand can itself expand source macros:
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

### Consumed command payloads

Native evaluator commands use `Completion::WeakReady`, an explicit transient
dependency outcome with the same producer and publication checks as `Ready`.
The producer keeps the command message strongly rooted throughout `request`,
including helper waits and pool handoffs, until its task reaches the next
command boundary. The task roots the message on its operand stack when it
reads it. Completed command cells keep their single-assignment bookkeeping
but do not retain consumed messages, evaluated source strings or historical
`&env` snapshots. Reading a reclaimed weak outcome reports cancellation;
the native command protocol never reads a consumed key again. Ordinary
helper, initializer, binding and effect-lane outcomes keep their existing
strong rooting and completion behavior.

### Namespace-chain retention (light-cljs-499, 2026-10-26)

Remeasured on macOS arm64, Homebrew Rust 1.99.1, from `aec0374b`, after
task-context lending, task-local macro-function caching and the recent GC
accounting fixes. The generated chain has four or eight namespaces requiring
their predecessor, each with 50 helper/macro/caller triples; each inline macro
expands `(list (quote +) x I)`, and each caller invokes its own macro once.
All measurements use one compilation slot and no compiler cache.

The original 400-call CLI workload still failed at the default 67,108,864-byte
limit, at `scale.n4/m24` (call 225), with `heap byte or string limit exceeded`.
At a 4 GB limit it completed, peaking at 173,656,974 managed bytes and
204,226,560 bytes RSS, down from the original report's roughly 550 MB managed
heap but still retaining historical snapshots.

Forced final collections, with the compiler host/session still alive:

| Calls / namespaces | Before, live bytes (4 GB limit) | After, live bytes (default 64 MiB) | After, peak managed bytes |
|---|---:|---:|---:|
| 200 / 4 | 55,954,438 | 11,332,192 | 21,293,600 |
| 400 / 8 | 173,396,882 | 17,731,120 | 28,927,160 |

The additional four namespaces retain 6,398,928 bytes (about 1.53 MiB each),
including their 200 distinct macro functions. Both builds run their emitted
application in Node and print `5200` and `11200`, respectively.
Fresh native-only release CLI runs also complete at the default limit: the
400-call inline chain peaks at 28,933,816 managed bytes and 75,988,992 bytes
RSS; the helper-calling variant peaks at 43,442,228 managed bytes and
94,699,520 bytes RSS and also prints `11200`.

Root-source isolation before the fix, collecting after each removal, showed
that the bridge memo and command outcomes overlapped heavily. Clearing only
the bridge left 54,571,560 / 170,623,188 live bytes at 200 / 400 calls. Clearing
shared factory roots made no further difference; dropping the compiler host
too left 50,736,320 / 164,236,140 bytes and **zero persistent handles**. The
completed command cells still traced their messages through
`DependencyOutcome`. These were the dominant holder, alongside the bridge's
independent pins. This diagnostic deliberately removed roots in a throwaway
probe; normal compilation preserves all observable values.

The final heap inventory corroborates the source: command vectors fall from
5,236 / 10,468 to one at either size, and occurrences fall from
34,280 / 108,560 to 2,906 / 5,706. Persistent host handles fall from
34,481 / 108,961 to 201 / 401 (the macro handles and current environment).
Function counts stay at 1,374 / 1,978. The improvement releases consumed
messages and unobserved forms, without removing definitions or rebuilding
task state.

The checked-in regression reproduces the generator, enforces the default
heap cap, forces a final collection before teardown and limits the four-to-eight
namespace increment to 8 MiB:

```sh
CARGO_BUILD_JOBS=3 cargo test --locked --no-default-features --features bbp-macros \
  --lib bbp_evaluator::tests::namespace_macro_chains_fit_the_default_heap_with_linear_live_bytes \
  -- --exact --nocapture
```

`weak_bridge_entries_preserve_live_occurrences_and_raw_metadata` also forces
collection around retained occurrences and raw metadata, checks exact Datum
and source reuse, enforces conversion budgets on memo hits and verifies dead
provenance is pruned.

The pruning regressions also retain only the raw metadata of
`^{1 [1 [2 [3]]] 1.0 nil} []`: positional metadata now retains the first key's
vector as well as the second equal key. Restoring the original compiler Datum
requires ten nodes and depth five. Descendant source spans and nested metadata
survive collection and pruning; releasing the metadata reclaims every descendant.
A real emitted macro saves its first
environment in task state across an envelope update, input-handle release,
collection and pruning; the old/new slot values and shared namespace identity
remain intact. Before positional input storage, the forced-final-collection
regression reproduced the table's exact live and peak managed byte counts at
both sizes; these measurements describe that earlier representation.

Review-fix validation on the same toolchain: all 33 native evaluator tests and
all 66 native `bbp_macros` tests pass, as does the pool stress with
`LIGHT_CLJS_STRESS_SEEDS=12` (one, two, four and eight slots).
The engine's `--all-targets` suite passes 2,102 tests and its
seven doctests pass. With `bbp-macros,quickjs`, all 66 macro integration
tests (including QuickJS comparisons) and the memory regressions pass; the
available compiler targets run 933 tests, with 931 passes and two matching
environment failures: the playground's wasm crate discovers the enclosing
main-checkout workspace, and
`dev_watch::a_failed_restart_waits_for_work` cannot execute `ps` in the
sandbox (the same permission failure when run alone). The `compiler`
integration target cannot build its eight missing pinned research includes;
`research/` was left untouched. The default V8 suite is reserved for the
orchestrator.

Compiler and engine format checks pass. Strict engine Clippy retains the
nine existing `deprecated`, `chunks_exact_to_as_chunks` and
`byte_char_slices` diagnostics; allowing only those categories passes for
all engine targets and for the native/QuickJS compiler library, binaries
and `bbp_macros` target. Plain engine-free compiler library/binary Clippy
passes strictly. All-target compiler lint remains blocked by the missing
research includes with QuickJS, and by existing tests calling unavailable
legacy evaluator methods in the engine-free configuration.

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

## Native helper bindings and Penpot (light-cljs-520, 2026-10-27)

At `10c883cc`, demanded source helpers have task-owned binding overlays and
source-order installation as described above. The cold-root regression first
reproduced a two-slot result capturing `:app` instead of the one-slot `:a`;
joining the existing effect lane before installation fixes that difference.
The held-unit case also verifies inherited lane progress without replay.

Validation on macOS arm64, Homebrew Rust/Cargo 1.99.1 and Node 26.12.1:

- Native evaluator, macro, reader and Proxy targets pass 127 tests. With
  `bbp-macros,quickjs`, the library, binaries and those targets pass all 426
  tests, with one normally ignored test. Both runs use three build jobs and
  eight test workers. The binding regressions cover nested overlays, bound
  `set!`, simultaneous initializers, throws, pending/initialized/failed roots,
  nil/false/undefined, task isolation and one/two/four/eight-slot output equality.
- QuickJS-only library, binaries and available integration targets other than
  `compiler` run 824 tests: 822 pass and two retain the documented environment
  failures. The playground wasm crate discovers the enclosing main-checkout
  workspace; `a_failed_restart_waits_for_work` cannot execute `ps` in the sandbox.
  The full `compiler` target and its all-target lint still cannot build eight
  absent pinned research includes. Engine-free all-target checking also finds
  existing unguarded legacy-engine test references. Research was left untouched.
- All 2,123 engine tests and seven doctests pass. These results are reused
  after the binding-lane change because engine sources and inputs are unchanged.
  Compiler/engine formatting and direct JavaScript runtime checks pass. Strict
  engine Clippy retains nine diagnostics in `deprecated`,
  `clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices`. Allowing
  only those categories passes all engine targets and the native/native-plus-
  QuickJS compiler library, binaries and macro/reader/Proxy targets. Engine-free
  library/binary Clippy passes strictly. The default V8 suite is reserved for
  the orchestrator because its machine cache is inaccessible to this worker.

For Penpot only, `10c883cc` was temporarily merged with unlanded light-cljs-518
at `bef2fb01099bdcf0bd32bdb7ab7223eb38b0d411`, producing tree
`2eba74106282c24e3ea8de4a0cb1c2adcd83af1c`. The native-only debug CLI was rebuilt.
Both documented entries were run with five absolute read-only roots,
`--jobs 1 --no-cache --optimize none --esm --stats`, 100,000,000 macro expansions,
4,000,000,000 nodes, 3,600,000 ms, 4,000,000,000 heap bytes and a
1,000,000,000,000 poll budget. Both pass the missing binding-service call and
exit 1 at this next distinct boundary, tracked by light-cljs-521:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:893:4: macro expansion: while expanding fnc: /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:893:4: macro rumext.v2/fnc: big-boy-pants-js: TypeError: /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2/compiler.cljc:171:1: rumext.v2.compiler/compile-form: undefined is not a function; definition /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:320:1 (bytes 27920..28218)
```

Line 171 declares `compile-form` with `defmulti`. A reduced two-namespace fixture
with a demanded defmulti inside a binding reproduces the initializer failure;
the QuickJS-only CLI compiles it and Node prints `:bound`. The innermost missing
service still needs diagnosis in light-cljs-521. Neither Penpot entry completes.
The temporary merge was aborted afterward; no light-cljs-518 commits or research
changes are included in this branch.

The review fix at `13ac8b52` also handles an initializer reserved by a later
task before an earlier bound caller demands it. The reduced regression first
captured `"root"` instead of `:earlier` at two slots. Reserving before the lane
and selecting the earliest caller's task before factory execution fixes that
case, including mutable binding values, inner overrides and once-only effects.
All 128 selected native tests pass with `LIGHT_CLJS_STRESS_SEEDS=12`; the new
reservation-cancellation test passes separately. All 428 native-plus-QuickJS
tests pass, with one normally ignored test. QuickJS-only compatibility again
passes 822 tests with the same two environment failures described above.
Formatting, direct runtime checks and baseline-isolated native/combined Clippy
pass; QuickJS library/binary Clippy passes strictly, while its all-target check
still finds the same eight missing research includes. Engine sources and inputs
are unchanged, so the prior 2,123 tests and seven doctests remain applicable.

Both Penpot commands were repeated with the same five roots and all budgets
using a temporary merge of `13ac8b52` and unlanded light-cljs-518 at
`076a1c213e804b348605482c144a36b528d4716b` (tree
`1387195c50ce89d16a11ac8b8318ff56a9d8cae9`). Both pass binding services and
report the identical `compile-form` diagnostic above. That merge was aborted,
and the native-only CLI was rebuilt from this branch; research was untouched.

The second review fix at `113db5f1`, rebased on `b3b655cc`, retains candidate
activity ancestry and rechecks inherited caller order at task selection. Its
deterministic evaluator regression first reproduced `"root"` instead of
`"helper"` when the earliest caller reached the initializer indirectly. It
also covers an earlier waiter arriving after both candidates reserve. Separate
gated tests cancel a superseded reserver or its new owner with a real consumer
waiting on the initializer: the former preserves one factory effect, while
the latter wakes all consumers with failure before any factory effect. Both
paths clear pending claims and task lane guards.

On that rebased source, all 140 selected native evaluator/lane/macro/reader/Proxy
checks pass with `LIGHT_CLJS_STRESS_SEEDS=12`, three build jobs and eight test
workers. The native-only debug CLI was rebuilt and both documented Penpot
entries repeated with the same five absolute read-only roots and every budget
above, without a temporary merge because light-cljs-518 is now landed. Both
exit 1 with the identical `compile-form` diagnostic above; the binding-service
boundary remains passed and light-cljs-521 remains the next distinct failure.

The combined native/QuickJS library, binary and macro/reader/Proxy targets pass
all 434 tests, with one normally ignored test. The direct and indirect binding
fixtures also pass twelve seeds with QuickJS available for exact one/two/four/
eight-slot legacy comparisons. All 2,129 engine tests and seven doctests pass.
Compiler/engine formatting and the direct JavaScript runtime checks pass.

The full QuickJS-only suite passes 2,052 of 2,053 tests; the full default suite
with `LIGHT_CLJS_V8_CACHE=require` passes 2,129 of 2,130. Both have only the
existing `playground_crate_checks_for_wasm32` failure: Cargo discovers the main
checkout's workspace for this nested worktree's playground crate. The earlier
sandbox-only watch-process failure now passes. Both full runs include the
compiler target, using a temporary read-only worktree symlink to ClojureScript
at `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`; that symlink was removed afterward.
The same macOS arm64, Rust/Cargo 1.99.1 and Node 26.12.1 configuration uses three
build jobs and eight test workers, with compiler feature variants serialized.

Strict all-target Clippy passes for default and QuickJS-only features, and
engine-free library/binary Clippy passes strictly. Engine-free all-target
checking still fails in unchanged tests with unguarded QuickJS and legacy
evaluator references. Strict engine Clippy retains eight library diagnostics
and nine library-test diagnostics in the three established categories above;
allowing only those categories passes all engine targets, all combined
native/QuickJS compiler targets, and the selected native-only library, binary
and macro/reader/Proxy targets. No source lint suppressions were added.

## Demanded native multimethods (light-cljs-521, 2026-10-27)

The first missing call in the reduced demanded `defmulti` fixture was
`__lightDefineCompilerRoot`, emitted by its `defonce` expansion. Native tasks
now implement that root-definition service and construct `MultiFn` values as
native structs containing immutable fields and engine atom handles. The
single-product declaration uses the existing task-owned initializer protocol:
constructor/dispatch-expression effects run once in the earliest caller's
binding context, retaining suspended frames and lane inheritance. A repeated
`defmulti` preserves the original definition; a declaration alone does not
count as a definition for `defonce`.

Method declarations use the same source frontier as protocols. Each method
bundle initializes once; its computed dispatch key and other effects are not
replayed in another task or when an earlier frontier is selected. Persistent
maps in a private shared journal record declaration operations and subsequent
runtime mutations. Each supplied method/preference atom identity owns its history,
shared by every `MultiFn` constructed with that atom, including in another task.
The histories are independent when only one table is shared. An initializer-owned
private registry uses persistent identity maps and CAS bookkeeping without user
equality or effect replay. Constructor-supplied method and preference maps seed each view;
the supplied atoms continue to contain language-level tables after additions,
removals, preferences and resets. The visible dispatch/preference maps are
materialized in source order, cached by history
identity and frontier, and searched using native value equality and `isa?`.
Dispatch supports exact keys, nested vector keys, explicit or global hierarchies,
default methods, direct preferences, removal and reset. Immutable native
`deftype`/`defrecord` constructor tags participate in both local and global
hierarchies, including constructor prototype relations. Hierarchy mutations are
observed on every lookup. Exact table entries win before hierarchy/preference
search, including a preference for an ancestor. The preference behavior matches
the pinned CLJS/legacy runtime, whose recursive preference walks do not return inherited matches.

A native function root resolves runtime services through the consuming task's
global runtime, including named dispatch helpers compiled in another namespace
before their multimethod consumer is analyzed. Multi-arity and variadic function
roots and methods route through their dispatcher without attaching the legacy arity acceleration
properties; their rest argument is a native list. This keeps method functions
shareable without publishing a mutable JavaScript runtime or replaying their
declarations. Macro entry functions keep their host arity metadata. Native
multimethods pass direct and first-class `instance? MultiFn`/`ifn?` checks and,
as in the legacy runtime, fail `fn?`. Native immutable type and record
constructors pass both `fn?` and `ifn?`, matching their JavaScript function
representation in the legacy runtime. Bundle products record root ownership even
without a preceding `declare`, so a later ordinary definition can replace them.
No native-only code was added to the emitted runtime in `src/js_runtime.js`.
Fixed native calls share one member-lookup helper per unit. It recognizes
multimethod structs before reading `.call` and returns a consuming-task
dispatch method; the enclosing frame retains the method, receiver and
arguments across suspension. Sharing the helper keeps generated call syntax
shallow without changing engine source or nesting limits. The differential
table includes an 81-call generated helper whose method effects run exactly
once. Constructor-tree flattening also handles replaceable core functions:
it prepares each callee and method before operand effects, then invokes that
saved callable after its operands. This preserves core writes and getter
order while keeping generated syntax shallow, including nested replaceable
core calls around constructors. Method-function arguments retain consuming-task
reads even when global core-write tracking makes `-add-method` replaceable.
Function roots reading dynamic core settings such as
`*print-length*` also publish, resolving the setting in the consuming task.
A dispatch function that captures task-owned JavaScript state fails publication
explicitly; the constructor failure is shared, so later demands cannot replay
its effects. `demanded_multimethods_reject_task_owned_dispatch_without_replaying_effects`
checks this boundary at all four pool sizes.

`demanded_multimethod_dispatch_shapes_match_quickjs_at_every_pool_size` is one
24-row native/QuickJS comparison at jobs 1/2/4/8, checking identical emitted
JavaScript as well as literal results. It covers scalar and structural keys,
constructor/function keys, keyword/variadic dispatch, nested hierarchy dispatch,
global hierarchy changes,
preferences and ambiguity/missing-method errors, mutations, explicit construction,
source-frontier replacement with and without a prior declaration, defonce,
binding restoration, referenced fixed/variadic dispatch functions, retained
constructor table atoms, native type tags, shared and independently shared
constructor tables, and direct/first-class predicates on functions, constructors,
protocol callables, multimethods, callable collections and noncallable values.
`multifns_sharing_constructor_tables_observe_each_others_mutations` checks
addition, removal, preferences and reset across separate namespace macro callers
and a later constructor sharing the same atoms, with seeded scheduling at all
four pool sizes.
`demanded_multimethod_effects_run_once_across_suspension_and_lending` checks shared
constructor, method-key and helper effects under seeded scheduling and a held
earlier lane at four/eight slots; unheld scheduling runs at all four pool sizes.
The held case asserts that a lane was actually inherited. With two later
callers it needs a third worker to start the deliberately held earlier job;
this test perturbation disables starvation rescue.

Both Penpot entries (`app.main`, `app.render`) were rebuilt natively and run with
all budgets from the reproduction above, jobs 1, no cache, and five absolute
read-only roots under `/Users/admin/src/light-cljs/research/penpot-light`.
On macOS arm64 with Homebrew Rust/Cargo 1.99.1, production code
`b12d8ca6` over `54bd1144` passes the demanded
`rumext.v2.compiler/compile-form` boundary. Both commands exit 1 with this exact
next distinct error:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:906:3: macro expansion: while expanding fnc: /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:906:3: macro rumext.v2/fnc: big-boy-pants-js: unsupported operation: compiler literal; definition /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:320:1 (bytes 28314..28909)
```

At that checkpoint this call contained `#js` arrays, and the native input bridge
rejected every `DatumKind::CompilerLiteral`; even `(defmacro echo [x] (list 'quote x))
(echo #js [])` reproduces it. QuickJS compiles an equivalent macro whose result
is tested with `js/Array.isArray`, and Node prints `true`. This different bridge
gap is fixed by light-cljs-535 above. That section records the later boundary.
At the 521 checkpoint neither Penpot entry completed; research was
left unchanged.

Verification used macOS arm64, Homebrew Rust/Cargo 1.99.1, Node v26.12.1,
`CARGO_BUILD_JOBS=3`, three native/engine test threads and eight nextest slots.
Production `b12d8ca6` over `54bd1144` was rebased to `756aa045` over `ba7def01`.
The final rebase changes only test-specific waiter hooks and regressions;
production behavior, dependencies and native inputs are unchanged, so native
and QuickJS integration results are reused across it.

The final native-only suite passes all 96 tests with `LIGHT_CLJS_STRESS_SEEDS=12`.
All 24 demanded differential rows match QuickJS at jobs 1/2/4/8; effect and
capture-rejection tests pass with 12 seeds. The cross-task shared-table comparison
also passes with 12 seeds. Final combined library tests pass 346 tests (one
ignored), and reader/Proxy targets pass another 12. Function roots reading
`map first` or `*print-length*` publish; mutable user-array captures still fail.
The engine all-target suite passes 2,145 tests and all seven doctests pass.
Engine results are reused across subsequent compiler-only emitter changes:
engine source, dependencies and fixture inputs are unchanged. Both final native
Penpot commands reproduce the exact compiler-literal diagnostic above.
The existing light-cljs-535 remains the only follow-up for that gap.

Final QuickJS-only nextest (`--no-default-features --features quickjs -j 8
--no-fail-fast`) runs 2,054 tests: 2,053 pass, one fails and 108 are skipped.
The full V8 run at `8cb88f19` (`LIGHT_CLJS_V8_CACHE=require cargo nextest run
--locked -j 8 --no-fail-fast`) runs 2,130 tests: 2,129 pass, one fails and
109 are skipped. Its full result is reused for unchanged legacy behavior;
fresh final V8 cache, terse/name-hygiene and compilation-worker coverage passes
all 41 selected tests, including the new worker-first waiter regression.
Native changes have fresh stress/differential coverage; fresh full QuickJS
coverage verifies shared compiler behavior.

Both full legacy suites fail only `playground_crate_checks_for_wasm32`:
installed Rust/Cargo 1.99.0 resolves this nested playground crate to the outer
checkout workspace and rejects it before compiling Rust. Manifests, playground
script and CLI test are unchanged from master. `LIGHT_CLJS_CARGO` pins the
existing wasm toolchain through `/usr/bin/env`, with its `RUSTC`, library path
and `CARGO_HOME=/Users/admin/.cargo` specified.

Final default-feature strict all-target Clippy passes. Combined native/QuickJS
all-target Clippy passes allowing only the existing engine categories
`deprecated`, `clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices`;
the strict combined check reports eight existing engine-library diagnostics.
Engine-free strict all-target Clippy still fails in unchanged examples, benches
and tests requiring legacy evaluator APIs; engine-free library/binary strict
Clippy passes. No source lint suppressions were added. Compiler/engine Rust
formatting passes. `src/js_runtime.js` is unchanged; no manifest regeneration
is needed. The documentation-only final commit reuses these checks.
