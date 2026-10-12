# Invalid canonical iterator name premise

Pellet 182 is resolved as invalid/not applicable. Its description incorrectly
claimed that both ClojureScript pins returned map iterator entries, keys and
values for the unchanged source. The retained 181 catalog already recorded a
thrown TypeError, not returned entries. Fresh independent builds now confirm
that result for release `1.12.134` and clean checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, using `:optimizations :none`,
`:static-fns false` and `:def-emits-var false`.

The original complete source is retained byte for byte in the
[fixture](../../fixtures/compiler-iterator-name-original.cljc), SHA-256
`9c0082c2dcd72d457b2994cceb77103567539d88d929beb3f4d524f153f23fd4`.
Both checked core sources have no `iterator-seq` declaration. Both independently
compile that original with four undeclared `user/iterator-seq` warnings, then
execute it to the exact typed packet:

```json
{"id":"runtime-map-traversal-iterators","outcome":{"status":"threw","error":{"category":"type-error"}},"trace":[],"stdout":""}
```

Their canonical public function is `es6-iterator-seq`, already resolved and
implemented locally. The [correction producer](iterator_name_correction.py)
retains its full declaration and both core source hashes. No runtime alias or
public API obligation is introduced by the erroneous source spelling.

The [local regression](../../compiler_iterator_name.rs) confirms the complete
original rejection in application and demanded macro phases, with fresh and
reused MacroHosts. A separately identified diagnostic changes the spelling to
`es6-iterator-seq` and checks the complete literal map entries/keys/values/ES6
iteration result in QuickJS and Node after each same-host rejection. That
changed diagnostic supplies no credit to the original. Existing iterator and
sequence completion suites also retain qualified/saved/apply, lazy demand,
receiver, tail retry and phase isolation behavior.

The [durable receipt](iterator-name-correction-receipt.json) is byte-identical to
`/private/tmp/light-cljs-182-iterator-name-correction-final-v2/evidence.json`,
SHA-256 `9ea985ac25fe853a56d9d8bb7a5269caefa6b8b3e4f46cf28a232f87494b191e`.
It binds the original source, checked pins/settings, complete declarations,
compiler/producer sources, Cargo audit executable, commands, environment,
reused macro warmup, exact four local rejection contexts, two raw pin packets
and full artifact census. The audit actually ran; missing engine execution
following source rejection is never counted as an engine pass.

[Permanent guards](test_iterator_name_correction.py) reject altered source,
returned-result substitution, missing raw evidence and altered raw pin output,
warning, local module, rejection or command, even after refreshed artifact
hashes. A detached summary claiming semantic credit also fails. All mutations
are restored and pristine executable replay succeeds.

The historical 181/179 catalogs, original bodies, packets, receipts and expected
local rejections are unchanged. The live 182 description retains its original
requirements and appends this correction. The map traversal ownership summary
identifies 183–185 as the remaining factory, source macro and equality gaps.
The invalid spelling control remains whole-source zero-credit; no passing
source/arity or full-language credit is added, and strict 125 is unchanged.

Verification:

```sh
cargo test --locked --test compiler_iterator_name canonical_iterator_name_preserves_rejection_and_same_host_recovery -- --exact
cargo test --locked --test compiler_iterator_helpers --test compiler_sequence_completion
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/iterator_name_correction.py --output /private/tmp/light-cljs-182-iterator-name-correction-final-v2
PYTHONDONTWRITEBYTECODE=1 ITERATOR_NAME_RECEIPT=/private/tmp/light-cljs-182-iterator-name-correction-final-v2/evidence.json python3 tests/reference/compiler-language/test_iterator_name_correction.py
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/test_runtime_map_traversal.py
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/extract_inventory.py --check
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/test_full_ledger.py
```

No compiler/runtime behavior changed, so a performance comparison or benchmark
rerun would not measure this correction. Previously validated 181 benchmarks
remain historical evidence. No cancellation or singular/plural limit suite was
run; blocked 156 was neither inspected nor retried. No push or PR is involved.
