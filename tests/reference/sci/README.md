# Pinned JVM SCI reference corpus

The 2,475 committed rows in `tests/fixtures/sci-outcomes.tsv` are **historical
pinned JVM SCI observations**, preserved with their original source and provenance.
The native coverage sections below describe historical milestones; their native
successes do not establish compiler-backed evaluator compatibility.

[Evaluator accounting](evaluator-accounting.md) describes the current execution
gate: every original row has a separate executable disposition. There are 1,484
matching observations (1,180 original-source portable rows and 304 explicit
source adaptations), 906 nonpassing gaps and 85 intentional retirements.
Gap/retirement assertions are accounting regressions, never conformance passes.
The corpus/protocol/provenance checks remain offline and require no Java,
network or `research/`. See the [evaluator profile](../../../docs/evaluator.md)
for the production execution API.

## Regenerate or compare explicitly

Requirements for this opt-in job: POSIX, Python 3, Git, Clojure CLI, a JVM, and a
clean SCI source checkout at
`5fe89056384418be50d232b4278344cb3a1a526b`. The first invocation may fetch Maven
dependencies. There is no release, alternate checkout, or version fallback.
`CLOJURE` may name a CLI executable; it cannot change the required SCI/Clojure
versions or effective options.

From the repository root:

```sh
# Compare every case; writes no committed files.
python3 tests/reference/sci/regenerate.py --checkout research/sci --native

# Deliberately regenerate only after all cases and identity checks succeed.
python3 tests/reference/sci/regenerate.py --checkout research/sci --write

# Equivalent explicit Rust reference job, using an explicitly supplied path.
LIGHT_CLJS_SCI_CHECKOUT="$PWD/research/sci" \
  cargo test --locked --test sci_reference \
  pinned_sci_reference_matches_committed_outcomes -- --ignored --exact --nocapture

# Offline corpus/provenance checks and launcher failure tests.
cargo test --locked --test sci_reference --test provenance
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover \
  -s tests/reference/sci -p 'test_regenerate.py' -v
```

The launcher checks the exact Git root, full revision and clean tracked/untracked
state both before and after execution. A temporary empty working directory plus
`-Srepro` prevents project/user configuration from substituting the oracle. It
pins Clojure 1.12.4 and uses the checkout's `deps.edn` through `:local/root`:
Edamame 1.6.44, sci.impl.types 0.0.3 and graal.locking 0.0.2. The driver verifies
that core, parser, analyzer, interpreter, namespaces and types resources were
actually loaded from that source tree. It records the effective Clojure/Java
runtime and options in `runtime.tsv`; Java version/vendor may differ between
runs, but each run must report them. Captured invocation/artifacts also retain
the exact executable and options used.

The default JVM timeout is 180 seconds, configurable via positive `--timeout`.
A hung process group, including launcher descendants, is killed; the launcher
is reaped.
Each case writes a start marker with ID/source and an end marker to diagnostics.
Each invocation retains its source corpus snapshot, command, stdout, stderr and
runtime in the printed temporary artifact directory. Missing tools, revision
mismatch, dirty checkout, missing/wrong resource identity, unknown error
classification, unsupported serialization, JVM failure, timeout, and missing,
duplicated, extra or reordered case results all fail. A zero exit code without
all cases does not pass. Check mode fails on any mismatch; write mode updates
golden rows only after the complete run succeeds.

## Format: sci-outcomes-v1

The TSV has seven columns: `id`, `profile`, `source`, `outcome`, `effects`,
`stdout`, `stderr`. Source is one literal line of Clojure code, using source
escapes for newlines/tabs; it is never printed and reparsed as transport. IDs are
unique across the existing fixture files. The Rust test helper
`tests/support/sci_protocol.rs` provides typed `Value`, `Outcome`, `Phase`,
`Category`, `Case`, and strict decoders for future native integration tests.

Every transported value is tagged. This small ASCII wire grammar is independent
of JSON, EDN reader behavior and the interpreter front end:

| Value | Encoding |
| --- | --- |
| nil, boolean | `nil`, `(bool true)`, `(bool false)` |
| signed integer | `(int -9223372036854775808)`; exact canonical decimal i64 |
| double | `(float 8000000000000000)`; 16 lowercase hex IEEE-754 bits, preserving -0/infinities; all NaNs use `7ff8000000000000` |
| reduced ratio | `(ratio -2 3)`; canonical i64 numerator and positive denominator > 1; zero/integral values use `int` |
| reference-only BigInt | `(bigint 9223372036854775808)`; arbitrary canonical decimal digits |
| reference-only BigDecimal | `(decimal 120 2)`; exact unscaled decimal integer and signed i32 scale, preserving `1.20M` |
| string | `(string u0061d800)`; `u` followed by zero or more four-digit UTF-16 units; empty string is `(string u)` |
| character | `(char 03bb)`; one four-digit UTF-16 unit |
| symbol / keyword | `(symbol - u0078)`, `(keyword u006e0073 u0078)`; separate optional namespace (`-` if absent) and nonempty name units |
| list / vector / sequence | `(list VALUE...)`, `(vector VALUE...)`, `(seq VALUE...)`; preserve kind and element order |
| map | `(map (entry KEY VALUE)...)`; sorted by complete encoded entry, never by JVM map traversal order |
| set | `(set VALUE...)`; sorted by complete encoded value |

Canonical decimal integers have no plus sign, leading zero or negative zero.
Maps/sets reject duplicate encoded keys/elements. This validates transport
uniqueness, not the interpreter's eventual numeric equivalence/hash rules.
The transport preserves scalar numeric categories and sequence kinds even when
Clojure's `=` would consider two values equal. It does not compare metadata,
function identity, atoms, arbitrary host objects or cycles; closures are tested
by calling them. Unsupported values fail serialization. Ratios outside the
bounded component range are outside v1 and need an explicit protocol extension
before reference cases are added.

`effects` is a vector of values received by the supplied `observe` callback;
the callback returns its argument unchanged. `stdout` and `stderr` are UTF-16
strings. Each context and capture resets for each case. Channels preserve their
own order; this protocol does not assert a merged stdout/stderr/observe event
order. Completed effects/output survive failures, and later cases still run.
Finite sequence results are realized during serialization while capture bindings
remain active; those realization effects are included. The corpus has no
infinite sequence result. Driver serialization bounds each collection at 10,000
items and nesting at 128; the Rust decoder also limits total value nodes to
10,000 and input length to 1 MB. These are reference transport limits, not the
native evaluator's future resource policy.

An outcome is `(return VALUE)` or `(error PHASE CATEGORY)`:

| Phase | Categories currently covered | Classification evidence |
| --- | --- | --- |
| `read` | `reader` | SCI `:phase "parse"` |
| `analysis` | `unresolved-symbol`, `invalid-form` | SCI `:phase "analysis"` plus anchored diagnostics for unresolved names, invalid if/quote, missing/nonvector function parameters, invalid adjacent overloads and anchored recur tail/target-arity diagnostics |
| `execution` | `arity`, `arithmetic`, `type`, `exception-info`, `duplicate-key` | Cause chain ArityException or anchored multi-arity dispatch diagnostics, ArithmeticException/NumberFormatException, ClassCastException/IllegalArgumentException, the explicit ExceptionInfo data marker, or checked collection constructor duplicate-key diagnostics |

The driver fails on an unrecognized error instead of treating every exception
as an expected failure. Error messages and stack frames are diagnostic evidence,
not equality fields. Returned values are encoded outside the language exception
catch, so serialization failure cannot masquerade as a language error. Top-level
forms execute incrementally: already completed effects survive a later read or
analysis failure. Error data and more categories can extend this version only
with explicit fixtures and documentation.

## Source anchors and attribution

`tests/reference/fixture-origins.tsv` supplies exact pinned paths, named test
anchors, line ranges, adaptations and observed behavior. `tests/provenance.rs`
checks coverage/identity without consulting the checkout. Upstream-derived
assertions cover core do/if/map behavior, sequential/nested lets, both closure
tests, top-level effects before analysis failure, unresolved definition analysis
and fixed-function arity. Independent transport/boundary probes remain honestly
marked `hand-authored`.

Design anchors at the same pin:

- `src/sci/core.cljc`, `eval-string`, `init`, `eval-string*`, `parse-next` and
  `eval-form`: context/options and incremental API contract.
- `test/sci/parse_test.cljc`, `parse-string-test` and `parse-test`:
  parsed forms and ordered per-context evaluation.
- `src/sci/impl/parser.cljc`, `parse-next*`: parse error phase.
- `src/sci/impl/analyzer.cljc`, `throw-error-with-location`: analysis phase.
- `test/sci/error_test.cljc`, `analysis-error-test` and `arity-error-test`:
  semantic error probes without incidental stack/message equality.
