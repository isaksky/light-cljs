# Phase74: native sample of the combined V8 candidate

The frozen phase71 executable is sampled at two-millisecond intervals while
building main. Mimalloc statistics are enabled. This diagnostic run takes
24.79 s; complete output matches phase42, including its known ESM defect.

Among 9,130 main-thread observations, `update_reader_view` appears in 882
inclusive observations (9.7%), and `count_namespace_globals` in 292 (3.2%).
Global-arena iteration has 479 exclusive observations. These overlap and must
not be added. Reader alias/refers reconstruction and hashing on each form is
a concrete next target; unchanged reader configuration need not be copied.

V8 weak-map lookups, megamorphic property access and ephemeron GC remain
prominent. Mimalloc reports 1,019.8 MiB peak committed arenas and 2 GiB reserved
address space; these are not live Rust allocation bytes or additional RSS.
The allocator has purged 1.3 GiB cumulatively by exit, so those statistics alone
cannot identify retained objects or justify treating reservation as a leak.

The compressed raw sample, exclusive/inclusive summaries, allocator log,
commands and hashes are preserved. Production remains QuickJS; this is an
unpromoted prototype profile, not a new clean benchmark or validation result.
