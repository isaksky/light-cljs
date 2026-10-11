# Public protocol and constructor oracle links

This reconciliation supplies exact pinned-reference fixture links for **131**
existing ordinary operation mappings: 18 public/local protocol rows, 64
constructor rows, 45 factory rows and four iterator-helper rows. It preserves
their existing complete declaration/body hashes, signatures, application/macro
test identities and support hashes. The links supply no static execution pass.

`compiler-public-gate-links.py` selects **29 complete existing literal fixtures**.
The selected protocol fixture executes all four identities and both direct
protocol-method arities through saved/apply calls, along with custom marker and
predicate dispatch. Each constructor/factory fixture supplies every positional
slot, invokes direct and saved/new construction and saved/apply factory calls,
and asserts nominal identity and ordinary results. The iterator contract retains
all lazy effects, INIT/START state, repeated `hasNext`, exhaustion, empty/reversed
bounds, multiple vector chunks, mutable host-array observations and metadata
checks. Original expectations are unchanged.

These are independent literal operation contracts. The retained complete upstream
body adapters still establish their original source assertions separately. A body
calling `reduced`, `symbol` or `cons` is not reclassified as evidence that it calls
`Reduced.`, `Symbol.` or `Cons.`. Likewise a passing constructor fixture cannot
cover unrelated source assertions, unknown signatures or unmapped public names.

## Reproduce and integrate

```sh
# Reviewable updates without touching shared catalogs:
python3 -B tests/reference/compiler-public-gate-links.py \
  --propose /tmp/light-cljs-125-public-gate-proposal

# Apply precisely those updates; no assertion/phase catalogs change:
python3 -B tests/reference/compiler-public-gate-links.py --write
python3 -B tests/reference/compiler-public-gate-links.py --check

# Fresh current local Node/QuickJS and both checked reference executions:
python3 -B tests/reference/compiler-public-gate-links.py --run \
  --artifacts /tmp/light-cljs-125-public-gate-oracles-fresh
```

The proposal contains `obligation-updates.jsonl`, `case-additions.jsonl` and
`public-gate-link-receipts.jsonl`. Applying it changes only the ordinary operation
and full-case catalogs and creates the receipts catalog. Each receipt retains the
original literal fixture path/row hash, source hash, result-contract hash,
signature and exact local test identity.

Family generators can preserve the overlay by applying the new
`attach_reference(mapping)` helper to their generated ordinary mappings before
comparison/writing. For the public protocol, public constructor, core factory
and iterator generators, load it with `runpy.run_path` and apply it to their
`mappings`/`obligations` list. The helper changes only the three reference identity
fields. Run this overlay after those generators when rebuilding the catalogs.

## Focused execution receipt

The focused run used implementation HEAD `5a09986` and the unchanged fixture
inputs. All 29 fixtures passed emitted Node, hosted QuickJS, Maven ClojureScript
**1.12.134** and checkout **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. Both
references verified `:optimizations :none`, `:static-fns false` and
`:def-emits-var false`. Reference identities and settings are retained for every
observation under `/tmp/light-cljs-125-public-gate-oracles/`. The initial oracle
run used the compiler-only catalog setting; the separate 29-case hosted QuickJS
run is retained in `/tmp/light-cljs-125-public-gate-quickjs.json`. Subsequent
`--run` invocations perform both local engines directly.

The existing exact named phase contracts execute each fixture in QuickJS and
Node applications and canonical macro computations:

```sh
cargo test --locked --test compiler_public_protocols -- \
  public_protocol_api_arities_and_macro_phase_execute --exact --nocapture
cargo test --locked --test compiler_public_constructors -- \
  public_constructor_api_arities_and_macro_phase_execute --exact --nocapture
cargo test --locked --test compiler_core_factories -- \
  core_factories_execute_in_applications_and_macros --exact --nocapture
cargo test --locked --test compiler_iterator_helpers -- \
  iterator_helpers_execute_in_applications_and_macros --exact --nocapture
```

Exact one-test/one-pass/zero-ignored results and invocation records are retained
under `/tmp/light-cljs-125-public-gate-phases/`. The normal full ledger must still
execute both the linked oracles and these exact phase tests before counting the
131 rows covered. No zero-missing full-language result, other public arity,
unregistered source body, broader regression or performance claim follows from
this focused reconciliation.
