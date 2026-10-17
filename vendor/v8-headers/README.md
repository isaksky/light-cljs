# Headers for the V8 stack-limit shim

`include/` is an unmodified copy of the V8 headers shipped in rusty_v8 152.2.0
(V8 15.2.124.1). The C++ shim only exposes the public `Isolate::SetStackLimit`
method missing from the pinned Rust binding. It links against the same V8
archive as the Rust crate; these files do not build another engine.

The upstream BSD license is in `LICENSE`, obtained from
https://raw.githubusercontent.com/v8/v8/15.2.124.1/LICENSE.
Retain the original notices in each header. Updating the Rust V8 dependency
requires updating these headers and the matching pointer-compression define.
