# Phase86: canonical bridge subtree sharing — not retained

Isolated phase85 variant. Both bridges reuse the previous canonical Datum after
an exact match, allowing parent comparisons to recognize shared child identity.
Descriptor traversal, validation, mutation checks and conversion budgets remain
in place.

Main none takes **20.41 s / 1044201472 peak RSS bytes**, versus phase85's
20.48 s / 1040678912 bytes. This does not show a useful improvement, so the change
is not retained. Complete output matches phase42, including its known main ESM
defect. No full suite was run for this rejected trial. The incremental patch,
scripts, exact command and hashes are preserved.
