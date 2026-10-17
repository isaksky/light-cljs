# Active ancestor stack — V8 trial, not retained

An isolated phase58 trial replaces the compiler form encoder's active-ancestor
WeakSet with an array stack. Only the path's bounded ancestors are searched;
the separate arbitrary-length sequence cycle detector remains a WeakSet.

Main none takes **25.09 s / 1,302,429,696 bytes peak RSS**. The small difference
from phase58's 25.69 s is not convincing evidence of a speed gain. Phase64's
QuickJS trial fails to improve production; the change is not retained.

Only the expected embedded compiler form bridge differs in the complete none
output; replacing that exact function restores byte identity with phase42.
No full suite was run for this rejected optimization. Main's known ESM syntax
failure and V8's resource-limit gaps remain.

The patch, commands, timing, equivalence check and hashes are archived. Frozen
executable and complete output remain under `/private/tmp/light-cljs-phase63`.
