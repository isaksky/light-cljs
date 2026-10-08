# Bounded upstream coverage and fixture provenance

Surveyed on 2026-10-07 (America/Denver), against the clean research checkout of
[clojure/clojurescript](https://github.com/clojure/clojurescript), commit
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. Every upstream path and anchor below
refers to that revision; line ranges are navigation aids, not moving references.
The recorded sections were read, including their assertions. This is an evidence
index for the implemented subset, not a claim of complete compiler or suite
coverage and not a work queue.

The oracle remains Maven ClojureScript **1.12.134**, with `:optimizations :none`,
`:static-fns false`, and `:def-emits-var false` in the analysis environment.
[Milestone evidence](milestone-evidence.md) records the successful 105-case
value/effect comparison and two namespace stdout comparisons. Research source
inspection and release-oracle execution are distinct evidence. The 296-file
reader corpus measures syntax handling, not executable semantics.

## Local identities and origins

[fixture-origins.tsv](fixture-origins.tsv) explicitly assigns an origin to each
of the **105** existing [execution fixtures](../fixtures/execution.tsv), using
its first-column name as its stable ID. The original **74**, from `nil` through
`discard-reader-form`, remain hand-authored; the **31** milestone probes from
`def-value` through `js-branch-method` also remain hand-authored. Similarity to an
upstream assertion does not turn those independent programs into adaptations.
Their source, expected values, traces, ordering, and four-column format are
unchanged by this inventory.

The named Rust test
`namespaces::malformed_and_unsupported_namespace_specifications_have_explicit_diagnostics`
is an **adaptation** of the applicable `analyzer_tests/spec-validation`
assertions, with additional local subset-boundary checks. Its source attribution
and reduction rationale are recorded in the sidecar and beside the Rust test.
Other namespace, analyzer, CLI, and complete-file tests cited below are
hand-authored unless explicitly identified otherwise. There are currently no
imported verbatim or generated execution cases.

IDs in the tables denote `tests/fixtures/execution.tsv` entries unless prefixed
with `analyzer::`, `namespaces::`, or `execution::`, which denote Rust test
function names in the corresponding `tests/*.rs` file. A mapping says which
behavior is exercised; it does not claim that the whole upstream test was ported.

| Existing local group | Stable IDs / named evidence | Observed behavior |
| --- | --- | --- |
| Literals | `nil`, `true`, `false`, `integer` through `trailing-point`, `string` through `empty-string` | Supported scalar spellings, finite numbers, signed zero, exact UTF-16, escaped and Unicode strings. |
| Truth and sequencing | `truth-zero` through `selected-missing-else`, `empty-do`, `do-result`, `do-order`, `top-level-order` | ClojureScript truthiness, omitted else, final values, ordered effects. |
| Lexical bindings | `empty-let` through `returned-closure`, `let-initializer-order`, `unicode-local`, `javascript-reserved-local` | Sequential initialization, shadowing, scope exit, persistent/independent captures. |
| Functions and calls | `empty-function` through `nested-argument-order` | Fixed-arity function values, implicit bodies, local self-recursion, callee-before-argument order. |
| Context and branching | `if-test-once` through `discard-empty`, `macro-name-local`, `discard-reader-form` | One test evaluation, only selected branch effects, discarded values, macro-name shadowing, reader discards. |
| Globals | `def-value` through `def-value-context` | Ordinary mutable definitions/redefinitions, self-reference, undefined declarations, lexical versus global identity, initializer effects and definition values. |
| Host access | `js-math` through `js-branch-method` | Global paths, getters and property reads, receiver retention, evaluation order, local host-root shadowing, undefined properties. |
| Namespace runtime | `namespaces::acceptance_program_prints_four_from_api_and_cli`, `namespaces::globals_keep_namespace_identity_and_lexical_shadowing`; `execution::namespace_program_matches_standard_clojurescript_stdout`, `execution::dependency_initialization_matches_standard_clojurescript` | Two-namespace stdout `4`, aliases/full qualifiers, dependency/source ordering, shared dependency once, global redefinition across namespaces. |

## Additional hand-authored outcome probes

The subsequent [outcome harness work](outcome-evidence.md) adds **17** independent
cases in `tests/fixtures/execution-outcomes.tsv`, for **122** executable cases.
Their five columns are stable ID, source, outcome JSON, trace JSON, and stdout
JSON. The original 105 cases and their expectations remain unchanged. The new
sidecar record identifies these as hand-authored host-boundary/failure probes;
no additional upstream assertion or suite-port coverage is claimed.

## Inspected upstream sections

### Analyzer and compiler suites

All active test bodies and surrounding helpers were read in the five requested
files under `src/test/clojure/cljs/`. The table groups assertions by source
behavior; upstream AST layout, spec conformance and generated JavaScript strings
are not local compatibility contracts.

| Path, test and assertion anchor at the pinned revision | Applicable behavior and current coverage / exclusion |
| --- | --- |
| `analyzer_tests.clj`, `analyze-ops`: scalar constants (628–633), do (658–670), let/local/shadow chains (671–687, 1126–1147) | Scalar fixture groups, `empty-do`, `do-result`, `do-order`, `empty-let`, `sequential-let`, `same-scope-shadow`, `shadow-initializer`; `analyzer::closures_resolve_to_distinct_persistent_binding_ids`, `analyzer::initializer_references_the_previous_binding`. Keyword/collection ASTs excluded. |
| `analyzer_tests.clj`, `analyze-ops`: fn (800–859), if (860–873), invoke (874–884) | `empty-function`, `function-body`, `single-method`, named/self recursion and closure IDs; `missing-else`, selected-branch/effect IDs; `function-callee`, `callee-and-argument-order`. Upstream keyword/core-function invocation examples are operation-class analogues, not directly ported cases. Variadic/multiple methods excluded. |
| `analyzer_tests.clj`, `analyze-ops`: def (770–783) | `def-value`, `global-read`, `global-uninitialized`, `def-uninitialized-value`, `global-function-value`; `analyzer::global_identity_is_stable_and_distinct_from_lexical_and_host_identity`. Covers declaration/initializer/qualified identity; upstream AST assertions alone do not verify returned values or redefinition. |
| `analyzer_tests.clj`, `spec-validation` (69, 75, 81/87, 93, 153, 159, 171) | The attributed namespace validation adaptation covers malformed require libraries, unknown/repeated options, non-symbol/conflicting aliases, unknown clauses. `:refer`, `:rename`, `:use` assertions are excluded; repeated `:require` clause at 177 is a gap below. |
| `analyzer_tests.clj`, `test-cljs-3320` (1538–1543) | `namespaces::aliases_cannot_duplicate_or_redirect_namespace_qualifiers` rejects `js` alias. Upstream warns; local rejection is a deliberate stricter boundary, not warning-text equivalence. |
| `analyzer_tests.clj`, `analyze-ops`: ns (1095–1097), host access (1080–1094), JS vars (1109–1112), local JS shadow (697–705); `test-cljs-1785-js-shadowed-by-local` (416–423) | Named namespace runtime tests; `js-property`, `js-method`, `js-method-receiver`, `js-property-once`, `js-math`, `js-local-shadow`, `js-global-distinct`. Quoted receivers require reduction; warnings, expanded dot syntax, and invalid/non-finite host identifier `js/-Infinity` excluded. |
| `analyzer/spec_tests.clj`, `test-binding`, `test-const`, `test-def`, `test-do`, `test-fn`, `test-if`, `test-let`, `test-local` (19–180) | Same primitive/definition fixture mappings as above. `test-let` (164) and `test-fn` (96) have missing exact empty-body shapes noted below. Schema assertions are excluded. `test-invoke` uses unsupported core functions; generic-call overlap only. |
| `analyzer/spec_tests.clj`, `test-host-call` (109–115), `test-host-field` (117–123), `test-js-var` (155–158), `test-ns` (217–222) | Shorthand `.substring`/`.-length`, host vars and one-item require vectors map to host and namespace tests. Explicit `(. target ...)`, Goog runtime loading and upstream AST schema excluded. |
| `compiler_tests.clj`, `test-amp-as-local-clj-2954` (411–433, primitive let assertion 423–426) | `analyzer::invalid_special_forms_have_source_diagnostics` rejects `&` bindings/parameters. The upstream `loop*` case is unsupported. |
| `compiler_tests.clj`, `fn-scope-munge` (69–90) | `named-recursion`, `self-captured`, `javascript-reserved-local`, `unicode-local` cover semantic name isolation; exact generated-name format excluded. |
| `compiler_tests.clj`, `test-3368-global-shadowing` (376–383) | Specifically `[window js/window]`: current `js-local-shadow`, `shadow-initializer`, `js-emitter-name-collision` provide indirect coverage; exact host-initializer shape is a gap. |
| `compiler_tests.clj`, `test-optimized-invoke-emit` (264–305; JS path 281–282) | `js-path-receiver`, `js-path-order`, `callee-and-argument-order`, `nested-argument-order` cover semantic order/receiver concerns. Static/variadic/protocol dispatch and exact optimized JS remain excluded. |
| `analyzer_api_tests.clj`, all six tests: `cljs-warning-test`, `with-warning-handlers-test`, `vary-warning-handlers-test`, `get-options-test`, `get-js-index-test`, `throw-test` | Warning handlers, compiler-state/options/index inspection and API lookup failures; no applicable executable primitive assertion. API `throw-test` is not a source-language throw regression. |
| `analyzer_pass_tests.clj`, `test-walk`, `remove-local`, `test-and-or-*`, `test-core-predicates`, `test-cljs-3309`, `test-lite-mode-pass` | Upstream AST walking/mutation, macro/type/boolean optimizations, exact code and core vector rewriting are excluded. Host words inside an optimizer test do not make the optimizer assertion covered by basic interop. |

The remaining bodies in these five files concern warning settings, type/extern
inference, destructuring, macro expansion, metadata, private/dynamic/constant
Vars, collections, loop/recur, try/throw/case, constructors, protocols/records,
`js*`, dynamic namespace operations, and analyzer/schema internals. These are
excluded by the source subset or the absence of an upstream analyzer API contract.
Notable boundaries that could otherwise be mistaken for current coverage:

- `analyzer_tests/test-constants` (296–308) concerns `^:const` and `set!`, and
  `test-cljs-1992` (1222–1232) concerns `declare` function metadata. Neither is
  ordinary mutable global redefinition evidence.
- `analyzer_tests/test-cljs-1536` (599–607) tests REPL `:def-emits-var true`.
  Our definition-result contract uses ordinary mode with that setting false.
- `test-cljs-1274` (349–354) accepts a current-namespace-qualified **definition**;
  local definitions intentionally accept simple symbols only. Qualified reads
  are supported. Undeclared-global warning/AST cases in `test-cljs-2139` and
  `analyzer/spec_tests/test-var` differ deliberately from local hard errors.
- Namespace metadata, `:use`, `:refer`, `:rename`, macro/import canonicalization,
  global imports, and runtime `require`/`ns*` assertions are excluded, including
  `test-cljs-1346`'s runtime-require portion and `test-cljs-2037`'s repeated
  runtime requires. Narrow file-header `:require :as` is supported.

### Runtime, static and self-host suites

For the large runtime files, only the named bodies below were audited; outlines
and keyword discovery elsewhere do not count as surveyed semantics. Paths in
this table start at `src/test/`.

| Path and inspected body / assertion anchor | Applicable behavior and current coverage / exclusion |
| --- | --- |
| `cljs/cljs/core_test.cljs`, `original-closure-stmt` setup and `test-401-411` (557–566) | Retained earlier lexical value and sequential let capture map to `closure-example`, `closure-before-rebinding`, `shadow-initializer`, `sequential-let`. Macro wrappers would need faithful primitive reduction. |
| `cljs/cljs/core_test.cljs`, `foo580` definitions and `test-580` (767–771) | Declaration followed by initialization with a global self-reference: related `global-uninitialized`, `global-self-reference`, `global-self-init`. Its map/keyword-call wrapper is excluded; declaration with an intervening reader closure lacks an exact local case. |
| `cljs/cljs/core_test.cljs`, `test-cljs-1274` (1228–1232), `test-cljs-1536` and preceding lets (1262–1274) | Qualified/unqualified global reads map to `namespaces::globals_keep_namespace_identity_and_lexical_shadowing`. Same-named lexical initializer entering a def is a direct-case gap. Qualified def setup is unsupported. This runtime `test-cljs-1536` differs from the same-named analyzer test. |
| `cljs/cljs/core_test.cljs`, `test-cljs-2267` and const setup (1438 onward) | Seven `if` assertions include truthy zero/empty string and false nil/false; `truth-zero`, `truth-empty-string`, `truth-nil`, `truth-false` cover those values. Const propagation/keyword result values are excluded, so removing their wrappers would be an adaptation, not constant-folding coverage. |
| `cljs/cljs/primitives_test.cljs`, `test-dot` (659–678) | `.-length`, shorthand substring calls and nested method/property access overlap with `js-property`, `js-method`, `js-nested-method`, `js-property-once`. Explicit-dot and `memfn` assertions are excluded. Numeric receiver and computed-result property cases are noted below. |
| `cljs/cljs/primitives_test.cljs`, `test-3352` (956 onward) | Integer `-0` versus floating `-0.0`: `integer-zero`, `negative-zero`. Upstream division/infinity operations are excluded; this does not establish radix/UTF-16 upstream provenance. |
| `cljs/cljs/invoke_test.cljs`, complete file (50 lines): `hof-fn-expr-should-be-bound`, `hof-arg-should-be-bound`, `hof-fn-expr+args-should-be-bound` (20–31), JS path call (40) | Compiler fixture shapes, with no runtime assertions in those forms. `callee-and-argument-order`, `nested-argument-order`, `js-path-order` supply independent semantic checks. Variadic/multiple arities, complement, delay, keyword calls, records and Closure library alias loading excluded. |
| `cljs/cljs/specials_test.cljs`, complete file (25 lines), `test-cljs-3125` | All four assertions concern `set!` result values for host/global/record mutation. Mutation and record/runtime wrappers excluded; def redefinition and property reads do not cover writes. |
| `cljs/static/core_test.cljs`, complete file (17 lines), namespace header/comment and `foo-test` | Specifically exercises namespace component `static`, a JavaScript reserved word, with trivial equality. It is not a static-function-dispatch suite. Local generated binding IDs suggest applicability, but no exact reserved-namespace runtime fixture exists. |
| `self/self_host/test.cljs`, `test-compile-str` (176 onward) | Empty `(fn [])` maps to `empty-function`; function-valued if maps to `truth-function`; `(.toString "a")` maps to string-method behavior. The ordinary versus `:static-fns true` call assertions distinguish compilation modes. Exact JS, core-runtime calls, self-host API and static dispatch are excluded. |
| `self/self_host/test.cljs`, `test-CLJS-1330` (657 onward) | Evaluates `(.toString 1)` to `"1"`: numeric method receiver is applicable but lacks a direct fixture. |
| `self/self_host/test.cljs`, `test-eval-str-with-require` (843 onward), first require/qualified-call assertion; `test-cljs-2356` (1320 onward) | Minimal require/qualified access maps to namespace acceptance/identity tests; circular `circular.a`/`circular.b` error maps to `namespaces::cycles_report_the_dependency_path`. Bootstrap loader APIs/runtime integration are excluded. |
| `self/self_host/test.cljs`, `test-cljs-2367` (1372 onward), nonmacro `(def b (def a 3))` source | Nested initialized def result is an applicable direct-case gap; `def-value-context` is related but nests def in let instead. Macro-loading half is excluded. |
| `self/self_host/test.cljs`, `test-analyze-str`, `test-eval-str`, `test-CLJS-1612`, `test-cljs-1854`, `test-self-host-self-require` | Self-host state/APIs, Var-mode definition results, syntax quote/macros, reload/const replacement, namespace exposure as host objects and self-require reload are excluded. A require/as header inside a macro test alone is not alias-runtime evidence. |
| `self/self_parity/test.cljs`, complete `run-tests` body (263–375) | Runs core/primitives/specials/static suites; `cljs.ns-test` is reader-discarded in both lists. This is orchestration evidence, not additional independent semantic coverage. |

Other bodies explicitly inspected and excluded in `core_test.cljs` were
`unchecked-get-test`, `js-invoke-test`, `memfn-test`, `test-383`, `test-513`,
`test-reify-meta`, `test-letfn-shadowing`, `test-493`, `test-481`, `test-495`,
`test-2764`, `test-541`, `test-const-emission`, `test-cljs-2741`,
`test-cljs-3452`, `test-cljs-3472`, `test-static-props-methods`,
`test-new-method`, and `test-instance-method-new`: their assertions depend on
core/runtime functions, mutable/metadata-bearing objects, letfn, collections,
arity machinery, constants, mutation, or static-member/new syntax. No coverage
is inferred from a supporting literal or ordinary call in those bodies.

Other bodies explicitly inspected and excluded in `primitives_test.cljs` were
`test-js-primitives`, `test-equiv`, `test-arithmetic`, `test-apply`,
`test-booleans`, `test-try-catch`, `test-misc`, `test-496`, `test-717`,
`test-1556`, and `test-char?`: they assert JS-object/array or collection runtime,
equality/arithmetic/coercion, apply/variadic/sequence behavior, core boolean/char
predicates, exceptions, atoms, mutation or JS literal emission. In particular,
`test-booleans` is predicate coverage, not bare `if` truthiness coverage.

### Applicable gaps and limits of current evidence

These are findings about the inspected assertions and current coverage, not a
second task queue. Except for the source-inferred validation difference, they
are absent direct fixture shapes, not observed compiler failures.

| Source anchor | Missing direct coverage / observed limit |
| --- | --- |
| `analyzer_tests/spec-validation` (175–180) | Upstream rejects multiple `:require` clauses. `src/compiler.rs` currently loops over clauses without a repeated-clause guard, and local namespace tests do not cover the shape. Source inspection indicates a compatibility difference when both libraries exist; no new runtime probe was executed in this audit. It is an applicable validation gap, not a macro/runtime exclusion. |
| `compiler_tests/test-3368-global-shadowing` (376–383) | Binding a local from the same-named host root, e.g. `(let* [Math js/Math] (.sqrt Math 16))`. Current host-shadow and initializer tests cover these concerns separately. |
| `analyzer/spec_tests/test-let` (164), `test-fn` (96) | Nonempty bindings/parameters with an empty body. Existing `empty-let` and `empty-function` have empty vectors; effects in the former shapes are not directly asserted. |
| `analyzer_tests/test-cljs-1346` (548–564) | Nested ns rejection. Standalone-ns rejection and first-header loader tests are indirect; no exact nested def/fn diagnostic fixture exists. |
| `core_test/test-cljs-1536` (1262–1274) | A def initialized from a same-spelled lexical binding, e.g. `(let* [x 2] (def x x)) x`; current global/lexical shadow tests do not use this initializer shape. |
| `core_test/test-580` and preceding defs (767–771) | Declaration then assignment with an intervening closure reading that global. Existing declaration, self-reference and redefinition cases cover the components, not this combination. |
| `self_host/test-CLJS-1330` (657), `test-cljs-2367` (1372) | Number-literal method receiver and a def nested inside an initialized def. String/object receivers and def-in-let values are covered. |
| `cljs/static/core_test.cljs` namespace purpose; `primitives_test/test-dot` (659–678) | Reserved namespace component; property read of a computed method result; a two-argument string method. Related name-isolation/nested/generic-argument cases exist, but no exact local forms. |

Ordinary global/function redefinition observed by an existing closure is covered
by `global-redefinition`, `global-function-redefinition`,
`global-capture-versus-local` and the namespace oracle. No direct matching
assertion was found in the inspected upstream bodies; those cases retain their
independent hand-authored provenance. Similarly, scalar spelling/UTF-16 breadth,
whole-file/CLI behavior, dependency initialization order, and getter-backed
receiver/effect traces have useful local evidence without an upstream test-port
claim. Full runtime-failure categories and broader JavaScript/namespace surfaces
remain outside this bounded inventory.

## Provenance format and validation

The sidecar is UTF-8 TSV, with `#` comment lines and exactly nine nonempty fields:

```text
local_path  local_ids  origin  repository  commit  source_path  anchor  rationale  behavior
```

The separators are actual tabs. `local_ids` is a comma-separated list of explicit
stable IDs; grouping is allowed only when the remaining metadata accurately
applies to every listed case. An execution ID is unique across
`tests/fixtures/execution.tsv` and `tests/fixtures/execution-outcomes.tsv` and
must not be renamed just because its position changes. A Rust regression uses its named `#[test]` function as its ID. The pair
`local_path` + ID identifies a fixture in the sidecar.

- `hand-authored`: an independent example. The four upstream-source fields are
  `-`; record its local rationale and asserted behavior. Reading upstream to
  check an independently written example does not change this origin.
- `imported`: a verbatim upstream regression body within local test plumbing.
  Record the HTTPS repository, full 40-digit commit, repository-relative source
  path, named test plus assertion anchor, any harness differences, and behavior.
- `adapted`: the same pinned source fields, with the exact reduction/substitution
  and what it preserves in `rationale`. Name an assertion or subsection when a
  large upstream test mixes applicable and unsupported features.
- `generated`: the same pinned source fields for the seed/specification and
  assertion anchor. `rationale` must also identify a committed generator and its
  exact command/seed/parameters so the case can be regenerated. Name each generated
  output ID; generated files are not upstream verbatim tests.

`cargo test --locked --test provenance` checks column counts, origin categories,
unique/nonempty execution IDs, all execution cases having exactly one provenance
assignment, references to existing local fixtures, and pinned source fields for
all derived origins. It rejects missing metadata and abbreviated revisions. It
uses committed files only, so normal tests need no research checkout. Rationale
faithfulness, generator reproducibility, and attribution still require review;
this structural check does not prove semantic equivalence. Register an additional
local fixture source in that test when adding a new fixture location.

## Repeatable upstream regression procedure

1. Record `git -C research/clojurescript rev-parse HEAD` and check its status.
   Read the complete enclosing test and relevant helpers at that revision, not
   just a search hit. Identify the assertion, its asserted behavior, compiler
   mode, and any supporting definitions. Record dirty source patches separately
   or use a clean pinned checkout; a commit alone must identify the source read.
2. Classify the exact assertion against [the supported subset](../../docs/compiler.md).
   Keep unsupported runtime/collection/macro/optimization assertions as explicit
   boundaries here. A `def`, ordinary redefinition, narrow namespace, or basic
   host-member assertion is no longer excluded merely for using that feature.
3. Reduce only behavior-preserving wrappers. For simple symbol bindings and one
   fixed arity, replacing `let`/`fn` with `let*`/`fn*` can preserve lexical behavior.
   Replace an outer `is (= EXPECTED EXPR)` with `EXPR` and a separately verified
   expected value; replace observation-only harness wrappers with `observe`.
   Document each step. Do not translate destructuring, variadic/multiple methods,
   `loop`/`recur`, `letfn`, `binding`, `with-redefs`, collection operations or core
   arithmetic into a convenient lookalike: that would test different semantics.
   Likewise, removing `^:const` cannot establish constant-folding conformance.
4. Add a descriptive stable ID and the executable source/value/trace row, then
   its source/provenance record. Use a separate record whenever the behavior or
   reduction differs. Keep independent probes hand-authored. For copied source,
   retain applicable upstream copyright/EPL attribution in the fixture comments
   or adjacent attribution document; a provenance link does not replace a notice.
   Multi-file namespace regressions keep their source files and a stable named
   test; register their provenance source with the validator.
5. Verify expected values and complete ordered traces with the pinned release,
   not by copying light-cljs output. The harness supplies `observe` and the
   documented host object, resets state per case, and wraps each source in
   `(fn* [observe] ...)`. Use a fresh output directory for a standalone reduction:

   ```sh
   clojure -Srepro -Sdeps '{:deps {org.clojure/clojurescript {:mvn/version "1.12.134"}}}' -M tests/reference/execution.clj release 1.12.134 - OUTPUT cases PATH.tsv
   # From OUTPUT, load the compiled cases and shared result driver:
   node -e 'require("./reference.js"); require("/absolute/path/to/light-cljs/tests/support/execution.js")'
   ```

   For namespace programs use `program SOURCE_ROOT ENTRY_NS` in place of
   `cases PATH.tsv`, then run `node reference.js` from `OUTPUT` and compare exact
   stdout/order. The harness prints and asserts the effective pinned version and
   settings. For runtime failures, add a named row to `execution-outcomes.tsv`
   and compare its failure category, preceding trace, and captured stdout with
   the same runner. A runtime throw is an outcome; a crashed or timed-out runner,
   missing outcome, serialization error, or reference build failure fails the
   test. Never weaken an expectation to match a suspected compiler defect.
6. Run the relevant Rust/Node tests and the differential test after adding or
   changing executable fixtures. The shared commands are:

   ```sh
   cargo test --locked --test provenance --test execution
   cargo test --locked --test execution -- --ignored --nocapture
   ```

   Add the relevant analyzer/namespace test command for those fixture types.
   Record the actual result, version/options, command, and any faithful-reduction
   limitations in this index or a focused evidence file. Missing tools, missing
   outcomes, crashes and timeouts are failures. A research-checkout comparison,
   if separately useful, never replaces the required release-oracle result.

## Verification of this inventory

Only provenance data, this index, and metadata-validation tests were added. The
execution TSV, Rust/Node execution loaders, and pinned release harness remained
byte-for-byte unchanged from implementation commit `482352c`. The normal
provenance tests and existing fast execution tests passed locally; all 105
programs still produced their expected values and traces. The prior successful
release comparison is retained in [milestone evidence](milestone-evidence.md);
no new executable case or semantic change required repeating it for this audit.
No hosted CI result or full upstream-suite conformance is claimed.
