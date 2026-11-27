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

- **light-cljs before:** release V8 build at commit `e9cb291c` (`target/bench/light-cljs-e9cb291c`).
- **light-cljs after:** release V8 build at commit `bbe99a12` (`target/bench/light-cljs-bbe99a12`, pellet `light-cljs-1171`).
- **shadow-cljs:** `3.5.3` (with ClojureScript `1.12.134`), run via `clojure -M:shadow release <build>`.
- **Other tools:** Node.js `v26.12.1`, Clojure Command Line Interface `1.12.7.1681`, OpenJDK `25.0.2`, Babashka `v1.13.230`.
- **Machine load (unreserved):** baseline run load average `4.59 6.94 7.54`; after run load average `5.37 4.98 5.76`.

### Before vs. after (`advanced` / `release`)

#### shadow-cljs 3.5.3 (unchanged across both runs)

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

#### light-cljs after (`bbe99a12`, `light-cljs-1171`)

| Program | Raw bytes | Delta vs. `prn` | Change vs. before | `gzip -9` bytes | Delta vs. `prn` | Change vs. before |
|---|---:|---:|---:|---:|---:|---:|
| `prn` | 118,064 | 0 | 0 | 30,947 | 0 | 0 |
| `pprint-only` | 198,748 | +80,684 | -77,363 | 53,176 | +22,229 | -19,389 |
| `cl-format-only` | 257,026 | +138,962 | -18,529 | 68,860 | +37,913 | -3,558 |
| `code-dispatch` | 276,015 | +157,951 | -220 | 72,730 | +41,783 | +90 |

#### Summary (wins and losses across all four programs)

- **`pprint-only` (wins on deltas and total raw; loses on total `gzip -9`):**
  - Raw delta vs. `prn`: light-cljs `+80,684` bytes vs. shadow-cljs `+142,782` bytes (**62,098 bytes less** added by light-cljs).
  - `gzip -9` delta vs. `prn`: light-cljs `+22,229` bytes vs. shadow-cljs `+31,073` bytes (**8,844 bytes less** added by light-cljs).
  - Total raw output: light-cljs `198,748` bytes vs. shadow-cljs `245,898` bytes (**47,150 bytes smaller**).
  - Total `gzip -9` output: light-cljs `53,176` bytes vs. shadow-cljs `52,350` bytes (**light-cljs is 826 bytes larger**, because its `prn` baseline starts `9,670` gzip bytes higher: `30,947` vs. `21,277`).
- **`cl-format-only` (wins on raw delta; loses on total raw, total `gzip -9`, and `gzip -9` delta):**
  - Raw delta vs. `prn`: light-cljs `+138,962` vs. shadow-cljs `+142,187` bytes (`3,225` bytes less added).
  - Total raw output: light-cljs `257,026` vs. shadow-cljs `245,303` bytes (`11,723` bytes larger).
  - `gzip -9`: light-cljs `68,860` (`+37,913` delta) vs. shadow-cljs `52,106` (`+30,829` delta) — **light-cljs adds 7,084 more gzip bytes** and is `16,754` total gzip bytes larger.
- **`code-dispatch` (loses on every column; `gzip -9` grew `+90` bytes vs. before):**
  - Raw: light-cljs `276,015` (`+157,951` delta) vs. shadow-cljs `245,758` (`+142,642` delta) — `15,309` more raw bytes added (`30,257` total raw larger).
  - `gzip -9`: light-cljs `72,730` (`+41,783` delta) vs. shadow-cljs `52,291` (`+31,014` delta) — `10,769` more gzip bytes added (`20,439` total gzip larger). Compared to `e9cb291c` before this change, `code-dispatch` shrank `220` raw bytes (`276,235` -> `276,015`) but **grew `+90` `gzip -9` bytes** (`72,640` -> `72,730`) from the lazy initialization wrappers and `defmethod` closures.
