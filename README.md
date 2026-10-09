# light-cljs

A small Rust compiler for an executable ClojureScript core, with a borrowing
lexer, handwritten syntax parser, semantic analyzer, and JavaScript emitter.
The parser pulls tokens into a node arena; analysis resolves local references
to binding IDs in a separate expression arena. Compiler macros run in the native
Rust interpreter, with explicit `.clj`/`.cljc` imports and a bounded executable JS
subset. The library has no dependencies or unsafe code.
Tokens never allocate text; syntax leaves keep byte spans wherever possible.

A separate native SCI-style interpreter has owned scalar and immutable collection
values, metadata, structural equality/hashing, source storage, semantic reading,
owned forms, primitive evaluation, user/native macro expansion, and explicit garbage collection in
`light_cljs::interpreter`. Its
[compatibility profile](docs/interpreter.md) and
[pinned JVM reference corpus](tests/reference/sci/README.md) define the target;
native calls, explicit host function registration, checked arithmetic and general
predicates, lexical closures, multiple/variadic arities, mutual recursion and
stack-safe `loop*`/function `recur`, context-owned namespaces, definitions and
late-bound Vars, portable exceptions, `try`/`catch`/`finally`, collection core
functions, callable data, finite seq traversal, reductions and eager higher-order
helpers, ordinary destructuring, `let`/`loop`/`fn`/`defn`/`letfn`, and function
pre/post conditions, conditional/threading macros, assertions and constant-data
`case`, memoized delays, real lazy-sequence producers, lazy `for` comprehensions,
effectful `doseq`/`dotimes`/`while` iteration, dynamic Vars, atoms, validators,
watches and volatiles, bounded readable printing, core string/name operations and
the literal `clojure.string` subset, explicit host input/output, incremental read
APIs and nested string I/O scopes, set algebra/relational operations, data walks,
and restricted EDN reading work. Native `--eval` and `--repl` modes run scripts
without Node, Java, SCI, or generated JavaScript.

```rust
use light_cljs::interpreter::{eval_string, ValueView};
let result = eval_string("(let [{:keys [x]} {:x 10}] ((fn [y] (+ x y)) 2))").unwrap();
assert_eq!(result.inspect(), ValueView::Integer(12));
```

## Run native Clojure scripts and a REPL

```sh
cargo run --quiet -- --eval --text '(reduce + (range 1 6))'
# 15
cargo run --quiet -- --eval --source-root examples/native examples/native/demo.clj
# Hello from native Rust
# {:squares [1 4 9 16 25], :sum 15}  (map order may differ)
printf '(+ 20\n 22)\n(* *1 2)\n' | cargo run --quiet -- --repl
# 42
# 84
cargo run --quiet -- --repl
```

