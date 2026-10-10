# Compiled reify objects

Contract: ClojureScript **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, compared
independently with Maven **1.12.134** and checkout runtime **0.0.135109660**.
Each reference build checks the revision and `:optimizations :none`,
`:static-fns false`, `:def-emits-var false`. Observed tools: Clojure **1.12.4**,
Java **25.0.2**, Node **v24.18.0**.

## Implementation and source evidence

Reify uses the same anonymous-constructor, field, and protocol installation
machinery as compiled user types. A source site has one constructor; each
evaluation constructs a new instance with the visible lexical locals as fields.
Methods access their own receiver's fields, including reads delayed inside
returned functions. Nested reify expressions copy their own environment. A
metadata copy constructs another instance of the same type with current field
values; it shares captured references intentionally, including atoms.

The full relevant upstream bodies were read: `core_test.cljs/test-reify-meta`,
`test-cljs-1451`, `test-cljs-1420`, `test-cljs-3406`,
`destructuring_test.cljs/test-cljs-1600`, and `recur_test.cljs/cljs-2085-test`
with all four Search record declarations. The implementation references are
`src/main/clojure/cljs/core.cljc/reify`, `dt->et`, `adapt-obj-params`,
`adapt-ifn-params`, `adapt-ifn-invoke-params`, `adapt-proto-params`, and
`src/main/clojure/cljs/analyzer.cljc/parse-type` and `parse 'recur`.
`src/main/cljs/cljs/core.cljs/get-in`, `type`, `IHash`, and `IEquiv` supply the
small runtime additions needed by the original tests and identity probes.

The executable language matrix now maps **36 original assertion sites** to
these six complete source bodies. Nine mappings are new: the single lookup
regression and the eight recurrence assertions. Existing mappings retain their
original IDs and source hashes. The recurrence import preserves the actual
record declarations and constructors; independent reify recurrence probes are
not substituted for those original source assertions. The matrix has **zero
missing assertions for this selected set**, without claiming whole-suite or
whole-core coverage. The exact assertion hashes, paths, anchors and adaptation
rationales remain in `compiler-language/witnesses.jsonl`, the immutable source
inventory, and `fixture-origins.tsv`.

## Independent execution

**35 independent outcome fixtures** (`reify-*`) test fresh identity and
per-site types, visible/unused/shadowed captures, loop captures, independent
mutable environments, copied scalar fields, borrowed receivers, nested closures
and reify expressions, destructuring, overloaded methods, recurrence, arbitrary
thrown object identity, marker/partial protocols, and saved/apply calls.
They also cover Object fixed/variadic/large signatures, direct source metadata,
explicit metadata overrides, false/undefined/nil metadata, and custom
hash/equivalence participating in maps and sets. Effects remain observable in
the existing `{outcome, trace, stdout}` protocol.

Five direct Rust/Node tests use literal host expectations instead of the shared
encoder, checking constructor/receiver identity, independent captured fields,
public protocol slots, IFn invocation, Object overloads, nested capture copying
and invalid method/recur forms. The three-namespace fixture under
`tests/fixtures/namespaces/reify` creates a Potok-style event implementing
separate update/effect/watch protocols. It executes aliased and saved protocol
calls, destructured state/context, shared captured state, metadata copies and a
watch closure entirely in generated JavaScript, with exact stdout checked
against both references.

The pinned semantics include a few details that must not be normalized away:

- Reify metadata copies have fresh identity even when the metadata is unchanged;
  their type and methods remain the same. Source metadata initializes a receiver
  directly rather than calling an explicitly overridden `IWithMeta` method.
- The IFn `.call` adapter and direct invocation slot have different capture
  behavior after recurrence. Existing differential cases retain that contract;
  nested reify values created before recurrence retain their copied captures.
- An Object dispatcher with fixed and variadic overloads invokes the variadic
  delegate with a function receiver. Captured fields can therefore be undefined.
  Single variadic Object methods preserve the instance receiver. Independent
  fixtures assert both outcomes. The upstream warning about an explicit target
  in Object recurrence is observable in reference output; light-cljs does not
  expose that warning stream.
- Reify uses separate forms for overloads. `extend-type` retains grouped method
  signatures; its syntax is not substituted for the upstream reify grammar.
- An initial custom-equivalence probe used `#{a b c}` where `a` and `b` compare
  equal: release 1.12.134 returned a two-member set, while the pinned checkout
  threw `Error: Duplicate key`. The final reify probe uses explicit
  `(hash-set a b c)` and passes both versions. This documents the literal-reader
  version difference rather than claiming that duplicate literal behavior
  passed either common contract or an imported-source assertion.

## Verification

```sh
cargo test --locked --tests --quiet
cargo test --locked --test compiler_reify --test compiler_protocols \
  --test compiler_macros --test namespaces --test execution \
  --test compiler_language --test provenance --quiet
node tests/support/js_runtime.test.js
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution compiled_reify -- --ignored --nocapture

python3 -B - <<'PY'
import json, subprocess
fixtures = {'cljs-metadata-reify', 'cljs-destructure-test-cljs-1600',
            'cljs-protocol-test-cljs-1451', 'cljs-state-test-cljs-3406',
            'cljs-reify-test-cljs-1420', 'cljs-reify-cljs-2085-test'}
command = ['python3', '-B', 'tests/reference/compiler-language/report.py',
           '--run', '--strict', '--reference', 'both']
for row in map(json.loads, open('tests/reference/compiler-language/witnesses.jsonl')):
    if row['fixture_id'] in fixtures:
        command += ['--id', row['assertion_id']]
raise SystemExit(subprocess.call(command))
PY

python3 -B benchmarks/measure_compiler_interop.py --reify \
  --output benchmarks/results/compiler-2026-10-10/reify
```

The full ordinary suite passed **651 tests**, with **34 explicitly ignored**
reference/benchmark tests. The later focused compiler, macro, namespace,
provenance and execution run passed **126 tests** with **30 reference skips**.
The strict imported run passed **36/36** against both references, and the reify
differential tests passed the event program and all **35 independent fixtures**.
Direct Node runtime assertions, all-target Clippy, formatting, inventory checks
and whitespace checks passed. Final metadata refinements were checked by the
focused direct/execution/provenance tests and the pinned differential tests.

## Output and startup

The [raw report](../../benchmarks/results/compiler-2026-10-10/reify/output-startup.json)
records raw/gzip output bytes and nine fresh Node parse/evaluation samples per
program, validating each result. Timing excludes process launch and source
reading; historical figures are unpaired observations, not browser latency or
regression thresholds. Primitive-only output remains **88 bytes** without the
runtime. The report includes the common collection sample plus captured-instance
and metadata-copy reify programs.

The runtime is **97,086 raw / 22,766 gzip bytes**, up **2,404 / 606 bytes**
from the protocol milestone. The common collection program is **97,243 bytes**
with a **2.017 ms** median (previous unpaired observation: **1.794 ms**).
Captured-instance and metadata-copy samples are **97,832** and **97,936 bytes**,
with **2.309 ms** and **2.450 ms** medians. Final focused direct/execution/
provenance verification passed **47 tests**, with **30 explicit reference skips**;
the separately invoked final differential tests passed both references.
