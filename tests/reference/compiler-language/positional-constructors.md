# Direct positional collection constructors

Pellet 177 preserves the complete source:

```clojure
[(count (array-map \a 1 "a" 2)) (get (array-map \a 1 "a" 2) \a) (count (hash-set \a "a"))]
```

Both independently executed ClojureScript pins, release `1.12.134` and clean
checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, return `[2 1 2]` with
`:optimizations :none`, `:static-fns false`, and `:def-emits-var false`.
The expectations in `positional-constructor-contracts.json` were derived from
fresh complete-source compilations and Node executions in both pins before
comparison with this implementation. The catalog freezes exact source hashes,
complete typed outcome/effects/stdout packets, diagnostics, and its derivation
artifact directory. The producer freezes the entire catalog's SHA-256 and
requires new independent reference executions for delivery.

The upstream `array-map` and `hash-set` macros distinguish reader constants
before JavaScript lowering. A direct call with distinct character/string or
numeric constant spellings preserves positional entries, first-match lookup,
and positional count and sequence. `array-map` retains this construction path
above eight pairs; the `hash-set` shortcut applies only through eight arguments.
Equal reader spellings, nonconstant arguments, saved functions and `apply`
retain runtime deduplication. `hash-map` and `set` continue to deduplicate.
The analyzer lowers only the direct constant-collision calls to the existing
positional collection IR; it does not change those runtime constructor functions.

The 22 complete controls cover the original discrepancy, forward/reverse order,
count, lookup, membership, entries/keys/values/sequence, numeric collisions,
equal arguments, direct/saved/apply/dynamic calls, zero/singleton/eight/nine
sizes, Unicode/named/quoted spellings, metadata, hash stability, map/set updates,
promotion, ordered operand effects, custom protocol effects, and caught odd
argument diagnostics. Exact operand effects are `[:first :second :third
:fourth]` and captured output is `"2\n11\n"`; every observer's trace and stdout
are compared exactly. The independently pinned custom-key control requires no
hash/equality effects for identical keys.

Both pins' positional removal behavior is retained: `dissoc` locates a key,
allocates room for one removed pair, then skips all equal keys during copying.
For collided lowered entries, trailing slots remain `undefined` while count
falls by one. `disj` on the positional set delegates to its backing map.
The complete update controls explicitly inspect those slots with `undefined?`
so demanded macro results contain portable finite values; they retain metadata
and also prove that the original collection is unchanged. Promotion deduplicates
through the existing hash-map path and chooses the later value.

The actual Cargo-built audit compiles every complete source as an ordinary
application and a demanded macro computation in fresh and reused MacroHosts.
The macro quotes its actual computed result; it never quotes an expected packet.
Both resulting programs execute under QuickJS and Node, giving 176 accepted
frames. A source-specific nested duplicate diagnostic rejects in all four
application/macro and fresh/reused host contexts; a demanded recovery macro on
that same host computes `(+ 20 22)`, returning 42 under both engines (8 outcomes).
The ordinary Rust regression checks the same frozen complete sources.

The producer and replayer bind current compiler/proof/driver sources, exact
executable paths and hashes, commands and environments, reference identities
and settings, full namespace source bytes, emitted JavaScript, actual engine
observations and the complete artifact hash census. Modified, missing, extra,
stale or detached evidence rejects. Eight restored mutations failed closed:
complete phase source, emitted namespace, Node input, raw engine outcome,
same-host recovery source, executable hash, audit environment, and serialized
summary. File mutations were rejected even after updating the artifact census;
the final pristine replay passed. The guard observations are retained in
`positional-constructor-raw-guard-receipt.json`.

```sh
cargo test --locked --test compiler_positional_constructors canonical_positional_constructors_and_same_host_diagnostic_recovery -- --exact
python3 -B -m unittest discover -s tests/reference/compiler-language -p test_positional_constructors.py
python3 -B tests/reference/compiler-language/positional_constructors.py --output /private/tmp/light-cljs-177-positional-delivery-final-v2
```

The delivery receipt `positional-constructor-receipt.json` is byte-identical to
`/private/tmp/light-cljs-177-positional-delivery-final-v2/evidence.json`. A saved receipt
never substitutes for the current raw executable proof. These focused controls
add zero original upstream/SCI-body, public-arity, or full-language completion
credit. Set literal traversal remains Pellet 178 and generic map evaluation
order remains Pellet 179. Both remain before strict gate 125; no source or
criterion from either record is relaxed. Externally blocked 156 was untouched.
No cancellation or singular/plural limit test suite, push, or PR ran.

Delivery SHA-256: `2eaacafb0d83354b07764dca7c825605ad95a7389e38a86dae2b4bacab18a222`.
Surrounding verification passed 71 ordinary analyzer/reader/constructor/equality/
core-helper/hash-effect/literal/transient regressions, plus the new constructor
regression and 13 source/raw-reconciliation guards. The compiler benchmark
completed 423 validated measurements (nine samples, one iteration).
Inventory regeneration changed only the analyzer source hash; complete original
bodies, public obligations, scope and historical expectations remain unchanged.