`--eval` accepts a UTF-8 file, stdin/`-`, or `--text`. Program printing goes to
stdout immediately; the readable last value appears only after success.
`--repl` keeps definitions, namespaces and `*1`/`*2`/`*3`/`*e`, reads multiline
forms, and continues after language errors. Prompts go to stderr for terminal
input. Use `--source-root DIR` for required `.clj`/`.cljc` files; no source path
is discovered implicitly. `--help` lists resource and result-printing controls.
See the [native CLI contract](docs/interpreter.md#native-script-and-repl-commands)
for stream ownership, limits, error behavior, and supported scope.

The [complete native examples](examples/native) include namespace data processing,
custom macros, a lazy pipeline, stateful callbacks and recoverable exceptions.
[Conformance coverage](tests/reference/sci/README.md#integrated-native-conformance)
accounts for every supported language name and executes all pinned SCI cases
with explicit assertions for declared differences.

## Execute small programs

```sh
cargo run --quiet -- --compile examples/closure.cljs \
  | node --print 'eval(require("fs").readFileSync(0, "utf8"))'
# 10

cargo run -- --compile program.cljs > program.js
cargo run -- --analyze --text '(let* [x 10] x)'
```

`--compile` emits a self-contained JavaScript expression whose value is the
last source form. Supported forms are `nil`, booleans, safe integers, finite
floating-point numbers, strings, `if`, `do`, sequential `let*`, and single fixed
arity `fn*` functions and calls. Closures capture lexical bindings; named `fn*`
supports self-recursion. Empty bodies return `nil`. Only `nil` and `false` are
false in conditionals, including when tests produce `0` or `""`.

```rust
use light_cljs::{analyze, emit_js, parse};

let source = "(let* [x 10 f (fn* [] x)] (let* [x 20] (f)))";
let syntax = parse(source).unwrap();
let program = analyze(source, &syntax).unwrap();
let javascript = emit_js(&program);
// Or use light_cljs::compile(source) for all three stages.
```

Ordinary `let`, `fn`, `defn`, and selected control/threading macros expand natively.
User macros use explicit `:require-macros` imports; compile-time interpreter Vars
remain separate from runtime JavaScript globals. The complete example has two
runtime namespaces and one macro namespace:

```sh
cargo run --quiet -- --compile --entry demo.main --source-root examples/macros | node
# 12
```

See [compiler macros and embedding](docs/compiler.md#compiler-macros-and-embedding)
for imports, host macros, reload, `&env`, resource limits, and runtime restrictions.

Compilation errors include source locations and produce no partial JavaScript.
Definitions and narrow JavaScript interop are also supported. Compile the
included two-namespace example with explicit source roots:

```sh
cargo run --quiet -- --compile --entry demo.main \
  --source-root examples/namespaces > /tmp/light-demo.js
node /tmp/light-demo.js
# 4
```

Namespace loading supports ordinary `:require`/`:as` aliases and separate
`:require-macros` imports. Runtime collections, general library integration,
constructors, property writes, and optimization remain unsupported. See the
[executable core contract](docs/compiler.md) for exact limits and testing.

## Lexer API

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

## Lexer versus parser

Lexical coverage does not mean a complete reader or source validator. The lexer
preserves escape sequences and character spellings without decoding or validating
them; for example, `"\q"` is a `String` and `\unknown` a `Character` whose values
a reader must reject. Regex compilation, numeric/symbol/keyword validation,
namespace resolution, tag resolution, metadata application, map arity, nested
anonymous-function restrictions, and delimiter matching are also later work.
The [parser](docs/parser.md) now handles spelling validation, string/character
decoding, delimiters, ordinary map arity, and anonymous-function syntax. Reader
expansion and namespace-sensitive meanings remain deferred.

Context-dependent `%` notation and syntax-quote gensyms remain raw atoms. For
example, `%&rest` and `%1:kw` each remain one atom; inside `#()` the reader must
interpret the raw span as argument shorthand followed by another form. Outside
`#()` the same text can be a symbol. This lexer intentionally keeps no nesting
or reader context, and never executes reader evaluation.

## Parse syntax

```rust
use light_cljs::parse;

let source = "(+ 1 2)";
let tree = parse(source).unwrap();
let root = &tree.nodes[tree.roots[0].0];
assert_eq!(root.span, 0..7);
```

`SyntaxTree` owns a `Vec<Node>` and root IDs; retain the original source alongside
it. Collections store `Vec<NodeId>` children. `Parser::read_form()` also supports
reading and inspecting one top-level form at a time. It uses one-token lookahead,
skips comments and whole-form `#_` discards, and limits nesting to 128 levels.

```sh
cargo run -- --parse --text '(+ 1 2)'
cargo run -- --parse tests/fixtures/parser_forms.cljs
cargo run -- --parse research/clojurescript/src/main/cljs/cljs/core.cljs
```

Parser output is a flat arena listing with child IDs, byte spans, and escaped
source text. Parse errors include line/byte-column, the failing span, and an
opening span when available. See [the parser contract](docs/parser.md) and
[measured construction costs](benchmarks/parser.md).

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
cargo bench --bench parser -- --samples 9 --ms 100

# Optional: requires the gitignored reference checkout.
cargo test --release --test corpus -- --ignored --nocapture
cargo test --release --test parser_corpus -- --ignored --nocapture
# Required CI conformance, run explicitly locally (Java/Clojure CLI).
cargo test --test execution -- --ignored --nocapture
cargo bench --bench tokenizer -- --samples 9 --ms 150 \
  research/clojurescript/src/main/cljs/cljs/core.cljs \
  research/clojurescript/src/main/clojure/cljs/analyzer.cljc
```

Compiler execution and compiler CLI tests require Node.js on `PATH`, or `NODE`
set to its executable path. Native interpreter tests need only Rust; run
`cargo test --test native_cli` for the subprocess suite. The compiler differential
test uses the same tiny programs and compares
values and observable effects; it may download the pinned Maven dependencies.

The benchmark prepares eight fixtures before timing and accepts additional UTF-8
files. It consumes full tokens through `black_box`, emits raw CSV on stdout, and
reports configuration and medians on stderr. See the
[measurements and methodology](benchmarks/README.md). Historical subset scanner
experiments are preserved separately under `cargo bench --bench subset` and
must not be described as throughput for this expanded scanner or a complete
ClojureScript reader/compiler.

For the full native host API, run `cargo run --example embedding` or read the
[embedding example](examples/embedding.rs) and the documented
[options, permission and fork contracts](docs/interpreter.md#embedding-configuration-permissions-and-forks).
