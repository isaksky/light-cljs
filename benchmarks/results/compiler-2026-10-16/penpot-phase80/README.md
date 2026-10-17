# Phase80: shared entries in modified arena leaves — not retained

An isolated phase78 variant converts a frozen arena leaf to individually shared
entries on its first copy. Later snapshots copy references and only the modified
entry. Dense, untouched leaves keep their original layout. Four arena tests pass,
including repeated writes across snapshots and mixed-direction iteration across
dense and shared leaves.

Main none takes **20.46 s / 1175191552 peak RSS bytes**, versus phase78's
20.31 s / 1175699456 bytes. This shows no useful timing or memory gain, so the
change is not retained. No full suite was run for this rejected experiment.
Complete output is byte-identical to phase42, including its known main ESM
defect. Production remains QuickJS phase72; the latest fully validated V8
prototype remains phase76.
