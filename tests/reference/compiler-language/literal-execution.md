# Ordinary literal execution regression reconciliation

Pellet 193 corrects obsolete expectations in the two complete existing tests
`map_source_order_and_numeric_literal_collisions_have_explicit_owned_semantics`
and `set_reader_rejection_and_distinct_operand_effects_stay_separate`.
All seven executable sources and both duplicate-set rejection sources retain
their exact bytes. The complete original test bodies, comments and packets are
in [the historical snapshot](literal-execution-historical-before193.json).

Fresh independent release `1.12.134` and clean checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` executions used
`:optimizations :none`, `:static-fns false`, and `:def-emits-var false`.
The first independent execution preceded every expectation change. A second
fresh execution with the committed producer confirmed all nine originals.
Both runs and the local failures remain identified in
[the reference receipt](literal-execution-reference-receipt.json).

| Existing case | Both pins' value/error | Exact trace |
| --- | --- | --- |
| map-literal-number-collision | `[2 false]` | `[]` |
| map-direct-constructor-order | `4` | `[1,3,2,4]` |
| map-literal-children-order | `4` | `[1,2,3,4]` |
| set-literal-discard-order | `3` | `[2,1]` |
| set-literal-return-order | set with members 1 and 2, count 2 | `[2,1]` |
| set-literal-failure-order | type error | `[["throw","type"]]` |
| set-direct-number-deduplication | `[2 2]` | `[]` |

Every packet includes empty stdout. Both complete rejection controls reject at
the reader with `Duplicate key:` diagnostics; their full independent stderr
and source hashes are retained. Their existing local duplicate-member checks
remain in the complete test.

These results agree with the delivered numeric, positional-constructor, set
reader and map reader contracts from 176/177/178/179, and direct `hash-map`
operand grouping from 184. The small source map still interleaves each key with
its value; its expectation is unchanged. The old set case ID is retained for
historical correspondence, while its direct small constructor count now
requires positional entries. No production implementation changes are needed.

Both complete named tests now execute their identical emitted scripts in Node
and QuickJS using the existing shared observation encoder. A scoped QuickJS
text-stream adapter rejects byte output. Both engines compare the entire
packet, including ordered effects and stdout. The existing unordered set
encoding is unchanged; no trace or finite sequence sorting was introduced.
`LIGHT_LITERAL_EXECUTION_AUDIT_ROOT` optionally saves both actual engine inputs
and outputs for independent comparison.

Reproduce the independent references and original-source preservation guards:

```sh
python3 -B tests/reference/compiler-language/literal_execution_reference.py --output /tmp/literal-execution-fresh
python3 -B -m unittest discover -s tests/reference/compiler-language -p test_literal_execution.py
cargo test --locked --test execution map_source_order_and_numeric_literal_collisions_have_explicit_owned_semantics -- --exact
cargo test --locked --test execution set_reader_rejection_and_distinct_operand_effects_stay_separate -- --exact
```

The [verification receipt](literal-execution-verification-receipt.json) records
the final actual packets, tests, strict Clippy and ordinary benchmark evidence.
Original-language site credit stays zero and strict language completion stays
false. The distinct direct Var callback regression remains owned by 194.
Cancellation/resource-limit suites and externally blocked 156 are excluded.
