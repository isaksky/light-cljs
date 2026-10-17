# Patched rusty_v8 bindings

These are the Rust bindings from the published `v8` crate **152.2.0** (V8
**15.2.124.1**), with a source build wrapper. The upstream crate records commit
`2768994f664e8a6e3aba27503606c58339136e2a`; its published package is the source
authority, verified by the SHA-256 in `prepare.bb`.

`engine-budget.patch` adds optional allocator accounting for resizable buffers,
growable shared buffers and WebAssembly memory. Reservations cover committed
pages, release on shrink/destruction, preserve allocator lifetime and serialize
shared growth. Ordinary allocators retain upstream behavior. A versioned native
symbol makes an unpatched archive fail at link time instead of silently linking
an incompatible allocator vtable.

The default build downloads and verifies the source crate, pinned ICU data and
pinned Rust dependencies into Cargo's build-output directory, then uses
upstream's source build and pinned native toolchains. Gitiles archive metadata
varies, so Rust dependencies are verified using a canonical path/content digest.
The recipe and engine patch invalidate the prepared-source cache. No research
directory, machine-specific path or separately installed V8 is required.

Prerequisites: Rust 1.91 or newer, babashka (`bb`), Git, curl, tar, Python 3 for
upstream V8 tools, a C/C++ toolchain and libclang for bindgen. On macOS install
Xcode command-line tools and LLVM, then set `LIBCLANG_PATH` to LLVM's `lib`
directory if bindgen cannot locate it. The initial source build takes several
minutes; ordinary compiler edits reuse it. Upstream downloads its matching
Chromium clang/linker and Rust toolchains. `--no-default-features` avoids V8 and
uses the existing QuickJS macro backend. WebAssembly targets do not build V8.

`upstream_build.rs` is the original build script with a public entry point,
three obsolete relative dependency watches removed, and release engine builds
for both Cargo profiles (`V8_FORCE_DEBUG=1` remains an opt-in). `build.rs` prepares
the source root before invoking it. Rust binding source is otherwise unchanged.
The local manifest declares the upstream Rust 1.91 toolchain minimum and keeps
license files in source packages. The root compiler's release profile uses thin
LTO and one code-generation unit, matching the measured build configuration.

To reuse a previously built **patched** engine, set both `RUSTY_V8_ARCHIVE` and
`RUSTY_V8_SRC_BINDING_PATH` to a copied archive/bindings pair from its `gn_out`
directory. Keep that pair outside the build output that Cargo will overwrite.
The ABI marker rejects official unpatched archives. No override is needed for a
normal build; the source recipe is the default.

Rust bindings: MIT, [LICENSE](LICENSE). V8: BSD and notices in
[LICENSE.v8](LICENSE.v8). Public headers used by light-cljs's C++ adapter live in
`../v8-headers/include` and match the patched engine. Source-build transitive
dependencies retain their upstream notices in the downloaded source tree.
