# Executable compiler language coverage

This inventory measures source assertions in eighteen explicitly inventoried files
at ClojureScript **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. It is a coverage
matrix, not a task queue or a claim to execute the entire upstream suite.
[README.md](README.md) defines extraction, complete-body retention, stable IDs,
section scope and unrelated dispositions. [manifest.json](manifest.json) gives
the exact inventory counts. Original assertion expressions and expectations are
retained; old passing execution fixtures and invalid-source diagnostics are
unchanged.

## Commands

Run from the repository root with Python 3.10+. Node is needed for generated
JavaScript. Clojure CLI, Java, Maven 1.12.134 dependencies and the pinned clean
research checkout are needed for both references.

```sh
# Source evidence and checked inventory completeness; no test execution claim.
python3 -B tests/reference/compiler-language/extract_inventory.py --check

# Report all assertions, with no cached or fabricated passes.
python3 -B tests/reference/compiler-language/report.py --scope historical-selected --show-missing

# Default reference selection is both; rejection stays visible as missing.
python3 -B tests/reference/compiler-language/report.py --scope historical-selected --run --feature functions-apply

# Inspect a single assertion, retaining its complete upstream body in the inventory.
python3 -B tests/reference/compiler-language/report.py --scope historical-selected --run --strict \
  --id 'core_test.cljs::test-401-411::assertion-001'

# Whole-inventory completion gate. Intentionally exits 1 at this baseline.
python3 -B tests/reference/compiler-language/report.py --scope historical-selected --run --strict \
  --output /tmp/compiler-language.json

# Fast local feedback cannot establish reference conformance.
python3 -B tests/reference/compiler-language/report.py --scope historical-selected --run --reference none \
  --feature functions-apply

# Fast accounting, existing runtime regressions and provenance.
cargo test --locked --test compiler_language --test provenance --test execution
```

`--feature` and `--id` are exact, repeatable filters. Features select the
inventory's primary feature section; each row also retains other relevant
features for navigation. Unknown/empty selections are errors, not zero-test
successes. `--json` emits every selected assertion, its disposition, source
anchor, fixture ID and observed backend results. `--output` saves the same JSON.
`--show-missing` prints stable missing IDs and concrete reasons.

Normal runs build the current compiler and use the executable path returned by
Cargo, including when `CARGO_TARGET_DIR` is configured. `--no-build` explicitly
uses `target/debug/light-cljs`; combine it with `--binary PATH` for another
prebuilt executable. Reports record its path and SHA-256. `NODE` and `CLOJURE`
select executable paths; the fast Rust accounting test also honors `PYTHON`.
`--checkout` selects the reference source directory, while its required revision
remains pinned. There is no automatic checkout/release substitution.

Each execution keeps a fresh temporary artifact directory with invocation
arguments, exact stdin/source, stdout, stderr, reference builds and `report.json`.
`--artifacts PATH` selects a new directory explicitly; existing directories are
rejected to avoid mixing observations. Local layers have individual timeouts
(`--timeout`, default 30 seconds). Reference compilation and Node execution each
use `--reference-timeout` (default 180 seconds).
On POSIX, timeout cleanup kills only the owned process group. File-backed output
preserves partial diagnostics without waiting for pipe-holding descendants.

## Meaning of the result columns

The new [six-column fixture file](../../fixtures/compiler-language.tsv) records
ID, source, expected outcome JSON, effect trace JSON, stdout JSON and an optional
native stdout expectation. Its [witness mappings](witnesses.jsonl) pin the exact
original assertion hash and explain each adaptation. The existing
[provenance sidecar](../fixture-origins.tsv) also covers every new fixture.

Lexer and parser **acceptance** say only that syntax was handled. Native
**passed** means that the native result matched its separately declared
expectation; CLJS-specific host values or protocol contracts can be explicitly
inapplicable to that backend. Neither contributes to compiler coverage.

Generated JavaScript is compiled independently for every witness and executed
with the existing [outcome runner](../../support/execution.js). The same
`{id, outcome, trace, stdout}` protocol and private-runtime inspection adapter
used by `tests/execution.rs` are retained. The release and checkout run the same
sources through [execution.clj](../execution.clj), which verifies the effective
revision and `:optimizations :none`, `:static-fns false`, `:def-emits-var false`
on every build. Returned values, thrown categories, effects and stdout are all
compared to literal expected outcomes, not just to each other.

An assertion is **covered** only when its complete faithful witness passes
generated JavaScript **and both** pinned references in the current invocation.
No witness, omitted backend, compiler rejection, skip, timeout, unavailable
reference, malformed output, duplicate ID, partial process output or mismatch
can increase that count. Outcomes must contain exactly the expected IDs in order
with every protocol field. JSON booleans cannot equal numeric 0/1 accidentally.
Known native successes therefore cannot conceal missing JavaScript support.

