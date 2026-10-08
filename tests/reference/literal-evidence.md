# Numeric and UTF-16 literal evidence

Audited locally on **2026-10-08** against
[clojure/clojurescript](https://github.com/clojure/clojurescript) at clean commit
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. All upstream paths and anchors
below refer to that revision. This bounded audit covers scalar compilation;
it does not claim complete reader, compiler, or runtime-library coverage.

The audit adds **28** executable cases: **24** text-source rows (**5 adapted**,
**19 independent**) and **4 independent** byte-encoded source rows. All **230**
programs and all three namespace comparisons pass against both Maven
ClojureScript **1.12.134** and the checkout (reported **0.0.135109660**).
[fixture-origins.tsv](fixture-origins.tsv) retains the source, stable anchor,
reduction, local identity and assertion for each adaptation. No decoding,
number-conversion or emission defect was exposed. Unsupported numeric diagnostics
now name the actual category: big integers, decimal literals, or ratios.

## Numeric coverage and existing overlaps

IDs below are rows in `tests/fixtures/execution.tsv`. Programs use `observe`
to retain every value in a grouped test, including intermediate signed zeros.

| Behavior | New cases and existing coverage |
| --- | --- |
| Signed decimal/hex/octal/radix values | `number-reader-bases`, `number-reader-positive-bases`, `number-reader-negative-bases` retain all 18 spellings from the runtime-reader assertions, including `0x`/`0X`, `r`/`R`, bases 2/8/16/36 and both signs. Existing `integer`, `negative`, `positive`, `hex`, `negative-hex`, `octal`, `radix`, `radix-n` are mapped, not relabeled or duplicated as upstream imports. |
| Integer zero | Existing `integer-zero` checks `-0`; `number-integer-zeros` checks `0`, `+0`, `-00`, signed hexadecimal and signed radix zeros. Every result is positive zero. |
| Safe integer interval | Existing `safe-integer` checks 9007199254740991. `number-safe-negative` adds -9007199254740991; `number-safe-bases` checks both endpoints as hex/octal/binary, plus signed adjacent interior decimal values. |
| Float syntax and zero | Existing `float`, `exponent`, `trailing-point`, `negative-zero`; `number-float-exponents` adds signed positive/negative exponents, leading-zero decimal and trailing-dot exponent syntax. `number-float-zeros` separates positive `0.0`/`+0.` from negative exponent/trailing-dot zeros. |
| Largest finite doubles | `number-float-max` checks both signs of 1.7976931348623157e308 and 1.7976931348623158e308, which still rounds to the largest finite double. |
| Normal/subnormal boundary | `number-float-min-normal` checks the smallest normal, largest subnormal and negative smallest normal; `number-float-subnormal` checks both signs of the smallest subnormal and its next neighbor. |
| Underflow and rounding | `number-float-underflow` checks both signs of deep underflow and decimal inputs immediately below/above half the smallest subnormal. `number-rounding-ties` uses exact halfway decimals around 1 and -1 to distinguish ties-to-even rounding in both directions. |
| Decimal-to-JavaScript round trips | `number-decimal-roundtrips` covers 0.1, 0.30000000000000004, a 17-digit value, and exponent/decimal printing transitions at 1e-7, 1e-6, 1e20 and 1e21. `number-float-integer-rounding` checks ±9007199254740993.0 rounding to ±9007199254740992. These tokens explicitly request floating conversion. |

`analyzer::numeric_limits_are_reported_instead_of_silently_losing_integer_precision`
checks 18 out-of-range tokens, standalone and nested after a Unicode binding
and newline. Both signs of the first unsafe integer are tested in decimal,
hexadecimal, octal, binary and base 36; just-beyond and very large decimal
magnitudes cannot silently round. Both signs of 1.7976931348623159e308 and
extreme positive exponents must report `NumberOutOfRange`. The nested literals
are in an unselected branch: analysis still rejects them. Every check asserts
the exact original byte span and analysis stage.

`analyzer::unsupported_number_categories_have_precise_diagnostics` checks 15
big-integer, decimal, ratio and symbolic spellings after a Unicode/newline
prefix, with exact category and byte-span assertions. This includes decimal,
hex and octal `N` suffixes, exponent `M`, leading-zero decimal `09M`, reducible
ratio `1/1`, and all three `##NaN`/`##Inf`/`##-Inf` symbols.

## String coverage and transport

| Behavior | New cases and existing coverage |
| --- | --- |
| Named escapes | Adapted `string-reader-escapes` contains tab, return, newline, backslash, quote, backspace and formfeed. Existing `escapes` retains its independently authored mixed-string regression. |
| Octal/Unicode escapes | `string-octal-escapes` checks one/two/three-digit octal escapes, zeros, a three-digit escape followed by an ordinary digit, and 0377. `string-unicode-digit-escapes` promotes the local snapshot's fullwidth/Arabic hex-digit test to execution. `string-escaped-null` checks null code units between ordinary characters. |
| Unicode strings | Adapted `string-reader-unicode` retains the seven upstream Arabic, Thai, Hiragana, Chinese, Yiddish, Polish and Russian strings. Existing `unicode` already covers accent, Greek and emoji. |
| UTF-16 units | Existing `surrogate` and `surrogate-pair` cover a lone high surrogate and an escaped emoji pair. `string-utf16-units` returns a mixed string and observes all seven `charCodeAt` units plus length. `string-surrogate-order` covers reversed pairs, repeated high/low surrogates and the lowest valid pair. |
| Literal value preservation | `string-quote-backslash-runs` has one/two/three backslashes before quotes and a trailing backslash. `string-source-text` contains JavaScript-shaped calls, comment markers, an HTML closing script tag, template syntax and a literal backslash-u sequence. Its value must survive verbatim, with empty stdout. Existing `string` and `empty-string` remain the ordinary/empty baselines. |
| Actual source control bytes | `string-raw-line-endings` compiles `"a<CR><LF>b<CR><FF>c<CR>d"` to `a<LF>b<LF>c<LF>d`. `string-raw-controls` retains actual null, tab, backspace, formfeed and LF inside a string. |
| Separators and supplementary limits | `string-raw-separators` observes raw U+2028/U+2029 and returns their escaped equivalents. `string-raw-supplementary` contains U+10000 and U+10FFFF as raw UTF-8, complementing the escaped pair cases. |

The four `string-raw-*` rows live in
[execution-literals.tsv](../fixtures/execution-literals.tsv), whose columns are
ID, **source UTF-8 hex**, expected value JSON, expected trace JSON. Hex prevents
TSV line splitting or newline translation from changing the source. Both
harnesses decode the bytes before compilation; the Clojure harness writes only
the ID in its source comment so multiline fixture text cannot escape the comment.
The ordinary text-source and outcome formats retain their meanings.

The shared outcome protocol tags negative zero before JSON serialization.
Finite-number JSON representations distinguish the remaining supported doubles.
String values never pass through a replacement-producing UTF-8 conversion:
`JSON.stringify` escapes lone surrogates, and valid pairs can use scalar UTF-8
without losing their UTF-16 units. The new independent Node probe
`execution::harness_preserves_utf16_units_and_embedded_controls_independently`
asserts literal JSON bytes and an explicit code-unit array without either
compiler. The existing independent numeric probe checks negative zero, NaN and
infinities. Thus agreement cannot hide a shared lossy serializer for these values.

## Upstream assertion dispositions

| Pinned source and stable anchors | Applicable behavior, reduction or exclusion |
| --- | --- |
| `src/test/clojure/cljs/analyzer_tests.clj`, `analyze-ops`, constants (628-633) | Existing `integer` and `string` cover scalar values; nil/booleans also have existing cases. Upstream child/op/val map layouts are not a local API contract; keyword/autoresolved-keyword constants remain unsupported. No redundant AST-only adaptation was added. |
| `src/test/clojure/cljs/compiler_tests.clj`, `test-js-negative-infinity` (92-96), `test-cljs-2352` (98-106) | Nonfinite literal/constant emission is outside the finite-number subset; `js/-Infinity` also falls outside the accepted host-identifier spelling. Existing `host-nan`, `host-infinity`, `host-negative-infinity` and the independent harness probe cover transport of those host values, not support for those literal forms. Exact emitted text is not asserted. |
| Same file, `test-cljs-1643` (203-205) | Excluded: attempting to emit a Clojure/JVM function object as a constant has no source counterpart in this subset. Supported `fn*` values have separate executable coverage. |
| `src/test/cljs/cljs/primitives_test.cljs`, `test-arithmetic` (35-244 inspected), numeric predicates and coercions (153-179, 208-224) | Literal hypotheses are covered by signed/scalar/extreme-float rows. Arithmetic functions, apply/map, equality/predicates, integer coercions and collection invocation assertions need the excluded core runtime. They were not reduced to unrelated literal assertions or claimed as ports. In particular `integer? 1e308` is a predicate test, not a safe-integer literal requirement. |
| `src/test/cljs/cljs/reader_test.cljs`, `test-reader`, basic reading (19-44) | Scalar number/string/nil/boolean and discard behavior maps to existing cases. The escape payload is adapted as `string-reader-escapes`; collection, symbol-as-data, keyword, character and metadata assertions remain unsupported for execution. Runtime `reader/read-string` itself is not implemented. |
| Same test, number literals (46-50) | All 18 nonzero base/sign spellings become three observed compilation cases. Existing `integer-zero` and new `number-integer-zeros` cover the three zero tokens; whitespace around zero is reader trivia, already handled by parser tests. No separate whitespace-only execution row was added. |
| Same test, Character Literals (108-110), Unicode (115-157) | Seven string entries are adapted; symbol/keyword/compound values and character literals remain excluded. Malformed Unicode escapes are parser failures; existing `parser::strings_decode_utf16_and_preserve_unescaped_spans` and vendored snapshot error rows preserve those boundaries. |
| Local `tests/reference/parser_literals.clj`, `cases`, numeric and string groups; `parser_literals.tsv`; `parser_reference::literal_syntax_and_utf16_values_match_reference_snapshot` | Existing hand-authored reader snapshots cover syntax categories and decoded UTF-16, not emitted runtime values. Execution adds line normalization, octal widths, Unicode digit escapes and surrogate-unit checks. Huge unsuffixed integers become explicit range diagnostics; `N`, `M`, and ratios become analysis exclusions. `36rN` already executes as digit N, while `36r10N` remains covered by the unchanged syntax snapshot. Malformed numbers, escapes and unsupported character values stay parser-only evidence. |

Read the pinned implementation in
`src/main/clojure/cljs/vendor/clojure/tools/reader/impl/commons.clj`, integer,
ratio and float patterns plus `match-int`, `match-ratio`, `match-float` and
`match-number` (45-96); `tools/reader.clj`, `read-unicode-char` (103-136),
`read-number`, `escape-char` and `read-string*` (263-304); and
`tools/reader/reader_types.clj`, `IndexingPushbackReader/read-char` (106-127).
These establish integer spelling, `Double/parseDouble`, Character.digit escape
semantics, short octal terminators and CR/CRLF/CRFF normalization.

Also read `src/main/clojure/cljs/compiler.cljc`, `escape-char`/`escape-string`
(154-178) and `emit-constant*` methods (304-356): strings are escaped by UTF-16
unit, while numeric constant classes have separate emission paths. The local
implementation audit covered `src/literals.rs`, `src/analyzer.rs::number`, and
`src/emitter.rs::value`. The JVM implementation is a source of hypotheses; the
release and checkout runtime comparisons establish the executable results.

## Intentional restrictions versus mismatches

There were **no reference mismatches for the supported cases**. Integer tokens
outside ±(2^53-1) are deliberately rejected even if a particular larger value
is exactly representable or standard ClojureScript emits it after rounding.
Explicit floating tokens use IEEE-754 rounding and may have integral values
outside the safe-integer interval. Finite rounded overflow-boundary inputs are
accepted; inputs that round to infinity are rejected. Underflow to signed zero
is accepted and its sign retained.

`N` big integers, `M` decimals, ratios (even `1/1`) and symbolic nonfinite
literals remain unsupported, regardless of how the standard reader reduces or
emits them. Character literals also remain unsupported although strings can
contain the corresponding code units. These are documented subset boundaries,
not unexplained failures or reasons to weaken the reference expectations.

## Reproduction and local validation

```sh
cargo test --locked
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution -- --ignored --nocapture
```

The default reference corpus now includes the hex-source rows automatically,
including in the required release conformance CI job. For a standalone hex-only
reproducer, use the reference harness's explicit `cases-hex` mode:

```sh
repo=$(pwd)
out=$(mktemp -d)
clojure -Srepro \
  -Sdeps '{:deps {org.clojure/clojurescript {:mvn/version "1.12.134"}}}' \
  -M tests/reference/execution.clj release 1.12.134 - "$out" \
  cases-hex tests/fixtures/execution-literals.tsv
(cd "$out" && node -r ./reference.js \
  "$repo/tests/support/execution.js")
```

Local validation passed **92 fast tests** (six explicit reference/corpus tests
ignored), Clippy, formatting and the four differential tests. The standalone
hex-only reproduction also passed all four byte-source cases. Both baselines confirmed
`{:optimizations :none, :static-fns false, :def-emits-var false}`. Tools were
Node **v24.18.0**, Rust **1.99.0**, Clojure CLI **1.12.4.1618**, effective Clojure
**1.12.4**, and OpenJDK **25.0.2**. These are local runs; no hosted CI result is
claimed. All parser/snapshot tests remain unchanged and run in the fast suite.
No parser implementation changed, so the large reader corpus was not rerun.
The five adaptations retain the ClojureScript copyright/EPL 1.0 notice in
`execution.tsv`; none is claimed to be imported verbatim.
