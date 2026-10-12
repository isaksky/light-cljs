# Unary record map factory metadata

Generated `map->R` metadata now describes one map parameter for every record,
including zero-field records. Positional factories retain their declared field
vectors. The analyzer changes only `:arglists` construction; emitted invocation,
metadata providers, deferred roots, and provenance use the existing paths.
The map parameter is spelled `m` locally. Upstream creates a gensym, so generated
parameter spelling is not used as an equivalence claim.

The complete immutable `sci-record-constructor-metadata` original is retained in
both original TSV files. The dedicated test computes that entire source without
changing its forms, then captures its full typed result, effects and stdout in
fresh/reused applications and actual demanded MacroHost computations, observed
by QuickJS and Node. All eight observations return positional `([x y])`, unary
map arglist counts `[1]`, and namespace `"user"`. The explicit namespace module
avoids the earlier synthetic `cljs.user` wrapper spelling. The original top-level
source adapter independently preserves its original form order and namespace.

The historical SCI expectation retains a sequence container and a `user` Symbol.
Canonical ClojureScript pins return a list and namespace string; historical
expectations and dispositions remain unchanged, with no SCI semantic pass credit.
The fresh proof executes the unchanged original separately in ClojureScript
1.12.134 and checkout a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e.

The same complete controls cover zero/one/three-field records, exact positional
arglists, unary map arglists containing a Symbol, names/docs/namespaces/source
sites, Var/root identity, nil fields, extra map entries, declarations,
redefinition, and saved factories/Var/metadata. They run in both pins as ordinary
namespace programs and in all eight local contexts. A separate actual macro
failure confirms metadata reflection does not demand an unrelated deferred root,
and that the same fresh/reused host recovers to construct another record and
reflect its correct metadata. Existing executable callback metadata and deferred
root suites provide independent interaction coverage.

Initial controls used the name `Empty`, which collides with the pins' private
`cljs.core.Empty` type. Both the top-level adapter and ordinary namespace build
emitted that constructor under `cljs.core.Empty` while its methods referenced
`user.Empty`, then threw. These failed candidates remain under
`/tmp/light-cljs-195-proof-v1` and `/tmp/light-cljs-195-proof-v2` and provide no
pass evidence. The focused controls use unique `MetadataZeroRecord`,
`MetadataOneRecord`, and `MetadataManyRecord` names; all metadata, declaration,
redefinition and invocation checks remain. No immutable original was renamed.
The pins' namespace control profile contains the full control source and matches
the application module used by the actual-phase driver.

```sh
python3 -B tests/reference/compiler-language/record_map_constructor_metadata.py --output /tmp/light-cljs-195-fresh-proof
cargo test --locked --test compiler_record_map_constructor_metadata --test compiler_source_metadata --test compiler_var_test_metadata --test compiler_test_metadata --test compiler_types --test compiler_positional_constructors
cargo clippy --locked --all-targets -- -D warnings
cargo bench --locked --bench compiler -- --samples 9 --iterations 2
```

Normal compiler performance uses its validated Node observations and resident
phase checks, nine samples with two iterations each. This is smoke validation;
no performance speedup or statistically significant change is claimed.
Cancellation/resource-limit suites and externally blocked light-cljs-156 are
outside this verification. Existing repository-wide formatting drift is unchanged.
