# Supported JavaScript grammar

The source frontend (`src/frontend/`, entry point `frontend::parse_script`)
accepts the small, deliberately documented subset of JavaScript below and
rejects everything else with a positioned `SyntaxError`. Requirements:
[`design.md`](design.md) §§7, 12–14. This document and the parser are kept in
agreement by tests: every `js accept` example below must parse, and every
`js reject` example must fail with the stated error kind at the stated
`line:column` (`frontend::tests::grammar_doc_examples`).

Status: everything below parses. Scripts made of literals other than
strings, bindings, operators, assignment, blocks, `if`/`else` and `while`
also compile to VM bytecode and execute (`frontend::compile_script`; see
"Execution" below). String values, calls, property access, functions and
closures parse but are rejected by the compiler until later pellets
(clj-quickjs-21, -20, -4).

## Semantics model

- All source is **strict-mode** code (as if it began with `"use strict"`):
  strict reserved words, legacy octal literals and escapes, `with`, and
  binding `eval` or `arguments` are rejected.
- Accepted forms keep ordinary JavaScript meaning. A form whose meaning the
  subset does not implement is rejected, never reinterpreted.
- The subset additionally **reserves** the global constants `undefined`,
  `NaN` and `Infinity`: they parse as literals and cannot be declared,
  used as parameters, or assigned. (In JavaScript they are non-writable
  globals, so this only forbids shadowing.) It also reserves `await` and
  `yield` everywhere.

## Lexical grammar

| Element | Accepted | Rejected |
|---------|----------|----------|
| Whitespace | TAB, VT, FF, SP, NBSP, ZWNBSP (BOM), Unicode `Zs` | – |
| Line terminators | LF, CR, CRLF, U+2028, U+2029 | – |
| Comments | `// ...` and `/* ... */` (a block comment containing a line terminator counts as one for ASI) | unterminated `/*` (truncated input); HTML-like `<!--` / `-->` comments (they lex as operators and hit `--`) |
| Identifiers | ASCII `[A-Za-z$_][A-Za-z0-9$_]*` | non-ASCII identifier characters, `\u` escapes in identifiers |
| Reserved words | all ECMAScript reserved words plus the strict-mode ones (`let`, `static`, `yield`, `implements`, `interface`, `package`, `private`, `protected`, `public`) and `await`; any of these may follow `.` as a property name | as binding names, assignment targets or references |
| Numbers | decimal integers, fractions (`1.5`, `.5`, `5.`), exponents (`1e-3`), `0x`/`0o`/`0b` integers; values are IEEE-754 doubles, correctly rounded; overflow gives `Infinity` | legacy octal / leading zeros (`017`, `08`), numeric separators (`1_000`), BigInt (`10n`), a literal immediately followed by an identifier or digit (`3in`, `1.toString`), missing digits (`0x`, `1e`) |
| Strings | `'...'` and `"..."` with escapes `\n \t \r \b \f \v \0 \xHH \uHHHH \u{H...}`, surrogate-pair `\u` escapes (combined), line continuations, identity escapes (`\'`, `\\`, `\q`), raw U+2028/U+2029 | unterminated strings, raw LF/CR inside a string, octal escapes (`\07`, `\1`), `\8` `\9`, malformed `\x`/`\u`, lone surrogate escapes (strings hold Unicode scalar values) |
| Template literals | – | any backtick |
| Regular expressions | – | a `/` where an expression is expected (the parser decides; `a / b / c` is division) |
| Punctuators | every ECMAScript punctuator is lexed by maximal munch (`a>>>=b` is `a`, `>>>=`, `b`) so the parser can name an unsupported operator precisely | other characters (`@`, `#`, ...) |

## Syntactic grammar

EBNF; `{ x }` is repetition, `[ x ]` is optional. Terminals are quoted.

