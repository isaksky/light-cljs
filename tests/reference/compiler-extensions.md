# Compiled protocol extension

Contract: ClojureScript **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, checked
against Maven **1.12.134** and checkout runtime **0.0.135109660**. Each reference
run verifies the revision, `:optimizations :none`, `:static-fns false`, and
`:def-emits-var false`. Tools: Clojure **1.12.4**, Java **25.0.2**, Node **v24.18.0**.

## Behavior and architecture

The compiler handles `extend-type`, `extend-protocol`, `specify!`, `specify`, and
macro-style `^{:extend :instance}` targets. Native selectors follow the CLJS
mapping, including nil/undefined, object, boolean, number, string, array,
function, symbol, bigint, and default. Named type/record and JavaScript constructor
extensions install prototype methods; built-in collection and named-value types
have distinct extension prototypes around their protected payloads.

Existing values observe later extensions. Opted-in metadata methods precede
instance and prototype methods, followed by native/default dispatch. Saved method
functions use current fallback tables. A currently selected method finishes even
when it replaces its own extension; subsequent calls use the replacement.
Native replacement replaces a method's overload function, whereas prototype
replacement changes only the written arity slots. Repeated protocol/target groups
keep the last group, and repeated native method forms keep the last method form.
Extension method recur uses the full explicit parameter count while preserving
its actual JavaScript receiver on every iteration. This includes boxed
JavaScript primitive prototype dispatch and detached public method slots.
The reference warns about some accepted repeated/underspecified forms; this
compiler preserves their execution behavior without exposing that warning stream.

`specify!` evaluates its target once and returns it. `specify` calls `ICloneable`
and returns a distinct value with preserved collection payload. Instance methods
do not mutate that original. JavaScript primitive property writes are silent
no-ops; nil instance writes and unsupported clone operations fail. IFn extension
signatures must be grouped, including a single signature, as required by the
upstream extension macro.

Core predicates consult native protocol membership. Raw core protocol methods
and native object sequence, lookup, reduction, contains, empty and printing
operations dispatch through extensions. Public operations retain upstream
fast-path precedence: nil remains empty for `seq`/`count`/`get`, and arrays/strings
keep their direct sequence/index/count/reduce paths despite native table entries.
Native object/default methods cannot override owned collection implementations.

## Source evidence and isolation

Read complete relevant source before adaptation: all of
`src/test/cljs/cljs/extend_to_native_test.cljs`,
`new_new_test.cljs::test-414` and its IWoz/noz setup, and
`core_test.cljs::test-cljs-2960`/`test-cljs-3313` with their declarations.
The implementation contract also uses `core.cljc`'s native selector map,
`extend-type`, `extend-protocol`, `specify!`, `specify`, method adaptation and
validation helpers, and `core.cljs`'s protocol predicates, public core fast
paths, and printing dispatch.

The executable matrix covers **all 31 assertion sites** in the native-extension
file plus **all four CLJS-414 assertions**. It executes the entire selected test
bodies, including all setup, extensions, helper functions, primitive printer
restoration, and original assertion expressions. Only test wrappers become
`do`/`assert`. The earlier object seq/get/reduce, slash-method and contains
witnesses remain unchanged. Original assertion hashes, source paths, anchors and
adaptation rationale are in `compiler-language/witnesses.jsonl` and
`fixture-origins.tsv`. CLJS-3313 has no assertion macro and is not counted as
an additional source site; an independent probe executes its metadata precedence.

Native tables are shared by upstream namespaces. The matrix's `isolated: true`
witnesses compile together but load and execute one fixture per fresh Node
process. Independent extension comparisons use the same process isolation, and
the full execution-reference driver isolates these mutation fixtures as well.
The compiler's private runtime tables are separate per compilation; standalone
host tests also create a fresh Node process for every program.

**30 independent outcome fixtures** cover every requested native selector,
default dispatch, existing type/record/built-in values, saved calls and `apply`,
instance/type/metadata precedence, replacement during a call, partial replacement,
duplicate forms, return values, native/full-arity method recur, clone membership, IFn/Object methods, raw
core dispatch tables, native fast paths, failure categories, effects and stdout.
Five Rust/Node tests additionally compare literal host arrays independently of
the shared encoder. Three namespaces exercise qualified types, protocol and core
aliases, saved functions, later extensions, metadata, existing instances and
instance precedence. Seven malformed-form probes retain located local diagnostics;
all **188** current boundary classifications agree with both references apart
from the already documented `let-binding-amp` release/checkout distinction.

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
cargo test --quiet --test execution compiled_extension_ -- --ignored --nocapture

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --quiet --test execution analyzer_boundary_classifications_match_reference \
  -- --ignored --nocapture

python3 -B - <<'PY'
import json, subprocess
command = ['python3', '-B', 'tests/reference/compiler-language/report.py',
           '--run', '--strict', '--reference', 'both']
for row in map(json.loads, open('tests/reference/compiler-language/witnesses.jsonl')):
    if (row['assertion_id'].startswith('extend_to_native_test.cljs::')
        or row['fixture_id'] == 'cljs-extension-test-414'):
        command += ['--id', row['assertion_id']]
raise SystemExit(subprocess.call(command))
PY

python3 -B benchmarks/measure_compiler_interop.py --extensions \
  --output benchmarks/results/compiler-2026-10-10/extensions
```

The strict imported run passed **35/35** sites on generated JavaScript and both
references. Both references matched the independent isolated outcomes, including
literal values, side effects, failures and stdout, and the namespace program.
An initial independent IFn probe used the ungrouped extension shape rejected by
both references; the runtime probe now uses the valid grouped form, and a
separate negative fixture preserves the original rejection. An initial primitive
instance expectation incorrectly assumed strict-mode assignment failure; both
references returned the primitive unchanged, which is now the tested contract.

The full ordinary suite passed **671 tests across 63 targets**, with **40 explicitly
ignored** oracle/research checks. The final compiler/analyzer/macro/namespace/
execution/provenance run passed **183 tests across 12 targets**, with **36 explicit
oracle skips**; the required extension comparisons were run separately. After the
final clone-membership refinement, the isolated local and both-reference runs
passed all **30 fixtures**. Direct Node runtime assertions, all-target Clippy,
a final library Clippy check, formatting, inventory verification and whitespace
checks passed. The accounting suite passed all **17 tests**, including process
isolation. No skipped reference check is counted as a successful comparison.

## Output and startup

The raw report is `benchmarks/results/compiler-2026-10-10/extensions/output-startup.json`.
It retains source/result pairs, output SHA-256, raw/gzip bytes, and nine fresh Node
parse/evaluation samples per program. Timing excludes process launch and reading
source. Earlier measurements are unpaired observations, not performance thresholds.

The runtime is **116,360 raw / 27,263 gzip bytes**,
compared with **110,876 / 25,934 bytes** at the record milestone. Primitive output
remains **88 bytes**, without the runtime. The collection sample is
**116,517 bytes** with a **2.329 ms** median.
Native and instance extension samples are **116,824** and **116,899 bytes**,
with **2.449 ms** and **2.699 ms** medians.
