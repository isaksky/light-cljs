# Final intended-language gate — 2026-10-11

**Language completion remains incomplete.** This receipt validates implementation
HEAD `bab7423` plus the focused full-gate harness changes. The live completion
owner remains `light-cljs-125`; its zero-missing execution acceptance is unchanged.
Evaluator migration, closed family implementations and generated catalogs do not
establish language completion.

## Independent scope and current adapters

The pinned inventory retains 97 complete runtime/reference files, 1,328 bodies,
4,728 assertion sites and 1,327 intended public declaration/arity rows, plus
syntax, local API, SCI and assertion-free body/module obligations. All original
source, setup, assertion expectations and body hashes remain unchanged.
`extract_inventory.py --check` verifies freshness rather than semantics.

This change adds executing adapters and reviewable evidence:

- Five ordinary `.cljs` namespace macro/import adapters preserve the original
  reference trees and hashes. Local assertions use the adopted shared ordinary
  import maps; both references still execute the original JVM phase-map contract.
  These are registration/source contracts, not proof of a test-framework macro's
  expansion. Thirty exact diagnostic probes retain their original reference
  sources, locations, categories and exception classes. The local duplicate-alias
  adaptation preserves ordinary imports; old local diagnostic text stays in
  provenance. A nested namespace declaration in a deferred `def` initializer now
  produces its precise top-level-context diagnostic.
- Twenty-four complete promised set/string/walk bodies retain all setup and
  assertion paths, adding 225 source-site mappings and 49 public arity mappings.
  Each complete body also executes as a local application and macro computation
  on Node and QuickJS. The complete walk metadata body records every failing
  assertion ID before returning: release `1.12.134` loses list and lazy-sequence
  metadata (original paths 001 and 005), while the checkout and both local engines
  preserve it. That difference remains separate from execution passes.
- Twenty-two already mapped complete upstream core bodies and six independent
  literal workflows add 260 reviewed operation mappings. Direct-call/arity
  receipts exclude quoted/data/discarded forms and ambiguous thread-injected
  arities. Saved-function contracts assert their bindings, calls, values and
  effects. Metadata cross-check rows establish the advertised operation only.
  Review removed the keyed three-argument `clojure.set/join` mapping because its
  complete upstream body calls only the two-argument arity; that evidence gap
  remains required under the full gate.
- The new SCI catalog retains 1,174 unchanged portable sources and their original
  source/row/expectation hashes. Exact compiler-emitted Node and QuickJS results,
  trace and output are compared with both checked pins. Raw observations survive
  the existing documented Number/character/finite list-versus-sequence projection.
  A canonical-only print prefix faithfully supplies the original streaming output
  profile. Three `Box`/`Empty` cases run in isolated reference processes to prevent
  prelude mutations from contaminating later cases. Lazy value/trace realization
  now stays inside the observation harness's stdout capture, with stream recovery
  tested even on encoder failure.

At this gate invocation, the catalogs contained 667 ordinary operation mappings and 702 phase mappings
bound to 30 exact named Rust tests. Catalog presence supplies no pass. A local
application/macro test without a canonical reference fixture now remains missing
unless it has an explicit adopted dialect contract. Both pins are required for
compatibility-intended local operations; registration cannot invent observations.

## Executed unfiltered gate (historical receipt)

```sh
python3 -B tests/reference/compiler-language/report.py --run --strict \
  --reference both --checkout research/clojurescript --no-build \
  --binary target/debug/light-cljs --timeout 300 --reference-timeout 600 \
  --artifacts /tmp/light-cljs-125-full-current
```

This unfiltered invocation exited **1**, correctly retaining `strict_complete: false` and `full_scope_selected: true`. It executed 911 registered local witness cases, both reference paths, all 27 supplemental boundaries, 30 exact named phase tests and the unchanged 1,174-row SCI cohort. All **702** mapped phase observations passed. They do not establish phase eligibility for the other ledger rows. The SCI intersection is **1,079** fully agreeing rows: local Node/QuickJS have 1,117 passes, 25 mismatches and 32 compilation rejections; release has 1,093 passes, 74 mismatches and seven rejections; checkout has 1,091 passes, 76 mismatches and seven rejections. No missing-tool, timeout or zero-test outcome is counted as a pass.

| Full ledger result | Rows |
| --- | ---: |
| Covered | 4736 |
| Verified source rejection | 43 |
| Verified warning | 31 |
| Missing | 3596 |
| Explicit version difference | 207 |
| Explicit unrelated/internal/dialect/host disposition | 1377 |


Missing rows comprise 215 source assertions, 1054 public declaration/arity rows, 22 syntax rows, 974 local API rows, 1316 SCI rows, 12 assertion-free test bodies, 3 source programs. None of the registered intended source witnesses has a missing observation; these source gaps have no executing adapter. Operation/phase bindings with absent pinned compatibility remain missing.

The inherited historical-selected subreport has 2,287 covered, 43 source rejections, 31 warnings, eight missing, 166 version differences and 702 unrelated rows. The eight historical namespace-AST assertions are deliberately not selected by the intended gate; this invocation does not claim completion of that different historical scope.

The exact inputs, invocation records, emitted JavaScript, raw observations and
machine report are retained under `/tmp/light-cljs-125-full-current/`; the summary
log is `/tmp/light-cljs-125-full-current.log`. Warnings, located source rejections,
version differences, unavailable tools, skipped execution and timeouts cannot
substitute for literal ordinary/phase evidence.

## Confirmed implementation owners

Duplicate searches of the complete live and historical queue found no focused
live owners before creating these independent records. Both pin identities and
none/false/false compilation settings were verified.

| Exact contract | Local observation | Both pinned observations | Live owner |
| --- | --- | --- | --- |
| `(first (macroexpand-1 '(let [[x] [1]] x)))` and analogous fn/defn/letfn probes | Standalone emitted Node/QuickJS throws missing expansion service; user macro computation fails in the macro phase | `let*`, `fn*`, `def`, `true` | light-cljs-145 |
| `(let [f '#(%0 %-1)] [(count (second f)) (count (nth f 2))])` | `[0 2]` in both local engines and fresh EvalSession | `[2 2]` | light-cljs-146 |

Hosted EvalSession application execution of all four macroexpand probes already
succeeds because that embedding installs form services. Preserve those consumers;
the focused defect is the canonical static quoted-source/macro-phase contract.
`core.cljc:3323` defines the canonical expansion-time macro. This finding does not
promise arbitrary eval/load services in a bare runtime. The ignored service audit
captures failures and never supplies coverage.

The reader reproduction observes quoted parameter bindings independently of
invocation, services or namespace state. Original SCI expectations remain intact,
including their historical `[0 2]`; canonical reconciliation must not erase them.
Precise programs, source hashes, both-pin observations and acceptance live in the
Pellet descriptions. Supplemental forensic artifacts are
`/tmp/light-cljs-125-sci-focused-blockers.json`,
`/tmp/light-cljs-125-sci-service-phase-probes.tsv` and
`/tmp/light-cljs-125-sci-remaining-audit.json`.

A focused fresh hosted audit of all 95 initially nonagreeing SCI expressions
preserves their original exact sources and expectations. Its classifications are
30 macro-declaration context adapters, 19 SCI reader extensions, 12 namespace/
source/Var metadata contracts, nine hosted services, eight pin version differences,
seven canonical rejections, six macroexpand phase contracts, three `Box`/`Empty`
prelude collisions and one nonpositive-placeholder discrepancy. These are explicit
reconciliation leads, not a blanket dialect exclusion or execution pass. New final
SCI observations and any changed intersection are reported by the executed gate.

## Explicit outside scope

The intended language scope retains pure transients, transducers, sequence/
eduction, sorted collections, multimethods, public core protocols/constructors and
the promised string/set/walk/EDN/reader facilities. Public operations cannot be
excluded because their upstream implementation uses private storage.

