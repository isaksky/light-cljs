# big-boy-pants-js

A Rust-native JavaScript runtime for concurrent compile-time execution. This is
an implemented **architectural slice**, with a broad tested JavaScript subset,
not a complete ECMAScript engine or an integrated compiler.
[Requirements](docs/design.md) are authoritative; Pellets is the backlog.
All compiler-like examples are synthetic fixtures, not light-cljs output.

## Run

Use the Rust 1.95 environment pinned by `flake.lock`. Cargo is not on the base
shell PATH in the verification environment; enter it first:

```sh
nix develop
export CARGO_BUILD_JOBS=3
cargo build --all-targets
cargo test --all-targets -- --test-threads=3
cargo test --doc -- --test-threads=3
cargo fmt --all -- --check
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
