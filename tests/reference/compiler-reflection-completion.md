# Reflection and host completion evidence

`light-cljs-124` completes the supported printing, source reflection and explicit
host-hook contracts. This is family evidence, not a full-language completion
claim. All 21 public declaration/arity obligations owned by this family and all
23 owned local API rows have independently hashed application and macro-phase
execution witnesses. Source catalogs alone never supply execution evidence.

The new reflection suite checks real saved `re-matches` Vars obtained from core
public maps inside a required macro helper and in applications, including
dereferenced calls, source metadata and the runtime/macro spelling overlap for
`js-obj`. Source ordinary Var identity/metadata inspection leaves an unavailable
initializer undemanded; consuming its root triggers that initializer. Macro
identity and metadata remain visible while root consumption reports a phase
error. Private visibility, actual referred Var identity, constructor imports,
Namespace equality/hash/identity and public arities execute explicitly.

The compiler declaration view uses 889 pinned public name/kind declarations,
filtered by the executable registry. Nineteen newly exposed Var metadata entries
are reproduced from the pinned source, independent of test outcomes. Empty
source namespaces register in emitted programs. No upstream analyzer AST or
cache representation is emulated.

The complete pinned `var_test.cljs` adapter retains both support definitions,
all four original assertions, both metadata callback invocations and the two
function/Var `test` calls. Its complete program passes Node and QuickJS in
`complete_upstream_var_test_runs_metadata_callbacks_in_both_engines`. A fresh
checked reference run also passed release 1.12.134 and checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`; settings remained optimizations none,
static-fns false and def-emits-var false. Artifacts are under
`/tmp/light-cljs-124-var-reference`, with exact observations in
`/tmp/light-cljs-124-var-reference-outcomes.json` and
`/tmp/light-cljs-124-var-local.json`.

The complete `ns_test.cljs` source was read. Its `.clj` macro companions, Closure
host facilities and analyzer imports do not establish this project's intentional
source macro dialect. Existing namespace tests and the new reflection/host
tests retain executable source imports/refers/private metadata, required source
macros, saved callable values and actual namespace/session state; those tests
make no unchanged whole-project upstream namespace compatibility claim.

Printing source bodies, their exact negative-zero reference difference and
all writer/capture effects are described in
[printing completion evidence](compiler-printing-completion.md).
Fallible clock/random/file providers, explicit console/process capabilities,
bounded synchronous taps, injected scheduling, shared compiler loading and
original exception identity/recovery are described in
[host-hook evidence](compiler-host-hooks.md).

Executed source/accounting checks:

```sh
python3 -B tests/reference/compiler-reflection-completion.py
python3 -B tests/reference/compiler-printing-completion.py --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B tests/reference/compiler-language/test_full_ledger.py
node --test tests/support/js_runtime.test.js
git diff --check
cargo bench --locked --bench compiler -- --samples 9 --iterations 1
```

The generators, independently retained inventory, all 14 full-ledger accounting
tests, direct Node runtime assertions and whitespace check passed. The nine-sample
compiler performance smoke completed; raw observations are
`/tmp/light-cljs-124-compiler-bench.csv` (fresh macro-host analysis median
22.59 ms). These samples are not a historical comparative performance claim.

The first broader Rust run caught a default random hook capturing `Math.random`
too early. The default now invokes the current host method; explicit injection
still takes priority. Its focused core-helper, host and session rerun passed
3 + 5 + 20 tests. Strict library Clippy was also attempted; pre-existing library
style warnings prevent a `-D warnings` pass. No warning-as-error success or
ignored reference/tooling execution is claimed.

The complete broad Rust run used:

```sh
cargo test --locked --quiet --no-fail-fast -- --skip generated_map_keys_keep_reader_name_parts_before_runtime_spelling_equality --skip form_and_bounded_environment_use_actual_nested_call_and_shadow_ids
```

It executed 66 target summaries: 564 passed, 2 failed, 62 explicitly ignored.
The two exact macro regressions were excluded only because their demonstrated
pre-existing defects have live focused owners `light-cljs-130` and
`light-cljs-131`; this is not a full-suite pass. The regex deadline recovery
assertion failed under parallel compiler/reference load and passed its exact
isolated retry. That timing-dependent defect has live owner `light-cljs-133`.
The other failure identified stale historical SCI dispositions for APIs newly
supplied by this family; their final source-preserving reconciliation and
whole-corpus rerun are recorded below. Ordinary library Clippy passes with 49
existing warnings. The repository-wide formatting check also reports existing
formatting drift; no formatting pass is claimed and unrelated files were not
rewritten.

The focused host/session/core-helper rerun passed 5 + 20 + 3 tests. Source
reflection passed all 7 tests, and relevant protocol/record/eduction suites
passed 5 + 6 + 2 tests with one external oracle check explicitly ignored.
Printing and native string execution passed 9 + 4 tests, retaining the two
explicit native string oracle tests as ignored while separately executing their
58 canonical printing comparisons against both pins. These local results are
separate from the 52 complete upstream printing source sites and the full
pinned `var_test` observations described above.

The final historical SCI migration keeps its original seven columns byte for
byte. Fresh targeted audit observations distinguish newly agreeing results
from explicit compiler-macro, reader, printing and finite-result-domain
contracts. Remaining historical discrepancies previously carrying the narrower
124 owner now have the live `light-cljs-125` reconciliation owner and exact
source/profile leads in that Pellet; they remain nonpassing and never become
canonical coverage solely through queue closure.

The final 55-row targeted audit reviewed 24 newly portable observations and 31
remaining nonpassing comparisons. The 25 printing rows include 14 newly portable
observations and 11 differences independently established by pinned CLJS source
execution. Original record and raw output/flush bodies were also checked against
both pins; see the printing evidence for the exact depth/length behavior. The
final affected Rust run passed 46 tests with three explicit external oracle
checks ignored; direct Node runtime assertions passed. The 14 full-ledger and
41 report/failure-injection tests pass after source inventory regeneration.

The final full historical corpus rerun passed:

```sh
cargo test --locked --test evaluator_conformance every_historical_sci_case_has_an_executable_evaluator_disposition -- --exact --nocapture
```

All 2,475 disposition rows executed and matched their reviewed accounting:
1,208 portable, 304 explicitly adapted, 878 nonpassing gaps and 85 retirements.
This restores the executable source-preserving accounting gate; it does not
claim the 878 gaps or 85 retirements are CLJS conformance passes. Its complete
log is `/tmp/light-cljs-124-sci-whole-final.log`.
