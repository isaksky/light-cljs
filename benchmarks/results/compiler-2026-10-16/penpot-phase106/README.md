# QuickJS lazy bridge diagnostic paths — rejected

The phase105 runtime-only change was applied to validated production phase101
in an isolated directory. Cold main none takes **96.12 s / 815,857,664 bytes
RSS**, versus an immediately following frozen phase101 build at **95.27 s /
798,474,240 bytes**. It is 0.9% slower with 2.2% more RSS; the change is rejected.

Replacing only the changed `compilerFormBridge` function restores exact phase101
output identity. Explicit ES-module syntax checking passes. Phase105 retains the
1,400 successful differential cases shared by this exact JavaScript change.
No full-suite run was performed for this rejected candidate.

The scripts, source patch, binary/output hashes, exact commands, logs and timings
are retained here. Production and the Penpot port are unchanged.
