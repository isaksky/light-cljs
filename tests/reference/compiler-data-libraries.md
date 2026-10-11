# Compiler-owned set, walk and source data readers

`clojure.set` and `clojure.walk` enter the existing namespace resolver as embedded
first-party sources. Explicit source roots override them. The same compiled
functions execute application and macro helper code. No interpreter API or Rust
collection traversal callback is used.

## Evidence and provenance

`tests/compiler_data_libraries.rs` migrates all 117 native `sci-data-*` set/walk
fixture cases from `tests/fixtures/sci-outcomes.tsv`, retaining IDs, original
values, ordered `observe` effects, collection predicates, all 16 error bodies and
empty output checks.
Only the two top-level require fixtures become ordinary namespace requires, and
`catch Exception` becomes `catch js/Error`. The result vector is checked in
QuickJS and Node. Additional independent tests cover all public APIs, malformed
map entries, short-circuit callback failures, record identity/fields, metadata,
opaque values, macro helper imports, callback reentry, fuel and cancellation.

`generate_library_witnesses.py` extracts full test body/setup/callback contexts
from the pinned checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` under
`research/clojurescript/src/test/cljs/clojure/{set,walk}_test.cljs`.
`tests/fixtures/compiler-libraries/provenance.tsv` records original body SHA-256,
assertion counts and exclusions. The executable witness contains 22 bodies and
124 assertions. Twenty bodies are complete. Two set bodies omit four sorted-set
rows (Pellet 119); `cljs.walk/test-walk` and its multimethod helpers require sorted
collections and multimethods (Pellets 119 and 47), and remain unexecuted. The
upstream copyright and EPL notice are retained. Assertions keep their setup and
callback forms; test macros become ordinary assertion functions.

## Pinned reference outcomes

Both references compiled the same witness with `:optimizations :none`,
`:static-fns false` and `:def-emits-var false`, using Clojure 1.12.4 and Java
25.0.2. The pinned checkout passes all 22 bodies. Maven ClojureScript 1.12.134
passes 21; its `cljs.walk/retain-meta` body fails at the first list metadata
assertion, so subsequent assertions in that body do not execute on the release.
Our runtime passes all bodies and preserves the native and newer checkout's
metadata contract.

Exact reference compilation commands (Node is run from each output directory):

```sh
clojure -J-Xmx1g -Srepro -Sdeps '{:deps {org.clojure/clojurescript {:mvn/version "1.12.134"}}}' -M tests/reference/execution.clj release 1.12.134 - /tmp/light-cljs-library-oracle-release program tests/fixtures/compiler-libraries library.upstream run
clojure -J-Xmx1g -Srepro -Sdeps '{:deps {org.clojure/clojurescript {:local/root "research/clojurescript"}}}' -M tests/reference/execution.clj checkout a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e research/clojurescript /tmp/light-cljs-library-oracle-checkout program tests/fixtures/compiler-libraries library.upstream run
node -e 'require("./reference.js"); console.log(JSON.stringify(globalThis.lightProgramRun()))'
```

## Preserved native contracts and bounded execution

Set and walk functions preserve supported native arity errors, including saved
function and apply calls. Set functions validate the existing set/map domains including nil and preserve
zero-arity failures for intersection/difference. Projection retains row metadata
and stored equivalent key representatives, as the native fixtures require.
Natural join retains the indexed path for uniform schemas. Heterogeneous rows
join on each pair's shared keys: otherwise arbitrary first-row order changes the
answer. This preserves native fixture `sci-data-set-join-5`; a separate pinned
checkout probe confirmed three matching rows for the original fixture.

Walks use an explicit frame stack and shared 256-depth budget, plus a shared
8-call callback reentry bound before default engine stack limits. Both counters
restore after failure. Shallow map walking commits entries incrementally, so a bad
entry fails before later callback effects. Records retain declared fields and
extended entries; map entries remain entries, lists remain lists, lazy sequences
remain sequence values, and opaque atoms/delays are not dereferenced or forced.

`MacroHost::register_compiler_data_reader` compiles a function without native
state and imports/exports immutable typed `Datum` values. Tests preserve source
order through discards (including discard-only input) and untaken branches, exact source/tag provenance,
built-in override support and rejection of atoms/functions/host objects. Reader
output counts against the compiler generated-node budget. Failed readers drop
their JavaScript heap so a reused host starts clean.

Focused checks actually run: `cargo test --test compiler_data_libraries`,
`cargo test --test compiler_bootstrap --test compiler_reader --test interpreter_strings --test interpreter_data --test interpreter_io`,
`node tests/support/js_runtime.test.js`,
`node tests/support/js_core_helpers.test.js`, and
`cargo test --test namespaces --test compiler_namespace_completion` (38 tests).
Full unrelated suites are not claimed.

Performance checks: `cargo bench --bench compiler -- --samples 9 --iterations 2
--primitive-only` and the corresponding `--application-only` run passed all
checksums. The normal unfiltered command stops at the existing
`benchmarks/fixtures/compiler/collections/bench/macro.cljs` retired
`:require-macros` form. Its benchmark/native resource migration belongs to the
live Pellet 115; this run is not reported as passing.
