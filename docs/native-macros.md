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
`--no-default-features` embeds no engine. Interactive sessions still use QuickJS.
Explicit compiler data readers use the selected compilation engine: native
forms on the Pants build, and QuickJS on the V8 build and QuickJS reference.
The Pants build shares `light.build`'s resource, configuration and dependency
services with those builds. Requests suspend the task before invoking its
compiler callback, preserving the caller and initializer ancestry when VM
steps move between worker threads. Reader definition gensyms retain the
canonical emitted unit's identity; reader calls count apart from macro calls.
Reader tasks keep their own import memo and gensym reservations while sharing
the runtime's code and aggregate budget.
Outside a namespace scope, reader gensyms skip imported symbols and earlier
generated spellings. Reader results reject JavaScript `undefined`, including
inside collections, literal payloads and metadata, at the original tag span;
`null` remains valid nil data.
See [the cache-suite compatibility results](compiler-cache.md#pants-build-cache-suite-compatibility).

A compilation retains one shared runtime and aggregate execution control. Each
task has private function handles and mutable JavaScript state. Native forms and
publishable closures cross rooted dependency boundaries; mutable Maps remain
task-owned. Commands do not serialize forms, invoke a reader, or replay effects.
Macro input nodes are memoized across the runtime's macro tasks by compiler Datum
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

### List and sequence markers (light-cljs-546)

Native `list?` distinguishes lists and plain Cons cells from positional,
chunked and lazy sequences. An immutable non-IList tag travels with positional
sequence storage through shared tails and metadata; it does not participate in
equality, hashing, tracing or source occurrence identity. Empty sequence tails
remain EmptyList values. `concat` retains its LazySeq identity even for finite
or empty inputs; `iterate`, `keys` and `vals` retain their sequence identities.
No predicate realizes a lazy value or classifies a sequence by its elements.

`list?`, `seq?`, `sequential?`, `coll?`, `vector?` and `map-entry?` use native
builtin marker checks and the existing resumable declaration adapter for user
markers. The source differential table covers direct, saved and `apply` calls,
immutable/mutable types, records and host marker extensions at jobs 1/2/4/8.
Each of the six default marker extensions is compared too, including unmasked
types and types declaring only a partition-one protocol. Builtin negative
markers and partition-zero declarations block host/default fallback. Mask
membership comes from the original declaration's task-local type information;
later extension bundles and overlays install direct markers without changing
that declaration's mask.
Demanded helpers publish without task-local copies; a cold helper in a lazy
thunk resumes without replaying its effect. The table also checks metadata,
equality, hashing and source input. Engine coverage retains occurrences and
metadata across publication, owner release and GC.
Plain cons cells retain their actual sequence tails, so `rest` preserves the
tail's sequence kind, metadata and occurrence identity. `conj` of a sequence view
creates a Cons with that view as its tail; the new Cons has no collection metadata.

JavaScript array sequence operations reuse the task-owned reduction cursor,
retaining the original mutable array rather than taking a snapshot. Their
classification, metadata, traversal and direct equality/hash observations match
the legacy runtime; these cursors remain task-owned. Emitted application code
and `src/js_runtime.js` are unchanged.

Unary `concat` and binary `concat` with an exhausted left operand preserve the
actual operand sequence, including its marker, metadata and identity. Variadic
`concat` still traverses its final operand. `cons` onto nil constructs a List,
so its stack operations and explicit List extensions observe the List type.
Array cursor equality compares elements incrementally, preserves identity and
count shortcuts, and stops before reading getters after a mismatch. Element
comparisons can demand helpers and suspend through the ordinary callback path.
The immutable sequence-view tag also retains countedness: indexed views and
finite integer ranges use the count shortcut, while hash-map, set, key, value
and floating range views do not. Metadata and shared tails retain that tag.
Occurrence wrappers are transparent when copying the tag to a shared tail.
Finite integer ranges retain their end and step through metadata, rest and
next; counting computes the cardinality from the actual head even after
realizing a lazy range tail. This preserves JavaScript rounding when a step
beyond the safe-integer boundary leaves the head unchanged. Length mismatches
therefore do not realize the range or read array
getters. The source differential checks ten-million-element ranges under a
64 MiB native macro memory limit, against both array cursors and small vectors.
Array cursor equality also reads queue descriptor payloads through source
occurrences and rejects immutable and mutable records before element access,
after honoring an explicit left-hand record IEquiv implementation. Mutable
records require this dispatch in the cursor adapter because they are host
objects rather than native structs.
Cursor metadata copying returns the same cursor for identical metadata and
preserves an existing view when its backing array has become exhausted.
`peek` honors explicit core-type and host IStack extensions while retaining
builtin precedence over defaults. Map entries keep their own dispatch identity.
The `-peek` protocol entry dispatches nil extensions separately from public
`peek`, which returns nil for nil. The source differential table covers these
review regressions, including large array cursors and exact getter/equality
effects, at jobs 1/2/4/8 under each default collection marker extension.
The same table checks direct, saved and apply counts of uncounted user
ISeqable types returning ten-million-element ranges under the 64 MiB limit,
including helper demand after an observable callback effect. A grid of
`(count (nthrest (range a b s) k))` covers empty, positive, negative,
fractional and rounded boundary tails across chunk boundaries. Native
`nthrest` preserves the input for nonpositive counts and uses ordinary
resumable sequence traversal for demanded tails.

Fresh native-only debug builds at `ea1e76d5`, based on `b3ef562c`, on macOS
arm64 with Homebrew Rust/Cargo 1.99.1 pass the former `core.isList` failure and the next small service gap,
`core.peek`. The latter now observes supported persistent stacks without
realizing other sequence kinds and dispatches user IStack methods through the
existing resumable adapter. Its differential includes direct, saved, `apply`
and protocol calls, exact unsupported-sequence errors and a cold helper demand
after a callback effect.

Both documented Penpot entries were rerun with all five absolute read-only roots,
`--jobs 1 --no-cache` and every budget in the reproduction below. Both exit 1
with this identical first different diagnostic:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/main/ui/shapes/text/fo_text.cljs:17:1: macro expansion: while expanding mf/defc: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/main/ui/shapes/text/fo_text.cljs:17:1: macro rumext.v2/defc: big-boy-pants-js: Error: unsupported operation: native macro runtime core.flatten; definition /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:334:1 (bytes 522..819)
```

The probes take 193.55 s (`app.render`) and 201.85 s (`app.main`). Neither
entry completes natively; research is unchanged. The recursive traversal
service is tracked separately by light-cljs-551. Before adding `peek`, both
entries reached its identical unsupported-service diagnostic at the same source
location, in 187.40 s and 187.05 s respectively.

The range-count change at `2f42bde2` passes all 183 selected
evaluator/macro/reader/Proxy tests with
`LIGHT_CLJS_STRESS_SEEDS=12`, three build jobs and three nextest slots. All
2,168 engine tests and seven doctests pass on the same code. The rebase onto
`b3ef562c` integrates heap-layout and shared-heap statistics changes, so the
engine, native and combined checks and both Penpot probes were rerun. Fresh
combined native/QuickJS coverage at `b300be08` passes 516 library, CLI and
macro/reader/Proxy tests, with only the existing wasm workspace check failing
(one existing ignored test). The predicate, peek,
metadata, identity and equality-effect source table matches QuickJS byte for
byte at jobs 1/2/4/8 under all six default marker extensions. Engine strict
Clippy retains nine baseline diagnostics (eight for the native compiler's
engine dependency); allowing only `deprecated`, `clippy::chunks_exact_to_as_chunks`
and `clippy::byte_char_slices` passes engine and native combined all-target
checks and the selected native targets. No source suppressions were added.
The final production checkpoint `ea1e76d5` only removes an identity conversion
from the count-validation error path. The full native and engine results are
reused across that cleanup; all 63 form/lazy tests and the complete native-only
source table pass again. The source table also imports explicit queue descriptors
through both evaluator bridges at every jobs setting, since source `#queue`
syntax lowers to an `into` form before macro input. The native cache regression
`generated_core_writes_survive_cached_artifacts` passes against the final CLI.

The default V8 and QuickJS-only full nextest runs pass 2,132 and 2,055 tests
respectively; each fails only the documented `playground_crate_checks_for_wasm32`
environment check. The pinned wasm Cargo assigns this worktree's excluded
playground crate to `/Users/admin/src/light-cljs/Cargo.toml`'s workspace before
checking source. Both full runs were repeated after the sequence review fixes,
including cache checks against the final compiler identity. Default and QuickJS
all-target Clippy pass strictly, as do engine-free library/CLI checks.
Native-only and engine-free all-target lint retain existing benchmark/example
calls to unavailable legacy evaluator methods; engine-free compiler tests also
import disabled `rquickjs` APIs. The unchanged execution fixture has a
cfg-specific `unused_enumerate_index` diagnostic without QuickJS. Compiler and
engine format checks cover the integrated source. The read-only ClojureScript
symlink uses `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` and is removed afterward.

The default-feature V8 200k build compares the same-base baseline `b3ef562c`
with the integrated sequence fixes at `118e784a`. Both release executables
were built in this worktree with the matching source trees. Generate the fixture with
`bb benchmarks/generated-project/generate.bb target/g200k --seed 42 --lines 200k`,
build the release CLI with `LIGHT_CLJS_V8_CACHE=require`, then run
`/usr/bin/time -l <saved-cli> release bench --no-cache` from `target/g200k`
(default jobs 1). Three quiet paired runs give median wall time 7.07 s before
and 7.00 s after (-1.0%); ranges are 6.99–7.32 s and 6.96–7.10 s. Median
user+system CPU is 7.58 s and 7.51 s; median peak RSS is 1,037 MiB and
1,035 MiB. An earlier set was discarded because peer tests were active;
all three retained pairs have clean before/after process snapshots.
All six outputs have SHA-256
`1a75bc87b26f312196671d4de9ed73110fc8aef077749fd8b6d5ce352bf4524e`;
Node and the fixture's Babashka oracle both print `484264` for inputs 42 and 3.
The host is macOS 15.7.3, Apple M3 Ultra (Virtual), 24 logical CPUs and 32 GiB
RAM, with Rust/Cargo 1.99.1, Node 26.12.1 and Babashka 1.12.218. No native/V8
Penpot speed ratio is reported because neither native entry finishes.

The second review fixes compare fresh default-feature release executables from
`eea0eb4c` and `ea1e76d5` on that same fixture and command. One clean paired run
takes 7.38 s before and 7.20 s after, with user+system CPU of 7.82 s and 7.68 s
and peak RSS of 1,007 MiB and 1,012 MiB. Process snapshots are quiet before and
after that pair. Two later pairs are discarded because peer lint/build activity
resumed; one pair does not establish a performance improvement. Both retained
outputs have the SHA-256 above, and Node and the Babashka oracle both return
`484264` for inputs 42 and 3. The release builds use
`LIGHT_CLJS_V8_CACHE=require`; legacy emitted JavaScript remains unchanged.

Final review-fix validation at `141bfa60`, including the native call/root
changes from `a629b7e4`, passes all 184 selected native evaluator, macro,
reader and Proxy tests with `LIGHT_CLJS_STRESS_SEEDS=12`, three Cargo build
jobs and three nextest slots. All 2,178 engine tests and seven doctests pass.
The native/QuickJS source table matches emitted output and effects at jobs
1/2/4/8 under every default marker extension, including mutable record
IEquiv overrides, rounded range-tail counts and user ISeqable range counts.
The native cached-artifact regression passes too. Fresh full QuickJS and
default V8 nextest runs pass 2,055 and 2,132 tests respectively, each failing
only the same `playground_crate_checks_for_wasm32` workspace diagnostic
described above. Both all-target compiler Clippy configurations pass strictly.
Engine strict Clippy retains its nine baseline diagnostics; allowing only
the three documented categories passes engine all-target and selected native
checks. Engine-free library/binary Clippy passes strictly; engine-free
all-target checks retain unavailable legacy evaluator APIs and rquickjs imports.

The final rebase onto `bba14eb8` changes only the initializer-slot integration
test. Its five tests, compiler/engine formatting and isolated engine all-target
Clippy pass after the rebase. Production sources and root build inputs are
identical to `141bfa60`, so the other results remain valid across that rebase
and this documentation update. The same macOS arm64/Rust 1.99.1 toolchain and
pinned read-only ClojureScript checkout are used throughout.

Both Penpot entries at `141bfa60` again stop at the exact `core.flatten`
diagnostic shown above, in 184.58 s (`app.render`) and 184.20 s (`app.main`).
These diagnostic probes overlap native/engine verification; they establish
the boundary rather than a speed comparison. Neither entry builds natively,
and the different recursive traversal gap remains with light-cljs-551.

The final default-feature V8 comparison uses fresh release executables from
`d66755ad` before this review and `141bfa60` after it, including the integrated
master changes. With the same seed-42 200k fixture and `release bench --no-cache`
command (jobs 1), three quiet paired runs have median wall times of 7.32 s
before and 7.29 s after; ranges are 7.19–7.42 s and 7.20–7.29 s. Median
user+system CPU is 7.75 s and 7.76 s, and median peak RSS is 1,037 MiB and
1,036 MiB. Process snapshots contain no active peer builds or tests. All six
outputs retain the SHA-256 recorded above; Node and the Babashka oracle both
print `484264` for inputs 42 and 3. The final test-only rebase changes no
production input to either executable.

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
a task-owned RegExp view. Unquoted regex expressions and `re-pattern` results
are regex descriptors too (light-cljs-563, below). Unquoted instant expressions
retain their existing JavaScript evaluation. Host RegExp results export their
intrinsic pattern and flags; finite Date results export their intrinsic epoch
value as an instant. These conversions read internal slots without user getters
and charge the literal payload to the form conversion limits. Invalid Dates
are rejected.
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
keys. Host-object keys and sorted sets remain unsupported; custom
comparators (`sorted-map-by`) are described in light-cljs-561 below.
Incomparable keys raise catchable `Error`s with the legacy
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
valid calls the Rust path deliberately declines, such as large or non-keyword
`defmulti` metadata merges. The native bootstrap handles declined `js-obj` calls as described
below; other declined calls still fail explicitly. Expanding a wrapper such
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
using native form intrinsics. `seq` returns positional sequence views for map entries
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

Vars (light-cljs-589): `resolve`, `(var x)`/`#'x`, `ns-publics`, `ns-interns`
and the other analyzer-side reflections give native Vars. The analyzer resolves the symbol as
before (an absent binding gives nil, and resolving another namespace's name is
an observed dependency); the emitted unit builds the Var with the runtime's
`makeVar`/`definitionVar`, as in the V8 build. A native Var is an immutable
struct of type `cljs.core/Var` with the legacy Var's slots: `val`, a thunk that
reads the binding's root, `sym` and `_meta`. A definition's metadata is the
legacy base map (`:ns :name :doc :file :line :column :arglists`), then the
definition's reader metadata, then `:ns`, `:name` and `:test`. Taking a Var never
demands the definition: its `:test` is captured by
`__lightCompilerVarMetadata` as the legacy registry does, from a root already
initialized or else from the definition's own `:test` unit (the catalog gives
each root its `:test` unit), which is demanded only when the metadata is read;
a failure there stays failed. A macro's Var (`compilerMacroVar`) has `:macro
true` and throws when dereferenced. `deref`, invocation, `apply`, `.call`,
`.apply`, `meta`, `with-meta`/`vary-meta`, `=`, `hash`, `str`, `pr-str`,
`var?`, `fn?`, `ifn?` and `test` follow the legacy Var: dereferencing or
calling reads the root then, and the printed form is `#'ns/name`. `apply` of
a Var calls its root through the root's `call` for up to 20 arguments, as the
legacy Var's adapters do. A Var's metadata with its `:test` is materialized
once per task: `meta` and `._meta` give that one map. A Var made in one task
is recognized in another by its type's name, as a MultiFn is. The engine
type is *keyed* by `sym` (`__rt.structType`'s third argument is the field
index): Vars of one symbol are equal and hash alike also as map keys, set
members and collection elements, whichever task made them, while other
structs keep identity (see "Protocols and types"). `tests/bbp_macros.rs`
(`native_var_identity_apply_and_metadata_match_quickjs` too)
(`native_var_reflection_and_namespace_views_match_quickjs_at_every_pool_size`)
checks the reflections and the namespace views below against the QuickJS
reference at 1, 2, 4 and 8 threads, and the verbatim `DEFONCE_PENDING` fixture
of `tests/compiler/macros.rs` now passes natively.

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
Where a legacy engine registers a program's namespaces, and before every source
macro expansion, a native task gets the namespace names it lacks as data
(`MacroEvaluator::register_namespaces`, command 5 of the runtime), and a
helper's producer task gets its parent's. Each becomes a Namespace of the task's
runtime, after `cljs.user` and `cljs.core` as in the legacy runtime, so
`all-ns`, `find-ns` and `the-ns` see every namespace of the expanding program,
in program order, on whichever task and thread runs the expansion. The legacy
runtimes see what their engine registered last, which can lag the expanding
program by the namespaces analyzed since. A macro that reads the inventory
declares `(b/depend-on! :namespaces)` for the cache.

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
SameValueZero persistent map from a dispatch key to the method. Its native map
lookup in Rust invokes no user equality or getters. The key of a struct is its type's identity; of a native form its
kind (`"keyword"`, `"symbol"`, `"list"`, `"vector"`, `"map"`, `"set"`); of a
primitive `"null"`, `"boolean"`, `"number"` or `"string"`; then `"function"`,
`"array"`, and `"_"` for `default`. A JavaScript object (a mutable type's
instance, an interop object) is resolved by its type, then `object`, then
`default`. `satisfies?` asks the protocol's marker table the same way.

An immutable table snapshot holds its version, source frontier, implementation
map and this task's installed struct types (light-cljs-515). The bounded
`__rt.cljProtocolLookup` leaf checks both guards before selecting an entry.
A stale snapshot, an undeclared struct or a host object returns to the existing
JavaScript resolver. That path demands a struct's declaration before installing
pending protocol extensions; waits, failures and their effects keep their order.
Known structs select their type, then `object`, then `default`. The direct
snapshot for built-in record operations selects only a type override, retaining
the record builtin ahead of object/default extensions. Core resolver table
identities are bound once without demanding their declarations. The lookup does
not memoize calls or compiler-root reads: binding, replacement and `.call` reads
still occur at each call site.

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
every `concat` is lazy, realizing one collection at a time.
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
The compiler drains `Runtime::take_lazy_wakeups` after VM steps and queues
parked readers on their pool. Evaluators without a pool use the dependency
status polling fallback (at most 20 ms after realization ends).

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

`flatten` (light-cljs-551) walks `ISequential` branches depth-first and keeps
the original nonsequential leaves, including nil, maps, sets, strings and
arrays. A nonsequential root produces an empty lazy sequence. Native list,
vector, cons, lazy and positional/chunked views use their sequence markers;
user sequential types obtain their children through `ISeqable` and `ISeq`.
Compiler queue descriptors expose an uncounted, unchunked sequence over their
positional payload, including when a lazy thunk returns one. Retained leaves
keep their occurrence identity and metadata.

