# Compiled numeric operations and fundamental predicates

## Contract and source inventory

The JavaScript core now resolves 129 additional numeric/coercion/predicate names
as first-class functions. Resolution uses `CoreFnId`; locals, namespace globals,
refers, aliases and saved values share the existing callable ABI. Runtime
arithmetic uses JavaScript Numbers and operators, never native i64 or ratios.

The implementation inventory was read from ClojureScript commit
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`:

| Core section | Implemented API family |
| --- | --- |
| `core.cljs` 242–318, 1106, 1603–1609, 2118, 2221–2459 | nil/some/undefined, truth/boolean, identity, number/string/character, function/callability and collection predicates |
| `core.cljs` 2735–3100, 4455–4463 | `+ - * / inc dec < <= > >= == max min abs`, integer and floating coercions, `char`, unchecked arithmetic/coercions, identity array coercions, `js-mod mod quot rem`, every public bit operation, sign/parity/integer/float/double/infinity/NaN predicates |
| `core.cljs` 3290, 3367, 3513–3569, 4198, 6848 | list/reversible, keyword/symbol/ident, simple/qualified and named identity predicates, `not=`, map entries |
| `core.cljc` 1086–1439 | Direct numeric/predicate macro arity contracts and JavaScript operator behavior |

`predicates_test.cljs` was read in full, including the complete fixture tables.
All 15 ordinary value rows and eight JavaScript numeric rows execute every
predicate column. The six `goog.math.Integer`/`goog.math.Long` rows construct
external Closure library types; those constructors and libraries remain
unresolved and are not counted as JavaScript Number support. Random generation,
parsing namespaces, sorting helpers, arbitrary protocol implementations, and
external libraries are separate surfaces, not newly claimed APIs.

The complete `primitives_test.cljs/test-arithmetic`, `test-bit-operations`,
`test-booleans`, `test-char?`, `test-3352`, and relevant
`core_test.cljs/test-730`, `test-cljs-3425` bodies were read before implementation.
The language matrix maps **220** executable assertions from these bodies with
exact original hashes and adaptation explanations in `witnesses.jsonl` and
`fixture-origins.tsv`. Of these, 217 are new fixtures. Five original arithmetic
assertions also require `map`, `loop` or `for`; their complete source remains
missing in the inventory until those compositions execute. No assertion has
been weakened or reclassified to conceal them. In particular, the upstream
first bit-xor assertion has a one-argument equality and an optional message;
that predicate is retained exactly, while independent tests actually execute
variadic bit-xor.

## Numeric and predicate behavior

* `+` and `*` have zero-argument identities and preserve their unary argument;
  subtraction negates and division takes the reciprocal at unary arity.
  Variadic arithmetic folds left. Comparisons accept unary and variadic calls.
* `int` uses ToInt32; `long` and `unchecked-int` truncate toward zero without
  wrapping. `byte`, `short`, `float`, `double` and the typed-array coercions
  preserve their argument, as upstream does. Unchecked arithmetic does not
  introduce JVM overflow. `char` uses UTF-16 code units.
* `quot`, `rem` and `mod` preserve their separate upstream formulas, including
  signed zero and infinity behavior. Bit operations use JavaScript's signed
  32-bit operands and five-bit shift distances; unsigned right shift returns
  the unsigned Number. `bit-count` covers all 32 bits.
* `integer?` intentionally uses upstream's parseFloat/parseInt rule, so `1e21`
  is false despite `Number.isInteger(1e21)`. `NaN?` uses coercive `isNaN`;
  `number?` excludes boxed Numbers. Parity rejects non-integers with `Error`.
* Only nil/undefined/false are logically false. Character predicates count
  UTF-16 units. Named and callable collection predicates use private brands;
  a host object with a `call` member cannot impersonate an IFn value.
* Collection predicates retain list/Cons, indexed/chunked sequence, map entry,
  hash/array map sequence, set, queue and lazy sequence distinctions. Hash map
  kind survives metadata and dissociation. Owned collection `toString` methods
  provide the same printed values when JavaScript numeric coercion reaches them.

Direct malformed numeric macro calls are analysis errors, with exact source
spans; saved runtime functions use upstream's ordinary JavaScript dispatcher.
For example, direct `(-)` is invalid, whereas `(let [f -] (f))` returns NaN.
Single-arity functions such as saved `inc` ignore extra arguments. Invalid input
can coerce, return NaN, or throw Error/TypeError; the runtime does not invent
universal numeric type or arity checks.

**Evaluation-order distinction:** light-cljs emits ordinary function calls for
resolved core values and evaluates each argument exactly once, in order, as
required for this member. Upstream direct comparison macros can skip later
arguments or duplicate a middle expression; their saved function values use the
eager ABI. The differential effect probe therefore binds `<` to a local and
retains all argument effects. A separate expected-output test explicitly checks
the light-cljs direct-call behavior, including unary comparison effects. This
is an explicit difference from upstream direct macro expansion, not a claim
that their effectful direct comparison macros are identical.

## Verification

```sh
cargo test --locked -j 4 --quiet
node --test tests/support/js_runtime.test.js
cargo clippy --locked -j 4 --lib -- -D warnings
python3 -B tests/reference/compiler-language/extract_inventory.py --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT=research/clojurescript \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution \
  numeric_functions_match_pinned_values_effects_and_stdout -- --ignored --nocapture

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT=research/clojurescript \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution \
  analyzer_boundary_classifications_match_reference -- --ignored --nocapture

