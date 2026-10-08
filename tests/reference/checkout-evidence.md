# Pinned source-checkout oracle evidence

Local execution: 2026-10-07, America/Denver. This is supplemental evidence;
Maven ClojureScript **1.12.134** remains the required CI reference. No hosted CI
run is claimed.

## Reference identities and inspected build conventions

The clean `research/clojurescript` checkout is from
`https://github.com/clojure/clojurescript` at
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. Its reported synthetic compiler
version is **0.0.135109660**; the full Git commit identifies this oracle,
not that synthetic version or an assumed release equivalence.

Inspected at that commit:

- `deps.edn` provides `src/main/clojure`, `src/main/cljs`, and `resources`.
  Its dependencies include Closure Compiler `v20250820`, Closure Library
  `0.0-20250515-f04e4c0e`, tools.reader `1.3.6`, and transit-java `1.0.362`.
  Clojure CLI resolves these through `org.clojure/clojurescript {:local/root …}`;
  compiler sources are not overlaid on the Maven ClojureScript jar.
- `src/main/clojure/cljs/build/api.clj` exposes the ordinary `build` API used by
  both paths. `src/test/clojure/cljs/build_api_tests.clj` uses compiler/analyzer
  environments with `clojure.test`, while `src/test/clojure/cljs/test_runner.clj`
  returns a failing process status on test failures/errors.
- The `:compiler.test` / `:compiler.test.run` aliases add upstream test roots
  and the JVM runner. `:runtime.test.build` uses `resources/test.edn`, whose
  advanced optimization, npm dependencies, and parallel build are outside this
  oracle configuration. This work does not run or claim the upstream test suite.
- `src/main/clojure/cljs/util.cljc`, `synthetic-clojurescript-version`, explains
  why a source checkout reports a source-derived version rather than the Maven
  release number.

Both paths verify effective `{:optimizations :none, :static-fns false,
:def-emits-var false}` during fixture analysis. The last setting is installed
in `cljs.analyzer/empty-env`, not passed as an ineffective build option. Both
use ordinary compilation, fresh outputs, no analysis cache, and no parallel
build. Namespace fixtures check settings throughout their source namespaces.
The checkout path verifies its Git root, full revision, clean status, and loaded
build API, analyzer, compiler, and core runtime source locations before building,
then repeats the identity check after building.

The reference harness generates namespace wrappers and uses `array`, `set!`,
and the standard runtime to expose test functions to Node. These are reference
helpers, not additional source features implemented by light-cljs. Both oracles
compile the same supported forms, with the same `observe` and JavaScript host
helpers. General macros and runtime integration remain excluded.

## Reproduction and observed results

Tools: Clojure CLI **1.12.4.1618**, effective Clojure **1.12.4**, OpenJDK
**25.0.2**, Node **v24.18.0**, Rust **1.99.0**. Java/Clojure versions come from the
compiler JVM itself. `-Srepro` excludes user Clojure CLI configuration.

```sh
# Required release path; omitted LIGHT_CLJS_REFERENCE also selects release.
LIGHT_CLJS_REFERENCE=release cargo test --locked --test execution -- --ignored --nocapture

# Both oracles, compared to one another before checking light-cljs.
LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution -- --ignored --nocapture
```

The combined command passed all **three** tests locally in **14.23 seconds**:

| Comparison | Release versus checkout | light-cljs versus each |
| --- | --- | --- |
| All 105 `execution.tsv` cases, including def/global/interop probes | Identical values and ordered effect traces | Identical |
| `examples/namespaces`, entry `demo.main` | Exactly `4\n` | Identical |
| `tests/fixtures/namespaces`, entry `order.main` | Exactly `shared\nleft\nright\nmain\n10\n20\n7\n7\n40\n` | Identical |

No supported-case discrepancy was observed. `both` fails with an explicit
`UPSTREAM DISCREPANCY` if its two reference outputs differ, before using either
as a light-cljs expectation. It preserves both outputs and builds for diagnosis.
`LIGHT_CLJS_REFERENCE=checkout` runs just the explicitly selected checkout for
focused investigations; this never counts as the required release comparison.
The CI job explicitly selects `release`.

Standalone compiler invocations, from the repository root:

```sh
clojure -Srepro -Sdeps '{:deps {org.clojure/clojurescript {:mvn/version "1.12.134"}}}' \
  -M tests/reference/execution.clj release 1.12.134 - OUTPUT cases tests/fixtures/execution.tsv

checkout="$PWD/research/clojurescript"
revision=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e
clojure -Srepro -Sdeps "{:deps {org.clojure/clojurescript {:local/root \"$checkout\"}}}" \
  -M tests/reference/execution.clj checkout "$revision" "$checkout" OUTPUT cases tests/fixtures/execution.tsv
```

Replace `OUTPUT` with a fresh writable absolute directory. Either invocation
also accepts `program examples/namespaces demo.main` or
`program tests/fixtures/namespaces order.main` in place of `cases TSV`.
To execute a program build, use `node reference.js` from `OUTPUT`. To evaluate
the case functions, load `reference.js` there and then evaluate the repository's
`tests/support/execution.js`; the Rust driver automates this and saves that
exact Node script as `driver.js` when a comparison fails.

Each test prints a fully shell-quoted compiler invocation with the actual
output directory, identity, options, and tool versions. All six successful
temporary builds from the combined run were confirmed removed.

## Failure enforcement and diagnostics

`cargo test --test execution` passed four fast tests without launching Clojure
or requiring a checkout. This includes selection validation for missing path,
missing/full revision, unknown selector, and combined reference selection.
`cargo clippy --all-targets -- -D warnings` passed. Existing reader and compiler
sources were unchanged; prior corpus results remain in `milestone-evidence.md`.

Local failure injections selected the single ignored case-comparison test with
`--exact --nocapture`; every run exited **101**, never a passing skip:

| Injection | Observed failure |
| --- | --- |
| Checkout path `/nonexistent/light-cljs-reference` | Required checkout unavailable |
| Full revision of forty zeroes against the real checkout | Checkout revision mismatch |
| Wrapper substitutes Maven release dependency while retaining checkout arguments | Checkout source was not loaded |
| Wrapper supplies nonexistent local dependency jar | Clojure CLI local dependency resolution failure |
| Wrapper appends a TSV containing malformed `(fn* [)` source | Reference build failure with reader diagnostic and generated source retained |
| `CLOJURE=/usr/bin/false` | Required tool failed |
| Launcher executes `sleep 20`, timeout set to `1` second | Required tool timed out |

Failed builds retain `invocation.sh`, `build.stdout`, `build.stderr`, and any
generated `src/` / outputs. Logs write directly to files so a failed launcher
cannot lose diagnostics through buffered pipes. Failures before process launch
still report the failing configuration and retained directory. Once Node is
reached, `node-version.txt` and `driver.js` are also retained; completed execution
adds `results.txt`. Clojure's subprocess agent pool is closed on completion so
Git identity checks do not impose a 60-second JVM shutdown delay.
