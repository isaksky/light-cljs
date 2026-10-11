# Compiled atoms and volatiles

Contract: ClojureScript checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`,
compared with Maven **1.12.134** and checkout runtime **0.0.135109660**.
The reference runner verifies `:optimizations :none`, `:static-fns false`,
`:def-emits-var false`, source origin and revision. Runs used Clojure
**1.12.4**, Java **25.0.2** and Node **v24.18.0**. The selected release and
checkout observations agree.

## Contract and source evidence

The six complete bodies in `src/test/cljs/cljs/core_test.cljs` were read and
adapted: `test-atoms-and-volatile`, `swap-vals-returns-old-value`,
`deref-swap-arities`, `deref-reset-returns-old-value`,
`reset-on-deref-reset-equality` and `test-cljs-3406`. All **56 original assertion
sites** pass. Whole-body witnesses preserve shared bindings, mutations, all
fixed/variadic swap calls, expected old/new pairs, typed failures and the full
custom reify implementation. Only test wrappers become `do`/throwing `assert`,
and `thrown?` becomes typed `try`/`catch`. Decimal `45M` remains in the fixture.

`compiler-language/witnesses.jsonl` records each assertion ID and source hash;
`fixture-origins.tsv` records the commit, path, anchors, local IDs and adaptation
rationale. The original complete bodies and exact assertion spans remain in the
pinned inventory. This completes these six state tests, not other mutable-state
features such as dynamic bindings or the entire upstream suite.

Runtime semantics were read from `src/main/cljs/cljs/core.cljs`: `IDeref`,
`IAtom`, `IReset`, `ISwap`, `IVolatile`, `IWatchable`, `Atom`, `Volatile`,
`deref`, the atom/volatile functions, metadata mutators and printing extensions.
`src/main/clojure/cljs/core.cljc/vswap!` supplies macro evaluation order.
The supporting `update` function executes the original custom state test's
collection transformation, rather than replacing it with a different operation.

### Compiled bootstrap target-order repair (2026-10-10)

The first-party compiled `macro-vswap!` now emits the pinned protocol calls
`(-vreset! vol (f (-deref vol) args...))`. Both checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` and the Maven 1.12.134 macro source
repeat `vol`: the reset target is evaluated first, then `f`, then the dereference
target, then the arguments. Saving `vol` in a generated `let*` had changed this
order and selected the wrong old value when the target expression changed.

The native-disabled `compiler_bootstrap.rs` regression uses successive targets
with initial values 1 and 10 and an effectful function expression. It asserts
`[17 17 10 2 1 [:target :fn :target 3 4]]`: the first target receives the update,
the second supplies the old value, the target runs twice and the function runs
once. Before the repair it produced `[8 8 10 1 1 [:target :fn 3 4]]`.
The retained `state-volatile-order` ledger remains unchanged.

Focused verification with `LIGHT_CLJS_DISABLE_NATIVE_EVAL=1`:

- `cargo test --locked --test compiler_bootstrap --quiet`: **12 passed**.
- `cargo test --locked --lib compiler_bootstrap::tests --quiet`: **4 passed**.
- `cargo test --locked --test execution tiny_programs_execute_with_expected_values_and_effects --quiet`:
  **1 passed**, covering all **1,698** retained outcomes, traces and stdout.
- With `LIGHT_CLJS_REFERENCE=both`, the explicit checkout path/revision and
  `--ignored --exact --nocapture`,
  `compiled_state_matches_pinned_values_effects_and_stdout`: **41 state cases
  matched each pinned reference**, including target order and custom protocols.
- `rustfmt --edition 2024 --check tests/compiler_bootstrap.rs` and
  `git diff --check`: passed. The repository-wide `cargo fmt --all -- --check`
  still reports pre-existing differences outside the changed files.

The original task called compare-and-set identity-based. **Both pinned CLJS
runtimes instead use structural `=`**, explicitly in `compare-and-set!`.
`state-compare-structural` proves equal but distinct vectors compare-and-set
successfully, while `identical?` is false. NaN, signed zero and nil/undefined
comparisons are separately checked. This implementation follows the pinned CLJS
contract; it does not import JVM atom identity semantics.

## Independent probes

**41 independent outcome fixtures** compare literal values, effect order,
failure categories and stdout with both references. They cover:

- Creation options (including a map), no initial validation, validator installation
  and removal, CLJS truthiness, false/noncallable validators, thrown identity,
  reentry and failed installation after a validator changes state.
- All swap/reset result forms, callbacks that mutate then return or throw, and
  the different old-value capture points for atoms and custom dereference.
- Watch addition/removal/replacement, equal but distinct keys, snapshot iteration,
  nested notification, unchanged-state resets, thrown objects and nil after state
  commits, and raw versus public watch method return values.
- Custom `IDeref`/`IReset`/`ISwap`/`IVolatile`/`IWatchable` implementations,
  marker/partial protocol declarations, protocol predicate identity, variadic
  method tails, and saved core calls through `apply`.
- Volatile macro target duplication and callback evaluation order, metadata
  independence/failures, constructors, printing, undefined state, closures and
  global definitions. Invalid state operations retain native Error categories.

The shared two-namespace fixture checks closure-captured reference identity,
watch output, atom/volatile globals, virtual core aliases and custom aliased
`IDeref`/`IReset` implementations. Literal stdout is compared with both
references. Generated programs use only their embedded JavaScript runtime.

Independent Node assertions bypass the shared encoder and compiler. They check
callback receiver/reference identity, validator/watch thrown identity, state
commit timing, metadata and identity hashing, private brands, protocol dispatch,
exact variadic tail identity and a 150,000-element apply tail. Fixed swap callback
invocation uses a null receiver; variadic swap follows CLJS `apply` receiver
semantics. Atom/volatile output is branded opaque to the outcome adapter.

## Reproduction

```sh
cargo test --locked --tests --quiet
node tests/support/js_runtime.test.js
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution compiled_state -- --ignored --nocapture

python3 - <<'PY'
import json, subprocess
command = ['python3', '-B', 'tests/reference/compiler-language/report.py', '--scope', 'historical-selected',
           '--run', '--strict', '--reference', 'both']
for row in map(json.loads, open('tests/reference/compiler-language/witnesses.jsonl')):
    if row['fixture_id'].startswith('cljs-state-'):
        command += ['--id', row['assertion_id']]
raise SystemExit(subprocess.call(command))
PY
```

The full ordinary Rust suite passed **633** tests, with **27** explicitly ignored
reference tests. After the final callback/identity refinements, the focused
execution, compiler macro, reader, namespace, provenance and inventory checks
passed **117** tests, with **23** explicitly ignored references. The two focused
state differential tests then passed both references, and the strict imported
assertion run passed all 56 selected sites. Node runtime assertions, all-target
Clippy (plus a final library Clippy check), formatting, source inventory and
whitespace checks passed.

## Output size and startup

[Raw samples](../../benchmarks/results/compiler-2026-10-09/state/output-startup.json)
retain nine fresh Node parsing/initialization samples per program. Primitive
`42` output remains **104 bytes**. The same metadata program grows from the
previous recorded milestone's **71,651 bytes** to **78,450 bytes** (6,799 bytes,
about 9.5%). Median parse/initialization for that program was **1.668 ms**,
compared with the previous recorded **1.429 ms**. A program that constructs an
atom, registers a watch and swaps it is **78,578 bytes**, with a **2.138 ms**
median. These are observations from separate runs, not a controlled latency
regression threshold. The runtime is embedded once per compilation unit and
primitive-only programs still omit it.
