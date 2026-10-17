# Live allocation snapshots around macro-session release

An isolated phase33 compiler pauses after app.render analysis, records live
allocations with macOS MallocStackLogging=lite and malloc_history -allBySize,
then drops its diagnostic macro host and records another snapshot. It resumes
normal emission afterward. The complete output matches phase33 byte for byte
and passes Node syntax checking. Neither the pauses nor early host replacement
are production changes. The 43.93-second instrumented run is not a benchmark.

| Snapshot | Live allocation records | Allocated bytes |
| --- | ---: | ---: |
| Program plus macro session | 1,689,428 | 312,381,712 |
| Program after macro-session release | 689,568 | 104,108,000 |

These are summed live allocator sizes, not RSS. Stack recording and allocator
retention affect the process footprint. The CLI already drops its temporary
host when analysis returns; this is not evidence of a post-compilation leak.

About 208 MB is released with the session. The analysis-time snapshot includes
30.95 MB on stacks classified as QuickJS. This is a priority-based stack
classification, not an exhaustive engine ownership census; categories can
contain earlier nested callers. Raw snapshots and the classifier are retained.

The actionable discovery is dead reader-validation literals. reader_identity
calls keyword/symbol_literal, which append to Program.strings/named_literals,
then reader_named copies those entries into a temporary validation value.
Duplicate-key checks, large-map ordering and nested set/case validation leave
the unused entries behind. They also get copied by program_fragment::append.
The reader-identity category alone accounts for 16,672,992 live bytes before
host release; copies are attributed to their copying sites instead.

The largest reader-identity allocation stacks include two 6,324,224-byte table
buffers through Analyzer::check_map_keys -> reader_identity -> keyword.
Another 233,742 live string-buffer copies (7,737,280 bytes) originate in
program_fragment::append; that group includes both live and dead literals.
These observations motivate phase42's read-only name resolution.

Full snapshots (before-heap.txt and after-heap.txt), frozen diagnostic executable
and render output remain under /private/tmp/light-cljs-phase41. Scripts,
summed categories, logs and equivalence evidence are archived here. The
summarizer streams the snapshots rather than loading their ~726 MB at once.
