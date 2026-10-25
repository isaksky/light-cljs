# Frozen measurement setup

Frozen 2026-10-25T23:30:43Z, before measured invocations. Source revision:
`6405a52ce18c2d5234ba0389c2b4a1416e6e9ad0`. No compiler, runner or fixture
changes. This predates automatic GC/aggregate-budget work (472), helper demand
(463), and native compile-time collection lowering (477).

Working directory for every command:
`/Users/admin/.codex/tmp/pellets-orchestrate/light-cljs-20261025-a/484-1`.

```sh
CARGO_BUILD_JOBS=3 cargo build --locked --release --no-default-features --features bbp-macros --target aarch64-apple-darwin --target-dir /Users/admin/.codex/tmp/pellets-orchestrate/light-cljs-20261025-a/484-1/target
# After the coordinator grants an exclusive build/test-free measurement window:
bb benchmarks/native-macros.bb benchmarks/native-macros-6405a52c/runs --jobs 1,2,4,8 --timeout-ms 120000
bb benchmarks/native-macros-test.bb
```

Native-only Cargo release, repository thin LTO and one codegen unit. The first
build may overlap peer work; the runner's rebuild and all timings/profiling wait
for the quiet window. Five fresh processes per supported cell; ascending jobs
on rounds 1/3/5, descending on 2/4, fixed collections/environment/helper order.
The first failure stops its cell. No warmups, compiler cache disabled, OS caches
not flushed. Compiler macro timeout 60 s, poll budget 100 million, external
compiler and Node timeout 120 s; other resource defaults unchanged. Node result
validation and the release build are outside compiler timing. Exact per-sample
commands, loads, stats, output/map hashes and traces are retained by the runner.
The test script's optional report argument is smoke-only; measured artifacts
will be independently checked against the five-run report and their hashes.

| Input | Bytes | SHA-256 |
| --- | ---: | --- |
| collections.cljs | 575 | `986f21e4ded480f0d9f76935fef6aacf39acfe4c38de0cdedd594d6f3490ede9` |
| environment.cljs | 510 | `bb8031cf1556cb72834d01ee0d0112fb12de5dde220db47402742f748450e4f8` |
| helper.cljs | 308 | `d618180f854f9d506bd84afb01acba90e3c15eb0b03d1ae2515d48f46a357ff3` |

Runner SHA-256: `fbabfb9118eb0725728083f3a3606b0721e9ecfcce1b2e0d85f95cc22e13f7f0`.
Cargo.toml: `8debece5da7f2af519b1cfe87577b32471531b0f8a95d6bde2af2fae299f0c75`.
Cargo.lock: `6f496a0e19fd9c9b7e6f023573d9cde0b2d28be92888cf9b1bb0ca8d6826d3da`.

No qualified V8 reference is available: the coordinator identified no existing
release V8 binary with verified provenance for this revision; earlier checks
used debug binaries or older revisions. No V8 build, repair or distribution.
Native matched-unit availability will also be checked; kernel ratios cannot
stand in for compiler k.

After measurement, select the successful jobs-1 fixture with the largest median
wall time. Run exactly one additional unchanged compiler invocation, with its
same compile arguments and limits, and attach `/usr/bin/sample PID 30 1 -mayDie
-file PROFILE` immediately. Both compiler and sampler are capped at 30 s and
reaped. Preserve the exact command, stderr, sampled stacks and result/map/trace.
If any higher-job cell succeeds, apply the same single-profile procedure to
the cell with the worst wall-time scaling. Do not lengthen inputs or repeat a
short-lived process to improve sampling. An insufficient sample is a reported
limitation, not permission to widen the experiment.

Profile invocation frozen at 2026-10-25T23:32:16Z:
`bb benchmarks/native-macros-6405a52c/profile.bb`.
Its SHA-256 is `57fb14ed7ec06957970a9cabcb8980332676d0aea68cd3ad64f10dd206f737a6`.
The script preserves the selected sample's complete compiler arguments and
fixed map path, records both commands, and forcibly reaps either timed-out
process tree. The profile is additional evidence, excluded from timing medians.