**Version difference** is a separate disposition, never a pass. A witness may
declare `version_difference` with an explanation and distinct complete
`release`/`checkout` expected protocol objects. The checkout object must match
the fixture's original expectation. Only successful observations of both
versioned expectations and the generated JavaScript contract establish that
disposition. A missing tool, rejection, or guessed release limitation cannot.
No version-difference witness has been asserted at this baseline; checkout-only
syntax stays missing until someone supplies and executes explicit evidence.

**Unrelated** records concern upstream internals, deliberately excluded library
or build features, or inactive source assertions. Reasons remain per assertion.
They do not contribute to the passing count. Strict completion requires zero
missing selected in-scope assertions; verified version differences remain
separately reported. Focused runs may exit 0 with rejected features visible,
but mismatches, failed processes, invalid protocol, timeouts and unavailable
tools still exit nonzero. `--strict` additionally fails every missing assertion.
A report-only invocation establishes no new passes and normally fails strict
completion until it is executed.

The report also reads the current `analysis-boundaries.tsv` as a separate table.
Valid reference-accepted multiple-arity, variadic and recur rejection rows are
**missing capabilities**. Actual invalid-source errors stay **negative tests**.
Those diagnostic rows are never added to executable semantic passing counts.
Their existing analyzer/API/CLI tests and reference classifications are preserved.

## Baseline observed on 2026-10-09 (America/Denver)

The first executable witness set contains **25 assertion-faithful adaptations**.
It retains whole tested operations: apply and its sequence tail, multi/variadic
arity clauses, loop/recur, mutual letfn and reify protocol wrappers. It does not
replace these with direct calls, maps or other already-supported equivalents.

| Backend | Observed result for the 25 witnesses |
| --- | --- |
| Lexer | 25 accepted |
| Parser | 25 accepted |
| Native interpreter | 22 passed; 3 explicitly inapplicable CLJS host/protocol cases |
| Generated JavaScript | 5 passed; 20 rejected and retained as missing |
| Maven ClojureScript 1.12.134 | 25 matched complete expected outcomes |
| Pinned checkout (reported runtime 0.0.135109660) | 25 matched complete expected outcomes |

The **5 covered** assertion sites are basic equality, zero/three-argument
addition, and two original closure/rebinding assertions. Existing independently
written or reduced probes do not automatically cover an upstream assertion;
the remaining inventory is conservatively missing until faithful evidence is
mapped and executed. See the manifest for total and unrelated counts, and run
the report for current missing counts. These are source assertion sites, not
dynamic `doseq`/property iteration counts and not a whole-suite percentage.

With the final scope dispositions, the **2,213** inventoried source assertion
sites comprise **5 covered, 1,761 missing, 0 verified version differences and
447 unrelated**. All **556** complete top-level test/helper/fixture forms are
retained. Before execution, a report-only invocation correctly gives zero
covered and 1,766 missing. These baseline counts do not replace the live report.

Reproduction of that baseline used:

```sh
python3 -B tests/reference/compiler-language/report.py --scope historical-selected --run --no-build \
  --reference both --output /tmp/light-cljs-language-baseline.json
```

The reference runs verified Maven 1.12.134 and the pinned checkout identity before
and after compilation, with effective settings above, Clojure **1.12.4** and
Java **25.0.2**. The full 25-witness run passed its comparison checks while
reporting the 20 JavaScript rejections; strict completion remains incomplete.
The focused closure command above separately checks that a fully observed
single-assertion selection can pass strict mode.

The fast accounting suite explicitly tests missing/rejected/skipped/timed-out/
reference-unavailable paths, partial/duplicate/reordered output, stdout and
effect mismatches, version-difference separation, invalid selections and the
boundary diagnostic distinction. The Rust integration test, two provenance
tests and 24 existing execution tests passed; eight existing explicit reference
tests stayed ignored in that fast run. The new witnesses were compared to both
references with the command above. Formatting and focused Clippy also passed.

## Updating coverage with a language implementation

Read the original complete body, referenced fixture/helper setup and all relevant
assertions before adding a witness. Preserve the tested language operation,
bindings, effects, expected values and failure behavior. An adaptation that
removes the feature under test must remain missing. Give each new executable
case an independent stable fixture ID and add its traceable provenance, original
assertion hash and rationale to the mapping. Dynamic/helper assertions require
all relevant calling contexts, not just one convenient passing iteration.

Run the selected section and both pinned references. Add independent direct
runtime checks where the shared encoder might conceal a bug. Update this
baseline's historical narrative only by adding dated evidence; the runner
always derives current coverage from current observations. Updating the source
inventory is explicit: adjust justified section classifications in the extractor,
regenerate, and review complete bodies, changed assertions and manifest counts.
`--check` detects omission or source drift; no unsupported implementation should
be reclassified as unrelated to make strict completion pass.

