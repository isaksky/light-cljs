# Compiler reader evidence — 2026-10-09

Contract: Maven ClojureScript **1.12.134** and checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e** (reported compiler version
`0.0.135109660`), Clojure 1.12.4, Java 25.0.2. The reference driver verifies
identity before and after compilation and uses `:optimizations :none`,
`:static-fns false`, and `:def-emits-var false`.

## Source and assertion mapping

The complete relevant bodies were read in the pinned checkout:

| Source under `src/test/cljs/cljs/` | Evidence and adaptation |
| --- | --- |
| `primitives_test.cljs`, `test-regexps` | Eleven complete assertions, including the first assertion's `r1`/`r2` bindings, execute through the language report. The exact assertion IDs, hashes and adaptation rationale are in `compiler-language/witnesses.jsonl`. Assertion 002 uses `js*` and remains missing in the interop work; it is not counted as covered here. |
| `reader_test.cljs`, `testing-cljs-3291` | All thirteen original Date equality/printing assertions are `reader-date-upstream-01` through `13`. Only the `is`/`deftest` wrappers were removed. Gregorian dates before 1582 and fractional seconds retain their original expectations. |
| `syntax_quote_test.cljs`, `test-syntax-quote` | Read all four namespace/alias assertions. Independent probes cover current namespace, core qualification, alias qualification, unquote and splicing; these probes do not claim those source assertion sites. |
| `tagged_literals_test.cljs`, `test-identity-custom-literal`, `test-runtime-reader` | Read both complete bodies. The embedding reader callback test covers explicit compile-time tag expansion. The `cljs.reader/read-string` library and its runtime tag registry are separate from source reading and are not implemented by this member. |

Implementation references also include the complete relevant tools.reader
anonymous-function, argument, metadata, tag and syntax-quote readers;
`cljs/tagged_literals.cljc`; `cljs/instant.clj`; and the Date, UUID, Var,
regex, metadata and printing implementations in `cljs/core.cljs`.
The imported assertions retain their original license attribution in the
fixture/inventory. `fixture-origins.tsv` distinguishes those assertions from
the independent compiler probes.

## Behavior and boundaries

Source reader constructs lower through semantic `Form` values. Opaque compiler
values preserve RegExp, Date, UUID and host literal kinds, and exact wide integer
identity until JavaScript Number emission. No native values are printed and
reparsed. `#js` retains original child forms and spans, so nested unresolved
symbols point to their own source and side effects remain ordered.

Ordinary integers, BigInt spellings, decimal spellings and integral ratios emit
JavaScript Numbers with reference rounding and overflow. Non-integral ratios
remain invalid constants in both pinned JVM compilers. Reader duplicate checks
keep exact integer identity, including keys that round to the same Number at
runtime. This is distinct from a safe-integer restriction. The wide-map dual
reference probe checks that reading succeeds and metadata survives. Its entry
count is not a conformance claim: the existing documented map policy deduplicates
Number-equal keys, whereas upstream literal array-map construction can retain
both rounded keys (see `compiler-collections-runtime.md`). Native interpreter
arithmetic remains its existing separately scoped numeric profile.

The source reader retains exact UTF-16, including lone surrogates. Anonymous
parameters have fresh identities, `%` aliases `%1`, numeric argument syntax is
validated, `%&` produces nil or a seq, and nested shorthand fails. Discarded
arguments participate in tools.reader parameter discovery. Reader handlers run
once even in discarded forms or unselected runtime branches; explicit registered
handlers override built-in tags. `#=` remains disabled. Compiler `.cljs` reader
conditionals remain with the separate namespace/loading work.

Date/UUID tests observe identity, value equality, printing and actual host type.
Regex tests retain captures, flags, failed matches, empty patterns and lazy
`re-seq` tails. Var quote ignores local shadows, dereferences the current global
root, preserves definition metadata and supports invocation. Metadata is also
checked on collection values, quoted symbols and functions.

## Verification

