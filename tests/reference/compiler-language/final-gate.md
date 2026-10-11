# Final intended-language gate — 2026-10-11

**Language completion remains incomplete.** This receipt checks the comprehensive
intended-language scope at implementation HEAD `9de6468`, after the runtime
reader and mapped core Var metadata fixes. Evaluator migration and family-level
implementation receipts do not establish full language completion. The live
completion owner is `light-cljs-125`; its executable acceptance is unchanged.

## Independent scope and evidence

The reproducible pinned inventory retains 97 complete runtime/reference files,
1,328 bodies, 4,728 assertion sites and 1,327 intended public declaration/arity
rows. It also retains syntax, local API, historical SCI and assertion-free
body/module obligations. Every original assertion, setup/effect path and source
hash remains in the ledger. `extract_inventory.py --check` passed; that result is
source freshness, not semantics.

The current catalog has 222 declaration/local obligation mappings and 262 phase
mappings. Independently intended obligations without ordinary mappings include
1,187 public declaration rows, 21 syntax rows, 927 local API rows, all 2,395 SCI
rows, ten assertion-free bodies and three source programs. Those missing mappings
are evidence gaps, not automatically implementation defects. Their reconciliation
remains in the full gate; closed family Pellets do not manufacture passes.

## Confirmed public implementation omissions

Focused audits executed the same reviewed probes against the canonical local
compiler/evaluator and both reference identities: Maven ClojureScript `1.12.134`
and clean checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`. The reference
harness verified `:optimizations :none`, `:static-fns false` and
`:def-emits-var false`. Duplicate searches found no focused live owners before
creating these independently actionable records.

| Contract | Local observation | Both pinned observations | Live owner |
| --- | --- | --- | --- |
| `Inst` custom millisecond dispatch | Unresolved `Inst` | `[true 42]` | light-cljs-138 |
| `IDerefWithTimeout` direct method | Unresolved protocol | `[true [100 :timeout]]` | light-cljs-138 |
| Built-in/custom `IUUID` membership and predicate | Unresolved protocol | `[true true true]` | light-cljs-138 |
| `ASeq` membership of an array sequence | Unresolved protocol | `true` | light-cljs-138 |
| Public `Cons` construction | Maximum call stack size exceeded | First item `1` | light-cljs-139 |
| Public `Reduced` construction | Does not support deref | Dereferenced value `42` | light-cljs-139 |
| Public `Symbol` construction | Does not support name | Name `"foo"` | light-cljs-139 |
| `->Cons`, `->Reduced`, `->Symbol` factories | Unresolved factory names | `[1 42 "foo"]` | light-cljs-139 |
| Var source filename versus user `:file "fake"` | Fake filename retained | Compiler source filename wins | light-cljs-140 |

The protocol scope requires the direct `-deref-with-timeout` method. Pinned
ClojureScript `deref` has one arity; an initial three-argument probe was invalid
and supplies no defect evidence. Public constructors are intended source-visible
contracts, even though the implementation uses private payload storage. These
omissions cannot be reclassified as private storage or dialect exclusions.

Exact programs, source locations, commands and acceptance live in the three
Pellet descriptions. Disposable source/invocation/output receipts are in
`/var/folders/hm/d7rr9ds96qx995ns72ry9g4m0000gn/T/light-cljs-125-public-audit-0izt8k28/`
and `/tmp/light-cljs-125-sci-{public-audit,residual-reference,value-domain-reference}`.
They are observations of these specific contracts, not full family passes.

## SCI reconciliation and result-domain boundaries

A focused thirteen-row ignored audit executes and captures historical sources;
its passing test means observation capture succeeded. Independently pinned
comparisons distinguish these canonical CLJS observations from SCI conventions:

| Historical SCI lead | Canonical CLJS observation confirmed with both pins |
| --- | --- |
| Record method recur / captured method closure | `3` / `[2 nil true]` |
| Extension overload replacement / method recur | `[2 3]` / `2` |
| Excess arguments to a fixed function | Returns `1`, retaining `[:callee 1 2]` effects |
| Repeated Var expressions and root reference | `[false 2 2]` |
| `select-keys` representative key | Query-key metadata survives |
| Map/set constants containing nested list in `case` | Type errors |
| Volatile metadata mutation | Same returned values as the canonical evaluator |
| Protocol method metadata `:protocol` comparison | `false`; documentation and arglists agree |

The three `sci-into-xf-bad-*` original operations produce values outside the
finite export bridge. Both pinned bridges report `unsupported outcome object`.
A wrapper around each complete original operation returns `[false false true]`
for nil?, vector? and ITransientCollection membership under both pins and
locally. This is a transient-result export boundary, not proof of a missing
intermediate transducer operation or canonical arity error.

The preserved `load.shared/value` setup selects `#?(:clj 40 :cljs 0)`, so its
canonical value `0` is not a loader omission. Historical `clojure.core/ns-resolve`
has no public runtime/macro declaration in the pinned CLJS core sources;
unresolved spelling alone cannot establish a canonical omission. Built-in macro
metadata mutation still needs source-contract/export reconciliation. These are
focused findings; the 312 current SCI gap rows owned by the full gate remain
nonpassing until their individual contracts are reconciled. Original SCI
observations and dispositions are preserved by this receipt.

