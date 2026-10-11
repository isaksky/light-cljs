# Canonical sequence, iterator and realization completion — 2026-10-11

`light-cljs-121` completes the public sequence traversal/conversion surface in
`src/js_runtime.js`, shared by QuickJS macros, QuickJS applications and emitted
Node programs. `tree-seq` preserves depth-first traversal, branch/children callback
order and the upstream `mapcat` demand. `flatten` traverses sequential values and
keeps nonsequential leaves, including nil, maps, sets and strings. `iter` selects
IIterable, nil, string, array or ISeqable traversal in that order. Collection-taking
sequence helpers and their arities remain those audited in
[compiler-sequences.md](compiler-sequences.md); the sequence/eduction prerequisite
suites also execute during this change.

`es6-iterator` accepts every seqable source and nil. Construction sequences its
input; each successful `.next` advances before returning its result. Native
`Symbol.iterator` methods preserve that same demand for persistent collections,
lazy sequences and eduction. Maps expose `.has`, `.get`, `.keys`, `.values` and
`.entries`; sets expose `.has`, `.keys`, `.values` and `.entries`. Entry iterators
return JavaScript arrays, while the collections' default iterators return their
ordinary seq elements. Iterator-to-seq conversion preserves successful nonempty
tails, retries throwing `.next` calls, and reprobes exhausted nil tails exactly as
the pinned ES6IteratorSeq source does. Nil/string/array iterator exhaustion and
`.remove` retain their source-visible return behavior.

Public `ChunkBuffer`/`ArrayChunk`/`ChunkedCons` constructors and their positional
factories execute source arities. Chunk buffers expose add/chunk/count and mutable
buffer/end fields. Array chunks retain backing arrays and their indexed/count/
reduce/drop contracts, including upstream reduction over the backing array beyond
the chunk's end field. Chunked cons retains the original chunk rather than reading
and copying its elements at construction. Its rest calls the chunk's drop protocol,
chunk traversal returns the original chunk and lazy tail, and metadata/hash caching
remain observable. The private payload representation stays an implementation choice.

## Complete bodies and independent contracts

[compiler-sequence-completion.py](compiler-sequence-completion.py) structurally
reproduces **35 complete upstream workflows**, with original body/setup hashes
and upstream Eclipse Public License notice in
[compiler-sequence-completion.tsv](../fixtures/compiler-sequence-completion.tsv):

- All 27 `seqs_test.cljs` deftests, preserving every `are` row, nested setup,
  both original LITE_MODE branches under the normal false configuration, and
  original assertion messages and expected predicates.
- `iterator_test.cljs::coll-iter-seq-match` with the complete `seq-iter-match`
  helper and `TestIterRec` declaration, covering map/set/queue/record traversal.
- `core_test.cljs::test-delay`, `test-transducers`, `test-cljs-2028`; the full
  transducer chain retains `mapcat flatten`, buffered completion and all effects.
- `collections_test.cljs::test-lazy-seq-realized?`, `test-cljs-2474`,
  `test-cljs-1809`; `interop_test.cljs::test-es6-interfaces`.

These bodies retain **346 active source assertion sites**, plus one inactive
LITE_MODE site. Previously linked source assertions keep their original witnesses;
63 new source witnesses use five additional complete report cases. Independent
Rust tests execute all 35 bodies in Node, QuickJS applications and QuickJS macros,
even when their original report witness is reused. No parser or name-registration
result is treated as execution. All sixteen public declaration/arity obligations
owned by this family, its three local registration obligations, and thirteen
additional chunk/conversion declaration rows now have hashed direct ordinary and
macro execution mappings (32 total).

`compiler_sequence_completion.rs` supplies literal demand traces, custom IFn,
ISeqable and IChunk callbacks, nil/empty/lazy tree leaves, infinite finite prefixes,
repeated/interleaved consumption, thunk failure/retry and reentrant realization,
chunk backing mutation and constructor identity, lazy `apply` tails, Reduced
termination, native ES6 interfaces and separate macro/application root effects.
A reused MacroHost recovers through fresh compilation after a throwing lazy macro
and a deadline during infinite tree traversal. The existing canonical sequence and
eduction contract suites verify terminal cancellation, callback order and GC of
abandoned captured prefixes/reducer cycles in both engines.

The original test.check `iteration-seq-equals-reduce` property retains the historical
bounded 49-seed adapter and its explicit sampling rationale; this change does not
claim to run the original 1,000 randomly generated cases. The two sequence-owned
assertions in the mixed `reader_test.cljs::test-717` body remain reader integration
evidence for the reader/full-language records. `invoke_test.cljs` is an assertion-free
compiler-spelling fixture with goog.string calls and an undefined declared-fn; it
is retained as a source-module obligation owned by light-cljs-125. It is not
reported as an executed module. Its lazy/HOF/IFn behavior has independent direct
contracts here and in the existing apply suite. The inactive upstream lazy-cache
bug stays inactive. The full intended-language strict gate remains incomplete.

