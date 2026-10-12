# Safe local regression proposal after194

The completed read-only command `cargo test --locked --lib --tests -- --list`
exited zero after 7m24s of compilation. Its complete output is
`/tmp/light-cljs-125-after194-safe-test-inventory.log`. It lists 848 test identities
across 160 Cargo target summaries. Current complete Rust test bodies, including
nested library modules and imported support modules, match every listed identity
exactly. This inventory executes zero semantic regressions.

The complete source review proposes 711 ordinary active tests and leaves 104
ignored tests unexecuted. Thirty-three whole active tests are excluded under the
current cancellation/resource-limit boundary. The 64 exact phase tests in the
permanent phase witnesses are all present among the safe candidates. The other
647 ordinary tests are independent named regressions outside that phase census;
this includes the new separately executed private after194 arity test. No fresh
pass is inferred for any other ordinary regression from list-only output or a
phase registration.

`safe-source-regression-audit-after194.json` retains every Cargo identity, exact
current file/function-body hashes, ignore state, whole-test disposition, manual
review basis, full-gate phase registration and explicit command. Its previous
incomplete source-only inspection remains in
`safe-source-regression-audit-after194-before-list-history.json`, with no
execution credit. `safe_regression_inventory_after194.py` is read-only and
rejects incomplete lists, duplicate identities, detached current bodies and
unreviewed resource signals. `safe-regression-commands-after194.json` proposes
129 complete target commands. No command in this proposal has run.

## Exclusions and allowed ordinary tests

The exact 33 exclusions appear in the JSON receipt with located body hashes.
They cover library bootstrap/service cancellation; evaluator sticky cancellation
and deadlines; macro expansion/count/node/conversion/execution exhaustion;
regex controls; EDN providers; transducer/sequence/eduction cancellation;
library traversal controls; CLI/REPL/session source/output/OOM/budget controls;
the mixed lazy-source test containing infinite demand under a short deadline;
and the whole historical SCI accounting test. Low-memory collection-retention,
structural parser nesting-bound and malformed/unbounded protocol-validation
tests are conservatively excluded from this safe proposal as documented in
their individual rows.

The important indirect exclusion is
`evaluator_conformance::every_historical_sci_case_has_an_executable_evaluator_disposition`.
Its complete body calls `tests/support/sci_evaluator.rs::run` for every historical
original. That helper cancels `sci-recur-operand-failure` after 256 effects and
uses terminal deadlines for `sci-deferred-lazy-self` and
`sci-deferred-lazy-mutual`. The whole mixed test is excluded; its safe subparts
are never extracted or used as replacement evidence. The two ignored historical
audits remain unexecuted. The current full-gate compiler/typed-corpus adapters
are separate from that native helper; registration is never treated as an
execution claim for those three IDs.

Ordinary finite cycle correctness, finite lazy range/GC/handle ownership,
constant-stack recurrence, large argument/vector behavior, the IFn terminal rest
slot and print-length/print-level remain eligible. The word terminal in an IFn
rest-slot name describes argument position. The printing depth test observes
ordinary object-prefix rendering. CLI help and retired-option validation reject
options before execution and exercise no resource exhaustion. These distinctions
are explicitly recorded rather than inferred from a substring skip.

Never execute `benches/evaluator.rs` or
`tests/support/eval_cli_terminal.py` under this proposal. Do not use unfiltered
`cargo test`, `--ignored` or a benchmark command. All ignored jobs require their
own exact source/context/artifact setup and individual boundary audit before a
separate authorized execution. No ignored job is a skipped pass.

## Command proposal

Each proposed command runs an entire Cargo target with all exact excluded names
passed through libtest `--skip`; ordinary body/setup/effect paths remain intact.
Default ignored handling stays enabled. For targets with no exclusions, the
command is simply `cargo test --locked --test TARGET --`; library tests use
`--lib`. Targets containing only ignored jobs receive no execution command.
Every command has the exact expected active-safe count, ignored count and
excluded names, so a future runner must reject count drift or unexpected test
identities. It should execute sequentially, retain complete stdout/stderr/exit
and actual binary/source provenance, and report actual failures without rewriting
expectations. Cargo skips accept substrings; the retained full qualified names
are unique exact identities in the current target census.

For a local traversal that avoids repeating already executed full-gate phase
checks, each target may also skip only the exact 64 registered phase identities
once the parent's current full gate has independently confirmed they passed.
Registration alone does not authorize that omission. The separately completed
private arity test likewise has its own retained proof; keep that proof distinct
from comprehensive regression completion.

Regeneration is a read-only operation:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -B tests/reference/compiler-language/safe_regression_inventory_after194.py --list-log /tmp/light-cljs-125-after194-safe-test-inventory.log --output /tmp/light-cljs-125-after194-safe-regression-proposal.json
```

The source recipe does not implement a task queue or launch tests. Production,
shared witness/report/manifest inputs, all completed arity proofs and the
156/native-crash boundary remain unchanged. Existing unrelated rustfmt drift is
outside this proposal.
