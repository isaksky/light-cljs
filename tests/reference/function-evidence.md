# Fixed-arity function and invocation evidence

The [2026-10-09 function arity audit](compiler-functions.md) supersedes this
historical audit's multiple/variadic and repeated-parameter exclusions. Its
single-fixed-method invocation observations remain regression requirements.

Audited locally on **2026-10-08** against
[clojure/clojurescript](https://github.com/clojure/clojurescript) at
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. All paths and anchors below refer
to that clean checkout. This bounded audit maps the inspected assertions and
fixture shapes; it does not claim to run or port the full upstream suites.
[fixture-origins.tsv](fixture-origins.tsv) records exact sources and reductions.

The later [compiler-language inventory](compiler-language/evidence.md) treats
this audit's valid-source multi-arity, variadic, apply, recur, letfn and protocol
exclusions as **missing compiler capabilities**. This document preserves the
earlier fixed-arity milestone and its original expectations; its negative
signature diagnostics are not successful implementations of those features.

This work adds **36** executable rows: **20** in `execution.tsv` (**7 adapted**,
**13 independent**) and **16 independent** rows in `execution-outcomes.tsv`.
Together with the existing 143, **179** runtime cases compare values, failure
categories, effect traces, and stdout. Function values are always observed by
invocation, never by serializing a function or comparing emitted JavaScript.
Existing rows and expectations are unchanged.

## Audited assertions and reductions

Execution IDs refer to `tests/fixtures/`; analyzer names refer to
`tests/analyzer.rs`. Existing hand-authored cases keep their original provenance.

| Pinned upstream source and inspected anchors | Disposition |
| --- | --- |
| `src/test/clojure/cljs/analyzer_tests.clj`, `analyze-ops`, fn operation/children/local assertions (800–824) | Existing `empty-function`, `named-recursion`, `self-captured` and ID tests cover anonymous/named identity. New adapted `fn-named-empty` executes `(fn a [])`; `fn-self-returned-and-called` preserves the returned self-reference of `(fn b [] b)` and adds a terminal branch/argument so a later invocation produces a scalar. Environment-map/children layouts have no local API counterpart. The `fn b [& a]` self-binding example is variadic and is excluded as a signature. |
| Same file/test, variadic/method/arity/body/parameter assertions (825–859) | Zero/one/two fixed parameters map to `empty-function`, `single-method`, adapted `fn-parameter-empty` and `fn-two-parameters`, and existing `function-body`. Parameter binding identity has separate LocalId checks. New independent empty-method and method-body cases cover both accepted primitive spellings, named and anonymous. Variadic fixed-prefix counts, multiple methods, method-vector/AST layout assertions are excluded. |
| Same file/test, all invoke assertions (874–884) | Keyword invocation and Var invocation (`#'str`) require excluded runtime features. Generic argument ordering is covered independently by `callee-and-argument-order`, `nested-argument-order` and the new higher-order cases; this is not a direct port of the keyword/Var assertions. |
| Same file, `warning-forms`/`no-warn`/`all-warn` (40–59) | The wrong-argument-count source motivates the independent arity experiments below. Warning-handler toggles, undeclared-var and keyword-arity diagnostics are excluded from the local API. Core `+` is not substituted with special arithmetic; local probes return parameters and observe effects. |
| Same file, `test-cljs-2005` (425–434), `test-cljs-2863` (436–455) | Duplicate overload arities, fixed arity exceeding a variadic prefix, and multiple variadic overload warnings: excluded because only one nonvariadic method is supported. Local `fixed_function_signature_boundaries_have_precise_diagnostics` verifies that this restriction remains explicit. |
| `src/test/clojure/cljs/compiler_tests.clj`, `fn-scope-munge` (69–90), all three assertions | Already adapted by `nested-named-function` and `nested-named-host-collision`; see the lexical audit. New escaped-self and independent-self-closure cases extend runtime coverage. Exact generated-name assertions remain excluded. |
| Same file, `no-warn-on-emit-invoke-protocol-method` (173–200), all eleven input forms | Excluded: custom protocol analyzer state, protocol dispatch under static functions, and emission-time warning collection. The presence of primitive arguments does not make these ordinary-call assertions. |
| Same file, `test-optimized-invoke-emit` (264–305), complete body | The three higher-order expression shapes at 287–301 are reduced to `invoke-hof-expression`, `invoke-hof-argument`, and `invoke-hof-expression-argument`. Exact binding regexes and `:static-fns true` optimization are excluded. Variadic/multiple-arity dispatch, keyword invocation, record fields, hinted declarations and Closure-library imports are excluded. The direct JS-path receiver property has existing `js-path-receiver`/`js-path-order` evidence. |
| `src/test/cljs/cljs/invoke_test.cljs`, complete file, especially its three `hof-*` functions (20–31) | These are compiler fixtures, not runtime assertions. The three adaptations execute their callee/argument shapes with effects. Simple defn becomes fn*/let*. One-argument `complement` is reduced to a captured predicate call plus `if` boolean negation, preserving the result and predicate effects for the supplied arguments. Creation is additionally observed to check once-only evaluation. General complement/apply behavior and static dispatch are not claimed. Remaining forms have the exclusions in the compiler row above; `delay` is also unsupported. |
| `src/test/cljs/cljs/core_test.cljs`, `original-closure-stmt`/`test-401-411` (557–566), `test-cljs-1536` and preceding lets (1262–1274) | Already faithfully reduced in the lexical audit. New `fn-returned-independent-self-closures` checks two escaped environments by repeated invocation; `fn-self-parameter-captured` checks a parameter shadowing the self-name inside another closure. These additions are independent regressions. |
| Same file, `test-383` (537–544), `test-477` (691–693), `foo-1284`/`test-cljs-1284` (1110–1124), `test-cljs-2741` (1603–1609) | Multi-arity/variadic selection, rest-sequence construction, collection invocation, and `Invalid arity` dispatcher errors are excluded. In particular, the six assertions in `test-cljs-2741` do not specify the behavior of a single fixed method. They must not be converted into wrong-argument-count errors for this subset. |
| Same file, `test-513` (546–550), `test-letfn-shadowing` (568–572), `test-481` (701–707), `test-541` (727–734) | The letfn-shadowing assertion is now covered intact by [the letfn audit](compiler-letfn.md). The other assertions retain their separate vector lookup/try, doseq/atom and printing/Symbol-helper requirements; independent capture/effect probes do not claim them. |
| Same file, `foo580`/`test-580` (767–771) | Map/keyword invocation is inseparable from the original indirect self-reference assertion, so the test itself is excluded. New independent `global-declared-call-from-closure` executes declaration, intervening caller creation, assignment, and a later call. Existing `global-self-reference` and new `global-self-call-after-redefinition` exercise supported global self-reference. |
| Same file, `foo-var`/`foo-set`/`test-cljs-982-var-deref` (1012–1023), `print-foo-1187`/`test-cljs-1187` (1054–1071) | Excluded: captured Var dereferencing after set!, and Var-valued case/apply. Replacing a Var with a captured function value would change redefinition behavior. Independent `global-function-snapshot-and-live-call` demonstrates the supported distinction: saved functions keep the old value, global reads see redefinition. |
| Same file, `incme`/`test-cljs-1225` (1091–1096), `test-1210` (1141–1146), `test-cljs-2585` (1595–1598) | Variadic local/core inc; destructuring/or with arguments; or/int?/string? plus host deletion: excluded as assertions. The last callee-expression shape has analogous independent `function-callee` coverage, not a macro/predicate port. |
| `src/test/cljs/cljs/primitives_test.cljs`, `test-arithmetic`, “Testing numeric equality in collections” (199–206) | Both closure assertions depend on loop/recur or for, collection construction, and map invocation. Repeated loop captures cannot be reduced to sequential independent calls without changing the tested property; excluded. |
| Same file, complete `test-apply` (390–455) | Arithmetic/list/apply nesting; infinite-sequence consumption; single/multiple/variadic arity and rest-sequence assertions all require excluded runtime/apply behavior. Even the single-arity function examples explicitly assert apply's argument expansion, so replacing apply with a direct call would not port those assertions. |
| Same file, complete `test-partial-and-comp` (492–514) | All ten assertions depend on partial/comp plus arithmetic or sequence operations; excluded. Ordinary higher-order calls are tested directly without claiming library-helper coverage. |

## Signature and invocation contract

The supported signature is exactly one fixed parameter vector, optionally named,
in either `(fn* name? [parameters] body...)` or
`(fn* name? ([parameters] body...))` spelling. An empty body returns nil;
a longer body evaluates each expression once and returns its last value.
The parameter may shadow the optional self-name. Returned closures preserve
parameters and self-references after the creating call has returned, and separate
calls produce independent environments. `fn-escaped-self-closure` and
`fn-returned-independent-self-closures` make these properties observable.

Bindings must be simple symbols, with duplicate parameter names rejected.
Multiple methods, variadic `&`, destructuring, qualified/dotted parameters and
invalid vectors have explicit diagnostics. The new analyzer test checks exact
categories and byte spans for the method spelling; existing tests cover the
direct spelling. These are deliberate subset restrictions, not claims that
all upstream validation or warning behavior is identical.

Both reference compilers and light-cljs produce these outcomes:

| Probe family | Observed result and effects |
| --- | --- |
| `arity-{direct,local,global,higher-order,method}-few` | Missing parameters bind JavaScript `undefined`, preserved as `{"$value":"undefined"}`. No runtime arity exception. |
| `arity-missing-is-false` | Missing parameter follows nullish ClojureScript truthiness and selects the false branch. |
| `arity-{direct,local,global,zero}-extra` | Extra arguments are ignored by the body but evaluated in order. The body starts after all argument effects. |
| `failure-extra-argument-before-body` | An unused extra argument throws before the body can run. |
| `failure-number-call`, `failure-boolean-call`, `failure-host-object-call` | TypeError; argument effects occur before attempting to invoke a missing/non-callable `.call`. |
| Existing `failure-nil-call`, new `failure-undefined-call`/`failure-callee-expression` | TypeError while reading `.call` from a nullish value. Callee effects occur, but argument effects do not. |
| Literal `(nil 1)` | Analysis error, separately confirmed with Maven 1.12.134: cause `Can't call nil`. Local diagnostic is `cannot call nil` at the callee span. It is not a runtime failure row. |

The reference emits `:fn-arity` warnings for the four known local/global
few/extra cases. Direct function expressions (including a named inline method)
and an unknown higher-order parameter emitted no arity warning in these probes.
Warnings do not prevent execution. light-cljs intentionally has no arity-warning
API and does not invent stricter count checking. Numeric, boolean and plain host
object callees likewise pass analysis and fail dynamically. Collection/keyword/
protocol callability is excluded, not generalized from these non-callable probes.

`global-callee-captured-before-arguments` confirms that an argument redefining
`f` does not change the callee already selected for that call. Conversely,
`global-self-call-after-redefinition` shows that a saved anonymous global function
reads the new global during its body. `named-self-call-after-global-redefinition`
keeps using its explicit local self-name. These independent cases run with
`:static-fns false`; ordinary global replacement does not implement Var boxes,
with-redefs, or dynamic bindings.

## Implementation cross-check

Read the pinned `src/main/clojure/cljs/analyzer.cljc` functions
`analyze-fn-method-param`, `analyze-fn-method`, `fn-name-var`, and `parse 'fn*`
(2225–2430): vector/method normalization, self-name followed by parameters, and
body return context explain the supported reductions. `parse-invoke*`
(4025–4087) issues warnings when function arity information is known;
`analyze-seq` (4370–4396) rejects a literal nil operator.

In `src/main/clojure/cljs/compiler.cljc`, `emit-fn-method` (950–965) and
`emit* :fn` (1032 onward) emit a plain JavaScript function for one fixed method.
Only the multiple-method dispatcher supplies the separate invalid-arity path.
`emit* :invoke` (1200–1330) uses `.call(null, ...)` for ordinary calls under
`:static-fns false`. `src/main/cljs/cljs/core.cljs`, `complement` (4465–4473),
confirms that its one-argument method is precisely predicate invocation plus
boolean negation; the adaptation retains that method's behavior only.
No production compiler defect was exposed and no compiler change was needed.

## Reproduction and verification

Maven **1.12.134** and the clean checkout (reported **0.0.135109660**) verified
`{:optimizations :none, :static-fns false, :def-emits-var false}`. Tools were Node
**v24.18.0**, Rust **1.99.0**, Clojure CLI **1.12.4.1618**, effective Clojure
**1.12.4**, and OpenJDK **25.0.2**. Results are local; no hosted CI run is claimed.

```sh
cargo test --locked
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution \
  programs_match_standard_clojurescript_results_and_effects -- --ignored --nocapture
```

The full fast suite passed **90 tests** (with its six explicit research/reference
tests ignored); Clippy and formatting passed. Two final empty-body fixture
additions were checked by the focused runtime and provenance tests, then the
runtime differential comparison above passed all **179 cases** against both
baselines. The previous 177-case comparison also passed; its rerun was required
by those added fixture shapes. Namespace, reader and parser implementation and
corpus fixtures are unchanged. Existing namespace differential evidence remains
applicable; those successful builds were not repeated for a fixture-only change.
CI's existing required release comparison includes all new runtime rows.

The isolated literal-nil analyzer observation is reproducible with:

```sh
clojure -Srepro -Sdeps '{:deps {org.clojure/clojurescript {:mvn/version "1.12.134"}}}' \
  -M -e '(require (quote [cljs.analyzer :as ana]) (quote [cljs.env :as env]))
  (env/with-compiler-env (env/default-compiler-env {:optimizations :none :static-fns false})
    (binding [ana/*cljs-static-fns* false]
      (try (ana/analyze (assoc (ana/empty-env) :def-emits-var false) (read-string "(nil 1)"))
        (catch Exception e (prn (.getMessage e) (some-> e .getCause .getMessage))))))'
# nil "Can't call nil"
```

Adapted fixtures retain the ClojureScript copyright/Eclipse Public License 1.0
notice in `execution.tsv`. No imported verbatim or generated cases are claimed.
