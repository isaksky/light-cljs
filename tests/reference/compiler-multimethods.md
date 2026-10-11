# Multimethod and hierarchy evidence

`compiler_multimethod_evidence.rs` compares eight portable probes in QuickJS and
Node, Maven ClojureScript **1.12.134**, and checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. Three probes retain complete
upstream ClojureScript test bodies; five retain the source programs from SCI's
`multimethods_test.cljc` at **5fe89056384418be50d232b4278344cb3a1a526b**.
The separate SCI oracle executes those five programs against that exact checkout.
The fixture preserves original body hashes and expected observations. This is a
new focused comparison; it does not modify the immutable SCI outcome corpus or
claim that JVM interface dispatch is portable.

The full intended-language ledger additionally maps **33 assertion sites** from
five complete ClojureScript bodies: `test-hierarchy`, `test-multimethods-1`,
`test-multimethods-2`, `test-cljs-3367-backward-conflict-prefers`, and
`test-cljs-1144`. Each namespace program preserves setup definitions, loops,
mutation order, exception paths, and the original `cljs.core-test` identity.
`deftest`/`testing` become `do`; `is` becomes `assert`. Tests with custom hierarchy
Vars retain those Vars. Every source assertion remains in the pinned full-body
inventory with its original hash.

Two adaptations are explicit. `ObjMap` is aliased to `PersistentArrayMap` in the
hierarchy body, and `obj-map` is aliased to `array-map` in the edge-case body.
Private upstream ObjMap storage representation is outside the dialect contract;
these aliases preserve the hierarchy operations and every map construction but
do not establish distinct ObjMap representation behavior. The unused circle
helper spells the same host constant as `js/Math.PI` instead of `Math/PI`.

Sixteen public API/arity obligations have complete upstream source-body witnesses.
The [live verification snapshot](../../benchmarks/results/compiler-2026-10-11/multimethods/verification.json)
records 31 selected source sites and 16 public arities covered, with all 27
public macro-phase checks passing. Eleven public arities lack original source
assertion witnesses and remain missing in the ledger.
Local runtime and expansion-phase contracts live in `compiler_multimethods.rs`.
All 27 public names/arities also have hash-bound mappings to its explicit
`public_api_arities_execute_in_macro_phase` test.
Public operations absent from the five upstream bodies remain without ordinary
source-body witnesses, even when a local contract tests them: the current ledger
requires original upstream assertion mappings. Phase witnesses and live execution
are assessed independently. A fixture or mapping alone never marks an obligation
passed, and the full language gate still requires unrelated remaining work.

```sh
cargo test --locked --test compiler_multimethod_evidence
cargo test --locked --test compiler_multimethod_evidence -- --ignored --nocapture
cargo test --locked --test compiler_multimethods
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B tests/reference/compiler-language/report.py --run \
  --feature multimethods-hierarchies
```

The feature selection includes 31 multimethod/hierarchy-classified assertion
sites; two mixed-body assertions also retain their existing family assignments.
Whole-body execution still checks all 33. The complete source bodies and this
adaptation retain the upstream Eclipse Public License 1.0 notices; see the
compiler-language manifest and the notice headers in the namespace fixtures.

## Verification on 2026-10-11

- `cargo test --locked --test compiler_macros --test compiler_multimethods --test compiler_multimethod_evidence --test eval_sessions --test provenance --lib`: 157 passed; the two external-oracle tests are ignored by this ordinary invocation and were separately executed with `--ignored`.
- `cargo test --locked --test compiler_multimethods`: all 11 contracts passed again after the final conservative alias-demand change. They execute application observations with both QuickJS and Node; the macro API contract runs the 27 declared public name/arity obligations inside canonical compile-time JavaScript.
- `node tests/support/js_runtime.test.js`: passed, including method/cache rollback and frontier patch replay.
- `cargo bench --locked --bench evaluator -- --iterations 5`: passed every checked result. Aggregate samples were 72,567,166 ns for five fresh sessions, 564,125 ns for five reused-session executions, and 318,662,375 ns for five bounded-loop/reset/recovery runs. These are a smoke measurement, not a speedup claim.
- `git diff --check`: passed. `cargo fmt --check` reports repository-wide formatting differences; unrelated files were not reformatted. The new multimethod contract test was formatted independently.

The full unrestricted language gate, cross-platform execution, and the complete
repository test suite were not run for this focused change. The source-feature
report and reference oracle commands above are distinct from those broader gates.
