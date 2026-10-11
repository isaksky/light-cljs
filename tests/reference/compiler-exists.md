# Safe demand-root existence, 2026-10-11

Pellet light-cljs-158 fixes `exists?` in compiler-owned macro execution.
The emitted check uses root availability before reading a value. Unavailable
roots return false; existing roots still initialize lazily, preserve sticky
errors and cycles, and distinguish undefined from nil/false. Conditional owners
retain their original source order, previous generations and selected products.
No exception-as-false fallback or eager initialization was introduced.

The complete original `cljs.ns-test/test-cljs-2292` preserves both original
false/true assertions and the namespace rename/exclude/alias/use context.
The original module hash remains
`21243db745474306fdfb034dd99b4ba5cee1a858493e4e8182fc2542968d1f1b`,
the complete body hash remains
`419c0dbcddcffa2538e2b17d086a14040f6fba4e97d1e7df910966b6e189dcc4`,
and the complete adapted source remains
`62f941cd8dcba802c664c068ee0d6c1901bb98f62485edd533e4f4e9569b02d3`.

Fresh execution passes on ClojureScript 1.12.134 and the clean checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, with checked
optimizations:none/static-fns:false/def-emits-var:false. Node and QuickJS
applications and actual macro computations pass with fresh and reused hosts.
[Retained receipts](compiler-exists-receipts.json) bind the original source,
literal expectations, public fixed arity, complete body and named phase test.
The source-context generator promotes only this previously pending body and
adds its two original sites and one public arity mapping; other retained bodies
only refresh their shared fixture/test hashes. Broad language completion remains
with light-cljs-125.

Focused tests cover unavailable/unknown/qualified globals, lexical shadows,
undefined/nil/false roots and overlays, conditional products, own/prior group
products, nested host paths and exactly-once ordered expression effects.
They also verify source visibility, initializer failures, genuine root/group
cycles, located ordinary unavailable diagnostics, failure/reuse and phase isolation.
The macro, namespace, interop, execution and evaluator-conformance suites pass
(186 tests, 45 explicitly ignored optional audits). The full namespace driver
adds one passing test across twelve contexts. Runtime JavaScript assertions,
inventory identity and focused witness validation pass.

[Validated performance evidence](../../benchmarks/results/compiler-2026-10-11/exists/output-startup.json)
retains nine fresh Node observations of the unchanged complete integrated
program: median parse 4.56 ms, initialization 3.76 ms, execution 10.20 ms and
total 18.95 ms; emitted size 346,370 bytes (82,486 gzip).
[Compiler samples](../../benchmarks/results/compiler-2026-10-11/exists/compiler.csv)
retain nine samples per phase, two iterations each, including fresh/reused
macro compilation. These are local observations, not a claimed speedup.

```sh
cargo test --locked --test compiler_macros exists_
cargo test --locked --test compiler_residual_source_context -- --nocapture
python3 -B tests/reference/compiler-residual-source-context.py --catalogs
python3 -B tests/reference/compiler-residual-source-context.py --case ns-exists --run --output /tmp/exists-reference
python3 -B tests/reference/compiler-residual-source-context.py --case ns-exists --audit-local --output /tmp/exists-local
node tests/support/js_runtime.test.js
python3 -B benchmarks/measure_compiler_language.py --output /tmp/exists-performance
cargo bench --locked --bench compiler -- --samples 9 --iterations 2
```