- **`prn` baseline (loses on raw and `gzip -9`):** light-cljs `118,064` raw / `30,947` `gzip -9` vs. shadow-cljs `103,116` raw / `21,277` `gzip -9` (`14,948` raw and `9,670` gzip bytes larger before requiring `cljs.pprint`).
- **Stop rule (`light-cljs-1171`):** both targets are met — `pprint-only` adds fewer raw and gzipped bytes over `prn` than shadow-cljs (`+80,684` vs. `+142,782` raw; `+22,229` vs. `+31,073` `gzip -9`), and `--size-report` shows nothing of `cl-format` or `code-dispatch` in data-only builds.

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
- **Cached rebuild after a one-file edit of the calling namespace** (installing `cljs.pprint` and `clojure.string` from the persistent compiler cache while the entry namespace recompiles, verified via `--stats` cache hits/misses), alternated across 10 rounds with shadow-cljs `compile`.
- **`light-cljs --watch` step** after editing the calling namespace (1 warm-up edit + 10 timed edits, verified on every edit that only the calling namespace recompiled and `cljs.pprint` did not).
- **`light-cljs test`** on one test that pretty-prints (`pprint-test`) vs. one that does not (`prn-test`), both uncached and warm-cached, 10 rounds each.
- **Node.js load time (time to first output and total wall time)** for both the development build (`compile`) and the `advanced` / `release` build, 10 rounds each, alternated between light-cljs and shadow-cljs, verifying byte-for-byte identical standard output on every run.

Reproduce (build an immutable release V8 binary first, then run the benchmark script):

```sh
cargo build --locked --release
mkdir -p target/bench
cp target/release/light-cljs "target/bench/light-cljs-$(git rev-parse --short=8 HEAD)"
bb benchmarks/cljs-pprint/bench.bb --light "target/bench/light-cljs-$(git rev-parse --short=8 HEAD)" --runs 10
```

### Baseline setup (2026-11-27, unreserved)

- **light-cljs baseline:** release V8 build at commit `e13215e4` (`target/bench/light-cljs-e13215e4`, SHA-256 `c20c0402cd6ca867...`).
- **shadow-cljs:** `3.5.3` (with ClojureScript `1.12.134`), run via `clojure -M:shadow compile|release <id>`.
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

#### Uncached `advanced` / `release` (light-cljs `--no-cache` vs. shadow-cljs clean `release`)

| Program | Compiler | Wall time | Processor time (user + system) | Peak memory | 1-minute load before |
|---|---|---:|---:|---:|---:|
| `prn` | light-cljs | 0.022 s (0.022-0.024) | 0.010 s (0.010-0.010) | 19.4 MB (19.3-19.5) | 7.80-13.05 |
| `prn` | shadow-cljs | 9.540 s (9.069-10.848) | 38.925 s (24.680-43.570) | 1019.0 MB (864.0-1088.5) | 7.64-14.12 |
| `pprint-only` | light-cljs | 0.190 s (0.144-0.262) | 0.180 s (0.170-0.220) | 100.9 MB (98.8-103.8) | 7.80-13.05 |
| `pprint-only` | shadow-cljs | 11.280 s (9.627-13.925) | 45.090 s (27.610-57.630) | 1022.3 MB (948.0-1239.1) | 7.48-14.14 |
| `prn-test` | light-cljs | 0.073 s (0.070-0.076) | 0.060 s (0.060-0.070) | 54.7 MB (53.7-55.3) | 13.03-13.03 |

`cljs.pprint` adds **+168 ms** (+81.5 MB peak memory) to an uncached `advanced` build in light-cljs, compared to **+1,740 ms** (+3.3 MB peak memory) in shadow-cljs.

#### Warm development `compile` (nothing changed: light-cljs `--reloadable` vs. shadow-cljs `compile`)

| Program | Compiler | Wall time | Processor time (user + system) | Peak memory | 1-minute load before |
|---|---|---:|---:|---:|---:|
| `prn` | light-cljs | 0.022 s (0.021-0.024) | 0.010 s (0.010-0.010) | 19.5 MB (19.3-19.6) | 5.69-9.38 |
| `prn` | shadow-cljs | 4.867 s (4.308-5.626) | 15.235 s (10.130-21.340) | 583.1 MB (503.6-627.3) | 5.69-9.38 |
| `pprint-only` | light-cljs | 0.061 s (0.037-0.096) | 0.025 s (0.020-0.050) | 36.0 MB (35.9-36.6) | 5.69-9.38 |
| `pprint-only` | shadow-cljs | 4.891 s (4.419-5.345) | 14.420 s (11.710-20.010) | 580.6 MB (512.4-646.6) | 6.36-9.27 |

