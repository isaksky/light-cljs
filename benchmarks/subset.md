# Historical subset-tokenizer measurements

Measured October 7, 2026. These are measurements of this limited lexical subset,
not complete ClojureScript tokenization or compilation. No generated lexer was
benchmarked, so these results establish no advantage over generated lexers.

## Machine and method

- Apple M3 Ultra **virtual machine**, ARM64, 24 visible CPUs, 32 GiB RAM;
  macOS 15.7.3. One benchmark thread, no CPU pinning or frequency control.
- `rustc 1.99.0 (b940084d7 2026-09-28)`, LLVM 23.1.2;
  Cargo 1.99.0; `aarch64-apple-darwin`. Default optimized Cargo bench profile,
  no custom `RUSTFLAGS`, no native-CPU tuning, no LTO override. See
  [environment](results/environment.txt) and the
  [verbose assembly-build command](results/build.txt). Final measured source
  files have [SHA-256 hashes](results/source-sha256.txt).
- Inputs are generated or read before timing and reused in memory. Calibration
  doubles traversal counts until a batch takes at least 20 ms, then selects a
  batch size targeting 150 ms. Nine samples per input/variant; variant order
  rotates each round. The initial baseline-only run targeted 100 ms.
- Each traversal passes the source through `std::hint::black_box`; each complete
  emitted token passes through another `black_box`. Returned token counts are
  checked outside timing. Measurements include scanner, iteration, token
  materialization, counting, and barrier overhead. They exclude allocation,
  input generation, file I/O, validation assertions, and printing. Tokens are
  streamed, not collected into a vector.
- Rates are medians of per-sample rates; MB/s means **1,000,000 bytes/second** and
  Mtokens/s means **1,000,000 tokens/second**. Raw CSV preserves individual
  durations, batch counts, bytes/second, and tokens/second. Ranges in the logs
  are sample minima/maxima, not confidence intervals. VM scheduling and warm,
  mostly repetitive synthetic inputs limit generalization.

## Final implementation

`table-usize` was the original public scanner and is now frozen in
`benches/support/subset.rs`. These numbers do not measure the expanded lexer. The final run followed promotion of
the table classifier and added a deterministic file of roughly 32 MiB, generated
before launching the benchmark. Its chunks vary between short/long atoms,
parentheses, whitespace, and UTF-8. Only file reading happens before timing;
these numbers do not measure disk throughput.

| Input | Bytes | Tokens per traversal | MB/s | Mtokens/s |
| --- | ---: | ---: | ---: | ---: |
| Short atoms | 262,154 | 91,184 | 1,185.3 | 412.3 |
| Long atoms (1,024/1,152 bytes) | 263,780 | 484 | 3,234.7 | 5.9 |
| Dense parentheses | 262,150 | 262,150 | 964.8 | 964.8 |
| Whitespace-heavy | 262,612 | 1,808 | 2,852.4 | 19.6 |
| Unicode | 262,170 | 67,970 | 1,435.9 | 372.3 |
| Large mixed buffer (~16 MiB) | 16,777,384 | 178,230 | 2,993.7 | 31.8 |
| Irregular file (~32 MiB) | 33,554,524 | 982,033 | 2,560.2 | 74.9 |

The first five fixtures are approximately 256 KiB. Complete patterns are
repeated to reach the minimum size, preserving UTF-8 boundaries. Exact patterns
are in [the harness](../benches/subset.rs). See [final CSV](results/final.csv)
and [final summary with sample ranges](results/final.txt).

## Individual changes evaluated

The original scanner used `matches!` for byte classification and `usize` spans.
After inspecting its optimized loop, two separate variants were measured:
table classification with the same spans, and compact spans with the original
classifier. These changes were not combined.

| Input | Table gain, run 1 | Table gain, run 2 | Compact-span gain, run 1 | Compact-span gain, run 2 |
| --- | ---: | ---: | ---: | ---: |
| Short atoms | +7.5% | +7.4% | -0.4% | -0.5% |
| Long atoms | +41.2% | +39.1% | +0.3% | -1.2% |
| Dense parentheses | +12.6% | +11.6% | +1.0% | +1.0% |
| Whitespace-heavy | +52.1% | +52.1% | +2.5% | +4.0% |
| Unicode | +11.7% | +10.0% | +0.3% | +1.0% |
| Large mixed | +37.6% | +38.7% | +0.6% | +1.0% |

These are ratios of medians against the match/usize baseline in the same run.
The table improved every fixture in both independent runs and remained faster
in the final confirmation run, so its 256-byte constant table was retained.
The original baseline and compact experiment remain only in benchmark support
code for reproducibility. Raw evidence: [baseline](results/baseline.csv),
[comparison 1](results/comparison-1.csv), [comparison 2](results/comparison-2.csv).

`size_of::<Token>()` was **24 bytes**, `size_of::<CompactToken>()` **12 bytes**,
and `size_of::<Tokenizer>()` **24 bytes** on this target. This is actual Rust
layout, including padding, not a stable ABI guarantee. One million stored tokens
would therefore require 24 MB versus 12 MB of element storage, excluding vector
metadata and spare capacity. Storage throughput was not benchmarked.

Compact spans showed no broad streaming improvement; in the final run, their
whitespace-heavy result fell 4.0% below baseline. The initial public scanner kept
`usize` for direct slicing and no additional file-size restriction. Revisit
compact spans if a later reader retains large token arrays. The benchmark-only
compact scanner validates `source.len() <= u32::MAX` **before** any narrowing;
the inclusive maximum and rejection above it are unit tested via the length
validator without allocating a multi-gigabyte string.

## Optimized loop inspection

```sh
cargo rustc --release --bench subset -- --emit=asm
rg -n 'scan_match|scan_table|scan_compact' target/release/deps/subset-*.s
```

The `#[inline(never)]` scan functions give named inspection points; iterator
`next()` is inlined inside them. The initial ARM64
[baseline assembly](results/baseline-scan.s) has no bounds-check panic path in
the scanning function. EOF/loop comparisons remain; the atom classifier already
uses a range check and bit test rather than one branch for each delimiter.

The [final table loop](results/scan-table.s) performs a source-byte load, table
load, and delimiter/EOF branches. The table's `u8` index produces no extra
bounds-check panic path. Stack stores around inline-assembly barriers correspond
to materializing tokens for `black_box` and are included in the measurements.
The final [match](results/scan-match.s) and [compact](results/scan-compact.s)
loops are also saved. There was no measured reason here to add unsafe indexing,
SIMD, or a more complex scanning algorithm; none was introduced.

## Reproduce

From the repository root:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo bench --bench subset -- --samples 9 --ms 150 > /tmp/tokenizer.csv

# Optional larger on-disk fixture; Python is only needed for this generator.
python3 benchmarks/generate_large_input.py
cargo bench --bench subset -- --samples 9 --ms 150 \
  target/benchmark-inputs/large-irregular.cljs > /tmp/tokenizer-with-file.csv
```

The saved file was generated with Python 3.14.6. The generator specifies its own
32-bit PRNG and writes whole UTF-8 chunks; output lives under ignored `target/`.
Pass other UTF-8 file paths after the options to add more inputs. All variants
must accept the input, so benchmark files must fit within the compact variant's
`u32::MAX`-byte limit. Re-run on intended deployment hardware before extrapolating.
