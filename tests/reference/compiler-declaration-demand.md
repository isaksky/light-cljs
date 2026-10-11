# Compile-time declaration bundle demand

Implemented 2026-10-10 from starting commit `1a8dd4d`. The witnesses in
`tests/compiler_macros.rs::declaration_bundle_demand`, the five supplemental
bundle tests in that file, and the product-state/cancellation tests in `compiler_bootstrap`
are original local phase-contract assertions. They are hand-authored, with no
upstream test-body or compatibility claim. Their provenance rows are recorded in
`fixture-origins.tsv`.

The ordinary-root prerequisite was verified with
`cargo test --locked --test compiler_macros ordinary_helper_demand --quiet`:
**10 passed** before implementation.

## Executing coverage

All eleven declaration macro witnesses disable native evaluation and execute
macro code in QuickJS. Successful applications execute in Node; the two deliberate
throwing application initializers are compiler-only cases and are not executed.

- Record positional/map factories, type constructors and recursive methods,
  own/external protocols, count/get/hash/meta, record metadata and readable
  results. The same helper runs in both phases with separate mutation state.
- Saved protocol functions, apply/higher-order dispatch, zero-method markers,
  nil/string/array/default selectors, extend-protocol and later source replacement.
- Unavailable targets of unrelated protocols remain pending during compilation;
  the runtime host is explicitly supplied. A required computed target initializes
  once in each phase. Core count and user protocol queries demand candidates.
- Reify captures and executed specify! use ordinary expression lowering.
  Standalone specify! affects the application only and remains emitted.
- Deferred method macros use their definition-site generation and expand once;
  final emission reuses their analyzed IR. Descriptor/factory products support
  source redefinition, and Object member queries demand matching extensions.
- A genuine bundle/target cycle contains its dependency path. Failure after the
  first native installation rolls back both method and marker properties;
  repeated reads retain one failure/effect. A matching new extension generation
  repairs failure, and a fresh compilation on the same host has fresh state.
- An earlier delayed macro expansion excludes later extensions even after newer
  dispatch is ready. Frontier restoration also preserves an executed helper's
  mutation of the current dispatch table.
- An independent canonical-registry witness stages descriptor/factory products,
  throws before publication, checks shared sticky failure, rejects an ordinary
  read of an unfinished descriptor, and validates a fresh replacement bundle.
- Cancellation occurs after the string part of an extend-protocol bundle has
  installed, while its next computed target calls a test-only cancellation
  service. That evaluator rejects further execution; a fresh compilation on the
  reused MacroHost constructs an independent heap and successfully dispatches.

## Commands and outcomes

- `cargo test --locked --test compiler_macros --test compiler_protocols --test compiler_types --test compiler_records --test compiler_extensions --test compiler_reify --test provenance --quiet`:
  macro **94 passed**, protocols **5**, types **5**, records **6**, extensions **5**,
  reify **5**; provenance **1 passed / 1 failed**. The command exited nonzero due
  to the pre-existing provenance contradiction below.
- After the final frontier-mutation witness and restoration change,
  `cargo test --locked --test compiler_macros --quiet`: **95 passed**.
- `cargo test --locked --lib cancelled_extension_bundle --quiet`: **1 passed**.
- `cargo test --locked --lib declaration_products_are_atomic --quiet`: **1 passed**.
- `cargo test --locked --lib program_fragment --quiet`: **3 passed**.
- `cargo test --locked --test compiler_warnings --test compiler_deferred --quiet`:
  warnings **11 passed**, deferred **3 passed**.
- `node tests/support/js_runtime.test.js`: all canonical runtime assertions passed.
- `cargo check --locked --all-features --quiet`: passed.
- `git diff --check`: passed.

The provenance validator still rejects the pre-existing `numeric-number-keys`
row for claiming both hand-authored origin and an upstream source. This exact
row and rule exist at starting HEAD and are tracked by `light-cljs-127`. No
provenance claim or validator was weakened. The previously recorded `vswap!`
issue remains tracked by `light-cljs-126`.

No complete cargo suite, JVM oracle, platform-specific suite, benchmark or
unchanged application port was run. This delivery changes the source-level
bundle classification described in `docs/compiler.md`; namespace import and
final reachability/effect migration remain separate queue work.
