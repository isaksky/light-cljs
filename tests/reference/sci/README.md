# SCI reference fixtures

`tests/fixtures/sci-outcomes.tsv` holds 2,475 observations recorded from SCI at
revision `5fe89056384418be50d232b4278344cb3a1a526b` (see [NOTICE.md](NOTICE.md)
for attribution). These are the original SCI results; they are never rewritten to
match light-cljs.

`evaluator-dispositions.tsv` assigns every SCI row a status and is executed by
`tests/evaluator_conformance.rs` through a fresh `EvalSession` per row. Each line
has 14 tab-separated fields: the seven original SCI fields (ID, profile, source,
outcome, effects, stdout, stderr), then status, the source actually evaluated,
the light-cljs outcome, effects, stdout, stderr, and a reason with
`owner=`/`kind=`/`family=` tags. Statuses:

- `portable` – unchanged source matches the SCI observation under CLJS
  (JavaScript Number) semantics.
- `adapted` – the source was explicitly adapted (for example an explicit
  `:require` of `clojure.string`, or a JVM catch class replaced by `:default`)
  and then matches.
- `gap` – light-cljs currently differs; its observation is recorded so changes
  are visible, but it is not accepted as correct.
- `retired` – depends on behavior light-cljs deliberately does not provide
  (exact JVM numerics, `.clj` loading, the `:bb` reader feature).

Only `portable` and `adapted` rows count as coverage.

## Commands

```sh
# Run the dispositions (no external tools)
cargo test --locked --test evaluator_conformance

# Re-run SCI on the JVM and compare with the committed observations
python3 -B tests/reference/sci/regenerate.py --checkout /path/to/pinned/sci
# ...then also run the evaluator, session, CLI and provenance suites
python3 -B tests/reference/sci/regenerate.py --checkout /path/to/pinned/sci --evaluator
```

`regenerate.py --write` updates the committed fixtures and `runtime.tsv` only after
every case matches. The same JVM comparison is available as the ignored test
`pinned_sci_reference_matches_committed_outcomes` in `tests/sci_reference.rs`
(set `LIGHT_CLJS_SCI_CHECKOUT`). The ignored
`audit_historical_corpus_with_compiler_evaluator` test in
`tests/evaluator_conformance.rs` prints current observations for review; set
`LIGHT_CLJS_SCI_AUDIT_IDS` to a comma-separated list of fixture IDs to restrict it.

`historical/` and `retired-origins.tsv` are archival tables from the removed native
interpreter; no current tooling reads them.
