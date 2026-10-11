# Compiled lazy sequences and comprehensions

Contract: ClojureScript checkout **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**,
compared independently with Maven **1.12.134** and checkout runtime
**0.0.135109660**. Both reference runs verify `:optimizations :none`,
`:static-fns false`, and `:def-emits-var false`. Observed tools: Clojure 1.12.4,
Java 25.0.2, Node v24.18.0. No version difference occurred in this selection.

## Public arity inventory

Numbers count public function arguments. Collection overloads below are
implemented as ordinary callable values, including saved values and `apply`.
The constructor column records distinct transducer APIs. They are now
implemented and verified separately by the [eager transducer audit](compiler-transducers.md);
the historical counts in this sequence audit cover collection overloads only.
The former constructor-rejection check now verifies callable constructors.

| API | Implemented arities | Separate transducer constructor |
| --- | --- | --- |
| `range` | 0, 1, 2, 3 | — |
| `repeat`, `repeatedly` | 1, 2 | — |
| `replicate` | 2 | — |
| `iterate`, `cycle` | 2; 1 | — |
| `map`, `mapcat` | 2, 3, 4, 5+ | 1 |
| `map-indexed`, `filter`, `remove`, `keep`, `keep-indexed` | 2 | 1 |
| `take`, `drop`, `take-while`, `drop-while`, `take-nth` | 2 | 1 |
| `take-last`, `drop-last` | 2; 1, 2 | — |
| `concat` | 0, 1, 2, 3+ | — |
| `partition` | 2, 3, 4 | — |
| `partition-all` | 2, 3 | 1 |
| `partition-by` | 2 | 1 |
| `interleave` | 0, 1, 2, 3+ | — |
| `interpose` | 2 | 1 |
| `split-at`, `split-with` | 2 | — |
| `dorun`, `doall` | 1, 2 | — |
| `bounded-count`, `nthrest`, `nthnext` | 2 | — |
| `mapv`, `filterv` | 2, 3, 4, 5+; 2 | — |
| `first`, `second`, `rest`, `next`, `ffirst`, `nfirst`, `fnext`, `nnext` | 1 | — |
| `last`, `butlast`, `reverse`, `not-empty` | 1 | — |
| `some`, `every?`, `not-every?`, `not-any?`, `run!` | 2 | — |

The chunk support used by these operations includes `IChunk`, `IChunkedSeq`,
`IChunkedNext`, `IDrop`, `chunk-first/rest/next`, `chunk-cons`, `chunk-buffer`,
`chunk-append`, `chunk`, `array-chunk` (1/2/3), and their raw methods. Existing
`seq`, `count`, `nth`, `reduce`, metadata, equality/hash and printing consumers
accept the producer values. `*print-length*` bounds sequence printing, including
infinite producers.

`for` takes a binding vector and one body expression; `doseq` takes a binding
vector and zero or more body forms. Nested generators support sequential/map
patterns and `:let`, `:when`, `:while` in source order. The innermost `for` loop
retains upstream chunk filling and early `:while` termination. `doseq` drops
results, returns nil, and retains every permitted effect. Returned closures keep
their iteration's bindings. Invalid binding vectors/modifiers remain compilation
errors. The macro host uses dedicated compiler expanders without replacing the
native interpreter's `for`/`doseq` Vars.

## Realization contracts

Lazy mapping, filtering, keeping and comprehensions cache successful results.
Small vector/array/list sources preserve their unchunked demand; ranges and large
vectors preserve chunk boundaries. A first result from a mapped 100-element range
therefore invokes the mapper 32 times. A filtered empty chunk continues to the
next chunk without manufacturing an empty sequence element.

Two/three-source `map` obtains every source seq before checking emptiness.
Four-or-more sources follow the upstream lazy sequence of argument tuples:
source-seq checking short-circuits, arguments remain lazy through `apply`, and
host-function receivers follow `apply`. The tail's `rest` calls are deferred.
`mapcat` uses `apply concat`, including its bounded constructor-time probing;
it is not replaced with a more-lazy flattening loop. Unary `concat` preserves its
input sequence representation when forced; the older simplified Cons expectation
was corrected against the reference.

`repeat` is pending, `cycle` has pending tails, and `iterate` realizes a seed only
when needed. Metadata copies retain their own pending fields. Reducing an iterate
runs its producer directly; it does not populate or reuse sequence-tail caches.
Reduced results stop before requesting the next input. Exceptions stop callbacks
at the throwing element, while work already performed by chunk realization
remains observable. `dorun 0` still sequences its input, and bounded traversal
retains the upstream one-element lookahead where applicable.

## Source evidence

The complete relevant bodies were read before implementation. Paths below are
relative to the pinned checkout's `src/test/cljs/cljs` directory:

- `collections_test.cljs`: `test-range`, `test-cycle`, `test-repeat`,
  `test-iterate`, `test-string`, `test-split-at`.
- `seqs_test.cljs`: `test-chunked`, `test-cljs-2911`, `test-take`, `test-drop`,
  `test-nthrest`, `test-nthnext`, `test-partition`.
- `chunked_seq.cljs`: complete `test-cljs-2693`.
- `primitives_test.cljs`: complete `test-list-comprehensions`, `test-inext`,
  and both original escaped-closure predicates in `test-arithmetic`.
- `core_test.cljs`: complete selected `test-585`, `test-587`, `test-604`,
  `test-725`, `test-724`, `test-cljs-1744`, `test-cljs-2113`, `test-cljs-2799`,
  `test-cljs-3270`, `test-cljs-3271`.

The implementation references are `src/main/clojure/cljs/core.cljc` (`for`,
`doseq`) and `src/main/cljs/cljs/core.cljs` (the functions and producer/chunk types
listed above). The compiler macro adaptation preserves the upstream license
notice; its subexpressions are prepared before the enclosing syntax quote to
avoid unnecessary native evaluator depth.

**314 source assertion sites** pass generated JavaScript and both references.
Full-body witnesses retain setup, assertion order and original expectations;
`testing/is/are` become `do/assert` and individually bound table rows. The normal
non-lite mode is selected. Only independently inventoried transducer assertions
are omitted. Decimal `M` literals use their identical JavaScript numeric values.
Every imported site has a source hash, test anchor, fixture ID and adaptation
rationale in [witnesses.jsonl](compiler-language/witnesses.jsonl) and
[fixture-origins.tsv](fixture-origins.tsv). Existing previously mapped assertions
remain mapped to their original witnesses.

**56 independent probes** pass literal expected values, effects and stdout against
both references. They cover every named collection arity, finite prefixes,
shortest-input termination, repeated traversal, chunked/unchunked counts, IFn and
host callbacks, pending metadata, custom chunks/drop protocols, exceptions,
Reduced, nested modifiers, and closure captures. They remain separate from
imported assertion coverage.

`tests/compiler_sequences.rs` supplies **five direct host-result tests** without
the shared encoder, namespace aliases/native macros, malformed comprehensions,
constructor-arity separation, and long empty prefixes. Under a 96 MiB V8 heap,
100,000 chunked skipped inputs and 20,000 unchunked cached-producer inputs pass,
as does a finite prefix of an infinite `mapcat`. An exploratory 100,000-input
unchunked case exhausted that heap; it is not reported as a pass or a constant
space guarantee. Ordinary source heads and realized tails use JavaScript GC.

## Verification

The focused analyzer/compiler/macro/namespace/provenance/execution regressions
passed **161 tests**, with **40** reference-only tests explicitly ignored in that
ordinary run. The five new direct tests also pass. The sequence reference test
was separately run with both references; the strict imported selection has zero
missing assertions. Runtime assertions, inventory/accounting checks, Clippy and
formatting pass.

```sh
cargo test --locked --test analyzer --test compiler_language \
  --test compiler_macros --test namespaces --test provenance --test execution \
  --test compiler_deferred --quiet
cargo test --locked --test compiler_sequences --test compiler_deferred \
  --test execution --quiet
node tests/support/js_runtime.test.js
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution \
  compiled_sequences_match_pinned_values_effects_and_stdout -- --ignored --nocapture

python3 - <<'PY'
import json, subprocess
args = ['python3', '-B', 'tests/reference/compiler-language/report.py', '--scope', 'historical-selected',
        '--run', '--strict', '--reference', 'both']
for row in map(json.loads, open('tests/reference/compiler-language/witnesses.jsonl')):
    if row['fixture_id'].startswith('cljs-sequences-'):
        args += ['--id', row['assertion_id']]
raise SystemExit(subprocess.call(args))
PY

python3 benchmarks/measure_compiler_interop.py --sequences \
  --output benchmarks/results/compiler-2026-10-10/sequences
```

[Measured output/startup](../../benchmarks/results/compiler-2026-10-10/sequences/output-startup.json)
retains nine fresh V8 parse/evaluate samples for each program and checks its
numeric result. Runtime source grows from **132,563** to **151,256 bytes** versus
the retained deferred baseline; compressed runtime grows from **30,955** to
**35,081 bytes**. Primitive `42` remains **88 bytes**. The lazy pipeline is
**151,543 bytes**, and the nested comprehension is **157,247 bytes**. Median
parse/evaluate samples are **7.61 ms** and **5.64 ms**, including the program's
work. Historical timings are not paired experiments or performance thresholds.
