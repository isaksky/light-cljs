# Canonical nonpositive anonymous placeholders — light-cljs-146

The compiler reader follows the anonymous argument registration used by
ClojureScript **1.12.134** and checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. In the pinned vendored tools.reader,
`garg`, `read-fn`, `register-arg` and `read-arg` at
`src/main/clojure/cljs/vendor/clojure/tools/reader.clj:530–589` establish the
contract: `%` aliases `%1`, positive positions fill missing parameters through
the largest position, and `%-1` shares `%&`'s rest symbol. `%0` and other negative
positions appear in the body with generated names but add no parameter binding.
Repeated positions reuse a symbol within one shorthand; separate shorthands
allocate fresh identities. Nested shorthand remains invalid.

## Independent pinned shape observations

The twelve exact expression sources and complete outcomes are in
`/tmp/light-cljs-146-shapes/{cases,evidence}.json`. Both references passed all
twelve. Each observation checks `fn*`, vector parameters and a sequential body,
then compares every parameter and body name after removing only its fresh
`__<id>#` suffix. All three form predicates are true; effects and stdout are
empty. Fresh suffix numbers are not a portable identity contract.

| Shorthand | Parameters | Body |
| --- | --- | --- |
| `#()` | `[]` | `[]` |
| `#(%0)` | `[]` | `[p0]` |
| `#(%-1)` | `[& rest]` | `[rest]` |
| `#(%-2)` | `[]` | `[p-2]` |
| `#(%0 %-1)` | `[& rest]` | `[p0 rest]` |
| `#(%0 %-2)` | `[]` | `[p0 p-2]` |
| `#(%-1 %& %-1 %&)` | `[& rest]` | `[rest rest rest rest]` |
| `#(% %3 %0 %-1 %-2 %& %3 %0)` | `[p1 p2 p3 & rest]` | `[p1 p3 p0 rest p-2 rest p3 p0]` |
| `#(%0 %0)` | `[]` | `[p0 p0]` |
| `#(%-2 %-2)` | `[]` | `[p-2 p-2]` |
| `#(%2 %0)` | `[p1 p2]` | `[p2 p0]` |
| `#(%& %0)` | `[& rest]` | `[rest p0]` |

Reference receipts retain `release 1.12.134` and the exact checkout identity,
with `:optimizations :none`, `:static-fns false`, and `:def-emits-var false`.
The checked reference runner verifies the chosen identity around compilation.

## Historical SCI reconciliation

The retained `sci-reader-anonymous-nonpositive` source is exactly:

```clojure
(let [f '#(%0 %-1)] [(count (second f)) (count (nth f 2))])
```

Its original SCI expectation remains `[0 2]`, with empty effects, stdout and
stderr. Its original fixture, all seven original disposition columns, catalog
expectation and independent identity hashes are unchanged:

| Identity | SHA-256 |
| --- | --- |
| Exact source | `532f3962d6223d79ee44e8264bb6f910a06b4d6042cf8464bb274efa22152bc1` |
| Original SCI row | `2c9518261c4a0c1e82097eeb870ec72fac1446ed542fb185f6f6e24c08685575` |
| Retained disposition body | `33d1e03325ef8e5e8423d8c19d357432badd1c8d97d0925ec603939084132829` |

`sci_execution.canonical_contract` explicitly binds this single row to canonical
`[2 2]`. The original SCI comparator continues to report its mismatch.
`full_ledger` independently compares the complete fresh observation against the
canonical expectation in Node, QuickJS and both references. It requires exact
source/setup hashes, pinned identities/settings, the current named local driver
and adapter hashes, and current macro-phase test evidence. Failed, missing,
stale, rejected or partial evidence leaves the obligation missing.

Successful reconciliation appears as `status: version-difference` and
`canonical_reconciliation.disposition: canonical-dialect-difference`, retaining
both literal expectations and every original layer status. The pins agree with
each other; this explicit difference is between canonical CLJS and historical
SCI. It contributes no `covered` SCI row.

The final scoped run in `/tmp/light-cljs-146-sci-final/evidence.json` observed
`[2 2]`, empty trace and stdout in both local engines and both pins. All four
original comparisons retain `mismatch` against historical `[0 2]`. Reference
stderr is empty and the source/setup hashes and identities match the catalog.
Reproduce this exact observation with:

```sh
python3 -B tests/reference/compiler-language/sci_execution.py \
  --ids sci-reader-anonymous-nonpositive --local --references \
  --output /tmp/light-cljs-146-sci-final
```

