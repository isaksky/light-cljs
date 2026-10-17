# Capture and replay the Instaparse grammar macro

An isolated compiler based on phase29 records the real
`instaparse.abnf/precompile-cljs-grammar` invocation while compiling the
unchanged Penpot port's `instaparse.abnf` entry. It records emitted sources,
demanded units, declaration frontiers, inputs and the encoded result.
Opaque provenance is removed from the comparison descriptors. No capture
instrumentation is retained in the production compiler.

Both engines reproduce exactly the captured result. This checks the value
of this invocation, not general engine equivalence or a full Penpot build.

| Execution | First invocation ms | Second invocation ms |
| --- | ---: | ---: |
| Original compiler invocation, including compiler callbacks | 11101 | — |
| QuickJS replay | 10966 | 10996 |
| Node replay | 1460 | 1408 |
| QuickJS replay with consecutive demand cache experiment | 10944 | 10977 |
| QuickJS replay with keyword record-field lookup prototype | 9311 | 9324 |

The replay has already emitted unit sources, so it excludes Rust analysis
and emission callbacks. Its near equality to the original invocation shows
that JavaScript execution dominates this macro. The Node comparison is a
bounded diagnostic, not a prediction for the whole compiler and not a
decision to replace QuickJS.

The separate QuickJS interrupt profile reports 101,172 polls, zero dropped.
These are interrupt-check frequencies, not elapsed-time percentages. It
highlights declaration-demand checks, protocol dispatch and record-field
comparisons. The consecutive-demand cache showed no useful gain and was
rejected. The record-field prototype motivates phase32, including its
independent upstream correctness check.

Capture, generated replay sources, runtime snapshot and executables remain
under `/private/tmp/light-cljs-phase31/`; their key fingerprints accompany
these results. Reproduction uses `prepare.bb`, `run.bb`, `make-replay.bb`
and `profile.bb`, with the unchanged port and a phase29 source checkout.
The existing standalone probe takes runtime-source and expression-source
paths. `profile.bb` builds the same probe against the instrumented engine.
The large capture and emitted application are not copied into the repository.
