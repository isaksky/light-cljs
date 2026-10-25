# Measurement and command verification

These measurements describe the engine package. Compiler-shaped kernels below
include an optional Node jitless reference; historical standalone rows do not
compare against other engines or the compiler. The source-driven
architectural proofs and their limitations are mapped in
[architecture.md](architecture.md#requirement-map).

## Reproduce

Run these commands from `crates/big-boy-pants-js/` in the light-cljs checkout.
Cargo now uses the root workspace profiles, lockfile and `target/` directory.
The results below predate that import and used the standalone crate's profiles;
they are historical measurements, not a baseline for the workspace build.

Inside the pinned `nix develop` environment, use `CARGO_BUILD_JOBS=3` throughout.
Test commands use `--test-threads=3`. Release measurements:

```sh
cargo run --release --example bench
cargo run --release --example bench -- --quick
cargo run --release --example bench -- memory
cargo run --release --example bench -- work --workers 4 --rounds 9
cargo run --release --example bench -- strings --rounds 3
cargo test --test measurement -- --test-threads=3
```

The full invocation runs every row group; quick uses smaller sizes. The
`memory` and `strings` selectors isolate those groups. The work selector above
uses four executor workers and nine measured rounds; tests still use three
test threads. The benchmark owns its default target output only.

## Automatic collection pauses (2026-10-25)

Measured engine commit `84ac7c0e`, macOS aarch64, virtual Apple M3 Ultra
(24 available CPUs), Homebrew Rust/Cargo 1.99.1, workspace release profile.
Peer builds/tests were stopped. Load average was 3.29/3.56/3.51 before and after.
Build with `CARGO_BUILD_JOBS=3 cargo build --release --locked -p big-boy-pants-js
--example bench`, then run `target/release/examples/bench gc --quick --rounds 5`
from the repository root. One discarded warmup and five measured rounds use
fresh runtimes; each worker allocates 4,000 short-lived arrays. The policy floor
is 1,024 objects, with no hard object cap. These are collector pause observations,
not throughput/scaling ratios: eight workers perform eight times the total work.

| Workers | Total pause ms/run, median [min..max] | Maximum pause ms, median [min..max] | Collections/run, median [min..max] | Last live objects, median [min..max] |
| --- | --- | --- | --- | --- |
| 1 | 0.7 [0.6..0.7] | 0.2 [0.2..0.2] | 4 [4..4] | 495 [495..495] |
| 8 | 13.2 [12.5..14.4] | 0.2 [0.2..0.2] | 82 [78..92] | 495 [495..497] |

Pause timing includes stopped-world tracing and sweep, excluding rendezvous
waits, payload destruction and post-pause observers. Overlapping worker requests
can cause additional collections; counts are not allocations divided by the
threshold. Values are rounded by the harness. Object admission is an aggregate
object-count budget only; these rows do not establish a managed-byte/RSS bound.

## Compiler-shaped kernel baseline (2026-10-25)

Run from this crate directory (or add `-p big-boy-pants-js` from the repository
root):

```sh
cargo run --locked --release --example bench -- scaling
cargo run --locked --release --example bench -- kernels
bb scripts/kernels.bb
bb scripts/kernels.bb kernels --quick --rounds 3
```

`kernels` selects single-thread ns/iteration; `scaling` selects 1, 2, 4 and 8
registered OS threads. Both select the same five sources. `--quick` reduces
sizes, and `--rounds N` changes the number of measured rounds. The default is
one discarded warmup and five measured rounds, each with a fresh `Runtime`.
Compilation, Runtime construction, thread creation/registration, and numeric
argument rooting happen before the timer. The start barrier releases all
workers together. Each worker spawns and runs four tasks to completion using
one shared executable and Runtime. Spawn/run, result assertions, task/result
drop and worker joins are timed. Runtime destruction and collection are not;
there is no collection between tasks, matching the strategy's original probe.
The result is aggregate iterations/s and wall ns per aggregate iteration;
speedup is the one-thread median ns divided by the row median ns. Spreads are
min..max of measured rounds, not confidence intervals. Work per thread stays
constant, so total work and retained garbage grow with thread count.

The arithmetic loop touches only VM locals (task startup still allocates).
`allocate` is the strategy's exact cons/map/count body, with list length 1000;
one iteration builds and maps one list, using constructor objects, a captured
closure and recursion. `properties` repeatedly reads/writes two object shapes.
Their full task sizes are 400,000 loop bodies, 40 list pairs, and 40,000 loop
bodies respectively. The `cljs_map` and `cljs_protocol` rows each repeat the
checked emitted `tests/fixtures/light-cljs/{map,protocol}.js` program 40 times
per task. Each iteration performs the corpus's 16 assoc/reduce steps or 16
protocol calls/Box allocations. Only the runtime declaration is hoisted once
per task; emitted expressions, namespace setup, protocol registration and
expected totals remain unchanged. Runtime factory initialization is included
once per task. These are real-output kernels, not warmed macro sessions;
their iteration units differ from the synthetic loops and from one another.
No mutable JavaScript runtime object is shared between tasks.

The Babashka script works from any directory and runs the release bench. If
`node` is on PATH it asks the bench for `--node-kernels`, which emits the exact
same source, sizes and expected results, then runs it under `node --jitless`.
It prints a reference column and engine/Node ratio for the one-thread rows.
Node uses the same warmup, measured rounds and four calls per round; parsing
and process startup are outside its timer. Node's GC policy and lack of engine
task bookkeeping differ. Node keeps one process across rounds, so built-in
caches may also differ. This is an end-to-end kernel reference rather
than an isolated opcode comparison. Node is optional and never invoked by
Rust tests. Small correctness instances run all five kernels with 1/2/4/8
threads in `tests/measurement.rs`; neither tests nor scripts impose a speed
threshold.

### Recorded Mac run

Measured on the 24-CPU Apple M3 Ultra (Virtual), 32 GiB RAM, macOS 15.7.3
(24G419), Homebrew Rust 1.99.1 (`58cf8f9c3`), Node v26.12.1, Babashka
v1.12.218. The workspace release profile uses thin LTO and one codegen unit;
no `RUSTFLAGS` or `RUST_MIN_STACK` override was set. Command, from this crate:
`CARGO_BUILD_JOBS=3 bb scripts/kernels.bb`. This ran the release `bench -- scaling`
with its full sizes, four tasks per thread, one warmup and five measured rounds,
then the Node reference. The tracked checkout was clean. Built/measured commit:
`bca99ec4e783166ce1c5975538d1a61591df8597`; the same benchmark/runtime/build inputs
are preserved at `37738cf5c076f5ca661ba13ffa4e337aa9468deb` after rebasing onto
`3a6de453` (only an unrelated promise test's scratch-directory setup changed).

Other orchestration builds/tests were stopped for the entire timing window.
Host load averages (1/5/15 minute) were `4.82 3.97 3.22` before and
`7.10 6.80 5.67` after the engine run; the latter includes the benchmark's own
workers. Background host activity was not otherwise controlled.

| Kernel | Threads | ns/iteration median [min..max] | Iterations/s | Speedup |
|---|---:|---:|---:|---:|
| arithmetic | 1 | 329.5 [322.4..330.1] | 3035320.9 | 1.000x |
| arithmetic | 2 | 167.3 [167.0..171.9] | 5978547.3 | 1.970x |
| arithmetic | 4 | 86.5 [85.3..87.6] | 11563903.5 | 3.810x |
| arithmetic | 8 | 43.3 [43.2..46.0] | 23076801.7 | 7.603x |
| allocate | 1 | 13554648.2 [13315575.0..13648202.1] | 73.8 | 1.000x |
| allocate | 2 | 11112343.8 [10500254.0..18082029.7] | 90.0 | 1.220x |
| allocate | 4 | 26305818.9 [26176690.6..26493554.2] | 38.0 | 0.515x |
| allocate | 8 | 31815732.6 [31570174.3..32477078.9] | 31.4 | 0.426x |
| properties | 1 | 6107.8 [6104.6..6173.5] | 163725.1 | 1.000x |
| properties | 2 | 3794.0 [3760.6..3923.4] | 263574.6 | 1.610x |
| properties | 4 | 4258.4 [3980.0..4408.3] | 234832.5 | 1.434x |
| properties | 8 | 10486.6 [10357.4..10627.4] | 95359.4 | 0.582x |
| cljs_map | 1 | 16264334.4 [15961063.0..16376408.9] | 61.5 | 1.000x |
| cljs_map | 2 | 15767915.5 [14745397.8..16613702.5] | 63.4 | 1.031x |
| cljs_map | 4 | 40901901.4 [40022752.5..41196247.4] | 24.4 | 0.398x |
| cljs_map | 8 | 45031670.5 [44053418.6..45313605.2] | 22.2 | 0.361x |
| cljs_protocol | 1 | 1824261.5 [1815839.1..1896904.9] | 548.2 | 1.000x |
| cljs_protocol | 2 | 1708634.2 [1679633.6..1783499.6] | 585.3 | 1.068x |
| cljs_protocol | 4 | 4560010.2 [4436908.8..4633556.2] | 219.3 | 0.400x |
| cljs_protocol | 8 | 5020414.7 [4972272.2..5059423.8] | 199.2 | 0.363x |

| Kernel | Node jitless ns/iteration median [min..max] | Engine / Node median |
|---|---:|---:|
| arithmetic | 9.4 [9.3..9.5] | 35.05x |
| allocate | 131736.7 [131169.8..133569.8] | 102.89x |
| properties | 34.9 [34.3..35.8] | 175.01x |
| cljs_map | 340387.5 [331689.1..341481.5] | 47.78x |
| cljs_protocol | 37695.6 [35213.0..42745.3] | 48.39x |

The allocation kernel's **0.426x at eight threads reproduces the roughly 0.4x
collapse** in the strategy (its earlier single-run probe reported 0.37x).
This uses the same 40 rounds of 1000-element lists and four tasks per thread;
barrier-based timing now excludes thread creation, and the table reports
medians/spreads rather than one sample. Arithmetic reaches 7.603x, while both
real-output kernels fall to about 0.36x. These measurements establish a
baseline, not an achieved Phase 0 performance gate. The current single-thread
ratios are recorded as measured; the strategy's coarse 30–45x process-wall-time
estimate is not a bound on these rows, especially the property/allocate loops.

Validation: all 10 `--test measurement` tests passed after the stack-safety
integration, as did engine formatting and Clippy with only the three existing
Rust 1.99 diagnostic categories allowed. Strict Clippy still reports the nine
existing diagnostics described below. The full engine suite was not repeated
for this benchmark-only change. Quick scaling plus Node and single-thread
script modes also completed successfully. The final rebase changed only an
unrelated test setup; benchmark and measurement-test inputs were unchanged.

## Property dispatch optimization (2026-10-25)

Compared the landed kernel harness at `30cc71cb` with implementation
`3b15101a7d0610bd2790aa62e1dd3a64932264ec`, in the same isolated worktree on
the Mac/Rust 1.99.1 environment above. Both builds used the workspace release
profile, without `RUSTFLAGS` or stack overrides. Commands from the repository
root, separately at each revision:

```sh
CARGO_BUILD_JOBS=3 cargo build --locked --release -p big-boy-pants-js --example bench --bin big-boy-pants-js
target/release/examples/bench kernels --rounds 5
```

These are full sizes, four tasks on one registered thread, one discarded
warmup and five measured rounds, with identical sources, expected results and
timing boundaries. Builds, tests and profiles were stopped during timing;
other orchestration workers also paused their builds/tests. Background host
activity remained uncontrolled. Before-run load averages changed from
`3.34 3.91 4.53` to `2.83 3.72 4.44`; after-run averages changed from
`2.53 3.43 4.26` to `2.70 3.42 4.24`.

| Kernel | Before ns/iteration median [min..max] | After ns/iteration median [min..max] | Before / after |
|---|---:|---:|---:|
| arithmetic | 329.8 [328.0..330.1] | 336.3 [333.0..341.8] | 0.981x |
| allocate | 12298531.8 [12152139.1..12406706.5] | 3323289.6 [3253749.7..3342961.5] | 3.701x |
| properties | 6144.6 [6111.9..6351.5] | 1062.8 [1059.6..1064.9] | 5.782x |
| cljs_map | 15953251.0 [15868079.7..16012860.9] | 5611864.6 [5535960.2..5714907.0] | 2.843x |
| cljs_protocol | 1799292.7 [1791667.2..1818180.2] | 623347.4 [619620.8..626982.6] | 2.887x |

The four property-using rows improve substantially. The arithmetic control
is about 2% slower in this comparison; this change does not optimize its
local-variable loop. No scaling or new Node comparison is claimed here.

For a separate profile at each revision, the exact `Kernel::Allocate` source
from `examples/bench/workloads.rs` was copied to an ignored script under
`target/`, followed by `kernel(2000)` and an assertion that it returns
2,000,000. The release CLI ran that script while
`sample "$profile_pid" 5 1 -file profile.txt` sampled it for five seconds at
one-millisecond intervals. Both processes completed with the expected total.
The main thread supplied 3,782 before samples and 3,787 after samples:

| Self frame | Before samples | After samples |
|---|---:|---:|
| `vm::proxy::inspect` | 1020 (27.0%) | 20 (0.53%) |
| `module::namespace::probe` | 581 (15.4%) | absent from sampled stacks |
| `vm::property::lookup` | 228 | 37 |
| `vm::property::check_inherited` | 203 | 52 |

Proxy/namespace probes were the top two self frames before (42.3% combined)
and are no longer top frames. The leading after frames are
`dispatch::execute_budgeted` (489), destruction of `dispatch::Fault` (275),
`dispatch::step_instruction` (213), and `native::start_conversion` (208).
Mutex operations, allocation, key hashing and constructor-key UTF-16
encoding remain visible; these are sample distributions over equal wall
intervals, not equal amounts of completed work.

The implementation replaces linear intrinsic-view scans with a constant-time
slot lookup plus full generation check. Ordinary reads, own descriptors,
writes and prototype walks classify exotics during the access they already
need. Namespace entries and typed-array views are copied out before any
export-cell/buffer access; proxy and typed-value conversions resume through
the existing VM continuations. Numeric-key parsing and its string encoding
now run only for typed-array receivers on these paths. Named keys retain
their existing UTF-16 strings, and read/write walks materialize array-index
names only when ordinary storage needs them. Constructor-key synthesis and shape/key
caching are not changed.

Validation used `CARGO_BUILD_JOBS=3` and `--test-threads=3`. At `3d89c9e1`,
`cargo test --locked -p big-boy-pants-js --all-targets` passed **1,964 tests**
in 58 result groups, with zero failures or ignored tests, including the
QuickJS-derived corpus; `--doc` passed all **six doctests**. After rebasing
onto `30cc71cb`, the runtime and regression-test content was unchanged, so
those results were reused; the changed `--test measurement` suite passed
all **10 tests**, including the new shared-runtime kernel check. Engine
formatting and `cargo check --locked -p big-boy-pants-js --all-targets`
passed. Final all-targets strict Clippy still reports the same nine existing
Rust 1.99 diagnostics; allowing only `deprecated`,
`clippy::chunks_exact_to_as_chunks`, and `clippy::byte_char_slices` after
`-D warnings` passes, with no source suppressions or new diagnostics.
Focused regressions cover proxy/namespace/typed-array inheritance, function
setters, UTF-16 names, foreign-owner faults, intrinsic lock-free access, and
intrinsic slot holes and stale generations.

## Segmented allocation (2026-10-25)

Compared `98a33109644b8f1e0fb7bd2082c25c0e46e815f5` with allocator implementation
`c48eda0bdfea996f3e940e93f3faedc816aadb82`, using the same Mac/Rust 1.99.1
workspace release profile described above. Command at both revisions, from
repository root: `CARGO_BUILD_JOBS=3 cargo run --locked --release -p
big-boy-pants-js --example bench -- scaling --quick --rounds 3`. The after run
invoked the resulting binary directly after its build completed. Both runs use
one discarded warmup, three measured rounds, four tasks per thread, and the
unchanged canonical sources and result checks. Quick sizes are 4,000 iterations
per task for arithmetic/properties and two iterations for each other kernel.

No other build or test ran during either timing window. Background host activity
was uncontrolled: load averages changed from `1.63 2.14 3.22` to
`2.79 2.37 3.26` before, and from `3.65 3.53 3.33` to `4.74 3.80 3.43` after.
No stack overrides or `RUSTFLAGS` were used for these measurements. These quick
rows are a paired allocation-change comparison, not a replacement for the
full-size Phase 0 scaling gate.

| Kernel | Threads | Before ns/iteration median [min..max] | After ns/iteration median [min..max] | Before / after | After scaling |
|---|---:|---:|---:|---:|---:|
| arithmetic | 1 | 313.9 [303.6..320.8] | 327.4 [314.7..327.4] | 0.959x | 1.000x |
| arithmetic | 2 | 161.2 [161.1..168.3] | 160.2 [157.4..162.0] | 1.006x | 2.043x |
| arithmetic | 4 | 87.7 [87.1..87.8] | 85.9 [83.8..86.4] | 1.021x | 3.810x |
| arithmetic | 8 | 44.7 [44.3..46.8] | 44.1 [44.0..44.7] | 1.014x | 7.425x |
| allocate | 1 | 3197020.9 [3183557.4..3269572.9] | 3368651.0 [3352906.2..3412270.8] | 0.949x | 1.000x |
| allocate | 2 | 5203765.6 [4559171.9..5533393.2] | 4238260.4 [4192677.1..4475028.6] | 1.228x | 0.795x |
| allocate | 4 | 10133656.2 [10127460.9..10176294.3] | 10079332.0 [10019949.2..10119239.6] | 1.005x | 0.334x |
| allocate | 8 | 10965673.2 [10941818.4..11005496.7] | 10989174.5 [10967417.3..11140337.9] | 0.998x | 0.307x |
| properties | 1 | 1033.4 [1029.7..1069.9] | 1082.6 [1075.2..1103.7] | 0.955x | 1.000x |
| properties | 2 | 915.6 [895.2..923.4] | 927.7 [908.0..1160.6] | 0.987x | 1.167x |
| properties | 4 | 1182.8 [1021.3..1261.0] | 1763.4 [1390.7..1999.1] | 0.671x | 0.614x |
| properties | 8 | 3243.9 [2689.1..3432.7] | 3579.4 [3321.6..3647.8] | 0.906x | 0.302x |
| cljs_map | 1 | 9493333.4 [9485562.5..9593838.5] | 10273859.4 [10252666.8..10346088.5] | 0.924x | 1.000x |
| cljs_map | 2 | 13642382.8 [13638968.8..13695411.5] | 14103614.6 [14091247.4..14787518.2] | 0.967x | 0.728x |
| cljs_map | 4 | 37995263.0 [37491960.9..38857571.6] | 38555520.8 [37033105.5..38653082.0] | 0.985x | 0.266x |
| cljs_map | 8 | 37764840.5 [37653488.3..39014160.8] | 38561640.6 [38168747.4..38856173.2] | 0.979x | 0.266x |
| cljs_protocol | 1 | 4792307.4 [4778343.8..4882416.8] | 4996552.1 [4943744.8..5006802.0] | 0.959x | 1.000x |
| cljs_protocol | 2 | 6627838.5 [6277471.4..6849049.5] | 5899648.4 [5786901.1..6086953.1] | 1.123x | 0.847x |
| cljs_protocol | 4 | 17914750.0 [17779442.7..18179776.0] | 16933380.2 [16158937.5..16958410.2] | 1.058x | 0.295x |
| cljs_protocol | 8 | 17612559.2 [17363738.9..17772612.0] | 16632587.9 [16473045.6..16872180.3] | 1.059x | 0.300x |

The allocation kernel's eight-thread throughput remains essentially unchanged
(91.2 to 91.0 iterations/s); relative scaling is 0.292x before and 0.307x after.
Its two-thread median improves by 1.228x, while its one-thread median regresses
by 5.4%. Most other contended rows remain poor. This does **not** demonstrate
the full shared-heap scaling gate: these JavaScript kernels still read and
mutate existing objects under the table lock. Stable segment lookup and atomic
header checks also add work to those accesses.

The narrower allocation contract is exercised directly by
`heap::storage::tests::reserved_allocations_and_shareability_checks_do_not_take_the_table_lock`:
ordinary and persistent allocations, including shareability checks of object
parts, complete while another thread holds the object-table lock. Only batch
refills need that lock. Existing-object access remains the scope of
light-cljs-471; no duplicate performance follow-up was created here.

For attribution, a separate `target/release/examples/bench scaling --rounds 1`
run used the full sizes. After its four-thread allocation row completed,
`sample "$bench_pid" 5 1 -file target/allocation-profile.txt` captured the
eight-thread allocation phase. This profiled run is separate from the paired
quick timing rows above. The eight workers contributed 25,816 sampled stacks
(3,227 each); the waiting main thread is excluded from the percentages below.

| Top self frame | Worker samples | Worker sample share |
|---|---:|---:|
| `__psynch_mutexwait` | 15,318 | 59.3% |
| `__psynch_mutexdrop` | 6,661 | 25.8% |
| `dispatch::execute_budgeted` | 404 | 1.6% |
| `pthread_mutex_lock` | 276 | 1.1% |
| `dispatch::Fault` destruction | 270 | 1.0% |

`Heap::allocate`, including its descendants, accounted for 241 stacks (0.93%);
only 15 mutex-wait samples were beneath it, attributable to batch refills.
The leading remaining mutex-wait callers were existing-object paths:
`proxy::unwrapped` (3,557), `property::lookup` (2,753),
`property::prepare_write` (1,827), `dispatch::resolve_this` (1,727), and
`proxy::inspect` (1,359). The common allocation path is no longer the serialized
operation; existing-object locking remains the measured bottleneck. These are
wall-time sampled stacks, not instruction counts or an isolated allocator
throughput benchmark.

Validation at `c48eda0b` used `CARGO_BUILD_JOBS=3` and `--test-threads=3`:

- `cargo test --locked -p big-boy-pants-js --all-targets` passed **1,969 tests**
  in 58 result groups, with no failures or ignored tests. `--doc` passed all
  **six doctests**. This includes the full root inventory, both public and
  crate-internal all-holder allocation/collection tests, the GC suites,
  publication, the QuickJS-derived corpus and the 64 KiB worker-stack tests.
- Fifty iterations of the compiled libtest binary filtered to `heap::`
  (53 tests), the complete `gc` binary (51 tests), and the `gc_roots` binary's
  exact `every_holder_retains_while_other_workers_allocate_and_collect` test
  all passed: **5,250 test executions**. The new four-worker identity stress
  covers eight rounds and 16 collections per execution, keeps partially used
  reservations across collection, checks retained objects' unique stamps,
  rejects stale IDs, and rejects duplicate IDs across recycled slots.
- `cargo fmt -p big-boy-pants-js -- --check` passed. Strict
  `cargo clippy --locked -p big-boy-pants-js --all-targets -- -D warnings`
  still reports the same nine pre-existing Rust 1.99 diagnostics in
  `deprecated`, `clippy::chunks_exact_to_as_chunks`, and
  `clippy::byte_char_slices`. Adding only `-A` for those three categories
  passes; no source suppressions were added.
- ThreadSanitizer passed all **53 heap tests and 121 public GC/root tests**,
  with an instrumented standard library and no race reports. Miri is absent
  (`cargo miri --version` reports no such command). Homebrew's Rust lacks its
  TSan runtime library, so the matching installed rustup Rust 1.99.1 dylib
  supplied the runtime. Reproduction on this machine:

```sh
sanitizer_lib="$(rustup run stable rustc --print sysroot)/lib/rustlib/aarch64-apple-darwin/lib"
export RUSTC_BOOTSTRAP=1
export RUSTFLAGS="-Zsanitizer=thread -L native=$sanitizer_lib -C link-arg=-Wl,-rpath,$sanitizer_lib"
export CARGO_TARGET_DIR=target/tsan
export CARGO_BUILD_JOBS=3
cargo test --locked -Zbuild-std --target aarch64-apple-darwin -p big-boy-pants-js --lib heap:: -- --test-threads=3
cargo test --locked -Zbuild-std --target aarch64-apple-darwin -p big-boy-pants-js --test gc --test gc_roots -- --test-threads=3
```

## Object access without the table lock (2026-10-25)

light-cljs-471 removed the object-table lock from existing-object access: frozen
bodies are read with no lock, mutable bodies under their own slot access word
(contract in [architecture.md](architecture.md#heap-roots-and-safepoints)).
After that change the profile's top frame was the property-key table's read
lock, so name lookups now go through an epoch-validated thread-local cache, and
`GetField`/`Key::lookup` borrow constant names instead of cloning them.

Both binaries came from the same Mac/Rust 1.99.1 workspace release profile on an
Apple M3 Ultra (Virtual) host reporting 24 available CPUs. The baseline is
`b4ec39703e52b25cfe9bf7cc52a9b63d2ad6247e`, built before any source edit. The
final binary is the working tree described in the light-cljs-471 commit. Each
was built with `CARGO_BUILD_JOBS=3 cargo build --locked --release -p
big-boy-pants-js --example bench`, copied aside, and run as `bench scaling`:
full size, one discarded warmup, five measured rounds, four tasks per thread.
No other build or test ran during any timing window. Host background activity
was not controlled. Load averages were `1.98 2.21 2.76` → `8.28 6.55 4.76` for
the baseline run and `2.59 3.10 3.27` → `5.92 4.07 3.62` for the final run.

| Kernel | Threads | Before ns/iteration median [min..max] | After ns/iteration median [min..max] | Before scaling | After scaling |
|---|---:|---:|---:|---:|---:|
| arithmetic | 1 | 311.4 [309.7..315.3] | 337.9 [335.8..339.9] | 1.000x | 1.000x |
| arithmetic | 8 | 43.2 [42.9..44.5] | 43.7 [43.5..44.7] | 7.208x | 7.724x |
| allocate | 1 | 3227198.2 [3163549.7..3287121.4] | 3361345.8 [3353193.8..3384494.0] | 1.000x | 1.000x |
| allocate | 2 | 4343762.8 [4267607.8..4612418.8] | 2070419.9 [2029196.1..2191692.8] | 0.743x | 1.624x |
| allocate | 4 | 10222366.9 [10106138.4..10536385.2] | 1491898.9 [1463117.8..1575597.4] | 0.316x | 2.253x |
| allocate | 8 | 11060169.6 [10928626.1..11161242.4] | 1465033.9 [1459920.6..1482360.0] | 0.292x | 2.294x |
| properties | 1 | 1071.1 [1061.9..1078.5] | 1042.1 [1039.6..1066.0] | 1.000x | 1.000x |
| properties | 2 | 951.1 [920.8..989.0] | 665.6 [649.4..705.7] | 1.126x | 1.566x |
| properties | 4 | 1134.6 [1042.5..1529.5] | 660.3 [652.0..750.6] | 0.944x | 1.578x |
| properties | 8 | 3343.4 [3223.2..3384.8] | 1068.7 [949.4..1136.5] | 0.320x | 0.975x |
| cljs_map | 1 | 5694556.5 [5692872.7..5843830.7] | 5736397.9 [5709430.5..5804557.0] | 1.000x | 1.000x |
| cljs_map | 8 | 23202487.4 [22823951.9..23394430.7] | 1180897.8 [1162437.8..1220608.9] | 0.245x | 4.858x |
| cljs_protocol | 1 | 667301.8 [652064.3..683168.8] | 658736.5 [654191.7..660824.2] | 1.000x | 1.000x |
| cljs_protocol | 8 | 2294287.8 [2282441.6..2306093.9] | 135864.7 [131952.8..141041.8] | 0.291x | 4.848x |

The two- and four-thread arithmetic, cljs_map and cljs_protocol rows are in the
same output files. Final arithmetic scales 1.961x and 3.855x; cljs_map
1.849x and 3.182x; cljs_protocol 1.873x and 3.175x. At eight threads, aggregate
throughput rose 7.6x for allocate (90.4 to 682.6 iterations/s), 3.1x for
properties, 19.6x for cljs_map and 16.9x for cljs_protocol.

**The 6x gate is not met.** Allocate-and-traverse reaches 2.29x and the
property-heavy kernel 0.98x at eight threads. Arithmetic stays near linear.

The unpaired one-thread rows above differ by up to 8.5% (arithmetic, which
touches no heap object). So the single-thread check was repeated as
interleaved A/B pairs right after, three pairs of `bench kernels` alternating
the two copied binaries, with load `4.91 3.93 3.58` → `3.31 3.63 3.49`. Medians
of the three per-run medians, before → after:

| Kernel | Before | After | Change |
|---|---:|---:|---:|
| arithmetic | 332.6 | 338.4 | +1.7% |
| allocate | 3378760.9 | 3391446.4 | +0.4% |
| properties | 1068.5 | 1053.6 | -1.4% |
| cljs_map | 5824774.5 | 5775376.6 | -0.8% |
| cljs_protocol | 665291.7 | 663438.5 | -0.3% |

All single-thread rows are within the 5% limit in the paired comparison.

Remaining contention, from `sample <pid> 5 1` of a separate final-binary
`bench scaling --rounds 1` during the eight-thread allocate and properties
phases (wall-time stacks, not instruction counts):

| Phase | Top self frames (share of 21,032 / 14,426 worker samples) |
|---|---|
| allocate | `dispatch::execute_budgeted` 4,399 (20.9%); `Heap::read` closure in `dispatch::callee` 1,312 (6.2%); `dispatch::Fault` drop 1,030 (4.9%); `step_instruction` 820; `return_flow` 697 |
| properties | `property::Key` drop 2,559 (17.7%); `property_read_step` 1,897 (13.2%); `execute_budgeted` 1,221 (8.5%); `property_step` 1,065 (7.4%); `step_instruction` 951 |

Every `__ulock_wait` sample (2,723 and 3,511) is the main thread in
`pthread_join`. Worker mutex waits total 51 and 2 samples, under realm lookup
and allocation refills. No lock is the serialized operation any more. The
growth in self time on short frames at eight threads, against flat
single-thread cost, points at shared cache lines rather than locks. That
attribution is inferred, not instruction-level proof. The frames that grow
contain atomic reference-count traffic on immutable data shared by every task
running one `Executable`:

- `callee` clones the function's `Arc<Code>` for each call, and frames keep and
  drop it.
- `Key::Name` clones the constant property name's `JsString` once per access.

light-cljs-473 plans to intern name constants per executable. The per-call
`Arc<Code>` traffic is the focused follow-up light-cljs-486 (remove shared code
reference-count traffic from VM calls).

The changed contracts are tested by:

- `heap::storage::tests::object_access_and_publication_complete_while_the_table_lock_is_held`:
  owned reads and writes, frozen reads, liveness, a publication walk and marking
  all finish while another thread holds the table lock.
- `unlocked_access_keeps_owner_faults_and_generation_checks`
- `frozen_bodies_are_lent_mutably_only_as_discarded_copies`
- `owned_and_frozen_access_interleave_with_collections`
- `value::share::tests::attach_and_publication_serialize_on_the_function_access_word`:
  both orders, forced with hooks inside the holder's critical section.
- `object::tests::cached_lookups_follow_sweeps_and_stay_within_their_table`

Validation of the final content used `CARGO_BUILD_JOBS=3` and
`--test-threads=3`:

- `cargo test --locked -p big-boy-pants-js --all-targets` passed **1,975 tests**
  in 59 result groups, with no failures or ignored tests. `--doc` passed all
  **six doctests**.
- Fifty iterations of the compiled test binaries made 600 non-empty runs, all
  passing, for **22,550 test executions**:
  - 550 runs of the complete `atoms`, `cancellation`, `concurrency`
    (multi-worker adversarial suite), `executor`, `executor_gc`, `gc`,
    `gc_roots`, `promises`, `publication`, `safepoints` and `wakeup` binaries:
    279 tests per iteration, 13,950 executions.
  - 50 runs of the lib binary filtered to `heap:: value::share atom:: task::`,
    covering root inventory, storage, GC, publication, atoms, promises/jobs,
    cancellation and handoff. Every iteration reported 172 passed, 0 failed,
    0 ignored and 358 filtered out: 8,600 executions.
  - A first lib loop passed the four filters as one quoted zsh scalar. libtest
    received a single unmatched filter and ran zero tests 50 times. Those empty
    runs are excluded from the counts above. The lib repetitions were then
    rerun on the unchanged final source, with the filters passed as separate
    words, from `crates/big-boy-pants-js/`:

    ```sh
    for i in {1..50}; do ../../target/debug/deps/big_boy_pants_js-<hash> heap:: value::share atom:: task:: --test-threads=3 || break; done
    ```

    Check each run's `test result:` line for the expected nonzero count.
- `cargo fmt -p big-boy-pants-js -- --check` passed. Strict
  `cargo clippy --locked -p big-boy-pants-js --all-targets -- -D warnings`
  still fails with only the same nine pre-existing diagnostics (`deprecated`,
  `clippy::chunks_exact_to_as_chunks`, `clippy::byte_char_slices`). Allowing
  only those three categories passes. No source suppression was added for
  those nine diagnostics. The change does add two narrow attributes in
  `heap/storage.rs`. `Slot::body_mut` carries `#[allow(clippy::mut_from_ref)]`
  at the `UnsafeCell` boundary; it is an `unsafe fn` whose documented
  precondition is that the caller holds the slot's access word and observed
  the slot live and not frozen. The compile-time `Sync` assertion helper
  `freezable_kinds_are_sync`, which is never called, carries
  `#[allow(dead_code)]`.
- ThreadSanitizer, with the commands of the previous section, passed the lib
  filters `heap:: value::share object::tests::` (**77 tests**, passed after
  `--`). It also passed the `gc`, `gc_roots`, `publication` and `atoms` binaries
  (**164 tests**). There were no race reports.

## Task-local call code ownership (2026-10-25)

light-cljs-486 gives frames an owning, task-local reference count around the
shared immutable executable. A bounded identity cache reuses these owners;
ordinary calls, native callbacks, constructors and dispatch-frame switches no
longer clone the globally shared code handle on each transition. Callable
probes copy constructor metadata, and repeated constructor prototype reads do
not retain code in property snapshots. Function creation still takes a shared
code reference. The lifetime/rooting contract is in
[architecture.md](architecture.md#embedding); no unsafe code was added.

Both binaries used the same release profile on the Apple M3 Ultra (Virtual),
macOS aarch64 host reporting 24 available CPUs. The baseline is `a15d53f7`;
the final code is `78e0ccf9`. Each was built using `CARGO_BUILD_JOBS=3 cargo
build --locked --release -p big-boy-pants-js --example bench` and copied aside.
`bench scaling` used full sizes, one discarded warmup, five measured rounds
and four tasks per thread. No concurrent build or test ran during any timing
or sampling window. Host background activity was not controlled. Load averages
were `2.92 3.29 3.44` → `5.90 4.04 3.70` before and
`5.90 4.04 3.70` → `5.70 4.33 3.83` after.

| Kernel | Threads | Before ns/iteration median [min..max] | After ns/iteration median [min..max] | Before scaling | After scaling |
|---|---:|---:|---:|---:|---:|
| arithmetic | 1 | 339.2 [336.8..344.8] | 335.5 [335.0..337.1] | 1.000x | 1.000x |
| arithmetic | 2 | 172.0 [170.7..173.4] | 170.4 [169.3..172.6] | 1.972x | 1.968x |
| arithmetic | 4 | 87.3 [86.7..88.0] | 85.9 [85.8..86.5] | 3.887x | 3.905x |
| arithmetic | 8 | 44.8 [43.8..46.1] | 43.4 [43.2..45.0] | 7.575x | 7.724x |
| allocate | 1 | 3369743.5 [3356549.7..3406196.9] | 3512637.2 [3506870.6..3535788.5] | 1.000x | 1.000x |
| allocate | 2 | 2072603.6 [2038643.4..2126765.0] | 1888249.6 [1875128.6..1927022.4] | 1.626x | 1.860x |
| allocate | 4 | 1495310.7 [1472086.3..1539830.0] | 1116576.2 [1108065.4..1122500.7] | 2.254x | 3.146x |
| allocate | 8 | 1511503.5 [1469594.6..1516876.5] | 842235.0 [807246.9..866222.0] | 2.229x | 4.171x |
| properties | 1 | 1056.7 [1044.4..1105.7] | 1042.2 [1035.0..1075.8] | 1.000x | 1.000x |
| properties | 2 | 654.3 [649.7..706.4] | 657.6 [649.6..679.5] | 1.615x | 1.585x |
| properties | 4 | 545.1 [540.7..566.4] | 564.8 [557.1..605.7] | 1.939x | 1.845x |
| properties | 8 | 672.8 [630.8..757.6] | 682.2 [656.5..741.9] | 1.571x | 1.528x |
| cljs_map | 1 | 5731282.8 [5711793.0..5821765.6] | 5900252.1 [5848978.4..5920530.2] | 1.000x | 1.000x |
| cljs_map | 2 | 3122659.0 [3055502.0..3148778.8] | 3096581.0 [3058940.8..3136930.5] | 1.835x | 1.905x |
| cljs_map | 4 | 1770133.0 [1759038.0..1827922.3] | 1671555.1 [1659149.9..1694137.8] | 3.238x | 3.530x |
| cljs_map | 8 | 1171078.6 [1167948.1..1178075.4] | 964270.2 [963657.0..974868.3] | 4.894x | 6.119x |
| cljs_protocol | 1 | 653833.3 [651838.5..659684.4] | 674796.9 [672929.4..680066.7] | 1.000x | 1.000x |
| cljs_protocol | 2 | 355892.1 [349856.2..428930.9] | 361439.3 [348163.0..375207.0] | 1.837x | 1.867x |
| cljs_protocol | 4 | 208198.2 [203730.1..209253.8] | 198099.9 [197812.8..200874.2] | 3.140x | 3.406x |
| cljs_protocol | 8 | 138106.0 [137140.2..140085.7] | 127185.8 [123198.0..132255.7] | 4.734x | 5.306x |

**The 6x allocation gate is still missed.** Eight-thread allocate-and-traverse
scaling rose from **2.229x to 4.171x**, and aggregate throughput from 661.6 to
1,187.3 iterations/s (**1.795x**, or +79.5%). Properties stayed around 1.5x
scaling (1.571x before, 1.528x after), with overlapping timing ranges.

The controlled change removes shared code-count updates while retaining the
same executable, runtime, workloads and dispatch/budget policy. Its large
multi-thread gain supports shared reference-count contention as a material
cost. This is a controlled comparison, not instruction-level attribution or
proof that every remaining plateau is cache-line contention.

Immediately afterward, three interleaved A/B pairs of `bench kernels` used
the same full-size, five-round settings. Medians of the three per-run medians
give the paired single-thread check; load was `5.70 4.33 3.83` →
`3.41 3.91 3.72`:

| Kernel | Before ns/iteration | After ns/iteration | Change |
|---|---:|---:|---:|
| arithmetic | 337.5 | 334.2 | -0.98% |
| allocate | 3359383.3 | 3522639.6 | +4.86% |
| properties | 1048.4 | 1036.1 | -1.17% |
| cljs_map | 5744834.1 | 5869623.4 | +2.17% |
| cljs_protocol | 658256.5 | 664179.2 | +0.90% |

All paired rows meet the no-more-than-5% regression bound. The allocation
row is close to that limit: task-local ownership adds a cache lookup and an
extra owning indirection in exchange for removing shared count contention.

A separate final-binary `bench scaling --rounds 10` supplied profiles.
`sample <pid> 2 1 -file <output>` started immediately after each four-thread
allocate/properties row, sampling its eight-thread phase for two seconds at
one-millisecond intervals. Profile-run timings are excluded from the tables
above. Self samples below exclude the main thread, including its joins and
between-round teardown:

| Phase | Top self frames (share of 8,087 / 10,427 worker samples) |
|---|---|
| allocate | `execute_budgeted` 918 (11.35%); `Fault` drop 550 (6.80%); `step_instruction` 399 (4.93%); `native::start_conversion` 286 (3.54%); `OrdinaryObject::position` 255 (3.15%) |
| properties | `Key` drop 2,243 (21.51%); `property_read_step` 1,648 (15.81%); `property_step` 923 (8.85%); `execute_budgeted` 734 (7.04%); `step_instruction` 577 (5.53%) |

The allocation profile has `cached_callee` at 98 self samples (1.21%); the
old shared-code-cloning callee closure is gone. The remaining dispatch,
conversion and ordinary property work is already scoped by **light-cljs-474**
and **light-cljs-473**, respectively. Those existing focused follow-ups cover
the missed gate; no duplicate pellet or additional optimization is introduced
here. Neither wall-time sampling nor overlapping property ranges justify a
new claim that the property plateau was fixed by code ownership.

Validation at `78e0ccf9` used Homebrew Rust/Cargo **1.99.1**, with
`CARGO_BUILD_JOBS=3` and `--test-threads=3`:

- `cargo test --locked -p big-boy-pants-js --all-targets` passed **1,977 tests**
  in 58 result groups, with no failures or ignored tests. `--doc` passed all
  **six doctests**.
- Fifty iterations of the compiled `calls` (16), `gc` (51), `gc_roots` (70),
  `cancellation` (3), `wakeup` (4), and `executor_gc` (5) binaries, plus `need`
  filtered to `generator` (3) and the lib binary filtered to
  `vm::frames::tests::` (7), passed **7,950 test executions** in 400 non-empty
  runs. These cover nested calls, collection at each instruction, dependency
  waits, migration, cancellation and detached generator/async roots. The new
  unit regressions hold 1,000 cached handles without changing the shared code
  count, distinguish different tasks' owners, and retain a detached
  activation through cache eviction, owner destruction and thread migration.
- `cargo fmt -p big-boy-pants-js -- --check` passed. Strict
  `cargo clippy --locked -p big-boy-pants-js --all-targets -- -D warnings`
  reports only the nine existing diagnostics in `deprecated`,
  `clippy::chunks_exact_to_as_chunks`, and `clippy::byte_char_slices`.
  Adding `-A` for only those three categories passes; no source suppressions
  were added.

## Shapes and named-property caches (2026-10-25)

light-cljs-473 adds immutable ordinary-object layouts, per-instruction atomic
shape/slot caches, and named constants classified at executable load. Runtime
keys bind lazily because `Executable::load` is runtime independent; the binding
includes a runtime/collection stamp so reclaimed key slots cannot alias a stale
cache. Cache hits read live descriptors, and prototype reads still walk the live
chain. Bounded worker-local transition caches and local owning shape handles
avoid global locks/reference-count writes during repeated construction. See
[the object/cache contract](architecture.md#ordinary-objects-and-named-property-caches).

The baseline is **`d26073c8`**, including the preceding allocation, access and
call-ownership work. The measured implementation is **`ffab5053`**. Both used
`CARGO_BUILD_JOBS=3 cargo build --locked --release -p big-boy-pants-js --example bench`
and saved binaries, on the same Apple M3 Ultra (Virtual), macOS 15.7.3 aarch64
host with 24 reported CPUs and Homebrew Rust/Cargo 1.99.1. No concurrent build,
test or other worker benchmark ran during the timing/profile windows. Host
background activity was not controlled.

Three interleaved baseline/implementation pairs ran `bench kernels --rounds 5`
at full sizes, with one discarded warmup and five measured rounds per row.
These are medians of the three per-run medians; the last column shows the range
of the three paired percentage changes. Load averages were `2.98 3.65 3.64`
at the first baseline's start and `2.98 3.46 3.56` at the last implementation's end.

| Kernel | Before ns/iteration | After ns/iteration | Change | Paired changes |
|---|---:|---:|---:|---:|
| arithmetic (control) | 335.1 | 338.3 | +0.95% | +0.42%..+1.14% |
| allocate | 3549346.9 | 3437718.0 | -3.15% | -3.37%..-2.71% |
| properties | 1044.1 | 907.1 | -13.12% | -13.64%..-13.10% |
| cljs_map | 5875324.5 | 5728587.0 | -2.50% | -3.19%..-2.29% |
| cljs_protocol | 666217.7 | 669277.6 | +0.46% | -0.60%..+0.46% |

The following separate baseline/implementation `bench scaling --rounds 5`
pair used full sizes and four tasks per thread. Load averages were
`2.98 3.46 3.56` → `5.10 3.99 3.75` before and
`5.10 3.99 3.75` → `4.79 4.11 3.81` after.

| Kernel | Threads | Before ns/iteration median [min..max] | After ns/iteration median [min..max] | Before scaling | After scaling |
|---|---:|---:|---:|---:|---:|
| arithmetic | 1 | 334.3 [333.6..347.1] | 334.6 [333.7..337.7] | 1.000x | 1.000x |
| arithmetic | 2 | 170.5 [169.5..171.1] | 170.1 [169.7..172.6] | 1.960x | 1.967x |
| arithmetic | 4 | 86.6 [85.9..86.9] | 85.9 [85.8..86.0] | 3.860x | 3.894x |
| arithmetic | 8 | 43.5 [43.3..46.8] | 43.3 [43.1..45.7] | 7.683x | 7.731x |
| allocate | 1 | 3515224.0 [3487753.4..3539073.4] | 3416861.7 [3393259.6..3436401.3] | 1.000x | 1.000x |
| allocate | 2 | 1915297.5 [1871655.2..2015674.0] | 1789652.7 [1780443.6..1808765.5] | 1.835x | 1.909x |
| allocate | 4 | 1134613.3 [1122712.1..1158471.7] | 1007772.5 [954205.3..1037512.0] | 3.098x | 3.391x |
| allocate | 8 | 829520.1 [817311.8..853489.7] | 626034.5 [607629.6..652319.8] | 4.238x | 5.458x |
| properties | 1 | 1043.4 [1040.7..1089.8] | 906.4 [905.8..910.3] | 1.000x | 1.000x |
| properties | 2 | 674.8 [641.7..728.4] | 460.9 [457.1..465.9] | 1.546x | 1.967x |
| properties | 4 | 669.2 [627.7..719.3] | 232.0 [230.9..232.3] | 1.559x | 3.907x |
| properties | 8 | 991.9 [959.4..1044.5] | 126.6 [116.8..134.2] | 1.052x | 7.158x |
| cljs_map | 1 | 5908490.9 [5874848.7..5938452.1] | 5682248.7 [5660056.8..5711738.5] | 1.000x | 1.000x |
| cljs_map | 2 | 3104429.2 [3066865.5..3144851.2] | 2963928.9 [2948061.7..2993671.6] | 1.903x | 1.917x |
| cljs_map | 4 | 1679671.0 [1667076.8..1727339.3] | 1573317.8 [1559218.5..1596236.7] | 3.518x | 3.612x |
| cljs_map | 8 | 971862.2 [966463.9..990853.5] | 891196.8 [881893.3..901006.4] | 6.080x | 6.376x |
| cljs_protocol | 1 | 674785.7 [671043.0..683121.6] | 666353.1 [663665.6..670429.2] | 1.000x | 1.000x |
| cljs_protocol | 2 | 362722.9 [360180.6..406245.7] | 361193.1 [358171.5..458797.9] | 1.860x | 1.845x |
| cljs_protocol | 4 | 198264.4 [193584.2..202722.9] | 201024.0 [198821.5..204179.4] | 3.403x | 3.315x |
| cljs_protocol | 8 | 124068.7 [121283.4..127777.5] | 123775.4 [122334.3..126507.5] | 5.439x | 5.384x |

**The local acceptance wording is not fully met.** Allocation, property-heavy
and emitted-map single-thread rows improve, but the paired protocol result does
not demonstrate improvement. Its four-thread scaling median is 1.39% slower,
with overlapping ranges; a strict all-row nonregression claim is not supported.
Arithmetic is a control, not an optimization result. The earlier 6x allocation
gate also remains missed at 5.458x. No control-based normalization turns these
misses into successes.

A separate final-binary `bench kernels --rounds 20` supplied a protocol profile.
Immediately after the emitted-map row, `sample <pid> 2 1 -file <output>` sampled
the protocol phase for two seconds at one-millisecond intervals. Those timings
are excluded above. Subtracting direct-child counts from the call tree and
excluding the main thread gave **1,339 worker self samples**:

| Frame | Self samples | Share |
|---|---:|---:|
| `execute_budgeted` | 131 | 9.78% |
| `Fault` drop glue | 77 | 5.75% |
| `step_instruction` | 77 | 5.75% |
| `memmove` | 62 | 4.63% |
| `native::start_conversion` | 41 | 3.06% |
| `property::lookup_cached` | 22 | 1.64% |

The bbp-integration group's numeric-miss rule calls for numbers, remaining top
frames and a focused follow-up. Existing **light-cljs-474**, "Cut per-instruction
interpreter overhead to within 3x of V8's interpreter", explicitly covers
`execute_budgeted`, `step_instruction`, success-path `Fault` drops and native
conversion startup. It is the nonduplicate follow-up for these remaining
dispatch costs; this change does not alter its budgets, safepoints or native
call setup, and does not claim the protocol target is achieved.

Validation of the final implementation used `CARGO_BUILD_JOBS=3` and
`--test-threads=3`: **2,001 engine tests across 60 result groups passed**, with
no failures or ignored tests, including the QuickJS-derived corpus. The six
cache regressions cover shape/deletion/order changes, dictionary fallback,
current descriptors/accessors, prototype/method replacement, concurrent tasks
on different noninitial slots, concurrent runtimes, and forced GC/key reuse.
The final concurrent-layout fixture was also rerun separately (six passed).
All six doctests and the formatter passed. Strict Clippy retains the nine pre-existing diagnostics in
`deprecated`, `clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices`;
allowing only those categories passes, with no source suppressions.

## Primitive instruction batching (2026-10-25)

light-cljs-474 adds immutable poll/run metadata and a guarded primitive
dispatcher. Numeric operations avoid generic conversion startup and success-path
`Fault` results; local/constant copies retain the existing cheap primitive
payload copies. A run charges completed instructions in batches capped by the
remaining budget. Branches end batches, backward branches begin at their poll,
and conversion misses leave operands and pc unchanged for the generic path.
The direct ordinary-bytecode frame-entry path already delivered by earlier
work remains in use. No serialized opcode or value ownership format changed.
See [the execution contract](architecture.md) for the unchanged safepoints and
cancellation boundaries.

The baseline is **`5059ce9c`**, including 469/470/471/473 and call ownership;
the measured implementation is **`fb580226`**. Both used the workspace release
profile (thin LTO, one codegen unit), Homebrew Rust/Cargo **1.99.1**, macOS
**15.7.3**, Apple M3 Ultra (Virtual), 24 available threads, and Node **26.12.1**.
No builds, tests or peer benchmarks ran during these timing/profile windows.
Each binary ran `bench kernels reads --rounds 5`: one discarded warmup and five
measured rounds, full sizes, four tasks per kernel round, one worker. Node ran
the exact source emitted by `bench --node-kernels kernels --rounds 5` through
`node --jitless`, with matching sizes/warmup/rounds. These are one sequential
before/after pair, not repeated paired trials. Load averages were
`2.52 2.96 4.05` → `2.23 2.86 3.99` before and
`2.23 2.86 3.99` → `2.28 2.85 3.97` after.

| Kernel | Before ns/iteration median [min..max] | After ns/iteration median [min..max] | Node jitless median [min..max] | After / Node |
|---|---:|---:|---:|---:|
| arithmetic | 333.7 [332.5..335.1] | 88.7 [87.9..89.3] | 9.6 [9.5..9.6] | 9.24x |
| allocate | 3427026.8 [3409437.2..3512381.0] | 2864674.7 [2853468.5..2894785.2] | 136767.4 [133227.9..139207.3] | 20.95x |
| properties | 901.5 [898.5..905.5] | 479.9 [478.7..485.0] | 36.4 [34.0..36.9] | 13.18x |
| cljs_map | 5687601.8 [5674896.6..5712793.0] | 4392302.1 [4354116.7..4484907.8] | 340843.2 [336278.4..352767.2] | 12.89x |
| cljs_protocol | 660625.0 [653450.0..666082.6] | 599737.8 [574167.7..615270.3] | 38327.9 [36299.5..46473.4] | 15.65x |

The simpler `reads.js_baseline_loop` fell from **302.9 [302.4..305.2]** to
**65.8 [65.5..66.7] ns/iteration**. `reads.js_need_loop` fell from
435.8 [434.4..437.8] to 148.4 [147.5..149.4]; the Rust `RunGuard::need` control
was 53.8 [53.4..55.3] before and 54.7 [53.9..54.9] after.

Separate `bench kernels --rounds 20` runs supplied profiles, excluded from
the timings above. `sample <pid> 1 1` began at launch during arithmetic;
`sample <pid> 2 1` began immediately after the arithmetic row during the
allocation/call-heavy kernel. Worker self samples below subtract immediate
child counts in the call tree and exclude the waiting main thread. Arithmetic
had 750 before / 753 after samples; allocation had 1,401 before / 1,389 after.
Inlining attributes the primitive implementation to `execute_budgeted`.

| Kernel / frame | Before self samples | After self samples |
|---|---:|---:|
| arithmetic / `execute_budgeted` | 143 (19.07%) | 613 (81.41%) |
| arithmetic / `step_instruction` | 118 (15.73%) | 0 |
| arithmetic / `native::start_conversion` | 110 (14.67%) | 0 |
| arithmetic / `Fault` drop | 83 (11.07%) | 0 |
| arithmetic / `fmod` | 73 (9.73%) | 134 (17.80%) |
| allocate / `execute_budgeted` | 200 (14.28%) | 254 (18.29%) |
| allocate / `Fault` drop | 97 (6.92%) | 92 (6.62%) |
| allocate / `memmove` | 49 (3.50%) | 53 (3.82%) |
| allocate / `enter_constructor` | 42 (3.00%) | 44 (3.17%) |
| allocate / heap `Directory::slot` | 34 (2.43%) | 44 (3.17%) |

**The 3x target remains missed for every kernel.** The explicit numeric-miss
fallback is used: these ratios and remaining frames are recorded, and focused
follow-up **light-cljs-487** investigates guarded fusion to reduce primitive
operand-stack traffic. It does not claim arithmetic fusion will eliminate the
remaining call, constructor, property or allocation costs.

Validation of the unchanged implementation at `0942c89e` used
`CARGO_BUILD_JOBS=3` and `--test-threads=3`: `cargo test --locked -p
big-boy-pants-js --all-targets` passed **2,002 tests in 60 result groups**,
including the QuickJS-derived corpus; `--doc` passed **six doctests**. The new
regression compares generic and optimized execution at every short budget
prefix, including saved pc/locals/operands, numeric edge cases and conversion
fallback. The affected call/budget/safepoint/GC/cancellation/native/dependency
suites also passed separately (300 tests). Engine formatting passes. Strict
Clippy reports only the unchanged nine diagnostics in `deprecated`,
`clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices`; allowing only
those categories passes, with no source suppressions. Rebase onto `5059ce9c`
was a no-op, so these checks were reused rather than repeated.

## Guarded numeric instruction fusion (2026-10-25)

light-cljs-487 derives bounded numeric expressions from the unchanged bytecode:
two local/constant operands, optionally one nested arithmetic operation, and an
optional local store or forward conditional branch. All operands must already
be Numbers and the entire expression must fit the remaining instruction budget.
A miss or a short slice executes the original instructions, including cuts
inside the expression. Intermediate numeric operands bypass the Value stack;
conversion, call, native and safepoint behavior is unchanged.

The baseline is **`3ee7a407`**; the measured production implementation is
**`6b934c4c`** (`635369ff` adds architecture documentation only). Both use the
workspace release profile (thin LTO, one codegen unit), Homebrew Rust/Cargo
**1.99.1**, macOS **15.7.3**, Apple M3 Ultra (Virtual), 24 available threads,
and Node **26.12.1**. The before and after binaries each ran
`bench kernels reads --rounds 5`, full sizes, four tasks and one worker per
kernel round, one discarded warmup and five measured rounds. Node ran the
exact source emitted by `bench --node-kernels kernels --rounds 5` with
`node --jitless`. No builds, tests or peer benchmarks ran during this window.
This is one sequential before/after pair, not repeated paired trials.
Load averages were `3.34 4.05 3.66` before, `3.05 3.96 3.63` between binaries,
and `2.97 3.91 3.62` after both the optimized binary and Node.

| Kernel | Before ns/iteration median [min..max] | After ns/iteration median [min..max] | Node jitless median [min..max] | After / Node |
|---|---:|---:|---:|---:|
| arithmetic | 88.9 [88.4..90.0] | 43.0 [42.1..43.9] | 9.4 [9.4..9.5] | 4.57x |
| allocate | 2878922.7 [2871640.4..2919474.2] | 2874045.1 [2851552.9..2908085.4] | 131902.6 [131247.4..134496.4] | 21.79x |
| properties | 479.0 [474.3..492.9] | 487.0 [483.8..496.2] | 35.3 [34.7..35.9] | 13.80x |
| cljs_map | 4391359.1 [4348997.7..4448382.5] | 4476821.9 [4428561.2..4545083.6] | 334351.6 [325286.5..339460.2] | 13.39x |
| cljs_protocol | 595379.2 [589367.4..630926.3] | 593066.9 [588804.2..598216.9] | 34753.6 [31398.7..44243.2] | 17.06x |

Arithmetic improves **2.07x**, but these results do **not** demonstrate a broad
compiler-kernel improvement: allocation and protocols change by −0.17% and
−0.39%, while properties and emitted maps regress by +1.67% and +1.95% in
this pair. The simple `reads.js_baseline_loop` improves from
**65.3 [65.1..67.6]** to **22.1 [21.7..22.9] ns/iteration**.
`reads.js_need_loop` falls from 148.6 [146.9..149.1] to
110.4 [109.4..111.5]; the Rust `RunGuard::need` control is
54.4 [53.6..55.4] before and 54.1 [53.6..55.1] after.

A separate final `bench kernels --rounds 40` supplied profiles, excluded from
the timings. `sample <pid> 1 1` started at launch during arithmetic;
`sample <pid> 2 1` started immediately after the arithmetic row during
allocation. Self counts subtract immediate children in each call tree and
exclude the main thread, including joins and runtime teardown. There are
**755 arithmetic** and **1,390 allocation worker samples**. Inlining attributes
fusion to `execute_budgeted`.

| Kernel / remaining frame | Self samples | Share |
|---|---:|---:|
| arithmetic / `execute_budgeted` | 404 | 53.51% |
| arithmetic / `fmod` | 347 | 45.96% |
| allocate / `execute_budgeted` | 240 | 17.27% |
| allocate / `Fault` drop | 114 | 8.20% |
| allocate / `enter_constructor` | 63 | 4.53% |
| allocate / `memmove` | 54 | 3.88% |
| allocate / heap `Directory::slot` | 49 | 3.53% |

**The 3x jitless target remains missed for every kernel.** The group numeric-miss
fallback is explicit: focused follow-up **light-cljs-488** investigates guarded
integral remainder, whose general `fmod` path now accounts for nearly half the
arithmetic samples. It does not promise improvement to call/allocation-heavy
compiler kernels or duplicate the native collections/protocol work in 477/478.

Validation at `6b934c4c`, with `CARGO_BUILD_JOBS=3` and `--test-threads=3`, passed
**2,005 tests across 60 result groups** with `cargo test --locked -p
big-boy-pants-js --all-targets`, including budget, cancellation, GC, safepoint,
native, conversion and resume coverage, plus **seven doctests** with `--doc`.
The differential regression checks every budget prefix through completion and
resumes each cut, including conversion callbacks and throws. Another checks
all 121 pairs of eleven IEEE-number edge values across ten expressions and
every short repeated slice, including serialized executable reloads. Engine
formatting passes. Strict Clippy reports only the existing nine diagnostics in
`deprecated`, `clippy::chunks_exact_to_as_chunks` and `clippy::byte_char_slices`;
allowing only those categories passes, without source suppressions. These
checks remain applicable to the following documentation-only commits.

### Guarded integral Number remainder (2026-10-25)

light-cljs-488 replaces floating-point remainder in the primitive and fused
paths only when both Number operands round-trip exactly through `i32` and the
divisor is nonzero. Integer remainder is then exact; `wrapping_rem` handles
`i32::MIN % -1`, and `copysign` preserves the dividend's negative zero.
Fractions, larger Numbers, NaN, infinities and zero divisors keep the general
f64 path. The generic dispatcher, BigInt/conversion fallback, bytecode, budgets,
fusion boundaries, roots and polls are unchanged.

Baseline **`176c2bf1`** and implementation **`c72d54cd`** were built separately
with the workspace release profile (thin LTO, one codegen unit), on the same
Apple M3 Ultra (Virtual), macOS **15.7.3**, Homebrew Rust/Cargo **1.99.1**,
24 available threads, and Node **26.12.1**. An orchestrator-reserved quiet
window excluded peer builds, tests and benchmarks. Each saved binary ran
`bench kernels reads --rounds 5`, full sizes, four tasks and one worker per
kernel round, with one discarded warmup and five measured rounds. Node ran
the exact source from `bench --node-kernels kernels --rounds 5` using
`node --jitless`. This is one sequential before/after pair, not repeated paired
trials. Load averages were `4.14 5.81 5.66` before, `3.82 5.69 5.62` between
binaries, and `3.54 5.56 5.57` after the implementation run.

| Kernel | Before ns/iteration median [min..max] | After ns/iteration median [min..max] | Node jitless median [min..max] | After / Node |
|---|---:|---:|---:|---:|
| arithmetic | 44.3 [42.8..44.8] | 21.8 [21.5..22.8] | 9.8 [9.8..10.3] | 2.22x |
| allocate | 2912403.1 [2891740.1..2947070.1] | 2922045.6 [2877829.2..2932777.3] | 139937.2 [138851.0..141245.6] | 20.88x |
| properties | 491.6 [478.8..495.0] | 481.8 [479.3..489.6] | 38.1 [35.7..43.2] | 12.65x |
| cljs_map | 4514382.6 [4473398.7..4600451.0] | 4477008.9 [4461394.8..4595574.0] | 343786.2 [338455.2..359535.7] | 13.02x |
| cljs_protocol | 601236.7 [598409.1..606631.8] | 611843.5 [595828.4..619191.1] | 37298.4 [34488.8..46408.1] | 16.40x |

Arithmetic improves **2.03x and meets the 3x jitless target**. The other four
kernels still miss it: allocation and protocols regress by **+0.33%** and
**+1.76%**, while properties and emitted maps improve by **1.99%** and
**0.83%** in this pair. These mixed small changes do not demonstrate a broad
compiler-kernel improvement. Existing native collections/protocol work in
477/478 covers those remaining costs; no duplicate follow-up is created.
The reads control is likewise essentially unchanged: `js_baseline_loop`
22.4 [22.0..23.2] to 22.8 [22.0..44.8], `js_need_loop`
115.4 [113.4..116.9] to 114.7 [113.6..116.7], and Rust `RunGuard::need`
54.6 [54.3..55.1] to 55.0 [54.4..55.5] ns/iteration or ns/read respectively.

A separate final-binary `bench kernels --rounds 80` supplied profiles and is
excluded from those timings. The longer run keeps arithmetic active throughout
`sample <pid> 1 1` at launch; `sample <pid> 2 1` starts immediately after its
arithmetic row during allocation. Self counts subtract immediate children in
each call tree and exclude the main thread, including joins and runtime
teardown. There are **753 arithmetic** and **1,414 allocation worker samples**.
Inlining attributes the numeric path to `execute_budgeted`.

| Kernel / remaining frame | Self samples | Share |
|---|---:|---:|
| arithmetic / `execute_budgeted` | 751 | 99.73% |
| arithmetic / `fmod` | 0 | 0.00% |
| allocate / `execute_budgeted` | 237 | 16.76% |
| allocate / `Fault` drop | 85 | 6.01% |
| allocate / `memmove` | 59 | 4.17% |
| allocate / `enter_constructor` | 56 | 3.96% |
| allocate / `nanov2_free` | 46 | 3.25% |
| allocate / heap `Directory::slot` | 45 | 3.18% |

Validation at `c72d54cd`, with `CARGO_BUILD_JOBS=3` and `--test-threads=3`,
passes **2,007 tests across 60 result groups** with
`cargo test --locked -p big-boy-pants-js --all-targets` and **seven doctests**
with `--doc`. The direct remainder regression checks bit-exact non-NaN results
for all 841 pairs of 29 edge values and 40,000 deterministic integer/IEEE
pairs. The generic f64 dispatcher remains an independent oracle for every
budget prefix, resumed cuts, and repeated short fusion slices, with 26 Number
edge values for remainder expressions, BigInt errors, conversion callbacks
and throws. Engine formatting passes. Strict Clippy reports the unchanged
nine diagnostics in `deprecated`, `clippy::chunks_exact_to_as_chunks` and
`clippy::byte_char_slices`; allowing only those categories passes without
source suppressions. These checks also cover the following documentation-only
measurement commit.

## Method and limits of the measurements

`examples/bench/main.rs` handles timing, a counting allocator and reports;
`examples/bench/workloads.rs` supplies workloads shared with
`tests/measurement.rs`. JavaScript in `examples/bench/` and `examples/e2e/work.js`
is synthetic fixture code, not light-cljs output. No benchmark dependency or
C-engine wrapper is used. QuickJS `tests/microbench.js` at the attributed
revision informed only the named-row/empty-loop reporting style; no workload
or score is copied from it.

The separate `tests/light_cljs.rs` compatibility gate now runs real emitted
programs and the current compiler runtime factory. Its checked map, vector and
protocol loops are available in `tests/fixtures/light-cljs/` with provenance and
expected results. They have not been substituted into the historical benchmark
rows below, and passing them is not a timing or scaling measurement.

The compatibility pass on macOS with Rust 1.99.1 ran 739 tests successfully
across `light_cljs`, `map_iterables`, `object_statics`, `quickjs_collections`,
`sets`, `js_source`, `exceptions`, `exception_waits` and `need`, using
`cargo test --locked -p big-boy-pants-js` with those `--test` arguments and
`-- --test-threads=3`. The full `--all-targets` command aborted in
`frontend::tests::deep_nesting_is_rejected_without_overflowing_the_stack` with
a native stack overflow. Running that exact test alone with `--lib`,
`-- --exact --test-threads=3` also aborted on untouched `074706d4` with the same
toolchain. This is a pre-existing stack-safety limitation, not a passing suite.

Strict `cargo clippy --locked -p big-boy-pants-js --all-targets -- -D warnings`
reported the same nine diagnostics on that baseline and this pass: deprecated
`Atomic::fetch_update`, `clippy::chunks_exact_to_as_chunks`, and
`clippy::byte_char_slices` in existing symbol, intrinsic and Intl code. Allowing
only those three categories after `-D warnings` passed; no lint suppression was
added to the source. The package's `cargo fmt -- --check` and doctests passed.

Timing rows use one discarded warmup, then five measured rounds (three in quick
mode), reporting median and [min..max]. Each round uses a fresh Runtime, with
setup outside the interval: compilation, input building, task spawning except
in startup rows, and pre-completion of ready cells. Results are checked against
Rust-computed values after timing. Rows pairing a JS intrinsic loop with an
arithmetic baseline report the difference as an estimate, not isolated opcode
cost. Most rows run on one embedder thread; `work` uses the reference executor.

The counting allocator is enabled only for memory/count passes; it counts
requested bytes rather than allocator overhead. Memory rows run once. Shared
input/code are measured once; per-task spawn and park bytes include task state,
root registration, waiter entries, private heap objects and table growth.
`kept_B/t` is capacity retained after task drop and collection. Host `Vec<Task>`
storage is excluded from per-task figures. `HeapUsage` snapshots count live
objects/table capacity and shallow owned bytes, excluding trie bodies, code,
atom cells and task/native state. `TaskStats` gives shallow saved-state estimates,
not total retained memory. RSS includes previous row groups, allocator caching
and everything else in the process. A memory-only run avoids earlier update
rows dominating high-water RSS.

Statistics are read-only and not free: heap_usage walks object slots under the
object lock; Task::stats briefly locks saved task state and returns None while
a run holds it. The report records CPU/profile/parallelism and start/end load.
Variation on a shared machine prevents close performance comparisons. Counting
allocation changes timing, so ordinary timing rows leave it off.

Not isolated here: cross-thread wake latency, heavily contended heap/cell/atom
operations, collection pauses under load, needAll retained memory, actual compiler
startup/workloads or comparisons with another engine. Explicit GC means persistent
update garbage remains allocated until collection. First rope flattening is
synchronous; the following unit reads reuse the cache. Rope release is outside
the timing interval. Runtime resource/policy caps are in
[grammar.md](grammar.md#resource-and-policy-limits).

## Current representative run

Recorded 2026-10-25 UTC at code baseline `72201c8b`, before this documentation
rewrite; no runtime code changed. Linux 6.18.57 x86_64, AMD Ryzen 7 9850X3D,
Rust 1.95.0, optimized release build. The harness reports 14 available logical
CPUs; this pass had no peer workers and capped Cargo jobs at three. Full mode,
one warmup plus five measured rounds, two executor workers. Load averages at
start/end: 2.32/1.68 (one-minute values). Full invocation including its cold
release compilation took 66 seconds. Historical numbers from earlier layouts
are superseded by this run, not treated as a matched performance comparison.

| Row | Median [min..max] | Unit / workload |
| --- | ---: | --- |
| startup.spawn | 124.3 [124.2..125.1] | ns/task, 20,000 tasks |
| startup.first_run_to_complete | 232.0 [230.9..232.3] | ns/task |
| reads.js_need_loop | 472.9 [472.7..474.8] | ns/iteration, 1,000,000 reads |
| reads.js_baseline_loop | 325.8 [323.2..326.7] | ns/iteration |
| reads.js_need_minus_baseline | 146.9 [146.2..151.6] | ns/read estimate |
| reads.rust_runguard_need | 90.6 [90.6..91.2] | ns/read |
| park.full_cycle | 924.7 [915.5..925.2] | ns/cycle, 100,000 cycles |
| park.complete_and_wake | 122.9 [122.6..123.1] | ns/cycle |
| persistent.vec_nth_minus_baseline | 68.1 [67.8..73.4] / 71.9 [67.9..74.0] | ns/op at 32 / 32,768 elements |
| persistent.map_get_minus_baseline | 83.9 [82.6..87.1] / 132.8 [127.9..139.1] | ns/op at 32 / 32,768 entries |
| persistent.vec_conj_loop | 586.3 [585.0..589.1] | ns/iteration, 100,000 updates |
| persistent.map_assoc_loop | 1260.0 [1245.3..1273.9] | ns/iteration, 100,000 updates |
| work.alone | 1188.7 [1099.5..1207.1] | us/task, 64 tasks |
| work.while_consumers_parked | 1289.6 [1225.3..1470.7] | us/task, plus 1000 parked consumers |
| work.publish_to_all_resumed | 1352.9 [1314.0..1977.3] | ns/consumer, including executor queueing |
| strings.build, append 10k / 100k | 1465.1 [1435.4..1489.7] / 1426.6 [1417.3..1438.8] | ns/unit |
| strings.build, prepend 10k / 100k | 1458.2 [1445.3..1480.1] / 1411.8 [1403.0..1440.1] | ns/unit |
| strings.flatten, append 100k | 5.0 [4.4..5.3] | ns/unit, first contiguous read |
| overhead.startup_counting_off / on | 395.8 [395.1..396.3] / 437.1 [435.9..437.7] | ns/task; instrumentation changes cost |

Every parked consumer remained parked until useful work completed. Timing alone
does not establish equal throughput or scaling; the asserted still-parked count
establishes that waiting did not occupy those workers.

Shared input stayed **3001 objects / 1,428,020 requested bytes** from 1 to 10,000
consumers. Code was shared once: 2226 bytes for consumer.js, 3080 for
consumer_fold.js. Each consumer added one live heap object (builder or callback),
every returned input subtree kept identity, and only the 3001 input objects
remained after task drop and collection. Selected memory rows:

| Fixture / consumers | Spawn B/task | Park B/task | Shallow VM B/task | Live heap objects | Retained capacity B/task |
| --- | ---: | ---: | ---: | ---: | ---: |
| consumer / 1 | 1192.0 | 1252.0 | 912 | 3002 | 444.0 |
| consumer / 10,000 | 1154.2 | 712.0 | 912 | 13001 | 85.2 |
| fold / 1 | 1216.0 | 2644.0 | 1304 | 3002 | 444.0 |
| fold / 10,000 | 1178.2 | 2104.0 | 1304 | 13001 | 52.5 |

The update counting pass retained 1494.0 B/conj and 5732.4 B/assoc until explicit
collection, which released 717,396,504 requested bytes after both workloads.
Full-run RSS high-water reached 1,030,120 kB, dominated by those preceding updates;
it is not a suspended-task memory estimate.

## Verification run

The command catalogue below records the old README/docs command inventory and
its results in this worktree. Wrappers were normalized to the pinned development
shell and `CARGO_BUILD_JOBS=3`; each test invocation includes
`-- --test-threads=3` (or appends that flag to its existing harness options).
Placeholder script paths (`path/to/script.js`, `file.js`, `<file>`) were replaced
with `examples/demo.js`; they were notation, not runnable files. The revised
README uses the concrete path. Duplicate invocations caused by flag/wrapper
variants were still run. Build/test/example/measurement commands no longer live
in hundreds of feature paragraphs; the maintained entry points are README and
this document.

One stale test filter, `frontend::tests::delete_of_identifiers_is_rejected`,
exited successfully while selecting zero tests. It is retired; the correct
filter is `frontend::tests::delete_of_identifiers_follows_strictness` (one passing
test). An inventory run during the architecture rewrite saw the temporarily
missing Root inventory section and failed; after the table was restored its
16 tests passed. This was an editing-order documentation failure, not a runtime
defect. No assertions or fixture bodies were changed.

The pre-consolidation inventory contained 368 normalized command spellings.
The 365 invocations below were executed before the finish checks; the other
three (full suite, fmt, clippy) are recorded separately. The clj-quickjs-30 corpus
passed **856 tests in 17 binaries**, with no failures. Its exact command is row
63 and is maintained in README. No runtime code defect was found by this sweep.

Additional checks, also with CARGO_BUILD_JOBS=3 in the pinned shell:

| Exact command | Result |
| --- | --- |
| `cargo test --lib heap::inventory:: -- --test-threads=3` | 16 passed after root table restoration |
| `cargo test --lib frontend::tests::grammar_doc_examples -- --exact --test-threads=3` | 1 passed, covering all 351 retained fences |
| `cargo test --lib frontend::tests::delete_of_identifiers_follows_strictness -- --exact --test-threads=3` | 1 passed; replacement for the retired zero-match filter |
| `cargo check --all-targets` | passed |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --all-targets -- -D warnings` | passed |

Both demo file invocations completed with 89; resumable_counter made five
host effects over 26 runs. All four e2e variants and all five benchmark
invocations completed with their internal checks. The exact exit/results below
include redundant wrapper/flag variants intentionally retained for the audit.
Elapsed seconds include build/lock wait and are not benchmark timings.

<details>
<summary>Complete command inventory and observed results</summary>

Every row uses CARGO_BUILD_JOBS=3 inside nix develop. Test flags are shown
exactly; a successful zero-match retired filter is labelled rather than counted
as proof. Full-suite/fmt/clippy results are outside this historical catalogue.

| # | Executed command | Result | Elapsed s |
| ---: | --- | --- | ---: |
| 1 | `cargo build --all-targets` | passed | 14 |
| 2 | `cargo test --doc -- --test-threads=3` | 6 passed, 0 failed | 1 |
| 3 | `cargo run` | passed | 0 |
| 4 | `cargo run -- examples/demo.js` | passed | 0 |
| 5 | `cargo run --example resumable_counter` | passed | 0 |
| 6 | `cargo test --lib resume_tests -- --test-threads=3` | 10 passed, 0 failed | 0 |
| 7 | `cargo test --test resumable -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 8 | `cargo test --test gc -- --test-threads=3` | 51 passed, 0 failed | 0 |
| 9 | `cargo test --lib heap:: -- --test-threads=3` | 49 passed, 0 failed | 0 |
| 10 | `cargo test --test collections -- --test-threads=3` | 9 passed, 0 failed | 0 |
| 11 | `cargo test --lib collections:: -- --test-threads=3` | 14 passed, 0 failed | 3 |
| 12 | `cargo test --test collection_intrinsics -- --test-threads=3` | 13 passed, 0 failed | 0 |
| 13 | `cargo test --lib vm::intrinsics:: -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 14 | `cargo test --test publication -- --test-threads=3` | 30 passed, 0 failed | 0 |
| 15 | `cargo test --lib value::share:: -- --test-threads=3` | 8 passed, 0 failed | 0 |
| 16 | `cargo test --test dependency -- --test-threads=3` | 14 passed, 0 failed | 0 |
| 17 | `cargo test --lib dep:: -- --test-threads=3` | 11 passed, 0 failed | 0 |
| 18 | `cargo test --test js_source -- --test-threads=3` | 579 passed, 0 failed | 71 |
| 19 | `cargo test --test js_functions -- --test-threads=3` | 61 passed, 0 failed | 2 |
| 20 | `cargo test --lib frontend:: -- --test-threads=3` | 103 passed, 0 failed | 11 |
| 21 | `cargo test --test wakeup -- --test-threads=3` | 4 passed, 0 failed | 1 |
| 22 | `cargo test --lib task:: -- --test-threads=3` | 98 passed, 0 failed | 3 |
| 23 | `cargo test --test sim_host -- --test-threads=3` | 31 passed, 0 failed | 0 |
| 24 | `cargo test --lib dep::graph:: -- --test-threads=3` | 7 passed, 0 failed | 0 |
| 25 | `cargo test --test atoms -- --test-threads=3` | 13 passed, 0 failed | 0 |
| 26 | `cargo test --lib atom:: -- --test-threads=3` | 7 passed, 0 failed | 0 |
| 27 | `cargo test --test cancellation -- --test-threads=3` | 3 passed, 0 failed | 1 |
| 28 | `cargo test --lib task::cancel_tests -- --test-threads=3` | 15 passed, 0 failed | 0 |
| 29 | `cargo test --test safepoints -- --test-threads=3` | 13 passed, 0 failed | 0 |
| 30 | `cargo test --lib heap::gc:: -- --test-threads=3` | 24 passed, 0 failed | 0 |
| 31 | `cargo test --test executor -- --test-threads=3` | 13 passed, 0 failed | 0 |
| 32 | `cargo test --lib exec:: -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 33 | `cargo test --test executor_gc -- --test-threads=3` | 5 passed, 0 failed | 0 |
| 34 | `cargo test --test gc_roots -- --test-threads=3` | 70 passed, 0 failed | 3 |
| 35 | `cargo test --lib heap::inventory:: -- --test-threads=3` | exit 101 during edit; 15 passed, 1 failed; rerun above: 16 passed | 0 |
| 36 | `cargo test --test need -- --test-threads=3` | 54 passed, 0 failed | 15 |
| 37 | `cargo test --lib task::need_tests -- --test-threads=3` | 8 passed, 0 failed | 0 |
| 38 | `cargo test --test exceptions -- --test-threads=3` | 22 passed, 0 failed | 1 |
| 39 | `cargo run --example e2e` | passed | 0 |
| 40 | `cargo run --example e2e -- failure` | passed | 0 |
| 41 | `cargo run --example e2e -- cancel` | passed | 0 |
| 42 | `cargo run --example e2e -- companion` | passed | 0 |
| 43 | `cargo test --test e2e -- --test-threads=3` | 10 passed, 0 failed | 0 |
| 44 | `cargo test --test native_fold -- --test-threads=3` | 16 passed, 0 failed | 1 |
| 45 | `cargo test --lib vm::native:: -- --test-threads=3` | 21 passed, 0 failed | 2 |
| 46 | `cargo test --test exception_waits -- --test-threads=3` | 54 passed, 0 failed | 0 |
| 47 | `cargo test --test concurrency -- --test-threads=3` | 17 passed, 0 failed | 0 |
| 48 | `cargo test --test concurrency seeded -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 49 | `cargo run --release --example bench` | passed | 66 |
| 50 | `cargo test --test measurement -- --test-threads=3` | 9 passed, 0 failed | 4 |
| 51 | `cargo test --test js_source intl_list_format -- --test-threads=3` | 6 passed, 0 failed | 1 |
| 52 | `cargo test --test js_source intl_duration_format -- --test-threads=3` | 6 passed, 0 failed | 1 |
| 53 | `cargo test --lib value::string::tests::string_to_number_budgeted -- --test-threads=3` | 1 passed, 0 failed | 3 |
| 54 | `cargo test --test quickjs_intl -- --nocapture --test-threads=3` | 1 passed, 0 failed | 1 |
| 55 | `cargo test --test js_source using_declarations -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 56 | `cargo test --test js_source await_using -- --test-threads=3` | 4 passed, 0 failed | 4 |
| 57 | `cargo test --test js_source async_disposable_stack -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 58 | `cargo test --test js_source for_using_of_loops -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 59 | `cargo test --test exception_waits -- --test-threads=3` | 54 passed, 0 failed | 1 |
| 60 | `cargo test --test js_modules using_module_resources_dispose_after_evaluation -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 61 | `cargo test --test quickjs_language quickjs_operator_cases_run_unmodified -- --exact --nocapture --test-threads=3` | 1 passed, 0 failed | 0 |
| 62 | `cargo test --test exceptions quickjs_using -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 63 | `cargo test --test quickjs_language --test quickjs_functions --test quickjs_objects --test quickjs_builtins --test quickjs_collections --test quickjs_bigint --test quickjs_async --test quickjs_intl --test js_source --test js_functions --test js_modules --test exceptions --test error_stacks --test need --test native_fold --test cancellation --test malformed_input -- --test-threads=3` | 856 passed, 0 failed | 102 |
| 64 | `cargo test --test closures -- --test-threads=3` | 10 passed, 0 failed | 1 |
| 65 | `cargo run -- examples/demo.js` | passed | 0 |
| 66 | `cargo test --test native_unwind -- --test-threads=3` | 10 passed, 0 failed | 0 |
| 67 | `cargo test --lib frontend::tests::arrow_functions_and_asi -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 68 | `cargo test --test js_functions arrow_function_values -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 69 | `cargo test --lib frontend::tests::comma_operator -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 70 | `cargo test --test js_source comma_operator_evaluates_left_to_right -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 71 | `cargo test --lib frontend::tests::bitwise_and_shift_precedence -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 72 | `cargo test --test js_source bitwise_operators_follow_to_int32 -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 73 | `cargo test --lib frontend::tests::break_and_continue -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 74 | `cargo test --test js_source while_break_and_continue -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 75 | `cargo test --lib frontend::tests::exponent_operator_syntax -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 76 | `cargo test --test js_source exponent_operator -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 77 | `cargo test --test exceptions break_and_continue -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 78 | `cargo test --test exceptions a_jump_through_finally -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 79 | `cargo test --test exception_waits jump -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 80 | `cargo test --test exception_waits a_break_through_finally -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 81 | `cargo test --lib frontend::tests::update_expressions -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 82 | `cargo test --test js_source update_expressions_on_bindings -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 83 | `cargo test --test js_source update_expressions_on_properties -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 84 | `cargo test --test need property_update_resumes_after_wait -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 85 | `cargo test --test gc_roots property_update_base_survives -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 86 | `cargo test --lib frontend::tests::do_while_statements -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 87 | `cargo test --test js_source do_while_loops -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 88 | `cargo test --lib frontend::tests::for_statements -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 89 | `cargo test --test js_source for_loops -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 90 | `cargo test --test js_source string_values -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 91 | `cargo test --test publication strings_publish -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 92 | `cargo test --test gc strings_in_frames -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 93 | `cargo test --lib value:: -- --test-threads=3` | 50 passed, 0 failed | 1 |
| 94 | `cargo test --test js_source strings_are_code_unit_sequences -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 95 | `cargo test --lib frontend::tests::unicode_string_escapes_decode_code_units -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 96 | `cargo test --test js_source addition_concatenates_strings -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 97 | `cargo test --lib value::string:: -- --test-threads=3` | 21 passed, 0 failed | 1 |
| 98 | `cargo test --test js_source strings_ -- --test-threads=3` | 9 passed, 0 failed | 6 |
| 99 | `cargo test --test js_source strings_compare_by_code_units -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 100 | `cargo test --lib frontend::tests::labelled_statements -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 101 | `cargo test --test js_source labelled_break_and_continue -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 102 | `cargo test --test exception_waits labelled_jumps_wait_and_resume_after_migration -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 103 | `cargo test --lib frontend::tests::switch_statements -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 104 | `cargo test --test js_source switch_statements -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 105 | `cargo test --test exception_waits switch_waits_survive_migration_and_collection -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 106 | `cargo test --lib frontend::tests::compound_assignment -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 107 | `cargo test --test js_source compound_assignment_on_bindings -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 108 | `cargo test --test js_source compound_assignment_on_properties -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 109 | `cargo test --test js_source deep_property_compound_assignments -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 110 | `cargo test --test need property_compound_assignment_resumes_after_wait -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 111 | `cargo test --test gc_roots property_compound_assignment_base_survives -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 112 | `cargo test --test js_source typeof_operator -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 113 | `cargo test --lib vm::dispatch::tests::typeof_classifies_every_object_kind -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 114 | `cargo test --lib frontend::tests::unary_binds_tighter_than_binary_and_looser_than_calls -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 115 | `cargo test --test js_source void_operator -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 116 | `cargo test --lib frontend::tests::var_statements -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 117 | `cargo test --test js_source var_declarations_hoist_to_the_function -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 118 | `cargo test --test js_functions top_level_function_declarations_are_var_scoped -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 119 | `cargo test --lib frontend::tests::object_literals -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 120 | `cargo test --test js_source object_literals_and_named_properties -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 121 | `cargo test --test publication publishing_an_object_literal_is_rejected_with_property_path -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 122 | `cargo test --test gc_roots object_property -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 123 | `cargo test --lib frontend::tests::computed_member_access -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 124 | `cargo test --test js_source computed_member_access -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 125 | `cargo test --lib vm::property::tests::computed_keys_convert_by_to_property_key -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 126 | `cargo test --lib frontend::tests::directive_prologues_set_strictness -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 127 | `cargo test --test js_source use_strict_directives -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 128 | `cargo test --lib frontend::tests::default_parameters -- --test-threads=3` | 1 passed, 0 failed | 3 |
| 129 | `cargo test --test js_functions default_parameters -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 130 | `cargo test --lib frontend::tests::legacy_octal_literals_in_sloppy_mode -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 131 | `cargo test --test js_source legacy_octal_literals_in_sloppy_scripts -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 132 | `cargo test --lib frontend::tests::legacy_octal_escapes_in_sloppy_mode -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 133 | `cargo test --test js_source legacy_octal_escapes_in_sloppy_scripts -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 134 | `cargo test --test js_source object_intrinsics -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 135 | `cargo test --test publication closures_using_builtin_globals_publish -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 136 | `cargo test --test executor two_workers_share_the_realm_intrinsics -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 137 | `cargo test --test gc_roots realm_intrinsics_retain -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 138 | `cargo test --lib realm:: -- --test-threads=3` | 5 passed, 0 failed | 0 |
| 139 | `cargo test --lib frontend::tests::template_literals -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 140 | `cargo test --test js_source template -- --test-threads=3` | 10 passed, 0 failed | 0 |
| 141 | `cargo test --test js_source tagged_template -- --test-threads=3` | 8 passed, 0 failed | 1 |
| 142 | `cargo test --lib frontend::tests::tagged_template_syntax -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 143 | `cargo test --test js_source method_calls_bind_this -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 144 | `cargo test --lib frontend::tests::this_expressions -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 145 | `cargo test --test need a_method_resumes_with_the_same_this_after_a_wait_and_migration -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 146 | `cargo test --lib frontend::tests::array_literals -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 147 | `cargo test --lib object::array -- --test-threads=3` | 8 passed, 0 failed | 0 |
| 148 | `cargo test --test js_source array_literals -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 149 | `cargo test --test publication publishing_an_array_is_rejected_with_index_path -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 150 | `cargo test --test gc_roots array_element -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 151 | `cargo test --test js_source define_property -- --test-threads=3` | 5 passed, 0 failed | 0 |
| 152 | `cargo test --lib vm::property::tests::inherited_read_only_properties_reject_writes -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 153 | `cargo test --lib vm::descriptor -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 154 | `cargo test --test js_source function_own_properties -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 155 | `cargo test --test publication function_own_properties_follow_publication_rule -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 156 | `cargo test --lib value::share::tests::a_function_gaining_properties_during_a_check_is_not_cached -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 157 | `cargo test --test gc_roots function_properties -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 158 | `cargo test --test js_source new_operator_constructs_objects -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 159 | `cargo test --test js_source a_constructor_suspends_at_a_need_and_completes_after_publication -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 160 | `cargo test --test need a_constructor_resumes_with_its_new_object_after_a_wait_and_migration -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 161 | `cargo test --lib frontend::tests::new_expressions -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 162 | `cargo test --test js_source prototype_reflection -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 163 | `cargo test --lib vm::prototype -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 164 | `cargo test --test js_source object_extensibility -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 165 | `cargo test --test publication freezing_keeps_ownership_and_published_functions_are_frozen -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 166 | `cargo test --lib object:: -- --test-threads=3` | 27 passed, 0 failed | 3 |
| 167 | `cargo test --test js_source own_property_key_order -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 168 | `cargo test --lib vm::keys -- --test-threads=3` | 6 passed, 0 failed | 1 |
| 169 | `cargo test --test js_source instanceof_operator -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 170 | `cargo test --lib frontend::tests::binary_precedence_and_left_associativity -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 171 | `cargo test --test js_source delete_property_references -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 172 | `cargo test --lib frontend::tests::delete_of_identifiers_is_rejected -- --test-threads=3` | 0 matched; retired filter, replaced above | 0 |
| 173 | `cargo test --test js_source in_operator -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 174 | `cargo test --lib frontend::tests::for_in_heads -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 175 | `cargo test --test js_source for_in_over_own_properties -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 176 | `cargo test --test gc_roots for_in -- --test-threads=3` | 4 passed, 0 failed | 0 |
| 177 | `cargo test --test js_source for_in_walks_the_prototype_chain -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 178 | `cargo test --lib vm::keys:: -- --test-threads=3` | 6 passed, 0 failed | 0 |
| 179 | `cargo test --test js_source for_in_over_arrays -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 180 | `cargo test --lib frontend::tests::for_var_heads -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 181 | `cargo test --test js_source var_heads_in_for_loops -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 182 | `cargo test --lib frontend::tests::sloppy_for_in_var_initializer -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 183 | `cargo test --test js_source sloppy_for_in -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 184 | `cargo test --test js_source methods_on_primitive_bases -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 185 | `cargo test --test js_source methods_on_primitive_bases_allocate_no_wrappers -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 186 | `cargo test --test js_functions call_apply_and_reflect_apply -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 187 | `cargo test --test js_functions forwarded_calls_wait_and_resume_on_another_thread -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 188 | `cargo test --test js_source native_error_bound_constructor_prototypes -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 189 | `cargo test --test js_functions function_length_and_name -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 190 | `cargo test --test js_functions function_metadata_property_operations -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 191 | `cargo test --test js_source array_push -- --test-threads=3` | 2 passed, 0 failed | 3 |
| 192 | `cargo test --test js_source symbol_to_primitive -- --test-threads=3` | 4 passed, 0 failed | 3 |
| 193 | `cargo test --test exception_waits symbol_to_primitive -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 194 | `cargo test --test js_source objects_convert_with_to_primitive -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 195 | `cargo test --test js_source object_conversions_cover_hints_operators_and_property_keys -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 196 | `cargo test --test need to_primitive -- --test-threads=3` | 3 passed, 0 failed | 1 |
| 197 | `cargo test --test exception_waits to_primitive -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 198 | `cargo test --test js_source primitive_wrapper -- --test-threads=3` | 8 passed, 0 failed | 3 |
| 199 | `cargo test --test js_functions arrows_capture_this -- --test-threads=3` | 2 passed, 0 failed | 2 |
| 200 | `cargo test --test need an_arrow_retains_lexical_this_after_a_wait_collection_and_migration -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 201 | `cargo test --test js_source string_length_and_index_properties -- --test-threads=3` | 1 passed, 0 failed | 2 |
| 202 | `cargo test --test js_source string_index_reads_allocate_no_wrappers -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 203 | `cargo test --test js_functions arguments_ -- --test-threads=3` | 9 passed, 0 failed | 0 |
| 204 | `cargo test --lib frontend::tests::object_literal_methods -- --exact --test-threads=3` | 1 passed, 0 failed | 3 |
| 205 | `cargo test --test js_source object_literal_methods -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 206 | `cargo test --test publication method_home_object_prevents_publication -- --exact --test-threads=3` | 1 passed, 0 failed | 1 |
| 207 | `cargo test --test gc_roots method_home_object -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 208 | `cargo test --test js_source parse_ -- --test-threads=3` | 15 passed, 0 failed | 8 |
| 209 | `cargo test --lib frontend::tests::accessor_literals -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 210 | `cargo test --test js_source accessor -- --test-threads=3` | 5 passed, 0 failed | 0 |
| 211 | `cargo test --test need getter_waits_on_dependency_and_resumes -- --exact --test-threads=3` | 1 passed, 0 failed | 1 |
| 212 | `cargo test --test need accessor_cancellation_and_dependency_failure_unwind -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 213 | `cargo test --test gc_roots accessor -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 214 | `cargo test --test publication accessor -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 215 | `cargo test --test js_source math_ -- --test-threads=3` | 8 passed, 0 failed | 2 |
| 216 | `cargo test --test js_source math_pow -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 217 | `cargo test --test js_source global_object_bindings -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 218 | `cargo test --test js_functions global_ -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 219 | `cargo test --test gc_roots global_object -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 220 | `cargo test --test js_source stored_proto_accessor -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 221 | `cargo test --test publication built_in_proto_accessors_are_shared_across_tasks -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 222 | `cargo test --lib vm::prototype:: -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 223 | `cargo test --test js_source sloppy_implicit_globals_and_delete -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 224 | `cargo test --test js_source error_ -- --test-threads=3` | 17 passed, 0 failed | 2 |
| 225 | `cargo test --test exceptions uncaught_error_objects_report_name_and_message -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 226 | `cargo test --test js_source string_split -- --test-threads=3` | 4 passed, 0 failed | 6 |
| 227 | `cargo test --test js_source string_code_unit_coercion_waits_yields_collects_and_migrates -- --test-threads=3` | 1 passed, 0 failed | 3 |
| 228 | `cargo test --test error_stacks -- --test-threads=3` | 9 passed, 0 failed | 1 |
| 229 | `cargo test --test js_source array_constructor_and_is_array -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 230 | `cargo test --test js_source native_error -- --test-threads=3` | 3 passed, 0 failed | 2 |
| 231 | `cargo test --test js_source json_parse -- --test-threads=3` | 9 passed, 0 failed | 11 |
| 232 | `cargo test --test js_source symbol_ -- --test-threads=3` | 20 passed, 0 failed | 17 |
| 233 | `cargo test --test publication symbols_publish_with_identity -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 234 | `cargo test --lib vm::keys::tests::symbol_keys_follow_all_strings_in_creation_order -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 235 | `cargo test --test js_source json_stringify -- --test-threads=3` | 17 passed, 0 failed | 14 |
| 236 | `cargo test --test js_modules json_stringify_namespace_snapshot_limits -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 237 | `cargo test --test js_source symbol_registry -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 238 | `cargo test --test executor symbol_registry_is_shared_across_tasks -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 239 | `cargo test --test gc_roots symbol_registry_retains_unreferenced_symbols_across_collection -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 240 | `cargo test --lib regexp::parse::tests:: -- --test-threads=3` | 11 passed, 0 failed | 0 |
| 241 | `cargo test --lib bigint -- --test-threads=3` | 18 passed, 0 failed | 0 |
| 242 | `cargo test --test js_source bigint -- --test-threads=3` | 32 passed, 0 failed | 9 |
| 243 | `cargo test numeric_separator -- --test-threads=3` | 3 passed, 0 failed | 10 |
| 244 | `cargo test --test js_source bigint_ -- --test-threads=3` | 32 passed, 0 failed | 9 |
| 245 | `cargo test --lib value::bigint:: -- --test-threads=3` | 16 passed, 0 failed | 0 |
| 246 | `cargo test --test atoms bigint_compare_and_swap -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 247 | `cargo test --test js_modules bigint_namespace_definitions_use_same_value -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 248 | `cargo test --lib regexp::matcher::tests:: -- --test-threads=3` | 16 passed, 0 failed | 0 |
| 249 | `cargo test --lib vm::date::tests::date_time_arithmetic -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 250 | `cargo test --test js_source date_ -- --test-threads=3` | 32 passed, 0 failed | 8 |
| 251 | `cargo test --lib vm::date::tests::date_utc -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 252 | `cargo test --test js_source date_to_iso_string -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 253 | `cargo test --test js_source date_utc_fields -- --test-threads=3` | 3 passed, 0 failed | 1 |
| 254 | `cargo test --test js_source float_typed_array -- --test-threads=3` | 4 passed, 0 failed | 0 |
| 255 | `cargo test --lib object::typed_array::tests:: -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 256 | `cargo test --test js_source regexp_ -- --test-threads=3` | 28 passed, 0 failed | 1 |
| 257 | `cargo test --test js_source overlapping_capture_output -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 258 | `cargo test --test js_source typed_array_ -- --test-threads=3` | 24 passed, 0 failed | 3 |
| 259 | `cargo test --test js_source for_of_ -- --test-threads=3` | 5 passed, 0 failed | 0 |
| 260 | `cargo test --test exception_waits for_of_ -- --test-threads=3` | 5 passed, 0 failed | 0 |
| 261 | `cargo test --test js_source string_replace -- --test-threads=3` | 6 passed, 0 failed | 3 |
| 262 | `cargo test --test js_source regexp_unicode_mode -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 263 | `cargo test --test promises -- --test-threads=3` | 60 passed, 0 failed | 2 |
| 264 | `cargo test --test js_source promise_thenable_adoption -- --exact --test-threads=3` | 1 passed, 0 failed | 1 |
| 265 | `cargo test --test date_host -- --test-threads=3` | 11 passed, 0 failed | 0 |
| 266 | `cargo test --test js_source intl_number_format -- --test-threads=3` | 17 passed, 0 failed | 2 |
| 267 | `cargo test --test js_source date_ -- --test-threads=3` | 32 passed, 0 failed | 7 |
| 268 | `cargo test --test js_source regexp_case_insensitive_unicode -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 269 | `cargo test --test js_source regexp_case_insensitive_legacy -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 270 | `cargo test --test js_source intl_plural_rules -- --test-threads=3` | 5 passed, 0 failed | 1 |
| 271 | `cargo test --test sim_host intl_default_locale_resolution -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 272 | `cargo test --test js_source regexp_property_escapes -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 273 | `cargo test --lib vm::date:: -- --test-threads=3` | 10 passed, 0 failed | 3 |
| 274 | `cargo test --test js_source spread_ -- --test-threads=3` | 5 passed, 0 failed | 1 |
| 275 | `cargo test --test need iterator_next_waits_on_dependency -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 276 | `cargo test --lib frontend::tests::spread_forms -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 277 | `cargo test --test js_source rest_ -- --test-threads=3` | 7 passed, 0 failed | 1 |
| 278 | `cargo test --test js_source array_and_object_destructuring_waits_collects_and_migrates -- --test-threads=3` | 1 passed, 0 failed | 3 |
| 279 | `cargo test --test js_source array_from_ -- --test-threads=3` | 4 passed, 0 failed | 1 |
| 280 | `cargo test --test need array_from_iterator_waits_on_dependency -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 281 | `cargo test --lib regexp:: -- --test-threads=3` | 32 passed, 0 failed | 1 |
| 282 | `cargo test --test js_source regexp_unicode_string_properties -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 283 | `cargo test --test js_source array_destructuring -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 284 | `cargo test --test js_source generator_ -- --test-threads=3` | 12 passed, 0 failed | 15 |
| 285 | `cargo test --test exception_waits generator_ -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 286 | `cargo test --test js_source weak_map -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 287 | `cargo test --test gc weak_map_ -- --test-threads=3` | 9 passed, 0 failed | 0 |
| 288 | `cargo test --test publication weak_maps_ -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 289 | `cargo test --test js_source intl_collator -- --test-threads=3` | 13 passed, 0 failed | 2 |
| 290 | `cargo test --test js_source destructuring -- --test-threads=3` | 7 passed, 0 failed | 4 |
| 291 | `cargo test --lib frontend::tests::computed_property_names -- --exact --test-threads=3` | 1 passed, 0 failed | 3 |
| 292 | `cargo test --test js_source computed_prop -- --test-threads=3` | 5 passed, 0 failed | 3 |
| 293 | `cargo test --test need computed_property_names_wait_and_migrate -- --exact --test-threads=3` | 1 passed, 0 failed | 1 |
| 294 | `cargo test --test js_source indirect_eval -- --test-threads=3` | 6 passed, 0 failed | 2 |
| 295 | `cargo test --test js_source indirect_eval_on_small_worker_stacks -- --exact --test-threads=3` | 1 passed, 0 failed | 1 |
| 296 | `cargo test --test js_source math_sum_precise -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 297 | `cargo test --test need math_sum_precise_waits_and_migrates -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 298 | `cargo test --lib task::sim_tests:: -- --test-threads=3` | 12 passed, 0 failed | 0 |
| 299 | `cargo test --test sim_facilities -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 300 | `cargo test --test js_source weak_ref -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 301 | `cargo test --test gc weak_ref -- --test-threads=3` | 5 passed, 0 failed | 1 |
| 302 | `cargo test --test gc kept_objects -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 303 | `cargo test --test publication weak_refs -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 304 | `cargo test --test js_source primitive_wrappers_survive_wait_collection_and_migration -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 305 | `cargo test --test gc weak_map_symbol_keys_follow_traced_reachability -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 306 | `cargo test --test quickjs_async -- --test-threads=3` | 4 passed, 0 failed | 0 |
| 307 | `cargo test --test js_source direct_eval -- --test-threads=3` | 10 passed, 0 failed | 3 |
| 308 | `cargo test --test js_source base_classes -- --test-threads=3` | 7 passed, 0 failed | 0 |
| 309 | `cargo test --lib frontend::tests::class_syntax -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 310 | `cargo test --test js_source derived_classes -- --test-threads=3` | 8 passed, 0 failed | 0 |
| 311 | `cargo test --test need derived_constructor_waits_inside_super -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 312 | `cargo test --test js_source quickjs_date_regexp_cases_run_unmodified -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 313 | `cargo test --test quickjs_collections -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 314 | `cargo test --test quickjs_bigint -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 315 | `cargo test --test js_functions -- --test-threads=3` | 61 passed, 0 failed | 2 |
| 316 | `cargo test --lib frontend:: -- --test-threads=3` | 103 passed, 0 failed | 12 |
| 317 | `cargo test --test js_modules -- --test-threads=3` | 89 passed, 0 failed | 4 |
| 318 | `cargo test --test js_source super_property_access -- --test-threads=3` | 6 passed, 0 failed | 0 |
| 319 | `cargo test --test need super_property_access_waits_collects_and_migrates -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 320 | `cargo test --test js_source object_constructor -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 321 | `cargo test --lib module::namespace:: -- --test-threads=3` | 4 passed, 0 failed | 0 |
| 322 | `cargo test --test js_functions super_ -- --test-threads=3` | 5 passed, 0 failed | 0 |
| 323 | `cargo test --test js_functions function_constructor -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 324 | `cargo test --test quickjs_objects quickjs_object_cases_run_unmodified -- --exact --nocapture --test-threads=3` | 1 passed, 0 failed | 1 |
| 325 | `cargo test --test js_source proxy_ -- --test-threads=3` | 12 passed, 0 failed | 2 |
| 326 | `cargo test --lib heap::inventory::proxy_edges_retain_and_reclaim -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 327 | `cargo test --test js_source logical_assignment -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 328 | `cargo test --test need logical_assignment -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 329 | `cargo test --lib frontend::tests::nullish_and_logical_assignment_grammar -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 330 | `cargo test --test js_source array_mutators -- --test-threads=3` | 9 passed, 0 failed | 1 |
| 331 | `cargo test --test need array_mutators -- --test-threads=3` | 2 passed, 0 failed | 2 |
| 332 | `cargo test --test js_source array_search -- --test-threads=3` | 3 passed, 0 failed | 2 |
| 333 | `cargo test --test need array_search -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 334 | `cargo test --test js_source array_callbacks -- --test-threads=3` | 3 passed, 0 failed | 7 |
| 335 | `cargo test --test need array_callbacks -- --test-threads=3` | 2 passed, 0 failed | 3 |
| 336 | `cargo test --test async_generators -- --test-threads=3` | 9 passed, 0 failed | 2 |
| 337 | `cargo test --test class_static_blocks -- --test-threads=3` | 5 passed, 0 failed | 1 |
| 338 | `cargo test --test js_source private_class -- --test-threads=3` | 5 passed, 0 failed | 0 |
| 339 | `cargo test --test async_iteration --test async_generators -- --test-threads=3` | 16 passed, 0 failed | 5 |
| 340 | `cargo test --test js_source array_copy -- --test-threads=3` | 6 passed, 0 failed | 6 |
| 341 | `cargo test --test need array_copy -- --test-threads=3` | 2 passed, 0 failed | 3 |
| 342 | `cargo test --test map_iterables -- --test-threads=3` | 5 passed, 0 failed | 1 |
| 343 | `cargo test --test need map_iterable_ -- --test-threads=3` | 2 passed, 0 failed | 3 |
| 344 | `cargo test --lib vm::native::add_entries::tests -- --test-threads=3` | 6 passed, 0 failed | 5 |
| 345 | `cargo test --test sets --test map_iterables -- --test-threads=3` | 13 passed, 0 failed | 2 |
| 346 | `cargo test --test boolean_bigint_wrappers -- --test-threads=3` | 8 passed, 0 failed | 1 |
| 347 | `cargo test --test js_source regexp_script_property_escapes -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 348 | `cargo test --test builtin_subclasses -- --test-threads=3` | 17 passed, 0 failed | 1 |
| 349 | `cargo test --test js_source string_ -- --test-threads=3` | 52 passed, 0 failed | 15 |
| 350 | `cargo test --lib string_methods:: -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 351 | `cargo test --test object_statics -- --test-threads=3` | 6 passed, 0 failed | 2 |
| 352 | `cargo test --test need object_statics_wait_collect_and_migrate -- --test-threads=3` | 1 passed, 0 failed | 2 |
| 353 | `cargo test --test js_source regexp_ -- --test-threads=3` | 28 passed, 0 failed | 2 |
| 354 | `cargo test --test js_source string_replace -- --test-threads=3` | 6 passed, 0 failed | 3 |
| 355 | `cargo test --test math -- --test-threads=3` | 8 passed, 0 failed | 2 |
| 356 | `cargo test --test js_source math_ -- --test-threads=3` | 8 passed, 0 failed | 2 |
| 357 | `cargo test --test reflect -- --test-threads=3` | 16 passed, 0 failed | 3 |
| 358 | `cargo test --test object_to_string -- --test-threads=3` | 6 passed, 0 failed | 1 |
| 359 | `cargo build --release` | passed | 25 |
| 360 | `cargo run -- examples/demo.js` | passed | 0 |
| 361 | `cargo run --release --example bench -- --quick` | passed | 3 |
| 362 | `cargo run --release --example bench -- memory` | passed | 0 |
| 363 | `cargo run --release --example bench -- work --workers 4 --rounds 9` | passed | 2 |
| 364 | `cargo run --release --example bench -- strings --rounds 3` | passed | 2 |
| 365 | `cargo test --test measurement string_concatenation_is_amortized_linear -- --test-threads=3` | 1 passed, 0 failed | 5 |

</details>

### Post-rebase finish checks

Run at `0e5bc989`, rebased onto master `72201c8b`. Every command used the default
target directory, with no peer workers. The later result-recording changes are
documentation only.

| Exact command | Observed result |
| --- | --- |
| `CARGO_BUILD_JOBS=3 nix develop -c cargo test --all-targets -- --test-threads=3` | **1940 passed, 0 failed, 0 ignored**, 57 result groups (including empty binary/example groups); 145 s wall time |
| `CARGO_BUILD_JOBS=3 nix develop -c cargo test --doc -- --test-threads=3` | **6 passed**, including compile-fail root/scope lifetime checks |
| `CARGO_BUILD_JOBS=3 nix develop -c cargo test --lib heap::inventory:: -- --test-threads=3` | **16 passed** |
| `CARGO_BUILD_JOBS=3 nix develop -c cargo test --lib frontend::tests::grammar_doc_examples -- --exact --test-threads=3` | **1 passed**, 351 preserved example fences |
| `CARGO_BUILD_JOBS=3 nix develop -c cargo fmt --all -- --check` | passed |
| `CARGO_BUILD_JOBS=3 nix develop -c cargo clippy --all-targets -- -D warnings` | passed |

No code defect follow-up was needed. This is a verification record, not a second
backlog; future implementation work remains in Pellets.

### Review corrections

The required Sol review returned fix_first with five documentation findings.
The full-suite record was committed while review ran; the remaining corrections
restore sloppy receiver/unmapped-arguments limitations, restrict RegExp execution
to lookahead, document cancellation's required enqueue and correct Task::run's
unlimited-budget/no-host signature. No runtime code or assertion changed.
The root inventory (16) and grammar examples (1) passed again, as did these
focused checks, each with CARGO_BUILD_JOBS=3 inside nix develop:

| Exact command | Result |
| --- | --- |
| `cargo test --lib regexp::matcher::tests::unsupported_features_fail_explicitly -- --exact --test-threads=3` | 1 passed |
| `cargo test --test js_functions call_apply_and_reflect_apply -- --exact --test-threads=3` | 1 passed |
| `cargo test --test js_functions arguments_object_holds_every_argument -- --exact --test-threads=3` | 1 passed |
| `cargo test --lib task::cancel_tests::cancelled_parked_consumer_leaves_the_cell_when_its_run_cleans_up -- --exact --test-threads=3` | 1 passed |

Formatter check and warnings-denied Clippy passed again after these corrections.
All five findings were fixed; no runtime follow-up was filed.
