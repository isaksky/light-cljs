# Pinned V8 source-build baseline — validated, still isolated

The phase107 compiler is linked against a locally built, unmodified V8 152.2.0
instead of the upstream prebuilt archive. This establishes a reproducible native
baseline for fixing the known backing-store limit gap; **no allocation fix is
implemented in this phase**. Production remains phase101.

The published crate omits inputs needed for a full build. Its exact upstream
revision, `2768994f664e8a6e3aba27503606c58339136e2a`, pins:

- ICU `d578f2e8b7bd5938e21cfb6bf15c079e0aa5b738`. Restored `icudtl.dat` is
  10,876,560 bytes, with Git blob ID `9655a2e17f08c56d6dbbc47f6e52e3fd3ef2436e`.
- Vendored Rust dependencies `afbc96607d0e659715d803cc099607dd1737fc41`.
  All 279 files already in the crate match the restored upstream tree.

The build uses Chromium's pinned Clang/linker and Rust toolchains, with local
Clang 23's libclang for binding generation. The successful release build takes
5 minutes 40 seconds using 20 jobs. The native archive, generated bindings and
GN arguments are frozen together; validation uses the crate's supported local
archive/binding overrides to test that exact library without a second native
build. Their hashes and the precise environments are recorded.

Setup failures are retained and excluded from results. An initial setup race
used the original registry path and stopped before native compilation because
ICU data was absent. A dependency-path assertion now prevents that race. The
correct local-source attempts then identified missing vendored Rust sources and
a missing linker in the host Clang installation. The final build restores the
pinned sources and uses Chromium's complete native toolchain. Temporary download
signatures are omitted from archived logs.

Full validation passes **1,426 tests, zero failures, 106 ignored**, normal
concurrency, strict Clippy, formatting and all three Node workflows. Sequential
cold builds:

| Build | Wall | Peak RSS bytes |
| --- | ---: | ---: |
| render | 3.84 s | 485,801,984 |
| main none | 23.12 s | 985,628,672 |
| main advanced | 29.07 s | 1,165,656,064 |
| immediate frozen phase107 main none | 23.06 s | 967,016,448 |

The source build and prebuilt engine perform essentially alike. All three
outputs match phase107 byte-for-byte and pass explicit ESM syntax checks; the
immediate baseline output also matches. This is not a bundled/browser execution
of Penpot. No fresh Shadow run was needed for this build-tooling check; phase107
retains the latest five-way comparison.

The remaining work is the backing-store accounting fix and backend packaging.
The dramatic speed goal remains unmet.
