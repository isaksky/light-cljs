# Compiler-backed evaluator accounting of historical SCI cases

This is a migration execution gate and an explicit unresolved-gap report, not
whole-language completion or permission to remove supported behavior.
`tests/evaluator_conformance.rs` enumerates all **2,475** original rows of
`tests/fixtures/sci-outcomes.tsv`. It executes each disposition through a fresh
`EvalSession`, with no native interpreter or adapter in the build. The adapter uses
immutable `Datum` for host observations and finite result export; it imports no
native interpreter types. Results, ordered `observe` effects, stdout and stderr
are compared separately, including completed effects before failure.

| Disposition | Rows | Evidence |
| --- | ---: | --- |
| portable | 1,185 | Original source agrees with the documented CLJS comparison projection |
| adapted | 304 | Recorded source explicitly imports SCI-preinstalled libraries and/or replaces JVM catch selectors with `:default`; projected observation agrees |
| gap | 901 | Current evaluator observation is recorded, but does **not** satisfy the SCI comparison |
| retired | 85 | Exact JVM numeric, configured `:bb` reader or native `.clj` loading contract is outside the settled compiler-backed target |

Only **1,489** matching observations contribute to conformance coverage. The 901
gaps include 305 cases that emit an owned diagnostic but lack a matching SCI
phase/category transport contract. The other 596 entries require language or
dialect review; their recorded baselines are **not accepted language semantics**.
An accounting test passing means those unresolved cases remain visible and their
current observations remain reproducible. It cannot establish that migration is
complete. The repaired `sci-state-volatile-order` and `sci-state-volatile-two-targets`
execute unchanged source and match the original values and ordered effects
under the Number projection after **light-cljs-126**.

The matching rows span the entire corpus, including 175 lazy, 141 transducer
constructor, 106 control, 96 binding/destructuring, 94 collection, 73 sequence,
66 iteration, 60 string, 159 set/walk/EDN data, 56 state, 32 IO, 26 transduce,
25 record, 23 exception and 20 macro cases. All 12 loop and all eight letfn
observations match. Cardinality checks require at least 1,000 unchanged-source
portable rows and at least 30 matching rows each in nine substantial families;
a selected whitelist cannot silently replace this coverage.

## Original observations, adaptations and ownership

`evaluator-dispositions.tsv` stores fourteen tab-separated fields:

1. The original seven SCI fields **verbatim**: ID, profile, source, outcome,
   effects, stdout, stderr.
2. Status, explicit evaluator source, evaluator outcome, effects, stdout, stderr.
3. Reason with `owner=`, `kind=` and `family=` tags.

The test verifies one and only one disposition for each original ID, rejects
orphans/duplicates/stale observations, and verifies every case still has its
existing origin in `tests/reference/fixture-origins.tsv`. The original upstream
pin, test anchor, adaptation rationale and independent-source attribution remain
there. This manifest is additional local execution evidence; it makes no new
claim of deriving Rust implementation from SCI. The historical oracle file is unchanged; the SCI launcher uses `--evaluator` for canonical execution.

Import adaptations use the ordinary first-party compiler loader for
`clojure.string`, `clojure.set`, `clojure.walk` and `clojure.edn`; SCI had
preinstalled those namespaces. Catch adaptations explicitly replace JVM
`Exception`, `Throwable` and `clojure.lang.ExceptionInfo` selectors with
`:default`. They preserve the recorded execution path's values/effects/output,
and do **not** assert JVM subclass dispatch. Every changed source is visible in
its row; source text is never silently substituted by the normal gate.

The current live owners inspected with `pl --json list` are recorded per case:

| Owner | Unresolved rows | Responsibility |
| --- | ---: | --- |
| light-cljs-116 | 305 | Explicit error-transport accounting; SCI categories are not inferred from arbitrary exception messages |
| light-cljs-125 | 219 | Residual state/function/control/protocol/record/library integration review |
| light-cljs-121 | 122 | Sequence/lazy/iteration and finite-demand review |
| light-cljs-124 | 110 | Printing, namespace/Var reflection and host hooks |
| light-cljs-9 | 63 | Reader/contextual/syntax-quote review |
| light-cljs-49 | 57 | JVM versus ECMAScript string/regex/coercion review |
| light-cljs-122 | 23 | Core numeric/data utility review |
| light-cljs-120 | 3 | Equality/metadata review |

