# Control-flow contexts and evaluation-order evidence

Audited locally on **2026-10-08** against
[clojure/clojurescript](https://github.com/clojure/clojurescript) at clean commit
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. Paths and anchors below refer to
that revision. This is a bounded audit of the supported primitive core, not a
claim to port or run the complete upstream suites.

This adds **23** executable cases: **18** value/trace rows (**4 adapted**,
**14 independent**) and **5 independent** failure rows. Together with the
previous 179, **202** cases compare results, effects, failures, and stdout.
[fixture-origins.tsv](fixture-origins.tsv) records exact sources and reductions.
Existing cases and the shared host harness are unchanged. No production compiler
defect was exposed; no compiler or language-surface change was needed.

## Form/context matrix

Each cell names a behavior test in `tests/fixtures/execution.tsv`. The `flow-`
rows are independent probes unless identified as adaptations below. `Value`
means a value must be delivered to an enclosing expression; `Discard` means
execution must continue after its effects; `Return` means the last value exits
the current function. Every fixture is run inside `(fn* [observe] ...)` by both
compilers. An earlier top-level form in that wrapper is discarded, and its last
form is returned.

| Form | Value | Discard | Return |
| --- | --- | --- | --- |
| `if` | `flow-if-value`: argument to observe; nested let/do test; selected do result | `flow-if-discard`: selected let effects, then `after` | `flow-if-return`: nested if test and selected let result |
| `do` | `flow-do-value`: binding initializer; discarded if then returned let | `flow-do-discard`: all effects, including the final nested if/let, before `after` | `flow-do-return`: discarded let then selected nested do value |
| `let*` | `flow-let-value`: callee expression; sequential initializers and body finish before either argument | `flow-let-discard`: if/do/let initializers and final if effects before `after` | `flow-let-return`: nested let/if/do initializers and body return the final branch value |
| Empty `do` | `flow-empty-value`: observe receives nil | `flow-empty-discard`: continues to `after`; existing `discard-empty` | Existing `empty-do` |
| Empty `let*` body | `flow-empty-value`: initializer effect followed by observed nil | `flow-empty-discard`: initializer effect survives | Existing `empty-let`, `let-empty-body-effects` (including an effectful initializer) |
| Omitted `if` else | `flow-empty-value`: false returns observed nil; true selects empty do | `flow-empty-discard`: nil/true tests each run once and execution continues | Existing `missing-else`, `selected-missing-else` |
| Dynamic truthiness | `flow-truth-value`: selected string reaches observe | `flow-truth-discard`: selected effect followed by `tail` on every call | `flow-truth-return`: selected observed string exits the function |
| Ordinary invocation | `invoke-hof-argument` nested call; arguments in `flow-let-value` | Existing `discard-call`; matrix observe calls | `flow-let-value`, `flow-if-callee`, existing `callee-and-argument-order` |
| Method invocation | `flow-method-value`: observed result followed by `after` | Existing `js-discarded-method` | Existing `js-method-receiver`, `js-path-receiver`, `js-path-order` |
| Initialized `def` | Existing `def-value-context` | `flow-global-initializers`, existing `def-initializer-order` | Existing `def-value` |

The empty function forms are already covered by `empty-function`,
`fn-anonymous-empty-method`, `fn-named-empty`, `fn-named-empty-method`,
`fn-parameter-empty`, and `fn-method-parameter-empty`. Function bodies always
execute in Return context when invoked, regardless of the context where the
function value was created; adding Discard/Value *function-body* variants would
misidentify the context. Function construction itself is tested as a value by
`function-callee`, discarded by `discard-fn`, and returned then invoked by
`returned-closure` and the function audit's escaped-closure cases.

A declaration without an initializer has existing Discard and Return evidence
(`def-uninitialized-redefinition`, `def-uninitialized-value`,
`def-uninitialized-redefinition-value`). Value-position declarations remain a
specified subset exclusion: the reference emits invalid JavaScript for that
shape, and the analyzer rejects it. Namespace declarations are loader directives,
not runtime expressions in any of the three contexts. Loop/recur, try/throw,
set!, case, macros and collection/runtime operations remain excluded.

## Nested roles and observable frontiers

Tests, initializers, callees, receivers, and arguments always need a Value,
even inside a discarded or returned parent. Branches inherit their enclosing
context; all but the last body expression discard their values. The value
wrappers for do/let must return to their caller without prematurely returning
from the enclosing source function. This role matrix accounts for all three
nestable control forms without multiplying the suite by equivalent wrappers:

| Nested role | `if` | `do` | `let*` |
| --- | --- | --- | --- |
| Test | `flow-if-return` | `flow-if-discard`, all `flow-truth-*` | `flow-if-value` |
| Initializer | `flow-let-discard`, `flow-let-return` | `flow-let-discard`, `flow-do-value` | `flow-let-return` |
| Branch/body | `flow-let-discard`, `flow-do-return` | `flow-if-value`, `flow-let-return` | `flow-if-discard`, `flow-if-return`, `flow-do-value` |
| Callee | `flow-if-callee` (both selection and creation effects) | Existing `callee-and-argument-order` | `flow-let-value` |
| Argument | `flow-let-value` first argument | Same selected first argument; existing `nested-argument-order` | `flow-let-value` second argument |
| Method receiver/argument | `flow-method-value` receiver and first argument | Same test/first argument | Same receiver branch and second argument |

The nine main matrix rows put `.fail` in unselected paths. These would produce
observable failure outcomes and extra trace entries if emitted eagerly or
hoisted across the branch. The dynamic truthiness rows pass true, false, nil,
empty string, nonempty string, zero, nonzero number and a function through a
parameter. Each test emits one `test` marker; the function's failing body must
never run merely because the function is truthy. This avoids relying solely on
constant-foldable inputs. Existing `truth-negative-zero` and
`arity-missing-is-false` separately cover negative zero and undefined.

The distinct `callee-init`, `choose`, `callee-ready`, `arg1`, `arg2`, `body`
markers in `flow-let-value` expose repeated callee evaluation, reordered
arguments and a body entered too early. `flow-global-initializers` observes the
first global from the next initializer, finishes both local initializers before
the body, then redefines the first global; the second keeps its initialized value
while the captured global reader sees the new value. Existing
`global-callee-captured-before-arguments` checks selection of a global function
before an argument redefines it.

`flow-method-value` requires `receiver`, `ready`, `method-get`, first-argument
effects, second-argument effects, then exactly one `call` entry in that order.
The result is 40 from the method's `this.value`, so losing the receiver cannot
pass. Receiver evaluation and getter access each occur exactly once. Existing
`js-path-order` also checks the getter on a dotted host path.

The five new `execution-outcomes.tsv` rows measure distinct failure frontiers:

| Case | Effects retained; execution that must not occur |
| --- | --- |
| `flow-failure-test-value` | Test prefix and TypeError; neither branch, enclosing observe, nor subsequent form executes |
| `flow-failure-initializer-discard` | First initializer, false test, selected prefix and RangeError; later initializer/body/following source form do not execute |
| `flow-failure-argument-return` | Callee and first-argument prefix, then TypeError; second argument/body do not execute |
| `flow-failure-method-receiver` | Receiver prefix and RangeError; method getter and both arguments do not execute |
| `flow-failure-method-argument` | Receiver, method getter, true test and TypeError; other branch/second argument/method body do not execute |

Only the host harness throws. These cases add no throw, try, set!, atoms or raw
js* to the supported source language. Existing runtime-error rows cover nullish
and non-callable callees, including whether argument effects occur before the
failure. All outcome comparisons preserve effects preceding an exception.

## Dependency and source order

No new namespace fixture was necessary. The existing fast test
`namespaces::depth_first_dependencies_and_top_level_initializers_run_once_in_source_order`
has a shared dependency whose **global initializer** prints `shared`, a left
initializer printing `left:first`, a later source form printing `left:last`,
then a right initializer and the entry namespace. Its exact stdout is
`shared\nleft:first\nleft:last\nright\nmain\n7 7\n`. It exercises initializer
completion, dependency-before-consumer loading, source order and the diamond's
single shared load.

The existing shared `order/{shared,left,right,main}.cljs` fixture and
`execution::dependency_initialization_matches_standard_clojurescript` compare
both compilers directly: `shared\nleft\nright\nmain\n10\n20\n7\n7\n40\n`.
Consumers read initialized dependency globals; the shared dependency prints
once; redefinition is visible through a later call. Both checks were rerun.
The distinct fixtures keep their distinct stdout expectations; the temporary
fast-test source is not claimed to be the reference fixture.

## Inspected upstream assertions and implementation

| Pinned source and stable anchors | Disposition |
| --- | --- |
| `src/test/clojure/cljs/analyzer_tests.clj`, `analyze-ops`, do assertions (658–670) | Existing `empty-do`, `do-result`, `do-order` and new context matrix execute nil/final-value/statement ordering. `(do nil)` and singleton `(do 1)` use the same retained final-expression behavior; matrix empty/selected bodies cover the nil boundary. Vector/children/op AST layouts have no local API counterpart. |
| Same file/test, let assertions (671–685), if assertions (860–873) | Existing `empty-let`, `let-value`, `let-first-of-two`, missing-else and truth tests plus the matrix cover the applicable values, branches and bodies. Binding/children representation is excluded; lexical identity has its own audit. No redundant scalar-only rows were copied from the AST assertions. |
| `src/test/clojure/cljs/compiler_tests.clj`, `test-optimized-invoke-emit` (264–305) | Existing adapted `invoke-hof-expression`, `invoke-hof-argument`, `invoke-hof-expression-argument` cover the three compound callee/argument shapes; new matrix adds control-form nesting. Exact static-dispatch/temporary-name text, variadic/multi-arity/protocol/record/Closure-library optimizations are excluded; see the function audit. |
| Same file, `test-cljs-3077` (308–349) | Loop/recur capture wrappers and exact generated-wrapper strings are excluded. Its third, ordinary closure capture has existing `closure-example`, `returned-closure`, and escaped-closure behavior coverage; those independent tests are not ports of an emission-string assertion. |
| Same file, `test-externs-infer-is-nan` (385–390) | New adapted `flow-host-boolean-test` executes its exact conditional expression. The upstream assertion is about avoiding truth_ after type inference; that optimization is not claimed. Only the expression's runtime behavior is reused. |
| Same file, `test-goog-lib-infer-boolean` / `test-goog-module-infer-cljs-3438` (392–410) | Excluded: Closure-library/module resolution and return-type inference optimizations, not primitive conditional semantics. |
| `src/test/cljs/cljs/core_test.cljs`, `test-cljs-2267` and preceding const definitions (1430–1445), all seven assertions | New adapted `flow-truth-{value,discard,return}` retains all seven input values and branch decisions, uses parameter values instead of const definitions, strings instead of keyword results, and observed results instead of equality/test wrappers. It adds an independent function input. Const metadata/propagation is explicitly excluded. Existing `truth-*` cases retain their independent provenance. |
| `src/test/cljs/cljs/primitives_test.cljs`, `test-booleans` (457–469), all ten predicate results | Excluded as assertions: true?/false?/boolean? identity predicates and vector construction are not `if` truthiness. Host undefined truthiness has existing `arity-missing-is-false`; it does not port the predicate assertions. |
| Same file, `test-try-catch` (471–481) | Excluded: catch dispatch, finally and atom mutation. New host-failure rows only assert abrupt completion of supported expressions, not exception-handling semantics. |
| Same file, `test-dot` (659–679) | Existing `js-property`, `js-method`, `js-nested-method` and new `flow-method-value` cover supported property/method access and nested receivers independently. Explicit dot syntax, memfn and str-based receiver creation are excluded. The new host's getter/receiver/order assertions are independent additions. |
| Same file, `test-1556` (948–955) | Excluded: JS object literal statement parsing/property access. Replacing the object literal with a scalar would lose the syntax ambiguity being tested; ordinary `do` ordering is tested independently. |
| `src/test/cljs/cljs/specials_test.cljs`, complete file, `test-cljs-3125` (21–25), all four assertions | Excluded: set! result values, object writes, dynamic-var updates and mutable record/protocol methods. The approved read/method interop cannot faithfully express these mutations; no substitute runtime feature was added. |

Read `src/main/clojure/cljs/analyzer.cljc`, `parse 'if` (1857–1871),
`analyze-do-statements` / `parse 'do` (2468–2499), and let binding/body analysis
(2504–2622): tests and binding initializers require values, earlier body forms
are statements, and branch/body tails supply the enclosing result. The local
`analyzer::context_tracks_discarded_returned_and_argument_values` already checks
the public context tags, while the matrix checks behavior across emitter wrappers.

Read `src/main/clojure/cljs/compiler.cljc`, `emit* :if` (692–708), `emit* :do`
(1119–1126), `emit-let` (1144–1167), `emit* :invoke` (1200–1320), and `emit-dot`
(1498–1509). The reference uses expression/statement/return contexts, value
wrappers for do/let, direct host method syntax and ordinary `.call(null, ...)`
when static functions are disabled. It can omit truth checks or fold constant
tests; the local implementation may use a uniform truth helper. Tests compare
results and effects, never those emission choices or upstream AST map layouts.

## Reproduction and results

```sh
cargo test --locked
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution -- --ignored --nocapture
```

Final local validation passed **90 fast tests** (six explicit reference/corpus
tests ignored), Clippy and formatting. The differential command passed all four
reference tests: **202** runtime cases and all three existing namespace stdout
comparisons against **both** Maven **1.12.134** and the pinned checkout (reported
**0.0.135109660**). Both references confirmed
`{:optimizations :none, :static-fns false, :def-emits-var false}`. The earlier
202-case pass preceded completion of the seven-value truthiness rows; the final
run includes their final sources and all failure rows.

Tools were Node **v24.18.0**, Rust **1.99.0**, Clojure CLI **1.12.4.1618**,
effective Clojure **1.12.4**, and OpenJDK **25.0.2**. These are local runs; no
hosted CI run is claimed. The existing required conformance job includes the new
rows automatically. Reader and parser regression tests remain in the fast suite;
the large research corpus is unchanged and was not rerun for these fixture-only
additions. Adaptations retain the ClojureScript copyright/EPL 1.0 notice already
present in `execution.tsv`; none is described as a verbatim imported test.
