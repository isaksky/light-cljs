# Complete original SCI reader conditional source contexts

The separate diagnostic adapter preserves all 30 original returned SCI
reader-conditional sources and historical `:clj` expectations. Its actual source
filename is `.cljc`, selecting the adopted `:cljs` feature in both the local
namespace compiler and both real upstream compilers. Neither branches nor
original source bytes are rewritten. The frozen catalog retains original source,
row, disposition/body and expectation hashes. It contains 20 independently
reviewed canonical literals and three focused public contrasts. It changes no
shared source/API ledger or expectation.

Application source places the complete original expression in `(do ORIGINAL)`
inside `run`. Macro source places the same complete expression in a demanded
`upstream` macro and quotes its finite result. Fresh and reused actual
`MacroHost`s compile genuine `user.cljc` files; reused hosts are warmed with a
real independent macro. The compiler-emitted application output is evaluated
in QuickJS and Node for all four contexts. Every source/compiled JS/raw outcome
and reused warmup has a retained hash. Compilation rejections and observation
failures remain failures. Pure source expressions have empty effects/output;
the adapter does not substitute predicate or reader results.

The reference oracle reads the unchanged original bytes with genuine
`cljs.analyzer/forms-seq*` and filename `sci-original.cljc`, analyzes each selected
form in order in `user`, and uses real pin core/emitter/reader code. It explicitly
asserts `optimizations=none`, `static-fns=false`, `def-emits-var=false` in each
actual source analysis. The shared genuine reference helper also checks compiler
identity, a clean exact checkout and bootstrap settings. Each expression is
observed in an isolated Node process; unsupported object transport cannot abort
or credit another row. No reference JVM macro companion is passed to the local
compiler.

Fresh current producer execution at
`/tmp/light-cljs-125-original-reader-cljc-v4/evidence.json` yields:

| Scope | Count | Evidence |
| --- | ---: | --- |
| Original historical agreement | 2 | All eight local phase/engine frames and both pins match original literals |
| Exact canonical `:cljs` differences | 18 | All eight local frames and both pins match reviewed different literals |
| Confirmed public reader contrasts | 3 | Both pins match canonical literals; current locals differ or reject |
| Other unresolved originals | 7 | No passing semantic/phase credit |

The full receipt SHA256 is
`1849f51111d31fe835cad2168e46cbe5aca9756fcadff68a56223521212f72be`.
`sci-cljc-source-probe-receipt.json` is its byte-exact durable copy. Each pin's
raw comparison against unchanged SCI originals is three passes, 22 mismatches,
two observation errors and three reader rejections. The third pin-original
pass (`metadata-no-match`) still has a local strict unresolved-symbol rejection,
so it is not an original agreement across all layers.

The 20 rows with complete diagnostic observation agreement are:

| `sci-context-reader-` suffix | Exact canonical value | Original agreement |
| --- | --- | --- |
| default | `[1 5]` | no |
| discard-no-match-child | `[2 3]` | no |
| discard-quote-no-match | `[2 3]` | no |
| feature-clj | `3` | no |
| map-splice | `{}` | no |
| nested | `[2]` | no |
| nil | `[1 false]` | no |
| no-match | `42` | yes |
| no-match-eof | `1` | no |
| quote-no-match | `2` | yes |
| quote-no-match-child | `[(quote 1) 2 3]` | no |
| splice-empty | `[0 1 2]` | no |
| splice-list | `(0 3)` | no |
| splice-map | `{}` | no |
| splice-map-entries | `[]` | no |
| splice-nested | `[0 4]` | no |
| splice-no-reflatten | `[]` | no |
| splice-scalars | `[]` | no |
| splice-set | `#{0}` | no |
| splice-vector | `[0 3]` | no |

These are explicit source-feature differences under the adopted `:cljs` dialect,
not a replacement of historical SCI expectations or automatic general reader
retirement. Both original agreements remain separate from those 18 differences.
No source assertions, public API arities or unrelated reader forms acquire
credit from these cases.

## Focused missing public behavior