- `deps.edn`: exact source dependencies; `NOTICE.md` and `SCI-LICENSE.txt`
  retain SCI attribution and its license for adapted assertions.

Local verification on 2026-10-08 used Clojure CLI 1.12.4.1618, Clojure 1.12.4
and the Java runtime recorded in `runtime.tsv`. All 289 expected rows were
produced by the pinned checkout. This is a focused local reference run, not an
upstream suite or hosted CI claim.

## Native ownership foundation coverage

`tests/interpreter_values.rs::scalar_storage_matches_selected_pinned_sci_expectations`
passes the scalar outputs from `sci-scalars`, `sci-integers`, `sci-doubles`,
`sci-utf16`, `sci-characters` and `sci-symbols-keywords` through native value
constructors and inspection. These rows retain their existing hand-authored
provenance. The test preserves categories, exact UTF-16 and floating-point bits;
it does **not** read or evaluate their source. Ratios and collection storage are
covered by the collection tests described below.

`retained_scalar_source_matches_sci_parse_plus_string_assertions` adapts only
the first scalar/source assertions of `test/sci/parse_test.cljc`,
`parse+string-test`, lines 23–31, at the pinned revision: value `1` and source
`"1"` from `"1 {:a   1} [1  2 3]"`. The existing parser supplies the span and a
host constructor supplies the integer. Dropping parser/source/context storage
before those assertions adds the Rust lifetime check; this is not a native
implementation of SCI's `parse-next+string` API. Attribution is in `NOTICE.md`.

Ownership design anchors at that same revision are `src/sci/lang.cljc`,
`Var` (lines 71–172), and `src/sci/impl/types.cljc`, `ConstantNode`/`->constant`
(lines 293–303). They motivate state identity and distinct executable constants;
the Rust arenas/collector are independently implemented, not translated SCI
code. The remaining ownership, cycle, drop and reuse probes are hand-authored
Rust contracts. JVM object identity and garbage-collector internals are not
portable oracle assertions.

## Native immutable collection coverage

`tests/interpreter_collections.rs` constructs equivalent native values and
compares equality/hash relationships, lookup results, reduced ratios, metadata
replacement, nested metadata, list tails and unordered map contents against
selected corpus observations. These are native collection operations, not
execution of the fixture source. The collection-construction effects and
`eval-colls-once` programs remain reference expectations for future evaluation.
The constructor API accepts already evaluated ordered inputs; map/set iteration
is unspecified and must not be used to recover source evaluation order.

The metadata assertions adapt `core_test.cljc::meta-test` (1610–1641); the
eight-entry map content assertion adapts `array-based-map-test` (1667–1671)
without relying on array-map iteration. `eval-colls-once` (1225–1227) is preserved
in the oracle. The new equality/key/effect/list-tail probes are independently
authored. The current 289-row corpus was regenerated successfully against the
pin and recorded runtime. See the profile for deliberate hash-number and
NaN boxing differences.

## Native semantic reader coverage

`tests/interpreter_reader.rs` lowers actual source using the native parser and
compares ordinary reader values, metadata and anonymous-parameter relationships
with 27 new `sci-reader-*` cases. Quote/deref/var are ordinary reader lists, not
implementation AST assertions. Duplicate fixtures record read errors; targeted
Rust checks assert duplicate categories and source spans. Invalid metadata and
argument probes catch SCI's read-string exception and return `:invalid`; native
checks assert specific lower errors. These cases exercise reading, not native
evaluation. SCI parse-test/EOF adaptations are identified in fixture-origins.tsv;
all other new cases are independent probes. The direct parse+string span check
adapts the exact three strings in parse_test.cljc (23–35).

The 289-row corpus was regenerated from the clean pin on 2026-10-08. The
interpreter profile documents explicit unsupported forms, separate source
provenance instead of location metadata, preserved syntax-parser acceptance,
bounded numbers, anonymous index rules and resource limits.


## Native primitive evaluation coverage

`tests/interpreter_eval.rs` executes 36 selected corpus sources entirely in Rust,
comparing exact typed values and stable phases/categories. These are literals,
quote and reader data, if/do, primitive let*, collection members/metadata,
unreachable-branch analysis, quote/if arity and evaluated duplicate map/set keys.
The eleven new `sci-primitive-*` cases include two adaptations: the literal
let* nested scope assertion in `core_test.cljc::let-test` (176–177), and the
zero-operand if assertion in `core-test` (65–66). Existing do/if adaptations now
also execute natively. The remaining new cases are independent probes.

The completion-order unit tests use a test-only native instruction probe;
Native `observe` callbacks now execute the numeric/call fixtures below; stdout/stderr still require later features. Namespace/definition coverage is documented below. Closure reference cases are documented below. The current 289-case oracle
was regenerated against the pinned clean checkout on 2026-10-08. Native tests
also cover owned input/code/error spans, repeated analyzed execution, foreign
owners, reset and scratch-root cleanup. Those Rust lifecycle tests are local
contracts rather than translated SCI assertions. See the interpreter profile
for strict raw let* shape validation and the distinction between whole-expression
analysis and incremental top-level evaluation.


## Native ABI and numeric coverage

`tests/interpreter_native.rs` executes 39 `sci-native-*` fixtures: exact/float
arithmetic, ratio reduction and DECIMAL64 conversion, comparisons, signed zero,
NaN/infinity, negative rem/mod, arity/type/range failures, predicates, first-class
native calls and ordered argument effects. Integral ratio and numerator/denominator
results explicitly use `long` in portable fixtures because JVM can return in-range
BigInt/BigInteger values. The native representation always canonicalizes these to i64.
The extracted zero?-if assertion adapts `core-test` (61–62); the extracted int?
assertion adapts `permission-test` (500–504) without claiming permission enforcement.
All other native-call probes are independently authored. The 289-row corpus was
regenerated and checked against the same clean SCI pin on 2026-10-08.

Type categories normalize ClassCastException/IllegalArgumentException; numeric
NumberFormatException (nonfinite floating quotient) is arithmetic. The native
structured error retains its more specific numeric/type category; tests map it
to the reference transport category. Checked long endpoints, bounded ratio
results, f32-rounded values stored as double, and representation-based identity
have separate native tests and documented differences in the compatibility profile.


## Function and recursive local binding reference coverage

The 42 `sci-fn-*` and `sci-letfn-*` rows cover fixed and variadic calls,
fixed-over-variadic priority in either source order, named recursion across
arities, nil/sequence rest values, body analysis and delayed execution,
parameter/self shadowing, independent and transitive captures, rebinding,
anonymous literals, higher-order native calls, ordered effects before arity
errors, and 22-argument calls. Local-function cases cover forward references,
mutual recursion, self-return identity, empty bodies and peers retained by an
escaped closure. All 289 rows were regenerated and compared against the clean
pinned JVM checkout on 2026-10-08.

`fn-test` (205–244), `letfn-test` (1186–1197) and `self-ref-test` (1706–1709)
in `test/sci/core_test.cljc` supply the adapted assertions identified in the
manifest. Simple `fn` and `let` macros become primitive `fn*` and `let*` in
these fixture sources. Other probes are independently authored. The oracle has
no raw `letfn*` analyzer form: its `letfn` macro creates and initializes local
Vars (`src/sci/impl/namespaces.cljc`, `letfn*`, 351–361). The native equivalent
sources are explicitly listed in [native-letfn.tsv](native-letfn.tsv), keyed
by oracle case ID. These change `(f [args] body)` specifications to
`f (fn* [args] body)` bindings and retain the same calls and effects. They do
not claim a native macro expander or native Var implementation.

Design inspection used `src/sci/impl/analyzer.cljc`,
`expand-fn-args+body` (324–360), `analyze-fn*` (426–644), and
`src/sci/impl/fns.cljc`, `multi-arity-dispatch` (173–200). The pinned raw
function analyzer has permissive behaviors preserved by seven explicit
`sci-fn-raw-*` cases:

- Duplicate fixed arities select the last body; duplicate parameter names
  resolve to the last argument.
- A fixed body larger than the variadic minimum is accepted when it precedes
  the variadic clause. The reversed adjacent order fails analysis.
- A fixed clause between two variadic clauses permits both; calls use the
  first variadic body. The adjacent two-variadic case fails analysis.
- `[&]` acts as a zero-argument fixed signature; forms after the first rest
  name are ignored; qualified symbol parameters are accepted.

