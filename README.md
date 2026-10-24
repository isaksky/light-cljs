# clj-quickjs

A Rust-native JavaScript runtime for highly concurrent compile-time execution.
Requirements: [`docs/design.md`](docs/design.md). Backlog: Pellets (`pl list`).

This is an architectural slice in progress, not a finished JavaScript engine.
No JavaScript source frontend exists yet.

## Build and test

The toolchain comes from `flake.nix` (`nix develop` if `cargo` is not on PATH).

```sh
cargo build --all-targets
cargo test --all-targets          # unit + integration tests
cargo test --doc                  # doc tests, including compile-fail handle tests
cargo run                         # early VM test program (hand-assembled bytecode)
cargo run --example resumable_counter   # budgeted yield/resume with a host counter
cargo test --lib resume_tests     # yield/resume and host-effect unit tests
cargo test --test resumable       # resume on another embedder thread
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

## Crate layout

Library (`src/lib.rs`) plus a thin binary (`src/main.rs`). Modules:
`runtime` (embedding entry point), `task` (task state, execution guard, run
outcomes), `vm` (instruction format, executable validation, task-owned frames,
dispatch, test assembler), `heap` (object identity, roots and scoped handles,
tracing contract), `value`, `ids`, `error`, `host` (leaf host effects). See
the `lib.rs` crate docs and [`docs/architecture.md`](docs/architecture.md).

## Supported and deferred

### VM core (clj-quickjs-1)

Supported:

- Embedding API: `Runtime`, immutable shared `Executable`, `Task`,
  persistent `Rooted` handles, scoped `Local` handles (`Runtime::scope`), and
  `RunOutcome::{Complete, Failed}`. The embedder drives tasks with
  `Task::run` on its own threads; the runtime owns no threads.
- Execution guard: a task being run cannot be run again concurrently
  (`RunError::AlreadyRunning`, never blocks); finished tasks report
  `RunError::Finished`.
- A new byte instruction format, validated at load time (unknown opcodes,
  truncated operands, bad indices and branch targets, stack underflow and
  depth mismatches, falling off the end): constants, `undefined`, local
  get/set, `pop`, `dup`, number `add`/`sub`/`mul`/`lt`, `===`, `!`, `jump`,
  `jump_if_false`, `return`, and allocation of property-less objects. The
  opcode table is in `src/vm/instr.rs`.
- Task-owned VM state (instruction position, operands, locals, diagnostic
  context); failures report executable, task, world, instruction and opcode.
- Stable object identity across tasks and runs; task results are persistent
  roots that outlive the task.

Deferred (explicitly not implemented):

- JavaScript source: programs are hand-assembled bytecode (`Assembler`), an
  early VM test tool, not JavaScript source support.
- `Waiting` is reserved but never produced (`Yielded` and budgets: see
  clj-quickjs-16 below).
- Garbage collection: the heap is a temporary non-reclaiming stub. Tracing
  hooks exist; no collector runs.
- Arithmetic on non-number operands fails with `UnsupportedOperand` instead of
  applying JavaScript coercions. No strings, properties, calls, closures,
  exceptions, collections, atoms or dependency cells yet.

### Budgeted yield and leaf host effects (clj-quickjs-16)

Supported:

- `Task::run_with(Budget, &mut dyn Host)`: a run executes at most
  `Budget::instructions(n)` instructions and returns `RunOutcome::Yielded` when
  the budget runs out. Instruction position, operands and locals stay in the
  task; the next run, on any thread, continues at the saved instruction
  (including a branch target). The budget is checked only between
  instructions, so no instruction or effect is ever replayed.
- `host_effect op` (opcode `0x11`): a non-reentrant leaf host effect through the
  `Host` trait. It takes and returns primitives only, cannot reenter the VM
  (`RunError::InsideHostEffect`), open a heap scope, or suspend. Host errors
  fail the task (`VmErrorKind::HostEffect`); `Task::run` supplies no host.
- Example: `cargo run --example resumable_counter` round-robins two tasks with
  a budget of 5 instructions and prints the host counter once per effect.
  Hand-assembled bytecode: an early VM test, not JavaScript source support.

Deferred:

- `Waiting(key)` and dependency waits (pellets clj-quickjs-6, -23, -7).
- Resumable natives that call JavaScript or suspend (clj-quickjs-12); a
  scheduler/executor with budgets (clj-quickjs-8). Budgets count instructions
  only, not time or allocation.
