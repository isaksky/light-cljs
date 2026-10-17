# Profile after native CLI allocator selection

Frozen phase50 V8 CLI, 256 MiB V8 heap, same main-none command and port.
The instrumented run takes 28.02 seconds and its complete output matches phase42.
This is a sampled run, not a clean timing comparison with phase50's 26.51/26.58.

Of 4,322 main-thread observations, only 24 are exclusive leaves in the system
malloc library (0.56%), compared with 23.2% before selecting mimalloc. Mimalloc
itself is still visible: 64 `mi_free` and 49 `mi_malloc_aligned` leaves. This
does not imply zero allocation cost or that inclusive totals can be added.

Remaining native costs include generated-unit parameter scanning (245 exclusive
`emit_unit_parts` observations), and deduplicated table functions called from
reader-form lowering (138), reader binding enumeration (113) and reader context
construction (76). V8 WeakMap lookup, property access and ephemeron processing
are also substantial. Raw compressed samples and exact counts are preserved.

A separate clean run with a 128 MiB V8 heap fails after 17.70 seconds during
Instaparse ABNF expansion with a controlled memory-limit diagnostic. Its peak
RSS is 995,098,624 bytes, but it is not a completed build. The smaller 64 MiB
trial was not started after that failure. `heaps.bb` preserves that fail-fast
ordering; `main-128.time` preserves the diagnostic and resource usage.
