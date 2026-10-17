# Native V8 macro backend — integrated and validated

Clean native compilations now select V8 by default. Interactive sessions and
WebAssembly retain QuickJS; `--no-default-features` selects the QuickJS-only
native compiler. Both engines execute the same emitted macro bodies and runtime.
The compiler service interface is independent of either engine. Release builds
default to thin LTO and one code-generation unit, matching the measurements.

The pinned V8 source recipe builds the allocator-budget patch from verified
inputs. It restores the omitted ICU data and Rust dependencies, verifies the
latter by canonical path/content digest (Gitiles tar metadata varies), and uses
upstream's pinned native toolchains. A native ABI marker rejects an unpatched
engine archive at link time. The source build completed in 6m02s on this Mac;
the final recipe and setup failures are retained. The vendored bindings/headers
are about 4.6 MB; the optimized executable is about 65 MB. Build prerequisites
and optional reuse of matching patched artifacts are documented in `vendor/v8`.

Validation at normal concurrency:

- Default V8 backend: **1,442 passed, zero failed, 106 ignored**.
- QuickJS-only backend: **1,406 passed, zero failed, 106 ignored**.
- Strict Clippy for both configurations, formatting, and all three Node workflows.
- WebAssembly playground built and passed its execution checks; its dependency
  graph excludes V8. The native source build and ABI-rejection check passed.

A test-only numeric inspection method was gated to V8 tests after the QuickJS
suite exposed an unused-method warning; the subsequent QuickJS Clippy check
compiled all test targets without warnings. This does not change runtime code.

Sequential cold builds, **all experimental engine settings unset**:

| Build | Wall | User CPU | Peak RSS bytes |
| --- | ---: | ---: | ---: |
| light-cljs render | 3.68 s | 4.43 s | 482,934,784 |
| light-cljs main none | 22.62 s | 26.83 s | 971,407,360 |
| light-cljs main advanced | 28.23 s | 32.11 s | 1,151,631,360 |
| Shadow release | 59.12 s | 157.64 s | 3,901,341,696 |
| Shadow compile | 32.08 s | 91.69 s | 2,550,611,968 |

Main none takes **29.5% less wall time and 61.9% less RSS** than fresh Shadow dev.
Advanced takes 52.2% less wall time than Shadow release, with the previously
documented scope/optimizer differences: light-cljs advanced is not Closure
advanced, and Shadow's main build also includes the render/rasterizer/highlight
modules. All three light-cljs outputs match phase111 byte-for-byte and pass
explicit ESM syntax checks. The 996-file port fingerprint is unchanged.
Penpot itself has not been bundled and run in a browser.

The patch, exact environments, logs, source/artifact hashes, source recipe and
all five benchmark records accompany this file. A separate collection-store
specialization experiment follows; it is not included in these measurements.

After promotion, the main working tree was rebuilt with ordinary Cargo release
defaults (no profile overrides), reusing the exact source-built engine archive.
All 313 recorded source files match the validated candidate. Final cold runs
from `target/release/light-cljs` are retained under `production-bench/`:

| Build | Wall | Peak RSS bytes |
| --- | ---: | ---: |
| render | 3.73 s | 498,450,432 |
| main none | 21.97 s | 947,617,792 |
| main advanced | 28.24 s | 1,177,829,376 |

All three complete outputs match the candidate byte-for-byte and pass explicit
ESM syntax checks. Against the fresh Shadow runs above, production main uses
31.5% less wall time and 62.8% less RSS. The rejected phase113 experiment is not
included. The Penpot port is untouched.
