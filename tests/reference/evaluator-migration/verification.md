# Migration gate verification — October 10, 2026

**Incomplete.** This evidence does not authorize native deletion or close
light-cljs-114. The supported production compiler/evaluator fixtures were
migrated, and old native entrypoints are fenced, but broad acceptance still
exposes existing queued defects. The live Pellet description retains the full
acceptance criteria and resumable evidence.

Starting Git commit: `0413d551bce3493f761be15624a5e3dbea3fd580`.
Tools: macOS 15.7.3 ARM64; Rust/Cargo 1.99.0; Node v24.18.0; Python 3.14.6;
Clojure CLI 1.12.4.1618; Java 25.0.2. Required CLJS references are Maven 1.12.134
and checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`; the checkout reports
ClojureScript `0.0.135109660` and Clojure 1.12.4. These are local executions,
not GitHub Actions or cross-platform results.

## Executed focused checks

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

## Pinned comparisons actually executed

With `LIGHT_CLJS_REFERENCE=both`, explicit checkout path and full revision,
`LIGHT_CLJS_DISABLE_NATIVE_EVAL=1`:

- `cargo test --test execution compiled_helpers_match_pinned_values_effects_and_stdout -- --ignored --exact --nocapture`: 1 passed; **58** outcomes, ordered effects and stdout match each reference.
- `cargo test --test execution compiled_sequences_match_pinned_values_effects_and_stdout -- --ignored --exact --nocapture`: 1 passed; **56** outcomes, effects and stdout match each reference.
- `cargo test --test compiler_transducers eager_transducer_fixtures_match_pinned_release_and_checkout -- --ignored --exact --nocapture`: 1 passed; **316** eager transducer observations match each reference.

The total is **430 cases per reference**. Other ignored pinned oracle tests were
not rerun and are not passes. Local phase/macro dialect assertions do not require
an upstream compiler to accept source-only `.cljs` macros. Historical SCI golden
rows were preserved; a new JVM regeneration of every SCI row was not run locally.

## Known failing acceptance

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

## Broad command status

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

## Performance

The existing `--application-only` benchmark executed with the fence and nine
samples: **270 measured rows**, all complete values checked in Node. The
[benchmark record](../../../benchmarks/compiler.md#evaluator-migration-gate--october-10-2026)
retains raw samples and output sizes. Concurrent compiler builds make timing
comparisons noisy; no reliable startup regression/speedup or native-GC claim is
made. CI configuration was updated but no remote Actions run was started.
