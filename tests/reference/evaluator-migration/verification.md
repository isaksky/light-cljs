# Historical pre-removal migration gate verification — October 10, 2026

This file preserves pre-removal evidence; it is not a post-removal validation report.

**The light-cljs-114 execution gate is complete.** The final default-stack full
Cargo run and both native-disabled gate modes passed after the four prerequisite
repairs and the retained lazy-constructor regression described below. This is
local evidence for the named migration contract. Physical native removal and
post-removal builds remain light-cljs-115; full intended-language completion
remains light-cljs-125. Queue closure cannot turn the explicit language gaps
into passing semantics. Earlier incomplete attempts are retained below.

Initial starting Git commit: `0413d551bce3493f761be15624a5e3dbea3fd580`.
Tools: macOS 15.7.3 ARM64; Rust/Cargo 1.99.0; Node v24.18.0; Python 3.14.6;
Clojure CLI 1.12.4.1618; Java 25.0.2. Required CLJS references are Maven 1.12.134
and checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`; the checkout reports
ClojureScript `0.0.135109660` and Clojure 1.12.4. These are local executions,
not GitHub Actions or cross-platform results.

## Initial focused checks (before the final rerun)

All Cargo commands below use `--locked`; compiler/evaluation commands also have
`LIGHT_CLJS_DISABLE_NATIVE_EVAL=1`, unless explicitly marked historical.

| Command/scope | Result |
| --- | --- |
| `cargo check`; `cargo check --all-features` | Both passed |
| `cargo test --test compiler_macros` | 99 passed, 0 ignored; migrated source namespaces and one new ABI/syntax immutability counterpart |
| Final `cargo test --test compiler_macros ordinary_dual_use_require_executes_exact_tools_example_through_api_and_cli -- --exact --nocapture` | 1 passed, 98 filtered; exact third program repeats with fresh/reused hosts in QuickJS and Node, plus CLI compilation |
| Historical `cargo test --test compiler_native_adapter` without fence | 3 passed; native-only ABI/stats/namespace-object contracts, excluded from gate |
| `cargo test --test analyzer --test compiler_reader` | 35 analyzer passed; final reader rerun 7 passed after replacing native readers/counters with compiled functions and fresh compilation heaps |
| `cargo test --test compiler_destructuring_completion` | 9 passed; complete upstream bodies retained verbatim, first-party assertion macros execute compiled JavaScript |
| `cargo test --test evaluator_migration` before descriptor witness | 3 passed; two acceptance programs, standalone mutation, fresh/reused hosts, host effects and native-fence subprocess |
| `cargo test --lib compiler_bootstrap::tests` | 4 passed; cold regeneration/collection, root and bundle cancellation/recovery |
| `cargo test --lib application::tests` | 2 passed |
| `cargo test --lib eval_compiler::tests` | 4 passed |
| `cargo test --lib active_tests` | 4 passed |
| `cargo test --lib compiler_pure_tests` | 1 passed |
| `cargo test --lib compiler_default_reader_tests` | 1 passed |
| `cargo test --lib program_fragment::tests` | 3 passed |
| `node tests/support/js_runtime.test.js` | Passed all assertion groups |
| `node tests/support/js_core_helpers.test.js` | Passed |
| `python3 -B -m unittest discover -s tests/reference/compiler-language -p 'test_*.py'` | 58 passed, including separate historical native/current evaluation accounting |
| `python3 -B -m unittest discover -s tests/reference/sci -p test_regenerate.py` | 6 passed, 1 skipped: explicit `LIGHT_CLJS_SCI_CHECKOUT` transport job not selected |
| `python3 -B tests/reference/compiler-language/extract_inventory.py --check` | Exact pinned source/hash inventory verified: 3,237 assertions, 696 bodies; counts remain 2,535 missing / 702 unrelated, not semantic passes |
| `git diff --check` | Passed |
| Final `rustc --test` evaluator-conformance target against Cargo-built dependencies, native disabled | 2 passed, 0 failed, 2 ignored in 141.70s; all 2,475 dispositions executed. Exact compile/invocation commands are in [SCI accounting](../sci/evaluator-accounting.md#local-verification-2026-10-10). This is not a fresh Cargo/full-gate run |

## Initial pinned comparisons actually executed

With `LIGHT_CLJS_REFERENCE=both`, explicit checkout path and full revision,
`LIGHT_CLJS_DISABLE_NATIVE_EVAL=1`:

- `cargo test --test execution compiled_helpers_match_pinned_values_effects_and_stdout -- --ignored --exact --nocapture`: 1 passed; **58** outcomes, ordered effects and stdout match each reference.
- `cargo test --test execution compiled_sequences_match_pinned_values_effects_and_stdout -- --ignored --exact --nocapture`: 1 passed; **56** outcomes, effects and stdout match each reference.
- `cargo test --test compiler_transducers eager_transducer_fixtures_match_pinned_release_and_checkout -- --ignored --exact --nocapture`: 1 passed; **316** eager transducer observations match each reference.

The total is **430 cases per reference**. Other ignored pinned oracle tests were
not rerun and are not passes. Local phase/macro dialect assertions do not require
an upstream compiler to accept source-only `.cljs` macros. Historical SCI golden
rows were preserved; a new JVM regeneration of every SCI row was not run locally.

## Initial acceptance failures (resolved before the final rerun)

- **light-cljs-126:** `RUST_MIN_STACK=8388608 cargo test --test execution`
  produced **52 passed, 1 failed, 43 ignored**. The sole failure is
  `tiny_programs_execute_with_expected_values_and_effects`, specifically
  `state-volatile-order`: actual `[8 8 10]` with one target effect versus expected
  `[17 17 10]` with two target effects. The stack override is diagnostic evidence,
  not a fix and not the normal gate environment.
- **light-cljs-127:** `cargo test --test provenance -- --nocapture` produced
  **1 passed / 1 failed**. `numeric-number-keys` remains labeled hand-authored
  while claiming an upstream source; the validator was preserved.
- **light-cljs-128, resolved:** quoted `#js` output now retains explicit compiler
  descriptors, including nested syntax-quoted literals and metadata/provenance.
  `LIGHT_CLJS_DISABLE_NATIVE_EVAL=1 cargo test --locked --test evaluator_migration quoted_js_macro_literals_retain_explicit_descriptors -- --exact`
  passes the required `(defmacro m [] '(let [out #js []] out))` witness in both
  engines. The compiler macro, compiler reader and evaluator suites pass 119
  tests (one startup measurement remains intentionally ignored), including
  arbitrary host result rejection and bounded nested descriptor export.
