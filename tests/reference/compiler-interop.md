# JavaScript source-language interop

The contract is ClojureScript checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, compared separately with Maven
**1.12.134**. Every reference build verifies `:optimizations :none`,
`:static-fns false`, `:def-emits-var false`, the loaded compiler source and
revision. Node **v24.18.0** executes generated output; production output uses
ordinary browser JavaScript and no Node API or external `cljs.core`.

## Source assertions and adaptations

Complete relevant bodies were read before implementation. The source inventory
retains the full bodies and original assertion hashes for its selected files;
`fixture-origins.tsv` and `compiler-language/witnesses.jsonl` supply local IDs.

| Pinned source and test anchor | Executed adaptation |
| --- | --- |
| `src/test/cljs/cljs/primitives_test.cljs::test-dot` | Complete body, all 16 assertions. Explicit/shorthand properties and methods, nesting, and `memfn` execute unchanged. |
| `primitives_test.cljs::test-arrays` | Complete body, including `amap`, `areduce`, cloning, multidimensional access, saved helpers and `apply`. |
| `primitives_test.cljs::test-make-array` | All 40 dimensional assertions and inputs. For these nested arrays containing nil/numbers, the helper compares JSON serialization instead of calling the separately scoped `js->clj` library conversion. Holes and undefined retain the original nil observations. |
| `core_test.cljs::{aget-test,aset-test,unchecked-get-test,memfn-test}` | Four complete bodies; no direct-call substitution for `apply`. |
| `core_test.cljs::test-instance-method-new` | Entire assertion and the namespace `:refer-global [String Object]` context; namespace witness `cljs-interop-qualified-methods`. |
| `core_test.cljs::test-new-method`; `qualified_method_test.cljs::qualified-method-return-position-test` | Constructor-value and returned-method checks execute in that namespace witness. The constructor test has no assertion site and does not increase matrix coverage. |
| `array_access_test.cljs::{aget-test,aset-test,unchecked-aget-test,unchecked-set-test,checked-aget-test,checked-aset-test,checked-aget'-test,checked-aset'-test}` | Eight complete bodies, retaining all alias helpers and host inputs. `thrown?` becomes typed try/catch; `is` becomes throwing `assert`. Warning suppression wrappers are removed; warning output is not a tested contract. IDs start `interop-upstream-array-`. |
| `specials_test.cljs::test-cljs-3125` | The first two property-assignment assertions motivate independent `interop-property-set`; no matrix assertion claim. Global/mutable-field protocol cases remain with definitions/types coverage. |
| `new_new_test.cljs`; `interop_test.cljs` | Complete files read for constructor/receiver interactions. Their record/protocol and ES6 collection-interface contracts do not become passes merely from supporting JavaScript call syntax. |
| `src/test/clojure/cljs/compiler_tests.clj::{test-optimized-invoke-emit,test-goog-ctor-import-gen}` | Full relevant emission assertions inspected. Exact compiler text and Closure imports are upstream implementation/build checks, not claimed runtime assertions. |

The selected strict matrix comprises **86 assertion sites** in eight witnesses.
Independent `interop-*` probes separately observe constructor/property effects,
getters/setters, computed indices, missing/null receivers, method replacement,
hyphenated/Unicode names, raw templates, actual function arguments and receivers,
metadata wrappers, read-only host objects, type fields and record metadata copies.
Direct Node assertions inspect arrays, holes, values and exceptions without the
shared outcome encoder.

## Implementation choices

Resolved expression nodes retain source spans. Dot/new/template forms are fully
analyzed before JavaScript emission; invalid shape, member, placeholder or arity
cannot return partial output. JavaScript fragments themselves are an explicit
source escape hatch and are not parsed by the Rust analyzer. Template operands
remain inline, so short-circuiting and getter/argument sequencing follow the
literal JavaScript. Generated bindings are renamed around literal segments to
prevent accidental capture, including names in nested raw functions.

The output scope no longer forces strict mode. This preserves actual `this` and
parameter/`arguments` aliasing observed by raw JavaScript and CLJS helpers.
Public language-value objects accept host properties; immutable collection
storage and inspection snapshots remain private/frozen. JavaScript writes cannot
replace vector entries or forge a language-value brand. Type fields are visible
under their munged properties and record lookup follows writes. Metadata copies
own fresh field accessors so writing a copy cannot mutate the original.

The pinned compiler's constructor expression emission has an observable
precedence quirk: `(new (do ... ctor) arg)` constructs its generated function
wrapper, then calls the returned constructor as a function. Both references
return `false` in `interop-new-expression`, with the same constructor/argument
trace. The emitter preserves that behavior; simple and nested property-based
constructors have separate checks. Host lookup is an implementation detail and
does not introduce an extra source constructor call.

Qualified method probes use a local named `Text` for `js/String`, or retain the
original `:refer-global` namespace. Binding a local called `String` to `js/String`
produces upstream `var String = String` hoisting and is not a valid substitute
for that namespace context.

## Verification

The reproducible focused comparison is:

```sh
LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT=research/clojurescript \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --test execution compiled_interop_match_pinned_values_effects_and_stdout -- --ignored --nocapture
```

For the matrix, select the witness-mapped IDs whose fixture ID starts
`cljs-interop-` and pass each as `--id` to:

```sh
python3 -B tests/reference/compiler-language/report.py --scope historical-selected --run --reference both --strict
```

This selection reports **86 covered, zero missing and zero version differences**.
That is a focused feature report, not whole-inventory completion. Other retained
assertion sites remain visible in the live report. The source inventory check
verifies all pinned source bytes and hashes without claiming execution.

The focused oracle comparison matches **64 outcomes, effects and stdout** against
both references. The boundary comparison checks **181 classifications** against
both, retaining the documented release/checkout difference for
`let-binding-amp`. The full Rust suite passes **640 tests** with 30 ignored
reference/diagnostic tests; the focused interop, language and provenance suites,
direct JavaScript runtime assertions and warning-free library Clippy check pass.

Fast and diagnostic checks:

```sh
cargo test -j 4 --quiet
cargo test --test compiler_interop --test compiler_language --test provenance
node --test tests/support/js_runtime.test.js
cargo clippy -j 4 --lib -- -D warnings
python3 -B tests/reference/compiler-language/extract_inventory.py --check
```

The same dual-reference environment with
`analyzer_boundary_classifications_match_reference -- --ignored --nocapture`
checks the retained source diagnostics, including formerly unsupported JS forms.

## Output and startup measurements

`python3 -B benchmarks/measure_compiler_interop.py --output
benchmarks/results/compiler-2026-10-10/interop` records exact emitted snapshots,
compiler/runtime hashes and nine fresh V8 parse/evaluation samples per program.
The saved [JSON report](../../benchmarks/results/compiler-2026-10-10/interop/output-startup.json)
contains all samples and validation expectations.

The runtime is **84,375 bytes / 19,769 gzip bytes**, compared with the preceding
definitions report's 80,031 / 18,497 bytes (**+4,344 raw, +1,272 gzip**).
Primitive `42` output shrinks from 104 to **88 bytes** because it no longer forces
strict mode. Dot, constructor and raw-template samples remain runtime-free at
110, 218 and 102 bytes respectively. The collection sample is **84,532 bytes**.
Its median V8 compilation plus execution is **1.524 ms**, compared with the
historical definitions sample's 1.521 ms; these are not paired measurements or
confidence bounds. They exclude process startup and source reading. OS caches
and scheduling are uncontrolled, and browser startup is not inferred.