```
Script              = { StatementListItem } EOF
StatementListItem   = FunctionDeclaration | LexicalDeclaration | Statement
Statement           = Block | ";" | IfStatement | WhileStatement
                    | ReturnStatement | ExpressionStatement
Block               = "{" { StatementListItem } "}"
IfStatement         = "if" "(" Expression ")" Statement [ "else" Statement ]
WhileStatement      = "while" "(" Expression ")" Statement
ReturnStatement     = "return" [no LineTerminator here] [ Expression ] ";"
ExpressionStatement = Expression ";"     (* not starting with "{" or "function" *)
LexicalDeclaration  = ( "let" | "const" ) Declarator { "," Declarator } ";"
Declarator          = BindingIdentifier [ "=" AssignmentExpression ]
FunctionDeclaration = "function" BindingIdentifier FunctionRest
FunctionExpression  = "function" [ BindingIdentifier ] FunctionRest
FunctionRest        = "(" [ BindingIdentifier { "," BindingIdentifier } [ "," ] ] ")"
                      "{" { StatementListItem } "}"

Expression            = AssignmentExpression
AssignmentExpression  = ConditionalExpression
                      | Identifier "=" AssignmentExpression
ConditionalExpression = LogicalOr [ "?" AssignmentExpression ":" AssignmentExpression ]
LogicalOr             = LogicalAnd { "||" LogicalAnd }
LogicalAnd            = Equality { "&&" Equality }
Equality              = Relational { ( "==" | "!=" | "===" | "!==" ) Relational }
Relational            = Additive { ( "<" | ">" | "<=" | ">=" ) Additive }
Additive              = Multiplicative { ( "+" | "-" ) Multiplicative }
Multiplicative        = Unary { ( "*" | "/" | "%" ) Unary }
Unary                 = ( "!" | "-" | "+" ) Unary | Call
Call                  = Primary { Arguments | "." IdentifierName }
Arguments             = "(" [ AssignmentExpression { "," AssignmentExpression } [ "," ] ] ")"
Primary               = Identifier | Literal | FunctionExpression | "(" Expression ")"
Literal               = Number | String | "true" | "false" | "null"
                      | "undefined" | "NaN" | "Infinity"
```

Notes on the grammar:

- Precedence, loosest to tightest: assignment (right-associative),
  conditional (right-associative), `||`, `&&`, equality, relational,
  additive, multiplicative, prefix unary, call/member. All binary operators
  are left-associative. This is ECMAScript's ordering restricted to the
  supported operators.
- `ReturnStatement` is accepted only inside a function body.
- `FunctionDeclaration` and `LexicalDeclaration` are statement-list items:
  allowed in the script, a function body or a block, but not as the direct
  body of `if`/`else`/`while` (strict mode rejects that; wrap it in a block).
- Function parameter names must be distinct (strict mode).
- `BindingIdentifier` excludes reserved words, `undefined`, `NaN`,
  `Infinity`, `eval` and `arguments`.
- The assignment target may be parenthesized (`(a) = 1`), as in JavaScript.
- `a.b` (an `IdentifierName` after `.`, reserved words included) is parsed
  so later stages can resolve compiler-reserved intrinsic bindings such as
  `__rt.need` (clj-quickjs-21). The subset has no object values; assigning
  to a property is rejected.
- `const` requires an initializer.

### Automatic semicolon insertion

A statement-ending `;` may be omitted when the next token is `}`, the end
of input, or is preceded by a line terminator. A line break never ends a
statement the next token can continue, exactly as in JavaScript: `a` newline
`(b)` is a call, and `a = b` newline `+c` is `a = b + c`. `return`
followed by a line break returns `undefined` (restricted production).
Postfix `++`/`--` after a line break begins a new statement (where it is
rejected as unsupported). No semicolon is ever inserted to create an empty
statement, so `if (x)` newline `else y` is an error.

## Rejected forms

Every form below is rejected where it starts. Error kinds:
`UnknownToken`, `InvalidToken`, `UnexpectedEnd` (truncated input),
`UnexpectedToken`, `Unsupported` (valid JavaScript outside the subset),
`ReservedWord`, `Invalid` (a JavaScript early error), `TooDeep`.

| Form | Kind |
|------|------|
| `var`, `for`, `do`-`while`, `break`, `continue`, `switch`, labelled statements, `debugger`, `with` | Unsupported |
| `throw`, `try`/`catch`/`finally` (planned: clj-quickjs-11) | Unsupported |
| classes, `new`, `this`, `super`, `import`/`export`, dynamic `import` | Unsupported |
| `async` functions and arrows, `await`, generators (`function*`) | Unsupported |
| arrow functions, rest/default parameters, destructuring | Unsupported |
| object and array literals, computed member access `a[b]`, optional chaining `?.`, spread `...` | Unsupported |
| template literals, regular expression literals, BigInt, numeric separators | Unsupported |
| `typeof`, `void`, `delete`, `~`, `++`, `--`, `**`, `&`, `|`, `^`, `<<`, `>>`, `>>>`, `??`, `in`, `instanceof`, the comma operator, compound assignment (`+=`, `&&=`, ...) | Unsupported |
| assignment to a property (`a.b = 1`) | Unsupported |
| references to `eval` or `arguments` | Unsupported |
| reserved word as a binding name or reference; binding or assigning `undefined`/`NaN`/`Infinity`; binding `eval`/`arguments` | ReservedWord |
| `return` outside a function, `const` without initializer, duplicate parameter, declaration as an `if`/`while` body, invalid assignment target (`1 = 2`) | Invalid |
| nesting over the `MAX_NESTING` budget (256 units), or a syntax tree more than 256 nodes high; see "Nesting limit" below | TooDeep |

