# Canonical numeric case constants

The unchanged complete source `(case 1 1 :integer 1.0 :float)` now returns
`:integer` with exact empty effects/stdout. Public `case` validates reader
identity recovered from its borrowed source form, then emits ordinary
JavaScript Number matching in source order. Integer, floating and decimal
reader categories can remain distinct even when execution rounds them to the
same Number. No numeric execution tower is introduced.

The independently frozen catalog contains 16 complete accepted sources and 11
true duplicate sources. Both release `1.12.134` and clean checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` independently produced the literal
packets in `/tmp/light-cljs-175-derive-final`; production reruns both pins under
`:none`, `:static-fns false`, `:def-emits-var false` before local comparison.
Each source retains its exact bytes and SHA-256.

Controls include reversed first-match order; hexadecimal, octal, `N`, integral
ratio and `M` spellings; grouped/default and nested vector/list/map/set case
constants; signed integer/float zero; infinities and NaNs; exact integers on
both sides of 2^53 and beyond i64; decimal constants differing beyond Number
precision; and selector/selected/default branch effects occurring once. True
reader duplicates include equivalent spellings, grouped and nested duplicates,
floating rounding, decimal scale and infinities. The pins equate signed
floating zero during duplicate validation. NaN is nonreflexive, so repeated
NaN constants are accepted and never match, including nested constants. A
missing default is caught by the complete source's explicit failure control.
These are source case tests, with no new collection constructor or reader
literal collection semantics.

`case_numeric.py` builds the exact Rust audit executable, then compiles actual
ordinary namespace application sources and demanded macro namespace sources in
fresh and reused MacroHosts. The macro computes each complete expression before
quoting its finite result. Every successful namespace executes in QuickJS and
Node. All 128 accepted frames match the independent typed packets, including
empty observer effects/stdout. Every one of 44 local diagnostic contexts then
uses the same host for a real recovery macro computing `(+ 20 22)`; all 88 engine
outcomes return 42. The reused host also receives a real macro warmup.

The raw replayer checks current compiler/producer sources, the frozen catalog,
pin source/settings, executable hashes, exact build/audit/Node commands and
environment, complete source/emitted/raw artifact censuses and eight frames per
accepted source. Rehashed source, packet, pin emission, recovery and command
mutations fail closed. Missing, altered or stale artifacts and serialized
positive summaries cannot supply acceptance. Controls receive zero original
SCI or full-language completion credit.

The current character/string case producer also accepts the numeric source as
a control and retains all true duplicate/malformed diagnostics. Its fresh
neighbor receipt has 88 passing application/macro engine frames and 56 recovery
outcomes. The old 173 contracts and receipt are preserved byte-for-byte as
`case-char-string-accepted-historical-173-{contracts,receipt}.json`; they retain
the earlier local numeric failures and original SCI `[2 2]` expectation. The
unchanged SCI case source still has a canonical `[1 1]` difference and zero
original semantic credit. Fresh raw integration into the full ledger remains
`strict_complete: false`; only the analyzer source hash changed in the
regenerated inventory manifest, with every body, obligation and expectation
unchanged.

Verification passed 181 ordinary regression tests across numeric and
character/string case, bootstrap, macros, analyzer, equality and reader suites;
three numeric proof guards, eleven character/string proof/integration guards,
22 full-ledger guards and byte-exact inventory regeneration. The validated
compiler benchmark completed 423 measurements (nine samples, two iterations).
A test-output formatting assertion failed during development and was corrected
to print the keyword via `str`; its exact rerun passed. One bootstrap test named
for expansion limits accidentally ran and passed in the initial suite; later
runs excluded `limit`, `limits` and cancellation names. No investigation or
repair of externally blocked 156, push or PR occurred.


Delivery receipts are byte-identical to the fresh raw summaries:
`case-numeric-receipt.json` SHA-256
`cf123cd22291e8ddb1614359b3a058fa084139b56916e08fda77e093e9bece82`,
and `case-char-string-accepted-receipt.json` SHA-256
`a106b9943befecb14893e2d90b7573221c92a56473bd1c470a94b6ab02122748`.
The preserved historical 173 receipt retains SHA-256
`acf2ed9dd52b5fb17434ecb9500736d958c76c2a38b8b880b0a88d6975544c9b`.

Reproduction:

```sh
cargo test --locked --test compiler_case_numeric canonical_numeric_reader_identity_and_same_host_recovery -- --exact
python3 -B tests/reference/compiler-language/case_numeric.py --output /tmp/new-numeric-case-proof
CASE_NUMERIC_RECEIPT=/tmp/new-numeric-case-proof/evidence.json python3 -B -m unittest discover -s tests/reference/compiler-language -p test_case_numeric.py
cargo test --locked --test compiler_macros --test compiler_reader --test analyzer --test compiler_equality -- --skip limit --skip limits --skip cancel
cargo bench --locked --bench compiler -- --samples 9 --iterations 2
```
