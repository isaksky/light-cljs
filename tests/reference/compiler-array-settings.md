# Retained array compiler setting evidence

This narrow `light-cljs-132` slice retains both complete source bodies
`cljs-2861-test` and `unchecked-arrays-file-scope-test` from
`src/test/cljs/cljs/array_access_test.cljs` at
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, plus the exact alpha/beta/helper
setup and public `(def *unchecked-arrays* false)` declaration provenance.
The retained upstream material is Copyright (c) Rich Hickey and distributed
under Eclipse Public License 1.0; notices remain in every generated source.

Each local fixture root is
`tests/fixtures/compiler-array-settings/{cljs-2861-test,unchecked-arrays-file-scope-test}`.
Entry namespace is `cljs.array-access-test`; exported function is `run` and the
last source expression is `run`. Complete original expressions remain in the
body; only `deftest` and `is` become a nonthrowing accumulator returning true or
all failed original assertion IDs. Alpha retains its helper call and beta
import; ordinary `:require` imports replace upstream macro-only imports. Local
`helper.cljc` contains the ordinary macro:

```clojure
(defmacro unchecked-arrays? []
  (:light-cljs/unchecked-arrays &env))
```

Beta retains `(set! *unchecked-arrays* true)`. This setting affects compilation
of beta only; alpha's macro observes false and the public runtime variable
remains false. No fixture defines the runtime variable or hard-codes alpha's
macro result.

The upstream oracle roots are separate:
`tests/reference/compiler-array-settings/original/{cljs-2861-test,unchecked-arrays-file-scope-test}`.
They preserve the original `cljs/array_access/alpha.cljs`, `beta.cljs`, and
`helper.clj` module paths and exact file contents, including the helper's call
to `cljs.analyzer/unchecked-arrays?`. That JVM helper is never compiled locally.
The full ledger explicitly supplies `reference_source_root`, a complete file
hash map, and a reviewed adaptation rationale; the report verifies this map
and uses the original tree for both build input and the macro classpath.

Reproduce source/provenance/ledger mappings and both required references from
the repository root:

```sh
python3 -B tests/reference/compiler-array-settings.py
python3 -B tests/reference/compiler-array-settings-report-tests.py
python3 -B tests/reference/compiler-array-settings.py --references
cargo test --locked --test compiler_array_settings
```

`--write` updates generated artifacts and the two public/metadata phase mappings
using the hash of the complete Rust suite. `--fixtures-only` omits phase mappings
while that Rust suite is being edited. `--references` requires Clojure CLI,
Java, Node, the clean pinned checkout, and Maven ClojureScript 1.12.134. It runs
the common identity/options-checked reference harness, asserts the full outcome
protocol, and retains invocations, compiler stdout/stderr, generated programs,
Node driver/results, and aggregate JSON in the printed temporary directory.

Observed locally on 2026-10-11: both bodies returned true with no effects and no
stdout in Maven ClojureScript **1.12.134** and the clean checkout at the full
commit above. Effective options were `{:optimizations :none, :static-fns false,
:def-emits-var false}`. Compiler JVM reported Clojure 1.12.4 and Java 25.0.2.
The focused report tests reject missing/changed helper hashes, a local tree
substitution, absent rationale, absolute paths, and parent traversal; they also
assert that reference build input and classpath use the original helper tree.

The public declaration and local metadata obligation map to
`array_setting_var_and_macro_state_execute_in_both_phases` in
`tests/compiler_array_settings.rs`. This named test directly reads the runtime
variable and Var metadata in application execution and compiler-executed macros,
and observes source-ordered setting changes through ordinary `&env` macros.
The exact complete suite hash is retained by both obligation and phase mappings.
No inventory row or static registration is claimed as execution evidence.

Verification on 2026-10-11: the six array-setting tests and nine related compiler
suites passed 219 tests (one unrelated reference test remains ignored). The
focused suites include source/macro compilation, namespace loading, warning
locations, both array settings source bodies, CLI checked modes, utilities and
QuickJS/Node observations. Both direct JavaScript runtime suites passed.
The source inventory checker and Python report/inventory/full-ledger/oracle
routing suites passed; the two complete array bodies passed both pinned
references with the original helper/module paths. The ordinary compiler
benchmark completed 423 verified samples across 47 scenarios/phases, with nine
samples and 32 iterations per phase; no timing comparison is claimed.

Commands and local receipts for this run:

```sh
cargo test --locked --test analyzer --test compiler_macros \
  --test compiler_namespace_completion --test compiler_boundary_completion \
  --test namespaces --test compiler_utilities_bodies \
  --test compiler_utilities_completion --test compiler_warnings \
  --test compiler_array_settings --test cli
node tests/support/js_runtime.test.js
node tests/support/js_core_helpers.test.js
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B tests/reference/compiler-language/test_report.py
python3 -B tests/reference/compiler-language/test_inventory.py
python3 -B tests/reference/compiler-language/test_full_ledger.py
cargo bench --locked --bench compiler -- --samples 9 --iterations 32
```

Cargo and benchmark receipts are `/tmp/light-cljs-132-regressions.log`,
`/tmp/light-cljs-132-benchmark.csv` and `/tmp/light-cljs-132-benchmark.log`.
The final original-module oracle receipts are
`/var/folders/hm/d7rr9ds96qx995ns72ry9g4m0000gn/T/light-cljs-array-settings-oracles-wgubvxhp`.
The pinned metadata generator also exposed existing drift in other mapped core
Vars; the current change preserves only this setting's exact declaration row.
That separate reconciliation has live owner `light-cljs-135`.
