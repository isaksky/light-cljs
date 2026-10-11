# Pinned SCI observations and canonical evaluator coverage

The immutable `tests/fixtures/sci-outcomes.tsv` corpus records 2,475 observations
from SCI revision `5fe89056384418be50d232b4278344cb3a1a526b`. The pinned JVM oracle
checks original observations; it does not execute or validate the current evaluator.

`tests/evaluator_conformance.rs` executes `evaluator-dispositions.tsv` against the
canonical `EvalSession`. Only portable and explicitly adapted matches count as
coverage. Recorded gaps and retired dialect contracts never count as passes.
See [evaluator accounting](evaluator-accounting.md) for comparison, ownership and
resource contracts. Compiler language accounting remains independently governed
by `tests/reference/compiler-language/report.py`.

```sh
python3 -B tests/reference/sci/regenerate.py --checkout /path/to/pinned/sci
python3 -B tests/reference/sci/regenerate.py --checkout /path/to/pinned/sci --evaluator
cargo test --locked --test evaluator_conformance
```

`--evaluator` additionally runs canonical evaluator/session/CLI and provenance
suites after matching the oracle. `--native` is no longer supported. The launcher
retains strict checkout verification, timeouts, process-group cleanup, complete
case ordering, runtime metadata and crash diagnostics.

The removed interpreter's API inventory, dialect dispositions and verification
narrative are retained under `historical/` solely as archival evidence. They are
not read by canonical coverage tooling. Native embedding provenance whose tests
were removed is preserved separately in `retired-origins.tsv`.
