# Lazy bridge diagnostic paths — rejected

Isolated from validated V8 phase102. The bridge records reusable path components
and formats a path only on conversion failure, instead of concatenating strings
at every successful node. No descriptor or provenance representation changes.

Cold main none takes **22.51 s / 955,154,432 bytes RSS**, versus an immediately
following frozen phase102 build at **22.95 s / 962,707,456 bytes**. The 1.9%
wall-time difference does not establish a useful gain. The same change regresses
QuickJS slightly (phase106), so it is rejected on both engines.

The two Node differential probes pass 500 descriptor/provenance cases and 900
conversion/error-path/limit/getter/lazy-realization comparisons. Replacing only
the changed `compilerFormBridge` function in the complete output restores exact
phase102 byte identity. The complete output passes explicit ES-module syntax
checking. No full-suite run was performed for this rejected candidate.

The scripts, source patch, frozen binary/output hashes, exact commands, logs and
timings are retained here. Production, phase102 and the Penpot port are unchanged.