The driver recognizes missing/nonvector parameter declarations and the two
exact overload diagnostics as analysis `invalid-form`. Multi-arity dispatch
uses a generic JVM exception rather than ArityException; its anchored
`Cannot call ... with N arguments` diagnostic maps to execution `arity`.
All unknown errors still fail the reference job. The top-level `do` in
`sci-fn-unused-body-analysis` preserves its completed `observe` effect before
the later function body fails analysis.


## Loop and recur reference coverage

The 40 `sci-loop-*` and `sci-recur-*` rows cover loop initialization, 10,000
function backedges, simultaneous swaps, twenty target slots, nested loops and
function targets, retained captures, operand effects and failures, all four
variadic rest-slot replacement kinds, and target/arity/tail validation. Ten
adaptations come from `core_test.cljc::recur-test` (706–796) and `loop-test`
(800–850); the manifest identifies each exact assertion and reduction. The
remaining 30 are independently authored probes. All 289 oracle rows were
regenerated from the clean pinned checkout on 2026-10-08.

The native test executes all 40 sources. Thirty-seven compare exact oracle
outcomes/effects. Three explicitly verify a deliberate stricter contract:
`sci-recur-if-test`, `sci-recur-callee` and `sci-recur-argument` return true from
`fn?` in pinned SCI, because that raw analyzer retains its recur context in these
value positions. Native analysis rejects them as non-tail recur. The probes only
construct functions; they do not execute those invalid recur placements or rely
on SCI's internal recur marker as a language value. These differences are
asserted directly, not skipped or treated as successful matching outcomes.

Design inspection used `src/sci/impl/analyzer.cljc::recur-target` (35–48),
`analyze-children-tail` (81–87), `return-recur` (253–310), `analyze-loop*`
(921–933), `return-if` (944–973), `analyze-try` (997 onward) and computed/native
call analysis (2125–2222). SCI lowers loop to let plus an implicit function;
the Rust IR has an explicit loop target in the current frame. SCI's function
and loop recur slot replacement informed the independent Rust trampoline.

Native tests also run 100,000 iterations each for loop/function recur, sample
constant stack/live-frame storage with explicit periodic collection, test wide
recur, source ownership, generated-form attribution, polling cancellation and
GC during nested operand evaluation. Protected-region tests now include source-level try/catch/finally and fresh
nested recur targets, as described in exception coverage below.

## Native namespace and Var coverage

`tests/interpreter_namespaces.rs` executes 37 focused `sci-var-*` and
`sci-namespace-*` fixtures plus get/contains projections, comparing typed
results, phases and completed effects.
The corpus adapts fourteen assertions from `core_test.cljc::def-test`,
`declare-test`, `defonce-test`, `resolve-test`, `conditional-var-test`, and
`namespaces_test.cljc::vars-partitioned-by-namespace-test`, `ns-map-test`,
`misc-namespace-test`. The manifest records exact lines and each adaptation;
other fixtures and Rust ownership/embedding tests are independently authored.

These cases verify unbound declarations, metadata/docstrings and replacement,
initializer/metadata order, forward references, defonce skipped analysis and
metadata effects, stable Var reads, lexical shadowing, private qualified access,
namespace identities, public/intern/referral views, copied unbound values,
repeated definitions after unmapping, reserved metadata precedence, macro-name
shadowing and UTF-16 string lookup. Vars/namespace/unbound
objects are projected into serializable results rather than added to transport.
Namespace loading, aliases and dynamic binding are outside these assertions.

All 289 rows matched the clean pinned SCI checkout on 2026-10-08. Native tests
execute the 37 namespace/introspection rows; this remains focused feature coverage.


## Native exception coverage

`tests/interpreter_exceptions.rs` executes all 46 `sci-exception-*` rows and the
existing `sci-throw-error` case. Five new rows adapt `core_test.cljc::throw-test`
(976–980), `try-catch-finally-throw-test` (1010–1017), and `ex-message-test`
(1154–1156); the manifest records exact transformations. The other 41 are
independent probes of catch ordering/category/scope, identity, data/message/cause,
finally result replacement, argument/initializer failures, analysis frontiers,
raw try clause ordering and protected recur targets. Every native result and
completed effect matches the pinned JVM oracle. The driver additionally
recognizes the anchored throw-arity and unresolved-catch-class analysis messages.

All 289 rows regenerated successfully at the clean SCI pin and recorded runtime.
Default SCI options expose Exception and qualified clojure.lang.ExceptionInfo;
Throwable and bare ExceptionInfo native aliases are explicitly outside those
oracle assertions. Source stacks, terminal host/control failures, GC/root cleanup
and reset behavior use independent Rust tests. See the interpreter profile for
these native contracts and for the preserved raw try clause parsing behavior.


## Native collection core coverage

`tests/interpreter_collection_core.rs` executes all 159 `sci-coll-*` rows.
The collection-literal and quoted-symbol cases adapt `core_test.cljc::core-test`
(107–117) and `call-quoted-symbol-test` (1607–1608); other probes are independent.
Construction, lookup, nil/default handling, nested updates, callback order,
metadata, map-entry/Cons categories and callable data compare directly to SCI.
Deferred keys/vals failures are forced inside evaluated programs so transport
errors cannot be mistaken for language outcomes. Short-circuit reads/equality and
merge callback/error order are included. All 448 rows regenerated at the clean
pinned checkout and recorded runtime on 2026-10-08.

The transport now records execution `bounds` for IndexOutOfBoundsException.
The classifier also recognizes unsupported collection operations and the exact
vec conversion/empty-pop diagnostics as type errors, and literal-keyword arity
as an analysis invalid form. Unknown failures still fail the reference job.
Storage sharing, checked callback ownership, GC and terminal errors have
independent native tests; the interpreter profile documents hash-trie versus
JVM array-map timing and the eager-storage/fallible equality APIs.


## Finite traversal and eager higher-order coverage

`tests/interpreter_sequences.rs` executes all 122 `sci-seq-*` rows, comparing
exact typed outcomes and completed effects. Traversal covers all finite seqable
kinds, nil and UTF-16 surrogate units. Reduction covers zero-argument empty
calls, singleton/initializer preservation, one-layer reduced termination,
vector indexes, map entries and fallback error ordering. `ensure-reduced` preserves
existing marker identity and wraps other values once. Additional probes pin
ordinary reduce's absence of one-argument completion calls, nested markers, and
separate source-node versus reducer-callback effects on controlled lazy inputs.
Higher-order cases cover short-circuit truthiness, shortest-input mapv, applied variadic tails,
combinator entry arities, effects and callback failures.

Three rows adapt `test/sci/core_test.cljc::core-test` map (95–96), `fn-test`
(214), and `try-catch-finally-throw-test` (998); the first changes lazy map to
mapv while retaining the inc input and adds independent multi-input variants,
the second uses primitive fn* and projects rest with vec, and the catch assertion
is unchanged. Other new probes are independent. All 556 corpus rows regenerated
against the clean SCI pin and recorded Clojure 1.12.4 runtime on 2026-10-08.

Native-only tests cover 100,000-element reduction/mapv/next traversal, bounded
stack/live frame storage, GC during callbacks, retained sequence slices and
applied rest values, traced combinator cycles, owner boundaries and iteration
cancellation/finally behavior. The ensure-reduced tests additionally retain closures
and their captures across collection and owner reset, reject foreign plain and
already-wrapped values, and check allocation accounting on both branches.
No lazy core name is supplied as an eager substitute.


## Native resource-control coverage

`tests/interpreter_limits.rs` adapts `test/sci/interrupt_fn_test.cljc` at the same
`5fe89056384418be50d232b4278344cb3a1a526b` pin:
`interrupt-uncatchable-test` (catch-all loop and finally effect),
`interrupt-not-maskable-by-finally-test` (throwing finalizer and ordinary exception
replacement), `interrupt-forge-resistant-test` (guessed ex-info interrupt marker),
`direct-recursion-no-recur-test`, and `normal-completion-under-budget-test`.
Primitive loop*/fn* replace macros, host-configured Rust limits/tokens replace
SCI interrupt callbacks, and a finite vector replaces range for reduction.
`research/sci/doc/interrupt.md` and `src/sci/interrupt.cljc` inform the cooperative
boundary and native-callback limitations; their implementation is not translated.

Resource thresholds, sticky reentry, allocation/output accounting, source spans,
root accounting and bounded cleanup are independently authored native contracts.
A host checkpoint test covers the common Macroexpansion poll API; language macro
coverage is described below. The default-context oracle remains 556 rows;
these host policy tests run entirely in Rust and do not alter driver options.


## User/native macro coverage