LIGHT_CLJS_REFERENCE=release cargo test --locked --test execution \
  programs_match_standard_clojurescript_results_and_effects -- --ignored --nocapture
```

The full Rust suite passes **608 tests**, with **15 explicitly ignored** optional
reference/research tests. Node runtime assertions, Clippy, formatting, provenance
and checked source inventory also pass.

The **56** numeric/predicate typed outcome cases pass generated JavaScript and
both references, comparing values, ordered effects, failure categories and
stdout. They include host-supplied nil/non-finite/signed-zero values, shadowing,
aliases, saved functions, apply, all predicate table cells and explicit runtime
identity checks. Node assertions bypass the compiler and shared encoder.
All **140** analysis boundary classifications match both references, including
10 new malformed numeric/predicate calls; the existing release/checkout
amp-binding distinction stays explicit. The full **622**-outcome comparison
passes against release 1.12.134.

The full `LIGHT_CLJS_REFERENCE=both` run was also executed (619 cases at that
invocation, before the last three numeric probes were added). It correctly failed
its release-versus-checkout agreement gate on two **pre-existing** set fixtures:
`set-literal-effects` and `set-dynamic-collapse`. Release deduplicates their
runtime-equal members; this checkout throws Error after evaluating them.
All remaining shared outcomes agree, and the last three numeric probes pass
the separate both-reference numeric run. This failure is recorded separately; it
is not reported as a full checkout pass or a numeric failure. Both runs verify
revision/resource identity and effective `:optimizations :none`,
`:static-fns false`, `:def-emits-var false` settings before accepting evidence.

To reproduce the focused strict 220-assertion matrix run:

```sh
python3 - <<'PY'
import json, subprocess
from pathlib import Path
root = Path('tests/reference/compiler-language')
selected = {'test-arithmetic', 'test-bit-operations', 'test-booleans',
            'test-char?', 'test-3352', 'test-730', 'test-cljs-3425'}
witnesses = {json.loads(line)['assertion_id']
             for line in (root / 'witnesses.jsonl').read_text().splitlines()}
ids = [row['id'] for line in (root / 'inventory.jsonl').read_text().splitlines()
       if (row := json.loads(line))['test'] in selected and row['id'] in witnesses]
subprocess.run(['python3', '-B', str(root / 'report.py'), '--run', '--strict',
                '--reference', 'both',
                *[arg for identity in ids for arg in ('--id', identity)]], check=True)
