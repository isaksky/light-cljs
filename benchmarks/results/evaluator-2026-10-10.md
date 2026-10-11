# Synchronous evaluator baseline — 2026-10-10

Host: macOS arm64, Rust 1.99.0 (Homebrew), Node v24.18.0. This is a
focused startup/size baseline for pinned rquickjs 0.14.0, not an engine comparison.

Commands and observed results:

- `cargo test --locked --release --test evaluator startup_measurement -- --ignored --nocapture`:
  100 fresh runtimes, canonical factory loads, scalar evaluations and teardowns
  took **900.210209 ms** total (~9.002 ms/session). Live canonical heap accounting
  reported **766,276 bytes**; canonical source was **175,593 bytes**.
- `cargo build --locked --release --example evaluator`: success.
- `target/release/examples/evaluator`: printed **42**.
- `stat -f '%z bytes' target/release/examples/evaluator`: **5,101,168 bytes**
  for the unstripped release embedding example, including compiler and QuickJS.

The startup loop creates a new heap each time, rather than timing a warm engine.
This initial baseline has no pre-existing evaluator for a before/after claim.
Binary size includes the still-present transitional compiler/native macro adapter;
consumer migration/removal will change it. Repeat the same commands on the same
machine/profile for regression comparisons. Timing is reported, not asserted.

Verification in this implementation:

- `cargo test --locked --test evaluator --test compiler_reader --test compiler_interop`:
  passed (7 evaluator, 7 reader and 3 interop tests at that run; measurement ignored).
- `cargo test --locked --test evaluator`: passed after adding the raw-JS hygiene
  regression and allocator teardown accounting assertion (8 tests; measurement ignored).
- `cargo check --locked --all-features`: passed.
- `node tests/support/js_runtime.test.js`: passed, including canonical gensym and
  explicit macroexpand service hooks.

The evaluator test executes direct expected outcomes in both QuickJS and Node,
checks caught allocation/cancellation/fuel failures and panic containment, and
repeats 30 fresh sessions containing mutable roots, cyclic objects and callbacks.
Debug teardown asserts allocator balance after the engine drops. Required pinned
upstream ClojureScript/SCI conformance and other platform builds were not rerun
for this host integration; existing dedicated CI jobs retain those checks.