`tests/interpreter_macros.rs` executes all 50 `sci-macro-*` rows. These cover raw
arguments, lexical/qualified/special resolution, &form metadata, the documented
&env view, one-step/repeated/env-first expansion, effectful analysis, retained
code, macro Var restrictions, captured macros, definitions and namespace changes
from generated top-level do, and malformed results. Adapted assertions come from
`core_test.cljc::macro-test`, `defmacro-test`, `macroexpand-1-test`,
`macroexpand-call-test` and `macro-val-error-test`; exact lines and transformations
are in the provenance manifest. Other cases are independently authored.
All 606 corpus rows regenerated against the clean pinned checkout and recorded
Clojure 1.12.4 runtime on 2026-10-08.

SCI's bare-macro-value error lacks ex-data despite originating in `analyze`.
The driver recognizes its anchored rejection message as Analysis/invalid-form.
SCI also leaves macro invocation arity/arithmetic failures without a phase tag;
these rows retain the existing Execution transport label. Native execution
preserves the dedicated Macroexpansion phase and source invocation. Tests compare
those categories/effects and explicitly test the native phase; they do not claim
identical phase metadata. New macro implementation choices and the bootstrap
expansion-shape difference are documented in the interpreter profile.

Host callback ABI, direct data conversion/source ownership, optional attribution,
reanalysis without caching, cancellation, reset/foreign-owner results, and common
fuel/allocation/depth controls are independently tested in Rust. No subprocess or
JVM participates in native execution. The pinned macroexpand implementation and
ADR 0004 informed env-first arities and identity-based outer expansion.


## Syntax quote coverage

`tests/interpreter_syntax_quote.rs` executes all 75 `sci-syntax-quote-*` rows.
Thirteen adaptations cover `core_test.cljc::syntax-quote-test` (1030–1076),
`meta-on-syntax-quote-test` (1407–1408), and `resolve-unquote` (1413–1414).
The manifest records exact assertions and changes; remaining rows are independent
nested scope, qualification, alias, shape, metadata, macro and invalid-splice probes.
Generated identity spellings are compared through relationships rather than IDs.
All 681 rows regenerated successfully against the clean pinned SCI checkout and
recorded Clojure 1.12.4 runtime on 2026-10-08.

The driver identifies the exact raw `unquote-splice not in list` exception as a
reader error because SCI leaves that read-time failure without ex-data. Deferred
invalid tails are forced within the evaluated source whenever an error is the
expected outcome; serializer exceptions remain reference-job failures. Native-only
tests cover retained code/forms/tails across GC/reset, multiple pull reads under
changed namespace state, empty function metadata after macro expansion, source
provenance and small allocation thresholds. The interpreter profile documents
finite concat views, one-argument sequence and array-map storage boundaries.


## Binding and function macro coverage

`tests/interpreter_bindings.rs` executes 114 `sci-binding-*` cases, including the
original binding-order case and 113 new cases. Focused adaptations cover
`core_test.cljc::destructure-test`, `defn-test`, `macro-test` lexical environments,
`pre-post-conditions-test`, `destructure_test.cljc` sibling/default behavior, and
`clojure_destructure_test.cljc` singleton/trailing keyword-argument maps. The
manifest records exact anchors and adaptations, including caught AssertionError
results instead of JVM-formatted messages. All 794 rows regenerated against the
clean pinned SCI checkout and recorded Clojure 1.12.4 runtime on 2026-10-08.

Independent probes cover nested/rest/alias/default bindings, evaluate-once and
eager-default effects, declaration visibility, named self versus Var recursion,
escaping letfn groups, destructured recur and pre/post order. Rust-only checks
exercise direct owned macroexpand execution, malformed/unsupported declarations,
source attribution, GC ownership/cycles, shared resource limits and 100,000 recur
iterations. Generated AST shapes, gensym spellings and assertion messages are not
compared. The interpreter profile documents native letfn expansion, fixed assertion
messages, always-enabled assertions, and stricter malformed rest/alias validation.


## Conditional, threading and case coverage

`tests/interpreter_control.rs` executes all 132 `sci-control-*` rows. Focused
adaptations cover `core_test.cljc::cond-test`, `condp-test`, `case-test`, `ifs-test`,
`whens-test`, `threading-macro-test`, and `assert-test`; exact anchors and changes
are in the provenance manifest. Independent cases cover truthiness, single input
and predicate evaluation, skipped effects, destructuring/scope, empty bodies,
thread argument order, nil propagation, ignored surplus binding clauses, malformed
clauses, assertion message effects, and constant/grouped case behavior. Case probes
also compare structural collection equality, numeric categories, duplicate rejection,
default-before-result macro effects and tail recur. All 926 rows regenerated against
the clean pinned checkout and recorded Clojure 1.12.4 runtime on 2026-10-08.

SCI conditional-binding vector failures omit phase and ordinary host exception
categories. The driver classifies only their exact anchored validation diagnostics
as invalid-form and retains the Execution transport phase; the protocol now admits
that category there. Native validation retains its dedicated Macroexpansion phase.
Assertion message text and internal generated ASTs are not compared; the profile
records fixed assertion text and the compact native case* layout. Structural
equality assertions for thread expansion preserve the upstream test's list/seq
value equality. Rust-only checks cover ownership, GC, case/recur stack behavior,
macro attribution, original/generated source spans and shared resource exhaustion.

## Delay and lazy producer coverage

`tests/interpreter_deferred.rs` executes all 74 `sci-deferred-*` rows, comparing
values, phases and ordered effects. Two adaptations cover
`core_test.cljc::core-delay-test` (1199–1200) and
`lazy-seq-macroexpand-test` (1983–1984); the latter evaluates lazy syntax directly
and projects with vec, while a native embedding test replays macro expansion.
The manifest records the exact changes. Other cases are independent probes.
All 1000 corpus rows regenerated at the clean SCI pin and recorded Clojure 1.12.4
runtime on 2026-10-08.

Reference inspection used `analyzer.cljc::analyze-lazy-seq` (935–942),
`namespaces.cljc::delay*` (396–400), and `lazy-seq*` (1251–1255). SCI's direct lazy
analyzer uses a once-only closure, whereas evaluating its ordinary macro expansion
retains the producer across body failures. Direct lazy body failures therefore
throw once and then realize empty; expanded bodies retry. Delays cache both
successful values and exceptions. Nested wrappers, invalid body results, reentrant
demand and lazy self/mutual return cycles have explicit oracle assertions.

An additional temporary pinned probe invoked real `sci.interrupt/interrupt!`
through a supplied host callback with `:interrupt-fn (fn [] nil)`. It confirmed
that delay caches an uncatchable interrupt, while a directly analyzed lazy body
remains unrealized after interruption and yields nil on a subsequent demand.
These extra host options are outside the ordinary golden corpus. Native tests use
the interpreter's cancellation token and assert its sticky scope/cleanup contract.

Native-only tests also cover source/captured value ownership, GC within producer
callbacks, retained exception identity/provenance, unreachable cycles, owner/reset
boundaries, finite consumption of infinite producers, 20,000 consecutive wrappers,
50,000 discarded elements with bounded live storage/stack, and shared fuel,
allocation and call-depth termination. No JVM participates in native execution.
The interpreter profile documents mutable realization APIs and the existing
hash-trie versus small JVM array-map key-realization timing difference.


## Core lazy sequence coverage

`tests/interpreter_lazy.rs` executes all 268 `sci-lazy-*` rows with exact values,
error phases/categories and ordered effects. Two adaptations cover
`core_test.cljc::core-test` (95–96, map inc projected with vec) and
`interrupt_fn_test.cljc::normal-completion-under-budget-test` (108–114, finite
range reduction). The manifest records each adaptation; the remaining rows are
independent probes. All 1268 corpus rows regenerated against the clean pinned
SCI checkout and Clojure 1.12.4 runtime on 2026-10-08.

Coverage includes finite/infinite-prefix transforms and producers, nil/empty and
fractional/nonpositive counts, range directions/zero steps/NaNs/bounded endpoints,
partition sizes/steps/padding, shortest-input multi-collection operations,
apply/destructuring/equality/collection consumers, bounded doall/dorun and returned
identity. Effect and retry probes use unchunked inputs to distinguish genuine
callback/lookahead behavior from excluded JVM chunking. Callback exceptions,
consumed captures, repeatable producer failures and construction-versus-demand
errors have explicit repeated-demand cases. Incidental JVM diagnostic text is not
compared; catch probes return stable markers. The ratio range assertion uses
numeric value equality because the established bounded native profile normalizes
whole ratios to i64 rather than JVM BigInt.