| Outside boundary | Adopted scope basis |
| --- | --- |
| Shadow/Closure build systems, bundling, hot reload, assets/WASM, dependency acquisition and unchanged application ports | Explicit prior compiler milestone exclusions; ecosystem ports are not language acceptance substitutes |
| Standalone spec/pprint/test/math/reducers/data/datafy/browser libraries | Not among the promised standard library namespaces; core operations used by them remain intended |
| Newer async/await/proxy libraries | Outside the adopted synchronous language milestone; ECMAScript promises/proxies are not inherently forbidden |
| Browser/npm/filesystem/network/clock/scheduler facilities | Require explicit embedding capabilities; pure operations remain intended |
| Upstream self-hosted compiler, exact analyzer AST/cache, generated JS/optimizer spelling, private trie storage and optional lite collections | Explicit compiler/build/private representation boundaries; exposed public constructors/protocols remain intended |
| JVM/GraalVM, `.clj` macro companions, native exact integer/ratio/BigDecimal tower and retired native ownership/GC descriptors | User-adopted JavaScript dialect and evaluator migration; JS BigInt interop is not a promised numeric tower |

Macro/source/import/phase differences require their local executing contracts.
Unsupported expansion/export result domains remain diagnostics; they do not prove
that intermediate language operations are unavailable.

## Additional verification and remaining acceptance

- `cargo test --locked --tests --no-fail-fast` traversed 77 targets: 618 passes,
  one failure and 64 ignored. The tiny-program target built a 539 MB single Node
  source input, exceeding V8's string limit. Its harness now executes bounded
  independent batches with original case order, namespace indices and every
  literal assertion retained. Complete `cargo test --locked --test execution`
  then passed all 53 active tests, with 43 explicitly ignored. Every other target
  in the full traversal passed: 619 distinct active checks now have passing
  current evidence. Ignored oracle/measurement/audit checks are not silently
  counted as passes or all claimed rerun.
- `cargo check --locked --all-features --all-targets`: exit 0, repeated after the
  batching fix. Inventory `--check` and pinned core Var metadata generator
  `--check`: exit 0. The new core/library adapter generators and SCI catalog
  independently reproduce their source identities.
- `python3 -B -m unittest discover -s tests/reference/compiler-language -p
  'test_*.py'`: all 82 tests passed. These check provenance, both-pin/engine
  requirements and honest runner failure accounting; they add no semantic rows.
- `node tests/support/execution.test.js`, `js_runtime.test.js`, and
  `js_core_helpers.test.js`: exit 0. All five final ordinary namespace fixtures
  also recompiled and passed their Node observations after an equivalent Unicode
  escape retained two original docstring spaces without trailing whitespace.
  Original reference-tree bytes remain unchanged.
- `cargo bench --locked --bench evaluator -- --iterations 20`: exit 0; fresh,
  reused and sticky bounded-loop/reset recovery all validated. Twenty-iteration
  totals were 380,552,333 ns fresh, 2,480,083 ns reused and 1,533,077,666 ns bounded
  recovery. This normal validation is not an interleaved performance comparison
  or a throughput-improvement claim.
- All four new Rust test files pass rustfmt. Staged `git diff --check` passes.
  Existing untouched rustfmt drift in execution/reference-path and integration
  assertions remains preserved; it does not justify unrelated formatting changes.

Additional logs are `/tmp/light-cljs-125-regressions-current.log`,
`/tmp/light-cljs-125-execution-rerun.log`, `/tmp/light-cljs-125-python-final2.log`,
`/tmp/light-cljs-125-check-final.log`, `/tmp/light-cljs-125-perf-current.log` and
`/tmp/light-cljs-125-local-namespace-final.log`.

Full acceptance is not met. The remaining original source/public/local/SCI/body/
module and phase evidence, required ignored/upstream checks and zero-missing full
gate remain unfinished. The focused light-cljs-145 reconciliation below supplies
its four original source/phase rows and one public arity; the next implementation
owner is light-cljs-146, followed by light-cljs-125 for the remaining full evidence
reconciliation. Focused implementation fixes do not satisfy the full gate.
The completion Pellet stays open with this resumable receipt; no standard-language
criterion or original expectation has been relaxed.

## Focused quoted macro expansion reconciliation — light-cljs-145

The four macroexpand-1 failures recorded above now pass unchanged source
execution in bare Node and QuickJS and both pinned references. A normal demanded
macro test supplies their four exact phase mappings and the public `[quoted]`
arity; its separate literal oracle also passes both pins. The focused ledger
receipt has five covered rows with passed macro phase columns. See
[quoted source and phase evidence](quoted-macroexpand-evidence.md) for complete
source/test hashes, observations and commands. The earlier unfiltered gate
counts and SCI classifications remain the original historical receipt; this
focused run does not establish a new full strict result.

## Focused nonpositive anonymous reader reconciliation — light-cljs-146

The exact `(let [f '#(%0 %-1)] [(count (second f)) (count (nth f 2))])`
source now returns canonical `[2 2]` in Node, QuickJS and both pinned references.
Its normal demanded macro test computes the complete twelve-shape contract via
both quotation and runtime reading in application and macro phases. The ledger
records an explicit canonical dialect difference while preserving the original
SCI `[0 2]`, source/row/body hashes and every raw mismatch status. The focused
receipt retains `strict_complete: false`. See the
[reader audit](../compiler-anonymous-nonpositive.md) for full observations,
phase hashes, verification and the six unchanged historical macro diagnostic
accounting failures retained under light-cljs-125. This scoped reconciliation
does not replace the earlier full-gate receipt or complete its remaining rows.

## Recovered supplemental execution evidence — 2026-10-11

The interrupted light-cljs-125 preparation was recovered at HEAD `5a09986`.
No verification processes were still active. Original source bodies, assertions,
SCI fields and expectations were retained; generated bytecode was restored to
its committed bytes. This recovery adds evidence adapters, without declaring
language completion or changing production semantics.

- Twenty-one complete residual fixtures retain nineteen upstream bodies and
  two complete equality callers, adding 107 original assertion links and 26
  phase rows. The current source-runtime `pr-str` observer avoids interpreting
  a namespace-runtime collection through another runtime instance. All 21
  fixtures passed Node/QuickJS application and demanded macro computations in
  165.36 seconds. Eighteen exact recovered reference sources plus one fresh
  `test-717` adapter run supply both-pin ordinary evidence for the nineteen
  bodies; the equality callers reuse their unchanged full reference contracts.
  See [residual provenance and exact commands](../compiler-residual-bodies.md).
- 131 existing public protocol/constructor/factory/iterator operation mappings
  now link 29 complete literal oracles. Another 51 utility, regex and tagged-data
  mappings link two new complete oracles and one reused tagged-data oracle.
  These retain declaration/signature/source/test hashes and literal expectations;
  catalog registration alone supplies no execution credit. Recovered independent
  local/two-pin observations were verified, and all four public family suites
  passed again (13 tests, zero ignored). Generator checks preserve their owned
  rows without reordering unrelated evidence. See the [public links](../compiler-public-gate-links.md)
  and [utility/regex/tagged-data links](../compiler-surface-gate-links.md).
- The full runner now registers 120 additional complete SCI catch-selector
  adapters, with structural UTF-8 edit receipts and original result/effect/output
  expectations. Two exact host-input sources still differ from both references;
  the other 118 have independently agreeing observations in Node, QuickJS,
  release and checkout. The record-method fixture explicitly supplies SCI's
  original namespace-global observe callback. No original corpus/disposition
  bytes were changed. See [complete SCI context evidence](sci-context-evidence.md).
- Fifty other unchanged original SCI sources have a dedicated sequential
  macro/namespace/source-position adapter and immutable fresh raw receipts.
  Its 35 original-source agreements, eight verified reference version
  differences and seven unresolved contracts are supplemental evidence, still
  separate from full-ledger integration. Rejections, phase effects and changed
  reference traces never become historical SCI passes. See [original source-context receipts](sci-source-context-evidence.md).
