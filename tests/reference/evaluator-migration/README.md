# Evaluator migration execution gate

This gate verifies the settled compiler-emitted JavaScript evaluator contract.
The Rust interpreter and its native adapter are physically absent. The gate first
checks that their source, support adapter, test targets and benchmarks have not
returned, then checks all targets with all features. Historical native results
cannot contribute to canonical coverage. Whole-language completion remains a
separate reference-backed requirement.

Run from the repository root:

```sh
cargo test --locked
python3 -B tests/reference/evaluator-migration/gate.py --artifacts /tmp/light-cljs-evaluator-gate
python3 -B tests/reference/evaluator-migration/gate.py --reference --checkout research/clojurescript --artifacts /tmp/light-cljs-evaluator-reference
```

Required tools are Rust/Cargo, Python 3, and Node. Focused upstream comparisons
add Git, Clojure CLI and Java, Maven ClojureScript **1.12.134**, and the source
checkout at **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. Tools are required;
missing tools, rejected required programs, failed tests or interrupted commands
cannot pass. Each command's exact arguments, exit status and full diagnostics
are retained in the requested artifact directory. The first failure stops the
gate and retains prior results. Ignored oracle/performance tests in ordinary
Cargo invocations remain explicitly ignored; `--reference` executes only the
three named pinned comparisons, never all ignored tests implicitly.

The gate discovers every `compiler_*` and `eval*` integration target, including
newly added suites, plus analyzer/boundary/CLI/execution/namespace/provenance and
SCI corpus checks. There are no native adapter exclusions. Selected
compiler/bootstrap/application/evaluator unit modules also execute, including
cold source regeneration, demand/bundle cancellation and clean reuse.

## Required case ownership and execution evidence

The table maps each mandatory case family to its implementation owner and
executing replacement. It does not infer passing from a closed Pellet.

| Case family | Owner | Executing suite or module |
| --- | --- | --- |
| Exact factor/connection `[42 42]`, hits `[1 2 0]`, fresh/reused hosts and standalone mutation `[1 2 1]` | 102, 114 | `evaluator_migration::acceptance_programs_execute_in_independent_heaps_with_fresh_and_reused_hosts` executes literals and host effect counts/order in independent QuickJS and Node heaps; `ordinary_helper_demand` checks initializer ordering |
| Exact tools/app `[42 42 42]`, ordinary imports, `.cljs` precedence, `:cljs` conditional/splicing and companion rejection | 103, 114 | `compiler_macros::ordinary_dual_use_require_executes_exact_tools_example_through_api_and_cli` repeats the exact source with fresh/reused hosts in QuickJS and Node and compiles through the CLI; ordinary import tests, `eval_sessions::namespaces_sources_and_reader_aliases_are_incremental` |
| Core cold bootstrap, compiled control/iteration/interop macros | 108 | `compiler_bootstrap`; `compiler_bootstrap::tests::regenerate_from_first_party_sources_and_collect_during_bootstrap` |
| Number edges, equality/keys, UTF-16/forms/metadata/&env/syntax quote/provenance, quoted #js descriptors, rejection paths | 100, 107, 128 | `compiler_macros::javascript_form_bridge`, `compiler_reader`, `evaluator`, `evaluator_migration::quoted_js_macro_literals_retain_explicit_descriptors`, boundary and provenance suites |
| Source/nested/generated declarations, binding-kind/phase errors, local shadowing | 101 | `compiler_macros::same_file_javascript_macros`, migrated older source fixtures and phase-error import tests |
| Reentrant helper compilation, recursive calls versus analysis cycles, frozen namespaces/frontiers/generations | 109 | `compiler_macros::reentrant_deferred_units` |
| Closures/apply/higher-order/functions in collections, untaken host branches, root cycles/failures/redefs/defonce/bindings | 102 | `compiler_macros::ordinary_helper_demand`; state, functions, binding and lazy SCI evaluator dispositions |
| Protocol/type/record/extend-type/extend-protocol bundles, marker/native/default selectors, computed targets, reify/specify! | 110 | `compiler_macros::declaration_bundle_demand`, frontier/failed-extension tests, protocol/type/record/extension/reify suites, bootstrap bundle rollback tests |
| Compile-only root elimination, shared runtime helpers, required effects/order/throws, optimizer equivalence | 111 | `compiler_optimization`, `application::tests`, namespace and macro suites |
| Lifetime/foreign handles/cycles/callback captures, sticky cancellation/limits and recovery | 106, 112 | `evaluator`, `eval_sessions`, bootstrap cancellation tests; native GC/internal counters have these replacement lifetime/limit regressions before retirement |
| Embedding forms/analyze/evaluate/invoke/eval/macroexpand, loading/state/IO/libraries/transducers | 112, 118, 123 | `eval_sessions`, `evaluator_conformance`, library/reader/transducer suites |
| Script/text/stdin/REPL/history/errors/reset/explicit controls | 113 | `eval_cli`, `eval_examples`, `cli`; all use the canonical evaluator |
| Canonical operations in QuickJS/Node and direct runtime assertions | 106, 114 | `evaluator`, `compiler_core_helpers` (bounded applied partial/juxt constructors in both phases), direct `js_runtime.test.js` and `js_core_helpers.test.js` |

## Historical expectations and remaining language work

[SCI evaluator dispositions](../sci/evaluator-dispositions.tsv) account for every
historical SCI row and preserve the original source/observation and provenance.
Portable and adapted rows execute through `EvalSession`; gap and retired rows
remain nonpassing, with explicit reasons. The original SCI oracle observations
are never overwritten. A complete accounting test is not a claim that every row
has CLJS parity. The [compiler-language report](../compiler-language/README.md#evaluator-migration-accounting)
also distinguishes its historical `native` column from current `evaluation`;
lexer/parser acceptance cannot satisfy semantic reference layers.

Intentional changes include JavaScript Number (rounded wide integers, fractions
and infinity), `.cljs`/`.cljc` sources with `:cljs` selection, source-only macros
without runtime callable/Var values, ECMAScript during expansion, fresh mutable
roots on clean compilation and compiler-owned binding kinds. Native shared-owner
forks, heap/collector counters, class descriptors, JVM types and resource-unit
contracts are retired as documented in the [evaluation API](../../../docs/evaluation.md).
Remaining intended-language gaps stay in the live Pellets queue and the broader
language ledger; this gate does not close light-cljs-125 or erase missing rows.

## Executed verification

Exact local results, ignored checks, benchmark observations and any live blockers
are recorded in [verification.md](verification.md). Queue closure alone does not
authorize deleting unsupported behavior or changing reference observations.
