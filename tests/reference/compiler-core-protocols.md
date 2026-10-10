# Public core protocol consumers

Contract: ClojureScript **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**,
compared independently with Maven **1.12.134** and checkout runtime
**0.0.135109660**. Reference runs check the revision and
`:optimizations :none`, `:static-fns false`, `:def-emits-var false`.

## Protocol inventory and dispatch

| Family | Public consumers and behavior |
| --- | --- |
| `IFn`, `Fn` | Calls, saved function values, `apply`, `ifn?`, `fn?`; marker membership does not supply a missing call implementation. |
| `ILookup`, `IIndexed`, `ICounted` | Both `get`/`nth` arities, keyword calls, `contains?`, `get-in`, destructuring, `count`, and corresponding predicates. |
| `ISeqable`, `ISeq`, `INext`, `ISequential`, `IList` | `seq`, `first`, `rest`, `next`, `cons`, `sequence`, sequence predicates, linear `nth`, counting and traversal. A minimal custom sequence needs no `INext`. |
| `ICollection`, `IEmptyableCollection`, `IAssociative`, `IFind`, `IVector`, `IMap`, `ISet`, `IStack`, `IReversible` | Collection predicates and updates, `find`, `empty`, `peek`, `pop`, `rseq`, custom vector entries and built-in collection integration. |
| `IReduce`, `IKVReduce`, `IIterable`, `IMapEntry` | Both `reduce` arities, `reduce-kv`, `into`, `vec`, `key`, `val`, `keys`, `vals`, custom iterator traversal and map-entry fallback. |
| `IEquiv`, `IHash`, `IComparable` | Equality, comparison, custom keys/set members, collision resolution and stable identity fallback; custom hashes are coerced to signed 32-bit values. |
| `IMeta`, `IWithMeta` | `meta`, `with-meta`, `vary-meta`; copies preserve immutable payloads and custom methods. |
| `IDeref`, `IAtom`, `IReset`, `ISwap`, `IVolatile`, `IWatchable` | Dereference, state changes, watch callbacks and state predicates; built-in atom fast paths retain upstream precedence. |
| `IPrintWithWriter`, `IWriter` | Custom printing, `write-all`, raw `-pr-writer`, `-write`, and `-flush`; ordinary printing passes nil options, preserving dynamic print settings. |

Membership, direct implementation, and method availability are distinct.
Generated type/reify masks retain upstream partition semantics: when a type
declares a fast protocol in a partition, predicates do not infer other protocols
from native fallback tables. Raw method dispatch still has its normal fallback.
Marker-only and partial protocols satisfy their declared predicates; invoking a
missing required method fails at the protocol boundary. A raw method slot alone
does not imply protocol membership. Native tables cannot replace a built-in
collection's existing implementation. Nil and primitive fast paths retain the
ordering in upstream public consumers. Arrays support public indexed lookup while
remaining opaque host values to the private storage/observation adapter.

Reduction selects the declared reduction implementation before sequence
traversal, and uses `IIterable` when appropriate. Uncounted two-argument
sequence reduction delegates its tail back through public `reduce`. Generic
sequence traversal observes custom `first`/`next`, including extensions of
built-in lists. Every built-in and iterator reduction stops immediately on
`Reduced`; a custom reduction's return is preserved verbatim. `reduce-kv` falls
back through `reduce` and each entry's key/value methods when `IKVReduce` is absent.
Counting a custom sequence stops at a counted tail without reading beyond it.
Built-in construction in `into` follows the observable transient path: persistent
`-conj` extensions do not override construction, and vector map entries are
called at indexes zero and one.

Custom hash/equality values retain equivalent map keys and set representatives.
Built-in maps, sets and sequences compare through the corresponding interfaces
of custom values. `ISequential` alone does not give a custom value structural
hashing or equality; those operations retain default identity without `IHash`
and `IEquiv` implementations. Uncounted sequential equality stops at the first
mismatch rather than counting both inputs in advance.

## Source evidence

Read full relevant upstream bodies and their support definitions before adapting:
`extend_to_native_test.cljs` (including the entire custom-writer test),
`destructuring_test.cljs::test-protocol-method-destructuring` and its types,
`collections_test.cljs::test-784`, `CustomVectorThing`/`test-cljs-2128`,
`FixedHash`/`test-phm-fixed-hash`, `core_test.cljs::test-cljs-3406`,
`MapWithNoIKVReduce`/`test-cljs-2083`, and the sequence test bodies.
The public consumers and primitive/default extensions in `core.cljs` are the
implementation contract. The persistent custom-hash behavior has independent
probes; the mixed transient/hash-map test is not claimed as an imported pass.