- Six historical quoted-macro gap observations have exact migration diagnostic
  receipts after light-cljs-145, preserving all fourteen original fields and
  nonpassing status. Updated source attribution does not prove SCI compatibility.
  The full evaluator disposition regression verifies the current diagnostic
  observations separately from the immutable original expectations.

Two optional lite-storage final assertions, three standalone test/spec alias
bodies plus their source program, and two assertion-free JVM analyzer callers
retain explicit outside-scope dispositions. Exact source hashes and regression
checks protect those boundaries; ordinary transients, namespace aliases/renames,
quoted-source ordering and Var behavior remain intended and independently
required. These dispositions apply the existing adopted lite/library/compiler
API boundaries, not a newly reduced standard-language scope.

The fresh nonexecuting command
`python3 -B tests/reference/compiler-language/report.py --strict --output /tmp/light-cljs-125-recovery-static-report.json`
exited **1**: zero covered, 8,605 missing and 1,385 outside dispositions, all 27
supplemental contracts not run. This is static accounting only; it neither
supersedes the earlier executed 4,736-covered/3,596-missing receipt nor counts
new focused evidence as an unfiltered full-gate pass. The unchanged full gate
was not repeated during recovery because independently confirmed canonical
omissions remain open.

Focused owner light-cljs-147 retains both-pin evidence for canonical quoted
`when` expansion omitting the else form. New focused owner light-cljs-148 retains
both-pin depth-printing evidence: `(binding [*print-level* 0] (pr-str [1] {:a 1}
#{1} #js [1] #js {:a 1}))` is `"# # # # #js #"` canonically, but locally
`"# # # # #"`. The printing probe deliberately remains mismatched locally.
Release negative-zero printing is separately a verified version difference;
no original printing expectation was silently rewritten. Detailed diagnostic
sources, settings and receipts are in the surface-links document and checked
`compiler-surface-printing-probes.py` reproducer.

Fresh twenty-iteration `cargo bench --locked --bench evaluator -- --iterations 20`
validated fresh/reused sessions, result release, sticky poll-budget failure,
reset and recovery: 364,265,958 ns fresh, 2,268,416 ns reused and 1,453,673,708 ns
bounded recovery for twenty iterations. This is a normal focused smoke check,
not a comprehensive performance acceptance claim.

The zero-missing full execution gate, all remaining source/public/local/SCI/body/
program/phase integration and required ignored/upstream checks remain unfinished.
The useful preparation is committed; light-cljs-125 remains open and its current
workspace ownership is released for light-cljs-147, then light-cljs-148, then the
full gate. No standard-language acceptance criterion was relaxed.

Recovery verification: `python3 -B -m unittest discover -s tests/reference/compiler-language -p 'test_*.py'`
passed all **100** runner/accounting/provenance tests. The complete
`cargo test --locked --test evaluator_conformance -- --nocapture` passed four
active tests in 197.94 seconds, with its two explicit audits still ignored.
The residual test passed one exact complete-body test; the source-context driver
passed its normal macro/effect/location test and the explicit 50-source audit.
All eight affected family/overlay/residual generator checks, the pinned inventory
`--check`, scoped Rust 2024 formatting and `git diff --check` passed. Formatting
changed only two cosmetic Rust source spans; residual phase hashes were freshly
regenerated afterward. These checks do not replace the required broader full
regression/upstream/ignored/performance acceptance. Recovery logs are under
`/tmp/light-cljs-125-recovery-*`; exact reference source receipts are linked above.


## Evidence continuation after light-cljs-147 and light-cljs-148

Both earlier focused repairs are closed at commits `1b33fa0` and `9f9a3c5`.
This continuation adds useful evidence while preserving the original acceptance:

- Three independent literal higher-order/collection workflows now supplement the
  twenty-two complete upstream core bodies. Their public factory arities, saved
  calls, returned functions, memoization effects, trampoline recursion, collection
  transformations and transducer paths add 91 reviewed public/local operation
  receipts (351 total; nine independent literal oracles). All six core-gate tests
  passed in 6.90 seconds, executing the workflows in Node/QuickJS applications
  and macro phases; all three new literal sources independently passed both
  pins. Sources/expectations remain adjacent in `compiler-core-gate.py`; exact
  calls/signatures are reproduced by `compiler-core-gate-mappings.py`. Fresh
  reference artifacts: `/tmp/light-cljs-125-higher-order-reference-final`.
- Three more complete assertion-free upstream bodies retain their actual final
  returned values. The original array-access macro module stays byte-identical;
  a separate suppression/restoration contract returns `[true true]`. All four
  pass both engines/phases and both references. No assertion is invented for an
  assertion-free source. The canonical driver alone explicitly imports the
  unchanged macro module with `:require-macros`; local ordinary import semantics
  are retained. See [complete residual evidence](../compiler-residual-completion.md).
- The fifty original SCI source-context contracts are now specialized within the
  full runner. They replace matching generic-expression adapters only after the
  original source/row/obligation/body/expectation identities agree: 1,294 generic
  rows become 1,244 generic rows plus fifty specialized rows, with no source lost
  or executed twice. Fresh local/reference validation gives 35 covered, eight
  explicit pin differences and seven missing. Stored receipts never grant a
  present pass. Both local engines must match the same independently executed
  pin for a version difference; original SCI agreement remains unresolved. An
  unrelated public macro operation receives no eligibility from these sources.
  Fresh artifacts: `/tmp/light-cljs-125-sci-source-context-integrated-current`.

- Sixteen complete SCI string/set/walk/EDN sources add a separate import-context
  scaffold, preserving original require/alias/:refer declarations, callback
  ordering, printing and return values. Both local engines pass all sixteen;
  both references agree with fifteen originals and throw for the original EDN
  discard contract expecting `2`. That original expectation remains unresolved.
  This bounded cohort is supplemental, not full-ledger coverage; its plan retains
  another 181 unexecuted candidate sources. See
  [library source/context evidence](sci-library-context-evidence.md).

The two newly confirmed semantic omissions have focused live owners immediately
before this gate: light-cljs-149 (variadic callable-type `IFn` rest binding) and
light-cljs-150 (persistent map/vector/set Object `.equiv`). Their complete original
probes and SHA256 bindings are reproducible in
`compiler-residual-completion-omissions.json`. Both pins independently return the
original expected result, while local application and macro paths at `ce1487c` fail.
The full interop body retains all seven assertions and `LITE_MODE false`; the
Partial body retains its original variadic implementation and actual result `3`.

A focused light-cljs-149 continuation subsequently repaired variadic IFn rest
binding. The unchanged original Partial body now returns `3` in both application
engines and QuickJS macro computations, matching fresh observations from both
pins. Only its exact assertion-free body, phase and reference mappings were
integrated. Seven supplemental callable contracts also match both references,
including borrowed receivers, overload behavior, ordered effects and saved
sequence entries on infinite inputs. See the dated continuation in
[residual completion evidence](../compiler-residual-completion.md).
A focused light-cljs-150 continuation then repaired persistent collection
Object `.equiv`, retaining all seven unchanged non-lite upstream assertions.
Both applications and independent QuickJS macro computation now return `true`,
matching fresh observations from both pins. Seven assertion mappings and the
full source/body/phase provenance are integrated; nine supplemental contracts
preserve custom equality, receiver binding, metadata, sequence compatibility,
hash membership and identity. Focused owners light-cljs-151 and light-cljs-152
retain separate demonstrated sequence/name Object-dispatch and false extension
cycle defects. The full light-cljs-125 gate remains unfinished; this repair
supplies no comprehensive zero-missing claim.