The traversal uses linked native cons worklists rather than recursive host
calls, with constant-time pushes at every depth. Each tree node's successful
expansion is cached in its own lazy cell before descending, so an exception
retry does not repeat completed parents' protocol callbacks. Lazy thunks
capture pure functions and persistent values, so helpers can publish them
and protocol/helper demands can park without replay.
Construction checks the root and prepares its children, matching the legacy
`rest(tree-seq ...)`: `mapcat`'s bounded argument count reads ahead four mapped
children, or a full source chunk, without walking those children. Later reads
share the engine's lazy once-cells, including the existing exception/retry,
rooting, GC and aggregate-budget contracts above.

Lazy settlements also enqueue the engine's woken readers through the compiler
pool's existing task handoff. A published traversal can demand a cold user-type
declaration or helper while another resumable task waits on it, then wake that
reader after the thunk completes. The source-macro table compares applications
and effects with QuickJS at jobs 1/2/4/8, including that shared user branch.
An earlier reader also lends its effect-lane position to the activity realizing
that shared value and its nested helpers. The engine exposes the pending
attempt's realizer as an opaque task identity; the compiler retains that wait
only until settlement. The loan targets the enclosing root activity, retaining
priority when temporary initializer entries come and go. This keeps a later
realizer's shared-atom access from waiting behind the earlier reader that needs
its result. The pooled evaluator regression holds the realizer's queued step
until its earlier reader is registered in the pool, then checks inherited lane
opening, actual reader resubmission and exactly-once effects after a cold helper.

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
The histories are independent when only one table is shared. Each supplied atom
retains a private auxiliary atom through a traced edge; that auxiliary atom owns
the shared history state. CAS attaches one state without user equality or effect
replay. Retaining a supplied atom preserves its history even after its `MultiFn`
instances are discarded. Once the supplied atom, its instances and other references
to the history are unreachable, the auxiliary state, journal and captured method
values can be reclaimed together; no compilation-wide registry roots them.
Constructor-supplied method and preference maps seed each view;
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
Fixed native calls classify callable collections with `cljCallable` and read
the actual `.call` member with `cljCallMember` before evaluating operands.
The engine delegates getters and Proxies to its ordinary saved property frames;
the enclosing frame retains the callee, member and operands across waits.
Multimethods select one shared consuming-task dispatch method per unit.
No adapter closure or argument array is created for an ordinary fixed call.
The differential
table includes an 81-call generated helper whose method effects run exactly
once. Constructor-tree flattening also handles replaceable core functions:
it prepares each callee and method before operand effects, then invokes that
saved callable after its operands. This preserves core writes and getter
order while keeping generated syntax shallow, including nested replaceable
core calls around constructors. Method-function arguments retain consuming-task
reads even when global core-write tracking makes `-add-method` replaceable.
Function roots reading dynamic core settings such as
`*print-length*` also publish, resolving the setting in the consuming task.
Native compiler-root reads use `cljReadRoot` to check the task's live binding
overlay and root property on every access. A ready bundle's exact installed
getter can read its live product directly; replacing the root/getter or public
registry retains ordinary property behavior. Product accessors keep their own
receiver, and no dynamic root value is cached. Pending bundles and ordinary
getters use the same resumable demand/property path as before.
Bound and deferred reads retain the original Proxy's invariant checks,
including when a suspended root or product getter replaces the root with a
fixed data property or a non-configurable accessor without a getter.
A dispatch function that captures task-owned JavaScript state fails publication
explicitly; the constructor failure is shared, so later demands cannot replay
its effects. `demanded_multimethods_reject_task_owned_dispatch_without_replaying_effects`
checks this boundary at all four pool sizes.

`demanded_multimethod_dispatch_shapes_match_quickjs_at_every_pool_size` is one
32-row native/QuickJS comparison at jobs 1/2/4/8, checking identical emitted
JavaScript as well as literal results. It covers scalar and structural keys,
constructor/function keys, keyword/variadic dispatch, nested hierarchy dispatch,
global hierarchy changes,
preferences and ambiguity/missing-method errors, mutations, explicit construction,
source-frontier replacement with and without a prior declaration, defonce,
binding restoration, referenced fixed/variadic dispatch functions, retained
constructor table atoms, native type tags, shared and independently shared
constructor tables, and direct/first-class predicates on functions, constructors,
protocol callables, multimethods, callable collections and noncallable values.
External `reset!`/`swap!` replacements are reconciled on immediate table reads and
mutations through existing instances, as well as on later construction; swap
callbacks run once.
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

## Native sequential tree traversal (light-cljs-551, 2026-10-28)

Both documented Penpot entries now pass `core.flatten`. With the five absolute
read-only roots, `--jobs 1 --no-cache` and every budget in the reproduction below,
`app.render` and `app.main` both exit 1 at this identical next diagnostic:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/render_wasm/api/enums.cljs:15:25: macro expansion: while expanding cljs.core/js-obj: big-boy-pants-js: unsupported operation: bootstrap macro js-obj (bytes 712..738)
```

The shim's `app.common.render-wasm.enums/serializers` generates a wide `js-obj`.
The Rust bootstrap expander deliberately declines more than eight pairs, and
the native backend has no bootstrap fallback for that call. A nine-pair
`--compile --text` probe repeats the error; the unchanged V8 compiler emits
an application whose ninth property reads as `9` in Node. This is a different
bootstrap-expansion boundary, tracked by light-cljs-553. Native Penpot
compilation remains incomplete there; research was unchanged.

Checks before the reviewed sequence integration used source `854b4555`, based
on `b3ef562c` plus the sequence review fixes through `61952e05`, on macOS arm64
with Homebrew Rust/Cargo 1.99.1 and
`CARGO_BUILD_JOBS=3`. Both native-only Penpot commands were rebuilt and repeated
on that source: `app.render` took 206.59 s and `app.main` 207.75 s, with concurrent
acceptance tests running. These are failure reproductions, not complete-build
performance comparisons.

The native-only evaluator/macro/reader/Proxy selection passes 188 tests with
`LIGHT_CLJS_STRESS_SEEDS=12` and three test threads. The combined native/QuickJS
selection passes 490 tests with the same 12 seeds, including identical emitted
applications and effects at jobs 1/2/4/8. That broad combined result is reused
across the final queue-only lazy-normalization addition, which separately passes
four combined library regressions and all 64 engine form/lazy tests; the final
native-only selection includes it. The final engine source passes all 2,169 tests
and seven doctests. The 3,000-level published tree starts with a lazy queue,
survives dropping its owner and collection, and is read by two fresh tasks.

The complete default V8 and QuickJS-only runs report 2,133 and 2,056 passes,
respectively, each with the existing `playground_crate_checks_for_wasm32`
workspace-path failure. Root and engine formatting, JavaScript syntax, and
strict V8/QuickJS Clippy pass. Strict engine all-target Clippy retains the nine
existing diagnostics; engine and native checks pass allowing only `deprecated`,
`clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices`. Engine-free
strict library/binary Clippy passes; its all-target check still fails unchanged
legacy `Evaluator` API calls in examples and benchmark/test helpers.

The default V8 release compiler was built with `LIGHT_CLJS_V8_CACHE=require`.
For the seed-42 200k fixture, three quiet interleaved pairs of
`release bench --no-cache` compare the saved `e28daa30` binary with the final
integrated source. Median wall time is 7.05 s before and 7.08 s after (1.0043x,
+0.43%); ranges are 6.98–7.27 s and 7.03–7.12 s. Two later pairs were discarded
when a peer suite started. The difference is within the observed variation.
Emitted JavaScript is byte-identical, and Node and the Babashka oracle both
print `484264`.

### Integration with the reviewed sequence implementation

The flatten branch was rebased onto master `a4e173db`, dropping its older
copies of light-cljs-546 in favor of the final reviewed implementation.
The eleven flatten, queue-view and lazy-reader commits apply without conflicts;
the queue/record equality, counted range, `IStack` and `nthrest` fixes remain
intact. On rebased source `da76220f`, the combined native/QuickJS library, CLI,
macro, reader and Proxy selection passes all 492 tests (one normally ignored)
with `LIGHT_CLJS_STRESS_SEEDS=12` and three test workers. This includes both
the flatten and expanded sequence-predicate differential tables at jobs
1/2/4/8. Compiler/engine formatting, JavaScript syntax and direct runtime checks
also pass. The native-only evaluator/lane/macro/reader/Proxy selection passes
all 195 tests with twelve stress seeds. All 2,180 engine tests and seven
doctests pass. Strict engine Clippy retains the same nine diagnostics;
allowing only the three established categories passes all engine targets.

The native CLI was rebuilt from this source and both Penpot entries repeated
with the same five absolute read-only roots and every documented budget,
`--jobs 1 --no-cache`. Both pass flatten and exit 1 with the identical wide
`js-obj` diagnostic above, still tracked by light-cljs-553. `app.render`
takes 205.19 s while the engine suite runs concurrently; `app.main` takes
205.40 s. Research is unchanged.

Fresh complete QuickJS-only and default V8 nextest runs on `dac6136a` pass
2,056 and 2,133 tests respectively, each failing only the unchanged
`playground_crate_checks_for_wasm32` workspace-path check. The temporary
ClojureScript symlink pointed at the pinned `a5e94c98` checkout and was removed
after verification. Both legacy
configurations pass strict all-target Clippy. Combined native/QuickJS
all-target and selected native-only Clippy pass with only the established
engine diagnostic categories allowed; strict combined lint reports the same
eight engine-library diagnostics. Engine-free strict library/binary lint
passes, while its all-target check retains the unchanged benchmark's legacy
`Evaluator` API errors. No lint suppressions were added to source.
The wasm check uses the installed Rust 1.99.0 toolchain through
`LIGHT_CLJS_CARGO`, with `CARGO_HOME=/Users/admin/.cargo`.
All other checks use Homebrew Rust/Cargo 1.99.1, Node 26.12.1, three build
jobs, three native/engine test workers and eight legacy nextest slots.
The commits after `da76220f` change documentation only; all these results
cover the same production source and test inputs. Neither the legacy emitter
nor `src/js_runtime.js` differs from master.

The rebuilt default V8 release compiler was timed against the saved
`e28daa30` executable on the same seed-42 200k fixture, using
`release bench --jobs 1 --no-cache`. One quiet pair takes 7.06 s before and
6.89 s after (0.9759x); CPU time is 7.49 s and 7.40 s, with peak RSS
1,015.02 MiB and 1,040.08 MiB. Process checks before, between and after these
two runs contain no peer builds, tests or compiler measurements. A later
pair was discarded when a peer V8 measurement started. This comparison has
one accepted pair. Emitted JavaScript is byte-identical, and both executables'
applications and the Babashka oracle print `484264`.

### Exception retries and pooled-reader review fixes

The source differential now retries a throwing descendant under completed
`ISeqable` and user `ISeq` parents. Before the fix, the native parent counter
was `2` against QuickJS's `1`, and the successful user sequence callbacks
repeated on retry. Per-node lazy walk cells retain those completed expansions;
all retry results and effects now match QuickJS at jobs 1/2/4/8.

Linked native cons worklists replace copying list prepends. The publication,
owner-drop and GC regression tests both 5,000- and 10,000-level trees under the
64 MiB managed-heap limit and a 30-second bound per depth. Admitted allocation
is 68,600,656 and 118,380,570 bytes respectively (1.73x when depth doubles).
The deterministic pooled-reader regression forces a later realizer to park
inside a cold helper's initializer, then lends an earlier reader's lane. It
exposed a loan that expired when the temporary initializer entry ended. The
loan now follows the enclosing root activity through later shared-atom access.
Both tasks are resubmitted by actual pool wakeups, with one helper compilation
and exactly one thunk effect and atom increment.

Source `070a3ab3`, rebased onto master `653a5052`, passes all 361 active combined
native/QuickJS library tests (one ignored), all 61 native-only evaluator tests,
and the full flatten differential again. Before the rebase, the complete
native selection passes 196 tests and the combined selection passes 492 tests
with twelve stress seeds; all 2,180 engine tests and seven doctests pass.
The rebase adds an optional release stress feature, equivalent cfg gates for
the tested debug and ordinary release configurations, an ignored generated
project test and documentation. It changes no active production code or
dependency versions for these checks, so the full seeded/engine results are
reused after inspecting that delta. Formatting, JavaScript syntax and direct
runtime checks pass.

Both rebuilt native Penpot entries pass flatten and reproduce the exact wide
`js-obj` diagnostic above, with the five absolute read-only roots, every budget,
`--jobs 1 --no-cache`. `app.render` takes 202.18 seconds and `app.main` 205.73
seconds while native stress checks run concurrently. These results are reused
across the equivalent cfg-only rebase; research is unchanged. Neither entry
completes, and light-cljs-553 remains the existing follow-up for that different
bootstrap gap.

Fresh full default V8 and QuickJS-only nextest runs on the rebased source pass
2,133 and 2,056 tests respectively. Each fails only the existing
`playground_crate_checks_for_wasm32`: Rust/Cargo 1.99.0 resolves the excluded
worktree playground crate to the main checkout's workspace and rejects it
before compiling source. The diagnostic matches the prior baseline, and the
playground manifests and check are unchanged.

Default V8 and QuickJS-only strict all-target Clippy pass. Combined native and
QuickJS all-target Clippy passes with only the existing engine categories
`deprecated`, `clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices`
allowed; strict combined and engine checks retain their eight library and nine
all-target baseline diagnostics. Native selected-target Clippy passes with
those categories allowed. Engine-free strict library/binary Clippy passes;
its all-target check retains the unchanged legacy evaluator API errors in
examples, benches and tests. No source lint suppressions were added.
Checks use macOS arm64, Homebrew Rust/Cargo 1.99.1, Node 26.12.1, three Cargo
build jobs, three native/engine test workers and eight nextest slots. The wasm
check uses the installed 1.99.0 toolchain with `CARGO_HOME=/Users/admin/.cargo`.
The ClojureScript source symlink pointed at pinned `a5e94c98` during checks and
was removed afterwards.

Three quiet, interleaved pairs compare the saved pre-review default V8 release
executable with the rebuilt review-fix source on the seed-42 200k fixture,
using `release bench --jobs 1 --no-cache`. Median wall time is 7.24 seconds
before and 7.17 seconds after (0.9903x); ranges are 7.17–7.40 and 7.12–7.26
seconds. Median user+system CPU is 7.78 and 7.66 seconds, and median peak RSS is
1,019.0 and 1,004.6 MiB. The difference is smaller than the observed variation.
Process snapshots before and after every accepted run contain no peer builds,
tests or compiler measurements. An initial guard refused to start while peer
tests were active. All six emitted applications are byte-identical, with
SHA-256 `1a75bc87b26f312196671d4de9ed73110fc8aef077749fd8b6d5ce352bf4524e`;
Node and the Babashka oracle both print `484264` for inputs `42` and `3`.

The final rebase onto master `6cd56b89` adds only the `nthrest` source
differential. On `66525b00` that table passes in both combined native/QuickJS
and native-only configurations at jobs 1/2/4/8. Final formatting and combined
all-target lint pass with the same baseline engine categories allowed. No
production code, dependency or benchmark input changed in this test-only
rebase, so the complete suites, Penpot reproductions and timings above remain
valid. Subsequent changes only record this verification.

## Wide native object bootstrap (light-cljs-553, 2026-10-28)

The native backend now expands `js-obj` calls beyond eight pairs. The small
Rust expander and legacy bootstrap path retain their existing behavior. A
native bootstrap builder is installed only when that Rust expander declines a
call. It consumes the rooted `&form` directly, avoiding a VM argument frame
proportional to the operand count, and uses bounded native list/vector builders.
The bootstrap retains immutable builder code identity; its callable handle is
resolved and cached in the active native task. Helper producers never call a
handle owned by the bootstrap's task. A lent context keeps its cached handle,
including across new bootstrap bindings, without rebuilding the function.
The native bridge keeps its last real map environment when a bootstrap call
receives an unused nil environment.

The builder preserves the legacy split: through eight raw pairs, duplicate
keys replace whole pairs; wider inputs associate into an array map, retain the
first stored key, and promote after nine distinct keys. Existing native form
equality, hashing and legacy traversal determine duplicate keys and order.
Computed expression keys bind before the initial string properties; symbol
assignments precede computed-key assignments. Odd trailing operands remain
ignored. Source occurrences, metadata, generated names and aggregate budgets
use the ordinary native macro bridge. Application emission remains conventional.

The table-driven source fixture compares exact JavaScript, warnings and Node
effects with QuickJS at jobs 1/2/4/8 and twelve scheduling seeds. It includes
the eight/nine boundary, generated serializer objects, promotion with duplicate
keys, full hash collisions, structural and computed keys, characters and exact
UTF-16 strings, nested objects and application-side `clj->js`. A cold helper
evaluates a wide object while retaining operand effects and input identity.
Separate checks cover 8,192 pairs with the default VM stack, invalid keys and
values with their original locations, aggregate generated-node denial, and
the exact retained key occurrence and generated-name sequence.

The first subsequent Penpot gap was the garden compiler's private, documented
`defmulti`. Native compilation now also merges small keyword metadata maps
with attributes and docstrings, retaining their legacy precedence and metadata
attached to the original metadata map itself. The merge declines immediately
on the ninth distinct key, before array-map promotion could change its order;
overriding an existing key in an eight-entry map remains supported. Metadata
maps without additions retain their original representation, including maps
with more than eight entries. The default engine still uses its existing
fallback for those metadata merges.

### Penpot builds and remaining boundary

**`app.render` builds completely on the native backend at jobs 1 and 4.**
Both native runs emit identical, syntactically valid JavaScript. These runs
use source `f7f9aef0`, macOS arm64, Homebrew Rust/Cargo 1.99.1, and a native-only
release binary built with `--locked --release --no-default-features --features
bbp-macros --bin light-cljs`. The five absolute read-only roots and all budgets
are those in the Penpot reproduction above, with `--compile --esm --optimize
none --no-cache --stats`. No other builds or tests run during these timings.

`/usr/bin/time -l` reports wall/user/system seconds and maximum resident bytes:

| Backend | Jobs | Wall s | User s | System s | Maximum RSS MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| Native | 1 | 12.87 | 12.41 | 0.45 | 404.6 |
| V8 | 1 | 1.96 | 2.31 | 0.15 | 401.6 |
| Native | 4 | 8.76 | 11.44 | 0.79 | 481.8 |
| V8 | 4 | 1.27 | 3.41 | 0.21 | 593.8 |

These are single complete-build measurements. V8 output is identical at jobs
1 and 4 too. Each output has 12,680,976 bytes. Native SHA-256 is
`0a5320e48cd525cce43524c9fde45c2dd71ace555525014b98767b9b363e330b`;
V8 SHA-256 is
`d352af3a2bb2d35f1447f3d4fc6592de609dc30e92c3424f7321afc607a8d9a3`.

The native/V8 comparison has exactly 47 changed internal `reify` IDs and 16
object-property reorderings. Aligning unique object properties by name and
those numeric `reify` arguments leaves identical parsed trees (1,608,373
nodes), with identical property expressions. Their property evaluation and
enumeration order differs at the sixteen objects listed below. The tables
record the exact build differences.

For the four V8 `reify` IDs `73125 131632 132255 132291`, native subtracts 32.
For these 43 V8 IDs, native subtracts 64; all other `reify` IDs agree:

```text
198138 199249 199284 199310 229640 244565 249745 249812 250514 251051
251099 361169 361189 362127 362212 362296 362373 362437 364131 364270
364369 364393 364461 364507 364522 364564 364735 379949 380013 380203
385842 386766 386826 386864 432450 432514 432594 432680 475625 475668
476038 476502 476588
```

Object locations are generated JavaScript line numbers at `f7f9aef0`.
The last column gives native order as one-based indices into the V8 order;
each property's expression is unchanged. Seven marker objects share one row.

| Lines | V8 property order | Native permutation |
| --- | --- | --- |
| 117384 | width height children x id data-mapping transform data-colors y ref | 7 1 6 9 4 10 5 2 8 3 |
| 120565, 120576, 120587, 120598, 120609, 120629, 120649 | refX children orient id viewBox fill refY fillOpacity markerWidth markerHeight | 5 1 3 8 10 6 4 7 2 9 |
| 120730 | width height children patternTransform x id viewBox preserveAspectRatio y patternContentUnits | 7 1 8 10 4 9 5 6 2 3 |
| 141005 | width height className x id href style decoding y | 1 7 8 9 4 3 6 5 2 |
| 143016 | width maskUnits height className children x id data-old-y data-old-width data-old-x y data-old-height | 12 1 9 2 8 11 6 10 4 7 3 5 |
| 144407 | xmlns:penpot width xmlns height children viewBox style xmlnsXlink fill version | 6 2 10 7 8 3 9 1 4 5 |
| 144490 | xmlns:penpot width xmlns height className children viewBox xmlnsXlink fill version | 7 2 10 8 3 5 9 1 4 6 |
| 144542 | width xmlns height className children viewBox xmlnsXlink fill version | 6 1 9 7 2 4 8 3 5 |
| 144587 | width xmlns height children id viewBox style xmlnsXlink fill version | 6 1 10 7 8 2 9 5 3 4 |
| 144630 | width xmlns height children viewBox style xmlnsXlink fill version | 5 1 9 6 7 2 8 3 4 |

`app.main` passes both former bootstrap errors and exits 1 at this next,
different runtime-service boundary:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/plugins/flags.cljs:37:3: macro expansion: while expanding app.util.object/add-properties!: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/plugins/flags.cljs:37:3: macro app.util.object/add-properties!: big-boy-pants-js: Error: unsupported operation: native macro runtime core.memoize; definition /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/util/object.cljc:151:1 (bytes 1094..1786)
```

