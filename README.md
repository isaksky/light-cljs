# light-cljs

A borrowing Rust lexer for ClojureScript source. It recognizes the lexical forms
used by ClojureScript's JVM reader while leaving form construction and reader
semantics for the next compiler stage. There are no library dependencies, token
text allocations, unsafe code, or SIMD.

```rust
use light_cljs::Tokenizer;

let source = "(+ 1 2)";
for token in Tokenizer::new(source) {
    println!("{:?} {:?} {:?}", token.kind, token.span, &source[token.span.clone()]);
}
```

`Tokenizer` borrows an `&str` and implements `Iterator<Item = Token>` and
`FusedIterator`. `Token` holds a kind and a half-open `Range<usize>` of byte
offsets. Every span, including an error span, is nonempty and can slice the
original UTF-8 source. Calls after EOF continue returning `None`.

## Token contract

| Source | Kind | Treatment |
| --- | --- | --- |
| `(` `)` `[` `]` `{` `}` | `OpenParen` / `CloseParen`, `OpenBracket` / `CloseBracket`, `OpenBrace` / `CloseBrace` | Each delimiter is a token; matching belongs to the reader. |
| Symbols, keywords, numbers, booleans, `nil`, `%`, `%1`, `%&` | `Atom` | Raw text; classification and contextual interpretation belong to the reader. |
| `"text"` | `String` | Includes quotes; backslashes protect the next character from delimiter recognition. |
| `#"pattern"` | `Regex` | Includes `#` and quotes; regex escapes are preserved. |
| `\a`, `\)`, `\space`, `\u03bb`, `\o141` | `Character` | Includes the backslash and complete character spelling. |
| `;comment`, `#!comment` | `Comment` | Emitted, excluding the terminating CR/LF. `#!` is recognized wherever a dispatch form can start. |
| `'` / backtick / `~` / `~@` | `Quote` / `SyntaxQuote` / `Unquote` / `UnquoteSplicing` | Prefix only. |
| `@` | `Deref` | Prefix only. |
| `^`, legacy `#^` | `Metadata` | Prefix only; the metadata form follows. |
| `#{` | `SetStart` | Includes the opening brace. |
| `#(` | `AnonymousFnStart` | Includes the opening parenthesis. |
| `#'` | `VarQuote` | Prefix only. |
| `#_` | `Discard` | Does **not** discard the following form; all its tokens are emitted. |
| `#?`, `#?@` | `ReaderConditional`, `ReaderConditionalSplicing` | Does not select or skip branches. |
| `#:ns`, `#::alias`, `#::` | `NamespacedMap`, `AutoNamespacedMap` | Includes the adjacent namespace/alias spelling; the map brace is separate. |
| `##` | `SymbolicValue` | Prefix; `Inf`, `-Inf`, or `NaN` follows as an atom. |
| `#js`, `#inst`, `#uuid`, `#app/tag` | `Dispatch` then `Atom` | `#` and the tag are separate tokens; no tag handlers run. |
| `#=` | `ReadEval` | Recognized for reader compatibility; never evaluated. |
| `#<` | `Error(UnreadableForm)` | This dispatch sequence is explicitly unreadable. |

An unterminated string/regex returns `Error(UnterminatedString)` or
`Error(UnterminatedRegex)`, spanning from its opening delimiter to EOF. A lone
backslash or hash at EOF returns `Error(MissingCharacter)` or
`Error(MissingDispatch)`. Errors always advance; `#<` permits scanning the rest
of the input, while unterminated literals consume the remainder. Missing forms
after prefixes such as `#_` or `'` are reader errors, not lexical EOF errors.

Quotes, commas, semicolons, and brackets inside literals are literal content.
Multiline strings and regexes are accepted lexically. Character literals consume
a literal first character even if it is whitespace or a delimiter; remaining
named/Unicode/octal spellings run to a symbol terminator. A character consisting
of CRLF or CRFF is kept together to match the indexing reader's normalization.