## Verification

Pinned independent references: Maven ClojureScript **1.12.134** and checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, with verified `:optimizations :none`,
`:static-fns false`, `:def-emits-var false`. All 35 complete workflows return true
against both references. Thirteen independent direct literal/protocol probes also
match both; macro wrappers are omitted from those reference-only probes because
same-file compiler-executed macros are an intentional local dialect contract.

```sh
cargo test --locked --test compiler_sequence_completion --test compiler_sequence_bodies \
  --test compiler_sequence_contracts --test compiler_eduction_contracts --quiet
cargo test --locked --test compiler_sequence_bodies \
  sequence_completion_fixtures_match_pinned_release_and_checkout -- --ignored --exact --nocapture
cargo test --locked --test compiler_sequences --test compiler_deferred \
  --test compiler_sequence_transducers --test compiler_eductions --test execution \
  --test evaluator --test compiler_interop --test compiler_core_helpers --quiet
cargo test --locked --test analyzer --test compiler_protocols --test compiler_records \
  --test compiler_types --test compiler_macros --quiet
cargo test --locked --test compiler_language --test provenance --quiet
node tests/support/js_runtime.test.js
node tests/support/js_core_helpers.test.js
python3 -B tests/reference/compiler-sequence-completion.py
python3 -B tests/reference/compiler-language/extract_inventory.py --check
rustfmt --edition 2024 --check tests/compiler_sequence_completion.rs tests/compiler_sequence_bodies.rs
git diff --check
cargo bench --locked --bench evaluator -- --iterations 20
```

Focused sequence/iterator suites, both oracles, accounting, provenance and focused
formatting pass. Analyzer/protocol/record/type suites pass. The 102-test macro
suite has 100 passes and two existing failures:
`generated_map_keys_keep_reader_name_parts_before_runtime_spelling_equality` and
`javascript_form_bridge::form_and_bounded_environment_use_actual_nested_call_and_shadow_ids`.
Both exact failures reproduce in a disposable `git archive` checkout of starting
commit `2874a10` at `/tmp/light-cljs-121-baseline.e8PShg`; these earlier macro
metadata/name-part gaps remain with light-cljs-125. The baseline all-target Clippy
command also reproduces the same 49 diagnostics. Neither failure is attributed
to this sequence change.
Ordinary suite invocations retain explicitly ignored reference tests; the new
35-body oracle above is separately run. The validated evaluator benchmark reports
20-iteration totals of 292182625 ns fresh, 2011375 ns reused and 1312905917 ns for
bounded-loop/reset/recovery, comparable to the preceding sorted audit's
285057583/2019917/1293872291 ns. These are observations, not timing thresholds.
Whole-repository `cargo fmt --all -- --check` reports existing drift beginning in
untouched analyzer logic. `cargo clippy --locked --all-targets -- -D warnings`
reports 49 existing diagnostics in unchanged analyzer/compiler/session/host code;
this focused change does not alter those code paths. Full hosted CI, platform-specific
runs and the complete upstream project suite were not executed.

A broader `report.py --run --feature lazy-sequences --reference both` diagnostic
run returned 644 covered observations, zero semantic mismatches, and all 27
supplemental boundary checks verified. It ran before the final constructor/hash
and lazy chunk-next refinements; the final focused suites and 48-workflow oracle
(35 full bodies plus 13 literal probes) above verify those refinements. Its
family-owned missing source sites are only the two mixed reader-test assertions.
The remaining missing rows in its overall report are broader unselected public
obligations, and cannot be inferred passed from this selected family. A later
`--reference none` report verifies the ordinary engines and direct phase rows,
while correctly retaining absent reference observations as missing.

The 32 exact declaration mappings bind to these final named tests, each of which
was also run separately with Cargo's one-test/one-pass/zero-ignored outcome:

```sh
cargo test --locked --test compiler_sequence_completion -- --exact public_sequence_conversion_chunk_and_traversal_run_in_both_phases --nocapture
cargo test --locked --test compiler_sequence_completion -- --exact public_chunk_constructors_retain_backing_storage_and_custom_protocol_demand --nocapture
cargo test --locked --manifest-path /tmp/light-cljs-121-baseline.e8PShg/Cargo.toml --test compiler_macros generated_map_keys_keep_reader_name_parts_before_runtime_spelling_equality -- --exact --nocapture
cargo test --locked --manifest-path /tmp/light-cljs-121-baseline.e8PShg/Cargo.toml --test compiler_macros javascript_form_bridge::form_and_bounded_environment_use_actual_nested_call_and_shadow_ids -- --exact --nocapture
```
