# Testing and benchmarking

## Prerequisites

- **Rust 1.91+**, **babashka (`bb`)**, **Git**, and a **C/C++ toolchain** (for the V8 build).
- **Node.js** on `PATH` (or via `$NODE`) for JS execution and runtime tests.
- **Pinned upstream ClojureScript checkout** in the gitignored `research/clojurescript` directory, which some tests include (see [tests/reference/README.md](../tests/reference/README.md)).
- **[cargo-nextest](https://nexte.st)** (`cargo install cargo-nextest --locked` or `brew install cargo-nextest`) is strongly recommended: it runs each test in its own process across all binaries in parallel (~1.5 minutes on a 24-vCPU machine vs. ~4 minutes with plain `cargo test`).

## Common commands

```sh
cargo nextest run                                  # full test suite
cargo nextest run --test compiler                  # one test binary
cargo nextest run --test compiler records::        # one suite (module)
cargo nextest run records::record_updates          # substring filter
cargo nextest run --no-fail-fast -j 8              # keep running; limit concurrency
cargo test --test compiler records::               # same via plain cargo test

cargo clippy --all-targets -- -D warnings          # strict lint
bb playground/build.bb lint                        # wasm32 playground lint
node tests/support/js_runtime.test.js              # JS runtime tests directly in Node
```

### Regenerating runtime tables

After editing `src/js_runtime.js` or moving runtime code between units:

```sh
# Rewrite unit `requires:` headers in src/js_runtime.js, then review the diff
LIGHT_CLJS_UPDATE_RUNTIME_MANIFEST=1 cargo test --test compiler runtime_manifest::

# Rewrite the core operation table's units column
LIGHT_CLJS_UPDATE_CORE_OPERATIONS=1 cargo test --test compiler core_operations::
```

## Integration test structure

Integration tests are grouped into a small number of binaries so they build and link once:

- `compiler` (`tests/compiler/`)
- `evaluation` (`tests/evaluation/` — evaluator, `--eval` CLI, and SCI reference suites)
- `frontend` (`tests/frontend/` — tokenizer, parser, analyzer, namespaces)
- `execution`, `cli`, `provenance`, `benchmark_variants`

To add a suite, add a module file in the appropriate directory and register `mod <name>;` in that directory's `main.rs`.

### Test helpers and profiles

- **`row_tests!`** ([tests/support/row_tests.rs](../tests/support/row_tests.rs), loaded by `compiler` and `evaluation`): splits long fixture loops into per-row or range tests so nextest can run them in parallel.
- **`phase_oracles`** ([tests/support/phase_oracle.rs](../tests/support/phase_oracle.rs)): tests that verify a complete program produces identical results as an application and as a macro computation, across fresh and reused macro hosts in both V8 and Node. Run all of them with `cargo nextest run phase_oracles::`.
- **Nextest config** ([.config/nextest.toml](../.config/nextest.toml)): runs 16 tests concurrently by default, schedules slowest tests first, and configures slow-test timeouts. Use `--profile ci` to run across all CPUs and report every failure.
- **Temporary directories** ([tests/support/temp_dir.rs](../tests/support/temp_dir.rs)): creates `light-*` directories in `$TMPDIR` and cleans them up on pass, fail, or panic (unless nextest kills the process on timeout). Set `LIGHT_CLJS_KEEP_TMP=1` to keep them and print their paths. Suites taking an explicit artifact directory (such as `LIGHT_PHASE_ORACLE_ARTIFACTS`) never delete it.

### Optimization and size ratchets

- **`size_ratchet`** ([tests/compiler/size_ratchet.rs](../tests/compiler/size_ratchet.rs)) pins emitted sizes of small programs ([tests/support/ratchet_fixtures.rs](../tests/support/ratchet_fixtures.rs)) across `--optimize` levels and verifies them in Node. Print the size table with:
  ```sh
  cargo nextest run --test compiler size_ratchet:: --no-capture
  ```
- **`generated_optimization`** compiles seeded multi-namespace programs containing tagged dead code across optimization levels, runs them in Node, and checks dead-code survival against expected survivors (`--no-capture` prints per-seed tables). Its `lazy_seed_*` tests also convert namespaces into lazy roots and verify chunked builds match single-file output without leaking lazy-only definitions into startup ([docs/code-splitting.md](code-splitting.md)).
  - Re-run a specific seed: `LIGHT_CLJS_GENERATED_SEED=<seed>`
  - Run more seeds: `LIGHT_CLJS_GENERATED_COUNT=240`

### Deadlines and suite timing

Some tests enforce wall-clock deadlines on macro expansion and evaluation. Under heavy machine load, lower concurrency (`-j`) or rerun a timed-out test individually.

To inspect where suite time is spent, capture a run's output and summarize it:

```sh
bb benchmarks/nextest_times.bb run.log
bb benchmarks/nextest_times.bb before.log after.log   # compare two runs
```

## CI workflows

- [`.github/workflows/conformance.yml`](../.github/workflows/conformance.yml):
  - `fast-tests`: V8 test suite, strict Clippy, and Node runtime tests (downloads the prebuilt V8 engine just like local builds).
  - `playground`: builds and checks the WebAssembly compiler with the browser macro host.
- [`.github/workflows/pages.yml`](../.github/workflows/pages.yml): builds and publishes the project site and playground to GitHub Pages on pushes to `master`.

## Reference comparisons against ClojureScript

Optional `#[ignore]`d suites compare against standard ClojureScript and require Java, the Clojure CLI, and (for corpus tests) `research/clojurescript` (see [tests/reference/README.md](../tests/reference/README.md)):

```sh
cargo test --test execution -- --ignored --nocapture          # vs. ClojureScript 1.12.134
cargo test --release --test frontend corpus::clojurescript_checkout_has_no_lexical_errors -- --ignored --nocapture
cargo test --release --test frontend parser_corpus::parse_clojurescript_checkout -- --ignored --nocapture
```

## Benchmarks

See [benchmarks/README.md](../benchmarks/README.md) for full details.

```sh
cargo bench --bench tokenizer   # also: parser, subset, compiler, evaluator
cargo bench --locked --bench compiler -- --levels --output DIR   # startup & steady-state by --optimize level in Node
cargo bench --locked --bench compiler -- --callees               # call costs in Node
```

To generate large synthetic ClojureScript applications (shared with shadow-cljs benchmarks, with a checksum oracle and tagged dead code):

```sh
bb benchmarks/generated-project/generate.bb /tmp/cljs-200k --seed 42 --lines 200k
```

See [benchmarks/generated-project/README.md](../benchmarks/generated-project/README.md) and [benchmarks/compiler.md](../benchmarks/compiler.md#startup-and-steady-state-by-optimization-level--october-13-2026).
