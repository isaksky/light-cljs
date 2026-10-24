# Attribution

## Array length changes (clj-quickjs-154)

`tests/js_source.rs::array_length_assignment` adapts the truncation,
index-write, extension and non-configurable-element groups from
`research/quickjs/tests/test_builtin.js`, `test_array`, at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The QuickJS MIT copyright and permission notice is retained with the test;
see `research/quickjs/LICENSE`. The fixtures are synthetic, not light-cljs
output. Other edge cases and the GC fixture were written for this runtime.

`research/quickjs/quickjs.c`, `set_array_length`, `JS_DefineProperty` and
`JS_ToArrayLengthFree` at the same revision were read as semantic references.
The Rust storage and VM implementation were written afresh; no C code was
copied or wrapped.
