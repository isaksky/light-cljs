# Attribution

## Non-constructor function rejection (clj-quickjs-135)

The generator row in `tests/js_source.rs::non_constructors_reject_new` adapts
`research/quickjs/tests/test_language.js::test_constructor` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The QuickJS MIT copyright and permission notice accompanies the test.
`research/quickjs/quickjs.c`, `JS_IsConstructor`, `JS_CallConstructorInternal`
and `JS_ThrowTypeErrorNotAConstructor` were semantic references; the Rust code
was written afresh. The source-label diagnostic policy, documented in
`grammar.md`, intentionally differs from QuickJS's function-name lookup.
Other new test cases are synthetic fixtures, not light-cljs output.

## Optional chaining (clj-quickjs-134)

Semantic references: `optional_chain_test` and `js_parse_postfix_expr` in
`research/quickjs/quickjs.c`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The delete and parenthesized
receiver rows of `tests/js_source.rs::optional_chaining` adapt
`tests/test_language.js::test_optional_chaining` at that revision. The full
QuickJS copyright and MIT license notice is retained beside
`object_literals_and_named_properties` in that test file. Remaining fixtures
are synthetic, not light-cljs output. No QuickJS implementation code was copied.

## Generator and async syntax (clj-quickjs-54)

`src/frontend/tests.rs::generator_and_async_syntax` adapts the two function
bodies in `test_parse_semicolon` and the two async arrow ASI cases in
`test_parse_arrow_function`, from `research/quickjs/tests/test_language.js`
at revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The bodies use `let` instead of `var` and assert parsing plus explicit
compilation rejection, since execution is deferred. The QuickJS MIT
copyright and permission notice is retained with that test; see
`research/quickjs/LICENSE`. Other new fixtures are synthetic, not light-cljs
output. `js_parse_unary`, `js_parse_assign_expr2` and
`js_parse_function_decl2` in `research/quickjs/quickjs.c` at that revision
were semantic references; the Rust implementation was written afresh.

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

## Regular expression pattern early errors (clj-quickjs-56)

`src/frontend/tests.rs::regexp_pattern_early_errors` adapts the `/aaa]/u`
position case from `research/quickjs/tests/test_builtin.js`,
`test_line_column_numbers`, at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
`docs/grammar.md` repeats that spelling. The QuickJS MIT copyright and
permission notice is retained with the test; see `research/quickjs/LICENSE`.
Other cases are synthetic fixtures, not light-cljs output.
`research/quickjs/quickjs.c::js_parse_regexp` was consulted as a semantic
reference; validation reuses the existing Rust pattern parser.
`src/regexp/script_names.rs` is generated from the Unicode 17
`unicode_script_name_table` in `research/quickjs/libunicode-table.h` at the
same revision through `scripts/quickjs-data/generate.sh`. It retains the
QuickJS and Unicode notices; the complete MIT notice is in
`src/intl/data/QUICKJS-LICENSE` and Unicode License v3 in `docs/UNICODE-LICENSE`.
Only exact property-alias recognition is added; no Script matching code is
adapted. The seven string property names follow the ECMAScript property list.

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

## JSON.parse input error stacks (clj-quickjs-162)

`tests/js_source.rs::json_parse_error_positions` adapts the `@` marker
technique from `get_string_pos`/`assert_json_error` and the two invalid JSON
rows in `research/quickjs/tests/test_builtin.js::test_json`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The copyright/MIT notice at `string_code_unit_access` in that test file
applies to these adapted rows as well. All fixtures are synthetic JavaScript,
not light-cljs output. `json_next_token`, `json_parse_string`,
`json_parse_number`, `json_parse_value`, `js_parse_error_pos`, `get_line_col`,
`JS_ThrowError2` and `build_backtrace` in `research/quickjs/quickjs.c` at the
same revision were consulted for token positions, messages and stack format;
no C implementation was copied. The requested UTF-16/CR/EOF diagnostic
convention is documented in grammar/architecture where it differs from the
pinned helper.

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

## Math.sqrt and Math.log2 (clj-quickjs-205)

Consulted `js_math_funcs` in `research/quickjs/quickjs.c`,
`research/quickjs/tests/test_bigint.js` (`test_bigint1`, `compute_pi`), and
`research/quickjs/examples/pi_bigint.js` (`main`) at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04) as semantic
references for unary Math methods and pi digit sizing. No implementation or
test body was copied; the new source fixtures are synthetic, not light-cljs
output. Numeric work uses Rust binary64 operations and exponent inspection.

## JSON.parse (clj-quickjs-159)

`tests/js_source.rs::json_parse` adapts the basic object row from
`research/quickjs/tests/test_builtin.js::test_json`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The full QuickJS copyright and MIT permission notice from
`research/quickjs/LICENSE` is retained at `string_code_unit_access` in the
same test file. Other cases are synthetic fixtures, not light-cljs output.
`json_parse_value` and `json_next_token` in `research/quickjs/quickjs.c` at
that revision were consulted as semantic references. The iterative Rust
parser and continuation were written afresh; no C implementation was copied.

## Symbol primitives (clj-quickjs-127)

