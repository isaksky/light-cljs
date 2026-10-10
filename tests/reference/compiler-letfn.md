# Compiler letfn evidence

## Contract and upstream evidence

The references are Maven ClojureScript **1.12.134** and checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e** (reported runtime
`0.0.135109660`). Both runs checked their identities before and after compilation
and effective `:optimizations :none`, `:static-fns false`, and
`:def-emits-var false`, with Clojure 1.12.4, Java 25.0.2 and Node v24.18.0.

The complete `src/test/cljs/cljs/letfn_test.cljs::test-letfn` and
`core_test.cljs::test-letfn-shadowing` bodies were read and retained in the
source inventory. All **nine** assertions now have executable witnesses:
`cljs-language-letfn-mutual`, `cljs-letfn-mutual-1`,
`cljs-letfn-mutual-3` through `cljs-letfn-mutual-8`, and
`cljs-language-letfn-shadowing`. Each keeps both function declarations and its
original asserted expression. Earlier assertions in the parity body are
side-effect-free. Assertion hashes and individual mappings are in
`compiler-language/witnesses.jsonl`; source commit/path/anchors and adaptation
rationales are in `fixture-origins.tsv`.

The letfn section of `src/test/clojure/cljs/analyzer_tests.clj::analyze-ops`
(lines 908–957) establishes binding groups, function initializers and local
references. The local analyzer test observes stable IDs, forward references,
restored outer scopes and exact source spans instead of upstream AST field names.
The implementation was also checked against `analyzer.cljc::parse 'letfn*`,
`core.cljc::letfn`, and the complete `core_test.cljs::test-541` body. The latter
also requires printing capture and Symbol instance predicates; its original
assertion remains separately missing. Our independent effect tests do not claim
that assertion.

`letfn*` gets its own resolved expression. All group names are allocated before
any initializer is analyzed. The emitter declares fresh lexical slots, initializes
functions, then executes the body. Every function uses the existing named,
overloaded, variadic, apply and recur machinery. Duplicate names retain the last
initializer, matching the reference's name-to-initializer map. The macro bridge
also accepts lazy sequence declarations returned by syntax quote.

Function metadata expressions are evaluated during initialization; calls to an
already initialized peer work, and calls to an uninitialized peer throw before
later initialization/body effects. Non-function initializers and malformed groups
are rejected with source locations. Rejection of arbitrary `letfn*` initializer
expressions is the explicit local validation contract, not a claim about every
malformed form accepted by the upstream internal primitive.

## Executed behavior and explicit differences

**28 independent `letfn-*` probes** match both references exactly for values,
ordered traces, exception categories/values and stdout. They cover three-function
cycles; core/global/outer shadows; independent escaping groups; names and parameter
shadows; fixed/multiple/variadic methods and apply; 100,000-step function recur;
variadic recur; body-tail recur; empty and duplicate groups; destructured
parameters; qualified macros; function metadata and initialization/call failures;
and value/discard/return contexts. Rust macro tests execute a syntax-quoted group
and inspect a forward peer's `:letfn` macro environment binding. Direct Node tests
inspect function identity, retained group values and iteration captures without
the shared encoder.

Two independent probes are intentionally outside the exact-conformance fixtures.
Both pins emit function-scoped `var f`/`var g` slots which are reused:

- A same-spelled nested group retargets the outer function's `g` reference. The
  reference observes `[9 9]` where independent lexical groups observe `[7 9]`.
- Groups created in successive loop iterations retain each iteration's `n` but
  share the last `g` slot. The reference observes `[3 3 3 3]`; independent groups
  observe `[0 1 2 3]`.

`pinned_letfn_var_reuse_differences_remain_explicit` checks the exact reference
and local result arrays independently. Neither is counted as a matching
conformance pass or as an imported upstream assertion. Preserving separate
`LocalId` slots is required by the local lexical-identity contract; emitted names
are never derived from source spelling to reproduce these upstream collisions.

## Verification

The focused Rust suites passed **168 tests**, with **16** explicitly ignored
reference tests in the ordinary invocation. The two selected letfn reference
tests passed separately: **28 exact matches** and **2 verified differences** on
each pin. The strict selected assertion report has **9 covered, 0 missing,
0 version differences**. The source inventory itself is unchanged.

```sh
cargo test --locked --test analyzer --test boundaries --test cli \
  --test compiler_macros --test compiler_reader --test execution \
  --test namespaces --test provenance --test compiler_language \
  --test interpreter_bindings --test interpreter_functions
node tests/support/js_runtime.test.js
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution letfn -- --ignored --nocapture

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution \
  analyzer_boundary_classifications_match_reference -- --ignored --nocapture

python3 - <<'PY'
import subprocess
ids = [f'letfn_test.cljs::test-letfn::assertion-{i:03}' for i in range(1, 9)]
ids += ['core_test.cljs::test-letfn-shadowing::assertion-001']
command = ['python3', '-B', 'tests/reference/compiler-language/report.py',
           '--run', '--strict', '--reference', 'both']
for identity in ids:
    command += ['--id', identity]
raise SystemExit(subprocess.call(command))
PY
```

All **153** analyzer boundary classifications matched both references. Standalone
Node runtime assertions, all-target Clippy, formatting, byte-identical inventory
regeneration and whitespace checks also passed.

## Output and initialization observation

The constant `9` emits **103 bytes**. A sequential group
`(let* [g (fn* [] 9) f (fn* [] (g))] (f))` emits **247 bytes**, and the forward
reference `(letfn* [f (fn* [] (g)) g (fn* [] 9)] (f))` emits **259 bytes**.
That is **12 additional bytes**, or **5 gzip bytes** (152 versus 157); neither
program includes the runtime prelude. No general runtime code was added.

On this Mac with Node v24.18.0, precompiled `vm.Script` evaluation medians were
approximately **0.68 microseconds** for the sequential group and **0.65
microseconds** for letfn (seven batches of 10,000 evaluations after 1,000 warmups).
These small local timings show no material initialization regression; they are
not browser performance claims. Measurements compile the exact source strings
above with `target/debug/light-cljs --compile --text`, count UTF-8 output bytes,
and evaluate each result through `vm.Script.runInThisContext()`.
