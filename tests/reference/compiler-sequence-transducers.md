# Lazy transducing sequence in the canonical runtime

`sequence` implements the three pinned public signatures: `[coll]`,
`[xform coll]` and `[xform coll & colls]`. One-argument coercion preserves existing
sequence identity without forcing lazy input and yields `()` for nil. Transducing
calls construct a JavaScript pull adapter, apply the transform once, and return
memoized lazy chunks. Separate calls allocate independent state. Both compiler
macro bodies and emitted applications use this same runtime and ordinary
IFn/IIterable dispatch; no per-element Rust callbacks or native value kind is used.

## Pinned demand and completion observations

The checked oracle is Maven ClojureScript **1.12.134** and the clean source checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. Both use the checked execution harness
with `:optimizations :none`, `:static-fns false` and `:def-emits-var false`.
`tests/fixtures/compiler-sequence-transducers.tsv` stores **30** literal JSON
observations measured from each reference. Both references agree; there are no
local overrides. These observations run independently in QuickJS and Node.

Construction applies the transform but consumes no source input. First demand
fills 32 output positions, then probes the iterator once more before checking the
chunk size. With `(sequence (map observe) (range 100))`, constructor effects are
`[]`; `(first s)` observes inputs `0..32`; `(nth s 31)` adds no effects;
`(nth s 32)` observes through `64`. This is output chunking, so rejection can pull
many more source inputs. A chunked source's own mapping can realize a whole source
chunk even when downstream `take` stops after two inputs. An unchunked source
stops exactly at the terminating input. `(take 0)` consumes the first available
input, then completes without emitting output. Empty source still invokes
completion, which can itself emit values.

The adapter buffers zero, one or many emissions per step, including nil. It
consumes the buffer before pulling again and clears consumed entries. Exhaustion
and Reduced termination invoke completion once; successful repeated traversal
reuses cached outputs and performs no duplicate steps or completion. Multi-source
calls probe from left to right and stop at the shortest input. In the asymmetric
source fixture, the left source realizes its third input while discovering that
the right source is exhausted; no third transform step occurs. Custom iterable
receivers and callable mapping objects retain their normal protocol/receiver ABI.
Exceptions remain failures; sticky cancellation at transform, source, step and
completion boundaries leaves the engine session terminal.

## Complete source bodies and ledger boundary

Three complete upstream bodies retain every setup, original comparison and
assertion path, with exact original SHA-256 hashes in fixture origins:

| Source under `src/test/cljs/cljs` | Body | Assertions |
| --- | --- | ---: |
| `core_test.cljs` | `test-cljs-2028` | 3 |
| `seqs_test.cljs` | `test-ISequential-indexOf` | 14 |
| `seqs_test.cljs` | `test-ISequential-lastIndexOf` | 17 |

Only `deftest`/`testing` to `do` and `is` to `assert` harness adaptations are used;
comments are removed when flattening TSV source. The upstream one-argument `=`
assertions and trailing `is` messages are retained exactly. The ledger already
mapped the complete index bodies; it now also maps the complete pending-state
body and binds all three public signatures to the named shared macro/application
helper. The two-source signature is exercised alongside four-source `apply`.

The mixed `core_test.cljs:test-transducers` body still contains eduction and a
composite transformation using flatten. Its complete body is retained in the
source ledger and is not claimed as fully executed here. Eduction belongs to
`light-cljs-60`, flatten/remaining iterator contracts to `light-cljs-121` and
final complete-language reconciliation to `light-cljs-125`. The reducers test file
contains no direct `sequence` call; its independent eager reduction family remains
covered by the separate transducer audit. No passing selected suite is a full
intended-language completion claim.

## Verification receipt (2026-10-11)

Commands actually executed:

```sh
cargo test --locked --test compiler_sequence_transducers -- --include-ignored --nocapture
cargo test --locked --test compiler_sequence_contracts --test compiler_sequences \
  --test compiler_deferred --test compiler_transducer_contracts --quiet
cargo test --locked --test compiler_transducers --test compiler_core_helpers --quiet
cargo test --locked --test compiler_language --test provenance --quiet
node tests/support/js_runtime.test.js
node tests/support/js_core_helpers.test.js
python3 -B tests/reference/compiler-language/test_full_ledger.py
python3 -B tests/reference/compiler-language/test_report.py
python3 -B tests/reference/compiler-language/report.py --run \
  --id core_test.cljs::test-cljs-2028::assertion-001 --timeout 60 \
  --reference-timeout 240 --output /tmp/light-sequence-ledger-report.json
```

The focused sequence oracle uses a 60-second test session deadline for its
100,000-input rejected-prefix probe; runtime session defaults are unchanged.
A separate GC test performs three 500-prefix batches with 1,000-element captures
and reducer cycles. It forces collection while JavaScript is active and checks
post-collection QuickJS heap growth stays below 128 KiB in an 8 MiB session.
The equivalent Node run uses a 32 MiB heap. Explicit result handles are released;
no new Rust-owned JavaScript roots are introduced.

Seven Node smoke measurements over 100,000 inputs using map-inc/filter-odd
transducing sequence produced **2,500,000,000** and a median **869.843 ms**.
These validate finite traversal and provide local timing evidence, not a paired
historical benchmark or a performance threshold. Full intended-language,
full mixed transducer/reducers, unrelated platform and release benchmark suites
were not run for this focused change.

Final outcomes: focused sequence suite **3 passed**, including the checked dual
reference build; canonical contracts and retained sequence/deferred/transducer
contracts **18 passed**; eager transducers/core helpers **5 passed**, with the
separate eager reference rebuild ignored. Both direct runtime scripts passed.
The compiler-language/provenance suites passed **3** tests.
The ledger regression suites passed **11** and **41** tests, and
`extract_inventory.py --check` verified byte-identical source inventories.
The selected ledger invocation verified the complete pending-state source adapter
against both references and passed all **30** bound macro phase arities (27
transient plus 3 sequence); the displayed full-language status remains
**INCOMPLETE**, as expected for this selected invocation.