The final fast run, `cargo test -j 4 --quiet`, passed **594 tests**, with **13
explicitly ignored** optional reference/research checks. The existing 120-level
native-reader regression passes without increasing its stack or reducing its
depth; scalar reader temporaries are kept outside recursive collection frames.
`node --test tests/support/js_runtime.test.js` and
`cargo clippy -j 4 --lib -- -D warnings` are the runtime and static checks.

The dedicated reader comparison executes all `reader-runtime-*` independent
probes and the thirteen Date assertions: **49 complete outcomes passed** per
reference, comparing expected values, effects and stdout independently:

```sh
LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT=research/clojurescript \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --test execution reader_forms_match_pinned_values_effects_and_stdout -- --ignored --nocapture
```

The same environment with
`cargo test --test execution analyzer_boundary_classifications_match_reference -- --ignored --nocapture`
matched **130 boundary classifications** per reference and retained the existing
`let-binding-amp` release/check-out difference. The strict language report's
selection `primitives_test.cljs::test-regexps::assertion-{001,003,004,005,006,007,008,009,010,011,012}`
passes all **11** witnesses in generated JavaScript and each reference. Pass each
expanded ID as a separate `--id` to
`python3 -B tests/reference/compiler-language/report.py --scope historical-selected --run --reference both --strict`.
`extract_inventory.py --check` verifies the unchanged pinned inventory.

The attempted entire execution-corpus dual-reference run exposed the existing
`set-literal-effects` and `set-dynamic-collapse` differences: release returns a
deduplicated result, while checkout throws after evaluating operands. That run
is **not** reported as a full dual-reference pass. The reader selection has its
own complete comparison, rather than hiding those unrelated differences.

## Output and startup cost

The reproducible report is under
`benchmarks/results/compiler-2026-10-09/reader/`, generated with:

```sh
python3 -B benchmarks/run_compiler.py \
  --output benchmarks/results/compiler-2026-10-09/reader --samples 9 --iterations 8
```

It saves source/binary hashes and snapshots, validates each result, measures
resident compilation and operations, and separately measures fresh CLI/Node
processes. Fresh Node time includes process startup, the typed validator and
output; OS caches and scheduling are uncontrolled, so these are not browser
startup or isolated runtime-construction estimates.

Against the previous `collections/output-sizes.json`, the integer (104 bytes),
nested closure (313 bytes) and JS nested method (142 bytes) outputs retain
identical hashes. The collection-bearing fixture grows from **24,944 to 33,683
bytes**, or **5,642 to 7,722 gzip bytes**. This material increase comes from the
shared reader/metadata/printing helpers; the runtime is still included only once
and remains absent from primitive-only programs. Raw timing distributions and
fresh-process medians are in the report's `summary.json` and CSV files.

## Integration update — 2026-10-10

The previously reported whole-corpus set differences now have exact versioned
expectations. The complete 1,694-case execution comparison passes both references
with full outcomes, ordered effects and stdout. Duplicate readable executable
forms also reject before evaluation locally, including native macro inputs,
matching both pinned readers. Dynamic equality of distinct source forms remains
a separate runtime check and explicit release/checkout difference. See
[the integration evidence](compiler-language-completion.md).

## Compiler-owned datum extraction — 2026-10-10

The new ownership checks are hand-authored local contracts, independent of
upstream implementations: `compiler_forms_can_be_constructed_without_any_evaluator`
and `compiler_forms_and_ir_survive_host_teardown_without_native_roots` in
`tests/compiler_reader.rs`, plus the native-owner weak-reference and nested-origin
unit tests in `interpreter/reader.rs` and `macro_host.rs`. They verify no native
root/storage remains reachable from compiler forms, exact UTF-16 and floating
bits, metadata/tagged payloads, source identity, and owned diagnostics after host
teardown. The IR ownership case executes emitted JavaScript after teardown;
existing reader/macro suites continue asserting ordered effects and runtime values.
The metadata-annotated `&form` alias now retains its original nested expansion
origin rather than acquiring the next expansion's origin.

The focused extraction run passed **155 tests** with **one existing ignored manual
protocol timing test**, using:

