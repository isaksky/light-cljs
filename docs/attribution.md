# Attribution

## Regular expression literal parsing (clj-quickjs-55)

`src/frontend/tests.rs::regexp_literals` adapts the two default assignments
in `research/quickjs/tests/test_language.js::test_regexp_skip` to call
arguments and array elements, since destructuring assignment is deferred.
`docs/grammar.md` repeats those spellings. The QuickJS MIT copyright and
permission notice is retained with the test (`research/quickjs/LICENSE`).
Reference revision: `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`
(VERSION 2026-06-04). Other fixtures are synthetic, not light-cljs output.
`js_parse_regexp` and `js_parse_skip_parens_token` in
`research/quickjs/quickjs.c` at that revision were consulted as semantic
references; the Rust scanner and parser changes were written afresh.

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

## String code-unit access (clj-quickjs-143)

`tests/js_source.rs::string_code_unit_access` adapts the charAt,
charCodeAt, codePointAt, fromCharCode and fromCodePoint assertions from
`research/quickjs/tests/test_builtin.js`, `test_string`, and the complete
UTF-16 unit range checked by `rope_concat`, at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The latter uses a flat construction through `apply` and per-unit round trips
instead of a quadratic concatenation loop in this runtime's flat storage.
The QuickJS MIT copyright and permission notice is retained with the test
(`research/quickjs/LICENSE`). Other edge cases are synthetic fixtures,
not light-cljs output. `js_string_charAt`, `js_string_charCodeAt`,
`js_string_codePointAt`, `js_string_fromCharCode` and
`js_string_fromCodePoint` in `research/quickjs/quickjs.c` at the same
revision were consulted for semantics; the Rust implementation was written
afresh.

## String substring search (clj-quickjs-144)

`tests/js_source.rs::string_index_of_and_last_index_of` adapts all 47
`indexOf` / `lastIndexOf` assertions from `research/quickjs/tests/test_builtin.js`,
`test_string` lines 265-315, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
It refers to the full QuickJS copyright and MIT permission notice retained
on `string_code_unit_access` in the same file (`research/quickjs/LICENSE`).
Other search tests are synthetic fixtures, not light-cljs output.
`js_string_indexOf` in `research/quickjs/quickjs.c` at that revision was read
as a semantic reference. The incremental Rust KMP implementation was written
afresh; no upstream implementation code was copied.

## String extraction and combination (clj-quickjs-145)

`tests/js_source.rs::string_substring_slice_concat_pad` adapts the substring,
concat and infinite-target empty-filler padding assertions from `test_string`
in `research/quickjs/tests/test_builtin.js` (lines 249, 263, 341), revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The full QuickJS copyright and MIT permission notice is retained on
`string_code_unit_access` in the same test file (`research/quickjs/LICENSE`).
The remaining cases are synthetic fixtures, not light-cljs output.
`js_string_substring`, `js_string_slice`, `js_string_concat` and `js_string_pad`
in `research/quickjs/quickjs.c` at that revision were read as semantic
references. The resumable Rust implementation was written afresh.

## Number predicates and constants (clj-quickjs-147)

