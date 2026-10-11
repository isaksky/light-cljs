# Mapped core Var metadata reconciliation

The source oracle is clean ClojureScript checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`; the independent release oracle is
Maven **1.12.134**. The canonical generator now reads every `CoreFnId::resolve`
spelling, including combined alias arms and multiline arms. Its full output has
**636 declarations**, adding **338** to the prior table and preserving all prior
names. The data remains EPL-1.0 with its source revision in the table header.

The generator checks the exact **32** mapped spellings without a pinned runtime
`cljs.core` analyzer declaration against `core-var-metadata-boundaries.tsv`.
These retain their existing compiler intrinsic, macro, library or extension paths;
the catalog records the live residual public-contract owner, light-cljs-125.
A new unknown spelling fails regeneration instead of silently disappearing.

The reflection generator now compares an independent source-field projection
without rewriting canonical rows. Pinned analyzer metadata contains additional
fields and supplies constructor Var arglists `()`, including `Namespace`;
constructor invocation signatures remain independently inventoried. `str` and
`Var` retain their prior useful metadata. `str_`, `->Var` and `clone` retain their
own source names, docs, locations and arities rather than borrowing an operation
alias's metadata. Private mapped declarations keep the pinned `:private` bit.
The runtime reader core-name catalog is regenerated from both source tables.

## Executing evidence and provenance

`compiler-core-var-metadata.cljs` is an independently authored observation of
pinned public declarations, not an adapted upstream test body. The complete Rust
test executes its exact assertions in a compiler-executed QuickJS macro, a
QuickJS application and Node. It checks dynamic history/assert/error-sink Vars,
source docs/locations/namespaces, empty and nonempty sequential arglists and
parameter vectors, alias identities, bound root values and callable saved Vars.
Clone dispatch executes on a persistent vector. Its namespace-name observation
accepts the pinned symbol and the local Namespace handle explicitly.

The same application contract passes the checkout oracle unchanged. The release
oracle retains all expressions/assertions, with explicitly recorded source line
changes and the release's nil `*assert*` docstring. Those version differences do
not replace the local checkout expectations. Reference builds verify origin,
identity, `:optimizations :none`, `:static-fns false`, and `:def-emits-var false`.
Only local ordinary macro setup supplies the separate expansion-phase evidence.

The independent ledger retains 97 full source files, 1,328 bodies and 4,728
assertion sites. Regeneration adds metadata obligations: **5,219** independent
obligations, with **1,327** public API/arity rows unchanged. Sixteen executing
mappings (11 local metadata observations and five dynamic public declarations)
pin the complete test and support source hashes. Existing reflection, printing
and utility execution mappings retain their actual tests and reconcile changed
row hashes; two new utility metadata rows reuse the executing utility contract.
No generated declaration establishes a semantic pass. The static strict report
still correctly exits 1 with **8,570 missing** and **1,377 unrelated** rows, zero
execution passes, and all 27 supplemental contracts not run. Full language
completion remains light-cljs-125.

## Reproduction and verification

```sh
clojure -Srepro -Sdeps '{:deps {org.clojure/clojurescript {:local/root "research/clojurescript"}}}' \
  -M tests/reference/generate_core_var_metadata.clj --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B tests/reference/compiler-core-var-metadata.py
python3 -B tests/reference/compiler-reflection-completion.py
python3 -B tests/reference/compiler-utilities-completion.py
python3 -B tests/reference/compiler-printing-completion.py
python3 -B tests/reference/compiler-array-settings.py
python3 -B tests/reference/compiler-core-var-metadata.py --reference both \
  --artifacts /tmp/light-cljs-core-metadata-references
cargo test --locked --test compiler_core_var_metadata --test compiler_reader \
  --test compiler_reflection_completion --test compiler_utilities_completion \
  --test provenance --test compiler_language --test compiler_host_hooks \
  --test compiler_printing_completion
node tests/support/js_runtime.test.js
rustfmt --check tests/compiler_core_var_metadata.rs tests/compiler_reflection_completion.rs
python3 -B benchmarks/measure_compiler_language.py --output /tmp/light-cljs-core-metadata-performance
```

The pinned generator, inventory and all six provenance/mapping scripts pass.
The focused suites above pass **46** Rust tests; direct Node runtime assertions
and both reference observations pass. Broad reflected programs are sent to Node
on stdin because complete recovered metadata exceeds OS argv limits. A broader
array-settings run passes six tests and exposes the pre-existing exact `&env`
key-set failure from the reader-context extension at starting HEAD `6f50c2d`;
focused live owner light-cljs-137 retains its reproduction and acceptance.
`cargo clippy --locked --test compiler_core_var_metadata -- -D warnings` also
reports 51 pre-existing diagnostics in untouched compiler sources; this scoped
metadata change does not claim the final regression/lint gate passed.

Nine fresh Node processes checked the integrated benchmark's complete
value/effects/stdout. The emitted program is **313,001 bytes**, deterministic gzip
**75,116 bytes**, with local median parse **4.135 ms**, initialization **3.188 ms**,
execution **7.846 ms**, total **15.317 ms**. These are local observations with
uncontrolled scheduling, not a timing comparison. Primitive `42` remains
**88 bytes** with zero runtime preludes; metadata is emitted on demand rather
than adding the full table to every program.