```sh
cargo test --locked --lib \
  --test compiler_reader --test provenance --test compiler_macros --test analyzer \
  --test interpreter_reader --test interpreter_macros --test interpreter_reuse \
  --test compiler_destructuring_completion --test compiler_reify
```

`cargo clippy --locked --lib -- -D warnings`,
`cargo check --locked --all-targets --all-features`, `cargo fmt --all --check`
and `git diff --check` also passed.

The required four compiler suites are included in that run. The full suite,
optional JVM/upstream comparisons and benchmarks were not rerun for this ownership
change; earlier verification above remains historical evidence.

## Compiler support extraction — 2026-10-10

`compiler_reader` now performs contextual reading directly over compiler-owned
`Datum`/`Form` arenas. Its immutable `NamespaceView` retains the definition-site
aliases, refers, excludes, source identity, binding IDs/kinds and declaration
frontier. Static macro/name classification is in `compiler_names`; core metadata
queries read the pinned table directly. Namespace metadata and reflection do not
initialize a Context. Syntax quote produces the same canonical construction calls
used in emitted programs. No new upstream behavior claim or numeric evaluator is
introduced by this extraction.

The ordinary seed binding/function transforms were extracted from
`interpreter/binding_macros.rs`; extended map destructuring was extracted from
`macro_host::compiler_destructuring`. Both now consume ordered owned data without
native evaluation. Reused operands keep their original child spans and metadata;
new nodes retain macro origins. Method declaration construction and assertion
formatting are also pure. Existing core/declaration semantics and the reference
pins documented above remain the semantic evidence.

`source_provider` owns the source request/input/error contracts independently of
native eager loading. Explicitly registered native custom reader callbacks retain
a scoped adapter that binds the captured compiler namespace. Remaining native
macro execution and legacy companion-file loading are explicit migration seams;
they are replaced by their later migration members. Group 212 revision 3 contains
the exact bootstrap registration/resource inventory and dispositions, including
all inline macro sources and `compiler_sequence_macros.clj`.

Verification commands and observed outcomes:

- `cargo test --locked --test compiler_reader --test compiler_destructuring_completion --test compiler_namespace_completion --test compiler_macros --test provenance --test compiler_warnings --test compiler_boundary_completion --test interpreter_loading --quiet`: 104 tests passed. The reader/warning suites were rerun after the native callback namespace guard and original-symbol diagnostic checks; all 17 passed. Macro/namespace suites were rerun after pure core import/alias resolution; all 54 passed.
- `cargo test --test compiler_protocols --test compiler_types --test compiler_records --test compiler_reify --test compiler_extensions --quiet`: all 26 tests passed, exercising the extracted method construction and binding transforms.
- `cargo test --locked --lib compiler_ --quiet`: all 4 selected ownership/origin/no-Context tests passed. `cargo test --locked --lib namespace_snapshot_tests --quiet`: the immutable snapshot/UTF-16 test passed.
- `cargo test --locked --test analyzer --quiet`: all 35 tests passed. `cargo check --locked --all-features --quiet` and `git diff --check` passed.
- Built starting HEAD in an independent temporary archive and executed both CLI outputs under Node. Seven exact generated-program observations matched: numeric/character literals, syntax quote/splicing, namespaced maps, ordinary destructuring/anonymous functions, function preconditions, ordered map effects, and protocol/type methods. Five reader/unresolved-name diagnostics matched byte-for-byte. A default undeclared reference inside `fn` remains rejected, now precisely at the original `future` symbol (bytes 14..20, formerly the entire fn call at 7..21). Warning-mode compilation retains the undeclared-var category, executes to `1`, and likewise identifies the original symbol. The warning suite locks in this precision improvement.

No full language/native suite, new JVM differential regeneration or evaluator
migration gate was run for this extraction. Their earlier provenance and remaining
migration obligations are not relabeled as completion here.

## Reader coverage completion — 2026-10-11

