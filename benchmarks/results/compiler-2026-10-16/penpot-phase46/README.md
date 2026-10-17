# Exact-size ASCII name allocation

`Name::new` formerly collected `str::encode_utf16()` into `Rc<[u16]>`.
Rust's generic collection path first builds a vector, then allocates the shared
slice. ASCII now uses the exact-size byte iterator mapped to `u16`, allowing
the standard library to allocate the shared slice directly. Non-ASCII text
keeps the existing conversion. `Name::from_utf16`, including unpaired surrogate
handling, is unchanged. There is no interning or unsafe code in this change.

The focused probe compares both implementations for ten representative strings
and every valid Unicode scalar embedded in a string. All values match. Across
the ten cases, allocations/reallocations fall from **29 to 12**, with requested
bytes falling from **856 to 392**. Repeated three-million-component loops take
304/319 ms before and 144/139 ms after. These microbenchmarks do not predict a
whole-build speedup. `names.rs` and `micro.bb` reproduce them.

The clean isolated V8 main-none run takes **39.28 seconds**, 44.08 user seconds,
1.08 system seconds and **1,429,569,536 bytes peak RSS**, versus phase45's
42.74 seconds / 1,444,806,656 bytes. Its entire output matches phase42 exactly.
The known main ESM syntax defect remains. No engine code changed in this phase.

The full V8 suite at default test concurrency **failed** two tests:
`core_operations::signatures_hold_on_sample_arguments` exceeded its deadline,
and `macros::recursive_and_growing_expansions_obey_depth_count_size_and_execution_limits`
did not receive its expected poll-budget diagnostic. Both pass individually
without changes. The failed full run is preserved in `tests-v8.log`; isolated
reruns are in `retry-signatures.log` and `retry-budget.log`. A retry of the
complete combined candidate is recorded in phase47 with four test threads.
These observations do not establish that the experimental V8 budget behavior
is production-ready.

`name-allocation.patch` is the engine-independent delta from phase45.
Frozen binary and full output remain under `/private/tmp/light-cljs-phase46/`,
with hashes here. This phase's evidence alone does not establish a production
speedup; the production rerun follows validation of the combined changes.