## Explicit gaps

Known differences from full JavaScript within the accepted forms, and
checks left to later stages:

- **Scope errors are checked by the compiler, not the parser.**
  `parse_script` accepts redeclaration (`let a; let a;`) and a `let` that
  repeats a parameter name; `compile_script` rejects them (see
  "Execution"). Temporal-dead-zone use and assignment to a `const` are
  run-time errors, as in JavaScript.
- **Nesting limit.** Nesting is charged against one budget of 256 units
  (`MAX_NESTING`); input that exceeds it is rejected with `TooDeep`. Costs
  per level, weighted by the Rust stack each construct uses: a statement 1,
  plus 1 for a block or an `if`/`while`; a function 3 plus its body block;
  an assignment 1; `? :` 1 plus its branch assignments; a prefix operator
  or one link of a binary-operator or member chain 1; each call's
  argument list 3 plus each argument's assignment; parentheses 3 plus the
  inner assignment. So about 64 nested parentheses or calls, 128 nested
  blocks or `else if` links, 64 nested function declarations, 42 nested
  function expressions, or a chain of about 250 binary operators fit. The whole budget fits a 2 MiB thread stack
  even in a debug build (tested). Separately, the syntax tree itself may be
  at most 256 nodes high (statements, functions and expressions on any
  root-to-leaf path, counted by each node's `height`), which also catches
  finished subtrees wrapped again, such as `(a.b.b).b.b`; later recursive
  passes can rely on that bound.
- **Strings are Unicode scalar values**, not UTF-16 code-unit sequences:
  a lone surrogate escape is rejected, so string length and indexing
  semantics are not yet decided.
- **Identifiers are ASCII only.**
- **Diagnostics stop at the first error**; there is no error recovery.
  Positions are byte spans plus 1-based line and column (columns count
  Unicode scalar values).
- **Directive prologues are not interpreted**: `"use strict"` is an ordinary
  expression statement, and every script is strict anyway.
- Hashbang comments (`#!`) are rejected as an unknown token.

## Execution

`frontend::compile_script(name, source)` parses, resolves bindings and lowers
a script to a validated `Executable` (`src/frontend/lower.rs`); a task
running it completes with the script's **completion value**, the value
JavaScript `eval` would return (the last expression statement that ran;
`if` and `while` produce `undefined` unless their body produced a value).
`frontend::compile_function(source)` compiles a source made of exactly one
function declaration into an entry point whose parameters are the task's
arguments (missing ones are `undefined`); `return` completes the task and
falling off the end returns `undefined`.

Executed, with JavaScript semantics:

| Form | Semantics |
|------|-----------|
| Number, `true`/`false`, `null`, `undefined`, `NaN`, `Infinity` literals | IEEE-754 doubles; `-0` is preserved; no constant folding |
| `let`, `const`, parameters, block scoping and shadowing | one frame local per live binding; `let x;` sets `undefined` each time it runs (loop bodies included) |
| `+ - * / %` and unary `-`, `+` | ToNumber on `undefined` (NaN), `null` (0), booleans (0/1), then IEEE arithmetic; `%` keeps the dividend's sign |
| `< > <= >=` | ToNumber, then numeric comparison (anything with NaN is false) |
| `===`, `!==` | IsStrictlyEqual (`NaN !== NaN`, `0 === -0`) |
| `==`, `!=` | IsLooselyEqual for the supported types: `null == undefined`, neither equals anything else, booleans compare as numbers |
| `!`, `&&`, `\|\|`, `? :`, `if`, `while` | ToBoolean (falsy: `0`, `-0`, `NaN`, `null`, `undefined`, `false`); `&&`/`\|\|` short-circuit and return an operand |
| `x = v` | evaluates `v`, then stores; the expression's value is `v` |
| `return` (in `compile_function`) | completes the task |

Errors:

| Situation | When | Reported as |
|-----------|------|-------------|
| A name declared twice in one block, script or function body (including a `let` that repeats a parameter) | compile time | `SyntaxError`, kind `Invalid`, at the second declaration |
| String values, calls, property access, function declarations and expressions, and (in `compile_function`) unshadowed uses of the entry function's own name | compile time | `SyntaxError`, kind `Unsupported`, at the form |
| More than 65,535 live bindings or 65,536 distinct constants | compile time | `SyntaxError`, kind `TooLarge` |
| Reading or assigning a name with no binding (strict mode; there is no global object) | run time, only if reached, after the assigned value is evaluated | `VmErrorKind::ReferenceError`: "x is not defined" |
| Reading or assigning a `let`/`const` before its declaration ran (temporal dead zone), including inside its own initializer | run time, only if reached | `VmErrorKind::ReferenceError`: "cannot access 'x' before initialization" |
| Assigning to an initialized `const` | run time, only if reached, after the value is evaluated | `VmErrorKind::TypeError`: "assignment to constant variable 'x'" |

Run-time failures carry the source position (`Diagnostic::source`, line and
column) of the failing reference or operator. These errors cannot be caught
until `throw`/`try` exist (clj-quickjs-11).

Gaps specific to execution:

- **No string values**, so `+` and the relational operators lower to
  numeric operations; they must change when strings arrive.
- **No objects from source.** Operators applied to objects (reachable only
  through hand-built bytecode today) fail with `UnsupportedOperand` where
  JavaScript would call ToPrimitive.
- **No functions or closures yet.** Because the subset has no closures,
  `break`, `continue` or labels, whether a binding is initialized is known
  from source order alone; the compiler decides it statically. Closures
  will need a run-time check.

## Examples

Accepted (each block is one script):

```js accept
// Declarations, functions, calls, nested blocks.
function add(a, b) { return a + b; }
const total = add(1, 2 * 3);
let i = 0, acc;
acc = 0;
while (i < total) {
  if (i % 2 === 0) { acc = acc + i; } else if (i > 4) { { acc = acc - 1; } }
  i = i + 1;
}
```

```js accept
let make = function (x,) {
  return function inner(y) { return x === y ? "same" : (x < y || -x >= +y); };
};
make(1)(2,)
```

```js accept
let a = 1
let b = a
/ 2 / 1
const s = 'it\'s \u{1F600} \x41\
!'
```

```js accept
const f = __rt.need("defn/foo", "callable")
f.call(undefined, NaN, Infinity, null, true, false, 0x1F, 0b101, 0o17, .5, 5., 1e3)
```

Rejected (`kind line:column` of the reported position):

```js reject Unsupported 1:9
let r = /ab+c/g;
```

```js reject Unsupported 1:1
class Point {}
```

```js reject Unsupported 2:1
let x = 1;
async function f() {}
```

```js reject Unsupported 1:9
let g = (a, b) => a + b;
```

```js reject Unsupported 1:5
x = `template`;
```

```js reject Unsupported 1:2
a++;
```

```js reject Unsupported 1:2
a>>>=b
```

```js reject ReservedWord 1:5
let if = 1;
```

```js reject ReservedWord 1:7
const undefined = 1;
```

```js reject Invalid 1:1
return 1;
```

```js reject Invalid 1:7
const c;
```

```js reject UnexpectedToken 1:11
let a = 1 let b = 2;
```

```js reject UnexpectedEnd 2:15
function f(a) {
  return a + 1
```

```js reject InvalidToken 1:11
let n = 3.toFixed;
```

```js reject UnknownToken 1:9
let n = @x;
```

## References

Structure consulted (no code adapted): `research/quickjs/quickjs.c` at
revision 7d1c13c5d8f8 (`VERSION` 2026-06-04): `next_token`,
`js_parse_expr_binary`, `js_parse_assign_expr`, `js_parse_statement`,
`js_parse_expect_semi`; and the ASI cases of `test_parse_semicolon` in
`research/quickjs/tests/test_language.js` (its `yield`/`await`/arrow cases
are outside the subset). The tests in `src/frontend/tests.rs` are written
for this project and are not adapted from QuickJS.