The full fourteen-row/eight-predicate integer table, private tapset-dependent
setup, complete 100,000-trial generative sources, inference checks and invocation
module remain honestly accounted for. An undefined declared function also fails
canonical invocation-module initialization; this observation supplies neither a
pass nor an automatic exclusion. No missing table row, loop, branch or expectation
was removed to make an adapter pass.

The fresh nonexecuting strict report exits 1, with zero covered, 8,605 missing and
1,385 outside dispositions. This static check grants no semantic credit. The
comprehensive zero-missing `--run --strict` gate and broad remaining acceptance
remain unfinished; another unchanged incomplete full run is not evidence of
completion. Useful evidence is saved and light-cljs-125 is released behind its
focused blockers, rather than closed. No production implementation changed in
this continuation.


Continuation verification: all **103** existing/updated runner, accounting and
provenance tests passed in 147.55 seconds; the **six** new library-context tests
passed separately. The final focused Rust run passed eight active tests (core,
residual and source-context contracts); its two explicit source/library audits
remain ignored in the normal invocation and have separately executed fresh
50-source and 16-source receipts. Core/residual generators, inventory `--check`,
scoped Rust 2024 formatting and `git diff --check` passed. These checks validate
this evidence work; they do not replace the unfinished full regression,
upstream/ignored and performance acceptance.


### Predicate, qualified core and complete namespace continuation — 2026-10-11

This continuation starts from `83070de`, after focused owners 149–153 closed.
It preserves the original gate criteria and every original expectation. The
small in-scope public `js-fn?` omission is repaired as a compiler-emitted macro:
it evaluates its single operand once and recognizes JavaScript functions, while
metadata-wrapped functions remain `fn?`/`ifn?` without becoming `js-fn?`. Wrong
arities produce located failures; the same MacroHost recovers afterward. Actual
macro computations retain independent runtime roots and ordered effects.
The exact function predicate contract matches both required pins in
`/tmp/light-cljs-125-predicate-discovery`; broad numeric/type predicates match
both pins in `/tmp/light-cljs-125-predicate-domain-reference`.

Core evidence now has **468** reviewed source/arity call receipts, up from 351.
There are **44** executable complete workflows: 22 original upstream bodies and
22 independently specified literal workflows. Eleven literal workflows are also
executed with actual `clojure.core/` qualified calls; only executable call heads
are qualified, leaving quoted data, lexical bindings, factories and effects
unchanged. Both pins pass all eleven in
`/tmp/light-cljs-125-qualified-core-reference`. Twenty-one literal oracles have
independent obligation mappings in the full report; a redundant qualified
workflow remains in the executing core fixture without manufacturing a new
mapping or an unreferenced report fixture. Public arity and phase receipts bind
exact source signatures, original body hashes and their complete executing tests.

Another **13 complete original bodies / 22 original assertion sites** now have
source/setup/reference/application/macro adapters, including original macro
modules and metadata assertions. Required `.clj` modules stay byte-identical in
canonical reference trees; local `.cljs` ordinary macro imports have an explicit
dialect adaptation. The full integer table, private tap setup, complete
generative/inference/eval sources and complete invocation module retain an
explicit reviewed basis in `compiler-residual-source-boundary-review.json`.
That review changes no disposition and supplies no execution credit.

Two complete pending source contexts reveal distinct production defects:
`ns_shadow_test.cljs` requires namespace `baz` and defines a same-named ordinary
function, whose original recursive call fails in both local application engines
and actual macro compilation; the original top-level lexical `let` definitions
in `top_level.cljs` pass applications but macro demand cannot find `bar`.
Both full originals pass both required pins. Their exact sources and hashes are
retained under `unwitnessed-source-cases` in the source-context proposals, with
fresh local/reference receipts in the residual completion documentation.
Focused ordinary owners **light-cljs-154** and **light-cljs-155** precede 125.
Neither failing program is a passing TSV entry, boundary exclusion or partial
assertion substitute. They are not implemented in this continuation.

Focused regression checks passed: 21 library unit tests, 12 bootstrap tests,
104 macro tests, 10 evaluator tests and 53 execution tests; 44 explicit ignored
reference/consumer tests remain distinct. The final evidence/predicate run
passed 11 active tests with one explicit library audit ignored in normal mode;
the complete library cohort executes that named ignored driver separately.
All three direct Node runtime/helper/observation scripts passed.
`run_compiler.py --samples 9 --iterations 1` completed validated measurements:
180 fresh-process rows, 423 resident rows and 423 allocation rows, with exact
binaries/source archive/hash manifests under
`/tmp/light-cljs-125-predicate-compiler-performance`. These measurements provide
current validated outputs, without a before/after timing claim. Primitive
output sizes remain 88 / 297 / 126 bytes. Initial invalid one-sample arguments
and a nonexistent evaluator-reuse Cargo target were corrected before these
successful checks and never treated as passes.

The complete `--run --strict` zero-missing acceptance is still unfinished.
Fresh library evidence and the nonexecuting accounting check are recorded below;
static rows, retained receipt history and a repeated incomplete full run do not
grant semantic credit. Useful coherent work is committed, then 125 is released
behind its focused namespace owners rather than closed.


The fresh nonexecuting strict accounting check exits 1 with **0 covered,
8,605 missing and 1,385 outside dispositions**, preserving all 9,990 rows.
`extract_inventory.py --check`, all three core/residual generators and reviewed
mapping reproduction pass. Scoped Rust 2024 formatting and `git diff --check`
pass. A full check of `compiler_bootstrap.rs` still reports its 129-line baseline
formatting diff, reproduced from unchanged HEAD; the sole new macro registry
entry follows the existing formatted list, and unrelated formatting is preserved.
The initial Python run occurred before the expanded active receipt file was
written and correctly rejected the old sixteen-receipt file; its five stale
receipt errors are not passing evidence. Final fresh receipt and Python results
follow.


The expanded SCI library cohort now executes **191 complete original sources**
(string 34, set 57, walk 44, EDN 56), retaining the initial sixteen source
bindings and their complete active/nested evidence history. Both local engines
match 183 originals and differ from eight; release matches 163 and checkout
169. **163 four-way agreements are covered; 28 original contracts remain
missing**, including six release/checkout metadata differences and the retained
EDN discard callback/throwing difference. No additional local-vs-both-pins
implementation gap was found. Six remaining library candidate contexts retain
explicit `not-run` planning rows, without an inferred pass or exclusion.

The full runner executes this independent import cohort fresh and checks exact
compiler/driver/adapter/library/oracle/source hashes, raw Unicode-safe JSON
frames, identities, both engines and both pins. It preserves disjoint generic,
source-context and library identities, and grants no macro-phase credit or
automatic version reconciliation. Receipt replay verified all 191 current rows:
`/tmp/light-cljs-125-sci-library-context-191-current/evidence.json`. Independent
read-only review found no material defects in the predicate macro, qualified
call/arity mappings or fresh library merge. Final Python checks include the
static-all-missing/fresh-agreement-only full-ledger regression and fail if these
original mismatches are excused.


A bounded integration projection of the **actual fresh 191 library layers** into
the complete ledger produces **163 covered, 8,442 missing and 1,385 outside**,
with `strict_complete: false`; all other obligations were explicitly not run.
Its retained original mismatches still cause the execution-failure check to
fail. This verifies the new merge without relabeling the projection as a
comprehensive run: `/tmp/light-cljs-125-library-ledger-integration.json`.


Final Python verification checked all **117 current runner/accounting/provenance
cases**. The full 117-case invocation passed 116 and retained one already-loaded
obsolete test assertion that required arbitrary build-warning text changes to
be rejected; valid warning text is retained, while malformed stderr types are
rejected. After the test-only correction, all **13 current library adapter and
ledger tests** passed in the targeted rerun (8.46 seconds). No source expectation,
semantic comparison or acceptance criterion changed. Fresh 191-row literal
receipt replay, inventory checks, generators and diff checks passed afterward.
The corrected test and the earlier stale sixteen-receipt failures remain
recorded as failed attempts, rather than being hidden by a pass claim.


