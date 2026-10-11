# Compiled multiple and variadic function arities

This extends the historical single-fixed-method audit in
[function-evidence.md](function-evidence.md). Source identity is ClojureScript
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`; both the clean checkout and Maven
**1.12.134** were executed with `:optimizations :none`, `:static-fns false` and
`:def-emits-var false`. Every reference invocation verifies identity and settings
before/after its build. Checkout reports **0.0.135109660**. Local tools were Node
**v24.18.0**, Clojure **1.12.4**, Java **25.0.2**, and Rust **1.99.0**.

## Behavior

`fn*`, ordinary `fn` and `defn` now lower multiple method bodies with separate
parameter scopes and one shared optional self-name. Fixed methods dispatch by
exact argument count. An unmatched call to a fixed overload set throws
`Error("Invalid arity: N")`. A variadic method is the default branch, including
underfilled calls; missing fixed values are `undefined`, and zero rest is nil.
One fixed method remains a plain JavaScript function with its historical
missing/extra-argument behavior. Repeated parameter symbols resolve to the last
binding; distinct emitted names retain lexical identity.

Direct rest values are private immutable indexed sequences, including rest
lengths above the vector chunk threshold. `apply` uses the compiled function's
bounded arity and sequence entry, avoiding argument-stack expansion for a large
rest and preserving a supplied sequence tail. Fixed overloads expose their
`cljs$core$IFn$_invoke$arity$N` methods; variadics also expose the variadic
entry, maximum fixed arity and `cljs$lang$applyTo`. Metadata wrappers retain
those entries at this milestone. The subsequent [apply audit](compiler-apply.md)
replaces metadata wrappers with the upstream callable-object receiver model.
No native interpreter value enters JavaScript.

The pinned `apply` implementation has a specific limitation: its bounded fixed
path rejects more than 20 arguments even when a variadic function's fixed prefix
is larger. Its sequence path, and ordinary direct calls, support larger prefixes.
`overload-large-fixed-apply-limit` retains that observed error rather than
silently claiming all large calls behave alike.

Named self-calls retain the function across global replacement; anonymous
functions resolve globals when their bodies run. The selected callee is read
before argument effects, including an argument replacing that global. Returned
closures, parameter/self-name shadowing, independent captures, native macros,
namespace passing and literal/explicit metadata are exercised.

The native macro bridge also accepts already-realized lazy method lists produced
by syntax quote. The functions namespace fixture executes this real expansion
through the native host and both reference compilers. Rest bindings are recorded
as arguments in the macro environment, with the correct argument index.

## Source assertions and independent probes

Complete relevant upstream bodies were read: `analyzer_tests.clj/analyze-ops`,
`test-cljs-2005`, `test-cljs-2863`; `compiler_tests.clj/test-optimized-invoke-emit`;
`core_test.cljs/test-383`, `test-477`, `test-cljs-2741`; complete `invoke_test.cljs`
and `apply_test.cljs`; and `primitives_test.cljs/test-apply`.
Implementation cross-checks include `analyzer.cljc/analyze-fn-method`,
`parse 'fn*`, `compiler.cljc/emit-fn-method`, `emit-variadic-fn-method`,
`emit-apply-to`, `emit* :fn`, and the core `apply`/`gen-apply-to` helpers.

The executable inventory adds **38** witnesses to **11** existing arity
witnesses: **49 selected assertions covered, 0 missing, 0 version differences**.
[Witness mappings](compiler-language/witnesses.jsonl) retain each assertion hash,
source identity, fixture ID and adaptation rationale. Finite `test-apply`
assertions retain all five surrounding function bindings. Error assertions keep
the actual source call and use a host assertion adapter to check `Error` and,
for `test-cljs-2741`, the exact original message regex. Those adapters do not
claim compiled `try` support. Full original test bodies remain in the inventory.
Infinite-source library and protocol assertions remain separately visible;
this result is not a whole functions/apply section completion claim.

**51 independent overload cases**, plus **10 existing single-fixed arity cases**,
match complete outcomes, traces and stdout against both references. They cover
fixed/rest prefix lengths 0, 1, 2, 5, 20 and 26, direct/indirect/apply calls,
method ordering, zero/40-element rests, undefined elements, rest predicates,
function `.length`, metadata, recursion through self, redefinition and effects.
The separate namespace program prints `10 13 10 13 true 100 11` identically in
all three implementations. Direct Node assertions independently check callable
properties, exact error messages, immutable non-array rests and a 150,000-value
rest through `apply` and metadata.

The declaration matrix retains genuine malformed vectors/bindings and adds
three invalid overload combinations. Duplicate fixed overloads and a fixed
method wider than the variadic prefix warn upstream; local analysis rejects
them with located declaration errors. Two variadics with different prefixes
warn and produce invalid JavaScript upstream (two `default` branches); local
analysis rejects before emission. These stage differences are explicit in
`analysis-boundaries.tsv`, not counted as semantic passes. All **143** recorded
reference classifications passed against both baselines. Valid multiple,
variadic and repeated-parameter declarations now have accepted boundaries.

## Verification

Focused analyzer, boundary, CLI, execution, compiler macro, namespace, provenance,
compiler-language accounting, native binding and native macro suites passed.
The direct runtime assertions are also run by the execution suite. Clippy and
formatting checks passed; the pinned inventory regeneration check passed.

```sh
cargo test --locked --test analyzer --test boundaries --test cli \
  --test execution --test compiler_macros --test namespaces \
  --test provenance --test compiler_language \
  --test interpreter_bindings --test interpreter_macros
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check

export LIGHT_CLJS_REFERENCE=both
export LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript"
export LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e
cargo test --locked --test execution \
  compiled_overloads_match_pinned_values_effects_and_stdout -- --ignored --nocapture
cargo test --locked --test execution \
  analyzer_boundary_classifications_match_reference -- --ignored --nocapture

python3 - <<'PY'
import json, subprocess
witnesses = [json.loads(line) for line in
             open('tests/reference/compiler-language/witnesses.jsonl')]
ids = [w['assertion_id'] for w in witnesses if w['fixture_id'].startswith(
    ('cljs-functions-', 'cljs-language-multi-variadic', 'cljs-language-apply-'))]
subprocess.run(['python3', '-B', 'tests/reference/compiler-language/report.py', '--scope', 'historical-selected',
                '--run', '--strict', '--reference', 'both',
                '--output', '/tmp/compiler-functions.json'] +
               [arg for identity in ids for arg in ('--id', identity)], check=True)
PY
```

## Output cost

The primitive expression `1` remains **103 bytes** and `(fn* [x] x)` remains
**137 bytes**, without runtime inclusion. A two-fixed-method function is
**574 bytes**, also without a runtime. A one-fixed-plus-rest function is
**43,506 bytes** including the shared runtime (10,460 gzip bytes).

The runtime changes add **1,546 bytes** (41,448 to 42,994), or **524 gzip bytes**
(9,795 to 10,319). This comparison restores only the pre-change `apply`,
metadata function wrapper and runtime export list; it preserves all earlier
reader/numeric work. Gzip uses Python `gzip.compress(..., mtime=0)`.
A local Node runtime-initialization check compiled each factory once, warmed it
500 times and measured nine batches of 1,000 creations: median **11.00 µs**
before and **10.82 µs** after. This small difference is noise, not a speedup
claim; it excludes JavaScript parse/compile time and is not a browser benchmark.
