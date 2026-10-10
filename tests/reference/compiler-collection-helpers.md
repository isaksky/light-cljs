# Compiled persistent collection helpers

Contract: ClojureScript **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**,
compared independently with Maven **1.12.134** and checkout runtime
**0.0.135109660**. Each reference invocation verifies its identity and
`:optimizations :none`, `:static-fns false`, `:def-emits-var false`.

## Public behavior

The compiler resolves `assoc-in`, `update-in`, `select-keys`, `merge`,
`merge-with` and `zipmap` as first-class core values. They compose with the
existing `cons`, `list*`, `vec`, `into`, `array-map`, `set`, `empty`, `empty?`,
`peek`, `pop`, `find`, `key`, `val`, `keys`, `vals`, `get-in` and `update`.
Saved values, `apply`, and macro expansions use the same runtime operations.

Nested updates preserve records and collection metadata while retaining earlier
immutable snapshots. Empty update paths associate the nil key, as in CLJS;
empty lookup paths return the original value. Missing entries remain distinct
from present nil/false. In particular, the private lookup sentinel can now pass
through an out-of-range vector lookup without accidentally requesting an `nth`
exception. `select-keys` accepts lookup-capable values, returns a plain map, and
retains source metadata. Structural duplicate keys retain the original key
representative; all constructors and updates preserve equality/hash agreement.

`merge` and `merge-with` preserve the first map/record, combine left to right,
and distinguish present nil/false values from missing entries. The preliminary
truthiness scan and subsequent reduction remain separate, following the core
implementation. `zipmap` initializes both input sequences, advances keys before
values, and stops at the shorter input. Custom seqable paths, key collections,
entry collections and reducer inputs use public protocols.

`set` clears metadata when coercing an existing set. `empty?` checks a counted
collection through its count protocol before traversing it. `keys` and `vals`
project actual map entries; their `rest`/`next` advances the underlying sequence
immediately on each call, without memoizing that advance. A public `MapEntry`
constructor supplies indexed lookup, invocation defaults, stack operations and
entry protocols. Updating or attaching metadata produces an ordinary vector;
`empty` returns nil. `PersistentQueue.EMPTY` supports ordinary queue helpers.

All four `into` arities execute, including a supplied transforming reducer.
The three-argument form calls completion exactly once after reduction, handles
`Reduced` without reading later items, and retains destination metadata.
Built-in vector/map/set construction continues to bypass overrides of persistent
`-conj`, while custom destinations use their collection protocols. Zero arguments
return an empty vector, one argument preserves identity, and building into an
editable built-in yields a fresh persistent result even for empty input.
This does not expose transient collections or add the separate transducer
constructor APIs.

## Assertion-level source evidence

`compiler-language/inventory.jsonl`, `bodies.jsonl` and `manifest.json`
retain the full pinned source and spans. The source inventory
now includes `hash_map_test.cljs` and `map_entry_test.cljs`, for **17 files**,
**3,223 assertion sites** and **694 complete top-level bodies**. Unexecuted sites
remain missing; merely retaining a file is not a coverage claim.

The executable selection maps **164 original assertion sites** to **17 fixtures**
whose IDs start with `cljs-helpers-`. `witnesses.jsonl` records each assertion hash,
fixture identity, and adaptation rationale. `fixture-origins.tsv` records the
source commit, file, and test/helper anchor. The selected sections are:

- Complete `core_test.cljs` bodies `test-in-operations`, `test-select-keys`,
  and `test-780` (including its original atom setup); the ordinary map assertion
  in `test-cljs-2943`; and both `into` assertions in `test-cljs-3054`.
- Complete `collections_test.cljs` bodies `test-map-operations`,
  `test-cljs-1189`, `test-cljs-1212`, `test-cljs-1809`, `test-cljs-2145`,
  `test-cljs-2442`, and `test-cljs-3324`; plus the complete pop/stack sections of
  `test-vectors`, retaining the original 33/97-element inputs and snapshots.
- Complete `hash_map_test.cljs::test-array-map-with-duplicate-keys`, including
  runtime `apply`, direct construction, and differently annotated equal symbols.
- `map_entry_test.cljs::map-entry-interface-tests`, retaining every public helper
  interface section with the original `(MapEntry. :key :val nil)` argument.
  Sorted tree-node inputs and the separate comparator section are outside this
  helper selection. Each original `are` row and thrown-error assertion executes.