`tests/js_source.rs::symbol_values` adapts the initial symbol-key property and
identity equality cases from `research/quickjs/tests/test_builtin.js`,
`test_symbol`, revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`
(VERSION 2026-06-04). The full MIT copyright and permission notice is retained
in that test file on `string_code_unit_access`. Other cases are synthetic
fixtures written for this runtime, not light-cljs output.
`research/quickjs/quickjs.c`, `js_symbol_constructor`, `JS_NewSymbolFromAtom`
and `JS_ToPropertyKey` at that revision were semantic references. The Rust
representation, key storage and continuations were written afresh.

## JSON.stringify (clj-quickjs-160)

`tests/js_source.rs::json_stringify` adapts the compact JSON round trip from
`research/quickjs/tests/test_builtin.js::test_json` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The full QuickJS copyright and MIT permission notice is retained at
`string_code_unit_access` in the same test file. Remaining cases are synthetic
fixtures, not light-cljs output. `js_json_check`, `js_json_to_str` and
`JS_JSONStringify` in `research/quickjs/quickjs.c` at that revision were read
as semantic references. The resumable Rust serializer was written afresh;
no upstream C implementation was copied.

## JSON.stringify indentation (clj-quickjs-161)

`tests/js_source.rs::json_stringify_indentation` adapts the nested indentation
row from `research/quickjs/tests/test_builtin.js::test_json` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04), using
both escaped ordinary strings and the upstream template-literal layout.
The full QuickJS copyright and MIT permission notice is retained at
`string_code_unit_access` in that test file. Other cases are synthetic
fixtures, not light-cljs output. `JS_JSONStringify` and `js_json_to_str`
in `research/quickjs/quickjs.c` at that revision were semantic references;
the resumable Rust gap conversion and indentation stages were written afresh.

## Symbol registry (clj-quickjs-128)

`tests/js_source.rs::symbol_registry` adapts the registry identity and key
lookup rows from `research/quickjs/tests/test_builtin.js::test_symbol`,
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The full QuickJS copyright and MIT permission notice is retained on
`string_code_unit_access` in the same test file. Other cases are synthetic
fixtures, not light-cljs output. `js_symbol_for` and `js_symbol_keyFor` in
`research/quickjs/quickjs.c` at that revision were semantic references;
the runtime-owned Rust registry and continuation were written afresh.

## JavaScript Map tests (clj-quickjs-169, clj-quickjs-170)

`tests/js_source.rs::map_number_and_object_keys` adapts the integer-key,
-2147483648 arithmetic-equivalence and 1000 object-identity cases in
`research/quickjs/tests/test_builtin.js::test_map` and `test_map1`, at
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
Fixtures are synthetic, not light-cljs output. The implementation was
written afresh; `map_normalize_key`, `js_map_constructor`, `js_map_set`,
`js_map_get`, `js_map_has`, `js_map_delete`, `js_map_clear` and
`js_map_get_size` in `research/quickjs/quickjs.c` were semantic references.
`tests/js_source.rs::map_for_each_live_iteration` also adapts the 1000-entry
object-key callback deletion loop from `test_map1` at the same revision.
`js_map_forEach` in `research/quickjs/quickjs.c` was a semantic reference for
live visitation; the Rust resumable state machine was written afresh.

The adapted tests retain this notice from `research/quickjs/LICENSE`:

Copyright (c) 2017-2021 Fabrice Bellard
Copyright (c) 2017-2021 Charlie Gordon

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.

`tests/js_source.rs::map_bigint_keys` and
`map_bigint_for_each_deletes_all_keys` (clj-quickjs-171) adapt the
BigInt cases in `research/quickjs/tests/test_builtin.js::test_map` (lines
899-905) and `test_map1` (line 845), at the same revision above. Fresh-key
lookups, updates and direct deletion extend the original fixture. The
upstream callback/deletion loops cover both 1000-key BigInt ranges.
`research/quickjs/quickjs.c::map_hash_key` (line 54339) was
consulted as a semantic reference; no implementation was copied. These
synthetic fixtures retain the QuickJS copyright and MIT notice above.

## Rope strings (clj-quickjs-146)

`tests/fixtures/js/quickjs_rope.js` retains `rope_concat` and `test_rope`
unchanged from `research/quickjs/tests/test_builtin.js`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
A minimal asserting helper and invocation were added; the QuickJS MIT
copyright and permission notice from `research/quickjs/LICENSE` is retained
in the fixture. This is a synthetic fixture, not light-cljs output.
The Rust representation and tests were written afresh, without copying C.

## Regular expression pattern parser (clj-quickjs-186)

`src/regexp/parse.rs::tests::annex_b_and_escapes` adapts the parse-level
patterns from `research/quickjs/tests/test_builtin.js::test_regexp`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The full QuickJS copyright and MIT permission notice is retained above that
test. Other tests are synthetic fixtures, not light-cljs output.
`get_class_atom`, `re_parse_term`, `re_parse_captures`, `re_parse_char_class`,
`re_parse_alternative`, `re_parse_disjunction` and `lre_compile` in
`research/quickjs/libregexp.c` at that revision were consulted as semantic
references. No upstream implementation was copied or linked.

## BigInt primitives (clj-quickjs-196)

Semantic references: `research/quickjs/quickjs.c` (`js_bigint_new`,
`js_bigint_normalize`, `js_bigint_from_string`, `js_strict_eq2`) and
`research/quickjs/tests/test_bigint.js::test_bigint1`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The arbitrary-precision literal examples use the upstream mathematical
constants; Rust code and synthetic source test bodies were written afresh,
without copying QuickJS code or test bodies. The 1 Mbit size policy follows
the user decision recorded in `docs/architecture.md`.

## BigInt comparisons (clj-quickjs-197)

Semantic references: `research/quickjs/quickjs.c` (`js_compare_bigint`,
`js_relational_slow`, `js_eq_slow`) at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
Rust comparison code was written afresh. `bigint_relational_and_loose_equality`
in `tests/js_source.rs` adapts the ten-comparison matrices from
`research/quickjs/tests/test_bigint.js::test_less` and `test_eq` and the
large-number equality cases of `test_bigint3`; its full QuickJS copyright
and MIT permission notice is retained on that test. Additional signed,
coercion and suspension fixtures are synthetic, not light-cljs output.

## Intl locale data (clj-quickjs-209)

`src/intl/data/*.rs` is generated/repacked from
`research/quickjs/intl/locale-data.c` and `intl/intl_private.h`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04), using
`scripts/quickjs-data/generate.sh`, `export.pl` and `repack-intl.pl`.
Input and tool hashes are retained in `src/intl/data/provenance.txt`;
`src/intl/data/QUICKJS-LICENSE` reproduces the QuickJS MIT notice.
The source labels its tables as auto-generated from CLDR, without a verified
CLDR release. The CLDR/Unicode data notice is Unicode License v3, reproduced
in [UNICODE-LICENSE](UNICODE-LICENSE) from the
[Unicode license](https://www.unicode.org/license.txt).

`intl.c::js_intl_find_locale_data`, `gen-locale-data.js`, `tests/test_intl.js`
and the service source files listed in the architecture coverage table were
read as semantic/data-coverage references at the same revision. Runtime code
and new source tests are written afresh (synthetic fixtures, not light-cljs
output); no QuickJS runtime is linked or wrapped. Identifiers retain their
existing Unicode 16.0.0 policy; imported snapshot Unicode decoders and full
collation/break tables are not introduced by this pellet.

## Regular expression matching (clj-quickjs-187)

`src/regexp/matcher/tests.rs::upstream_rows` adapts the matching assertions
from `research/quickjs/tests/test_builtin.js::test_regexp` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The full QuickJS copyright and MIT permission notice precedes that test.
Other matcher tests are synthetic fixtures, not light-cljs output.
`lre_exec`, `lre_exec_backtrack` and `re_parse_term` in
`research/quickjs/libregexp.c` were consulted as semantic references at
the same revision. The Rust compiler and matcher were written independently;
no upstream implementation was copied or linked.

## BigInt arithmetic (clj-quickjs-198)

Semantic references: `research/quickjs/quickjs.c` (`js_bigint_add`,
`js_bigint_mul`, `js_unary_arith_slow`, `js_binary_arith_slow`, `js_add_slow`),
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
Rust arithmetic was written afresh using the existing in-tree limb type.
`tests/js_source.rs::bigint_additive_and_multiplicative_operators` adapts the
`bigint_pow` loop and mathematical constant from
`research/quickjs/tests/test_bigint.js`; its full copyright and MIT permission
notice is retained on the test. Other arithmetic fixtures are synthetic,
not light-cljs output.

## Locale identifiers (clj-quickjs-210)

`src/intl/tag.rs`, `src/intl/locale.rs`, and `src/vm/native/intl.rs` are fresh Rust
implementations using `research/quickjs/intl/intl.c` (`is_wellformed_language_tag`,
`canonicalize_locale`, `js_canonicalizeLocaleList_core`, `js_intl_getCanonicalLocales`)
and `research/quickjs/intl/locale.c` (constructor, options, keyword parsing and
component getters) as semantic references at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. New tests are synthetic fixtures,
including round-trip inputs from the pellet's acceptance list corresponding to
`research/quickjs/tests/test_intl.js::test_locale`; no upstream test text is
copied. The QuickJS MIT notice is retained in `src/intl/data/QUICKJS-LICENSE`.
No new generated tables or CLDR source are introduced.

## BigInt division and exponentiation (clj-quickjs-199)

Semantic references: `research/quickjs/quickjs.c` (`js_bigint_divrem`,
`js_bigint_pow`, `js_binary_arith_slow`), revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The Rust normalized limb division and exponentiation were written afresh.
`tests/js_source.rs::bigint_division_remainder_and_exponentiation` adapts the
eight division/remainder/power assertions of
`research/quickjs/tests/test_bigint.js::test_bigint3`; its full QuickJS
copyright and MIT permission notice is retained on the test. Other new tests
are synthetic fixtures, not light-cljs output.

## Integer typed arrays (clj-quickjs-166)

`tests/js_source.rs::integer_typed_array_elements`,
`typed_array_views_share_buffer` and `typed_array_after_transfer` adapt the
integer element, buffer-view and detached-read rows in
`research/quickjs/tests/test_builtin.js::test_typed_array`, replacing deferred
join/toString checks with individual element assertions. The QuickJS MIT
copyright/permission notice is retained alongside these fixtures.
`js_typed_array_constructor`, `typed_array_init`, `JS_ToUint8ClampFree`,
`js_TA_get_int8` through `js_TA_get_uint32` and the typed-array getters in
`research/quickjs/quickjs.c` were semantic references; the Rust representation
and VM integration were written afresh. Reference revision:
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04), license:
`research/quickjs/LICENSE`. Other fixtures are synthetic, not light-cljs output.

## BigInt bitwise and shift operators (clj-quickjs-200)

Semantic references: `research/quickjs/quickjs.c` (`js_bigint_logic`,
`js_bigint_not`, `js_bigint_shl`, `js_bigint_shr`, `js_binary_logic_slow`,
`js_shr_slow`), revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`
(VERSION 2026-06-04). The Rust limb algorithms were written afresh.
`tests/js_source.rs::bigint_bitwise_and_shift_operators` adapts the bitwise
and shift assertions of `research/quickjs/tests/test_bigint.js::test_bigint1`
and `test_bigint3`, comparing string-conversion cases by exact integer value.
Its full QuickJS copyright and MIT permission notice is retained on the test.
Other new cases are synthetic fixtures, not light-cljs output.

## Locale likely subtags (clj-quickjs-211)

`src/intl/likely.rs` and the leaf methods in `src/intl/locale.rs` were written
afresh using `research/quickjs/intl/locale.c` (`likely_subtags_lookup`,
`locale_add_likely_subtags`, `locale_minimize_base`, `js_locale_maximize`,
`js_locale_minimize`) as semantic references at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The required nine maximize/five minimize rows in the pure unit test
`likely_subtags_add_and_remove` and source test `intl_locale_maximize_minimize`
adapt `research/quickjs/tests/test_intl.js::test_locale` at that revision. The
QuickJS copyright and MIT permission notice precedes each test; additional
fixtures are synthetic, not light-cljs output. The unchanged supplemental data
retains its QuickJS and Unicode notices described above.

## Date UTC time-value core (clj-quickjs-179)

`src/vm/date.rs::tests::date_time_arithmetic` and
`tests/js_source.rs::date_utc_and_time_value` adapt the Date.UTC rows in
`research/quickjs/tests/test_builtin.js::test_date` (lines 683–712), at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The precision rows are identified there as test262 fp-evaluation-order cases.
The QuickJS copyright and MIT permission notice from `research/quickjs/LICENSE`
is retained with both tests. All fixtures are synthetic, not light-cljs output.
`research/quickjs/quickjs.c` functions `time_clip`, `set_date_fields`,
`set_date_fields_checked`, `js_date_constructor`, `js_Date_UTC`,
`js_date_getTime` and `js_date_setTime` were semantic references at that
revision; the Rust implementation and remaining tests were written afresh.

## Date UTC ISO formatting (clj-quickjs-180)

`tests/js_source.rs::date_to_iso_string` adapts the 2017 timestamp/ISO row
from `research/quickjs/tests/test_builtin.js::test_date` (line 661), revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The QuickJS copyright and MIT permission notice from `research/quickjs/LICENSE`
is retained on that test. Other fixtures are synthetic, not light-cljs output.
`research/quickjs/quickjs.c::get_date_fields` and `get_date_string` were
semantic references at that revision; the integer decomposition, formatter
and remaining tests were written afresh in Rust.

## Floating-point typed arrays (clj-quickjs-167)

`tests/js_source.rs::float_typed_array_byte_layout` adapts the shared-buffer
sequence in `research/quickjs/tests/test_builtin.js::test_typed_array`
(lines 484-506), asserting individual little-endian bytes instead of the
deferred typed-array `toString` method. The full QuickJS copyright/MIT notice
is retained on the test. Other fixtures are synthetic, not light-cljs output.
Semantic references: `js_TA_get_float16`, `js_TA_get_float32` and
`js_TA_get_float64` in `research/quickjs/quickjs.c`, and `tofp16`/`fromfp16` in
`research/quickjs/cutils.h`, at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The Rust bit conversion and rounding implementation was written afresh;
no upstream conversion code was copied. License: `research/quickjs/LICENSE`.

## RegExp objects (clj-quickjs-188)

`tests/fixtures/js/regexp_objects.js` adapts the non-i/u/v/d rows of
`research/quickjs/tests/test_builtin.js::test_regexp`, lines 717–766, at
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The full QuickJS copyright and MIT license notice precedes the fixture.
The eval raw-NUL row uses RegExp construction and an actual-NUL literal
because eval remains unavailable. Additional assertions are synthetic.
`js_regexp_constructor`, `js_regexp_get_source`, `js_regexp_get_flag`,
`js_regexp_get_flags`, `js_regexp_exec`, `js_regexp_test` and
`js_regexp_toString` in `research/quickjs/quickjs.c` at that revision were
consulted as semantic references. Object/native Rust code is independently
written and no QuickJS implementation is linked or copied.

## BigInt function and StringToBigInt (clj-quickjs-201)

Semantic references: `research/quickjs/quickjs.c` (`JS_ToBigIntCtorFree`,
`js_bigint_constructor`, `JS_StringToBigInt`, `js_bigint_from_string`,
`js_bigint_from_float64`, `js_bigint_funcs`, `js_bigint_proto_funcs`) and
`research/quickjs/tests/test_bigint.js::test_bigint2`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The implementation and synthetic
source tests are written fresh; no QuickJS source or fixture is copied.

## Date UTC field access (clj-quickjs-181)

`tests/js_source.rs::date_utc_fields` adapts the `setUTCHours(18, 10, 11)`
timestamp/ISO row from `research/quickjs/tests/test_builtin.js::test_date`
(lines 660–663), revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`
(VERSION 2026-06-04). The QuickJS copyright and MIT permission notice from
`research/quickjs/LICENSE` is retained on that test. Other fixtures are
synthetic, not light-cljs output. `research/quickjs/quickjs.c::get_date_field`,
`set_date_field` and `set_date_fields` were semantic references at that revision;
the table-driven Rust methods and continuation extension were written afresh.

## Typed-array sources and methods (clj-quickjs-168)

`tests/js_source.rs::typed_array_methods` adapts the join, clamped toString,
shared-buffer layout, array construction, set and fill rows in
`research/quickjs/tests/test_builtin.js::test_typed_array`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The QuickJS MIT copyright and permission notice is retained beside that test;
see `research/quickjs/LICENSE`. Fixtures are synthetic, not light-cljs output.
`js_typed_array_constructor`, `js_typed_array_set_internal`,
`js_typed_array_fill` and `js_typed_array_join` in `research/quickjs/quickjs.c`
at that revision were semantic references. The Rust continuation and tests
for overlap, resumability and edge cases were written afresh.

## String replacement (clj-quickjs-189)

`js_string_GetSubstitution`, `js_string_replace` and
`js_regexp_Symbol_replace` in `research/quickjs/quickjs.c`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04), were read
as semantic references for dollar substitutions, empty-match progress,
lastIndex handling and callback ordering. The Rust continuation and
substitution code was written independently; no C implementation was copied
or linked. Replacement tests are synthetic fixtures, not light-cljs output
or adapted QuickJS test text. Upstream license: `research/quickjs/LICENSE`.

## BigInt string formatting (clj-quickjs-202)

`tests/js_source.rs::bigint_to_string_with_radix` adapts three radix assertions
from `research/quickjs/tests/test_bigint.js::test_bigint3`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The QuickJS MIT copyright and permission notice is retained with the test;
see `research/quickjs/LICENSE`. Other fixtures are synthetic, not light-cljs
output. `js_thisBigIntValue`, `js_bigint_toString`, `js_bigint_valueOf` and
`js_bigint_to_string1` in `research/quickjs/quickjs.c` at that revision were
semantic references; the Rust chunked formatter and native continuation
changes were written afresh, without adapting upstream implementation code.

## Unicode RegExp matching (clj-quickjs-190)

`tests/fixtures/js/regexp_unicode.js`, run by
`tests/js_source.rs::regexp_unicode_mode`, adapts the two lastIndex rows
from `research/quickjs/tests/test_builtin.js::test_regexp` (lines 792–801),
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The fixture retains QuickJS's copyright and MIT permission notice from
`research/quickjs/LICENSE`; its other rows are synthetic, not light-cljs output.
`research/quickjs/libregexp.c::lre_exec` and
`research/quickjs/quickjs.c::js_regexp_exec` at that revision were read as
semantic references for starting offsets and UTF-16 capture positions.
The Rust code-point reader, matching changes and other tests were written
independently; no C engine implementation was copied or linked.

## Promise reactions (clj-quickjs-246)

`tests/promises.rs` adapts the rejection-counter case from
`research/quickjs/tests/test_std.js`, `test_async_promise_rejection`, at
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The timer uses the optional
simulated-host bindings; the QuickJS LICENSE copyright and permission notice
is retained in the test file. The Rust implementation is written fresh, consulting the
Promise routines in `research/quickjs/quickjs.c` listed in architecture.md.

## Date host rule tables (clj-quickjs-182)

`src/host/timezone/tables.rs` contains timezone types and immutable values from
`research/quickjs/temporal/tzdata_generated.c` and `temporal/timezone.h` at
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, exported by the existing
`scripts/quickjs-data` pipeline and repacked by `repack-timezone.pl`. The full
QuickJS notice accompanies it in `src/host/timezone/QUICKJS-LICENSE`, provenance
and input hashes in `provenance.txt`; the Unicode data notice is retained in
`docs/UNICODE-LICENSE`. Rule lookup, Date conversions and synthetic source
tests are fresh Rust/JavaScript implementations; no QuickJS engine code is linked.

## BigInt to Number (clj-quickjs-203)

`tests/js_source.rs::bigint_to_number_conversion` adapts the two Number rows
from `research/quickjs/tests/test_bigint.js::test_bigint3`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The QuickJS MIT notice is retained beside the test; see
`research/quickjs/LICENSE`. Remaining fixtures are synthetic, not light-cljs
output. `js_bigint_to_float64` and `js_number_constructor` in
`research/quickjs/quickjs.c` at that revision were semantic references.
The Rust sign/magnitude rounding implementation was written independently.

## Decimal NumberFormat (clj-quickjs-213)

Semantic references: `research/quickjs/intl/numberformat.c`
(`intl_mv_init_from_value`, fraction/significant rounding, `add_grouping`,
constructor, bound getter, resolvedOptions) and decimal rows in
`research/quickjs/tests/test_intl.js::test_numberFormat`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The Rust implementation is written afresh and consumes the already attributed
pinned locale tables. Numbering-system names and digit mappings are extracted
from `research/quickjs/intl/intl.c` through the existing exporter, with complete
C/Rust equality verification, repeatability, edge cases and pinned input hashes.
The decimal integration examples adapt the upstream test expectations and retain the QuickJS MIT notice
beside the test. No C engine, ICU, new dataset or crate dependency is used.

## Date non-locale formatting (clj-quickjs-183)

`research/quickjs/quickjs.c::get_date_fields`, `get_date_string`,
`getTimezoneOffset`, `js_date_constructor`, and the Date prototype registration
table at revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` were semantic
references for local/UTC text, minute-offset truncation, the UTC alias, and
function-call behavior. The Rust formatter is independently written; the new
source fixtures are synthetic, not light-cljs output, and do not copy upstream
test text. No additional data or dependencies were imported.

## Unicode RegExp simple folding (clj-quickjs-191)

`tests/fixtures/js/regexp_case_fold.js` adapts the two complemented-class
replacement rows from `research/quickjs/tests/test_builtin.js::test_regexp`
(lines 775–776), revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`.
The fixture retains the QuickJS MIT notice. `libregexp.c::lre_canonicalize`
usage and class closure were consulted for semantics.

`src/regexp/case_fold_data.rs` and `src/frontend/unicode_ident.rs` derive from
`research/quickjs/libunicode-table.h`, expanded offline using the pinned
`libunicode.c`/`libunicode.h` decoder at that revision through the existing
`scripts/quickjs-data/` exporter and `repack-unicode.pl`. Copyright (c)
2017–2018 Fabrice Bellard; the full MIT notice is retained in
`src/intl/data/QUICKJS-LICENSE`. The runtime matcher is in-tree Rust.

The outputs are checked across the complete code-point domain against Unicode
17.0.0 [CaseFolding.txt](https://www.unicode.org/Public/17.0.0/ucd/CaseFolding.txt)
and [DerivedCoreProperties.txt](https://www.unicode.org/Public/17.0.0/ucd/DerivedCoreProperties.txt),
with hashes in the repacker and `src/regexp/case_fold_provenance.txt`.
The QuickJS FB05/FB06 representative is reversed to the normative C/S target.
The Unicode License v3 notice is reproduced in `docs/UNICODE-LICENSE`.

## BigInt width wrapping (clj-quickjs-204)

`tests/js_source.rs::bigint_as_int_n_and_as_uint_n` adapts all 19 assertions in
`research/quickjs/tests/test_bigint.js::test_bigint_asintn`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The QuickJS copyright and MIT permission notice is retained beside the test;
see `research/quickjs/LICENSE`. Additional fixtures are synthetic, not
light-cljs output. `js_bigint_asUintN` and `js_bigint_funcs` in
`research/quickjs/quickjs.c` at that revision were semantic references.
The Rust wrapping implementation was written independently using our existing
canonical sign/magnitude and two's-complement helpers.

### Intl.PluralRules and shared service helpers (clj-quickjs-212)

Semantic references: `research/quickjs/intl/pluralrules.c`, `intl/intl.c` and
`tests/test_intl.js` at QuickJS revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`.
The implementation and source tests are independently written Rust/synthetic
fixtures. The six integer selection examples agree with `test_pluralRules`.
The pinned selector's rounding and ordinal omissions are superseded by the
pellet's explicit requirements; no generated snapshot tables were changed.

The finite CLDR predicate families in `src/intl/plural_rules.rs` were checked
against Unicode CLDR release-48
[cardinal rules](https://raw.githubusercontent.com/unicode-org/cldr/release-48/common/supplemental/plurals.xml)
and [ordinal rules](https://raw.githubusercontent.com/unicode-org/cldr/release-48/common/supplemental/ordinals.xml).
Copyright © 1991-2025 Unicode, Inc.; SPDX-License-Identifier: Unicode-3.0;
the applicable notice is reproduced in [UNICODE-LICENSE](UNICODE-LICENSE).
This separately identified rule reference does not assert a CLDR release for
the QuickJS locale snapshot. No C/C++ code or external runtime is linked.

## Unicode RegExp property escapes (clj-quickjs-192)

`tests/fixtures/js/regexp_properties.js` adapts the five property replacement
rows from `research/quickjs/tests/test_builtin.js::test_regexp` (lines 769–774)
at revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` and retains the MIT
notice. `libunicode.c::unicode_prop`, `unicode_general_category` and the
NUL-separated alias pools in `libunicode-table.h` at that revision supply
only offline reference decoding/data for the existing verified exporter.
`src/regexp/property_data.rs` retains QuickJS and Unicode notices; hashes and
regeneration are shared with folding/identifiers in
`src/regexp/case_fold_provenance.txt` and `scripts/quickjs-data/README.md`.
The independently pinned Unicode 17.0.0 UCD files are covered by
`docs/UNICODE-LICENSE`. ECMAScript property spellings follow
https://tc39.es/ecma262/multipage/text-processing.html#table-binary-unicode-properties.

## Percent and currency NumberFormat (clj-quickjs-214)

Semantic references: `research/quickjs/intl/numberformat.c` (constructor option
order, fraction defaults, percent scaling and pattern substitution),
`research/quickjs/intl/intl.c` (`js_intl_get_currency_digits` and
`js_intl_get_currency_symbol`), and percent/currency examples from
`research/quickjs/tests/test_intl.js::test_numberFormat`, all at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
Implementation is written afresh over the already attributed locale, pattern
and currency pools. The integration examples adapt upstream expectations and
refer to the full QuickJS copyright/permission notice beside the decimal tests
and `src/intl/data/QUICKJS-LICENSE`. No new dataset or C runtime is used.

## Date ISO parsing (clj-quickjs-184)

`src/vm/date.rs::tests::date_iso_parse_fields` and
`tests/js_source.rs::date_parse_iso_strings` adapt ISO parsing, fraction
truncation and ISO round-trip rows from
`research/quickjs/tests/test_builtin.js::test_date` (lines 623–637, 662–680),
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The copyright/MIT permission notice from `research/quickjs/LICENSE` is retained
with the source tests and in the unit-test module. Other cases are synthetic
fixtures, not light-cljs output. `research/quickjs/quickjs.c` functions
`js_Date_parse`, `js_date_parse_isostring`, `string_get_digits`,
`string_get_milliseconds`, `string_get_tzoffset`, and `js_date_constructor`
were semantic references. The Rust parser/continuation is written afresh;
QuickJS's 128-byte prefix truncation and nine-digit fraction scan cap are not
copied. Non-ISO syntax extensions are described below (-185).

## Date fallback parsing (clj-quickjs-185)

`src/vm/date.rs::tests::date_fallback_parse_fields` and
`tests/js_source.rs::date_parse_fallback_strings` adapt the non-ISO construction
rows in `research/quickjs/tests/test_builtin.js::test_date` (lines 637–658),
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The complete copyright/MIT permission notice from `research/quickjs/LICENSE`
is retained beside the source test and in the unit-test module. Other test
inputs are synthetic fixtures, not light-cljs output.
`research/quickjs/quickjs.c` functions `js_date_parse_otherstring`,
`string_get_month`, `string_get_tzabbr`, `string_get_tzoffset`,
`string_get_digits`, `string_get_milliseconds` and `js_Date_parse`, including
the `js_tzabbr` names/offsets, supplied the semantic reference. The incremental
Rust state machine is written afresh. It retains the fallback's nine-digit
fraction grammar, but does not copy the fixed byte-buffer or NUL truncation;
stray closing parentheses are rejected even inside ignored word suffixes.

## Array and argument spread (clj-quickjs-130)

The first two rows of `tests/js_source.rs::array_spread_and_spread_arguments`
adapt `research/quickjs/tests/test_language.js::test_spread`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. Rust assertions inspect elements,
length and own names rather than relying on array string conversion. The
QuickJS MIT notice is retained beside the test. `research/quickjs/quickjs.c`
`OP_append` and the iterator helpers were consulted as semantic references;
no engine code was copied. Other fixtures are synthetic, not light-cljs output.

## RegExp match indices (clj-quickjs-193)

`tests/fixtures/js/regexp_indices.js`, run by
`tests/js_source.rs::regexp_has_indices`, adapts the match-indices row from
`research/quickjs/tests/test_builtin.js::test_regexp` (lines 803–807), revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. It substitutes the supported `u`
flag for upstream's `v` flag and retains the QuickJS MIT notice. Additional
capture, identity and resource checks are synthetic. The indices path in
`research/quickjs/quickjs.c::js_regexp_exec` at that revision was consulted
as a semantic reference; no C implementation was copied.

## RegExp Unicode sets (clj-quickjs-194)

`tests/fixtures/js/regexp_sets.js` adapts the v/vi replacement, class-string,
UTF-16 indices and outside-class punctuation rows from
`research/quickjs/tests/test_builtin.js::test_regexp` (lines 778–810), revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The fixture retains the QuickJS
copyright and MIT notice. Other tests are synthetic fixtures, not light-cljs
output. `research/quickjs/libregexp.c::re_parse_nested_class`,
`re_parse_class_set_operand` and REStringList were semantic references;
implementation is fresh Rust over existing attributed Unicode data.
The reserved punctuation grammar and static MayContainStrings rules follow
[ECMA-262 RegExp patterns](https://tc39.es/ecma262/multipage/text-processing.html#sec-patterns)
and [MayContainStrings](https://tc39.es/ecma262/multipage/text-processing.html#sec-static-semantics-maycontainstrings).
### Intl.Collator root rules and normalization (clj-quickjs-218)

Semantic references: `research/quickjs/intl/collator.c` (transform_string,
compare_case_order, is_combining_mark, is_significant and constructor/options)
and `research/quickjs/tests/test_intl.js` (test_collator sort rows), revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. Synthetic sorting tests adapt those
rows under the QuickJS MIT notice already included in tests/js_source.rs.
The engine implementation uses its own saved native continuations and root
primary/secondary/tertiary passes; no QuickJS execution engine is embedded.

The existing exporter plus `scripts/quickjs-data/repack-collation.pl` expand
`research/quickjs/libunicode-table.h` through the pinned offline `libunicode.c`
and `cutils.c` decoder. Generated NFD, full folding and combining-class tables
retain snapshot provenance in `src/intl/data/collation_provenance.txt`.
QuickJS copyright and MIT permission: `src/intl/data/QUICKJS-LICENSE`;
Unicode data notice: `docs/UNICODE-LICENSE`. These tables are a normalization
and folding snapshot, not a complete DUCET database or a verified CLDR release.

### Intl.Collator locale tailorings (clj-quickjs-219)

`research/quickjs/intl/collator.c` (`transform_string`, locale collation
selection) and `research/quickjs/tests/test_intl.js` (`test_collator`) at
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` supply the semantic reference and
five adapted source assertions for German phonebook and Turkish ordering.
The existing offline repacker additionally pins and extracts the transform
and its two classification helpers to export the tailoring weights. Their
generated data shares `src/intl/data/collation_provenance.txt`, the QuickJS
copyright/MIT notice in `src/intl/data/QUICKJS-LICENSE`, and the Unicode notice
in `docs/UNICODE-LICENSE`. No C code is linked into the Rust runtime. Other
new tests are synthetic fixtures, not light-cljs output.

## Number fixed decimal formatting (clj-quickjs-150)

Semantic references: `research/quickjs/quickjs.c` (`js_number_toFixed`) and
`research/quickjs/dtoa.c` (`JS_DTOA_FORMAT_FRAC`, exact half-away rounding),
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (2026-06-04).
The Rust implementation reuses in-tree BigInt arithmetic and decimal rounding;
no C implementation is copied. The six rounding cases from
`research/quickjs/tests/test_builtin.js::test_number` are adapted in
`src/number.rs::tests::fixed_formatter_exact_rounding` and
`tests/js_source.rs::number_to_fixed`. The QuickJS copyright and MIT notice
is retained in `src/intl/data/QUICKJS-LICENSE` and the existing
`string_code_unit_access` notice in the source tests. Other fixtures are
synthetic, not light-cljs output.


## JavaScript WeakMap tests (clj-quickjs-173)

`tests/gc.rs::weak_map_drops_entries_of_unreachable_keys` and
`weak_map_cycles_are_reclaimed` adapt the object/symbol entries, five deletes,
dropped keys and cyclic/chained WeakMap scenarios from
`research/quickjs/tests/test_builtin.js`, `test_weak_map` and
`test_weak_map_cycles`, inspected revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
Collection is driven by the embedding API; returned persistent vectors let
host tests retain maps and acquire weak handles before collection. The second
cycle value is an object to observe reclamation. Other tests are fresh
synthetic fixtures, not light-cljs output. `research/quickjs/quickjs.c`
`js_weakref_is_target`, `js_map_constructor`, `js_map_set`, `js_map_get`,
`js_map_delete` and `js_weak_map_proto_funcs` were semantic references only;
implementation is new Rust over the existing collector, with no C source port.
The adapted tests retain the upstream LICENSE notice:

Copyright (c) 2017-2021 Fabrice Bellard
Copyright (c) 2017-2021 Charlie Gordon

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.

## Number exponential and precision formatting (clj-quickjs-151)

Semantic references: `research/quickjs/quickjs.c` (`js_number_toExponential`,
`js_number_toPrecision`) and `research/quickjs/dtoa.c` (`js_dtoa`,
`JS_DTOA_FORMAT_FIXED`, `JS_DTOA_FORMAT_FREE`, `JS_DTOA_EXP_ENABLED`), revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (2026-06-04).
The Rust implementation uses the existing exact Decimal generator and shared
shortest-digit conversion; no C implementation is copied. Five rounding rows
from `research/quickjs/tests/test_builtin.js::test_number` are adapted in
`src/number.rs::tests::exponential_and_precision_exact_rounding` and
`tests/js_source.rs::number_to_exponential_and_precision`. The QuickJS copyright
and MIT notice is retained in `src/number.rs` and the existing
`string_code_unit_access` notice in the source tests, with a full copy in
`src/intl/data/QUICKJS-LICENSE`. Additional fixtures are synthetic, not light-cljs
output.

### Intl.DateTimeFormat date fields (clj-quickjs-215)

Semantic reference: `research/quickjs/intl/datetimeformat.c` (component layout,
field extraction, option processing and bound format) and `intl/intl_private.h`
(pool indexing), revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`.
The 36 date component rows in `tests/sim_host.rs` adapt
`research/quickjs/tests/test_intl.js::test_dateTimeFormat` at that revision,
with the QuickJS MIT notice retained beside the rows. Existing generated
`src/intl/data/dates.rs` and locale records retain their QuickJS and Unicode
notices; no data or generator changes were made. Runtime code is native Rust
using the existing safe readers and Date conversion, with no linked C engine.

### Intl.DateTimeFormat time fields (clj-quickjs-216)

Semantic reference: `research/quickjs/intl/datetimeformat.c` (hour-cycle option
precedence, manual time widths/layout, day-period hour buckets and fractions),
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. Five time rows in
`tests/sim_host.rs::intl_date_time_format_time_fields` adapt
`research/quickjs/tests/test_intl.js::test_dateTimeFormat`; the retained QuickJS
copyright and MIT notice above the date test also covers these rows. Existing
generated am/pm, day-period and time-pattern pools keep their QuickJS and
Unicode notices. No table bytes or generation steps changed.

### Math.sumPrecise (clj-quickjs-158)

Semantic references: `research/quickjs/quickjs.c` (`js_math_sumPrecise` and
`sum_precise_*`), revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`.
The Rust integer accumulator is an independent implementation, not copied C.
The first row of `tests/js_source.rs::math_sum_precise` adapts
`research/quickjs/tests/test_builtin.js::test_math` at that revision; the other
rows are synthetic fixtures, not light-cljs output. The copyright/MIT notice
is retained in that test file, with the complete license also available at
`src/intl/data/QUICKJS-LICENSE`.

### Intl.DateTimeFormat styles (clj-quickjs-217)

Semantic reference: `research/quickjs/intl/datetimeformat.c` (style pattern
selection, field widths, quotes, combined substitution and zone display),
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The ten date-style rows in
`tests/sim_host.rs::intl_date_time_format_date_style` adapt
`research/quickjs/tests/test_intl.js::test_dateTimeFormat`; the copyright and MIT
notice above the date-field test covers these rows. Native Rust interprets the
existing generated date/time/combined pools, preserving their Unicode/QuickJS
notices and provenance. No table bytes, importer or generated representation
changed, and no C engine is linked.
## JavaScript WeakRef tests (clj-quickjs-176)

`tests/js_source.rs::weak_ref_deref_within_job` adapts the two object/symbol
iterations, two WeakRefs and dereference checks from
`research/quickjs/tests/test_builtin.js::test_weak_ref`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The upstream MIT copyright and permission notice is retained immediately above
under "JavaScript WeakMap tests" and applies to this adapted fixture as well.
The post-drop/post-collection expected values intentionally follow ECMA-262:
targets remain available in the same job, unlike QuickJS's reference-counting
immediacy. The adapted fixture uses the optional `std.gc()` collection point;
synthetic timer fixtures exercise collections after job boundaries. Other fixtures
are synthetic, not light-cljs output. `js_weakref_constructor`,
`js_weakref_deref`, `js_weakref_is_target` and `js_weakref_proto_funcs` in
`research/quickjs/quickjs.c` were semantic references only; no C code was ported.


## Symbol prototype and wrappers (clj-quickjs-129)

The descriptive-string, valueOf and wrapper rows in
`tests/js_source.rs::symbol_prototype_and_wrappers` adapt
`research/quickjs/tests/test_builtin.js::test_symbol` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The full QuickJS MIT copyright/license notice is retained in the same Rust
file above `string_code_unit_access`. Other cases are synthetic fixtures,
not light-cljs output. The implementation independently follows the semantics
of `js_thisSymbolValue`, `js_symbol_toString`, `js_symbol_valueOf`,
`js_symbol_get_description`, `js_symbol_proto_funcs` and `JS_ToObject` in
`research/quickjs/quickjs.c` at that revision; no C source was copied.

### Base classes (clj-quickjs-136)

The C/E/P cases in `frontend::tests::class_syntax` and `tests/js_source.rs::base_classes`
adapt the base-class portions of `research/quickjs/tests/test_language.js::test_class`
at revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The QuickJS copyright and
MIT license notice are retained in both test modules. `js_parse_class`,
`js_parse_class_default_ctor`, `js_op_define_class` and `OP_set_home_object` in
`research/quickjs/quickjs.c` were consulted for semantics; no C code was ported.
All other class tests are synthetic fixtures, not light-cljs output.

### Derived classes (clj-quickjs-137)

The first row of `tests/js_source.rs::derived_classes` adapts the inheritance
rows of `research/quickjs/tests/test_language.js::test_class` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The existing QuickJS MIT notice
in that test module applies. `js_parse_class`, `OP_check_ctor`, `OP_init_ctor`
and `JS_CallConstructorInternal` in `research/quickjs/quickjs.c` were consulted
as semantic references; no C implementation was ported. Remaining tests are
synthetic fixtures, not light-cljs output.

### Intl.ListFormat (clj-quickjs-220)

`research/quickjs/intl/listformat.c`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04), supplies the
separator selector exported offline by `scripts/quickjs-data/repack-lists.pl`.
Its input SHA-256 and exporter provenance accompany `src/intl/data/lists.rs` in
`list_provenance.txt`. The QuickJS copyright and MIT permission notice are
preserved in `src/intl/data/QUICKJS-LICENSE` and copied to each exporter result;
the Unicode notice remains in `docs/UNICODE-LICENSE`. Runtime state machines
are independently written Rust; no QuickJS runtime is linked.

The first three `tests/js_source.rs::intl_list_format` rows adapt
`research/quickjs/tests/test_intl.js::test_listFormat` at that revision; the full
QuickJS copyright and MIT notice appear at the top of the test file. Remaining
fixtures are synthetic, not light-cljs output. Snapshot limitations, including
Spanish and default-language fallbacks, are recorded in grammar/architecture.

## Unmodified built-in source cases (clj-quickjs-163)

`tests/fixtures/js/quickjs_test_builtin_builtins.js` extracts
`research/quickjs/tests/test_builtin.js` lines 1–71, 191–233, 235–342,
344–361, 363–401, 527–610 and 1066–1090 at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
All 388 selected lines, including the assertion helpers and expected values,
are byte-for-byte unchanged. Only a header carrying the complete QuickJS
LICENSE notice and six case calls are added. This is adapted QuickJS test
text, labelled as a synthetic fixture, not light-cljs output.
`tests/quickjs_builtins.rs` selects each complete function range with the
original assertion helpers, retaining all JSON position helpers, and compiles
once per case. It checks the embedded range lengths without requiring the
research checkout at test time. The rope case retains both upstream
100,000-code-unit loops and runs with collection between budgeted slices.

## Complete Date and RegExp upstream fixture (clj-quickjs-195)

`tests/fixtures/js/quickjs_test_builtin_date_regexp.js` preserves lines 1-71,
612-713 (`test_date`) and 715-811 (`test_regexp`) of
`research/quickjs/tests/test_builtin.js` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The 270 upstream lines are unchanged; only the descriptive/license header
and one trailing call per case are added. The full QuickJS MIT notice from
`research/quickjs/LICENSE`, copyright Fabrice Bellard and Charlie Gordon,
is retained in the fixture. This is synthetic test input, not light-cljs output.

`tests/js_source.rs::quickjs_date_regexp_cases_run_unmodified` selects each case
independently. Date runs unmodified in UTC, UTC+01:00, UTC-05:00 and
Europe/Paris (a zone with DST). RegExp's upstream line 744,
`a = eval("/\0a/");`, executes unchanged with direct eval support
(clj-quickjs-107). Both cases run every original assertion without
source substitutions or changed expected values.

## JavaScript FinalizationRegistry tests (clj-quickjs-177)

`tests/gc.rs::finalization_callbacks_run_as_jobs_after_collection` adapts both
blocks of `research/quickjs/tests/test_builtin.js::test_finalization_registry`
at revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The QuickJS MIT copyright/license notice retained above test_async_gc in that
file also covers these adapted blocks. Both object and number held values
are unchanged; added counters/order effects check asynchronous owner-task
execution. `js_finrec_constructor`, `js_finrec_register`, `js_finrec_unregister`,
`js_finrec_mark`, `finrec_delete_weakref` and `js_finrec_job` in
`research/quickjs/quickjs.c` were semantic references only, with no C port.
Other fixtures are synthetic, not light-cljs output.

### Relative time formatting (clj-quickjs-221)

`src/intl/data/relative_time.rs` repacks literal tables and auto-return strings
from `research/quickjs/intl/relativetimeformat.c`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, through the pinned data pipeline.
QuickJS's MIT notice is retained in `src/intl/data/QUICKJS-LICENSE`; the Unicode
notice for locale data is in `docs/UNICODE-LICENSE`. Runtime code is a Rust-native
implementation, using that file as a semantic reference. The synthetic
`intl_relative_time_format` test references the 16 result rows in
`research/quickjs/tests/test_intl.js::test_relativeTimeFormat` at that revision;
the QuickJS copyright/license notice is retained at the top of the test file.

## Complete collection source cases (clj-quickjs-178)

`tests/fixtures/js/quickjs_test_builtin_collections.js` extracts
`research/quickjs/tests/test_builtin.js` lines 1-71, 456-522, 845-906,
908-934, 936-959, 961-982 and 984-1005 at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The full QuickJS MIT notice from `research/quickjs/LICENSE`, including
Fabrice Bellard and Charlie Gordon's copyright, is retained in its header.
This is adapted QuickJS test text, a synthetic fixture, not light-cljs output.

The selected upstream spans contain 295 lines. Only `test_weak_ref` changes:
lines 976-977 and 979-980 assert each `deref()` is not undefined within the
same job, then six inserted lines capture both references separately per
iteration and schedule a timer to collect and assert both are undefined.
The fixture header records the exact replacements and inserted block.
Every other selected byte is unchanged; the license header and six trailing
case calls are added. The integration test checks embedded range lengths;
the upstream bytes were compared directly after reversing those documented
WeakRef edits. `tests/quickjs_collections.rs` adds harness-only WeakMap
constructor observation without editing upstream bodies, allowing the
Rust-side live-entry diagnostic to check all three still-rooted maps.

## Upstream BigInt fixtures (clj-quickjs-206)

`tests/fixtures/js/quickjs_test_bigint.js` and `quickjs_pi_bigint.js` preserve
the complete `research/quickjs/tests/test_bigint.js` (279 lines) and
`research/quickjs/examples/pi_bigint.js` (118 lines), respectively, at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
Only descriptive headers and the full copyright/license notice from
`research/quickjs/LICENSE` are prepended; the example's original comment
remains intact. Both are synthetic fixtures, not light-cljs output.
The upstream bodies were compared byte for byte during implementation.

`tests/quickjs_bigint.rs` runs the whole BigInt test unchanged, then replaces
exactly its five final calls with one call for each individual case. The
example driver prepends exactly this line, also recorded in its fixture header:
`"use strict"; function print(value) { __rt.effect(0, value); }`.
No other lines, assertions, expected values or default digit counts change.
The example's output is compared with the first 1002 characters of the
unchanged 2000-decimal-place `test_pi` expectation.

### Intl.Segmenter (clj-quickjs-222)

`src/intl/segmenter.rs` adapts the grapheme/Hangul and ASCII/non-ASCII word
heuristics from `research/quickjs/intl/segmenter.c`, pinned revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. Its MIT notice is retained in the
module (QuickJS LICENSE: Fabrice Bellard and Charlie Gordon, 2017–2021).
`tests/js_source.rs::intl_segmenter_grapheme_and_word` adapts the six assertions
in `research/quickjs/tests/test_intl.js::test_segmenter`; the existing QuickJS
MIT notice in that test file applies. Regional-indicator pairing is corrected;
all UAX #29 and sentence limitations are documented in architecture/grammar.
The existing generated combining-class data and provenance are reused without
changes; the Unicode notice remains in `docs/UNICODE-LICENSE`.

### Intl.DisplayNames code names (clj-quickjs-223)

Semantic references: `research/quickjs/intl/displaynames.c` and
`research/quickjs/intl/intl.c::js_intl_find_display_name`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The new `js_source` code-name
rows adapt `research/quickjs/tests/test_intl.js::test_displayNames`; the
QuickJS MIT notice is retained in that test file. Runtime code is written
fresh in Rust. Name data reuses `src/intl/data/display_names.rs`, already
exported and repacked by `scripts/quickjs-data/`; its provenance, QuickJS
license and `docs/UNICODE-LICENSE` remain applicable and unchanged.

### Intl.DisplayNames language names (clj-quickjs-224)

Language-name semantics reference `research/quickjs/intl/displaynames.c`,
`intl/intl.c::js_intl_find_display_name`, and the shared Locale parser at
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. Runtime code is written
fresh in Rust. The `js_source::intl_display_names_language` acceptance rows
adapt `research/quickjs/tests/test_intl.js::test_displayNames`; the full MIT
notice already retained in that file applies. The pipeline pins and extracts
the literal pairs in `intl/displaynames.c::dn_fallback_language`, retaining
the existing en/fr language pools from `intl/locale-data.c`. Generated data
and updated input/converter hashes are in `src/intl/data/display_names.rs`
and `provenance.txt`; `QUICKJS-LICENSE` and `docs/UNICODE-LICENSE` apply.

### Intl.DurationFormat (clj-quickjs-225)

DurationFormat semantics were researched in `research/quickjs/intl/durationformat.c`
and `research/quickjs/temporal/duration.c` at pinned revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04), with
Rust-native continuations and integer subsecond composition. Unit pattern
literals are imported from `research/quickjs/intl/numberformat.c` through
`scripts/quickjs-data`; the existing full MIT notice in
`src/intl/data/QUICKJS-LICENSE` and Unicode notice in `docs/UNICODE-LICENSE`
apply. Input and converter hashes are retained in unit_provenance.txt.
The first three assertions in `tests/js_source.rs::intl_duration_format`
adapt `research/quickjs/tests/test_intl.js::test_durationFormat`;
the existing QuickJS MIT notice in that test file applies.

### Complete Intl source fixture (clj-quickjs-226)

`tests/fixtures/js/quickjs_test_intl.js` preserves lines 1-470 of
`research/quickjs/tests/test_intl.js` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04),
byte for byte, including all helpers, ten complete cases and expected strings.
Only a descriptive header, the full copyright/MIT notice from
`research/quickjs/LICENSE`, and ten trailing case calls are added. Its header
and `tests/quickjs_intl.rs` list adapted rows: none. This is adapted QuickJS
test text, a synthetic fixture, not light-cljs output. The integration test
runs each case separately in two simulated host zones with en-US as the
default locale and collection between instruction-budget slices.

## Super properties (clj-quickjs-138)

`tests/js_source.rs::super_property_access` and the super-properties examples in
`docs/grammar.md` adapt the h/H inheritance rows of
`research/quickjs/tests/test_language.js::test_class` and the `delete super.a`
row of `test_delete`, revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`
(VERSION 2026-06-04). The full QuickJS MIT notice is retained in
`tests/js_source.rs` beside `object_literals_and_named_properties`.
Other fixtures are synthetic, not light-cljs output. `js_method_set_home_object`,
`CASE(OP_get_super)`, `CASE(OP_get_super_value)` and `js_parse_postfix_expr` in
`research/quickjs/quickjs.c` at that revision were semantic references. The Rust
implementation was written afresh; no C implementation was copied or wrapped.
### Unmodified operator source fixture (clj-quickjs-77)

`tests/fixtures/js/quickjs_test_language_operators.js` preserves lines 1-32
and 39-220 of `research/quickjs/tests/test_language.js` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04), byte for
byte. These contain `assert`, `assert_throws`, `test_op1`, `test_cvt`,
`test_eq`, `test_inc_dec`, constructor `F` and `test_op2`, including the
reserved-word property names. Only a descriptive header, the full
copyright/MIT notice from `research/quickjs/LICENSE`, and five trailing
case calls are added. Adapted rows: none. This is adapted QuickJS test
text, a synthetic fixture, not light-cljs output. `tests/quickjs_language.rs`
checks the embedded upstream line count, compiles once per case in upstream
non-strict mode, and runs each independently with bounded instruction
slices and collection between slices.

### Object construction (clj-quickjs-280)

`js_object_constructor` in `research/quickjs/quickjs.c` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04)
was a semantic reference for direct Object argument rules and derived
allocation. The Rust implementation is original; no C implementation was
copied or wrapped. New constructor/lifetime tests are synthetic fixtures,
not light-cljs output; the upstream operator fixture remains unmodified.

## Public class fields (clj-quickjs-139)

The S/P rows in `frontend::tests::class_fields` and
`tests/js_source.rs::class_fields` adapt
`research/quickjs/tests/test_language.js::test_class` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The existing QuickJS copyright and
MIT license notices in both test modules apply. `research/quickjs/quickjs.c`
`js_parse_class`, the field-initializer path, and `CASE(OP_define_field)` were
read as semantic references. The Rust implementation is original; all other
new fixtures are synthetic, not light-cljs output.

### Unmodified function, scope and eval fixtures (clj-quickjs-113)

`tests/fixtures/js/quickjs_test_closure.js` preserves the whole
`research/quickjs/tests/test_closure.js` (lines 1-221).
`quickjs_test_language_functions.js` preserves lines 1-38, 271-280,
445-577 and 641-666 of `research/quickjs/tests/test_language.js`;
`quickjs_test_builtin_functions.js` preserves lines 1-123 and 403-455
of `research/quickjs/tests/test_builtin.js`. Revision:
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
The selected bytes were compared against that checkout; adapted rows: none.
Each fixture prepends a descriptive header and the full copyright/MIT notice
from `research/quickjs/LICENSE`; the language and builtin fixtures append
five and three calls respectively. Original caught `__loadScript` lines
remain. These are adapted QuickJS test text, synthetic fixtures, not
light-cljs output. `tests/quickjs_functions.rs` runs the complete closure
file and each selected language/builtin case in its original script mode.

### Unmodified object, class and symbol fixtures (clj-quickjs-140)

`tests/fixtures/js/quickjs_test_language_objects.js` preserves lines 1-32,
222-249, 251-258, 260-269, 281-354, 378-387, 428-433, 435-443 and 609-632
of `research/quickjs/tests/test_language.js`.
`tests/fixtures/js/quickjs_test_builtin_objects.js` preserves lines 1-71,
124-172, 174-189 and 813-843 of `research/quickjs/tests/test_builtin.js`.
Both use revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`
(VERSION 2026-06-04). All selected bytes, including assertion helpers,
strictness directives and the optional-chaining comment, were compared
against that checkout. Adapted rows: none. Only descriptive headers,
the full copyright/MIT notice from `research/quickjs/LICENSE`, and one
trailing call per case are added; file-level `__loadScript` lines are excluded.
These are adapted QuickJS test text, synthetic fixtures, not light-cljs output.
`tests/quickjs_objects.rs::quickjs_object_cases_run_unmodified` checks embedded
upstream line counts and runs all eleven cases independently with the original
script modes, bounded instruction slices and collection between slices.

### Resource-management fixture (clj-quickjs-242)

`tests/fixtures/js/quickjs_test_language_using.js` adapts lines 1-32 and 677-993,
with calls from 1022-1023, of `research/quickjs/tests/test_language.js` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04). Its header
retains the full copyright/MIT notice from `research/quickjs/LICENSE` and lists
the only adaptations: wrapping the two `SuppressedError.prototype.constructor`
assignments in TypeError assertions because realm intrinsics remain immutable.
All original assertions and expected values are unchanged; helpers and
`test_await_using` are byte-for-byte upstream. The Rust driver in
`tests/exceptions.rs` is original. This is synthetic fixture text, not
light-cljs output; no QuickJS implementation code is copied or wrapped.
