# Canonical quoted when expansion — light-cljs-147

The compiler-owned bootstrap macro now constructs `(if test (do ...))`,
matching `cljs/core.cljc:392–395` in the checked source
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`. `when-not` retains its explicit
nil then-form, as defined at lines 427–430. Runtime analysis still supplies
normal omitted-else semantics; this change preserves source-visible expansion
data without changing runtime lowering.

`compiler-quoted-when.py` regenerates and validates one complete fixture and
its owned ledger mappings. The original `macro_test.cljs::macroexpansion`
body retains all four original assertions and literal expectations. Its full
body hash is `c9ac1adbfd03fcb1f63cf15762cf54f263cc4ca412f7278a36d933a1edc6de94`.
It needs no fixture setup; its unused namespace macro imports are not introduced
as local .clj inputs. Only the `deftest`/`is` test harness becomes `do`/`assert`.
The pinned body bytes are checked before generation. No source assertion,
expectation, iteration or setup is removed.

The same fixture also asserts sixteen independent literal expansion-data
comparisons: both one-step and repeated expansion for empty/multiple bodies,
true/false/nil/zero conditions, uninterpreted condition/body forms and when-not
controls. Ordinary evaluation checks condition effects once, body effects in
order, false-condition skipping and nil results. None of the expected expansion
forms is computed by the implementation under test.

`quoted_when_body_shape_and_effects_execute_in_both_phases` executes that entire
contract in fresh Node and QuickJS applications and as a demanded macro
computation. Its macro state advances once, while the separately initialized
application state stays zero. It additionally executes the exact original
macro-phase count/print probe and checks this literal result in both engines:

```clojure
[3 "(if true (do 1))" 3 "(if true (do 1))"
 4 "(if false nil (do 2))" 4 "(if false nil (do 2))"]
```

`when_condition_and_body_keep_original_diagnostic_spans` verifies source
locations for unresolved symbols in the condition and both body positions.
The existing normal service tests retain dynamic-form hosted expansion,
shadowing, source arity/location failures, recursion bounds, unsupported macro
result domains and recovery checks.

## Current receipts

The normal test file SHA-256 is
`c06ac3d7a48c38612b25d7f580a39224413a52c3d17cbe427a3f852649b38014`;
the complete support fixture SHA-256 is
`53514fd65aa51b51dcd6fb59b56d20c3094841acb4c6aeeafd9d00baf68eb67d`.
The three new public mappings retain exact canonical declaration and signature
hashes. The existing macroexpand-1 and four SCI phase mappings retain their
sources, original expectations and historical observations.

- `/tmp/light-cljs-147-verification/evidence.json` records the complete body and
  ordinary count/print probe passing on Node and both checked references:
  release `1.12.134` and the exact checkout above, with `optimizations=none`,
  `static-fns=false`, and `def-emits-var=false` independently verified.
- `/tmp/light-cljs-147-verification/both-engines.json` records exact ordinary
  outcomes, empty trace and stdout for the complete body and count/print probe
  in both application engines. This expression runner also records rejection
  of the standalone macro-declaration probe because its wrapper nests the
  declaration inside a function. That harness rejection remains visible;
  the normal direct macro test above supplies the supported top-level proof.
- `/tmp/light-cljs-147-verification/phase-final/` retains the final exact normal
  named test invocation, one pass and zero ignores. Its final phase hashes are
  in `phase-evidence-final.json` alongside the preserved binding-probe proof.
- `/tmp/light-cljs-147-verification/focused-ledger-final.json` classifies four
  original assertions and three new public arities as covered from actual
  observations. Its scope stays selected and `strict_complete` stays false.
- `/tmp/light-cljs-147-retained-probes/evidence.json` records all four unchanged
  SCI binding-expansion probes passing both application engines and both pins.

The obsolete explicit-nil expectation in the local
`quoted_expansion_contract_and_dynamic_host_service_are_independent` test was
corrected explicitly from `(if true (do 7) nil)` to `(if true (do 7))`.
Its complete source hash is now
`48fed54f2568a7ce4996729d8c19aab1624c060c2e69dee674fb9c1a545fb850`.
The old source hash in the light-cljs-145 evidence document remains a historical
receipt. Original SCI fixture/disposition/expectation bytes are unchanged.

Reproduce generation and focused behavior:

```sh
python3 -B tests/reference/compiler-quoted-when.py
cargo test --locked --test compiler_quoted_when --test compiler_sci_service_probes
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B -m unittest discover -s tests/reference/compiler-language -p 'test_*.py'
```

The affected six Rust suites passed 195 tests, with 44 existing explicit ignores
supplying no coverage (`/tmp/light-cljs-147-affected.log`). The final extended
quoted-when suite passed both tests (`/tmp/light-cljs-147-quoted-final.log`),
and the corpus source/expectation provenance test passed
(`/tmp/light-cljs-147-sci-reference.log`). All 100 Python accounting tests passed
against the stable catalogs (`/tmp/light-cljs-147-python-stable.log`), inventory
freshness passed, and the focused Rust formatting and whitespace checks passed.

The normal compiler benchmark produced 423 checked samples over 47 scenario /
phase pairs, retained in `/tmp/light-cljs-147-perf.csv`. This is smoke validation,
without a comparative performance claim. This focused source correction does
not establish full language completion; light-cljs-125 retains that obligation.