New assertion mappings cover the complete `test-784`,
`test-sequential-equality`, and `test-seq-operations` bodies: **15 assertion
sites**. The first retains every original map input, assertion, and minimal
custom reify sequence. Its single-generator `doseq` loops lower to `seq`/`next`
loops with the same bindings and order; other test wrappers become `do`/`assert`.
No callable or custom sequence is replaced with a built-in stand-in.
Source hashes, anchors, and adaptation rationale live in
`compiler-language/witnesses.jsonl` and `fixture-origins.tsv`.

Existing complete extension, destructuring, custom-vector and state bodies are
included in the integration comparison. The selected matrix has **79 source
assertion sites**. The inventory still reports unrelated/unimplemented library
assertions separately; these counts do not describe the whole upstream suite.
**45 independent `core-protocol-*` fixtures** exercise dispatch precedence, membership,
all reduction arities, custom iterators/entries, collision keys, partial methods,
metadata, writer options, and host nil/undefined boundaries. Additional literal
Node assertions use a fresh runtime and bypass the compiler and observation
encoder.

## Explicit version differences

Release 1.12.134 emits unchecked fast method-slot calls for several declared
protocols. The pinned checkout uses protocol dispatch, which reports an ordinary
`Error` when the method is absent. Nine marker/partial-method fixtures therefore
expect `TypeError` on release and `Error` on checkout and generated JavaScript.
A tenth fixture declares only `IHash` and `IEquiv`: release throws `TypeError`,
while checkout and generated JavaScript use the default identity implementations.
`core_protocol_reference_outcome` records these exact versioned expectations.
They are verified differences, not matching-release conformance passes.

An independent custom vector probe also preserves a failure shared by both
references: `conj` on a persistent map uses its indexed methods, whereas `into`
uses the construction path and invokes the vector. A vector without `IFn`
therefore fails with `TypeError` during that `into` call.

## Reproduction

```sh
cargo test --locked --tests --quiet
node tests/support/js_runtime.test.js
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B tests/reference/compiler-language/test_report.py

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --quiet --test execution compiled_core_protocol_ \
  -- --ignored --nocapture

python3 -B - <<'PY'
import json, subprocess
selected = {'cljs-state-test-cljs-3406',
            'cljs-destructure-test-protocol-method-destructuring',
            'cljs-deftype-test-cljs-2128', 'cljs-deftype-test-cljs-2083'}
command = ['python3', '-B', 'tests/reference/compiler-language/report.py',
           '--run', '--strict', '--reference', 'both']
for row in map(json.loads, open('tests/reference/compiler-language/witnesses.jsonl')):
    if (row['fixture_id'].startswith('cljs-core-protocol-')
        or row['fixture_id'] in selected
        or row['assertion_id'].startswith('extend_to_native_test.cljs::')):
        command += ['--id', row['assertion_id']]
raise SystemExit(subprocess.call(command))
PY

python3 -B benchmarks/measure_compiler_interop.py --types --extensions \
  --output benchmarks/results/compiler-2026-10-10/core-protocols
```

## Observed verification and output

The ordinary suite passed **672 tests across 63 targets**, with **41 explicitly
ignored** reference/research checks. After the final nil-printer-options correction,
`cargo test --locked --quiet --test compiler_records --test execution --test provenance`
passed **53 tests**, with 37 explicit reference skips. The required reference
comparisons were invoked separately: all **45 independent fixtures** matched their
versioned expected outcomes on both references, comprising 35 identical contracts
and the ten explicit differences above. The strict imported-source selection
passed **79/79** on generated JavaScript and both pinned references, with zero
missing selected assertions. Direct Node assertions, all-target Clippy, formatting,
source inventory verification, all **17** coverage-accounting tests, and whitespace
checks passed. Skipped checks are not counted as successful comparisons.

The raw measurement is
`benchmarks/results/compiler-2026-10-10/core-protocols/output-startup.json`.
It retains source, verified expected results, SHA-256, raw/gzip output bytes and
nine fresh Node parse/evaluation samples for every program. The final runtime
hash matches the measured source. Timings exclude process launch and source
reading; historical measurements are unpaired observations, not thresholds.

The runtime is **123,196 raw / 28,716 gzip bytes**, compared with
**116,360 / 27,263 bytes** at the extension milestone. Primitive-only output is
still **88 bytes** and omits the runtime. The common collection program is
**123,353 bytes**, with a **2.780 ms** median; the custom type reduction program
is **123,990 bytes**, with a **3.110 ms** median. Native/instance extension
programs are **123,660 / 123,735 bytes**, with **2.706 / 2.837 ms** medians.
