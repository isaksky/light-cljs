# Public collection construction evidence

This receipt covers the thirteen public collection/keyword/lazy sequence types
owned by light-cljs-141 and their generated positional factories. The existing
Cons, Reduced and Symbol construction fixtures and source-body evidence remain
in the same executable suite.

| Constructor | Exact public positional fields |
| --- | --- |
| List | meta, first, rest, count, __hash |
| EmptyList | meta |
| Keyword | ns, name, fqn, _hash |
| Cycle | meta, all, prev, current, _next |
| Repeat | meta, count, val, next, __hash |
| Iterate | meta, f, prev-seed, seed, next |
| PersistentVector | meta, cnt, shift, root, tail, __hash |
| Subvec | meta, v, start, end, __hash |
| ChunkedSeq | vec, node, i, off, meta, __hash |
| PersistentQueue | meta, count, front, rear, __hash |
| PersistentArrayMap | meta, cnt, arr, __hash |
| PersistentHashMap | meta, cnt, root, has-nil?, nil-val, __hash |
| PersistentHashSet | meta, hash-map, __hash |

`compiler-public-constructors.py` verifies each complete declaration's pinned
byte span and SHA-256 before binding its exact signature and generated factory
to `public_constructor_api_arities_and_macro_phase_execute`. Execution evidence
is bound to that test's source hash and both fixture files. Reconciliation
replaces only this named test's rows; earlier assertion witnesses retain their
authoritative fixture links.

The body fixture contains eighteen complete upstream tests. The original four
symbol/keyword, reduction and sequence operation bodies are retained. The new
bodies cover map operations and dropping; vectors and subvecs; persistent sets;
cycle, repeat and iterate; nested lazy realization; persistent hash and array
maps; large array maps; hash-set counts; chunked sequences; and empty/seq behavior.
The complete `array-map-conversion-threshold` setup accompanies `test-pam`.
All original collection inputs, loops, side effects, exception paths, metadata
checks and realization assertions remain. `is`/`are` become `assert`, `testing`
becomes `do`, and `LITE_MODE` is bound to false with both source branches retained.
Reader-discarded source remains reader-discarded, including its original form.

The upstream inventory records 383 active assertion sites across those bodies,
plus the inactive reader-discarded map assertion and non-default LITE_MODE
branch. The retained JSON receipt contains the exact declaration forms, source
spans, body/setup hashes, signature hashes and independent reference results.

All eighteen complete bodies pass independently on Maven CLJS **1.12.134** and
checkout **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. The checked harness uses
`:optimizations :none`, `:static-fns false`, and `:def-emits-var false`.
Body-only reference artifacts are in
`/tmp/light-cljs-141-complete-bodies-reference-1`. Each body also returns true in
local QuickJS application and canonical macro execution.

The final construction run executes **27 independent contracts and 18 complete
bodies**, with all **45 cases passing on each reference**. Its full build logs
and checked results are in `/tmp/light-cljs-141-constructors-final-reference-4`;
the receipt retains the observed results and the current source/fixture hashes.
The explicit ChunkedSeq witness preserves the supplied node for first and
chunk-first, while chunk-next resumes from the backing vector's full chunk.
Reduction follows the backing vector range, independently of the supplied node.
Canonical vector/map root slots remain supported opaque runtime values;
private upstream trie representation is outside the public construction contract.

Reconciliation binds 26 new public declarations/factories and 26 corresponding
local metadata registrations, alongside the earlier twelve rows, for 64 rows in
the named execution test. It adds 49 assertion witnesses while preserving prior
links for other source sites. Inventory regeneration/check, all 73 ledger
accounting tests, exact body/evidence reconciliation and diff checks pass.

The supplementary
`explicit_collection_slots_and_protocol_membership_survive_construction` test
also passes as one complete source case on each checked reference. Its retained
raw source, TSV whitespace adaptation, hashes and outcomes are in
`/tmp/light-cljs-141-explicit-slots-final-reference-2` and the JSON receipt.
It checks the precise protocol membership of Cycle, Iterate, Repeat and
ChunkedSeq; divergent explicit vector count/tail/reduction slots; negative
Subvec counts; reconstruction from a non-nil canonical hash-map root; and the
supplied hash-set backing map. The supplementary JavaScript probe independently
observes the requested protocol and slot values in both existing reference
builds; its script and results remain under `/tmp/light-cljs-141-additional-runtime-probe*`.

The final probe also retains the complete source-equivalent PAM/List/Set edge
observation `[3 true nil 8 8 3 0 1 (2)]` on both pins. A supplied array-map count
controls its iterator/reduction even when the array is shorter; conversion at
the threshold retains the resulting absent nil entry. A coherent eight-entry
array map converts to a nine-entry hash map after adding one key. List count
zero retains its supplied first and raw next tail, and hash-set counts follow
the supplied backing map. These observations supplement the coherent public
construction fixtures without asserting a private trie ABI.

Provenance verification passes both tests. All adapted body origins and the
twenty-two new independently authored contract origins are registered. Fixture
IDs use the established `-p` spelling for predicate names; the final dual
reference run executes those stable IDs with complete source bodies unchanged.
Mutable public slot setters retain canonical brand guards. The runtime owner's
final direct JavaScript regressions cover forged prototypes for every added
mutable slot; the receipt includes the current runtime and regression source
hashes and records those checks as passed.

```sh
python3 -B tests/reference/compiler-language/extract_inventory.py
python3 -B tests/reference/compiler-public-constructors.py --write --reference both --artifacts /tmp/light-cljs-141-constructors-final-reference-4
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B -m unittest discover -s tests/reference/compiler-language -p 'test_*.py'
python3 -B tests/reference/compiler-public-constructors.py
cargo test --locked --test provenance
```

This is evidence for the named construction work. It does not declare the
whole intended language or light-cljs-125 complete.
