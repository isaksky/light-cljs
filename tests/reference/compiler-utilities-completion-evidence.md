# Complete utility source adaptation

`compiler-utilities-completion.py` reproduces 41 complete bodies from the pinned
ClojureScript revision `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` into
`tests/fixtures/compiler-utilities-completion.tsv`. It validates each original
body/setup span against the pinned Git bytes and SHA-256. This source audit does
not establish an execution pass.

The selection retains 611 active in-scope assertion sites:

| Pinned source | Complete bodies | Assertion sites |
| --- | ---: | ---: |
| `parse_test.cljs` | 4 | 55 |
| `other_functions_test.cljs` | 14 | 250 |
| `primitives_test.cljs` | 7 | 155 |
| `core_test.cljs` | 8 | 38 |
| `array_access_test.cljs` | 8 | 113 |

All four non-generative parse bodies are retained: long, double, UUID and boolean,
including safe integer boundaries, subnormal double rounding, random UUID
parsing, and the invalid-type exception paths. All fourteen other-functions
deftests are retained, including every row of the large every-pred/some-fn arity
matrices and update-vals/update-keys metadata setup.

The primitive bodies are test-js-primitives, test-booleans,
test-partial-and-comp, test-arrays, test-make-array, test-misc and test-char?.
The complete primitive-arrays-equal helper accompanies test-make-array. The
complete mixed test-misc preserves its memoized random call, mutation, loops,
lazy/sequence/transducer/hash/metadata operations and exception assertions.
Its utility subset is never counted as a full-body result.

The core bodies are test-uuid, aget-test, aset-test, unchecked-get-test,
memfn-test, test-730, test-cljs-2865 and test-str_. The complete str-fn-2865
helper accompanies test-cljs-2865. The array-access bodies are aget-test,
aset-test, unchecked-aget-test, unchecked-set-test, checked-aget-test,
checked-aset-test, checked-aget'-test and checked-aset'-test, including effects,
negative/out-of-range indices, non-array objects and nested accesses. Checked
bodies retain all four complete original aliases. The two suppress-errs calls
use the exact pinned `.cljc` macro expansion into with-redefs of *print-err-fn*
to nil, with the full original macro body/hash validated and retained in setup
provenance. That phase/import adaptation preserves the original runtime effects.

The adapter changes testing into do and expands every are row with its original
binding/evaluation order. It records is/are failures without throwing, then
asserts the accumulated result after the entire original body. This matters for
the parse tests' `(try ... (is false) (catch :default _ (is true)))`: a throwing
replacement for is false would be caught and could falsely pass an invalid-type
parse that unexpectedly succeeded. `--caught-false` emits an isolated negative
execution probe using exactly this adapter. Original exception classes, literal
contents, assertion messages, effects and helper bodies are retained.

408 previously unmapped source sites link to 24 additional complete report
cases. The other 203 sites retain their existing authoritative witness links.
The complete-body engine suite can execute all 41 fixtures independently of
those historical links. Five independent literal oracle probes in
`compiler-utilities-contracts.tsv` retain the complete Rust application
operations and expectations; same-file macro wrappers are removed only for
the independent upstream references, because they are a local phase dialect.
The fifth probe preserves the complete independent 100,000-input seeded numeric
roundtrip workflow. It is separate from the original test.check properties.

39 exact declaration mappings (22 public arity rows, sixteen metadata
Vars, and the historical SCI subs declaration) bind ordinary and macro execution
to public_utilities_execute_in_applications_and_macros. Each mapping retains the
declaration and resolved signature hashes, named test and whole Rust source
hash. The script validates them on every reproduction run. Mapping alone does
not declare passing execution. The complete mapped metadata reconciliation adds
the rand and subs metadata rows to these existing executing contracts; see the
[mapped metadata audit](compiler-core-var-metadata.md).

The following residual source obligations remain explicit in the ledger:

| Source/body | Exact retained obligation | Existing owner |
| --- | --- | --- |
| parse_test::test-gen-parse-long | Original chk/quick-check 100000 with gen/large-integer and prop/for-all*. Complete upstream test.check harness integration has no execution mapping. | light-cljs-125 |
| parse_test::test-gen-parse-double | Original chk/quick-check 100000 with gen/double, including NaN handling. Complete upstream test.check harness integration has no execution mapping. | light-cljs-125 |
| string_test::test-cljs-2300 | Complete external test.check capitalize property retains its generator/for-all/fmap setup and its subs reference implementation; ordinary substring execution does not execute this property. | light-cljs-125 |
| Both generative parse failure branches | Original `(is (:result res) (pr-str res))` diagnostic paths belong to those complete unmapped test.check workflows. | light-cljs-125 |
| array_access_test::cljs-2861-test | Original *unchecked-arrays* compiler configuration/no-op contract; verified gap tracked by the focused compiler-setting record. | light-cljs-132 |
| array_access_test::unchecked-arrays-file-scope-test | Original alpha namespace dependency and file-scope configuration; verified gap tracked by the same focused record. | light-cljs-132 |
| array_access_test.cljc source-program | Assertion-free suppress-errs macro module import integration. Its exact expansion/effects execute through the complete checked fixtures, but the original module itself has no execution mapping. | light-cljs-125 |

