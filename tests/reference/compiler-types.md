# Compiled user-defined types

Contract: ClojureScript **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, independently
compared with Maven **1.12.134** and checkout runtime **0.0.135109660**. Reference
runs verify the revision and `:optimizations :none`, `:static-fns false`,
`:def-emits-var false`. Tools: Clojure **1.12.4**, Java **25.0.2**, Node **v24.18.0**.

## Implementation and evidence

Type fields have resolved lexical identities, separate from parameters that can
shadow them. Generated methods keep the actual JavaScript receiver and read its
live fields. Returned functions keep that receiver; assignment requires truthy
`:mutable`, `:unsynchronized-mutable` or `:volatile-mutable` metadata. Explicit
host property writes retain the upstream interop behavior. Constructors evaluate
arguments in order; factories, basis vectors and factory documentation are emitted.
Type redefinition creates a new constructor/prototype without changing old instances.
Saved factories read the current constructor Var.

`IFn`, lookup, sequence, reduction and collection implementations dispatch through
the same public protocol machinery used by ordinary functions. In particular,
`reduce` checks `IReduce` before sequence traversal, `into` can consume a reducible
receiver, and `reduce-kv` checks `IKVReduce`. Keyword/symbol calls now route through
`ILookup`, retaining its actual two/three-argument distinctions. The custom-vector
upstream regression also executes its indexed/count/vector interfaces through
Subvec iteration and reduction.

`deftype*` accepts constructor fields, protocol-mask data and its optional body.
Method metadata emitted by upstream-style macros identifies the hidden receiver.
Low-level methods lacking that annotation retain the pinned missing-receiver
failure; masks affect normal protocol predicates. The two IFn invocation paths
retain their observable recurrence difference: the `.call` adapter clears its
hidden field receiver, while the direct invoke slot used by finite `apply` keeps it.

Implementation references read in full: `core.cljc/deftype`, `dt->et`,
`build-positional-factory`, `prepare-protocol-masks`, `fast-path-protocols`,
`analyzer.cljc/parse-type`, mutable-field `set!` analysis, and
`compiler.cljc/emit* :deftype` and `emit-fn-method`.

The complete imported bodies and their declarations were read before adaptation:

- `core_test.cljs`: `tset-638`, `test-920-watch-ops-return-ref`,
  `test-919-generic-cas`, `test-cljs-2083`, `test-cljs-2407`.
- `new_new_test.cljs`: `test-ifn`, `test-get-basis`, `test-515`.
- `collections_test.cljs`: `CustomVectorThing` and `test-cljs-2128`.
- `destructuring_test.cljs`: `First`, `DestructuringWithLocals`, both required
  protocols and the full `test-protocol-method-destructuring` body.

The executable matrix contains **30 assertion sites** for these complete bodies,
with zero missing assertions in this selection. Eight destructuring sites retain
their existing witness IDs; 22 sites are new. A namespace fixture keeps the original
`cljs.core-test` namespace for exact generated factory documentation. The custom
vector case preserves both original iterator assertions and both reduction branches.
The complete `new_new_test.cljs` and `collections_test.cljs` files are now inventoried,
bringing the source inventory to fourteen files, 2,876 assertion sites and 650
complete top-level bodies. Unexecuted assertions remain missing or explicitly
unrelated; this is not a whole-suite coverage claim. Original source hashes,
anchors and adaptation rationales are retained in `compiler-language/witnesses.jsonl`
and `fixture-origins.tsv`.

`qualified_method_test.cljs` contains the String qualified-method return-position
case, already covered by the interop member; it contains no type declaration.
The pinned `seqs_test.cljs` contains no `deftype` declaration. Independent seqable
receiver probes cover the requested user-type sequence behavior without relabeling
unrelated built-in sequence assertions as imported type coverage.

## Independent behavior and corrected earlier differences

