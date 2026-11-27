# `cljs.pprint` benchmarks: light-cljs vs. shadow-cljs

Reproducible programs and measurement scripts for the bundled `cljs.pprint` library live in [`benchmarks/cljs-pprint/`](cljs-pprint/).

## Output size

Measured with [`benchmarks/cljs-pprint/size.bb`](cljs-pprint/size.bb), which builds four equivalent ClojureScript programs with both compilers at maximum optimization (`light-cljs release <id>` at `--optimize advanced`, and `clojure -M:shadow release <id>` with shadow-cljs 3.5.3), verifies that both outputs print identical standard output in Node.js, and records raw bytes and `gzip -9` bytes as absolute sizes and as the delta each program adds over its own `prn` baseline:

- `prn` ([`prn.cljs`](cljs-pprint/src/pprint_bench/prn.cljs)): prints `{:a 1 :b (vec (range 20)) :c #{:x :y :z} :d '(foo bar baz)}` with `prn` (does not require `cljs.pprint`).
- `pprint-only` ([`pprint_only.cljs`](cljs-pprint/src/pprint_bench/pprint_only.cljs)): prints the same value with `cljs.pprint/pprint`.
- `cl-format-only` ([`cl_format_only.cljs`](cljs-pprint/src/pprint_bench/cl_format_only.cljs)): formats `"~a is ~:r: ~{~d~^, ~}~%"` with `cljs.pprint/cl-format`.
- `code-dispatch` ([`code_dispatch.cljs`](cljs-pprint/src/pprint_bench/code_dispatch.cljs)): pretty-prints a `defn` form with `with-pprint-dispatch` and `cljs.pprint/code-dispatch`.

Reproduce (build an immutable release binary first, then run the comparison script):

```sh
cargo build --locked --release
mkdir -p target/bench
cp target/release/light-cljs "target/bench/light-cljs-$(git rev-parse --short=8 HEAD)"
bb benchmarks/cljs-pprint/size.bb --light "target/bench/light-cljs-$(git rev-parse --short=8 HEAD)" --size-report
```

**Target difference:** `light-cljs.edn` builds `:target :esm` (an ECMAScript module exporting `main`, invoked by a one-line Node.js dynamic `import`), whereas `shadow-cljs.edn` builds `:target :node-script` (a CommonJS Node.js script that invokes `main` directly). Because the two targets carry different module/bootstrap wrappers, the absolute `prn` baselines (`118,064` vs. `103,116` raw; `30,947` vs. `21,277` `gzip -9`) are not strictly like-for-like; subtracting each compiler's own `prn` baseline cancels that fixed target overhead when comparing the bytes `cljs.pprint` adds.

### Setup (2026-11-27, unreserved)

- **light-cljs before (`light-cljs-1171` baseline):** release V8 build at commit `e9cb291c` (`target/bench/light-cljs-e9cb291c`).
- **light-cljs after trimming (`light-cljs-1171`):** release V8 build at commit `bbe99a12` (`target/bench/light-cljs-bbe99a12`).
- **light-cljs after compile-cost reductions (`light-cljs-1172`, rebased onto `light-cljs-1175`):** release V8 build at commit `2d303398` (`target/bench/light-cljs-2d303398`, SHA-256 `3b838b69a9a89660...`).
- **shadow-cljs:** `3.5.3` (with ClojureScript `1.12.134`), run via `clojure -M:shadow release <build>`.
- **Other tools:** Node.js `v26.12.1`, Clojure Command Line Interface `1.12.7.1681`, OpenJDK `25.0.2`, Babashka `v1.13.230`.
- **Machine load (unreserved):** `e9cb291c` load average `4.59 6.94 7.54`; `bbe99a12` load average `5.37 4.98 5.76`; `2d303398` load average `14.12 10.31 9.77`.

### Before vs. after (`advanced` / `release`)

#### shadow-cljs 3.5.3 (unchanged across all runs)

| Program | Raw bytes | Delta vs. `prn` | `gzip -9` bytes | Delta vs. `prn` |
|---|---:|---:|---:|---:|
| `prn` | 103,116 | 0 | 21,277 | 0 |
| `pprint-only` | 245,898 | +142,782 | 52,350 | +31,073 |
| `cl-format-only` | 245,303 | +142,187 | 52,106 | +30,829 |
| `code-dispatch` | 245,758 | +142,642 | 52,291 | +31,014 |

#### light-cljs before (`e9cb291c`)

| Program | Raw bytes | Delta vs. `prn` | `gzip -9` bytes | Delta vs. `prn` |
|---|---:|---:|---:|---:|
| `prn` | 118,064 | 0 | 30,947 | 0 |
| `pprint-only` | 276,111 | +158,047 | 72,565 | +41,618 |
| `cl-format-only` | 275,555 | +157,491 | 72,418 | +41,471 |
| `code-dispatch` | 276,235 | +158,171 | 72,640 | +41,693 |

#### light-cljs after trimming (`bbe99a12`, `light-cljs-1171`)

| Program | Raw bytes | Delta vs. `prn` | Change vs. `e9cb291c` | `gzip -9` bytes | Delta vs. `prn` | Change vs. `e9cb291c` |
|---|---:|---:|---:|---:|---:|---:|
| `prn` | 118,064 | 0 | 0 | 30,947 | 0 | 0 |
| `pprint-only` | 198,748 | +80,684 | -77,363 | 53,176 | +22,229 | -19,389 |
| `cl-format-only` | 257,026 | +138,962 | -18,529 | 68,860 | +37,913 | -3,558 |
| `code-dispatch` | 276,015 | +157,951 | -220 | 72,730 | +41,783 | +90 |

#### light-cljs current (`2d303398`, `light-cljs-1172` on `light-cljs-1175`)

| Program | Raw bytes | Delta vs. `prn` | Change vs. `bbe99a12` | `gzip -9` bytes | Delta vs. `prn` | Change vs. `bbe99a12` |
|---|---:|---:|---:|---:|---:|---:|
| `prn` | 118,064 | 0 | 0 | 30,947 | 0 | 0 |
| `pprint-only` | 198,238 | +80,174 | -510 | 53,096 | +22,149 | -80 |
| `cl-format-only` | 255,561 | +137,497 | -1,465 | 68,928 | +37,981 | +68 |
| `code-dispatch` | 274,612 | +156,548 | -1,403 | 72,845 | +41,898 | +115 |

#### Summary (wins and losses across all four programs at `2d303398`)

