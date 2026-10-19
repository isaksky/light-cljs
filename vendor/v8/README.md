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
directory if bindgen cannot locate it. The initial source build takes from several
minutes to hours under load, once per machine (see the cache below). Upstream downloads its matching
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
normal build: the machine-wide cache below, then the source recipe, is the default.

## Machine-wide engine cache

A source build runs once per machine for a given engine. `build.rs` (with the cache logic in `cache.rs`) keeps each
built archive and its generated `src_binding.rs` in a per-user cache and reuses
them from every checkout, worktree and clean target directory, so no gn, ninja,
source download or bindgen step runs on a hit. The default location is
`~/Library/Caches/light-cljs/v8` on macOS, `$XDG_CACHE_HOME/light-cljs/v8` (or
`~/.cache/light-cljs/v8`) on Linux and `%LOCALAPPDATA%\light-cljs\v8` on
Windows. No setup is needed.

An entry is keyed by the full text of `prepare.bb` (source pins), the engine
patch and `upstream_build.rs` (GN and bindgen logic), the crate version, target
and host triples, the Cargo profile, the enabled crate features, address
sanitizer use and the environment upstream turns into GN or bindgen arguments
(`V8_FORCE_DEBUG`, `GN_ARGS`, `EXTRA_GN_ARGS`, `DISABLE_CLANG`,
`CLANG_BASE_PATH`, `RUSTY_V8_MUSL_SYSROOT`, `RUSTY_V8_GLIBC_PREFIX`,
`RUSTY_V8_BINDGEN_RESOURCE_DIR`, and bindgen's `BINDGEN_EXTRA_CLANG_ARGS` with its
target-specific forms), where an unset variable differs from an empty one. Both profiles build a release engine, but
their archives differ in the build-directory names embedded in source paths,
so the debug and release profiles keep separate entries. The directory name
carries a hash of this material and the entry records it in full; an entry is
used only when the recorded material is identical. Changing any key input
misses and builds from source. Bump `CACHE_SCHEMA` in `cache.rs` when the
wrapper itself changes what a source build produces. Tool locations (`GN`,
`NINJA`, `PYTHON`, compiler caches, `LIBCLANG_PATH`) and the installed SDK are
not keyed; after a toolchain change that should rebuild the engine, delete the
cache directory.

A build holds a per-entry lock, so a second build of the same engine waits for
the first and then reuses its result. A source build publishes by copying the
pair into a private `.tmp-` directory, flushing it and renaming it into place,
so readers never see a partial entry. An interrupted publish leaves only the
temporary directory, which no lookup reads and a later publish removes after a
day. An entry whose recorded file sizes do not match is ignored and replaced.
light-cljs's `tests/v8_cache.rs` checks that a source build publishes its pair from
`gn_out` and that a fresh target directory then reuses it without building.

Settings, read by the build script:

- `LIGHT_CLJS_V8_CACHE=require` fails the build on a miss instead of building
  from source, also when no cache directory can be found, for automated runs
  that must never spend hours on V8.
  `off` always builds from source and publishes nothing. `on` is the default.
- `LIGHT_CLJS_V8_CACHE_DIR=<absolute path>` moves the cache.
- `LIGHT_CLJS_V8_CACHE_SEED=<dir>` publishes, on a miss, the
  `librusty_v8.a`/`src_binding.rs` pair in `<dir>` as this build's entry
  instead of building. Use it only for a pair built from these same sources,
  configuration and Cargo profile, for example copies of `gn_out/obj/librusty_v8.a`
  and `gn_out/src_binding.rs` from an earlier source build of that profile.

Explicit `RUSTY_V8_ARCHIVE`/`RUSTY_V8_SRC_BINDING_PATH` pairs bypass the cache.

CI runners start without this cache. The `v8-engine` job in
`.github/workflows/conformance.yml` restores the cache directory from the Actions
cache under a key hashed from the root and engine `Cargo.toml`, `build.rs`, `cache.rs` and the
three key files above (falling back to the newest engine), checks the crate with
`LIGHT_CLJS_V8_CACHE=require` and, only on a miss, builds from source.
`bb vendor/v8/ci-cache.bb CACHE-DIR TARGET-DIR` then deletes every entry the build
did not use (it reads the entry named in the build script's output). The job
saves the directory for later runs and uploads it as an artifact, which the jobs
that link V8 in the same run download before building with `LIGHT_CLJS_V8_CACHE=require`.

Rust bindings: MIT, [LICENSE](LICENSE). V8: BSD and notices in
[LICENSE.v8](LICENSE.v8). Public headers used by light-cljs's C++ adapter live in
`../v8-headers/include` and match the patched engine. Source-build transitive
dependencies retain their upstream notices in the downloaded source tree.