The committed `reader-sources.tsv` retains **seven complete source files, 137
complete top-level bodies, and 72,143 UTF-8 bytes**, from the same pinned checkout
above: `reader_test.cljs`, `syntax_quote_test.cljs`, `tagged_literals_test.cljs`,
`metadata_test.cljc`, the complete `data_readers.cljc` setup table, and the vendored tools.reader and EDN implementation files.
Every source hash and body byte range regenerates from pinned Git objects. The
ordinary corpus test validates those spans against both whole-file and incremental
Rust parsing, plus tokenizer progress and fused EOF. The complete original
copyright/license notices remain inside the retained source bytes and generated
fixtures. Python verification checks that the older reader execution fixture
still contains every original setup/test body. It does not run an oracle.

The whole-checkout parser run separately accepted **296 files, 2,834,491 bytes,
343,327 syntax nodes, and 6,594 roots**. These are exclusively parser counts.
Neither corpus is an execution claim for tools.reader, the upstream compiler,
JVM support code, or the entire checkout.

| Family | Parser / Rust reader evidence | Macro / actual application evidence and disposition |
| --- | --- | --- |
| Spelling, UTF-16, characters, numeric forms | Existing 99-case captured literal oracle plus seven-file source pins; every-family truncations retain valid UTF-8 ranges, progress, rollback and terminal EOF | Literal checks now execute in both Node and QuickJS, including lone-surrogate code units. Non-integral ratios/JVM exact-number operations remain explicit dialect boundaries; spelling acceptance alone is not numeric execution. |
| Quote, syntax quote, unquote/splicing, gensyms | Contextual construction keeps owned child source spans; invalid splice diagnostics remain distinct from syntax acceptance | All four complete upstream syntax-quote assertions execute, retaining the namespace and alias expressions. Local macro/application checks prove per-quote sharing and cross-quote freshness; existing unquote/splice checks execute in both engines. |
| Anonymous functions and rest | Syntax accepts ratio indices; semantic reading rejects non-integral indices at their own span. Family truncations retain incremental behavior | Application checks include discarded argument discovery and `%&`; a demanded macro helper and application body exercise shorthand and explicit `&` rest bindings and public reader calls. |
| Metadata | Exact target/related spans reject invalid metadata targets after successful parsing. Stacked metadata, symbols, strings and selected conditional collections are retained | Local metadata checks execute in both phases/engines. The whole upstream metadata file is source inventory, not a new whole-body runtime pass. Its wider public-protocol evidence stays in the independent intended-language ledger. |
| Tags, #js, Date, UUID, queue, regex | Unknown tags reject after parsing; exact tag payload spans and finite conversion remain compiler-owned | Both complete upstream tagged-literal bodies execute (five assertions), using explicit source identity and runtime identity/inc/`clojure.set/union` handlers. Existing entire reader fixture executes **7,487 dynamic assertions** per engine, including the full Date month/day/hour loop; this differs from source assertion-site counts. Configured readers preserve source order, discarded/untaken-form effects and fresh compilation roots. |
| Aliases and namespaced maps | Missing aliases locate the original keyword or namespace spelling, rather than becoming parser success | All upstream syntax-quote aliases plus local `::r/x` and `#::r` underscore-escape cases execute in Node/QuickJS applications and canonical macro computation. Namespace parts survive the macro data bridge; synthetic `:light-cljs/id` environment keys now retain their namespace part. |
| Discards and reader conditionals | `.cljc` selects the first matching `:cljs`/`:default`, supports collection splices and diagnoses selected map arity; `.cljs` rejects contextual conditionals. Malformed inactive literal/collection/read-eval syntax still fails parsing | Selected anonymous parameters, inactive tags/duplicates, exact reader effect counts and session recovery have direct observations. Owned structured diagnostics preserve failing spans after session teardown and retain the existing public located message without a doubled location prefix. Existing namespace/reader suites separately execute emitted conditional programs. |
| Duplicate numeric data | Reader identity equates integer/`N`/integral-ratio spellings and equivalent decimal spellings, while retaining integer/float/decimal distinctions, including nested keys; emitted Number equality remains separate | Source sets now detect `1`/`1N`, integral-ratio and decimal duplicates. Distinct integer/float reader values remain accepted, while the documented runtime Number/map policy can collapse equal emitted keys. Generated macro Numbers retain their actual data rather than being reclassified by the invocation's source spelling. |
| Public reader APIs and phase boundary | The expanded ledger binds the two `tagged-literal` declarations, four retained core/EDN read APIs and `&` syntax to exact local tests | Fixed constructor/predicate arities, saved/applied calls, tag/form lookup, zero/options/explicit-stream/EOF reads and string/options reads execute in a called application function and a demanded macro helper. Options-only `read` now uses the current input in the shared runtime, including macro execution. No registration/name scan supplies a pass. |
| Runtime code-reader namespace state | Source compiler reading uses its immutable NamespaceView; runtime string reading is a separate layer | The previously demonstrated `[:user/x :user/x]` discrepancy is repaired by **light-cljs-134**: the preserved SCI probe freshly executes as `[:user/x :other/x]` in its explicit user session. Standalone/default compiler tests intentionally use `cljs.user`. New direct Node/QuickJS runtime tests execute aliases, auto-resolved maps, syntax quote, nested/throwing reads and missing-alias locations; canonical macro and reused-session tests execute caller contexts and restoration. Strict EDN/cljs.reader rejections remain separate. These local runtime observations do not relabel an unexecuted pinned-CLJS comparison. |
| Read-eval and JVM boundaries | `#=` is rejected even in discarded/inactive branches; JVM classes/host reader evaluation and exact numeric operations are excluded by the adopted dialect | No execution or parser pass is claimed for those excluded semantics. |

