# big-boy-pants-js: the Rust implementation

bbp's first implementation, in Rust. The project now continues in Goose (see
[../README.md](../README.md)); this crate remains as the JavaScript parser and
bytecode compiler Goose consumes and as the reference engine for semantics. It is
an implemented **architectural slice**, with a broad tested JavaScript subset,
not a complete ECMAScript engine or an integrated compiler.
[Requirements](docs/design.md) are authoritative; Pellets is the backlog.
The checked light-cljs corpus executes real compiler output and the current
runtime factory; other compiler-like examples remain synthetic fixtures.

## Run

This crate is part of the light-cljs workspace. From the repository root,
select it with `cargo build -p big-boy-pants-js` or
`cargo test -p big-boy-pants-js --all-targets -- --test-threads=3`.
The commands below run from `crates/big-boy-pants-js/`. Cargo uses the root
workspace's lockfile, profiles and `target/` directory. Building this package
alone does not build the compiler or either legacy JavaScript engine.

Use an installed Rust toolchain, or the retained Linux development environment
(`nix develop` in this directory). Historical measurements used Rust 1.95
and the standalone crate's build profiles; remeasure with the workspace
profiles before comparing new results.

```sh
export CARGO_BUILD_JOBS=3
cargo build --all-targets
cargo test --all-targets -- --test-threads=3
cargo test --doc -- --test-threads=3
cargo fmt -p big-boy-pants-js -- --check
cargo clippy --all-targets -- -D warnings
cargo run
cargo run -- examples/demo.js
cargo run --example resumable_counter
cargo run --example e2e
cargo run --example e2e -- failure
cargo run --example e2e -- cancel
cargo run --example e2e -- companion
```

The binary compiles a script and prints its completion value; the explicit
file command above uses a checked-in script. It is script-only. Module graph
loading and evaluation use the library's `ModuleLoader` and
`Runtime::spawn_module` API. The library owns no execution worker pool; `Executor` is an
optional reference scheduler.

The end-to-end example constructs a persistent input in Rust, shares it between
two JavaScript consumers, parks both on one VM helper, does useful work on the
freed workers, collects, publishes the helper and resumes on other workers.
The variants exercise producer failure, consumer cancellation, and a nested
native fold callback with closure and finally state. Assertions live in
[`tests/e2e.rs`](tests/e2e.rs); the complete requirement map is in
[architecture](docs/architecture.md#requirement-map).

## Repository history

The engine was imported without squashing from commit
`7a6e6478469ded185d0eb6bc633eaa7ea6f57033`. All 777 commits reachable from
that commit retain their original hashes. Its tree was placed under
`crates/big-boy-pants-js/`; older commits retain their original root-relative
paths. For example, inspect pre-import runtime history with:

```sh
git log 7a6e6478469ded185d0eb6bc633eaa7ea6f57033 -- src/runtime.rs
```

Develop the engine here with the compiler. There is no submodule or separate
checkout required. The `bbp-integration` group in the light-cljs Pellets project
holds the integration work. The original sibling repository is retained as a
historical checkout.

## Contract and coverage

- [Embedding and concurrency](docs/architecture.md): execution outcomes,
  rooting, safepoints, publication, atoms, dependency identity and cancellation.
- [JavaScript contract](docs/grammar.md): supported syntax and native behavior,
  feature-to-test map, explicit gaps, the reserved `__rt` binding, and the
  single [resource and policy limit reference](docs/grammar.md#resource-and-policy-limits).
- [Measurements and command results](docs/measurement.md): exact invocations,
  observed results, methodology and practical limits of the evidence.
- [Attribution](docs/attribution.md): pinned QuickJS paths, adaptations,
  generated data and retained license notices.

Source compiles to validated immutable bytecode and runs in explicit task-owned
frames. Supported families include functions/closures, strict and sloppy scope,
objects and descriptors, classes and private names, arrays and iterators,
generators, async functions/generators, Promises, static modules, BigInt,
strings/Unicode, RegExp, Date, a bounded Intl surface, errors and disposal.
Persistent vectors/maps, transient builders, atoms and dependency reads use
private native intrinsics. These claims apply to the documented contracts and
passing cases, not every API or edge case in these families.

Notable limits include immutable realm intrinsics, task-owned ordinary mutation,
partial Proxy and JSON protocols, restricted module syntax, limited locale/zone
coverage and explicit resource caps. There is no browser/Node host, QuickJS C
API/bytecode compatibility, JIT, full Clojure collection compatibility or compiler
integration. Full incremental invalidation and automatic GC triggering remain
deferred. See the contract for exact boundaries and rejection tests.

## Semantic corpus

From the workspace root, run the real compiler compatibility gate with:

```sh
cargo test --locked -p big-boy-pants-js --test light_cljs --test map_iterables -- --test-threads=3
```

[`tests/light_cljs.rs`](tests/light_cljs.rs) boots the current `src/js_runtime.js`
factory and executes seven checked compiler programs, covering arithmetic,
closure capture, persistent maps/vectors, sequence reduction, user macro
expansion and protocol dispatch. The map, vector and protocol programs contain
small loops for future measurements. They use one shared emitted runtime;
engine tests need neither a compiler build, an external checkout, V8 nor Node.
This is execution evidence for a bounded corpus, not native shared-form or
compile-time scheduler integration, general ClojureScript compatibility, or a
performance result.

Regenerate from this checkout using
`bb crates/big-boy-pants-js/rust/tools/generate-light-cljs.bb`. It builds the compiler
with `--no-default-features --features quickjs` (QuickJS expands macros during generation), then
compiles the checked `.cljs` sources. `tests/fixtures/light-cljs/generation.txt`
records the compiler revision and command, `expected.tsv` supplies independent
numeric expectations, and `LICENSE` retains the emitted runtime's MIT notice.
The generator factors out identical runtime text without changing JavaScript
semantics; tests reassemble the exact output before compiling it in this engine.

Run this in the development shell, with `CARGO_BUILD_JOBS=3`:

```sh
cargo test --test quickjs_language --test quickjs_functions \
  --test quickjs_objects --test quickjs_builtins --test quickjs_collections \
  --test quickjs_bigint --test quickjs_async --test quickjs_intl \
  --test js_source --test js_functions --test js_modules --test exceptions \
  --test error_stacks --test need --test native_fold --test cancellation \
  --test malformed_input -- --test-threads=3
```

This is the bounded clj-quickjs-30 corpus command. The full suite above also
covers the subsequently landed gap-specific integration binaries. Upstream
fixture selection and adaptations are described in [attribution](docs/attribution.md);
malformed-source, reserved-binding and bytecode-boundary checks are in
[`tests/malformed_input.rs`](tests/malformed_input.rs). Hand-assembled bytecode
is an early VM test tool, not evidence of source frontend coverage.

## Measure

```sh
cargo run --release --example bench
cargo run --release --example bench -- --quick
```

The harness checks its results and reports startup, ready reads, park/wake,
persistent operations, shared and suspended memory, useful work and strings.
[Measurement](docs/measurement.md) records the runs and caveats. These numbers
measure this implementation only; there is no matched comparison with another
engine or the unavailable compiler.