- **light-cljs-129:** `cargo test --test execution deeply_nested_accepted_source_compiles_and_executes -- --exact --nocapture`
  aborted with a Rust default-test-stack overflow. The accepted source was not
  rejected or skipped to make the gate pass.

## Initial broad command status (incomplete historical attempts)

`cargo test --locked --no-fail-fast` and the normal native-disabled gate were
attempted at the default test stack. Both reached the required descriptor witness
and then the execution target's fatal stack overflow. On this macOS host the
aborted test children remained in an uninterruptible exiting state, with no CPU
activity, after printing the fatal error. Their Cargo parents were terminated;
the full command returned 143, and the gate returned 1 with consumer status -15.
These interrupted commands are **not complete or passing Cargo/gate runs**.
An earlier accounting harness in those runs also exposed a source-adaptation
classification bug; that harness/manifest was repaired before the final reviewed
standalone corpus check. The historical oracle expectations were unchanged.

The diagnostic gate used `RUST_MIN_STACK=8388608` solely to collect the remaining
failure inventory. Its consumer command completed all **37 targets** with
**421 passed, 4 failed, 49 ignored**, exit 101; the driver returned 1. Failures
were `evaluator_migration`, `execution`, `provenance` and the then-current SCI
harness's overly small poll budget. The SCI harness was subsequently repaired
without changing any matching row's observation, and its complete standalone
check passed as recorded above. The known deeply nested source passed only with
the diagnostic stack override. This does not satisfy default-stack acceptance
or fix light-cljs-129. Logs/receipts remain under
`/tmp/light-cljs-114-gate-diagnostic`; default-stack evidence is under
`/tmp/light-cljs-114-gate-final` and `/tmp/light-cljs-114-cargo-final.log`.
The gate stops at consumer failure, so later unit/Node/accounting/reference
stages did not execute through this driver; their independently executed
results appear above. The required normal gate must be rerun after the fixes.