#### Cached rebuild after a one-file edit (installing `cljs.pprint` from persistent compiler cache)

| Program | Mode | Wall time | Processor time (user + system) | Peak memory | 1-minute load before |
|---|---|---:|---:|---:|---:|
| `prn` | light-cljs `prepare` | 0.016 s (0.013-0.018) | 0.000 s (0.000-0.000) | 15.3 MB (15.2-15.5) | 7.75-8.36 |
| `prn` | light-cljs `advanced` | 0.026 s (0.024-0.027) | 0.010 s (0.010-0.010) | 20.2 MB (20.1-20.3) | 7.75-8.36 |
| `prn` | shadow-cljs `compile` | 4.619 s (4.495-4.841) | 17.520 s (10.840-18.600) | 602.0 MB (575.1-665.4) | 7.73-8.50 |
| `pprint-only` | light-cljs `prepare` | 0.033 s (0.028-0.038) | 0.020 s (0.020-0.020) | 33.5 MB (33.4-33.7) | 7.75-8.36 |
| `pprint-only` | light-cljs `advanced` | 0.069 s (0.044-0.103) | 0.035 s (0.030-0.050) | 34.5 MB (34.2-34.7) | 7.75-8.36 |
| `pprint-only` | shadow-cljs `compile` | 4.476 s (4.266-5.219) | 15.455 s (10.660-18.730) | 615.4 MB (584.6-658.1) | 7.53-8.57 |

Installing `cljs.pprint` and `clojure.string` from the persistent cache (verified `cache.hits >= 2`, `cache.misses = 1`) adds **+17 ms** at `prepare` and **+43 ms** at `advanced` (and drops peak memory from `100 MB` uncached to `33.5-34.5 MB` because the V8 macro host never starts).

#### `light-cljs --watch` step after editing the calling namespace (library not recompiled)

| Program | Compiler step | Edit-to-log (`--poll-ms 50`) | Watch peak memory | 1-minute load before |
|---|---:|---:|---:|---:|
| `prn` | 0.9 ms (0.8-1.1) | 20.4 ms (3.5-52.9) | 21.0 MB | 11.48-12.22 |
| `pprint-only` | 1.1 ms (1.0-1.7) | 56.4 ms (3.6-63.1) | 118.4 MB | 11.48-11.48 |

Every timed edit recompiled only the calling namespace (`cljs.pprint` was never recompiled). The library adds **+0.2 ms** to the incremental watch step (`0.9 ms` -> `1.1 ms`). Watch peak memory is higher (`118.4 MB` vs. `21.0 MB`) because the startup build starts the V8 macro host to compile `cljs.pprint`.

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

### Baseline wins and losses (`e13215e4`)

- **Where light-cljs wins:**
  - **Uncached `advanced` / `release` build:** `0.190 s` vs. `11.280 s` for `pprint-only` (**59x faster**; delta `+168 ms` vs. `+1,740 ms`) and `100.9 MB` vs. `1022.3 MB` peak memory (**10x lower**).
  - **Warm development `compile` (nothing changed):** `0.061 s` vs. `4.891 s` for `pprint-only` (**80x faster**) and `36.0 MB` vs. `580.6 MB` peak memory (**16x lower**).
  - **Cached rebuild after a one-file edit:** `0.033 s` (`prepare`) and `0.069 s` (`advanced`) vs. `4.476 s` shadow-cljs `compile` (**65x-136x faster**) and `33.5-34.5 MB` vs. `615.4 MB` peak memory (**18x lower**).
  - **Development build Node.js load time:** `83.3 ms` vs. `106.6 ms` first output for `pprint-only` (**22% faster**; `63.8 MB` vs. `80.6 MB` peak memory) and `64.8 ms` vs. `86.5 ms` for `prn` (**25% faster**). The delta `cljs.pprint` adds to development load is also smaller (`+18.5 ms` vs. `+20.1 ms`).
  - **`advanced` / `release` Node.js load delta added by `cljs.pprint`:** `+13.4 ms` (`56.7` -> `70.1 ms`) vs. `+14.4 ms` (`55.8` -> `70.2 ms`), leaving `pprint-only` first output essentially tied (`70.1 ms` vs. `70.2 ms`).