## Application, macro and source-location checks

`anonymous_nonpositive_bindings_execute_in_application_and_macro_phases` in
`tests/compiler_reader.rs` executes the exact retained source in both application
engines, and executes the complete
`tests/fixtures/reader-coverage/reader/nonpositive.cljs` shape/identity/rest
contract as both an application computation and a demanded QuickJS macro.
Every table row is checked through both source quotation and runtime
`read-string`, for 24 complete shape assertions in each phase. The runtime
reader retains its separate implementation and follows the same rest/unbound
position contract.
The macro mutation count proves that macro computation occurs once and its
state remains separate from application state. A normal hashed
`phase-witnesses.jsonl` mapping binds that complete test and fixture to the
retained SCI obligation.

Independent compiler-form tests retain shorthand and placeholder origins,
original source spans, shared `%-1`/`%&` identity, repeated-name identity, and
fresh identities between forms. Unbound `%0`/`%-2` analysis errors retain the
offending placeholder's location; nested or malformed shorthand still rejects.
Parser checks keep syntax representation separate from semantic reader expansion.
EvalSession checks cover exact quoted data, reused sessions, macro computation
and isolated rest invocation.

The current exact phase receipt is
`/tmp/light-cljs-146-phase/evidence.json`; its complete Rust source hash is
`7f9aa4dbeb85e106cc48596eba2ed34d0c6deed129efff567d8df64ccc7221b4`
and support fixture hash is
`2cb30fa001bb64978f134bba0c2db24dcff6bd7e6aca18fd67c3036f7849d37c`.
The live expanded ledger receipt,
`/tmp/light-cljs-146-ledger-receipt.json`, verifies the one canonical difference
with both pins, both application engines and this phase receipt. It retains
`strict_complete: false` and a selected scope.

## Verification

- Focused reader/session/parser checks pass 54 tests (19/21/14), followed by a
  passing final fresh-rest-identity provenance rerun. Logs:
  `/tmp/light-cljs-146-final-focused.log` and
  `/tmp/light-cljs-146-fresh-provenance.log`.
- The broader affected seven-suite run passes 178 tests; these overlap the
  focused checks. Log: `/tmp/light-cljs-146-affected.log`.
- All 85 Python inventory/report/SCI accounting tests pass, and regenerated
  inventory freshness passes. Logs: `/tmp/light-cljs-146-python.log` and
  `/tmp/light-cljs-146-inventory.log`.
- The normal evaluator accounting gate selects this canonical expectation by
  the exact historical body alone. A regression to `[0 2]`, changed effects or
  extra output fails. This row is counted as `canonical-dialect-difference`,
  excluded from portable/adapted SCI passes and matching-family totals. The
  narrow Rust accounting regression checks changed source/body and each full
  observation field. All historical disposition bytes remain unchanged.
- The complete 2,475-row evaluator accounting run reports 1,207 portable,
  304 adapted, one canonical difference, 877 gaps and 86 retirements. It exits
  **101** with two passing tests, one failing whole-corpus test and two explicit
  ignores. The nonpositive row succeeds. Six unrelated retained gap diagnostics
  differ from current macro diagnostics:
  `sci-macro-env-explicit`, `sci-macro-env-shadow`, `sci-macro-env-qualified`,
  `sci-macro-env-merged`, `sci-macro-identity-stop`, and
  `sci-deferred-lazy-macroexpand-head`. Their original expected diagnostic strings
  and failure criteria remain intact. Log:
  `/tmp/light-cljs-146-evaluator-accounting.log`. This scoped reader correction
  does not claim a passing whole-corpus evaluator gate. The existing
  light-cljs-125 diagnostic/SCI boundary reconciliation obligation retains these
  six diagnostics; the duplicate-owner lookup is saved in
  `/tmp/light-cljs-146-owner-search.json`.
- The compiler benchmark smoke run produces 423 samples across 47
  scenario/phase pairs. Raw data: `/tmp/light-cljs-146-perf-final.csv`.
  This bounded run supplies no comparative speedup or startup claim.
- Whitespace checking and the changed evaluator accounting file's Rust format
  check pass. The whole-repository `cargo fmt --all -- --check` still reports
  unrelated existing formatting differences; no broad reformat was applied.

This is the scoped nonpositive-placeholder reader contract. It establishes no
whole-language, whole-reader or full SCI conformance pass; light-cljs-125's
remaining obligations retain their own live evidence requirements.