## Reader member — 2026-10-09

Reader lowering adds twelve faithful witnesses from the complete
`primitives_test.cljs/test-regexps` body: assertion 001 and assertions 003–012.
The manifest retains the original hashes and exact bindings/expressions; the
second assertion still requires `js*` and remains missing. The focused strict
run passed all twelve in generated JavaScript, Maven 1.12.134 and the pinned
checkout, with all values, traces and stdout checked. Native results are
explicitly inapplicable for these compiler/JavaScript regex witnesses.

The inventory itself is unchanged and `extract_inventory.py --check` passes.
The original baseline counts above remain historical. The live report derives
current coverage; this focused result is not completion of all reader/metadata
or interop assertions elsewhere in the retained files.

The [reader audit](../compiler-reader.md) records thirteen additional original
Date assertions from `reader_test.cljs`, independent literal/macro/alias/error
probes, numeric reader behavior, verification commands and measured output cost.
Those Date assertions are tracked in `fixture-origins.tsv`; they are outside the
twelve-file inventory and do not inflate its covered count.

## Namespace member — 2026-10-09

`namespace-cases.jsonl` adds an actual namespace-program witness for both
`core_test.cljs/test-cljs-1274` assertions. The original namespace, qualified
`def`, and both equality expressions are retained. The report compiles this
program through the namespace CLI and the checked program reference mode,
including actual exported-function execution and the existing typed result,
trace and stdout comparison. Omitted or invalid build/outcome evidence remains
missing, with dedicated accounting regressions for this program path.

The focused strict run covers both assertions in generated JavaScript, Maven
1.12.134 and the pinned checkout. `ns-refer`, `ns-refer-clojure`, and
`def-same-ns` now appear as accepted local boundaries; those diagnostic rows are
not additional passing semantic assertions. The [namespace audit](../compiler-namespaces.md)
records independent CLJC/macro/metadata/alias probes, exact commands, source
precedence, scope boundaries and the zero-byte primitive output change.

## Numeric and predicate member — 2026-10-09

The [numeric audit](../compiler-numeric.md) records 217 additional faithful
witnesses and the strict **220 covered, 0 missing** result for its selected
arithmetic, bit, boolean, character, signed-zero, object and NaN assertions.
Every comparison runs generated JavaScript plus both pinned references with
full outcomes and checked settings. The original bit-xor assertion shape is
preserved; separate independent probes execute its operations. Five arithmetic
assertions requiring map/loop/for remain unclaimed in the source inventory.

The same audit records 56 independently expected/table-derived outcome cases,
all applicable predicate table rows, direct Node assertions, 10 new arity
boundaries and runtime size/startup changes. Closure Integer/Long rows are
external-library scope. The direct comparison macro/function evaluation-order
distinction and the two pre-existing full-suite release/checkout set-literal
differences are stated explicitly; neither becomes a fabricated passing count.

## Function arity member — 2026-10-09

The [function audit](../compiler-functions.md) adds 38 faithful witnesses to the
11 existing multiple/variadic/apply witnesses. The focused strict comparison
reports **49 covered, 0 missing**, with all values, errors, effects and stdout
checked in generated JavaScript, Maven 1.12.134 and the pinned checkout.
The original source inventory and body hashes are unchanged. This is the exact
selected arity witness set, not completion of the entire functions/apply section;
infinite-source library and protocol helper contexts remain separately visible.
The `fn-multiple`, `fn-variadic` and repeated-parameter boundaries are accepted.
Invalid overload declarations remain negative diagnostics with the reference
warning/emission stages recorded explicitly.

## Apply and invocation completion (2026-10-09)

The [apply audit](../compiler-apply.md) maps 94 additional assertion sites. Its
selected set contains 143 assertions covering complete active apply tests, both
primitive apply/combinator tests, and nine core invocation regression bodies.
It retains infinite argument tails, every doseq function binding, actual
metadata/IFn calls, receiver identity, and full expected outcomes. Another 228
independent probes remain separately counted. The strict pinned comparison
command and explicit adaptation rationale are in that audit; this section does
not substitute saved counts for a current report.

## Loop and recurrence member — 2026-10-09

The [recurrence audit](../compiler-recur.md) records **7/7** strict imported
assertion witnesses, **53** independent value/effect/stdout comparisons, and
**153** matching analyzer-boundary classifications against both pinned references.
Targets are stack-safe and independent; lexical captures, simultaneous argument
rebinding, variadic rest identity and protocol/IFn receiver conventions execute
in generated JavaScript. The original record-wrapper assertions remain missing
until their actual record contexts execute; reify receiver probes are explicitly
independent. Local mutual recursion and unrelated helper/library contexts retain
their existing inventory status.