## Explicit outside scope

The comprehensive intended **language** scope retains pure transients,
transducers, sequence/eduction, sorted collections, multimethods, public core
protocols/constructors, and the promised string/set/walk/EDN/reader facilities.
It does not claim compatibility with the entire upstream ecosystem.

| Outside boundary | Adopted scope basis |
| --- | --- |
| Shadow/Closure build systems, bundling, chunking, hot reload, assets, WASM targets, dependency acquisition and unchanged application ports | Explicit prior compiler milestone exclusions; application ports are not language acceptance substitutes |
| Standalone spec, pprint, test harness, math, reducers, data/datafy and browser libraries | Not among the promised standard library namespaces; core language operations used by them remain intended |
| Newer async/await and proxy libraries | Outside the adopted synchronous language milestone; ECMAScript promises/proxies are not inherently forbidden |
| Browser/Node/Bun/npm/filesystem/network/clock/scheduler facilities | Require explicit embedding capabilities; pure operations remain language obligations |
| Upstream self-hosted compiler, exact analyzer AST/cache, generated JS/optimizer spelling, private trie/node storage and optional lite collection profile | Explicit compiler/build/private representation boundaries; public constructor/protocol contracts stay intended |
| JVM/GraalVM execution, `.clj` macro companions, native exact integer/ratio/BigDecimal tower and retired native ownership/GC descriptor APIs | Explicit user-adopted JavaScript dialect and evaluator migration; JS BigInt interop is not a promised Clojure numeric tower |

Macro/source/import/phase differences require their local executing assertions.
Warnings, located source rejections, version differences, missing tools, skips
and timeouts remain separate from execution passes. QuickJS availability, parser
acceptance and metadata generation cannot satisfy these contracts.

## Executed gate and verification receipt

```sh
python3 -B tests/reference/compiler-language/report.py --run --strict \
  --reference both --checkout research/clojurescript \
  --artifacts /tmp/light-cljs-125-full-execution
```

This unfiltered full-scope invocation exited **1**, correctly retaining
`strict_complete: false` and `full_scope_selected: true`. It executed all 877
registered local witness cases, both pinned reference paths, all 27 supplemental
boundary contracts, and 19 exact named phase tests covering 262 mapped
obligations. All 262 mapped phase observations passed; this does not imply macro
coverage for the remaining 9,685 ledger rows.

| Full ledger result | Rows |
| --- | ---: |
| Covered | 3,252 |
| Verified source rejection | 13 |
| Verified warning | 31 |
| Missing | 5,073 |
| Explicit version difference | 201 |
| Explicit unrelated/internal/dialect/host disposition | 1,377 |

