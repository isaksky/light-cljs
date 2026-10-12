# Canonical method receiver binding

Type and reify protocol adapters bind the receiver name to JavaScript `this`
inside the method body. That binding shadows a later formal parameter with the
same name. The analyzer now resolves that name to the existing receiver local,
while retaining every formal argument slot, source name, and invocation arity.
Ordinary functions and repeated nonreceiver parameters still use the last
binding. Field reads retain their separate implicit receiver.

The complete immutable `sci-record-method-duplicate-parameter` original remains
unchanged in both SCI TSV files:

```clojure
(defprotocol P (p [x n])) (defrecord R [x] P (p [n n] [x (if (record? n) :receiver n)])) (p (->R 3) 9)
```

The dedicated actual-phase regression first failed before the repair with the
complete typed result `[3 9]`. Both ClojureScript 1.12.134 and checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` independently returned
`[3 :receiver]`. Before changing production, separate canonical probes confirmed
the same receiver shadowing for deftype, reify, and receiver/field collisions,
and last-binding-wins behavior for ordinary functions and nonreceiver arguments.
Their initial raw diagnostic artifacts remain in
`/tmp/light-cljs-196-preflight`; the provisional nil expectations intentionally
reported mismatches while retaining every actual packet.

The fresh proof executes the entire original in its original top-level form
order and `user` namespace in both pins and locally. It also computes the entire
original in fresh/reused applications and actual demanded MacroHost computations,
then observes each result with QuickJS and Node. All eight complete typed packets
return `[3 :receiver]`, empty effects, and empty stdout, matching the historical
SCI expectation exactly. Historical expectations and dispositions are unchanged;
this focused proof does not claim broader ledger completion.

The complete controls run in both pins and all eight local contexts. They cover
receiver identity, hidden field access, distinct ordinary arguments, repeated
nonreceiver arguments, receiver/field name collisions, escaping closures,
saved/direct public method invocation with an unrelated explicit receiver,
declarations, positional/map factories, generation changes, and canonical last
method replacement. They also preserve deftype/reify receiver behavior and
ordinary function duplicate parameters. A separate fresh/reused macro test
demands declared record factories and methods, confirms an unrelated deferred
root remains untouched, intentionally throws, and verifies the same host recovers
to execute another colliding receiver method.

The proof independently reruns every retained Node input and every retained
QuickJS input in fresh engines, comparing complete raw packets. It independently
reruns both original pin inputs too. It binds original source/row hashes,
producer/runtime/observer hashes, both reference source trees and release JAR,
executable paths/hashes, full emitted sources, and the raw artifact census.
Reproduce into a new absent directory:

```sh
python3 -B tests/reference/compiler-language/record_receiver_binding.py --output /tmp/light-cljs-196-fresh-proof
cargo test --locked --test compiler_records --test compiler_types --test compiler_reify --test compiler_protocols --test compiler_sci_record_duplicate_method_probe --test compiler_record_map_constructor_metadata --test compiler_ifn_public_context --test compiler_ifn_rest --test compiler_core_extension_demand
cargo clippy --locked --all-targets -- -D warnings
cargo bench --locked --bench compiler -- --samples 9 --iterations 2
```

All 34 focused interaction tests pass. Normal compiler performance uses validated
Node observations, resident phase checks, and nine samples with two iterations.
This is performance smoke validation, with no speedup claim. Cancellation and
resource-limit suites and externally blocked light-cljs-156 were excluded.
Existing repository-wide formatting drift remains outside this task.
