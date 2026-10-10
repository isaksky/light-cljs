# Expanded lexer measurements

[Compiler collection measurements](compiler.md) separate parse/analyze/emit/total
compilation, output bytes, fresh CLI/Node costs, and validated resident immutable
operations. The standard-library `run_compiler.py` driver retains nine-sample
distributions, allocation counts and an unchanged primitive baseline.
The later [language integration measurement](compiler.md#language-integration-october-10-2026)
records expanded runtime size and validated parse/initialize/execute timings.

Native runtime measurements, source-attributed diagnostics and the reproducible
saved-binary comparison driver are documented in [interpreter.md](interpreter.md).
They measure the current native implementation and remain historical baselines
for the adopted [rquickjs evaluator migration](../docs/evaluator-migration.md).
They are not rquickjs measurements or an engine-selection prerequisite. Migration
implementation will retain useful startup, lifetime and regression checks while
retiring obsolete native-only harnesses.
The [transducer pipeline measurements](interpreter.md#equivalent-native-transducer-pipelines)
include a dedicated harness, paired full-value validation and a reproducible
`run_interpreter_transducers.py` report runner.

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
cargo test --release --test corpus -- --ignored --nocapture
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
