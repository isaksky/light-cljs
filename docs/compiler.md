# Executable core

The compiler now supports the milestone “small programs execute correctly.”
It deliberately stops before definitions, namespaces, macros, collections,
library/runtime integration, or optimization.

## Pipeline and ownership

`parse(source)` constructs the existing syntax arena. `analyze(source, &syntax)`
constructs a separate `Program`; `emit_js(&program)` recursively writes one
JavaScript expression into a `String`. `compile(source)` composes these stages
and distinguishes parser errors from analysis errors. `--compile` accepts files,
stdin, or `--text`; `--analyze` prints the resolved representation. Existing token
and parser CLI modes retain their behavior.

`Program` owns `Vec<Expr>`, binding records, and UTF-16 string values. `ExprId`,
`LocalId`, and `StringId` are integer indices into those arrays. Expression and
binding records retain original byte spans. Analysis requires the exact source
used to parse the syntax; the resulting program no longer borrows that source.
IDs are local to a single compilation unit, and the emitter expects an
analyzer-produced program.

The environment is a stack of scopes containing binding IDs. Lookup walks scopes
and their bindings from newest to oldest. Binding records survive scope exit.
Each `let*` initializer is analyzed before its binding is installed; previous
bindings are visible, including earlier occurrences of the same spelling.
`fn*` installs its optional self-name and parameters before analyzing its body.
A parameter may shadow the self-name. Every emitted local name derives only from
its ID (`l0`, `l1`, …), so shadowing, Unicode names, and JavaScript reserved words
do not change binding identity.

## Supported syntax and values

| Construct | Behavior |
| --- | --- |
| `nil`, booleans | JavaScript `null`, `true`, `false`. |
| Integer literals | Signed decimal, hexadecimal, octal, and radix spellings, limited to ±(2^53−1). Larger integers report a range error. Integer `-0` is ordinary zero. |
| Floating-point literals | Finite IEEE-754 doubles; negative zero is preserved. Overflow reports a range error; underflow follows double rounding. |
| Strings | Exact decoded UTF-16, including lone surrogates. JavaScript escaping preserves quotes, backslashes, controls, and Unicode. |
| `(if test yes no?)` | Evaluate the test once and only the selected branch. The omitted branch is `nil`. |
| `(do forms...)` | Evaluate in order, returning the last value, or `nil` when empty. |
| `(let* [name init ...] body...)` | Sequential simple-symbol bindings and an implicit `do` body. Repeated names create distinct bindings. |
| `(fn* name? [params...] body...)` | A lexical closure with one fixed parameter list; optional self-name supports recursion. |
| `(fn* name? ([params...] body...))` | Equivalent single-method syntax. Multiple methods and variadic arguments are unsupported. |
| `(callee args...)` | Evaluate callee first, then arguments left to right, then call. |
| Multiple source forms | An implicit `do`; the file's result is its last value, or `nil` when empty. Top-level lexical scopes do not leak into following forms. |

Bindings must be simple symbols: no namespace slash, dot, destructuring, or `&`.
Duplicate function parameters are rejected. Calls use plain JavaScript function
invocation; this stage does not add arity checks or support collection/protocol
invocation. A literal `nil` callee is rejected during analysis. Other non-callable
values fail when invoked in JavaScript. There is no tail-call elimination.

Globals and built-in library functions, including arithmetic, are unresolved.
Characters, keywords, regexes, symbolic numbers, big integers, decimals, ratios,
empty lists, collection literals, reader prefixes, metadata, tags, reader
conditionals, and reader anonymous functions report unsupported constructs.
Parser comment/discard handling is retained. Unresolved symbols are errors even
in unselected branches. No implicit JavaScript global lookup occurs.

## Context, truthiness, and the macro boundary

Expressions carry `Value`, `Discard`, or `Return` context during analysis. The
emitter propagates those contexts recursively: conditionals can be ternary
expressions or statements; bodies discard earlier values and return their last
value when needed. `let*` and `do` in value position use small immediately invoked
arrow functions. This keeps evaluation order and avoids hoisting expressions
out of branches or call arguments.

The generated unit has a local truth helper, testing `value != null && value !==
false`. Thus `0`, negative zero, and empty strings are true. The nullish check
also handles `undefined` when a compiled function is invoked by a host harness.
Output includes no printing: evaluating it produces the program's value.

List analysis dispatches primitive special forms before touching binding vectors
or other operands. The macro hook then runs inside the analyzer, where the current
lexical environment and expression context are available, before ordinary call
analysis. Macro execution is unsupported. A small diagnostic catalog recognizes
common core macro spellings such as `let` and `fn`; it is not a complete macro
resolver. Locals shadow those macro names. Unknown names remain unresolved.

## Verification

`tests/fixtures/execution.tsv` contains the shared tiny programs and their expected
JSON values/effect traces. The execution harness wraps each program in
`(fn* [observe] ...)` and supplies a host function that records its argument and
returns it. This checks effects without adding interop or a runtime library to
the compiled language. The cases cover shadowing and closures, self-recursion,
all three emission contexts, callee/argument/initializer order, branch effects,
truthiness, numeric spellings, and UTF-16 values. Negative zero and `undefined`
are represented explicitly in test output. Separate tests cover complete files,
CLI diagnostics, invalid binding forms, and resolved IDs.

```sh
cargo test
cargo test --release
cargo clippy --all-targets -- -D warnings

# Compile those same programs with the standard compiler and compare behavior.
cargo test --test execution -- --ignored --nocapture

# Reader regression coverage stays independent of executable-language coverage.
cargo test --release --test parser_corpus -- --ignored --nocapture
```

Normal execution tests require Node.js (`NODE` can select its executable).
The optional differential test invokes `tests/reference/execution.clj` using
Clojure CLI, Java, and Maven ClojureScript **1.12.134**, compiles a temporary Node
program, and compares results and effects with the Rust compiler. Only that
reference harness uses the standard runtime and namespaces. It downloads Maven
dependencies if absent and removes its temporary build on completion. JavaScript
text is never compared. The repository corpora remain parser regression tests.
