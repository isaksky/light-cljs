# Exact library dialect reconciliation

The 22 records in `sci-library-reconciliation-contracts.jsonl` retain their
complete original SCI source, expected outcome, effects and output. Eleven
canonical string/set observations agree between the local compiler and both
required CLJS pins but differ from the historical SCI expectation. Eleven strict
EDN observations agree with the adopted local/SCI contract and differ from both
CLJS pins. These are separate source-specific differences; neither category is
an original SCI pass or a release-versus-checkout disagreement.

`sci_library_reconciliation.py` checks the unchanged full 191-context reference
oracle and current source bindings. Its Rust driver executes every complete
context in application and actual MacroHost phases, with fresh and reused hosts
and both Node and QuickJS engines. The resulting 176 observations retain literal
outcomes, errors, callback order and output. All original SCI receipt statuses
and recursively retained history remain unchanged.

The report runs this audit only alongside fresh complete 191-context evidence
from both required pins. The full ledger revalidates the local build, raw phase
frames, compiler/library/producer hashes, original full-oracle observations and
the sidecar's attachment to the current report. A stored sidecar alone grants
no credit. A missing phase, altered source, stale producer, partial reference
catalog or changed original comparison fails closed. The failure policy explains
only an exact verified `mismatch`; rejection, timeout, unavailable evidence and
execution failures still fail.

The standalone proof against the checkpoint's complete unfiltered execution is
retained at `/tmp/light-cljs-125-library22-rust-phase-audit-1/full-gate-reconciliation.json`.
It preserves all original unresolved comparisons alongside the separate literal
contracts. The complete full-gate library evidence is at
`/tmp/light-cljs-125-full-9964c3c/sci-library-context/evidence.json`. Earlier loader
and subset namespace experiments remain failed harness observations; they do
not supply exact-source reference credit.

Reproduce a fresh local audit and reconcile against a separately executed
complete current library oracle:

```sh
python3 -B tests/reference/compiler-language/sci_library_reconciliation.py \
  --run-local /tmp/light-cljs-library-dialect-fresh \
  --full-evidence /path/to/fresh/sci-library-context/evidence.json \
  --output /tmp/light-cljs-library-dialect-fresh/reconciliation.json
python3 -B -m unittest discover -s tests/reference/compiler-language \
  -p 'test_library_reconciliation.py'
```

This cohort does not complete the intended-language ledger or establish
cancellation, whole-suite or production performance acceptance.
