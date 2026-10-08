# Parser construction baseline

Measured October 7, 2026 (America/Denver). These are **syntax construction**
rates, including the lexer, literal spelling validation/decoding, node/child/root
allocations, and destruction. They are not full ClojureScript reader or compiler
throughput. Reader expansion, name resolution, numeric materialization, tag
execution, and regex compilation are absent; see [the contract](../docs/parser.md).

## Environment and method

- `rustc 1.99.0 (b940084d7 2026-09-28)`, Homebrew; LLVM 23.1.2;
  Cargo 1.99.0; `aarch64-apple-darwin`.
- Apple M3 Ultra **(Virtual)**, 24 visible physical/logical CPUs, 32 GiB RAM;
  macOS 15.7.3, build 24G419. No CPU affinity or frequency control.
- Default optimized Cargo bench profile, no dependencies, native-CPU override,
  LTO override, or custom allocator in the timed binary. Single parsing thread.
- Two consecutive runs of nine samples per input, targeting 100 ms/sample after
  at least 20 ms calibration. Medians below are computed independently per run.
  Source is resident and warm; allocator pages/caches are also warm. Each
  iteration creates fresh vectors, then drops the entire tree within the timer.
- Input generation, file I/O, initial validation, reporting, line-index creation,
  and expected-node-count assertions are outside the timed region. The harness
  makes the source opaque per traversal, passes the complete tree by reference
  through `black_box`, and consumes a node count. It does not print per node or
  insert a per-node optimization barrier.
- Nodes/second means **retained arena nodes**, including collection and prefix
  nodes. Comments and `#_` subtrees are excluded from the reported node count;
  the work to scan/build/drop discarded syntax is still timed.

Actual `size_of` on this target: `Node` **48 B**, `NodeKind` 32 B, `NodeId` 8 B,
`Parser` 104 B, `SyntaxTree` 48 B; the existing `Token` remains 24 B. These are
inline sizes, excluding vector allocations and source storage. No compact-ID or
alternative tree-layout experiment was performed. No parser-specific performance
change is claimed; the previously measured scanner implementation is retained.

## Timing results

Decimal MB/s = bytes/s ÷ 1,000,000; Mnodes/s = nodes/s ÷ 1,000,000.

| Input | Bytes | Nodes | Run 1 MB/s | Run 2 MB/s | Run 2 Mnodes/s |
| --- | ---: | ---: | ---: | ---: | ---: |
| Short atoms | 262,154 | 79,786 | 80.0 | 83.8 | 25.5 |
| Long atoms (~1 KiB each) | 263,780 | 363 | 1,896.3 | 1,870.4 | 2.6 |
| Dense parentheses | 262,150 | 131,075 | 37.1 | 36.9 | 18.4 |
| Whitespace-heavy | 262,612 | 1,356 | 1,782.6 | 1,706.8 | 8.8 |
| Unicode symbols | 262,170 | 58,260 | 98.5 | 94.1 | 20.9 |
| One wide vector | 350,002 | 150,001 | 78.0 | 74.9 | 32.1 |
| Reader syntax fixture | 262,944 | 48,960 | 129.1 | 118.7 | 22.1 |
| Large reader fixture (~8 MiB) | 8,388,644 | 1,561,960 | 107.9 | 105.2 | 19.6 |
| `cljs/core.cljs` | 375,722 | 54,959 | 122.8 | 119.5 | 17.5 |
| `cljs/analyzer.cljc` | 209,684 | 24,674 | 147.5 | 133.7 | 15.7 |

Inputs with few nodes per byte are much faster in byte terms. Dense collections
and real source expose substantially more construction/allocation work. The
runs also show scheduling/measurement variation: analyzer samples ranged from
113.9 to 150.6 MB/s in run 2. These runs establish a baseline, not a statistically
controlled comparison between implementations. No generated parser/lexer, unsafe
scanner, SIMD implementation, or complete ClojureScript compiler was compared.

## Allocation counts

Counted in a **separate build and invocation**, with `count-allocations` enabled.
That feature adds a benchmark-only `GlobalAlloc` adapter forwarding to `System`;
its required unsafe forwarding does not appear in the library. Atomic counters
are enabled only around one parse per input, after fixture creation/validation.
No timing data is taken from this instrumented binary. The harness rejects
attempts to time it or request counts without the feature.

`allocations` counts alloc/alloc_zeroed calls; `reallocations` counts realloc
calls separately. `requested_bytes` sums requested sizes across both, including
the entire new size at each reallocation. It is **not peak or retained memory**,
nor a sum of copy costs. No deallocation or system-page/malloc statistics are
claimed. Counts include discarded subtrees and decoded strings, but exclude
source buffers, diagnostics/line indices, and output.

| Input | Allocations | Reallocations | Requested bytes |
| --- | ---: | ---: | ---: |
| Short atoms | 11,400 | 11,425 | 13,939,040 |
| Long atoms | 123 | 12 | 54,848 |
| Dense parentheses | 56,177 | 30 | 28,011,776 |
| Whitespace-heavy | 454 | 17 | 227,232 |
| Unicode | 9,712 | 9,736 | 7,485,536 |
| Wide vector | 3 | 32 | 29,359,936 |
| Reader syntax | 10,658 | 2,905 | 7,009,824 |
| Large reader | 339,958 | 91,915 | 224,264,192 |
| `core.cljs` | 17,113 | 1,990 | 7,035,112 |
| `analyzer.cljc` | 7,549 | 930 | 3,472,104 |

The ordinary `Vec<Node>` arena plus child vectors remains the initial design.
These counts make its costs visible; they do not establish a benefit for custom
allocation, preallocation, compact IDs, or shared child storage.

## Reproduce

```sh
cargo bench --bench parser -- --samples 9 --ms 100 \
  research/clojurescript/src/main/cljs/cljs/core.cljs \
  research/clojurescript/src/main/clojure/cljs/analyzer.cljc

cargo bench --bench parser --features count-allocations -- --allocations \
  research/clojurescript/src/main/cljs/cljs/core.cljs \
  research/clojurescript/src/main/clojure/cljs/analyzer.cljc
```

The eight generated fixtures require no research checkout. The two optional
files came from ClojureScript commit
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`. Additional UTF-8 paths can be appended;
invalid input stops the harness before measurement. The lexical-only fixture
contains `#=` and is intentionally distinct from the parser fixture.

Raw evidence: [run 1 CSV](results/parser-1.csv), [run 1 report](results/parser-1.txt),
[run 2 CSV](results/parser-2.csv), [run 2 report](results/parser-2.txt),
[allocation CSV](results/parser-allocations.csv),
[allocation build report](results/parser-allocations.txt),
[environment](results/parser-environment.txt),
[source hashes](results/parser-source.sha256). Each timing run has 90 samples.
Documentation comments in `src/lib.rs` changed after run 1; executable source
was identical across timing runs. Production code and fixtures are otherwise
identified by the source hashes.