## Boundaries and Unicode

Whitespace follows the **JVM** compiler's vendored `tools.reader`, not Rust's
`char::is_whitespace` or the JavaScript host's whitespace set. Commas, ASCII
`09`–`0D`, `1C`–`20`, U+1680, U+2000–2006, U+2008–200A, U+2028–2029, U+205F,
and U+3000 are skipped. NBSP, NEL, figure space, narrow NBSP, zero-width space,
and BOM remain atom content. Inside comments, only CR or LF ends the comment.

Atoms end at whitespace or a macro-terminating character:
`" ; @ ^` backtick `~ ( ) [ ] { } \`.
The characters `#`, `'`, `:`, and `%` can appear **inside** symbol-like atoms:
`foo#`, `foo'`, and `foo:bar` each stay whole. At the beginning of a form, the
relevant reader prefixes are recognized. Number candidates additionally end at
all macro characters: `1:kw` produces atoms `1` and `:kw`, whereas `x:kw` is one
atom. Digit detection matches the reader's UTF-16 `Character.isDigit(char)`
classification, including BMP decimal digits; that does not validate numbers.

The independent [reader class snapshot](tests/reference/reader_classes.txt) is
regenerated using [the reference script](tests/reference/reader_classes.clj).
Compatibility is based on the local ClojureScript checkout at
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` and its vendored reader, especially
`macro-terminating?`, `read-token`, `read-number`, `read-char*`, `read-string*`,
`read-regex`, `macros`, and `dispatch-macros` in
`research/clojurescript/src/main/clojure/cljs/vendor/clojure/tools/reader.clj`.

## Reader work remains separate

Lexical coverage does not mean a complete reader or source validator. The lexer
preserves escape sequences and character spellings without decoding or validating
them; for example, `"\q"` is a `String` and `\unknown` a `Character` whose values
a reader must reject. Regex compilation, numeric/symbol/keyword validation,
namespace resolution, tag resolution, metadata application, map arity, nested
anonymous-function restrictions, and delimiter matching are also reader work.

Context-dependent `%` notation and syntax-quote gensyms remain raw atoms. For
example, `%&rest` and `%1:kw` each remain one atom; inside `#()` the reader must
interpret the raw span as argument shorthand followed by another form. Outside
`#()` the same text can be a symbol. This lexer intentionally keeps no nesting
or reader context, and never executes reader evaluation.

## Inspect tokens

```sh
cargo run -- --text '(+ 1 2)'
cargo run -- tests/fixtures/reader_forms.cljs
printf '(café,λ)' | cargo run --quiet
```

```text
OpenParen   0..1   "("
Atom        1..2   "+"
Atom        3..4   "1"
Atom        5..6   "2"
CloseParen  6..7   ")"
```

The CLI reads a UTF-8 file, `-`, or stdin, then prints tab-separated kinds, byte
spans, and escaped source text. It owns the input buffer; the scanner borrows it.
Lexical errors are printed as tokens and cause a nonzero exit status, as do
invalid UTF-8 and I/O errors.

## Verify and measure

```sh
cargo test
cargo test --release
cargo clippy --all-targets -- -D warnings
cargo bench --bench tokenizer -- --samples 9 --ms 150

# Optional: requires the gitignored reference checkout.
cargo test --release --test corpus -- --ignored --nocapture
cargo bench --bench tokenizer -- --samples 9 --ms 150 \
  research/clojurescript/src/main/cljs/cljs/core.cljs \
  research/clojurescript/src/main/clojure/cljs/analyzer.cljc
```

The benchmark prepares eight fixtures before timing and accepts additional UTF-8
files. It consumes full tokens through `black_box`, emits raw CSV on stdout, and
reports configuration and medians on stderr. See the
[measurements and methodology](benchmarks/README.md). Historical subset scanner
experiments are preserved separately under `cargo bench --bench subset` and
must not be described as throughput for this expanded scanner or a complete
ClojureScript reader/compiler.
