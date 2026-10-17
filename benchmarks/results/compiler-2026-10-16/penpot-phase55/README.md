# Isolated persistent namespace member IDs trial

Based on phase54. Definition snapshots share an append-only arena of the IDs
belonging to their namespace. Reader bindings and declaration transport use those
IDs rather than rescanning unrelated globals. Deferred-unit private tails remain
separate, and the interactive evaluator constructs its own namespace IDs.

The valid rebuilt trial takes 25.84 s / 1,358,200,832 bytes RSS, with complete
output matching phase42. This is a small single-run improvement over phase54,
with higher RSS. It is not promoted and has not passed the full suite.

The first attempted measurement used a stale binary from the shared Cargo target
directory: this project had been prepared before the prior project's build, so
its source mtimes did not force compilation. Its binary hash equals phase54.
That invalid measurement is preserved with `stale-target-` names and is excluded
from the result above. The run script now touches both crate roots before
building, and the valid binary hash differs. The subsequent compilation caught
an interactive evaluator call site that still used the old API; that diagnostic
is preserved in `build-missing-caller.log`. The final patch includes its fix.

`namespace-ids.patch` applies to phase54 and contains every final source edit.
Preparation/run/archive scripts, commands, logs, timing and hashes are preserved.
