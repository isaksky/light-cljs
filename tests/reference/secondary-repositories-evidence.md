# Bounded secondary repository audit

Audited locally on **2026-10-08**. These are sources of hypotheses, not replacement
language oracles: Clojure runs on the JVM; Closure Compiler tests JavaScript
printing and transformations; Shadow CLJS tests its build and integration layer;
Closure Library tests runtime libraries. Applicable behavior is checked with
Maven ClojureScript **1.12.134**, `:optimizations :none`, `:static-fns false`, and
`:def-emits-var false`, then separately with the pinned ClojureScript checkout.

All four checkouts were clean at inspection. Every repository-relative path and
anchor below refers to its full recorded revision. The inspected scope is
bounded; file discovery and outlines are not claims that every body was read or
that an upstream suite was run.

| Local checkout | Repository | Commit |
| --- | --- | --- |
| `research/clojure` | [clojure/clojure](https://github.com/clojure/clojure) | `4278bcea436709ed2869e31b38f1c8e1e3ca5ad7` |
| `research/shadow-cljs` | [thheller/shadow-cljs](https://github.com/thheller/shadow-cljs) | `b8d76ecdffc8bb441f4200f3121885f23dd1da2a` |
| `research/closure-compiler` | [google/closure-compiler](https://github.com/google/closure-compiler) | `21768cdbfa949bcce3f80d4285feb66475c4758c` |
| `research/closure-library` | [clojure/closure-library](https://github.com/clojure/closure-library) | `38c238f011ba49820b6b0c4f831261f464cac152` |

## Clojure: complete requested files, no new distinct primitive case

Read all three files under `test/clojure/test_clojure/`, including helpers,
namespace setup and disabled forms: `fn.clj` (55 lines), `special.clj` (122 lines,
without a final newline), and `evaluation.clj` (226 lines). None is an executable
ClojureScript oracle. Existing fixture names below retain their original
provenance; a later overlap does not turn them into Clojure adaptations.

| Source and inspected test/section | Applicable overlap or specific exclusion |
| --- | --- |
| `fn.clj`, `fn-error-checking`, every testing block (14–55) | Bad/missing parameter declarations and malformed signature lists overlap the `fn-*` rows of `analysis-boundaries.tsv`. The upstream assertions are macro `fn` spec failures with JVM exception classes/messages; copying those expectations to primitive `fn*` would be wrong. Multiple signatures remain unsupported. Existing release/checkout classifications distinguish primitive acceptance, warnings and errors. No duplicate cases added. |
| `special.clj`, `multiple-keys-in-destructuring` through `binding-types-not-allowed` (26–77), `or-doesnt-create-bindings` (95–97), `resolve-keyword-ns-alias-in-destructuring` (104–107), `typehints-retained-destructuring` (118–122) | Map/sequence/variadic destructuring, keyword/symbol directives, defaults, alias expansion and JVM reflection metadata are excluded. Replacing them with simple symbol bindings would remove the behavior under test. The alias assertion does not establish ordinary `:require :as` global lookup. |
| `special.clj`, `keywords-not-allowed-in-let-bindings` and `namespaced-syms-only-allowed-in-map-destructuring` (79–93) | The simple invalid keyword/qualified binding shapes overlap `let-binding-keyword` and `let-binding-qualified`; vector/map destructuring and macro-spec wording are excluded. |
| `special.clj`, `amp-not-allowed-as-let-or-loop*-binding-name` (99–102) | Primitive `(let* [& 42] &)` overlaps the existing attributed `let-binding-amp` boundary. Crucially Maven ClojureScript 1.12.134 accepts this primitive while the pinned checkout rejects it; the JVM rejection cannot be used to erase that recorded difference. Macro `let` and `loop*` remain excluded. |
| `special.clj`, `quote-with-multiple-args` (109–116) | Quote, JVM compiler exceptions, cause traversal and ex-data form preservation are outside the subset. |
| `evaluation.clj`, `Eval` and helpers (25–53) | Runtime eval of lists/functions, JVM compiler entry points and runtime namespace rebinding are excluded. These are not ordinary source call-order assertions. |
| `evaluation.clj`, `Literals` and `evaluates-to-itself?` (57–80) | Scalar strings, finite supported numbers, nil and booleans overlap existing literal/UTF-16 fixtures. JVM object identity is not a JavaScript value contract. Ratio, BigDecimal, characters, keywords and the unsafe integer are explicit local boundaries; none is coerced into a different supported value. |
| `evaluation.clj`, `SymbolResolution` with setup (84–180): qualified global (112–115), lexical binding (172–173), current-ns global (175–177) | Existing namespace acceptance/lexical fixtures and `global-lexical-shadow` cover the supported lookup behavior. Runtime `in-ns`/`eval` wrappers are excluded. No new lookup counterexample remained after matching these behaviors to existing cases. |
| Same test: unresolved/global visibility/class/special-form assertions (117–170, 177–180) | Existing `unknown-global`, `unknown-alias`, `forward-global` and special-dispatch boundaries cover the intentional local restrictions. JVM errors are not reference expectations: standard ClojureScript may warn and emit. Private Vars, Java class/import lookup and class-before-local resolution are excluded. In particular, a JVM `Boolean` lookup does not specify explicit `js/Boolean` lexical-shadow behavior. |
| `evaluation.clj`, `Metadata`, `Collections`, empty `Macros`/`Loading` tests (182–226) | Structs, metadata, quoted/runtime collection evaluation and empty-list results are excluded. The non-callable-list assertion is reader-discarded upstream, so it is not claimed as an active regression. The final two deftests have no assertions. |

## Shadow CLJS: verified roots and bounded exclusions

The root `project.clj` (68–75) configures `src/test`. The dependency-consumer
`deps.edn` (14–18, 68–71) includes `src/repl`, `src/dev` and the development
`src/test` path. `shadow-cljs.edn` (44–47, 328–336) confirms the source paths and
browser/node test targets. The separate `test-project/shadow-cljs.edn` (1–33)
configures its own `src/main` for node-test, Karma and extern inference. These
additional local roots were inspected only to the bounds listed below.

The optional upstream-suite profile in `project.clj` (133–140) points to
`../oss/clojurescript/src/test/{clojure,cljs,self,cljs_cp}` and benchmark sources;
those directories are absent locally. Its associated configuration at
`shadow-cljs.edn` (611–624) uses `:static-fns true`. It is neither a second copy
of this audit's `research/clojurescript` baseline nor an executed test suite.

| Source and inspected scope | Applicable overlap or specific exclusion |
| --- | --- |
| `src/test/shadow/build/ns_form_test.clj`, complete file | `test-ns-alias-conflict` (96–103) is applicable narrow `:require :as` rejection, already covered by `namespaces::malformed_and_unsupported_namespace_specifications_have_explicit_diagnostics`. Other active tests concern refer/rename/as-alias, macro and package requires, defaults, metadata, global-import clauses and namespace AST rewriting. The large `test-parse-ns` parity form (27–94) is inside `comment`, not an active assertion. |
| `src/test/shadow/build/npm_test.clj`, complete file and `with-npm` helper | Package/path resolution, browser overrides, exports and dynamic imports are excluded. `test-resolve-to-global` (113–128) checks module-to-global rewriting and emitted `module.exports=(React);`; it is not an ordinary `js/React` lookup regression. |
| `src/test/shadow/cli_util_test.clj`, complete file | `parse-dummy`, `parse-with-init-command` and `upgrade-old-args-style` concern command-line argument processing, outside source-language behavior. |
| `src/test/shadow/build/closure/ReplaceRequirePassTest.java`, `SourceMapReportTest.java`, and `src/test/foo.js`, complete files | Java demonstration entry points for require rewriting and source-map reporting, plus a package-resolution fixture; no applicable primitive-language assertion. |
| `src/test/circular/main.cljs`, `foo.cljs`, `util/lib.cljs`, complete files; `src/repl/shadow/cljs/devtools_test.clj` (332–347) | The main → foo → util.lib → foo graph is expressible, but the associated body catches/formats errors without asserting the expected failure. Existing `namespaces::cycles_report_the_dependency_path` already tests cycle rejection. No runtime outcome was inferred from this fixture. |
| `test-project/src/main/test/js_interop_test.cljs`, complete file | Active `es6-as` and `es6-indirect` test ES module/default/nested imports and cross-eval constants. Earlier CommonJS examples are in `comment`. Replacing imports with a host object would remove the behavior asserted, so these are excluded. |
| `test-project/src/main/test/infer_test.cljs`, complete file | `obj-property`/`can-properly-call-obj` rely on `^js`, JavaScript object literals and extern inference. Other bodies are runner fixtures, trivial equality and asynchronous behavior. No broader interop was added to make them applicable. |
| `src/dev/demo/foo_test.cljs`, `test_fail.cljs`, complete files | Deliberately passing/failing test-runner fixtures, with no new primitive counterexample. |
| `src/repl/shadow/cljs/analyzer_test.clj`, complete file | `test-analyzer-data`, `test-analyzer-data2`, `test-macroexpand`, `test-analyzer-deftype` inspect or print destructuring/type-hint/static-fn/macro/type data. They are not executable primitive assertions. |
| `src/repl/shadow/cljs/modern_test.clj`, complete file | Class macros, inheritance, constructors and mutation are excluded. |
| `src/repl/shadow/cljs/build_test.clj`, 1–50, 393–441, 560–770 | Old suite under `comment` beginning at 33. Read require-order, REPL namespace/definition/if/call/reload bodies and `trying-to-break-static-fns`: runtime requires, REPL state, core operations, optimization, reify, dynamic binding and variadic dispatch are excluded. Simple def/if snippets have no asserted runtime outcome. |
| `src/repl/shadow/cljs/closure_test.clj`, `src/repl/shadow/build/js_inspector_test.clj`, complete files | Extern property discovery, module transpilation and JavaScript inspection are excluded. |

**No new executable fixture** was extracted from this repository. Its applicable
alias/cycle behavior already has local coverage. Other discovery was by filename
or anchor search only; unlisted bodies were not surveyed.

## Closure Compiler: JavaScript hypotheses checked as ClojureScript

Paths below start at `test/com/google/javascript/jscomp/`. Only the named bodies
and their immediately relevant helpers/comments were inspected, not whole large
files. These tests primarily assert printed or transformed JavaScript, so even
a faithful small reduction is an **adapted hypothesis**, not a verbatim imported
ClojureScript test or evidence that we implement Closure optimization passes.

| Source and inspected anchors | Applicable overlap, new case or exclusion |
| --- | --- |
| `CodePrinterTest.java`, `testLiteralProperty` (1025–1027) | `(64).toString()` becomes `secondary-numeric-receiver`; compare its returned string, not printer text. `secondary-signed-receivers` is an additional **original** negative integer/negative-zero probe, not another upstream assertion. |
| Same, `testExponents`, `testBiggerThanMaxLongNumericLiterals`, `testMinusNegativeZero` (3213–3217) | Existing float/exponent/negative-zero fixtures cover supported values. Exact minified spellings and arbitrary unsafe JavaScript integer acceptance are excluded; JavaScript unary minus and ClojureScript integer `-0` reader semantics are not interchangeable. |
| Same, `testZero` (3040–3045) | Its null immediately followed by `3` motivates `secondary-null-followed-by-digit`, checking length and both UTF-16 units. The Unicode escape is legal in both source languages; Java/JavaScript printer wrappers are removed. |
| Same, `testOctalInString`, `testOctalInStringNoLeadingZero`, `testStringEscapeSequences` (3048–3104, 3220–3248) | Existing `string-octal-escapes`, `string-reader-escapes`, `string-escaped-null`, UTF-16/control fixtures cover legal ClojureScript spellings and resulting values. JavaScript-only escape acceptance such as `\8`, `\9`, `\v`, and regex grammar are not copied into ClojureScript expectations. |
| Same, `testDirectEval`, `testIndirectEval`, `testFreeCall1`, `testFreeCall2`, `testFreeCall3` (2815–2838 for the free-call tests), adjacent optional-chain/tagged-template cases | `secondary-detached-property-call` and `secondary-detached-local-call` adapt the direct/free-call receiver distinction, using an extracted native string method to make receiver loss observable. Eval binding rules, optional chaining, templates and compiler node flags are excluded. Existing `js-method` is the direct receiver-preserving control. |
| Same, `testBreakTrustedStrings`, `testBreakUntrustedStrings` (806–846) | HTML/script embedding policy is distinct from Node execution. Existing `string-source-text` checks source-like values; this audit does not claim HTML-safe output. |
| `ExpressionDecomposerTest.java`, `testCannotExpose_expression4/5`, `testBug117935266_expose_call_target`, `testBug117935266_move_call_target`, `testBug117935266_expose_call_parameters` (1277–1344), `testExposeExpression15`, `testExposeExpression17` (1615–1624), `testExposeFreeCall` (1627–1636), `canExposeMethodCallee` | Callee/receiver/property evaluation before arguments is applicable. Existing callee/getter/order fixtures cover most shapes; new `secondary-receiver-before-redefinition` makes receiver capture observable by reassigning its global during argument evaluation. Exact hoisted temporaries, decomposition eligibility and transformed ASTs are excluded. `canExposeMethodCallee` has a correctness TODO and was not used as a normative outcome. |
| `NormalizeTest.java`, `testSplitVar`, `testSplitLet`, `testLetManyBlocks`, `testLetOutsideAndInsideBlock`, `testLetOutsideAndInsideFn`, `testMakeLocalNamesUnique`, `testMakeParamNamesUnique`, `testLetsInSeparateBlocks`, `testRemoveDuplicateVarDeclarations1` through `5`, `testShadowFunctionName` | Ordered initialization and separate lexical identities overlap `let-initializer-order`, `scope-restoration-effects`, `parameter-and-capture`, `named-scope-restores-outer` and the existing distinct-ID checks. JavaScript var merging/hoisting, rest/destructured/default parameters, imports/catches and exact normalized names are excluded. No JavaScript let/var rule is substituted for ClojureScript let*. |

## Closure Library: bounded runtime survey, no direct import

Paths below start at `closure/goog/`. The repository is the local
`clojure/closure-library` fork at the recorded commit, not an assumed current
revision of Google's repository.

| Source and inspected scope | Relevance and exclusion |
| --- | --- |
| `base_test.js`, `getFoo`/`add` helpers and bind/partial section (621–782), especially `testBindWithCall`, `testPartialUsesGlobal`, `testPartialWithCall`, `testPartialAndBind`, `testBindAndPartial` | Runtime wrappers bind/rebind `this`. They reinforce the receiver distinction, but `goog.bind`/`goog.partial`, variadic argument assembly and sloppy/global receiver substitution are library behavior. Removing those functions would remove the assertion's subject. No direct adaptation. |
| `functions/functions_test.js`, helpers (18–45) and `testPartialRight*` section (143–215) | Explicit undefined receiver for a free call versus `.call(obj)` is relevant background. Partial-right argument concatenation and wrapper semantics are excluded. The new native detached-method probes derive from Closure Compiler, not this library suite. |
| `string/string_test.js`, `testQuote`, `testQuoteSpecialChars`, `testCrossBrowserQuote`, `testEscapeString` (827–879) | String quote/eval round trips and null encoding are pertinent value-preservation principles, already covered by scalar/UTF-16 fixtures and the new Compiler-derived null/digit case. Runtime `goog.string.quote`/`escapeString`, dynamic eval and browser compatibility behavior are excluded; our compiler does not implement these library functions. |

**No new fixture originates from this repository.** The inspected tests have
useful overlapping principles but no remaining library-independent regression
that would justify another case. No claim is made about uninspected modules.

## New executable evidence and attribution

The six additions bring the runtime corpus from **238 to 244**: four value rows
in `execution.tsv` and two failure rows in `execution-outcomes.tsv`. Five are
adapted hypotheses, one is original, and **none is a direct import**. All use the
existing host and outcome harness. There is no new language feature, compiler
change or harness helper. Existing case origins and expected results stay intact.

| Stable local ID | ClojureScript behavior asserted |
| --- | --- |
| `secondary-numeric-receiver` | `(.toString 64)` returns `"64"`. |
| `secondary-signed-receivers` | Negative integer receiver returns `"-1"`; native `valueOf` retains negative zero. Original extension. |
| `secondary-receiver-before-redefinition` | Getter lookup precedes both arguments; redefining global target to nil in the first argument leaves the captured receiver intact, returning 40 with trace `["method-get","a","b",["call","a","b"]]`. |
| `secondary-null-followed-by-digit` | The escaped null and following digit remain two code units: length 2, first unit 0, second unit 51. |
| `secondary-detached-property-call`, `secondary-detached-local-call` | Calling an extracted string `slice` without the original receiver throws TypeError after the argument trace `[1]`, both for a property expression and a local holding its value. |

[fixture-origins.tsv](fixture-origins.tsv) records the exact repository, full
commit, source path, anchor, adaptation and asserted behavior for every addition.
The observations are independent literal expectations, then checked against the
standard compiler. JavaScript printer/decomposition output is never an oracle
for their ClojureScript runtime outcomes.

The adaptations are modifications of small test shapes from
`CodePrinterTest.java` and `ExpressionDecomposerTest.java`; their notices are
retained here, adjacent to the source/provenance index:

```text
Copyright 2004 The Closure Compiler Authors.
Copyright 2009 The Closure Compiler Authors.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```

[Full upstream license at the audited revision](https://github.com/google/closure-compiler/blob/21768cdbfa949bcce3f80d4285feb66475c4758c/LICENSE).
No source body was copied from the other three repositories.

## Reproduction and actual local verification

```sh
cargo test --locked --test provenance --test execution
LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution programs_match_standard -- --ignored --nocapture
```

The fast command passed **nine execution tests and two provenance tests**; six
explicit reference tests were ignored by that fast invocation. The second
command ran the runtime comparison and passed **244 outcomes, ordered traces
and stdout values against each baseline**, including all six additions. Release
and checkout also agreed with each other. The target redefinition emits an
ordinary reference warning in both baselines, while its runtime result remains
40; warnings were not treated as failed or silently dropped outcomes.

Actual tool versions: Rust **1.99.0**, Node **v24.18.0**, Clojure CLI
**1.12.4.1618**, effective Clojure **1.12.4**, OpenJDK **25.0.2**. The Maven
compiler reported **1.12.134**; the clean checkout at the full revision above
reported **0.0.135109660**. Both printed and verified
`{:optimizations :none, :static-fns false, :def-emits-var false}`.

The unchanged namespace and boundary comparisons retain their previous evidence
in [boundary-evidence.md](boundary-evidence.md); they were not repeated for this
fixture-only change. Reader/compiler code and existing regression assertions
were untouched. These are actual local results, not hosted CI results or runs of
the four secondary repositories' suites. The existing required CI conformance
command consumes the same fixture files, including the additions.