## Letfn coverage, 2026-10-09

The executable matrix now maps all eight `letfn_test.cljs::test-letfn` assertions
and `core_test.cljs::test-letfn-shadowing::assertion-001`. The strict selected
report has **9 covered, 0 missing, 0 version differences** against Maven 1.12.134
and the pinned checkout. Each witness retains the complete function group and
original asserted expression. The separate independent probes and two explicit
upstream slot-reuse differences are recorded in [compiler-letfn.md](../compiler-letfn.md);
they do not inflate upstream assertion counts.

## Destructuring and conditions member — 2026-10-09

The selected ordinary-destructuring matrix now contains **65 covered, 0 missing,
0 version differences**: all 60 original assertion sites before the extended-map
directives in `destructuring_test.cljs`, plus the complete
`core_test.cljs::test-1210` and `test-cljs-3299-trailing-keys` bodies (five sites).
The complete source bodies and supporting definitions were read at
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`. Implementation inspection also covered
`core.cljc::destvec*`, `destructure`, `maybe-destructured`, `fn`, `loop`, and
`assert`, and `core.cljs::nth`, `map`, and `seq-to-map-for-destructuring`.

Sixteen whole-body witnesses retain every original predicate and required helper,
including the actual `First` and `DestructuringWithLocals` deftypes, IFn/Object
methods, protocol definitions, record and reify methods. `testing` becomes `do`;
`is` becomes a throwing assertion; the witness returns true only after all original
assertions ran. Two namespace programs retain actual keyword-alias resolution,
redirecting the original string-library alias consistently to the existing core
namespace alias. Hashes, individual assertion mappings, source paths/anchors and
adaptation rationales are recorded in `witnesses.jsonl`, `namespace-cases.jsonl`,
and `../fixture-origins.tsv`. Unasserted expressions in the original bodies remain
executed but do not add assertion counts. The extended `:keys!`, `:select`,
`:excess` and `:missing` bodies stay visible as missing; they are outside this
member's stated ordinary binding syntax and have not been reclassified.

The compiler supplies the map conversion helper and JavaScript assertions.
Protocol/IFn/Object method declarations pass through the same `fn` expander as
ordinary functions, preserving source subforms and the original recurrence ABI.
The supporting type/record constructor bridge binds receiver fields outside
parameter destructuring; method captures and record/reify metadata copies survive.
This adds the method execution needed by these tests, without claiming completion
of general extension APIs or all record collection operations.

**60 independent probes** have explicit expected outcomes, ordered effects and
stdout. **56 match both pins exactly**; four differences below are independently
verified against exact local, release and checkout expectations. The probes cover
missing versus nil/false keys, eager default expressions, nested/rest/as bindings,
qualified directives, keyword argument sequence conversion, nil/lazy/custom
sequence and indexed inputs, initializer/access counts, destructured loop/function
recurrence, fixed/multiple/variadic signatures, core shadows, captures, metadata,
condition ordering, result binding, exceptions and discarded/value/return contexts.
Long-vector map inputs realize 32-element chunks, including nested mapped chunks;
list inputs realize only the needed prefix. Direct Node assertions independently
check values, Error identity/text and body/message evaluation counts. The later
type member corrects named type/record method maps to ordinary expressions;
reify and IFn condition probes retain their separate historical contract. Native macro
fixtures emit actual checked functions and destructured record methods; a separate
namespace program exercises qualified core declarations/assertions. Malformed
patterns and postcondition recurrence retain usable source diagnostics.

### Explicit observed differences

These are separate probes and are not counted as matching imported assertions.
Both pins run with `:optimizations :none`, `:static-fns false` and
`:def-emits-var false`; identities and effective settings are checked before and
after compilation (Maven **1.12.134**, checkout runtime **0.0.135109660**, Clojure
**1.12.4**, Java **25.0.2**, Node **v24.18.0**).

- `destructure-defaults-order` binds explicit `a`/`b` map keys with effectful
  defaults and an effectful initializer. Local and release traces are `[0,1,2]`;
  checkout gives `[1,2,0]`. All return `[9,2,true]`. Ordinary local `:or` retains
  the release's initializer-first contract.
- `destructure-reify-pre-failure` throws a JavaScript Error locally before its
  body effect. Both upstream pins treat the method's condition map as an ordinary
  discarded expression, return 9, and emit trace `[9]`. Checking method conditions
  is an explicit requested local guarantee. Related method postcondition maps also
  produce upstream undeclared-`%` warnings; successful body returns alone are not
  evidence of upstream assertion checks.
- `destructure-type-fields-capture` and `destructure-record-fields-capture`
  originally retained definition-time lexical captures locally. The type member
  now matches both pins: named types resolve `capture` as a namespace Var,
  returning NaN and `[NaN,NaN]`. Those probes moved into the matching differential
  set; ordinary receiver-field and reify-capture probes remain covered.

### Verification

```sh
cargo test --locked --test analyzer --test boundaries --test cli \
  --test compiler_macros --test compiler_reader --test execution \
  --test namespaces --test provenance --test compiler_language \
  --test interpreter_bindings --test interpreter_functions