- **`pprint-only` (wins on deltas and total raw; loses on total `gzip -9`):**
  - Raw delta vs. `prn`: light-cljs `+80,174` bytes (`+80,684` at `bbe99a12`) vs. shadow-cljs `+142,782` bytes (**62,608 bytes less** added by light-cljs).
  - `gzip -9` delta vs. `prn`: light-cljs `+22,149` bytes (`+22,229` at `bbe99a12`) vs. shadow-cljs `+31,073` bytes (**8,924 bytes less** added by light-cljs).
  - Total raw output: light-cljs `198,238` bytes vs. shadow-cljs `245,898` bytes (**47,660 bytes smaller**).
  - Total `gzip -9` output: light-cljs `53,096` bytes vs. shadow-cljs `52,350` bytes (**light-cljs is 746 bytes larger**, because its `prn` baseline starts `9,670` gzip bytes higher: `30,947` vs. `21,277`).
- **`cl-format-only` (wins on raw delta; loses on total raw, total `gzip -9`, and `gzip -9` delta):**
  - Raw delta vs. `prn`: light-cljs `+137,497` (`+138,962` at `bbe99a12`) vs. shadow-cljs `+142,187` bytes (`4,690` bytes less added).
  - Total raw output: light-cljs `255,561` vs. shadow-cljs `245,303` bytes (`10,258` bytes larger).
  - `gzip -9`: light-cljs `68,928` (`+37,981` delta; `+68` vs. `bbe99a12`) vs. shadow-cljs `52,106` (`+30,829` delta) — **light-cljs adds 7,152 more gzip bytes** and is `16,822` total gzip bytes larger.
- **`code-dispatch` (loses on every column; `gzip -9` grew `+115` bytes vs. `bbe99a12`, `+205` vs. `e9cb291c`):**
  - Raw: light-cljs `274,612` (`+156,548` delta; `-1,403` vs. `bbe99a12`) vs. shadow-cljs `245,758` (`+142,642` delta) — `13,906` more raw bytes added (`28,854` total raw larger).
  - `gzip -9`: light-cljs `72,845` (`+41,898` delta) vs. shadow-cljs `52,291` (`+31,014` delta) — `10,884` more gzip bytes added (`20,554` total gzip larger).
- **`prn` baseline (loses on raw and `gzip -9`):** light-cljs `118,064` raw / `30,947` `gzip -9` vs. shadow-cljs `103,116` raw / `21,277` `gzip -9` (`14,948` raw and `9,670` gzip bytes larger before requiring `cljs.pprint`).
- **Stop rule (`light-cljs-1171`):** still met at `2d303398` — `pprint-only` adds fewer raw and gzipped bytes over `prn` than shadow-cljs (`+80,174` vs. `+142,782` raw; `+22,149` vs. `+31,073` `gzip -9`), and `--size-report` shows nothing of `cl-format` or `code-dispatch` in data-only builds.

### `--size-report` diagnosis (what reached `directive-table`)

#### Before (`e9cb291c`)

In `pprint-only`, `--size-report` reported `1327 of 2342` runtime units (`187,273` runtime bytes) and `219` constant-pool entries (`13,225` bytes). Five load-time paths kept `directive-table`, `cl-format`, and `code-dispatch` alive:

1. **Data printers built with `formatter-out` at load:** `pprint-set` (`"~<#{~;~@{~w~^ ~:_~}~;}~:>"`), `pprint-array` (`"~<[~;~@{~w~^, ~:_~}~;]~:>"`), and `pprint-pqueue` (`"~<<-(~;~@{~w~^ ~_~}~;)-<~:>"`) invoked `cached-compile` at namespace load, pulling `compile-format` and `directive-table` directly into `simple-dispatch`.
2. **`code-dispatch` methods registered via `use-method`:** top-level `(use-method code-dispatch ...)` calls went through a helper function instead of top-level `defmethod` (`-add-method` with a `fn` literal), so unread multimethod removal (`light-cljs-1165`) could not drop `code-dispatch`, `pprint-code-list`, `pprint-ideref`, or `pprint-pqueue`.
3. **Eager `code-dispatch` formatters and `*code-table*`:** `pprint-hold-first` and `pprint-if` were top-level `(formatter-out ...)` definitions, and `*code-table*` called `(two-forms (add-core-ns {...}))` at load time, rooting `rt$isSpecialSymbol`, `rt$mapcat`, and every `code-dispatch` formatter.
4. **Top-level `defdirectives` and `cached-compile`:** `(defdirectives ...)` expanded to a top-level `hash-map` whose `:params` `array-map`s referenced host globals (`js/Number`, `js/String`), rooting `rt$PersistentHashMapValue`, `rt$arrayMap`, and `(rt$member_fromArrays)`. Separately, `(def cached-compile (memoize compile-format))` stayed a root because `compile-format` had a forward `(declare compile-format)`.
5. **Top-level regular expression and `WeakMap` initializers:** `empty-or-whitespace-re` (`#"^[\s\xa0]*$"`), `param-pattern`, and `uid-map` (`(js/WeakMap.)`) ran at load time even when only used by `cl-format` or `pprint-ideref`.

#### After (`bbe99a12`)

