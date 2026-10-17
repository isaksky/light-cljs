# Deferred namespace registration measurements

Isolated instrumentation of phase58 records the source size and emission time
of each deferred unit. Penpot main produces **437 units**, containing
**102,853,884 bytes of namespace registration but only 919,796 bytes of code**.
One late unit registers 851 namespaces in 717,833 bytes for a 378-byte body.
Whole-registration hashes are mostly distinct (375), because the current
namespace changes while preceding namespaces repeat.

Measured emission totals 2.466 s, including the diagnostic registration hash.
Summed expression/string/global table sizes describe snapshots; they do not
assert that every table element was visited. This diagnostic motivates caching
at namespace granularity rather than at whole-registration granularity.

Complete output matches phase42. This is instrumentation, not a production
change or new full-suite validation. The patch is relative to phase58; commands,
per-unit measurements, aggregate data and hashes are archived. Frozen binary
and complete output remain under `/private/tmp/light-cljs-phase67`.