These links assign evidence review; they do not defer current migration
requirements automatically. The existing library port light-cljs-123 is closed;
any residual promised-library regression discovered here still needs resolution
before claiming its replacement complete. No queue records were changed.

## Explicit comparison and resource contracts

The comparison projects safe JVM integers (absolute value at most
9,007,199,254,740,991) and doubles to IEEE-754 JavaScript Number bits. It preserves
negative zero and canonicalizes NaNs. Larger integer, ratio, BigInt and Decimal
observations are rejected from the portable projection rather than rounded into
a pass. JVM characters project to exact one-unit UTF-16 strings. Exported finite
sequences project to readable lists; vectors remain vectors, map/set contents
remain unordered, and metadata/function identity are outside SCI v1 as before.
UTF-16 strings, including lone surrogates, compare exactly. Passing rows must
match this projection of the **original** SCI observation; a new evaluator
expectation cannot override a failed comparison into a pass.

Evaluator failures are `(diagnostic (string ...))`; unsupported finite-data
exports are `(transport (string ...))`. They retain the first owned message
line, excluding incidental QuickJS stack frames whose process-global script
numbers change between runs. These formats intentionally do not invent SCI
read/analysis/execution categories.

Ordinary rows retain the evaluator's default 1,000,000-poll budget and a
30-second backup deadline, including cold compiler bootstrap. Only
`sci-recur-operand-failure` cancels after 256 observation events; this nonpassing
probe currently loops. The repaired `sci-deferred-lazy-partial-infinite` uses
the ordinary default budget and returns true after exactly five observations.
Only `sci-deferred-lazy-self` and `sci-deferred-lazy-mutual` use a three-second
backup deadline. Their deadline or poll-budget termination is represented as
`(resource-bound lazy-cycle)`, with the complete observed effects retained. This
explicit nonpassing resource outcome avoids comparing machine speed, and does
not normalize ordinary evaluator errors. The three remaining resource probes
stay owned language gaps.
All returned collection exports retain the bounded compiler form bridge. No
infinite result is counted as successful finite transport.

## Run and audit

```sh
LIGHT_CLJS_DISABLE_NATIVE_EVAL=1 \
  cargo test --locked --test evaluator_conformance -- --nocapture

# Explicit audit reports go to the system temporary directory. They never
# update committed expectations or claim that nonmatching cases passed.
cargo test --locked --test evaluator_conformance \
  audit_historical_corpus_with_compiler_evaluator -- --ignored --exact --nocapture
cargo test --locked --test evaluator_conformance \
  audit_explicit_library_import_adaptations -- --ignored --exact --nocapture
```

The ordinary gate needs QuickJS's bundled runtime and the Rust toolchain; it
does not run Node/JVM SCI or fetch an upstream checkout. The main pinned SCI
regeneration job remains the separate explicit command in the parent README.
Audit reports require review before changing any status or baseline. A formerly
nonpassing row that now matches the projection fails accounting until its
disposition is migrated, preventing stale gap classifications from hiding new
coverage.

### Unchecked-array macro environment snapshots, 2026-10-11

The broad regression at `9ed35d8` reported stale evaluator snapshots for exactly
`sci-binding-defn-env` and `sci-binding-fn-self-env`; the diagnostic remains in
`/tmp/light-cljs-133-all-binaries.log`, lines 317–328. An exact-source audit for
light-cljs-136 at starting commit `0f90430` confirmed the added
`:light-cljs/unchecked-arrays` key documented by light-cljs-132. The reviewed
observations are retained in `/tmp/light-cljs-136-env-audit.tsv`.

Only those two current evaluator key-set observations and their explanations
changed. Their original seven SCI fields and fixture origins remain unchanged.
Both remain `gap`, owned by light-cljs-125: SCI's flat lexical environment differs
from the documented compiler map with `:locals` binding records. Refreshing the
snapshot establishes reproducibility, not SCI conformance.

The independent local test
`compiler_array_settings::named_function_macro_environments_capture_boolean_array_settings`
executes source macros in both `defn` and self-named `fn` bodies. It checks the
exact environment keys, Boolean setting, return context, argument records and
presence/absence of the lexical self. Default false, source-file true and reset
false are captured during definition and observed after resetting the setting,
in independent QuickJS and Node application heaps. The existing array-setting
witnesses retain their original named tests with refreshed whole-file hashes;
regenerated inventory records are not execution evidence.

