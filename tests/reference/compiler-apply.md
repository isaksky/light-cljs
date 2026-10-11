# Compiled apply and higher-order invocation

The source contract is ClojureScript commit
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`. The executable witnesses compare
Maven **1.12.134**, the clean checkout (runtime **0.0.135109660**), and generated
JavaScript. Every reference build verifies identity and `:optimizations :none`,
`:static-fns false`, and `:def-emits-var false`. Tools were Node **v24.18.0**,
Clojure **1.12.4**, Java **25.0.2**, and Rust **1.99.0**.

## Invocation contract

`apply` implements every prefix arity and uses bounded argument counting for
compiled variadic functions. Prefix construction never counts an unbounded
sequence. Variadic entries receive the sequence tail; plain JavaScript functions
receive themselves as their `this` receiver. Fixed overloads and callable data
retain their own arity rules. The pinned bounded fixed entry still rejects
counts above 20; the sequence entry supports larger fixed prefixes.

Callable vectors, maps, sets, keywords, symbols and Vars expose `.apply` as well
as `.call`. `reify IFn` supplies custom fixed invocation methods, with lexical captures. Methods receive the object as
self. `.apply` bundles arguments above twenty into the last IFn method's array
tail; explicit `.call` retains its explicit last argument. One fixed method
retains JavaScript's permissive missing/extra-argument behavior. This invocation
support does not claim general protocol declarations or user type construction.

Metadata functions are callable objects, as in upstream `MetaFn`. Their fixed
entries invoke the wrapped function normally; the final entry calls `apply`.
They retain metadata and `fn?`/`ifn?`, without copying a compiled function's
`cljs$lang$applyTo`. This corrects the earlier arity milestone's receiver model.

`identity`, `constantly`, `complement`, `comp`, `partial`, `juxt` and `fnil`
implement their public constructor and returned-function arities. Their
variadic delegates retain lazy tails. Supporting helpers include `list*`,
`cons`, `iterate`, `take`, `range`, `array`, `array-seq`, `to-array`, and
`apply-to`. Numeric folds, comparisons, equality and collection builders have
sequence entries; a 150,000-argument sum/vector avoids the JS argument stack.
Comparisons and equality can stop on an infinite argument tail.

`this-as`, `js-arguments` and `copy-arguments` preserve receiver/argument
observations. Return/discard contexts retain the enclosing invocation; expression
scopes use ordinary function IIFEs as upstream does. In particular a nested
value-context `this-as` sees that scope's global receiver. `iterate` separately
caches its current seed and next node: bounded counting must not run the next
step one value early. Effects, failures and stdout are part of the comparisons.

## Assertion accounting

The complete relevant upstream test bodies and implementation helpers were read.
**94 additional assertion mappings** bring the selected set to **143**:
all active `apply_test.cljs` assertions; complete
`primitives_test.cljs/test-apply` and `test-partial-and-comp`; and the complete
core regressions `test-fn-with-metadata`, `test-383`, `test-477`, `test-580`,
`test-660`, `test-cljs-1225`, `test-cljs-2741`, `test-cljs-2832`, and
`test-cljs-3386`. Assertion hashes, source anchors, context and adaptation
rationales remain in the executable inventory and provenance sidecar.

The `doseq` assertions execute every original function binding. Several upstream
metadata assertions accidentally put their `apply` call in the message slot of
`is`; their witnesses strengthen the original one-argument equality into a real
comparison with that call, and record this explicitly. Error assertions use a
host assertion adapter, including exact arity-message regexes where upstream
requires them. These adapters do not claim compiled `try` support.

**228 independent probes** cover all callable-data paths, IFn arities 0–21,
receiver identities and boxing, escaped and metadata functions, combinator
arities, nil/empty tails, large argument lists, infinite tails, effect order,
failure timing and stdout. Direct Node assertions independently check receiver
identity, actual object/function ABI, exact arity errors, and 150,000 arguments
without relying on the shared value encoder. These probes remain separate from
imported assertion counts. This is not a whole core-library coverage claim.

## Verification

The final strict run passed **143/143** generated-JavaScript, release and
checkout outcomes, with **0 missing** and **0 version differences**; lexer and
parser accepted all 143. All **228** independent outcomes, traces and stdout
matched both references. The focused Rust suites passed **134 tests** (13
explicit reference tests ignored in the ordinary invocation); the dedicated
invocation differential test passed separately. Clippy, formatting, provenance,
inventory regeneration and whitespace checks passed.

```sh
cargo test --locked --test execution --test analyzer --test boundaries \
  --test cli --test compiler_macros --test namespaces --test provenance \
  --test compiler_language
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution \
  compiled_invocation_matches_pinned_values_effects_and_stdout -- --ignored --nocapture

