# Demanded transient and sorted receiver candidates

This third independent Pellet 125 cohort exports **31 additional links**:
14 public declaration/signature rows and 17 local operation cross-checks,
with 31 exact actual-phase requirements and two whole original source oracles.
The earlier 24-row and 35-row proofs remain unchanged. No shared catalog,
production source, original assertion, literal, option or queue state is edited.

The complete existing `CONTRACT` sources from `compiler_transients.rs` and
`compiler_sorted_contracts.rs` remain frozen byte-for-byte. Each existing exact
test runs its whole contract in QuickJS/Node applications and through actual
MacroHost `defmacro` computation. Reference TSV transport normalizes only
newlines/tabs outside literals; the helper rejects comments or multiline string
literals that would make that transport unsafe. Both original source and
normalized reference source hashes are retained separately.

The transient contract directly demands all eight genuine custom receiver
methods: `-as-transient`, `-conj!`, `-persistent!`, `-assoc!`, `-dissoc!`,
`-assoc-n!`, `-pop!` and `-disjoin!`. It asserts the full original ordered event
vector and retains native transient mutation/identity, after-persistent errors,
hashing, metadata, lookup and saved public operations. Its previously unmapped
zero/one-argument public `conj!` signatures also have literal result/identity
assertions in this complete contract.

The sorted contract directly demands `-sorted-seq`, `-sorted-seq-from` and
`-entry-key` on genuine sorted collection receivers. Local links retain
comparator identity, sorted constructors, source key representatives, metadata,
hashing, both traversal directions, from-key behavior, reduction termination,
scalar boundaries and all caught negative controls. The full original `doseq`
loops are also executed. Saved bindings `sub=subseq` and `rsub=rsubseq` execute
valid three/five-argument calls; invalid direct arities are not used to grant
supported operation credit. Method definitions alone receive zero credit.

Final executed proof:
`/tmp/light-cljs-125-arity-receivers-verified-v3/receipt.json`, SHA-256
`070b538949e924b7226fca1a28c450cd2234b6c99775aa78a4020a60a2eb1079`.
Production HEAD is `8be52ad6d2e37e0c9e9a7a64a61244f14f7094d9`. The receipt
retains current compiler/build/library hashes, helper/driver/audit executable
hashes, full source/literal/call proofs, exact named Rust execution logs and four
passing required-pin outcomes. Adjacent JSONL files contain parent-owned
integration candidates. The final raw log is
`/tmp/light-cljs-125-arity-receivers-verified-v3.log`.

Actual focused commands:

```sh
cargo test --locked --test compiler_transients transient_api_arities_protocols_and_macro_phase_execute -- --exact --nocapture
cargo test --locked --test compiler_sorted_contracts sorted_contract_executes_in_application_and_macro_phases -- --exact --nocapture
PYTHONDONTWRITEBYTECODE=1 python3 -B -m unittest discover -s tests/reference/compiler-language -p test_arity_resume_receivers.py -v
python3 -B tests/reference/compiler-language/arity_resume_receivers.py --export-owned /tmp/light-cljs-125-arity-receivers-candidates
```

The initial exact Rust executions passed in 1.59 and 2.80 seconds, with one test,
zero failures and zero ignored tests each. Final provenance-bound replays repeat
both using `--message-format=json` and then run both pins against the unchanged
complete literal oracles with timeout 600. All seven focused guards pass in
0.285 seconds, including original effect/literal drift, receiver-inclusive
signatures, undemanded method definitions and detached saved operation bindings.

The first exploratory reference invocation supplied raw multiline source to the
older TSV transport and was rejected with unexpected EOF before semantic
execution. It grants zero credit. Faithful whitespace-normalized replay passes
both full original contracts and the separately audited original custom sorted
receiver context. No failing expectation was replaced or assertion removed.

A separate temporary staged loader check at
`/tmp/light-cljs-125-arity-staged-loader-current-v1` confirms that all three
candidate cohorts combine without duplicate obligations: **90 unique ordinary
and actual-phase bindings with 21 exact whole-source oracle links** pass the
existing `full_ledger` validators. This is candidate shape validation, not a
comprehensive semantic report or final strict acceptance. No original source
assertion credits are added by any of these three cohorts.

All 21 combined guards pass in 0.585 seconds at
`/tmp/light-cljs-125-arity-all-guards-current-v1.log`. All 21 exact whole-source
oracles also freshly pass the current generic report application adapter at
`/tmp/light-cljs-125-arity-generic-report-adapter-v1/receipt.json`. This separate
proof uses `report.local_layers`, retaining the existing lexer/parser/compiler
and Node observation route, current production binary/source hashes and every
original literal. It confirms the candidate fixtures can execute through the
ordinary report path before integration; it supplies no new comprehensive gate
counts or additional actual-phase credit.

Remaining unreviewed rows stay missing. No production repair, cancellation or
limit suite, 156 investigation, shared table write, commit or queue mutation was
performed. Parent-owned integration and the full strict gate remain required.

After the frozen comprehensive run completed, parent-owned integration added
all 90 ordinary links, 75 new phase entries and 21 full original oracles. Fifteen
existing phase entries now use the reviewed anchors; their entire previous rows
remain in `arity-resume-prior-phase-witnesses.jsonl`. The three call-receipt tables
retain exact reviewed demands and source/literal provenance.
The normal focused report at `/tmp/light-cljs-125-arity-focused-current/report.json`
freshly verifies all 21 whole sources in both pins and all 90 linked ordinary and
actual macro obligations. The command is saved at
`/tmp/light-cljs-125-arity-focused-current-command.json`; exit 0 with
`all_selected_fixtures: false` and `strict_complete: false`. This focused source
selection adds no original assertion sites and supplies no comprehensive gate
or registration-only pass. The independent preparation claims above describe
the delegate's earlier work; integration does not rewrite those historical facts.
