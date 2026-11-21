# Gap Benchmark (`benchmarks/gap-bench`)

Unified Babashka benchmark suite for closing the remaining gaps between `light-cljs` and `shadow-cljs` across compiler speed, cold/warm cache behavior, build memory, scaling, watch/hot-reload, output size, startup breakdown, and runtime speed/memory — designed to complete in **≤ 8 minutes** (full head-to-head) or **~2.5–3 minutes** (fast `--reuse-shadow` loop).

## Quick start

```sh
# 1. Build a release binary (or point --light at an immutable binary)
cargo build --locked --release

# 2. First run: builds & caches the shadow-cljs baseline, runs full head-to-head (~6–7 min)
bb benchmarks/gap-bench/run.bb --fresh-shadow --light target/release/light-cljs

# 3. Fast iteration loop: reuses cached shadow compiles/watch/size while STILL running
#    live alternated runtime benchmarks against saved shadow JS artifacts (~2.5–3 min)
bb benchmarks/gap-bench/run.bb --light target/release/light-cljs \
  --baseline benchmarks/gap-bench/out/gap-bench-PREV_SHA.edn

# 4. Run a single slice for tight inner-loop work (10s–85s)
bb benchmarks/gap-bench/run.bb --only compile --light target/release/light-cljs
bb benchmarks/gap-bench/run.bb --only watch   --light target/release/light-cljs
bb benchmarks/gap-bench/run.bb --only size    --light target/release/light-cljs
bb benchmarks/gap-bench/run.bb --only runtime --light target/release/light-cljs

# 5. Run unit tests for parsers, math, scorecard & gap ranking
bb benchmarks/gap-bench/test.bb
```

## What each slice measures

1. **`compile` — Compiler performance, cold/warm cache, build memory & scaling canary**
   - `re-frame-200k` (200k lines, seed 42):
     - Clean build (`--no-cache --stats`, repeated + SHA-256 determinism check, oracle checksum, `check-waste.bb --strict`)
     - Cold cache-fill (`--cache-dir .gap-cache --stats`, cache-fill overhead vs clean, cache disk size)
     - Warm unchanged rebuild
     - Warm leaf-body edit (`src/workload/besmeik.cljs`, unused `defn`)
     - Warm declaration edit (`src/workload/besmeik.cljs`, unused `defprotocol`)
     - `--stats` phase breakdown (`resolve-parse`, `analysis`, `deferred-units`, `prepare-application`, `emission`, `write`)
   - `re-frame-50k` clean build → `50k → 200k` log-log scaling exponent canary (wall, CPU, peak RSS).
   - `shadow-cljs` capped at its smallest adequate heap (`-J-Xmx1536m`, configurable via `--shadow-heap`): clean, warm unchanged, warm leaf edit.

2. **`watch` — Watch & hot-reload in headless Chromium**
   - `re-frame-200k` via [`benchmarks/watch-vs-shadow.mjs`](../watch-vs-shadow.mjs):
     - Watch startup to first build, startup CPU, Chromium dev page load, startup/steady peak RSS, 0 console errors.
     - 3 edit scenarios with jittered pauses (`250–650 ms`): view leaf (`app.baga.views`), widely required (`workload.common`), and macro (`workload.sobis/hainfoulvei`).
     - End-to-end save→running time plus stage breakdown (noticing, tool step, output→message, message→running) and CPU/edit.

3. **`size` — Output size, runtime slice & ES2015 lowering**
   - `simple`, `todomvc`, `flow`, and `re-frame-200k`: raw, `gzip -9`, `brotli -q 11`, whole bundle (with React) vs app-only (subtracting React-only builds).
   - `todomvc` ES2015-lowered size (`esbuild --target=es2015`).
   - `light-cljs --size-report` runtime slice (kept/total units, runtime bytes, constant pool).

4. **`runtime` — Runtime speed, startup phase breakdown & runtime memory** (always live-alternated against shadow)
   - **`re-frame-200k` startup probe** ([`startup-probe.mjs`](startup-probe.mjs)): V8 parse/compile, runtime units init, top-level defs, module load total, first iteration, total, oracle checksum.
   - **`re-frame-200k` Node throughput & peak RSS**: 1 and 6 iterations (first output, wall, steady-state ms/iter, CPU, peak RSS, oracle checksums).
   - **TodoMVC in headless Chromium**: 9/9 behavior checks + 0 console errors ([`re-frame-browser.mjs`](../re-frame-browser.mjs)) and 1,000-todo load/add/toggleAll/edit/clearCompleted + CDP script ms & JS heap ([`re-frame-runtime.mjs`](../re-frame-runtime.mjs)).
   - **`core-ops` (76 microbenchmarks)** ([`benchmarks/core-ops/`](../core-ops/)): all 76 checksums verified, geometric mean, median, parity count (≤1.05×), per-group breakdown, top slower ops.
   - **`extend-ops` (16 `extend-type`-active microbenchmarks)** ([`extend-ops/`](extend-ops/)): companion to `core-ops` with `extend-type` active on `Atom`, `js/RegExp`, and `default` to cover real-app protocol dispatch paths.

## Worktree isolation & shadow baseline

- Auto-detects `research/` from the current checkout or the main git checkout (`git rev-parse --git-common-dir`), treating `research/` strictly **read-only**.
- Stages mutable copies, npm tools (`esbuild`, `playwright-core`), cached `shadow-baseline.edn`, and saved `shadow-artifacts/` into gitignored `benchmarks/gap-bench/work/`.
- Outputs `PREFIX.edn` (full raw data for `--baseline` diffs) and `PREFIX.md` (Executive Scorecard + Ranked Remaining Gaps >1.05× + detailed tables) into `benchmarks/gap-bench/out/`.
