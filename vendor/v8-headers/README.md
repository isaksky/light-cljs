# Headers for the V8 C++ shims

`include/` is an unmodified copy of the V8 headers shipped in rusty_v8 152.2.0
(V8 15.2.124.1). The C++ shims in `src/v8_*.cc` only use public V8 API the
pinned Rust binding lacks: `Isolate::SetStackLimit` and an
`ArrayBuffer::Allocator` subclass that charges the macro memory budget. They
link against the crate's stock prebuilt V8 archive; these files do not build
another engine.

The upstream BSD license is in `LICENSE`, obtained from
https://raw.githubusercontent.com/v8/v8/15.2.124.1/LICENSE.
Retain the original notices in each header. Updating the Rust V8 dependency
requires updating these headers and the matching pointer-compression define.