Native tests adapt `interrupt_fn_test.cljc::host-seq-producers-test` (116–125)
and `host-materializers-test` (127–135): Context fuel replaces the host interrupt
callback, while the original range/repeat/cycle/iterate/doall/count/into/reduce
forms are retained. Additional Rust cases cover the remaining unbounded loops,
allocation/depth controls, GC during callbacks, retained values/source diagnostics,
owner boundaries and stable stack/live storage across 20,000 discarded core nodes.
Native first(map observe vector) explicitly asserts one callback as the documented
unchunked difference. No JVM participates in native execution.

Language concat now produces genuine deferred nodes for finite inputs as well as
lazy sources. Existing syntax-quote tests realize their finite returned sequences
through Context before transport or context detachment; they still compare the
original pinned sequence categories and effects. Immutable handles never execute
new callbacks during inspection.

## Comprehension and iterative macro coverage

`tests/interpreter_iteration.rs` executes all 90 `sci-iteration-*` cases.
Six adaptations cover `core_test.cljc::for-test` (867–877), `doseq-test`
(889–897), `dotimes-test` (1176–1180), and `while-test` (1404–1405).
Comprehensions are projected with vec, supplied effect callbacks use observe,
and the while atom becomes an isolated context Var. Exact changes are recorded
in the provenance manifest. All 1,358 corpus rows regenerated against the clean
SCI pin and Clojure 1.12.4 on 2026-10-08.

Independent cases cover nested, empty and infinite inputs; per-generator while
termination; ordered modifiers; destructuring and lexical capture; eager outer
input versus deferred inner input/body effects; memoization and failure retry;
zero-generator doseq; dotimes count coercion; and empty/false body analysis.
Reference effect cases use unchunked inputs. Native validation deliberately
reports useful Macroexpansion errors for malformed vectors/unknown modifiers,
where the pin can expose unclassified host NPE/AssertionError. These malformed
cases are independently checked for original spans and absent runtime effects.

Native tests also cover direct owned macroexpand replay, macro attribution,
source/capture lifetime, reset ownership, collection during callbacks, shared
fuel/allocation/depth controls, and terminal cancellation with finally cleanup.
Discarding 20,000 single/nested comprehension elements and executing 20,000
effectful iterations preserves stable live heap/root and native-stack samples.
The implementation uses existing traced closures and indexed loops; no JVM is
involved in native execution.

## Contextual reader coverage

`tests/interpreter_contextual_reader.rs` evaluates 76 `sci-context-reader-*`
cases against the pin and checks callback traces. Six additional
`reader-difference` rows record retained parser boundaries: top-level splices,
conditional metadata, malformed skipped maps, and callback effects before later
syntax failures. The native reader checks its intentional result separately.

Profiles `reader-bb`, `reader-tags`, and `reader-fallback` add explicit oracle
configuration: respectively features `#{:bb}`; readers `t/tag=inc`, `x/str=str`
and `t/data` recording/returning its data; and a reader-lookup fallback returning
`[tag data]`. Only the three `sci-context-reader-tag-syntax-error-*` difference
rows also receive `reader-tags` configuration. All other profiles retain the
recorded default `#{:clj}` context. This is reference-only host configuration;
Rust tests install equivalent explicit native definitions.

Adapted anchors are `core_test.cljc::reader-conditionals` (1110–1112),
`readers-test` (1341–1345), `data-readers-test` (1543–1545), and
`read_test.cljc::tag-fallback-test` (81–87). The fallback adapts the host-provided
reader lookup rather than implementing language dynamic bindings. Remaining
cases are identified as hand-authored in the provenance manifest. Unknown-tag
exceptions have no SCI phase data at this pin; the driver recognizes the exact
anchored `No reader function for tag ...` diagnostic as a read failure.

Native-only checks cover live feature/alias/handler updates, UTF-8 spans,
retained data after reset, callback arity/owner failures, forwarded body frames,
exception identity and shared fuel/allocation/call-depth exhaustion. The
[compatibility profile](../../../docs/interpreter.md) documents supported tag
APIs, unsupported inst/uuid values, skipped validation and parser differences.


## Namespace loading fixtures

The 51 `sci-loading-*` rows use the `loading` profile. Its explicit load callback
reads only the committed module map `load-sources.tsv` (namespace, file identity,
source), mirrored by the Rust integration tests. It does not search a classpath
or the filesystem for interpreted programs. Assertions adapt
`namespaces_test.cljc::{nested-libspecs-test,cyclic-load-test,self-require-test,
as-alias-test,refer-clojure-exclude}` and
`core_test.cljc::{load-fn-test,reload-test,reload-all-test}` at the recorded pin.
Host print/atom effects become the existing `user/observe` callback. Additional
probes establish private refers, macro loading, file/namespace restoration and
core rename/only behavior; the manifest distinguishes these from adaptations.
Native libspec validation is deliberately stricter on malformed options, and its
filesystem ambiguity policy, permanent core registry, and nested ns execution
boundary are documented in the interpreter profile.


## Dynamic Var coverage

`tests/interpreter_vars.rs` executes all 74 `sci-vars-*` cases with exact values,
ordered effects and arity error phases. Adaptations come from pinned
`vars_test.cljc::{dynamic-var-test,var-get-set-test,alter-var-root-test,
with-redefs-test}` and `core_test.cljc::{alter-meta!-test,
built-in-vars-are-read-only-test,dynamic-meta-def-test}`. The provenance manifest
records anchors and adaptations. Remaining cases are independent probes of
nested scopes, nil/false/unbound bindings, inherited mutation, initializer order,
closure/macro/lazy timing, redefinition cleanup, namespace scope and metadata.

Native tests add explicit builtin permission, owning guards across Rust error,
panic and reset paths, out-of-order drops, isolated contexts, callback and reader
reentry, named/host file scopes, retained roots during collection, unreachable
cycles and shared resource controls. The interpreter profile documents expansion
shape, malformed syntax diagnostics, Namespace validation and stable displayed
Var names as intentional boundaries. All 1565 rows regenerated against pinned
SCI and Clojure 1.12.4 on 2026-10-08. Normal tests use only committed fixtures.

## Atom and volatile coverage

`tests/interpreter_state.rs` executes 94 `sci-state-*` rows. Two adaptations retain
`core_test.cljc::atom-with-meta-test` (1410–1411) and `vswap-test` (1798–1800).
Independent probes establish state identity, mutation/validation/watch ordering,
reentrant swap retries, old/new results, exception identity and checked-exception
wrapping, atom metadata, and SCI's twice-evaluated `vswap!` target. The pin's
`core_protocols.cljc` dispatches atom operations to Clojure host APIs; native
execution implements those operations directly and does not expose reify/IAtom
protocol extensions.

The `state-validators` profile explicitly binds Clojure 1.12.4 `set-validator!`
and `get-validator` in SCI's user namespace because these two functions are not
exported by this SCI pin. No other profile receives those bindings. Normal native
tests resolve their corresponding core functions. Single-watch effect order is
compared exactly; multiple-watch membership/count probes avoid promising JVM
hash-map iteration order. Scalar JVM boxing identity is outside the native
profile, with native representation-identity CAS tests kept separate.

Native checks additionally cover GC during callbacks, cyclic closure/atom/volatile
ownership, detached roots, context reset and foreign results, reentrant host eval,
source diagnostics, stable storage across repeated updates, and cooperative
fuel/allocation/depth/cancellation limits. All 1659 rows were regenerated against
the clean SCI pin and Clojure 1.12.4 on 2026-10-08.


## Printer and literal string coverage

`tests/interpreter_strings.rs` executes 131 `sci-string-*` rows with exact UTF-16
values, effects, arity/type/bounds categories and execution phases. Five exact
adaptations cover `io_test.cljc::print-readably-test` (116–119),
`core_test.cljc::symbol-on-var-test` (1640–1641), and
`namespaces_test.cljc::require-test` (17–19). The manifest distinguishes these
from independent numeric print-control, metadata, UTF-16 indexing/replacement,
case, escape and namespace probes. `io.cljc` and the direct clojure.string
copy-vars at `namespaces.cljc:2401–2422` supply design context, not translated
implementation. All 1790 corpus rows regenerated against the clean SCI pin and
Clojure 1.12.4 on 2026-10-08.

Native-only tests cover finite semantic-reader round trips, exact controls/lone
surrogates and floating bits, arbitrary host name components, identity cycles,
delay non-forcing, GC inside lazy/escape callbacks, retained output/reset ownership,
unused lazy join separators, metadata recursion and common fuel/allocation/output/
depth/cancellation controls. They explicitly establish differences for additional
readable escapes, opaque identity display, UTF-8-unrepresentable host characters,
Rust's contextual/locale-independent Unicode case rules, sequence display category
and permanent native namespace reload. Regex overloads and I/O sinks are outside
this pellet. See the interpreter profile for the complete exact surface.


