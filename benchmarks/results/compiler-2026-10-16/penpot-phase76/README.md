# Phase76: resolve reader bindings on demand

Isolated V8 candidate based on phase71. The compiler's private reader now
resolves names used by syntax quotation against the current analyzer name
index. It no longer builds an entire runtime binding table, revisits pending
declarations or scans expressions for binding kinds. Public NamespaceView
binding introspection is unchanged. Local refers, macro precedence, alias
collisions and definition-time snapshots retain their existing resolution.

**1,408 tests passed, 0 failed, 106 ignored**, with four test threads; strict
Clippy, formatting and all three Node workflows pass. This validates the
phase71 typed root registration as well as the reader change. The focused
alias-collision/captured-definition test also passed before benchmarking.

Clean sequential cold builds with a 256 MiB V8 heap, without statistics or
terminal GC:

| Build | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: |
| render | 3.40 | 555548672 |
| main none | 20.98 | 1237204992 |
| main advanced | 26.36 | 1310638080 |

The diagnostic main run with statistics and terminal GC is 21.04 s /
1182187520 bytes, versus phase71's 22.40 s / 1177714688 bytes. Every complete
output matches phase42, including the known main ESM syntax defect.

The candidate is about 31% faster than fresh shadow dev, at 52% of its RSS.
The dramatic speed goal remains unmet. V8's resource-limit, watchdog and
packaging gaps remain, and it is not promoted. Production remains QuickJS
phase72. `changes.patch` applies to phase71. Full logs, scripts, commands,
timings and hashes are archived here; frozen binary and large outputs remain
under `/private/tmp/light-cljs-phase76/`.
