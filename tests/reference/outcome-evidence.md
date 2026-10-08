# Runtime outcome protocol and verification

Local execution: **2026-10-08**, America/Denver. Maven ClojureScript **1.12.134**
remains the required reference; no hosted CI run is claimed. The supplemental
clean source checkout was **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e** (reported
compiler version `0.0.135109660`). Both verified effective settings
`{:optimizations :none, :static-fns false, :def-emits-var false}`.

## Protocol and boundaries

Both compilers expose independent case functions and `[id, source]` metadata to
`tests/support/execution.js`. Every synchronous case emits exactly one JSON line:

```json
{"id":"after-failure","outcome":{"status":"returned","value":42},"trace":[42],"stdout":"after failure\n"}
```

A thrown exception replaces the outcome with, for example,
`{"status":"threw","error":{"category":"type-error"}}`. Built-in
TypeError, RangeError, ReferenceError, SyntaxError, URIError, EvalError, and Error
instances map to stable categories, without their engine-specific messages,
names, or stacks. Other thrown values use `thrown-value` with an encoded `value`;
throwing undefined is distinct from returning undefined or throwing null.
Serialization and runner errors remain process failures, outside the catch that
captures a compiled case's exception.

Encoding preserves ordinary JSON scalars, with these explicit exceptions:

| JavaScript value | Encoding |
| --- | --- |
| null / source nil | `null` |
| undefined | `{"$value":"undefined"}` |
| negative zero | `{"$number":"-0"}` |
| NaN | `{"$number":"NaN"}` |
| positive infinity | `{"$number":"+Infinity"}` |
| negative infinity | `{"$number":"-Infinity"}` |

Encoding recurses through arrays and plain host objects in results and traces.
Plain objects receive a `$object` wrapper so their keys cannot impersonate a
special-value tag. Unsupported types and cycles fail instead of silently
vanishing or becoming null. This is test transport, not source collection or
runtime-library support.

The runner resets host state and trace per case. It captures synchronous
`process.stdout.write`, including `console.log`, restores the stream even after
a throw, and joins byte chunks before UTF-8 decoding. Effects and stdout before
an exception remain in that case's outcome; execution continues with the next
case. Async programs and arbitrary host runtime objects are outside this
bounded harness.

Node has a **30-second** process timeout. File-backed stdin/stdout/stderr avoid
unbounded pipe-reader joins, and `CASE START` / `CASE END` stderr markers retain
the active case's ID/source on a crash or hang. Failure artifacts include the
exact `input.js`, `invocation.txt`, and partial `stdout.txt` / `stderr.txt` in a
reported temporary directory. Reference builds retain generated source with
case-ID comments, compiler diagnostics, and their existing reproduction files.
The reference build timeout remains **180 seconds** by default, configurable
with `LIGHT_CLJS_REFERENCE_TIMEOUT_SECONDS`.

Rust validates the complete ordered sequence of case IDs before comparisons.
Missing, duplicated, reordered, or additional outcomes fail, even when a runner
exits zero. No failed compilation, process crash, timeout, or serializer failure
can supply a successful comparison.

## Fixtures and independent probes

The original **105** rows in `tests/fixtures/execution.tsv` are unchanged,
including their expected values/traces. `execution-outcomes.tsv` adds **17**
hand-authored cases with five columns: stable ID, source, outcome JSON, trace
JSON, stdout JSON. `fixture-origins.tsv` records their independent origin; these
are not imported upstream assertions. Provenance validation covers both files
and requires globally unique IDs.

The new source cases use supported `observe`, local calls, property/method
access, and JavaScript host paths. Host callbacks supply null, undefined, -0,
NaN, infinities, and thrown errors; unsupported source literals and try/throw
forms remain unsupported. Cases assert nested special-value traces, ordinary
stdout, effects/stdout before failure, native error categories, thrown null and
undefined, and successful execution after the final throw.

The nil-callee probe confirms an easily missed order detail: `(f (observe
"argument"))` with local `f` bound to nil fails while accessing `.call`, before
observing the argument. Both standard compiler paths and light-cljs produced
only the earlier `"before"` trace. This expectation was checked against the
release oracle before recording it.

Four additional fast Rust tests exercise the harness independently of either
compiler:

- Literal expected protocol bytes cover all special values in nested results
  and traces, object tag collisions, thrown undefined, preserved stdout,
  split UTF-8 bytes, host-state reset, and continuation after failure.
- Empty/mismatched/duplicate cases, unsupported function/symbol values, and a
  cyclic result fail without producing an outcome.
- Direct Node exit and an infinite loop fail with their case ID/source and
  retained input. The injected hang uses a 2-second test timeout.
- Missing, duplicate, reordered, and extra outcomes fail. A Node program that
  exits zero without reporting its case also fails with the case ID/source.

## Commands and observed results

Tools: Node **v24.18.0**, Rust **1.99.0**, Clojure CLI **1.12.4.1618**,
effective Clojure **1.12.4**, and OpenJDK **25.0.2**.

```sh
cargo test --locked --test execution --test provenance
cargo clippy --all-targets -- -D warnings

LIGHT_CLJS_REFERENCE=release cargo test --locked --test execution \
  programs_match_standard_clojurescript_results_and_effects -- --ignored --exact --nocapture

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution -- --ignored --nocapture
```

Observed locally: **8 fast execution tests and 2 provenance tests passed**;
Clippy passed with warnings denied. The focused release comparison matched all
**122** case outcomes/traces/stdout in **3.80 seconds**. The combined command
passed all **3** oracle tests in **14.35 seconds**: all 122 cases, exact `4\n`
namespace acceptance stdout, and the existing diamond-dependency initialization
stdout. Release and checkout agreed before comparison with light-cljs.

A separate failure injection supplied a temporary TSV row
`reference-invalid-source` with malformed source `(fn* [)`, using a `CLOJURE`
wrapper that appended `cases PATH.tsv` to the normal reference invocation.
The exact ignored case-comparison test exited **101** with `required tool
failed`; the generated case source retained that ID and source, and
`build.stderr` retained the compiler diagnostic. This was a failure-enforcement
check, not a conformance success.

For a standalone build, omitting `cases TSV` compiles both committed files;
specifying `cases PATH.tsv` compiles just that file with the same outcome
protocol. Load the generated `reference.js` from its output directory, then
load the repository's `tests/support/execution.js` as in the existing reference
documentation.

No compiler, reader, language-surface, or CI policy change was needed. The
existing reader/corpus evidence remains in `milestone-evidence.md`; this work
reran the affected execution, provenance, and reference paths rather than
claiming a new whole-upstream-suite result.