**31 independent `deftype-*` fixtures** execute constructors, mutable metadata,
shadowing, escaping closures, borrowed receivers, callable overloads, saved/apply
calls, keyword lookup, seqable/reducible objects, reduced termination, recurrence,
destructuring, identity/hashing, type redefinition, nested reify capture copying,
raw annotated/unannotated types, masks, munged fields, Object overloads, evaluation
order, JavaScript constructor arity, collection operations and basis vectors.
The existing outcome/trace/stdout encoder records both successful and failing
paths. Five Rust/Node tests additionally use literal host values and check thirteen
located invalid forms. Three source namespaces execute alias-qualified types and
protocol methods, same-named constructors, saved factories, redefinition, old
instance methods and constructor identity in the emitted program.

Two older named-type bridge discrepancies are corrected. Named types do not close
over enclosing lexical locals; those names resolve as namespace Vars, yielding
undefined before initialization. Initial method maps are ordinary expressions,
not `fn` pre/post conditions. The retained `destructure-type-fields-capture` and
`destructure-record-fields-capture` probes now match the reference NaN results,
and moved out of the separate known-difference set. Reify-specific historical
condition behavior is outside this type correction.

An initial namespace probe called its type `Box`, colliding with the actual
`cljs.core/Box`. The reference resolved that constructor into core and failed
while installing namespace methods. The final independent probe uses `UserBox`
in both namespaces so it exercises the requested independent namespace identity;
the collision failure is not counted as a pass.

## Verification

```sh
cargo test --locked --tests --quiet
cargo test --locked --quiet --test analyzer --test compiler_types \
  --test compiler_reify --test compiler_protocols --test compiler_macros \
  --test namespaces --test execution --test compiler_language --test provenance
cargo test --locked --quiet --test sci_reference --test tokenizer
node tests/support/js_runtime.test.js
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --quiet --test execution compiled_deftype -- --ignored --nocapture

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --quiet --test execution compiled_destructuring -- --ignored --nocapture

python3 -B - <<'PY'
import json, subprocess
command = ['python3', '-B', 'tests/reference/compiler-language/report.py', '--scope', 'historical-selected',
           '--run', '--strict', '--reference', 'both']
for row in map(json.loads, open('tests/reference/compiler-language/witnesses.jsonl')):
    if row['fixture_id'].startswith('cljs-deftype-') or row['fixture_id'] == 'cljs-destructure-test-protocol-method-destructuring':
        command += ['--id', row['assertion_id']]
raise SystemExit(subprocess.call(command))
PY

python3 -B benchmarks/measure_compiler_interop.py --types \
  --output benchmarks/results/compiler-2026-10-10/types
```

The strict source run passed **30/30** on all three backends. Both pinned
references matched **31** independent type outcomes, the namespace program, and
**58** existing destructuring outcomes. Five literal host tests passed, including
thirteen malformed/immutable-write cases. The final focused compiler, macro,
namespace, execution, inventory and provenance run passed **167 tests**, with
**32 explicitly ignored** reference tests; the remaining SCI/tokenizer targets
passed **10 tests**, with one explicit reference skip. Direct Node runtime tests,
all-target Clippy, formatting, source inventory and whitespace checks passed.

The broad ordinary run passed **646 tests** before the provenance test found a
duplicate sidecar entry for the combined core namespace witness. That entry was
consolidated; the final focused run includes the passing provenance test, and the
remaining targets above completed the ordinary regression set. Reference skips
are not counted as passes; the required differential tests were invoked separately.

## Output and initialization

The [raw report](../../benchmarks/results/compiler-2026-10-10/types/output-startup.json)
contains source, validated results, raw/gzip sizes and nine fresh Node parse/evaluate
samples for each program. Process launch and file reads are excluded. Historical
figures are unpaired observations, not browser latency claims or regression limits.
Primitive-only output remains 88 bytes and omits the runtime. Mutable-type and
custom-reduction samples exercise the emitted methods rather than constant results.

The runtime is **104,288 raw / 24,452 gzip bytes**,
compared with **97,086 / 22,766 bytes** at the reify milestone. The common
collection program is **104,445 bytes**, with a
**1.834 ms** median. Mutable-type and
custom-reduction programs are **104,884** and
**105,061 bytes**, with **2.124 ms** and
**2.094 ms** medians. The increase includes
custom collection/reduction dispatch and low-level type mask handling.