These are unresolved complete source/harness/namespace obligations, not
exclusions. A deterministic roundtrip loop or sampled generator does not execute
the original test.check properties. Public API/direct macro execution mappings
remain separate from these source-body obligations and from any full-language
completion claim.

The test.check and source-module assignments identify the remaining harness and
import integration work. The two *unchecked-arrays* assignments identify the
compiler setting itself. None changes the source hashes, original expectations,
intended dispositions or required full-language execution gate.

Reproduce source evidence with:

```sh
python3 -B tests/reference/compiler-utilities-completion.py
```

## Runtime contract and executed verification — 2026-10-11

The analyzer now lowers the thirteen previously missing utility names to the
shared canonical JavaScript runtime. Existing subs/rand/clone behavior is reused.
All four array constructor functions retain distinct function identities and
one/two-argument overloads, ordinary array elements, scalar initializer identity,
sequence demand and holes. Parser grammars preserve complete-string validation,
safe integers, signed zero, NaN, infinity and double rounding. Random selection
uses injectable Math.random with direct literal ranges and ordered effects.
Primitive/boxed JS symbols, the pinned special-symbol set, ragged array conversion,
UTF-16 substring bounds, saved functions and custom ICloneable dispatch execute in
both phases. Public clone/-clone aliases retain their respective Var metadata.
Fifteen canonical metadata entries were added from the pinned reference analyzer;
preexisting metadata entries were preserved.

Observed tools: Node v24.18.0 and Rust 1.99.0. Independent references were Maven
ClojureScript 1.12.134 and checkout a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e,
with verified optimizations:none, static-fns:false and def-emits-var:false.

These commands passed:

```sh
cargo test --locked --test compiler_utilities_bodies --test compiler_utilities_completion
cargo test --locked --test compiler_utilities_bodies utilities_completion_fixtures_match_pinned_release_and_checkout -- --ignored --nocapture
cargo test --locked --test evaluator --test compiler_interop --test compiler_core_helpers --test compiler_language --test provenance --test boundaries
cargo check --locked --all-features
node tests/support/js_runtime.test.js
node tests/support/js_core_helpers.test.js
python3 -B tests/reference/compiler-utilities-completion.py
python3 -B tests/reference/compiler-language/extract_inventory.py --check
rustfmt --check tests/compiler_utilities_bodies.rs tests/compiler_utilities_completion.rs
git diff --check
cargo bench --locked --bench compiler -- --samples 9 --iterations 1 --primitive-only
```

The complete-body suite executed all 41 workflows in Node and QuickJS applications
and QuickJS macros. The explicit ignored reference test was run: all 46 body/literal
workflows matched both references. Negative adapter injection failed as intended
in Node, QuickJS applications and macros. The independent seeded integer/double
roundtrip test executed 100,000 inputs per workflow in both application engines
and macro execution; it does not claim execution of the original test.check
properties. The prerequisite evaluator suite passed its ten execution/ownership/
capability/limit tests; its startup measurement remained ignored. An initial
command named a nonexistent compiler_numeric test target and was corrected to the
actual boundary and core-helper suites above.

The benchmark was a bounded smoke check, nine samples with one iteration, rather
than a comparative speed claim. Median total compilation times were 1,584 ns for
integer, 13,708 ns for nested-closure and 5,583 ns for js-nested-method. Artifacts
were retained at /tmp/light-cljs-122-compiler-bench.csv. Full upstream runtime tests,
the original external test.check property harnesses, the complete repository test
suite, and hosted CI were not run. Whole analyzer formatting still has preexisting
drift outside this change; new Rust test files pass rustfmt and the changed
analyzer fragment follows its Rust 2024 formatting.

The final family report executed:

```sh
python3 -B tests/reference/compiler-language/report.py --run --feature utilities --reference both --output /tmp/light-cljs-122-report.json --artifacts /tmp/light-cljs-122-report-final
```

All **445 intended utility-owned obligations were covered**: 408 source assertion
sites, 22 public declaration/arity rows and fifteen local API rows. Two additional
utility-named JVM analyzer tag-inference probes retain their precise preexisting
unrelated disposition. There were zero missing intended utility obligations and
zero utility version differences. The report also verified all 27 supplemental
boundary contracts and the registered exact phase tests. Its full-language strict
completion remained false, with 4,299 missing independent obligations across the
other families; a family selection is not a full-language completion claim.
Report artifacts and exact commands/outcomes are in /tmp/light-cljs-122-report-final.
