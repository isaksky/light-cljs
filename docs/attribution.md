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

## Array push (clj-quickjs-155)

`tests/js_source.rs::array_push` adapts the append of 3 and 4 to `[1,2]`
from `research/quickjs/tests/test_builtin.js`, `test_array` (lines 220-222),
at revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The join assertion is replaced by direct element and length checks; the
QuickJS MIT copyright and permission notice is retained with the test
(`research/quickjs/LICENSE`). Other cases are synthetic fixtures written
for this runtime, not light-cljs output. `js_array_push` in
`research/quickjs/quickjs.c` at that revision was read as a semantic
reference; the Rust implementation was written afresh.

## OrdinaryToPrimitive (clj-quickjs-70)

`research/quickjs/quickjs.c`, `JS_ToPrimitiveFree`, `js_add_slow`,
`js_binary_arith_slow`, `js_relational_slow`, `js_eq_slow`,
`js_object_valueOf` and `js_object_toString`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04), were read
as semantic references. The Rust continuation and synthetic source tests
were written afresh; no C implementation or upstream test text was copied.

## Number and String wrappers (clj-quickjs-72)

`tests/js_source.rs::primitive_wrappers_convert_with_value_of` adapts the
three Number/String wrapper equality groups in `test_eq` from
`research/quickjs/tests/test_language.js`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The test refers to the full QuickJS copyright and MIT permission notice
on `loose_equality_with_a_boolean_left_operand` in the same file;
see `research/quickjs/LICENSE`. Other cases are synthetic fixtures written
for this runtime, not light-cljs output.

`js_number_constructor`, `js_string_constructor`, `js_thisNumberValue`,
`js_thisStringValue`, `js_number_valueOf`, `js_number_toString` and
`js_string_toString` in `research/quickjs/quickjs.c` at that revision were
read as semantic references. No upstream implementation code was copied.
