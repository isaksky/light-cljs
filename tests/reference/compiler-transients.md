# Canonical transient collections — 2026-10-11

`light-cljs-117` adds transient vectors, array/hash maps, and hash sets to the
first-party JavaScript runtime used by Node applications, QuickJS evaluation,
and compiled macros. The prerequisite ECMAScript macro/interop path was checked
with `compiler_interop`, `compiler_protocols`, `compiler_types`, and
`compiler_core_helpers` (16 tests passed).

## Implementation and observable contracts

The public functions `transient`, `persistent!`, `conj!`, `assoc!`, `dissoc!`,
`disj!`, and `pop!`, their fixed/variadic paths, and all six IEditableCollection/
ITransient* protocols lower through the existing generic core emitter. Protocol
methods `-as-transient`, `-persistent!`, `-conj!`, `-assoc!`, `-assoc-n!`,
`-dissoc!`, `-disjoin!`, and `-pop!` support saved functions and custom dispatch.
Transients supply IFn/ILookup/ICounted, plus IIndexed for vectors. They are not
persistent collections, sequences, metadata values, or structural hash/equality
values. No native implementation or immutable editable-construction substitute
remains; `into` uses the real public transient protocols, including custom targets.

Private mutable arrays reuse the canonical collection cells, equality, hashes,
and original key representatives. Conversion copies persistent storage;
persistence freezes the result and revokes mutation/count ownership. Persistent
aliases never expose mutable storage. This canonical array representation has
linear-time conversion, unlike upstream tree-based constant-time conversion;
no upstream internal storage/tree shape is advertised. Persistent results drop
metadata, while `into` reapplies destination metadata.

Both pinned versions permit TransientHashMap lookup after persistence, and reject
TransientArrayMap/vector lookup. A set follows its backing map's restriction.
Direct transient IIndexed dispatch retains the pinned numeric coercions, while
public nth rejects nonnumeric indexes. Three-argument transient-vector nth returns an out-of-range fallback after
persistence but rejects in-range access. Transient-set lookup returns the query
key; persisted lookup retains the original representative. Array-map promotion
returns a new transient hash map, leaving the previous editable alias independent.
Bounds/hash/entry failures retain earlier completed mutations and do not damage
persistent sources. Malformed `into` transforms can expose an opaque transient,
matching the references; the three earlier persistent-fallback differences in
`compiler-transducers.tsv` are removed.

## Complete source and phase evidence

The references are Maven **ClojureScript 1.12.134** and clean checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, using `:optimizations :none`,
`:static-fns false`, and `:def-emits-var false` independently checked by
`execution.clj`. No transient version difference was observed.

`compiler-transients.tsv` retains fourteen complete bodies from collections,
core, and seqs tests, plus one independent API/arity/protocol contract. Original
body hashes, FixedHash setup, threshold binding, loops, effects, and expectations
are preserved. Only testing/is/are harness forms become executable do/assert/
bound rows, with ordinary `LITE_MODE=false`. The fifteen workflows execute in
both Node and QuickJS and independently match both references. The macro
contract performs the same mutations/assertions before returning persistent
expansion data. A separate test checks custom editable `into` and revocation of
an escaped reducing accumulator. Direct Node assertions bypass compiler lowering
and the shared observation encoder, covering storage forgery and failed effects.

The full intended-language catalog contains fourteen complete-body adapters,
61 new assertion mappings, eleven faithful public source/arity mappings, and
27 source/arity-bound macro witnesses. The transient-selected live report covers
76 source assertions and those eleven public obligations, with all 27 macro
witnesses passed. The historical selected scope is retained as historical evidence;
its exclusion does not gate the new intended-language adapters.

Two source sites using optional `obj-map`/`hash-map-lite` remain **missing**.
Sixteen source-specific public obligation mappings (protocol identities/methods
and the zero/one-argument conj! declarations) also remain missing in the ledger;
the independently sourced upstream bodies do not establish those exact mappings.
Their actual ordinary and macro behavior is executed by the independent contract;
a registration or absent original witness is not claimed as semantic failure or
as coverage. The overall language gate remains incomplete and belongs to the
remaining live Pellets. No omitted or failed site is counted as a pass.

## Verification commands and outcomes

Run from the repository root:

```sh
node tests/support/js_runtime.test.js
cargo test --locked --test compiler_transients --test compiler_transducers --test compiler_transducer_contracts --test execution
cargo test --locked --test compiler_transients transient_fixtures_match_pinned_release_and_checkout -- --ignored --exact --nocapture
cargo test --locked --test compiler_transducers eager_transducer_fixtures_match_pinned_release_and_checkout -- --ignored --exact --nocapture
cargo test --locked --test compiler_language --test provenance
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B tests/reference/compiler-language/report.py --run --feature transient-collections --output /tmp/transient-completion-verified.json
```

The direct runtime assertions pass. Transient tests pass (three ordinary tests,
including the macro contract), transducer tests pass (316 Node/QuickJS observations
and provenance), and the six transduction ABI/lifetime tests pass. The execution
suite has 53 passing ordinary tests across the broad run and corrected targeted
reruns, with 43 unrelated reference tests ignored. During conversion
of `into`, two execution tests exposed the upstream requirement that a custom
IVector map entry also be callable; both named failing tests were rerun and passed
after matching that dispatch. The fifteen transient and 316 transducer workflows
were explicitly run against **both** references: neither focused oracle is left
ignored. Language accounting (including failure/subset/zero-test handling), both
provenance tests, and regenerated ledger reproducibility pass. The focused ledger
run also verifies all 27 existing supplemental boundary contracts.

No full intended-language strict pass, all unrelated ignored reference tests,
Windows/Linux execution, or broad application performance claim was attempted.

A nine-sample Node smoke compared 500 builds of 256-element vectors followed by
count/nth/hash checks with baseline `ba40f83`, current persistent construction,
and current transient construction. All checksums matched **871341648000**.
Medians were **755.39 ms**, **788.14 ms**, and **655.07 ms** respectively
(`node /tmp/transient-perf.js`, output `/tmp/transient-perf-final.json`). These
samples show about 4.3% persistent-path overhead and 16.9% faster transient
construction in this bounded workload; they are not a general performance guarantee.
