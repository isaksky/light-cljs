# Supported JavaScript grammar

The source frontend (`src/frontend/`, entry point `frontend::parse_script`)
accepts the small, deliberately documented subset of JavaScript below and
rejects everything else with a positioned `SyntaxError`. Requirements:
[`design.md`](design.md) §§7, 12–14. This document and the parser are kept in
agreement by tests: every `js accept` example below must parse, every
`js reject` example must fail with the stated error kind at the stated
`line:column`, and every `js compile-reject` example must parse but fail to
compile that way (`frontend::tests::grammar_doc_examples`). Examples marked
`js sloppy-accept` / `js sloppy-reject` are parsed as non-strict code
(`ScriptOptions::SLOPPY`); all others are strict.

Status: everything below parses and also compiles to VM bytecode and
executes (`frontend::compile_script`; see
"Execution" below): literals (string values since clj-quickjs-65), bindings,
operators, assignment, blocks, `if`/`else`, `while`, `do`-`while`
(clj-quickjs-80), `for` with an empty, expression or `let`/`const` head
(clj-quickjs-81), function declarations
and expressions, calls, closures (clj-quickjs-21), arrow functions
(clj-quickjs-53), and the
compiler-reserved intrinsic binding `__rt` (see "Reserved intrinsic
binding"). Object literals with literal keys, `o.name` reads and
`o.name = v` assignments (clj-quickjs-114) parse and execute; method
calls `o.f()` pass their base as the receiver of a built-in function, and
the built-in global `Object` with `Object.prototype` and `Object.is`
exists (clj-quickjs-117; see "Built-in globals" under "Execution"), with
`Object.defineProperty` and `Object.getOwnPropertyDescriptor` for data
property attributes (clj-quickjs-119).
`throw` and `try`/`catch`/`finally` (clj-quickjs-11) parse and
execute; see "Exceptions" below. Unlabelled `break` and `continue` in `while`,
`do`-`while` and `for` loops (clj-quickjs-78, -80, -81) parse and execute, including
jumps out of `try` statements that run their finally blocks on the way
(clj-quickjs-79). Labelled statements and labelled `break`/`continue`
(clj-quickjs-82) parse and execute. Prefix and postfix `++`/`--` on identifier bindings
(clj-quickjs-64) parse and execute. `switch` statements (clj-quickjs-83)
parse and execute. Arithmetic, bitwise, shift and `**` compound assignment
(`+=`, `-=`, `*=`, `/=`, `%=`, `**=`, `&=`, `|=`, `^=`, `<<=`, `>>=`,
`>>>=`) on identifier bindings (clj-quickjs-84) parses and executes; the
logical assignments `&&=`, `||=` and `??=` do not. The `typeof` operator
(clj-quickjs-69) parses and executes, including on a name with no binding.
The `void` operator (clj-quickjs-133) parses and executes.
`var` statements (clj-quickjs-94) parse and execute, with function-level
hoisting; top-level function declarations of a script or function body
are var-scoped. `"use strict"` directive prologues are interpreted and
non-strict (sloppy) scripts and functions compile (clj-quickjs-95; see
"Directive prologues and strictness").

## Semantics model

- Source is **strict-mode** code by default: `parse_script`,
  `compile_script` and `compile_function` treat all code as if it began
  with `"use strict"`. The `_with` entry points (`parse_script_with`,
  `compile_script_with`, `compile_function_with`) take
  `ScriptOptions { strict }`; with `ScriptOptions::SLOPPY` the top level
  is non-strict code, as an ECMAScript script is, and a `"use strict"`
  directive makes a script or function strict (see "Directive prologues
  and strictness"). In strict code, strict reserved words, legacy octal
  literals and escapes, `with`, and binding `eval` or `arguments` are
  rejected. Sloppy code differs only where listed under "Directive
  prologues and strictness"; every other sloppy-only form is still
  rejected as in strict code.
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
| Comments | `// ...` and `/* ... */` (a block comment containing a line terminator counts as one for ASI); a hashbang comment `#! ...` at the very start of the source (byte 0, before any whitespace), running to the first line terminator, which still counts for ASI and line numbers | unterminated `/*` (truncated input); `#!` anywhere else (`UnknownToken`); HTML-like `<!--` / `-->` comments, which are not comments here: they lex as operators (`a <!-- b` is `a < !--b`) |
| Identifiers | Unicode: a start character (`ID_Start`, `$`, `_`) then part characters (`ID_Continue`, `$`, ZWNJ U+200C, ZWJ U+200D), from Unicode 16.0 tables (`src/frontend/unicode_ident.rs`, generated by `scripts/gen-unicode-ident.sh`); any of them may be written `\uHHHH` or `\u{H...}`. A name is its code points after escapes are decoded, compared without normalization: `a\u0062c` is `abc`, while `Ãµ` (U+00C3 U+00B5) and `õ` (U+00F5) are different names | a character that cannot start a token where one starts (`€`, a combining mark: `UnknownToken`); an escape that is malformed, is not `\u`, or names a code point invalid at its place (`\u0030x`, `a\u0020b`, a surrogate: `InvalidToken`); an identifier start character or `\` right after a number literal (`InvalidToken`) |
| Reserved words | all ECMAScript reserved words plus the strict-mode ones (`let`, `static`, `yield`, `implements`, `interface`, `package`, `private`, `protected`, `public`) and `await`; any of these may follow `.` or be an object literal key as a property name, also written with escapes (`a.l\u0065t` is the property `let`, so is the key in `{l\u0065t: 1}`) | as binding names, assignment targets or references; a reserved word written with escapes (`l\u0065t`, `tru\u0065`) is never the keyword and is rejected (`ReservedWord`) everywhere except right after `.` and as an object literal key (not a shorthand) |
| Numbers | decimal integers, fractions (`1.5`, `.5`, `5.`), exponents (`1e-3`), `0x`/`0o`/`0b` integers; values are IEEE-754 doubles, correctly rounded; overflow gives `Infinity`; a member access may follow a literal (`0.1.a`, `0x1.a`, `.5.a`, `1..a`: the second `.` starts the access); in sloppy code only (clj-quickjs-51; ECMAScript B.1.1), a `0` followed by digits: all octal digits make a legacy octal integer (`017` is 15), which takes no fraction or exponent, so `01.a` is a member access; an `8` or `9` among them makes a decimal literal that may take a fraction and exponent (`08` is 8, `09.5`, `08e1`) | in strict code, legacy octal / leading zeros (`017`, `08`) at the `0`, also right after a `"use strict"` directive (`"use strict"; 010`); in sloppy code, a legacy octal integer followed by a fraction (`01.5` is `01` then `.5`, `UnexpectedToken`) or an exponent (`01e1`), and a `n` suffix or separator after a leading `0` (`01n`, `08n`, `0_1`, `08_1`: `InvalidToken`); numeric separators (`1_000`), BigInt (`10n`), a literal immediately followed by an identifier or digit (`3in`, `1.toString`, `0.a`), a number literal where a property name is expected (`0.1.2`), missing digits (`0x`, `1e`) |
| Strings | `'...'` and `"..."` with escapes `\n \t \r \b \f \v \0 \xHH \uHHHH \u{H...}`, each `\u` escape is its UTF-16 code unit(s), so two surrogate escapes spell a pair (`"\u{dbff}\u{dfff}"` is U+10FFFF) and a lone surrogate escape (`\u{d800}`, a high surrogate before a non-surrogate) is a lone code unit (clj-quickjs-141), line continuations, identity escapes (`\'`, `\\`, `\q`), raw U+2028/U+2029; in sloppy code only (clj-quickjs-258; ECMAScript B.1.2), legacy octal escapes of at most three octal digits and value 0o377 (`"\07"` is U+0007, `"\101"` is `A`, `"\400"` is U+0020 then `0`; `\0` not followed by a digit is the null character in either mode) and `\8` `\9` (the characters `8` and `9`) | unterminated strings, raw LF/CR inside a string, malformed `\x`/`\u`; in strict code, octal escapes (`\07`, `\1`, `\08`) and `\8` `\9` at the `\`, also in a directive before a `"use strict"` directive (`"\07"; "use strict";`) and in the string right after one (`"use strict"; "\07"`) |
| Template literals (clj-quickjs-57) | `` `...` `` with substitutions `${ Expression }`, nested to any depth within the nesting budget; one token per part (NoSubstitutionTemplate, TemplateHead, TemplateMiddle, TemplateTail): the `}` that closes a substitution (the first `}` not matching a `{` inside it) starts the next part. The text may contain raw line terminators (CR and CRLF read as LF, U+2028/U+2029 kept; they count for line numbers but not for ASI) and the escapes of string literals, except that a digit escape other than `\0` not followed by a digit is an error in either mode; `` \` `` and `\$` are text. Each part keeps its cooked value (escapes decoded) and its raw value (escapes as written, CR and CRLF read as LF; for tagged templates, clj-quickjs-58) | unterminated (`UnexpectedEnd` at the backtick, or at the `}` of the open part); malformed `\x`/`\u` escapes, octal escapes (`\1`, `\07`, `\00`) and `\8` `\9` (`InvalidToken` at the `\`, also in sloppy code); a substitution that is not one Expression followed by `}` (`` `${}` ``, `` `${a b}` ``: `UnexpectedToken`); a tagged template (`` f`x` ``, also with a line break before the template: `Unsupported`, clj-quickjs-58) |
| Regular expressions | – | a `/` where an expression is expected (the parser decides; `a / b / c` is division) |
| Punctuators | every ECMAScript punctuator is lexed by maximal munch (`a>>>=b` is `a`, `>>>=`, `b`) so the parser can name an unsupported operator precisely | other characters (`@`, `#`, ...) |

## Syntactic grammar

EBNF; `{ x }` is repetition, `[ x ]` is optional. Terminals are quoted.

```
Script              = { Directive } { StatementListItem } EOF
Directive           = String ";"     (* a whole ExpressionStatement; see "Directive prologues and strictness" *)
StatementListItem   = FunctionDeclaration | LexicalDeclaration | Statement
Statement           = Block | ";" | IfStatement | WhileStatement | DoWhileStatement
                    | ForStatement | SwitchStatement | VariableStatement
                    | ReturnStatement | ThrowStatement | TryStatement
                    | BreakStatement | ContinueStatement | LabelledStatement
                    | ExpressionStatement
Block               = "{" { StatementListItem } "}"
IfStatement         = "if" "(" Expression ")" Statement [ "else" Statement ]
WhileStatement      = "while" "(" Expression ")" Statement
DoWhileStatement    = "do" Statement "while" "(" Expression ")" [ ";" ]   (* ";" always optional *)
ForStatement        = "for" "(" [ ForInit ] ";" [ Expression ] ";" [ Expression ] ")" Statement
ForInit             = Expression | ( "let" | "const" ) Declarator { "," Declarator }
SwitchStatement     = "switch" "(" Expression ")" "{" { CaseClause } "}"   (* at most one "default" *)
CaseClause          = ( "case" Expression | "default" ) ":" { StatementListItem }
ReturnStatement     = "return" [no LineTerminator here] [ Expression ] ";"
ThrowStatement      = "throw" [no LineTerminator here] Expression ";"
BreakStatement      = "break" [no LineTerminator here] [ Identifier ] ";"
ContinueStatement   = "continue" [no LineTerminator here] [ Identifier ] ";"
LabelledStatement   = Identifier ":" Statement   (* the Statement is not a declaration *)
TryStatement        = "try" Block ( Catch [ Finally ] | Finally )
Catch               = "catch" [ "(" BindingIdentifier ")" ] Block
Finally             = "finally" Block
ExpressionStatement = Expression ";"     (* not starting with "{" or "function" *)
LexicalDeclaration  = ( "let" | "const" ) Declarator { "," Declarator } ";"
VariableStatement   = "var" Declarator { "," Declarator } ";"
Declarator          = BindingIdentifier [ "=" AssignmentExpression ]
FunctionDeclaration = "function" BindingIdentifier FunctionRest
FunctionExpression  = "function" [ BindingIdentifier ] FunctionRest
FunctionRest        = FormalParameters FunctionBody
FormalParameters    = "(" [ FormalParameter { "," FormalParameter } [ "," ] ] ")"
FormalParameter     = BindingIdentifier [ "=" AssignmentExpression ]   (* default value *)
FunctionBody        = "{" { Directive } { StatementListItem } "}"

Expression            = AssignmentExpression { "," AssignmentExpression }
AssignmentExpression  = ConditionalExpression | ArrowFunction
                      | Identifier AssignOperator AssignmentExpression
                      | Call "." IdentifierName "=" AssignmentExpression
                      | Call "[" Expression "]" "=" AssignmentExpression
AssignOperator        = "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "**="
                      | "&=" | "|=" | "^=" | "<<=" | ">>=" | ">>>="
ArrowFunction         = ArrowParameters [no LineTerminator here] "=>" ConciseBody
ArrowParameters       = BindingIdentifier | FormalParameters
ConciseBody           = FunctionBody | AssignmentExpression
ConditionalExpression = LogicalOr [ "?" AssignmentExpression ":" AssignmentExpression ]
LogicalOr             = LogicalAnd { "||" LogicalAnd }
LogicalAnd            = BitwiseOr { "&&" BitwiseOr }
BitwiseOr             = BitwiseXor { "|" BitwiseXor }
BitwiseXor            = BitwiseAnd { "^" BitwiseAnd }
BitwiseAnd            = Equality { "&" Equality }
Equality              = Relational { ( "==" | "!=" | "===" | "!==" ) Relational }
Relational            = Shift { ( "<" | ">" | "<=" | ">=" ) Shift }
Shift                 = Additive { ( "<<" | ">>" | ">>>" ) Additive }
Additive              = Multiplicative { ( "+" | "-" ) Multiplicative }
Multiplicative        = Exponentiation { ( "*" | "/" | "%" ) Exponentiation }
Exponentiation        = Unary | Update "**" Exponentiation
Unary                 = ( "!" | "-" | "+" | "~" | "typeof" | "void" ) Unary | Update
Update                = ( "++" | "--" ) Unary
                      | Call [ [no LineTerminator here] ( "++" | "--" ) ]
Call                  = Primary { Arguments | "." IdentifierName | "[" Expression "]" }
Arguments             = "(" [ AssignmentExpression { "," AssignmentExpression } [ "," ] ] ")"
Primary               = Identifier | Literal | FunctionExpression | ObjectLiteral
                      | ArrayLiteral | TemplateLiteral | "this" | "(" Expression ")"
TemplateLiteral       = NoSubstitutionTemplate
                      | TemplateHead Expression { TemplateMiddle Expression } TemplateTail
ObjectLiteral         = "{" [ PropertyDefinition { "," PropertyDefinition } [ "," ] ] "}"
PropertyDefinition    = PropertyName ":" AssignmentExpression | Identifier
PropertyName          = IdentifierName | String | Number
ArrayLiteral          = "[" { [ AssignmentExpression ] "," } [ AssignmentExpression ] "]"
                        (* an omitted element is a hole; see the notes *)
Literal               = Number | String | "true" | "false" | "null"
                      | "undefined" | "NaN" | "Infinity"
```