The same native-only release build, roots and budgets at jobs 1 take 11.58 s
wall, 11.10 s user, 0.43 s system and 383,713,280 bytes maximum RSS. This is a
failure reproduction, not a complete-build comparison. A reduced source macro
that memoizes `gensym` reproduces the missing service. The focused follow-up
is light-cljs-556. Research is unchanged.

### Default V8 build check

Three quiet interleaved pairs compare the saved default V8 release binary from
`b73d0059` with `f7f9aef0` on the seed-42 200k fixture, using `release bench
--jobs 1 --no-cache`. Median wall time is 6.80 s before and 6.80 s after
(1.000x); ranges are 6.76–6.84 and 6.79–6.84 s. Median user+system CPU is
7.31 and 7.30 s; median maximum RSS is 1,024.7 and 1,035.6 MiB.
All six outputs are byte-identical with SHA-256
`1a75bc87b26f312196671d4de9ed73110fc8aef077749fd8b6d5ce352bf4524e`.
Node and the Babashka oracle both print `484264` for inputs `42` and `3`.

### Verification

The combined native/QuickJS library and `bbp_macros` suites pass 363 and 122
tests respectively, retaining their two existing ignored stress cases.
Native-only `bbp_macros` passes 120 tests (one existing ignored generated
project case); all 61 native evaluator tests pass. Both configurations use
`LIGHT_CLJS_STRESS_SEEDS=12`, `CARGO_BUILD_JOBS=3` and three test workers.

Full default V8 nextest passes 2,133 tests and QuickJS-only nextest passes
2,056, each failing only `playground_crate_checks_for_wasm32`. The exact
baseline remains Rust/Cargo 1.99.0 rejecting the worktree playground crate as
belonging to the main checkout workspace before compiling Rust. The test,
playground manifests and script are unchanged. Both full commands use
`--locked -j 8 --no-fail-fast`; default V8 uses `LIGHT_CLJS_V8_CACHE=require`.
The pinned WASM command uses `LIGHT_CLJS_CARGO` with its installed 1.99.0
`RUSTC`, library path and `CARGO_HOME=/Users/admin/.cargo`.

Default V8 and QuickJS-only strict all-target Clippy pass. Combined strict
Clippy retains eight unchanged engine-library diagnostics. Combined all-target
and native selected-target Clippy pass allowing only `deprecated`,
`clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices` after
`-D warnings`. Engine-free strict library/binary Clippy passes; its all-target
check fails on unchanged `examples/evaluator.rs:15` calling unavailable
`Evaluator::print` and `Evaluator::evaluate`. No source lint suppressions were
added. Rust formatting, native runtime JavaScript syntax and the direct
JavaScript runtime tests pass.

Checks use Homebrew Rust/Cargo 1.99.1, Node 26.12.1 and Babashka 1.12.218.
The ClojureScript source link points at pinned `a5e94c98` during verification
and is removed for hand-back. Production source and tests are unchanged since
`f7f9aef0`; subsequent commits record these results and reuse the checks.

### Review-fix verification

Source `04daaa28` resolves bootstrap builders in the active task and preserves
the metadata map's own metadata. The producer regression uses two evaluators
from one native runtime, then explicitly parks and lends the producer context;
each task owns its handle and cache hits evaluate no builder code. Source-level
wide-object differentials additionally demand a second cold helper and assert
actual lending and borrowing at jobs 2/4/8. They retain exact JavaScript,
warnings and Node effects against QuickJS at jobs 1/2/4/8 with twelve seeds.

Complete-expansion differentials cover metadata-bearing metadata maps,
including an empty original map and attributes whose own metadata is ignored.
Boundary rows cover 8/9/10 original keys, an eight-key override, insertion of
a ninth key, and a 20,000-key map that is declined. Source applications compare
nested metadata, eight-key overrides, and unchanged nine/ten-key maps against
QuickJS at all four job counts.

Combined native/QuickJS library and macro suites pass 365 and 123 tests, with
their two existing ignored cases, using twelve stress seeds and three test
workers. Full default V8 nextest passes 2,133 tests and reproduces only the
same `playground_crate_checks_for_wasm32` workspace failure described above.
Default V8 strict all-target Clippy passes.

An additional unfiltered native-only library run passes 288 tests and fails
27 legacy-oriented tests. Repeating it with the handoff production source
`231652c6` in this worktree passes 286 and fails the same 27 tests. Every
failure's first diagnostic is identical after normalizing process-local
`SourceId`s. They require unsupported direct bootstrap macros, legacy reader
support or fake legacy host services; the combined configuration passes them.
This baseline run restores both source files before further checks. The native
evaluator's 61 tests and the new producer/bounded-metadata regressions pass
within the unfiltered current-source run.

Native-only `bbp_macros` separately passes 121 tests with twelve stress seeds
and its existing ignored case. QuickJS-only full nextest passes 2,056 tests,
with only the same WASM workspace failure; strict all-target Clippy passes.
Native strict Clippy reports the same eight engine diagnostics, with identical
locations and categories as the handoff. Native selected-target and combined
all-target Clippy pass allowing only `deprecated`,
`clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices`. Engine-free
strict library/binary Clippy passes. Its all-target check encounters unsupported
legacy evaluator methods; a focused compiler-benchmark check reproduces the
same seven method-not-found diagnostics on both `231652c6` and current source.
Formatting, direct JavaScript runtime tests and all four emitted Penpot module
syntax checks pass. These checks retain the toolchain and source pin above.

The rebuilt native-only release CLI again **builds `app.render` at jobs 1 and
4**. With the same five absolute roots, budgets and commands above, the quiet
`/usr/bin/time -l` measurements are:

| Backend | Jobs | Wall s | User s | System s | Maximum RSS MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| Native | 1 | 14.36 | 13.79 | 0.55 | 405.0 |
| V8 | 1 | 2.13 | 2.48 | 0.16 | 409.5 |
| Native | 4 | 9.34 | 12.22 | 0.84 | 471.2 |
| V8 | 4 | 1.30 | 3.49 | 0.22 | 597.3 |

Each output is byte-identical to its corresponding earlier backend output,
with the same sizes and SHA-256 values above. Thus all 47 internal `reify` ID
changes and all 16 property reorderings are unchanged; this metadata-bound fix
resolves none of those reorderings. Light-cljs-557 owns them. `app.main` at
jobs 1 still exits 1 with the exact `core.memoize` diagnostic above, in 12.18 s
wall, 11.65 s user and 0.48 s system, with 368.5 MiB maximum RSS. The existing
light-cljs-556 owns that different runtime-service gap; no new pellet is filed.

The review-fix default V8 guard compares the saved handoff binary (production
source `f7f9aef0`) with `04daaa28`. Three quiet interleaved pairs on the same
seed-42 200k project use `release bench --jobs 1 --no-cache`. Median wall time
is 7.60 s before and 7.59 s after (0.999x), with ranges 7.27–7.67 and
7.17–7.61 s. Median user+system CPU is 8.14 and 8.10 s; median maximum RSS is
1,009.3 and 1,039.1 MiB. All six outputs retain SHA-256
`1a75bc87b26f312196671d4de9ed73110fc8aef077749fd8b6d5ce352bf4524e`.
Node and the Babashka oracle both print `484264` for inputs `42` and `3`.
No peer builds or tests overlap these timings or the Penpot measurements.

## Exact native/V8 emission (light-cljs-557, 2026-10-28)

The 553 `app.render` differences above are resolved. Fresh release builds at
jobs 1 and 4 emit byte-identical JavaScript **and complete source maps** under
both backends. The seed-42 generated 200k project also matches in both files
at both job counts. These comparisons use the five read-only Penpot roots,
budgets and direct-CLI commands documented above, `--no-cache`, and
`--source-map-include-sources`. Penpot uses `--optimize none`; generated uses
`--optimize advanced`. Mac arm64, Homebrew Rust/Cargo 1.99.1, locked dependencies,
and the required existing V8 machine cache were used.

Every difference falls into these five classes:

| Class | Cause and correction |
| --- | --- |
| 16 reordered property lists | Macro-created native hash maps/sets traversed the engine's spread-hash trie. Rumext constructs property templates and operands by traversing these maps, making the difference observable in evaluation and enumeration order. Native macro constructors now select the existing legacy hash-path traversal view, including direct intrinsics and empty maps that later promote. Positional literals retain their positions; the engine's ordinary internal map contract is unchanged. |
| 47 shifted `reify` IDs | `with-meta` with explicit `nil` discarded the metadata annotation. The custom record macro strips metadata from 2 Point, 6 Matrix, 8 Rect and 16 Shape fields. The missing 32 `Nil` and 32 `WithMeta` IR nodes shifted later expression IDs by 32, then 64. A compiler-specific intrinsic retains explicit nil metadata, with ordinary core-protocol dispatch; ordinary engine `cljWithMeta` is unchanged. No gensym or task counter was responsible. |
| Primitive source positions | The legacy bridge reuses the latest equal scalar argument's provenance, including newly constructed scalar results. Native compiler import uses a call-local span annotation for this rule. Exported input Datums, their reader categories and their public occurrence spans retain their original identities. A retained whole input collection restores its original subtree. |
| Repeated keyword source positions | Imported literal keywords are interned on the legacy side, so repeated equal keyword arguments use the last occurrence. The compiler span annotation matches this rule for imported keywords. Runtime keyword construction remains fresh. |
| Shared-Datum source positions | Anonymous-function parameters can share one Datum across several source occurrences. Native remembering stopped at the first identity; legacy remembering retains the last occurrence. Native remembering now visits all occurrences, including reused subtrees. |

After the collection/primitive fixes, Penpot had only its 47 ID changes and
45 mapping differences (15 legacy-only segments, 30 changed origins).
Generated JS was already identical, with 42 remaining mapping differences
(7 legacy-only segments, 35 changed origins). The explicit metadata,
interned-keyword and shared-Datum fixes remove all of them. There are no
remaining JS, metadata or map-field differences and no accepted exceptions.

With fixed map basenames `render.map` and `generated.map`, all four builds of
each workload (two backends, two job counts) have these hashes. The JavaScript
includes its `sourceMappingURL` line; maps include sources. Paths affect hashes.

| Workload | File | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| `app.render` | JS | 12,681,008 | `0cc8853beec0a1a4ceb69e202365d76a42b50889460201c225ae654277dcb7e1` |
| `app.render` | map | 9,834,037 | `a81ff610b257f7eae0efdace3e08470a84884a1b0c022f13bcbac81cb8d86868` |
| generated 200k | JS | 3,456,940 | `d260017e883e3ec20d63eb933bb14deac41664920b5e59e81e7b5c6276ebc0d1` |
| generated 200k | map | 15,037,740 | `7f4deb7d1cdd3ddea479de94a28fdcbaf1025ae01dc40a0db75cebd8e8b1a813` |

Five small `bbp_macros` regression tests compare complete JS, source mappings
and warnings against the available legacy backend at jobs 1/4 and optimization
levels none/prepare/advanced. They cover map promotion and explicit hash-map
construction with observable property effects, set traversal, new primitives,
repeated keywords versus fresh keyword construction, shared anonymous-function
parameters, and explicit nil metadata before `reify` through direct, saved and
`apply` calls. The five tests pass against V8. The default JavaScript runtime
has no changes; all compatibility machinery is confined to native compilation.
The existing positional-literal matrix and exact sequence-element identity/span
assertions remain unchanged: compiler mapping annotations never replace those
native input values. The annotation storage and import branch are feature gated
out of the default build.

Annotations follow reused subtrees through subsequent native and compiler-owned
macro expansions. The scalar table covers a nested macro handoff, an unchanged
subtree beside an equal scalar argument, and a compiler-owned `let` handoff.

Final verification uses production source `ca2ba2e2`, three build jobs and three
`cargo test` workers; compiler nextest uses eight slots while the peer is active.
The saved release binaries again match all four JS/map files at jobs 1/4. The
generated `release bench` output also matches across backends and jobs 1/4
(one JavaScript module), retaining main-module SHA-256
`1a75bc87b26f312196671d4de9ed73110fc8aef077749fd8b6d5ce352bf4524e` and
printing `484264` in all four Node executions.

- Full default V8 `cargo nextest run --locked --no-fail-fast -j 8`:
  2,133 pass, 109 skipped, one existing WASM workspace failure (254.2 s).
- Full QuickJS-only nextest with `--no-default-features --features quickjs`:
  2,056 pass, 108 skipped, the same single failure (289.8 s).
- Native-only `LIGHT_CLJS_STRESS_SEEDS=12 cargo test --locked
  --no-default-features --features bbp-macros --test bbp_macros --test
  bbp_reader --test bbp_proxy_has -- --test-threads=3`: 126 macro, 12 reader
  and two proxy tests pass; the existing large-project case stays ignored.
- Combined `bbp-macros,quickjs`, twelve stress seeds, with `--lib --test
  bbp_macros --test bbp_reader --test bbp_proxy_has --test compiler_cache`:
  all 521 tests pass (365 library, 128 macro, 12 reader, two proxy, 14 cache),
  with the two existing ignored cases.
- Engine all-target tests and doctests pass 2,180 and five tests. This result
  is reused after the final root-package fixes: no engine source or dependency
  input changed after that run. Compiler/engine formatting and all eight full
  output syntax checks pass.

The only full-suite failure is `playground_crate_checks_for_wasm32`: Cargo
reports that the worktree's `playground/crate/Cargo.toml` believes it belongs
to the main checkout's workspace. Its diagnostic matches the 553 baseline;
the test and workspace manifests remain unchanged from `01175afc`.

Strict default and QuickJS all-target Clippy pass. Strict engine/native checks
retain the eight engine-library/nine engine-test diagnostics in `deprecated`,
`clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices`. Engine and
combined all-target Clippy pass allowing only these categories; focused native
library/binary/macro/reader/proxy/cache targets pass with the same isolation.
Engine-free library/binary strict Clippy passes. Engine-free all-target and
native-only all-integration-test attempts encounter the existing missing legacy
`Evaluator` APIs (`print`, `evaluate_js`, `string`, etc.). With both legacy
features off that evaluator is an empty enum; its implementation and the
affected evaluation tests are byte-unchanged from `01175afc`. These are explicit
configuration/lint limitations, not passing broad native/engine-free checks.

The final default V8 guard has six clean, alternating-order pairs: median wall
time is 7.27 s before and 7.18 s after (0.988x), with overlapping ranges. All
twelve retained outputs keep the same module hash and print `484264`. The
[benchmark record](../benchmarks/native-macros.md) lists every retained run and
the three intervening pairs excluded for peer-build overlap.

### Nil-metadata identity and BigInteger provenance corrections

Review corrections through `bdfead5c` preserve ordinary collection identity when
`with-meta` receives nil and the collection already has nil metadata, including
an explicit nil from an earlier removal. Actual metadata removal still records
explicit nil for compiler export. Symbols and records retain their legacy copy
behavior; a map entry becomes a vector, whose subsequent nil-metadata update
keeps its identity. The differential table covers 26 value shapes through
direct, saved and `apply` calls with both nil and JavaScript `undefined`.
Undefined metadata remains distinct from null metadata for identity checks,
including subsequent metadata updates. An engine fixture separately checks the
original occurrence identity and exported explicit nil after a real removal.

The scalar-origin walk also excludes a BigInteger's internal decimal string:
the legacy bridge encodes that literal directly as a number and never decodes
the payload as a string argument. Two enclosing-`let` macro fixtures compare
complete mappings for a generated equal string and a returned BigInteger beside
that string, at jobs 1/4 and all three optimization levels. Both review findings
are reproduced by the new regressions on the handoff code before the fixes.

Fresh release builds of `bdfead5c` retain every JS/map hash above for both
workloads at jobs 1/4. All eight JavaScript outputs pass Node's syntax check; all four
generated modules print the Babashka oracle's `484264`. The six parity and
metadata-identity fixtures pass against V8. Combined native/QuickJS validation
with twelve stress seeds passes all 522 library, macro, reader, proxy and cache
tests, with the two existing ignored cases. The final engine run passes 2,181
tests and five doctests. Strict engine Clippy still reports the same eight
library/nine test diagnostics; engine and combined all-target checks pass with
only the three established diagnostic categories allowed.

Final native-only validation with twelve seeds passes 127 macro, 12 reader
and two proxy tests, retaining the existing ignored large-project case. Full
default V8 nextest passes 2,133 tests (109 skipped); full QuickJS-only nextest
passes 2,056 (108 skipped). Each has only the existing
`playground_crate_checks_for_wasm32` workspace failure, with the same diagnostic
and unchanged test/manifests as `01175afc`. Default and QuickJS all-target
Clippy pass strictly. Engine-free library/binary Clippy passes strictly;
all-target Clippy retains the existing evaluator-example missing `print` and
`evaluate` APIs and `tests/execution.rs:1772` unused-enumeration diagnostic.
Those sources are unchanged from `01175afc`. Both formatter checks pass.

The default-suite and strict-Clippy results remain valid after the final
undefined-metadata correction: that delta changes only the optional native
engine, native-gated macro tests and documentation. The final native and
combined runs, engine tests and checks above cover that correction directly.