## Independent full-ledger evidence refresh — October 11, 2026

From frozen production HEAD `95abbfdefedbdc09268a30ffd891e137f5743fc7`, this
continuation changes the semantic harness, source adapters and receipts only.
No production implementation is claimed. The public core gate now retains
**50 workflows**, comprising **22 complete original upstream bodies** and
**28 independently specified literal workflows**. Six new workflows exercise
sequence consumers/producers and nested updates/name operations, in ordinary
and `clojure.core/` qualified call forms. Ordered atom effects prove actual lazy
factory consumption and reduce-kv callbacks; expected values are literal and
were written independently of local outputs. Their executing call heads add
**292 reviewed public-operation receipts**, for **760 total**, and six exact
both-pin oracle links, for **27 total**. Registration supplies no semantic pass.

All six Rust core cohort tests pass local Node/QuickJS applications and actual
macro computation. Both required compilers pass the six new exact fixtures at
the checked settings. The independent seven-case reference document also
retains the separate failing-local `prim-seq` probe:
`/tmp/light-cljs-125-core-independent-reference/evidence.json`, SHA-256
`f5a43cb23cf938f75a93d41c60e4e55a264c885ea4cf7b5fe4b1fb62dde105cf`.
The original authored consumer workflows that included `prim-seq`, exact
six-result probe, literal expectations and located compiler failures remain in
`../compiler-core-unresolved-probes.json`. The passing consumer workflows
retain their other operation paths. No original upstream body was shortened.
Public `prim-seq` arities one/two are still intended and missing, with focused
ordinary owner **light-cljs-157**; compile rejection precedes runtime execution.

The new complete source contexts add **20 original assertion sites across
five complete bodies**. In total, **11 supported contexts retain 17 complete
bodies and 45 original sites**. One executing Rust test checks both local
engines and both phases with fresh and reused macro hosts, and passes. Exact
original namespace clauses/helpers, eager nested assertion messages, the
static namespace, and three complete runtime tagged-reader handlers retain
both-pin source proofs. The original renamed-core `exists?` body remains
separately uncovered: applications and both pins return true, but macro demand
forces an unavailable excluded Var. Its complete original program and failure
receipts remain in the source proposals; focused owner **light-cljs-158**
precedes the broad evidence owner. The source adaptation and exact receipts are
in `../compiler-residual-completion.md`.

All **191 original SCI library sources** were executed again in both local
engines and both pinned compilers after production hashes changed; complete
prior records, including nested historical failures, remain verbatim in each
new active receipt. There are **163 original four-way agreements**, **six exact
verified walk release/checkout differences**, and **22 unresolved original
contracts**. The six classifications require actual distinct literal pin
observations, checked walk source hashes and both local engines matching the
same reviewed checkout observation. Their original SCI expectations remain
unresolved. Other mismatches remain missing and still fail execution accounting.
The fresh document is
`/tmp/light-cljs-125-sci-library-context-191-95abbfd/evidence.json`, SHA-256
`dc1a2971ae9fee862c4812f3ebf7edc50da8a240a1fee71ddaf94db65fb0b2f6`.
The [library evidence](sci-library-context-evidence.md) records exact source
basis, replay commands, mutation checks and current hash bindings. These runtime
contracts grant no macro-phase evidence.

The complete zero-missing `--run --strict` acceptance remains unfinished.
Independent static accounting still retains all **9,990 rows**: **zero covered,
8,605 missing and 1,385 outside**. Static registration and stored receipt history
grant no semantic credit. A projection of only this fresh library audit retains
all other execution and phase columns as unexecuted and remains incomplete;
it is not a comprehensive gate invocation. This partial fresh projection reports
**163 covered, 8,436 missing, six version differences and 1,385 outside**, with
`strict_complete: false`; its artifact is
`/tmp/light-cljs-125-independent-library-projection.json`. No unchanged incomplete
full run is presented as progress.

The separate **light-cljs-156** asynchronous cancellation repair is an external
limitation: automatic approval review rejected that action twice, citing
possible cybersecurity risk. This continuation did not retry or delegate that
repair, investigate the native crash, or run its cancellation/limits suites.
That unresolved acceptance prevents completion of the broad Pellet even after
independent evidence work. This evidence is saved coherently and 125 is released
with its original requirements intact.


Verification on the frozen production checkout passes the six core Rust tests
and the one complete source-context Rust test (all local phases/engines and
fresh/reused hosts), all 14 focused library adapter/receipt tests, the integrated
library ledger test, and exact fresh receipt replay. Core workflow reproduction,
all 760 reviewed mappings, source-context catalog reproduction, inventory
regeneration and whitespace checks pass. Independent read-only review found no
expectation weakening, false `prim-seq` credit, or broad mismatch exemption.
Scoped Rust 2024 formatting passes for both edited tests. Repository-wide
formatting fails in 29 files, each independently verified byte-identical to
baseline HEAD; no unrelated formatting rewrite is included. No new production
performance claim is made by these harness/evidence changes.


Expanding the reviewed call-name set also qualifies the existing collection
workflow's `map` call. That exact changed existing fixture passes both pins
independently at current settings, with literal expectations unchanged:
`/tmp/light-cljs-125-qualified-existing-reference/evidence.json`, SHA-256
`b83851b60444a66cf8bf2247792f6609cd9df22b4c31e6e8e04b8401bd885a70`.


Final full Python discovery passes **119 tests**, zero failures/errors, in
587.734 seconds, including the full-report strict incomplete accounting and all
adapter/hash/pin/history mutation cases. Log:
`/tmp/light-cljs-125-independent-python.log`. The focused Rust command passes
**seven tests**, zero failures/ignored, with log
`/tmp/light-cljs-125-independent-focused.log`. All production files remain
byte-identical to frozen HEAD; no cancellation/limits suite was run.


## Array/protocol and original source refresh — October 11, 2026

From frozen production HEAD `a8e8a43fda48f4f22674c6f82ec3ee048c99c3fc`, this
continuation adds six whole literal workflows: array dimensions/identity and
ordered mutations, direct persistent collection protocol calls, and custom
state protocol dispatch, with ordinary and `clojure.core/` qualified variants.
All declared custom ISwap arities execute, including the last sequence-valued
argument; reset/deref, volatile and delay protocols retain literal results and
ordered effects. Array paths preserve real nested arrays, cloned identity,
saved calls, receiver `this-as`, public js-invoke and once-only effects. Direct
metadata protocol checks observe the newly attached metadata, not just equal
collection contents. All application and actual macro checks run Node/QuickJS.

The core gate now has **56 workflows** (22 full upstream bodies and 34 literal
workflows), **865 reviewed call receipts** (105 more), and **32 mapped literal
both-pin oracles** (five more). Public protocol method definitions are never
executing call heads: a structural traversal visits their bodies while preserving
method names/bindings, and three explicit evidence tests protect call arities,
qualification and quoted forms. Independent review found no false declaration
credit or expectation weakening.

Both complete authored consumer workflows that originally exposed prim-seq are
restored **byte-exact**, including the two calls and all literal expected values.
They pass current local phases/engines and both pins. The preserved omission
file retains every original field and failure receipt, with a separate current
resolution pointing to closed light-cljs-157 and actual fresh verification.
Historical failure evidence is not erased or relabeled.