Notes on the grammar:

- Precedence, loosest to tightest: the comma operator, assignment
  (right-associative), conditional (right-associative), `||`, `&&`, `|`,
  `^`, `&`, equality, relational, shift (`<<`, `>>`, `>>>`, clj-quickjs-62),
  additive, multiplicative, exponentiation (`**`, clj-quickjs-63), prefix
  unary and prefix `++`/`--`, postfix `++`/`--` (clj-quickjs-64),
  call/member. All binary operators but `**`, and the comma
  operator, are left-associative; `**` is right-associative
  (`2 ** 3 ** 2` is `2 ** (3 ** 2)`). This is ECMAScript's ordering
  restricted to the supported operators.
- The base of `**` cannot be a prefix-operator expression without
  parentheses (ECMAScript's ExponentiationExpression): `-2 ** 2` and
  `a ** -b ** c` are rejected `Invalid` at the `**`; `(-2) ** 2`,
  `-(2 ** 2)` and `2 ** -2` are valid. An update expression may be the
  base: `++a ** 2` and `a++ ** 2` are valid.
- `++`/`--` (clj-quickjs-64) take an identifier binding, possibly
  parenthesized (`(a)++`); any other operand is an early error (`Invalid`:
  `1++`, `(a + b)++`, `f()++`, `++a++`, `++-a`). `undefined`, `NaN` and
  `Infinity` are `ReservedWord`, as for assignment, and so is an intrinsic
  of `__rt` (`__rt.count++`); a property (`a.b++`) is `Unsupported` until
  property references exist. A postfix update ends a member/call chain:
  `a++()` and `a++.b` are syntax errors, and `a--b` is `a--` followed by
  `b` (`UnexpectedToken` at `b`). `- -a` is two negations, `--a` one
  decrement.
- `ReturnStatement` is accepted only inside a function body.
- `BreakStatement` and `ContinueStatement` (clj-quickjs-78) are accepted
  only inside the body of a `while`, `do`-`while` or `for` loop of the
  same function (not in the test of a `do`-`while` or the head of a
  `for`): a function
  nested in the loop starts a new context (`while (x) { function f() {
  break; } }` is an early error). An unlabelled `break` is also accepted
  inside the case block of a `switch` of the same function
  (clj-quickjs-83), and leaves the innermost loop or `switch`; `continue`
  in a `switch` needs an enclosing loop of the same function, which it
  continues. With a label (clj-quickjs-82; only on the
  same line as the keyword) they are accepted anywhere inside a statement
  of the same function that has that label, which for `continue` must be
  a loop (`L: { continue L; }` is an early error).
- `LabelledStatement` (clj-quickjs-82): an identifier followed by `:` at
  the start of a statement is a label (reserved words are not labels). A
  labelled statement may be the body of `if`/`else` and of loops, and
  labels stack: in `L1: L2: while (x) continue L1;` (a label set)
  `continue` to either label continues the loop. Early errors (`Invalid`):
  a `break`/`continue` label that no enclosing statement of the same
  function has (labels do not cross function boundaries), reported at the
  label; a label inside a statement with the same label (`L: L: ;`,
  `L: { L: ; }`), at the second label; a labelled declaration
  (`L: function f() {}`, `L: let x;`). Sibling statements may reuse a
  label, and labels are separate from bindings (`let x; x: { break x; }`
  is valid).
- `SwitchStatement` (clj-quickjs-83): `case` and `default` clauses in any
  order, each followed by a statement list (declarations allowed) that
  ends at the next clause or the closing `}`. The clause bodies together
  are one scope (the case block), so a name declared in two clauses is a
  redeclaration (a compiler `Invalid` error at the second one, see
  "Execution"). Early error (`Invalid`): a second `default`, at its
  keyword.
- `FunctionDeclaration` and `LexicalDeclaration` are statement-list items:
  allowed in the script, a function body, a block or a `switch` clause,
  but not as the direct body of `if`/`else`/`while`/`do`/`for` or of a
  labelled statement (strict mode rejects that; wrap it in a block).
- `VariableStatement` (clj-quickjs-94) is a Statement, so it may also be
  the body of `if`/`else`/`while`/`do`/`for` or of a labelled statement
  (`if (x) var y = 1;`). Its names are bindings of the enclosing function
  or script, wherever the statement is (see "Execution"); `var` in a `for`
  head is not supported yet.
- Function parameter names must be distinct (strict mode, and in
  ECMAScript any list with a default value; the subset requires it in
  sloppy functions too).
- Default parameter values (clj-quickjs-100): a parameter may be followed
  by `=` and an AssignmentExpression, in function declarations,
  expressions and arrow parameter lists (`(a, b = 1, c) => a + c`), and
  parameters without one may follow it. The default is an
  AssignmentExpression, so a `,` ends it; it may itself be an arrow
  function with defaults (`(a = (b = 1) => b) => a`). A `"use strict"`
  directive in the body of a function or arrow with a default is an early
  error (`Invalid`, at the directive), in strict and sloppy code alike.
- Arrow functions (clj-quickjs-53). A `(` whose matching `)` is followed
  by `=>` starts an arrow parameter list (the ECMAScript cover grammar);
  otherwise the parentheses are a parenthesized expression, where `(a, b)`
  is the comma operator. An arrow function is a whole
  AssignmentExpression, so it cannot be an operand, callee or member object
  without parentheses: `1 + x => x`, `f(a) => a` and `() => {}()` are
  syntax errors, `(() => 1)()` is a call. Its body is a function body block
  (where `return` is allowed) or an AssignmentExpression, the value it
  returns. `async` arrows remain unsupported; `async => async` is an arrow
  with a parameter named `async`.
- The comma operator (clj-quickjs-61) appears only where the grammar has
  Expression: expression statements, parentheses, `if`/`while`/`do`-`while` tests,
  the parts of a `for` head (other than a declaration list) and
  `return`/`throw` operands. Arguments, declarators, `? :` branches and
  concise arrow bodies are AssignmentExpressions, so `f(a, b)` passes two
  arguments, `f((a, b))` one, `let a = 1, b;` declares two bindings,
  `a ? b, c : d` is a syntax error and `x => a, b` is `(x => a), b`. A
  comma expression is not an assignment target: `(a, b) = 1` is `Invalid`.
- `BindingIdentifier` excludes reserved words, `undefined`, `NaN`,
  `Infinity`, and in strict code `eval` and `arguments`.
- The assignment target may be parenthesized (`(a) = 1`, `(a) += 1`), as
  in JavaScript. A compound assignment (clj-quickjs-84) takes the same
  targets as `=` and is right-associative too: `a -= b *= 2` is
  `a -= (b *= 2)`.
- `a.b` (an `IdentifierName` after `.`, reserved words included) reads a
  named property, and `a.b = v` (the only property assignment; the target
  may be parenthesized) writes one (clj-quickjs-114); `__rt.name(...)` is
  the compiler-reserved intrinsic binding instead (see "Reserved intrinsic
  binding"). A call of a property, `a.b(...)`, is a method call: `a` is
  evaluated once and passed as the receiver (clj-quickjs-117). Computed access to `__rt`
  (`__rt[x]`) and assignment to one of its intrinsics (`__rt.count = f`)
  are rejected by the parser as `ReservedWord`.
- `ArrayLiteral` (clj-quickjs-118): `[a, b]`, with each element an
  AssignmentExpression. A comma that does not follow an element is an
  elision, a hole: `[ , ]` has length 1 and no element, `[1, , 3]` has a
  hole at index 1. One comma after the last element ends the list without
  adding anything (`[1, 2,]` has length 2). Spread elements (`[...a]`)
  are `Unsupported`, and so is an array literal as an assignment target
  (`[a, b] = v`, destructuring); `let [a] = v` stays rejected as before.
- `a[k]` (clj-quickjs-115; parsed since clj-quickjs-118) reads a
  computed property, and `a[k] = v`
  (the target may be parenthesized) writes one; the key is an Expression
  (`a[b, c]` is `a[c]` after evaluating `b`). It joins a member chain like
  `.` (`a.b[c].d`, `a[0][1]`), with no line-terminator restriction: `a`
  newline `[0]` is one member expression, as in JavaScript. A call of a
  computed property, `a[k](...)`, is a method call like `a.m(...)`
  (clj-quickjs-117: the base is evaluated once and passed as the
  receiver), and compound assignment and `++`/`--` of a computed property
  are `Unsupported`, as for `a.b` (clj-quickjs-260, -76).
- `const` requires an initializer.
- `ForStatement` (clj-quickjs-81): a `let`/`const` head declares bindings
  scoped to the whole loop (a `const` needs an initializer here too). For-in
  and for-of heads (`for (x in o)`, `for (let x of y)`) and `var` heads
  (`for (var i = 0; ;)`) are rejected as `Unsupported`; the `in` operator
  is itself unsupported, so an
  expression head has no `in` ambiguity yet.

### Automatic semicolon insertion

A statement-ending `;` may be omitted when the next token is `}`, the end
of input, or is preceded by a line terminator. A line break never ends a
statement the next token can continue, exactly as in JavaScript: `a` newline
`(b)` is a call, and `a = b` newline `+c` is `a = b + c`. `return`
followed by a line break returns `undefined` (restricted production).
Likewise a line break after `break` or `continue` ends the statement
(`while (1) break` newline `x` is a loop and the statement `x`).
After `do` Statement `while (` Expression `)` a missing `;` is always
inserted, even on the same line (ECMAScript ASI rule for `do`-`while`):
`do x; while (0) y;` is two statements, the loop and `y`.
A semicolon is never inserted in a `for` head: `for (a` newline `b;;)` is
an error, as in JavaScript.
Postfix `++`/`--` is a restricted production: after a line break the
operator begins a new statement as a prefix update (`a` newline `++b` is
`a; ++b;`). No semicolon is ever inserted to create an empty
statement, so `if (x)` newline `else y` is an error.

An arrow function is a whole AssignmentExpression, and only `,` (the
comma operator, clj-quickjs-61) can follow one whose body is a block in
the same Expression; so a line break after that body ends the statement
unless the next token is `,`: `() => {}` newline `, 1` is one comma
expression completing with 1, while `() => {}` newline `() => {}`
is two statements (the script completes with the second function), and
`() => {}` newline `+1` is two statements completing with `1`; on one line
both are syntax errors. A concise body is an expression and continues as
one: `x => x` newline `(1)` is `x => x(1)`. A line break before `=>` is a
syntax error (`x` newline `=> 1`).

## Rejected forms

Every form below is rejected where it starts. Error kinds:
`UnknownToken`, `InvalidToken`, `UnexpectedEnd` (truncated input),
`UnexpectedToken`, `Unsupported` (valid JavaScript outside the subset),
`ReservedWord`, `Invalid` (a JavaScript early error), `TooDeep`.

| Form | Kind |
|------|------|
| `var` as a `for` head, for-in and for-of loops, `debugger`, `with` | Unsupported |
| classes, `new`, `super`, `import`/`export`, dynamic `import` | Unsupported |
| `this` at the script top level (clj-quickjs-104) and inside an arrow function, its parameters included (lexical `this`, clj-quickjs-98) | Unsupported |
| `async` functions and arrows, `await`, generators (`function*`) | Unsupported |
| rest parameters, destructuring (including a destructuring catch parameter or arrow parameter) | Unsupported |
| optional chaining `?.`, spread `...` (in an array literal, a call or an object literal), an array literal as an assignment target (`[a] = v`) | Unsupported |
| in object literals: computed keys `{[k]: v}` (clj-quickjs-233), methods `{m() {}}`, getters and setters, `async` and generator methods, `__proto__: v` (it sets the prototype) | Unsupported |
| tagged templates (`` f`x` ``, clj-quickjs-58), regular expression literals, BigInt, numeric separators | Unsupported |
| `delete`, `??`, `in`, `instanceof`, logical assignment (`&&=`, `\|\|=`, `??=`) | Unsupported |
| compound assignment to a property (`a.b += 1`, `a[0] += 1`, clj-quickjs-260), `++`/`--` on a property (`a.b++`, `a[0]++`, clj-quickjs-76) | Unsupported |
| a reserved word as a shorthand property (`{if}`) | ReservedWord |
| a string or numeric key as a shorthand property (`{1}`) | UnexpectedToken |
| a shorthand initializer outside a destructuring pattern (`({a = 1})`) | Invalid |
| references to `eval` or `arguments` in strict code (in sloppy code they parse, and the compiler rejects those that do not denote a binding the program declares) | Unsupported |
| reserved word as a binding name or reference; binding, assigning (also with a compound operator: `undefined += 1`), incrementing or decrementing `undefined`/`NaN`/`Infinity`; binding `eval`/`arguments` in strict code, including a function name or parameter checked again because the function's body starts with a `"use strict"` directive | ReservedWord |
| computed access to the reserved intrinsic binding (`__rt[x]`), assignment to one of its intrinsics (`__rt.count = f`) or `++`/`--` on one (`__rt.count++`) | ReservedWord |
| an arrow function as an operand, callee or member object without parentheses (`1 + x => x`, `() => {}()`), a line break before `=>` | UnexpectedToken |
| `return` outside a function, `break` outside a loop or `switch` of the same function, `continue` outside a loop of the same function, a second `default` in a `switch`, `const` without initializer, duplicate parameter (arrow functions included), declaration as an `if`/`while`/`do`/`for` or labelled body, a `break`/`continue` label that no enclosing statement of the same function has, `continue` to a label of a statement that is not a loop, a label inside a statement with the same label, `const` without initializer in a `for` head, invalid assignment target (`1 = 2`, `1 += 2`, `f() += 1`), an operand of `++`/`--` that is not an identifier binding (`1++`, `f()++`, `++a++`), a line break between `throw` and its expression, a `let`/`const`/function declaration in a catch body that repeats the catch parameter, a prefix-operator expression as the base of `**` (`-a ** b`) | Invalid |
| nesting over the `MAX_NESTING` budget (256 units), or a syntax tree more than 256 nodes high; see "Nesting limit" below | TooDeep |

## Explicit gaps

Known differences from full JavaScript within the accepted forms, and
checks left to later stages:

- **Scope errors are checked by the compiler, not the parser.**
  `parse_script` accepts redeclaration (`let a; let a;`), a `let` that
  repeats a parameter name and declarations of the reserved binding `__rt`;
  `compile_script` rejects them (see "Execution"). Temporal-dead-zone use
  and assignment to a `const` are run-time errors, as in JavaScript.
- **Script-level `var` and function declarations are script bindings.**
  A `var` or function declaration at the top level of a script binds a
  name of that script only, like a top-level `let`: there is no global
  object yet, so it does not become a property of one and is not visible
  to other scripts (clj-quickjs-104 will make them global object
  properties). Function declarations in blocks are lexical (strict-mode
  semantics; the sloppy-mode Annex B hoisting out of blocks is not
  implemented).
- **Nesting limit.** Nesting is charged against one budget of 256 units
  (`MAX_NESTING`); input that exceeds it is rejected with `TooDeep`. Costs
  per level, weighted by the Rust stack each construct uses: a statement 1,
  and each label of a labelled statement 1,
  plus 1 for a block or an `if`/`while`/`do`-`while`/`for`/`try`, 2 for a
  `switch` (its case block counts as a block) (the
  expressions of a `for` head are charged as expressions); a function or
  arrow function 3 plus its body block (or, for a concise arrow body, its
  assignment);
  an assignment 1; `? :` 1 plus its branch assignments; a prefix operator
  or one link of a binary-operator, comma-operator or member chain 1; each call's
  argument list 3 plus each argument's assignment; parentheses 3 plus the
  inner assignment; an object literal or an array literal 3 plus each
  value's or element's assignment; a computed member `[k]` 1 as a chain
  link plus 3 for its key expression while it is parsed. So about 64 nested parentheses or calls, 128 nested
  blocks or `else if` links, 64 nested function declarations, 42 nested
  function expressions, or a chain of about 250 binary operators fit. The whole budget fits a 2 MiB thread stack
  even in a debug build (tested). Separately, the syntax tree itself may be
  at most 256 nodes high (statements, functions and expressions on any
  root-to-leaf path, counted by each node's `height`), which also catches
  finished subtrees wrapped again, such as `(a.b.b).b.b`; later recursive
  passes can rely on that bound.
- **Objects have only data properties** (clj-quickjs-114). Getters,
  setters, methods, computed keys and `__proto__: v` in literals,
  `delete`, and `++`/`--` and compound assignment on properties are
  rejected explicitly (their pellets: -233, -120, -121, -124, -126, -76,
  -260).
  An object literal inherits from `Object.prototype` (clj-quickjs-117);
  an embedder-made object (`Scope::new_object`) has a null prototype.
  Their attributes (`writable`, `enumerable`, `configurable`) are set
  with `Object.defineProperty` and read with
  `Object.getOwnPropertyDescriptor` (clj-quickjs-119); a descriptor with
  `get`/`set` fails with `UnsupportedOperand` until accessors exist
  (clj-quickjs-120). Nothing reads `enumerable` yet (`for`-`in`,
  `Object.keys`: clj-quickjs-85, -123).
- **Computed keys are primitives** (clj-quickjs-115, with the
  instructions of clj-quickjs-118). A key is converted by ToPropertyKey
  when the access runs, so for a write after the right-hand side has been
  evaluated (as current ECMAScript's PutValue and QuickJS's `put_array_el`
  do; with primitive keys the conversion has no visible effect, so the
  order becomes observable only with ToPrimitive): strings as they are,
  numbers by Number::toString (`o[1]` is `o["1"]`, `o[-0]` is `o["0"]`,
  `o[1.5]` is `o["1.5"]`), booleans, `null` and `undefined` by ToString.
  An object key needs ToPrimitive (clj-quickjs-70) and fails with
  `UnsupportedOperand`, except on an `undefined`/`null` base, which is
  checked first and throws its catchable `TypeError` (`null[{}]`);
  symbols do not exist yet (clj-quickjs-127). Primitive bases (`"abc"[1]`)
  fail with `UnsupportedOperand` until clj-quickjs-73, function bases
  until clj-quickjs-125.
- **Property names are never forgotten** (clj-quickjs-115). Each new
  name a write uses, by `o.name = v`, `{name: v}` or a computed
  `o[k] = v` (also one that then fails), is interned in the runtime-wide
  property-key table for the life of the runtime (reads and array
  indices on arrays intern nothing). With computed keys the table grows with the number of
  distinct names a program writes (`o["k" + i] = i` in a loop adds one
  name per iteration), not with live objects; the collector reclaiming
  unused names is clj-quickjs-259 (`docs/architecture.md`, "Ordinary
  objects", Consequences).
- **Arrays have no methods and a read-only `length`** (clj-quickjs-118).
  An array literal is an array object with elements, holes and an
  automatic `length`, inheriting from `Array.prototype`, a realm
  intrinsic that inherits from `Object.prototype` and has no methods yet:
  `[].push` reads `undefined` until the `Array.prototype` method pellets
  (-153..-156), and `[].toString()` fails with `UnsupportedOperand`
  rather than give `"[object Array]"` (`Array.prototype.join`,
  clj-quickjs-39). Assigning `a.length` (truncation or extension), the
  `Array` constructor (so `Array.prototype` has no JavaScript name yet)
  and `Array.isArray` belong to clj-quickjs-39 and fail explicitly
  (`UnsupportedOperand`) or are unbound; `Object.getPrototypeOf([])` is
  the reflection pellet's. A persistent vector (`__rt.vector(...)`) is
  not an array: `v[0]` and `v.length` read `undefined`.
- **Built-in objects are shared and immutable** (clj-quickjs-117,
  `docs/architecture.md`, "Realm intrinsics"). `Object`,
  `Object.prototype`, `Array.prototype` (clj-quickjs-118) and their
  methods are one set of objects shared by
  every task of a runtime; any write to one (`Object.prototype.x = 1`,
  `Object.is = f`) throws a catchable `TypeError` ("built-in objects are
  shared by all tasks and immutable"), and so does
  `Object.defineProperty` on one (clj-quickjs-119), although their properties report
  the standard writable attributes. Programs therefore cannot extend
  built-in prototypes. An ordinary object may still shadow an inherited
  method with its own property (`o.toString = f`).
- **Built-in globals cannot be assigned** until the global object exists
  (clj-quickjs-104): `Object = x` (also compound and `++`/`--`) is a
  compile-time `Unsupported` error. A lexical binding named `Object`
  shadows the built-in as usual.
- **`this` is only a non-arrow function's receiver** (clj-quickjs-116):
  `this` at the script top level (the global object, clj-quickjs-104) and
  an arrow's lexical `this` (clj-quickjs-98) are rejected; there is no
  `new` (clj-quickjs-71) or `call`/`apply`/`bind` (clj-quickjs-102); a
  computed method call `o[k]()` passes `o` like `o.m()` (clj-quickjs-118).
  In sloppy code `this` of a call without an object receiver (a plain
  call `f()`, which would see the global object, or a primitive one,
  which would be wrapped, clj-quickjs-73) fails with `UnsupportedOperand`
  when evaluated instead of giving a wrong value.
- **Built-in functions have no `Function.prototype` yet**
  (clj-quickjs-102): their own `length`, `name` (and for `Object`,
  `prototype`, `is`, `defineProperty` and `getOwnPropertyDescriptor`) read normally, any other property read
  (`Object.call`, `Object.is.toString`) fails with `UnsupportedOperand`.
  `Object(v)` of a number, string or boolean and
  `Object.prototype.toString`/`valueOf` of such a receiver need wrapper
  objects (clj-quickjs-72, -73) and fail the same way; `new Object()` is
  clj-quickjs-71.
- **Objects are task-owned.** An object or array belongs to the task that
  created it: only that task reads or writes its properties (another task
  fails with `ForeignObject`), and publishing a value that reaches one to
  another task is rejected (`Runtime::publish`), with a path that names
  the property or element index leading to the innermost one.
- **No string `length`, indexing or `String.fromCharCode` yet**
  (clj-quickjs-142 and the string-methods pellets). A string is already a
  sequence of UTF-16 code units (clj-quickjs-141; `docs/architecture.md`,
  "String values"), so these will read stored code units.
- **Identifier tables are Unicode 16.0**, the version compiled into the perl
  that generated them; code points assigned later are not identifier
  characters until the tables are regenerated.
- **Diagnostics quote a name as the source spells it**: an undeclared
  `\u{F5}` fails with "\u{F5} is not defined", although the name it
  looks up is `õ`. An escaped reserved constant as an assignment target
  (`\u0075ndefined = 1`) is rejected as `Invalid` ("invalid assignment
  target"), not `ReservedWord`.
- **Diagnostics stop at the first error**; there is no error recovery.
  Positions are byte spans plus 1-based line and column (columns count
  Unicode scalar values).
- **Sloppy mode is partial** (clj-quickjs-95). Non-strict code differs
  from strict code only in allowing `eval` and `arguments` as binding
  names, which are then read and assigned like any binding, legacy
  octal and leading-zero decimal number literals (clj-quickjs-51),
  legacy octal and `\8` `\9` string escapes (clj-quickjs-258), and
  assignments that `[[Set]]` rejects (`o.k = v` of a read-only property or
  on a non-extensible object), which sloppy code ignores instead of
  throwing a `TypeError` (clj-quickjs-119). Every other
  sloppy-only form or behaviour is still the strict one, until the pellet
  named for it: a reference to `arguments` that would denote a function's
  own `arguments` object (clj-quickjs-97) and to `eval` that would denote
  the global `eval` (clj-quickjs-106, direct eval -107/-108) are compile
  errors (`Unsupported`, see "Directive prologues and strictness"); silently
  ignored stores to a function expression's own name (clj-quickjs-96);
  implicit globals and `delete` of identifiers (clj-quickjs-105); `with`
  (clj-quickjs-110); `this` substitution (a sloppy function's `this` fails
  explicitly unless its receiver is an object, see above) and the mapped
  `arguments` object (not filed). Strict reserved words (`let`, `static`, `yield`,
  `implements`, `interface`, `package`, `private`, `protected`, `public`)
  stay reserved, duplicate parameter names stay `Invalid`, a function
  declaration as a labelled or `if` body stays `Invalid`, and function
  declarations in blocks stay lexical (no Annex B hoisting).
- **Default parameter values lack what the subset lacks elsewhere**
  (clj-quickjs-100): a function's own `arguments` object is rejected in a
  default as anywhere (clj-quickjs-97), and so is `this` in an arrow's
  default (clj-quickjs-98; a non-arrow function's default sees its
  `this`, clj-quickjs-116), and
  `eval` (clj-quickjs-106, -109); an anonymous function as a default gets
  no inferred `name`, and a function's `length` (the parameters before the
  first default) does not exist yet (clj-quickjs-101).

## Directive prologues and strictness

(clj-quickjs-95; ECMAScript "Directive Prologues and the Use Strict
Directive"; QuickJS `js_parse_directives` and
`js_parse_function_check_names` in `research/quickjs/quickjs.c` at
7d1c13c5d8f8.)

- A **directive prologue** is the leading run of expression statements of
  a script or function body (an arrow function's block body included) that
  each consist of exactly one string literal token, followed by `;` or an
  inserted one. A parenthesized string (`("use strict");`), a string
  operand (`"use strict" + 1;`, also across a line break) or any other
  statement ends it, so a `"use strict"` after `1;` is an ordinary
  statement.
- A directive whose **exact source text** is `"use strict"` or
  `'use strict'` makes that script or function strict-mode code from the
  start, and with it every function nested in it. Any escape sequence or
  line continuation (`"use\x20strict"`, `'use \` newline `strict'`)
  makes it an ordinary directive with no effect. A function's own
  directive applies to its name and parameters too: they were parsed
  before the body, so they are checked again (`function eval() {
  "use strict"; }` and `function f(arguments) { "use strict"; }` are
  `ReservedWord` at the name).
- **Arrow functions** inherit the strictness of the code around them and
  may also have their own directive in a block body (`(eval) => {
  "use strict"; }` is `ReservedWord`).
- A function or arrow with a **default parameter value** (clj-quickjs-100)
  cannot have a `"use strict"` directive (exact source text, as above):
  it is `Invalid` at the directive, even in code that is already strict
  (ECMAScript FunctionBody early errors; QuickJS
  `js_parse_function_check_names`).
- Directives stay **ordinary expression statements**: they evaluate to
  their string, so a script that is only `"use strict";` completes with
  `"use strict"`.
- **`eval` and `arguments` in sloppy code** may be declared as
  parameters, `let`/`const`/`var` bindings, function names and catch
  parameters, and such bindings are read, assigned (`=`, compound, `++`)
  and closed over as usual. A reference to either is a compile error
  (`Unsupported`) where it would denote something the subset lacks:
  `eval` with no binding of that name in scope (the global `eval`,
  clj-quickjs-106); `arguments` in a `function` (directly or through
  arrows) that does not itself bind `arguments` in an enclosing scope, or
  binds it only with `var`, which in JavaScript keeps the function's
  `arguments` object (clj-quickjs-97). In a script outside any `function`,
  an unbound `arguments` is an ordinary unresolvable name
  (ReferenceError at run time, `"undefined"` for `typeof`).
- The resulting mode is recorded on the tree (`Script::strict`,
  `Function::strict`) and in each function's compiled code
  (`ExecutableSpec::strict`, `Executable::is_strict`; hand-assembled code
  is strict). Later sloppy-mode semantics consult it: the parser through
  its `strict` flag (the mode of the code being parsed), the compiler
  through `FnCtx::strict` (the mode of the function being lowered), and the
  VM through the running code's `Code::strict`.
- **Number literals by mode** (clj-quickjs-51): the lexer follows the
  parser's mode, so legacy octal and leading-zero decimal literals (see
  "Lexical grammar", Numbers) are accepted in sloppy code only. The token
  right after a directive is lexed before the directive is recognised, so
  a `"use strict"` directive checks that token again: `"use strict"; 010`
  and `function f() { 'use strict'; 010; }` are `InvalidToken` at the
  `0`, also when lexing it as sloppy code fails (`"use strict"; 01n` is
  the strict error at the `0`, not the sloppy one at the `n`). The token
  after a strict function's closing `}` is lexed in the enclosing mode
  again.
- **String escapes by mode** (clj-quickjs-258): likewise, legacy octal
  and `\8` `\9` escapes (see "Lexical grammar", Strings) are accepted in
  sloppy code only, and a `"use strict"` directive checks the string
  right after it again (`"use strict"; "\07"` is `InvalidToken` at the
  `\`, also when lexing that string as sloppy code fails). The directives
  before it in the same prologue are strict code too (ECMAScript early
  errors; QuickJS `js_parse_directives` lexes the prologue again), so it
  checks them as well: `"\07"; "use strict";` and
  `function f() { "\8"; "use strict"; }` are `InvalidToken` at the
  escape.

```js sloppy-accept
// Sloppy code may bind eval and arguments; directives apply per function.
function f(eval) { var arguments = 2; return 1; }
function g() { 1; "use strict"; let arguments; }
function h() { "use\x20strict"; let eval; }
```

```js sloppy-reject ReservedWord 1:12
function f(eval) { "use strict"; }
```

```js sloppy-reject ReservedWord 2:12
"use strict";
function f(eval) {}
```

```js sloppy-reject ReservedWord 1:2
(eval) => { "use strict"; }
```

```js reject Invalid 1:21
function f(a = 1) { "use strict"; }
```

```js sloppy-accept
// Legacy octal and leading-zero decimal literals (clj-quickjs-51).
x = 017 + 08 + 09.5 + 01.a;
function f() { "use strict"; }
y = 010;
```

```js reject InvalidToken 1:5
x = 017;
```

```js sloppy-reject InvalidToken 1:15
"use strict"; 010;
```

```js sloppy-reject UnexpectedToken 1:3
01.5;
```

```js sloppy-accept
// Legacy octal and \8 \9 string escapes (clj-quickjs-258).
x = "\07" + "\101" + "\400" + "\8" + '\9';
function f() { "use strict"; }
y = "\377";
```

```js reject InvalidToken 1:6
x = "\101";
```

```js sloppy-reject InvalidToken 1:16
"use strict"; "\8";
```

```js sloppy-reject InvalidToken 1:17
function f() { "\07"; "use strict"; }
```

## Execution

`frontend::compile_script(name, source)` parses, resolves bindings and lowers
a script to a validated `Executable` (`src/frontend/lower.rs`); a task
running it completes with the script's **completion value**, the value
JavaScript `eval` would return (the last expression statement that ran;
`if`, `while`, `do`-`while`, `for` and `try` produce `undefined` unless
their body produced a value (a loop keeps the value its body produced last, so
`5; do { 6; } while (false)` is 6 and `5; do { } while (false)` is
`undefined`; the head of a `for` never sets it, so
`5; for (let i = 0; i < 0; ) {}` is `undefined`); a `try` statement's value is that of its block or catch body, never
that of a finally block that completes normally; a `break` or `continue`
keeps the value its loop body produced so far, as ECMAScript `UpdateEmpty`
does, so `5; while (true) { 6; break; }` is 6 and
`5; while (true) { break; }` is `undefined`; a finally block that a jump
runs and that completes normally keeps it, so
`5; while (true) { try { 6; break; } finally { 7; } }` is 6; a labelled
statement's value is its body's, and a `break` to its label keeps the
value produced so far (clj-quickjs-82), so `5; L: { break L; }` is 5,
`5; L: { 6; break L; }` is 6 and `5; L: if (true) break L;` is
`undefined`; a `switch` (clj-quickjs-83) is `undefined` unless a clause
that ran produced a value, so `switch (1) { case 1: 7; case 2: 8; }` is 8,
`5; switch (1) { case 1: }` is `undefined` and
`5; switch (1) { case 1: 6; break; }` is 6).
`frontend::compile_function(source)` compiles a source made of exactly one
function declaration into an entry point whose parameters are the task's
arguments (missing ones are `undefined`); `return` completes the task and
falling off the end returns `undefined`.

Executed, with JavaScript semantics:

| Form | Semantics |
|------|-----------|
| Number, `true`/`false`, `null`, `undefined`, `NaN`, `Infinity` literals | IEEE-754 doubles; `-0` is preserved; no constant folding |
| String literals `"..."`, `'...'` (clj-quickjs-65) | immutable string values loaded from the constant pool, one constant per distinct content; a string is a primitive: publishable to other tasks and usable as a persistent map key, compared by content. A string is a sequence of UTF-16 code units, lone surrogates included (clj-quickjs-141): `"\u{10ffff}" === "\u{dbff}\u{dfff}"` and `"\u{dbff}" + "\u{dfff}" === "\u{10ffff}"` are true, a lone surrogate equals only itself (not U+FFFD), and equality, map keys and `<` compare code units. The embedder reads the exact code units (`JsString::code_units`, or `Debug`, which writes a lone surrogate as `\u{d800}`; `cargo run` prints a string result that way) or lossy text (`Display`, `to_string_lossy`: U+FFFD) |
| `let`, `const`, parameters, block scoping and shadowing | one frame local per live binding; `let x;` sets `undefined` each time it runs (loop bodies included) |
| `var a = 1, b;` (clj-quickjs-94) | one binding per name per function body (or script), wherever the `var` statements are (nested blocks, loop and `if` bodies, `try`/`catch`/`finally` blocks and `switch` clauses included, nested functions excluded); hoisted: it exists and is `undefined` from function entry, with no temporal dead zone (`x; var x = 1;` reads `undefined`); repeating `var` is allowed and `var x;` does nothing (it does not reset an assigned value); an initializer is an assignment that runs when the statement does; a `var` named like a parameter, or like a function declaration at the top of the same body, is that binding; a `var` statement has no completion value. Closures capture a `var` under the rule of every binding: by value if it is never assigned, otherwise through one cell for the whole call, so closures made in different loop iterations share it (unlike a `let` in the loop body). A `var` in a catch block may repeat the simple catch parameter (ECMAScript Annex B, as QuickJS does): it declares the function's binding, and its initializer assigns the parameter |
| `+` | when either operand is a string: ToString of both, concatenated (a new string; a result over 2^30 - 1 code units fails with `OutOfMemory`, as QuickJS's "string too long"). ToString gives `"undefined"`, `"null"`, `"true"`/`"false"`, and for numbers Number::toString in radix 10: shortest round-trip digits (ties to the even digit), plain decimal for `1e-6 <= \|x\| < 1e21` (`"0.000001"`, `"19686109595169230000"`), otherwise exponent form (`"1e+21"`, `"1e-7"`), `"0"` for `-0`, `"NaN"`, `"Infinity"` (clj-quickjs-66). Otherwise ToNumber of both, then IEEE addition. An object operand fails with `UnsupportedOperand` until ToPrimitive (clj-quickjs-70). Lowered to `add` when both operands are statically numbers, otherwise to `plus`, which decides at run time |
| `` `a${x}b${y}c` `` untagged template literals (clj-quickjs-57) | a new string: the cooked text parts and the ToString of each substitution, in order (ECMAScript TemplateLiteral evaluation, QuickJS `js_parse_template`); the substitutions are evaluated left to right, each converted right after it is evaluated and before the next runs. ToString is that of `+` (`"undefined"`, `"null"`, `"true"`/`"false"`, Number::toString); an object substitution fails with `UnsupportedOperand` (ToPrimitive with hint string is clj-quickjs-70). Lowered to the first text part as a constant, then per substitution its code, the `to_string` opcode (`0x55`) and `plus`, then the next text part (when not empty) and `plus`; a result over 2^30 - 1 code units fails with `OutOfMemory` |
| `- * / %` and unary `-`, `+` | ToNumber on `undefined` (NaN), `null` (0), booleans (0/1) and strings (StringToNumber, next row), then IEEE arithmetic; `%` keeps the dividend's sign |
| StringToNumber (clj-quickjs-67), wherever ToNumber applies | leading and trailing whitespace and line terminators (including NBSP, BOM and Unicode `Zs`) are ignored; empty or all-whitespace is 0; an unsigned `0x`/`0o`/`0b` integer (any length, correctly rounded); a decimal with optional sign, fraction and exponent (`"-1.5e3"`, `".5"`, `"5."`; `"-0"` is -0; `"010"` is 10, no legacy octal); `Infinity` with an optional sign; anything else is NaN (`"1_000"`, `"1n"`, `"-0x1"`, `"12px"`, `"infinity"`) |
| `< > <= >=` | IsLessThan on primitives: two strings compare lexicographically by UTF-16 code units, a proper prefix being smaller (`"10" < "9"`, `"B" < "a"` and `"" < "a"` are true; `"\u{10000}" < "\uFFFF"` is true because U+10000 is the code units D800 DC00; clj-quickjs-68); any other pair converts with ToNumber, then compares numerically (anything with NaN is false; `"10" < 9` and `"abc" >= 1` are false). An object operand fails with `UnsupportedOperand` until ToPrimitive (clj-quickjs-70). Lowered to `compare_lt`/`compare_gt`/`compare_le`/`compare_ge` when neither operand is statically a number, otherwise to ToNumber and the number-only `lt`/`gt`/`le`/`ge` |
| `&`, `\|`, `^`, `<<`, `>>`, `>>>` and unary `~` (clj-quickjs-62) | ToNumber of both operands (left first, after both are evaluated), then ToInt32 (NaN and ±Infinity are 0; truncate, wrap modulo 2^32), ToUint32 for the left operand of `>>>`; a shift count is `ToUint32(right) & 31`; the result is a 32-bit integer (unsigned for `>>>`), never `-0` |
| `**` (clj-quickjs-63) | ToNumber of both operands (left first, after both are evaluated), then Number::exponentiate: IEEE `pow`, except that a NaN or ±Infinity exponent with base `1` or `-1` gives NaN (`1 ** NaN`, `(-1) ** Infinity`); `x ** 0` is 1 even for NaN |
| `===`, `!==` | IsStrictlyEqual (`NaN !== NaN`, `0 === -0`; strings by content, never equal to a non-string) |
| `==`, `!=` | IsLooselyEqual for the supported types: `null == undefined`, neither equals anything else, booleans compare as numbers, two strings by content (`"1" == "01"` is false); a string compared with a number or boolean compares as a number, the string by StringToNumber and the boolean as 0 or 1 (`"" == 0`, `" 1 " == 1` and `"1" == true` are true, `"abc" == NaN` is false; clj-quickjs-67) |
| `typeof x` (clj-quickjs-69) | evaluates the operand, then gives its type name as a string (ECMAScript TypeofOperator, QuickJS `js_operator_typeof`): `"undefined"` for `undefined`, `"object"` for `null`, `"boolean"`, `"number"`, `"string"`; `"function"` for a function object (function declarations and expressions, arrow functions, closures) and a built-in function (`Object`, `Object.is`, ...; clj-quickjs-117), the callable values; `"object"` for every other object: persistent vectors, maps and occurrences, transient builders, atoms, error objects caught by `catch`, plain objects and arrays. An operand that is a name with no binding, possibly parenthesized (`typeof nope`, `typeof (nope)`), gives `"undefined"` instead of a ReferenceError: with no global object the compiler knows this statically and emits `undefined`; any other operand (`typeof (0, nope)`, `typeof nope()`) is evaluated as usual. A binding in its temporal dead zone is still a ReferenceError, as a plain read is. `typeof __rt` is a `ReservedWord` compile error, as any other non-call use. Lowered to the `typeof` opcode (`0x4E`), which allocates nothing on the heap and calls no JavaScript |
| `void x` (clj-quickjs-133) | evaluates the operand (its effects and failures happen: `void f()` calls `f`, `void nope` is a ReferenceError, unlike `typeof nope`), discards its value and gives `undefined`. A prefix operator with the precedence of `!`, so `void a ** 2` is `Invalid` at the `**`. No new opcode: the operand, then `pop` and `undefined` (QuickJS `js_parse_unary`, `TOK_VOID`: `OP_drop`, `OP_undefined`) |
| `!`, `&&`, `\|\|`, `? :`, `if`, `while`, `do`-`while`, `for` | ToBoolean (falsy: `0`, `-0`, `NaN`, `null`, `undefined`, `false`, `""`); `&&`/`\|\|` short-circuit and return an operand |
| a directive (`"use strict";`, clj-quickjs-95) | an ordinary string expression statement: evaluates to the string, which becomes the completion value of a script; its only other effect is the strictness recorded at compile time ("Directive prologues and strictness") |
| `x = v` | evaluates `v`, then stores; the expression's value is `v` |
| `{a: 1, "b": 2, 3: 4, c}` object literals (clj-quickjs-114) | a new ordinary object owned by the running task (`docs/architecture.md`, "Ordinary objects"), inheriting from `Object.prototype` (clj-quickjs-117; lowered to `new_object_literal`): the values are evaluated left to right and each defines a writable, enumerable, configurable data property in creation order; a key is an identifier name (reserved words included), a string, or a number's ToString (`1.50` is `"1.5"`, `0x10` is `"16"`); a shorthand `{c}` is `{c: c}`; a duplicate key keeps its first position and its last value. Two evaluations are distinct objects (`({}) !== ({})`); `typeof` is `"object"` |
| `o.name` (clj-quickjs-114) | evaluates `o`; an ordinary object gives its own property, else the first one along its prototype chain (`Object.prototype` for literals, clj-quickjs-117), else `undefined`; a built-in function gives its own property and fails with `UnsupportedOperand` for any other name (no `Function.prototype` yet, clj-quickjs-102); persistent vectors, maps and occurrences, builders and atoms have no JavaScript properties (`undefined`); `undefined`/`null` throw a catchable `TypeError` ("cannot read property 'x' of undefined") at the member expression; function objects, caught error objects, numbers, booleans and strings fail with `UnsupportedOperand` until their properties exist (clj-quickjs-101, -230, -73); an object owned by another task fails with `ForeignObject` |
| `o.name = v` (clj-quickjs-114) | evaluates `o`, then `v`, then writes: an ordinary object's own writable property is updated; otherwise, unless the first object along its prototype chain with the property has it read-only, a new own data property is added (writable, enumerable, configurable), so `o.toString = f` shadows the inherited method; a built-in object (`Object.prototype`, `Object`, ...) throws a catchable `TypeError` ("built-in objects are shared by all tasks and immutable", clj-quickjs-117); a value without JavaScript properties (vectors, maps, builders, ...) or an `undefined`/`null` base throws a catchable `TypeError` ("cannot set property 'x' of null") at the member expression, after `v` ran; the other bases fail as for a read. An own or inherited read-only property (`Object.defineProperty`, clj-quickjs-119) and a non-extensible receiver reject the write: a catchable `TypeError` ("cannot assign to read-only property 'k'") in strict code, nothing in sloppy code. The expression's value is `v` |
| `[a, , b,]` array literals (clj-quickjs-118) | a new array object owned by the running task (`docs/architecture.md`, "Array objects"), inheriting from `Array.prototype` (a realm intrinsic without methods yet, which inherits from `Object.prototype`; see "Explicit gaps"): the elements are evaluated left to right and each defines the next index; an elision skips an index, leaving a hole (no own property: reading it continues along the prototype chain and gives `undefined`); `length` is the number of elements and holes (`[1, , 3].length` is 3, `[ , ].length` 1, `[1, 2,].length` 2). Two evaluations are distinct objects (`[] !== []`); `typeof` is `"object"`. Lowered to `new_array` and one `append_element`/`append_hole` per element |
| `o[k]` (clj-quickjs-115, -118) | evaluates `o`, then `k`, then converts `k` by ToPropertyKey (see "Explicit gaps") and reads as `o.name` does: on an ordinary object the key string names the property (`o[1]`, `o["1"]` and `o[1.0]` are one property, `o["01"]` another); on an array an array index (the canonical decimal string of an integer `0` to `2^32 - 2`, from a string or a number: `a[1]`, `a["1"]`, `a[-0]`) names an element, `length` its length, any other key an ordinary property; failures as for `o.name`, at the `[`. Lowered to `get_elem` |
| `o[k] = v` (clj-quickjs-115, -118) | evaluates `o`, `k`, then `v`, then checks that `o` is not `undefined`/`null` (a catchable `TypeError`), converts `k` and writes as `o.name = v` does; writing an array index at or beyond an array's `length` sets `length` to that index plus one (`let a = [1]; a[3] = 4` gives length 4 with holes at 1 and 2), and any other key adds an ordinary property without changing `length`; assigning an array's `length` fails with `UnsupportedOperand` (clj-quickjs-39). The expression's value is `v`. Lowered to `put_elem` |
| `++x`, `--x`, `x++`, `x--` on a binding (clj-quickjs-64) | reads `x` (the checks of a read), ToNumber, adds or subtracts 1, stores (the checks of `=`, so a `const` is a TypeError after the read); a prefix update's value is the new number, a postfix update's the old one converted (`let t = true; t++` is 1, `t` is 2); no new opcode: `to_number`, `add`/`sub` and the binding's store; failures of the conversion are reported at the operator token |
| `x op= v` on a binding, for `op` one of `+ - * / % ** & \| ^ << >> >>>` (clj-quickjs-84) | reads `x` (the checks of a read: an unresolvable name or a binding in its temporal dead zone is a ReferenceError before `v` is evaluated), evaluates `v`, applies `op` exactly as `x op v` does (so `+=` concatenates when either value is a string, and the others apply ToNumber to both values, the old one first), then stores (the checks of `=`, so a `const` is a TypeError after `v` is evaluated); the expression's value is the stored value. As in JavaScript (QuickJS `js_parse_assign_expr2`), the old value is the one read before `v` runs: `let a = 1; function f() { a = 10; return 5; } a += f(); a` is 6. No new opcode: the binding's read and store around the operator's own instructions; failures of the operator are reported at the `op=` token |
| `a, b` | evaluates `a` and discards its value, then evaluates `b`, the value; as the last expression statement, `b` is the completion value |
| `return` (in `compile_function`) | completes the task |
| `do body while (test)` (clj-quickjs-80) | runs the body, then evaluates the test, and repeats while it is truthy: the body always runs at least once |
| `for (init; test; update) body` (clj-quickjs-81) | runs the init once (an expression's value is discarded), then repeats: evaluates the test (absent: true) and stops when it is falsy, runs the body, then the update (its value discarded) |
| `for (let ...; ...; ...)`, `for (const ...; ...; ...)` | the head's bindings are scoped to the loop and in their temporal dead zone during their own initializers (`for (let i = i; ;)` is a ReferenceError); each iteration has its own copy of the `let` bindings (CreatePerIterationEnvironment, before the first test and before each update), so a closure created in an iteration keeps that iteration's binding, and the next iteration starts from the values it ended with, including assignments made by closures; updating a `const` head is a TypeError |
| `break`, `continue`; `break L`, `continue L`, `L: body` (clj-quickjs-82) | `break` leaves the innermost enclosing loop, `continue` evaluates its test again (in a `for` loop, runs the update first); `break L` leaves the enclosing statement labelled `L` (a loop or any other statement, continuing after it) and `continue L` continues the enclosing loop labelled `L` (directly or through a label set `L: M: while ...`), leaving every loop in between; an unlabelled `break`/`continue` passes by labelled statements that are not loops; leaving a `try` block pops its catch handler, so a later exception is not caught by it; leaving a `try` block or catch body runs its finally block first (innermost first, clj-quickjs-79), and a finally block that completes abruptly (`return`, `throw`, `break`, `continue`) replaces the jump; a jump out of a finally block discards its pending completion (return value, exception or jump) |
| `switch (e) { case a: ... default: ... }` (clj-quickjs-83) | evaluates `e` once, then the `case` tests in source order, comparing each with IsStrictlyEqual (`===`: `-0` matches `0`, `NaN` matches nothing, `"1"` does not match `1`) and evaluating none after the first match; runs from the matching clause, or from `default` (wherever it is) if none matches, through the following clauses (fall-through); an unlabelled `break` leaves the switch, `continue` the switch and the iteration of its enclosing loop; the clauses form one block scope whose function declarations are created on entry, before the tests |
| function declarations | hoisted: entering a script, function body or block creates the closures of the declarations directly in it, in source order, before its first statement runs. At the top level of a script or function body they are var-scoped (clj-quickjs-94): one may repeat a parameter, a `var` or another function declaration there, which all share one binding, and the last declaration wins (`function g() { return 1; } function g() { return 2; } g()` is 2; `var h; function h() {}` leaves `h` the function). In a block they are lexical (strict mode) |
| function expressions | each evaluation creates a new function object (`===` compares identity); a named expression's name is visible only inside it, is the running function, and assigning it is a TypeError |
| arrow functions | lowered exactly as an anonymous function expression whose concise body is `{ return body; }`: same closures, captures, cells and temporal dead zone; the subset has no `arguments`, `new.target` or `super`, so an arrow's lexical versions of them do not arise (they stay rejected inside arrows); `this` exists in non-arrow functions (clj-quickjs-116) but an arrow's lexical `this` is clj-quickjs-98, so it is rejected inside arrows, their parameters included |
| `f(a, b)` | evaluates the callee, then the arguments left to right, then calls in a new task-owned frame; missing arguments are `undefined`, extra ones are ignored; falling off the end returns `undefined`; the callee's `this` is `undefined` (strict-mode semantics; see `this` below); no `arguments` or `new` |
| `function f(a, b = expr) { ... }`, `(a = expr) => ...` (clj-quickjs-100) | on entry, the parameters are initialized left to right; a parameter's default is evaluated only when its argument is `undefined` (missing or passed: `f(1)` and `f(1, undefined)` run it, `f(1, null)` does not) and becomes its value. Parameters are bindings with a temporal dead zone: a default sees the parameters before it, the enclosing scopes and the function's own name, but reading itself or a later parameter is a ReferenceError (`function f(a = b, b) {} f()`). The parameters have a scope of their own around the body's: the body's `let`/`const`/`var`/function declarations are invisible to the defaults (`const x = 1; function f(a = () => x) { var x = 2; return a(); }` gives 1); a body `var` that repeats a parameter is a separate binding starting with the parameter's value, and a body function declaration may repeat one (a `let`/`const` may not). Closures created in a default capture the parameter bindings under the usual value/cell rule, so they see the body's later assignments to a parameter, and one run before its parameter is initialized fails with the run-time ReferenceError. QuickJS: `js_parse_function_decl2` (`has_parameter_expressions`) |
| `o.m(a, b)` (clj-quickjs-117) | a method call: evaluates `o` once, reads `m` (the checks of `o.name`), evaluates the arguments, then calls with `o` as the receiver (`call_method`); a parenthesized callee `(o.m)()` is the same call, `(0, o.m)()` is a plain call. A built-in function sees the receiver as `this`, and so does a JavaScript function (clj-quickjs-116). `(f = o.m)()` and `let f = o.m; f()` are plain calls too. A plain call passes `undefined` as the receiver |
| `this` (clj-quickjs-116) | inside a non-arrow function (its body and its parameters' defaults, and blocks, `try` and nested statements in it): the receiver the running call was made with, the very object (`get_this`, which reads the frame's `this` slot and allocates nothing): `o` for a method call `o.m()`, `undefined` for a plain call `f()`, for a task's entry function and for a callback of a resumable native (`__rt.reduce`). A function nested in it has its own `this`. In strict code the receiver is used as is, so `this.x` in a plain call throws a catchable TypeError. In sloppy code an object receiver is used as is, and evaluating `this` with any other receiver fails the task with `UnsupportedOperand` (it would be the global object, clj-quickjs-104, or a wrapper, clj-quickjs-73). The frame's `this` is task-owned and traced with the frame, so it survives waits, collections and migration |
| `Object` and other built-in globals (clj-quickjs-117) | a name with no lexical binding that is a built-in global (today only `Object`) is the runtime's shared, immutable intrinsic (`get_builtin_global`), the same object in every task; a `let`/`const`/`var`/parameter of that name shadows it |
| `Object(v)`, `Object()` (clj-quickjs-117) | `undefined`, `null` or no argument: a new ordinary object inheriting from `Object.prototype`; an object (any heap value): `v` itself; a number, string or boolean: `UnsupportedOperand` (wrapper objects, clj-quickjs-72). `Object.length` is 1, `Object.prototype.constructor === Object` |
| `Object.is(a, b)` (clj-quickjs-117) | SameValue: like `===` except `Object.is(NaN, NaN)` and `!Object.is(0, -0)` |
| `Object.prototype.toString()`, `.valueOf()` (clj-quickjs-117) | inherited by every literal: `toString` gives `"[object Object]"` for an ordinary object and the runtime's other non-function heap values, `"[object Function]"` for a function, `"[object Undefined]"`/`"[object Null]"` for those receivers (a plain call); `valueOf` gives the receiver itself, and throws a catchable `TypeError` for `undefined`/`null`; primitive receivers and error objects fail with `UnsupportedOperand` (clj-quickjs-72/-73, -230) |
| `Object.defineProperty(o, k, d)` (clj-quickjs-119) | `o` must be an object (else a catchable `TypeError`); `k` goes through ToPropertyKey as in `o[k]`; `d` must be an object whose `enumerable`, `configurable`, `value`, `writable`, `get`, `set` are read in that order, inherited ones included, and a field present with the value `undefined` counts (ToPropertyDescriptor). A `get`/`set` that is not a function or `undefined`, or one together with `value`/`writable`, is a `TypeError`. A new property takes the given fields, absent ones `undefined`/false (`{value: 1}` is read-only, non-enumerable and non-configurable); an existing one keeps the absent fields. A non-configurable property rejects becoming configurable, a change of `enumerable`, and, when it is also read-only, becoming writable or a value that is not SameValue (`NaN` matches `NaN`, `0` does not match `-0`): a catchable `TypeError` ("cannot define property 'k': ..."), nothing changed. On an array (clj-quickjs-118) an index key defines an element with the same rules (a new index extends `length`; one at or beyond a read-only `length` is a `TypeError`), and `length` (writable, non-enumerable, non-configurable) can be made read-only, after which writes of `length` and of new indices at or beyond it are rejected like other read-only properties; a define that would change `length` fails with `UnsupportedOperand` (ArraySetLength, clj-quickjs-39). Returns `o`. A built-in object as `o` is a `TypeError` (immutable); a value without JavaScript properties (vectors, maps, ...) is not extensible (`TypeError`); a function, error object or a descriptor with `get`/`set` (accessors, clj-quickjs-120) fails with `UnsupportedOperand`; another task's object with `ForeignObject` |
| `Object.getOwnPropertyDescriptor(o, k)` (clj-quickjs-119) | `undefined` if `o` has no own property `k` (inherited ones do not count), else a new object inheriting from `Object.prototype` with `value`, `writable`, `enumerable`, `configurable` in that order; works on built-in objects (`Object.getOwnPropertyDescriptor(Object, "length")` is `{value: 1, writable: false, enumerable: false, configurable: true}`); an array reports its elements and its `length` with their current attributes; it gives `undefined` for vectors, maps and other values without JavaScript properties. `undefined`/`null` `o` throw a catchable `TypeError`; a number, string or boolean `o` (wrapper objects, clj-quickjs-72), a function or an error object fail with `UnsupportedOperand` |
| `return` (in any function) | returns from that function |
| closures | a function sees the bindings of every enclosing scope, including after they are assigned and after the enclosing call returned; each execution of a block (each loop iteration) makes new bindings |
| a function's own name (`compile_function`, or a declaration never assigned) | the running function object (`current_function`), so self-recursion captures nothing |
| `__rt.name(args)` | the private VM operation of that intrinsic (see "Reserved intrinsic binding") |

Strings convert with StringToNumber and compare loosely with numbers
(clj-quickjs-67); each line below evaluates to `true`:

```js accept
+" 12\n" === 12 && +"0x1f" === 31 && +"" === 0;
("12345" | 0) === 12345 && ("0x12345" >>> 0) === 0x12345;
"6" * "7" === 42 && "2" ** "10" === 1024;
"" == 0 && "123" == 123 && "1" == true && "abc" != NaN;
```

Two strings compare by UTF-16 code units, a string and a number as numbers
(clj-quickjs-68); each line below evaluates to `true`:

```js accept
"b" > "a" && "a" < "aa" && "" < "a" && "B" < "a" && "a" <= "a";
"10" < "9" && !("10" < 9) && !("abc" < 1) && !("abc" >= 1);
"\u{10000}" < "\uFFFF" && !("\uFFFF" < "\u{10000}");
```

`typeof` gives a type name; a name with no binding is `"undefined"`, not a
ReferenceError (clj-quickjs-69); each line below evaluates to `true`:

```js accept
typeof 1 === "number" && typeof null === "object" && typeof typeof 1 === "string";
typeof nope === "undefined" && typeof (nope) === "undefined" && typeof undefined === "undefined";
typeof (() => 1) === "function" && typeof __rt.vector() === "object" && typeof "" === "string";
```

`typeof` of a binding in its temporal dead zone still fails, at run time
(`ReferenceError` "cannot access 'x' before initialization", at `x`), and
`typeof` is a prefix operator, so it cannot be the base of `**` unparenthesized:

```js reject Invalid 1:10
typeof a ** 2;
```

`void` evaluates its operand and gives `undefined` (clj-quickjs-133); the
line below evaluates to `true`:

```js accept
void 0 === undefined && void void 1 === undefined && typeof void 0 === "undefined";
```

Default parameter values run only for `undefined` arguments, left to
right, and do not see the body's declarations (clj-quickjs-100); the
script below evaluates to `true`:

```js accept
const x = 1;
function f(a, b = a + 1, g = () => x) { var x = 2; return b * 10 + g(); }
f(1) === 21 && f(1, undefined) === 21 && f(1, 5) === 51 && ((a = 1, c) => a + c)(undefined, 2) === 3;
```

Array literals have holes and a `length` one past the greatest index
written (clj-quickjs-118); each line below evaluates to `true`:

```js accept
[1, 2, 3][2] === 3 && [1, 2,].length === 2 && [ , ].length === 1 && [].length === 0;
[1, , 3].length === 3 && [1, , 3][1] === undefined && [] !== [] && typeof [] === "object";
let a = [1]; a[3] = 4; a.length === 4 && a["0"] === 1 && a[2] === undefined;
```

Computed member access converts its key by ToPropertyKey, so a number
and its string name one property (clj-quickjs-115); each line below
evaluates to `true`:

```js accept
let o = {}; o[1] = "one"; o["1"] === "one" && o[1.5] === undefined;
let p = {}; p[-0] = 0; p[true] = 1; p["0"] === 0 && p.true === 1 && p[undefined] === undefined;
let q = {}; let k = "y"; q[k] = 7; q.y === 7 && ({})["toString"] === Object.prototype.toString;
```

An array literal is not an assignment target (destructuring is its own
pellet):

```js reject Unsupported 1:1
[a, b] = [b, a];
```

Arrays inherit from `Array.prototype`, which has no methods yet: its
`toString` fails explicitly instead of answering `"[object Array]"`
(`UnsupportedOperand` at run time, at the call's `(`):

```js accept
[1, 2].toString();
```

A body `let`/`const` cannot repeat a parameter, with or without defaults:

```js compile-reject Invalid 1:25
function f(a = 1) { let a; }
```

Errors:

| Situation | When | Reported as |
|-----------|------|-------------|
| A name declared twice in one block, script, function body or `switch` case block (including a `let`/`const` that repeats a parameter, also with default values, and a function declaration that clashes with a `let`/`const` or repeats one in a block), other than the var-scoped repetitions above | compile time | `SyntaxError`, kind `Invalid`, at the second declaration |
| A `var` whose name a `let`, `const` or block-level function declaration binds in the same body (`let a; var a;`) or in a block enclosing the `var` within the same function (`{ let a; { var a; } }`), or a `for` head binding of an enclosing loop | compile time | `SyntaxError`, kind `Invalid`, at the `var`'s name |
| Assigning a built-in global (`Object = x`, `Object += 1`, `Object++`; clj-quickjs-104 makes them global-object properties), and (in `compile_function`) assignment to the entry function's own name | compile time | `SyntaxError`, kind `Unsupported`, at the form |
| Misuse of the reserved intrinsic binding `__rt` | compile time | see "Reserved intrinsic binding" |
| More than 65,535 live bindings, 65,536 distinct constants, 65,536 functions in one function, or 65,535 call arguments or captured bindings | compile time | `SyntaxError`, kind `TooLarge` |
| Reading, assigning, incrementing or decrementing a name with no binding (strict mode; there is no global object, so sloppy code fails the same way until clj-quickjs-105), except as the operand of `typeof` | run time, only if reached, after the assigned value is evaluated (a compound assignment reads first, so it fails before evaluating its right operand) | `VmErrorKind::ReferenceError`: "x is not defined" |
| Reading, assigning, incrementing or decrementing a `let`/`const` before its declaration ran (temporal dead zone), including inside its own initializer, a parameter in its own or an earlier parameter's default value, as the operand of `typeof`, and from a closure that runs before it | run time, only if reached | `VmErrorKind::ReferenceError`: "cannot access 'x' before initialization" |
| Assigning to an initialized `const` (`c = 1`, `c += 1`, `c++`), or to a named function expression's name inside it (also in sloppy code until clj-quickjs-96) | run time, only if reached, after the value is evaluated | `VmErrorKind::TypeError`: "assignment to constant variable 'x'" |
| Calling a value that is not a function | run time | `VmErrorKind::TypeError`: "number is not a function", "string is not a function" (names the value's type) |
| Calls nested deeper than the task's `VmLimits` (for example unbounded recursion) | run time | `VmErrorKind::StackOverflow` |

Run-time failures carry the source position (`Diagnostic::source`, line and
column) of the failing reference or operator, and the call chain
(`Diagnostic::call_chain`). The four run-time errors above are JavaScript
exceptions: a `try` statement can catch them (see "Exceptions").

The position is the token of the failing operation, as in QuickJS
(`emit_source_loc` at the operator token; clj-quickjs-52), not the start of
the enclosing expression. Every form that can fail at run time follows this
convention, and so must every form added later (`[]` member access, ToPrimitive
in operators, and so on):

| Failing operation | Position |
|-------------------|----------|
| a binary operator (`1 + 2 * f`, `a ** f`, `f < 2`, `a & f`, ...) | the operator token (`*`) |
| a prefix operator (`-f`, `+f`, `~f`) | the operator token (where the expression starts) |
| `++`/`--`, prefix or postfix, and compound assignment `x op= v` | the `++`/`--` or `op=` token, for the conversion and the operator |
| a call `f(a)`, including `__rt.name(...)` intrinsics (a need's wait, a native's failure) | the call's `(` |
| a caller frame in the call chain (`CallSite::source`), also for `Task::source_location` of a task suspended in a call | that call's `(` |
| a member access `o.name`, read or write (clj-quickjs-114) | the `.` (`[` for a computed access) |
| a reference to, assignment to or update of a binding (ReferenceError, temporal dead zone, `const` TypeError) | the identifier |

For example (each script compiles; the comment gives where it fails at run
time, as `tests/js_source.rs` `runtime_error_positions_follow_operator_tokens`
checks):

```js accept
let f = function(){};
  1 + 2 * f;          // UnsupportedOperand at 2:9, the `*`
```

```js accept
let n = null;
 1 + n ( 1234 );      // TypeError "null is not a function" at 2:8, the `(`
```

```js accept
let v = 1;
  __rt.nth ( v, 0 );  // collection error at 2:12, the `(`
```

```js accept
function g() { return missing; }
function f() {
  return 1 + g ( );
}
f();                  // ReferenceError at 1:23; call chain 3:16, then 5:2
```

```js accept
1 + (not_def = 1);    // ReferenceError at 1:6, the identifier
```

Compile-time errors (`SyntaxError`) keep their own positions, given with each.

### Exceptions

`throw`, `try`/`catch`/`finally` and `return` through finally blocks follow
JavaScript (clj-quickjs-11). The VM keeps the handlers and the *pending
completion* of a running finally block (normal, a saved return value, a
saved exception, or a `break`/`continue` jump with its resume point,
clj-quickjs-79) in task-owned state, so a task can yield, wait or migrate
inside any of them and resume exactly there; a finally block never runs
because the task suspends. An abrupt completion of a finally block (`return`,
`throw`, or a `break`/`continue` out of it) replaces the pending one; a
normal completion keeps it.

```js accept
function f() {
  try { return 1; } finally { cleanup(); }
}
try { f(); } catch (e) { throw e; } finally {}
```

| Thrown | Value seen by `catch` | Reported if uncaught |
|--------|-----------------------|----------------------|
| `throw v` (any value) | `v` itself, by identity | `VmErrorKind::Thrown`, "uncaught exception: v", at the last `throw` of it |
| ReferenceError, TypeError, StackOverflow above | a fresh immutable error object (no properties yet) | the original error, at the failing instruction, also when the object is rethrown |
| a failed or cancelled cell read by `__rt.need` (clj-quickjs-27) | a fresh immutable error object whose cause is the producer's published error value (`undefined` for a cancelled cell) | `DependencyFailed(key)` / `DependencyCancelled(key)` at the `need`, also when rethrown |
| any other run-time failure (`UnsupportedOperand`, collection, builder, atom or binding-cell contract violations, host errors, out of memory) | not catchable: the task fails at once without running catch or finally blocks | that failure |

A failure that leaves through finally blocks keeps the position and call
chain where it was raised. A `__rt.need` that waits raises nothing and runs
no handler. Cancelling the task (`Task::cancel`) is not catchable: it runs
the pending finally blocks, skipping catch blocks, then the task ends
cancelled; a `return`, exception, `break`, `continue` or `__rt.need` that
would escape such a finally block ends the cleanup there (`src/task.rs`,
"Cancellation and finally blocks").

Gaps specific to execution:

- **Strings convert between primitives only** (clj-quickjs-65,
  clj-quickjs-66, clj-quickjs-67, clj-quickjs-68). String literals,
  `===`/`==` between strings, relational comparison of two strings (by
  code units), `+` with a string operand (ToString of a primitive, then
  concatenation), StringToNumber (in the arithmetic, bitwise, shift, `**`
  and `++`/`--` operators, in `==` against a number or boolean, and in
  relational comparison against a non-string), ToBoolean and strings as
  map keys execute. Every operation that would convert an object fails
  with `UnsupportedOperand` instead of guessing (ToPrimitive,
  clj-quickjs-70). `typeof`,
  `length`, indexing and string methods are not
  supported yet. Untagged template literals execute (clj-quickjs-57);
  tagged templates and `String.raw` are clj-quickjs-58.
- **Error objects are opaque.** A caught ReferenceError, TypeError or
  StackOverflow is an object without properties (`e.message`, `e.name`,
  `instanceof` and the `Error` constructors do not exist yet); it can be
  compared, passed around and rethrown.
- **No ToPrimitive on objects.** Source can now create function objects and
  (through `__rt`) collections and builders; arithmetic, relational,
  bitwise and shift operators and `++`/`--` applied to them fail with `UnsupportedOperand` where JavaScript
  would call ToPrimitive (`===`/`==` compare them by identity).
- **Temporal dead zone: static within a function, dynamic across
  closures.** An exception or `return` only
  leaves a `try` block for its catch or finally block (outside the scope
  of the block's bindings); an unlabelled `break` only leaves scopes and
  `continue` jumps to the loop test, from which the body is entered again
  at its start (clj-quickjs-78; the test of a `do`-`while` follows its body
  outside the body's scope, and its back edge also enters the body at its
  start, clj-quickjs-80; a `for` head's bindings are all initialized
  before its test, and its test, body, update and `continue` target all
  lie after them in its scope, while the back edge enters the body at its
  start, clj-quickjs-81); a `break` or `continue` through finally
  blocks enters each one at its start and then goes on leaving scopes
  (clj-quickjs-79); a labelled `break` (clj-quickjs-82) only leaves scopes,
  to the end of its labelled statement, which is never a declaration, and
  a labelled `continue` jumps to the `continue` target of an enclosing
  loop like an unlabelled one, so no jump enters a scope past a
  declaration. So
  within the function that declares a binding, whether it is initialized at a reference is
  known from source order, and the compiler
  decides it statically. A closure may run at any time, so a binding a
  closure captures is checked at run time when it may be read before its
  initialization: it lives in a binding cell, which raises the
  `ReferenceError` (clj-quickjs-20). The one jump that does enter a scope
  past declarations is a `switch` selecting a clause (clj-quickjs-83):
  control enters the case block at the matching clause (or `default`), past
  the declarations of the clauses before it. So every `let`/`const`
  declared in a clause before the last non-empty one lives in a binding
  cell, captured or not, and each read or assignment after its declaration
  checks it at run time (`switch (1) { case 0: let y = 1; case 1: y; }` is
  a ReferenceError at `y`). A reference before the declaration, in an
  earlier clause or a `case` test, always runs before it and is still
  decided statically, as are the bindings of the last non-empty clause,
  which no clause after it can reach.
- **Captured bindings are values or cells.** A captured binding that is
  never assigned and is initialized whenever a closure over it is created
  is copied into the closure; any other captured binding (assigned
  somewhere, or captured by a closure that may be created before the
  binding is initialized: for example a hoisted function declaration that
  uses a `let`/`const` of its own statement list, since the declaration is
  created before any of them is initialized) lives in a task-owned binding
  cell. Both behave as JavaScript bindings. The difference is publication:
  a closure holding a cell cannot be published to other tasks
  (`Runtime::publish` rejects it; another task calling it fails with
  `VmErrorKind::ForeignBinding`). So a function meant to be shared should
  capture only `const`s and never-assigned parameters that are initialized
  before it is created (a function expression after the `const`, not a
  hoisted declaration). A closure in a default parameter value that
  captures its own or a later parameter (`function f(g = () => b, b)`)
  holds a cell (clj-quickjs-100).
- **Functions have no properties.** No `length`, `name`, `prototype`,
  `call`/`apply`/`bind`, `arguments`, `new`, rest parameters, getters or
  generators; `this` is only a non-arrow function's receiver (see
  "Explicit gaps").

## Reserved intrinsic binding

`docs/design.md` §§5, 7 let the compiler lower known operations to private
runtime intrinsics. This subset reserves exactly one binding for them,
`__rt` (`src/frontend/intrinsics.rs`). It is not a JavaScript value; it is a
binding the compiler places in the outermost scope of every program, and
the contract below makes every occurrence of `__rt` resolve to it, so an
intrinsic is recognized by binding resolution, never by the spelling of an
arbitrary callee (a user function named `vector` or `need` is an ordinary
function):

| Rule | Example | Rejected as |
|------|---------|-------------|
| No shadowing: `__rt` cannot be declared by `let`, `const`, a parameter, a function declaration, a function expression's name, or the `compile_function` entry | `let __rt = 1;` | `ReservedWord` (compiler) |
| No reassignment of the binding or of an intrinsic | `__rt = x;`, `__rt.count = f;` | `ReservedWord` (compiler; parser for the property) |
| No computed access: intrinsics are named statically | `__rt["count"](v)` | `ReservedWord` (parser) |
| No aliasing: `__rt` appears only as `__rt.name(args)`, a direct call | `const r = __rt;`, `f(__rt)`, `const c = __rt.count;` | `ReservedWord` (compiler) |
| Known names, fixed arities, literal host operation selectors | `__rt.nope()`, `__rt.nth(v)`, `__rt.effect(op, x)` | `Invalid` (compiler) |

Intrinsics evaluate their arguments left to right, then run one VM
instruction (operands, results and run-time errors: `crate::vm::intrinsics`,
`crate::host`, for `need`, `crate::task`, and for `reduce` and `needAll`,
`crate::vm::native`):

| Call | Instruction | Result |
|------|-------------|--------|
| `__rt.vector(x1, …, xn)` | `new_vector n` | new persistent vector |
| `__rt.map(k1, v1, …, kn, vn)` | `new_map n` | new persistent map (even argument count) |
| `__rt.count(c)` | `count` | element or entry count |
| `__rt.nth(v, i)` | `nth` | vector element |
| `__rt.get(c, k)`, `__rt.getOr(c, k, d)` | `get`, `get_or` | lookup (fails if absent / `d` if absent) |
| `__rt.assoc(c, k, v)`, `__rt.dissoc(m, k)` | `assoc`, `dissoc` | persistent update |
| `__rt.conj(v, x)`, `__rt.popLast(v)` | `conj`, `pop_last` | persistent append / remove-last |
| `__rt.vectorBuilder()`, `__rt.mapBuilder()` | `new_vector_builder`, `new_map_builder` | task-owned transient builder |
| `__rt.builderConj(b, x)`, `__rt.builderAssoc(b, k, v)` | `builder_conj`, `builder_assoc` | the builder, updated in place |
| `__rt.freeze(b)` | `freeze` | persistent collection; the builder is frozen |
| `__rt.effect(op, x)` | `host_effect op` | the leaf host effect's result; `op` must be an integer literal `0..=65535` |
| `__rt.need(definition, kind)` | `need` | the shared result `kind` of `definition` in the task's world, suspending the whole task while it is pending (clj-quickjs-23); both are non-negative integers (a bad literal is a compile error) |
| `__rt.reduce(f, init, v)` | `reduce` | resumable native fold (clj-quickjs-12): `f(acc, x)` for each element of the vector `v` in order, starting from `init`; a callback may suspend the whole task, which continues inside the fold without repeating completed elements |
| `__rt.needAll(defs, kind)` | `need_all` | resumable native (clj-quickjs-12): a new vector of the shared results `kind` of each definition id in the vector `defs`, suspending at the call for each pending one |

```js compile-reject ReservedWord 2:14
function helper(x) {
  const rt = __rt;
  return x;
}
```

```js compile-reject ReservedWord 1:12
function f(__rt) { return 1; }
```

```js compile-reject Invalid 1:6
__rt.toString();
```

```js compile-reject ReservedWord 1:8
typeof __rt;
```

```js compile-reject Invalid 1:41
switch (x) { case 1: let a; case 2: let a; }
```

```js reject ReservedWord 1:5
__rt[name](1);
```

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
// String values execute (clj-quickjs-65): content equality, concatenation
// of two strings, ToBoolean, map keys by content.
let greeting = "hello, " + 'world';
const m = __rt.map(greeting, 1);
greeting === "hello, world" && "1" != "01" && !"" ? __rt.get(m, "hello, world") : 0
```

```js accept
// `+` with a string operand concatenates ToString of both (clj-quickjs-66).
let n = 1, t = true;
"got |" + n + "|, expected |" + t + "|" + null + undefined + 1e21 + -0
```

```js accept
// Arrow functions; ASI ends the statement after a block body.
let g = (a, b) => a + b;
const k = x => () => x, h = (a,) => { return k(a)(); }
() => {}
() => {}
() => {}
+1
```

```js accept
// The comma operator; arguments and declarators are separate items.
let a = 1, b = (a, 2);
f((a, b), b);
if (a = a + 1, a < b) a, b;
() => {}
, 1
```

```js accept
// Unlabelled `break` and `continue`; ASI after the keyword.
let i = 0;
while (true) {
  i = i + 1;
  if (i < 3) continue
  try { break; } catch (e) {}
}
```

```js accept
// `break` and `continue` run the finally blocks they leave (clj-quickjs-79).
while (x) {
  try { if (y) continue; break; } finally { cleanup(); }
}
```

```js accept
// `do`-`while` (clj-quickjs-80): `continue` jumps to the test; the `;` after
// `while (...)` is inserted even on the same line.
let n = 0;
do { n = n + 1; if (n < 3) continue; } while (n < 5) n
```

```js accept
// `for` (clj-quickjs-81): `continue` runs the update; each iteration has
// its own `i`, so each closure keeps its iteration's value.
const b = __rt.vectorBuilder();
for (let i = 0, j; i < 3; i++) { if (i === 1) continue; __rt.builderConj(b, () => i); }
for (n = 0; ; ) break;
for (;;) { break; }
```

```js accept
// Labelled statements (clj-quickjs-82): `continue` to any label of a loop's
// label set, `break` out of any labelled statement; a label is not a binding.
outer: inner: for (let i = 0; i < 3; i++) {
  while (true) { if (i === 1) continue outer; break inner; }
}
let x = 0;
x: { if (x) break x; x = 1; }
while (1) L: break
x
```

```js accept
// `switch` (clj-quickjs-83): `===` matching, `default` anywhere,
// fall-through; `break` leaves the switch, `continue` its loop. The
// clauses are one scope.
for (let i = 0; i < 4; i++) {
  switch (i) {
    default: log(i);
    case 0: case 1: { break; }
    case 2: continue;
    case "3": let s = i; function f() { return s; }
  }
}
```

```js accept
const f = __rt.need("defn/foo", "callable")
f.call(undefined, NaN, Infinity, null, true, false, 0x1F, 0b101, 0o17, .5, 5., 1e3)
```

```js accept
// Bitwise and shift operators (clj-quickjs-62).
let h = (x << 5) - x + 7 | 0;
const lo = h & 0xFFFF, hi = h >>> 16, sign = ~h >> 31;
```

```js accept
// Exponentiation (clj-quickjs-63): right-associative, tighter than `*`.
const area = 3 * 2 ** 2, big = 2 ** 3 ** 2, half = (-2) ** -1, neg = -(2 ** 2);
```

```js accept
// Prefix and postfix `++`/`--` on bindings (clj-quickjs-64); a line
// break before `++` starts a new statement.
let i = 0, j = i++ + ++i, k = (j)-- ** 2;
i
++j
```

```js accept
// Compound assignment on bindings (clj-quickjs-84): right-associative,
// with the targets of `=`; `a>>>=b` is one `>>>=` token.
let s = "", n = 1, m = 2;
s += "x" + n; n -= m *= 3; (n) **= 2; n %= 7; m /= 2;
m <<= 1; m >>= 1; m >>>= 0; m &= 0xFF; m |= 1; m ^= n; a>>>=b
```

```js accept
#!/usr/bin/env qjs
let x = 1;
```

```js accept
// Unicode identifiers and escapes; `Ãµ` and `õ` are different names.
let Ãµ = 3, π = 3.14, ℮ = 2.72, 𝒜 = 1, x\u{200C}y = 0, a\u0062c = 2;
const õ = Ãµ + abc + a.l\u0065t;
```

```js accept
// `var` (clj-quickjs-94): function-scoped and hoisted; top-level function
// declarations are var-scoped, so the second `g` replaces the first.
function f(a, flag) {
  if (flag) var x = a;
  { var a = a + 1; }
  function g() { return 1; }
  function g() { return x === undefined ? a : x; }
  return g();
}
var r = f(1, false), r2;
```

```js accept
// A member access may follow a number literal (clj-quickjs-48).
const a = 0.1.a, b = 0x1.a, c = .5.a, d = 1..toString;
```

```js accept
// Object literals and named properties (clj-quickjs-114): keys may be
// reserved words, strings or numbers; `{get}` is a shorthand.
let get = 1;
const o = {x: 1, if: 2, "two words": 3, 4: 5, get,};
o.y = o.x + o.if;
({a: {b: o}}).a.b.y
```

The built-in global `Object`, `Object.prototype` and method calls
(clj-quickjs-117); each line below evaluates to `true`:

```js accept
({}).toString() === "[object Object]" && ({}).toString === Object.prototype.toString;
Object.is(NaN, NaN) && !Object.is(0, -0) && Object() !== Object() && typeof Object === "function";
let o = {toString: () => "own"}; o.toString() === "own" && Object(o) === o && o.valueOf() === o;
```

```js accept
// A method call passes its base as `this` (clj-quickjs-116); a plain
// call, `(0, o.m)()` included, passes `undefined`.
let counter = {n: 0, add: function (k = 1) { this.n = this.n + k; return this; }};
counter.add().add(2).n === 3 && (counter.add)() === counter;
function who() { return this; }
who() === undefined && (0, counter.add) !== undefined;
```

Data property attributes (clj-quickjs-119); each line below evaluates to
`true`:

```js accept
let o = {}; Object.defineProperty(o, "k", {value: 1}) === o && Object.getOwnPropertyDescriptor(o, "k").writable === false;
let p = {}; Object.defineProperty(p, "k", {value: 1}); let r = false; try { p.k = 2; } catch (e) { r = p.k === 1; } r;
let d = Object.getOwnPropertyDescriptor({x: 1}, "x"); d.writable && d.enumerable && d.configurable && Object.getOwnPropertyDescriptor({}, "x") === undefined;
```

Rejected (`kind line:column` of the reported position):

```js reject Unsupported 2:12
// `this` at the script top level (clj-quickjs-104).
let self = this;
```

```js reject Unsupported 2:29
// An arrow's lexical `this` (clj-quickjs-98).
function f() { return () => this; }
```

```js reject ReservedWord 1:3
({if});
```

```js reject Unsupported 1:3
({[key]: 1});
```

```js reject Unsupported 1:3
({get x() { return 1; }});
```

```js compile-reject Unsupported 2:1
let o = {f: function () { return 1; }};
Object = o.f;
```

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

```js reject UnexpectedToken 1:10
() => {} + 1
```

```js reject UnexpectedToken 2:1
let f = x
=> x + 1;
```

```js reject Invalid 1:13
let g = (a, a) => a;
```

```js reject UnexpectedToken 1:6
a ? b, c : d;
```

```js accept
x = `a${b, `c${d}`}e
f`;
```

```js reject Unsupported 1:6
x = f`template`;
```

```js reject InvalidToken 1:7
x = `a\1`;
```

```js reject Unsupported 1:1
a.b++;
```

```js reject Invalid 1:1
f()++;
```

```js reject UnexpectedToken 1:4
a--b;
```

```js reject Unsupported 1:2
a&&=b
```

```js reject Unsupported 1:3
h ??= 1;
```

```js reject Unsupported 1:1
a.b += 1;
```

```js reject Invalid 1:1
f() *= 2;
```

```js reject Invalid 1:4
-2 ** 2;
```

```js reject ReservedWord 1:5
let if = 1;
```

```js reject ReservedWord 1:7
const undefined = 1;
```

```js reject ReservedWord 1:1
l\u0065t x = 1;
```

```js reject InvalidToken 1:5
let \u0030x = 1;
```

```js reject UnknownToken 1:5
let € = 1;
```

```js reject Invalid 1:1
return 1;
```

```js reject Invalid 1:7
const c;
```

```js reject Invalid 2:18
while (x) {
  function f() { continue; }
}
```

```js reject Invalid 1:17
while (x) break outer;
```

```js reject Invalid 1:23
switch (x) { default: default: }
```

```js reject Invalid 1:22
switch (x) { case 1: continue; }
```

```js reject Invalid 1:15
L: { continue L; }
```

```js reject Invalid 2:3
L: {
  L: while (x) break L;
}
```

```js reject Invalid 1:4
L: function f() {}
```

```js reject Invalid 1:4
do let a = 1; while (0);
```

```js reject UnexpectedToken 1:7
do x; y;
```

```js reject Unsupported 1:1
for (const x of xs) {}
```

```js reject Unsupported 1:1
for (k in o) {}
```

```js reject Unsupported 1:6
for (var i = 0; i < 3; i++) {}
```

```js compile-reject Invalid 1:16
{ let a; { var a; } }
```

```js compile-reject Invalid 2:5
let a;
var a;
```

```js reject Invalid 1:12
for (const k; ;) {}
```

```js reject Invalid 1:10
for (;;) let x;
```

```js reject UnexpectedToken 2:1
for (a
b;;) {}
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

```js reject UnexpectedToken 1:12
let n = 0.1.2;
```

```js reject UnknownToken 1:9
let n = @x;
```

```js reject UnknownToken 2:1
let x = 1;
#!/usr/bin/env qjs
```

## References

Structure consulted (no code adapted): `research/quickjs/quickjs.c` at
revision 7d1c13c5d8f8 (`VERSION` 2026-06-04): `next_token`,
`js_parse_expr_binary`, `js_parse_assign_expr`, `js_parse_statement`,
`js_parse_expect_semi`; and the ASI cases of `test_parse_semicolon` in
`research/quickjs/tests/test_language.js` (its `yield`/`await` cases
are outside the subset). For arrow functions (clj-quickjs-53):
`js_parse_skip_parens_token` (deciding that a parenthesized list is an
arrow's parameters by what follows its `)`), `js_parse_assign_expr2`
(arrows only at the start of an AssignmentExpression) and
`js_parse_function_decl2` (concise and block arrow bodies), plus the
`test_parse_arrow_function` cases of `test_language.js` (adapted in
`frontend::tests::arrow_functions_and_asi` and
`tests/js_functions.rs` `arrow_function_values`, with the QuickJS notice;
its `async` arrow cases are outside the subset). For functions and closures (clj-quickjs-21):
`js_parse_function_decl` (function name and self-binding scoping),
`get_closure_var` / `add_closure_var` (resolving a name through enclosing
functions and threading it down as captured variables) and
`instantiate_hoisted_definitions` (function declarations created on scope
entry). For hashbang comments (clj-quickjs-49): `skip_shebang`. For the comma operator
(clj-quickjs-61): `js_parse_expr2` / `js_parse_expr` (each operand but the
last is evaluated and dropped). For the bitwise and shift operators
(clj-quickjs-62): `js_parse_expr_binary` (precedence levels),
`js_binary_logic_slow`, `JS_ToInt32Free`, the `OP_shr` / `OP_sar` cases of
`JS_CallInternal` and `js_unary_arith_slow` (`~`); the run-time cases are
adapted from `test_op1` and `test_cvt` of
`research/quickjs/tests/test_language.js` in `tests/js_source.rs`
`bitwise_operators_follow_to_int32`, with the notice. For the exponentiation
operator (clj-quickjs-63): `js_parse_unary` (the `PF_POW_FORBIDDEN`
restriction on a unary base), `js_pow` and `OP_pow`; `2 ** 8` in
`tests/js_source.rs` `exponent_operator` is adapted from `test_op1`, with
the notice. For `++`/`--` (clj-quickjs-64): `js_parse_unary` (the
`TOK_INC`/`TOK_DEC` case), `js_parse_postfix_expr` (no line terminator
before a postfix operator), `OP_inc` / `OP_post_inc` and `js_post_inc_slow`
(the old value converted with ToNumeric); the four identifier groups of
`test_inc_dec` in `test_language.js` are adapted in `tests/js_source.rs`
`update_expressions_on_bindings`, with the notice. For loose
equality (clj-quickjs-60): `js_eq_slow` (a boolean operand converted with
ToNumber); the `null`/`undefined`/boolean cases of `test_eq` in
`test_language.js` are adapted in `tests/js_source.rs`
`loose_equality_with_a_boolean_left_operand`, with the notice. For Unicode
identifiers (clj-quickjs-50): `next_token`'s `lre_js_is_ident_first` /
`lre_js_is_ident_next` checks and `ident_realloc` (escapes decoded into the
name, an escaped reserved word never a keyword); the tables here are
generated from perl's Unicode database, not from QuickJS's
`libunicode-table.h`. For `break` and `continue` (clj-quickjs-78):
the `TOK_BREAK` / `TOK_CONTINUE` case of `js_parse_statement_or_decl`,
`emit_break` (the chain of enclosing loops and handlers a jump leaves, and
its early errors), `push_break_entry` and `close_scopes`; for jumps through
finally blocks (clj-quickjs-79), `emit_break`'s `OP_gosub` to each
enclosing finally block and the `OP_ret` it returns through (here a pending
jump completion of the task-owned handler stack, `src/vm/exception.rs`).
For `do`-`while` (clj-quickjs-80): the `TOK_DO` case of
`js_parse_statement_or_decl` (`continue` targets the test; a missing `;`
after `while (...)` is inserted, as `js_parse_expect_semi` documents); its
`set_eval_ret_undefined` on every iteration is not followed, since
ECMAScript keeps the completion value across iterations. `test_do_while`
of `research/quickjs/tests/test_loop.js` is adapted in
`tests/js_source.rs` `do_while_loops` with the notice.
For `for` (clj-quickjs-81): the `TOK_FOR` case of
`js_parse_statement_or_decl` (a scope for the head's lexical bindings,
`continue` targeting the update, `close_scopes` before the first iteration
and `OP_close_loc` on the loop variables before the update, the
per-iteration copy that this lowering does with new binding cells). The
first group of `test_for` of `research/quickjs/tests/test_loop.js` is
adapted in `tests/js_source.rs` `for_loops` with the notice.
For `switch` (clj-quickjs-83): the `TOK_SWITCH` case of
`js_parse_statement_or_decl` (the discriminant evaluated once, `case`
tests in order with `OP_strict_eq`, then a jump to `default`; one scope
for the case block; "duplicate default"). `test_switch1` and
`test_switch2` of `research/quickjs/tests/test_loop.js` are adapted in
`tests/js_source.rs` `switch_statements` with the notice.
For `typeof` (clj-quickjs-69): `js_parse_unary` (an identifier operand
read with `OP_get_var_undef`, so an unresolvable name is not an error),
`js_operator_typeof` and `OP_typeof`; the `typeof` rows of `test_op2` of
`test_language.js` are adapted in `tests/js_source.rs` `typeof_operator`
(notice on `identifiers_are_not_normalized`, which also runs the
`typeof õ` case of `test_unicode_ident`).
For `void` (clj-quickjs-133): the `TOK_VOID` case of `js_parse_unary`
(`OP_drop` then `OP_undefined`); the `assert(x, void 0)` of
`test_destructuring` in `test_language.js` is the `void 0` case of
`tests/js_source.rs` `void_operator` (written fresh, no upstream text).
For `var` (clj-quickjs-94): `js_parse_var` (declarators; an initializer
lowered as an assignment to the resolved name), `define_var` (a `var`
reuses the function's one variable of that name and is an error inside a
lexical declaration's scope, except a catch parameter's) and
`instantiate_hoisted_definitions` (function declarations of the body
assigned at entry, so the last of the same name wins).
For default parameter values (clj-quickjs-100): `js_parse_function_decl2`
(`has_parameter_expressions`: a scope of its own for the parameters, a
default evaluated when `OP_get_arg` is `undefined`, the body's `var`s
initialized from parameters of the same name); two cases of
`test_function_length` and `test_argument_scope` of `test_language.js` are
adapted in `tests/js_functions.rs` `default_parameters` with the notice.
`test_try_catch5` of `research/quickjs/tests/test_loop.js` is adapted in
`tests/exceptions.rs` `break_and_continue_run_finally_blocks`, and
`test_try_catch1`–`test_try_catch8` with their upstream strings and
`assert` calls in `tests/fixtures/js/quickjs_try_catch.js` (clj-quickjs-227;
`let` for `var`, an `assert` that throws its message string, and a counted
`for` for the `for`-`in` of `test_try_catch8`), each with the notice.
For run-time error positions (clj-quickjs-52): `emit_source_loc` calls at
the operator token in `js_parse_postfix_expr`, `js_parse_unary` and
`js_parse_assign_expr2`, and `build_backtrace` / `get_line_col` (a caller
frame at its call); cases of `test_line_column_numbers` of
`research/quickjs/tests/test_builtin.js` are adapted in `tests/js_source.rs`
`runtime_error_positions_follow_operator_tokens` with the notice. The tests
in `src/frontend/tests.rs` are written for this project and are not adapted
from QuickJS, except the hashbang line of `research/quickjs/tests/test_std.js`
in `hashbang_comment_only_at_start`, which carries its notice, the name
`Ãµ` of `test_unicode_ident` (`research/quickjs/tests/test_language.js`),
whose case `identifiers_are_not_normalized` in `tests/js_source.rs` adapts
with its notice, and the first statement of `test_labels2` in
`test_language.js` (without its label) in `break_and_continue`, which
carries the notice; `test_while_break` of
`research/quickjs/tests/test_loop.js` is adapted in `tests/js_source.rs`
`while_break_and_continue` with the notice;
`tests/fixtures/js/quickjs_closure.js` is adapted from
`research/quickjs/tests/test_closure.js` and carries its notice (`docs/architecture.md`, "QuickJS references and provenance").