- **Where light-cljs loses:**
  - **`advanced` / `release` Node.js load time on `prn` baseline:** light-cljs is **+0.9 ms slower** to first output (`56.7 ms` vs. `55.8 ms`; wall `58.8 ms` vs. `58.0 ms`) and uses **+0.8 MB higher peak memory** (`44.1 MB` vs. `43.3 MB`), reflecting the larger `prn` ECMAScript module runtime baseline (`118,064` vs. `103,116` raw bytes) plus the static-import module wrapper.
  - **`advanced` / `release` Node.js peak memory on `pprint-only`:** light-cljs uses **+0.4 MB higher peak memory** (`54.0 MB` vs. `53.6 MB`), and its upper range on first output is `1.6 ms` higher (`75.9 ms` vs. `74.3 ms`).
  - **Warm development `compile` delta over `prn`:** light-cljs adds `+39 ms` (`22 ms` -> `61 ms`) while shadow-cljs adds `+24 ms` (`4.867 s` -> `4.891 s`, though shadow-cljs's delta sits well inside its ~1 s JVM startup range and its absolute wall time is `80x` slower).

### Baseline `--stats` and `--stats-file` diagnosis (`e13215e4`)

In an uncached `--compile --optimize prepare` build of `pprint-only` (`122.3 ms` reported internal wall time, `1,561` macro calls: `1,512` in `cljs.pprint`, `47` in `clojure.string`, `2` in `pprint-bench.pprint-only`), the exclusive stage trace splits the cost into:

- **Top-level analysis (`trace.top-level-ms` `33.8 ms`, `trace.top-level.engine-ms` `14.2 ms`):** expanding `cljs.pprint`'s top-level macros (`defdirectives`, five `def-pprint-type` forms, and defining 11 namespace macros).
- **Deferred units (`trace.unit-ms` `45.3 ms`, `trace.unit.engine-ms` `18.6 ms`):** out of `296` deferred units, `250` non-host units take `13.6 ms` combined (`0.054 ms` each), while just `46` V8-host units take `33.2-34.9 ms` (`72%` of all unit time) across `5,310` engine entries.
- **Drain and V8 macro-host startup (`trace.drain-ms` `26.8 ms`, `trace.drain.bookkeeping-ms` `30.1 ms`):** starting the embedded V8 macro host on the first non-native macro call is a fixed compiler cost shared by any build that invokes a non-native macro (`prn-test` pays `27.3 ms` in `trace.drain-ms` / `28.0 ms` in `trace.drain.bookkeeping-ms` for `cljs.test`).

The two largest library contributors in `src/compiler_libraries/cljs/pprint.cljs` are:

1. **`defdirectives` (single largest contributor):** one giant top-level macro form at line 2394 that passes all ~30 directive definitions through the V8 macro host during top-level expansion and produces `cljs.pprint/get-directive-table`, the single slowest unit in the entire compilation at **`11.2 ms`** (`6.8 ms` V8 engine, `112` engine calls, `101` expansions — `23%` of all deferred-unit time).
2. **`getf` and `setf` macros (second largest contributor):** `62` calls to `(getf :field)` and `20` calls to `(setf :field val)` across `21` pretty-writer units (`write-tokens`, `write-initial-lines`, `pretty-writer`, `start-block`, `end-block`, `write-token` methods, and helpers). Upstream already defines plain functions `get-field` and `set-field` for the exact same `@@this` map reads and `swap!` updates; using the macros turns all `21` units into V8-host units totaling **`7.1 ms`** and **`925` V8 engine calls** (`53%` of all unit engine calls).
