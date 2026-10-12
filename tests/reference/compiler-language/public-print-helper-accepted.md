# Public printing helper acceptance

The canonical runtime exposes `pr-str*`, `StringBufferWriter`, and the generated `->StringBufferWriter` factory. `pr-str*` dispatches `IPrintWithWriter/-pr-writer` directly with nil options and flushes its writer; it preserves collection print bindings and bypasses the generic `pr-str` native/nil printer and metadata prefix. `StringBufferWriter` dispatches `IWriter`, calls the supplied JavaScript buffer's `append` with that buffer as receiver, returns the callback result, and flushes to nil.

The original two complete callers and expectations in `public-print-helper-gap-contracts.json` are unchanged. The initial and final negative receipts, diagnostic producer and original exploratory nil source remain historical evidence with zero credit. New independently sourced contracts cover direct/qualified/saved/apply printing and constructors, protocol/type identity, callback receiver/order/results, printing length/level/readability/metadata/namespace bindings, custom printing with nil options, exception/binding restoration, and same-host recovery. Release1.12.134 and clean checkouta5e94c988fd0b50ffcff1ce6add12ffbb7839a3e execute each complete source under none/static-fnsfalse/def-emits-varfalse.

Nine complete sources agree in both pins and all eight QuickJS/Node application/macro fresh/reused frames:72 accepted engine observations. The tenth is a preserved nil failure/recovery control: release returns type-error, checkout returns error, and production matches checkout. Its eight local observations grant zero two-pin acceptance credit. Dedicated regressions also verify phase heap isolation, clean recompilation using a reused host, unsupported writer macro-result diagnostics, and recovery after rejection.

`public_print_helpers.py` builds the actual Rust audit, verifies full source/emitted/raw/command/environment/executable hashes and exact typed packets, then reruns that executable from the original source. Only the temporary source-root prefix in source metadata may differ. Artifact mutations remain rejected after recensus. `public-print-helper-accepted-receipt.json` records the fresh normal125 report acceptance. The normal report runs this proof when selecting the linked public callers; three source/arity/phase witnesses bind the exact operation contracts. Missing reference or phase evidence leaves them missing. The broader125 strict gate remains incomplete.

Reproduce focused acceptance:

```sh
python3 -B tests/reference/compiler-language/report.py --run \
  --fixture public-pr-str-star --fixture public-string-buffer-writer \
  --fixture public-print-writer-factories --artifacts /tmp/fresh-public-print-helpers
PUBLIC_PRINT_RECEIPT=/tmp/fresh-public-print-helpers/public-print-helpers/evidence.json \
  python3 -B tests/reference/compiler-language/test_public_print_helpers.py
cargo test --locked --test compiler_public_print_helpers --test compiler_printing_completion
```

Cancellation/resource-limit suites and the externally blocked156 work are excluded. Ordinary printing length/level contracts execute unchanged.

Focused printing/public-constructor tests and normal analyzer, CLI, core-gate, interop, inventory/accounting, optimization and protocol suites passed. The application compiler benchmark validated30groups with nine samples and two iterations each; raw270rows are `/tmp/light-cljs-186-compiler-bench.csv`. No comparative performance claim is made. Strict all-target Clippy fails with the identical56library diagnostics on unchanged HEAD3395a55 and this change, confirmed by independent archive builds; focused duplicate-checked owner187 precedes125.