node tests/support/js_runtime.test.js
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution destructuring -- --ignored --nocapture

python3 - <<'PY'
import json, subprocess
from pathlib import Path
rows = map(json.loads, Path('tests/reference/compiler-language/witnesses.jsonl').read_text().splitlines())
command = ['python3', '-B', 'tests/reference/compiler-language/report.py', '--scope', 'historical-selected',
           '--run', '--strict', '--reference', 'both']
for row in rows:
    if row['fixture_id'].startswith('cljs-destructure-'):
        command += ['--id', row['assertion_id']]
raise SystemExit(subprocess.call(command))
PY
```

The strict 65-site report passes every selected JavaScript, release and checkout
outcome. Original inventory bytes remain unchanged. The focused Rust run passes
**172 tests**, with **18** reference tests explicitly ignored in that invocation;
the two selected pinned comparisons above pass separately. Node runtime assertions,
all-target Clippy, formatting, inventory regeneration and whitespace checks pass.
Native-only results are not used to claim these compiler passes.

### Output and initialization observation

Primitive `1` remains **103 bytes**, and `(fn* [x] x)` remains **137 bytes**,
without the collection runtime. `(assert true)` is **191 bytes**, also without
that runtime. The shared runtime grows from **54,045** to **59,124 bytes**
(+5,079, about 9.4%); deterministic gzip grows from **12,806** to **13,979 bytes**
(+1,173, about 9.2%). This baseline retains all earlier workspace runtime changes;
only this member's edits are compared.

On this Mac with Node v24.18.0, precompiled `vm.Script` factory evaluation plus
one checked vector/nth operation had median **71 microseconds before** and
**67 microseconds after** (seven batches of 1,000 after 100 warmups; ranges
66–73 and 65–70 microseconds). The overlapping local observations show no
material initialization regression; they are not browser performance claims.

## Exceptions member — 2026-10-09

The selected exception matrix contains **42 covered, 0 missing, 0 version
differences**: all eight assertion sites in
`primitives_test.cljs::test-try-catch`, all seven in
`core_test.cljs::test-data-conveying-exceptions`, and all 27 in
`core_test.cljs::test-Throwable->map`. Full original bodies and implementation
sources were read at `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` before adaptation.
The fifteen witnesses retain exception operations and literal expected results.
For the two shared finally-state assertions only, a host array/push/at adapter
replaces atom/reset!/deref storage, preserving the cleanup side effect and the
pending return. This is exception coverage, not atom coverage. Throwable->map's
whole-body witness executes all three original chains and every assertion,
including the original syntax-quoted ExceptionInfo identity.
`witnesses.jsonl` and `../fixture-origins.tsv` retain exact assertion hashes,
source/test anchors, fixture IDs and adaptation reasons. Other inventory sites
that need unrelated collection/library functionality remain visible as missing.

Implementation evidence includes `analyzer.cljc::parse throw/try`,
`compiler.cljc::emit* :throw/:try`, `core.cljc::instance?`, and
`core.cljs::ExceptionInfo`, `ex-info`, `ex-data`, `ex-message`, `ex-cause`,
`pr-writer-ex-info`, `Throwable->map` and the generic Error printer. The analyzer
`analyze-ops` try/throw assertions (lines 721–734 and its throw section) were read
as validation evidence, not counted as executable AST coverage. The complete
`test-cljs-3473` body was inspected; its nested throw expression is independently
executed without claiming its upstream warning-inference assertions. The
`core_test.cljs::test-atoms` validator identity regression informed an actual
protocol-method throw/catch probe retaining the same exception, data and cause;
no native atom result is claimed as compiler support.

**56 independent probes** match complete literal outcomes, ordered effects and
stdout in generated JavaScript and both references. They cover success, discard,
value and return contexts; empty clauses; typed dispatch and class expressions;
unmatched throws, rethrow and nested try; throwing catches/finally; callbacks and
initializers; nested recurrence; captured catch bindings; arbitrary nil,
undefined, false, zero and string throws; constructor and helper arities; saved
and aliased helpers; method/class identity; printing and cause-chain data.
Independent Node checks compare exact object/data/cause identity and primitive
throw outcomes without the shared serializer. Its explicit completion flag was
already correct; new compiled cases ensure falsy throws never become successes
and retain preceding stdout and effects in local and reference runs.

This work also fixes macro-generated `:default` being tested as a keyword class,
clause-free single-value try introducing an extra arguments scope, and compiler
syntax quoting falling back to the caller namespace for ExceptionInfo. Native
macro and namespace tests execute those paths. Direct instance? macros read a
symbolic class after the value, matching the pinned macro; expression classes and
saved function calls retain ordinary left-to-right argument evaluation.

The nine new malformed-form/recurrence boundaries match both references; the
complete boundary run matches **162 classifications**. Direct analyzer tests
also cover empty/missing bindings, duplicate/default catch ordering and nested
valid targets. Existing invalid-source classifications remain separate from
semantic assertion counts.

### Verification

```sh
cargo test --locked --test analyzer --test boundaries --test cli \
  --test compiler_macros --test compiler_reader --test execution \
  --test namespaces --test provenance --test compiler_language
