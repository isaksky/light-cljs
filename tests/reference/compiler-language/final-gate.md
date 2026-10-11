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
