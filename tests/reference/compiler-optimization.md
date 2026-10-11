# Rust application preparation

`tests/compiler_optimization.rs` exercises the application-only Rust preparation
boundary through `prepare_application`, and compares its optional optimizer with
unoptimized emission from the same analyzed `Program`. `emit_js(&Program)` uses
the default optimized preparation. Compile-time helper execution uses the
separate demand-root emission path; JavaScript does not perform this optimizer.

These are repository-specific architecture and regression tests, not new
upstream ClojureScript conformance witnesses. Their contracts come from the
`native-cljs-macros` phase-separation design and the effect and identity semantics
already tested by the compiler macro, state, and protocol suites. No upstream
reference compiler execution or new language coverage is claimed here.

## Executable observations

- A pure constant and helper demanded only by macro expansion disappear from
  application output; a helper used by both phases still produces `[42 42]`.
  The output is checked for absence of the compile-time demand-root machinery.
- Unknown initializer calls and standalone mutations execute exactly once in
  source order. An otherwise unused throwing initializer still aborts execution,
  preserving the preceding effect and preventing the following one.
- Optimized and unoptimized applications agree on redefined functions, saved
  function values, callee selection before argument mutation, dynamic bindings,
  `with-redefs`, and escaped Var identity, metadata, and invocation.
- Protocol extension replacement affects saved dispatch functions in both modes.
- Immutable multi-arity targets and resolved core aliases report one specialized
  call and preserve the unoptimized result. Repeated prepared emission is stable.
- Explicit `GlobalId` exports retain an otherwise unused pure definition while
  unrelated pure definitions disappear.
- A namespace required solely for its macro keeps both initializer and standalone
  runtime effects, before the entry namespace's effects; its pure macro helper
  disappears from final output.

Node executions use `NODE` when supplied and otherwise `node`. Each execution
starts a fresh process so global host effects and protocol registrations cannot
leak between optimized and unoptimized observations. Namespace fixtures use a
unique temporary source directory that is removed after the test.

## Verification

`cargo test --test compiler_optimization` — **11 passed, 0 failed, 0 ignored**.
The suite executed both preparation modes and fresh Node processes.
`rustfmt --edition 2024 tests/compiler_optimization.rs` completed successfully.

No upstream oracle or cross-platform execution was performed. The focused
application benchmark below is separate from these integration tests.

Additional scoped verification:

- `cargo test --lib application::tests` — 2 passed. Three macro invocations do
  not run application preparation, each helper generation is analyzed/emitted
  once, unused application-only units are analyzed but not compile-time emitted,
  and repeated prepared emission leaves the preparation counter unchanged.
- `cargo test --lib program_fragment::tests` — 3 passed.
- `cargo test --test compiler_bootstrap` — 11 passed.
- `cargo test --test compiler_optimization --test compiler_interop` — 11 and
  3 passed after the final host-prelude and raw-JS reachability changes.
- `cargo test --test namespaces` — 28 passed, including source/dependency order,
  `.cljs`/`.cljc` selection, aliases, CLI/API output and ordinary macro imports.
- `cargo test --test compiler_macros ordinary_` — 16 passed; filters
  `reentrant_deferred_units`, `declaration_bundle_demand`, and
  `same_file_javascript_macros` separately passed 6, 6, and 8 tests.
- The combined analyzer/protocol/extensions/type/record execution command passed
  35 analyzer, 5 extension, 5 protocol, 5 type and 6 record tests before the
  execution binary aborted on its pre-existing debug-stack issue. The exact command
  was `cargo test --test namespaces --test analyzer --test execution --test
  compiler_optimization --test compiler_protocols --test compiler_extensions
  --test compiler_types --test compiler_records`.
- `RUST_MIN_STACK=8388608 cargo test --test execution` — 52 passed, 1 failed,
  43 ignored. The sole failure is the existing `state-volatile-order` fixture,
  tracked in `light-cljs-126`. The ignored tests require external pinned oracles;
  they were not executed or counted as passes.

The default-stack abort in `deeply_nested_accepted_source_compiles_and_executes`
was independently reproduced with the exact test on a clean `git archive` of
starting commit `9730c8b`. A temporary diagnostic test confirmed overflow during
`analyze`, before application preparation. It is recorded as `light-cljs-129`;
raising the test-thread stack above was a diagnostic workaround, not a fix.
No broad execution pass is claimed.

## Focused application benchmark

Added `--application-only` to `benches/compiler.rs`; the existing benchmark
paths remain available. This mode analyzes each source once and independently
measures Rust preparation, prepared emission, and fresh Node startup plus
parsing/execution. Each phase retains the existing CSV schema and two warmup
batches. Prepared-emission checksums are output bytes per iteration; the dumped
files were independently checked against all corresponding checksums. Every
Node invocation checks its complete serialized result against the unoptimized
result, including warmups and all measured samples.

Executed on this macOS host with Node **v24.18.0** and Rust **1.99.0**:

```sh
cargo bench --bench compiler -- --application-only --samples 9 --iterations 1 --dump /tmp/light-cljs-111-bench > /tmp/light-cljs-111-bench/results.csv 2> /tmp/light-cljs-111-bench/build.log
```

The dump directory was created before the command. The command exited 0 and
produced **270 samples**: five scenarios, two modes, three phases, nine samples.
The raw CSV, build log, and ten emitted files remain outside the repository in
`/tmp/light-cljs-111-bench`. The primitive and collection scenarios reuse the
existing benchmark fixtures. `macro-helpers` has 24 bounded helper/constant pairs
used during macro expansion, plus a shared runtime helper yielding `[42 42]` at
the start of its result vector. Preparation/emission measurements exclude
analysis and macro execution. Node measurements include process startup, input
transfer, JS parsing/evaluation, serialization, and validation.

All timing entries below are medians; paired values are **off / on**.

| Scenario | Output bytes | Preparation µs | Prepared emission µs | Fresh Node ms |
| --- | ---: | ---: | ---: | ---: |
| integer | 88 / 88 | 0.416 / 0.667 | 0.292 / 0.292 | 58.529 / 55.511 |
| nested-closure | 297 / 297 | 2.292 / 3.583 | 1.375 / 1.250 | 53.708 / 53.584 |
| js-nested-method | 126 / 126 | 0.833 / 1.541 | 0.667 / 0.625 | 54.273 / 56.340 |
| collections | 192,302 / 192,302 | 4.416 / 8.166 | 8.833 / 9.458 | 65.754 / 61.467 |
| macro-helpers | 196,645 / 192,469 | 400.083 / 355.583 | 45.292 / 32.875 | 55.987 / 57.439 |

The macro-helper output decreased by **4,176 bytes (2.12%)**. The other fixture
sizes were unchanged. Startup medians moved in both directions; this short,
sequential run does not establish a startup speedup or a statistically reliable
regression. The byte reduction is deterministic for these fixtures. The legacy
full `benchmarks/run_compiler.py` suite was not run, and no cross-platform or
allocation-count benchmark claim is made.
