# Sample the current native execution

macOS sample observed the frozen phase33 production executable compiling
app.main, requesting a sample every 10 milliseconds. The compiler exited
successfully after 121.357 seconds. The complete output is byte-identical
to phase33 and passes Node syntax checking. No CPU-heavy work overlapped
the compiler except the sampler itself. This is a diagnostic, not a new
clean benchmark or an exact accounting of elapsed time.

The call tree contains 10,143 main-thread observations. JS_CallInternal is
the leaf in 4,075 (about 40%). Other frequent leaves include QuickJS native
calls, reference-count freeing, equality, property operations, Map lookups
and string hashing, plus the platform allocator. These runtime operations
also serve bridge conversion, so this is not a pure macro-body percentage.

The largest named Rust leaf is emit_unit_mode with 191 samples (about 1.9%).
Rust SipHash has 111 and UTF-16 encoding 73. A linker-deduplicated symbol
accounts for 678 and cannot be reliably assigned to a Rust function from
this release binary. Recursive inclusive counts in the raw report must
not be summed or treated as time shares.

Canonical snapshot comparison (form_bridge::exact) has six leaf samples;
this profile does not identify it as a large cost. The expensive transport
stages include JavaScript traversal and construction as well as native
encoding/decoding. The previous phase's scoped timers remain the better
accounting of the stage boundaries.

This supports reducing repeated runtime dispatch and representation work,
while preserving the conversion checks, rather than assuming there is one
large remaining Rust pass. Phase37 tests a representation-preserving early
rejection of primitive values in private native-brand inventories. No
production code changes were made by this profiling phase.

The compressed full profile and leaf summary are retained here. The raw
52 MB profile, compiler output and reproducible runner remain under
`/private/tmp/light-cljs-phase36/`. The production source/binary snapshot
and its complete validation remain phase33.
