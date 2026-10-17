# Compare live render allocations after the reader-identity fix

The separate live-allocation comparison of render confirms reduced retained
data: 312,381,712 to 290,492,640 bytes after analysis (7.0% less), and
104,108,000 to 94,240,688 bytes after releasing the macro session (9.5% less).
The reader-identity allocation category disappears; the classified QuickJS
allocation total is unchanged. These are live allocator sizes, not RSS.
The instrumented output also matches the validated output exactly.

| Snapshot | Old records | New records | Old bytes | New bytes |
| --- | ---: | ---: | ---: | ---: |
| before | 1689428 | 1444726 | 312381712 | 290492640 |
| after | 689568 | 556411 | 104108000 | 94240688 |

Uses the same macOS live-stack diagnostic as phase41, after all clean
benchmarks completed. Its time/footprint is not a benchmark. Raw snapshots
and frozen diagnostic executable remain under /private/tmp/light-cljs-phase42/heap.
The classifier's priority and ownership limitations remain those of phase41.
