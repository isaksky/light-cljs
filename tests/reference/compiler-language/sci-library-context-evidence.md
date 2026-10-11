# Complete original SCI library/import contracts

This bounded cohort retains 191 complete original SCI bodies: 34 for
`clojure.string`, 57 for `clojure.set`, 44 for `clojure.walk`, and 56 for
`clojure.edn`. The 197-candidate planning file retains every portable returned
library source. Six fixtures remain explicitly `not-run`: namespace
reflection/repeated require, two input-stream fixtures, and two record fixtures.
They need additional source-specific setup. Planning grants no execution credit.

The catalog retains original source bytes, corpus/disposition-row hashes,
profile, expectation, require declarations, and complete runtime bodies.
Leading original require forms remain compiler-top-level; declaration and
runtime slices reconstruct the complete adapted source. Twenty-four fixtures
use the existing structural JVM catch-selector bridge to `:default`; every
selector edit is recorded, with quoted data and reader strings unchanged.
Original expectations are never changed. The original sixteen entries retain
reference namespace identities; appended entries use `sci.library.case16`
through `case190`. Local `user` and per-case reference namespaces are explicit
fixture settings; returned identities are never renamed to force agreement.

The local Rust fixture compiles ordinary namespace applications with all four
libraries preinstalled and inspects results through public language predicates.
Every case owns a fresh MacroHost, Node process, and QuickJS heap. Four bounded
scoped workers preserve input order when joined. The Python runner partitions
execution into sixteen-source invocations, retains every result in order, and
applies the supplied timeout separately to each batch. A failed batch cannot
become passing evidence. A normal four-library Rust smoke test checks literal
values and callbacks in both engines. Fresh serial/four-worker observations
for the original sixteen fixtures were literally identical.

JSON frames split only on LF. Literal U+0085, U+2028 and U+2029 inside strings
remain original data; they are not interpreted as frame boundaries. Duplicate
JSON fields, missing/extra/reordered frames, incomplete outcomes, and non-JSON
numbers remain invalid. This fixes the earlier supplemental audit's transport
failure on two Unicode fixtures without changing sources, values or contracts.

## Fresh execution and receipts

```sh
python3 -B tests/reference/compiler-language/sci_library_context.py \
  --write-catalog --plan tests/reference/compiler-language/sci-library-context-plan.jsonl \
  --local --references --timeout 240 \
  --output /tmp/light-cljs-125-sci-library-context-191-current
python3 -B tests/reference/compiler-language/sci_library_context_receipts.py \
  --write-evidence /tmp/light-cljs-125-sci-library-context-191-current/evidence.json
python3 -B tests/reference/compiler-language/test_sci_library_context.py
python3 -B tests/reference/compiler-language/test_sci_library_ledger.py
python3 -B tests/reference/compiler-language/sci_library_context_receipts.py \
  --evidence /tmp/light-cljs-125-sci-library-context-191-current/evidence.json
```

The manifest-bound fresh run completed all 191 originals in both local engines
and both independently checked pinned compilers:

| Layer | Original expectation matches | Original expectation differs |
| --- | ---: | ---: |
| Local Node | 183 | 8 |
| Local QuickJS | 183 | 8 |
| Maven ClojureScript 1.12.134 | 163 | 28 |
| Checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` | 169 | 22 |

There are **163 four-way original agreements** and **28 unresolved original
contracts**. Every active layer is a complete execution observation; no
compilation rejection, missing engine or harness failure is promoted to a
pass. All eight local original differences also differ in both pinned
compilers; this cohort found no case where both pins meet the original
expectation and both local engines fail it. The unresolved contracts comprise
nine string cases, two set metadata cases, six walk metadata cases and eleven
EDN cases. All original result, callback and printing contracts remain intact.

The raw local JSON and pinned stdout/stderr case frames are retained in active
receipts, along with pinned build identity/settings output. All production
source/build inputs, the Python transport adapter, Rust driver, inspector,
compiler-owned libraries, reference oracle/helper, and recursive observation
adapter bind current evidence. Compiler/adapter changes during a batch reject
that entire batch. Both ClojureScript identities and `:optimizations :none`,
`:static-fns false`, `:def-emits-var false` are checked. The receipt validator's
current hash binds active records, and its writer retains each complete earlier
record as history. The original sixteen therefore retain both the earlier
successful active evidence and their nested initial shallow-bridge harness
failures. Historical failures grant no passing credit.

`sci-data-edn-tag-discard` remains **original-contract-unresolved**. Its original
source is `(clojure.edn/read-string {:readers {'tag observe}} "#_#tag 1 2")`.
SCI and both local engines return `2` with callback trace `[1]` and empty stdout.
Both pinned compilers throw with empty callbacks/stdout. The original
expectation and all four observations remain unchanged; its original
expectation status stays unresolved. This is a source-specific canonical
contract difference requiring explicit reconciliation, with no broad EDN
retirement or inferred missing production feature.

The six release/checkout differences concern `clojure.walk` list/sequence
metadata preservation in walk, prewalk and postwalk. They remain missing under
the complete original four-way agreement rule. All other original differences
also remain missing; compiler agreement with each other cannot replace the
retained SCI expectation. No new production implementation is claimed here.
Runtime execution and the inspection fixture do not grant public macro
eligibility.

## Full-run integration

`expression_rows` specializes only exact matching obligation, original source,
original row, disposition-body and expectation bindings. Unrelated generic
expressions remain present. The full report executes the 191 library fixtures
fresh through this specialized adapter and merges exact identities. The ledger
uses `full_report_evidence` to verify both local engines, both checked pins,
all current hashes, literal observations, complete frame identities and build
settings. Only original four-way agreements become covered. Any nonagreement,
incomplete engine, missing pin or stale proof remains missing; the library
adapter never automatically excuses a version difference. Stored receipts are
replay artifacts and grant no report coverage. Macro-phase coverage remains
`not-run` for these runtime contracts.

Twelve focused adapter/receipt tests cover immutable complete sources and
expectations, bounded planning, preserved discard callbacks and prior history,
strict Unicode framing, timeouts, compiler drift, exact specialization,
provenance mutations, and missing-engine/mismatch promotion attempts. The
separate full-ledger test checks static missing status, fresh agreement-only
coverage, unresolved contracts, absent phase credit, and a failing gate exit.
