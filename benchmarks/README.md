# Expanded lexer measurements

[Seeded large projects](generated-project/README.md) generate reproducible 5k,
200k, or 800k-line applications in Babashka for light-cljs/shadow-cljs comparisons,
with an independent model interpreter and mandatory, classified unused code for
elimination checks.

[re-frame's example apps](re-frame-vs-shadow.md) compare light-cljs's ports of
TodoMVC and simple with shadow-cljs's builds of the unmodified originals:
release size raw, gzip and brotli with React counted apart, build time, and
every port edit by cause (`bb benchmarks/re-frame.bb`).

[The edit-to-reload loop](watch-vs-shadow.md) times `light-cljs watch` against
`shadow-cljs watch` on TodoMVC and the generated 200k re-frame app: from a
save to the output written, the reload message and the new code running in
headless Chromium, with startup and peak memory
(`bb benchmarks/watch-vs-shadow.bb`).

[The unified gap benchmark](gap-bench/README.md) combines compiler performance,
cold/warm cache behavior, scaling canary, watch/hot-reload, output size,
startup phase breakdown, Node/Chromium runtime & memory, `core-ops`, and
`extend-type`-active `extend-ops` into one ≤ 8 minute head-to-head scorecard
with ranked remaining gaps and `--baseline` diffing (`bb benchmarks/gap-bench/run.bb`).

## The shadow-cljs head-to-head

`generated-project/compare.bb` is the one kit for light-cljs against shadow-cljs.
It takes any directory with a `light-cljs.edn`, and a `shadow-cljs.edn` there
or in `--shadow-dir`, and runs release builds of both:

- three scenarios: a **clean build** (light `--no-cache`, shadow with
  `.shadow-cljs` removed), a **rebuild, nothing changed** (one untimed build
  fills each cache first) and, with `--leaf FILE`, a **rebuild after a one-file
  edit**, where each round appends a different unused `defn` to FILE, restored
  afterwards;
- `--runs N` rounds of each (default 5), the compilers alternated: each round
  reverses the previous round's order, so neither always goes first;
- before every build it waits until the 1-minute load average is at most
  `--max-load` (default 2, giving up after `--load-wait` seconds, default 900),
  records it, and repeats a round in which a build started above it (at most
  `--max-repeats` times, default 3; a round still above is kept and marked);
- medians and ranges of wall time, user CPU, system CPU and peak memory, from
  `/usr/bin/time`;
- `--warm-jvm N` adds N shadow release builds in one JVM, as a running
  shadow-cljs server does (wall time and CPU per build);
- the light-cljs commit (`--light-commit`, or read from a `light-cljs-SHA`
  binary name) and the binary's SHA-256, shadow-cljs, JDK, Node, Clojure CLI
  and Babashka versions, the machine and the operating system.

It writes `OUT.edn` (every sample, including discarded rounds) and `OUT.md`
(the tables to paste into a dated report section); `--out OUT` defaults to
`PROJECT/compare-result`. A generated project's outputs are also checked
against its oracle and `check-waste.bb --strict`; any other project's report
gives the bytes and gzip size of the `.js` files each build wrote.

```sh
# Generated projects: build an immutable binary and name it by commit first.
cargo build --locked --release
cp target/release/light-cljs /some/dir/light-cljs-$(git rev-parse --short=8 HEAD)
bb benchmarks/generated-project/generate.bb /tmp/g200k --seed 42 --lines 200k
bb benchmarks/generated-project/compare.bb /tmp/g200k --light /some/dir/light-cljs-SHA \
  --leaf src/workload/babomhol.cljs --warm-jvm 4

# Any other project: name the builds when a file has several, and where shadow lives.
bb benchmarks/generated-project/compare.bb PORT-DIR --light BIN --shadow-dir UPSTREAM-DIR \
  --light-build client --shadow-build client --shadow-cmd "npx shadow-cljs" --leaf src/app/core.cljs
```

Run it with no other heavy work on the machine. Allow for the load gate: a
shadow-cljs build leaves the 1-minute load average at 4 to 6 on the 24-CPU Mac,
and it takes 2 to 3 minutes to fall back to 2, so 5 rounds of the three
scenarios take about an hour at 200k and nearly two at 800k. Another job using
even one core keeps the load above 2: the run then waits `--load-wait` seconds
per build and repeats rounds, which is the gate working, not a hang. When the
machine cannot be had to itself, do not wait for it: pass `--max-load 1000`
so the gate never blocks, and label the results unreserved with the load range
the report records before every build (the 5-round baseline of November 13,
2026 ran this way).

`--shadow-heaps 1g,1536m,2g,...` sweeps shadow-cljs's maximum heap instead:
clean builds only, one shadow build per size in each round (alternated with
light-cljs like the compilers above), its JVM given `-J-Xmx` after the
`:shadow` alias's own `-Xmx8g` (the later flag wins, so the project stays
unedited), `-XX:+ExitOnOutOfMemoryError` and a garbage-collection log. A failed
build is recorded (with whether it ran out of memory), not fatal; every build
that succeeds has its output checked. `--light-memory-limits N,...` adds light
builds at each `--macro-memory-limit`. The report gives each configuration's
medians and stop-the-world GC pauses, shadow's smallest heap with no failed
build within 10% of its best median wall time, and light-cljs's peak memory
against shadow's there:

```sh
bb benchmarks/generated-project/compare.bb $R/projects/workload-200k-seed42 --light $R/bin/light-cljs-SHA \
  --runs 3 --max-load 1000 --shadow-heaps 1g,1536m,2g,3g,4g,8g --light-memory-limits 67108864
```

`--light-only` times light-cljs
alone; `--skip-unchanged` leaves out the timed rebuild with nothing changed
(its untimed cache-filling build still precedes the one-file edit); `bb
compare.bb` with no arguments lists every option. Results:
[generated-shadow-vs-light.md](generated-shadow-vs-light.md).

How fast the output runs: `--runtime 1,10,40` also runs a generated project's
two outputs in Node at each iteration count (input 42), alternated, every run's
checksum checked against the oracle, and `--runtime-only` skips the builds.
`bb benchmarks/re-frame.bb runtime todomvc` times re-frame TodoMVC's two builds
in headless Chromium. Results: [runtime-vs-shadow.md](runtime-vs-shadow.md).

`generated-project/growth.bb` reads `compare.bb` results of the same generated
project at several sizes, in either shape, and optionally the output of
`light-cljs release ID --no-cache --stats` builds of them, and fits how each
compiler's wall time, CPU time and peak memory, and each `--stats` counter,
grow with nonblank lines: the exponent of a log-log fit (1 is linear) and the
ratio and exponent between consecutive sizes. It writes `OUT.edn`, plot-ready
points and fits, and `OUT.md`:

```sh
bb benchmarks/generated-project/growth.bb --out $R/growth/growth $R/growth/*-seed42*.edn $R/growth/stats/*.txt
```

