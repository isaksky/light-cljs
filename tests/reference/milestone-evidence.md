# Globals, namespaces, and interop conformance evidence

Local execution date: 2026-10-07 (America/Denver). The original 74 hand-authored
programs already had a successful release-oracle comparison before this work.
This milestone repeated that baseline with explicitly pinned and verified options
**before adding compiler capabilities**. It did not substitute the research
checkout for the Maven release.

## Oracle and tools

- Oracle: Maven `org.clojure/clojurescript` **1.12.134**; checked at runtime with
  `cljs.util/clojurescript-version`.
- Effective fixture analysis settings: `{:optimizations :none, :static-fns false,
  :def-emits-var false}`. The harness explicitly binds `*cljs-static-fns*`, passes
  the optimization/static build options, supplies `:def-emits-var false` in
  `cljs.analyzer/empty-env`, and asserts all three live values while analyzing
  fixture forms. `:def-emits-var` is an analysis-environment setting, not a build
  option. No REPL mode is used.
- Local Clojure CLI: **1.12.4.1618**; Java: **OpenJDK 25.0.2**; Node:
  **v24.18.0**; Rust: **1.99.0**. `-Srepro` excludes user Clojure CLI configuration.
- Fresh temporary output directories, `:cache-analysis false`,
  `:parallel-build false`. Each single-program case has its own generated
  namespace, preventing definitions from leaking between cases.
- CI provisions Clojure CLI **1.12.4.1618**, Node **24.18.0**, Temurin Java **21**,
  stable Rust, and the same Maven release. The installer URL was checked locally
  and returned HTTP 200. Hosted CI has **not** been run from this workspace.

## Commands and observed outcomes

```sh
# Fresh baseline before implementation: 74 programs, zero differences.
cargo test --test execution programs_match_standard_clojurescript_results_and_effects -- --ignored --exact --nocapture

# Required final release conformance: all three tests passed locally.
cargo test --locked --test execution -- --ignored --nocapture

# Fast Rust/Node execution tests: three tests passed locally.
cargo test --test execution -- --skip programs_match_standard_clojurescript_results_and_effects --skip namespace_program_matches_standard_clojurescript_stdout --skip dependency_initialization_matches_standard_clojurescript
```

The implementation worker additionally ran `cargo test --release` successfully,
including all 15 namespace tests and the rejection of uninitialized `def` in
required value positions. `cargo clippy --all-targets -- -D warnings` passed.
The existing reader regressions passed via:

```sh
cargo test --release --test corpus --test parser_corpus -- --ignored --nocapture
```

Both corpus suites passed **296 source files**, **2,834,491 bytes** each. No reader
fixture or existing execution case was removed; the original execution TSV was
also verified byte-for-byte as the prefix of the expanded fixture.

The expanded comparison ran **105 programs** (the unchanged original 74 plus 31
hand-authored milestone probes), comparing normalized returned values and full
ordered effect traces. All matched. The isolated-namespace harness was also
built and executed with the pinned release. The final three-test reference run
took 5.97 seconds locally.

`namespace_program_matches_standard_clojurescript_stdout` compiled the same
`examples/namespaces/demo/{math,main}.cljs` sources with each compiler. Both
produced exactly `4\n` on stdout.

`dependency_initialization_matches_standard_clojurescript` compiled
`tests/fixtures/namespaces/order/{shared,left,right,main}.cljs` with each compiler.
Both produced exactly:

```text
shared
left
right
main
10
20
7
7
40
```

This exercises a shared transitive dependency loaded once, dependency-before-
consumer order, source order of initializers, identical global names in distinct
namespaces, aliases, a passed global function, and reading a redefined global.
The fixtures are hand-authored, not copied upstream tests.

The equivalent command for the current explicit-selection harness is:

```sh
clojure -Srepro -Sdeps '{:deps {org.clojure/clojurescript {:mvn/version "1.12.134"}}}' -M tests/reference/execution.clj release 1.12.134 - OUTPUT
# Optional standalone fixture/program modes:
clojure -Srepro -Sdeps '{:deps {org.clojure/clojurescript {:mvn/version "1.12.134"}}}' -M tests/reference/execution.clj release 1.12.134 - OUTPUT cases PATH.tsv
clojure -Srepro -Sdeps '{:deps {org.clojure/clojurescript {:mvn/version "1.12.134"}}}' -M tests/reference/execution.clj release 1.12.134 - OUTPUT program examples/namespaces demo.main
```

`OUTPUT` must be a fresh writable directory path. Run its generated launcher
with that directory as Node's working directory. The test automates both steps.

## Observed semantics and deliberate boundaries

- `(def answer 10) (def read-answer (fn* [] answer)) (def answer 20)
  (read-answer)` returns **20**. A function that calls a global function observes
  its later redefinition. Global self-recursion works without a named local fn.
- An initialized `def` returns its initializer value, including when used as a
  `let*` initializer or an observed argument. Initializer effects run once and
  in order. These behaviors were compared in the 105-case run.
- A fresh uninitialized definition is read as **undefined**. An uninitialized
  redefinition leaves an existing value unchanged. A tail uninitialized `def`
  returns **undefined**, including after initialization.
