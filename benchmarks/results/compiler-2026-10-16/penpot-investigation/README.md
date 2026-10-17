# Evidence for the Penpot investigation

Read [the report](../../../penpot-investigation.md) for conclusions, interpretation
and limitations. Source revision: `4d2a0d69004dd1c993aab3a65e3080079858e4bc`.

## Contents

- `render-profile.log`: complete `app.render` control profile.
- `render-both.log`: complete run with environment provenance/snapshots and
  expression warnings disabled. JavaScript output matches the control exactly.
- `synth4000-*.log`: untouched binary, diagnostic control, and environment-only
  ablation for the original single-namespace fixture.
- `macros-*.log`: 1,000 `when`-using functions with 0, 50 or 100 unused macros.
- `namespaces-*.log`: 40 identity functions per namespace, no JavaScript macros.
- `calls-*.log`: global-call/arity-search scaling.
- `generate.bb`: generates the synthetic inputs.
- `instrumentation.patch`: diagnostic-only instrumentation and ablation switches.
  This patch was **not applied to the production checkout**.
- `summary.txt`, `hashes.txt`, `validation.txt`: compact measurements, SHA-256
  output/binary identities and synthetic runtime checks.
- `prior-sample-summary.txt`: inclusive category counts from the supplied earlier
  `app.main` sample, excluding duplicate ancestors in the same category. Category
  percentages overlap and must not be added together.

The local scratch bundle, including emitted JavaScript and diagnostic source, is
`/private/tmp/light-cljs-investigation-20261016`. The supplied benchmark and sample
remain under
`/private/tmp/claude-501/-Users-admin-src-light-cljs/ef390548-9edc-4e71-ba3a-a2837e664e83/scratchpad/bench`.
Large outputs, binaries and the Penpot checkout are not copied into Git.

## Instrumentation

Timers record exclusive time by subtracting nested profiled phases. The global
allocator wrapper forwards operations to `System`, counting requested live
bytes. At bridge destruction it reports how many bytes clearing all four Rust
tables actually releases. This is not RSS or an allocation-stack profiler.
QuickJS reports its own memory use immediately before evaluator destruction.

`COUNT arity-expressions-scanned` adds the scanned arena range length for each
global-callee warning check. `COUNT` lines whose names are Rust container types
report cumulative newly allocated bytes when `Shared<T>` makes a copy. These
copy counters are not peak or live-byte measurements. `READERS` deduplicates
reader-context maps by their allocation address while traversing the catalog's
definition snapshots and analyzed programs, then counts their contained data.

The saved patch is the final audit version. The initial `render-profile` and
`synth4000` runs preceded addition of the `Shared<T>` copy counter, `READERS`
catalog audit and `bridge-nodes-encoded` counter. Their timers, bridge teardown
measurement, allocator instrumentation and ablation semantics are the same.
Those extra counters are present in `render-both` and the other synthetic runs.
One intermediate environment-only Penpot run was discarded after editing its
active shell wrapper corrupted its output files; it contributes no measurements.

These switches deliberately bypass behavior and are not safe general fixes:

- `PROBE_EPHEMERAL_ENV=1`: environment descriptors receive identity zero and
  skip canonical snapshot export/decode. Environment values are still imported.
  Source provenance recovery and escaped values need a proper ownership design.
- `PROBE_NO_WARNINGS=1`: skip expression warnings entirely.
- `PROBE_NO_SNAPSHOT=1`: broader snapshot ablation; not used in reported results.

## Reproduction

Use an isolated source archive. From the repository root:

```sh
evidence="$PWD/benchmarks/results/compiler-2026-10-16/penpot-investigation"
probe_dir=$(mktemp -d /private/tmp/light-cljs-probe.XXXXXX)
mkdir "$probe_dir/source"
git archive 4d2a0d69004dd1c993aab3a65e3080079858e4bc | tar -x -C "$probe_dir/source"
git -C "$probe_dir/source" apply "$evidence/instrumentation.patch"
cargo build --release --locked --manifest-path "$probe_dir/source/Cargo.toml"
bb "$evidence/generate.bb" "$probe_dir/fixtures"
```

Example synthetic run:

```sh
/usr/bin/time -l "$probe_dir/source/target/release/light-cljs" \
  --compile --entry probe.main --source-root "$probe_dir/fixtures/macros-100" \
  --esm --optimize none \
  --macro-expansions 100000000 --macro-nodes 4000000000 \
  --macro-timeout-ms 3600000 --macro-memory-limit 4000000000 \
  --macro-poll-budget 1000000000000 \
  > "$probe_dir/macros-100.js" 2> "$probe_dir/macros-100.log"
```

For `calls-N`, use entry `probe.main`; for `namespaces-N`, use `probe.n(N-1)`
(for example, `probe.n99` for 100 namespaces). The `synthetic-4000` entry is
`demo.main`. Run one compiler process at a time.

For Penpot, use entry `app.render` and source roots `frontend/src`, `common/src`,
`vendor`, `generated`, and `shims` under the existing `research/penpot-light`
port. Keep the other flags above. Prefix the command with
`PROBE_EPHEMERAL_ENV=1 PROBE_NO_WARNINGS=1` for the combined ablation. Do not
regenerate the port while compiling it. Compare emitted bytes or hashes before
interpreting performance results; these runs did not execute Penpot.

All ten primary synthetic modules were imported in Node 24.18.0 and their default
functions called with 7: call fixtures returned 1, namespace fixtures returned 7,
and macro fixtures returned 1. The full production test suite was not rerun;
no compiler implementation changes were made in the working checkout.