python3 - <<'PY'
import json, subprocess
core = {'test-fn-with-metadata', 'test-383', 'test-477', 'test-580',
        'test-660', 'test-cljs-1225', 'test-cljs-2741', 'test-cljs-2832',
        'test-cljs-3386'}
rows = map(json.loads, open('tests/reference/compiler-language/inventory.jsonl'))
ids = [r['id'] for r in rows if r['disposition'] == 'in-scope' and (
    r['source'].endswith('/apply_test.cljs') or
    (r['source'].endswith('/primitives_test.cljs') and
     r['test'] in {'test-apply', 'test-partial-and-comp'}) or
    (r['source'].endswith('/core_test.cljs') and r['test'] in core))]
subprocess.run(['python3', '-B', 'tests/reference/compiler-language/report.py',
                '--run', '--strict', '--reference', 'both',
                '--output', '/tmp/compiler-apply.json'] +
               [arg for identity in ids for arg in ('--id', identity)], check=True)
PY
```

Run reference reporting after compilation finishes; replacing the executable
while the report invokes it can make an individual parser observation unavailable.
A successful semantic comparison does not excuse that missing observation.

## Output cost

Primitive `1` remains **103 bytes**, and `(fn* [x] x)` remains **137 bytes**;
neither includes the runtime. The shared runtime grows from **42,994** to
**52,799 bytes** (+9,805, 22.8%); deterministic gzip grows from **10,319** to
**12,502 bytes** (+2,183, 21.2%). This compares the exact pre-change runtime
snapshot with the completed runtime; no earlier changes are subtracted.

Node factory initialization, compiled once with `new Function`, warmed 500
iterations and measured in nine alternating batches of 1,000 creations, had
medians **12.12 µs before** and **15.75 µs after**. This is a small local
initialization measurement with visible run-to-run noise; it excludes JS parse
and compilation and is not a browser benchmark. The additional callable
entries and supporting helpers have a measurable initialization cost.

## Deferred constructor migration verification (2026-10-10)

`partial` and `juxt` constructors now expose the same canonical fixed/variadic
sequence ABI as their returned functions. Applying either constructor to an
infinite lazy tail realizes only the bounded dispatch prefix (the witnesses
observe positions 0 through 4), then retains the remaining tail. Calling a
finite captured partial realizes its remaining values once; repeated calls
reuse the cached sequence. Juxt callbacks still execute in left-to-right order
on every invocation. This replaces retained native finite-demand behavior; it
is separate from the remaining intended sequence inventory.

`compiler_core_helpers::applied_partial_and_juxt_constructors_preserve_lazy_tails_in_both_phases`
executes exact effects and values in independent macro/application heaps,
QuickJS and Node, and two clean compilations with a reused MacroHost. Direct
`js_core_helpers.test.js` assertions exercise applied constructor prefixes and
returned function arities, finite capture reuse, unbounded construction, and
missing-argument behavior. Both Node scripts and all three compiler-core-helper
tests passed locally with native evaluation disabled at the default Rust stack.
The final full gate results are recorded in
[evaluator migration verification](evaluator-migration/verification.md).
