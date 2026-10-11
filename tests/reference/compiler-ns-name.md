# Public namespace name result types

`light-cljs-143` aligns `ns-name` with the complete pinned body `(.-name ns-obj)`.
Both Maven ClojureScript **1.12.134** and clean checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e** execute the exact unchanged
[contract](../fixtures/compiler-ns-name.cljs). The checked reference runner retains
`:optimizations :none`, `:static-fns false`, and `:def-emits-var false`.
[Receipt](compiler-ns-name-receipt.json) retains original/fixture hashes,
complete Namespace/create-ns/find-ns/ns-name bodies and their required lookup
bodies from both pins, actual observations, and independent local witness hashes.

The public behavior has two distinct paths. `Namespace.` and `->Namespace`
retain the supplied name slot: a symbol remains the same symbol, and a string
remains the same string. Public create/find operations supplying symbols still
return symbols through `ns-name`. Compiler-emitted ordinary/core Var metadata
retains its original `:ns` symbol, whose `.name` field is a string. Consequently
`(ns-name (:ns (meta #'x)))` returns a string. The local runtime previously
returned that metadata symbol directly. Qualified symbol and explicit host
object name fields now also follow the pinned field access.

`.findInternedVar` retains its supplying Namespace in the returned Var's
metadata, as the complete pinned Namespace body requires. Its live getter,
qualified Var identity and lazy root access remain intact. This is necessary
for symbol-name and string-name Namespace reflection to keep their respective
result types. Registered session/reader namespaces, equality/hash, interning,
source metadata symbols and qualifications are preserved.

`ns_name_result_types_execute_in_both_engines_and_macro_phase` runs direct,
saved and apply reflection and all constructor/factory invocation forms in
QuickJS and Node applications, a QuickJS macro, fresh/reused runtimes and a
reused macro host. Macro-created namespaces remain absent from applications;
metadata inspection leaves an unavailable application initializer undemanded.
`metadata_names_preserve_qualification_and_session_isolation` retains literal
`qualified.scope` spelling across incremental evaluation and an independently
fresh session. The existing core metadata contract now checks both the symbol
metadata representation and the string reflection result explicitly, and it
passes both reference pins with its retained release-specific source locations.
The string library local adapter explicitly compares the reflected name with
`"clojure.string"`; it no longer bypasses `ns-name` by substituting raw metadata.

Historical SCI sources and observations remain independently preserved in the
original fixture and first seven disposition columns. The reserved Var metadata
and string namespace-publics SCI observations differ from these pinned string
results and remain nonpassing SCI comparisons, with actual evaluator outcomes
recorded separately. Other historical source-site/location/factory differences
retain their existing owners. No namespace spelling is normalized, and this
scoped change does not close the incomplete `light-cljs-125` language gate.

Executed verification:

```sh
python3 -B tests/reference/compiler-ns-name.py --output /tmp/light-cljs-143-ns-name-reference-verified
python3 -B tests/reference/compiler-core-var-metadata.py --reference both --artifacts /tmp/light-cljs-143-core-metadata-references
cargo test --locked --test compiler_ns_name --test compiler_reflection_completion --test compiler_source_metadata --test compiler_core_var_metadata --test compiler_string_fixtures --test compiler_reader --test compiler_host_hooks --test eval_sessions --test compiler_namespace_completion --test compiler_public_constructors --test compiler_public_protocols --test compiler_boundary_completion --test compiler_array_settings --no-fail-fast
python3 -B tests/reference/compiler-reflection-completion.py
python3 -B tests/reference/compiler-core-var-metadata.py
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B tests/reference/compiler-language/test_full_ledger.py
python3 -B tests/reference/compiler-language/test_report.py
node tests/support/js_runtime.test.js
cargo bench --locked --bench compiler -- --samples 9 --iterations 1
```

The affected Rust targets ran in focused batches: **94 passed**, with the two
existing external string oracle tests explicitly ignored. The changed namespace
and core metadata contracts were separately executed on both pins, as recorded
above. Direct runtime checks, source/catalog reconciliation, all 14 full-ledger
and 41 report/failure-injection tests passed. Regenerating the inventory after
reviewing the three changed SCI rows restores its complete source hashes.
No ignored oracle test is counted as a pass.

The nine-sample resident compiler smoke emitted 423 observations to
`/tmp/light-cljs-143-compiler-bench.csv`. Macro analysis medians were 23.547 ms
with fresh hosts and 23.777 ms with reused hosts. These smoke observations are
not a comparative performance claim.


The explicit fresh full SCI audit changed exactly three existing observations.
After reviewing those rows separately and preserving their original seven
columns, the final whole-corpus accounting gate passed:

```sh
cargo test --locked --test evaluator_conformance every_historical_sci_case_has_an_executable_evaluator_disposition -- --exact --nocapture
```

All **2,475** historical rows executed: **1,208 portable**, **304 adapted**,
**877 nonpassing gaps**, and **86 retirements**. The full log is
`/tmp/light-cljs-143-sci-accounting.log`. The gap/retirement counts remain
nonpassing evidence; this is an accounting pass, not full CLJS compatibility.
