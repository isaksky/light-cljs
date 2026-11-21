# Core operations: light-cljs's output against shadow-cljs's

This is the output-speed gate. One ClojureScript source,
`src/core_ops/main.cljs`, times 78 core operations: map, vector, list and set
access and updates, seqs, reductions, printing, equality, records, calls and atoms. Both
compilers build it for release, and both outputs run in Node:
- light-cljs at `:advanced`, its release default (`light-cljs.edn`);
- shadow-cljs 3.5.3 with `:target :node-script` and Closure Compiler's advanced
  optimizations (`shadow-cljs.edn`, `deps.edn`).

Each operation's nanoseconds in the two outputs are compared. The results for
master are in `benchmarks/runtime-vs-shadow.md`, section "the per-operation
gate (light-cljs-1007)".

## Commands

From the repository root, with `LIGHT` an immutable copy of a light-cljs release
binary (`cargo build --locked --release`), named for its commit:

```sh
# Every operation: build both outputs, run them, print the table and write
# benchmarks/core-ops/out/core-ops-result.edn and .md.
bb benchmarks/core-ops/run.bb --light "$LIGHT"

# Before and after for the operations a change touches. Select the same
# operations both times.
OUT=benchmarks/core-ops/out
bb benchmarks/core-ops/run.bb --light "$BEFORE" --ops kw-small,get-small --out $OUT/before
bb benchmarks/core-ops/run.bb --light "$AFTER" --ops kw-small,get-small --out $OUT/after \
  --baseline $OUT/before.edn

# Where one operation spends its time: readable builds, one run per compiler
# under node --cpu-prof, and the functions with the most self time.
bb benchmarks/core-ops/run.bb --light "$LIGHT" --pseudo-names kw-small

# The runner's own tests (operation selection, checksum rule); nothing is built.
bb benchmarks/core-ops/test.bb
```

| Option | Meaning |
| --- | --- |
| `--light BINARY` | the light-cljs binary; otherwise `LIGHT_CLJS_BINARY`, otherwise `target/release/light-cljs` |
| `--light-commit SHA` | the commit the binary was built from; read from a `light-cljs-SHA` file name otherwise |
| `--ops A,B,...` | only these operations; all by default. A name selects that operation, or else every one whose name starts with it and a hyphen: `vec-conj` selects `vec-conj-64`, `assoc-small` both `assoc-small-*` |
| `--runs N` | Node processes per compiler, alternated (5) |
| `--rounds N` | timed rounds per operation in a process, which prints their median (5) |
| `--scale X` | multiply every operation's iteration count by X (1); for a quick check |
| `--out PREFIX` | write `PREFIX.edn` and `PREFIX.md` (`benchmarks/core-ops/out/core-ops-result`) |
| `--baseline FILE` | an earlier run's EDN: add light-cljs's median there, the change, and light/shadow there |
| `--pseudo-names OP` | profile OP, which must select one operation, instead of timing the gate |

It needs Node, Babashka, and the Clojure CLI with a JDK for shadow-cljs. The
Clojure CLI fetches shadow-cljs 3.5.3 on the first build.

## What a run does

- **Builds:** `light-cljs release bench` and `clojure -M:shadow release bench`
  in this directory, writing `out/light` and `out/shadow.js`. Both old outputs
  are removed first, so a failed build is never measured.
- **Runs:** one Node process per compiler and run:
  `node run-light.mjs 7 ROUNDS OPS SCALE` and `node out/shadow.js 7 ROUNDS OPS SCALE`.
  - The process times each selected operation in turn. It runs one warm-up at
    a tenth of the operation's iterations, then ROUNDS timed rounds.
  - It prints the median round as nanoseconds per operation, with a checksum.
- **Alternation:** the compilers alternate, and the first one changes each run.
- **Load:** no run waits for the machine. The 1-minute load average before each
  run is recorded, and the report calls the results unreserved when it was
  above 2.
- **Checksums:** every operation feeds a checksum. It must be the same in every
  process of both compilers, or the run fails.
- **Table:** each operation's median over the runs for each compiler,
  light/shadow, and each compiler's range over the runs.
- **EDN:** the environment (binary, its SHA-256 and commit, the versions, the
  machine), the options, every process's load average and rows, and the summary.
  `--baseline` reads an earlier EDN's summary.