## Explicit host I/O and reader coverage

The 49 `sci-io-*` rows cover print/pr/newline/flush, UTF-16 string capture,
stdout/stderr redirection, nested input/output scopes and exception restoration,
line terminators, shared line/form cursors, read arities, nil versus custom EOF,
first-form-only read-string, namespace resolution and reader failures. They
include four adapted upstream assertions from `io_test.cljc` and `read_test.cljc`;
exact source anchors are recorded in the provenance table. Native integration
also executes the previously recorded stdout/error-effect and reader EOF cases.
The corpus now contains 1839 rows regenerated against the clean pinned SCI tree.


## Set, walk, and EDN data library coverage

The 178 `sci-data-*` rows exercise all selected `clojure.set`, `clojure.walk`,
and `clojure.edn` exports against pinned SCI and Clojure 1.12.4. The upstream
`core_test.cljc::clojure-walk-test` stringify-keys assertion is adapted exactly;
remaining cases are independent probes, identified in the provenance manifest.
The pin directly copies these libraries from its Clojure runtime
(`namespaces.cljc:2374–2446`), so recorded runtime identity matters: Clojure 1.12.4
retains metadata on walk lists/seqs and projected relations.

Examples cover natural/mapped joins and nil rows, missing versus nil keys,
variadic set metadata, pre/post order through map-entry nodes, replacement
semantics, immediate callback failure, EDN first-form/EOF/tag options, and strict
reader dispatch. Caught assertions compare EDN reader failure behavior without
importing JVM exception class names. Native-only checks document strict option,
numeric, host-value and code-prefix exclusions, explicit input configuration,
resource limits and cursor ownership. The native unchunked lazy profile remains
unchanged; lazy short-circuit probes use list inputs to avoid JVM chunk effects.
All 2017 rows were regenerated against the clean pin on 2026-10-08.


## Embedding API coverage

Eleven `sci-embedding-*` rows compare native eval-data behavior, current namespace
resolution, lack of lexical capture, returned functions, definitions, macros,
error continuation and mutable state. The context redefinition row adapts
`core_test.cljc::ctx-test`; the manifest identifies independent probes separately.
Native integration tests additionally adapt `permission-test`, `ctx-test`,
`merge-opts-with-new-vars-test` and `merge-opts-preserves-features-test` through the
typed Rust API. They test alias/Var/native permission routes, callback/read/load
reentry, wrong owners, patch validation, and shared-owner fork boundaries. JVM
class-merging assertions are excluded explicitly. All 2028 committed rows were
checked against the clean pinned SCI tree and Clojure 1.12.4 on 2026-10-08.


## Integrated native conformance

`interpreter_conformance` executes **all 2,475 rows** in fresh contexts, including
2,416 accumulated feature cases, six complete example programs, 48 deterministic
generated variants and five inventory/transport probes. It checks exact tagged values
(including UTF-16, double bits, numeric and collection kinds), ordered `observe`
effects, both output channels and error phases/categories. Observe retains rooted
values until serialization, matching the SCI driver; lazy serialization effects
remain in capture scope. Unexpected native errors or unsupported transport values
fail instead of becoming stringified successes.

The 38 rows in [native-dispositions.tsv](native-dispositions.tsv) use the existing
seven-column corpus encoding plus a reason. Each executes and asserts its exact
native outcome and all effect/output channels. They cover bounded numeric
extensions, the excluded `hash`/`sort-by`/`zipmap` APIs, non-tail recur validation,
core runtime reader diagnostics, preserved parser-before-tag boundaries, and
explicit protocol/reify/record/extension restrictions and differences.
The test rejects stale source/profile text, orphan entries, duplicate IDs, and a
difference that now agrees with SCI. Other resource/numeric/host differences stay
in the named focused native suites; no entire family is skipped on mismatch.

[supported-apis.tsv](supported-apis.tsv) is the reviewed language inventory:
**375 names**, consisting of 344 registered definitions, 14 dynamic Vars, 14
primitive forms, analyzer-handled `declare`/`defonce`, and the `unquote` sentinel.
`interpreter_coverage` compares the inventory with actual namespace registries
(including private Rust registration functions), validates every evidence target,
and rejects new or removed APIs without a disposition. Evidence distinguishes
direct SCI assertions, expansion helpers, native-only checks, compiled REPL
replacements and explicit unsupported numeric registrations. Each reference identifies reachable use, not
just a token occurrence; it establishes some executable coverage, not exhaustive
arity/type/interaction coverage. The feature suites supply that additional depth.
`catch`/`finally` are clauses of `try`, not callable Vars.

[unsupported-families.tsv](unsupported-families.tsv) supplies executable rejection
probes for JVM/JS interop, threading/STM, nil/built-in protocol selectors and general types, sorted/transient
collections, remaining transducer constructors, transducing sequence/eduction,
regex, ambient effects, numeric extensions, reader eval,
built-in host tags, sorting, JVM hash and zipmap. JIT/async and cross-process caches
are absent implementation capabilities, not pretend registered APIs.

Native embedding/lifetime features have focused executable evidence:

| Feature | Suite and representative assertion |
| --- | --- |
| Pull reading, spans, owned forms, direct data construction | `interpreter_reader::source_order_and_forms_survive_syntax_source_and_context_lifetimes`, `supplied_data_preserves_metadata_surrogates_and_owner_checks` |
| Host callbacks, reentry and owner validation | `interpreter_native::host_callbacks_reenter_collect_reject_foreign_values_and_release_scratch` |
| Host macros and expansion diagnostics | `interpreter_macros::host_and_interpreted_macros_share_checked_raw_abi`, `effects_happen_per_analysis_and_diagnostics_attribute_generated_work` |
| Explicit namespace providers and loading | `interpreter_loading` (51 pinned cases plus provider, reload and cycle checks) |
| Permissions, option merging, forks and eval | `interpreter_embedding` |
| Exact host streams, cursor/flush behavior | `interpreter_io::channels_flush_and_bindings_restore_without_duplicate_writes`, `streamed_input_completes_multiline_forms_without_replaying_tag_handlers` |
| Fuel, allocation, output, depth and cancellation | `interpreter_limits` (including shared nested scopes and bounded cleanup) |
| Traced values, code, cycles and retained handles | `interpreter_reclamation`, `interpreter_collections`, `interpreter_values` |
| Scripts, REPL history, recovery and fresh processes | `native_cli`, `native_examples` |

The CI `native-conformance` job provisions exactly the recorded SCI revision and
Clojure CLI/runtime, checks launcher failures, then runs the reference comparison
and native suites. `--native` uses the same process-group timeout/crash enforcement
for the native Cargo process; stdout/stderr and invocation files are retained on
failure. Normal `cargo test --locked` remains independent of Java and SCI. The
existing Node-dependent compiler tests keep their separate compiler contract.

### Generated programs, replay and reduction

`tests/support/generated_native.rs` extends the test-only source-construction
approach in `tests/support/generated.rs` using the SCI corpus and wire format.
Version 1 fixes twelve seeds (including zero and u64::MAX), a wrapping 64-bit LCG,
15 expression nodes, depth three, at most five data elements and source length
below 8 KiB. Every seed exercises data, captured scope, functions, loop/recur,
destructuring, a user macro, bounded lazy consumption and ordered effects.
Four variants preserve semantics: original, consistent local alpha renaming,
identity binding, and `do`. Macro setup stays top-level in every variant because
wrapping its definition inside an expression changes analysis semantics.

Ordinary tests reproduce all 48 sources exactly, compare committed SCI outcomes,
and assert metamorphic equality. Failures print seed, complete source and replay
command. Metamorphic failures run a bounded greedy reducer (at most 32 attempts),
retaining only candidates that reproduce the same failure kind and variant.
It reduces expression subtrees and input data while preserving valid bindings;
it is a focused reduction, not a guarantee of a globally minimal program.

```sh
LIGHT_CLJS_NATIVE_SEED=42 cargo test --locked --test interpreter_generated

# Export one new seed to the same seven-column SCI protocol, obtain pinned
# expectations explicitly, then compare natively without Java.
LIGHT_CLJS_NATIVE_SEED=123 LIGHT_CLJS_NATIVE_EXPORT=/tmp/native-replay.tsv \
  cargo test --locked --test interpreter_generated generated_programs
python3 tests/reference/sci/regenerate.py --checkout research/sci \
  --fixtures /tmp/native-replay.tsv --write
LIGHT_CLJS_NATIVE_REPLAY=/tmp/native-replay.tsv \
  cargo test --locked --test interpreter_generated replay_exported
```