`tests/js_source.rs::number_predicates_and_constants` adapts the three
invalid Number string conversions from `research/quickjs/tests/test_builtin.js`,
`test_number`, revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`
(VERSION 2026-06-04). The full QuickJS copyright and MIT permission notice
is retained on `string_code_unit_access` in that file (see
`research/quickjs/LICENSE`). Other cases are synthetic fixtures written
for this runtime, not light-cljs output.

`js_global_isNaN`, `js_global_isFinite`, `js_number_isNaN`,
`js_number_isFinite`, `js_number_isInteger`, `js_number_isSafeInteger`
and `js_number_funcs` in `research/quickjs/quickjs.c` at the same revision
were read as semantic references. The Rust implementation was written afresh.

## Number prefix parsing (clj-quickjs-148)

`tests/js_source.rs::parse_int_and_parse_float` adapts every assertion in
`test_number` lines 365-380 of `research/quickjs/tests/test_builtin.js`,
including the three unary-plus StringToNumber regression rows, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The full QuickJS copyright and MIT permission notice is retained on
`string_code_unit_access` in that file (`research/quickjs/LICENSE`).
Other cases are synthetic fixtures, not light-cljs output. `js_parseInt`,
`js_parseFloat` and `js_atof` in `research/quickjs/quickjs.c` at that revision
were consulted for semantics. The Rust scanners were written afresh.

## Accessor properties (clj-quickjs-120)

`tests/js_source.rs::accessor_properties` adapts accessor read/write groups
from `research/quickjs/tests/test_builtin.js::test` and contextual get/set
keys from `research/quickjs/tests/test_language.js::test_object_literal`,
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The full QuickJS copyright/MIT permission notice on
`string_code_unit_access` in that file applies to these adapted cases too
(`research/quickjs/LICENSE`). Other new fixtures are synthetic and written
for this runtime, not light-cljs output. `JS_GetPropertyInternal`,
`JS_SetPropertyInternal`, `js_parse_object_literal`, `js_parse_property_name`
and `js_get_function_name` in `research/quickjs/quickjs.c` at that revision
serve as semantic references; no upstream C implementation was copied.

## Number radix formatting (clj-quickjs-149)

`tests/js_source.rs::number_to_string_with_radix` and the formatter unit test
adapt the four radix/decimal assertions in `test_number`, lines 385-386 and
399-400 of `research/quickjs/tests/test_builtin.js`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The full QuickJS copyright and MIT permission notice is retained on
`tests/js_source.rs::string_code_unit_access` (see `research/quickjs/LICENSE`).
Other tests are synthetic fixtures, not light-cljs output.
`js_number_toString` in `research/quickjs/quickjs.c` and `js_dtoa` in
`research/quickjs/dtoa.c` at that revision were consulted as semantic
references. The fixed-size exact-interval Rust formatter was written afresh;
no upstream implementation code was copied.

## Math numeric functions (clj-quickjs-152)

`tests/js_source.rs::math_basic_functions` adapts the floor, ceil, imul,
fround and hypot cases from `research/quickjs/tests/test_builtin.js`,
`test_math` (lines 347-359), revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The full QuickJS copyright and MIT permission notice is retained on
`string_code_unit_access` in that test file (`research/quickjs/LICENSE`).
Other cases are synthetic fixtures, not light-cljs output.
`js_math_hypot`, `js_math_fround`, `js_math_imul`, `js_math_funcs` and
`js_math_obj` in `research/quickjs/quickjs.c` at that revision were read as
semantic references. The Rust implementation was written afresh over the
existing realm and resumable native protocol, with scaled compensated
summation rather than the reference implementation's repeated hypot calls.

## Array join and toString (clj-quickjs-156)

`research/quickjs/quickjs.c`, `js_array_join` and `js_array_toString`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04), were read as
semantic references. The Rust continuations were written afresh. The basic
join/toString rows in `tests/js_source.rs::array_join_and_to_string` cover
`tests/test_builtin.js::test_array` at that revision; the full QuickJS MIT
copyright and permission notice is retained with that test. Other tests are
synthetic fixtures, not light-cljs output; no C implementation was copied.

## String splitting (clj-quickjs-157)

`tests/js_source.rs::string_split` adapts all 20 split assertions in
`research/quickjs/tests/test_builtin.js`, `test_string` lines 317-337,
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The full QuickJS copyright and MIT permission notice is retained on
`string_code_unit_access` in that file (`research/quickjs/LICENSE`). Other
split fixtures are synthetic, not light-cljs output. `js_string_split` in
`research/quickjs/quickjs.c` at that revision was consulted for semantics;
the resumable Rust implementation was written afresh.

## Error stack positions (clj-quickjs-59)

`tests/error_stacks.rs` adapts the `@` marker technique of `get_string_pos`
and position cases of `test_line_column_numbers` in
`research/quickjs/tests/test_builtin.js`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The QuickJS copyright and MIT permission notice are retained in that test.
The helper runs in Rust over synthetic JavaScript fixtures, not light-cljs
output; eval and NativeError instance checks are left to their own pellets.
`research/quickjs/quickjs.c`, `build_backtrace` and `get_line_col`, at the
same revision were read as semantic references. No C code was copied;
capture uses this runtime's explicit frames and immutable string storage.

## Math.pow (clj-quickjs-164)

`research/quickjs/quickjs.c`, `js_pow` and the `pow` registration in
`js_math_funcs`, revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`
(VERSION 2026-06-04), were read as semantic references. The existing Rust
exponentiation helper is reused by the new built-in. Tests are synthetic
fixtures, not light-cljs output; no upstream test text was copied.

## Array constructor and brand (clj-quickjs-153)

`research/quickjs/quickjs.c`, `js_array_constructor` and `js_array_isArray`,
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04),
were read for semantics; the Rust implementation was written afresh.
The first two rows in `tests/js_source.rs::array_constructor_and_is_array`
adapt `research/quickjs/tests/test_builtin.js::test_array` at that revision.
Its full copyright and MIT permission notice is retained on `array_push`
in that test file. Other fixtures are synthetic, not light-cljs output.
