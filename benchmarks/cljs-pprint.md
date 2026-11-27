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
