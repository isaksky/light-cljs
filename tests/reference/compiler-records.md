# Compiled records

Contract: ClojureScript **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, checked
against Maven **1.12.134** and checkout runtime **0.0.135109660**. Reference
runs verify the revision, `:optimizations :none`, `:static-fns false`, and
`:def-emits-var false`. Tools are Clojure **1.12.4**, Java **25.0.2** and
Node **v24.18.0**.

## Behavior

`defrecord` emits nominal constructors, positional and map factories, basis
vectors, factory documentation, immutable declared fields, extension entries,
and generated record protocols. `defrecord*` emits only the low-level constructor,
including `__meta`, `__extmap`, `__hash`, protocol masks and its optional body.
Annotated macro-generated methods can read those fields. The compiler follows
upstream constructor padding: a statically resolved record constructor appends
three nil arguments; an ordinary constructor alias uses JavaScript argument
binding. The positional factory ignores extra arguments and supplies nil internals.

Declared-field association preserves the type; removing a declared field produces
a plain map with metadata. Removing only extension entries preserves the record,
normalizes the empty extension map to nil, and invalidates the cached hash.
Map construction fills missing declared keys with nil, strips those keys from the
extension map, converts record input extensions to a plain map, and resets metadata.
Sequence and find return map entries. Empty records remain nominal records; `empty`
returns nil. Records do not acquire map callability, `IFind`, `IReduce`, or
`IEmptyableCollection` merely by storing map data. Explicit IFn implementations
and later IFn extensions work through saved calls and `apply`.

Equality requires the same current instance constructor, equal declared fields,
and equal extension maps; plain maps and other record types are unequal in both
directions. Structural map/set keys use the record hash and equality methods.
Hashing caches the result, excludes metadata, includes the nominal type name, and
preserves the runtime's existing equality-consistent hash contract; collection
hash integers are not an upstream byte-for-byte hash serialization contract.
Generated record core methods override duplicate declaration methods as the pinned
macro does. External extensions replace the relevant methods for existing instances.

User protocol, IFn and Object methods retain the existing type method machinery:
resolved fields, destructuring, overloads, escaped closures, exceptions, and recur.
Named record methods resolve surrounding names through namespace Vars rather than
capturing enclosing lexical locals. Redefinition leaves old instances and their
methods intact; saved factories and generated copy/update methods read the current
constructor Var, matching upstream even when a copy changes nominal generation.

`pr-str` uses the generated tagged record printer and supports external
`IPrintWithWriter` overrides. Default `str` and `.toString` retain upstream's
`[object Object]`; explicit Object methods may replace that behavior. Metadata
copies preserve explicit JavaScript undefined, methods, extensions and cached hash.
Cloning returns a distinct instance with those same values. Constructors do not
traverse or validate an opaque extension value before an operation actually uses it.

## Source evidence

Read in full before adaptation: `core.cljc/emit-defrecord`, `build-map-factory`,
`build-positional-factory`, `defrecord`, `dt->et`; `analyzer.cljc/parse-type` and
`parse new`; `compiler.cljc/emit* :defrecord`; and the relevant runtime predicate,
find, clone and empty implementations.

The complete `new_new_test.cljs::test-records` body contributes **27 assertions**.
The complete `core_test.cljs` regressions `test-cljs-2079`, `test-cljs-2787`, and
`test-cljs-3305`, and `seqs_test.cljs::test-cljs-2482` add **15 in-scope assertions**.
All setup, declarations, expressions and method bodies remain intact; only test
wrappers become executable `do`/`assert`. The full mixed sorted-map equality body
executes, while its three sorted-collection assertions retain their separate scope.
Existing witnesses cover factory documentation (`test-cljs-2407`), basis vectors,
metadata protocol precedence (`test-cljs-2960`), and all eight Search1–Search4
recurrence assertions. The combined selection has **59 assertion sites**, with
zero missing assertions in that selection.

`hashing_test.cljs` was read in full and contains no record-specific declaration
or assertion. Independent record key/hash probes therefore remain explicitly
independent. `core_test.cljs::test-cljs-1780` requires transducer execution and stays
in that separately excluded library family; its map transformation is not replaced
with a different operation to manufacture record coverage.

Original assertion/source hashes, paths, anchors and adaptation rationale are in
`compiler-language/witnesses.jsonl` and `fixture-origins.tsv`. The full sequence
source is now inventoried: **15 files, 3,137 assertion sites, 682 complete top-level
bodies**. The inventory is not a whole-suite pass claim.

## Verification

The **38 independent `record-*` outcome fixtures** cover constructor arities and aliases,
map inputs, arbitrary extension keys, data operations, nominal equality, structural
keys, metadata, undefined, cloning, predicates, callable records, methods,
exceptions, recurrence, extension, redefinition, raw syntax, evaluation order,
printing and stdout. Six Rust/Node tests separately check literal host arrays and
seven located invalid forms. Three namespaces execute qualified constructors,
saved factories, same-named types, old instances and generated copies after
redefinition, structural lookup and tagged printing.

```sh
cargo test --locked --tests --quiet
node tests/support/js_runtime.test.js
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --quiet --test execution compiled_record -- --ignored --nocapture

python3 -B - <<'PY'
import json, subprocess
command = ['python3', '-B', 'tests/reference/compiler-language/report.py', '--scope', 'historical-selected',
           '--run', '--strict', '--reference', 'both']
for row in map(json.loads, open('tests/reference/compiler-language/witnesses.jsonl')):
    if (row['fixture_id'].startswith('cljs-record-')
        or row['fixture_id'] in ['cljs-reify-cljs-2085-test',
                                'cljs-deftype-test-get-basis',
                                'cljs-protocol-test-cljs-2960']
        or row['assertion_id'].startswith('core_test.cljs::test-cljs-2407::')):
        command += ['--id', row['assertion_id']]
raise SystemExit(subprocess.call(command))
PY

python3 -B benchmarks/measure_compiler_interop.py --records \
  --output benchmarks/results/compiler-2026-10-10/records
```

The strict source run passed **59/59** on generated JavaScript and both pinned
oracles. Both oracles matched **38 independent record outcomes**, including exact
effects and stdout, and the namespace program. The first broader run stopped on
the obsolete `defrecord*` unsupported boundary; that boundary now records accepted
syntax, backed by executing raw-constructor and annotated-method cases. The final
ordinary run passed **664 tests across 62 targets**, with **38 explicitly ignored**
reference/research checks. Required record oracle tests were invoked separately;
ignored checks are not counted as passing comparisons. A repeated source run hit
its build timeout while waiting behind the full regression Cargo lock. It was
rerun using `--no-build --binary "$PWD/target/debug/light-cljs"` after verifying
that the binary was newer than the sources; all 59 assertions passed again. Direct Node runtime checks,
all-target Clippy, formatting, source inventory and whitespace validation passed.

## Output measurement

The raw report is `benchmarks/results/compiler-2026-10-10/records/output-startup.json`.
It retains validated source/result pairs, raw/gzip sizes and nine fresh Node
parse/evaluate samples per program. Process startup and source reading are excluded;
the historical type milestone is an unpaired observation, not a regression limit.

The runtime is **110,876 raw / 25,934 gzip bytes**,
compared with **104,288 / 24,452 bytes** at the type milestone. Primitive-only
output remains **88 bytes** and omits the runtime. The common collection sample
is **111,033 bytes**, with a **1.988 ms** median.
The record update/reduction and record method/metadata programs are
**111,562** and **111,846 bytes**, with
**2.971 ms** and **3.142 ms** medians.
The increase supplies record protocol implementations and the helper operations
needed by the upstream record bodies.
