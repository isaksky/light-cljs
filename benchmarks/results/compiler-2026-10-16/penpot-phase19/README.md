# Phase19 environment import by component

Diagnostic instrumentation of the isolated phase13 source archive, including
the shared-storage probes added during phase15. This does not include phase14's
incremental arity index or phase15's parsed-tree release. No production code
changes in this phase. Each profile ran alone after the clean benchmarks.

| Main environment component | Construct values, seconds | Snapshot values, seconds |
| --- | ---: | ---: |
| locals | 17.060 | 6.613 |
| macro inventory | 0.416 | 0.147 |
| namespace | 0.331 | 0.079 |
| reader context | 1.054 | 0.366 |

The complete JS import service takes 29.94 seconds; the table excludes scalar
components, root-map construction and other service overhead. Locals account for
about 79% of the service time. Rust descriptor encoding and snapshot decoding
are separate buckets (2.37 and 2.11 seconds respectively). The 48,216 imports
have 20,482 whole-locals-map cache misses and 27,734 hits. Macro inventory has
only 73 misses, namespace data 855 and reader context 11,821.

Reader setup remains a separate cost within macro calls: 14.79 seconds total,
including 11.52 seconds preparing aliases. Do not add those inclusive reader
times to their enclosing macro-call buckets.

Instrumented main: 186.20 seconds / 1,464,827,904 bytes RSS. Instrumented render:
24.44 seconds / 610,680,832 bytes RSS. These are diagnostic runs, not production
performance claims. Both application bodies match phase6b outside the instrumented
runtime and complete outputs pass Node syntax checking. Application execution
remains untested.

`prepare.bb` depends on the isolated
`/private/tmp/light-cljs-phase13-storage-profile/` source archive and saved inputs
under `/private/tmp/light-cljs-phase19/`. It is retained as instrumentation
evidence, not a standalone supported profiling runner.
