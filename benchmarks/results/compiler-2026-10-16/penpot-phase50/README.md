# Isolated CLI allocator experiment

The phase48 native profile placed 23.2% of main-thread exclusive observations
in the system allocator. This trial changes the CLI's Rust global allocator
to mimalloc while keeping the phase47 V8 compiler and all compiler semantics
unchanged. It does not include phase49's string-scan experiment.

The exact dependency is [`mimalloc` 0.1.52](https://github.com/purpleprotocol/mimalloc_rust/blob/master/Cargo.toml),
with `libmimalloc-sys` 0.1.49 pinned in `Cargo.lock`. The installed dependency's
build script selects its bundled v3 implementation by default. No `override`
feature is enabled: this does not replace V8's C++ allocator. The Rust global
allocator is selected only in `main.rs`; library consumers retain their own
allocator. [mimalloc upstream](https://github.com/microsoft/mimalloc) and the
crate's source are the implementation references, not evidence predicting this
compiler's performance.

The first clean main-none observation is **26.51 s**, 31.41 user seconds,
0.72 system seconds and **1,336,655,872 bytes peak RSS**. The complete output
matches phase42 exactly. Compared with phase47's 35.71 s / 1,373,241,344 bytes,
time falls 25.8% and RSS changes only slightly. macOS peak footprint is
1,146,262,528 bytes, higher than phase47's 1,021,710,912; these are different
memory measures. This is not evidence of a substantial memory improvement.

The fresh-process repeat takes **26.58 s / 1,348,861,952 bytes RSS**, with
31.42 user seconds and 0.70 system seconds. It also matches phase42 exactly.
Both observations are faster than phase47's shadow dev 30.27 s, but the
overall goal of a dramatic speed and memory improvement is not met.
No complete suite has yet been run with this allocator
variant. The V8 backend also retains phase44's resource-limit and portability
limitations. Neither engine nor allocator is promoted to production.

`allocator.patch` applies to the phase47 V8 prototype and includes exact
dependency versions. Logs, commands and source hashes are archived here;
large output and the frozen binary remain under `/private/tmp/light-cljs-phase50/`.
The baseline main ESM syntax defect remains.
