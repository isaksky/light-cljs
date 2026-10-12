# Deferred record initializer generations

The unchanged independent source is:

```clojure
(defprotocol P (p [x]))
(defrecord R [x] P (p [_] x))
(def old (->R 3))
(def factory ->R)
(defrecord R [x] P (p [_] (+ 10 x)))
(def current (->R 7))
[(p old) (p current) (p (factory 5)) (p (assoc old :x 4))]
```

Both required references, release **1.12.134** and checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, return `[3 17 15 14]` with
`optimizations:none/static-fns:false/def-emits-var:false`. The repeated-method
control adds `(p [_] 0)` before each actual implementation and has the same
result. Neither source nor its expected result was reduced or replaced.

At starting HEAD `cd9c2c4`, deferred old initialization used the current record
constructor and the faithful top-level-setup macro returned `[13 17 15 14]`.
Ordinary execution and complete sequential macro execution already passed.
The raw failure, both source forms, both pin builds and typed observations remain
in `/tmp/light-cljs-165-generation-gap-v2/evidence.json`; the namespace adapter
failure remains in `/tmp/light-cljs-165-verified-v3/audit`. The former receipt's
excluded split frames remain historical zero-credit observations.

## Implementation and execution

Deferred analysis already retains the definition-site namespace, resolution
program, macro definitions, and extension frontier. Emitted initializer execution
was missing the corresponding declaration-product view. The runtime now records
its bundle frontier when registering an ordinary pending root. During demand it
walks replacement bundle predecessors to select source-visible products, then
restores the caller's view in `finally`. This does not copy roots, replay effects,
or capture the view in escaped functions. The saved positional/map factories
and generated record assoc methods still read the active constructor Var when
invoked later. Ordinary roots and conditional definition groups stay live;
overlays still take precedence. Existing declaration transactions publish or
roll back products atomically.

The current receipt is
[sci-record-generation-receipt.json](sci-record-generation-receipt.json), SHA-256
`76ed6f01c54314fd564db907ddb74ca6999ceb490108cd84513b994fe4abcd54`, with raw
artifacts at `/tmp/light-cljs-166-verified-v2`. The producer validates complete
source/setup/body equality, literal typed observations, compiler/producer/binary
hashes, both pins and their settings, and unchanged producer inputs during the
run. It includes **42 compilations, 84 QuickJS/Node observations and 14 reference
observations** across seven complete sources. Each uses fresh/reused MacroHosts
and ordinary application, actual top-level-setup computed macro, and complete
sequential computed macro execution. None uses a precomputed expansion.

The added identity/effect source checks old/current dispatch, repeated reads,
saved constructor and factory identities, instance membership, positional/map
factories, assoc copies, old field visibility, and ordered once-only initializer
effects. Every frame also runs malformed-method failure and clean compilation
on the same host. A separate actual macro assertion checks nested demand into a
newer initializer, restoration to the older caller, sticky identical exceptions,
`defonce`, once-only effects, later repaired generation and fresh reused-host
state. Its intentionally throwing application initializer is not executed and
is not counted as an application pass.

Verifier negative controls rejected changing old dispatch back to13 and removing
a split-generation frame. The initial new-control run at
`/tmp/light-cljs-166-verified-v1` failed because its keyword observation expectation
used a shorthand string instead of the observer's namespace/name/spelling object;
that raw run is retained. Only the new expectation's encoding was corrected;
all original values and strict checks remain unchanged.

## Verification and performance

- Focused macro/bootstrap/deferred/protocol/type/record/reify/extension/core
  extension/multimethod suites: **164 passed**, six filtered by `--skip cancel
  --skip limit` (the latter excludes singular and plural names).
- Exact atomic product publication/cycle test: **1 passed**. An earlier mistaken
  exact-name invocation ran zero tests and supplies no coverage.
- Canonical Node runtime assertions and whitespace checks passed.
- Validated compiler performance smoke: **423 samples across 47 phase pairs**,
  nine samples/eight iterations, `/tmp/light-cljs-166-compiler-bench.csv`.
  Collection total median 0.04425ms; fresh/reused macro analysis medians
  24.729/24.786ms. These are local smoke measurements, not paired improvement
  or whole-program performance claims.

No cancellation or limit suite was run, and externally blocked156 was not
investigated. This focused result grants no comprehensive125 language-gate pass,
changes no original corpus/disposition row, and removes no original obligation.

```sh
python3 -B tests/reference/compiler-language/sci_record_duplicate_method.py \
  --output /tmp/light-cljs-record-generation-fresh
cargo test --locked --test compiler_macros --test compiler_records \
  --test compiler_types --test compiler_reify --test compiler_protocols \
  --test compiler_extensions --test compiler_core_extension_demand \
  --test compiler_bootstrap --test compiler_deferred --test compiler_multimethods \
  -- --skip cancel --skip limit
cargo test --locked --lib \
  compiler_bootstrap::tests::declaration_products_are_atomic_and_unfinished_value_reads_cycle \
  -- --exact
cargo bench --locked --bench compiler -- --samples 9 --iterations 8
```
