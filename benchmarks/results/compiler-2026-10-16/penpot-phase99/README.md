# Phase99: release application-consumed analysis programs

Once a deferred unit's analyzed body is appended to its application owner, the
session prepares its JavaScript source and retains only that source and immutable
namespace-registration tables. The full analyzed program becomes weakly held.
Macro initialization and namespace registration still occur on demand, through
the existing JavaScript root state machine. Preparing code executes no user code.
Existing analysis/emission counters continue to count analysis and engine demand.

Definition-test units and already-analyzed declaration groups use the same
handoff. Reentrant callers that still own a program can finish using it. A new
test verifies that completed application assembly releases those program owners;
another compares prepared and direct code/registration, including records,
protocols, metadata, raw JavaScript and later mutation of source tables.

Full validation: **1,418 passed, zero failed, 106 ignored**, normal concurrency;
strict Clippy, formatting and all three Node workflows pass. The focused macro
run passes 128 tests. `changes.patch` is relative to phase97.

| Clean cold build | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: |
| render | 3.70 | 480657408 |
| main none | 22.87 | 953434112 |
| main advanced | 28.92 | 1120272384 |

Main peak RSS falls **15.2%** versus phase97's clean run; advanced falls 9.3%.
Time is essentially unchanged. The first isolated main run is 22.52 s /
963117056 bytes. All three complete clean outputs match phase42. Explicit ESM
checking passes render and reproduces the known main defect; application
execution is not claimed. No diagnostic allocation instrumentation is present.

This improves the validated isolated V8 candidate, not yet production. V8's
backing-store limits, startup handling and packaging gaps remain. The dramatic
speed goal is unmet. Phase100 ports the storage change to QuickJS, using ordinary
registration evaluation without the rejected namespace bytecode cache.
