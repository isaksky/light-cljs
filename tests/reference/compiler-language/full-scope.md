# Full intended-language ledger

`report.py` defaults to **intended-language**. Its `--strict` gate currently
fails: implemented or historically tested behavior without a faithful execution
mapping remains missing. This ledger is accounting evidence, not a parallel task
queue. Pellets owns implementation work and the report identifies its owners.

```sh
python3 -B tests/reference/compiler-language/extract_inventory.py
python3 -B tests/reference/compiler-language/extract_inventory.py --check
cargo test --locked --test compiler_language --test provenance
python3 -B tests/reference/compiler-language/report.py --strict --output /tmp/full-language.json
# Preserve the exact old batch and its focused completion semantics:
python3 -B tests/reference/compiler-language/report.py --scope historical-selected --run --strict
```

## Independently sourced obligations

The source universe comes from the pinned Git tree at
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, rather than a list of passing fixtures
or `CoreFnId` spellings. `full-inventory.jsonl` and `full-bodies.jsonl` retain
**97 complete runtime/reference files, 1,328 complete forms, and 4,728 assertion
sites**. All `.cljs`, `.cljc`, and `.clj` files under `src/test/cljs/` are retained,
plus the historical analyzer/compiler files. `support-sources.jsonl` retains the
complete remaining JavaScript/EDN/auxiliary fixtures, including setup/effects.
Files without assertion macros, such as `invoke_test.cljs`, have independent
source-program obligations; assertion-free test bodies cannot become invented
passing assertions. Hashing, hash-map/hash-set, set equivalence, invocation,
iteration, printing, parsing, walk, reader, Vars, macros, and exceptions remain
visible. Whole helper/setup/loop/effect bodies and original expectations survive.

`obligations.jsonl` independently extracts declarations and signatures from
pinned `core.cljs`, `core.cljc`, `clojure.string`, `clojure.set`, `clojure.walk`,
and `cljs.reader`; protocol methods and source-generated type/record constructors
are separate rows. Fixed and variadic arities retain original parameter vectors;
unknown imported signatures remain explicitly unresolved. Syntax comes from the
pinned analyzer's `specials` dispatch set. Complete source and container spans,
body hashes, original license notices, and revision provenance are retained.

Declaration counts are source/branch/signature rows, not unique implementations
or passes: the full inventory contains **1,327 intended public declaration/arity
rows**. Explicit private declarations, named upstream storage helpers, optional
lite representations, JVM macro-import plumbing, and stated ecosystem boundaries
have reasoned dispositions. Ordinary missing standard core operations stay
intended. Public constructors and protocols are not excluded merely for being
implemented internally by upstream storage machinery.

The local `CoreFnId` table and `core_var_metadata.tsv` are independently hashed
cross-checks. Public rows include a static-registration hint or `unmapped`, which
establishes no supported arity or execution. Local metadata, supported API tables,
and original/migrated SCI fixture rows retain their own exact source/hash identity.
Retired numeric/native owner APIs and JVM-dependent SCI observations retain their
reasoned dialect dispositions. Historical SCI/metadata values never supply a
current execution pass.

## Evidence columns and completion

Every source-level report row has six explicit columns:

| Column | Evidence boundary |
| --- | --- |
| `parse` | Individual witness parser invocation; whole parser-corpus acceptance is separate. |
| `analysis_lowering` | Located diagnostic/warning check, or successful compilation preceding execution; never proof of all public arities. |
| `native` | `removed`: retained native observations are historical, following light-cljs-115. |
| `canonical_js` | Compiler-emitted JavaScript executed by the checked Node harness; rejected, skipped, timed-out and unavailable observations cannot pass. |
| `quickjs_macro` | Exact phase-specific execution test; ordinary runtime/source passes do not imply eligibility during macro expansion. |
| `execution_evidence` | Original value/effect/stdout contracts and both checked references; warnings/rejections/version differences stay separate. |

Absent evidence is `not-run`. No semantic pass is asserted from static source
inspection. Macro/import/phase differences are explicit local contracts, rather
than demands for unchanged JVM companion behavior. QuickJS engine availability
alone establishes no public operation's macro eligibility.

`full-cases.jsonl` and `full-witnesses.jsonl` accept further expression/namespace
adapters through the same body/hash/expectation/provenance validation and execution
runner as historical witnesses. They now include fourteen complete transient body adapters. `obligation-witnesses.jsonl`
maps a public name/arity to complete body assertion execution mappings, with its
exact declaration hash, signature hash, and a reviewed faithfulness rationale.
A selected body subset cannot satisfy an API obligation. API registrations and
single parser examples are not allowed to replace these mappings.

`phase-witnesses.jsonl` maps an obligation to a named Rust macro execution test,
its complete test-source hash (including helpers), declaration/signature hashes,
and a reviewed phase/arity rationale. The runner executes exactly that one test
and requires Cargo's one-test/one-pass/zero-ignored outcome. Zero tests, failures,
timeouts, unavailable tools or stale source cannot establish macro eligibility.
No phase witnesses are inferred from test names or existing implementation.
Public API completion requires both ordinary operation evidence and this phase
evidence. The phase catalog binds all 27 transient public name/arity rows and the three
`sequence` arities, plus `Eduction` and `eduction`, to their canonical macro
contracts; a binding alone still supplies no passing observation. The complete `test-cljs-2028` body supplies an
additional transducing-sequence witness. The complete `test-cljs-1209` body retains
its original `my-conj` setup and binds eduction reduction. Its independent audit
measures repeat effects, interleaving and completion against both references.
Full upstream bodies mixing sequence,
eduction and flatten remain subject to their assigned families; passing focused
sequence tests does not complete those mixed bodies.

