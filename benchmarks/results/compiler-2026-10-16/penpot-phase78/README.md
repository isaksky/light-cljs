# Phase78: share metadata map layers until mutation

Isolated V8 candidate based on phase76, not promoted. `Layered` now shares
both its base and its small own map through Rc. Taking an unchanged macro
analysis frame therefore increments references; mutating an own map copies
it only when another frame retains it. Lookup and compaction semantics stay
the same.

Both focused map tests pass. The new regression checks saved frames across
mutable values, removals, reinsertion and repeated compaction against independent
maps. A full suite has not yet run for this delta.

The diagnostic main build takes **20.31 s / 1175699456 peak RSS bytes**, versus
phase76's 21.04 s / 1182187520 bytes. This is a modest single-run timing gain
with little RSS change. Complete output matches phase42, including its known
main ESM syntax defect. Phase79 repeats the allocation probe to distinguish
retention from allocation churn.

`changes.patch` applies to phase76. Scripts, logs, exact command and hashes
are preserved here. Production remains the validated QuickJS phase72.
