# Compiled delays and lazy sequence values

Contract: ClojureScript `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, compared
with Maven **1.12.134** and checkout runtime **0.0.135109660**. Both reference
runs verify `:optimizations :none`, `:static-fns false` and
`:def-emits-var false`. Observed tools: Clojure **1.12.4**, Java **25.0.2**,
Node **v24.18.0**. No version difference occurred in this selection.

## Semantics and source evidence

The complete upstream `core_test.cljs/test-delay`,
`collections_test.cljs/test-lazy-seq-realized?`, `test-cljs-2474`, the discarded
`test-cljs-3420-lazy-seq-caching-bug`, and the surrounding lazy regressions were
read before implementation. The selected lazy sections of
`seqs_test.cljs/test-empy-and-seq`, `test-empty?`,
`test-ISequential-indexOf`, and `test-ISequential-lastIndexOf` retain their
setup, original assertions and metadata inputs. The implementations inspected
are `core.cljc/delay` and `lazy-seq`, plus `core.cljs/Delay`, `LazySeq`,
`force`, `delay?`, `realized?`, `nthrest`, and sequential index helpers.
All paths are relative to the pinned checkout's `src/main` or `src/test` trees.

Deferred macro bodies compile as zero-argument closures, preserving lexical
captures, ordinary recur/exception boundaries, runtime namespace resolution,
and native-macro output. `delay`, `force`, dereference, `delay?`, `realized?`,
`IPending/-realized?`, `Delay`/`->Delay`, and `LazySeq`/`->LazySeq` are supported.
Saved core functions, core aliases and custom pending implementations use the
same runtime dispatch.

A successful thunk clears its function and retains its exact result, including
nil/undefined and object identity. A throwing thunk remains available for retry;
reentrant realization uses ordinary calls and preserves the order of successful
inner/outer writes. There is no native interpreter exception-cache policy or
synthetic recursion error. Recursive user thunk calls can exhaust the host stack;
iterative traversal of returned LazySeq wrappers uses constant stack space.

Lazy sequence construction and metadata wrapping do not realize the body.
Each metadata wrapper has its own pending state and delegates to the source,
sharing successful contents and the cached structural hash. Annotating an
already-realized source still creates a pending wrapper. Matching metadata
identity returns the original value. Sequence identity remains distinct from
content equality. `seq`, `first`, `rest`, `next`, `nthrest`, `nthnext`, `count`,
`reduce`, metadata, equality/hash, printing, host iteration and index methods
operate on these values. Reduced termination never requests the next input.

The pinned LazySeq stores the final **raw** non-lazy result, then calls `seq`
on it during each traversal. A custom nonmemoized ISeqable can therefore execute
again; this preserves the upstream behavior illustrated by its discarded caching
regression rather than silently implementing that test's desired behavior.
Lazy hash calculation itself is cached. Delay `pr-str` reports pending/ready
without forcing; `str` retains the upstream host-object representation.

Payloads use ordinary JavaScript references in weakly keyed runtime storage.
Successful realization releases the captured thunk; unreachable pending or
failed values and cycles require no manual ownership/reclamation mechanism.

## Executable evidence

**27 source assertion sites** pass generated JavaScript and both pinned
references. Full-body witnesses preserve the assertion order of delay, pending
state and metadata regression tests. The random producer uses the equivalent
`js/Math.random` call. Selected seq sections preserve the ordinary non-lite
metadata branch; the inactive/alternative branch is not reported as passing.
The two upstream index assertions retain their message arguments outside unary
`=`; independent probes additionally assert actual indices. Assertion hashes,
anchors, adaptation rationales and local IDs are recorded in
[the witness matrix](compiler-language/witnesses.jsonl) and
[provenance](fixture-origins.tsv).

**23 independent probes** pass exact expected outcomes, effects and stdout
against both references. They cover retries, thrown nil/undefined, reentrancy,
nested failures, raw ISeqable effects, pending metadata states, infinite prefixes,
reduced termination, constructors, public fields, identity, hash caching and
printing. They are separate from imported assertion coverage.

`tests/compiler_deferred.rs` adds direct JSON/host expectations without the
shared result encoder, 100,000 nested wrappers, 5,000 abandoned failing values
with captured arrays under a 96 MiB V8 heap and explicit GC, and namespace/native
macro capture tests. Focused compiler, execution, macro, namespace, provenance
and accounting tests passed **125** tests (**39** explicitly ignored reference
tests). The separate analyzer checks passed **35** tests. Node runtime assertions,
Clippy, formatting and pinned inventory verification also pass.

```sh
cargo test --locked --test compiler_deferred --test execution --test compiler_language \
  --test compiler_macros --test namespaces --test provenance --quiet
cargo test --locked --test analyzer --quiet
node tests/support/js_runtime.test.js
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution \
  compiled_deferred_match_pinned_values_effects_and_stdout -- --ignored --nocapture

python3 - <<'PY'
import json, subprocess
args = ['python3', '-B', 'tests/reference/compiler-language/report.py',
        '--run', '--strict', '--reference', 'both']
for row in map(json.loads, open('tests/reference/compiler-language/witnesses.jsonl')):
    if row['fixture_id'].startswith('cljs-deferred-'):
        args += ['--id', row['assertion_id']]
raise SystemExit(subprocess.call(args))
PY

python3 benchmarks/measure_compiler_interop.py --deferred \
  --output benchmarks/results/compiler-2026-10-10/deferred
```

[Size/startup observations](../../benchmarks/results/compiler-2026-10-10/deferred/output-startup.json)
record nine fresh V8 parse/evaluate samples per program. Relative to the retained
collection-helper sample, runtime source grows **4,095 bytes** (128,468 to
132,563), compressed runtime **1,064 bytes** (29,891 to 30,955). Primitive `42`
remains **88 bytes**. Delay output is **132,781 bytes**, bounded lazy reduction
**132,937 bytes**; median parse/evaluation samples are **2.81 ms** and **3.57 ms**.
These historical timing samples are not paired experiments or performance
thresholds. General lazy producer/comprehension coverage remains a separate
feature; this evidence does not claim the whole lazy-sequences inventory passes.
