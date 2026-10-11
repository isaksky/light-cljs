# Original SCI declaration and source-context receipts

This catalog retains 50 complete original SCI sources and expectations: 30
macro-declaration contexts, 12 namespace/source-metadata contexts, and eight
pinned-version differences. Original source bytes, corpus-row hashes,
disposition-row hashes, top-level form order and UTF-8 spans remain independent
of execution evidence. No original expectation or evaluator disposition changes.

The local driver supplies the original `user` namespace and a namespace-global
`observe` fixture. It analyzes the original forms at their original coordinates
and executes the emitted application in independent Node processes and fresh
QuickJS heaps. The reference oracle uses sequential JVM macro declarations
in a separate per-case host namespace, then analyzes original application forms
in `user`. `sci-vars-macro-analysis-scope` explicitly initializes its ordinary
`x` root in both host and application phases. Reference macro effects have
separate receipts and cannot disappear into runtime observations. Local
production macros use the project's canonical JavaScript implementation.

## Reviewed execution

```sh
cargo test --locked --test compiler_sci_source_context \
  original_source_macro_declarations_execute_phase_and_application_contracts \
  -- --exact --nocapture
python3 -B tests/reference/compiler-language/sci_source_context.py
python3 -B tests/reference/compiler-language/sci_source_context.py \
  --local --references --timeout 240 \
  --output /tmp/light-cljs-125-sci-original-contexts-reviewed-bound
python3 -B tests/reference/compiler-language/test_sci_source_context.py
python3 -B tests/reference/compiler-language/sci_source_context_receipts.py \
  --evidence /tmp/light-cljs-125-sci-original-contexts-reviewed-bound/evidence.json
```