A coherent owner scope is **Preserve canonical inactive reader-conditional
handling under discard, metadata and quote prefixes**. The following complete
unchanged expressions execute with both pins under `.cljc` / `:cljs`; no focused
production repair was performed:

| Complete original source | Both pins | Current local evidence |
| --- | --- | --- |
| `[#_ #?@(:clj [1 2]) 3]` | `[]` | `[3]` in all eight ordinary/actual macro engine frames |
| `'[0 ^:private #?@(:clj [x y]) z]` | `[0 z]` | Four located compile/phase rejections: reader prefix requires one selected form |
| `'[0 '#?@(:clj [1 2]) 3]` | `[0 (quote 3)]` | Same four located compile/phase rejections |

`sci-cljc-inactive-prefix-gap-receipt.json` retains exact original fields,
complete source/row/body hashes, checked current compiler/driver/oracle hashes,
whole-source local/pin proof and canonical literals for all three. Its SHA256 is
`0d7c3808cc3041a73a7b2c03182875007eecb2a8954e77b8154682d0e6390faf`.
Parent should duplicate-check and assign one focused owner after reviewing
whether the common inactive-prefix consumption scope remains coherent. The
three source rows stay missing until that owner delivers canonical behavior;
queue creation and scope categorization supply no passes.

The seven other unresolved originals are `metadata-no-match`,
`skipped-arguments`, `skipped-regex-bigint`, `skipped-regex-identity`,
`skipped-tag`, `skipped-tag-identity`, and `splice-map-key`. The first uses an
undeclared `y` in an extra `meta` argument and needs exact strict source/arity
reconciliation. `skipped-arguments` selects `%3` with only two supplied inputs:
canonical applications return `[1 undefined]`, and macro form export rejects
undefined. The two active regex sources fail the finite observation transport.
The two active unknown tags and odd selected map reject in both pins. They all
retain zero source/semantic credit; active malformed or unexportable values do
not count as passing reader behavior.

## Reproduction and parent integration

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -B tests/reference/compiler-language/sci_cljc_source_probe.py --output /tmp/reader-cljc-new
PYTHONDONTWRITEBYTECODE=1 python3 -B -m unittest discover -s tests/reference/compiler-language -p test_sci_cljc_source_probe.py -v
rustfmt --edition 2024 --check tests/compiler_sci_cljc_source_probe.rs
```

Six independent guards pass. They freeze original source/row/expected bytes and
canonical literals, preserve the whole expression in both wrappers, require
both pins and all actual phase/engine frames, and refuse credit after a missing,
failed or mismatching phase/pin. Fabricated guard frames test accounting only;
they never supply semantic evidence. Earlier unsuccessful v1 environment-call
and v2 unsupported-observation attempts remain retained below `/tmp`; v3 fixed
transport and v4 freezes reviewed original/canonical literals and checks every
actual reference analysis setting. No earlier failure is replaced with a pass.

Parent retains this as diagnostic evidence only, with zero shared source or
phase acceptance credit for all 30 originals. The two exact original agreements
and 18 complete canonical differences are actual observed outcomes, not accepted
full-ledger witnesses: this diagnostic does not independently replay every raw
artifact or execution-binary producer. A robust accepted adapter and fail-closed
producer/artifact replayer are still required before any positive integration.
The three public contrasts and seven other unresolved rows stay nonpassing.
Stored receipts grant no fresh fallback and no blanket SCI or reader exclusion.
No shared report/ledger/inventory/witness table was edited by this subagent.
The separate inference optimizer scope work applies to its sole exact original
body and grants no execution/phase credit. Full-language strict acceptance and
all external blocked verification remain incomplete.


## Live focused reader owner

Fresh duplicate searches for open reader and conditional work were empty.
The three coherent public contrasts now have focused ordinary owner
**light-cljs-172, Preserve canonical inactive reader-conditional handling under
discard, metadata and quote prefixes**, ordered immediately before 125. Its
full description retains these exact originals, both-pin outcomes, local proof,
producer hashes and acceptance controls. No reader production repair is part
of this evidence checkpoint. All thirty diagnostic rows still supply zero
shared source or phase acceptance credit pending a robust accepted adapter.