A full-scope selection cannot claim completion when `--feature` or `--id` selects
a subset, even if every selected witness passes. Independently sourced API/arity,
syntax, local API/SCI and assertion-free module obligations still remain required.
Historical focused gates retain their earlier explicitly named scope.

## Ownership and boundaries

| Family | Responsible Pellet |
| --- | --- |
| Transients and public mutation protocols | light-cljs-117 |
| Transducer constructors/completion | light-cljs-118 |
| Sequence transforms / eduction | light-cljs-59 / light-cljs-60 |
| Multimethods and hierarchies | light-cljs-47 |
| Sorted collection/comparator contracts | light-cljs-119 |
| Hash/equality/metadata | light-cljs-120 |
| Sequence/chunk/iterator/lazy contracts | light-cljs-121 |
| Regex / standard libraries and EDN | light-cljs-49 / light-cljs-123 |
| Core parsing/data/array utilities | light-cljs-122 |
| Printing/reflection/host hooks | light-cljs-124 |
| Reader/source spelling | light-cljs-9 |
| Residual public families, full execution/provenance completion | light-cljs-125 |

Owners indicate the responsible family record; a closed historical implementation
record is not evidence that every expanded public obligation passed. The
completion record reconciles residual gaps in already delivered families. No new
engine/language implementation is performed by this ledger change.

Browser/npm/Closure integration, standalone spec/pprint/test/math/datafy/reducers,
async/proxy libraries, optional lite collections, and upstream private storage or
analyzer/compiler representations have the explicit scope bases adopted in the
language roadmap. All source remains available for reviewing those dispositions.
Transients, transducers, sorted collections and multimethods are **not** permanent
exclusions. The old 18-file manifest remains **3,237 sites: 2,535 selected and 702
unrelated**, including its original assertion/body hashes and witness provenance.

CI checks reproducibility and failure accounting, publishes the full scope's
visible incomplete report, and continues to require the historical semantic gate.
It does not label a known full-scope failure a language-conformance pass. The
broader completion Pellet owns requiring the full execution gate once its missing
obligations and phase witnesses have actually been supplied.

## Verification receipt (2026-10-11)

Source extraction used the clean pinned checkout above. Node was `v24.18.0` and
Rust `1.99.0`; the checked release oracle remained ClojureScript `1.12.134`.

- `python3 -B tests/reference/compiler-language/extract_inventory.py --check`:
  byte-identical historical/full inventories, bodies, public obligations and
  auxiliary sources. Historical source counts and all original body/assertion
  hashes were preserved.
- `cargo test --locked --test compiler_language --test provenance`: passed;
  accounting covers 18 extractor, 41 historical runner and 11 full-ledger tests,
  including failed/skipped/unavailable/timed-out/missing/subset and macro zero-test
  injection. Both provenance tests passed.
- `cargo check --locked --all-features`: passed.
- `cargo test --locked --test parser_reference --test sci_reference`: four tests
  passed; the explicit SCI/JVM execution test was ignored. These structural/parser
  checks do not increase semantic coverage.
- `rustfmt --check tests/compiler_language.rs` and `git diff --check`: passed.
  Whole-repository `cargo fmt -- --check` encountered existing formatting drift
  in untouched engine and test files; this accounting change preserves them.
- `python3 -B tests/reference/compiler-language/report.py --strict --output
  /tmp/light-cljs-116-full-final.json`: exit **1**, correctly incomplete with
  **8,206 missing obligations**, zero executed passes and 1,366 explicit unrelated
  source/internal/dialect/host dispositions. Every row has all six columns and an
  existing family owner. This is the static baseline, not a semantic test result.
- `python3 -B tests/reference/compiler-language/report.py --run --no-build
  --binary target/debug/light-cljs --id
  primitives_test.cljs::test-equiv::assertion-002 --reference both --strict
  --artifacts /tmp/light-cljs-116-execution-final`: the selected source witness
  passed QuickJS evaluation, emitted JavaScript/Node, release and checkout
  observations. All **27** supplemental boundary contracts verified, including
  both deliberate macro phase/import differences. Exit **1** correctly retained
  full-scope incompleteness: the independent source/API/arity/phase obligations remain missing in this
  selected report and `full_scope_selected` is false. No source/API/arity/phase
  completion is inferred from this passing subset.

The first direct run exposed stale native macro diagnostic text and a retired
local JVM-companion import fixture. Exact canonical rejections now retain the old
text as historical provenance. The duplicate-alias local fixture uses the single
ordinary import clause and `.cljs` macro definition; all duplicate specifications,
qualified/referred calls and identity/value observations remain. Its original
`.clj` fixture and source/tree hashes still run against both upstream references.

The selected execution receipt predates the final inheritance of explicit SCI/native retirement dispositions; this disposition-only reconciliation changes no source witness or boundary observation.

The full historical 18-file execution run, complete upstream runtime suite,
explicit SCI/JVM regeneration, hosted CI and runtime benchmarks were not rerun for
this ledger change. No new source-independent API or macro-phase semantic pass is
claimed; the checked expanded adapters begin empty. Family records retain links
to each assigned source/name/arity lead, with residual execution completion owned
by light-cljs-125.
