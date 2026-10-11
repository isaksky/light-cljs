# Compiled control forms and iteration

Contract: ClojureScript checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`,
compared with Maven **1.12.134** and checkout runtime **0.0.135109660**.
Every reference run verified its identity and `:optimizations :none`,
`:static-fns false`, `:def-emits-var false`, with Clojure **1.12.4**, Java
**25.0.2** and Node **v24.18.0**.

## Source and implementation

The complete `primitives_test.cljs/test-case`, `core_test.cljs/test-case-keyword`,
`some->test`, `some->>test`, `cond->test`, `cond->>test`, `as->test` and
`test-const-emission` bodies were read, including `constantly-nil` and
`cljs-2104` setup. The complete `primitives_test.cljs/test-misc` body retains its
atom-based dotimes assertion. `analyzer_tests.clj`'s case AST assertions establish
the grouped primitive shape; exact private AST fields are not runtime claims.
`src/main/clojure/cljs/core.cljc` control/iteration/assert macros and
`analyzer.cljc/parse 'case*` establish expansion and validation semantics.
All anchors use the pinned checkout above.

Core macros lower to lexical bindings, branches and existing recurrence targets.
`if-some`/`when-some` distinguish false from nil/undefined; `if-let`/`when-let`
use CLJS truthiness, including true-like zero and empty strings. Short-circuit
and threading operations preserve selected argument effects and exceptions.
`condp` captures predicate and expression once and supports ordinary clauses,
`:>>` result functions, defaults and catchable no-match failures.

Case selectors run once. Scalar and keyword cases use switch; general constant
values use language equality, including cross list/vector equality and unordered
map/set equality. Reader duplicate detection precedes JavaScript numeric lowering.
Grouped source branch macros expand once. Symbols are literal tests except for
upstream's all-scalar/const optimization. Public `case*` validates symbol selectors,
grouped numeric/string/const tests and branch shape; its absent default is nil.
Keyword-only no-match messages preserve upstream's transformed-selector spelling.

`while` and `dotimes` return nil in discard, value and return contexts. Each loop
iteration creates fresh lexical captures. Dotimes evaluates its bound once and
compares without native SCI/JVM long truncation; a bound of 2.5 runs indices
0, 1 and 2. Body recur is not tail because the macro supplies its own advance.
The compiler macro-call flag is scoped per invocation; source macro execution
retains the native interpreter's independent iteration contract.

`MacroHost::set_assertions(false)` omits conditions and messages entirely,
including function pre/post checks, and updates the macro-side dynamic `*assert*`
Var, including macro-side assert and explicit macroexpand under dynamic binding.
New hosts and convenience compilation enable assertions. Runtime source
cannot change this compile-time setting. Alias and renamed core macro calls use
the same resolved behavior; generated forms retain source diagnostics.

## Evidence and explicit limits

**30 original assertion sites** pass the strict selected report on emitted JS
and both references. The fixture sidecar and witness manifest retain assertion
hashes, exact source anchors and adaptation rationales. Test-local setup is
preserved, and assertions execute rather than becoming expected-value constants.

Two additional faithful witnesses remain **missing**, and are excluded explicitly
from that focused selection: `cljs-control-dotimes` still uses the original atom
and `swap!`; `cljs-control-as-thread-3` still calls original `reverse`. Those
mutable-state and sequence helpers belong to separate feature work. They are not
replaced by host operations to claim imported coverage. Independent tests exercise
actual emitted dotimes/while with host arrays, plus as-> with implemented callees.
The full eleven-file coverage report remains the integration gate. Lazy for and
doseq remain in the sequence feature.

**38 independent probes** compare literal expected results, effect traces,
exception observations and stdout against both references. Cases include grouped
constants, hash-colliding strings, numeric reader identities, composite equality,
const globals, keyword/string distinction, nil-aware scopes, condp arrows,
short-circuit failures, fractional bounds, captured closures, nested iteration,
throw order and all emission contexts. Direct Node assertions separately inspect
retained closures without the shared encoder. Namespace tests execute source
macros, core aliases/renames, grouped compile-time effects and assertion settings.
Twelve new negative rows bring the boundary corpus to **174** classifications,
all matching both references (including its previously documented unrelated
release/checkout difference). Negative diagnostics are not passing semantic sites.

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
cargo test --locked --test execution \
  compiled_control_matches_pinned_values_effects_and_stdout -- --ignored --nocapture

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution \
  analyzer_boundary_classifications_match_reference -- --ignored --nocapture

python3 - <<'PY'
import json, subprocess
command = ['python3', '-B', 'tests/reference/compiler-language/report.py', '--scope', 'historical-selected',
           '--run', '--strict', '--reference', 'both']
for row in map(json.loads, open('tests/reference/compiler-language/witnesses.jsonl')):
    if (row['fixture_id'].startswith('cljs-control-') and
        row['fixture_id'] not in ('cljs-control-dotimes', 'cljs-control-as-thread-3')):
        command += ['--id', row['assertion_id']]
raise SystemExit(subprocess.call(command))
PY
```

The full ordinary Rust invocation passed **631 tests**, with **24** explicitly
ignored reference tests. After the final emitter and assertion refinements,
focused compiler execution/macro suites and the native macro/control suites passed
again. The final assertion refinement passed **60** focused tests. Node runtime
assertions, all-target Clippy, final library Clippy, formatting, source inventory
verification and whitespace checks passed. The strict matrix also observed **21**
passing native assertions; the nine JS-error-dependent case assertions have no
native execution claim. Both separately missing helper witnesses were run through
both references and passed there, while generated-JS rejection remained visible.

## Output and initialization observation

The shared JavaScript runtime remains **61,844 bytes**: this member adds no runtime
payload. `42` remains **104 bytes**. `(while false)` emits **249 bytes** (158 gzip),
and `(case 1 1 2 3)` emits **251 bytes** (158 gzip), both without the runtime.
A numeric/string case with a default now omits the previously unconditional
runtime inclusion. `(dotimes [i 0])` emits **62,331 bytes** (14,840 gzip), using the
existing arithmetic runtime; `(case :a :a 1 2)` emits **62,191 bytes** (14,800 gzip),
using the existing keyword runtime. Sizes are unminified output and deterministic
gzip measurements from the current compiler.

On this Mac with Node v24.18.0, precompiled `vm.Script` evaluation medians across
seven batches of 10,000 after 1,000 warmups were **0.39 microseconds** for `42`,
**0.42 microseconds** for the empty while and **0.34 microseconds** for the scalar
case. Observed ranges overlap (approximately 0.32–0.55 microseconds). These are
local current initialization observations, not a browser benchmark or speedup
claim.