A whole-corpus accounting pass includes explicit gap/retired observations and
cannot turn them into supported CLJS behavior. The final reviewed ledger records
1,484 matching rows (1,180 unchanged-source portable and 304 documented
adaptations), 906 gaps and 85 retirements. Of the gaps, 305 concern diagnostic
transport and 601 require language/dialect review. The live owners govern those
gaps; recording them does not automatically satisfy migration acceptance.

## Initial performance measurement

The existing `--application-only` benchmark executed with the fence and nine
samples: **270 measured rows**, all complete values checked in Node. The
[benchmark record](../../../benchmarks/compiler.md#evaluator-migration-gate--october-10-2026)
retains raw samples and output sizes. Concurrent compiler builds make timing
comparisons noisy; no reliable startup regression/speedup or native-GC claim is
made. CI configuration was updated but no remote Actions run was started.

## Final resumed verification

Resumed from `4e31ce8a87d038b02041c020144ec104b55fa34b`, incorporating
`9e75092` (volatile target order), `766198f` (Number-key provenance),
`0be8eaa` (explicit quoted #js descriptors) and `4e31ce8` (analyzer stack).
All commands below ran with `RUST_MIN_STACK` unset. Gate children carry
`LIGHT_CLJS_DISABLE_NATIVE_EVAL=1`; the full Cargo command permits its explicitly
historical native compatibility tests. No native fallback was enabled in a gate.

| Exact command | Final result and retained receipt |
| --- | --- |
| `env -u RUST_MIN_STACK -u LIGHT_CLJS_DISABLE_NATIVE_EVAL cargo test --locked` | Exit 0; **912 passed, 0 failed, 52 ignored**, 84 harnesses; `/tmp/light-cljs-114-resume-cargo-final.log` |
| `env -u RUST_MIN_STACK python3 -B tests/reference/evaluator-migration/gate.py --artifacts /tmp/light-cljs-114-resume-gate` | Exit 0; **15 stages**; `/tmp/light-cljs-114-resume-gate/results.json` |
| `env -u RUST_MIN_STACK python3 -B tests/reference/evaluator-migration/gate.py --reference --checkout research/clojurescript --artifacts /tmp/light-cljs-114-resume-reference` | Exit 0; **19 stages**; `/tmp/light-cljs-114-resume-reference/results.json` |
| `rustfmt --edition 2024 --check tests/compiler_core_helpers.rs tests/interpreter_coverage.rs`; `git diff --check` | Passed |

Each gate completed all **37 consumer targets: 432 passed, 0 failed,
49 ignored**, then **19 unit tests** across seven modules, both direct Node
scripts, **58 compiler-language Python tests**, and **6 SCI launcher tests**
with one explicit skip (the separate `LIGHT_CLJS_SCI_CHECKOUT` transport job
was not selected). Both default/all-feature locked builds passed. Cold bootstrap,
all three acceptance programs, standalone mutation, fresh/reused host checks,
form/descriptors, declaration bundles, cancellation recovery, phase root/effect
contracts, scripts, REPL, embedding and the accepted nested source all execute
with the native fence. The new applied constructor witnesses execute literal
values/effects in both phases and both engines.

Reference mode independently verified the exact pinned inventory (**3,237
assertions, 696 bodies; 2,535 missing / 702 unrelated**) and reran **58 helper,
56 sequence and 316 eager transducer cases per reference**, including ordered
effects/stdout. That is **430 cases per reference**, against Maven 1.12.134 and
checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`. Other ignored oracle and
startup tests were not executed and are not passes. No remote Actions run,
cross-platform verification, whole-language strict pass or fresh JVM regeneration
of every historical SCI row is claimed. The local gate runs the commands wired
into the migration CI stages; broader intended-language CI remains separate.

### Retained behavior and evidence repairs

The first resumed full Cargo attempt passed the migration suites but exited 101
at `interpreter_coverage`, because four historical REPL history rows still pointed
to removed `tests/native_cli.rs`. The inventory now identifies the actual
compiler-backed history and error-recovery tests as `compiled-replacement`
evidence. The focused inventory check passed **2 tests**, and the final full run
passed it too. The failed attempt is retained in
`/tmp/light-cljs-114-resume-cargo.log`.

A scope audit also identified the retained native/CLJS finite-demand promise for
applied `partial` and `juxt`. Their constructors had lacked the canonical
variadic sequence ABI, causing eager realization of the supplied tail. They now
capture that tail, while preserving fixed arities, callback order and repeated
finite capture reuse. Direct Node checks and three native-disabled
`compiler_core_helpers` tests passed before the full gates. The exact infinite
partial witness now returns true after effects **0..4** using the default budget;
its special cancellation substitution was removed. Details are in
[the apply audit](../compiler-apply.md#deferred-constructor-migration-verification-2026-10-10).

The reviewed eighteen-source constructor audit and both complete gate corpus
checks preserve all **2,475 original SCI rows**. The two repaired volatile rows
and two recovered constructor rows are unchanged-source matches. Two delayed-tail
failure rows now preserve the original completed effects; their unmatched
diagnostic category transport stays nonpassing. Current accounting is
**1,488 matches (1,184 portable / 304 adapted), 902 gaps and 85 retirements**.
The gaps retain **305 diagnostic transport / 597 language-or-dialect review**
rows with live owners. Their existence does not waive a concrete retained
migration regression; the constructor regression above was resolved here rather
than deferred through owner assignment. Neither these counts nor this gate
authorize silently deleting intended behavior or closing the broader ledger.

### Final application performance check

`LIGHT_CLJS_DISABLE_NATIVE_EVAL=1 cargo bench --locked --bench compiler --
--application-only --samples 9 --iterations 1 --dump /tmp/light-cljs-114-resume-performance`
exited 0, recording **270 samples** after the final gates. Every complete off/on
result was checked. The runtime ABI repair adds 1,165 emitted bytes to
runtime-bearing applications; primitive sizes are unchanged and pure macro
helper elimination remains 4,176 bytes. The
[benchmark record](../../../benchmarks/compiler.md#final-resumed-application-measurement)
retains raw samples and medians. No native-GC, allocation, persistent QuickJS or
reliable before/after timing claim is made.


## Physical-removal verification — October 11, 2026

Starting commit: `b0a95e9`. The native interpreter, analyzed executable units,
stack machine, exact-number runtime, heap/GC/fork machinery, eager namespace
loader, native macro callbacks and form adapter are physically removed. Compiler
forms, source/provenance, syntax quote, metadata registries and pure declaration
transforms remain compiler-owned. `MacroHost` has no native context or fallback;
the obsolete analyzer branch and native evaluation-disable seam are removed.
The CLI was already using `EvalSession`; there was no remaining native CLI module.

The gate now rejects retired source/modules, native value types/imports, adapter
APIs, native test/benchmark targets and `src/**/*.clj`. Its final strengthened
source audit also covers embedding examples and passed independently after the
execution run. Compiler source loading selects `.cljs`/`.cljc` only; upstream
`.clj` reference oracles remain outside production loading. `cargo tree --locked
-e normal` retains only the pinned rquickjs direct dependency and its engine
support dependencies; no native evaluator feature or dependency remains.

The removed native-specific suites and benchmark binaries are not skipped
required tests. Supported semantics, ownership, cold bootstrap, source lifetimes,
failure/cancellation/reuse, CLI and embedding run in the retained canonical suites.
SCI's 2,475 original observations and evaluator dispositions are unchanged: 1,488
portable/adapted matches, 902 explicit gaps and 85 retired dialect contracts.
Historical native inventories and measurements are clearly archived and excluded
from current coverage. `--evaluator` replaces the SCI launcher's `--native` switch.
The compiler benchmark's macro fixtures now use `.cljs` and ordinary `:require`.

All final commands below exited **0**, on macOS 15.7.3 ARM64 with Rust 1.99.0,
Node 24.18.0, Python 3.14.6, Clojure CLI 1.12.4.1618 and Java 25.0.2.
`RUST_MIN_STACK` was unset. No native fence/environment override was needed.

| Command | Result |
| --- | --- |
| `cargo test --locked --no-fail-fast` | 489 passed, 0 failed, 51 explicitly ignored; includes library, binary, integration and doc-test targets |
| `cargo check --locked --all-features` | Passed |
| `cargo build --locked --all-features` | Passed; production binary/library linked without interpreter source |
| `python3 -B tests/reference/evaluator-migration/gate.py --reference --checkout research/clojurescript --artifacts /tmp/light-cljs-115-gate` | All 19 command stages passed: default check; all-feature/all-target check; 37 consumer targets (430 passed, 49 ignored); seven focused unit scopes; both Node runtime suites; accounting; pinned inventory and comparisons |
| `cargo run --locked --quiet --example embedding` | Callback invocation/error recovery, retained state and owned-form evaluation passed; final values 2 and 3 |
| `cargo run --locked --quiet --example evaluator` | Printed 42 |
| `cargo bench --locked --bench evaluator -- --iterations 1` | All three validated fresh/reuse/terminal-limit/reset scenarios passed |
| `cargo bench --locked --bench compiler -- --samples 9 --iterations 1` | 423 validated samples; source macros and filesystem macro namespaces run without adapters |
| `python3 -B benchmarks/run_compiler.py --help` | Passed after removing dependencies on retired runner scripts; helper import/hash/statistics assertions also passed |
| `git diff --check` | Passed |

The gate's Python accounting suites passed 58 compiler-language tests and seven
SCI launcher tests (one optional oracle transport job skipped). Inventory hashes
matched 3,237 assertions / 696 bodies. The gate executed 58 helper, 56 sequence
and 316 eager-transducer observations against **each** of Maven CLJS 1.12.134 and
checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`: 430 observations per reference.
The 19-stage manifest and complete logs remain in `/tmp/light-cljs-115-gate`;
full-suite and benchmark logs use `/tmp/light-cljs-115-*.log` on this host.
Benchmark invocations are smoke verification, not statistically supported
before/after performance claims.

Unexecuted checks remain explicit: opt-in oracles outside the three gate
comparisons, a fresh full JVM SCI regeneration, platform/CI runs, and dedicated
external leak instrumentation were not run. Existing lifecycle/foreign-value,
collection, repeated terminal failure and recovery regressions passed. No
native-dependent required test is ignored to achieve this result.

`light-cljs-48` and `light-cljs-66` were already closed with explicit superseded,
**not implemented**, dispositions; those dispositions are preserved. The evaluator
migration is complete. This does not close `light-cljs-125` or turn remaining
language gaps into passing semantics. Public API replacements and retired numeric,
descriptor, owner/fork/GC and resource-unit semantics are documented in
[the evaluation API](../../../docs/evaluation.md#native-api-migration-map).