The final default V8 timing guard retains three quiet alternating-order pairs:
median wall time is **7.05 s before and 7.05 s after** (1.000x), with overlapping
ranges. All six modules retain SHA-256
`1a75bc87b26f312196671d4de9ed73110fc8aef077749fd8b6d5ce352bf4524e`
and print `484264`. The [benchmark record](../benchmarks/native-macros.md)
contains the retained measurements and excluded first pair.
## Native memoized macro functions (light-cljs-556, 2026-10-28)

`memoize` keeps one native atom per returned function. Its state contains the
callback and a persistent map from argument vectors to successful results.
Keys use native Clojure equality and hashing, including structural collections,
sequence equality across lists and vectors, and metadata-independent equality.
Presence checks distinguish a missing entry from cached nil, false or undefined.
Zero and variadic arguments work, separate calls to `memoize` have independent
caches, and exceptions leave no entry. Recursive calls retain entries installed
by the inner call before adding the outer result.

Callbacks preserve Clojure `apply` receivers: fixed and overloaded fixed
functions receive themselves, while variadic dispatchers retain the null
receiver of their legacy `applyTo` path. Native variadic functions carry that
convention in immutable engine metadata rather than mutable JavaScript arity
properties. Differential coverage includes receiver reads, zero/variadic
calls, invalid arities, cold helpers and sequence application across the
20-argument boundary. The GC retention test roots only the memoized function;
its cached result has only a weak witness until the consumer retrieves it.
The native variadic dispatcher supplies nil for an empty rest argument,
matching the legacy `fnRest` behavior instead of constructing an empty list.
Named native dispatchers define their methods inside the named JavaScript
function. The methods capture its immutable self binding, so copying the
dispatcher to attach its apply convention preserves recursion and identity.
This also avoids capturing a forward, task-owned binding cell when memoizing
named variadic or overloaded callbacks. Method closures are allocated on each
call; their executable code remains shared. Differential rows cover returning
self, comparing self with the returned function, self-recursion and memoizing
these callbacks. The pooled fixture retains a recursive named variadic callback
and its memoized result across tasks at jobs 1/2/4/8 with seeded scheduling.

Ordinary native callbacks use consuming-task runtime reads and shareable arity
dispatchers, including inline callbacks passed through first-class constructors.
The memoized wrapper captures only its atom and resolves operations through the
calling task's runtime. Including the callback in the atom validates its
captures at construction, so an initializer cannot silently fall back to
replaying a task-local cache. As with native atoms and multimethod tables,
task-owned JavaScript objects in captures, keys or results fail the existing
shareability check; native persistent forms, metadata and atoms remain supported.

Cache access follows the compilation's existing source-order effect lanes.
The callback runs only after a miss, and a cold helper demand saves and resumes
the VM continuation before installing the result. The cache remains in the
aggregate heap budget, is rooted through its function, and becomes collectible
when the function is discarded. No native-only behavior was added to the legacy
JavaScript runtime or the default engine path.

The same property macro next required `zipmap`. Its native implementation starts
with an array map, associates keys with Clojure equality, promotes through the
existing map implementation, and stops at the shorter input. It sequences keys
before values and advances both tails after each pair, including the final
pair. Differential coverage checks duplicate structural keys and their retained
metadata, nil/false values, larger maps, unequal/empty inputs, first-class calls
and exact lazy realization effects. The pooled memoize fixture now obtains its
metadata through `zipmap` over a lazy value stream; the cold helper suspends
inside that stream without repeating any callback or generated-name effect.

### Penpot progression and remaining boundary

**`app.render` builds natively; `app.main` does not yet build.** Native-only
release source `bdbedcd4` includes the bootstrap dependency `04daaa28`, memoize
and zipmap. Both entries were run at jobs 1 with the five absolute read-only
Penpot roots, no cache, and all budgets in the reproduction above. The reduced
memoized-gensym release macro also compiles and prints `true`.

After passing both missing core services, `app.main` exits 1 with this first
non-trivial failure:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/plugins/flags.cljs:37:3: macro expansion: while expanding app.util.object/add-properties!: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/plugins/flags.cljs:37:3: macro app.util.object/add-properties!: big-boy-pants-js: TypeError: undefined is not a function; definition /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/util/object.cljc:151:1 (bytes 1094..1786)
```

The property macro uses `cuerdas.core/slug`, whose `clojure.string/escape` call
passes its transliteration map to raw JavaScript `cmap.call(null, ch)`
(`src/compiler_libraries/clojure/string.cljs:130`). Clojure invocation of native
maps works; the raw JavaScript property access has no callable adapter. This
is a different interop layer, tracked by the single follow-up light-cljs-558.
The reduced macro below reproduces the TypeError natively; the V8 compiler
emits an application that prints `y`:

```clojure
(defmacro m []
  (let [f {"x" "y"}]
    (js* "~{}.call(null, 'x')" f)))
(js/console.log (m))
```

A namespace requiring `clojure.string` and expanding
`(str/escape "àéx" {"à" "a" "é" "e"})` also fails natively and prints `aex`
with V8. No Penpot source was changed. Since `app.main` has no native output,
there is no successful native/V8 main-build comparison or timing claim.

### Verification

The final combined native/QuickJS library and `bbp_macros` suites pass 366 and
130 tests, with their two existing ignored stress cases. Both commands use
`LIGHT_CLJS_STRESS_SEEDS=12`, `CARGO_BUILD_JOBS=3` and three test workers. This
includes byte-identical QuickJS/native emitted applications at jobs 1/2/4/8,
held initializer lanes, generated names and metadata, exceptions, independent
closures, structural hash-map keys, aggregate memory limits and cross-task GC.
The rebase onto master `01175afc` adds documentation only relative to dependency
`04daaa28`; source, tests and build inputs are byte-identical to the release
build `b879bba7`, now represented by production commit `bdbedcd4`.

An extra unrestricted native-only library run before that dependency integration
hit 27 legacy-engine/bootstrap comparison failures. All 27 test names and error
signatures match a reproduced `b73d0059` baseline on the same toolchain, after
normalizing task/source IDs. Dedicated native-only evaluator and BBP integration
targets exercise the supported configuration; unrestricted legacy-comparison
library tests need the QuickJS feature.

Native-only checks pass all 62 evaluator tests, 128 `bbp_macros` tests (one
existing ignored generated-project case), two Proxy tests and twelve reader
tests. They use the same twelve scheduling seeds and three test workers.

Full default V8 nextest passes 2,133 tests and QuickJS-only nextest passes 2,056;
each fails only `playground_crate_checks_for_wasm32`. Rust/Cargo 1.99.0 rejects
the worktree playground crate as belonging to the main checkout workspace
before compiling Rust. This matches the existing baseline with unchanged
playground manifests, test and script. Both commands use `--locked -j 8
--no-fail-fast`; V8 uses `LIGHT_CLJS_V8_CACHE=require`. The WASM subprocess uses
`LIGHT_CLJS_CARGO` with the installed 1.99.0 Rust compiler/library paths and
`CARGO_HOME=/Users/admin/.cargo`.

Formatting, native runtime JavaScript syntax and the direct JavaScript runtime
tests pass. Strict all-target V8 and QuickJS Clippy pass. Combined strict lint
retains the same eight engine-library diagnostics; combined all-target and
native selected-target lint pass allowing only `deprecated`,
`clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices` after
`-D warnings`. Engine-free library/binary strict lint passes; its all-target
check fails on unchanged example and benchmark calls to unavailable legacy
`Evaluator::print`, `evaluate`, `evaluate_js`, `string` and `number` APIs. No source lint suppressions
were added. Checks use macOS arm64, Homebrew Rust/Cargo 1.99.1, Node 26.12.1,
Babashka 1.12.218 and the read-only ClojureScript source pin `a5e94c98`.

### Default V8 build check

Three quiet interleaved pairs compare the saved default V8 release binary from
`b73d0059` with the final source over master `01175afc`, using the seed-42 200k
fixture and `release bench --jobs 1 --no-cache`. Median wall time is 7.11 s
before and 7.14 s after (1.0042x); ranges are 7.08–7.16 and 7.03–7.16 s.
Median user+system CPU is 7.59 s for both; median maximum RSS is 1,039.2 and
1,045.0 MiB. The difference is smaller than the observed variation. Process
checks before and after every accepted run contain no peer builds, tests or
compiler measurements. Overlapping attempts were discarded.

All six outputs are byte-identical with SHA-256
`1a75bc87b26f312196671d4de9ed73110fc8aef077749fd8b6d5ce352bf4524e`.
Node and the Babashka oracle both print `484264` for inputs `42` and `3`.
The final native-only Penpot rerun again builds `app.render` with 12,680,976
bytes and SHA-256
`0a5320e48cd525cce43524c9fde45c2dd71ace555525014b98767b9b363e330b`,
identical to the initial native run and syntactically valid JavaScript.
`app.main` reproduces the exact light-cljs-558 diagnostic above. These Penpot
runs establish build behavior; no main-build performance comparison is claimed.

### Review-fix verification (2026-10-28)

Production source `f7c9e901` fixes the callback apply receiver and empty
variadic tail, and the cache GC test now roots only the memoized function.
Fresh combined native/QuickJS checks pass 366 library tests, 131 macro tests,
two Proxy tests and twelve reader tests; native-only checks pass 62 evaluator,
129 macro, two Proxy and twelve reader tests. Both configurations use
`LIGHT_CLJS_STRESS_SEEDS=12`, `CARGO_BUILD_JOBS=3` and three test threads, keeping
the existing ignored stress cases. The stronger receiver differential checks
its expected result with QuickJS before checking native expansion.

The full engine all-target suite passes 2,181 tests and all seven doctests.
The complete QuickJS-only suite passes 2,056 tests, and a fresh final default
V8 suite passes 2,133 tests. Each legacy run retains only the exact documented
`playground_crate_checks_for_wasm32` workspace-discovery failure. Engine and
QuickJS-only results from `d6ae8895` remain valid: the subsequent change affects
only the native variadic emitter branch and its test/documentation. Toolchains,
feature flags, the read-only ClojureScript source pin and test inputs are those
recorded above.

Strict all-target V8 and QuickJS Clippy pass. Engine strict lint retains eight
library diagnostics and nine with tests, in the same three baseline categories;
engine all-target, combined all-target and native production checks pass when
allowing only those categories after `-D warnings`. Engine-free library/binary
strict lint passes; its all-target check still fails on the unchanged evaluator
example's unavailable `print` and `evaluate` APIs. Formatting, native runtime
syntax, direct JavaScript runtime tests and both emitted Penpot syntax checks
pass. No source lint suppressions were added.

The fresh native-only release compiler builds `app.render` at jobs 1 and 4
with the same five roots and every documented budget. Both 12,680,976-byte
outputs are byte-identical to each other and the pre-review output, retaining
SHA-256 `0a5320e48cd525cce43524c9fde45c2dd71ace555525014b98767b9b363e330b`.
`app.main` at jobs 1 repeats the exact light-cljs-558 TypeError above; there is
still no native main output or native/V8 main-build timing comparison.

Three quiet interleaved pairs compare the saved pre-review V8 release binary
(`512b8a75` source) with `f7c9e901` on the same seed-42 200k fixture, using
`release bench --jobs 1 --no-cache`. Median wall time is 7.08 s before and
7.04 s after (0.9944x); ranges are 7.03–7.20 and 6.94–7.10 s. Median user+system
CPU is 7.55 and 7.56 s, and median maximum RSS is 1,014.7 and 1,014.2 MiB.
Before/after process checks contain no overlapping builds, tests or compiler
measurements. All six emitted applications retain SHA-256
`1a75bc87b26f312196671d4de9ed73110fc8aef077749fd8b6d5ce352bf4524e`;
Node and the Babashka oracle both print `484264` for inputs `42` and `3`.

### Named dispatcher self-binding verification (2026-10-28)

Source `390d1e0c` fixes named dispatcher identity and publication. Before the
fix, the new QuickJS differential returns `[false false]` natively when a
named variadic callback returns itself, instead of QuickJS's `[true true]`.
The final implementation passes six rows covering self-recursion, returning
self, comparing self to the returned function, memoization and overloaded
callbacks. The pooled memoize fixture also compares the recursive callback's
identity across tasks and cached results at jobs 1/2/4/8.

With `LIGHT_CLJS_STRESS_SEEDS=12`, `CARGO_BUILD_JOBS=3` and three test workers,
the combined native/QuickJS library and integration selection passes 366
library, 132 macro, two Proxy and twelve reader tests. Native-only checks
pass 62 evaluator, 130 macro, two Proxy and twelve reader tests. Both retain
their existing ignored stress cases. Formatting, native runtime syntax and
the direct JavaScript runtime checks pass. The full engine's 2,181 tests and
seven doctests from `d6ae8895` are reused: engine source, tests, dependency
inputs and toolchain are unchanged by this emitter-only fix.

Fresh full default V8 and QuickJS-only nextest runs pass 2,133 and 2,056
tests respectively, each failing only `playground_crate_checks_for_wasm32`.
The same installed Rust 1.99.0 WASM subprocess resolves the worktree playground
crate to the main checkout workspace before compiling it. Its exact diagnostic,
manifests, script and test match the prior review. The runs use
`cargo nextest run --locked -j 8 --no-fail-fast`, adding
`--no-default-features --features quickjs` for QuickJS and
`LIGHT_CLJS_V8_CACHE=require` for V8, with the same pinned ClojureScript
source and WASM toolchain override recorded above.

Strict V8 and QuickJS all-target Clippy and engine-free library/binary lint
pass. Combined strict lint reports the same eight engine-library diagnostics;
combined all-target and native production lint pass allowing only the three
established engine categories after `-D warnings`. Engine-free all-target lint
reaches 501 errors in legacy-dependent compiler fixtures plus the execution
fixture's `unused_enumerate_index` lint when no engine is enabled. Checking
both targets with pre-fix `5ac614f6` emitter source reproduces all 504 error
summary lines exactly. No source lint suppressions or test skips were added.

The rebuilt native-only release compiler again builds `app.render` at jobs 1
and 4 with all five absolute read-only roots and every documented budget.
Both 12,680,976-byte outputs are byte-identical to the pre-fix output and pass
JavaScript syntax checks, retaining SHA-256
`0a5320e48cd525cce43524c9fde45c2dd71ace555525014b98767b9b363e330b`.
`app.main` at jobs 1 repeats the exact light-cljs-558 TypeError quoted above.
There is still no native main output or native/V8 main-build performance
comparison. These repeated Penpot runs establish build behavior, with no new
performance claim while a peer suite is active.

Three quiet interleaved pairs compare the saved pre-fix V8 release binary
(`f7c9e901` source, unchanged through `5ac614f6`) with the rebuilt `390d1e0c`
production source on the same seed-42 200k fixture, using
`release bench --jobs 1 --no-cache`. Median wall time is 7.15 s before and
7.11 s after (0.9944x); ranges are 7.04–7.25 and 7.03–7.12 s. Median
user+system CPU is 7.63 and 7.57 s, and median maximum RSS is 1,055.8 and
1,029.0 MiB. The wall-time difference is within the observed variation.
Before/after process checks for all six accepted runs find no overlapping
builds, tests or compiler measurements; earlier preflight attempts stopped
before timing when a peer suite or build started.

All six emitted applications are byte-identical, retaining SHA-256
`1a75bc87b26f312196671d4de9ed73110fc8aef077749fd8b6d5ce352bf4524e`.
Node and the Babashka oracle both print `484264` for inputs `42` and `3`.
The final rebase onto master `01175afc` is a no-op; only documentation changes
follow production source `390d1e0c`, so the recorded checks cover the final
source, test and build inputs.

### Discarded named dispatcher scope (2026-10-28)

Native named dispatchers are parenthesized function expressions, including
when their value is discarded in statement position. A bare fixed multi-arity
dispatcher was parsed as a JavaScript declaration, hoisting its name into the
enclosing function. The regression checks that binding before and after the
discarded expression for fixed, variadic and mixed-arity functions. It derives
the local identifier from analysis and installs the scope probes after raw-JS
hygiene, then runs the legacy emission in QuickJS and native emission in BBP.
The pre-fix native case fails with `Error: hoisted`; all corrected cases pass.

Production commit `3647597b` rebases the memoize work onto master `c31d1b97`.
The additive test/documentation conflicts retain both changes. The native
selector merge keeps `CljWithExportedMeta = 91`, assigning `FnApplyNull = 92`
and `FnApplyReceiver = 93`; all 94 selectors remain ordered and distinct.
No native-only code enters `src/js_runtime.js` or the default emission path.

On macOS arm64, Homebrew Rust/Cargo 1.99.1, Node 26.12.1 and Babashka 1.12.218,
`LIGHT_CLJS_STRESS_SEEDS=12 CARGO_BUILD_JOBS=3 cargo test --locked
--no-default-features --features bbp-macros,quickjs --lib --test bbp_macros
--test bbp_proxy_has --test bbp_reader -- --test-threads=3` passes 367 library,
138 macro, two Proxy and twelve reader tests, retaining two existing ignored
stress cases. The earlier pre-rebase macro run was interrupted when master
advanced; these complete results replace it.

Native-only verification on the same source passes all 62 evaluator tests,
the dispatcher scope regression, 136 macro tests, two Proxy tests and twelve
reader tests. The macro/reader/Proxy command uses the same twelve seeds and
three test workers, with the existing generated-project stress case ignored.

Fresh `CARGO_BUILD_JOBS=3 cargo test --locked -p big-boy-pants-js --all-targets
-- --test-threads=3` passes all 2,182 engine tests; the matching `--doc` command
passes seven doctests. These runs cover the rebased intrinsic selector merge.

Fresh full nextest runs use `CARGO_BUILD_JOBS=3`, `--locked --no-fail-fast
-j 8`, the pinned ClojureScript symlink and the recorded WASM toolchain
override. QuickJS-only passes 2,056 tests (288.2 s); default V8 with
`LIGHT_CLJS_V8_CACHE=require` passes 2,133 (244.2 s). Each fails only
`playground_crate_checks_for_wasm32`, with the same worktree/main-checkout
workspace diagnostic. The test, playground manifests, build script and Cargo
inputs are unchanged from `fefa58b8`; these remain full-suite limitations.

Compiler/engine formatting, runtime syntax and the direct JavaScript runtime
checks pass. Strict V8 and QuickJS all-target Clippy and engine-free production
Clippy pass. Strict combined/engine checks retain eight engine-library/nine
engine-test diagnostics; all-target checks pass allowing only `deprecated`,
`clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices` after
`-D warnings`, as does native production lint. Engine-free all-target lint
retains the unsupported legacy-dependent fixture errors. No assertions, test
skips or source lint suppressions were changed.

Three quiet alternating-order V8 release pairs compare the saved handoff
binary with fresh production source `3647597b`, using the same seed-42 200k
fixture and `release bench --jobs 1 --no-cache`. Median wall time is
**7.15 s before and 7.14 s after** (0.9986x), with ranges 7.15–7.27 and
7.13–7.16 s. Median user+system CPU is 7.67/7.65 s and maximum RSS is
1,045.2/1,039.3 MiB. The difference is within run variation. Before/after
process snapshots contain no overlapping builds, tests or compilers; load
falls from 3.53/6.59/6.04 to 2.42/5.87/5.80.

All six main modules are byte-identical to the saved output, retaining SHA-256
`1a75bc87b26f312196671d4de9ed73110fc8aef077749fd8b6d5ce352bf4524e`.
Every Node run and the independent Babashka oracle print `484264` for `42 3`.

Fresh native-only release and default V8 release binaries use the five absolute
read-only Penpot roots and every documented budget, with `--compile --esm
--optimize none --no-cache --stats`. **`app.render` builds natively at jobs 1
and 4, and both outputs now match the fresh V8 jobs-1 output byte for byte.**
All three 12,680,976-byte modules pass Node syntax checks and retain SHA-256
`d352af3a2bb2d35f1447f3d4fc6592de609dc30e92c3424f7321afc607a8d9a3`.
The integrated metadata/traversal fixes from master account for the change
from the earlier native-only hash; the dispatcher scope correction does not
change application emission.

**`app.main` still does not build natively.** At jobs 1 it exits 1 with exactly
the `app/plugins/flags.cljs:37:3` / `app.util.object/add-properties!` TypeError
quoted in “Penpot progression and remaining boundary” above, including
definition `app/util/object.cljc:151:1` and bytes `1094..1786`. The callable-map
JavaScript `.call` interop remains the existing light-cljs-558 follow-up; no new
pellet is filed. This failing run takes 12.97 s wall, 12.43 s user, 0.48 s
system and 396,673,024 bytes maximum RSS. There is no native main output to
compare or successful native/V8 main-build timing claim.

All final checks cover production source `3647597b`; subsequent commits change
only this verification text. Master remains `c31d1b97`, so the final rebase is
a no-op and the recorded checks remain valid. The temporary pinned
ClojureScript source symlink is removed after verification.

## Native keep transducer and stack pop (light-cljs-560, 2026-10-28)

`keep` and `keep-indexed` now have native transducer arities. A direct
one-argument call lowers to a publishable factory, as `filter`, `remove` and
`partition-all` do; `keep-indexed` allocates its index cell when the factory is
applied to a reducer, so every reducer counts from zero. The core-value
adapters (`(let [k keep] (k f))`) share one `keepTransform`. Only nil results
are dropped (false is kept), completion and initialization delegate to the
downstream reducer, a reduced downstream result stops the reduction before the
next input is read, and other arities report `Invalid arity: n` as the legacy
runtime does. The lazy collection arities are unchanged.

Penpot's `app.main.style/css-case` then reached `core.pop`: its
`compile-concat` merges adjacent strings with `(conj (pop acc) ...)`. A new
`cljPop` engine leaf handles the stacks `cljPeek` observes: nil stays nil, a
vector drops its last element and keeps its metadata (a map entry pops to a
plain vector), an IList list is its rest, and a queue literal drops its first
element (an empty queue is itself; an imported queue's vector payload is read
through its occurrence wrapper). Empty vectors and lists throw the legacy
`Can't pop empty vector`/`list` errors; other kinds, including sequence views,
cons cells and lazy sequences, report `No protocol method IStack.-pop defined
for this value` without being realized. The native `pop` and `-pop` core
functions consult `IStack/-pop` extensions (including user types) before the
leaf; public `pop` leaves nil alone.