An arbitrary seed runs native metamorphic checks; only committed seeds or a
pinned regenerated replay have SCI expectations. Replay also accepts manually
reduced seven-column cases, validates their strict encoding/unique IDs, and limits
input to 512 KiB and 48 rows. A replay never weakens the main committed corpus.

### Complete native examples

`examples/native` contains multi-namespace order processing (`data_processing.clj`
with `reports/stats.cljc`), a custom macro with argument and emitted destructuring,
a memoized lazy pipeline, stateful callbacks with a validator/watch, and recoverable
exceptions with `finally`. The existing `demo.clj` is included too. Their exact
flattened sources and explicit load-provider sources are checked against the SCI
rows to prevent drift. `native_examples` copies the binary and sources into fresh
directories, clears the environment/PATH, runs every example with limits and a
ten-second deadline, and checks output and successful final results. No Java,
Node, SCI checkout or project files are accessible through the example setup.

### Local verification, 2026-10-08

Both `cargo test --locked` and `cargo test --locked --release` passed 406 tests
(the ten opt-in tests remain separate). The final explicit SCI/native invocation
matched all 2,087 oracle rows and passed 297 native/protocol/provenance tests.
The six launcher failure tests, strict all-target Clippy and formatting checks
passed. Seed 123 was exported, regenerated through pinned SCI, and replayed
successfully. The Rust embedding example also ran from a copied binary in a
fresh directory with an empty environment.