Missing rows comprise 530 source assertions, 1,187 public declaration rows,
21 syntax rows, 927 local API rows, 2,395 SCI rows, ten assertion-free bodies and
three source programs. This is executed incomplete evidence, not the earlier
static report's zero-pass baseline. The historical selected scope is also
incomplete in this invocation: 2,277 covered, 13 rejections, 31 warnings,
48 missing, 166 version differences and 702 unrelated rows.

The remaining registered-witness failures include four diagnostic cases covering
30 source sites (`spec-validation`, `test-defn-error`, `test-cljs-1346`, and
`test-cljs-2037`) and five namespace macro-import programs rejected by the
explicit `.clj`/`:require-macros` retirement. Exact current/expected diagnostics
and the original upstream programs remain in the report. Reconcile their local
phase/source adapters under light-cljs-125; neither generic source rejection nor
changing expected text alone establishes the original contract. These are
separate from the demonstrated public implementation omissions above.

The machine report and every command/input/output are retained under
`/tmp/light-cljs-125-full-execution/`; the complete summary log is
`/tmp/light-cljs-125-full-execution.log`. The supplemental boundary outcomes are
14 warning APIs, five public runtime contracts, four primitive permissiveness
contracts, two intentional differences, one reader API and one version
difference. They are verified boundary observations, not interchangeable
ordinary semantic passes.

Additional verification:

- `cargo test --locked`: exited 101 at the obsolete analyzer namespace/no-runtime
  expectation. `cargo test --locked --no-fail-fast` then traversed all 68 target
  summaries, with 590 passes, two failures and 62 explicitly ignored checks.
  Both failures concerned old namespace programs avoiding runtime initialization;
  reader namespace state now requires that initialization. Updated integration
  checks retain compact source-free primitive output, single runtime inclusion,
  `.cljs`/`.cljc` selection, API/CLI agreement, and actual Node result `3`.
  Full `cargo test --locked --test analyzer` and `--test namespaces` reruns
  passed all 36 and 28 tests respectively. Every other target in the full
  traversal passed. The 62 ignored oracle/measurement/audit checks were not
  silently treated as passes or all explicitly rerun.
- `cargo check --locked --all-features --all-targets`: exit 0.
- `python3 -B tests/reference/compiler-language/extract_inventory.py --check`:
  exit 0, including a repeat after the integration-test changes.
- `clojure -Srepro -Sdeps '{:deps {org.clojure/clojurescript {:local/root
  "research/clojurescript"}}}' -M tests/reference/generate_core_var_metadata.clj
  --check`: exit 0.
- `python3 -B -m unittest discover -s tests/reference/compiler-language -p
  'test_*.py'`: all 73 tests passed. These validate accounting/runner failure
  behavior; their success does not fill semantic evidence rows.
- `node tests/support/js_runtime.test.js` and
  `node tests/support/js_core_helpers.test.js`: exit 0.
- The focused thirteen-row historical SCI audit passed observation capture;
  both-pin projections and exact observation boundaries are described above.
- `cargo bench --locked --bench evaluator -- --iterations 20`: exit 0,
  validating fresh/reused sessions and sticky bounded-loop/reset recovery.
  Total times were 324,651,375 ns fresh, 2,255,875 ns reused and 1,366,722,542 ns
  bounded recovery for twenty iterations each. This normal validation run is not
  an interleaved performance comparison or a claimed throughput improvement.
- `git diff --check`: passed. `rustfmt --edition 2024 --check
  tests/analyzer.rs` still reports existing formatting drift at the untouched
  binding/protocol assertions; it does not justify unrelated
  formatting changes.

Full intended-language acceptance is not met. Missing source/public/local/SCI
mappings, all required reference/body/phase reconciliation, the 62 ignored checks
where required, and broader upstream/runtime/performance acceptance remain
unfinished under the live gate. Safe sequencing is to finish the public protocol,
constructor and Var provenance implementations, then resume the full gate and
reconcile all remaining evidence. No ordinary application port, source parser
pass or evaluator migration receipt substitutes for that work.