Differential source tests compare native and QuickJS emission:
`native_transducer_call_shapes_match_legacy` gains keep/keep-indexed rows for
nil filtering, composition with `partition-by`/`mapcat`/`filter`, callback and
reducer effect order under reduced termination, independent reducer state,
completion arities, lazy and chunked collection demand, arity and callback
errors. `keep_helpers_publish_and_resume_at_seeded_pool_sizes` publishes a
composed keep/keep-indexed helper and its step function, consumes them from two
namespaces under seeded scheduling at jobs 1/2/4/8 (six published helpers, no
per-task reconstruction, cold waits) and matches QuickJS at each size. Every
keep and keep-indexed callback, in the transducer and lazy sequence arities,
logs its input, demands a cold helper only it reads, and logs its result; each
helper's initializer logs a marker with the caller's binding, so the first
expansion's exact log shows the wait inside the callback mid-reduction, with no
replayed, skipped or duplicated effects or indices.
`native_pop_matches_quickjs_at_every_pool_size` covers the Penpot merge,
metadata, map entries, user IStack types, the protocol function, the empty
queue and unsupported kinds at jobs 1/2/4/8, plus a direct expansion over
imported queue descriptors (populated, single, empty), vectors with metadata,
lists and nil. The engine forms tests check the leaf over published
occurrences, a populated queue literal and occurrence-wrapped queue payloads.
Native `conj`, `count` and `vec` of a queue literal remain unsupported (pre-existing,
light-cljs-562).

### Penpot progression and next boundary

light-cljs-558 has not landed on master yet, so the Penpot runs use a local,
unpublished merge of `sol/native-558-559` (`bc10da48`) with this work; the
branch itself contains only the keep/pop changes. Fresh native-only release
builds use the five absolute read-only roots and every documented budget with
`--compile --esm --optimize none --no-cache --stats`.

`app.main` passes the `stl/css` keep transducer quoted in light-cljs-560, then
`app.main.style/css-case` at `core.pop`, and **still does not build natively**.
At jobs 1 and 4 it exits 1 with this exact first error:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/vendor/instaparse/abnf.cljc:129:21: macro expansion: while expanding precompile-cljs-grammar: /Users/admin/src/light-cljs/research/penpot-light/vendor/instaparse/abnf.cljc:129:21: macro instaparse.abnf/precompile-cljs-grammar: big-boy-pants-js: Error: unsupported operation: native macro runtime core.sortedMapBy; definition /Users/admin/src/light-cljs/research/penpot-light/vendor/instaparse/abnf.cljc:104:1 (bytes 5039..5064)
```

The macro runs instaparse's GLL parser at compile time, whose failure table is
`(atom (sorted-map-by >))` (`instaparse/gll.cljc:212`). Native sorted maps
support only the default comparator, so this is a missing ordered-collection
capability rather than another missing core function or transducer; it is
filed as light-cljs-561. A reduced `sorted-map-by >` macro fails natively the
same way and prints `[[3 2 1] [3 :c]]` with V8. The failing runs take 98.97 s
wall, 94.77 s user, 3.83 s system and 699,891,712 bytes maximum RSS at jobs 1,
and 54.08 s wall, 64.50 s user, 3.92 s system and 892,026,880 bytes at jobs 4
(the grammar macro now executes much more compile-time code before failing).
There is no native main output to compare or successful main-build timing claim.

`app.render` still builds natively at jobs 1 (13.94 s wall) and 4 (9.37 s), and
both outputs match a fresh V8 jobs-1 build (master release binary) byte for
byte, SHA-256 `d352af3a2bb2d35f1447f3d4fc6592de609dc30e92c3424f7321afc607a8d9a3`.

### Verification

On macOS arm64 with Homebrew Rust/Cargo 1.99.1, Node 26.12.1 and the
read-only ClojureScript pin `a5e94c98`, at source commit `2b86e4f9` on master
`8771ed4f`:

- Combined native/QuickJS library and `bbp_macros` suites pass 367 and 140
  tests (one existing ignored stress case each), with
  `LIGHT_CLJS_STRESS_SEEDS=12`, `CARGO_BUILD_JOBS=3` and three test workers.
- Native-only `bbp_macros` passes 138 tests (one ignored) with the same seeds.
- `cargo test --locked -p big-boy-pants-js --all-targets -- --test-threads=3`
  passes all 2,182 engine tests.
- Full default V8 nextest (`LIGHT_CLJS_V8_CACHE=require`, `-j 8`) passes 2,133
  tests and QuickJS-only nextest passes 2,056; each fails only the existing
  worktree `playground_crate_checks_for_wasm32` workspace diagnostic.
- Formatting passes. Strict all-target V8 and QuickJS Clippy pass; combined
  all-target, engine all-target and native library/binary/`bbp_macros` lint pass
  allowing only `deprecated`, `clippy::chunks_exact_to_as_chunks` and
  `clippy::byte_char_slices` after `-D warnings`.

No native-only code was added to `src/js_runtime.js` or the default build
path. Three alternating pairs of default V8 release builds (master `8771ed4f`
before, this branch after) run `release bench --no-cache` on the seed-42 200k
generated project: median wall time is **7.25 s before and 7.26 s after**
(ranges 7.21–7.34 and 7.22–7.31 s), median user CPU 7.39/7.40 s, and maximum
RSS 996/1,040 MiB (ranges 995–1,014 and 1,011–1,046 MiB), within run
variation. All six outputs keep SHA-256 prefix `1a75bc87b26f3121`.

The review-fix round (imported queue pop and keep callback suspension) was
verified at `7523d6c6` on the same toolchain: engine all-targets 2,183 tests;
combined library/`bbp_macros` 367/140 and native-only `bbp_macros` 138 (one
ignored each) with `LIGHT_CLJS_STRESS_SEEDS=12`; default V8 nextest 2,133
passed with only the worktree wasm32 diagnostic failing; formatting, strict
V8/QuickJS Clippy and the allowed-category combined/engine Clippy pass (strict
engine Clippy keeps the nine baseline diagnostics). The fix round changes no
default-build input: three alternating 200k pairs give median wall time
6.96 s (master) and 6.99 s (branch). Native `app.render` at jobs 1 (13.39 s)
and 4 (9.10 s) still matches a fresh V8 build, SHA-256 `d352af3a…a8d9a3`.

## One object in both worlds: raw JavaScript boundaries (light-cljs-558, 2026-10-29)

Penpot's `app.main` stopped at `clojure.string/escape`, whose raw
`cmap.call(null, ch)` invokes a native map. The first design (`.call`/`.apply`
adapters chosen at the compiled call site) went through five review rounds,
each fixing one boundary and breaking another. This section records why, and
the invariant that replaces it.

### Why patching call sites kept failing

A compiled `.call`/`.apply` site sees a receiver, a `this` operand and
positional operands; it cannot know the callee's world. The callee may be an
emitted ClojureScript function or a bootstrap core function (both read native
structs), a native collection or IFn adapter (native operands), a `js*`
function (reads fields as JavaScript properties), or a forwarder such as
`Function.prototype.call` whose eventual callee is invisible. Any decision at
the site is a finite list of callees; each round extended the list for one and
broke another. Worse, the conversion it chose made a *different object* (a
"struct view"), so `record?`, `identical?`, `this === o` aliasing and memo keys
failed whenever a view reached ClojureScript code, and nothing could map it
back, because every emitted function is a ClojureScript entry.

### The invariant

**A native form is one object in both worlds, and no value is converted at any
call boundary.** `.call`, `.apply`, direct invocation, method calls, `js*`
operands and callbacks pass native forms unchanged (only occurrence provenance
wrappers are unwrapped, as every intrinsic does); struct views no longer exist.
Raw JavaScript reads a native form's members through the engine's property
read, which defers to the task's runtime resolver exactly as collection
`.call`/`.apply` did before: a struct answers its fields, record slots,
`Object` methods and IFn `call`/`apply`; collections, keywords and symbols
answer `call`/`apply`; everything else is `undefined`. Conversion therefore has
one owner, the member read, and it is independent of who calls whom, how
deeply a call is forwarded, or which world the callee belongs to.

### Engine

`property::lookup` reaching a `Form` object with a name key in an ordinary
read returns `Read::Deferred(Deferred::Form)` naming that form (the base
itself, or the struct an `Object.create` object inherits from); an occurrence
wrapper continues the walk at the object it wraps. `read_result` (every `GetField`/`GetElem`,
including the optimized direct call's `cljCallMember`) and the native
`Action::FormMember` (built-ins that read `length`, `then`, …) push the traced
core-protocol continuation with op `jsMember` and the key. It calls the task's
resolver, which may suspend while declaring a cold type or demanding a helper
without replaying the read, and delivers the member while the original
receiver stays on the operand stack, so `o.m(args)` calls with `this === o`.
Without a resolver the member is `undefined`. Presence checks (`in`), symbol
and index keys, and ToPrimitive conversion lookups are unchanged: forms have no
ordinary properties. The former `GetField` hooks and the `cljCallMember`
special case are gone.

### Runtime

The resolver's `jsMember(x, name)` answers for a struct or record, after
declaring its type in this task: a declared field (the last field munged to
that member wins, as the legacy constructor's assignments do), a record's
`__meta`/`__extmap`/`__hash` slots, an `Object` method (one function per type
and member, dispatching on the declaring type with `this`, or the struct
`this` inherits from, as receiver), then
the IFn `call`/`apply` adapters of light-cljs-558's first rounds, whose
identities one runtime-owned atom still canonicalizes by collection family or
native type (SameValueZero keys and compare-and-set only). Collection,
keyword, symbol and multimethod receivers keep their family members.

`nativeInterop` only unwraps occurrences and converts nothing. It still
rejects explicit compiled interop with a non-struct native form, whose legacy
object layout is not modelled: as the receiver or an operand of a member read
or call (`(.-cnt m)`, `(.push arr m)`), a host call (`(js/Object.keys v)`) or
`instance?`. The exception is `.call`/`.apply`, which apply a function: their
receiver and operands pass through unchanged. Direct invocation of a
struct without declared IFn methods falls back to its `call` member (a field or
`Object` method), as legacy `f.call(null, …)` does, so the former "field named
`call`" boundary is gone too. `str` and printing of a deftype use its
`toString` `Object` method or `[object Object]`. `memoize` no longer maps
anything back. Removed: `viewOf`, the view/struct maps, `nativeCallReceiver`,
`nativeCallMethod`, the per-call operand classification and the member
registry's self-keys. The emitted type constructor is now used only by mutable
types.

### Tests

`native_raw_call_apply_matrix_matches_quickjs` now requires QuickJS-identical
output for every cell (the two documented boundary exceptions are deleted) and
adds plain emitted and core function receivers whose bodies assert `record?`
and `identical?` on struct operands, at jobs 1/2/4/8 with cold helper waits
under every stress seed. `native_object_call_and_apply_members_match_legacy`
passes a struct as its own `Object` method operand through direct and
forwarded calls; `native_borrowed_ifn_calls_match_legacy` forwards an IFn
record through nested `Function.prototype.call`;
`native_js_call_and_apply_read_struct_fields` reads fields from raw
JavaScript receivers and operands. The engine forms test resolves `jsMember`
for named and computed keys and suspends during both resolution and the call.

### Penpot progression

A fresh native-only release with the five absolute read-only roots and every
documented budget builds `app.render` at jobs 1 (16.53 s wall, 15.57 s user,
0.96 s system, 410,157,056 bytes maximum RSS) and jobs 4 (10.72 s wall,
14.38 s user, 1.63 s system, 499,662,848 bytes). Both outputs
(`app.render.js` and `chunks.json`, 12,687,975 bytes) are byte-identical to
the fresh default V8 release build at jobs 1 (2.19 s wall, 431,702,016
bytes), SHA-256
`8f55b0bd762c81193e719bd146c68d75a8052dbf2c23de5eacade3e9ef15374e`.

`app.main` passes the `clojure.string/escape` boundary and every later
raw-call site, and stops at the ordered-collection gap light-cljs-561 owns
(instaparse's `(atom (sorted-map-by >))`), the same first error master
recorded for light-cljs-560. At jobs 1 (108.77 s wall, 103.84 s user, 4.94 s
system, 700,235,776 bytes) and jobs 4 (54.79 s wall, 65.68 s user, 3.98 s
system, 874,659,840 bytes) it exits 1 with exactly:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/vendor/instaparse/abnf.cljc:129:21: macro expansion: while expanding precompile-cljs-grammar: /Users/admin/src/light-cljs/research/penpot-light/vendor/instaparse/abnf.cljc:129:21: macro instaparse.abnf/precompile-cljs-grammar: big-boy-pants-js: Error: unsupported operation: native macro runtime core.sortedMapBy; definition /Users/admin/src/light-cljs/research/penpot-light/vendor/instaparse/abnf.cljc:104:1 (bytes 5039..5064)
```

There is still no native `app.main` output to compare or main-build timing
to claim. Research was not changed.

### Verification

On macOS arm64 with Homebrew Rust/Cargo 1.99.1, Node 26.12.1, the read-only
ClojureScript pin `a5e94c98` and `CARGO_BUILD_JOBS=4`, on the source of
`5441c267` (the later commits change only documentation), master `ba7f0c89`:

- Combined `bbp-macros,quickjs` `bbp_macros` passes 148 tests and the library
  368 (one existing ignored stress case each), three test threads. The raw
  call/apply matrix requires QuickJS-identical output at jobs 1/2/4/8.
- Native-only `bbp_macros` passes 146 (one ignored) with
  `LIGHT_CLJS_STRESS_SEEDS=12`; the native `bbp_evaluator` selection passes 63,
  including cross-task raw member identity.
- `cargo test --locked -p big-boy-pants-js --all-targets -- --test-threads=3`
  passes all 2,184 engine tests, and the seven doctests pass.
- Full default V8 nextest (`LIGHT_CLJS_V8_CACHE=require`, `-j 8
  --no-fail-fast`) passes 2,133 tests in 214.3 s and fails only the unchanged
  worktree `playground_crate_checks_for_wasm32` workspace-discovery diagnostic.
- Compiler and engine formatting pass. Strict default all-target Clippy
  passes. Native `bbp-macros,quickjs` all-target and engine all-target Clippy
  pass allowing only `deprecated`, `clippy::chunks_exact_to_as_chunks` and
  `clippy::byte_char_slices` after `-D warnings`; strict engine Clippy keeps
  only those baseline diagnostics. No source suppressions, skipped tests or
  expected data were changed except the matrix's deleted boundary exceptions.

No native-only code was added to `src/js_runtime.js`, and the default
emitter path is unchanged. Five quiet alternating pairs (branch first)
compare a fresh default V8 release build of master `ba7f0c89` (built from
`git archive` in its own target directory) with the branch, on the seed-42
200k fixture with `release bench --no-cache`: median wall time is **7.46 s
before and 7.50 s after** (ranges 7.39–7.51 and 7.41–7.70 s), median user
CPU 7.58/7.65 s, maximum RSS 1,017–1,038 and 1,010–1,058 MiB, load average
2.7–4.8 with no other builds running. An earlier three-pair set under a
peer's load average of about 4.5 (7.76/8.18 s medians) is superseded. All
sixteen outputs keep SHA-256 prefix `1a75bc87b26f3121`.

### Review-fix round 1: host interop of structs (2026-10-29)

The first review found that deleting the views also deleted what they gave
host code: ToPrimitive coercion, field reflection, and two callable paths
that differed from the legacy runtime. All five fixes stay inside the
invariant: the engine and the member read own every conversion, no call
boundary does.