The full-suite accounting run also exposed a message-compatibility regression while adding owned reader locations; the public located message is preserved, and formatting adds its location once. The SCI collection-key row now executes successfully after source numeric-category repair, but retains its original SCI observation separately: emitted JavaScript Numbers intentionally collapse integer/float map identities. It is explicitly retired from SCI exact-key equivalence, not relabeled portable.

The macro suite also exposed two already failing baseline tests, reproduced in
an independent archive of starting commit `12f8bd0`. The old generated-map count
expected two distinct namespace/name keys to collapse, contrary to the current
canonical named-value contract; it now expects both entries. The actual macro
local-binding metadata bug constructed `:light-cljs/id` as an unqualified name;
its construction is corrected and the original shadow-ID assertions now pass.

`phase-witnesses.jsonl` and `obligation-witnesses.jsonl` contain seven precise
local API/syntax mappings. Supporting fixture hashes are validated alongside the
complete Rust test source, so changing the called helper cannot retain stale
phase evidence. Source-language assertion mappings and optional reference
columns remain independent; these local tests do not silently fill every
unmapped row of the full intended-language report.

### Verification

- `cargo test --locked --no-fail-fast --quiet`: 66 harnesses completed, 574 tests passed, 62 optional tests ignored, and the historical evaluator accounting target initially failed. After preserving the public reader diagnostic message and refreshing the single numeric-key disposition, `cargo test --locked --test evaluator_conformance --test compiler_reader --test eval_sessions --test eval_cli --quiet` passed all 50 tests (two explicit audits ignored). Its historical accounting target executes all 2,475 rows: 1,208 portable, 304 adapted, 877 nonpassing gaps and 86 retirements. Gaps/retirements are not conformance passes; no SCI oracle was regenerated. The entire suite was not rerun a second time.
- `cargo test --locked --test compiler_reader --test parser_corpus --test parser --test compiler_language --test provenance --test eval_sessions --test eval_cli --quiet`: 66 tests passed, one optional checkout test ignored. These final focused checks include the exact seven-file pins and precise numeric duplicate spans.
- `cargo test --locked --test compiler_macros --quiet`: all 102 macro tests passed. The EDN and library suites also passed (14 and eight tests), including complete reader bodies in Node and QuickJS.
- `node --test tests/support/js_runtime.test.js`: passed. `cargo check --locked --all-targets --all-features --quiet` and `git diff --check`: passed.
- `python3 -B tests/reference/generate_reader_coverage.py --verify` and `--check`: both passed, preserving seven files, 137 bodies and 72,143 source bytes. `python3 -B tests/reference/compiler-language/extract_inventory.py --check`: passed; historical counts remain unchanged. `python3 -B -m unittest discover -s tests/reference/compiler-language -p test_full_ledger.py`: 14 tests passed, including supporting-source hash invalidation.
- `cargo test --locked --test parser_corpus parse_clojurescript_checkout -- --ignored --nocapture`: passed with the syntax-only checkout counts above.
- `python3 -B benchmarks/run_compiler.py --output /tmp/light-cljs-9-benchmark-final --samples 9 --iterations 3 --baseline-ref 12f8bd0`: validated the full report and baseline primitive comparisons. All three primitive output hashes and raw/gzip byte counts are identical; runtime source grows 153 bytes. The baseline archive setup now tolerates existing fixture/bench entries. Preliminary setup failures and a source-change snapshot refusal were resolved before the final successful report. Concurrent suite activity makes these observations unsuitable for a timing-improvement claim.
- `cargo clippy --locked --lib -- -D warnings`: the same 49 baseline diagnostics remain. An independent starting-commit archive and the current tree have identical diagnostic code/message lists; this change adds no new Clippy diagnostics.