After rewriting `pprint-set`, `pprint-array`, and `pprint-pqueue` with `pprint-logical-block`, `print-length-loop`, `write-out`, and `pprint-newline`, switching `simple-dispatch` and `code-dispatch` to `defmethod`, and deferring `directive-table`, `cached-compile`, `*code-table*`, `pprint-hold-first`, `pprint-if`, `empty-or-whitespace-re`, and `uid-map` to first use (storing `param-pattern`'s source string instead of a top-level regex literal):

- `pprint-only` drops to `1226 of 2342` runtime units (`170,621` runtime bytes) and `79` constant-pool entries (`3,776` bytes), with `224` definitions removed at `advanced`.
- `--size-report` on `pprint-only`, `write` (`:stream nil`), and `simple-dispatch` shows zero roots from `cl-format` (`directive-table`, numeral tables, float/character/case/tab/iteration formatters) and zero roots from `code-dispatch`.
- `cl-format-only` drops `18,529` raw bytes (`112` definitions removed at `advanced`) because unused `code-dispatch` is no longer retained.

### Optimizer notes (recorded, not changed in this pellet)

Two general optimizer patterns were observed while trimming `cljs.pprint` and worked around in the library without changing the compiler:

- **Forward `declare` before `defn`:** `Inert::functions` in `src/application.rs` requires a global to have a single `Def` of a `fn` literal. Because `compile-format` had `(declare compile-format)` before `(defn- compile-format ...)`, `(def cached-compile (memoize compile-format))` was not recognized as inert until rewritten as a `defn` with lazy memoization.
- **Top-level regular expression literals:** `#"..."` analyzes to `re-pattern` (marked `throws` in `src/core_operations.tsv`), so unread `(def re #"...")` forms stay top-level roots even when the pattern is a valid constant literal.

---

## Build, cache, watch, test, and Node load time

Measured with [`benchmarks/cljs-pprint/bench.bb`](cljs-pprint/bench.bb), which compares requiring `cljs.pprint` (`pprint-only`) against its twin that calls `prn` (`prn`), plus a one-test `cljs.test` suite with and without `cljs.pprint` ([`pprint_test.cljs`](cljs-pprint/test/pprint_bench/pprint_test.cljs) vs. [`prn_test.cljs`](cljs-pprint/test/pprint_bench/prn_test.cljs)):

- **Uncached `--compile --optimize prepare`** (`--no-cache --stats`, light-cljs only): `prn`, `pprint-only`, and `prn-test` (for the `cljs.test` source-line compile-rate check).
- **Uncached `advanced` / `release`** (`--compile --optimize advanced --no-cache --stats` vs. `clojure -M:shadow release` with `.shadow-cljs` removed before each run), alternated across 10 rounds.
- **Warm development `compile` (nothing changed)** (`--compile --reloadable --cache-dir` vs. `clojure -M:shadow compile` with warm cache), alternated across 10 rounds.
- **Cached rebuild after a one-file edit of the calling namespace** (installing `cljs.pprint` and `clojure.string` from the persistent compiler cache while the entry namespace recompiles, verified via `--stats` cache hits/misses), alternated across 10 rounds with `clojure -M:shadow compile`.
- **Resident Java Virtual Machine `shadow.cljs.devtools.api/compile` (`shadow-warm-jvm`, after-reduction run):** runs three warm-up builds followed by 10 alternated unchanged builds and 10 alternated one-file-edit builds inside a single resident Java Virtual Machine (as a running shadow-cljs server does, without per-run Java Virtual Machine startup).
- **`light-cljs --watch` step** after editing the calling namespace (1 warm-up edit + 10 timed edits, verified on every edit that only the calling namespace recompiled and `cljs.pprint` did not).
- **`light-cljs test`** on one test that pretty-prints (`pprint-test`) vs. one that does not (`prn-test`), both uncached and warm-cached, 10 rounds each.
- **Node.js load time (time to first output and total wall time)** for both the development build (`compile`) and the `advanced` / `release` build, 10 rounds each, alternated between light-cljs and shadow-cljs, verifying byte-for-byte identical standard output on every run.

**Java Virtual Machine startup note:** Every command-line shadow-cljs build (`clojure -M:shadow compile` and `clojure -M:shadow release`) launches a fresh Java Virtual Machine, Clojure Command Line Interface, and shadow-cljs boot per run (`~4.5-5.5 s` of startup before compilation work). In normal interactive development shadow-cljs is run through a resident server/Java Virtual Machine; the `shadow-warm-jvm` table below measures cached `compile` inside one resident Java Virtual Machine without that startup cost.

Reproduce (build an immutable release V8 binary first, then run the benchmark script):

```sh
cargo build --locked --release
mkdir -p target/bench
cp target/release/light-cljs "target/bench/light-cljs-$(git rev-parse --short=8 HEAD)"
bb benchmarks/cljs-pprint/bench.bb --light "target/bench/light-cljs-$(git rev-parse --short=8 HEAD)" --runs 10
```

### Baseline setup (2026-11-27, unreserved)

- **light-cljs baseline:** release V8 build at commit `e13215e4` (`target/bench/light-cljs-e13215e4`, SHA-256 `c20c0402cd6ca867...`).
- **shadow-cljs:** `3.5.3` (with ClojureScript `1.12.134`), run via `clojure -M:shadow compile|release <id>` (fresh Java Virtual Machine per run; `shadow-warm-jvm` was added for the after-reduction run below).
- **Other tools:** Node.js `v26.12.1`, Clojure Command Line Interface `1.12.7.1681`, OpenJDK `25.0.2`, Babashka `1.13.230`.
- **Machine:** macOS `15.7.3`, Apple M3 Ultra (Virtual, `VirtualMac2,1`), 24 processor cores, 32 GiB memory.
- **Unreserved:** peer workers ran builds and test suites throughout; no run waited for a quiet machine. The 1-minute load average before each run ranged from `5.69` to `14.14` and is reported per row.
- **Repetitions:** 10 runs per measurement, alternated between compilers (and between programs where light-cljs runs alone). Tables show median with `(min-max)` range in parentheses.

### Baseline figures (`e13215e4`, unreserved)

#### Uncached `--compile --optimize prepare` (light-cljs `--no-cache --stats`)

| Program | Wall time | Processor time (user + system) | Peak memory | 1-minute load before |
|---|---:|---:|---:|---:|
| `prn` | 11.0 ms (10.4-16.2) | 0.0 ms (0.0-0.0) | 14.3 MB (14.2-14.4) | 7.80-8.21 |
| `pprint-only` | 139.9 ms (131.5-174.3) | 165.0 ms (150.0-180.0) | 99.8 MB (93.6-111.0) | 7.80-8.21 |
| `prn-test` | 64.2 ms (60.8-69.1) | 50.0 ms (50.0-60.0) | 52.7 MB (52.0-54.1) | 8.21-8.21 |

`cljs.pprint` adds **+128.9 ms** wall time and **+85.5 MB** peak memory over `prn` at `prepare` (`1,512` macro calls in `cljs.pprint`, `47` in `clojure.string`, `2` in `pprint-bench.pprint-only`). Compared to `cljs.test` (`prn-test` adds **+53.2 ms** over `prn` for `856` lines in `cljs.test`, or `1,025` lines including `clojure.walk` and `clojure.template`), `cljs.pprint` (`3,593` lines, `3,745` including `clojure.string`) has `4.20x` as many lines as `cljs.test` alone (`3.65x` including transitive bundled libraries) and takes `2.42x` as long (`0.58x` to `0.66x` of what its share of source lines predicts from `cljs.test`, well under the `2x` threshold).

#### Uncached `advanced` / `release` (light-cljs `--no-cache` vs. shadow-cljs clean `release`, fresh Java Virtual Machine per run)

| Program | Compiler | Wall time | Processor time (user + system) | Peak memory | 1-minute load before |
|---|---|---:|---:|---:|---:|
| `prn` | light-cljs | 0.022 s (0.022-0.024) | 0.010 s (0.010-0.010) | 19.4 MB (19.3-19.5) | 7.80-13.05 |
| `prn` | shadow-cljs | 9.540 s (9.069-10.848) | 38.925 s (24.680-43.570) | 1019.0 MB (864.0-1088.5) | 7.64-14.12 |
| `pprint-only` | light-cljs | 0.190 s (0.144-0.262) | 0.180 s (0.170-0.220) | 100.9 MB (98.8-103.8) | 7.80-13.05 |
| `pprint-only` | shadow-cljs | 11.280 s (9.627-13.925) | 45.090 s (27.610-57.630) | 1022.3 MB (948.0-1239.1) | 7.48-14.14 |
| `prn-test` | light-cljs | 0.073 s (0.070-0.076) | 0.060 s (0.060-0.070) | 54.7 MB (53.7-55.3) | 13.03-13.03 |

`cljs.pprint` adds **+168 ms** (+81.5 MB peak memory) to an uncached `advanced` build in light-cljs, compared to **+1,740 ms** (+3.3 MB peak memory) in shadow-cljs.

#### Warm development `compile` (nothing changed: light-cljs `--reloadable` vs. shadow-cljs command-line `compile`, fresh Java Virtual Machine per run)

| Program | Compiler | Wall time | Processor time (user + system) | Peak memory | 1-minute load before |
|---|---|---:|---:|---:|---:|
| `prn` | light-cljs | 0.022 s (0.021-0.024) | 0.010 s (0.010-0.010) | 19.5 MB (19.3-19.6) | 5.69-9.38 |
| `prn` | shadow-cljs | 4.867 s (4.308-5.626) | 15.235 s (10.130-21.340) | 583.1 MB (503.6-627.3) | 5.69-9.38 |
| `pprint-only` | light-cljs | 0.061 s (0.037-0.096) | 0.025 s (0.020-0.050) | 36.0 MB (35.9-36.6) | 5.69-9.38 |
| `pprint-only` | shadow-cljs | 4.891 s (4.419-5.345) | 14.420 s (11.710-20.010) | 580.6 MB (512.4-646.6) | 6.36-9.27 |

#### Cached rebuild after a one-file edit (installing `cljs.pprint` from persistent compiler cache; shadow-cljs fresh Java Virtual Machine per run)

| Program | Mode | Wall time | Processor time (user + system) | Peak memory | 1-minute load before |
|---|---|---:|---:|---:|---:|
| `prn` | light-cljs `prepare` | 0.016 s (0.013-0.018) | 0.000 s (0.000-0.000) | 15.3 MB (15.2-15.5) | 7.75-8.36 |
| `prn` | light-cljs `advanced` | 0.026 s (0.024-0.027) | 0.010 s (0.010-0.010) | 20.2 MB (20.1-20.3) | 7.75-8.36 |
| `prn` | shadow-cljs `compile` | 4.619 s (4.495-4.841) | 17.520 s (10.840-18.600) | 602.0 MB (575.1-665.4) | 7.73-8.50 |
| `pprint-only` | light-cljs `prepare` | 0.033 s (0.028-0.038) | 0.020 s (0.020-0.020) | 33.5 MB (33.4-33.7) | 7.75-8.36 |
| `pprint-only` | light-cljs `advanced` | 0.069 s (0.044-0.103) | 0.035 s (0.030-0.050) | 34.5 MB (34.2-34.7) | 7.75-8.36 |
| `pprint-only` | shadow-cljs `compile` | 4.476 s (4.266-5.219) | 15.455 s (10.660-18.730) | 615.4 MB (584.6-658.1) | 7.53-8.57 |

Installing `cljs.pprint` and `clojure.string` from the persistent cache (verified `cache.hits >= 2`, `cache.misses = 1`) adds **+17 ms** at `prepare` and **+43 ms** at `advanced` (and drops peak memory from `100 MB` uncached to `33.5-34.5 MB` because the V8 macro host never starts). In command-line shadow-cljs, the `pprint-only` vs. `prn` median difference was `-143 ms` (`4.476 s` vs. `4.619 s`), inside the ~1 s fresh-Java-Virtual-Machine startup jitter.

#### `light-cljs --watch` step after editing the calling namespace (library not recompiled)

| Program | Compiler step | Edit-to-log (`--poll-ms 50`) | Watch peak memory (single session) | 1-minute load before |
|---|---:|---:|---:|---:|
| `prn` | 0.9 ms (0.8-1.1) | 20.4 ms (3.5-52.9) | 21.0 MB | 11.48-12.22 |
| `pprint-only` | 1.1 ms (1.0-1.7) | 56.4 ms (3.6-63.1) | 118.4 MB | 11.48-11.48 |

Every timed edit recompiled only the calling namespace (`cljs.pprint` was never recompiled). The library adds **+0.2 ms** to the compiler's own incremental step (`0.9 ms` -> `1.1 ms`), while edit-to-log median rose from `20.4 ms` (`3.5-52.9 ms`) to `56.4 ms` (`3.6-63.1 ms`), with overlapping ranges dominated by the `50 ms` polling period. Watch peak memory is higher (`118.4 MB` vs. `21.0 MB`) because the startup build starts the V8 macro host to compile `cljs.pprint`.

#### `light-cljs test` (one-test suite)

| Mode | Program | Wall time | Processor time (user + system) | Peak memory | 1-minute load before |
|---|---|---:|---:|---:|---:|
| uncached (`--no-cache`) | `prn-test` | 104.4 ms (101.4-113.1) | 110.0 ms (100.0-110.0) | 66.7 MB (62.2-71.5) | 11.21-11.48 |
| uncached (`--no-cache`) | `pprint-test` | 217.6 ms (210.0-233.1) | 260.0 ms (250.0-290.0) | 121.9 MB (114.2-132.8) | 11.21-11.48 |
| warm cached | `prn-test` | 52.3 ms (49.2-53.6) | 40.0 ms (40.0-40.0) | 43.6 MB (41.1-47.9) | 11.21-11.21 |
| warm cached | `pprint-test` | 78.6 ms (73.2-82.9) | 80.0 ms (60.0-80.0) | 65.5 MB (62.7-67.5) | 11.21-11.21 |

`cljs.pprint` adds **+113.2 ms** uncached (+55.2 MB peak memory) and **+26.3 ms** cached (+21.9 MB peak memory) to `light-cljs test`.

#### Node.js load time: development build (time to first output)

| Program | Compiler | First output | Wall time | Processor time (user + system) | Peak memory | 1-minute load before |
|---|---|---:|---:|---:|---:|---:|
| `prn` | light-cljs | 64.8 ms (62.5-77.7) | 66.8 ms (64.7-79.8) | 60.0 ms (60.0-60.0) | 51.2 MB (50.3-52.1) | 8.61-8.61 |
| `prn` | shadow-cljs | 86.5 ms (84.1-90.3) | 89.2 ms (86.8-94.7) | 80.0 ms (80.0-80.0) | 67.0 MB (65.9-67.8) | 8.61-8.61 |
| `pprint-only` | light-cljs | 83.3 ms (80.9-93.0) | 85.7 ms (83.7-95.8) | 80.0 ms (80.0-90.0) | 63.8 MB (60.9-64.5) | 8.61-8.61 |
| `pprint-only` | shadow-cljs | 106.6 ms (104.4-109.1) | 109.7 ms (107.5-112.6) | 110.0 ms (110.0-110.0) | 80.6 MB (76.1-84.2) | 8.61-8.61 |

#### Node.js load time: `advanced` / `release` build (time to first output)

| Program | Compiler | First output | Wall time | Processor time (user + system) | Peak memory | 1-minute load before |
|---|---|---:|---:|---:|---:|---:|
| `prn` | light-cljs | 56.7 ms (55.1-64.0) | 58.8 ms (57.3-65.9) | 50.0 ms (50.0-50.0) | 44.1 MB (43.8-45.4) | 12.22-13.03 |
| `prn` | shadow-cljs | 55.8 ms (53.1-60.3) | 58.0 ms (55.0-63.3) | 50.0 ms (50.0-50.0) | 43.3 MB (43.0-44.0) | 12.22-13.03 |
| `pprint-only` | light-cljs | 70.1 ms (68.7-75.9) | 72.6 ms (70.8-79.1) | 70.0 ms (70.0-80.0) | 54.0 MB (52.8-56.3) | 12.22-13.03 |
| `pprint-only` | shadow-cljs | 70.2 ms (65.9-74.3) | 72.7 ms (68.0-77.0) | 70.0 ms (70.0-70.0) | 53.6 MB (52.4-55.7) | 12.22-13.03 |

### Baseline `--stats` and `--stats-file` diagnosis (`e13215e4`)

In an uncached `--compile --optimize prepare` build of `pprint-only` (`122.3 ms` reported internal wall time in `bench.bb`, `1,561` macro calls: `1,512` in `cljs.pprint`, `47` in `clojure.string`, `2` in `pprint-bench.pprint-only`), the exclusive stage trace splits the cost into:

- **Top-level analysis (`trace.top-level-ms` `33.8 ms`, `trace.top-level.engine-ms` `14.2 ms`):** expanding `cljs.pprint`'s top-level macros (`defdirectives`, five `def-pprint-type` forms, and defining 11 namespace macros).
- **Deferred units (`trace.unit-ms` `45.3 ms`, `trace.unit.engine-ms` `18.6 ms`):** out of `296` deferred units, `250` non-host units take `13.6 ms` combined (`0.054 ms` each in the `--stats-file` trace), while `46` V8-host units take `33.2 ms` (`trace.units.host-ms`) across `5,310` engine entries.
- **Drain and V8 macro-host startup (`trace.drain-ms` `26.8 ms`, `trace.drain.bookkeeping-ms` `30.1 ms`):** starting the embedded V8 macro host on the first non-native macro call is a fixed compiler cost shared by any build that invokes a non-native macro (`prn-test` pays `27.3 ms` in `trace.drain-ms` / `28.0 ms` in `trace.drain.bookkeeping-ms` for `cljs.test`).

The two largest library contributors in `src/compiler_libraries/cljs/pprint.cljs` are:

1. **`defdirectives` (single largest contributor):** one giant top-level macro form at line 2394 that passes all 34 directive definitions through the V8 macro host during top-level expansion and generates `cljs.pprint/get-directive-table`, the single slowest deferred unit in the compilation (`11.1 ms`, `6.8 ms` V8 engine, `112` engine calls, `101` expansions in the `--stats-file` trace; `trace.unit.longest-ms` `11.2 ms` in the `bench.bb` `--stats` capture — `23%` of all deferred-unit time).
2. **`getf` and `setf` macros (second largest contributor):** `60` active calls to `(getf :field)` and `19` active calls to `(setf :field val)` across `21` pretty-writer units (`write-tokens`, `write-initial-lines`, `pretty-writer`, `start-block`, `end-block`, `write-token` methods, and helpers; `2` more `getf` and `1` `setf` sit inside a `(comment ...)` example at the bottom of `pprint.cljs`). Upstream already defines plain functions `get-field` and `set-field` for the exact same `@@this` map reads and `swap!` updates; using the macros turns all `21` units into V8-host units totaling **`7.1 ms`** and **`925` V8 engine calls** (`53%` of all unit engine calls in the `--stats-file` trace).

### After reducing the two largest contributors (`2d303398`, unreserved)

**Changes in `src/compiler_libraries/cljs/pprint.cljs` (`2d303398`, rebased onto `light-cljs-1175` `bb1c61c1`):**
1. Replaced the top-level `defdirectives` macro (and the unused `process-directive-table-element` helper, plus the `hash-map` and three `dotimes` macro calls inside the generated table) with a plain map literal `{...}` of `directive-entry` calls and `loop` iterations in `get-directive-table`.
2. Replaced all `60` active `(getf ...)` and `19` active `(setf ...)` macro calls in `pretty-writer` with direct calls to the existing `get-field` and `set-field` functions (keeping the private `getf` and `setf` macro definitions intact; the `2` `getf` and `1` `setf` inside `(comment ...)` are unchanged).

**Setup (2026-11-27, unreserved):**
- **light-cljs after (shipped branch build):** release V8 build at commit `2d303398` (`target/bench/light-cljs-2d303398`, SHA-256 `3b838b69a9a89660...`), which includes `light-cljs-1175` (`bb1c61c1`) in addition to the `pprint.cljs` reductions.
- **Pre-rebase paired binary (`d60bf8a2`):** the same `pprint.cljs` diff before rebasing onto `light-cljs-1175`, built at commit `d60bf8a2` (`SHA-256 43dadea0ac8bf629...`) solely for the apples-to-apples paired comparison against `e13215e4` below (differing only in `pprint.cljs`).
- **shadow-cljs and other tools:** same as baseline (`shadow-cljs 3.5.3`, Node.js `v26.12.1`, Clojure Command Line Interface `1.12.7.1681`, OpenJDK `25.0.2`, Babashka `1.13.230`), plus the new `shadow-warm-jvm` resident-Java-Virtual-Machine scenario.
- **Unreserved:** peer workers ran builds and test suites throughout; 1-minute load average ranged from `7.84` to `16.88` across the `2d303398` suite (`~8.0-8.9` during the pre-rebase paired check).

#### Paired before vs. after (pre-rebase pair `e13215e4` vs. `d60bf8a2`, 10 alternated rounds on `pprint-only`, uncached, unreserved)

| Optimization level | light-cljs before (`e13215e4`) | light-cljs after (`d60bf8a2`) | Wall change | Peak memory before -> after |
|---|---:|---:|---:|---:|
| `--optimize prepare` | 139.8 ms (135.0-236.9) | 113.2 ms (110.2-123.5) | **-26.6 ms (-19.0%)** | 96.7 MB -> **87.3 MB** (-9.4 MB) |
| `--optimize advanced` | 152.5 ms (148.4-167.1) | 129.7 ms (126.6-143.0) | **-22.8 ms (-14.9%)** | 100.5 MB -> **88.3 MB** (-12.2 MB) |

#### `--stats` before (`e13215e4`) vs. after (`2d303398`, `pprint-only`, uncached `--optimize prepare`)

| Counter | Before (`e13215e4`) | After (`2d303398`) | Change |
|---|---:|---:|---:|
| `macro-calls` (`cljs.pprint` share) | 1,561 (1,512) | 1,473 (1,424) | -88 (-5.6%) |
| `trace.top-level.engine-ms` | 14.2 ms | 6.0 ms | -8.2 ms (-58%) |
| `trace.unit.engine-ms` | 18.6 ms | 13.1 ms | -5.5 ms (-30%) |
| `trace.engine-ms` | 37.3 ms | 23.6 ms | -13.7 ms (-37%) |
| `trace.engine.entries` | 5,310 | 3,187 | -2,123 (-40%) |
| `trace.units.host` | 46 | 24 | -22 (-48%) |
| `trace.units.host-ms` | 33.2 ms | 19.8 ms | -13.4 ms (-40%) |
| `cljs.pprint/get-directive-table` unit (`--stats-file`) | 11.1 ms (`host=1`, 112 calls) | 1.5 ms (`host=0`, 0 calls) | -9.6 ms (-86%) |
| `trace.unit.longest-ms` | 11.2 ms | 2.6 ms | -8.6 ms (-77%) |
| `memory.peak-rss-bytes` (peak memory) | 100.7 MB | 87.1 MB | -13.6 MB (-14%) |

#### Full 10-round suite on rebased `2d303398` vs. shadow-cljs 3.5.3 (unreserved)

##### Build, cache, watch, and test

| Scenario | Program | light-cljs (`2d303398`) | shadow-cljs 3.5.3 | light-cljs peak memory | shadow-cljs peak memory | 1-minute load before |
|---|---|---:|---:|---:|---:|---:|
| Uncached `prepare` (`--no-cache`) | `prn` | 11.8 ms (11.0-57.8) | — | 14.4 MB (14.2-14.9) | — | 15.35-15.35 |
| Uncached `prepare` (`--no-cache`) | `pprint-only` | 121.0 ms (112.0-160.0) | — | 92.6 MB (87.3-94.6) | — | 15.35-15.35 |
| Uncached `prepare` (`--no-cache`) | `prn-test` | 64.4 ms (61.8-89.7) | — | 54.2 MB (53.2-56.4) | — | 15.35-15.35 |
| Uncached `advanced` / `release` (shadow: fresh Java Virtual Machine) | `prn` | 0.023 s (0.022-0.040) | 9.751 s (8.965-10.213) | 19.5 MB (19.3-19.7) | 922.7 MB (820.1-1042.3) | 7.84-15.35 |
| Uncached `advanced` / `release` (shadow: fresh Java Virtual Machine) | `pprint-only` | 0.174 s (0.125-0.237) | 9.972 s (9.516-10.397) | 89.5 MB (84.9-94.8) | 1118.1 MB (890.6-1226.5) | 7.99-15.35 |
| Uncached `advanced` / `release` | `prn-test` | 0.073 s (0.072-0.075) | — | 54.3 MB (54.0-55.4) | — | 8.79-8.79 |
| Warm dev `compile` (unchanged; shadow: fresh Java Virtual Machine) | `prn` | 0.023 s (0.021-0.104) | 5.399 s (4.735-6.249) | 19.5 MB (19.4-20.6) | 605.2 MB (513.7-635.1) | 14.92-16.29 |
| Warm dev `compile` (unchanged; shadow: fresh Java Virtual Machine) | `pprint-only` | 0.071 s (0.036-0.110) | 5.258 s (4.986-6.591) | 36.1 MB (35.7-36.9) | 601.2 MB (526.5-664.5) | 14.92-16.10 |
| Cached one-file edit (`prepare` vs. shadow command-line `compile`, fresh Java Virtual Machine) | `prn` | 0.017 s (0.015-0.019) | 5.307 s (5.057-5.971) | 15.4 MB (15.3-15.5) | 611.1 MB (557.8-646.8) | 15.56-16.88 |
| Cached one-file edit (`prepare` vs. shadow command-line `compile`, fresh Java Virtual Machine) | `pprint-only` | 0.036 s (0.028-0.039) | 5.580 s (4.738-7.703) | 33.0 MB (32.6-33.3) | 590.0 MB (535.0-674.4) | 15.56-16.88 |
| Cached one-file edit (`advanced`) | `prn` | 0.027 s (0.026-0.063) | — | 20.3 MB (20.1-20.5) | — | 15.56-16.88 |
| Cached one-file edit (`advanced`) | `pprint-only` | 0.075 s (0.044-0.119) | — | 33.1 MB (32.6-33.9) | — | 15.56-16.88 |
| `--watch` step (compiler / edit-to-log, `--poll-ms 50`) | `prn` | 0.9 ms (0.7-1.2) / 17.2 ms (1.6-63.1) | — | 20.9 MB (single session) | — | 8.41-8.41 |
| `--watch` step (compiler / edit-to-log, `--poll-ms 50`) | `pprint-only` | 1.0 ms (0.9-1.6) / 18.4 ms (4.3-59.7) | — | 100.8 MB (single session) | — | 7.98-8.41 |
| `light-cljs test` (uncached) | `prn-test` | 97.1 ms (93.9-112.1) | — | 66.9 MB (61.7-71.5) | — | 7.90-7.98 |
| `light-cljs test` (uncached) | `pprint-test` | 179.1 ms (174.6-194.0) | — | 113.2 MB (109.0-118.5) | — | 7.98-7.98 |
| `light-cljs test` (warm cached) | `prn-test` | 49.6 ms (48.2-51.2) | — | 41.3 MB (41.0-45.5) | — | 7.90-7.90 |
| `light-cljs test` (warm cached) | `pprint-test` | 74.4 ms (71.1-75.9) | — | 64.8 MB (60.8-66.8) | — | 7.90-7.90 |

##### shadow-cljs `compile` in one resident Java Virtual Machine (`shadow.cljs.devtools.api/compile`, no Java Virtual Machine startup)

Measured in one resident Java Virtual Machine (`652.0 MB` process peak memory) after three warm-up builds of each target, 10 alternated builds per mode:

| Mode | Program | Wall time | Java Virtual Machine processor time | 1-minute load before |
|---|---|---:|---:|---:|
| unchanged | `prn` | 269.5 ms (221.3-304.7) | 482.3 ms (377.7-736.8) | 15.81-16.40 |
| unchanged | `pprint-only` | 281.6 ms (228.4-467.4) | 567.9 ms (406.1-1188.4) | 15.81-16.40 |
| one-file edit | `prn` | 261.9 ms (219.1-308.9) | 434.4 ms (378.1-533.7) | 15.35-15.81 |
| one-file edit | `pprint-only` | 271.4 ms (235.1-346.8) | 451.5 ms (420.4-562.7) | 15.35-15.81 |

##### Node.js load time (`2d303398` vs. shadow-cljs 3.5.3)

| Build | Program | Compiler | First output | Wall time | Processor time (user + system) | Peak memory | 1-minute load before |
|---|---|---|---:|---:|---:|---:|---:|
| development (`compile`) | `prn` | light-cljs | 67.9 ms (65.5-73.2) | 70.2 ms (68.0-76.6) | 60.0 ms (60.0-70.0) | 51.6 MB (50.7-53.1) | 15.93-16.10 |
| development (`compile`) | `prn` | shadow-cljs | 94.0 ms (87.3-113.9) | 96.9 ms (90.0-116.9) | 80.0 ms (80.0-90.0) | 69.1 MB (66.3-71.9) | 15.93-16.10 |
| development (`compile`) | `pprint-only` | light-cljs | 88.3 ms (85.9-110.3) | 91.6 ms (88.7-113.2) | 95.0 ms (90.0-100.0) | 66.0 MB (63.8-68.2) | 15.93-16.10 |
| development (`compile`) | `pprint-only` | shadow-cljs | 115.0 ms (108.4-133.7) | 118.6 ms (111.3-139.6) | 120.0 ms (110.0-140.0) | 83.4 MB (78.5-86.8) | 15.93-16.10 |
| `advanced` / `release` | `prn` | light-cljs | 55.6 ms (54.5-208.4) | 57.6 ms (56.4-210.9) | 50.0 ms (50.0-70.0) | 44.2 MB (43.8-45.4) | 8.41-8.79 |
| `advanced` / `release` | `prn` | shadow-cljs | 55.1 ms (53.3-58.2) | 57.9 ms (55.4-61.1) | 50.0 ms (50.0-50.0) | 44.1 MB (43.1-45.8) | 8.41-8.79 |
| `advanced` / `release` | `pprint-only` | light-cljs | 68.2 ms (67.0-77.8) | 70.4 ms (69.2-80.4) | 70.0 ms (70.0-70.0) | 54.2 MB (52.9-59.0) | 8.41-8.79 |
| `advanced` / `release` | `pprint-only` | shadow-cljs | 68.9 ms (67.4-77.2) | 71.5 ms (69.7-80.1) | 70.0 ms (70.0-70.0) | 55.2 MB (53.1-58.4) | 8.41-8.79 |

#### Library added cost summary (`pprint-only` minus `prn`, and `pprint-test` minus `prn-test`, at `2d303398`)

| Scenario | light-cljs added time | shadow-cljs added time | light-cljs added peak memory | shadow-cljs added peak memory |
|---|---:|---:|---:|---:|
| Uncached `prepare` (`--no-cache`) | +109.2 ms | — | +78.2 MB | — |
| Uncached `advanced` / `release` (shadow: fresh Java Virtual Machine) | +151 ms | +221 ms | +70.0 MB | +195.4 MB |
| Warm dev `compile`, unchanged (shadow: fresh Java Virtual Machine) | +48 ms | -141 ms (within ~1.5 s boot jitter) | +16.6 MB | -4.0 MB |
| Warm dev `compile`, unchanged (shadow: resident Java Virtual Machine) | +48 ms | +12.1 ms | +16.6 MB | shared `652.0 MB` resident process |
| Cached one-file edit, `prepare` / `advanced` (shadow: fresh Java Virtual Machine `compile`) | +19 ms / +48 ms | +273 ms | +17.6 MB / +12.8 MB | -21.1 MB |
| Cached one-file edit, `prepare` / `advanced` (shadow: resident Java Virtual Machine `compile`) | +19 ms / +48 ms | +9.5 ms | +17.6 MB / +12.8 MB | shared `652.0 MB` resident process |
| `--watch` step (compiler / edit-to-log) | +0.1 ms / +1.2 ms | — | +79.9 MB (single session) | — |
| `light-cljs test` (uncached / warm cached) | +82.0 ms / +24.8 ms | — | +46.3 MB / +23.5 MB | — |
| Node.js development load (first output / wall) | +20.4 ms / +21.4 ms | +21.0 ms / +21.7 ms | +14.4 MB | +14.3 MB |
| Node.js `advanced` / `release` load (first output / wall) | +12.6 ms / +12.8 ms | +13.8 ms / +13.6 ms | +10.0 MB | +11.1 MB |

At `2d303398`, `cljs.pprint` adds `+109.2 ms` to uncached `prepare` vs. `+52.6 ms` for `prn-test` (`2.08x` for `4.20x` as many lines as `cljs.test` alone, or `3.65x` including transitive bundled libraries — `0.49x` to `0.57x` of what its share of source lines predicts).

### Final wins, losses, stop rule, and remaining costs (`2d303398`)

- **Where light-cljs wins:**
  - **Uncached `advanced` / `release`:** light-cljs builds `pprint-only` in `0.174 s` (`0.125-0.237 s`) at `89.5 MB` peak memory vs. shadow-cljs's `9.972 s` (`9.516-10.397 s`, includes ~5 s fresh Java Virtual Machine startup) at `1118.1 MB`; the library's added cost is also smaller in light-cljs (`+151 ms` and `+70.0 MB` vs. `+221 ms` and `+195.4 MB`).
  - **Cached total wall time and peak memory (even against a resident shadow-cljs Java Virtual Machine with no Java Virtual Machine startup):**
    - Warm unchanged dev `compile` (`pprint-only`): light-cljs `71 ms` (`36-110 ms`, `36.1 MB`) vs. shadow-cljs resident Java Virtual Machine `281.6 ms` (`228.4-467.4 ms`, `652.0 MB` resident process) — `4.0x` faster total time at `18x` lower peak memory (against command-line shadow-cljs with a fresh Java Virtual Machine per run, `71 ms` vs. `5.258 s`, which is mostly ~5 s of Java Virtual Machine startup).
    - Cached one-file edit (`pprint-only`): light-cljs `36 ms` (`28-39 ms`, `33.0 MB`) at `prepare` and `75 ms` (`44-119 ms`, `33.1 MB`) at `advanced` vs. shadow-cljs resident Java Virtual Machine `271.4 ms` (`235.1-346.8 ms`, `652.0 MB` resident process) — `7.5x` (`prepare`) and `3.6x` (`advanced`) faster total time at `20x` lower peak memory (against command-line shadow-cljs with a fresh Java Virtual Machine per run, `5.580 s`, again mostly Java Virtual Machine startup).
  - **Development build Node.js load (`prn` and `pprint-only`):** `88.3 ms` (`85.9-110.3 ms`) vs. `115.0 ms` (`108.4-133.7 ms`) first output and `66.0 MB` vs. `83.4 MB` peak memory on `pprint-only`; `67.9 ms` (`65.5-73.2 ms`) vs. `94.0 ms` (`87.3-113.9 ms`) on `prn`. The library's added development load cost is level (`+20.4 ms` vs. `+21.0 ms`).
  - **Before vs. after reductions in light-cljs:** paired uncached `pprint-only` compile fell `-19.0%` at `prepare` (`139.8` -> `113.2 ms`, `-9.4 MB`) and `-14.9%` at `advanced` (`152.5` -> `129.7 ms`, `-12.2 MB`); `light-cljs test` uncached delta fell from `+113.2 ms` (`e13215e4`) to `+82.0 ms` (`2d303398`); `--watch` peak memory fell from `118.4 MB` to `100.8 MB` (`1.0 ms` compiler step, `18.4 ms` edit-to-log, library never recompiled).
- **Where light-cljs is level (overlapping ranges):**
  - **`advanced` / `release` Node.js load (`pprint-only` and `prn`):** `pprint-only` first output is `68.2 ms` (`67.0-77.8 ms`) vs. shadow-cljs `68.9 ms` (`67.4-77.2 ms`), wall `70.4 ms` (`69.2-80.4 ms`) vs. `71.5 ms` (`69.7-80.1 ms`), peak memory `54.2 MB` (`52.9-59.0 MB`) vs. `55.2 MB` (`53.1-58.4 MB`); `prn` first output is `55.6 ms` (`54.5-208.4 ms`, one high outlier under load) vs. `55.1 ms` (`53.3-58.2 ms`), wall `57.6 ms` (`56.4-210.9 ms`) vs. `57.9 ms` (`55.4-61.1 ms`), peak memory `44.2 MB` (`43.8-45.4 MB`) vs. `44.1 MB` (`43.1-45.8 MB`). All ranges overlap; light-cljs's `prn` median is `+0.5 ms` slower on first output and `+0.1 MB` higher peak memory, while its `pprint-only` median is `-0.7 ms` faster and `-1.0 MB` lower peak memory (added cost `+12.6 ms` vs. `+13.8 ms`).
- **Where light-cljs loses:**
  - **Added cost of `cljs.pprint` on cached builds against a resident shadow-cljs Java Virtual Machine:** light-cljs starts a fresh process and decodes/installs the cached `cljs.pprint` artifact from disk on every command-line run, adding **`+48 ms`** to warm unchanged `--reloadable` builds and **`+19 ms` (`prepare`) / `+48 ms` (`advanced`)** to cached one-file-edit builds, whereas shadow-cljs in a resident Java Virtual Machine (already holding `cljs.pprint`'s analyzer state in memory) adds **`+12.1 ms`** unchanged and **`+9.5 ms`** on a one-file edit.
  - **Added cost on command-line warm dev `compile` (and baseline cached one-file edit):** light-cljs adds `+48 ms` (`23 ms` -> `71 ms`) on warm unchanged dev `compile` while command-line shadow-cljs's median difference is `-141 ms` (`5.399 s` -> `5.258 s`, noise inside its ~1.5 s Java Virtual Machine startup range). Similarly, in the `e13215e4` baseline cached one-file-edit row, light-cljs added `+17 ms` (`prepare`) / `+43 ms` (`advanced`) while command-line shadow-cljs's median difference was `-143 ms` (`4.619 s` -> `4.476 s`).
  - **Added peak memory on cached command-line builds (vs. shadow-cljs's already-warm Java Virtual Machine):** light-cljs's peak memory rises `+12.8` to `+17.6 MB` (`15-20 MB` -> `33-36 MB`) when installing `cljs.pprint` from cache, whereas shadow-cljs's Java Virtual Machine already sits around `590-611 MB` for `prn` so adding `cljs.pprint` changes its peak memory within noise (`-4.0` to `-21.1 MB`).
  - **`prn` baseline median first output and upper-tail outlier on `advanced` Node.js load:** light-cljs `55.6 ms` (`54.5-208.4 ms`) vs. shadow-cljs `55.1 ms` (`53.3-58.2 ms`), reflecting the larger `prn` runtime baseline (`118,064` vs. `103,116` raw bytes).
- **Stop rule (`light-cljs-1172`):** Met by the **two largest contributors addressed** clause (`defdirectives` and `getf`/`setf`). The "every row at or below shadow-cljs" clause does **not** hold: light-cljs loses on cached added cost (against resident-Java-Virtual-Machine shadow-cljs and noisy command-line warm dev compile), `prn` `advanced` Node.js load median is `+0.5 ms` slower (within overlapping ranges), and uncached `prepare`, `--watch`, and `light-cljs test` have no shadow-cljs comparison row.
- **What remains in `cljs.pprint` (recorded, not changed here):**
  - `24` remaining V8-host units (`19.8 ms` total in `prepare` at `2d303398`): `21` of them call `cljs.pprint`'s own macros (`pprint-logical-block`, `print-length-loop`, `formatter-out` in `code-dispatch`, and `with-pretty-writer` in `pprint`/`write`); the remaining three are `clojure.string/reverse`, `cljs.pprint/string-buffer`, and `pprint-bench.pprint-only/main` (`1-2` engine bookkeeping calls each).
  - Top-level `def-pprint-type` (5 calls) and remaining macro registrations (`6.0 ms` in `trace.top-level.engine-ms`).
- **General compiler slownesses observed (recorded, not changed here):**
  - **`cljs.core/hash-map` and `cljs.core/dotimes` are V8 core macros, not native core macros:** `hash-map` and `dotimes` are listed in `src/compiler_names.rs::core_macro` but absent from `src/native_core_macros.rs`. Replacing `(defdirectives ...)` alone still left `get-directive-table` as a V8-host unit until `(hash-map ...)` (34 entries) was replaced by a map literal `{...}` and `dotimes` by `loop`, which dropped `get-directive-table` from `11.1 ms` (`host=1`, `112` engine calls) to `1.5 ms` (`host=0`, `0` engine calls) in `--stats-file`.
  - **Fixed V8 macro-host startup cost (`trace.drain-ms` / `trace.drain.bookkeeping-ms`):** any compilation that invokes at least one non-native macro pays `~25-30 ms` of fixed V8 macro-host startup and drain bookkeeping (measured at `26.8 ms` / `30.2 ms` for `pprint-only` and `25.3 ms` / `26.1 ms` for `prn-test` at `prepare` on `2d303398`), whereas macro-free builds like `prn` skip V8 startup entirely (`11.8 ms` total, `14.4 MB` peak memory).
