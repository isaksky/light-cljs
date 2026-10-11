# Ordinary compile-time helper value demand

Implemented and validated 2026-10-10 against starting commit `72ca8c5`.
The fixtures in `tests/compiler_macros.rs::ordinary_helper_demand` and
`compiler_bootstrap::tests::cancelled_demand_discards_roots_before_reusing_host`
are original, hand-authored local phase-contract witnesses, not copied upstream
test bodies. Their behavior follows the settled compiler migration contract.
No upstream compatibility result is inferred from these local executions.

The ten macro tests compile with native evaluation disabled and execute successful
applications in Node. Compiler-only failure-policy cases intentionally contain
application initializers that throw; their macro assertions execute in QuickJS,
and those throwing applications are not run. Coverage includes:

- Direct and macro-generated factor/twice/connection/doubled definitions yield
  `[42 42]`; compilation succeeds with `connect` unavailable, establishing zero
  compile-time calls. The application callback runs once. A separate initializer
  trace proves factor then connection source order.
- Fresh and reused hosts yield `[1 2 0]` for hits/next-id/id. A standalone swap!
  and set! affect application roots only; the macro observes initial zero.
- Closure captures, direct/apply/composition/map/reduce calls and functions stored
  in vectors/maps demand the same roots. Untaken unavailable-host branches do not
  demand them; a taken branch reports the initializer location. ECMAScript Math
  runs in selected compile-time code.
- Quote, Var identity/equality/hash/metadata and resolve avoid initializer effects; exists? retains its ordinary value check.
  Var dereference/invocation and resolve-then-consume use active roots.
- Self recursion (the existing deferred-unit witness), mutual recursion through
  declarations, located unavailable declaration reads and a→b→a initializer
  cycles. Analysis dependency cycles remain a separate diagnostic.
- Repeated failed reads retain the same error and one effect. Defonce retains the
  failed generation; ordinary redefinition repairs it. Pending/nil/false roots
  are preserved, declaration-only roots accept an initializer, and a pending
  undefined result remains preserved at compile time. Application defonce still
  replaces an undefined root.
- Saved function code/captures read active ordinary generations; declarations and
  redefinitions follow source order. Executed nested defs change only their phase.
- Parallel scope initializers, nested binding/with-redefs throws, pending/failed
  root restoration, initialization failure during scope entry, overlay set! and
  initialized-root set!. Standalone application mutation is never replayed.

The cancellation unit test calls a compiled source macro that demands a helper,
mutates its source atom and cancels from a test-only host service. Cancellation
is terminal in that heap. A subsequent clean compilation on the same MacroHost
allocates a distinct evaluator and observes a fresh atom. External host effects
are not claimed reversible.

The canonical runtime suite independently checks that deferred Var construction,
metadata/equality/hash perform zero getter calls, while deref/test consume the
root. Explicitly replaced Var test metadata remains authoritative. Normal
application Var behavior retains the existing assertions.

## Commands and outcomes

- `cargo test --locked --test compiler_macros --test compiler_warnings --test compiler_bootstrap --test compiler_deferred --test provenance --test evaluator --quiet`:
  macro **84 passed**, warnings **11 passed**, bootstrap **11 passed**, deferred
  **3 passed**, evaluator **9 passed / 1 ignored** (the opt-in startup measurement).
  Provenance **1 passed / 1 failed** with the pre-existing failure below; command
  exit status was nonzero and is not reported as a clean suite pass.
- Final focused rerun after additional generated/scope witnesses:
  `cargo test --locked --test compiler_macros ordinary_helper_demand --quiet`:
  **10 passed**. After adding standalone set! to the existing mutation witness,
  `cargo test --locked --test compiler_macros ordinary_helper_demand::roots_are_fresh_on_reuse_and_standalone_mutation_is_never_replayed --quiet`
  passed **1 test**.
- `cargo test --locked --test compiler_macros reentrant_deferred_units --quiet`:
  **6 passed** after the registry/code-cache change.
- `cargo test --locked --lib program_fragment --quiet`: **3 passed**, checking
  semantic arena remapping after adding the resolve identity IR.
- `cargo test --locked --lib cancelled_demand_discards_roots_before_reusing_host --quiet`:
  **1 passed**.
- `cargo test --locked --test execution compiled_definitions_preserve_namespace_macro_and_callback_scopes --quiet`:
  **1 passed** (namespace roots/macros, nested scopes and asynchronous callback
  restoration).
- `node tests/support/js_runtime.test.js`: all canonical runtime assertions
  passed, including the additional lazy Var getter checks.
- `cargo check --locked --all-features --quiet`: passed.
- `git diff --check`: passed.

The provenance validator rejects the `numeric-number-keys` row in
`fixture-origins.tsv`: it says hand-authored but includes an upstream repository,
commit, path and anchor. The identical invalid row and validator exist at starting
HEAD `72ca8c5`. Focused follow-up `light-cljs-127` records reconciliation; no fixture
claim or validator was weakened here. The pre-existing vswap! mismatch is already
recorded by `light-cljs-126`; its broad execution corpus was not repeated.

No full cargo suite, pinned JVM oracle, platform-specific suite, startup benchmark
or unchanged application port was run. Protocol/type/record/extension bundles
remain required work in `light-cljs-110`; ordinary namespace import migration is
`light-cljs-103`, and final effect-preserving reachability is `light-cljs-111`.

The subsequent declaration-bundle delivery is documented in
[compiler-declaration-demand.md](compiler-declaration-demand.md); the historical
command outcomes and prerequisite scope above are unchanged.