node tests/support/js_runtime.test.js
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution compiled_exceptions_match -- --ignored --nocapture

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution analyzer_boundary_classifications_match_reference -- --ignored --nocapture

python3 - <<'PY'
import json, subprocess
from pathlib import Path
rows = map(json.loads, Path('tests/reference/compiler-language/witnesses.jsonl').read_text().splitlines())
command = ['python3', '-B', 'tests/reference/compiler-language/report.py', '--scope', 'historical-selected',
           '--run', '--strict', '--reference', 'both']
for row in rows:
    if row['fixture_id'].startswith('cljs-exceptions-'):
        command += ['--id', row['assertion_id']]
raise SystemExit(subprocess.call(command))
PY
```

The focused Rust run passes **156 tests**, with **19** explicitly ignored
reference tests; the two selected pinned comparisons and strict 42-site matrix
pass separately. Node runtime assertions, Clippy, formatting, inventory checking
and whitespace checks pass. Each reference build verifies Maven **1.12.134** or
checkout **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e** (runtime
**0.0.135109660**) before and after compilation, plus `:optimizations :none`,
`:static-fns false`, `:def-emits-var false`, Clojure **1.12.4**, Java **25.0.2**
and Node **v24.18.0**. Arity/redefinition warnings remain explicit diagnostics;
their actual runtime effects and errors match both references.

### Output and initialization observation

Primitive `1` remains **103 bytes**, and `(fn* [x] x)` remains **137 bytes**.
`(try (throw 1) (catch :default e e))` is **228 bytes** with no runtime prelude.
Against the immediately preceding dirty-workspace baseline, the shared runtime
grows from **59,124** to **61,844 bytes** (+2,720, about 4.6%); deterministic gzip
grows from **13,979** to **14,710 bytes** (+731, about 5.2%).

On this Mac with Node v24.18.0, precompiled `vm.Script` factory evaluation plus
one vector/nth operation had medians **75 microseconds before** and
**79 microseconds after**, across seven batches of 1,000 after 100 warmups.
Ranges were **67–83** and **69–92 microseconds** under concurrent test load.
These noisy local observations overlap; they do not establish a browser startup
performance claim. Runtime inclusion remains conditional and occurs once per
compilation unit.

## Control and iteration member — 2026-10-10

The [control audit](../compiler-control.md) records **30** strict passing original
assertions for case, threading and the if-some/const regression, **38** independent
pinned probes and **174** matching analyzer boundary classifications. It documents
nil-aware bindings, condp arrows, case constants and collision behavior, switch
emission, nil-returning iteration with fresh captures, and assertion configuration.
Source macros, core aliases/renames and compile-time branch effects have direct
namespace coverage. Both reference identities and effective settings are checked.

The separately recorded original atom-based dotimes assertion and the third
as-> assertion using reverse remain missing, pending their mutable-state/sequence
helpers. They retain original operations and are explicitly omitted from the
30-site strict selection. Independent actual dotimes and as-> executions cover
this member's control behavior. No native-only pass or host replacement is counted
as imported coverage; the final full-inventory gate still includes those sites.

## Runtime metadata completion

[The metadata audit](../compiler-metadata.md) covers the complete metadata
interface helper inputs, direct literal/reify metadata, function/apply metadata
and independent protocol/propagation probes. Its strict selected run covers
32 original sites on emitted JavaScript and both pinned references. Remaining
reader, protocol, library and whole-suite assertions retain their own status.

## Atom and volatile completion

[The state audit](../compiler-state.md) maps all **56** assertion sites in the
six selected `core_test.cljs` state bodies. The strict selected run passes in
emitted JavaScript, Maven 1.12.134 and the pinned checkout. Whole-body witnesses
retain every original swap arity, shared mutation sequence and custom
`IDeref`/`ISwap`/`IReset` implementation. **41** independent value/effect/stdout
probes and a two-namespace program compare against both references; direct Node
checks separately verify identity, reentry and large argument tails. Both
references confirm structural compare-and-set, despite the original task's
identity wording. Dynamic bindings and other mutable-state sites keep their
separate live inventory status.

## Definitions, Vars and binding scopes

[The definitions audit](../compiler-definitions.md) records 24 original assertion
sites from complete binding/Var namespaces and the selected core Var, resolve and
defonce regressions. It includes original print/with-out-str/case/apply execution,
metadata test callbacks, cross-namespace temporary roots, initializer failure
semantics and synchronous Promise restoration. `var_test.cljs` is now retained as
the twelfth full source file; the four new assertion sites keep exact source hashes.
Independent probes, namespace programs, direct Node assertions and separately
counted analyzer boundaries complement the selected strict coverage report.

## JavaScript interop member — 2026-10-10

[The interop audit](../compiler-interop.md) records **86 covered assertion sites**
from eight witnesses: the complete `test-dot`, `test-arrays`, `test-make-array`,
core `aget-test`, `aset-test`, `unchecked-get-test`, `memfn-test`, and the original
qualified-method map assertion with its `:refer-global` namespace. Each selected
site passes generated JavaScript, Maven 1.12.134 and the pinned checkout, with
zero missing sites or version differences in that selection. Eight additional
complete array-access bodies and independent semantic probes have separate
provenance and do not inflate this matrix. Other inventory sections retain their
own live status.

## User protocol completion

The protocol audit adds 23 assertion witnesses: five complete runtime bodies
cover metadata precedence, qualified reify methods, native-object seq/lookup,
slash-named methods and native contains; eleven receiver expressions preserve
`no-warn-on-emit-invoke-protocol-method`'s warning-free compilation requirement.
The latter witnesses set `no_warnings`, and the runner rejects warnings from the
local compiler and either pinned reference. Reference compilation isolates these
cases so warnings from separate deliberately warned probes cannot mask the gate.
They use the batch's `:static-fns false` contract and real protocol declarations
instead of upstream-private analyzer environment injection. The private static
emitter AST is not claimed. See [compiler-protocols.md](../compiler-protocols.md)
for reproduction and scope.


## Compiled reify

[Reify evidence](../compiler-reify.md) adds the complete `test-cljs-1420` lookup
regression and all eight `cljs-2085-test` assertion sites, preserving their four
original record declarations. The focused reify gate also re-executes existing
metadata, destructuring, qualified-method and mutable-state witnesses: 36
original sites in all. Independent reify probes and the event namespace program
remain separate from imported-source assertion counts.


## User type member — 2026-10-10

The inventory now retains fourteen complete upstream files, adding
`new_new_test.cljs` and `collections_test.cljs` for the requested type contract.
The new files contribute honest missing/unrelated rows as well as witnesses;
adding them does not turn unexecuted source into coverage. The immutable inventory
now contains 2,876 assertion sites across 650 complete top-level bodies.

The strict selected type run covers **30 original assertion sites**, with zero
missing assertions for its selected complete bodies. These are
`core_test.cljs/{tset-638,test-920-watch-ops-return-ref,test-919-generic-cas,
test-cljs-2083,test-cljs-2407}`, `new_new_test.cljs/{test-ifn,test-get-basis,test-515}`,
`collections_test.cljs/test-cljs-2128`, and the previously mapped full
`destructuring_test.cljs/test-protocol-method-destructuring`. The namespace witness
keeps `cljs.core-test` so generated factory documentation is compared literally.
The custom-vector witness retains both iterator classes, both reduction branches,
and the original non-lite branch with `LITE_MODE` false.

All 30 sites execute in generated JavaScript, Maven **1.12.134** and the pinned
checkout. See [type evidence](../compiler-types.md) for commands and independent
cases. No other assertions in the added complete files are claimed as passing.


## Record completion

The record selection adds 42 in-scope assertion witnesses from complete
`new_new_test.cljs::test-records`, core equality/map-construction/contains regressions,
and `seqs_test.cljs::test-cljs-2482`. With existing record factory, basis, metadata
and Search1–Search4 witnesses, the strict selection passes 59 assertion sites on
generated JavaScript, Maven 1.12.134 and the pinned checkout. Every imported body
and setup is retained; mixed sorted-map assertions remain separately classified.
The full `seqs_test.cljs` is inventoried, bringing the catalog to 15 source files,
3,137 assertion sites and 682 complete top-level bodies. See
[record evidence](../compiler-records.md) for exact commands and limitations.

## Protocol extensions member — 2026-10-10

The [extension audit](../compiler-extensions.md) completes every assertion in the
pinned `extend_to_native_test.cljs` file (31 sites) and all four assertions in
`new_new_test.cljs::test-414`. The four new complete-body fixtures retain native
printing, array empty, all 17 protocol-predicate/dispatch combinations, and
nonmutating instance specification. Earlier object sequence/lookup/reduction,
slash-method and contains witnesses remain intact. All 35 sites pass generated
JavaScript, Maven 1.12.134 and the pinned checkout.

Witnesses that mutate native extension tables now carry `isolated: true`.
The runner compiles them together, then loads each witness in a fresh Node
process; ordinary fixtures retain their existing grouped execution. An accounting
regression checks separate invocations and exact case selection. This is runtime
isolation, not a claim that separate namespaces reset `cljs.core`.


## Core protocol consumers — 2026-10-10

The [core protocol audit](../compiler-core-protocols.md) inventories the public
consumers and their type/native dispatch boundaries. Fifteen new assertion
mappings execute the full minimal-user-sequence map-conj regression and basic
sequence/equality bodies. The integration selection also reuses complete native
extension, destructuring, custom-vector and state witnesses, for 79 source sites.
Independent `core-protocol-*` fixtures retain custom types/reify values, all
reduction arities, writers, keys, marker satisfaction and missing-method failures.
Release/checkout error-category differences are explicit in the audit and
versioned comparison; unexecuted source assertions remain missing.

The final strict selection passed **79/79** on generated JavaScript and both
pinned references. **45 independent protocol-consumer fixtures** matched their
versioned outcomes on each reference; ten retain explicit release/checkout
differences, separately from the source assertion coverage count. The ordinary
regression suite passed **672 tests**, with **41** explicit skips; the final
record/execution/provenance run passed **53 tests**. Literal Node tests, all-target
Clippy, inventory validation, formatting and the 17 accounting tests passed.


## Persistent collection helpers — 2026-10-10

The [helper audit](../compiler-collection-helpers.md) adds 164 assertion witnesses
from 17 fixtures, including full nested-operation, selection, duplicate-key and
constructor regressions, ordinary empty/stack sections, and MapEntry helper
interfaces. The new hash-map/map-entry source files are retained in full; the
inventory now contains 17 files, 3,223 sites and 694 complete top-level bodies.
Adding those files does not cover their remaining unexecuted assertions.

The strict selected run passed all 164 sites on generated JavaScript, Maven
1.12.134 and the pinned checkout, with zero missing selected assertions and no
version differences. Independent `helper-*` fixtures and literal Node checks
remain separate from imported assertion counts. The audit includes exact
selection commands, boundaries, provenance and output/startup measurements.


## Deferred values member — 2026-10-10

The [deferred-value audit](../compiler-deferred.md) adds **27 covered assertion
sites** across delay, lazy pending state, metadata regression and selected seq
sections. Six complete/section witnesses pass emitted JavaScript, Maven 1.12.134
and the pinned checkout with zero missing assertions in that explicit selection.
The matrix retains original hashes, stateful setup/order and adaptation reasons.
Twenty-three independent probes and direct host/GC tests remain separate from
source assertion accounting. Other lazy producer assertions remain unclaimed.

## Selected source completion (2026-10-10)

A full live run against Maven **1.12.134** and checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e** completed all **2,535 selected
assertion sites** from 18 files and 696 complete source bodies. The inventory
still retains all 3,237 sites; 702 explicitly audited unrelated sites are not
included in semantic coverage.

| Live disposition | Assertion sites |
| --- | ---: |
| Covered, agreeing across generated JavaScript and both references | 2,295 |
| Verified source rejections | 43 |
| Verified warnings | 31 |
| Explicit release/checkout version differences | 166 |
| Missing selected assertions | 0 |

The 166 version differences are independently checked observations, not extra
agreement passes. Rejections and warnings have exact phase/message/warning
contracts and separate result columns. Mixed positive/negative source bodies
retain all original assertions and require separate witnesses for each selected
part. Runtime witnesses retain complete expected values, ordered effects and
stdout. Four collection predicates found missing during review were restored
before this run.

This table supersedes the historical baseline counts above, without rewriting
the earlier evidence. See [the integration evidence](../compiler-language-completion.md)
for the multi-namespace program, deterministic interactions, runtime corrections,
full regression commands, supplemental boundary checks and output/startup method.
The required CI gate regenerates observations; committed witness mappings and
this historical result cannot substitute for a fresh passing run.

The final frozen-source rerun also completed the separate supplemental boundary
gate: all 27 contracts verified through 32 diagnostic probes and 13 runtime
proofs, with no remaining `missing-capability` row. These independent probes do
not change the source assertion counts above. See the
[supplemental contract](boundary-support.md) and
[final verification record](../../../benchmarks/results/compiler-2026-10-10/language-completion/verification.json).