- A separate pinned-release probe of `(let* [x (def a)] x)` produced invalid JS
  (`var x = ;`), which Node rejected with `SyntaxError`. The supported subset
  rejects uninitialized `def` in required value positions; statement and tail
  uses remain supported. This restriction is explicit, not a dropped test.
- Getter/method order is tested with a host object that records access and uses
  `this.value`. `(.method (.receiver js/lightHost) (observe "a") (observe "b"))`
  returns **40** with trace `["receiver","method-get","a","b",["call","a","b"]]`.
  `js/lightHost.path.method` records `"path"` before `"method-get"` and arguments.
- Missing host properties return **undefined**. A separate manual probe of
  `js/lightCljsMissingHost` produced **ReferenceError** in both compilers. This
  is focused milestone evidence; a general runtime-failure protocol remains
  outside this milestone.
- The normal macro/runtime, collection, constructor, mutation, package-import,
  and broader namespace features listed in `docs/compiler.md` remain excluded.
  No mismatch remains among the supported cases executed here.

For the missing-root probe, a one-row TSV with source `js/lightCljsMissingHost`
was passed through the `cases` command above; Node loaded the launcher and invoked
`globalThis.lightCases[0](() => {})` inside `try/catch`. The native probe compiled
that same source via `cargo run --quiet -- --compile --text
'js/lightCljsMissingHost'` and evaluated its output inside `try/catch`.

## Failure enforcement

`.github/workflows/conformance.yml` separates fast tests from the unconditional
**Required ClojureScript 1.12.134 conformance** job on push and pull request. Its
command explicitly runs the ignored reference tests; tool absence is never a
skip. Node has a 30-second bound, reference compilation a 180-second default
bound (`LIGHT_CLJS_REFERENCE_TIMEOUT_SECONDS`), and the CI job a 15-minute bound.
Failed tests retain their temporary reference artifacts and print the path.
Success requires verified settings, a generated launcher, the expected number
of outcomes, matching values/traces, and exact namespace stdout.

Failure injection was executed locally against
`programs_match_standard_clojurescript_results_and_effects` with `CLOJURE` set to:

| Substitute executable | Observed required failure |
| --- | --- |
| `/nonexistent/light-cljs-clojure` | `required tool unavailable` |
| `/usr/bin/false` | `required tool failed` |
| A script containing `exec sleep 20`, timeout set to `1` | `required tool timed out` |
| A script printing the expected settings marker but emitting no files | `reference compiler produced no launcher` |

Each run exited nonzero. The shared Node driver additionally rejects missing or
empty `lightCases`; Rust verifies exact case counts. No hosted CI success is
claimed.

## Inspected upstream sources and provenance

Repository: `https://github.com/clojure/clojurescript`; local research commit:
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. These sources informed hypotheses;
all runtime conclusions above were verified with Maven 1.12.134. The initial
74 and added 31 TSV cases are hand-authored. No test source was copied or claimed
as a verbatim upstream case.

| Source path and stable anchor | Inspection and local coverage/boundary |
| --- | --- |
| `src/main/clojure/cljs/compiler.cljc`, `emit* :def` | Emits initialized assignment values; omits uninitialized definitions in ordinary mode. Covered by `def-value`, `def-value-context`, `def-uninitialized-value`, and both uninitialized-redefinition cases. |
| `src/main/clojure/cljs/compiler.cljc`, `emit-dot`, `emit* :invoke` | Direct host method/path calls preserve receivers; field access precedes arguments. Covered by `js-method-receiver`, `js-path-receiver`, `js-path-order`, `js-property-once`. |
| `src/main/clojure/cljs/compiler.cljc`, `emit-source`; `src/main/clojure/cljs/analyzer.cljc`, `empty-env` | Identified the effective static-fns binding and analysis environment used by the harness assertions. |
| `src/test/clojure/cljs/analyzer_tests.clj`, `test-cljs-1536` | REPL `:def-emits-var true` AST expectations inspected; intentionally excluded because this milestone uses ordinary compilation. |
| `src/test/clojure/cljs/analyzer_tests.clj`, `test-cljs-3320` | `js` alias is reserved; namespace loader rejects that alias. No warning-text assertion copied. |
| `src/test/clojure/cljs/compiler_tests.clj`, `test-3368-global-shadowing` | Emission-level JS-global/local shadowing assertion inspected; hand-authored executable `js-local-shadow`, `js-global-distinct`, and `js-emitter-name-collision` cover the narrow subset. |
| `src/test/clojure/cljs/compiler_tests.clj`, `test-optimized-invoke-emit` | Host-call receiver emission inspected; static/variadic optimized invocation assertions are outside the selected mode and subset. |
| `src/test/cljs/cljs/primitives_test.cljs`, `test-dot` | Property length and direct/nested string-method assertions inspected. Independently authored `js-property`, `js-method`, `js-nested-method` exercise these operation classes. Expanded dot syntax, `memfn`, and `str` cases remain unsupported. |

This is a bounded milestone inspection, not an upstream-suite coverage claim.