All eight exact new/restored ordinary and qualified workflows pass both checked
compilers. Exact reference receipt
`/tmp/light-cljs-125-protocol-final-reference/evidence.json`, SHA-256
`9b3ab185784a9cb11b2822924b0b4b0669d6b7ad433ae06066263c55f89ee741`.
Six core Rust tests pass with no failures or ignored cases in
`/tmp/light-cljs-125-protocol-core-local.log`. The initial authored method on
`js/this` was corrected to the proper standard `this-as` receiver form before
acceptance; its evaluation failure is not passing evidence. An initial Atom
ISwap probe fails both pins as well as locally; built-in Atom lacks that protocol
in canonical CLJS, so it is not a defect or passing workflow. Complete custom
ISwap implementation probes supply the actual positive contracts. The initial
reference projection selected only seven mapped cases because the fully
qualified collection protocol variant has no separate name obligation; its
subsequent exact eight-case reference invocation also executes that supplemental
variant and all eight pass. No source assertion was removed.

All **191 original library contexts** and **50 original SCI source contexts**
were actually reexecuted in both local engines and both pins. Their complete
previous records remain verbatim in history and all original expectations remain
unchanged. Library results: **163 original agreements, six exact reviewed walk
pin differences, 22 unresolved contracts**; fresh library evidence SHA-256
`8d02d8158f33c72823316fa7aa01208722666536445065a19baf9bfc9d9e5115` at
`/tmp/light-cljs-125-sci-library-context-191-a8e8a43/evidence.json`.
Source results: **35 original agreements, eight pin differences, seven unresolved
contracts**, with exact new execution and history documented in
[sci-source-context-evidence.md](sci-source-context-evidence.md).
The fourteen focused library tests, integrated ledger test and fresh replay pass.
These runtime receipts grant no unrelated macro-phase eligibility.

The repaired full renamed-core exists? context independently passes both pins
and all 12 complete source contexts pass both phases/engines with fresh and
reused macro hosts. Supported source accounting remains **18 full bodies and
47 original sites**, without claiming newly retained candidates as passes.
Two new candidates retain **six full bodies/eight sites**: the EDN test-reader
helper and complete three-site stream-read body, and Foo record setup plus all
five record-literal sites. The complete EDN body passes both pins but local
applications/macros reject its required four-argument read; focused ordinary
owner **light-cljs-159** now precedes 125. Original setup, source hashes and
raw exact receipts are retained in the proposals and
`../compiler-residual-completion.md`. The record-literal candidate preserves its
canonical CLJC companion and local UnknownTag result; it remains intended and
unwitnessed while its source-reader/companion scope is reconciled. No automatic
boundary exclusion, fake stream, read-string substitution or shortened body is
credited.

Full zero-missing acceptance remains unfinished. This continuation does not
rerun an unchanged incomplete comprehensive gate or count registration as a
semantic pass. Production files are unchanged; no new production performance
claim is made. The separate light-cljs-156 cancellation repair remains an external
limitation after automatic approval review rejected it twice citing possible
cybersecurity risk. No retry, delegation, native-crash investigation/repair or
cancellation/limits suite was performed in this continuation.

The fresh library audit independently compared three of the 22 unresolved
original SCI contracts to literal canonical observations. The unchanged complete
join-values source returns `"12"` for its nil separator locally, while both pins
return `"1null2"`; complete project sources retain relation metadata and the old
symbol key locally, while both pins return nil relation metadata and the
requested equivalent symbol key. Both application engines and actual macro
computation reproduce these differences without errors. They are pure promised
library operations, not an exact-number, host, serialization-domain or pin-version
boundary. Historical native projection documentation and original SCI expectations
remain explicit, unchanged contract conflicts; they do not authorize a blanket
canonical exclusion. Focused ordinary owners **light-cljs-160** (join) and
**light-cljs-161** (project metadata/key representative) immediately precede 125,
following the stream-read owner 159. No implementation of these owners was
performed here.

`../compiler-library-unresolved-probes.json` retains all three untouched original
sources, original SCI expected observations, separate literal canonical
expectations, exact original row/source/library/production hashes, and all four
fresh raw execution layers (Node/QuickJS applications and actual macros, Maven
release and clean checkout). Its SHA-256 is `c4375fee2fd931d79af949dd04c63c59885b9a8e6eba5ebc342098b09ad8c25b`.
Raw local layer `passed` means comparison to the original SCI expectation only;
the explicit canonical comparison is **mismatch**, and all three full source
contracts remain **unresolved**. The raw reference mismatch similarly refers to
the original SCI expectation; the independent canonical literals pass both pins.
No receipt replacement, expectation weakening or semantic ledger pass follows
from this artifact. All required reference identities and options were checked.

Final verification for this continuation: **122 Python harness tests pass**
(`python3 -B -m unittest discover -s tests/reference/compiler-language -p
'test*.py'`, 660.063 seconds; log
`/tmp/light-cljs-125-a8e8a43-python.log`). This includes the three new executing
call/qualification guards and the source/library/ledger receipt tests. The six
core Rust tests and one complete source-context Rust test also pass, with all
56 core workflows and 12 supported source contexts exercising both local
engines and actual macro computation. All eight new/restored exact workflows
pass both checked references; all 191 library and 50 SCI source contexts were
freshly reexecuted against both pins with the explicit unresolved counts above.
The core fixture generator reproduces 22 complete upstream bodies/56 workflows;
all 865 reviewed call receipts and 32 literal pinned oracles reproduce; lossless
inventory `--check`, scoped Rust formatting and `git diff --check` pass. No
comprehensive zero-missing gate pass, cancellation/limits test result, whole
repository regression result or new production performance result is claimed.


## Chunk/state and complete source evidence — October 11, 2026

Starting production `9964c3c79bd2cbb3acba57b79c977be37390126f` includes closed
stream/join/project owners 159–161. This continuation changes evidence/harness
only; all production files remain unchanged. Eight complete ordinary/qualified
literal workflows add chunk buffer mutation, constructors, backing storage,
finite traversal/reduction, metadata, saved public factories/calls, custom
chunked-next versus chunked-rest dispatch, exact empty-chunk error and ordered
effects, plus atom reference metadata mutation and volatile updates. All 56
previous workflow rows remain byte-identical. Current totals: **64 workflows
(22 original bodies/42 literals), 915 reviewed call receipts (+50, including 24
public arities), 39 mapped literal oracles (+7)**. The supplemental qualified
chunk factory variant executes normally even without a distinct new obligation.
All 8 exact new workflows pass Node/QuickJS applications, actual macro
computation and both identity/settings-checked pins. Exact fresh reference
receipt `/tmp/light-cljs-125-public-arity-pins/receipt.json`; six core Rust tests
pass. Three existing call/qualification guards, fixture reproduction and edited
Rust formatting pass.

Complete apply/binding/letfn contexts retain **11 bodies/all 67 original sites**
previously present only as historical flattened witnesses, including full setup,
both complete apply loops, every original assertion/error/eager message,
cross-namespace dependency and mutually recursive bindings. All reference/local
application/macro/fresh/reused paths pass. Three exact independently missing
resolved public rows anchor these complete sources: fixed `[f x y args]` apply,
resolved `[bindings & body]` binding macro and resolved `[fnspecs & body]` letfn
macro. Unresolved imported letfn is not assigned an arity. Historical witnesses
remain intact and no existing source assertion is duplicated. Initial catalog
integration correctly rejected the three unanchored fixtures; their exact
operation/phase bindings resolve that harness failure without fake source credit.
Final ordinary source accounting: **16 contexts/30 bodies/50 independently added
sites**; the Rust suite additionally executes one configured source-tag context,
**17 contexts/31 bodies/118 original sites**. Original EDN stream source remains
integrated and was reverified rather than counted as new work.

The source-tag context preserves its complete original assertion and exact
identity data-reader configuration. Both pins and explicit public compiler-reader
embedding pass all local phases/engines; it remains an embedding-source-case
pending a faithful hosted report adapter. Generic CLI compilation cannot install
that embedding capability and gets no implied source-site pass. The full Foo
setup/all five record-literal assertions remain intended/unwitnessed. Both pins
pass with the original JVM companion and reject the unchanged no-companion
control with ClassNotFoundException; local source reading rejects UnknownTag.
No blanket scope exclusion or fabricated record-tag handler is proposed. Exact
sources, commands, proofs and pending scope are in
`../compiler-residual-completion.md` and the source-context proposal.

