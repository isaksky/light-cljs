# Compiler-owned clojure.string

`src/compiler_libraries/clojure/string.cljs` is ordinary compiler-owned
ClojureScript source. It calls compiler core sequence/printing functions and
JavaScript string/regex primitives. It does not call the interpreter native
registry. Namespace loading, Vars, aliases, and compiled macro helpers use the
same source module.

The public contracts and examples were inventoried from the pinned checkout at
`research/clojurescript`, revision
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`:

- `src/main/cljs/clojure/string.cljs`, all 20 public functions.
- `src/test/cljs/clojure/string_test.cljs`, `test-api` and `test-cljs-2300`.
- Local native inventory: `src/interpreter/strings.rs`, `string_definitions`.

The upstream copyright/license notice is retained on the implementation.
`tests/compiler_strings.rs` uses literal expected outcomes, including adaptations
of pinned API examples and additional boundary cases. Every group executes
independently in QuickJS and Node. A compiled macro uses join, regex replacement,
split, and surrogate-preserving reverse from the required namespace, and the
application verifies the 20 public Vars.

| Public functions | Arities | Native inventory |
| --- | --- | --- |
| join | 1, 2 | supported |
| split | 2, 3 | absent from the literal-only native engine |
| split-lines, trim, triml, trimr, trim-newline, blank? | 1 | supported |
| upper-case, lower-case, capitalize, reverse | 1 | supported |
| replace, replace-first | 3 | supported for literal string/character matches |
| escape | 2 | supported |
| index-of, last-index-of | 2, 3 | supported |
| starts-with?, ends-with?, includes? | 2 | supported |

## String and host semantics

Strings and indices use UTF-16 units. Reverse preserves valid surrogate pairs
and leaves unpaired units intact. Escape calls its map/function once for each
UTF-16 unit, so an astral character produces two callback invocations. The
empty-regex split follows the pinned leading-empty convention, including
separate surrogate units, and default split removes trailing empty parts.
Positive split limits retain the untouched suffix; negative limits retain
trailing empties. Unbounded regex split preserves JavaScript capture fields.

Join prints elements with compiler core `str`, including nil as an empty string,
false as `false`, and collections using their printed representation. Separators
follow the pinned `StringBuffer.append` conversion, `String(separator)`: nil
becomes `"null"` between elements, false becomes `"false"`, and keywords keep
their colon. Empty/single-element collections never convert the separator.
Each element prints before `next` realizes the following sequence element, then
the separator converts before that element prints.

`tests/compiler_string_join.rs` executes a dedicated public one/two-arity
contract, exact UTF-16 strings and conversion/effect/arity controls in Node and
QuickJS applications and fresh/reused actual MacroHost expansions. Its complete
five-call SCI source is byte-for-byte identical to `sci-string-join-values`.
Both required reference pins independently execute the same original source
and the full canonical contract via `tests/reference/compiler-string-join.py`.
The original SCI expectation `"12"` for `(join nil [1 2])` remains historical and
explicitly unresolved against the adopted canonical result `"1null2"`;
`compiler_string_fixtures.rs` records that conversion as a named override.
Public source/arity/phase bindings use the dedicated contract, while the complete
upstream string API body still executes unchanged. This canonical agreement
does not grant passing credit to the conflicting original SCI obligation.

Escape
replaces every non-nil result, including false and zero, as specified by the
upstream docstring. The native interpreter currently treats false like nil in
escape; compiler behavior deliberately follows the documented ClojureScript
contract.

Whitespace follows JavaScript/Closure `\s` plus NBSP: NBSP and BOM are blank and
trimmed; U+001C is preserved. The native engine's JVM-style whitespace inventory
has different membership. JavaScript case conversion includes contextual
Unicode behavior such as Greek final sigma; the native engine maps individual
Unicode scalar values. Compiler upper/lower/capitalize expect a string, while
the native case functions stringify non-nil values.

Index lookup uses JavaScript numeric conversion and clamping. In particular,
`(last-index-of "abc" "a" -100)` returns 0, whereas the native interpreter returns
nil for any negative backward starting index. String/character values have the
same representation in the compiler. Single fixed-arity functions retain the
compiler's ordinary JavaScript missing/extra argument behavior; multi-arity
functions use the normal compiler arity dispatcher. Native arity/type errors
are independently checked by interpreter tests.

## Documented corrections to pinned implementation quirks

The implementation follows the documented replacement and positive split
contracts where the pinned source has host-level quirks:

- Literal replacement strings are always literal, including `$&` and `$1`.
  The pinned `replace` implementation passes literal replacement strings to
  JavaScript `replace`, which interprets these tokens.
- Both replace functions adapt regex callbacks to a single full-match string,
  or a compiler vector containing the full match and capture groups. Named
  capture metadata is excluded. Pinned `replace-first` passes JavaScript's raw
  callback argument list directly to the function.
- Replace-first replaces exactly one occurrence even when given a global
  regex. The pinned implementation retains JavaScript's global behavior.
- Regex flags such as dotAll and Unicode are preserved; global/sticky traversal
  flags are normalized to the requested replacement operation. Pinned
  replace-all rebuilds only global, ignoreCase, multiline, and Unicode flags.
- Positive-limit split uses the regex match index and full matched text length,
  including for regexes with capture groups. Pinned positive-limit split uses
  the string/count of the `re-find` result vector, which produces incorrect
  offsets for capture groups.

Zero-width regexes other than the empty-pattern special case retain the pinned
finite positive-limit algorithm: a zero-width match may add an empty part
without consuming the suffix. The limit bounds iteration. Unbounded split uses
JavaScript's own zero-width matching rules.

## Complete native corpus and upstream body witnesses

`tests/compiler_string_fixtures.rs` reads every one of the 131 `sci-string-*`
rows from `tests/fixtures/sci-outcomes.tsv`, the exact filter used by
`tests/interpreter_strings.rs`. It retains the original ID, source, native
outcome, and effect trace. Expected strings are constructed as explicit
`\uNNNN` units, preserving lone surrogates. The test emits all 131 bodies and checks
returned values (or a caught failure boundary) and ordered effects in both
QuickJS and Node. Native exception phase/category distinctions are not equated
with JavaScript Error subclasses.

The 15 source bodies containing `*print-meta*` now execute in both engines.
`tests/fixtures/compiler-native-printing.tsv` records all 58 complete printing
bodies and explicit canonical ClojureScript outcomes; the historical native
outcomes remain separately recorded in `sci-outcomes.tsv`. Only ratio reader
literals are adapted to numeric division. Each migrated body is compared with
its exact saved source before execution. Metadata, bounded maps/sets, fractional
and negative limits, readability, and lazy printing effects all use the outcomes
observed independently from both pinned references. There are no deferred
metadata compiler rejections or provisional printing overrides.

Canonical metadata prints the complete map, including `:tag`; it remains visible
when `*print-readably*` is false. Metadata itself respects print level and length.
Collection `str`/`toString` uses the direct writer protocol and omits root metadata,
while still honoring level and length. Thus the retained native fixture named
`str-ignores-bindings` returns `"#"` in ClojureScript under its level-zero binding.
Negative, fractional, false, and NaN print lengths do not reach the exact numeric
zero termination condition; they print the full collection. Boolean true prints
one item. Print levels use decrement/comparison, so false hides a collection and
fractional levels can stop between integral depths. These are explicit host
contracts, separate from the native interpreter's historical expectations.

The source adapters are narrow and explicit:

- Ratio literals become equivalent numeric division expressions because the
  compiler targets JavaScript numbers and rejects ratio reader literals.
- Fixture requires are represented by the application ns clauses; the repeat
  require is retained at the supported top-level boundary.
- The native Namespace object/`ns-name` projection becomes the compiler Var's
  symbolic `:ns` metadata. Public-Var equality and repeated-require checks remain.
- The invalid gensym call uses `apply` so its runtime arity failure is observable;
  a direct invalid call is rejected during compiler analysis.
- Character expectations use compiler strings. The escape-surrogates fixture
  returns the exact original surrogate units but observes `[0 0]` because
  JavaScript `int` numerically converts the two one-unit strings.

The verbatim pinned `test-api` and `test-cljs-2300` bodies are saved in
`tests/fixtures/compiler-upstream-string-tests.cljs`, SHA-256
`f8ed030e26dbc9432017364e4dfd9be431898641d4d440341f29987b5d47ebf2`.
The test supplies lightweight `deftest`, `testing`, `is`, and `defspec` adapters:
assertions evaluate once and record their booleans. All 95 API assertions run, including the nested assertion used as a message expression.
The complete capitalization property, including its original reference function
and character-code-to-string setup, replays the exact 1,000 nonempty
printable-ASCII inputs generated by both pinned references with test.check 1.1.1,
seed 123456. The saved JSON input corpus contains 48,938 UTF-16 units across
strings of lengths 1 through 195; SHA-256
`0a66997ddc5a08887e6c8ecc2b6e4c6179681873d79b0d3bae716b7660a0fa86`.
The compiler's generation adapter reconstructs their character-code vectors,
then runs the unchanged `gen/fmap` setup and property body. A counter verifies
all 1,000 inputs actually execute. This replaces only test.check's generation
boundary and avoids a production dependency on that test library.

Canonical core `subs` is also now available to ordinary source and compiled
macro helpers. Its two/three argument implementation follows the pinned
`cljs/core.cljs` `subs` function: JavaScript `substring` clamps out-of-range
indices, swaps reversed endpoints, and counts UTF-16 units. The native corpus
covers both arities, fractions, surrogates, invalid receivers, and invalid arity.

The following explicit compiler outcome overrides are not silent skips. The
original native outcome remains in the shared corpus; the literal compiler
expectation and reason live beside the executable test.

| Native fixture ID | Difference classification |
| --- | --- |
| `sci-string-str-scalars` | JS ratio/character representation |
| `sci-string-str-collections` | JS character representation |
| `sci-string-name-type` | JS character/string identity |
| `sci-string-keyword-split` | JS pinned keyword split |
| `sci-string-keyword-type` | JS keyword argument conversion |
| `sci-string-symbol-split` | JS pinned symbol slash handling |
| `sci-string-case-coercion` | JS strict case receiver |
| `sci-string-trim-unicode` | JS whitespace membership |
| `sci-string-blank-type` | JS safe string conversion |
| `sci-string-includes-type` | JS character/string identity |
| `sci-string-index-empty` | JS index clamping |
| `sci-string-index-coerce` | JS index conversion without int32 narrowing |
| `sci-string-index-type` | JS search argument conversion |
| `sci-string-replace-char-type` | JS character/string identity |
| `sci-string-replace-string-type` | JS character/string identity |
| `sci-string-escape-map` | JS documented non-nil escape result |
| `sci-string-index-character-wrap` | JS index conversion without int32 narrowing |
| `sci-string-index-empty-wrap` | JS index conversion without int32 narrowing |
| `sci-string-subs-negative` | JS substring clamping |
| `sci-string-subs-too-long` | JS substring clamping |
| `sci-string-subs-reversed` | JS substring swaps reversed endpoints |

The 15 formerly deferred `*print-meta*` rows now execute with independently
verified canonical expectations:

- `sci-string-str-ignores-bindings`
- `sci-string-pr-meta-enabled`
- `sci-string-pr-meta-tag`
- `sci-string-pr-meta-unreadable`
- `sci-string-pr-tag-nil`
- `sci-string-pr-tag-false`
- `sci-string-pr-tag-true`
- `sci-string-pr-tag-keyword`
- `sci-string-pr-tag-integer`
- `sci-string-pr-tag-string`
- `sci-string-pr-meta-level-zero`
- `sci-string-pr-meta-level-one`
- `sci-string-pr-meta-level-two`
- `sci-string-pr-meta-length-zero`
- `sci-string-pr-meta-length-one`

The provenance check in `tests/reference/compiler-native-printing.py` verifies
all 58 original/adapted body hashes, expectation hashes, and the two pinned
reference identities. Its `--run` mode uses `tests/reference/execution.clj` to
compile every complete body and compare its returned printing outcome and
ordered effects against the saved expectations. Both references agree on all
58 bodies, including the two lazy realization callbacks. The actual observations
are retained under `/tmp/light-cljs-124-native-printing-references`.

## Executed pinned references

The executable reference witness at
`tests/fixtures/compiler-strings-reference/reference/main.cljs` contains the
same full `test-api` and `test-cljs-2300` bodies, with the original cljs.test and
test.check requirements. Its renamed namespace and runner are the only source
plumbing. The runner observes cljs.test reports without altering assertion
values; it calls the original defspec with 1,000 trials and seed 123456. A
capitalize wrapper records the generated strings while returning the actual
reference function's result. The two reference executions produced identical
saved inputs and identical outcomes:

| Reference | API passes | API failures/errors | Property trials | Seed | Property pass |
| --- | --- | --- | --- | --- | --- |
| Maven ClojureScript 1.12.134 | 95 | 0 / 0 | 1,000 | 123456 | true |
| Checkout a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e | 95 | 0 / 0 | 1,000 | 123456 | true |

The reference harness verifies compiler identity and uses `:optimizations
:none`, `:static-fns false`, and `:def-emits-var false`. Both builds use actual
reference `clojure.string`, cljs.test, and test.check 1.1.1. The compiler replays
the same strings in both QuickJS and Node and checks all 95 API observations.
A regular provenance test compares the copied local/reference full body text.
The documented contract corrections for literal replacement tokens, global
replace-first regexes, grouped replace-first callbacks, and positive split
capture handling remain separate from this unchanged upstream witness.

Commands executed:

```sh
cargo test --test compiler_string_fixtures --test compiler_strings -- --nocapture
cargo test --test compiler_string_fixtures full_upstream_bodies_pass_both_pinned_cljs_references -- --ignored --nocapture
cargo test --test compiler_string_fixtures all_58_native_printing_bodies_match_both_pinned_cljs_references -- --ignored --nocapture
python3 -B tests/reference/compiler-native-printing.py
```

The ignored reference check invokes `tests/reference/execution.clj` in program
mode for release 1.12.134 and the exact clean checkout SHA, then executes each
compiled runner in Node. It verifies the saved sample corpus rather than
rewriting it during normal reference verification.
