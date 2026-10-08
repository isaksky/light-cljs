# Lexical binding audit and conformance evidence

Audited locally on **2026-10-08** against
[clojure/clojurescript](https://github.com/clojure/clojurescript) at
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. This is a bounded assertion audit,
not an execution of the upstream test suite. Paths and line numbers below are
at that commit. The exact fixture origins, reductions, anchors, and asserted
behavior are in [fixture-origins.tsv](fixture-origins.tsv).

The new passing corpus adds **21** rows to `tests/fixtures/execution.tsv`:
**11 adapted** and **10 hand-authored**, for **126** rows there and **143**
shared runtime cases including the existing 17 outcome cases. Existing rows
and expectations are unchanged. The new two-namespace fixture is separately
compared by stdout. Four diagnostic reproducers in
[lexical-reference-limits.tsv](lexical-reference-limits.tsv) are outside the
passing conformance corpus; their differences are described below.

## Assertion-by-assertion disposition

Execution IDs refer to `tests/fixtures/execution.tsv`; named analyzer tests
refer to `tests/analyzer.rs`. Mapping a semantic property does not claim that
an upstream AST assertion or generated-name assertion was copied.

| Upstream section and all inspected assertion anchors | Applicable mapping or precise exclusion |
| --- | --- |
| `src/test/clojure/cljs/analyzer_tests.clj`, `analyze-ops`, let assertions at 672, 673, 675, 676, 677, 678, 679, 681 | Empty/no-binding behavior: `empty-let`; scalar binding/body: `let-value`; two bindings returning the first: new adapted `let-first-of-two`. `let-initializer-order` and `let-empty-body-effects` check effects even for unused bindings or an empty body. Exact `:op`, vector types, and `:children` layout are excluded as an upstream AST schema contract. |
| Same file/test, local assertions at 683, 684, 685, 686, 687 | `let-value`, `initializer_references_the_previous_binding`, and `scope_exit_retains_binding_ids_and_restores_reference_targets` check local identity, spelling, initializer visibility, and spans. Child-list emptiness and environment-map shape have no local API counterpart; excluded. |
| Same file/test, dotted local assertions at 649–657 and 689–696; dotted global assertion at 639–647 | Implicit `a.b.c` and `inc.foo.bar` symbol expansion is unsupported. Supported explicit `.-member` forms do not establish coverage of these assertions. No claim is made that all property access is unsupported. |
| Same file/test, local JS-shadow assertions at 698–702 and 703–705 | Existing `js-local-shadow` checks `js/Math` resolving to a same-named lexical binding. New `local-global-host-math` checks all three reference classes and a dotted host path together. `:env :locals` layout is excluded; `LocalId` is the local contract. |
| Same file/test, munging assertion at 1118–1125 | `let-initializer-order` covers ordered effects and references to different names. New `let-first-of-two` covers the two-binding form; namespace/alias ID checks establish local versus global identity. Absence of upstream `:ns` on a binding and its `:name` field are not emitted-name requirements. |
| Same file/test, two shadow-chain assertions at 1127–1135 and 1136–1147 | `same-scope-shadow`, `initializer_references_the_previous_binding`, and new adapted `let-shadow-chain-effects`. `scope_exit_retains_binding_ids_and_restores_reference_targets` proves separately allocated same-spelled records remain distinct after nested scopes exit. The recursive upstream `:shadow` chain itself is excluded. |
| Same file, `test-locals-mapped-to-sym` at 1342–1345, its single assertion | Excluded: accepts a caller-supplied upstream analyzer environment whose `:locals` map contains symbols instead of binding maps (the core.async compatibility workaround). Our source-based API does not accept that map. New ID checks exercise our supported contract; they are independent tests, not a port of this API assertion. |
| Same file, `test-cljs-1785-js-shadowed-by-local` at 416–423, its warning assertion | Warning text/collection is excluded: no warning-handler API. The underlying parameter/local shadow behavior is covered by `parameter-shadow`, `js-local-shadow`, and `local-global-host-math`. |
| Same file, `test-cljs-1105` at 279–282, one assertion; `test-symbols-munge-cljs-1432` at 284–288, two assertions | All three test constant-table IDs for keywords or quoted symbols (`:test-kw`/`:test_kw`, `:$`/`:.`, `'$`/`'.`). Those literals and the constant-table API are excluded. `different-source-spellings` is an independent local-binding test, not a reduction of constant-table semantics. |
| Same file, `test-unicode-munging-cljs-1457` at 290–293 | The `is` assertion tests the keyword constant ID; its second symbol equality is supplied as the assertion message. Constant-ID spellings, keywords, and quoted symbols are excluded. Existing `unicode-local` covers a supported Unicode local; the `C♯` local experiment below is not a port of a keyword test. |
| Same file, `test-cljs-2814` at 1347–1350, three assertions | Node module/global-export munging is excluded: no module exports, package loader, or corresponding munging API. Namespace-aware source globals remain supported and tested separately. |
| `src/test/clojure/cljs/compiler_tests.clj`, `fn-scope-munge` at 69–90, all three assertions | New adapted `nested-named-function` covers global `foo` with nested self-named `bar`; `nested-named-host-collision` covers nested `console` beneath an anonymous function. Added finite self-calls make binding identity observable. The three exact generated symbols are excluded; existing `named-recursion`, `self-captured`, and new `named_function_and_parameter_ids_restore_enclosing_local` check self/capture/parameter identity. |
| Same file, `test-munge-dotdot` at 108–118, three assertions; `test-resolve-dotdot` at 120–126, one assertion | Excluded: public munge API string/symbol output and the `cljs.core/..` macro/global. Dots are not supported in local binding names; these cases cannot faithfully reduce to ordinary local identities. |
| Same file, `test-3368-global-shadowing` at 376–383, one regex assertion | New adapted `same-host-name-initializer` retains the nested function and same-named host initializer. Browser `window` becomes Node `console`, both in the upstream analyzer's known JS-global list; a truthy `log` member makes initialization observable. Exact `window__$1` output excluded. Substituting `Math` is not equivalent in return context; see the retained limitation below. |
| Same file, `test-amp-as-local-clj-2954` at 411–433, exception-type/message assertions for let, let*, loop* | Existing `invalid_special_forms_have_source_diagnostics` rejects `&` as a `let*` binding or parameter. The `let` macro wrapper reduces faithfully; upstream exception classes/wording are excluded. `loop*` is unsupported. |
| `src/test/cljs/cljs/core_test.cljs`, `original-closure-stmt` plus `test-401-411` at 557–566, both assertions | New adapted `closure-statement-shadow` and `closure-sequential-shadow` preserve the strings and captured lexical identity, reducing simple let/fn/defn to let*/fn*/def. Existing numeric closure cases retain their original hand-authored provenance. |
| Same file, `test-letfn-shadowing` at 568–572, one assertion | Excluded: mutually visible letfn bindings require `letfn*`, which is outside this subset. Sequential let functions are not equivalent. |
| Same file, `test-cljs-1274` at 1228–1232, both assertions | Unqualified and current-namespace-qualified global reads map to `globals_keep_namespace_identity_and_lexical_shadowing` and the new shared `lexical.main` fixture. The qualified **definition** in upstream setup remains unsupported; ordinary simple definitions provide the same global-read property. These are existing/independent fixtures, not copied assertions. |
| Same file, `test-cljs-1536` plus its preceding lets at 1262–1274, all four assertions | Four new adaptations: `def-from-same-local`, `defn-from-same-local`, `defn-local-returned`, `defn-local-called`. Same-spelled lexical initializers/captures survive global registration, including both call/value contexts. Plain defn reduces to def plus unnamed fn*, preserving the enclosing lexical reference; introducing an explicit self-name here would change the behavior. |
| `src/test/cljs/static/core_test.cljs`, namespace/header and `foo-test` | The sole equality assertion uses unsupported core `=` and is not ported. The documented reserved namespace component `static` is tested independently by `lexical.static`, with qualified and aliased reads and actual values. |

The audit also read the lexical-name portions of `analyze-ops`' fn section
(804–824 and parameter assertions at 852–859). Self-name binding/reference
identity maps to `named-recursion`, `self-captured`,
`named_function_and_parameter_ids_restore_enclosing_local`; parameter identity
maps to `parameter-and-capture` and the scope-restoration ID test. The variadic
`fn b [& a]` example cannot execute in the subset; its self-binding property
is covered by a fixed-arity named function. Arity dispatch, variadic functions,
macro environments, destructuring, and full method AST schemas are not added.

## Implementation cross-check and local regressions

Read `src/main/clojure/cljs/analyzer.cljc` at the pinned commit:

- `analyze-let-bindings*` (2509 onward) analyzes an initializer using the old
  environment, then associates the new binding. Its validation rejects slash,
  dot, and `&` names. Existing local analysis follows this ordering.
- `analyze-fn-method-param`, `analyze-fn-method`, and `fn-name-var` (2225 onward)
  extend lexical environments with the function name and then parameters.
  `handle-symbol-local` (1318 onward) explains the API-only test above.
- `resolve-var`'s `js/` branch (1330 onward) consults an exact lexical name
  before host resolution; globals and alias/full namespace references have
  separate identity. A local `Math` shadows `js/Math`, while `js/Math.sqrt`
  still names the host path. Namespace aliases are not lexical bindings.
- `src/main/clojure/cljs/compiler.cljc`, `munge` (114 onward) and `emit-let`
  (1143 onward), explain the context-sensitive reference limitations below.

New independent runtime cases cover nested initializer scope exit, sibling
captures whose creating scopes have exited, parameter capture while calling an
outer capture, self-name scope exit, global redefinition after a lexical
snapshot, and collision with JavaScript keywords/globals/emitted helper names.
The emitter still uses only ID-based local/global storage. No production
analyzer/emitter defect was exposed or repair needed.

The five new analyzer tests check persistent distinct IDs, exact reference
targets, declaration/reference spans, separate namespace global identities,
and nine unresolved initializer/leakage occurrences at exact byte spans
(including multibyte text before an error). The namespace leakage test checks
the originating file, source span, and line/byte-column. These diagnostic tests
assert the deliberate local unresolved-name error policy, not equivalence to
standard ClojureScript's undeclared-var warnings.

`tests/fixtures/namespaces/lexical/{static,main}.cljs` checks equal global names
in different namespaces, a lexical `lib` alongside alias `lib/value`, current
and dependency full qualifiers under lexical shadowing, and captured local
values versus later global reads. Expected stdout is exactly:

```text
40
30
12
12
11
12
30
32
33
```

## Reference limitations retained, not converted to expectations

The four minimal diagnostic sources and light-cljs expectations are retained
in `lexical-reference-limits.tsv`. They are independent experimental probes,
not accepted passing conformance rows or new unsupported language features.

| Reproducer | Observed reference result | Local contract |
| --- | --- | --- |
| `return-spelling-collision` | Release and checkout return 2 with trace `[2]`: `a-b` and `a_b` collapse in a returned let. | Distinct IDs preserve trace `[1]`. |
| `return-keyword-collision` | Release and checkout return 2 with trace `[2]`: `class` and `class$` collapse in a returned let. | Distinct IDs preserve trace `[1]`. |
| `unicode-local-invalid-js` | Maven 1.12.134 emits `var C♯`, and Node rejects the generated module with SyntaxError before case execution. No checkout execution claim for this probe. | Returns 5; LocalId does not require a JavaScript identifier. |
| `return-host-initializer` | Release and checkout throw TypeError after emitting `var Math = Math`: hoisting hides the host during initialization. `Math` is absent from the analyzer's narrow known-JS-global list. | Initializer resolves the host before installing the new local, returning 4. |

The positive `different-source-spellings` and `keyword-and-helper-spellings`
cases intentionally put the colliding let in **statement** context. Upstream
`emit-let` supplies lexical renames there, so those values/effects agree with
both baselines. The distinct-ID analyzer test also checks return-context names.
The source expectations were not weakened to reproduce the reference bugs;
the differing shapes remain explicit, reproducible limitations. The
`same-host-name-initializer` adaptation uses `console` from the same known-host
list as upstream `window`, rather than silently claiming Math is equivalent.

To reproduce the three runtime discrepancies independently (from repository
root, with a fresh absolute output directory):

```sh
lexical_output=$(mktemp -d)
sed '/^unicode-local-invalid-js/d' tests/reference/lexical-reference-limits.tsv > "$lexical_output/cases.tsv"
clojure -Srepro -Sdeps '{:deps {org.clojure/clojurescript {:mvn/version "1.12.134"}}}' \
  -M tests/reference/execution.clj release 1.12.134 - "$lexical_output" cases "$lexical_output/cases.tsv"
node -e 'process.chdir(process.argv[1]); require(process.cwd()+"/reference.js"); require(process.argv[2]);' \
  "$lexical_output" "$PWD/tests/support/execution.js"
```

Use the documented checkout dependency/identity invocation in
[checkout-evidence.md](checkout-evidence.md) for the second baseline. To
reproduce the Unicode module failure, instead supply the original four-row
TSV directly; loading the launcher fails before the shared runner starts.
All four minimal probes were also compiled locally with `target/debug/light-cljs --compile --text` (wrapped in `fn* [observe]`) and executed through the shared runner; their local values/traces matched the retained TSV. The intentionally failing reference probes do not replace the successful comparisons.

## Verification

Both baselines verified `{:optimizations :none, :static-fns false,
:def-emits-var false}`. The release was Maven **1.12.134**; the clean checkout
above reported **0.0.135109660**. Tools: Node **v24.18.0**, Rust **1.99.0**,
Clojure CLI **1.12.4.1618**, effective Clojure **1.12.4**, OpenJDK **25.0.2**.
These are local results; no hosted CI run is claimed.

```sh
cargo test --locked --test analyzer --test namespaces --test execution --test provenance
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution -- --ignored --nocapture
```

The affected fast targets pass: **16 analyzer**, **17 namespace**, **8 execution**,
and **2 provenance** tests. Formatting and Clippy pass. The namespace
comparison passed against both baselines with exact stdout; existing namespace
acceptance and dependency-order comparisons also passed. After isolating the
reference limitations, the focused `programs_match_standard_clojurescript_results_and_effects`
comparison passed all **143** cases against both baselines in **9.89 seconds**.
There was no repeated full-suite run: failed case comparisons were rerun only
after the relevant fixture changes; successful namespace comparisons were not
repeated. CI's existing ignored-test invocation automatically includes the new
namespace oracle test. Reader/parser production code and corpus fixtures are
unchanged; their previously recorded whole-corpus evidence remains applicable.

## Attribution

The adapted fixtures derive from ClojureScript tests, Copyright (c) Rich Hickey.
All rights reserved. Their Eclipse Public License 1.0 notice is retained beside
the adapted execution rows. The upstream
[license at the audited commit](https://github.com/clojure/clojurescript/blob/a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e/epl-v10.html)
applies to those adaptations. No case is labeled a verbatim upstream import.