Routine fixtures require Rust, Python and Node, with QuickJS supplied by the crate;
Java and a research checkout are unnecessary. The source regeneration check uses
Git/research, independently of oracle execution.

A narrowly executed vendored JVM reader probe verified seven numeric
classifications: `#{1 1N}`, `{1 :a 1N :b}` and `{1 :a 2/2 :b}` reject; the
integer/float map/set variants, including zero/negative zero, remain distinct.
This was `clojure -Srepro -Sdeps '{:paths
["research/clojurescript/src/main/clojure"]}' -M -e` with
`cljs.vendor.clojure.tools.reader/read-string` over those seven literal strings.
It was not a full compiler/reference comparison. No new Maven/checkout
whole-reader semantic comparison, whole-language strict pass, SCI oracle
regeneration, cross-platform run or hosted CI result is claimed. Earlier
reference results above remain historical evidence.

### Runtime namespace execution follow-up (light-cljs-134)

`compiler_reader` now has five focused runtime namespace tests. They exercise
active `*ns*`, nested/throwing binding restoration, explicit aliases, namespaced
map underscore escapes, syntax-quote core/local/referred names, emitted dependency
namespace initialization, macro caller contexts, and reused session namespace
switches. Direct runtime checks execute in Node and QuickJS; source and canonical
macro fixtures remain separate. `clojure.edn`/`cljs.reader` profile rejections and
`#=` rejection execute in both engines. No runtime read acquires loader authority.

The seven scoped suites (`compiler_reader`, `compiler_macros`,
`compiler_namespace_completion`, `eval_sessions`, `compiler_data_libraries`,
`compiler_edn`, `compiler_host_hooks`) passed 178 tests. The additional library,
bootstrap, boundary, optimization, execution and provenance selection passed
108 tests; its 43 opt-in ignored execution tests were not run. The fresh selected
SCI audit observed `[:user/x :other/x]`, with unchanged effects/output and original
SCI columns retained. This is current canonical execution, not a fresh pinned
CLJS oracle comparison. Pinned seven-file reader source verification retains
137 bodies/72,143 bytes and explicitly reports its oracle unexecuted.

`cargo bench --locked --bench compiler -- --samples 9 --iterations 1` validated
423 samples across 47 scenario/phase combinations. The 20-iteration evaluator
benchmark validated fresh, reused and bounded-loop/reset/recovery results (15.63,
0.112 and 67.54 ms per iteration on this host). These are measurements and
validation, not a baseline-relative speedup claim or language coverage count.

Final-tree SCI accounting verified all 2,475 retained dispositions (1,209 portable,
304 adapted, 876 gap and 86 retired). Gaps/retirements are explicit nonconformance
observations. Two `keys &env` rows retain the SCI flat-environment observation
and their nonpassing status while freshly recording the added descriptive reader
context key; no SCI expectation was replaced to declare those rows compatible.
