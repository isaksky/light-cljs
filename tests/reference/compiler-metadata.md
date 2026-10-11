# Compiled runtime metadata

Contract: ClojureScript checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`,
compared with Maven **1.12.134** and checkout runtime **0.0.135109660**.
Reference runs check `:optimizations :none`, `:static-fns false`, and
`:def-emits-var false`, using Clojure **1.12.4**, Java **25.0.2**, and Node
**v24.18.0**. No release/checkout difference was observed in this selection.

## Source and behavior

The complete `metadata_test.cljc` file, `core_test.cljs/test-metadata`,
`test-fn-with-metadata`, `test-reify-meta`, and `apply_test.cljs/meta-fn-test`
bodies were read. The runtime contract comes from `src/main/cljs/cljs/core.cljs`
`IMeta`, `IWithMeta`, `meta`, `with-meta`, `vary-meta`, `MetaFn`, `Var`, `Atom`,
collection/sequence implementations and reference metadata functions. Source
metadata follows `analyzer.cljc/analyze-wrap-meta`, `elide-irrelevant-meta`,
`compiler.cljc/emit-constant`, and `core.cljc/reify` and record implementations.
All paths are in the pinned checkout above.

Metadata is stored separately from structural payloads and internal hashes.
`with-meta` and `vary-meta` preserve original values; metadata does not change
collection/symbol/Var equality or key lookup. Map entries produce ordinary
vectors when annotated. Records retain their constructor, methods, fields and
metadata through association and removal of extension entries. Removing a
record field produces an ordinary map with the same metadata. `empty` on a
record or an unsupported host value returns nil.

Vector/map/set/queue updates retain metadata. List `conj` and `empty` retain it;
Cons and ordinary seq `conj`/`empty` discard it. Collection `seq` creates an
unannotated view; an existing seq retains its own metadata. Tails normally
lose the head's metadata without stripping metadata already on the tail.
Queue seq tails and empties retain metadata. LazySeq `empty` retains metadata
in the pinned ordinary runtime; wrapping a lazy sequence does not realize it.
`sequence` preserves seq identity and laziness; `vec` removes metadata.
Nil and undefined metadata remain distinguishable during updates.

`IMeta` and `IWithMeta` dispatch to implemented type methods. Reify implements
both, retains captured locals and supports direct form metadata. Atoms support
`IMeta` and mutable `reset-meta!`/`alter-meta!`, with state independent of
metadata. They reject `with-meta`. Vars support immutable metadata copies and
live dereference/invocation; their metadata is distinct from the `.meta`
property that the reference mutation helpers assign. Functions satisfy `IMeta`
and wrappers preserve the callable-object arities and receiver rules, including
large `apply` tails and thrown value identity. Host objects, arrays, keywords
and primitive values do not gain metadata merely by having host properties.

Executable collection, function and reify metadata maps evaluate normally.
Quoted metadata remains data. Reader file/line/column/source fields and the
analyzer's private marker are elided from runtime metadata. Definition metadata
used by macros and Vars remains separate. A native macro bridge test verifies
both generated runtime metadata and macro-definition metadata.

Supporting helpers used by the complete upstream interface inputs include
`empty`, `peek`, `pop`, `rseq`, `keys`, `vals`, `repeat`, `cycle`, and naturally
ordered `sorted-map`. This is not a claim of the separate sorted/transient
collection API surface, arbitrary protocol extension or the full atom/watch
feature. Metadata predicates currently cover `IMeta` and `IWithMeta`.

## Evidence and reproduction

**32 original assertion sites** pass emitted JavaScript and both references:
12 collection/sequence helper sites run with **every original interface input**,
including sorted maps, large vectors/maps/sets, reverse/key/value sequences,
cycle/range/repeat/iterate, chunked map output and Cons. Literal and reify sites
add two; existing function/apply metadata witnesses add 18. Reader conditionals
select their original CLJS branch. Only `is`, `testing` and `deftest` wrappers
are adapted to executable assertions/`do`. The witness manifest and provenance
sidecar retain assertion hashes, source anchors and adaptation rationales.

**38 independent probes** compare literal outcomes, effects and stdout with
both references. Node assertions separately check private hash consistency,
key representatives, original metadata identity, lazy sharing, unsupported host
values, and callable receivers/arguments through 1,000 arguments. These do not
rely on the common result encoder. The full ordinary Rust suite passed **632**
tests, with **25** explicitly ignored reference tests; final compiler, macro,
reader, namespace, provenance and accounting checks passed **116** tests
(**21** explicitly ignored references) after the later refinements. All-target
Clippy, Node runtime assertions, formatting, whitespace checks and pinned source
inventory verification passed.

```sh
cargo test --locked --tests --quiet
cargo test --locked --test execution --test compiler_macros --test compiler_reader \
  --test compiler_language --test provenance --test namespaces --quiet
node tests/support/js_runtime.test.js
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution \
  compiled_metadata_match_pinned_values_effects_and_stdout -- --ignored --nocapture

python3 - <<'PY'
import json, subprocess
command = ['python3', '-B', 'tests/reference/compiler-language/report.py', '--scope', 'historical-selected',
           '--run', '--strict', '--reference', 'both']
for row in map(json.loads, open('tests/reference/compiler-language/witnesses.jsonl')):
    if (row['fixture_id'].startswith('cljs-metadata-') or
        row['assertion_id'].startswith('apply_test.cljs::meta-fn-test::') or
        row['assertion_id'].startswith('core_test.cljs::test-fn-with-metadata::')):
        command += ['--id', row['assertion_id']]
raise SystemExit(subprocess.call(command))
PY
```

[Size/startup samples](../../benchmarks/results/compiler-2026-10-09/metadata/output-startup.json)
compare the retained reader milestone binary with this implementation. That
older milestone predates several other compiler features, so the difference
must not be attributed only to metadata. Primitive `42` output remains **104
bytes**. Runtime-bearing metadata output and nine fresh-process parsing plus
initialization samples are recorded separately; the runtime remains embedded
once per compilation unit. These measurements are observations, not a latency
threshold or a general performance guarantee.
