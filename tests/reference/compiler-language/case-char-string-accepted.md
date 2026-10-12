# Canonical character/string case acceptance

The unchanged original `sci-control-case-char-string` remains:

```clojure
[(case \a "a" 1 \a 2 3) (case "a" \a 1 "a" 2 3)]
```

Its immutable SCI observation is `[2 2]`. Both canonical ClojureScript pins,
release `1.12.134` and checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`,
produce `[1 1]` with exact empty effects/stdout under ordinary `:none`,
`:static-fns false`, `:def-emits-var false` compilation. Acceptance retains an
explicit canonical difference and zero original SCI semantic credit. The old
`case_char_string_gap.py`, diagnostic driver and historical receipt remain
unchanged.

`case_char_string_accepted.py` runs fresh builds against both checked pins,
then the actual Rust-built source driver. Every case is compiled as a complete
ordinary namespace application and as an actually demanded `defmacro upstream`
computation, in fresh and reused MacroHosts, then executed in QuickJS and Node.
The macro computes the original expression before quoting its finite result;
no expected literal is inserted into the macro body. The reused host starts
with a real macro computing `(+ 20 22)`.

The core `case` macro retains its borrowed original source form as metadata.
Validation uses that source for reader identity while lowering the branches
computed by the macro. This preserves characters recursively even when their
JavaScript values equal strings; true reader duplicates still reject.

The independently derived, reviewed literal catalog records the original plus
characters, strings, mixed grouped/default cases, selector/branch and default
effects, nested vectors/list selectors, map/set constants, and equivalent
Unicode character spelling. Its derivation directory retains both independent
pin packets before local comparisons. Effects controls return their own atom
history, proving one selector event and only the selected branch in both
phases. All observer packets still require exact empty trace/stdout.

A neighboring map/set literal source is observed separately. Both pins return
`[2 1 2 true]` for the literal with distinct character/string keys/elements.
The current local result is `[1 2 1 true]`. Its mismatch remains explicit in
the receipt; it cannot
supply case acceptance or wider collection coverage. True character, string,
grouped and equivalent-spelling duplicates, plus malformed `case*` selector,
groups and constants, retain both-pin rejection evidence. Each local failure
is followed on the same host by a demanded recovery macro returning 42 in both
engines, across all four phase/lifecycle contexts.

The retained local numeric duplicate check is a separate nonpassing diagnostic:
`(case 1 1 :integer 1.0 :float)` returns `:integer` in both pins, while local
application and macro sources reject it. The receipt retains the two returned
pin packets and four local rejections/eight same-host recovery engine outcomes.
This repair preserves the existing local check without claiming pin parity.

`reconcile(document)` reconstructs the complete result from current raw
artifacts. It checks immutable original rows, the reviewed control catalog,
current compiler/harness/producer sources, both pins and settings, actual
executable hashes, exact Cargo/audit/Node commands and environment, complete
source/emitted/raw artifact censuses and all eight frames per source. Missing,
extra, altered, stale or detached artifacts fail closed. Serialized summaries
and the persistent receipt are never an acceptance fallback. Only the exact
original obligation is returned for optional full-ledger reconciliation;
collection diagnostics and controls grant no original body/API/program credit.

Focused commands:

```sh
cargo test --locked --test compiler_case_char_string_accepted canonical_case_constants_and_same_host_diagnostic_recovery -- --exact
python3 -B -m unittest discover -s tests/reference/compiler-language -p test_case_char_string_accepted.py
python3 -B tests/reference/compiler-language/case_char_string_accepted.py --output /tmp/light-cljs-173-case-accepted-delivery
```

The normal Rust regression runs the reviewed case literals in applications and
actual macro computations, plus same-host failure recovery. The ignored
`accepted_case_constants_execute_in_fresh_and_reused_macro_hosts` test is the
explicit artifact producer; its executable is invoked directly by the Python
helper after the exact Cargo build. No cancellation or limit suites are used.

Delivery verification produced
`case-char-string-accepted-receipt.json`, byte-identical to the fresh artifact
`/tmp/light-cljs-173-case-accepted-delivery/evidence.json` (SHA-256
`acf2ed9dd52b5fb17434ecb9500736d958c76c2a38b8b880b0a88d6975544c9b`).
The raw artifact tree remains the executable acceptance source; the checked-in
receipt is a retained observation, never a fallback. The preceding final-trial
receipt at `/tmp/light-cljs-173-case-accepted-final/evidence.json` remains
preserved; its later integration-test fix required the fresh delivery run.

The delivery run verifies 72 passing case frames (nine complete sources), eight
explicitly mismatching neighboring literal frames, and 64 recovery engine
outcomes after 32 same-host diagnostic contexts. Seven diagnostic sources also
reject in both pins; the eighth is the separately retained numeric discrepancy.
Duplicate-checked live Pellet 174 owns the character/string map/set literal
collapse; Pellet 175 owns numeric case source distinction. Both precede
Pellet 125 and grant no acceptance credit here.

Safe neighboring Rust suites passed 180 tests; the explicit both-pin control
check matched all 37 complete control outcomes, effects and stdout. The normal
validated compiler benchmark passed all 423 fixture measurements (nine samples,
two iterations). Full-ledger regression tests passed 22 checks. The case replay
and integration guards additionally exercise rehashed source/unit/raw packet,
command, executable and recovery changes, missing artifacts, changed original
bindings and detached fresh layers. Original inventory totals, source bodies,
SCI expectations and historical gap evidence remain unchanged.

The actual ledger integration selects the exact original obligation from the
full result by identity: independent public/API/program obligations always
remain in that ledger even with a selected source id. Its canonical difference
has zero original semantic credit, four passing demanded macro frames, and
`strict_complete: false`. The focused ledger receipt retains that exact row and
full result counts with this selection policy explicit; it cannot certify the
comprehensive language gate. No cancellation or singular/plural limit tests,
externally blocked Pellet 156 work, push or PR were performed.

## Numeric case completion (175)

The retained observations above describe 173 delivery. Its original contracts
and receipt are now preserved byte-for-byte under
`case-char-string-accepted-historical-173-{contracts,receipt}.json`.
[Numeric case completion](case-numeric.md) updates the current producer to accept
`(case 1 1 :integer 1.0 :float)` with `:integer`, using fresh independent pins
and all eight application/macro engine contexts. It moves this source from the
historical local diagnostic cohort into accepted controls; seven true
duplicate/malformed sources retain same-host recovery. The current receipt at
`/tmp/light-cljs-175-case-neighbors-final/evidence.json` has all 88 source frames
passing, including the character/string literal behavior completed in 174, and
56 recovery outcomes. The original SCI difference and zero-credit full-ledger
policy remain unchanged. Stored historical positives are never a fallback.
