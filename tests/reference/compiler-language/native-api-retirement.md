# Five historical native numeric API registrations

The existing group explicitly retires native exact integers, ratios and
BigDecimal, including native arbitrary precision. Only the exact historical
`clojure.core/bigdec`, `bigint`, `biginteger`, `ratio?`, and `rational?` API rows
fall under this proposal. Their immutable historical classifications are
`unsupported-explicit` or `sci-exact`; rewriting those fields to `native-only`
would destroy provenance. The existing ledger condition requiring `native-only`
misses these original registrations.

`native_api_retirement.review(path, full_original_line)` returns a dialect
disposition only for the exact historical path/name/row hash. Drift raises an
error; every other row returns None. This is an explicit retirement, with zero
semantic pass credit. Preserve current located unresolved-symbol diagnostics,
undeclared-Var warnings and empty stdout as nonpassing evidence.

The mixed whole `sci-native-numeric-predicates` source remains a gap: its ordinary
Number operations cannot be retired by either ratio predicate's API boundary.
Ordinary Number predicates and explicit ECMAScript BigInt interoperability stay
independent obligations. No historical TSV, original SCI expectation or shared
ledger/report/catalog/manifest is changed by this unregistered helper.

The exact scope-bound proposal with current parent-produced five rejections is
`/tmp/light-cljs-125-native-api-retirement-review/proposal-with-current-rejections.json`.
`validate_diagnostics` checks those rejection packets against current compiler
binary bytes, exact command/source/row hashes and located output; it never runs
a probe or returns a semantic pass. Integration must retain rejection columns
when applying the five dialect dispositions, instead of making their statuses
covered or their columns passed.