`--pseudo-names OP` builds the `:profile` builds instead: light-cljs with
`--pseudo-names`, and shadow-cljs with pseudo names and pretty printing. It
writes `out/light-profile` and `out/shadow-profile.js`. It runs OP once per
compiler with `node --cpu-prof` and prints the functions with the most self
time, each with its line in that output. The profiles are kept in
`out/prof/OP/light` and `out/prof/OP/shadow`, and Chrome DevTools' Performance
panel opens them. Times under the profiler, in readable builds, are not the
gate's.

## Reading the numbers

- **Order matters.** A process runs the selected operations in the order of
  `benches`, so the engine's type feedback from earlier operations is part of a
  later one's number. For example, light-cljs `6ceeb23b`'s `count-small` takes
  about 44 ns alone, but 35 ns after the four small-map lookups before it.
  Compare runs of the same `--ops`. `--baseline` says when the selections, or
  `--rounds` or `--scale`, differ.
- **Loop baselines.** `bit-and` and `aget-bit-and` run last. They measure the
  benches' loop with no core operation in it.
- **`map-literal-3`** passes its literal through a JavaScript array before
  counting it. The investigation's version counted the literal directly, and
  light-cljs folded that loop to a constant, so its old ratio of 0.06 measured
  nothing.

The source is the investigation's micro-benchmark
(`research/output-speed/investigation-6ceeb23b`). Only `map-literal-3` changed,
and the two loop baselines moved to the end. light-cljs-1064 added the four key
and value walks, before `lazy-walk-1000`, so that the seq walks after them run
with the type feedback of a program that also walks map keys. light-cljs-1065
added the two `into` operations after the printing ones, just before the loop
baselines, so that no earlier operation's number moves. light-cljs-1073 added
the six `pop`, `disj` and variadic-update operations after them, before the
loop baselines, for the same reason, and light-cljs-1067 the two small `into`
operations after those.

## Operations

| Group | Operations |
| --- | --- |
| Small array maps (7 keys) | `kw-small`, `get-small`, `get-small-miss`, `contains-small`, `count-small`, `assoc-small-existing`, `assoc-small-new`, `dissoc-small`, `update-small`, `map-literal-3`, `destructure-map-3`, `reduce-kv-small`, `reduce-entries-small`, `eq-map-7`, `hash-fresh-map-7`, `merge-small`, `select-keys-3` |
| Hash maps (100 keys) | `get-hash100`, `kw-hash100`, `assoc-hash100`, `dissoc-hash100`, `update-hash100`, `transient-map-100` |
| Nested maps | `get-in-3`, `assoc-in-3`, `update-in-3` |
| Vectors | `vec-conj-64`, `vec-literal-4`, `vec-nth-1024`, `vec-invoke-1024`, `vec-peek`, `vec-assoc-1024`, `destructure-vec-3`, `eq-vec-10`, `hash-fresh-vec-4`, `transient-vec-100` |
| Key and value walks (light-cljs-1064) | `keys-small`, `vals-small` (the 7-key maps), `keys-hash100`, `vals-hash100` |
| Seqs and reductions over 1,000 | `reduce-vec-1000`, `reduce-map-inc-1000`, `filter-count-1000`, `into-xform-1000`, `mapv-1000`, `lazy-walk-1000`, `seq-walk-vec-1000`, `reduce-range-1000` |
| Collection functions | `group-by-100`, `sort-by-100`, `sorted-map-assoc-100` |
| Printing | `str-nested-map`, `str-concat`, `str-sorted-map-20` |
| Records and protocols | `protocol-on-record`, `record-kw-field`, `record-dot-field`, `record-assoc`, `multimethod` |
| Calls and atoms | `multi-arity-call`, `variadic-call`, `apply-vec`, `unknown-fn-call`, `closure-alloc-call`, `swap-inc`, `swap-assoc` |
| `into` without a transducer (light-cljs-1065) | `into-vec-keys-small` (a vector of the 7-key maps' keys), `into-set-100` (a set of a 100-item vector) |
| `pop`, `disj` and the variadic updates (light-cljs-1073) | `vec-pop` (vectors of 3, 10, 33 and 100 items; the 33-item one takes its tail from the tree), `list-pop` (lists of 2 to 10 items), `disj-set-7` (one item from 7-item hash sets), `disj-sorted-set-100` (one item from a 100-item sorted set), `disj-variadic-set-7` (two items from the 7-item sets), `vec-assoc-variadic-1024` (two indexes of a 1,024-item vector) |
| `into` of small collections (light-cljs-1067) | `into-vec-small` (a vector of vectors of 2 to 8 items), `into-map-small` (a map of the 7-key maps) |
| Loop baselines | `bit-and`, `aget-bit-and` |