The unchanged compiler conformance job passed all seven opt-in comparisons
against both ClojureScript 1.12.134 and checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`. The opt-in lexer/parser corpus checks
scanned and parsed all 296 checkout sources (2,834,491 bytes). This records local
verification; no hosted CI run or whole-upstream SCI conformance is claimed.

## Custom eager transducers

The 40 `sci-transduce-*` cases cover `completing` and three/four-argument
`transduce` with ordinary user adapters. They verify original-reducer
initialization before transform application, completion after normal or early
termination, nil/empty inputs, preserved nested and completion markers, callable
Vars/data, independent per-application volatile state, exact unchunked source
demand, arity errors and exception identity/effects at every process stage.
All 2,141 cases were regenerated against pinned SCI and Clojure 1.12.4.
`interpreter_transducers` uses the shared exact transport; the full corpus
runner discovers the new suite automatically. Native embedding/limits/GC
tests additionally cover host reentry and ownership failures, retained callback
permissions, sticky cancellation and cyclic-state reclamation. Built-in transform
constructor arities beyond `map`/`filter`/`remove`/`take`/`drop`/`take-while`/`mapcat` and `cat`, transducing `sequence` and
`eduction` remain excluded.


The 32 `sci-into-xf-*` cases extend `into` to three arguments with custom callable
transducers and persistent vector/list/nil/set/map targets. They cover metadata,
source/target immutability after successful or failed steps, duplicate keys,
invalid map entries, exact UTF-16 sources, independent volatile state, completion
outputs for empty and nonempty inputs, bounded unchunked infinite inputs, and
ordinary arity/throw behavior. All 2,173 cases were regenerated against the clean
SCI pin and Clojure 1.12.4. Native tests additionally cover collection during
transform/source/step/completion, reset and foreign-owner failures, cancellation
with bounded finally cleanup, direct/evaluated resource limits, retained callback
permissions and repeated cyclic-state reclamation.


The 47 `sci-xf-basic-*` cases cover composable `map`, `filter` and `remove`
constructor arities. They pin reducer initialization/completion, direct and
applied multi-input map steps, bounded demand of an unbounded applied argument
tail, exact predicate truthiness/effects, callable data/Vars, preserved Reduced
identity and nesting, custom volatile-backed adapters in both composition orders,
interleaved process state, early termination and callback failures. Nil callback
probes catch the exception explicitly because the JVM's host NullPointerException
is outside the transport's error taxonomy; the strict harness is unchanged.
Native tests cover callback collection/reentry, owner reset/rejection, permissions,
sticky cancellation at each callback, fuel/allocation limits on no-output filters,
call depth and reclamation of retained and cyclic partially applied reducers.
All 2,220 oracle cases matched pinned SCI; the explicit native job passed 333
native/protocol/provenance tests. Formatting and strict all-target Clippy passed.

The 61 `sci-xf-bounds-*` cases cover `take`, `drop` and `take-while`
constructor arities, including direct/applied reducers, independent reusable and
interleaved counters, initialization without reset, custom stateful composition,
and finite consumption of an unbounded explicit lazy-seq/cons source. Source
observations distinguish the rejected take-while item and take-zero's first
source node from downstream reducer calls. Fractional integer/ratio/double
counts, NaN/infinities, checked integer underflow even after drop forwards,
callback reentry, lazy validation, predicate truthiness, Reduced identity/layers,
arity and exception timing are pinned. Native tests additionally exercise GC
between interleaved steps, callback GC/reset/foreign owners, fork/reset ownership,
cyclic abandoned/failed reducers, retained permissions, long constant-depth
reductions, bounded scratch roots and fuel/allocation/depth/cancellation while
unbounded processes have produced no output. All 2,281 cases were regenerated
against the clean SCI pin and Clojure 1.12.4.
The subsequent explicit oracle/native job matched all 2,281 cases and passed
340 native/protocol/provenance tests. Formatting, strict all-target Clippy and
nine-sample timing/allocation benchmark runs passed.

The 47 `sci-xf-cat-*` cases cover `cat` and one-argument `mapcat`, including
direct/applied reducer arities, map's variadic input forwarding, one-level
flattening of current seqables and exact UTF-16, and independent interleaved
processes. Controlled lazy sources prove that early reduction does not request
the next inner item or outer source node, including two nested cats and unbounded
inner input. Custom reducers pin original Reduced identity and nested contents,
exactly one outer completion for nonempty/empty subcollections, no completion
on exceptions, and preserved callback exception identity/effects. All prior
collection-taking mapcat fixtures retain their exact values and lookahead.
Native tests additionally cover collector/reentry/reset/foreign-owner failures
at each nesting boundary, permissions, sticky cancellation and bounded finally,
fuel/allocation on infinite inner and empty outer streams, nesting depth,
bounded scratch roots and abandoned/failed cyclic callback reclamation.
All 2,328 cases were regenerated against the clean SCI pin and Clojure 1.12.4.
The subsequent explicit oracle/native job matched all 2,328 cases and passed
346 native/protocol/provenance tests. Formatting and strict all-target Clippy
also passed.
Separate nine-sample timing/allocation runs validated complete and early nested
consumption, and the GC benchmark passed its live-storage checks; raw evidence
and workload-specific findings are in `benchmarks/interpreter.md`.

The 58 `sci-xf-partition-*` cases cover the one-argument `partition-all`
transducer: vector result groups, empty/exact/partial input, ordinary reduce
versus explicit completion, interleaved independent processes, reuse and repeated
completion, upstream/downstream early reduction, custom adapters, and cat
compositions. Reentrant and throwing callbacks pin clearing before emission;
completion flushing unwraps one Reduced layer and delegates completion once.
Numeric probes distinguish construction from reducer application, including
fractional/ratio/NaN/zero counts, nil caught narrowly at construction,
non-numbers, negative counts, signed capacity overflow and the rounded long
boundary. Prior lazy collection arities retain distinct fractional-count rules
and sequence group kinds. Controlled sources pin demand separately from reducer
callbacks; direct/applied arities and exact exception identity/effects are covered.
Native tests additionally cover every buffered value across callback GC,
reentry, reset/foreign owners, retained callback permissions, source lifetime,
cyclic failed/abandoned buffers and bounded roots, plus accounted growth,
large-count tiny-budget operation, cancellation/output/depth failures during
emission and completion, and bounded language finally. No native buffer reserves
the requested count eagerly; JVM ArrayList capacity allocation/OOM is not a
native behavior promise.

All 2,386 SCI cases matched the pinned reference; the subsequent native job
passed 355 native/protocol/provenance tests. Formatting, strict all-target Clippy,
the CLI example and nine-sample normalized lazy/transducer timing/allocation
comparisons passed. Ordinary GC checks passed; the instrumented GC assertion
reproduces a preexisting starting-commit failure tracked by `light-cljs-74`.
See `benchmarks/interpreter.md` and its retained current/baseline logs.

## Native protocol declarations

Twelve `sci-protocol-*` scalar probes cover `defprotocol`, ordinary method
functions, `satisfies?`, metadata, marker declarations, namespace aliases,
same-shape identity, Var rebinding and explicit missing implementations. The
reference remains SCI `5fe89056384418be50d232b4278344cb3a1a526b` on Clojure 1.12.4.
The adapted metadata/return cases cite `protocols_test.cljc` in the provenance
manifest; the other cases are independent probes. No descriptor, method function
or receiver is serialized as a claimed portable object.

Four native dispositions record deliberate restrictions: changing method shape,
zero-receiver signatures, variadic signatures and `:extend-via-metadata` fail as
native invalid forms. SCI accepts these sources. Positive receiver implementations
arrived in the later batches below. Host/core protocol customization remains
excluded; private Rust fixtures verify the original dispatch foundation. The
unsupported-family inventory preserves nil/built-in selector and host boundaries. Exhaustive older fixture adapters reject the new opaque
`ValueView::Protocol` category.

Verification matched all 2,398 pinned SCI rows. The full native integration job
passed 366 tests (one opt-in oracle test ignored); the final focused checks also
passed the added seventh public protocol test, private dispatch/GC tests,
conformance, coverage and provenance. The selected release suite passed 54 tests.
Formatting and strict all-target Clippy passed with default and all features.
The release dispatch/registration comparison and both ordinary/instrumented GC
benchmarks passed; see the [measured costs](../../../benchmarks/interpreter.md#protocol-declaration-and-dispatch-foundation-2026-10-08).

## Native captured reify objects

The `sci-reify-*` fixtures observe methods, booleans, metadata and scalar key/set
relationships. They cover lexical captures, saved/apply method functions, fixed
overloads, marker and partial protocols, identity, replacement metadata, same-shape
redefinition, recur and finally. Two explicit native dispositions preserve both
observed SCI results and intended native behavior: qualified method collisions
return `[:b :b]` in SCI and `[:a :b]` natively; SCI accepts an undeclared
implementation arity while native expansion rejects it.

`driver.clj` rejects nominal records and custom reify objects before generic map
serialization; functions and protocol descriptors also fail transport. No opaque
instance is a golden result. The opt-in launcher test checks these failures leave
the input unchanged, and verifies method/scalar observation succeeds. Run it with
`LIGHT_CLJS_SCI_CHECKOUT="$PWD/research/sci" PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/reference/sci -p test_regenerate.py`.
The native transport independently rejects objects/functions/descriptors.

Native integration tests cover retained object/method handles, fork/reset owner
boundaries, source/form drops, receiver-returning closures, collection during
callbacks, reentrant Var replacement, cyclic captures, warmed slot stability,
permissions, arity errors, fuel/allocation/depth exhaustion and sticky cancellation.
Host interfaces/core protocol customization and JS
code generation remain excluded. See the [runtime contract](../../../docs/interpreter.md#user-protocol-declarations).

Verification for this addition matched all 2,411 pinned SCI rows, including 13
new reify probes, and passed 380 native integration tests plus four private
protocol tests. The 62 focused release tests, seven launcher tests (including the
opt-in opaque-object cases), strict default/all-feature Clippy and formatting
checks passed. Ordinary/instrumented reify benchmarks validated equal invocation
results and stable heap/slot counts; the instrumented GC benchmark retained zero
warmed collector allocations at each cadence. Costs and raw results are in the
[benchmark report](../../../benchmarks/interpreter.md#captured-reify-construction-and-invocation-2026-10-08).


## Native immutable record data

Twenty-five `sci-record-*` fixtures project nominal records through `into {}`,
scalar fields/predicates, metadata, equality and map/set key observations. They
cover constructors/basis fields, persistent updates, zero-field records, sequences,
reductions, walks, namespace aliases, redefinition, bounded printing and lazy-key
effects. Their independent sources and verified outcomes have explicit provenance.
No opaque record is flattened by transport: the existing SCI guard rejects records
before `map?`, the launcher also verifies projected records, and native transport
rejects records and type descriptors explicitly through its unsupported-value path.

Five executable dispositions preserve the selected native contract: saved native
map constructors follow the current generation (SCI retains the old one), including
basis changes; empty-vector map input is rejected (SCI accepts it); print depth/meta
controls follow the existing native collection rules (SCI prints the type prefix at
level zero and omits record metadata); and public `hash` remains outside the native
API inventory. Native hash equality is checked with host `value_hash` and source
map/set key behavior. Constructor metadata observations project the map argument
list's size to avoid pinning SCI's internal generated symbol spelling.

This data addition preceded record protocol clauses and named-record extension,
described below. The runtime contract documents saved constructors and the nominal
generation boundary.

Verification: all 2,436 pinned SCI outcomes matched, and all 397 native integration
tests passed. The 99 focused release tests, 16 interpreter unit tests, and seven
launcher tests passed (the normal opt-in tests remain ignored). The strengthened
record capture/GC test also passed in release after its final update. Formatting,
strict default/all-feature Clippy, map/record timing and allocation benchmarks,
the general interpreter benchmark and instrumented collector checks passed.
See the [record measurements](../../../benchmarks/interpreter.md#immutable-record-data-2026-10-08).

## Native record protocol methods

Nineteen independent `sci-record-method-*` fixtures observe receiver fields,
definition-time lexical captures, overloads, destructuring, parameter shadowing,
updated receivers on recur, metadata and returned closures. They include the
Box/Size/reify acceptance program, saved functions/Vars/apply/map, qualified name
collisions, partial/marker membership, old type generations, protocol redeclaration,
method Var rebinding, and exceptions/finally effects. Transport only sees scalars
and projected collections; records and descriptors remain forbidden objects.

Five explicit dispositions preserve native function/validation rules. SCI permits
undeclared method arities and later duplicate overloads; native rejects both before
publishing a type. Unknown methods receive a native qualified implementation error
instead of SCI's unresolved-symbol error. Native pre/postconditions use the ordinary
fn expansion with fields in scope; SCI rejects the recorded postcondition probe.
Repeated parameter names retain native fn's last-binding-wins rule, while an
independent receiver slot protects field reads; SCI keeps the receiver binding in
the recorded repeated-receiver case. These differences are executed assertions,
never counted as exact SCI matches by the API inventory.

Native tests also retain old/new record generations across source/context drops,
shared-owner forks and reentrant callback collection/redefinition. Returned closures,
metadata and cyclic atom fields keep exactly the required captures; repeated type
redefinitions settle at stable warmed live heap/code/source counts. Permission
revocation, retained analyzed forms, call depth, fuel, allocation, output and sticky
cancellation/finally use the normal invocation boundary. Type descriptors own method
closures once; instance construction does not copy methods or captures.

Verification matched all 2,455 pinned SCI cases and passed all 408 native
integration tests. The 123 focused release tests and 16 interpreter unit tests
passed (one normal opt-in unit benchmark remained ignored). The CLI acceptance
returned `[3 [4 5] 7]`. Formatting, strict default/all-feature Clippy, validated
record call/construction/definition benchmarks and the instrumented collector
checks passed. See the [method costs](../../../benchmarks/interpreter.md#record-methods-2026-10-08).

## Native named-record extension

Twenty independent `sci-extension-*` probes cover `extend-type` and
`extend-protocol` on named record generations. They observe existing instances,
saved method functions, grouped overloads, inline-method replacement, preservation
of omitted methods/other protocols, partial and marker membership, captured
lexicals without implicit field bindings, redefinition, namespace collisions,
recur, returned closures and exception/finally effects. Fixtures serialize only
scalar/projected results.

Five executable native dispositions preserve the deliberate boundaries: alias
record selectors resolve natively but fail analysis in pinned SCI; undeclared
arities and duplicate implementations fail before publication; unknown methods
use the qualified native implementation diagnostic; and nil selectors remain
explicitly unsupported even though SCI handles the source. Alias resolution is
native evidence, never labeled an exact SCI pass. The inventory now exposes both
macros and rejects nil/built-in selectors and host proxies separately.

Native checks cover full-target validation/publication under every small
allocation budget, declaration/table fuel, retained Forms and owner-shared fork
revision invalidation, isolated contexts, current permission checks, reentrant
replacement/GC, in-flight closures and reclamation of replaced captures/old
record generations. All normal callback exception, cancellation, output,
allocation, fuel and call-depth paths retain existing invocation/finally behavior.

Verification matched all 2,475 pinned SCI cases and passed all 419 native
integration tests. The 141 focused release tests and 16 interpreter unit tests
passed (normal opt-in tests remain ignored). The strengthened long-declaration
checks exercise both external macros in source and retained-analysis paths.
Formatting, strict default/all-feature Clippy and validated timing/allocation/GC
benchmarks passed. CLI acceptance returns `8`. See the
[extension costs](../../../benchmarks/interpreter.md#record-extensions-2026-10-09).
