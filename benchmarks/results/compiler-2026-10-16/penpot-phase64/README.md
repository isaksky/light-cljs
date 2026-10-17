# Active ancestor stack — QuickJS negative result

Phase52's production-equivalent QuickJS candidate with phase63's active-ancestor
stack. Main none takes **94.22 s / 1,048,231,936 bytes peak RSS**, versus the
retained production's **93.83 s / 993,214,464 bytes**. This does not improve
production and is not retained. No full suite was run for the rejected change.

Only the expected embedded compiler form bridge differs in the complete none
output; replacing that exact function restores byte identity with phase42.
The known main ESM syntax failure remains. The patch, commands, timing,
equivalence and hashes are archived; complete output and executable remain
under `/private/tmp/light-cljs-phase64`.
