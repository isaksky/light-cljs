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