PY
```

The strict report covers **220**, with zero missing or version differences in
that exact selection. It does not claim completion of all core-helpers or of the
five cross-feature arithmetic assertions retained above.

## Output and startup cost

On Node v24.18.0, the embedded runtime grows from **32,829** to **41,448** source
bytes; gzip grows from **7,584** to **9,832** bytes. Primitive-only `3` still emits
**103** CLI bytes and includes no runtime; the existing exact primitive-output
regression also passes. A precompiled `vm.Script` factory microbenchmark (100
warmups, median of seven batches of 1,000 fresh factories, GC between batches)
measured about **0.019 ms** before and **0.055 ms** after. This measures factory
initialization on this host, not browser parse time or an application benchmark.
The added startup allocation is one set of core functions per compilation unit,
not per arithmetic invocation.

## Compile-time and application Number semantics

`light-cljs-100` verifies the canonical JavaScript path introduced by the macro
bootstrap. All **57** numeric fixture bodies in `execution-outcomes.tsv` now run
through compiler-emitted macro functions in QuickJS and through application
units in independent QuickJS heaps and Node. The original 56 typed expectations
are retained; `numeric-number-keys` adds independent assertions for nested numeric
keys, signed zeros, NaN and infinities. The first-class inventory checks every
supported numeric/coercion/predicate binding, alongside saved functions,
aliases, `apply`, argument effects and failure categories. Test-only
`numeric_observation.js` encodes primitive/vector results so undefined and
non-finite intermediate values can be observed without allowing unsupported
macro expansion values. It does not supply any language operation.

Source-declared macros also return computed Number data through the form bridge,
then execute in separate application heaps. Explicit expected results include
`(/ 1 2) => 0.5`, `(= 1 1.0) => true`, and
`(inc 9007199254740992) => 9007199254740992`. Subtracting the original value
after that rounded addition returns zero during expansion, preventing an
implementation that postpones rounding until output conversion. Coercion probes
cover wrapping `int`, truncating `long`, identity `byte`/`short`, and `float`/`double`
without JVM float rounding. Count/range/index-generated numbers, saved functions
in collections, numeric printing followed by symbol construction, and eager
argument effects share the same operations. Division and reciprocal expose NaN,
infinities and negative zero directly. Macro failure tests retain call,
definition and helper provenance and verify a subsequent clean compilation.

Number-equal keys find the same entry, and equal nested values have equal hashes.
Positive/negative zero compare equal and hash equally. NaN remains non-reflexive:
an inserted NaN key cannot be looked up with NaN, and a hash-set containing two
NaNs has two members. Both saved-constructor key probes and the complete numeric
corpus match release **1.12.134** and checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`. Runtime hash integers are internal;
these checks require equality consistency, not upstream hash-number identity.

The only production adjustment needed was wide radix literal conversion:
nondecimal integers exceeding u128 previously accumulated f64 rounding per
digit in application analysis, while macro arguments retained exact reader text
until one Number conversion. Applications now use that same single-rounding
boundary. Independent decimal expected values exercise binary, octal,
hexadecimal and radix-36 literals, both signs, in QuickJS and Node. Integer `N`
and decimal `M` spellings execute as Numbers; integral ratio literals execute as
Numbers, while non-integral ratio source literals keep the existing reader
rejection. The reader's exact identities remain diagnostic/source data.

The former Rust macro arithmetic proposal is retired. No compiler-specific
native numeric profile was introduced or retained. The historical native
embedding/eval adapter still has its old semantics until `light-cljs-115` removes
that consumer; it is not evidence about compiler-emitted macro execution.
The deliberate direct-comparison effect difference and reader-distinct
map/set literal policies described above remain explicit.

Executed commands for this delivery:

```sh
cargo test --locked --test compiler_bootstrap --quiet
cargo test --locked --test compiler_macros --test evaluator --test compiler_reader --quiet
cargo test --locked --test compiler_macros reader_numeric_spellings --quiet
cargo test --locked --test execution numeric_ --quiet
cargo test --locked --test analyzer numeric_literals --quiet
node --test tests/support/js_runtime.test.js
rustfmt --edition 2024 --check src/analyzer.rs tests/compiler_macros.rs tests/evaluator.rs tests/execution.rs
git diff --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT=research/clojurescript \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution \
  numeric_functions_match_pinned_values_effects_and_stdout -- --ignored --nocapture
```

The bootstrap suite passes 11 tests; the initial focused macro/reader/evaluator
run passes 59/7/9 tests with one explicitly ignored evaluator timing probe. The
additional reader-spelling test passes separately, bringing macro tests to 60.
The numeric application selection passes three tests with the optional upstream
test ignored by its default selection; the explicit upstream invocation then
passes all 57 outcomes against both references, verifying identity and effective
compiler settings. Direct Node runtime assertions and the numeric analyzer test
pass. Formatting and whitespace checks pass for changed files.

`cargo clippy --locked --lib -- -D warnings` was also executed and fails on nine
existing warnings in untouched `compiler_binding_macros.rs`,
`compiler_reader.rs`, `macro_host.rs` and `native_form_adapter.rs`; none is in the
changed numeric lowering. Full Rust, broad upstream and language-inventory
suites were not repeated for this focused delivery. This adds no new imported
upstream assertion count: the supplemental key probe has its own hand-authored
provenance row in `fixture-origins.tsv`.
