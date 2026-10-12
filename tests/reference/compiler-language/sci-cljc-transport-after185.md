# Original CLJC intermediate result transport after185

This supplemental proof resolves three previously missing original IDs inside the existing thirty-source `.cljc` cohort. It is an explicit override for those exact IDs, not another source registration or new obligation count. Every original source, original seven fixture columns, full disposition row, reader feature and SCI expectation stays unchanged. `sci_cljc_accepted_context.py` and its original reviewed contracts are not edited.

Both independently compiled pins use release1.12.134 and checkouta5e94c988fd0b50ffcff1ce6add12ffbb7839a3e with `:optimizations :none`, `:static-fns false`, `:def-emits-var false`, `.cljc` filename and the adopted `:cljs` reader feature. The complete original source executes before the test observer encodes its result. The application executes inside its ordinary function body; the demanded macro executes inside its actual macro body and returns only the resulting JSON string. The observer does not edit a source literal, replace a reader branch, precompute the original result or export an unsupported macro value.

The independently reviewed complete packets are:

| Original ID suffix | Complete canonical value | Original SCI value |
| --- | --- | --- |
| `skipped-arguments` | Vector `[1, {$value: "undefined"}]` | Vector `[1, 2]` |
| `skipped-regex-bigint` | Vector `[{$regex: {source: "x", flags: ""}}, 1e42]` | `42` |
| `skipped-regex-identity` | Set containing two separate `{$regex: {source: "x", flags: ""}}` entries, count `2` | `2` |

The regex source/flags, Number value, vector positions, set membership multiplicity/count, undefined brand, complete outcome, empty trace and empty stdout are literal contracts. Two equal regex descriptions do not merge the distinct set members. No sequence sorting, typed collection conversion, effect omission or value-only reduction is used. All 24 fresh/reused application/demanded macro QuickJS/Node frames must match exactly. The canonical reader branch differs from SCI's historical `:clj` branch in all three cases; original SCI semantic pass credit remains zero.

`run(directory, timeout=600)` creates fresh independent pin compilation/execution and actual local compiler/engine artifacts. `reconcile(document)` checks current compiler/source/helper/encoder/oracle/contracts, actual executable paths/hashes, exact command/environment bindings and the entire raw artifact census. It obtains the current exact audit executable through a fresh Cargo build replay and verifies its path/hash, then reruns the Rust compiler audit from the complete original input, compares every emitted unit and raw engine observation, independently recompiles both pins and compares their original source/emitted units, and reruns the retained pin-generated Node units. Serialized status alone cannot supply passing credit. Permanent raw guards keep forged source/emitted/raw declarations coherent and refresh the artifact census, then require rejection and pristine replay.

The remaining four original `.cljc` sources stay nonpassing with zero original semantic and phase credit. The independent complete thirty-source audit at `/private/tmp/light-cljs-125-cljc-seven-after185-discovery` confirms:

- `(meta '^:private #?(:cljs x) y)` returns `{:private true}` in both pins. Under `:cljs`, this contains quoted metadata-bearing `x` and an unbound extra `y` argument. Local fresh/reused application and demanded macro compilation rejects unresolved `y`. This source does not establish a missing metadata operation; the unbound/extra-argument difference remains explicit and unresolved. No new exception to project diagnostics or source pass is adopted here.
- `#?(:clj 42 :cljs #missing/tag [1 2])` and `#?(:cljs {#a 1 :a (a 1) :b} :clj 2)` activate unknown tags under `:cljs`; both pins reject them with missing reader-function diagnostics. All four local source phases reject the corresponding active unknown tags.
- `{:a #?(:clj 1) #?@(:clj [:b 2])}` leaves an odd map under `:cljs`; both pins reject one remaining form and all four local phases reject an odd selected map. No source rewrite or historical `:clj` expectation can make that unchanged `:cljs` source pass.

Those four findings are retained diagnostics, not newly credited reader rejections or demonstrated production defects. Historical failed serializers for the other three sources remain visible; their failure is replaced only by this complete independently reviewed transport proof.

The helper is `sci_cljc_transport_after185.py`, the original-source reference oracle is `sci_cljc_transport_after185.clj`, and `sci-cljc-transport-after185-encoder.cljs` is a test-only observation adapter. `tests/compiler_sci_cljc_transport_after185.rs` drives actual namespaces and MacroHosts. Literal contracts are in `sci-cljc-transport-after185-contracts.json`; guards are in `test_sci_cljc_transport_after185.py` and use `LIGHT_CLJC_TRANSPORT_RECEIPT` for fresh raw mutation fixtures. Production code and original SCI/upstream fixtures stay unchanged. Blocked156, native/asynchronous cancellation and resource-limit work remain excluded.

The final receipt `sci-cljc-transport-after185-receipt.json` is a byte-for-byte copy of `/private/tmp/light-cljs-125-cljc-transport-after185-final-v1/evidence.json`, produced after the shared report/full-ledger integration freeze. Both pins and all 24 actual phase/engine frames pass. The four guard tests pass with zero skips using that fresh receipt, including coherent local and pinned emitted-unit mutations, a missing engine frame and changed raw packet after refreshed artifact censuses; pristine evidence is restored and replayed. This proof retains zero original SCI pass credit and does not resolve the four diagnostic originals above.