The 50 original SCI source contexts were freshly executed after adding enforced
production/compiler/library/build and producer provenance. Drift during local
execution, missing/stale hashes and partial/reordered evidence cannot grant
coverage. A new atomic fresh writer preserves complete previous records and
unchanged original SCI expectations. All 18 focused source/receipt tests pass;
classification stays 35 original agreements, eight reviewed pin differences and
seven unresolved originals. Initial pre-writer legacy validations failed closed;
the fresh bound replay/current rerun passes. The earlier execution without this
new producer provenance is retained separately, never promoted by replacing
hashes.

All 191 original library sources were also actually reexecuted:180 local original
matches/11 mismatches,163 release matches/28 mismatches,169 checkout matches/22
mismatches. The prior active audit already had current production hashes; this
refresh preserves it verbatim and does not infer a new pass from its history.
Reconciliation stays 163 original agreements/six exact walk pin differences/22
unresolved originals. Three join/project original sources now match both
canonical pins while differing from original SCI expectations. Their fresh
canonical-fixed resolution and actual fresh/reused macro tests are separate in
`../compiler-library-unresolved-probes.json`; every earlier failure, original
source and expectation remains unchanged. The fresh source/library commands and
receipt hashes are recorded in their linked evidence notes. Runtime receipts
grant no unrelated macro eligibility.

During comprehensive gate preparation, repeated full phase-catalog revalidation
per local operation caused avoidable repeated hashing. `load_obligation_witnesses`
now validates one fresh phase snapshot per invocation; it never retains a cache
between calls. All 19 ledger tests pass, including current provenance/arity/subset
checks and the two-invocation snapshot regression. The bound manifest was
regenerated with unchanged source inventories/obligations/dispositions. Earlier
pre-regeneration manifest-drift checks failed closed and are retained as failures;
no acceptance was weakened or failed check counted as passing. Source/compiler
bytecode accidentally generated by a delegate was reconciled only for its exact
known owned files; unrelated `.DS_Store` is preserved.

The complete unfiltered invocation at checkpoint `501b363` finished with 6,059
covered obligations, 43 verified rejections, 31 verified warnings, 222 explicit
differences, 1,385 unrelated obligations and 2,251 missing obligations. The
missing obligations comprise 981 SCI cases, 660 public APIs, 546 local APIs,
40 original assertions, 22 syntax forms, one assertion-free test body and one
source program. Both required pins executed; all 40 registered actual macro-phase
tests and all 27 supplemental boundary checks passed. The strict gate exited 1
and remains incomplete. Raw observations and the complete report are retained at
`/tmp/light-cljs-125-full-9964c3c`; these are checkpoint results, preceding the
following adapter integrations, and do not establish their final acceptance.

The checkpoint's complete Python harness passed 126 tests in 294.009 seconds.
Full original acceptance remains in force. The separate 156 asynchronous
cancellation/native-crash repair remains externally blocked after automatic
approval review twice rejected it citing possible cybersecurity risk. It was not
retried, delegated, investigated or repaired; no cancellation/limits suites or
new production performance result are claimed in this continuation.

### Independent evidence integration after the complete checkpoint

The next integration adds 16 complete public-operation workflows to the earlier
64, bringing the retained core catalog to 80 workflows, 963 actual operation
receipts and 52 independent literal pin oracles. Twenty-nine additional public
arity rows have exact ordinary and actual MacroHost bindings. The new workflows
retain transducer initialization/completion and effects, metadata clone behavior,
and print callback identity/error/restoration observations. Six focused Rust tests
pass across both application engines and actual MacroHost execution; all 16
complete oracles freshly pass both checked pins. Earlier workflows are preserved
byte-for-byte. See `../compiler-core-public-arities.md`.

Four explicit hosted-source adapters execute exactly 12 additional original
assertions: configured identity data reading, six Closure import assertions,
three real Closure namespace assertions, and two unchanged REPL error-map
functions. They retain genuine source/setup and 32 fresh/reused application/macro
frames across QuickJS/Node plus both required pins. The integrated focused report
exits zero and covers exactly those 12 source assertions; its independently
required API rows still remain incomplete. It establishes neither generic CLI
embedding capability nor full Closure/REPL parity. All 20 helper/dispatch guards
pass. The source contract now retains 20 supported contexts/30 ordinary-phase
bodies/62 added original sites and the pending original record-tag context.
See `../compiler-residual-completion.md`.

Twenty-two precisely named original SCI library conflicts now have independent
canonical/strict-EDN literal contracts and 176 ordered actual application/macro
frames (fresh/reused, QuickJS/Node). Eleven original string/set cases agree
locally with both canonical pins while differing from SCI; eleven strict EDN
cases agree locally with their original SCI expectations while both canonical
pins return the preserved different behavior. The original 191-source receipt,
SCI expectations and unresolved reconciliation records remain unchanged.
A fresh sidecar must reproduce whole-source proof, current producer hashes,
both-pin literal contracts and actual macro execution before those exact cases
can be reported as explicit differences. Missing/stale/failed receipts cannot
supply credit or suppress failure. No blanket SCI exclusion is introduced.
See `library-dialect-evidence.md`; 32 focused ledger/reconciliation guards pass.

Seven complete syntax workflows add 21 source-bound ordinary/actual-phase
witnesses and seven literal pin oracles. All seven Rust tests pass, retaining
28 application/macro engine result frames, and all 14 required-pin outcomes
pass. Seven fail-closed syntax guards reject altered source/literals, detached
macro/reference adapters and nonexecuting quoted/discarded form text. The old
reader rest-binding test remains; its public `syntax::&` mapping now binds this
complete source oracle. The integrated candidates remain execution requirements,
never static declarations of passing coverage. See `../compiler-syntax-context.md`.

Raw `ns*` remains intended and unwitnessed. Both checked pins pass the unchanged
complete alias-update program, while local compilation rejects `ns*` at bytes
48..51. Focused live Pellet 162, **Support raw ns* alias directives in compiler
source**, owns that production fix ahead of 125. Its exact whole-source proof
is retained at `/tmp/light-cljs-125-syntax-nsstar-minimal/receipt.json`.
The six two-column SCI runtime capture metadata records retain all original
text/hashes as internal provenance; they contain no executable case/expectation.
Every executable SCI row retains its prior disposition.

The unfinished generative diagnostic is excluded from the verified catalog and
commit. The genuine real-provider Node controls pass both original 100000-trial
parse properties with each pin bundle. The actual hosted run completed normally
after one passing QuickJS application frame, then rejected a draft macro wrapper;
it receives zero source/phase credit. Exact source, patch, dependencies and
failure are preserved at
`/tmp/light-cljs-125-generative-audit-v1/handoff-final.json`; no owned verification
process remains running. A fresh original symbol/keyword namespace-context probe
also passes locally and both pins at
`/tmp/light-cljs-125-symbol-keyword-context-fresh/evidence.json`, but is not yet
registered and grants no coverage. Neither probe is a newly confirmed production
defect. Original missing obligations remain for a later independent continuation.

The complete pre-syntax integration Python harness passed 162 tests in 321.542
seconds. The final integrated Python harness passes **169 tests in 295.641 seconds**,
zero failures/errors, at
`/tmp/light-cljs-125-syntax-integrated-python-all.log`. Inventory/catalog
freshness, scoped Rust formatting, contract loaders and whitespace checks pass. The complete unfiltered checkpoint above
remains the last full strict result; no zero-missing or final acceptance claim
is made from focused integration counts. The 156 external limitation remains
unchanged and no cancellation/limits suite was run.

## Original source, API and declaration checkpoint — October 12, 2026

