# Canonical map value equality operand direction

Pellet 185 preserves this complete original source byte for byte:

```clojure
(let [events (atom []) m (apply hash-map (mapcat (fn [x] [x (reify IEquiv (-equiv [_ other] (swap! events conj x) (= x other)))]) [7 3 6 1 5 0 4 2 8])) n (zipmap (range 9) (range 9)) same (= m n)] [same @events])
```

Its SHA-256 remains
`e0173f7c4ecffce1ff4f8e59162e2d71be96fb7584c23facd7a7e8a47b6fefc8`.
Both independently compiled pins, release `1.12.134` and clean checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, return `[false []]`, trace `[]`
and stdout `""`. Before the fix, the new permanent local regression failed with
`[true [0 1 2 3 4 5 6 7 8]]`. No reduced source or normalized sequence supplies
credit. Historical 181 sources, expectations, failed packets and receipts remain
unchanged; current passing evidence belongs to this separate proof.

Canonical hash and sorted maps dispatch value equality on the looked-up right
value against the retained left value. Array maps have a distinct left-first
`IEquiv` implementation. The default runtime now respects this representation
boundary, including Object `.equiv` and public `-equiv`, while the public
`equiv-map` helper retains its right-first contract. Ordinary `=` preserves its
identity shortcut, operand evaluation order and adjacent variadic comparison.
No collection representation, traversal, key routing, record equality, numeric
semantics or sequence equality is changed.

The [32-source catalog](map-value-equiv-contracts.json) independently derives
both pins' complete typed expectations before local comparison. Its first
source and expectation retain the exact original 181 witness. Additional whole
sources cover direct, qualified, saved and applied `=` and `equiv-map`; Object
and protocol calls; hash, array, literal, nested and sorted maps; both asymmetric
array value directions; missing keys/counts and nonmaps; identity and one/three
arguments; short circuiting; ordered callback and argument effects; Number and
sequential values; and caught custom equality errors. Each observation preserves
value, count, lookup, sequence positions, effects, collection tags and stdout.
Independent roots are
`/private/tmp/light-cljs-185-independent-map-value-v1` and
`/private/tmp/light-cljs-185-independent-map-value-v2`. Reference settings remain
`:optimizations :none`, `:static-fns false`, `:def-emits-var false`.

The [producer](map_value_equiv.py) and
[Rust driver](../../compiler_map_value_equiv.rs) produce 64 reference packets
and 256 actual engine frames across fresh/reused MacroHosts and actual
application/demanded-macro phases in QuickJS and Node. They bind original source,
compiler/producer hashes, checked pins/settings, executable bytes, environment,
commands, emitted modules, raw typed packets and the complete artifact census.
Final raw proof is
`/private/tmp/light-cljs-185-map-value-delivery-final-v3/evidence.json`, SHA-256
`06b1f58ca40ec2ead6eace1fa531cbdc01914959d0a7f0394d5a29461ccd64e4`.
The [durable receipt](map-value-equiv-receipt.json) is byte-identical.
Earlier development artifacts grant no acceptance credit.

The [guards](test_map_value_equiv.py) reject changed canonical contracts,
boolean values, callback/key/value order, trace and stdout. Raw replayers reject
changed original source, emitted namespace, QuickJS output, Node input/output,
commands, environment, executable bindings, detached summary and missing
artifacts, including tampering with refreshed artifact hashes. Every mutation
is restored and pristine replay passes. Ordinary permanent regression executes
all 32 complete sources in both engines/phases, then exercises uncaught equality
errors and seven malformed source controls, each followed by recovery on the
same MacroHost and both engines. JSON object property canonicalization preserves
every typed collection, position, effect and output.

Verification:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/map_value_equiv.py --output /private/tmp/light-cljs-185-map-value-delivery-final-v3
PYTHONDONTWRITEBYTECODE=1 MAP_VALUE_EQUIV_RECEIPT=/private/tmp/light-cljs-185-map-value-delivery-final-v3/evidence.json python3 tests/reference/compiler-language/test_map_value_equiv.py
cargo test --locked --test compiler_map_value_equiv --test compiler_object_equiv --test compiler_atom_object_equiv --test compiler_sequence_object_equiv
cargo test --locked --test compiler_runtime_map_order --test compiler_hash_branch_effects --test compiler_equality_contracts --test compiler_transients --test compiler_public_constructors --test compiler_positional_constructors --test compiler_iterator_helpers --test compiler_sorted_contracts --test compiler_sorted_collections --test compiler_hash_map_source --test compiler_hash_map_from_arrays
cargo test --locked --test compiler_reader --test compiler_sequences
cargo test --locked --test compiler_macros generated_maps_preserve_owned_children_order_keys_and_quoted_data -- --exact
cargo test --locked --test compiler_macros generated_map_keys_keep_reader_name_parts_before_runtime_spelling_equality -- --exact
cargo test --locked --test compiler_macros quoted_macro_values_preserve_symbols_effects_and_generated_provenance -- --exact
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/extract_inventory.py --check
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/test_inventory.py
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/test_full_ledger.py
python3 benchmarks/run_compiler.py --output /private/tmp/light-cljs-185-benchmarks-final-v2 --samples 9 --iterations 2
```

The validated compiler benchmark bundle retains timing, allocation,
fresh-process and runtime-operation observations. Concurrent verification and
uncontrolled host scheduling prevent a controlled performance comparison.
Five surrounding dedicated reference/audit tests remain ignored; the current
map equality audit is explicitly executed. Original upstream/SCI bodies,
historical expectations and strict full-language criteria remain unchanged.
This proof grants zero original upstream-body/public-arity and full-language
completion credit; strict completion remains false. No distinct out-of-scope
omission was found. Cancellation and singular/plural limit tests are excluded
by inspected names and exact macro filters. Externally blocked 156 remains
untouched. No push or PR.
