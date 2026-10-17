# Phase21 interpreter interrupt profile

Diagnostic instrumentation of the phase20 production source, in an isolated
project under `/private/tmp/light-cljs-phase21/`. No production code changes.
The temporary C hook samples the current JS stack at QuickJS interrupt checks.
It records source coordinates and function names without retaining engine
objects or atoms and without accessing user properties. The fixed table dropped
no samples. Inclusive counts deduplicate a function within each sampled stack.

These are interrupt-check frequencies, not elapsed-time percentages. Polls occur
at selected interpreter control-flow points; native C operations and Rust work
are not independently sampled. Render collected 63,501 polls and main 549,340.
Diagnostic runs took 22.51/166.78 seconds. Both complete output files are
byte-identical to phase20 and pass Node syntax checking.

`publicMethod` appears in 20,060 render stacks and 212,802 main stacks. Bridge
decode appears in 27,018/149,661; canonical encoding's `visit` appears in
18,378/104,220. `listFromArray` appears in 11,768/64,026. It currently creates
each list node through `cons`, which calls general `count` on the newly built
tail. That enters protocol dispatch and declaration-demand preparation even
though the constructor already knows the number of items. This motivates a
separate direct-builder experiment. The profile alone does not establish its
speedup or authorize bypassing observable operations elsewhere.

Full logs, summaries and preparation scripts are retained here. The scripts
expect the repository working directory and the local temporary project paths;
they are diagnostic evidence, not supported standalone tooling. The original
phase21 binary and runtime source are fingerprinted here. Subsequent experiments
may reuse the temporary project's source and target directory, so use the saved
binary and preparation script for the recorded version.