- The nil, ordinary collection, queue and non-emptyable sections of
  `seqs_test.cljs::test-empy-and-seq`, and the ordinary collection/host-array rows
  of `test-empty?`. The separate lazy-seq source constructor and sorted/transient
  implementations retain their independent inventory status.

The full relevant source bodies were inspected, including the duplicate set and
keyword test files. Keyword namespace resolution and the hash-set source's
map-versus-set assertion are not represented as helper passes. No source operation
was replaced with a simpler operation to claim coverage. `is`/`are` become runtime
assertions; `testing` wrappers become `do`, with bindings and effects retained.

## Independent checks

The **58** `helper-*` fixtures in `execution-outcomes.tsv` are independent probes,
not additional imported source assertions. They cover every new helper through
saved values and `apply`, empty/nil paths, false/nil entries, duplicate structural
keys, metadata, nominal record retention and dissociation, user seqable/reducible
inputs and collection destinations, all `into` arities, completion/early stopping,
callback effects and exceptions. The custom argument-sequence probe retains
`apply list*` and verifies that spreading requests `next` before `first`. They compare values, traces and stdout against
both references. Literal Node assertions inspect immutable payloads directly,
without the shared observation encoder or compiler lowering.

The independent odd-`array-map` error probe uses a saved function value. The release
compiler attempts to emit an invalid ratio constant for a directly written odd
`array-map` call, so that compile-time path is not claimed as a matching runtime
error. Independent record names avoid the upstream `cljs.core/Box` collision, and
the custom seqable is bound before the predicate to avoid the pinned compiler's
unbound generated temporary when `reify` occurs inside nested `and` expansion.
These are probe construction constraints, not passing source assertions.

## Reproduction

```sh
cargo test --locked --tests --quiet
node tests/support/js_runtime.test.js
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B tests/reference/compiler-language/test_report.py

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --quiet --test execution \
  compiled_helpers_match_pinned_values_effects_and_stdout -- --ignored --nocapture

python3 -B - <<'PY'
import json, subprocess
command = ['python3', '-B', 'tests/reference/compiler-language/report.py',
           '--run', '--strict', '--reference', 'both']
for row in map(json.loads, open('tests/reference/compiler-language/witnesses.jsonl')):
    if row['fixture_id'].startswith('cljs-helpers-'):
        command += ['--id', row['assertion_id']]
raise SystemExit(subprocess.call(command))
PY

python3 -B benchmarks/measure_compiler_interop.py --collection-helpers \
  --output benchmarks/results/compiler-2026-10-10/collection-helpers
```


## Observed verification and output

All **164 selected source assertions** passed on generated JavaScript and both
pinned references, with zero missing selected assertions and no version
differences. All **58 independent helper cases** matched values, effects and
stdout on both references. Literal Node storage, error-message and traversal
checks passed. All-target Clippy, formatting, inventory verification, the 17
accounting tests and whitespace checks passed.

Ordinary regression verification completed **673 passing tests across 63
targets**, with **42 explicit skips**. The initial full invocation detected an
invalid question mark in a fixture ID; after normalizing that ID, provenance
and the remaining two targets passed on the targeted rerun. Ignored reference
checks are not counted as passes; the required helper comparisons above ran
explicitly. The final runtime error-message and variadic traversal refinements also passed
122 focused compiler/execution/macro/namespace/accounting/provenance tests,
with 38 explicit reference skips, plus the literal Node checks.

The retained measurement is
`benchmarks/results/compiler-2026-10-10/collection-helpers/output-startup.json`.
It includes source, expected numeric results, hashes, raw/gzip sizes and nine
fresh Node parse/evaluation samples per program. Final timings were collected
after the broad checks finished. They exclude process startup and source reads;
historical runs are unpaired and do not establish a speed regression.

The runtime is **128,468 raw / 29,891 gzip bytes**,
compared with **123,196 / 28,716** at the core-protocol milestone. This is
**5,272 additional raw bytes**. Primitive output remains
**88 bytes** with no runtime. The common collection program is
**128,625 bytes**, with a
**2.910 ms** median. The nested-helper
program is **128,979 bytes /
3.942 ms**; transforming `into` is
**129,303 bytes /
3.318 ms**.
