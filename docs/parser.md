# Syntax parser

`parse(&str) -> Result<SyntaxTree, ParseError>` constructs syntax from the
borrowing tokenizer. It is handwritten recursive descent with one cached token,
loops for siblings, and recursion only for nested forms. It never buffers the
entire token stream. Production code uses safe Rust and the standard library.

## Ownership and streaming

The caller owns the UTF-8 source. `SyntaxTree` owns `Vec<Node>` and `Vec<NodeId>`
roots. Each node contains a kind and half-open byte span; collection children
live in ordinary `Vec<NodeId>`s. IDs are `usize` indices local to one arena and
stay valid across vector growth. Children precede their parents in the arena.
Keep the exact source buffer alongside the tree for slicing spans.

Symbols, keywords, numbers, and regexes retain spelling by span. Ordinary strings
store an interior source span. Strings requiring escapes or CR normalization
store decoded `Vec<u16>` values; UTF-16 preserves unpaired surrogate escapes
legal in JVM and JavaScript strings. `StringValue::to_utf16(source)` materializes
a copy on request. That method is not used during parsing or benchmarking.
Character literals decode to a BMP Rust `char`; surrogate character escapes and
supplementary character literals are rejected to match the JVM reader.

For a compiler driver, use `Parser::new(source)` and call `read_form()` repeatedly.
Each successful call returns one root ID; inspect it through `nodes()`, perform
analysis/update namespace context, then request another form. The parser itself
does not yet accept or apply namespace context. `into_nodes()` transfers its
arena when finished. Reading multiple forms retains their nodes; it does not
implement per-form arena recycling.

## Supported construction

- Lists, vectors, maps, sets, and anonymous-function bodies; every collection has
  a full span including delimiters. Siblings do not consume recursive stack depth.
- Symbols, normal and auto-resolved keyword syntax, booleans, `nil`, integers,
  `N` big-integer spellings, floats, `M` decimals, ratios, and `##Inf`/`##-Inf`/`##NaN`.
  Numeric grammar is validated without choosing a runtime representation or
  converting arbitrarily large integers. Zero ratio denominators are rejected.
- Strings with standard, Unicode, and octal escapes; named, Unicode, octal, and
  literal characters. CR, CRLF, and CRFF in strings normalize to LF, following
  the reference indexing reader. Numeric escapes accept the reference BMP digit
  classes and fullwidth hexadecimal letters.
- Quote, syntax quote, unquote, unquote-splicing, deref, and var quote as prefix
  nodes; metadata as a metadata/target pair; tagged literals as tag/form pairs.
- Namespaced maps with the namespace/alias span and a map child. Auto-resolved
  namespaces remain marked for later resolution.
- Reader conditionals retain all feature/form pairs. Features must be ordinary
  keywords, with `:else` and `:none` rejected as reserved. `#?@` requires collection
  context. Ordinary maps must have even arity; maps containing direct conditional
  children defer arity validation until selection and splicing determine members.
- Anonymous-function argument nodes, including splitting `%&rest` and `%1:kw`
  inside `#()`. Explicit integer indices retain their source-backed numeric node.
  Ratio indices such as `%1/1` and `%3/2` are accepted structurally; the semantic
  reader applies its selected compatibility profile to these spellings. Nested `#()` is rejected.

Comments disappear. `#_` reads and validates one complete following form, then
removes that subtree from the arena. For `#_#_a b c`, only `c` remains. Discarded
collections/literals can allocate temporarily; their arena capacity is reused.
An opt-in `Parser::retaining_discards()` builder retains discarded subtrees in
a `discarded()` root side table for semantic validation. Ordinary children and
roots still omit them. Default `parse` and `Parser::new` retain their existing
discard-removal behavior.
The tree is not a lossless concrete syntax tree, although enclosing spans retain
the original whitespace/comments/discards between their delimiters.

## Errors and deferred meanings

Errors include unexpected/mismatched closing delimiters, missing forms,
unterminated literals, invalid spelling/escapes, odd ordinary maps, malformed
prefix operands, and excessive nesting. `ParseError` provides a primary byte
span plus an optional related opener/prefix span. EOF spans are empty at the
end of the source; all spans are valid UTF-8 slicing boundaries.

The default and hard maximum nesting limit is 128, counting nested collections,
prefixes, and discard operands. `with_nesting_limit` can lower it (larger requests
are clamped). A flat vector may have any number of siblings subject to memory.
On the first error, the current top-level subtree is rolled back; earlier forms
remain intact. Subsequent `read_form()` calls return `Ok(None)`, as after EOF.
There is no error recovery. `LineIndex` builds newline offsets separately and
reports 1-based line and **byte** column positions; the CLI constructs it only
when reporting a parser error.

This is a syntax parser, not a full ClojureScript reader or compiler. It does not
resolve aliases or symbols; apply/desugar metadata or validate metadata targets;
expand quotes, gensyms, or anonymous functions; choose conditional branches;
namespace map keys; invoke tags/constructors; compile regex patterns; detect
duplicate map keys/set members; or materialize/reduce numeric values. Numeric
overflow and target-specific restrictions belong to value construction. `if`,
`let`, and `defn` are ordinary symbols. Tagged literals and patterns can therefore
be structurally valid but semantically invalid. `#=` returns `ReadEvalDisabled`;
it is never evaluated, even in discarded or preserved conditional branches.

Preserving syntax is deliberately distinct from tools.reader's behavior: for
example, metadata is a node rather than merged properties, and this parser
validates conditional feature/pair structure even though reference `:preserve`
mode can retain malformed branch lists. It validates literal spelling in all
branches. Context-specific interpretation has separate compatibility tests in
`tests/compiler_reader.rs`; these execute the compiler-owned Rust reader,
canonical macro computation, and emitted JavaScript independently of parsing.

## Evidence

Tests cover incremental reading, every node family, source/child-span invariants,
UTF-16 values, invalid literals, discards, contextual argument boundaries,
delimiter diagnostics, fused errors/EOF, 100,000 vector siblings, deep malicious
nesting, malformed fragment combinations, and every UTF-8 truncation of the parser
fixture. A committed [literal snapshot](../tests/reference/parser_literals.tsv)
contains 99 cases from the vendored JVM reader; the
[generator](../tests/reference/parser_literals.clj) documents regeneration.
Normal tests need no Java or research checkout.

The committed [complete reader corpus](../tests/reference/reader-sources.tsv)
pins seven upstream source files, 137 complete bodies and their UTF-8 byte spans,
including reader/tag/syntax-quote/metadata tests and both vendored tools.reader
implementations. `python3 -B tests/reference/generate_reader_coverage.py --verify`
checks hashes, spans and generated execution fixtures using only Python. Its
`--check` mode regenerates against the pinned Git objects; neither mode runs a
JVM reader or establishes a semantic pass. `tests/parser_corpus.rs` independently
checks parsing, token progress, incremental body locations and fused EOF.
`tests/parser.rs` truncates every reader family at every UTF-8 boundary and
verifies rollback and terminal behavior after malformed partial forms.

The optional corpus test parses all 296 `.clj`, `.cljs`, and `.cljc` files in the
reference checkout at `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`: 2,834,491 bytes,
343,327 retained nodes, and 6,594 top-level forms. This is structural coverage,
not proof of semantic equivalence. See [benchmark results](../benchmarks/parser.md)
for allocation and construction measurements.
