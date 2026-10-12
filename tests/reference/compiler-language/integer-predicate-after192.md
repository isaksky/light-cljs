# Complete original integer predicates on initialized genuine Closure receivers

The narrow public `int?`, `pos-int?`, `neg-int?` and `nat-int?` repair recognizes
actual initialized Closure Integer and Long class instances. Signed predicates
use the genuine sign/zero methods rather than coercing objects to Numbers.
`integer?`, ordinary Number semantics, macro/application source policies and
external construction/loading boundaries remain unchanged. Classes are resolved
when the predicate is called because the explicit genuine loader runs after the
first-party runtime is created. No classes or library loader are synthesized.

The complete unchanged original `int-val-table` definition and `test-int-preds`
body are reused, including all 14 rows, eight predicate functions, negative zero,
large Numbers, six genuine object constructors, loop and assertion message.
The source hashes remain those in the preserved
[historical 191 diagnostic](integer_predicate_after191.md). Neither that diagnostic,
its expectations nor earlier failed histories were edited.

Before repair, a fresh baseline run independently reproduced exactly 98 passes
and 14 class-predicate failures in each of eight local contexts; all 64 Number
comparisons, six `instanceof` and six exact `toNumber` controls passed. Its raw
root/log/proof are `/tmp/light-cljs-192-baseline`, the sibling `.log`, and
`/tmp/light-cljs-192-fresh-baseline-proof.json`. The historical independent failure
matrix and artifact proof accepted this fresh execution before production edits.

After repair, all eight QuickJS/Node × application/demanded macro × fresh/reused
MacroHost contexts pass all 112 original comparisons. Both independently fresh
whole-original-namespace reference builds pass 112/112: release 1.12.134 and
checkout a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e, with optimizations none,
static-fns false and def-emits-var false. The immutable originals remain outside
the external LongImpl alias/assertion adapters, matching the prior audited
embedding. This does not claim namespace-wide cljs.test or Closure ecosystem parity.

Focused controls independently require Number zero/negative zero, signed Numbers,
fractions, NaN/infinities, exponent-notation limits, nil, maps, spoofed class-shaped
objects whose methods throw, and genuine zero/positive/negative Integer and Long
instances. The positive Integer exceeds finite Number magnitude, proving sign
behavior does not depend on `toNumber`. Six receiver effects occur exactly once
through direct predicates, a saved alias and apply. Separate ordinary permanent
regressions prove macro/application root isolation, reuse and located receiver-error
recovery without any external class loader.

The acceptance producer and proof are `integer_predicate_after192.py` and
`integer_predicate_after192_proof.py`; their driver is
`tests/integer_predicate_after192.rs`. The driver itself requires every original
comparison to pass. The proof reconstructs the full source and inputs, validates
frozen producer/compiler bytes, Cargo executable, genuine library archive and
13 dependency hashes/order, complete raw artifact censuses, all paired engine
frames, exact focused controls and both pin identity/settings/commands/counters.
All 15 raw/source/emitted/input/loader/pin mutations reject and restore an identical
pristine proof. Comprehensive original-site/ordinary/phase ledger credit remains
zero here: Pellet125 still owns complete current ledger integration.

## Reproduction

Use fresh output directories and the existing byte-audited dependency origin.
Unavailable tools, altered origin, incomplete records or exceptions reject.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/original_integer_predicate_after190.py --reference release --output PINS/release
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/original_integer_predicate_after190.py --reference checkout --output PINS/checkout
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/integer_predicate_after192.py --prepare ACTUAL
LIGHT_INTEGER_PREDICATE_AFTER192=ACTUAL cargo test --locked --test integer_predicate_after192 whole_original_integer_table_passes_in_actual_hosts --message-format=json -- --ignored --exact --nocapture > ACTUAL.log 2>&1
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/integer_predicate_after192_proof.py --actual ACTUAL --log ACTUAL.log --references PINS --output PROOF.json --controls
cargo test --locked --test compiler_integer_predicates
```

## Regression scope

Numeric literal/case, core helper, predicate and macro regressions pass 124 tests.
The broad execution run passes 50 tests but preserves three unrelated failures:
obsolete map/set literal expectations (focused owner193) and the old Boolean
deferred Var metadata fixture (owner194). Both owners were duplicate-checked and
placed before125. The Var failure is independently reproduced using the unchanged
whole assertion suite with a byte-exact startingHEAD3235870 runtime copy.
No failed original was deleted, recategorized, normalized or credited as passing.

The [normal compiler performance record](integer-predicate-after192-performance.json)
retains 459 validated samples across 51 scenario/phase pairs, nine samples and four
iterations, without allocation instrumentation. Prior medians are context, not a
controlled performance comparison; concurrent verification can affect timing.
Native asynchronous cancellation and native resource-limit suites were excluded;
externally blocked156 remains untouched. No general language completion is claimed.

Final strict `cargo clippy --locked --all-targets -- -D warnings` passes with zero
diagnostics. Its compilation of target source does not execute excluded suites.

The permanent accepting receipt is `integer_predicate_after192_receipt.json`;
raw controls and byte-identical executable preservation are recorded in the sibling
`integer_predicate_after192_raw_controls.json` and
`integer_predicate_after192_executable_snapshot.json`. Accepted raw artifacts are
`/tmp/light-cljs-192-acceptance-v6`, its sibling `.log`, and
`/tmp/light-cljs-192-pins/{release,checkout}`. Earlier setup attempts remain at
`/tmp/light-cljs-192-acceptance{,-v2,-v3,-v4}` and sibling logs; the first attempts
exposed fixture/guard mistakes and grant no acceptance credit. No historical
artifact was rebound to the final producer snapshot.

The evaluator regression command passes with exactly these two tests excluded:
`terminal_controls_survive_catch_and_fresh_host_sessions_recover` and
`deadline_allocation_and_panics_produce_owned_failures`. All remaining ordinary
application/interop/collection/ownership/reuse/teardown checks pass; the existing
startup timing test stays explicitly ignored and is not acceptance evidence.

The prior accepted v5 producer sources remain preserved separately at
`/tmp/light-cljs-192-acceptance-v5-producers`; v6 repeats the entire acceptance and
raw controls after removing one trailing space from the new proof source.
