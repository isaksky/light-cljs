# Phase83: release completed units' input storage

Isolated phase78 variant. Definition snapshots and registration-time macro hosts
are optional boxed inputs, consumed by their one analysis. Completed units retain
neither empty inline storage for these large objects nor newly allocated default
macro hosts. The existing completed-input retention regression now also checks
that the host is absent; it passes.

Main none takes **20.21 s / 1136787456 peak RSS bytes**, versus phase78's
20.31 s / 1175699456 bytes. Complete output matches phase42, including the known
main ESM defect. No full suite has yet been run for this delta. An initial focused
test command matched zero tests; the corrected script and log show the actual
one-test pass. Production remains QuickJS phase72.
