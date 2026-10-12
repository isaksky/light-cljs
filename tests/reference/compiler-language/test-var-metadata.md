# Executable Var test metadata

The unchanged `metadata.projection` source from the historical
`test-metadata-var-reflection-gap-receipt.json` now returns `true` in ordinary
applications and actual demanded macro computations, in QuickJS and Node.
`test-metadata-var-reflection-reference.json` records fresh executions against
ClojureScript 1.12.134 and checkout a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e,
with optimizations `none`, static functions disabled and def-emits-var disabled.
`test-metadata-var-reflection-accepted.json` binds the current source, emitted
JavaScript, executable and raw observations. Raw artifacts are retained under
`/tmp/light-cljs-190-reference` and `/tmp/light-cljs-190-actual`.

Executable metadata has a separate definition-site compilation unit and cached
root state. Reflecting a Var can obtain its actual callback without executing
the ordinary initializer. The initializer later attaches that same callback.
Applications evaluate the same analyzed metadata expression in their own heap.
Var construction only captures the metadata provider; `meta` materializes and
memoizes the public metadata map. A ready root's callback is captured when its
Var is constructed, preserving ordinary callback-property snapshot behavior.

`tests/compiler_var_test_metadata.rs` covers fresh/reused hosts, both phases and
engines, callback invocation and identity before/after initialization, metadata
provenance, overrides, source generations, nested definitions, overlays, sticky
metadata failure and clean-host recovery. Its effect counters independently
show that unrelated pending roots and standalone application effects are not
replayed during macros. The original `tests/compiler_test_metadata.rs` and all
historical failure and acceptance receipts remain unchanged.

These are scoped reflection results. They grant no new operation or source-site
credit and do not complete the full-language gate. The inventory change only
refreshes the analyzer source hash. The externally blocked native cancellation
issue and cancellation/resource-limit suites remain excluded.

Focused reproduction and normal performance checks:

```sh
cargo test --test compiler_var_test_metadata --test compiler_test_metadata
cargo clippy --all-targets -- -D warnings
cargo bench --locked --bench compiler -- --samples 9 --iterations 2
PYTHONDONTWRITEBYTECODE=1 python3 benchmarks/measure_compiler_language.py --output /tmp/var-metadata-performance
```

The surrounding checks also execute the core/source Var metadata, deferred and
reflection-completion test targets plus ordinary-helper-demand, same-file-macro
and metadata selections from `compiler_macros`. Declaration-only slots retain
`:test nil`; source executable metadata is attached only with an initializer,
matching the pinned compiler's `:def` emission under def-emits-var false.
