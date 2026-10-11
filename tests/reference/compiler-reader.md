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
`python3 -B tests/reference/compiler-language/report.py --run --reference both --strict`.
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
