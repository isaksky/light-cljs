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
  --local --references --timeout 240 \
  --output /tmp/light-cljs-125-sci-library-context-191-a8e8a43
python3 -B tests/reference/compiler-language/sci_library_context_receipts.py \
  --write-evidence /tmp/light-cljs-125-sci-library-context-191-a8e8a43/evidence.json
python3 -B tests/reference/compiler-language/test_sci_library_context.py
python3 -B tests/reference/compiler-language/test_sci_library_ledger.py
python3 -B tests/reference/compiler-language/sci_library_context_receipts.py \
  --evidence /tmp/light-cljs-125-sci-library-context-191-a8e8a43/evidence.json
```

The manifest-bound fresh run at frozen production commit
`a8e8a43fda48f4f22674c6f82ec3ee048c99c3fc` completed all 191 originals in both
local engines and both independently checked pinned compilers:

| Layer | Original expectation matches | Original expectation differs |
| --- | ---: | ---: |
| Local Node | 183 | 8 |
| Local QuickJS | 183 | 8 |
| Maven ClojureScript 1.12.134 | 163 | 28 |
| Checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` | 169 | 22 |

There are **163 four-way original agreements**, **six verified pinned version
differences**, and **22 other unresolved original contracts**. The six version
differences retain unresolved SCI expectations; they do not become original
agreements. Every active layer is a complete execution observation; no
compilation rejection, missing engine or harness failure is promoted to a
pass. All eight local original differences also differ in both pinned
compilers; this cohort found no case where both pins meet the original
expectation and both local engines fail it. The 28 contracts that differ from
at least one original expectation comprise
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
record as history. This refresh actually executed every layer after production
source hashes changed; it did not rewrite hashes on historical observations.
The changed inputs since the previous active audit are `src/analyzer.rs`,
`src/compiler_roots.js`, `src/core_var_metadata.tsv`, `src/emitter.rs`, and
`src/js_runtime.js`. Their current hashes, and all other source/build hashes,
remained equal to the frozen starting snapshot throughout this audit. Catalog,
adapter, driver, original fixtures and tests were unchanged for the refresh.
All 191 prior records remain verbatim in `prior_evidence`. The original sixteen
therefore retain the earlier successful active evidence and their nested initial
shallow-bridge harness
failures. Historical failures grant no passing credit.

`sci-data-edn-tag-discard` remains **original-contract-unresolved**. Its original
source is `(clojure.edn/read-string {:readers {'tag observe}} "#_#tag 1 2")`.
SCI and both local engines return `2` with callback trace `[1]` and empty stdout.
Both pinned compilers throw with empty callbacks/stdout. The original
expectation and all four observations remain unchanged; its original
expectation status stays unresolved. This is a source-specific canonical
contract difference requiring explicit reconciliation, with no broad EDN
retirement or inferred missing production feature.

The only explicitly reconciled release/checkout differences are
`sci-data-walk-walk-list-meta`, `sci-data-walk-walk-seq-meta`,
`sci-data-walk-prewalk-list-meta`, `sci-data-walk-prewalk-seq-meta`,
`sci-data-walk-postwalk-list-meta`, and `sci-data-walk-postwalk-seq-meta`.
The release rebuilds list/seq forms without metadata; the checked checkout
wraps both rebuilt forms in `with-meta` using the original metadata. Prewalk
and postwalk recurse through those branches. The actual release
`clojure/walk.cljs` JAR entry hashes to
`79859e8103b3a5c0ea6b8be5ed3bab7f6b2c17569f6770182f77480aec4e5234`;
the checkout source hashes to
`52d33a5c841ce959eb2cb10aacb76b2d1bf06e93ef3ae8d30a05dc69ceb34378`.
Each of the six must retain its literal original `{:m :ok}` expectation,
execute a release observation of `nil` and a distinct checkout observation of
`{:m :ok}`, and execute both local engines with the same checkout observation.
All three observations also require empty callbacks and stdout. Source hashes,
raw frames, pin identities and settings are verified against current inputs.
Missing, identical or changed pin observations cannot get this classification.
Their report status is `version-difference`, reconciliation is
`verified-pinned-version-difference`, and original expectation status remains
`unresolved`. The other 22 differences remain missing; no automatic version
classification applies to them. No new production implementation is claimed here.
Runtime execution and the inspection fixture do not grant public macro
eligibility.

## Full-run integration

`expression_rows` specializes only exact matching obligation, original source,
original row, disposition-body and expectation bindings. Unrelated generic
expressions remain present. The full report executes the 191 library fixtures
fresh through this specialized adapter and merges exact identities. The ledger
uses `full_report_evidence` to verify both local engines, both checked pins,
all current hashes, literal observations, complete frame identities and build
settings. Only original four-way agreements become covered. The exact six
reviewed walk contracts can become verified version differences through
`version_contract` and `full_report_evidence`; their SCI expectations remain
unresolved. Any other nonagreement, incomplete engine, missing pin, modified
canonical contract or stale proof remains missing. Stored receipts are
replay artifacts and grant no report coverage. Macro-phase coverage remains
`not-run` for these runtime contracts.

Fourteen focused adapter/receipt tests cover immutable complete sources and
expectations, bounded planning, preserved discard callbacks and prior history,
strict Unicode framing, timeouts, compiler drift, exact specialization,
provenance mutations, missing-engine/mismatch promotion attempts, the six
literal version contracts, and rejection of automatic reconciliation for other
library differences. The separate full-ledger test checks static missing
status, fresh agreement coverage, exact reviewed version differences,
unresolved SCI contracts, absent phase credit, and a failing gate exit.

## Fresh audit after stream/join/project repairs — October 11, 2026

All 191 complete originals were reexecuted at frozen production
`9964c3c79bd2cbb3acba57b79c977be37390126f`, against both local engines and both
identity/settings-checked pins. Every complete previous record and source binding
remains verbatim in prior_evidence. All production input hashes match the retained
active audit from the completed fixes. The actual new layers give
**180 local original matches / 11 mismatches**, **163 release matches / 28
mismatches**, and **169 checkout matches / 22 mismatches**. Reconciliation remains
**163 original agreements, six exact reviewed walk pin differences, 22 other
unresolved originals**. The three repaired join/project sources now match both
canonical pins but differ from their original SCI expectations, which remain
unchanged and unresolved; their fresh literal canonical resolution and previous
failures are separate in `../compiler-library-unresolved-probes.json`.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -B tests/reference/compiler-language/sci_library_context.py --local --references --timeout 240 --output /tmp/light-cljs-125-sci-library-context-191-9964c3c
python3 -B tests/reference/compiler-language/sci_library_context_receipts.py --write-evidence /tmp/light-cljs-125-sci-library-context-191-9964c3c/evidence.json
python3 -B tests/reference/compiler-language/sci_library_context_receipts.py --evidence /tmp/light-cljs-125-sci-library-context-191-9964c3c/evidence.json
```

Fresh evidence SHA-256 `9ff8a88dd75e67162c54c8a6633787dcfd65ae1113d4d05885e24960b71ede30`.
Stored receipts grant no report pass or unrelated macro eligibility; all original
contracts, version contracts and strict acceptance are unchanged.
