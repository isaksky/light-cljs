# Printing completion evidence

The runtime preserves the selected public printing contracts: `pr`, `prn`,
`print`, `println`, their string variants and the upstream no-op `flush`.
Collection printers share writer/options handling, including length, level,
readability, metadata, namespace maps and alternate/fallback implementations.
Writer callbacks receive the upstream sequence of fragments. Capture and
language bindings restore sinks and print settings after nested calls and
exceptions. Canonical JavaScript printing retains raw UTF-16 code units; the
Rust display API escapes isolated surrogate units to return valid UTF-8 and
preserves valid pairs. Output and error callbacks are independently configured.
Collection `str`/`toString` follows direct writer dispatch and omits root
metadata, while generic `pr-str` includes it; nested metadata is preserved.
Records use the upstream generated pair writer: each key/value pair has its own
sequence level and length bound. A single-field record at level one prints
`#user.R{#}`, at level zero `#`, and with length one `#user.R{:x ...}`.

`tests/compiler_printing_completion.rs` runs nine focused contracts in Node and
embedded QuickJS. Printing helpers and sink/control contracts also run inside
compiler-executed QuickJS macros. The tests include raw UTF-16 code units,
Unicode host output, custom `IPrintWithWriter` options, `IWriter` write/flush
effects, bounded collection printing, records, eductions, both sinks, nested
capture, throwing callbacks and restoration.

The exact pinned `printing_test.cljs` source at
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` supplies three complete workflows and
52 assertion sites. `compiler-printing-completion.py` reproduces their bodies,
assertion mappings, exact `PrintMe` declaration/namespace and source hashes.
Every date-loop iteration is retained. `deftest`/`testing` become `do`; each
original `is` predicate executes unchanged and a nonthrowing accumulator records
its exact source assertion ID if false. The complete workflow returns true when
all checks pass, otherwise the complete failed-ID vector. An exception cannot be
mistaken for an expected failed predicate.

The release and checkout differ at
`test-printing::assertion-005`, `(is (= (pr-str -0.0) "-0.0"))`:
ClojureScript 1.12.134 prints `0`, while the pinned checkout and local runtime
print `-0.0`. The release expectation is the singleton failed-ID vector; the
checkout/local expectation is true. This proves that all other original checks,
including the date loops, succeeded. The report requires a consistent expectation
for a shared complete-body fixture, so its 35 assertion sites inherit the body's
explicit version-difference classification; only that single predicate differs.

Validation completed:

```sh
cargo test --test compiler_printing_completion -- --nocapture
cargo test --test compiler_string_fixtures --test compiler_printing_completion -- --nocapture
cargo test --test compiler_records --test compiler_eductions --test compiler_protocols --test compiler_printing_completion -- --nocapture
node tests/support/js_runtime.test.js
node tests/support/js_core_helpers.test.js
python3 -B tests/reference/compiler-printing-completion.py
rustfmt --check tests/compiler_printing_completion.rs
git diff --check
```

The focused suite passed all nine tests. The existing macro bridge regression
`javascript_form_bridge::identity_preserves_owned_forms_and_executes_returned_code`
also passed after the Rust display boundary repair. The related suites passed six record,
five protocol and two eduction tests; one separately gated eduction reference test
remained ignored. Both Node runtime/helper scripts passed. The source generator
and formatting/diff checks passed.

The string-corpus acceptance test also now executes all 131 original bodies in
both engines. Its 15 formerly rejected `*print-meta*` bodies and every other
printing body use the independently verified expectations in
`compiler-native-printing.tsv`. Both pinned references agree on all 58 complete
printing bodies and lazy realization effects. Original native outcomes remain
in `sci-outcomes.tsv`; provenance and an optional live reference check are in
`compiler-native-printing.py`.

An additional independent program retains the complete original
`sci-record-print` body in its exact `user` namespace and adds length-one and
level-two observations. Both pinned references verify record pair depth,
continuation markers, and root metadata. The original `sci-io-print-returns`
body is wrapped in `with-out-str` to observe the raw print sink independently
of the reference environment's default console newline behavior. Both pins
return six nil values and exactly three captured newlines. These executed
proofs are retained in
`/tmp/light-cljs-124-printing-disposition-reference-final`.

The whole-corpus evaluator accounting preserves all original seven SCI fields.
The 25 changed printing rows now contain final-runtime observations: 14 agree
with the original SCI projection and 11 explicitly retain independently
verified canonical dialect differences. The latter are not SCI conformance
passes. The reviewed patch/proof record is
`/tmp/light-cljs-124-printing-disposition-patch.json`.

The source-selected language report executed all 52 printing assertion witnesses
in generated Node JavaScript and both pinned references, plus the registered
compiler-phase witnesses and supplemental boundaries. Every selected layer
passed. The report classifies 17 source sites as covered and 35 as the explicit
complete-body version difference described above. All 44 printing/reflection
public/local obligations are covered, with no missing family obligations; the
independent whole-language ledger remains incomplete.

The final compiler binary reran all three complete printing programs after the
collection `str` and generated record pair-depth repairs. The frozen report is
`/tmp/light-cljs-124-printing-report-final-frozen.json`, with artifacts in the matching
directory. Its `refresh.py` uses the report's ordinary assembly functions and
retains the unchanged, actually executed reference and supplemental proofs
with explicit source-report/binary identities. All 19 printing macro obligations
were freshly executed after the record correction; unchanged macro proofs from
other families retain their verified source hashes. The final local
observations, compiler/runtime hashes, and the reviewed SCI disposition hash are
stored beside it.

The report's `reference_cases` runner executed all three generated namespace
programs against release 1.12.134 and the pinned checkout with
`:optimizations :none`, `:static-fns false` and `:def-emits-var false`.
All six observed workflow outcomes match the versioned expectations.
The executed artifacts are retained under
`/tmp/light-cljs-124-printing-reference-final`; complete observed outcomes and
reference identities are in
`/tmp/light-cljs-124-printing-reference-outcomes.json`.