- **Fields are the engine's own properties.** `__rt.structType` takes a
  fourth argument, the fields' JavaScript member names (the emitter passes
  the munged names; `null` means the field names). A struct's fields are its
  own enumerable data properties under those names (the last field munged to
  a name wins, as the legacy constructor's assignments do), and a record also
  has `__meta`, `__extmap` (the extension map or `null`) and `__hash`
  (`null`) unless a field already took the name. Reads, `in`,
  `Object.getOwnPropertyDescriptor`, `Object.keys`/`getOwnPropertyNames`,
  `Object.assign` and for-in all agree, without the resolver; round 2 below
  makes the struct a frozen object. The legacy
  runtime additionally lists its own `cljs$lang$protocol_mask$partition0$`
  and `…1$` bookkeeping slots on every instance; those are not fields and
  are not modelled, so the differential reflection tests filter `cljs$lang$`
  keys on both backends.
- **Conversions go through the member read too.** `Deferred::Form` now
  applies to ToPrimitive's `valueOf`/`toString` lookups as well as ordinary
  reads, and a struct's resolver answers an `Object` method, then its IFn
  `call`/`apply` adapters, then what a legacy instance inherits from
  `Object.prototype` (`toString`, `valueOf`; not `constructor`). So
  `(js/String t)`, template literals and `1 + n` use a deftype's declared
  `toString`/`valueOf`, a methodless struct converts to `[object Object]`
  and `NaN`, and a record (which has no legacy `toString`) likewise.
- **A `call` field shadows IFn everywhere.** `invokeStruct` reads the
  struct's `call` member as JavaScript would when a field or `Object`
  method named `call` exists, so `mapv`, `update`, `reduce` callbacks and
  direct calls agree; otherwise it enters the declared IFn entries directly.
- **Record `.apply` arguments are array-like.** `nativeApplyArgs` no longer
  expands a record as a sequence: as in the legacy adapters' `Array.from`, a
  record (no `Symbol.iterator`) supplies arguments only through a `length`
  member, so `f.apply(null, record)` reports `Invalid arity: 0`.
- **Built-in-initiated reads are tested.** The engine forms test
  `native_form_builtin_member_reads_resume_without_replay` reads `length`
  through `Array.from` and `Reflect.get` and `toString` through `String`,
  with the resolver parking on a distinct dependency per read, collection
  between waits and a 64 KiB resumption, counting exactly one resolution per
  read, and a throwing resolver whose error reaches the JavaScript `catch`;
  it also checks struct reflection without a resolver.

Differential tests against QuickJS: `native_struct_host_coercions_match_legacy`,
`native_struct_reflection_matches_legacy`,
`native_struct_call_fields_override_ifn_in_callbacks` and
`native_raw_apply_record_arguments_match_quickjs` reproduce the review's
scenarios (each failed before its fix), and
`native_struct_host_interop_matches_quickjs_at_every_pool_size` runs all four
paths with the types declared by a cold helper namespace at jobs 1/2/4/8
under every stress seed, so the resolver suspends inside ToPrimitive, raw
`.apply` and callback invocation.

#### Review-fix round 1 verification

On the same toolchain, at source `de5983ee` (master `ba7f0c89`, unchanged):

- Combined `bbp-macros,quickjs` `bbp_macros` passes 153 tests and the library
  368 (one ignored stress case each); native-only `bbp_macros` passes 151
  (one ignored) with `LIGHT_CLJS_STRESS_SEEDS=12`, the native `bbp_evaluator`
  selection 63; engine all-targets passes 2,185 tests and the seven doctests.
- Default V8 nextest (`LIGHT_CLJS_V8_CACHE=require`, `-j 8 --no-fail-fast`,
  pinned ClojureScript `a5e94c98`) passes 2,133 tests in 213.8 s and fails
  only the unchanged worktree `playground_crate_checks_for_wasm32`
  diagnostic. Strict default Clippy passes; native and engine all-target
  Clippy pass with the three baseline categories allowed (strict engine
  Clippy keeps them); compiler and engine formatting pass.
- Five quiet alternating 200k pairs (branch first, load average about 3):
  median wall time **6.97 s on master and 7.06 s on the branch** (ranges
  6.94–7.13 and 7.01–7.34 s), median user CPU 7.15/7.22 s, maximum RSS
  992–1,044 and 1,010–1,038 MiB, all ten outputs SHA-256 prefix
  `1a75bc87b26f3121`.
- Native `app.render` at jobs 1 (13.75 s wall, 13.21 s user, 409,075,712
  bytes maximum RSS) and jobs 4 (9.37 s, 12.13 s user, 511,606,784 bytes)
  stays byte-identical to the fresh V8 build (2.10 s), SHA-256
  `8f55b0bd…374e` over 12,687,975 bytes. `app.main` still exits 1 with the
  exact `core.sortedMapBy` error above at jobs 1 (94.26 s wall, 90.55 s user,
  702,693,376 bytes) and jobs 4 (52.84 s, 63.14 s user, 901,103,616 bytes).

### Review-fix round 2: a struct is a frozen object (2026-10-29)

The second review found the remaining edges of "a struct is an ordinary
object": conversion of multimethods, deletion and the integrity checks, and
duplicated record slots. The model is now stated once and applied to every
object operation: **a native form is a frozen object inheriting from
`Object.prototype`**. For a struct, record or multimethod that means its
fields are enumerable, non-writable, non-configurable own data properties;
`Object.isFrozen`/`isSealed` are true and `isExtensible` false;
`Object.freeze`/`seal`/`preventExtensions` return it unchanged; a strict
write to a field is "cannot assign to read-only property", to anything else
"cannot add property: the object is not extensible"; `delete` of a field is
"cannot delete non-configurable property" (a missing key deletes as `true`);
`Object.defineProperty` accepts only a compatible descriptor of a field and
rejects additions; `Object.getPrototypeOf` is `Object.prototype`, so
`instanceof Object` holds and `setPrototypeOf` to it is a no-op while any
other prototype is rejected as not extensible. Collections, keywords and
symbols are the same kind of object with no own properties. Every other
named member (an `Object` method, the IFn `call`/`apply` adapters, and what
a legacy instance inherits from `Object.prototype`, for multimethods too)
still comes from the task's resolver, including for ToPrimitive, so `str`,
`pr-str`, `js/String`, template interpolation and arithmetic coerce a
multimethod to `[object Object]` as the legacy runtime does, and `pr-str`
of a multimethod prints `#object[MultiFn …]` like the legacy constructor
name. A record's `__meta`/`__extmap`/`__hash` slots appear once even when
a field has one of those names.

This is deliberately not the legacy runtime's mutable instance: writes,
deletes, descriptor flags, the integrity queries and the prototype identity
differ, and a multimethod exposes its two native state slots. Those cells,
and only those, are listed with both results in
`native_form_object_operations_match_quickjs`, one table of every object
operation (get, set, has, delete, keys, names, entries, for-in, spread,
assign, descriptors, define, the integrity checks and changes, prototype
read and write, ToPrimitive through `String`, templates, `Number`, `str` and
`pr-str`, `typeof`, `Array.isArray`, JSON) times every form kind (deftype,
record, multimethod, map, vector, set, keyword, symbol, list) against
QuickJS at jobs 1 and 4; the legacy `cljs$lang$` bookkeeping slots are
filtered on both sides, and the collection cells the legacy runtime answers
from a collection's internals are asserted as native snapshots. The engine
tests `native_struct_integrity_matches_own_descriptors` (descriptors,
delete, `Reflect.deleteProperty`, the freeze family, define, prototype,
slot deduplication) and `native_form_toprimitive_member_reads_resume_without_replay`
(string, number and default hints parking in both the resolver and the
returned method on distinct dependencies, exactly one resolution per
conversion, the form as receiver, a resolver exception reaching `catch`)
cover the engine side; `native_multifn_host_coercions_match_legacy` the
multimethod coercions. Each failed before its fix.
## Native sorted-map-by (light-cljs-561, 2026-10-29)

`sorted-map-by` now builds a persistent native map ordered by an arbitrary
comparator: core functions such as `>` and `compare`, user functions returning
numbers or booleans, and `comparator` results. The engine stores the entries in
sorted positional storage, as for default `sorted-map`, plus the comparator as
one more edge. GC traces it, publication walks it (`.edge(0)`: a comparator
capturing a task-owned object makes the map unpublishable and cannot enter an
atom), and metadata changes, updates and full result realization keep it.

The engine never calls the comparator. The task runtime (`src/bbp_runtime.js`)
orders keys with the legacy runtime's `fnComparator` semantics: a number
result is the order, otherwise a truthy result means less and the reversed
call decides greater. Like the legacy `sortedPosition`/`sortedEntryIndex`, a
binary search compares (entry key, query key) and one more call tests the
found position, so lookups, `assoc`, `dissoc`, `conj`, `contains?`, invocation
and construction make exactly the legacy calls in the legacy order (the
differential table logs them). A compare-equal key keeps the stored key, and
an equal value keeps the map. New `formSortedMapBy`, `formSortedComparator`
and `formSortedEntry/Insert/Replace/Remove` leaves read or edit an entry only
at the position the runtime computed. Comparator calls are ordinary JavaScript
calls on the task VM, so a comparator can demand a cold helper and park
mid-search; the saved continuation resumes without replaying earlier calls.

The core-protocol frame offers `get` (two and three arguments), `contains?`,
`assoc`, `dissoc`, `conj`, map invocation, keyword lookup and `=` with a
sorted-map-by operand on either side to the task resolver, which runs those
operations as above. A map whose comparator is nil is a sorted-map-by map too
(`formIsSortedBy`): it keeps one entry and raises the legacy `TypeError` when
two keys are compared. `conj` follows the legacy `mapConj`: nil returns the
map, a pair is one entry, and any other value is a sequence of vector entries
(a map, a record, a list, a lazy sequence) added in order, or `Map conj
requires vector entries`. Map equality follows the legacy `equiv-map`: equal
counts, then each left entry, in seq order, is found in the right map (by its
comparator when it has one); an array map compares its own value first and a
hash or sorted map the found value first (`formIsArrayMap`), which an
asymmetric user `-equiv` observes. A struct's `-equiv` on the left still takes
precedence.

`=` that meets a sorted-map-by map *inside* other collections (a vector
element, a map value, a set member, a hash-map key, a lazy-sequence item) as
the side whose keys are looked up compares it on the task VM too. The engine's
structural equality records that one map comparison as a request, like a lazy
realization request; the instruction then runs the resolver's `=` for it in a
core-protocol frame (its comparator may demand a cold helper and park) and
executes again with every answer so far. Answers are taken in the order the
comparisons are met, and a pair decided with one is not memoized, so every
comparison met calls the comparator once, as each legacy `=` does, and none is
repeated. Each new comparison re-executes the (effect-free) instruction, so `n`
such comparisons in one instruction cost `O(n^2)` engine work. The retained
answers are charged to the aggregate heap budget before their buffer grows
(a denial is the catchable heap-limit error) and released with the frame that
holds them, also on cancellation. Inside a set comparison, as the legacy set
`equiv` catches an `Error` its lookups throw and answers false, the frame
catches an `Error` the runtime's `=` (or its comparator) throws and the
innermost enclosing set comparison is unequal; any other thrown value, and a
throw outside a set comparison, propagates. Without a task
runtime (an embedder's `formEquals`) the comparison still raises `unsupported
operation: native nested equality with a sorted-map-by map`. `seq`, `rseq`, `first`, `keys`,
`vals`, `count`, `reduce`, `reduce-kv`, `into` sources, hashing, metadata and
printing need no comparator and read the sorted storage directly. `sorted?`
is true. Native `rseq` (new) reverses vectors, default sorted maps and
sorted-map-by maps, honors `IReversible/-rseq` extensions, and otherwise
throws the legacy `No protocol method IReversible.-rseq defined for this
value`. Native `compare` (new) is the default sorted-map order with its
legacy errors, and `comparator` (new) returns the legacy -1/0/1 adapter; the
emitter lowers a direct `comparator` call to a closure that captures no
runtime object, so a comparator helper built with it can publish.

Explicit boundaries: `sorted-set-by` stays an
explicit boundary together with default `sorted-set`, for which the native
runtime has no sorted set storage yet (`unsupported operation: native macro
runtime core.sortedSetBy`); the comparator machinery here could serve it once
sorted sets exist. `subseq`/`rsubseq`, `find`, `empty`, `merge-with` and
`update-in` remain unprovided native core functions for every map kind, and
`conj` of a vector that is not a pair reports the existing native map-entry
error (the legacy runtime takes its first two elements). The engine's pair
memo can still skip a comparison legacy `=` repeats when a collection
containing an already-compared *ordinary* pair is shared; with a sorted-map-by
map inside, the pair is not memoized. Nothing was added to
`src/js_runtime.js` or the default (V8) compile path.

Tests: `native_sorted_map_by_matches_quickjs` compares emission and output with
QuickJS for instaparse's atom pattern, every operation above, user/core/
`comparator`/`compare` comparators, metadata, equality in both directions,
hashing, printing, `rseq`, comparator failures with catch/finally, and the exact
comparator call log. `sorted_map_by_helpers_publish_and_resume_at_seeded_pool_sizes`
publishes a comparator whose cold helper is read only inside comparator calls
and a sorted-map-by value with a published comparator, consumes them from two
namespaces at jobs 1/2/4/8 under seeded scheduling (five published helpers, no
per-task reconstruction) and matches QuickJS at each size; the first
expansion's log shows the helper initializing inside one comparator call,
exactly once.

Review fixes (same day): `native_nested_sorted_map_by_equality_matches_quickjs`
compares nested equality (vectors, map values, sets, hash-map keys, lazy
sequences, nesting, shared vectors, short-circuits, comparator failures) and
the comparator call count of each with QuickJS. The seeded fixture adds a
helper built by `(comparator heavier?)` whose predicate's cold demand parks
inside a nested `=` (eight published helpers). `native_sorted_map_by_matches_quickjs`
gains `conj` (nil, entry lists, lazy and mapped entry sequences, bad entries,
with the call log) and nil-comparator rows;
`native_sorted_map_by_equality_direction_matches_quickjs` pins the value
direction with an asymmetric `-equiv`, top level and nested; and
`native_rseq_protocol_dispatch_matches_quickjs` covers a user `IReversible`
type and a `PersistentVector` override by direct, aliased and apply calls,
with a cold helper demanded inside `-rseq`, at seeded jobs 1/2/4/8.
`native_sorted_set_by_stays_an_explicit_boundary` pins `sorted-set-by`. The engine test
`sorted_by_maps_retain_their_comparator_across_edits_publication_and_gc`
covers the leaves, retention through publication and GC, the `.edge(0)`
publication rejection, realization and the leaf errors.

### Penpot progression and next boundary

Fresh native-only release builds use the five absolute read-only roots and every
documented budget with `--compile --esm --optimize none --no-cache --stats`, on
macOS arm64 with a peer worker's tests running (load average about 4.5).

On this branch (master plus this work), `app.main` still stops first at the
`app/plugins/flags.cljs:37:3` `app.util.object/add-properties!` TypeError
(definition `app/util/object.cljc:151:1`, bytes `1094..1786`), because
light-cljs-558 is parked and unlanded: 13.25 s wall, 12.53 s user, 0.57 s
system, 394,756,096 bytes maximum RSS at jobs 1.

With `sol/native-558-559` (`1513442d`) merged into a throwaway local branch
(deleted afterwards; none of its commits are on this branch), `app.main` passes
abnf.cljc:129's `(atom (sorted-map-by >))` and **still does not build
natively**. At jobs 1 and 4 the exact first different failure is:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/vendor/instaparse/abnf.cljc:129:21: macro expansion: while expanding precompile-cljs-grammar: /Users/admin/src/light-cljs/research/penpot-light/vendor/instaparse/abnf.cljc:129:21: macro instaparse.abnf/precompile-cljs-grammar: big-boy-pants-js: Atom(NotShareable): atom operation: cannot publish: value[0].capture(1)[0]{:regexp}{:parsers}[0]{:regexp} is a task-owned object (owned by task#1), which is not shareable (in anonymous at instruction 9 [atom_cas @ byte 23], task#1, world#0) at source 1082:33
```

(`task#5` at jobs 4). instaparse's GLL trampoline pushes parse thunks onto its
`stack` atom; they capture parser maps whose `:regexp` values native `re-pattern`
made as task-owned JavaScript `RegExp` objects, which an atom cannot hold. That
is a host-value shareability gap, not an ordered-collection one; it is filed as
light-cljs-563. The failing runs take 100.63 s wall, 96.66 s user, 3.85 s system and
705,150,976 bytes at jobs 1, and 57.13 s wall, 68.05 s user, 4.22 s system and
910,442,496 bytes at jobs 4. There is no native main output to compare or
successful main-build timing claim.

`app.render` still builds natively at jobs 1 (14.11 s wall, 13.55 s user,
410,779,648 bytes) and 4 (9.58 s wall, 12.45 s user, 511,672,320 bytes); both
12,680,976-byte outputs pass `node --check` and match a fresh V8 jobs-1 build
(master release binary) byte for byte, SHA-256
`d352af3a2bb2d35f1447f3d4fc6592de609dc30e92c3424f7321afc607a8d9a3`.

### Verification

On macOS arm64 with Homebrew Rust/Cargo 1.99.1, Node 24.22.1 and the
read-only ClojureScript pin `a5e94c98`, on master `ba7f0c89`:

- `cargo test --locked -p big-boy-pants-js --all-targets -- --test-threads=3`
  passes all 2,184 engine tests; the engine doctests pass.
- Combined native/QuickJS library and `bbp_macros` suites pass 367 and 143
  tests (one existing ignored stress case each), with
  `LIGHT_CLJS_STRESS_SEEDS=12`; native-only `bbp_macros` passes 141 (one ignored).
- Full default V8 nextest (`LIGHT_CLJS_V8_CACHE=require`, `-j 8`) passes 2,133
  tests and fails only the existing worktree `playground_crate_checks_for_wasm32`
  workspace diagnostic.
- Formatting passes. Strict all-target V8 and QuickJS Clippy pass; engine
  all-target and combined native/QuickJS all-target Clippy pass allowing only
  `deprecated`, `clippy::chunks_exact_to_as_chunks` and
  `clippy::byte_char_slices` after `-D warnings` (strict engine Clippy keeps the
  eight library/nine library-test baseline diagnostics).

Updated expectations: `unsupported_operations_are_explicit_and_do_not_fall_back`
now pins `sorted-set-by` instead of `sorted-map-by` as unsupported, and
`native_transducer_call_shapes_match_legacy` gains a `sorted-map-by` into-target
row checked against QuickJS.

The default build changes only native-only emission (`comparator` lowering).
Three alternating pairs of default V8 release builds (master `ba7f0c89` before,
this branch after) run `release bench --no-cache` on the seed-42 200k generated
project with a peer worker active (load about 3.5-4.2): median wall time is
**7.63 s before and 7.61 s after** (ranges 7.58–7.98 and 7.53–8.47 s), median
user+system CPU 8.21/8.14 s and maximum RSS 1,036/1,054 MiB, within run
variation. All six outputs keep SHA-256 `1a75bc87b26f3121…` and print `484264`
for `42 3`.

Review-fix verification (nested equality, value direction, `conj`, nil
comparators, `comparator` helper publication and `rseq` dispatch), same
toolchain, at `3c2c878c` on master `ba7f0c89`: the engine all-target suite
passes 2,184 tests (run before the new engine test, which passes on its own;
the only later engine change is one field's visibility); combined
native/QuickJS library and `bbp_macros` pass 367 and 146 tests and native-only
`bbp_macros` passes 144 (one ignored each), with `LIGHT_CLJS_STRESS_SEEDS=12`;
full default V8 nextest passes 2,133 of 2,134 (the same worktree
`playground_crate_checks_for_wasm32` diagnostic); formatting, strict V8 and
QuickJS Clippy and the allowed-category engine and native Clippy pass (strict
engine Clippy keeps its nine baseline diagnostics). The default V8 release
binary does not contain the engine or `src/bbp_runtime.js`; three
`release bench --no-cache` runs on the seed-42 200k project take 7.53-7.58 s
wall (a peer test run active) and keep SHA-256 `1a75bc87b26f3121…`. Penpot
`app.render` at jobs 1 builds natively in 14.64 s (425 MB peak memory) and
byte for byte matches a fresh V8 release build (2.25 s), SHA-256 `d352af3a…`.

Review round 2 (set-equality `Error` boundary, budget-charged answers, answers
across a later lazy realization), at `61aa9ac2` on master `ba7f0c89`: engine
all-target suite 2,188 passed; combined native/QuickJS library and `bbp_macros`
367 and 146, native-only `bbp_macros` 144 (one ignored each,
`LIGHT_CLJS_STRESS_SEEDS=12`); default V8 nextest 2,133 of 2,134 (the same
worktree diagnostic); formatting and the same Clippy results as above. Three
default V8 `release bench --no-cache` runs on the seed-42 200k project take
7.63-7.69 s wall under peer load (load average 5.5-7.6), SHA-256
`1a75bc87b26f3121…` unchanged; the default binary contains none of the changed
code. Penpot `app.render` builds natively in 14.46 s and matches V8 byte for
byte (`d352af3a…`).

#### Review-fix round 2 verification

The branch merged master `a335bcef` (native sorted-map-by, light-cljs-561)
in `9efd151e`; both sides add to the core-protocol frame state, the native
module exports, the resolver, the runtime and this document, and the merge
keeps both. On the same toolchain, at the merged source (`3ad3f198` differs
only by a test lint fix):

- Combined `bbp-macros,quickjs` `bbp_macros` passes 161 tests and the library
  368 (one ignored stress case each); native-only `bbp_macros` passes 159
  (one ignored) with `LIGHT_CLJS_STRESS_SEEDS=12`, the native `bbp_evaluator`
  selection 63; engine all-targets passes 2,192 tests and the seven doctests.
- Default V8 nextest (`LIGHT_CLJS_V8_CACHE=require`, `-j 8 --no-fail-fast`,
  pinned ClojureScript `a5e94c98`) passes 2,133 tests in 216.5 s and fails
  only the unchanged worktree `playground_crate_checks_for_wasm32`
  diagnostic. Strict default Clippy passes; native and engine all-target
  Clippy pass with the three baseline categories allowed (strict engine
  Clippy keeps them); compiler and engine formatting pass.
- Five alternating 200k pairs (branch first) against a fresh default V8
  release build of master `a335bcef` (built from `git archive` in its own
  target directory), load average about 5–7 from a peer: median wall time
  **7.66 s on master and 7.64 s on the branch** (ranges 7.58–7.88 and
  7.55–7.75 s), median user CPU 7.84/7.83 s, maximum RSS 1,032–1,076 and
  1,012–1,064 MiB; all ten outputs keep SHA-256 prefix `1a75bc87b26f3121`.
  Five quieter pairs against the earlier master `ba7f0c89` gave 7.20/7.25 s.
- Native `app.render` at jobs 1 (14.84 s wall, 14.21 s user, 407,863,296
  bytes maximum RSS) and jobs 4 (9.82 s, 12.81 s user, 518,946,816 bytes)
  stays byte-identical to the fresh V8 build (2.35 s), SHA-256
  `8f55b0bd…374e` over 12,687,975 bytes.

With sorted-map-by native, `app.main` passes instaparse's grammar table
and stops at the next gap, a compile-time atom holding `re-pattern`
RegExps (light-cljs-563), at jobs 1 (102.21 s wall, 97.78 s user,
702,808,064 bytes) and jobs 4 (56.88 s, 67.70 s user, 895,647,744 bytes),
exiting 1 with exactly:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/vendor/instaparse/abnf.cljc:129:21: macro expansion: while expanding precompile-cljs-grammar: /Users/admin/src/light-cljs/research/penpot-light/vendor/instaparse/abnf.cljc:129:21: macro instaparse.abnf/precompile-cljs-grammar: big-boy-pants-js: Atom(NotShareable): atom operation: cannot publish: value[0].capture(1)[0]{:regexp}{:parsers}[0]{:regexp} is a task-owned object (owned by task#1), which is not shareable (in anonymous at instruction 9 [atom_cas @ byte 23], task#1, world#0) at source 1088:33
```

There is still no native `app.main` output to compare. Research was not
changed.

### Review-fix round 3: member precedence, evaluation order, inheritance (2026-10-29)

The third review found five edges in the frozen-object model; the fixes keep
the model and an adversarial pass over the same surface added the rows that
exercise it.

- **Apply adapters read `call` first.** The collection and multimethod
  `.apply` adapters read `this.call` before realizing the array-like
  argument, as the legacy adapters' `this.call(null, ...Array.from(args))`
  does, so a getter that replaces or throws is observed in the legacy order;
  the IFn adapter keeps realizing first.
- **IFn adapters win over same-declaration `Object` members.** The legacy
  installer writes a declaration's `Object` methods before its IFn
  `call`/`apply` adapters, so within one declaration the adapters win for
  `.call`, `.apply`, direct calls and callbacks; a later `extend-type`
  `Object` method named `call` replaces the adapter for `.call` and runtime
  callbacks, while a compiled direct call still enters the declared IFn
  entries, as the legacy compiler's inferred arity call does (the engine's
  `cljCallMember` preparation now asks the resolver with `jsDirectMember`).
  A type's pending `Object` extension bundles are installed before any
  member lookup, so precedence never depends on what ran before.
  (Superseded by "Member precedence: one rule" below: the direct-call
  bypass and the eager installation were both wrong, and the `apply`
  quirk was a demand-order artefact.)
- **Record slots override colliding fields.** `__meta`, `__extmap` and
  `__hash` resolve before a field of the same member name, as the legacy
  constructor's later assignments do; keys stay deduplicated.
- **Presence sees runtime members.** `in` and `Reflect.has` (the resumable
  probe) consult the resolver for a non-field name, never calling the
  member, on the form and through an inheriting object; the probe may park
  on a cold declaration. A leaf presence check (one that cannot run the
  resolver) still sees only fields.
- **Inherited writes honour frozen fields.** A write through an object
  inheriting from a struct (`Object.create`) to a field name is rejected as
  read-only, with a separate receiver too; other names continue at
  `Object.prototype` and are added to the child.

The adversarial pass also found and fixed: a borrowed IFn or multimethod
adapter reached with a non-struct receiver now throws a catchable
`TypeError` (the legacy adapters fail with their own error text, listed in
the sweep) instead of an engine fault, and a write through an occurrence
wrapper reports the frozen-object rejection. It confirmed, against QuickJS,
that a field shadows an `Object` method and `Object.prototype.toString` of
the same name, that an occurrence-wrapped macro argument reads, probes,
calls and enumerates as its form, and that neither runtime lets an object
inheriting from a collection call its `call` member (`Expected map`). Two
legacy quirks are not modelled: a later `extend-type` `Object` method named
`apply` does not replace the legacy IFn `apply` adapter (only `call` is
tested), and a legacy record's keyword lookup of a field colliding with a
slot returns the slot. Tests: `native_borrowed_apply_reads_call_before_arguments`,
`native_ifn_object_member_precedence_matches_legacy`,
`native_record_slots_override_colliding_members`,
`native_form_member_presence_matches_quickjs`,
`native_occurrence_forms_behave_as_their_values`, the engine's
`native_struct_inherited_writes_respect_frozen_fields` and
`native_form_member_presence_resolves_without_calling`, new sweep rows
(`has-call`, `has-inherited`, `child-get/has/has-inherited/call/define-field/set-field/set-other`,
`reflect-set-receiver`), the matrix's borrowed `.apply` order rows and the
pool-size test's presence cells through a cold declaration. Each failed
before its fix.

#### Review-fix round 3 verification

On the same toolchain, at source `401d3f00` (the following commit changes
one lint-only expression in the inherited write walk, re-linted and
re-tested with the engine forms suite), master `a335bcef`:

- Combined `bbp-macros,quickjs` `bbp_macros` passes 166 tests and the library
  368 (one ignored stress case each); native-only `bbp_macros` passes 164
  (one ignored) with `LIGHT_CLJS_STRESS_SEEDS=12`, the native `bbp_evaluator`
  selection 63; engine all-targets passes 2,194 tests and the seven doctests.
- Default V8 nextest (`LIGHT_CLJS_V8_CACHE=require`, `-j 8 --no-fail-fast`,
  pinned ClojureScript `a5e94c98`) passes 2,133 tests in 214.2 s and fails
  only the unchanged worktree `playground_crate_checks_for_wasm32`
  diagnostic. Strict default Clippy passes; native and engine all-target
  Clippy pass with the three baseline categories allowed (strict engine
  Clippy keeps them); compiler and engine formatting pass.
- Five alternating 200k pairs (branch first) against the fresh default V8
  release build of master `a335bcef`, load average about 2–4: median wall
  time **7.11 s on master and 7.10 s on the branch** (ranges 7.02–7.35 and
  6.98–7.45 s), median user CPU 7.25/7.24 s, maximum RSS 1,012–1,052 and
  982–1,055 MiB; all ten outputs keep SHA-256 prefix `1a75bc87b26f3121`.
- Native `app.render` at jobs 1 (13.90 s wall, 13.30 s user, 412,581,888
  bytes maximum RSS) and jobs 4 (9.38 s, 12.18 s user, 509,083,648 bytes)
  stays byte-identical to the fresh V8 build (2.12 s), SHA-256
  `8f55b0bd…374e`. `app.main` still exits 1 with the exact `re-pattern`
  RegExp `Atom(NotShareable)` error above (light-cljs-563) at jobs 1
  (95.51 s wall, 91.55 s user, 704,135,168 bytes) and jobs 4 (53.86 s,
  64.11 s user, 875,790,336 bytes).

### Member precedence: one rule (light-cljs-565 to 570, 2026-10-29)

Rounds 2 and 3 fixed precedence one call path at a time, and the fourth
review found four more paths that disagreed (565–568). Each fix had
assumed something about the legacy runtime that QuickJS does not do: the
legacy compiler emits no inferred arity call in compile-time units (`(f x)`
is always `f.call(null, x)`), and it installs an `Object` extension only
when compiled interop demands it, never eagerly. Probing QuickJS cell by
cell gives one rule, which every path now follows.

**The rule.** The member `name` of a struct `x` of type `T`, read at the
compiler frontier `F`, is the first of:

1. **Own properties**: a record's `__meta`, `__extmap` or `__hash` slot,
   then the last field whose munged member name is `name`.
2. **The last write to `name` by an installation into `T` visible at
   `F`.** `T`'s installations are its declaration and each of its extension
   bundles, in the order they are installed. Each installation writes every
   `Object` method `T` has so far (under its munged member name) and then,
   if it declares IFn methods, the `call` and `apply` adapters. So within a
   declaration the IFn adapters beat its own `Object` `call`/`apply`, and
   once any later installation into `T` is visible, an `Object` `call`/`apply`
   beats the adapters, whether that installation re-extends `call` or, as
   the legacy installer does, extends some other protocol and rewrites the
   accumulated `Object` methods.
3. **`Object.prototype`**: what a legacy instance inherits (`toString`,
   `valueOf`, `hasOwnProperty`, …; not `constructor`).

Otherwise the member is `undefined`.

**When an extension is installed (lazy demand).** A declaration is
installed when its type is declared. An `Object` extension bundle is
installed only when compiled interop on a value of `T` demands it: the
receiver of `(.m x)`, `(.-m x)`, `(.call x …)` or `(.apply x …)`, or of an
object inheriting from such a value, which the emitter wraps in
`declarationReceiver`. That installs every bundle registered up to `F`
whose selector is `T`, `object`, `default` or none, in source order. A raw
`js*` read, a direct call, a callback, a presence check or a ToPrimitive
conversion demands nothing and sees what is already installed. A
bundle extending another protocol `P` is installed when `P` is dispatched
on any value one of the bundle's `P` selectors matches: a type it extends
(a struct type or a core collection type), the value's JavaScript type
(`string`, `object`, …), `default`, or a target with no type name, which
matches every value. It is also installed by an `Object` demand it
matches. (A bundle is also installed when one of its products is read, but
the bundles that carry extension indices, `extend-type` and
`extend-protocol`, have no products, so that path never reaches a pending
bundle and is not exercised from source.) Installing a
bundle installs everything in it, for every type it extends, whichever
selector demanded it. Leaving the frontier hides an
installation whose bundle is after it, as the legacy journal undoes its
patches.

The native runtime builds a protocol's dispatch table for all types at
once, which forces every registered bundle of that protocol. Forcing is
not installing: a bundle forced only for a table build stays inactive
until the demand above happens, and its `Object` methods and its rewrite
of rule 2 stay invisible meanwhile (`decl.activate`, `decl.demandFor`).
While a bundle is pending, the tables of every protocol it indexes keep no
lookup cache, so every dispatch of those protocols, on any value, takes
the slow path that performs the demand; activation restores the cache.
Bundles are tracked as pending only when they can change a member: a
bundle with `Object` methods, or one extending a type whose IFn adapters
compete with an `Object` `call`/`apply`.

Classifying the receiver for a demand reads no host object property, as
the legacy demand reads only `constructor`: a struct's type and a form's
kind come from the engine, the JavaScript type from `typeof` and
`Array.isArray`. A mutable type instance's `__lightNativeType` tag is read
only where it can select something: by dispatch when the protocol's table
extends a mutable type, and by an `Object` demand only to match a
pending `Object` bundle whose selector names one. An `Object` demand
installs a bundle with no selector (an alias target), `default` or the
receiver's JavaScript type whatever the receiver is, a string included, as
the QuickJS reference does; only `@type` selectors look at the receiver's
type, and nothing is inspected once every `Object` bundle is installed
(`native_mutable_object_extension_alias_matches_quickjs`). Dispatch on a
host array a Proxy wraps (which the engine gives no key) takes the `array`
entry, as legacy `Array.isArray` does.

`native_member_precedence_table_matches_quickjs` reaches a bundle every
way against QuickJS: dispatch on another type of a bundle extending
several types, and on its own type; selectors naming a struct type, a core
collection type (`PersistentVector`), `string`, `object`, `default` and a
target with no type name; host receivers with a throwing
`__lightNativeType` getter (an array, a typed array, an `arguments` object,
and a Proxy of an array that also throws from `getPrototypeOf`) under
`array` and `object` selectors; `Object` methods in a bundle with another protocol,
reached by a dispatch on another type (still invisible), then by the
matching dispatch, or by an `Object` demand; and several bundles
extending one type, each installed by its own protocol.

**Who reads the member.** Every consumer reads the one member the rule
gives, with nothing chosen at the call site:

| Consumer | Reads |
| --- | --- |
| Compiled direct call `(f a)`, `((fn [g] (g a)) f)` | `f.call`, called with `null` |
| Runtime invocation (`mapv`, `update`, `reduce` callbacks, `invoke`) | `f.call` |
| Compiled `(.call f …)`, `(.apply f …)` | demand, then `f.call`, `f.apply` |
| Raw `js*` read, `in`, `Reflect.has`, ToPrimitive | `f[name]`, no demand |
| Clojure `apply` | the declared IFn arity entry for the count if there is one, else `f.call`, called with `f` as receiver and first operand |
| Borrowed adapter (`Function.prototype.call.call(m, r, …)`) | the member `m`, run with receiver `r` |

Clojure `apply` is the one consumer that reads past the rule: like the
legacy `cljs$core$IFn$_invoke$arity$n` entries, which only an IFn
declaration writes, a declared arity wins over any `call` member. A
borrowed member runs as the legacy one would with a foreign receiver: an
`Object` method dispatches on its declaring type with the receiver (or the
struct the receiver inherits from); the adapter of a single variadic IFn
method is that method itself, so it runs with any receiver; the fixed and
overloaded IFn adapters dispatch on the receiver's own IFn methods and
throw a catchable `TypeError` for a receiver that has none (the legacy
error text differs).

**One place.** `decl.objectWins` alone decides between an `Object` method
and the IFn adapters. The runtime's `structMember` applies rules 2 and 3
with it for every engine property read of a struct (the engine answers
rule 1 from the struct's own properties first): raw reads, presence,
conversions, the compiled direct call's `cljCallMember` preparation (an
ordinary property read again; the `jsDirectMember` resolver op is gone)
and compiled `.call`/`.apply`. `invokeStruct` (callbacks, `invoke`, and
Clojure `apply` without a declared arity) applies the same predicate
before entering the IFn methods directly. `declarationReceiver` is the
only place that installs an `Object` extension; it demands only for a
struct, an object inheriting from one, or an instance whose type tag names
a declared mutable type, so a host object carrying an unrelated
`__lightNativeType` property demands nothing.

**Boundary.** A mutable type's instances are host objects whose
prototype receives `Object` methods when their bundle is forced; their
installations are not deferred. An IFn extension (`extend-type T IFn`) installs
no `call`/`apply` member natively; QuickJS answers the same in every probed
cell (it does not install it before demand either).

The engine tests `native_form_presence_consumers_resume_without_replay`
(light-cljs-569: presence forwarded by Proxies without a `has` trap, and
`Object.defineProperty` with a descriptor inheriting from a struct, with
resolver waits, collection, exact resolution counts and exceptions) and
`native_occurrence_writes_reject_without_engine_fault` plus the `__proto__`
rows of `native_struct_inherited_writes_respect_frozen_fields`
(light-cljs-570) cover the review's untested engine paths; each fails when
its path is broken (the Proxy route answering false, the descriptor
treating form presence as absent, the inherited walk stopping at the form,
an engine fault in the occurrence store).

#### Verification

On macOS arm64 with Homebrew Rust/Cargo 1.99.1, Node 26.12.1, the read-only
ClojureScript pin `a5e94c98` and `CARGO_BUILD_JOBS=4`, at `d2f2306a`
(master `0ad2514f`):

- Combined `bbp-macros,quickjs` with `LIGHT_CLJS_STRESS_SEEDS=12`: the
  library passes 368 tests and `bbp_macros` 168 (one ignored stress case
  each); native-only `bbp_macros` passes 166 (one ignored). The new
  precedence tests fail on master's runtime and engine.
- `cargo test --locked -p big-boy-pants-js --all-targets` passes all 2,196
  engine tests, and the seven doctests pass.
- Default V8 nextest (`LIGHT_CLJS_V8_CACHE=require`, `-j 8
  --no-fail-fast`) passes 2,133 tests in 213.4 s and fails only the
  unchanged worktree `playground_crate_checks_for_wasm32` diagnostic.
  Strict default Clippy passes; native `bbp-macros,quickjs` all-target and
  engine all-target Clippy pass with the three baseline categories allowed
  (strict engine Clippy keeps exactly those nine diagnostics); compiler and
  engine formatting pass.
- Five alternating 200k pairs (branch first) against a fresh default V8
  release build of master `0ad2514f` (from `git archive`, its own target
  directory), load average about 3–5 from peers: median wall time
  **7.67 s on master and 7.70 s on the branch** (ranges 7.63–7.79 and
  7.64–7.85 s), median user CPU 7.84/7.90 s, maximum RSS 1,015–1,056 and
  1,008–1,040 MiB; all ten outputs are byte-identical.
- Penpot `app.render` with the native-only release build at jobs 1
  (13.50 s wall, 13.03 s user, 419,807,232 bytes maximum RSS) and jobs 4
  (9.14 s, 11.98 s user, 498,565,120 bytes) is byte-identical to the
  default V8 release build (2.00 s), SHA-256 `d352af3a…a8d9a3` over
  12,680,976 bytes of JavaScript.

#### Review-fix round 1 (2026-10-29)

The review of the bundle found three defects, each now a row of
`native_member_precedence_table_matches_quickjs` that failed before its
fix: forcing a protocol's bundles for a table build installed them for
every type (`other-type-dispatch`; forcing and installing are now
separate, as described above); Clojure `apply` without a declared arity
passed `null` instead of the callable as the call's first operand
(`apply-operand-method`, `apply-operand-field`); and a host object
carrying a `__lightNativeType` property aborted compiled interop
(`host-type-tag`).

Verification at `9ca6998f` (master `0ad2514f`), same toolchain: combined
`bbp-macros,quickjs` with `LIGHT_CLJS_STRESS_SEEDS=12` passes the library's
368 tests and `bbp_macros`' 168, native-only `bbp_macros` 166 (one ignored
each); engine all-targets 2,196 and seven doctests; default V8 nextest
2,133 (240.8 s; only the worktree `playground_crate_checks_for_wasm32`
diagnostic fails); Clippy and formatting as before. Five alternating 200k
pairs against a fresh master release build, load average 4–9 from peers:
median **7.28 s on master and 7.36 s on the branch** (ranges 7.25–8.73 and
7.28–7.63 s), all ten outputs byte-identical. Penpot `app.render` native at
jobs 1 (14.06 s wall, 411,074,560 bytes maximum RSS) and jobs 4 (9.56 s,
499,302,400 bytes) is byte-identical to V8 (2.11 s), SHA-256
`d352af3a…a8d9a3`.

#### Review-fix round 2 (2026-10-29)

Round 1 activated a pending bundle only through the type whose members it
could change, so a dispatch on another type or value the same bundle
names left it inactive. A pending bundle is now activated by any matching
selector, and its protocols' lookups bypass the cache while it is pending
(see "When an extension is installed" above). The six selector rows of the
table differed from QuickJS before this fix; the mixed-bundle and
several-bundle rows are new coverage. (Its product-demand row is removed
in round 3: it never reached its extension bundle.)

Verification at `05224182` (master `0ad2514f`), same toolchain: combined
`bbp-macros,quickjs` with `LIGHT_CLJS_STRESS_SEEDS=12` passes the library's
368 tests and `bbp_macros`' 168, native-only `bbp_macros` 166 (one ignored
each); engine all-targets 2,196 and seven doctests; default V8 nextest
2,133 (212.0 s; only the worktree `playground_crate_checks_for_wasm32`
diagnostic fails); Clippy and formatting as before. Five alternating 200k
pairs against a fresh master release build, load average about 2–4:
median **7.06 s on master and 7.08 s on the branch** (ranges 7.01–7.11 and
7.06–7.12 s), all ten outputs byte-identical. Penpot `app.render` native at
jobs 1 (13.45 s wall, 414,318,592 bytes maximum RSS) and jobs 4 (9.24 s,
515,801,088 bytes) is byte-identical to V8 (2.08 s), SHA-256
`d352af3a…a8d9a3`.

#### Review-fix round 3 (2026-10-29)

Round 2's demand read every receiver's `__lightNativeType`, so a host array
with a throwing getter aborted a dispatch the QuickJS reference answers;
typed arrays, `arguments` objects and Proxies already threw on master,
from dispatch's own tag fallback. Receivers are now classified without
reading host properties (see "Classifying the receiver" above), and the
table has a row for each of the four. The product-demand row is removed:
it never reached its extension bundle.

Verification at `84294955` (master `4ad6fd92`; the rebase onto `313a51b3`
adds a documentation file only), same toolchain: combined
`bbp-macros,quickjs` with `LIGHT_CLJS_STRESS_SEEDS=12` passes the
library's 368 tests and `bbp_macros`' 168, the Pants build's
`bbp_macros` 166 (one ignored each); engine all-targets 2,196 and seven
doctests; V8 build nextest 2,133 (225.1 s; only the worktree
`playground_crate_checks_for_wasm32` diagnostic fails); Clippy and
formatting as before. Five alternating 200k pairs against a fresh V8
release build of master `313a51b3`, load average about 4–8 from peers:
median **7.52 s on master and 7.52 s on the branch** (ranges 7.46–7.62
and 7.45–7.67 s), all ten outputs byte-identical. Penpot `app.render`
from the Pants build at jobs 1 (14.59 s wall, 420,724,736 bytes maximum
RSS) and jobs 4 (9.85 s, 504,004,608 bytes) is byte-identical to the V8
build (2.12 s), SHA-256 `d352af3a…a8d9a3`.

#### Review-fix round 4 (2026-10-29)

Round 3 let an `Object` demand on a host object install only bundles whose
selector names a mutable type, so an `Object` extension of a mutable type
through an alias (no selector) was never installed, from the instance or
from any other receiver. `demandObject` now installs a bundle with no
selector, `default` or the receiver's JavaScript type for every receiver,
as the QuickJS reference does, and looks at the receiver's type only for a
`@type` selector. In `native_mutable_object_extension_alias_matches_quickjs`
the alias rows demanded from the instance and from a string failed before
the fix; the row demanded from a struct of another type already passed
(struct receivers matched unnamed selectors), and a `default` `Object`
extension installs no instance member in either runtime.

Verification at `7bce515e` (master `313a51b3`; the rebase onto `fd5c185f`
adds one documentation file), same toolchain: combined
`bbp-macros,quickjs` with `LIGHT_CLJS_STRESS_SEEDS=12` passes the
library's 368 tests and `bbp_macros`' 169, the Pants build's `bbp_macros`
167 (one ignored each); engine all-targets 2,196 and seven doctests; V8
build nextest 2,133 (217.6 s; only the worktree
`playground_crate_checks_for_wasm32` diagnostic fails); Clippy and
formatting as before. Five alternating 200k pairs against a fresh V8
release build of master `fd5c185f`, load average about 4–7 from peers:
median **7.52 s on master and 7.42 s on the branch** (ranges 7.39–7.87
and 7.33–7.73 s), all ten outputs byte-identical. Penpot `app.render` from
the Pants build at jobs 1 (14.17 s wall, 408,911,872 bytes maximum RSS)
and jobs 4 (9.63 s, 515,948,544 bytes) is byte-identical to the V8 build
(2.13 s), SHA-256 `d352af3a…a8d9a3`.

## Shareable regex values (light-cljs-563, 2026-10-29)

instaparse's GLL trampoline stores parse thunks over parser maps whose
`:regexp` values come from `re-pattern` in a compile-time atom, and anchors
each with `(js/RegExp. (str "^" (.-source r)) flags)`. Native macro units made
both task-owned JavaScript `RegExp` objects, so the atom rejected the store with
an uncatchable `Atom(NotShareable)`.

### One object in both worlds

A native regex value follows light-cljs-558's invariant: it is **one object
in ClojureScript and JavaScript, and no boundary converts it**. Native
`re-pattern`, unquoted regex literals, `(js/RegExp. …)`, `(js/RegExp …)`,
construction through a local alias (`(let [R js/RegExp] (R. …))`, checked when
it runs by the runtime's `nativeConstruct`) and the constructor called as a
first-class value (`(R "a")`, `(apply js/RegExp …)`, `(mapv js/RegExp …)`:
the runtime's `invoke`/`prepareInvoke` and the emitted invoke adapters
recognize it) return a `regex` compiler-literal form made by the engine's
`formRegex(source, flags)` intrinsic. Each form evaluates the constructor
before its operands and promotes only the intrinsic `RegExp`; a global
`RegExp` an operand replaces does not change the call, and a replaced global
is called or constructed as it is, as in the QuickJS reference. Raw JavaScript (`js*`, or an explicit
`(.call js/RegExp …)`) still makes an ordinary task-owned RegExp. The form is
persistent and shareable, so atoms, published helpers and closures hold it,
and the engine treats it as a RegExp (`crates/big-boy-pants-js/src/regexp/shared.rs`):

- `crate::regexp::object::data` answers its compiled program (compiled once,
  lazily for a quoted literal from the bridge), so `exec`, `test`, the flag and
  `source` getters, `Symbol.split`/`replace`/`match`/`matchAll`/`search`, the
  RegExp constructor's brand check and `RegExp(r)`'s identity take it as it is.
- A property read continues at `RegExp.prototype` for every key, symbols
  included, and `Object.getPrototypeOf` and `instanceof` see that prototype.
- `Object.prototype.toString` reports `[object RegExp]`.
- It is not extensible (a host RegExp is), since a shared value cannot take
  properties; its one own property is `lastIndex`.

`nativeInterop` passes it through (before 558's rejection of other non-struct
forms), `js*` operands are emitted as written, and `instance?`, `regexp?`,
printing (`#"source"`), `str` (`/source/flags`), identity equality and hashing
read the form itself. So `(identical? r (.valueOf r))`, `(identical? r (js*
"~{}" r))`, a map lookup with what a JavaScript function returns for `r`, and
an atom holding it all behave as in QuickJS. The payload is the legacy export
text, `(?flags)source`, so a macro that returns a regex emits the same literal
as QuickJS and V8; the legacy export cannot spell `v`, so a Unicode-sets regex
exports without it, as in QuickJS, while the form keeps matching with it. An
invalid pattern still throws the RegExp `SyntaxError` at construction. A RegExp
that `js*` code creates (or a Date) is still a mutable host object that an
atom rejects. Nothing changes in `src/js_runtime.js` or in default (V8)
emission.

### The `lastIndex` rule

**Only a regex without the `g` and `y` flags is a shared value, and its
`lastIndex` is always 0.** That is the whole rule; no publication, task or
initializer changes a regex value.

It needs no per-task state because a RegExp's only state is `lastIndex`, and
ECMAScript moves it only for global or sticky regexes: `RegExpBuiltinExec`
reads it and writes it only with `g` or `y`; `search` writes it only when it
is not 0; `split` and `matchAll` match on a copy; `replace` and `match` reset
it only for a global regex. So a non-global, non-sticky regex that never
leaves 0 is exactly what legacy code observes, in every task and at every
`--jobs`. Writing 0 (the reset idiom) is accepted and changes nothing;
writing any other value, by assignment or `Object.defineProperty`, is a
`TypeError` (`unsupported operation: a shared regex value's lastIndex stays
0`), where a legacy RegExp would keep the value.

A global or sticky regex (`(re-pattern "(?g)o")`, `(js/RegExp. "a" "y")`,
`#"(?g)a"` unquoted) stays an ordinary task-owned RegExp, as in a legacy
isolate: matching moves its `lastIndex` within the task, an atom store or a
published closure over it fails with the usual `Atom(NotShareable)`
publication error, and a helper whose value holds one is initialized per task,
so every task sees the `lastIndex` its initializer left, as QuickJS does. A
macro may return one; it exports as the legacy regex literal. The one
exception is a quoted `#"(?g)…"` or `(?y)` literal in macro data, a `regex`
form the bridge makes rather than a task: its `lastIndex` is task-local,
starting at 0, and holds numbers only (another value is the same unsupported
`TypeError`), so it matches statefully within a task, as the QuickJS
reference's freshly imported RegExp does, and never carries state to another
task. Each such task entry is charged to the byte budget and released, with
its charge, when the task completes, fails, is cancelled or is dropped;
the entries of such a regex that dies while the task runs are released by
the next collection. A host regex a macro returns exports with its payload counted against the
conversion limits like a `regex` form's.

`lastIndex` stays a writable, non-enumerable, non-configurable own data
property for reads, `Object.getOwnPropertyDescriptor`, `Object.defineProperty`
(enumerable, configurable or accessor descriptors are rejected as on any
RegExp; making it read-only is a `TypeError`, which a host RegExp allows),
deletion (rejected) and key listing. An object that inherits from a regex
value meets `RegExp.prototype`'s getter-only properties on writes and may
shadow `lastIndex`. The value is not extensible, so it is sealed;
`Object.isFrozen` is false and `Object.freeze` a `TypeError`. The first use of
a quoted literal compiles its program once, concurrent first users waiting
for it; a syntax error is kept, a failure for lack of byte budget is retried
by the next use. The engine cell is charged to the byte budget.

### Why not per-task `lastIndex`

Review rounds 1 and 2 kept a `lastIndex` per task, starting from the value a
helper initializer left, "settled" by a walk of each published helper value.
Every escape path then needed its own settlement (an atom filled as an
initializer side effect, a lazy sequence realized later), the walk raced
collection through atom snapshots, and the task that ran an initializer
inline saw its own writes while other tasks did not, so output could depend
on scheduling. The rule above removes the state, the walk and the
`publish_settled` entry point.

### Why not a per-task view

The first round made the descriptor inert and gave each task a RegExp *view*
of it at interop and `js*` boundaries (a `nativeJsOperand` adapter chosen by
the text around each `js*` hole). Review found what 558 had found for struct
views: a view is a different object, so `.valueOf`, `js*` results and
JavaScript functions returned something that missed map lookups, failed
`identical?` and could not be stored in an atom; the `js*` adapter broke
assignment targets (`(~{}) = 1`) and member callees (`~{}()` lost its
receiver); consuming tasks rebuilt views from source and flags, losing the
`lastIndex` an initializer left; `(js/RegExp "a")`, a local alias and the `v`
flag still made task-owned RegExps. The engine-level value removes the views,
the adapter and the emitter's operand-position heuristic altogether.

`native_regex_values_match_quickjs` compares matching, printing, identity,
interop and exported regex results with QuickJS;
`native_regex_interop_preserves_identity`,
`native_js_templates_preserve_reference_semantics`,
`native_regex_constructor_forms_publish` and
`native_unicode_set_regexes_publish` cover the review's cases and each failed
before the fix (round 2 adds alias, apply and `mapv` calls;
`native_regex_constructor_calls_keep_macro_argument_identity` failed before
round 3 unwrapped constructor operands);
`native_regex_inherited_writes_match_quickjs` (failed before round 2; round 4
runs it on a shared regex too, which fails without the shared prototype
write walk), `native_regexp_adapters_preserve_callee_before_arguments`
(failed before round 4) and
`native_regex_property_mutations` cover the property contract, the latter
asserting the intentional differences on native output alone;
`native_regex_values_drive_string_methods` adds `search`,
`match`, `matchAll`, `replaceAll`, `split`, the prototype's generic `test` on
a sticky regex, key listing and the property descriptor. `regex_helpers_and_parse_thunks_publish_at_seeded_pool_sizes`
publishes eleven regex helpers (a twelfth, a global regex, is initialized per
task) and pushes parse thunks onto an atom from two namespaces, and `published_regex_state_matches_quickjs` publishes eight
(a local alias, alias and `apply` calls, a `v` regex, an atom an initializer
filled as a side effect, a lazy sequence) and initializes three global or
sticky helpers per task, read from two namespaces; both run at seeded jobs 1/2/4/8 and match QuickJS at each size.
`atoms_still_reject_host_regexps` keeps the `Atom(NotShareable)` rejection of
`js*` RegExps, Dates and global or sticky regexes. The engine's
`regexp::shared` unit tests cover the constant `lastIndex`, the payload
prefix, compilation after a heap-limit failure and one compilation for
concurrent first uses; `gc_roots`'
`quoted_regex_task_state_is_released_on_task_termination` and the evaluator's
`host_regex_exports_obey_conversion_limits` failed before round 4.

### Penpot progression and next boundary

Fresh Pants-build (native-only) release builds after review round 4,
rebased on master `99d68bf3` (which carries light-cljs-565–570), the five
absolute read-only roots, every documented budget and `--compile --esm
--optimize none --no-cache --stats`, on macOS arm64 with Homebrew Rust/Cargo
1.99.1 and Node 24.22.1, with peer load (load average 6-12).

`app.main` passes the `:regexp` store and **still does not build natively**.
At jobs 1 and 4 the exact first different failure is:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/vendor/instaparse/abnf.cljc:129:21: macro expansion: while expanding precompile-cljs-grammar: /Users/admin/src/light-cljs/research/penpot-light/vendor/instaparse/abnf.cljc:129:21: macro instaparse.abnf/precompile-cljs-grammar: big-boy-pants-js: Atom(NotShareable): atom operation: cannot publish: value[0].capture(1) is a task-owned object (owned by task#7), which is not shareable (in anonymous at instruction 9 [atom_cas @ byte 23], task#7, world#0) at source 1252:33
```

(`task#1` at jobs 4.) A closure over an instance of a `deftype` with an
`^:unsynchronized-mutable` field stored in an atom reproduces it: the parser's
listener closures capture such instances. That is a mutable-type shareability
question, not a regex one, filed as light-cljs-564. The failing runs take
116.39 s wall, 112.10 s user and 722,157,568 bytes peak memory at jobs 1, and
73.64 s wall, 88.54 s user and 941,342,720 bytes at jobs 4. There is no
Pants-build main output to compare.

`app.render` builds at jobs 1 (18.34 s wall, 15.98 s user, 427,540,480
bytes) and 4 (10.69 s, 14.21 s user, 505,135,104 bytes). Both
12,680,976-byte outputs match a fresh V8 release build of this branch
(2.59 s) byte for byte, SHA-256
`d352af3a2bb2d35f1447f3d4fc6592de609dc30e92c3424f7321afc607a8d9a3`.

### V8 build check

The changes are emitted only for native macro units (`native_forms`) and live
in `src/bbp_runtime.js`, the engine and the native bridge, which the V8 build
does not run. The fixture is regenerated with `bb
benchmarks/generated-project/generate.bb target/g200k --seed 42 --lines 200k`
(it prints `Generated 199129 nonblank lines (213984 physical, 9966430 bytes,
15450 top-level forms), 450 worker namespaces`; the generator is unchanged
since `5ed076d3`; round 1 measured a different checkout,
`research/cljs-200k`, which prints `855170`). Five alternating pairs (master
first) of V8 release builds of master `99d68bf3` (built from `git archive` in
its own target directory) and this branch run `release bench --no-cache` at
load average 6-12 from peers: median wall time **8.87 s on master and 8.77 s
on the branch** (ranges 8.69-9.33 and 8.73-8.98 s; the load, not the change,
sets the level), peak memory 1,074-1,109 and 1,069-1,123 MB. All ten outputs
keep SHA-256 prefix `1a75bc87b26f3121`, and the program prints `484264` for
`42 3`.

### Verification

On macOS arm64 with Homebrew Rust/Cargo 1.99.1, Node 24.22.1 and the read-only
ClojureScript pin `a5e94c98`, with `LIGHT_CLJS_STRESS_SEEDS=12`, on the
rebased tree: the combined Pants/QuickJS-reference library passes 369 tests
and `bbp_macros` 182, Pants-only `bbp_macros` 180 (one existing ignored
stress case each); engine all-targets 2,202 tests and the seven doctests;
full V8-build nextest (`LIGHT_CLJS_V8_CACHE=require`, `-j 8`) 2,133 of 2,134,
failing only the existing worktree `playground_crate_checks_for_wasm32`
diagnostic. Formatting and strict V8-build Clippy pass; combined all-target
Clippy passes allowing only `deprecated`, `clippy::chunks_exact_to_as_chunks`
and `clippy::byte_char_slices` after `-D warnings`; strict engine Clippy keeps
only those nine baseline diagnostics.
