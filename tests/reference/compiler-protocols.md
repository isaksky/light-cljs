# Compiled user protocols

Contract: ClojureScript `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, checked
against Maven **1.12.134** and checkout runtime **0.0.135109660**. Reference runs
verify revision and `:optimizations :none`, `:static-fns false`,
`:def-emits-var false`; Clojure **1.12.4**, Java **25.0.2**, Node **v24.18.0**.

## Source and executable evidence

The complete relevant upstream bodies were read before implementation:
`src/test/cljs/cljs/core_test.cljs/test-cljs-1451`, `test-cljs-2960`, and
`test-cljs-3313`; `src/test/cljs/cljs/extend_to_native_test.cljs`;
`src/test/clojure/cljs/compiler_tests.clj/no-warn-on-emit-invoke-protocol-method`;
and `src/test/clojure/cljs/analyzer_tests.clj/test-cljs-3120`.
The implementation contract comes from `src/main/clojure/cljs/core.cljc`'s
`defprotocol`, `implements?`, `satisfies?`, `extend-type`, `extend-protocol`,
protocol prefix and native type mappings, and `src/main/cljs/cljs/core.cljs`'s
native extensions and protocol definitions.

The executable language matrix maps **23 original assertion sites**, with their
source hashes and adaptation rationale, to current compiler and both reference
runs. Twelve retain complete runtime bodies: qualified reify calls, all four
metadata-extension assertions, all four native-object seq/lookup/reduce assertions,
slash-named methods, and both native contains assertions. Eleven retain every
receiver expression from the warning regression, use real declarations plus a
default implementation, and require zero compiler/reference warnings. They follow
the batch's static-fns false setting, rather than claiming the upstream private
static emitter's structure. The inventory keeps that original test body intact.

`test-cljs-3313` contains no assertion macro, so it does not increase the coverage
count. Metadata-before-direct dispatch is checked by `test-cljs-2960` and independent
probes. `test-cljs-3120`'s private analyzer table checks remain explicitly unrelated;
public method/protocol Var metadata is tested directly. This earlier member leaves native protocol families, record operations and
specification APIs to their later audits; the [extension audit](compiler-extensions.md)
now completes native and instance extension coverage.
No whole-upstream-suite coverage percentage is claimed.

**46 independent outcome fixtures** cover marker and partial membership, receiver
and argument ordering, all native receiver categories, missing implementation and
arity failures, saved methods and apply, fixed overloads, large fixed signatures,
metadata opt-in and false/nil entries, raw tables and slots, same-spelled methods,
protocol and method-root redefinition, prototype extension, and core protocol
membership. Three namespaces separately exercise aliases, qualified calls,
method metadata, same-spelled protocol/method names and native extension.
Direct Rust/Node tests bypass the shared encoder for receiver/reference identity,
public slots, metadata precedence, invalid constructors and malformed declarations.

Several details deliberately follow CLJS rather than JVM assumptions:

- A declaration evaluates to `undefined`. A repeated method declaration retains
  the last declaration; fixed single-arity functions retain JavaScript underfilled
  and extra-argument behavior.
- Protocol membership does not require complete methods and does not follow from
  metadata alone. General operations such as `seq` supporting strings do not make
  strings satisfy `ISeqable`; ordinary functions do not satisfy `IFn`.
- Existing direct slots survive protocol redefinition. Native dispatch uses the
  current method Var, and redefinition resets native tables. Predicate protocol
  symbols resolve statically; copying a protocol value into a differently named
  local does not copy its direct marker name.
- The reference accepts variadic protocol signatures with warnings. Its expansion
  forwards an unbound ampersand followed by the packed tail; the probe preserves
  this unusual result instead of pretending it is normal variadic dispatch.
  Light-cljs does not currently expose the upstream warning stream for these
  accepted forms; its runtime result agrees with both references.

## Reproduction

```sh
cargo test --locked --tests --quiet
node tests/support/js_runtime.test.js
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution compiled_protocols -- --ignored --nocapture

python3 -B - <<'PY'
import json, subprocess
command = ['python3', '-B', 'tests/reference/compiler-language/report.py', '--scope', 'historical-selected',
           '--run', '--strict', '--reference', 'both']
for row in map(json.loads, open('tests/reference/compiler-language/witnesses.jsonl')):
    if row['fixture_id'].startswith('cljs-protocol-'):
        command += ['--id', row['assertion_id']]
raise SystemExit(subprocess.call(command))
PY

python3 -B benchmarks/measure_compiler_interop.py --protocols \
  --output benchmarks/results/compiler-2026-10-10/protocols
```

The full ordinary Rust suite passed **645 tests** with **32 explicit skips**.
After the final raw-core-method refinement, focused compiler, execution,
provenance and inventory checks passed **47 tests**, with **28 explicitly ignored
reference tests**. The two protocol differential tests passed the three-namespace
program and all 46 independent outcome/effect/stdout fixtures against both
references. The strict imported run passed all 23 selected sites. Node's direct
runtime assertions, all-target Clippy, a final library Clippy check, formatting,
source inventory and whitespace checks passed. The final public slash-method
slot refinement was additionally checked by the standalone compiler tests.

## Output and startup

The [raw report](../../benchmarks/results/compiler-2026-10-10/protocols/output-startup.json)
records output bytes, gzip sizes and nine fresh Node parse/evaluation samples per
program, with result validation. Primitive-only output still omits the runtime.
Historical comparisons are not paired measurements; they exclude process launch
and source reading and do not establish browser startup latency.

The runtime is **94,682 raw / 22,160 gzip bytes**, up **10,307 / 2,391 bytes** from the interop milestone. Primitive `42` remains **88 bytes**. The common collection sample is **94,839 bytes**, with a **1.794 ms** median versus the earlier **1.524 ms** observation. Direct and metadata protocol samples are **95,193** and **95,104 bytes**, with **1.958 ms** and **2.195 ms** medians. These fresh-process measurements validate results and record every sample; historical timings are not paired regression thresholds.