[Compiler collection measurements](compiler.md) separate parse/analyze/emit/total
compilation, output bytes, fresh CLI/Node costs, and validated resident immutable
operations. The standard-library `run_compiler.py` driver retains nine-sample
distributions, allocation counts and an unchanged primitive baseline.
The later [language integration measurement](compiler.md#language-integration-october-10-2026)
records expanded runtime size and validated parse/initialize/execute timings.
[Startup and steady state by optimization level](compiler.md#startup-and-steady-state-by-optimization-level--october-13-2026)
times the size-ratchet programs at every `--optimize` level in Node and QuickJS,
with upstream ClojureScript's `:advanced` sizes beside them.

Archived native runtime measurements are documented in [interpreter.md](interpreter.md).
The interpreter, its benchmark harnesses and four Python runner scripts have been
removed. Retained results and fixtures describe historical native semantics and
are not current evaluator coverage or performance claims.
Use [evaluator measurements](../docs/evaluator.md#verification-and-measurements)
for the compiler-backed QuickJS path. Run
`cargo bench --locked --bench evaluator -- --iterations 20` for validated fresh,
reused and bounded-loop/reset/recovery scenarios.

Measured October 7, 2026. These numbers measure raw tokenization, including
comment tokens and reader prefixes. They exclude literal decoding/validation,
reader semantics, parsing, and compilation. They are not full ClojureScript
reader or compiler throughput, and no generated lexer was compared.

## Environment and method

Apple M3 Ultra **virtual machine**, ARM64, 24 visible CPUs, 32 GiB RAM,
macOS 15.7.3. Rust 1.99.0 (`b940084d7`), LLVM 23.1.2, Cargo 1.99.0;
`aarch64-apple-darwin`, default release/bench optimization level 3, no custom
`RUSTFLAGS`, CPU pinning, or native-CPU tuning. See the recorded
[environment](results/environment.txt) and [build invocation](results/full-build.txt).

One thread scans resident input buffers repeatedly. Input generation, file
reading, lexical-error validation, assertions, and printing happen outside the
timed batches. Each complete token and each traversal's source pass through
`std::hint::black_box`; token counting and barrier costs are included. Tokens
are streamed, not stored in a vector. Calibration runs for at least 20 ms per
input, targeting 150 ms per sample. Each run has nine samples per input.
Reported rates are medians; raw CSV includes every sample's duration and rates.
MB/s and Mtokens/s use decimal millions. Sample ranges in the logs are not
confidence intervals; VM scheduling and warm repetitive inputs affect results.

## Results

The table reports the second final run. The first six synthetic fixtures remain
from the original subset benchmark. Two new fixtures repeat
[reader_forms.cljs](../tests/fixtures/reader_forms.cljs), exercising literals,
comments, all collection delimiters, tags, metadata, and dispatch prefixes.
The final two inputs are actual compiler sources from ClojureScript commit
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`.

| Input | Bytes | Tokens per traversal | MB/s | Mtokens/s |
| --- | ---: | ---: | ---: | ---: |
| Short atoms | 262,154 | 91,184 | 683.1 | 237.6 |
| Long atoms | 263,780 | 484 | 2,228.3 | 4.1 |
| Dense parentheses | 262,150 | 262,150 | 475.9 | 475.9 |
| Whitespace-heavy | 262,612 | 1,808 | 2,772.2 | 19.1 |
| Unicode | 262,170 | 67,970 | 553.4 | 143.5 |
| Large mixed buffer (~16 MiB) | 16,777,384 | 178,230 | 2,163.6 | 23.0 |
| Reader syntax (~256 KiB) | 263,048 | 55,020 | 945.7 | 197.8 |
| Large reader syntax (~16 MiB) | 16,777,844 | 3,509,310 | 939.4 | 196.5 |
| `cljs/core.cljs` | 375,722 | 72,391 | 650.1 | 125.2 |
| `cljs/analyzer.cljc` | 209,684 | 32,545 | 770.0 | 119.5 |

Raw results: [run 1 CSV](results/full-inline-1.csv),
[run 2 CSV](results/full-inline-2.csv), and summaries with ranges for
[run 1](results/full-inline-1.txt) / [run 2](results/full-inline-2.txt).
Current measured `Token` and `Tokenizer` sizes are both **24 bytes**, including
padding. This is target-specific Rust layout, not an ABI guarantee.

## Assembly and one retained change

Inspection of the first optimized expanded scanner showed that LLVM left a
call to `Iterator::next()` per token despite `#[inline]`. Its scanning function
had no bounds-check panic path; EOF comparisons and classification branches
remained. See [initial assembly excerpts](results/full-initial.s) and
[initial measurements](results/full-initial.csv).

The only performance change evaluated here was replacing that annotation with
`#[inline(always)]`. Relative to the initial nine-sample baseline, two subsequent
runs improved `core.cljs` by 18.5% / 18.9%, `analyzer.cljc` by 21.7% / 24.2%, and
the 16 MiB reader fixture by 11.9% / 12.4%. Every fixture's median improved in
both runs (3.3–24.2% across the two runs). The runs were sequential, not an
interleaved statistical experiment; smaller differences may include noise.
The repeatable real-source improvements justified retaining this annotation.

The [final scan assembly excerpt](results/full-scan.s) contains the inlined
scanner, with no per-token `next()` call or bounds-check panic path in that
function. Calls remain for digit classification, dispatch forms, and characters;
those helpers still use safe checked access where needed. No unsafe indexing or
SIMD was introduced. Capture full assembly with:

```sh
cargo rustc --release --bench tokenizer -- --emit=asm
rg -n 'tokenizer4scan|Iterator4next' target/release/deps/tokenizer-*.s
```

Saved hashes identify [the initial](results/full-initial-sha256.txt) and
[final sources](results/full-source-sha256.txt). The final source adds an
explanatory comment after the timed runs; executable code is unchanged.

## Validation and reproduction

All tests pass in debug and release. An explicit corpus test additionally scanned
296 ClojureScript checkout files (`.clj`, `.cljs`, `.cljc`): 2,834,491 bytes and
459,460 tokens, with valid, ordered spans and no lexical errors. This is a corpus
smoke test, not proof of complete reader conformance. Focused tests cover all
token families, EOF recovery, escapes, UTF-8, source slicing, and character
classes checked against the actual vendored JVM reader.

```sh
cargo test
cargo test --release
cargo clippy --all-targets -- -D warnings
cargo bench --bench tokenizer -- --samples 9 --ms 150 > /tmp/lexer.csv

# Requires research/clojurescript:
cargo test --release --test frontend corpus::clojurescript_checkout_has_no_lexical_errors -- --ignored --nocapture
cargo bench --bench tokenizer -- --samples 9 --ms 150 \
  research/clojurescript/src/main/cljs/cljs/core.cljs \
  research/clojurescript/src/main/clojure/cljs/analyzer.cljc > /tmp/lexer-source.csv
```

The [historical subset report](subset.md) and `cargo bench --bench subset`
preserve the earlier classifier and compact-span experiments. That scanner
recognizes only parentheses, ASCII whitespace, commas, and atoms. Its throughput
and `u32` comparison must not be attributed to the expanded lexer. The public
scanner continues to use `usize` spans; compact storage has not been benchmarked
again for the expanded syntax.

Captured reify construction and invocation have a separate validated comparison:
`cargo bench --locked --bench interpreter_protocols` emits seven CSV samples per
phase for a captured closure and an equivalent captured protocol method. Use
`--features count-allocations` for a separate allocation run (elapsed time is zero
in that instrumentation mode). Parsing/analysis and GC are outside both phases;
construction returns a fresh callable/object, and invocation must return 10.

The [immutable record data measurements](interpreter.md#immutable-record-data-2026-10-08)
compare nominal record construction, lookup, assoc, sequence/reduce and key use
with validated ordinary-map operations, including allocation and warmed GC checks.

The same record benchmark now separately measures retained protocol calls against
a direct function reading the same record field and a captured reify method,
per-instance construction, and full record definitions. See the
[record method measurements](interpreter.md#record-methods-2026-10-08).

Named-record extensions add `extended-method` call/construction and
`record-extension` publication scenarios to `interpreter_records`; the same
workloads validate equivalent results against direct, inline and reify calls.
See the [extension measurements](interpreter.md#record-extensions-2026-10-09).

The complete [protocol and immutable record cost report](interpreter.md#protocol-and-immutable-record-cost-report)
adds nine-sample, same-binary dispatch/construction/map-operation pairs, isolated
macro/analysis/definition costs, retained/released generations, replacement
captures, callback GC, and equivalent fresh native CLI programs. Run
`run_interpreter_types.py` to retain raw timing/allocation/GC data, source and
fixture hashes, binaries and unchanged-fixture baseline comparisons.