```sh
LIGHT_CLJS_SCI_AUDIT_IDS=sci-binding-defn-env,sci-binding-fn-self-env \
  cargo test --locked --test evaluator_conformance \
  audit_historical_corpus_with_compiler_evaluator -- --ignored --exact --nocapture
cargo test --locked --test evaluator_conformance \
  every_historical_sci_case_has_an_executable_evaluator_disposition -- --exact --nocapture
cargo test --locked --test compiler_array_settings --test compiler_macros \
  --test compiler_reader --test provenance --quiet
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B tests/reference/compiler-language/test_full_ledger.py
```

The exact-source two-case audit passed. The complete 2,475-case accounting check
passed in 163.55 seconds, retaining 1,208 portable, 304 adapted, 877 gap and 86
retired rows. All 125 relevant Rust tests passed (seven array-setting, 104 macro,
12 reader and two provenance tests), as did inventory drift verification, all
14 ledger tests, validation of all 238 current phase witnesses, Rust formatting
and `git diff --check`. The execution logs are
`/tmp/light-cljs-136-evaluator-conformance.log` and
`/tmp/light-cljs-136-macro-array-suites.log`. No full repository suite or new
upstream oracle run was performed for this snapshot reconciliation.

### Local verification, 2026-10-10

The corrected harness completed the entire 2,475-row accounting check with native
evaluation disabled: **2 passed, 0 failed, 2 ignored, 141.70 seconds**. Counts were
1,180 portable, 304 adapted, 906 gap and 85 retired. The two ignored tests are
explicit report-generating audits. A separate four-probe audit executed each
bounded nonpassing case twice and verified identical observations against the
manifest. Original-row preservation and fourteen-field cardinality were also
checked independently, and the scoped diff passed `git diff --check`.

This verification compiled the integration-test source directly against the
existing Cargo-built library and dependency artifacts. It ran the same test
functions as the Cargo target, but was **not** a new `cargo test` invocation or a
complete repository gate. The exact local commands were:

```sh
rustc --edition=2024 --test tests/evaluator_conformance.rs \
  --extern light_cljs=target/debug/deps/liblight_cljs-83d5882418f132ee.rlib \
  -L dependency=target/debug/deps \
  -L native=target/debug/build/rquickjs-sys-065f3ee4d5969d6d/out \
  -o /tmp/light-cljs-evaluator-conformance-final-reviewed
LIGHT_CLJS_DISABLE_NATIVE_EVAL=1 \
  /tmp/light-cljs-evaluator-conformance-final-reviewed --nocapture \
  > /tmp/light-cljs-evaluator-conformance-resource-reviewed.log 2>&1
```

The local log is `/tmp/light-cljs-evaluator-conformance-resource-reviewed.log`.
An earlier standalone run in
`/tmp/light-cljs-evaluator-conformance-final-reviewed.log` failed because a global
10,000-poll harness budget starved valid cold macro bootstrap. That failed run
was preserved; passing observations were not rebaselined to its resource errors.
The corrected harness restores the default budget and confines special resource
handling to the four documented nonpassing probes. This passing accounting
result does not resolve the language gaps or authorize migration completion.

### Bounded constructor reconciliation, 2026-10-10

The applied `partial` and `juxt` constructors now retain their variadic sequence
tails. A reviewed 18-case audit executed every historical source containing an
applied constructor against the Cargo-built evaluator. Two unchanged-source
rows (`sci-deferred-lazy-partial-infinite`, `sci-seq-apply-juxt-constructor`)
now match the original projected SCI observations. Two delayed-tail error rows
now preserve the original completed effects; their owned diagnostic category
transport remains explicitly nonpassing. No original source/observation changed.
The former partial effect-cancellation special case was removed. Direct Node
and compiler witnesses check finite demand, capture reuse, invocation order and
fresh macro/runtime heaps. Final whole-repository and fenced gate results are
recorded in [migration verification](../evaluator-migration/verification.md).

`light-cljs-140` restores authoritative definition `:file`, while retaining the
original SCI reserved-metadata source and observation separately. Its unchanged
source now has a portable evaluator disposition; the metadata-effects row remains
explicitly owned by `light-cljs-125`. Both pinned CLJS references reject the fake
filename at their actual fixture site (3:25), independently of the historical
local site (1:1). See [source metadata evidence](../compiler-source-metadata.md).