All 50 original expectations pass locally in both Node and QuickJS. Maven
ClojureScript 1.12.134 has 40 passes, eight mismatches and two compilation
rejections. Checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` has 38 passes,
ten mismatches and two compilation rejections. Both reference identities and
`:optimizations :none`, `:static-fns false`, `:def-emits-var false` settings were
checked. All compiled reference cases have empty compiler-phase observation
receipts and exactly the expected stderr case-identity frames.

The immutable checked-in `sci-source-context-receipts.jsonl` contains:

- **35 original-source agreements:** all four engines/pins match the retained
  original outcome, ordered effects and stdout under the existing SCI
  Number/character/finite-list-vs-seq comparison projection.
- **Eight verified pinned-version differences:** both references executed the
  complete original source and yielded distinct raw contracts. The release and
  checkout split five/three and three/five passes/mismatches, respectively.
  These remain `original_expectation_status: unresolved`; verification of a
  difference does not grant an original SCI agreement.
- **Seven unresolved original contracts:** five raw result differences and
  two compilation rejections persist in both reference pins. These also remain
  nonpassing.

The eight version differences preserve initializer/default effect order,
sibling defaults, map `:as` defaults and keyword/symbol/string-key defaults.
Their literal values, callbacks, stdout and per-pin mismatch statuses stay in
the receipt. No universal destructuring contract is inferred from one version.

The seven unresolved originals are:

- `sci-macro-var-meta` and `sci-macro-multi-attrs`: both references reject
  application access to macro Var `m` (`Unable to resolve var: m in this
  context at line 1 sci-original.cljs`).
- `sci-macro-emit-defs`: both references return the literal list `(+ 40 2)`;
  the original SCI/local result is `42`.
- `sci-syntax-quote-dotted` and `sci-syntax-quote-qualified`: reference symbol
  qualification differs from the retained SCI symbol contracts.
- `sci-syntax-quote-metadata-vector`: both references add source-file and
  ending-position metadata beyond the retained SCI metadata map.
- `sci-binding-defn-arglist-conditions`: both references return `nil`, differing
  from the retained SCI arglist metadata result.

These may require explicit dialect or reference-fixture reconciliation. This
receipt does not classify them as implementation defects, retire their original
expectations or declare the broader gate passing. Fresh full-run integration
is described below; saved receipts alone never supply gate credit.

## Provenance and retained earlier evidence

Every active receipt binds the source/catalog, original row/expectation,
current Rust driver, local observation adapter, reference oracle, reference
helper and reference observation adapter hashes. The final run binds:

| Artifact | SHA-256 |
| --- | --- |
| Source-context catalog | `4cfbcaa84c03a9a5531eaede9a793accbe9058d61f278ae38233904a2c5fba3e` |
| Rust source-context driver | `44809ba6bb7c5cea177cf9fd7898cb22f837f5f65098b970a63426fa7535a873` |
| Local observation adapter | `8683da8f4ca1e0e8bbbae9180b1751ba8de25040d2995ab0fe3cbaa281bfc240` |
| Reference context oracle | `f780c3ebf6df47096db82e0150d6d312c17a572ce7eccf798c6a7fa516b1ab3c` |
| Reference oracle helper | `b4ddffb33f0a26721ebb455c45a290617f70f50eb1bf30e2b0f4768cc3cecbf8` |
| Reference observation adapter | `d3ef06a53a7c894f98bd38c397dcb5f5c3783cee3016949e1d50fe66d46e4e99` |

Receipts retain earlier raw layers and the SHA-256 of each originating evidence
file from `/tmp/light-cljs-125-sci-original-contexts`,
`/tmp/light-cljs-125-sci-original-contexts-final-local`,
`/tmp/light-cljs-125-sci-original-contexts-final-rejected`, and
`/tmp/light-cljs-125-sci-original-contexts-reviewed`. The first run's local
Rust-driver hash is `c0cc64d34c9c921afe366e4152437c5a9c5c7148df467429bd7508e6ba6b84f7`,
which differs from the reviewed driver. Its two original rejection layers lack
reference provenance. Those limitations stay visible in `prior_evidence`;
subsequent runs supply verified active provenance without editing that history.
Raw source inputs, output TSV/JSON, identity/settings markers, callback results
and stderr frames were checked against these recovered files. The final fresh
reference outcomes reproduce all prior literal outcomes and rejections.
Temporary build/log locations are optional replay artifacts; checked-in receipts
retain raw contracts and do not require those directories to validate.

Fifteen focused Python tests verify source/form/expectation preservation, stale
catalogs and provenance, incomplete execution, missing engines, distinct
version contracts, immutable pass classifications and literal raw-contract
stability even when the existing comparison projection would still agree.
They also reject replacing boolean expectations/settings with numeric values.
The focused Rust test covers raw macro arguments, repeated effects, sequential
macro redefinition, separate phase roots and original source locations in both
Node and QuickJS. It passed. No broad gate was run by this review.


## Fresh full-report source specialization

The full report now routes these same 50 original SCI identities through the
source-context driver. They overlap the generic expression catalog: immutable
obligation, original-source/row, disposition-body and expectation bindings are
checked before specializing them. The remaining 1,244 generic rows retain their
order and adapters; no source obligation is dropped or executed twice.

`report.py --scope intended-language --run --reference both` executes the 50
contexts afresh in `sci-source-context/` alongside the generic cohort. Its
per-row validator requires current Rust-driver/observation-adapter hashes,
both local engines, both pinned identities/settings and oracle/helper hashes.
The validator checks the actual outcome/effect/stdout observations against each
untouched original expectation. It never reads stored receipts to grant fresh
coverage. Static reports keep all 50 missing.

The fresh audit at `/tmp/light-cljs-125-sci-source-context-integrated-current`
reproduced every recorded literal reference outcome/rejection. Full-report
validation yields 35 covered originals, eight separately verified version
differences and seven missing originals. A version difference additionally
requires both local engines' entire projected observation to match the same
independently executed reference pin. Two distinct reference results cannot
excuse a local result that matches neither. The eight difference rows retain
`original_expectation_status: unresolved`; the seven unresolved originals keep
failing reference mismatches visible. Context execution grants no macro-phase
eligibility to other public API obligations. This focused run is not a broad
full-gate pass.


## Frozen-production source refresh — October 11, 2026

All 50 complete original source contexts were executed again on production HEAD
`a8e8a43fda48f4f22674c6f82ec3ee048c99c3fc`, in Node/QuickJS and both independently
checked pinned references. Every original source/setup/expectation binding is
unchanged. The fresh classification remains **35 original agreements, eight
verified pin differences and seven unresolved original contracts**. Both local
engines match all 50 SCI expectations; release matches 40, rejects two and
otherwise differs in eight; checkout matches 38, rejects two and otherwise
differs in ten. Neither pin rejection nor SCI/pin mismatch becomes an original
agreement. No unrelated public macro eligibility is inferred.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -B tests/reference/compiler-language/sci_source_context.py \
  --local --references --timeout 240 \
  --output /tmp/light-cljs-125-sci-source-context-50-a8e8a43
python3 -B tests/reference/compiler-language/sci_source_context_receipts.py \
  --evidence /tmp/light-cljs-125-sci-source-context-50-a8e8a43/evidence.json
```

Fresh evidence SHA-256:
`d3c3d6488f9b58bc000f98f537b06b3b2f9e9d39e3413b1bb280871c0602b7c7`.
Literal receipt replay passed. Active receipt layers now contain these actual
new executions; each complete earlier record is retained verbatim under
`prior_evidence`, including its prior historical raw snapshots. Additional
production-head/source/build hashes record the frozen observation context,
without relabeling older observations. The full runner still supplies fresh
layers and stored receipts never grant report coverage.
