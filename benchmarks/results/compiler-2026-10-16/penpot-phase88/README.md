# Phase88: QuickJS namespace registration cache — not retained

Isolated phase87 variant. It splits namespace registration from deferred unit
code and caches the latest compiled registration function per namespace, bounded
to 1,024 entries and 4 MiB of UTF-16 source. Every registration still executes
with fresh maps and sets. Changed source recompiles; replaced global `eval` uses
the ordinary evaluation path. The cache lives entirely inside the engine heap.

Four focused tests pass: fresh state and restored source views, eviction and
replaced eval, callback reentry, and stopping at the first exception with a
terminal owned diagnostic. Complete output matches phase42, including the known
main ESM defect.

Main none takes **89.04 s / 943587328 peak RSS bytes**, versus phase87's
91.34 s / 893534208 bytes. The small time change and increased RSS do not justify
retaining this delta. No full suite was run for this rejected trial. Phase89
uses this exact binary with caching disabled to separate caching from the
registration split. The patch is relative to phase87's formatted source.