Production baseline is `d838fca9518d01877fab1d0d5e4fb7746bc288dd`. The raw
`ns*` discrepancies described in the preceding historical checkpoint were
repaired by 162/164; 163 also fixed faithful complete namespace scalar result
execution. Their original sources and current both-pin receipts remain required.
This checkpoint changes evidence adapters, catalogs and documentation only.

The unfiltered command was actually run against that production baseline:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -B tests/reference/compiler-language/report.py --run --strict --reference both --checkout research/clojurescript --timeout 300 --reference-timeout 600 --artifacts /tmp/light-cljs-125-full-d838fca
```

It completed the original ordinary/reference layers, 48 exact macro tests, SCI
expression contexts, all 50 source contexts and all 191 library contexts. The
complete residual body test passed at 163.37 seconds without shortening its
original bodies or increasing evaluator limits. The invocation then exited **2**:
`Adopted source-specific scope basis changed`. It produced **no final report**
and therefore no new comprehensive coverage count or strict acceptance result.
The closed raw-`ns*` documentation paragraph changed the document hash; review
confirmed the promised canonical string/set contract itself was unchanged.
Eleven exact scope-basis hashes were refreshed, retaining their previous hashes
and every original source, expectation, literal and disposition. No acceptance
criterion was relaxed. The current 22-library reconciliation then passed all
176 actual fresh/reused application/macro frames against those complete original
source layers at `/tmp/light-cljs-125-library-current-reconciliation-v1`.

Fresh 50-source receipts report 35 original agreements, eight pin differences
and seven unresolved cases; fresh 191-library receipts report 163 agreements,
six walk pin differences and 22 unresolved original expectations. The writers
preserve every complete previous record and every source binding. These are
whole-source receipts, not inferred public arity or actual-phase coverage.

The new source cohorts execute 121 complete original SCI API bodies with 968
ordered application/actual-macro engine frames, five complete receiver-demanding
IFn workflows, and the complete configured custom-reader module. Shared catalogs
add 99 ordinary links, 98 phase requirements, 36 exact oracles and one original
upstream assertion witness. The seven local transient phase links also have
their own current exact execution receipt at
`/tmp/light-cljs-125-transient-local-phase-current-v2/evidence.json`; selected
fixture reports do not implicitly execute assertion-linked supplemental phases.
The four public declaration signatures reuse existing complete demanded receiver
workflows; no undemanded declaration or metadata value receives credit.

The fresh **43-fixture filtered report exits zero** at
`/tmp/light-cljs-125-new-cohort-integrated-v2/report.json`. All 99 newly mapped
ordinary obligations are covered; 91 of the new phase links execute through 13
exact single Rust tests, all passing with zero ignored tests. The seven
assertion-linked transient phase requirements pass in the separate exact receipt
above. The full original residual-body phase test again passed without shortened
source. After documentation registration was clarified, the sole custom-reader
source assertion was freshly reexecuted in both phases/engines and both pins at
`/tmp/light-cljs-125-custom-final-producer-integrated-v2/report.json`, also exit
zero. Twenty affected reader/dispatch guards pass after the final documentation and
whitespace edits (48.045 seconds); the final source assertion proof binds those
exact current producer bytes.
The 43-fixture report retains 132 covered, 8463 missing and 1396 unrelated rows;
these are **filtered invocation counts**, not a comprehensive checkpoint. Both
reports explicitly retain incomplete strict status and unexecuted obligations.
The exact positive-cohort verification is preserved at
`/tmp/light-cljs-125-final-new-cohort-verification-v1.json`.

Exactly nine reviewed SCI contexts now enter the full report through a fresh
dispatcher. Four preserve original SCI agreements; five retain different SCI
expectations and require independent canonical literal results in both pins and
both phases. The three original Box/Empty records retain their explicit namespace
profiles and all six default-header TypeError controls. Current documented proof
is `/tmp/light-cljs-125-reviewed-nine-integrated-v2/evidence.json`. The real ledger
integration independently replays these nine and the 22 library proofs, showing
four covered originals and 27 explicit differences with 31 actual macro columns
at `/tmp/light-cljs-125-fresh-selected-context-ledger-v2.json`. This is selected
integration evidence, not a comprehensive report. Differing SCI expectations
keep zero original semantic pass credit. The adapter engine-list shape is kept
out of the older flattened-expression classifier; detached raw proofs or source
bindings remain rejected.

The final stable Python harness passes **260 tests in 270.377 seconds**, with no
failures, errors or skips, at
`/tmp/light-cljs-125-all-python-guards-current-v2.log`. Its actual recursive type,
negative-zero and output controls ran through compiler/QuickJS/Node and both pins
at `/tmp/light-cljs-125-final-recursive-api-controls-v2/evidence.json`. Inventory
freshness, exact source/catalog loaders and scoped Rust formatting pass. Earlier
integration attempts that encountered adapter shape or generator-hash drift are
retained as nonpassing diagnostics; they do not supply final report credit.

The unchanged original repeated record method returns literal **9** in both
required pins while ordinary compilation and four actual fresh/reused macro
controls reject its second same-arity implementation. The located rejection,
clean-after-failure controls and full original proof are retained at
`/tmp/light-cljs-125-record-duplicate-method-confirmed-gap.json`, SHA-256
`c56459058e24afd870a2455ab73afef4412b855bca5a3a8c23d970fbc668811d`.
After a fresh live duplicate check, focused **light-cljs-165**, **Accept repeated
record protocol method arities with canonical last-method dispatch**, was added
immediately before 125. This checkpoint implements no production repair and does
not exclude the original record obligation. Pellets remains the live status and
ordering authority.

The focused 165 production follow-up now selects the last repeated protocol
method/arity. Its unchanged original returns 9 on QuickJS/Node application
execution and both faithful fresh/reused actual macro adapters. Current exact
source/build/binary/raw evidence and both pins are retained in
[`sci-record-duplicate-method-receipt.json`](sci-record-duplicate-method-receipt.json)
and `/tmp/light-cljs-165-verified-v4`, receipt SHA-256
`8aaee6479f33194e6e0445041d35769179fac38856e6d361d4ff45c4a799b1a8`.
This resolves that focused production discrepancy; comprehensive reconciliation
and zero-missing language validation still belong to 125.
The independent split record-redefinition setup still demands old records from
the current generation during macro computation; the source without repeated
methods has the identical discrepancy. Both pins and entire sequential macro
execution preserve `[3 17 15 14]`, while split macro setup yields `[13 17 15 14]`.
No split-generation passing frame or credit is supplied. Fresh duplicate checks
created focused **light-cljs-166, Preserve source-visible record generations in
demanded macro initializers**, immediately before125. Full retained evidence is
`/tmp/light-cljs-165-generation-gap-v2` and
`/tmp/light-cljs-165-verified-v3/audit`; no166 implementation is performed here.

The distinct original returned record closure remains unresolved under its
located source-root/macro boundary; the diagnostic does not prove a newly
required production gap. The full generative source remains unregistered and
nonpassing in phase: both original 100000-trial properties pass each genuine
provider pin bundle, but the corrected actual macro audit hit its normal
deadline. Its unchanged source/driver/dependencies and outcome are preserved at
`/tmp/light-cljs-125-generative-audit-v3/handoff.json`. No property options, loops,
expectations or evaluator limits were changed and no deadline was treated as a
pass. Five exact native numeric API rows retain historical bytes/rejections and
the user's explicit retired numeric-tower basis; other native/Number predicates,
mixed SCI bodies and JS BigInt interop are unaffected.

125 remains incomplete. The separate 156 cancellation repair was twice rejected
by automatic safety review for possible cybersecurity risk. Its preserved patch
was not retried, delegated, investigated or repaired; no cancellation/limits
suite was run. Remaining full-gate obligations, timeouts and source differences
cannot be converted into completion by this checkpoint.
