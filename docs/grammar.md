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

Status: regular expression literals compile and execute with the RegExp
object support of clj-quickjs-188 and Unicode mode of clj-quickjs-190.
Forms below parse and compile to
VM bytecode and execute (`frontend::compile_script`; see
"Execution" below): literals (string values since clj-quickjs-65), bindings,
operators, assignment, blocks, `if`/`else`, `while`, `do`-`while`
(clj-quickjs-80), `for` with an empty, expression or `let`/`const` head
(clj-quickjs-81), `for`-`in` over own enumerable properties with an
identifier, `let` or `const` head (clj-quickjs-85), function declarations
and expressions, calls, closures (clj-quickjs-21), arrow functions
(clj-quickjs-53), and the
compiler-reserved intrinsic binding `__rt` (see "Reserved intrinsic
binding"). Object literals with literal or computed keys, `o.name` reads and
`o.name = v` assignments (clj-quickjs-114) parse and execute; method
calls `o.f()` pass their base as the receiver of a built-in function, and
the built-in global `Object` with `Object.prototype` and `Object.is`
exists (clj-quickjs-117; see "Built-in globals" under "Execution"), with
`Object.defineProperty` and `Object.getOwnPropertyDescriptor` for data
and accessor properties (clj-quickjs-119/-120), and with `Object.getPrototypeOf`,
`Object.setPrototypeOf`, `Object.create`, `o.__proto__` and a literal's
`__proto__: value` for prototypes (clj-quickjs-121), with
`Object.preventExtensions`, `Object.isExtensible`, `Object.freeze` and
`Object.isFrozen` for extensibility (clj-quickjs-122), and with `Object.keys`
and `Object.getOwnPropertyNames` for own-key enumeration (clj-quickjs-123).
`Function.prototype` with `call` and `apply`, and the built-in globals
`Function` (a stub constructor) and `Reflect` with `Reflect.apply`
(clj-quickjs-102), call a function with an explicit receiver.
`throw` and `try`/`catch`/`finally` (clj-quickjs-11) parse and
execute; see "Exceptions" below. Unlabelled `break` and `continue` in `while`,
`do`-`while` and `for` loops (clj-quickjs-78, -80, -81) parse and execute, including
jumps out of `try` statements that run their finally blocks on the way
(clj-quickjs-79). Labelled statements and labelled `break`/`continue`
(clj-quickjs-82) parse and execute. Prefix and postfix `++`/`--` on identifier bindings
(clj-quickjs-64) and on properties `o.p` and `o[k]` (clj-quickjs-76) parse and
execute. `switch` statements (clj-quickjs-83)
parse and execute. Arithmetic, bitwise, shift and `**` compound assignment
(`+=`, `-=`, `*=`, `/=`, `%=`, `**=`, `&=`, `|=`, `^=`, `<<=`, `>>=`,
`>>>=`) on identifier bindings (clj-quickjs-84) and on properties `o.p` and
`o[k]` (clj-quickjs-260) parses and executes; the
logical assignments `&&=`, `||=` and `??=` do not. The `typeof` operator
(clj-quickjs-69) parses and executes, including on a name with no binding.
The `void` operator (clj-quickjs-133) parses and executes. The `instanceof`
operator (clj-quickjs-75) parses and executes. The `delete` operator on
property references `o.p` and `o[k]` (clj-quickjs-124) parses and executes;
`delete` of an identifier executes in sloppy code (clj-quickjs-105) and is
an early error in strict code. Sloppy assignment to an undeclared name
creates a configurable property of the global object.
The `in` operator (clj-quickjs-74) parses and executes.
Property reads and method calls on number and string primitives
(`(1).toString()`, clj-quickjs-73) execute.
`var` statements (clj-quickjs-94) parse and execute, with function-level
hoisting; top-level function declarations of a script or function body
are var-scoped. `var` heads of `for` and for-in statements
(`for (var i = 0; ;)`, `for (var k in o)`, clj-quickjs-89) parse and
execute. `"use strict"` directive prologues are interpreted and
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
  `yield` as identifiers everywhere; they are operators in async and
  generator bodies, respectively (generator execution: clj-quickjs-243).

## Lexical grammar

| Element | Accepted | Rejected |
|---------|----------|----------|
| Whitespace | TAB, VT, FF, SP, NBSP, ZWNBSP (BOM), Unicode `Zs` | – |
| Line terminators | LF, CR, CRLF, U+2028, U+2029 | – |
| Comments | `// ...` and `/* ... */` (a block comment containing a line terminator counts as one for ASI); a hashbang comment `#! ...` at the very start of the source (byte 0, before any whitespace), running to the first line terminator, which still counts for ASI and line numbers | unterminated `/*` (truncated input); `#!` anywhere else (`UnknownToken`); HTML-like `<!--` / `-->` comments, which are not comments here: they lex as operators (`a <!-- b` is `a < !--b`) |
| Identifiers | Unicode: a start character (`ID_Start`, `$`, `_`) then part characters (`ID_Continue`, `$`, ZWNJ U+200C, ZWJ U+200D), from verified Unicode 17.0.0 tables (`src/frontend/unicode_ident.rs`, generated by `scripts/gen-unicode-ident.sh`); any of them may be written `\uHHHH` or `\u{H...}`. A name is its code points after escapes are decoded, compared without normalization: `a\u0062c` is `abc`, while `Ãµ` (U+00C3 U+00B5) and `õ` (U+00F5) are different names | a character that cannot start a token where one starts (`€`, a combining mark: `UnknownToken`); an escape that is malformed, is not `\u`, or names a code point invalid at its place (`\u0030x`, `a\u0020b`, a surrogate: `InvalidToken`); an identifier start character or `\` right after a number literal (`InvalidToken`) |
| Reserved words | all ECMAScript reserved words plus the strict-mode ones (`let`, `static`, `yield`, `implements`, `interface`, `package`, `private`, `protected`, `public`) and `await`; any of these may follow `.` or be an object literal key as a property name, also written with escapes (`a.l\u0065t` is the property `let`, so is the key in `{l\u0065t: 1}`) | as binding names, assignment targets or references; a reserved word written with escapes (`l\u0065t`, `tru\u0065`) is never the keyword and is rejected (`ReservedWord`) everywhere except right after `.` and as an object literal key (not a shorthand) |
| Numbers | decimal integers, fractions (`1.5`, `.5`, `5.`), exponents (`1e-3`), `0x`/`0o`/`0b` integers; values are IEEE-754 doubles, correctly rounded; overflow gives `Infinity`; a member access may follow a literal (`0.1.a`, `0x1.a`, `.5.a`, `1..a`: the second `.` starts the access); in sloppy code only (clj-quickjs-51; ECMAScript B.1.1), a `0` followed by digits: all octal digits make a legacy octal integer (`017` is 15), which takes no fraction or exponent, so `01.a` is a member access; an `8` or `9` among them makes a decimal literal that may take a fraction and exponent (`08` is 8, `09.5`, `08e1`) | in strict code, legacy octal / leading zeros (`017`, `08`) at the `0`, also right after a `"use strict"` directive (`"use strict"; 010`); in sloppy code, a legacy octal integer followed by a fraction (`01.5` is `01` then `.5`, `UnexpectedToken`) or an exponent (`01e1`), and a `n` suffix or separator after a leading `0` (`01n`, `08n`, `0_1`, `08_1`: `InvalidToken`); numeric separators (`1_000`), a literal immediately followed by an identifier or digit (`3in`, `1.toString`, `0.a`), a number literal where a property name is expected (`0.1.2`), missing digits (`0x`, `1e`) |
| Strings | `'...'` and `"..."` with escapes `\n \t \r \b \f \v \0 \xHH \uHHHH \u{H...}`, each `\u` escape is its UTF-16 code unit(s), so two surrogate escapes spell a pair (`"\u{dbff}\u{dfff}"` is U+10FFFF) and a lone surrogate escape (`\u{d800}`, a high surrogate before a non-surrogate) is a lone code unit (clj-quickjs-141), line continuations, identity escapes (`\'`, `\\`, `\q`), raw U+2028/U+2029; in sloppy code only (clj-quickjs-258; ECMAScript B.1.2), legacy octal escapes of at most three octal digits and value 0o377 (`"\07"` is U+0007, `"\101"` is `A`, `"\400"` is U+0020 then `0`; `\0` not followed by a digit is the null character in either mode) and `\8` `\9` (the characters `8` and `9`) | unterminated strings, raw LF/CR inside a string, malformed `\x`/`\u`; in strict code, octal escapes (`\07`, `\1`, `\08`) and `\8` `\9` at the `\`, also in a directive before a `"use strict"` directive (`"\07"; "use strict";`) and in the string right after one (`"use strict"; "\07"`) |
| Template literals (clj-quickjs-57) | `` `...` `` with substitutions `${ Expression }`, nested to any depth within the nesting budget; one token per part (NoSubstitutionTemplate, TemplateHead, TemplateMiddle, TemplateTail): the `}` that closes a substitution (the first `}` not matching a `{` inside it) starts the next part. The text may contain raw line terminators (CR and CRLF read as LF, U+2028/U+2029 kept; they count for line numbers but not for ASI) and the escapes of string literals, except that a digit escape other than `\0` not followed by a digit is invalid in an untagged template in either mode (tagged parts instead have undefined cooked text); `` \` `` and `\$` are text. Each part keeps its cooked value (escapes decoded) and its raw value (escapes as written, CR and CRLF read as LF; for tagged templates, clj-quickjs-58) | unterminated (`UnexpectedEnd` at the backtick, or at the `}` of the open part); in untagged templates only: malformed `\x`/`\u` escapes, octal escapes (`\1`, `\07`, `\00`) and `\8` `\9` (`InvalidToken` at the `\`, also in sloppy code); a substitution that is not one Expression followed by `}` (`` `${}` ``, `` `${a b}` ``: `UnexpectedToken`) |
| Regular expressions (clj-quickjs-55) | `/pattern/flags` where an operand is expected, including `/=/`; a slash inside `[...]` or escaped with `\` is pattern text. Escapes are preserved in the AST. Flags are IdentifierPart characters; `dgimsuyv` are supported spellings | a line terminator, even escaped (`InvalidToken`), or unterminated body/class/escape (`UnexpectedEnd`), at the opening `/`; duplicate/unknown/escaped flags and combining `u` with `v` (`Invalid` at the `/`). Pattern early errors remain clj-quickjs-56; literals execute with d/g/m/s/u/y and iu; plain i and v are rejected at runtime |
| Punctuators | every ECMAScript punctuator is lexed by maximal munch (`a>>>=b` is `a`, `>>>=`, `b`) so the parser can name an unsupported operator precisely | other characters (`@`, `#`, ...) |

## Syntactic grammar

EBNF; `{ x }` is repetition, `[ x ]` is optional. Terminals are quoted.

```
Script              = { Directive } { StatementListItem } EOF
Directive           = String ";"     (* a whole ExpressionStatement; see "Directive prologues and strictness" *)
StatementListItem   = FunctionDeclaration | ClassDeclaration | LexicalDeclaration | Statement
ClassDeclaration    = "class" BindingIdentifier ClassBody
ClassExpression     = "class" [ BindingIdentifier ] ClassBody
ClassBody           = "{" { ";" | [ "static" ] ClassMethod } "}"
ClassMethod         = PropertyName FormalParameters FunctionBody
                    | ( "get" | "set" ) PropertyName FormalParameters FunctionBody
                    | [ "async" [no LineTerminator here] ] [ "*" ] PropertyName FormalParameters FunctionBody
Statement           = Block | ";" | IfStatement | WhileStatement | DoWhileStatement
                    | ForStatement | ForInStatement | ForOfStatement | SwitchStatement
                    | VariableStatement
                    | ReturnStatement | ThrowStatement | TryStatement
                    | BreakStatement | ContinueStatement | LabelledStatement
                    | ExpressionStatement
Block               = "{" { StatementListItem } "}"
IfStatement         = "if" "(" Expression ")" Statement [ "else" Statement ]
WhileStatement      = "while" "(" Expression ")" Statement
DoWhileStatement    = "do" Statement "while" "(" Expression ")" [ ";" ]   (* ";" always optional *)
ForStatement        = "for" "(" [ ForInit ] ";" [ Expression ] ";" [ Expression ] ")" Statement
ForInit             = Expression[~In] | ( "let" | "const" | "var" ) Declarator[~In] { "," Declarator[~In] }
                      (* "var": clj-quickjs-89 *)
                      (* [~In]: a top-level `in` ends it, ECMAScript's [In] parameter *)
ForInStatement      = "for" "(" ForInHead "in" Expression ")" Statement   (* clj-quickjs-85 *)
ForOfStatement      = "for" "(" ForInHead "of" AssignmentExpression ")" Statement (* clj-quickjs-237 *)
ForInHead           = LeftHandSideExpression   (* an identifier, possibly parenthesized *)
                    | ( "let" | "const" | "var" ) BindingIdentifier
SwitchStatement     = "switch" "(" Expression ")" "{" { CaseClause } "}"   (* at most one "default" *)
CaseClause          = ( "case" Expression | "default" ) ":" { StatementListItem }
ReturnStatement     = "return" [no LineTerminator here] [ Expression ] ";"
ThrowStatement      = "throw" [no LineTerminator here] Expression ";"
BreakStatement      = "break" [no LineTerminator here] [ Identifier ] ";"
ContinueStatement   = "continue" [no LineTerminator here] [ Identifier ] ";"
LabelledStatement   = Identifier ":" Statement   (* the Statement is not a declaration *)
TryStatement        = "try" Block ( Catch [ Finally ] | Finally )
Catch               = "catch" [ "(" BindingPattern ")" ] Block
Finally             = "finally" Block
ExpressionStatement = Expression ";"     (* not starting with "{" or "function" *)
LexicalDeclaration  = ( "let" | "const" ) Declarator { "," Declarator } ";"
VariableStatement   = "var" Declarator { "," Declarator } ";"
Declarator          = BindingPattern [ "=" AssignmentExpression ] (* required for array/object patterns *)
BindingPattern      = BindingIdentifier | ArrayBindingPattern | ObjectBindingPattern
ArrayBindingPattern = "[" { "," | BindingPattern [ "=" AssignmentExpression ] "," }
                      [ BindingPattern [ "=" AssignmentExpression ] | "..." BindingPattern ] "]"
ObjectBindingPattern = "{" ( [ BindingProperty { "," BindingProperty } [ "," ] ]
                       | { BindingProperty "," } "..." BindingIdentifier ) "}"
BindingProperty     = Identifier [ "=" AssignmentExpression ]
                    | PatternPropertyName ":" BindingPattern [ "=" AssignmentExpression ]
PatternPropertyName = PropertyName
FunctionDeclaration = [ "async" [no LineTerminator here] ] "function" [ "*" ] BindingIdentifier FunctionRest
FunctionExpression  = [ "async" [no LineTerminator here] ] "function" [ "*" ] [ BindingIdentifier ] FunctionRest
FunctionRest        = FormalParameters FunctionBody
FormalParameters    = "(" [ FormalParameter { "," FormalParameter } [ "," ] ] ")"
FormalParameter     = BindingPattern [ "=" AssignmentExpression ]   (* default value *)
FunctionBody        = "{" { Directive } { StatementListItem } "}"

Expression            = AssignmentExpression { "," AssignmentExpression }
AssignmentExpression  = ConditionalExpression | ArrowFunction | YieldExpression
                      | Identifier AssignOperator AssignmentExpression
                      | Call "." IdentifierName "=" AssignmentExpression
                      | Call "[" Expression "]" "=" AssignmentExpression
                      | ArrayAssignmentPattern "=" AssignmentExpression
                      | ObjectAssignmentPattern "=" AssignmentExpression
ArrayAssignmentPattern = "[" { "," | AssignmentElement "," }
                         [ AssignmentElement | "..." AssignmentTarget ] "]"
AssignmentElement     = AssignmentTarget [ "=" AssignmentExpression ]
AssignmentTarget      = Identifier | Call "." IdentifierName | Call "[" Expression "]"
                      | ArrayAssignmentPattern | ObjectAssignmentPattern
ObjectAssignmentPattern = "{" ( [ AssignmentProperty { "," AssignmentProperty } [ "," ] ]
                          | { AssignmentProperty "," } "..." LeftHandSideExpression ) "}"
AssignmentProperty    = Identifier [ "=" AssignmentExpression ]
                      | PatternPropertyName ":" AssignmentElement
AssignOperator        = "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "**="
                      | "&=" | "|=" | "^=" | "<<=" | ">>=" | ">>>="
ArrowFunction         = [ "async" [no LineTerminator here] ] ArrowParameters [no LineTerminator here] "=>" ConciseBody
YieldExpression       = "yield" [ [no LineTerminator here] [ "*" ] AssignmentExpression ]
                        (* generator bodies only; "*" requires an operand *)
ArrowParameters       = BindingIdentifier | FormalParameters
ConciseBody           = FunctionBody | AssignmentExpression
ConditionalExpression = LogicalOr [ "?" AssignmentExpression ":" AssignmentExpression ]
LogicalOr             = LogicalAnd { "||" LogicalAnd }
LogicalAnd            = BitwiseOr { "&&" BitwiseOr }
BitwiseOr             = BitwiseXor { "|" BitwiseXor }
BitwiseXor            = BitwiseAnd { "^" BitwiseAnd }
BitwiseAnd            = Equality { "&" Equality }
Equality              = Relational { ( "==" | "!=" | "===" | "!==" ) Relational }
Relational            = Shift { ( "<" | ">" | "<=" | ">=" | "instanceof" | "in" ) Shift }
Shift                 = Additive { ( "<<" | ">>" | ">>>" ) Additive }
Additive              = Multiplicative { ( "+" | "-" ) Multiplicative }
Multiplicative        = Exponentiation { ( "*" | "/" | "%" ) Exponentiation }
Exponentiation        = Unary | Update "**" Exponentiation
Unary                 = ( "!" | "-" | "+" | "~" | "typeof" | "void" | "delete" ) Unary | Update
                      | "await" Unary   (* async bodies only *)
Update                = ( "++" | "--" ) Unary
                      | Call [ [no LineTerminator here] ( "++" | "--" ) ]
Call                  = ( Primary | New ) { Arguments | "." IdentifierName | "[" Expression "]" | TemplateLiteral | OptionalLink }
OptionalLink          = "?." ( IdentifierName | "[" Expression "]" | Arguments )
                        (* no TemplateLiteral after an OptionalLink in the same chain;
                           parentheses end the chain; optional chains are not assignment targets *)
New                   = "new" ( Primary | New ) { "." IdentifierName | "[" Expression "]" | TemplateLiteral } [ Arguments ]
                        (* the first Arguments belong to `new`: `new F()()` calls the result *)
Arguments             = "(" [ SpreadElement { "," SpreadElement } [ "," ] ] ")"
SpreadElement         = [ "..." ] AssignmentExpression
Primary               = Identifier | Literal | FunctionExpression | ClassExpression | ObjectLiteral
                      | ArrayLiteral | TemplateLiteral | RegExpLiteral | "this" | "(" Expression ")"
                        (* `this`: script global object, ordinary receivers, lexical arrow captures *)
TemplateLiteral       = NoSubstitutionTemplate
                      | TemplateHead Expression { TemplateMiddle Expression } TemplateTail
                        (* after a member/call expression, including across a line break,
                           this is a tagged call; invalid escapes have undefined cooked text *)
ObjectLiteral         = "{" [ PropertyDefinition { "," PropertyDefinition } [ "," ] ] "}"
PropertyDefinition    = PropertyName ":" AssignmentExpression | Identifier
                      | PropertyName FormalParameters FunctionBody
                      | [ "async" [no LineTerminator here] ] "*" PropertyName FormalParameters FunctionBody
                      | "async" [no LineTerminator here] PropertyName FormalParameters FunctionBody
                      | "get" PropertyName "(" ")" FunctionBody
                      | "set" PropertyName "(" BindingPattern [ "=" AssignmentExpression ] ")" FunctionBody
PropertyName          = IdentifierName | String | Number | "[" AssignmentExpression "]"
ArrayLiteral          = "[" { [ SpreadElement ] "," } [ SpreadElement ] "]"
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
- `++`/`--` (clj-quickjs-64) take an identifier binding or a property
  `o.p` / `o[k]` (clj-quickjs-76), possibly parenthesized (`(a)++`,
  `(o.p)++`); any other operand is an early error (`Invalid`:
  `1++`, `(a + b)++`, `f()++`, `++a++`, `++-a`). `undefined`, `NaN` and
  `Infinity` are `ReservedWord`, as for assignment, and so is an intrinsic
  of `__rt` (`__rt.count++`). A postfix update ends a member/call chain:
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
  or script, wherever the statement is (see "Execution"), as are those of a
  `var` head of a `for` or for-in statement (clj-quickjs-89).
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
  returns. `async` arrows execute through promise jobs (clj-quickjs-248); `async => async` is an arrow
  with a parameter named `async`.
- Generator and async syntax (clj-quickjs-54): `function*`, `async function`,
  `async function*`, `async x => ...`, `async (x) => ...`, and object methods
  `*m() {}`, `async m() {}`, `async *m() {}` parse to function ASTs recording
  generator/async flags. `async` is an unescaped contextual keyword and has no
  line terminator before `function`, the arrow parameters or a method name.
  `yield`, `yield value` and `yield* value` have assignment precedence in
  generator bodies; a line terminator after `yield` ends its operand, so
  `yield\n1` is two statements. `await value` has unary precedence in async
  bodies. Neither operator may occur in that function's parameter defaults.
  Nested ordinary functions and non-async arrows start fresh operator contexts;
  `await` in a non-async arrow nested in async code is rejected. Both words
  remain reserved as identifiers, including in sloppy code, as before.
  Yield operands and concise async arrow bodies preserve `[~In]` in a for
  initializer; function bodies and parameter defaults clear it as usual.
  Generator calls create task-owned generator objects without running the body
  (clj-quickjs-243). Parameter defaults run during the call. `next(value)` runs
  until `yield` or completion; its argument supplies the previous yield's result
  and is ignored at the start. Each result has ordinary `value` and `done`
  properties. Completed generators always return `{value: undefined, done: true}`.
  Parenthesized yield expressions can be binary operands, such as
  `a + b * (yield c)`. Yields in try/catch/finally preserve live operands,
  handlers and pending finally completions across suspension (clj-quickjs-244),
  even when subsequent next calls come from different depths. A pending
  return or throw continues after a yielding finally block finishes; a wait
  in its resumed body parks the whole task without unwinding.
  Uncaught exceptions complete the generator and propagate to the next caller;
  executing generators reject re-entrant next with TypeError.
  Generator functions create their own prototype/property record eagerly, so
  publication rejects that task-owned record while preserving local callability.
  The inherited `Symbol.toStringTag` is `Generator`; general tag consumption by
  `Object.prototype.toString` remains explicitly unsupported.
  Delegated `yield* iterable` (clj-quickjs-245) acquires the iterator and caches
  its next method once, forwards each received next argument (initially
  undefined), and returns each non-done iterator result object unchanged.
  When done becomes truthy, its value becomes the yield* expression result.
  Protocol getters/calls can wait, be collected around and migrate like any
  other VM call; non-iterables and non-object next results throw TypeError.
  Async functions and await execute through promise jobs (clj-quickjs-248);
  async generators still fail compilation at the function's first token.
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
  `arguments` is an IdentifierReference in both modes (clj-quickjs-97);
  strict code cannot assign, increment or decrement that identifier.
- The `Unary` production's `delete IdentifierReference`, including a
  parenthesized reference and `undefined`/`NaN`/`Infinity`, is allowed only
  in sloppy code (clj-quickjs-105); strict code rejects it at `delete`.
- The assignment target may be parenthesized (`(a) = 1`, `(a) += 1`), as
  in JavaScript. A compound assignment (clj-quickjs-84) takes identifier
  and property targets (clj-quickjs-260), and is
  right-associative too: `a -= b *= 2` is `a -= (b *= 2)`.
- `a.b` (an `IdentifierName` after `.`, reserved words included) reads a
  named property, and `a.b = v` (the target may be parenthesized) writes
  one (clj-quickjs-114), as do `a.b op= v` (clj-quickjs-260) and
  `a.b++` (clj-quickjs-76); `__rt.name(...)` is
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
  consume the synchronous iteration protocol (clj-quickjs-130). On the left of
  `=`, an array literal becomes an assignment pattern (clj-quickjs-131),
  with identifier or property targets, defaults, nesting and a final rest element.
- `a[k]` (clj-quickjs-115; parsed since clj-quickjs-118) reads a
  computed property, and `a[k] = v`
  (the target may be parenthesized) writes one; the key is an Expression
  (`a[b, c]` is `a[c]` after evaluating `b`). It joins a member chain like
  `.` (`a.b[c].d`, `a[0][1]`), with no line-terminator restriction: `a`
  newline `[0]` is one member expression, as in JavaScript. A call of a
  computed property, `a[k](...)`, is a method call like `a.m(...)`
  (clj-quickjs-117: the base is evaluated once and passed as the
  receiver), `++`/`--` of a computed property is supported as for `a.b`
  (clj-quickjs-76), and so is compound assignment (`a[k] += v`,
  clj-quickjs-260).
- `const` requires an initializer.
- `ForStatement` (clj-quickjs-81): a `let`/`const` head declares bindings
  scoped to the whole loop (a `const` needs an initializer here too); a
  `var` head (clj-quickjs-89) declares bindings of the enclosing function
  or script. An initializer in a for-in `var` head
  (`for (var k = 1 in o)`) is `Invalid` as for `let`/`const`; the
  non-strict `var` form (Annex B) is clj-quickjs-90.
- `ForInStatement` (clj-quickjs-85): the head's first element (an
  expression, or the initializers of a declaration list) is parsed with
  `[~In]` (clj-quickjs-74), so a top-level `in` after it starts a for-in
  head; inside parentheses, brackets, braces, call arguments, template
  substitutions, function parameters and bodies or the middle operand of
  `? :` it is the `in` operator (`for (x = (a in b); ;)`), as in the test
  and the update; an arrow's expression body does not take it. The left
  side is an identifier,
  possibly parenthesized (`for ((k) in o)`), or one `let`/`const` binding
  without an initializer. Early errors (`Invalid`): an initializer
  (`for (let k = 1 in o)`, at the `in`), more than one binding
  (`for (let a, b in o)`), and any other target (`for (1 in o)`,
  `for (f() in o)`). A property target (`for (a.b in o)`, `for (a[0] in
  o)`) is `Unsupported` until clj-quickjs-87. The object is an Expression,
  where `in` is the operator again.
- `ForOfStatement` (clj-quickjs-237): identifier targets and a single
  `let`/`const`/`var` binding without an initializer. The right side is an
  AssignmentExpression: `for (x of a, b)` is a syntax error, while
  `for (x of (a, b))` is accepted. `of` is contextual (`for (let of of a)`
  declares `of`); `for (let of a)` and `for (async of a)` are syntax errors.
  Parenthesized `(async)` is an ordinary assignment target. Property and
  destructuring heads remain unsupported; using declarations are clj-quickjs-238.

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

Yield expressions, including delegated `yield*`, inside generator bodies are executable,
including binary operands and try/catch/finally bodies (clj-quickjs-244).
Async functions and await execute (clj-quickjs-248); async generators remain rejected.
`new` on a generator, async function, concise method, accessor or arrow is valid
syntax and throws a catchable TypeError at execution (clj-quickjs-135).

`Symbol()`, `Symbol.for(key)` and `Symbol.keyFor(sym)` use ordinary call syntax.
`new Symbol()` is accepted syntax
but fails at runtime with TypeError. Symbol-valued computed literal keys execute.

| Form | Kind |
|------|------|
| a property as a for-in/for-of target (`for (a.b in o)`, `for (a.b of o)`, clj-quickjs-87), `debugger`, `with` | Unsupported |
| class inheritance (`extends`), fields, static initialization blocks, `new.target` (clj-quickjs-99), `super`, `import`/`export`, dynamic `import` | Unsupported |
| execution of parsed async generators | Unsupported at the function during compilation |
| rest parameters, destructuring for-in/for-of heads | Unsupported |
| object spread `...` (in an object literal) | Unsupported |
| an optional chain as an assignment/update target (`a?.b = 1`, `a?.b++`), a `new` callee (`new a?.b()`), or before a tagged template in the same chain | Invalid |
| in object literals: computed shorthand `{[k]}` | UnexpectedToken |
| numeric separators | Unsupported |
| `??`, logical assignment (`&&=`, `\|\|=`, `??=`, also to a property: `a.b &&= 1`) | Unsupported |
| `delete super.x` (clj-quickjs-138) waits for `super` | Unsupported |
| `delete` of an identifier reference in strict code (`delete x`, ECMAScript 13.5.1.1), reported at the `delete` | Invalid |
| a reserved word as a shorthand property (`{if}`) | ReservedWord |
| a string or numeric key as a shorthand property (`{1}`) | UnexpectedToken |
| a shorthand initializer outside a destructuring pattern (`({a = 1})`) | Invalid |
| a second `__proto__: v` (or `"__proto__": v`) in one object literal (clj-quickjs-121; a shorthand `{__proto__}` does not count) | Invalid |
| direct `eval(source)` in parameter expressions (compiler check; clj-quickjs-109) | Unsupported |
| reserved word as a binding name or reference; binding, assigning (also with a compound operator: `undefined += 1`), incrementing or decrementing `undefined`/`NaN`/`Infinity`; binding or assigning/updating `eval`/`arguments` in strict code, including a function name or parameter checked again because the function's body starts with a `"use strict"` directive | ReservedWord |
| computed access to the reserved intrinsic binding (`__rt[x]`), assignment to one of its intrinsics (`__rt.count = f`), `++`/`--` on one (`__rt.count++`) or `delete` of one (`delete __rt.count`) | ReservedWord |
| an arrow function as an operand, callee or member object without parentheses (`1 + x => x`, `() => {}()`), a line break before `=>` | UnexpectedToken |
| `return` outside a function, `break` outside a loop or `switch` of the same function, `continue` outside a loop of the same function, a second `default` in a `switch`, `const` without initializer, duplicate parameter (arrow functions included), declaration as an `if`/`while`/`do`/`for` or labelled body, a `break`/`continue` label that no enclosing statement of the same function has, `continue` to a label of a statement that is not a loop, a label inside a statement with the same label, `const` without initializer in a `for` head, an initializer or more than one binding in a for-in head (`for (let k = 1 in o)`, `for (let a, b in o)`, `for (var k = 1 in o)` in strict code), a for-in target that is not a reference (`for (1 in o)`), invalid assignment target (`1 = 2`, `1 += 2`, `f() += 1`), an operand of `++`/`--` that is not an identifier binding or a property `o.p` / `o[k]` (`1++`, `f()++`, `++a++`), a line break between `throw` and its expression, a `let`/`const`/function declaration in a catch body that repeats the catch parameter, a prefix-operator expression as the base of `**` (`-a ** b`) | Invalid |
| nesting over the `MAX_NESTING` budget (256 units), or a syntax tree more than 256 nodes high; see "Nesting limit" below | TooDeep |

## Explicit gaps

Derived classes, `super` and class fields remain deferred (clj-quickjs-137/138/139).
Constructor rejection messages use the source spelling of an identifier or
member callee (`G`, `o.m`, `o[k]`), independently of its mutable `name` property.
Other callees use the value kind (`function` for an anonymous arrow), or the
number itself. This keeps the pellet's source-label rule; QuickJS instead reads
the rejected function's `name` property (`JS_ThrowTypeErrorNotAConstructor`).

Optional chains support member/index access, calls (including spread arguments)
and delete (clj-quickjs-134). They use the same object, call and delete behavior
and limits as ordinary chains. `super` remains unsupported, including `super?.x`.

Number `toFixed`, `toExponential` and `toPrecision` are supported
(clj-quickjs-150/-151). Digit-count strings longer than 4096 UTF-16 units
remain an explicit RangeError resource limit; budgeted generic numeric
conversion remains deferred (clj-quickjs-265).

WeakRef instances cannot be published across tasks (clj-quickjs-176).
The opt-in host timers and collection points are described under simulated
host facilities (clj-quickjs-175).

Generator `return(value)` and `throw(error)` are deferred.
`return` and `throw` methods are absent from the generator prototype, so yield*
does not yet forward abrupt resumes to its delegate (clj-quickjs-245). Async
generators remain unsupported. Suspended generators cannot publish
or cross tasks, including through immutable collections or shared references.
Generator functions also cannot publish because their own property record is
task-owned, as for an ordinary function whose prototype has been materialized.
Yield expressions with live operands and yields inside try/catch/finally are
executable; they do not require any of those deferred methods (clj-quickjs-244).

`Intl` provides locale identifiers and likely-subtag maximize/minimize
(clj-quickjs-209/-210/-211), plus decimal/percent/currency `NumberFormat` with bound `format`,
`resolvedOptions` and `supportedLocalesOf` over the policy's test locales
(clj-quickjs-213). Other formatting service constructors and their
`supportedLocalesOf`, Locale information methods, and NumberFormat's
unit style and parts/range methods remain deferred.

Known differences from full JavaScript within the accepted forms, and
checks left to later stages:

- **Synchronous iteration** (clj-quickjs-237): plain for-of, array/typed-array
  `values` and string iteration execute. Async iteration, `for (using ...)`,
  destructuring/property heads and other iterable consumers remain
  separate features. Iterator prototypes expose their `Symbol.toStringTag`;
  generic `Object.prototype.toString` symbol-tag dispatch remains deferred.

- **Rope flattening** (clj-quickjs-146): the first contiguous read, comparison
  or hash flattens synchronously in O(length). Traversal and destruction use
  explicit stacks, so deep ropes cannot overflow the native stack, but this
  first read can delay a budget yield or collection. The cached buffer and
  original rope graph remain allocated until the string is released.

- **String split** (clj-quickjs-157): string and undefined separators,
  ToUint32 limits, generic receivers and resumable object coercion execute.
  RegExp separators and `Symbol.split` dispatch remain deferred to the RegExp
  pellets; RegExp separators fail explicitly, and other object separators
  currently undergo ToString. Long scans and
  result copying are budgeted.
- **Regular expressions** (clj-quickjs-55): literals produce positioned
  `RegExp { pattern, flags }` AST nodes. The parser requests the regexp
  lexical goal at operand positions; `a` newline `/ b / g` remains division.
  Pattern early errors remain clj-quickjs-56. RegExp objects and methods
  execute with bounded matching, including u mode (clj-quickjs-188/-190).
  String replacement and iu folding execute (clj-quickjs-189/-191).
  Property escapes and match indices execute (clj-quickjs-192/-193);
  Unicode sets remain deferred (clj-quickjs-194).
- **JSON.parse** (clj-quickjs-159) recognizes the JSON grammar with
  resumable ToString and budgeted parsing. Open-container depth is capped
  at 512 and decoded member names at 4096 UTF-16 units, with catchable
  SyntaxErrors; callable revivers fail explicitly with TypeError.
  SyntaxError stacks start with `<input>:line:column`, followed by the
  JavaScript caller frames (clj-quickjs-162). Reviver traversal remains
  deferred. `JSON.stringify` (clj-quickjs-160/-161)
  serializes primitives, wrappers, arrays and objects with resumable getters/
  `toJSON`, cycle checks, bounded quoting/traversal and the indentation space
  argument. Replacer functions/lists fail explicitly; see "JSON.stringify"
  below for resource limits.
- **Error instances** (clj-quickjs-228/-229): `Error` and all seven NativeError
  constructors are callable and constructible,
  with resumable message conversion, inherited/accessor `options.cause`, and
  generic `Error.prototype.toString` and captured `stack` strings
  (clj-quickjs-59). Engine-raised errors use these ordinary instances too
  (clj-quickjs-230), preserving their original diagnostics internally.
- **Number prefix parsing** (clj-quickjs-148): global `parseInt` and
  `parseFloat`, and their identical `Number` aliases, execute with resumable
  object coercion and budgeted scans. Their global bindings are writable
  task-owned properties (clj-quickjs-104).
- **String extraction and combination** (clj-quickjs-145): `substring`,
  `slice`, `concat`, `padStart` and `padEnd` are supported, including generic
  receivers, resumable object coercion and budgeted result copying. Ropes and
  yielding during bulk primitive coercion remain deferred (-146, -265).
- **String methods remain partial** (clj-quickjs-143). Code-unit access
  and construction execute, including resumable object coercion. Other
  string methods remain deferred. Bulk primitive-string conversion can
  delay yielding and collection (clj-quickjs-265). See "String code-unit access" below.
- **Substring search** (clj-quickjs-144): `indexOf` and `lastIndexOf` now
  execute with UTF-16 indices, position clamping and resumable object
  coercion; long scans are budgeted. See "String substring search" below.
- **Scope errors are checked by the compiler, not the parser.**
  `parse_script` accepts redeclaration (`let a; let a;`), a `let` that
  repeats a parameter name and declarations of the reserved binding `__rt`;
  `compile_script` rejects them (see "Execution"). Temporal-dead-zone use
  and assignment to a `const` are run-time errors, as in JavaScript.
- **Global state is task-owned** (clj-quickjs-104). Each task has its own
  global object with writable built-in bindings. Script `var` and function
  declarations create its non-configurable properties at entry; `let` and
  `const` stay lexical. A published closure reads the receiving task's
  globals, while lexical captures keep their usual publication rules.
  Indirect eval shares both the task's global object and its script lexical
  environment, enforcing const/TDZ errors and lexical declaration conflicts.
  All lexical conflicts are checked before property-admissibility errors.
  Later script
  admission into an existing global environment is not implemented. Block function
  declarations stay lexical (sloppy Annex B hoisting is not implemented).
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
  even in a debug build, for parsing and for compiling (tested for every
  nesting form at every accepted depth; clj-quickjs-262 made the
  compiler's per-level expression frames small enough that right-nested
  assignment chains such as `a = a = ... 1` no longer overflow it before
  the limit). Separately, the syntax tree itself may be
  at most 256 nodes high (statements, functions and expressions on any
  root-to-leaf path, counted by each node's `height`), which also catches
  finished subtrees wrapped again, such as `(a.b.b).b.b`; later recursive
  passes can rely on that bound.
- **Objects have data and accessor properties** (clj-quickjs-114/-120).
  Literal getters and setters use identifier, string, numeric or computed names
  (clj-quickjs-233).
  Concise methods define data properties
  (clj-quickjs-126); `++`/`--` (clj-quickjs-76), compound
  assignment (clj-quickjs-260) and `delete` (clj-quickjs-124) on
  properties execute.
  An object literal inherits from `Object.prototype` (clj-quickjs-117)
  unless it has a `__proto__: v` (clj-quickjs-121);
  an embedder-made object (`Scope::new_object`) has a null prototype.
  The built-in accessor `Object.prototype.__proto__` is stored and reflected
  with its getter/setter functions (clj-quickjs-261).
  Their attributes (`writable`, `enumerable`, `configurable`) are set
  with `Object.defineProperty` and read with
  `Object.getOwnPropertyDescriptor` (clj-quickjs-119); accessor descriptors
  use `get`/`set` instead of `value`/`writable` (clj-quickjs-120).
  `Object.keys` lists only enumerable properties
  (clj-quickjs-123); `for`-`in` is clj-quickjs-85. `Object.getOwnPropertyNames`
  of `Object.prototype` includes `__proto__`; all three enumerations
  exclude symbol keys (clj-quickjs-127).
- **Computed keys use ToPropertyKey** (clj-quickjs-115, with the
  instructions of clj-quickjs-118). A key is converted by ToPropertyKey
  when the access runs, so for a write after the right-hand side has been
  evaluated (as current ECMAScript's PutValue and QuickJS's `put_array_el`
  do; object key conversion may call JavaScript): strings as they are,
  numbers by Number::toString (`o[1]` is `o["1"]`, `o[-0]` is `o["0"]`,
  `o[1.5]` is `o["1.5"]`), booleans, `null` and `undefined` by ToString.
  An object key uses ToPrimitive with hint string (clj-quickjs-70),
  then converts the primitive to a name or preserves a symbol identity.
  An `undefined`/`null` base is
  checked first and throws its catchable `TypeError` (`null[{}]`);
  symbols are preserved as keys (clj-quickjs-127). Number and string bases
  read through `Number.prototype` / `String.prototype` (clj-quickjs-73),
  after a string's own indices below its length (`"abc"[1]` is `"b"`)
  and its `length` (clj-quickjs-142); boolean bases fail with
  `UnsupportedOperand` until clj-quickjs-39.
- **Property names are never forgotten** (clj-quickjs-115). Each new
  name a write uses, by `o.name = v`, `{name: v}` or a computed
  `o[k] = v` (also one that then fails), is interned in the runtime-wide
  property-key table for the life of the runtime (reads and array
  indices on arrays intern nothing). With computed keys the table grows with the number of
  distinct names a program writes (`o["k" + i] = i` in a loop adds one
  name per iteration), not with live objects; the collector reclaiming
  unused names is clj-quickjs-259 (`docs/architecture.md`, "Ordinary
  objects", Consequences).
- **Array method gaps** (clj-quickjs-118, -154, -155, -156).
  An array literal is an array object with elements, holes and an
  automatic `length`, inheriting from `Array.prototype`, a realm
  intrinsic that inherits from `Object.prototype` and supports `push`,
  `join` and `toString`. Assigning `a.length` supports truncation and extension
  (clj-quickjs-154). `Array` is callable and constructible, exposes
  `Array.prototype`, and supports `Array.isArray` (clj-quickjs-153).
  Other array methods remain deferred. `push` on a
  primitive receiver needs wrapper objects (clj-quickjs-72), and an
  object-valued array-like `length` needs resumable ToPrimitive
  (clj-quickjs-70); both fail explicitly with `UnsupportedOperand`.
  A persistent vector (`__rt.vector(...)`) is
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
  built-in prototypes, nor change their prototypes
  (`Object.setPrototypeOf(Object.prototype, {})` is a `TypeError`,
  clj-quickjs-121). An ordinary object may still shadow an inherited
  method with its own property (`o.toString = f`).
  `Object.preventExtensions` and `Object.freeze` on a built-in object
  throw the same `TypeError` (clj-quickjs-122), while
  `Object.isExtensible` reports its `[[Extensible]]` flag (`true`) and
  `Object.isFrozen` `false`, as its descriptors do.
- **Freezing does not change ownership** (clj-quickjs-122,
  `docs/architecture.md`, "Extensibility and freezing"). A frozen object
  or array is still owned by the task that made it: other tasks cannot
  read it and it cannot be published (`Runtime::publish`), even when
  frozen deeply. Freezing a function (or making it non-extensible) gives
  it a task-owned property record, so it can no longer be published,
  like any property write on it; a published function is already
  immutable and reports itself non-extensible and frozen. Vectors, maps
  and the runtime's other values without JavaScript properties are
  frozen. `Object.seal`/`isSealed` and `Reflect.preventExtensions` are
  not provided. Freezing an accessor makes it non-configurable and keeps
  its getter and setter; the setter may still run (clj-quickjs-120).
- **Sloppy receiver substitution remains deferred.** Script top-level
  `this` is the task's global object, and arrows capture it by value.
  In sloppy code `this` of a call without an object receiver (a plain
  call `f()`, which would see the global object, or a primitive one,
  which would be wrapped, clj-quickjs-72, also through `call`/`apply`)
  fails with `UnsupportedOperand` when evaluated instead of giving a
  wrong value.
- **`Function.prototype` is partial** (clj-quickjs-102): it has
  `length`, `name`, `constructor` (the `Function` stub, whose call fails
  with `UnsupportedOperand` until clj-quickjs-111), `call` and `apply`.
  Its other standard methods (`bind`, clj-quickjs-103; `toString`) and
  `arguments`/`caller` are missing: a lookup that reaches
  `Function.prototype` without finding one (`Object.bind`,
  `Object.is.toString`) fails with `UnsupportedOperand` instead of
  answering from `Object.prototype`. A function's prototype cannot be
  changed (`Object.setPrototypeOf(f, p)`, `f.__proto__ = p` with another
  `p`: `UnsupportedOperand`). `Reflect` has only `apply`.
  `Object(v)` of a number, string or boolean and
  `Object.prototype.toString`/`valueOf` of such a receiver need wrapper
  objects (clj-quickjs-72) and fail the same way. Only `Number` and `String` are constructible built-ins (clj-quickjs-72);
  `new Object()` remains a catchable `TypeError`.
- **Objects are task-owned.** An object or array belongs to the task that
  created it: only that task reads or writes its properties (another task
  fails with `ForeignObject`), and publishing a value that reaches one to
  another task is rejected (`Runtime::publish`), with a path that names
  the property or element index leading to the innermost one.
- **String methods remain incomplete** (the string-methods pellets).
  String primitives and wrappers expose code-unit `length` and indices
  (clj-quickjs-142). The code-unit
  methods and `String.fromCharCode`/`fromCodePoint` execute (clj-quickjs-143).
- **Identifier tables are pinned Unicode 17.0.0**, independently verified
  alongside folding by `scripts/quickjs-data/`; later additions require a
  coordinated data upgrade.
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
  named for it: sloppy direct-eval var/function hoisting (-108) remains
  rejected during runtime compilation (see "Direct eval"); silently
  ignored stores to a function expression's own name (clj-quickjs-96);
  `with`
  (clj-quickjs-110); `this` substitution (a sloppy function's `this` fails
  explicitly unless its receiver is an object, see above) and the mapped
  `arguments` object (not filed). Strict reserved words (`let`, `static`, `yield`,
  `implements`, `interface`, `package`, `private`, `protected`, `public`)
  stay reserved, duplicate parameter names stay `Invalid`, a function
  declaration as a labelled or `if` body stays `Invalid`, and function
  declarations in blocks stay lexical (no Annex B hoisting).
- **Arguments objects are unmapped** (clj-quickjs-97): strict and sloppy
  functions both copy actual values without aliasing parameter locals.
  `callee` is absent; its restricted accessor is deferred. `Symbol.iterator`
  protocol support remains deferred (its identity is exposed by clj-quickjs-127).
  The object is ordinary,
  without a special arguments brand for `Object.prototype.toString`.
- **Default parameter values lack what the subset lacks elsewhere**
  (clj-quickjs-100): direct `eval` in parameter expressions is rejected
  (-109); indirect eval works.
  An arrow's default sees its lexical
  `this` (clj-quickjs-98); a non-arrow function's default sees its own
  receiver (clj-quickjs-116). Anonymous function defaults infer their
  parameter's name; `length` counts parameters before the first default
  (clj-quickjs-101).

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
  or an array pattern parameter (clj-quickjs-131)
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
  and closed over as usual. References to `eval` resolve to the global
  function unless shadowed; indirect calls compile source at runtime
  (clj-quickjs-106). `arguments` in a `function` (directly or through
  arrows) resolves to that function's implicit object unless shadowed;
  a plain `var arguments;` keeps the object (clj-quickjs-97).
  In a script outside any `function`,
  an unbound `arguments` is an ordinary unresolvable name
  (ReferenceError when read, `"undefined"` for `typeof`, and an implicit
  global on sloppy assignment, clj-quickjs-105).
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

Object conversions (synthetic fixtures, not light-cljs output):

```js accept
({}) == "[object Object]";
({valueOf: function () { return 2; }}) * 3;
let o = {toString: function () { return "x"; }};
`${o}`;
let a = {x: 1};
a[o]++;
o in a;
delete a[o];
```

## Execution

Optional links `a?.b`, `a?.[key]` and `a?.(args)` (clj-quickjs-134) test only
for null and undefined. A nullish link skips the whole remaining member/call
chain, including computed keys, arguments and spread iteration, and produces
undefined. Parentheses end that short circuit: `null?.b.c` is undefined, while
`(null?.b).c` throws TypeError. Member calls retain their receiver, including
`a?.b()`, `a?.[key]()`, `o.m?.()` and `(a?.b)()`. A plain `f?.()` has undefined
as its receiver. Delete produces true when short-circuited; otherwise it deletes
the final property using ordinary strict/sloppy delete semantics. Getters, key
conversion and calls retain the ordinary resumable execution behavior.

```js accept
null?.x;
undefined?.[sideEffect()]?.(argument()).remaining;
let a = {b() { return this; }};
(a?.b)();
a.b?.(...[]);
delete a?.b;
x?.5:1;
```

```js reject Invalid 1:1
a?.b = 1;
```

```js reject Invalid 1:6
new a?.b();
```

`Number.prototype.toExponential(fractionDigits)` and `toPrecision(precision)`
(clj-quickjs-151) accept Number primitives and wrappers. Both round the exact
binary64 value, choosing the larger magnitude on ties, and pad the requested
digits. `toExponential` accepts 0..100 fractional digits and always prints an
exponent with a sign; missing/undefined digits use the shortest round-trip
representation. `toPrecision` accepts 1..100 significant digits, using an
exponent when the rounded exponent is below -6 or at least the precision;
missing/undefined precision returns Number ToString. Negative zero prints
without a minus sign. Receiver validation precedes count conversion, and
nonfinite receivers return ToString after count conversion but before range
validation. Invalid receivers throw TypeError; invalid counts throw RangeError.
Object counts use resumable ToNumber, including getters and callbacks. An
object returning undefined converts to NaN (integer zero), rather than using
the missing-count behavior. Both share toFixed's 4096-unit count-string limit.

`Number.prototype.toFixed(fractionDigits)` (clj-quickjs-150) accepts Number
primitives and wrappers, including `Number.prototype`. It converts the digit
count with ToIntegerOrInfinity (missing/undefined/NaN becomes zero), checks
0..100 before handling nonfinite numbers, and throws catchable RangeError
outside that range. Other receivers throw TypeError before digit coercion.
Object digit counts use resumable ToNumber, including getters and callbacks.
As a resource policy, digit-count strings exceeding 4096 UTF-16 units throw
RangeError before flattening or parsing, including strings returned by callbacks.
Finite magnitudes below 1e21 round the exact binary64 value to the requested
decimal places, choosing the larger magnitude on ties and padding zeros.
Negative values rounded to zero keep `-`; negative zero itself prints without
`-`. Magnitudes at least 1e21 and nonfinite numbers use Number ToString.

`Intl` is a global ordinary object with `Object.prototype` and a non-writable,
non-enumerable, configurable `Symbol.toStringTag` of `Intl` (clj-quickjs-209).
`Object.prototype.toString.call(Intl)` produces `[object Intl]`; both `Intl()`
and `new Intl()` throw catchable TypeErrors. The namespace follows shared
intrinsic mutation restrictions. Decimal/percent/currency `Intl.NumberFormat` and
`Intl.PluralRules` and root `Intl.Collator` are installed;
other formatting service constructors remain deferred.

`Intl.getCanonicalLocales(locales)` canonicalizes a string, Locale, or array-like
list, skipping absent indices and removing duplicates in first-occurrence order.
Non-string primitive inputs have no length and give empty lists in this realm.
`undefined` gives an empty list; `null` and non-string/non-object elements throw
TypeError. Locale objects use their internal tags without calling `toString`.
Other object elements use resumable ToString; length/index getters and ToLength
also run through resumable native frames.

`new Intl.Locale(tag, options)` accepts a string or an object (a Locale's internal
tag, otherwise resumable ToString), with optional language/script/region and
calendar/collation/hourCycle/caseFirst/numeric/numberingSystem overrides. Options
must be an object or `undefined`, matching QuickJS GetOptionsObject. Calling
without `new` throws TypeError. Tags parse language, script, region, variants,
Unicode/other extensions and private use; canonicalization adjusts case, selected
deprecated aliases, variant order, attributes, keywords and singleton order.
Invalid tags/options throw RangeError. The explicit legacy `co-phonebook` alias
is accepted before the usual eight-character type-subtag check. The parser
retains the pinned QuickJS limits of fewer than 512 bytes and 64 subtags.
Locale prototype `toString`, `baseName`, `language`, `script`, `region`, `variants`,
`calendar`, `collation`, `hourCycle`, `caseFirst`, `numeric` and `numberingSystem`
check internal-slot branding, independently of the current prototype. The tag
descriptor is `Symbol.toStringTag = 'Intl.Locale'`. Absent string fields return
`undefined`; absent numeric returns `false`, matching the pinned snapshot. `variants` and
`firstDayOfWeek` options and general `@@toStringTag` consumption remain deferred.

`Intl.Locale.prototype.maximize()` and `minimize()` return a fresh Locale using
the intrinsic Locale prototype (clj-quickjs-211). They read branded internal
slots, preserve variants, Unicode attributes/keywords, other extensions and
private use, then apply constructor base-alias normalization, and throw TypeError
on non-Locale receivers. Maximize fills missing
language/script/region from the pinned likely-subtag table; minimize tries
language alone, language-region, then language-script, retaining the first that
maximizes to the same triple. Unmatched identifiers remain unchanged. These leaf
methods perform no user coercion or property access and cannot be constructed.

String `+`, `+=`, template interpolation and `String.prototype.concat` use
immutable ropes above 32 UTF-16 units (clj-quickjs-146). Repeated append or
prepend takes linear total construction work. Length stays O(1); the first
indexed read flattens once and later indexed reads are O(1). Content equality,
map/property keys and publication retain ordinary string semantics. See the
flattening limitation under "Explicit gaps".

`String.prototype.split(separator, limit)` returns a fresh array of UTF-16
substrings (clj-quickjs-157). Undefined separator returns the whole receiver;
an empty separator emits individual code units, including lone surrogates.
Nonempty separators consume non-overlapping matches from left to right,
preserving empty fields. Undefined limit defaults to `2^32 - 1`; other limits
use ToUint32 (so `-1` wraps), and zero returns an empty array. Coercion runs
in receiver, limit, separator order, including separator conversion for zero
limit. Null/undefined receivers throw TypeError before coercing arguments.
Object conversion callbacks can yield, wait, collect and migrate. Each scan
or copying step does at most 256 units/comparisons; appending emits one
ordinary own array element per step. RegExp/`Symbol.split` dispatch is deferred.

Global `JSON.parse` recognizes JSON into task-owned objects and arrays,
with budgeted parsing and resumable ToString (clj-quickjs-159); see
"JSON.parse" below for grammar, errors and resource limits.

Global `parseInt(string, radix)` and `parseFloat(string)` parse the longest
valid prefix after leading StrWhiteSpace and ToString. `Number.parseInt`
and `Number.parseFloat` are the same function objects (clj-quickjs-148).
`parseInt` converts radix with ToInt32 after string conversion; zero or
undefined detects `0x`/`0X` as radix 16, otherwise 10. Explicit radix 16
also strips hex prefixes; other valid radixes are 2..36. There is no octal
or binary prefix detection. Invalid radix or no digits gives NaN.
`parseFloat` accepts decimal fractions, exponents and signed `Infinity`,
discarding an incomplete exponent (`'1e+'` gives 1); it accepts no radix
prefix. Both preserve negative zero and round to binary64, including large
integers, overflow and underflow. Object coercion can yield, wait, collect
and migrate through OrdinaryToPrimitive, with string hint for the first
argument and number hint for radix. Scans inspect at most 256 units per
native step. Existing bulk primitive ToNumber conversion of a string radix
can still delay yielding (clj-quickjs-265).

String extraction and combination methods (`substring`, `slice`, `concat`,
`padStart`, `padEnd`) operate on UTF-16 units. Object coercion can suspend;
result copying runs in batches of at most 256 units (clj-quickjs-145).

String substring searches `indexOf` and `lastIndexOf` execute with generic
ToString receivers and searches, clamped positions and budgeted UTF-16
scans (clj-quickjs-144; see "String substring search" below).

`frontend::compile_script(name, source)` parses, resolves bindings and lowers
a script to a validated `Executable` (`src/frontend/lower.rs`); a task
running it completes with the script's **completion value**, the value
JavaScript `eval` would return (the last expression statement that ran;
`if`, `while`, `do`-`while`, `for` and `try` produce `undefined` unless
their body produced a value (a loop keeps the value its body produced last, so
`5; do { 6; } while (false)` is 6 and `5; do { } while (false)` is
`undefined`; the head of a `for` never sets it, so
`5; for (let i = 0; i < 0; ) {}` is `undefined`, as is
`5; for (let k in {}) {}`); a `try` statement's value is that of its block or catch body, never
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
| Async functions and `await` (clj-quickjs-248) | synchronous entry returns a Promise at the first await or completion; intrinsic PromiseResolve and internal reactions resume the detachable activation in the owning task. Rejections throw at await; return adopts values and thenables. Pending operands and handlers are traced; dependency reads remain whole-task waits. Empty queues complete and report suspended function names without retaining their activations. Async generators remain deferred. |
| `a + b * (yield c)` and yields in try/catch/finally (clj-quickjs-244) | pending operands stay on the saved activation's value stack without compiler spills. Handlers and normal/return/throw/jump completions are saved relative to the activation and rebased on next, preserving their values across GC and calls from different depths. Finally runs once when resumed control leaves the protected block; a yielding finally continues its pending completion after its last yield or dependency wait. |
| `function* g(x) { yield x; }`, generator expressions/methods and `next(value)` (clj-quickjs-243) | calls evaluate defaults and bind arguments before creating a generator; the body starts at first next. Yield suspends a traced task-owned activation and produces `{value, done: false}`; the next argument becomes the yield result. Return/fallthrough produces a done result, uncaught throw completes and propagates, and later next calls return undefined/done. Generators inherit the shared iterator identity and support for-of. Dependency waits, budgets, GC, cancellation and worker migration use ordinary attached VM frames. Re-entrant next and incompatible receivers throw TypeError; publication and foreign-task access fail. Generator return/throw methods and async generators remain deferred; delegated yield executes (clj-quickjs-245). |
| `var a = 1, b;` (clj-quickjs-94) | one binding per name per function body (or script), wherever the `var` statements are (nested blocks, loop and `if` bodies, `try`/`catch`/`finally` blocks and `switch` clauses included, nested functions excluded); hoisted: it exists and is `undefined` from function entry, with no temporal dead zone (`x; var x = 1;` reads `undefined`); repeating `var` is allowed and `var x;` does nothing (it does not reset an assigned value); an initializer is an assignment that runs when the statement does; a `var` named like a parameter, or like a function declaration at the top of the same body, is that binding; a `var` statement has no completion value. Closures capture a `var` under the rule of every binding: by value if it is never assigned, otherwise through one cell for the whole call, so closures made in different loop iterations share it (unlike a `let` in the loop body). A `var` in a catch block may repeat the simple catch parameter (ECMAScript Annex B, as QuickJS does): it declares the function's binding, and its initializer assigns the parameter |
| OrdinaryToPrimitive on object operands (clj-quickjs-70) | arithmetic, bitwise, unary and update operators use hint number; `+` and object/primitive `==` use hint default; templates and computed property keys use hint string. Number/default tries `valueOf`, then `toString`; string reverses that order. Non-callable methods are skipped; each method is called at most once with the original object as `this` and no arguments. The first primitive result wins; otherwise a catchable TypeError "cannot convert object to primitive value". Method exceptions propagate. Left conversion finishes before right conversion begins, after both expressions are evaluated. Equality between two objects uses identity; nullish equality never converts. Object.prototype.valueOf returns the object and Object.prototype.toString returns "[object Object]" for plain objects. A task-owned native continuation retains operands and progress across budget yields, dependency waits, GC and migration; cancellation drops it through the usual finally unwinding. No Rust interpreter recursion |
| `+` | when either operand is a string: ToString of both, concatenated (a new string; a result over 2^30 - 1 code units fails with `OutOfMemory`, as QuickJS's "string too long"). ToString gives `"undefined"`, `"null"`, `"true"`/`"false"`, and for numbers Number::toString in radix 10: shortest round-trip digits (ties to the even digit), plain decimal for `1e-6 <= \|x\| < 1e21` (`"0.000001"`, `"19686109595169230000"`), otherwise exponent form (`"1e+21"`, `"1e-7"`), `"0"` for `-0`, `"NaN"`, `"Infinity"` (clj-quickjs-66). Otherwise ToNumber of both, then IEEE addition. Object operands first use resumable ToPrimitive (clj-quickjs-70), left before right, with hint default. Lowered to `add` when both operands are statically numbers, otherwise to `plus`, which decides at run time |
| `` `a${x}b${y}c` `` untagged template literals (clj-quickjs-57) | a new string: the cooked text parts and the ToString of each substitution, in order (ECMAScript TemplateLiteral evaluation, QuickJS `js_parse_template`); the substitutions are evaluated left to right, each converted right after it is evaluated and before the next runs. ToString is that of `+` (`"undefined"`, `"null"`, `"true"`/`"false"`, Number::toString); an object substitution first uses resumable ToPrimitive with hint string (clj-quickjs-70). Lowered to the first text part as a constant, then per substitution its code, the `to_string` opcode (`0x55`) and `plus`, then the next text part (when not empty) and `plus`; a result over 2^30 - 1 code units fails with `OutOfMemory` |
| `- * / %` and unary `-`, `+` | ToPrimitive with hint number on objects, then ToNumber on `undefined` (NaN), `null` (0), booleans (0/1) and strings (StringToNumber, next row), then IEEE arithmetic; `%` keeps the dividend's sign |
| StringToNumber (clj-quickjs-67), wherever ToNumber applies | leading and trailing whitespace and line terminators (including NBSP, BOM and Unicode `Zs`) are ignored; empty or all-whitespace is 0; an unsigned `0x`/`0o`/`0b` integer (any length, correctly rounded); a decimal with optional sign, fraction and exponent (`"-1.5e3"`, `".5"`, `"5."`; `"-0"` is -0; `"010"` is 10, no legacy octal); `Infinity` with an optional sign; anything else is NaN (`"1_000"`, `"1n"`, `"-0x1"`, `"12px"`, `"infinity"`) |
| `< > <= >=` | IsLessThan on primitives: two strings compare lexicographically by UTF-16 code units, a proper prefix being smaller (`"10" < "9"`, `"B" < "a"` and `"" < "a"` are true; `"\u{10000}" < "\uFFFF"` is true because U+10000 is the code units D800 DC00; clj-quickjs-68); any other pair converts with ToNumber, then compares numerically (anything with NaN is false; `"10" < 9` and `"abc" >= 1` are false). Object operands first use resumable ToPrimitive (clj-quickjs-70), left before right, with hint number. Lowered to `compare_lt`/`compare_gt`/`compare_le`/`compare_ge` when neither operand is statically a number, otherwise to ToNumber and the number-only `lt`/`gt`/`le`/`ge` |
| `&`, `\|`, `^`, `<<`, `>>`, `>>>` and unary `~` (clj-quickjs-62) | ToNumber of both operands (left first, after both are evaluated), then ToInt32 (NaN and ±Infinity are 0; truncate, wrap modulo 2^32), ToUint32 for the left operand of `>>>`; a shift count is `ToUint32(right) & 31`; the result is a 32-bit integer (unsigned for `>>>`), never `-0` |
| `**` (clj-quickjs-63) | ToNumber of both operands (left first, after both are evaluated), then Number::exponentiate: IEEE `pow`, except that a NaN or ±Infinity exponent with base `1` or `-1` gives NaN (`1 ** NaN`, `(-1) ** Infinity`); `x ** 0` is 1 even for NaN |
| `===`, `!==` | IsStrictlyEqual (`NaN !== NaN`, `0 === -0`; strings by content, never equal to a non-string) |
| `==`, `!=` | IsLooselyEqual for the supported types: `null == undefined`, neither equals anything else, booleans compare as numbers, two strings by content (`"1" == "01"` is false); a string compared with a number or boolean compares as a number, the string by StringToNumber and the boolean as 0 or 1 (`"" == 0`, `" 1 " == 1` and `"1" == true` are true, `"abc" == NaN` is false; clj-quickjs-67) |
| `typeof x` (clj-quickjs-69) | evaluates the operand, then gives its type name as a string (ECMAScript TypeofOperator, QuickJS `js_operator_typeof`): `"undefined"` for `undefined`, `"object"` for `null`, `"boolean"`, `"number"`, `"string"`, `"symbol"`; `"function"` for a function object (function declarations and expressions, arrow functions, closures) and a built-in function (`Object`, `Object.is`, ...; clj-quickjs-117), the callable values; `"object"` for every other object: persistent vectors, maps and occurrences, transient builders, atoms, error objects caught by `catch`, plain objects and arrays. An operand that is a name with no binding, possibly parenthesized (`typeof nope`, `typeof (nope)`), gives `"undefined"` instead of a ReferenceError: the compiler emits a run-time global lookup allowing a missing property; any other operand (`typeof (0, nope)`, `typeof nope()`) is evaluated as usual. A binding in its temporal dead zone is still a ReferenceError, as a plain read is. `typeof __rt` is a `ReservedWord` compile error, as any other non-call use. Lowered to the `typeof` opcode (`0x4E`), which allocates nothing on the heap and calls no JavaScript |
| `void x` (clj-quickjs-133) | evaluates the operand (its effects and failures happen: `void f()` calls `f`, `void nope` is a ReferenceError, unlike `typeof nope`), discards its value and gives `undefined`. A prefix operator with the precedence of `!`, so `void a ** 2` is `Invalid` at the `**`. No new opcode: the operand, then `pop` and `undefined` (QuickJS `js_parse_unary`, `TOK_VOID`: `OP_drop`, `OP_undefined`) |
| `delete o.p`, `delete o[k]` (clj-quickjs-124) | evaluates `o`, then `k`; an `undefined`/`null` `o` is a catchable `TypeError` ("cannot delete property 'p' of null") before `k` is converted (ToPropertyKey uses hint string for an object key, as for `o[k]`). Then removes `o`'s own property (ECMAScript `[[Delete]]`, QuickJS `OP_delete`/`delete_property`; inherited properties are untouched) and gives `true` if `o` no longer has it: an absent property is `true`; a configurable one is removed, the others keeping their creation order (so it comes last when added again), also from a non-extensible object; an array element becomes a hole, `length` unchanged. A non-configurable property (`defineProperty` without `configurable: true`, frozen objects, an array's `length`, a constructor's `prototype`) stays and is a `TypeError` ("cannot delete non-configurable property 'k'") in strict code and `false` in sloppy code. A string base acts as its wrapper object: its indices below the length and its `length` are non-configurable, any other key `true`; a number or boolean base is `true`. A realm intrinsic or built-in function (`Object.prototype.toString`, `Object.keys`) that has the property fails with a `TypeError` in both modes (shared and immutable); another task's object (or an embedder-made one) is `ForeignObject`; a function's own properties are in its property record (deleting initial `length`/`name` creates the record first, clj-quickjs-101); Error instances use ordinary configurable properties; runtime values without JavaScript properties (vectors, maps) give `true`. Run-time failures are reported at the `delete`. Any other operand (`delete 1`, `delete f()`) is evaluated and gives `true`. A prefix operator with the precedence of `!`. Lowered to the base, the key (a string constant for `o.p`) and the `delete` opcode (`0x61`) |
| `v instanceof F` (clj-quickjs-75) | evaluates `v`, then `F`, then the `instanceof` opcode (`0x60`) at the operator token (ECMAScript InstanceofOperator and OrdinaryHasInstance; QuickJS `JS_IsInstanceOf`, `JS_OrdinaryIsInstanceOf`): `F` must be an object and callable (a function, arrow or built-in function), else a catchable TypeError "invalid 'instanceof' right operand" (`({}) instanceof {}`, `instanceof 1`, a vector); a primitive `v` gives `false` without reading `F.prototype`; otherwise `F.prototype` (a function's lazy prototype object, created on first read) must be an object, else a catchable TypeError "operand 'prototype' property is not an object" (an arrow function, a built-in method such as `Object.is`, which inherits no `prototype` from `Function.prototype` or `Object.prototype`, or after `F.prototype = 1`), and the result is whether that object is on `v`'s prototype chain, by identity, as the chain is when the operator runs (`Object.create`, `Object.setPrototypeOf` and `G.prototype = new F()` chains count; `Object.create(null)` is an instance of nothing). Relational precedence, left-associative. A function's chain runs through `Function.prototype` (clj-quickjs-102: `F instanceof Function && F instanceof Object`); engine-raised and explicitly constructed errors support `instanceof` through their matching NativeError and Error prototypes (clj-quickjs-230). `Symbol.hasInstance` is exposed but is not consulted yet. Bound functions delegate to their targets; a `prototype` accessor runs resumably (clj-quickjs-120) |
| `k in o` (clj-quickjs-74) | evaluates `k`, then `o`, then the `in` opcode (`0x61`) at the operator token (ECMAScript `in` and HasProperty; QuickJS `js_operator_in`, `OP_in`): an `o` that is not an object is a catchable TypeError "cannot use 'in' operator to search for 'k' in o" (`'x' in 1`, `'x' in null`, `'length' in 'abc'`), checked before `k` is converted; then ToPropertyKey(`k`) as for `o[k]` (`1 in {1: 0}` and `'1' in {1: 0}` are the same key) and the result is whether `o` or an object on its prototype chain has the property, whatever its value (`'x' in {x: undefined}` and `'toString' in {}` are true; `Object.create(null)` has nothing inherited; `'__proto__' in {}` is true through `Object.prototype`'s accessor, which is not run). Arrays have their elements (not holes) and `length`; a function has its own properties, its lazy `prototype` (reported without being created, so the function stays publishable) and those of `Function.prototype` (`'call' in f`, clj-quickjs-102) and `Object.prototype`. Persistent vectors, maps, occurrences, builders, atoms and cells have no JavaScript properties: `false`. Relational precedence, left-associative. A name `Function.prototype` does not have yet (`'bind' in Object`, `'toString' in f`, clj-quickjs-103 and later) fails with `UnsupportedOperand`; Error instances have ordinary properties; symbol keys (clj-quickjs-127) use identity. Object key conversion may call JavaScript |
| `!`, `&&`, `\|\|`, `? :`, `if`, `while`, `do`-`while`, `for` | ToBoolean (falsy: `0`, `-0`, `NaN`, `null`, `undefined`, `false`, `""`); `&&`/`\|\|` short-circuit and return an operand |
| a directive (`"use strict";`, clj-quickjs-95) | an ordinary string expression statement: evaluates to the string, which becomes the completion value of a script; its only other effect is the strictness recorded at compile time ("Directive prologues and strictness") |
| `x = v` | evaluates `v`, then stores; the expression's value is `v`. In sloppy code an unresolvable name creates a writable, enumerable, configurable global property (clj-quickjs-105); strict code throws ReferenceError. Ordinary property Set rules apply, including inherited setters and ignored sloppy writes on a non-extensible global object |
| `delete x`, `delete (x)` (clj-quickjs-105) | sloppy code only: an unresolvable name gives `true`; a global object binding deletes the own property if configurable (`true`), otherwise stays (`false`, including new script `var`/function globals and the fixed `undefined`/`NaN`/`Infinity` properties). Inherited properties stay and give `true`. A declarative binding (lexical, parameter, local `var`, function self-name, captured binding) gives `false` without reading it, including in its temporal dead zone. Getters never run. Lowered to `delete_global` for dynamic global names, or constant `false` for a declarative binding; failures are reported at `delete` |
| `{a: 1, "b": 2, 3: 4, c}` object literals (clj-quickjs-114) | a new ordinary object owned by the running task (`docs/architecture.md`, "Ordinary objects"), inheriting from `Object.prototype` (clj-quickjs-117; lowered to `new_object_literal`): the values are evaluated left to right and each defines a writable, enumerable, configurable data property in creation order; a key is an identifier name (reserved words included), a string, or a number's ToString (`1.50` is `"1.5"`, `0x10` is `"16"`); a shorthand `{c}` is `{c: c}`; a duplicate key keeps its first position and its last value. Two evaluations are distinct objects (`({}) !== ({})`); `typeof` is `"object"` |
| `{m(a) { return a; }}` concise methods (clj-quickjs-126) | identifier, reserved-word, string and numeric keys define writable, enumerable, configurable data properties. `get()`, `set()`, `async()` and `static()` are ordinary method names. Each method inherits the surrounding strictness (or becomes strict through its own `"use strict"` directive, clj-quickjs-266), has its own `this` and unmapped `arguments`, and supports default parameters and closures. Its `name` is the key's string value, its `length` counts parameters before the first default, and it has no own `prototype` and cannot be constructed (`new o.m()` throws TypeError). It records the literal as an immutable `[[HomeObject]]`, traced by GC and followed by publication: methods cannot publish while their home object is task-owned. `__proto__() {}` defines a method without setting the prototype. `super` remains deferred; generator methods execute through detachable activations (clj-quickjs-243); async methods execute (clj-quickjs-248). |
| `{__proto__: p, "__proto__": p}` in object literals (clj-quickjs-121) | a non-shorthand `__proto__` key, written as a name or a string, defines no property: its value, evaluated in its place among the others, becomes the literal's prototype if it is an object or `null` and is ignored otherwise (lowered to `set_proto` after the value; QuickJS `OP_set_proto`); a second one in the same literal is an early `Invalid` SyntaxError. A shorthand `{__proto__}` and a computed key define an ordinary own property |
| `o.name` (clj-quickjs-114) | evaluates `o`; an ordinary object gives its own property, else the first one along its prototype chain (`Object.prototype` for literals, clj-quickjs-117), else `undefined`; a built-in function gives its own property, else the one it inherits from `Function.prototype` (clj-quickjs-102: `call`, `apply`, `constructor`) and then `Object.prototype`, and fails with `UnsupportedOperand` for a standard `Function.prototype` name that does not exist yet (`bind`, `toString`, `arguments`, `caller`); persistent vectors, maps and occurrences, builders and atoms have no JavaScript properties (`undefined`); `undefined`/`null` throw a catchable `TypeError` ("cannot read property 'x' of undefined") at the member expression; a function object (clj-quickjs-125, `docs/architecture.md`, "Function own properties") gives its own property, else `prototype` of a `function` declaration or expression: an object created on first read (owned by the task, inheriting from `Object.prototype`, with `constructor` the function) and the same object afterwards (an arrow has none: `undefined`); `length` and `name` are initial own metadata (clj-quickjs-101); a missing `Function.prototype` name (`bind`, `toString`, `arguments`, `caller`) fail with `UnsupportedOperand`, any other name continues at `Function.prototype` (`f.call`, `f.apply`, `f.constructor === Function`, clj-quickjs-102) and then `Object.prototype`; a published function's `prototype` cannot be created (`UnsupportedOperand`); a number or string reads through `Number.prototype` / `String.prototype` (clj-quickjs-73, see `(1).toString()` below); a string's own `length` and code-unit indices are read before its prototype (clj-quickjs-142); caught engine-raised errors use ordinary Error instance properties (clj-quickjs-230); booleans fail with `UnsupportedOperand` until Boolean.prototype exists (clj-quickjs-39); an object owned by another task fails with `ForeignObject` |
| `o.name = v` (clj-quickjs-114) | evaluates `o`, then `v`, then writes: an ordinary object's own writable property is updated; otherwise, unless the first object along its prototype chain with the property has it read-only, a new own data property is added (writable, enumerable, configurable), so `o.toString = f` shadows the inherited method; a built-in object (`Object.prototype`, `Object`, ...) throws a catchable `TypeError` ("built-in objects are shared by all tasks and immutable", clj-quickjs-117); a function object gets the property in its task-owned property record (clj-quickjs-125), so it can no longer be published; a function that was published is shared and immutable (a catchable `TypeError`), one created by another task fails with `ForeignObject`; initial `length` and `name` reject writes as read-only, and a missing `Function.prototype` name fails with `UnsupportedOperand` unless the function has it as its own; a value without JavaScript properties (vectors, maps, builders, ...) or an `undefined`/`null` base throws a catchable `TypeError` ("cannot set property 'x' of null") at the member expression, after `v` ran; a number, string or boolean base (clj-quickjs-73) takes no property: a catchable `TypeError` ("cannot create property 'x' on a number", "cannot assign to read-only property '0' of a string") in strict code, nothing in sloppy code, and `__proto__` runs the setter, which ignores a primitive; the other bases fail as for a read. An own or inherited read-only property (`Object.defineProperty`, clj-quickjs-119) and a non-extensible receiver reject the write: a catchable `TypeError` ("cannot assign to read-only property 'k'") in strict code, nothing in sloppy code. The expression's value is `v` |
| `[a, , b,]` array literals (clj-quickjs-118) | a new array object owned by the running task (`docs/architecture.md`, "Array objects"), inheriting from `Array.prototype` (a realm intrinsic with `push`, which inherits from `Object.prototype`; see "Explicit gaps"): the elements are evaluated left to right and each defines the next index; an elision skips an index, leaving a hole (no own property: reading it continues along the prototype chain and gives `undefined`); `length` is the number of elements and holes (`[1, , 3].length` is 3, `[ , ].length` 1, `[1, 2,].length` 2). Two evaluations are distinct objects (`[] !== []`); `typeof` is `"object"`. Lowered to `new_array` and one `append_element`/`append_hole` per element |
| `o[k]` (clj-quickjs-115, -118) | evaluates `o`, then `k`, then converts `k` by ToPropertyKey (see "Explicit gaps") and reads as `o.name` does: on an ordinary object the key string names the property (`o[1]`, `o["1"]` and `o[1.0]` are one property, `o["01"]` another); on an array an array index (the canonical decimal string of an integer `0` to `2^32 - 2`, from a string or a number: `a[1]`, `a["1"]`, `a[-0]`) names an element, `length` its length, any other key an ordinary property; failures as for `o.name`, at the `[`. Lowered to `get_elem` |
| `o[k] = v` (clj-quickjs-115, -118) | evaluates `o`, `k`, then `v`, then checks that `o` is not `undefined`/`null` (a catchable `TypeError`), converts `k` and writes as `o.name = v` does; writing an array index at or beyond an array's `length` sets `length` to that index plus one (`let a = [1]; a[3] = 4` gives length 4 with holes at 1 and 2), and any other key adds an ordinary property without changing `length`; assigning an array's `length` follows ArraySetLength (clj-quickjs-154, see "Array length changes" below). The expression's value is `v`. Lowered to `put_elem` |
| `a.push(...items)` (clj-quickjs-155) | generic array-like append at ToLength(`length`), followed by a throwing length Set; returns the new length. `Array.prototype.push.call(o, item)` works through the global `Array` constructor. `push.length` is 1; null/undefined receivers and rejected Sets throw caught `TypeError`; primitive wrappers and object-valued length conversion fail explicitly. See "Array push" below |
| `Array(...args)`, `new Array(...args)`, `Array.isArray(v)` (clj-quickjs-153) | both constructor forms create task-owned arrays inheriting from `Array.prototype`; one Number argument is a hole-only uint32 length (otherwise catchable `RangeError`), other arguments become elements without coercion. The brand test accepts arrays and `Array.prototype`, and rejects persistent vectors and ordinary objects. `Array.prototype.constructor === Array`; see "Array construction" below |
| `++x`, `--x`, `x++`, `x--` on a binding (clj-quickjs-64) | reads `x` (the checks of a read), ToNumber, adds or subtracts 1, stores (the checks of `=`, so a `const` is a TypeError after the read); a prefix update's value is the new number, a postfix update's the old one converted (`let t = true; t++` is 1, `t` is 2); no new opcode: `to_number`, `add`/`sub` and the binding's store; failures of the conversion are reported at the operator token |
| `++o.p`, `o.p--`, `++o[k]`, `o[k]--` (clj-quickjs-76) | evaluates `o` once and, for `o[k]`, `k` once, then checks that `o` is not `undefined`/`null` (a catchable `TypeError`) and converts `k` with ToPropertyKey once; reads the property as `o.p` does (an absent one is `undefined`, so `let o = {}; o.x++` is NaN and writes NaN), ToNumber, adds or subtracts 1 and writes as `o.p = v` does (strict: a failed write is a `TypeError`); a prefix update's value is the new number, a postfix update's the old one converted (`let a = [true]; a[0]++` is 1, `a[0]` is 2). Lowered to `get_field`/`put_field` with the base `dup`ed, or to `to_property_key` (new, `0x5D`), `dup2` (new, `0x5C`), `get_elem` and `put_elem`; a postfix update keeps the old number in a scratch local across the write. The read, the checks and the write are reported at the `.`/`[`, the conversion at the `++`/`--` |
| `x op= v` on a binding, for `op` one of `+ - * / % ** & \| ^ << >> >>>` (clj-quickjs-84) | reads `x` (the checks of a read: an unresolvable name or a binding in its temporal dead zone is a ReferenceError before `v` is evaluated), evaluates `v`, applies `op` exactly as `x op v` does (so `+=` concatenates when either value is a string, and the others apply ToNumber to both values, the old one first), then stores (the checks of `=`, so a `const` is a TypeError after `v` is evaluated); the expression's value is the stored value. As in JavaScript (QuickJS `js_parse_assign_expr2`), the old value is the one read before `v` runs: `let a = 1; function f() { a = 10; return 5; } a += f(); a` is 6. No new opcode: the binding's read and store around the operator's own instructions; failures of the operator are reported at the `op=` token |
| `o.p op= v`, `o[k] op= v` for the same operators (clj-quickjs-260) | evaluates `o` once and, for `o[k]`, `k` once, then (for `o[k]`) checks that `o` is not `undefined`/`null` and converts `k` with ToPropertyKey once; reads the property as `o.p` does (which checks the base of `o.p`), so a `null`/`undefined` base is a catchable `TypeError` before `v` runs; evaluates `v`, applies `op` as `x op= v` on a binding does, and writes as `o.p = v` does (a rejected write, such as a read-only property, is a catchable `TypeError` in strict code and does nothing in sloppy code); the value is the result of `op`. As in QuickJS (`js_parse_assign_expr2`, `get_lvalue`/`put_lvalue`), the order is base, key, key conversion, read, `v`, operator, write: `let a = [1, 1], i = 0; a[i = i + 1] += 4` evaluates `i = i + 1` once, and `get().x += 1` calls `get` once. Lowered with the reference instructions of `o.p++` (`dup` / `get_field` / `put_field`, or `to_property_key` / `dup2` / `get_elem` / `put_elem`) around the operator's own instructions; no new opcode. The read, the checks and the write are reported at the `.`/`[`, the operator at the `op=` |
| `a, b` | evaluates `a` and discards its value, then evaluates `b`, the value; as the last expression statement, `b` is the completion value |
| `return` (in `compile_function`) | completes the task |
| `do body while (test)` (clj-quickjs-80) | runs the body, then evaluates the test, and repeats while it is truthy: the body always runs at least once |
| `for (init; test; update) body` (clj-quickjs-81) | runs the init once (an expression's value is discarded), then repeats: evaluates the test (absent: true) and stops when it is falsy, runs the body, then the update (its value discarded) |
| `for (let ...; ...; ...)`, `for (const ...; ...; ...)` | the head's bindings are scoped to the loop and in their temporal dead zone during their own initializers (`for (let i = i; ;)` is a ReferenceError); each iteration has its own copy of the `let` bindings (CreatePerIterationEnvironment, before the first test and before each update), so a closure created in an iteration keeps that iteration's binding, and the next iteration starts from the values it ended with, including assignments made by closures; updating a `const` head is a TypeError |
| `for (k in o) body`, `for (let k in o)`, `for (const k in o)` (clj-quickjs-85, -86) | evaluates `o` once (with a `let`/`const` head's binding in its temporal dead zone: `for (let k in k)` is a ReferenceError), then runs the body once per enumerable string key of `o` and its prototype chain: first `o`'s own keys, snapshotted when the loop starts, in OrdinaryOwnPropertyKeys order (array indices ascending, then the other keys in creation order; as `Object.keys`, clj-quickjs-123), then each prototype's, snapshotted when the walk reaches it (clj-quickjs-86), each key a string; an inherited key is skipped when an object nearer `o` had an own property of that name, enumerable or not (a non-enumerable own `x` hides an inherited enumerable `x`), so each name is visited at most once; before each key it checks that the key is still an own enumerable property of the object it was found on and skips it otherwise (so a property deleted or made non-enumerable before it is reached is not visited), and keys added to an object after its snapshot are not visited (a prototype the walk has not reached yet is read when it is reached; when the chain changes, the walk continues on the prototype it is on). `Object.prototype` and the other realm intrinsics have no enumerable properties, so the walk ends there without reading them under any lock, and after a function (`Function.prototype`, clj-quickjs-102). `null`, `undefined`, numbers and booleans run zero iterations, a string visits its indices `"0"`, `"1"`, ...; enumerating an object of another task fails with `ForeignObject`. An identifier head is assigned each key as by `k = key` (a `const` binding is a TypeError, an undeclared name creates a global in sloppy code and throws ReferenceError in strict code, at the name); a `let`/`const` head is a fresh binding per iteration scoped to the loop, so closures keep their iteration's key. `break`, `continue`, labels, `return` and exceptions leave or continue the loop as in other loops (through finally blocks too). The enumerator is task-owned frame state (`crate::vm::keys`): a yield, a dependency wait, a collection or migration inside the loop resumes it exactly. Lowered to `for_in_start` and `for_in_next` (new, `0x63`, `0x64`) with the enumerator in a scratch local |
| `for (var ...; ...; ...)`, `for (var k in o)` (clj-quickjs-89) | the head declares `var` bindings of the enclosing function or script, hoisted like a `var` statement's (readable, `undefined`, before the loop; one binding per name, shared with other `var`s and a parameter of that name); each initializer of a `for` head is an assignment run once, before the first test, and a for-in `var` head is assigned each key as by `k = key`; there is no per-iteration copy, so every closure created in the body shares the one binding and sees its latest value (closures made by `for (var i = 0; i < 3; i++)` all return 3 afterwards), and the binding keeps its last value after the loop (`for (var k in {x: 1, y: 2}) {}` leaves `k` `"y"`) |
| `break`, `continue`; `break L`, `continue L`, `L: body` (clj-quickjs-82) | `break` leaves the innermost enclosing loop, `continue` evaluates its test again (in a `for` loop, runs the update first); `break L` leaves the enclosing statement labelled `L` (a loop or any other statement, continuing after it) and `continue L` continues the enclosing loop labelled `L` (directly or through a label set `L: M: while ...`), leaving every loop in between; an unlabelled `break`/`continue` passes by labelled statements that are not loops; leaving a `try` block pops its catch handler, so a later exception is not caught by it; leaving a `try` block or catch body runs its finally block first (innermost first, clj-quickjs-79), and a finally block that completes abruptly (`return`, `throw`, `break`, `continue`) replaces the jump; a jump out of a finally block discards its pending completion (return value, exception or jump) |
| `switch (e) { case a: ... default: ... }` (clj-quickjs-83) | evaluates `e` once, then the `case` tests in source order, comparing each with IsStrictlyEqual (`===`: `-0` matches `0`, `NaN` matches nothing, `"1"` does not match `1`) and evaluating none after the first match; runs from the matching clause, or from `default` (wherever it is) if none matches, through the following clauses (fall-through); an unlabelled `break` leaves the switch, `continue` the switch and the iteration of its enclosing loop; the clauses form one block scope whose function declarations are created on entry, before the tests |
| function declarations | hoisted: entering a script, function body or block creates the closures of the declarations directly in it, in source order, before its first statement runs. At the top level of a script or function body they are var-scoped (clj-quickjs-94): one may repeat a parameter, a `var` or another function declaration there, which all share one binding, and the last declaration wins (`function g() { return 1; } function g() { return 2; } g()` is 2; `var h; function h() {}` leaves `h` the function). In a block they are lexical (strict mode) |
| function expressions | each evaluation creates a new function object (`===` compares identity); a named expression's name is visible only inside it, is the running function, and assigning it is a TypeError |
| arrow functions | lowered as an anonymous function expression whose concise body is `{ return body; }`: same closures, captures, cells and temporal dead zone; `arguments` resolves lexically through enclosing functions, including in defaults (clj-quickjs-97); an arrow creates no arguments object. `new.target` and `super` stay rejected. In bodies and default parameters, `this` is the nearest enclosing non-arrow function's receiver, captured by value when the arrow is created, through every intervening arrow (clj-quickjs-98). Plain calls, method calls, `call`, `apply`, `Reflect.apply` and `bind` cannot change it. An ordinary function nested inside an arrow has its own receiver. An arrow without an enclosing non-arrow function captures the script global object (clj-quickjs-104). A closure capturing `this` follows transitive publication rules: a primitive or otherwise shareable receiver is publishable, a task-owned object receiver is not; no binding cell is introduced. An arrow remains non-constructible (`new` throws a catchable TypeError) |
| `f(a, b)` | evaluates the callee, then the arguments left to right, then calls in a new task-owned frame; missing parameters are `undefined`, extra values are available through `arguments`; falling off the end returns `undefined`; the callee's `this` is `undefined` (strict-mode semantics; see `this` below) |
| `arguments` (clj-quickjs-97) | an ordinary task-owned object inheriting from `Object.prototype`, created at entry only for non-arrow functions that reference it directly or through arrows. It holds all actual arguments in writable, enumerable, configurable index properties and a writable, non-enumerable, configurable `length`. Defaults see original actual values before parameter initialization. A parameter, function or lexical declaration named `arguments` shadows it; a plain `var arguments;` preserves it. Arrows capture the binding under the value/cell rule; closures reaching the object cannot be published. Original arguments and the object are traced across yields, waits, GC and migration. At script level it is an ordinary identifier (ReferenceError if unresolved, `typeof` yields `"undefined"`) |
| `function f(a, b = expr) { ... }`, `(a = expr) => ...` (clj-quickjs-100) | on entry, the parameters are initialized left to right; a parameter's default is evaluated only when its argument is `undefined` (missing or passed: `f(1)` and `f(1, undefined)` run it, `f(1, null)` does not) and becomes its value. Parameters are bindings with a temporal dead zone: a default sees the parameters before it, the enclosing scopes and the function's own name, but reading itself or a later parameter is a ReferenceError (`function f(a = b, b) {} f()`). The parameters have a scope of their own around the body's: the body's `let`/`const`/`var`/function declarations are invisible to the defaults (`const x = 1; function f(a = () => x) { var x = 2; return a(); }` gives 1); a body `var` that repeats a parameter is a separate binding starting with the parameter's value, and a body function declaration may repeat one (a `let`/`const` may not). Closures created in a default capture the parameter bindings under the usual value/cell rule, so they see the body's later assignments to a parameter, and one run before its parameter is initialized fails with the run-time ReferenceError. QuickJS: `js_parse_function_decl2` (`has_parameter_expressions`) |
| `o.m(a, b)` (clj-quickjs-117) | a method call: evaluates `o` once, reads `m` (the checks of `o.name`), evaluates the arguments, then calls with `o` as the receiver (`call_method`); a parenthesized callee `(o.m)()` is the same call, `(0, o.m)()` is a plain call. A built-in function sees the receiver as `this`, and so does a JavaScript function (clj-quickjs-116). `(f = o.m)()` and `let f = o.m; f()` are plain calls too. A plain call passes `undefined` as the receiver |
| `this` (clj-quickjs-116, -104) | at script level: the task's global object, even in strict code. Inside a non-arrow function (its body and its parameters' defaults, and blocks, `try` and nested statements in it): the receiver the running call was made with, the very object (`get_this`, which reads the frame's `this` slot and allocates nothing): `o` for a method call `o.m()`, `undefined` for a plain call `f()`, for a task's entry function and for a callback of a resumable native (`__rt.reduce`). A function nested in it has its own `this`. In strict code the receiver is used as is, so `this.x` in a plain call throws a catchable TypeError. In sloppy code an object receiver is used as is, and evaluating `this` with any other receiver fails the task with `UnsupportedOperand` (it would be the global object, clj-quickjs-104, or a wrapper, clj-quickjs-72). The frame's `this` is task-owned and traced with the frame, so it survives waits, collections and migration |
| `new F(a, b)`, `new F` (clj-quickjs-71) | evaluates `F`, then the arguments (none without an argument list), then `construct` at the `new` token (ECMAScript EvaluateNew, QuickJS `OP_call_constructor`): `Number` and `String` construct wrappers (clj-quickjs-72, below); otherwise `F` must be an ordinary, non-generator, non-async function from a `function` declaration or expression; generators, async functions, concise methods, accessors, arrows and other non-constructors (a primitive, a built-in such as `Object`, a collection, a plain object) throw a catchable TypeError `… is not a constructor` reported at `new`. A new ordinary object owned by the task inherits from `F.prototype` (the lazy prototype object, created on first use, whose `constructor` is `F`) or, when `F.prototype` is not an object, from `Object.prototype`; `F` runs in a new task-owned frame with it as `this` (so it may yield, wait on `__rt.need`, be collected around and migrate like any call). If `F` returns an object, that object is the result, otherwise the new object is (return override). `new F()()` calls the result; `new new F()()` constructs with it. `new` on a published (shared) function fails like reading its `prototype` |
| `Object` and other built-in globals (clj-quickjs-117) | a name with no lexical binding that is a built-in global (`Object`; `Number` and `String`, clj-quickjs-73; `Function` and `Reflect`, clj-quickjs-102) is read from the task's global object (`get_global`); its initial value is the shared immutable intrinsic, the same object in every task. The binding can be replaced without changing the intrinsic. A `let`/`const`/parameter shadows it; script `var` reuses the property |
| `Object(v)`, `Object()` (clj-quickjs-117) | `undefined`, `null` or no argument: a new ordinary object inheriting from `Object.prototype`; an object (any heap value): `v` itself; a number, string or boolean: `UnsupportedOperand` (wrapper objects, clj-quickjs-72). `Object.length` is 1, `Object.prototype.constructor === Object` |
| `Object.is(a, b)` (clj-quickjs-117) | SameValue: like `===` except `Object.is(NaN, NaN)` and `!Object.is(0, -0)` |
| `Object.prototype.toString()`, `.valueOf()` (clj-quickjs-117) | inherited by every literal: `toString` gives `"[object Object]"` for an ordinary object and the runtime's other non-function heap values, `"[object Function]"` for a function, `"[object Error]"` for any Error instance, `"[object Undefined]"`/`"[object Null]"` for those receivers (a plain call); `valueOf` gives the receiver itself, and throws a catchable `TypeError` for `undefined`/`null`; primitive receivers fail with `UnsupportedOperand` (clj-quickjs-72/-73) |
| `a.join(separator)`, `a.toString()` (clj-quickjs-156) | `join` is generic over array-like objects and Number/String wrappers: ToObject, read `length` once and convert through ToLength (clamped to `2^53 - 1`), then ToString of the separator (undefined defaults to `","`), then read and stringify each element in order; holes, undefined and null contribute empty strings. Getters and object coercion are resumable and never replayed across yields or waits. Result length above `2^30 - 1` throws catchable RangeError `"string too long"`. One element per native step, then at most 256 units/part boundaries copied per step. `toString` gets `this.join`, calls it with no arguments if callable, otherwise calls intrinsic `Object.prototype.toString` (`"[object Array]"` for arrays). Array conversion in `+`, template literals and property keys follows these methods. Cyclic arrays recurse through explicit native frames until `VmLimits.max_call_depth` raises catchable StackOverflow; no Rust recursion or cycle elision. Null/undefined receivers throw TypeError; Boolean wrappers remain deferred |
| `Object.defineProperty(o, k, d)` (clj-quickjs-119) | `o` must be an object (else a catchable `TypeError`); `k` goes through ToPropertyKey as in `o[k]`; `d` must be an object whose `enumerable`, `configurable`, `value`, `writable`, `get`, `set` are read in that order, inherited ones included, and a field present with the value `undefined` counts (ToPropertyDescriptor). A `get`/`set` that is not a function or `undefined`, or one together with `value`/`writable`, is a `TypeError`. A new property takes the given fields, absent ones `undefined`/false (`{value: 1}` is read-only, non-enumerable and non-configurable); an existing one keeps the absent fields. A non-configurable property rejects becoming configurable, a change of `enumerable`, and, when it is also read-only, becoming writable or a value that is not SameValue (`NaN` matches `NaN`, `0` does not match `-0`): a catchable `TypeError` ("cannot define property 'k': ..."), nothing changed. On an array (clj-quickjs-118) an index key defines an element with the same rules (a new index extends `length`; one at or beyond a read-only `length` is a `TypeError`), and `length` (writable, non-enumerable, non-configurable) can be made read-only, after which writes of `length` and of new indices at or beyond it are rejected like other read-only properties; a define that changes `length` follows ArraySetLength (clj-quickjs-154, see "Array length changes" below). Returns `o`. A built-in object as `o` is a `TypeError` (immutable); a value without JavaScript properties (vectors, maps, ...) is not extensible (`TypeError`); Error instances use the ordinary object rules; another task's object with `ForeignObject` |
| `Object.getOwnPropertyDescriptor(o, k)` (clj-quickjs-119) | `undefined` if `o` has no own property `k` (inherited ones do not count), else a new object inheriting from `Object.prototype` with `value`, `writable`, `enumerable`, `configurable` in that order; works on built-in objects (`Object.getOwnPropertyDescriptor(Object, "length")` is `{value: 1, writable: false, enumerable: false, configurable: true}`); an array reports its elements and its `length` with their current attributes; it gives `undefined` for vectors, maps and other values without JavaScript properties. `undefined`/`null` `o` throw a catchable `TypeError`; a number, string or boolean `o` reports its wrapper's own property without allocating a wrapper (string indices and `length`, clj-quickjs-142; none for numbers or booleans); Error instances use the ordinary object rules |
| `Object.getPrototypeOf(v)` (clj-quickjs-121) | the prototype of `v` (`docs/architecture.md`, "Prototype reflection"): `Object.prototype` for literals, `Object()` results and descriptor objects, `Array.prototype` for arrays (whose prototype is `Object.prototype`, whose prototype is `null`), whatever `Object.create`, `Object.setPrototypeOf` or `__proto__` set, and `null` for vectors, maps and other values without JavaScript properties. `Function.prototype` for functions, arrows and built-in functions, and `Object.prototype` for `Function.prototype` (clj-quickjs-102). `undefined`/`null` (or no argument) throw a catchable `TypeError`; a number, string or boolean (wrapper objects, clj-quickjs-72) fail with `UnsupportedOperand` |
| `Object.setPrototypeOf(o, proto)` (clj-quickjs-121) | sets the prototype of the object or array `o` to `proto` and returns `o`; a number, string or boolean `o` is returned unchanged. A `TypeError` (catchable, nothing changed) for an `undefined`/`null` `o`, a `proto` that is neither an object nor `null`, a `proto` whose chain leads back to `o` (a cycle), a built-in object (shared and immutable) and a value without JavaScript properties or a non-extensible object; setting the prototype `o` already has always succeeds (`Object.setPrototypeOf(Object.prototype, null)`, `Object.setPrototypeOf(f, Function.prototype)`). Changing a function's prototype fails with `UnsupportedOperand`; another task's object (as `o` or anywhere on `proto`'s chain) with `ForeignObject` |
| `Object.create(proto)` (clj-quickjs-121) | a new ordinary object owned by the running task whose prototype is `proto`, an object or `null` (`Object.create(null)` has no `toString`); anything else (`Object.create(1)`, `Object.create()`) is a catchable `TypeError`. A second argument other than `undefined` (`Object.defineProperties`) fails with `UnsupportedOperand` |
| `o.__proto__`, `o.__proto__ = p` (clj-quickjs-121) | `Object.prototype`'s `__proto__` accessor, met when the read or write of `__proto__` (also `o["__proto__"]`) reaches `Object.prototype` without finding an own `__proto__` first: the read gives `Object.getPrototypeOf(o)`; the write sets the prototype like `Object.setPrototypeOf` when `p` is an object or `null` (its `TypeError`s are thrown in sloppy code too) and does nothing otherwise, and adds no own property, before the rule that built-in objects are immutable applies (`Object.prototype.__proto__ = null` is allowed); the assignment's value is `p`. An object whose chain does not reach `Object.prototype` (`Object.create(null)`) has an ordinary `__proto__` property, and an own `__proto__` data property shadows the accessor. It is a stored, non-enumerable, configurable accessor (clj-quickjs-261): `Object.getOwnPropertyDescriptor(Object.prototype, "__proto__")` returns `get`, `set`, `enumerable` and `configurable`; the built-in functions have names `get __proto__` / `set __proto__`, lengths 0 / 1 and inherit from `Function.prototype`. `Object.getOwnPropertyNames(Object.prototype)` includes `__proto__`. Its getter accepts number/string primitives and wrappers; Boolean.prototype remains unsupported |
| `Object.preventExtensions(o)` (clj-quickjs-122) | makes the object or array `o` non-extensible and returns `o`; a primitive (or no argument) is returned unchanged. Its existing properties stay writable, configurable and redefinable; adding one fails: `o.y = 1` and `o[k] = 1` are a catchable `TypeError` ("cannot add property 'y': the object is not extensible") at the member expression in strict code and do nothing in sloppy code, `Object.defineProperty` with a new key is always a `TypeError`, and so is `Object.setPrototypeOf(o, p)` (or `o.__proto__ = p`) with a different `p`. On an array it also forbids new elements (existing ones stay writable). A function gets a property record (its `prototype` created first), after which it cannot be published; a published function is returned unchanged (already immutable). A built-in object is a `TypeError` (shared and immutable); vectors, maps and other values without JavaScript properties are returned unchanged; another task's object fails with `ForeignObject`; Error instances follow the ordinary object rules |
| `Object.freeze(o)` (clj-quickjs-122) | `Object.preventExtensions(o)`, then every own property becomes non-writable and non-configurable (`enumerable` kept), array elements and `length` included; returns `o`. Writes of its properties are then rejected like other read-only properties (strict `TypeError` "cannot assign to read-only property 'x'", sloppy no-op), as are `a.length = n` and any index write on a frozen array; `Object.defineProperty` accepts only same-value redefinitions. Freezing is shallow and does not change ownership: a frozen object stays task-owned. Primitives, built-ins, functions and the other kinds as for `preventExtensions` |
| `Object.isExtensible(v)`, `Object.isFrozen(v)` (clj-quickjs-122) | `isExtensible`: the object's `[[Extensible]]` flag; `false` for a primitive (or no argument). `isFrozen`: `true` when the object is not extensible and no own property (elements and `length` included) is writable or configurable, however it got there; `true` for a primitive. A built-in object reports `true` / `false` (its descriptors are the standard ones; immutability comes from sharing). A function without a property record is extensible and not frozen; a published function, a vector, map, builder or other value without JavaScript properties is not extensible and frozen. Another task's object (or another task's function with a property record) fails with `ForeignObject`; Error instances follow the ordinary object rules |
| `Object.keys(o)`, `Object.getOwnPropertyNames(o)` (clj-quickjs-123) | a new array (inheriting from `Array.prototype`) of the own string keys of `o`: `keys` the enumerable ones, `getOwnPropertyNames` all of them, in ECMAScript OrdinaryOwnPropertyKeys order: keys that are array indices (canonical integers `0 ..= 2^32 - 2`) ascending, then the other names in creation order (a redefinition keeps the first position; an array's `length` comes first among them). `Object.keys({b: 1, a: 2, 1: 3, 0: 4})` lists `0`, `1`, `b`, `a`; `Object.getOwnPropertyNames([ , 1])` lists `1`, `length` (holes are absent). Inherited properties are not listed. Built-in objects list their stored properties (`Object.keys(Object)` is empty); a function lists the enumerable properties assigned to it; vectors, maps and other values without JavaScript properties list none. `undefined`/`null` (or no argument) throw a catchable `TypeError`; a primitive lists its wrapper's keys without allocating a wrapper (`Object.keys("ab")` is `["0", "1"]`, `Object.getOwnPropertyNames("ab")` is `["0", "1", "length"]`, none for numbers or booleans; clj-quickjs-142); Error instances use the ordinary object rules; another task's object with `ForeignObject` (`docs/architecture.md`, "Own-property keys") |
| `(1).toString()`, `"abc".valueOf()`, `n.x` on a number or string (clj-quickjs-73) | a property read on a number or string primitive (`n.x`, `s[k]`, `1..toString`) looks the key up as on its wrapper object, which is not allocated (ECMAScript GetValue; QuickJS `JS_GetPrototypePrimitive`): from `Number.prototype` / `String.prototype` along its chain to `Object.prototype`, else `undefined` (`(1).x`, `"abc"[3]`); `(1).toString === Number.prototype.toString`, `(1).constructor === Number`, `(1).__proto__ === Number.prototype`. A method call passes the primitive itself as `this` (`docs/architecture.md`, "Primitive bases"). `Number.prototype.toString(radix)` gives Number::toString of a number receiver (`(19686109595169230000).toString()` is `"19686109595169230000"`, `(-0).toString()` `"0"`, `(1e21).toString()` `"1e+21"`) with `radix` absent, `undefined` or 10; radixes 2..36 also execute, including fractional digits (clj-quickjs-149; see below); `Number.prototype.valueOf()` the number; `String.prototype.toString()` / `.valueOf()` the string; `Number.prototype` and `String.prototype` themselves count as `0` and `""` (`String.prototype.length` is 0); a Number/String wrapper is also a valid receiver for its respective methods (clj-quickjs-72); any other receiver (`let f = (1).toString; f()`) throws a catchable `TypeError`. `Number` and `String` are built-in globals carrying the prototypes. Fail with `UnsupportedOperand`: boolean bases (`Boolean.prototype`, clj-quickjs-39) and the other `Number.prototype`/`String.prototype` methods (clj-quickjs-39; absent methods read `undefined`, so calling one is a `TypeError`). `Number(v)` / `String(v)` and their `new` forms are described below (clj-quickjs-72). Writes: see `o.name = v` |
| `s.length`, `s[i]` on strings (clj-quickjs-142) | primitives and String wrappers expose `length` as the UTF-16 code-unit count and each canonical index below it as a one-code-unit string, in constant time (`"\u{10ffff}".length` is 2 and its index 0 is `"\u{dbff}"`). Indices are read-only, enumerable, non-configurable; `length` is read-only, non-enumerable, non-configurable. Out-of-range and noncanonical keys follow the prototype chain. Strict writes and deletes throw TypeError; sloppy writes do nothing and deletes return false. Wrapper `Object.defineProperty` accepts compatible descriptors through ValidateAndApplyPropertyDescriptor and rejects incompatible ones. Other wrapper properties are ordinary. Own keys sort all indices ascending, then `length`, then other names by creation order. Primitive reflection uses the same properties without allocating a wrapper; `in` still requires an object. |
| `f.call(t, a, b)`, `f.apply(t, list)` (clj-quickjs-102) | `Function.prototype.call` / `apply` (QuickJS `js_function_call`, `js_function_apply`), inherited by every function: call the receiver `f` with `this` `t` (`undefined` when absent) and the arguments `a, b`, or the elements of `list`: none for `undefined`/`null`, otherwise CreateListFromArrayLike (an array or any object: its `length` through ToLength, then its indices `0 .. length - 1` read with the prototype chain, so holes read inherited values; a `length` that is an object fails with `UnsupportedOperand`, ToPrimitive). A strict callee sees `t` as is; a sloppy one only when `t` is an object (else `UnsupportedOperand`, as for `this`). The call replaces the `call`/`apply` call in the same instruction (a tail call): a function object runs in a new task-owned frame, which may yield, wait on `__rt.need`, be collected around and migrate, and its result is the call's; a built-in runs to completion. A receiver that is not callable (`Function.prototype.call.call(1)`, `const c = f.call; c()`) and a `list` that is neither an object nor `undefined`/`null` (`f.apply(t, 1)`) are catchable `TypeError`s; an argument list longer than the remaining value-stack room, or more forwarding hops than the call-depth limit (`call` of `call` of ..., or `apply` through an array that holds itself), is a catchable `StackOverflow` |
| `Reflect.apply(f, t, list)` (clj-quickjs-102) | `f.apply(t, list)` without reading `f.apply` (QuickJS `js_reflect_apply`): `f` must be callable and `list` must be an object (`Reflect.apply(f, t)` and `Reflect.apply(f, t, undefined)` are catchable `TypeError`s) |
| `f.bind(t, a, b)` (clj-quickjs-103) | creates a bound function holding callable `f`, receiver `t` and arguments `a, b`; calls prepend those arguments and use `t`. Binding again preserves the first receiver. `new` unwraps bound targets, ignores bound receivers, and constructs the ordinary target; `instanceof` likewise uses the target. Calls, constructors and native callbacks enter task-owned frames that may suspend and migrate. Non-callable receivers throw `TypeError`; argument expansion and forwarding hops obey VM stack/depth limits. Bound functions have configurable, non-enumerable, read-only `length` (target length minus bound arguments, at least zero) and `name` (`"bound "` plus target name), and no initial own `prototype`. Their operands and metadata are traced; publication succeeds exactly when the target, receiver and arguments are shareable and no mutable own-property record has been added. No new EBNF forms are needed: these are existing method calls, property reads and `new` expressions |
| `Function.prototype`, `Function` (clj-quickjs-102) | `Function.prototype` is a built-in function that accepts any arguments and returns `undefined`, the prototype of every function and built-in function, inheriting from `Object.prototype`; `Function.prototype.constructor === Function`. `Function(...)` and `new Function(...)` are not supported yet (clj-quickjs-111): the call fails with `UnsupportedOperand`, `new` with a `TypeError` |
| `return` (in any function) | returns from that function |
| closures | a function sees the bindings of every enclosing scope, including after they are assigned and after the enclosing call returned; each execution of a block (each loop iteration) makes new bindings |
| `Number(v)`, `String(v)`, `new Number(v)`, `new String(v)` (clj-quickjs-72) | called: ToNumeric with BigInt rounded to binary64 (clj-quickjs-203) / ToString of the argument; no argument gives 0 / `""`, while an explicit `undefined` gives NaN / `"undefined"`. Constructed: a distinct task-owned ordinary object with `[[NumberData]]` / `[[StringData]]`, inheriting from `Number.prototype` / `String.prototype`. Their `valueOf()` / `toString()` accept their respective wrappers or primitives; a prototype lookalike without the internal slot throws a catchable TypeError. The prototypes themselves hold +0 / `""`. `typeof` a wrapper is `"object"`, strict equality uses identity, and `instanceof` follows its prototype. Own ordinary properties work as on literals, and publication rejects wrappers as task-owned. Object arguments use resumable ToPrimitive with number hint for Number and string hint for String. Wrapper equality, addition and other coercing operators call observable valueOf/toString methods through the VM; conversion callbacks may yield, wait, be collected around and migrate. A String wrapper exposes virtual own `length` and code-unit indices (clj-quickjs-142), as described above. Other keys use ordinary storage. Boolean wrappers are deferred; Number predicates and constants are described below (clj-quickjs-147). |
| a function's own name (`compile_function`, or a declaration never assigned) | the running function object (`current_function`), so self-recursion captures nothing |
| `__rt.name(args)` | the private VM operation of that intrinsic (see "Reserved intrinsic binding") |
| Number predicates and constants (clj-quickjs-147) | globals `isNaN(v)` and `isFinite(v)` apply ToNumber, including resumable ToPrimitive with number hint for objects. `Number.isNaN`, `.isFinite`, `.isInteger` and `.isSafeInteger` return false for non-numbers without coercion. Integers must be finite; safe integers additionally have magnitude at most 9007199254740991. Each function has length 1. `Number` owns non-writable, non-enumerable, non-configurable constants `EPSILON` (2^-52), `MIN_VALUE` (smallest positive binary64 subnormal), `MAX_VALUE` (largest finite binary64), `MAX_SAFE_INTEGER` (2^53 - 1), `MIN_SAFE_INTEGER` (-(2^53 - 1)), `NaN`, `POSITIVE_INFINITY`, `NEGATIVE_INFINITY`. |

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

`delete` removes own configurable properties (clj-quickjs-124); each line
below evaluates to `true`:

```js accept
let a = {x: 1, y: 1}; delete a.x && a.x === undefined && delete a.missing && delete a["y"];
let arr = [1, 2]; delete arr[0] && arr.length === 2 && arr[0] === undefined;
let r = false; try { delete null.a; } catch (e) { r = true; } r;
let o = {}; Object.defineProperty(o, "k", {value: 1}); let r = false; try { delete o.k; } catch (e) { r = o.k === 1; } r;
```

`delete` of an identifier is an early error in strict code and deletes
configurable global properties in sloppy code (clj-quickjs-105):

```js reject Invalid 1:16
function f() { delete x; }
```

```js sloppy-accept
delete nothing_here;
function f() { u = 3; } f(); globalThis.u === 3 && delete u && typeof u === "undefined";
u = 5; u === 5;
var v = 1; delete v === false && v === 1;
let w = 1; delete w === false;
delete undefined === false && delete NaN === false && delete Infinity === false;
```

Concise object-literal methods accept literal keys, including contextual
and reserved words (clj-quickjs-126), and computed keys (clj-quickjs-233). They use the literal as their home
object and the call's receiver as `this`. Methods inherit the surrounding
script or function's strictness (clj-quickjs-266); a `"use strict"` directive
in the method body also makes it strict. In strict methods, detached calls
receive `undefined`; in sloppy methods, evaluating `this` without an object
receiver fails with `UnsupportedOperand` until global-object substitution
and primitive wrappers exist (see "Explicit gaps").
`super` remains rejected explicitly (clj-quickjs-138/-112).

Computed property names (clj-quickjs-233) use an AssignmentExpression inside
brackets: `{[key]: value}`, `{[key]() {}}`, `{get [key]() {}}` and
`{set [key](v) {}}`. Generator and async methods accept computed names too;
key expressions use the surrounding function's `yield`/`await` context.
Execution evaluates each key, converts it with ToPropertyKey (hint string),
then evaluates its value, before starting the next property. Conversion can
call JavaScript, yield, wait, collect and migrate. Exceptions from a key or
its conversion stop before its value runs; conversion errors point to `[`.
Data properties are own writable, enumerable and configurable properties;
inherited setters are bypassed, including for a computed `__proto__` key.
Accessors merge their getter/setter halves as literal names do. Methods keep
the literal as home object. Computed methods and anonymous function/arrow
values receive the converted key as their name: symbol descriptions appear
inside brackets (`[d]`, `[]` for an empty description, an empty name when
absent), with `get ` or `set ` prefixes for accessors. Named functions and
other expressions retain their names. Rejected forms include `{[key]}`
shorthand and unparenthesized comma expressions inside the brackets.
Explicit gaps remain object spread, `super` and async generators. Key conversion
uses the shared OrdinaryToPrimitive path; `Symbol.toPrimitive` protocol
dispatch remains the existing conversion gap described below.

```js accept
let k = "x"; ({[k]: 1}).x === 1;
({[1 + 1]: "two"})["2"] === "two";
let s = Symbol("d"); let o = {[s]() { return this.v; }, v: 7}; o[s]() === 7 && o[s].name === "[d]" && Object.keys(o).length === 1;
let o = {get ["x"]() { return this.v; }, set ["x"](v) { this.v = v; }}; o.x = 7; o.x === 7;
```

```js reject UnexpectedToken 1:8
({[key]});
```

```js accept
({b() { return this._b; }, _b: {c: 42}}).b().c === 42;
let o = {m() { return this; }, get() { return 1; }, static() { return 42; }, if() { return 2; }}; o.m() === o && o.get() === 1 && o.static() === 42 && o.if() === 2 && o.m.prototype === undefined;
({"s"(a = 42) { return a; }, 1() {}}).s() === 42;
```

```js sloppy-accept
({m() { return 010; }}).m() === 8;
```

```js reject InvalidToken 1:16
({m() { return 010; }}).m();
```

```js sloppy-reject InvalidToken 1:30
({m() { "use strict"; return 010; }});
```

```js reject Unsupported 1:16
({m() { return super.x; }});
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

Array patterns bind names in declarations, ordinary/arrow parameters and
catch clauses, and assign identifiers or properties (clj-quickjs-131):

```js accept
let a = 1, b = 2;
[a, b] = [b, a];
let [first, , third = 3, ...tail] = [1, 2, undefined, 4, 5];
let [[nested]] = [[1]];
function sum([x, y]) { return x + y; }
(([x, y]) => x + y)([1, 2]);
try { throw [7]; } catch ([error]) { error; }
```

Invalid targets and a rest element followed by another element are early errors:

```js reject Invalid 1:2
[a + 1] = x;
```

```js reject UnexpectedToken 1:10
let [...r, b] = x;
```

Execution uses GetIterator, including custom iterators and strings. An elision
steps without reading `value`; exhaustion supplies `undefined` for subsequent
elements. Defaults run only for `undefined`, and rest collects a fresh array.
A pattern which stops before exhaustion calls `return` once. A default or
assignment failure also closes a live iterator, preserving the original throw;
failures in `next`, `done` or `value` do not close it. All getters, iterator
methods, defaults and setters can suspend and resume. Non-iterables throw
`TypeError`. Array patterns require declaration initializers.
Rest parameters and patterns in iteration heads remain explicit gaps.

Object patterns (clj-quickjs-132) share these declaration, assignment,
parameter and catch contexts, and compose with nested array patterns.
Shorthands, renamed properties, undefined-only defaults, computed keys and a
final rest property execute:

```js accept
let {a, b: c, d = 4, ...r} = {a: 1, b: 2, e: 5};
let o = {}; ({x: o.y} = {x: 3});
let {['x']: [{y = 7}]} = {x: [{}]};
(({a, b}) => a + b)({a: 1, b: 2});
try { throw {message: 'ok'}; } catch ({message}) { message; }
```

Execution first rejects null and undefined with TypeError, including empty
patterns; other primitives use ordinary property reads (Number has no own
`length`, so `let {length} = 1` binds undefined). Each computed source key is
converted once before preparing its assignment target and performing Get.
Computed-key conversion/read errors use the opening bracket's source position.
Escaped reserved words are valid property names with `:`, but cannot be
shorthand binding or assignment names.
Assignment target computed keys follow the array-pattern timing: expression
evaluation precedes Get/defaults, conversion occurs at PutElem. Getters,
conversions, defaults and setters remain resumable. Rest copies remaining own
enumerable string and symbol properties into a fresh ordinary object in own-key
order, rechecking descriptors before each Get; it defines data properties and
does not invoke inherited setters. Object pattern declarations need an
initializer. Object rest bindings require an identifier; assignment rest may
target a property. Rest must be last, without a default or trailing comma.

```js reject Invalid 1:6
({a: 1} = x);
```

```js reject UnexpectedToken 1:10
let {...r,} = x;
```

Explicit gaps: object spread,
rest parameters and destructuring iteration heads remain rejected.

Arrays inherit from `Array.prototype`, which supports `push`, `join` and
`toString`. The latter calls the receiver's `join` method:

```js accept
[1, 2].toString() === "1,2";
```

Functions take own properties, and a `function` declaration or
expression has a `prototype` object created on first read, whose
`constructor` is the function (clj-quickjs-125); each line below
evaluates to `true`:

```js accept
function F() {} F.prototype.constructor === F && F.prototype === F.prototype;
let f = function () {}; f.x = 1; f.x === 1 && f.y === undefined && (() => 1).prototype === undefined;
```

Functions inherit `call` and `apply` from `Function.prototype`, and
`Reflect.apply` does the same as `apply` (clj-quickjs-102); each line below
evaluates to `true`:

```js accept
function add(a, b) { return a + b; } add.call(null, 1, 2) === 3 && add.apply(null, [1, 2]) === 3 && Reflect.apply(add, null, [1, 2]) === 3;
function self() { return this; } self.call(7) === 7 && self.apply(8) === 8 && self.call() === undefined && Object.getPrototypeOf(self) === Function.prototype;
let r = false; try { Reflect.apply(function () {}, null, undefined); } catch (e) { r = true; } r;
```

Bound functions (clj-quickjs-103) use the existing call and constructor grammar:

```js accept
function f(a, b) { return this + a + b; } const g = f.bind(1, 2); g(3) === 6 && g.length === 1 && g.name === 'bound f';
function C(x) { this.x = x; } const B = C.bind(null, 7); const o = new B(); o.x === 7 && o instanceof B && o instanceof C;
let r = false; try { Function.prototype.bind.call(1); } catch (e) { r = true; } r;
```

The other missing methods (`toString`, `arguments`, `caller`) still fail
explicitly (`UnsupportedOperand`). Function metadata (clj-quickjs-101)
uses the existing function, initializer, assignment and property-access
EBNF; no new syntax or rejected forms change. Every function owns `length`
and `name`: non-writable, non-enumerable, configurable data properties.
`length` counts parameters before the first default. `name` is the declared
name, or the binding/parameter name for an anonymous function or arrow
directly initializing it or assigned to an identifier. An anonymous
function/arrow directly used as an object literal's property value infers
that property's string key (except the special `__proto__: value` form).
Other anonymous functions have the empty name. Parentheses preserve direct
inference; comma and conditional expressions do not. Array pattern parameters
count as one parameter; a top-level default still ends the `length` count.
Rest parameters remain rejected. Object patterns also count as one parameter. Concise method names are their
property's string key (clj-quickjs-126).

Reads, `in`, metadata descriptors, and `Object.getOwnPropertyNames` do not
attach a function's property record, so a shareable closure stays publishable.
Initial key order is `length`, `name`, then `prototype` for constructors.
The first property change copies metadata into the task-owned record and
materializes a constructor's prototype before user properties. Deletion
and redefinition then follow ordinary descriptors and creation order;
deleting metadata exposes `Function.prototype.length` (0) or `.name` ("").
`bind` uses the target's current own numeric length (zero if absent or
non-numeric), subtracts bound arguments and clamps at zero, and prefixes
the current string name with `bound ` (an empty suffix for non-strings).
Rejected assignments to an untouched function's read-only metadata
throw in strict code and do nothing in sloppy code without preventing
publication. Publishing implicitly freezes the function, and its
metadata descriptors then report non-configurable properties.

```js accept
(function (a, b) {}).length === 2 && ((a, b = 1, c) => {}).length === 1 && (() => {}).length === 0;
function f() {} const g = () => 1; let h; h = function () {}; f.name === "f" && g.name === "g" && h.name === "h" && (function () {}).name === "";
const f = () => 0; const d = Object.getOwnPropertyDescriptor(f, "name"); !d.writable && !d.enumerable && d.configurable && Object.getOwnPropertyNames(f)[0] === "length";
function f() {} delete f.name && f.name === "" && Object.getOwnPropertyDescriptor(f, "name") === undefined;
```

A body `let`/`const` cannot repeat a parameter, with or without defaults:

```js compile-reject Invalid 1:25
function f(a = 1) { let a; }
```

```js accept
function f(a, b = () => arguments) { return b; }
const args = f(1)(); args.length === 1 && args[0] === 1;
const top = () => arguments; typeof arguments === "undefined";
```

```js reject ReservedWord 1:1
arguments = 1;
```

```js sloppy-accept
function f() { var arguments; return () => arguments; } f(1, 2)();
```

Errors:

| Situation | When | Reported as |
|-----------|------|-------------|
| A name declared twice in one block, script, function body or `switch` case block (including a `let`/`const` that repeats a parameter, also with default values, and a function declaration that clashes with a `let`/`const` or repeats one in a block), other than the var-scoped repetitions above | compile time | `SyntaxError`, kind `Invalid`, at the second declaration |
| A `var` whose name a `let`, `const` or block-level function declaration binds in the same body (`let a; var a;`) or in a block enclosing the `var` within the same function (`{ let a; { var a; } }`), or a `for` head binding of an enclosing loop | compile time | `SyntaxError`, kind `Invalid`, at the `var`'s name |
| In `compile_function`, assignment to the entry function's own name | compile time | `SyntaxError`, kind `Unsupported`, at the form |
| Misuse of the reserved intrinsic binding `__rt` | compile time | see "Reserved intrinsic binding" |
| More than 65,535 live bindings, 65,536 distinct constants, 65,536 functions in one function, or 65,535 call arguments or captured bindings | compile time | `SyntaxError`, kind `TooLarge` |
| Reading, incrementing or decrementing a name with no binding (a name absent from the global object), except as the operand of `typeof`; assigning it in strict code | run time, only if reached, after the assigned value is evaluated (a compound assignment reads first, so it fails before evaluating its right operand in either mode) | `VmErrorKind::ReferenceError`: "x is not defined" |
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
| a member access `o.name`, read or write (clj-quickjs-114) | the `.` (`[` for a computed access); for `o.p++` and `o[k]--` (clj-quickjs-76) and `o.p op= v` and `o[k] op= v` (clj-quickjs-260) the read, the base check and the key conversion and the write are reported there, the number conversion and the arithmetic at the `++`/`--`, the operator at the `op=` |
| a reference to, assignment to or update of a binding (ReferenceError, temporal dead zone, `const` TypeError) | the identifier |

For example (each script compiles; the comment gives where it fails at run
time, as `tests/js_source.rs` `runtime_error_positions_follow_operator_tokens`
checks):

```js accept
let f = Object.create(null);
  1 + 2 * f;          // TypeError at 2:9, the `*`
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

For-of uses IteratorClose on body failures, returns, breaks and jumps leaving
the loop. An exception from the iterator's `return` getter/call, or its
non-object result, preserves a pending throw; otherwise it replaces the
completion (a non-object result throws TypeError). Exceptions from `next`,
`done` or `value` do not close. Consumer cancellation closes even while
waiting in `next`, and remains cancellation under the existing cleanup rules.
Waiting itself never closes an iterator.

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
| `throw Error("boom")` or `throw new Error("boom")` (clj-quickjs-228) | the task-owned Error instance with ordinary properties | `VmErrorKind::Thrown`, "uncaught exception: Error: boom", at the last `throw`; name/message are read from data properties only, with no user code; accessors or object-valued fields fall back to "an object" |
| `throw new TypeError("boom")` and the other NativeError constructors (clj-quickjs-229) | the task-owned instance, also an `instanceof Error` | `VmErrorKind::Thrown`, "uncaught exception: TypeError: boom" (using the instance's name and message as above) |
| ReferenceError, TypeError, RangeError, SyntaxError, StackOverflow above (clj-quickjs-230) | a fresh task-owned instance of the matching NativeError constructor (`InternalError` with message `"stack overflow"` for StackOverflow), also an `instanceof Error`, with writable, configurable, non-enumerable own `message` and `stack` | the original error, at the failing instruction, also when the object is rethrown |
| a failed or cancelled cell read by `__rt.need` (clj-quickjs-27, -230) | a fresh task-owned Error instance; failure has a writable, configurable, non-enumerable own `cause`, the producer's published error value by identity; cancellation has no own `cause` (`e.cause === undefined`) | `DependencyFailed(key)` / `DependencyCancelled(key)` at the `need`, also when rethrown |
| any other run-time failure (`UnsupportedOperand`, collection, builder, atom or binding-cell contract violations, host errors, out of memory) | not catchable: the task fails at once without running catch or finally blocks | that failure |

Synthetic fixtures, not light-cljs output:

```js accept
let refOK = false; try { nope; } catch (e) { refOK = e instanceof ReferenceError && e instanceof Error && e.message === 'nope is not defined' && e.toString() === 'ReferenceError: nope is not defined'; } refOK;
let typeOK = false; try { null(); } catch (e) { typeOK = e instanceof TypeError && e.name === 'TypeError'; } typeOK;
let rangeOK = false; try { let a = []; a.length = -1; } catch (e) { rangeOK = e instanceof RangeError && e.message === 'invalid array length'; } rangeOK;
```

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
  map keys execute. Object operands use resumable OrdinaryToPrimitive
  (clj-quickjs-70). `typeof`, `length`, indexing, the code-unit methods
  (clj-quickjs-143) and `toString`/`valueOf` (clj-quickjs-73) execute;
  additional string methods remain deferred. Untagged template literals execute (clj-quickjs-57);
  tagged templates and `String.raw` execute (clj-quickjs-58).
- **Error publication follows ordinary object ownership.** Engine-raised and
  explicitly constructed Error instances are task-owned, including after
  `Object.freeze`. Publishing one as a dependency result or failure, or
  publishing a closure that captures one, is rejected by the transitive
  publication check. Returned host roots may inspect their internal diagnostics.
  Property writes, deletion and prototype changes do not alter an engine
  error's original `VmError` or original dependency cause: `Rooted::error`
  and `Rooted::error_cause` read that internal slot, and rethrowing preserves
  its original kind, position and call chain. Explicitly constructed instances
  (`new TypeError(...)`, etc.) have no engine diagnostic; these APIs return
  `None` for them. Error objects are allocated only when a handler could
  observe them; allocation failure is uncatchable `OutOfMemory`.
- **Conversion gaps.** Symbols are primitives (clj-quickjs-127);
  `Symbol.toPrimitive` protocol dispatch (clj-quickjs-128) remains deferred.
  Number radix formatting supports every radix 2..36, including fractions
  (clj-quickjs-149); localized and fixed-precision Number methods remain deferred.
  `Function.prototype.toString` is absent: conversion uses the available
  Object.prototype default (`"[object Function]"`) until that method exists.
  Array conversion uses its dynamic `join` method (clj-quickjs-156).
  Boolean receivers of the generic array string methods still fail explicitly
  with `UnsupportedOperand` until Boolean wrappers exist.
  Native runtime collections have no conversion methods and throw TypeError.
  Leaf built-ins still reject object arguments where they require a
  conversion (descriptor keys, array lengths and apply's argument-list
  length); the operator and property-access paths below are resumable.
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
  start, clj-quickjs-81; a `for`-`in` head's binding is in its temporal
  dead zone in the object expression and initialized at the start of each
  iteration, before the body, and `continue` jumps back to before that
  initialization, clj-quickjs-85); a `break` or `continue` through finally
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
  Script-level `let` bindings use cells even without a statically visible
  assignment: indirect eval can update them dynamically (clj-quickjs-278).
  Bindings visible to a syntactic direct eval, including enclosing functions'
  bindings and immutable `const`s, also use cells. A closure over one cannot
  be published, even if that eval call is never executed (clj-quickjs-107).
  Functions without direct eval in their lexical subtree retain the rules above.
- **Function properties are partial** (clj-quickjs-125). Functions take
  own properties, a lazy `prototype` and `Function.prototype`'s `call`
  and `apply` (clj-quickjs-102), and `bind` (clj-quickjs-103), but ordinary
  functions expose their own `length` and `name` (clj-quickjs-101).
  `toString`, `arguments`, `caller` still fail with `UnsupportedOperand`. A
  function with a property record (or a read `prototype`) cannot be
  published, and a published function cannot get any (`TypeError`) or
  create its `prototype` (`UnsupportedOperand`), so `new` on it fails too. No
  rest parameters. Generators execute through detachable activations
  (clj-quickjs-243). Arrows capture their enclosing
  non-arrow function's receiver, or the script global object.

### Accessor execution (clj-quickjs-120)

`{get z() { return this.x; }, set z(v) { this.x = v; }}` creates an
enumerable, configurable accessor. Getters take zero parameters and setters
exactly one (which may have a default). Both own `this` and `arguments`,
inherit surrounding strictness (clj-quickjs-266), have a home object, cannot be constructed, have
no own `prototype`, and have names `"get z"` / `"set z"`. Identifier,
reserved-word (including escaped reserved words), string and numeric names work. `get` and `set` remain ordinary
keys in `{get: 1}`, `{get, set}` and `{get() {}, set() {}}`.

A later getter or setter preserves the other half of an accessor. A data
definition replaces both halves; an accessor after a data definition starts
with its other half undefined. `__proto__` accessor definitions create
properties without setting the object's prototype.

Reads call a present getter with the original receiver; absent getters
return undefined. Writes call a present setter with that receiver and the
assigned value, then return the assigned value. Inheritance preserves the
receiver. A missing setter rejects the write (TypeError in strict code,
ignored in sloppy code). Callback exceptions propagate normally.

`Object.defineProperty` supports accessor descriptors and data/accessor
conversions of configurable properties. Non-configurable properties cannot
change kind or accessor identities (SameValue). Partial and generic updates
preserve absent fields. ToPropertyDescriptor reads fields in specification
order and runs each field getter resumably. Object keys use resumable
ToPrimitive with string hint. `Object.getOwnPropertyDescriptor` returns
`get`, `set`, `enumerable`, `configurable` for accessors. Reflection, `in`,
key enumeration and `delete` do not invoke getters. Freeze preserves
accessors and removes configurability. Array indices support the same
descriptors, including length growth and deletion during length shrinkage.

Accessor calls in ordinary reads/writes, conversion-method lookup,
descriptor fields, `instanceof`, `bind` metadata, `apply` argument lists
and `push` length/index operations can yield, wait and migrate. `apply`
and `push` also convert object-valued lengths resumably. All pending values
are traced and cancellation discards the continuations. `super`, async and generator accessors remain unsupported. The stored
built-in `Object.prototype.__proto__` accessor uses the same call path
(clj-quickjs-261).

```js accept
let o = {get z() { return this.x; }, set z(v) { this.x = v; }};
o.z = 7; o.z === 7;
let d = Object.getOwnPropertyDescriptor(o, "z"); d.get.name === "get z";
({get: 1, set: 2, get a() { return this.get; }}).a === 1;
({get \u0069f() { return 1; }}).if === 1;
```

```js reject Invalid 1:3
({get a(x) {}});
```

```js reject Invalid 1:3
({set a() {}});
```

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

`new` on ordinary functions (clj-quickjs-71); each line below evaluates to
`true`:

```js accept
function F(x) { this.x = x; } const b = new F(2); b.x === 2 && (new F).x === undefined && new F(1) !== new F(1);
function* G() {} let caught = false; try { new G(); } catch(e) { caught = e instanceof TypeError && e.message === 'G is not a constructor'; } caught;
let o = {m() {}}; let caught = false; try { new o.m(); } catch(e) { caught = e instanceof TypeError && e.message === 'o.m is not a constructor'; } caught;
function G() { this.y = 1; return {y: 2}; } function H() { this.z = 1; return 5; } new G().y === 2 && new H().z === 1;
function P(v) { this.v = v; } P.prototype.twice = function () { return this.v * 2; }; new P(3).twice() === 6 && new P(3).constructor === P;
let r = false; try { new Object(); } catch (e) { r = true; } r;
```

```js reject Unsupported 1:23
function f() { return new.target; }
```

Data property attributes (clj-quickjs-119); each line below evaluates to
`true`:

```js accept
let o = {}; Object.defineProperty(o, "k", {value: 1}) === o && Object.getOwnPropertyDescriptor(o, "k").writable === false;
let p = {}; Object.defineProperty(p, "k", {value: 1}); let r = false; try { p.k = 2; } catch (e) { r = p.k === 1; } r;
let d = Object.getOwnPropertyDescriptor({x: 1}, "x"); d.writable && d.enumerable && d.configurable && Object.getOwnPropertyDescriptor({}, "x") === undefined;
```

Prototypes (clj-quickjs-121); each line below evaluates to `true`:

```js accept
let a = {x: 1}; let b = Object.create(a); Object.getPrototypeOf(b) === a && b.x === 1 && Object.getPrototypeOf(a) === Object.prototype;
let c = {}; let e = Object.create(c); let r1 = false; try { Object.setPrototypeOf(c, e); } catch (err) { r1 = true; } r1;
let n = Object.create(null); n.toString === undefined && Object.getPrototypeOf(Object.prototype) === null;
let q = {y: 2}; let s = {__proto__: q}; let t = {}; t.__proto__ = q; s.y === 2 && t.__proto__ === q && ({}).__proto__ === Object.prototype;
```

Own-property keys (clj-quickjs-123); each line below evaluates to `true`:

```js accept
let k = Object.keys({b: 1, a: 2, 1: 3, 0: 4}); k.length === 4 && k[0] === "0" && k[1] === "1" && k[2] === "b" && k[3] === "a";
let o = {a: 1}; Object.defineProperty(o, "h", {value: 2}); Object.keys(o).length === 1 && Object.getOwnPropertyNames(o)[1] === "h";
let n = Object.getOwnPropertyNames([ , 1]); n.length === 2 && n[0] === "1" && n[1] === "length";
```

```js reject Invalid 2:17
// Only one `__proto__: v` may set a literal's prototype.
({__proto__: 1, __proto__: 2});
```

Extensibility and freezing (clj-quickjs-122); each line below evaluates
to `true`:

```js accept
let a = {x: 1}; Object.preventExtensions(a) === a && !Object.isExtensible(a) && Object.isExtensible({});
let b = {x: 1}; Object.preventExtensions(b); let r = false; try { b.y = 2; } catch (e) { r = b.y === undefined; } r;
let f = Object.freeze({x: 1}); let s = false; try { f.x = 2; } catch (e) { s = f.x === 1; } s && Object.isFrozen(f) && !Object.isFrozen({});
let q = Object.freeze([1, 2]); let t = false; try { q[0] = 3; } catch (e) { t = q[0] === 1; } t && Object.isFrozen(q);
Object.isExtensible(1) === false && Object.preventExtensions(1) === 1 && Object.isFrozen(__rt.vector());
```

`instanceof` walks the prototype chain (clj-quickjs-75); each line below
evaluates to `true`:

```js accept
function F() {} function G() {} G.prototype = new F(); new G() instanceof F && new G() instanceof G && !(new F() instanceof G);
function F() {} ({}) instanceof Object && !(1 instanceof F) && !(Object.create(null) instanceof Object) && Object.create(F.prototype) instanceof F;
let r = false; try { ({}) instanceof {}; } catch (e) { r = true; } r;
```

`in` looks a key up through the prototype chain (clj-quickjs-74); each line
below evaluates to `true`:

```js accept
'x' in {x: undefined} && 'toString' in {} && !('y' in {x: 2}) && 1 in {1: 0};
0 in [1] && !(1 in [1]) && !('toString' in Object.create(null));
let r = false; try { 'x' in 1; } catch (e) { r = true; } r;
```

Numbers and strings read properties and call methods through
`Number.prototype` and `String.prototype` (clj-quickjs-73); each line below
evaluates to `true`:

```js accept
(19686109595169230000).toString() === "19686109595169230000" && (1.5).toString() === "1.5" && (1).valueOf() === 1;
"abc".toString() === "abc" && (1).toString === Number.prototype.toString && (1).constructor === Number;
let r = 0; try { null.toString(); } catch (e) { r = 1; } try { (1).x = 2; } catch (e) { r = r + 10; } r === 11;
```

A string has its own `length` and one read-only property per code-unit
index (clj-quickjs-142); each line below evaluates to `true`:

```js accept
"abc".length === 3 && "abc"[1] === "b" && "abc"["1"] === "b" && "abc"[3] === undefined;
"\u{10ffff}".length === 2 && "\u{10ffff}"[0] === "\u{dbff}" && Object.getOwnPropertyNames("ab")[2] === "length";
```

Script and arrow receivers:

```js accept
let self = this; self === globalThis;
```

```js accept
function f() { return () => this; } f.call(7).call(8) === 7;
function F() { this.get = () => () => this; } const o = new F(); o.get()() === o;
function f() { return (x = this) => x; } f.call(7).bind(8)() === 7;
```

```js accept
const f = () => this; f() === globalThis;
```

```js reject ReservedWord 1:3
({if});
```

```js accept
let key = "x"; ({[key]: 1}).x === 1;
```

```js accept
({get x() { return 1; }});
```

```js accept
let o = {f: function () { return 1; }};
Object = o.f; Object() === 1;
```

```js accept
let r = /ab+c/g;
```

```js accept
x = /abc\(/; x =/abc\(/;
f(/[/]/g, /\//, b = /abc\(/);
[b =/abc\(/]; x = /=/;
if (a) /re/.test(b);
a
/ b / g;
```

```js accept
/x/gimsuyd; /x/v; !/x/; a ? /x/ : /y/; a && /x/;
function f() { return /x/; }
throw /x/;
`${/}/}`;
(x = /[)]/) => x;
```

```js reject Invalid 1:1
/a/gg;
```

```js reject Invalid 1:1
/a/x;
```

```js reject Invalid 1:1
/a/uv;
```

```js reject UnexpectedEnd 1:1
/abc
```

```js reject InvalidToken 1:1
/a
/
```

```js accept
class Point {}
```

```js accept
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

```js accept
x = f`template`;
```

```js accept
tag
`a${x}b``\unicode`.member;
new tag`constructor`(1);
```

```js reject Invalid 1:5
a?.b`c`;
```

```js reject InvalidToken 1:7
x = `a\1`;
```

```js accept
// `++`/`--` on properties (clj-quickjs-76): the base and the key once.
let o = {n: 0}, a = [1, 2], i = 0;
o.n++; --o['n']; a[i++]++; ++(a[i]);
```

```js accept
// Compound assignment on properties (clj-quickjs-260): the base and the
// key once, then the read, the right operand, the operator, the write.
let o = {n: 1}, a = [1, 2], i = 0;
o.n += 2; a[i++] *= 3; (o['n']) **= 2; a[i] <<= o.n;
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

```js reject Unsupported 1:5
a.b &&= 1;
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

```js accept
for (const x of xs) {}
```

```js accept
for (k in o) {}
for (let k in o) { if (k === "x") continue; }
for (const k in o) break;
```

```js reject Invalid 1:16
for (let k = 1 in o) {}
```

```js reject Invalid 1:6
for (1 in o) {}
```

```js reject Unsupported 1:6
for (a.b in o) {}
```

```js accept
// `var` heads (clj-quickjs-89): one function binding, readable before and
// after the loop; a for-in `var` head keeps the last key.
function g() {
  const before = i;
  for (var i = 0, j; i < 3; i++) j = i;
  for (var k in {x: 1, y: 2}) {}
  return before === undefined && i === 3 && j === 2 && k === "y";
}
```

```js reject Invalid 1:16
for (var k = 1 in o) {}
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

## Generator and async syntax examples (clj-quickjs-54)

These synthetic fixtures cover executable generators, delegation and async functions; async generators remain deferred.

```js accept
function* g() { yield 1; yield* xs; }
```

```js accept
async function f() { await 1; }
```

```js compile-reject Unsupported 1:1
async function* g() { yield await 1; }
```

```js accept
let g = function* () { yield; };
```

```js accept
let f = async function () { await 1; };
```

```js accept
async x => await x
```

```js accept
async (x) => { return await x; }
```

```js accept
({*m() { yield 1; }})
```

```js accept
({async m() { await 1; }})
```

```js compile-reject Unsupported 1:3
({async *m() { yield await 1; }})
```

```js accept
async
x => x
```

```js accept
function* g() { yield
1 }
```

```js reject ReservedWord 1:1
yield 1;
```

```js reject Unsupported 1:1
await 1;
```

```js reject Invalid 1:17
function* g(x = yield 1) {}
```

```js reject Invalid 1:22
async function f(x = await 1) {}
```

```js reject UnexpectedToken 2:1
async (x)
=> x
```

## References

Structure consulted (no code adapted): `research/quickjs/quickjs.c` at
revision 7d1c13c5d8f8 (`VERSION` 2026-06-04): `next_token`,
`js_parse_expr_binary`, `js_parse_assign_expr`, `js_parse_statement`,
`js_parse_expect_semi`; and the ASI cases of `test_parse_semicolon` in
`research/quickjs/tests/test_language.js` (its `yield`/`await` cases
are adapted in `generator_and_async_syntax`). For arrow functions (clj-quickjs-53):
`js_parse_skip_parens_token` (deciding that a parenthesized list is an
arrow's parameters by what follows its `)`), `js_parse_assign_expr2`
(arrows only at the start of an AssignmentExpression) and
`js_parse_function_decl2` (concise and block arrow bodies), plus the
`test_parse_arrow_function` cases of `test_language.js` (adapted in
`frontend::tests::arrow_functions_and_asi` and
`tests/js_functions.rs` `arrow_function_values`, with the QuickJS notice;
its `async` arrow cases are adapted in `generator_and_async_syntax`). For functions and closures (clj-quickjs-21):
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
`update_expressions_on_bindings`, with the notice. Its member groups
(clj-quickjs-76) are adapted in `update_expressions_on_properties`, with the
notice; `to_propkey2` and `dup2` in `js_parse_postfix_expr` show the
reference evaluated once. For loose
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
For `instanceof` (clj-quickjs-75): `js_operator_instanceof`, `JS_IsInstanceOf`
and `JS_OrdinaryIsInstanceOf` (their TypeError messages "invalid
'instanceof' right operand" and "operand 'prototype' property is not an
object") and `OP_instanceof`; the `a instanceof Object` case of `test_op2`
in `test_language.js` is adapted in `tests/js_source.rs`
`instanceof_operator` (notice on
`loose_equality_with_a_boolean_left_operand`).
For `delete` (clj-quickjs-124): `js_parse_delete` (an identifier operand is
"cannot delete a direct reference in strict mode"; any other non-reference
operand is dropped and gives `true`), `CASE(OP_delete)`,
`JS_DeleteProperty` and `delete_property`; the `a = {x: 1, y: 1}`,
`"abc"[100]` and `null.a` rows of `test_delete` (line 222) in
`test_language.js` are adapted in `tests/js_source.rs`
`delete_property_references` (notice on
`object_literals_and_named_properties`).
For `in` (clj-quickjs-74): `js_operator_in` and `OP_in` (TypeError for a
non-object right operand before the key conversion, then
`JS_HasProperty`); the `"x" in a` cases of `test_op2` in
`test_language.js` are adapted in `tests/js_source.rs` `in_operator`
(notice on `loose_equality_with_a_boolean_left_operand`).
For properties of primitives (clj-quickjs-73): `JS_GetPrototypePrimitive`
(the wrapper's prototype for a property read on a primitive),
`js_thisNumberValue`, `js_number_valueOf`, `js_number_toString` and
`js_thisStringValue`; the `(19686109595169230000).toString()` case of
`test_cvt` in `test_language.js` is adapted in `tests/js_source.rs`
`methods_on_primitive_bases` (notice on `addition_concatenates_strings`).
For `var` (clj-quickjs-94): `js_parse_var` (declarators; an initializer
lowered as an assignment to the resolved name), `define_var` (a `var`
reuses the function's one variable of that name and is an error inside a
lexical declaration's scope, except a catch parameter's) and
`instantiate_hoisted_definitions` (function declarations of the body
assigned at entry, so the last of the same name wins).
For `var` heads of `for` and for-in statements (clj-quickjs-89): the
`TOK_FOR` case of `js_parse_statement_or_decl` (a `var` head declared with
`js_parse_var` without the `in` flag) and `js_parse_for_in_of` ("a
declaration in the head of a for-in loop can't have an initializer"
outside the non-strict `var` case). The second group of `test_for` and the
seventh group of `test_for_in` of `research/quickjs/tests/test_loop.js`
are adapted in `tests/js_source.rs` `var_heads_in_for_loops` with the
notice.
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

## Array length changes (clj-quickjs-154)

Execution supports assigning `a.length` or `a['length']`, and defining the
length with `Object.defineProperty(a, 'length', descriptor)`. Primitive
values convert with ToNumber and must equal their ToUint32 conversion:
finite integers from 0 through 4294967295, including numeric strings and
negative zero (stored as positive zero). Invalid values raise a catchable
`VmErrorKind::RangeError` and leave the array unchanged. Growing changes
only the length, leaving holes; shrinking deletes own elements from the
highest index down. A non-configurable element stops deletion, leaves
length at that index plus one and keeps lower elements. This rejected
assignment throws a catchable TypeError in strict code and is ignored in
sloppy code; the partial truncation remains in both modes.

`Object.defineProperty` always throws on rejection. Descriptor attributes
are validated before truncation. If `writable: false` accompanies a shrink,
length becomes read-only even when a non-configurable element blocks it.
Later assignments to read-only length fail (including same-value writes),
as do new element writes at or beyond it. Defining the same normalized
length value is still allowed. Sparse deletion visits stored elements,
so truncating from 4294967295 does not scan billions of holes.

Explicit gaps: an object length value needs resumable ToPrimitive
(clj-quickjs-70) and fails explicitly with `UnsupportedOperand`. Invalid
lengths raise ordinary RangeError instances with `name`, `message`, `stack`
and `instanceof RangeError` support (clj-quickjs-230).
Synthetic fixtures, not light-cljs output:

```js accept
let a = [1,2,3]; a.length = '2'; a.length === 2 && a[2] === undefined;
let a = []; a[4294967294] = 1; a.length = 0; a.length === 0;
let a = [1,2]; let caught = false; try { a.length = 1.5; } catch(e) { caught = true; } caught && a.length === 2;
```

## Array push (clj-quickjs-155)

Execution: `Array.prototype.push(...items)` appends at ToLength(`this.length`),
sets the new length, and returns it. It is generic over array-like objects:
primitive lengths are converted to numbers, truncated and clamped to
`[0, 2^53 - 1]`; a result beyond that limit throws a caught `TypeError`
before writing any item. Every Set throws on failure even in sloppy code,
including the final length Set when no items were supplied. Frozen arrays,
non-extensible receivers that need a new index, and read-only lengths throw
`TypeError`. Earlier successful writes remain if a later Set fails; an
array's final length above `2^32 - 1` throws `RangeError` after the preceding
index writes. Another task's array fails with `ForeignObject`.

The global `Array` constructor exposes its prototype (clj-quickjs-153).
`push.length` is 1, `push.name` is `"push"`,
and the method is writable, non-enumerable and configurable (intrinsics
remain immutable under the runtime's sharing rule).

```js accept
let a = [1, 2]; a.push(3, 4) === 4 && a[2] === 3 && a[3] === 4 && a.length === 4;
[].push() === 0 && Array.prototype.push.length === 1;
let o = {length: 1}; Array.prototype.push.call(o, "x") === 2 && o[1] === "x";
```

Explicit gaps: primitive receivers require wrappers (clj-quickjs-72), and
object-valued lengths require resumable ToPrimitive (clj-quickjs-70);
`push` fails explicitly with `UnsupportedOperand` for these cases. Null and
undefined receivers always throw caught `TypeError`.

### Number and String constructor examples (clj-quickjs-72)

Synthetic fixtures, not light-cljs output. Each script evaluates to true:

```js accept
Number('0x10') === 16 && Number() === 0 && String(null) === 'null' && String(1e21) === '1e+21';
let n = new Number(5); typeof n === 'object' && n !== 5 && n + 1 === 6 && n instanceof Number;
let s = new String('a'); s + "b" === "ab" && s.toString() === 'a' && s instanceof String;
(new Number()).valueOf() === 0 && (new String()).valueOf() === '';
```

Wrapper equality, addition and object arguments use resumable
OrdinaryToPrimitive, preserving overridden conversion methods.

```js accept
new Number(1) == 1 && 2 == new Number(2) && new String('abc') == 'abc';
Number({valueOf: function () { return 7; }}) === 7 && String({toString: function () { return 's'; }}) === 's';
```

### String code-unit access (clj-quickjs-143)

`String.prototype.charAt(pos)`, `charCodeAt(pos)` and `codePointAt(pos)`
require a non-null, non-undefined receiver, convert receivers to
strings, and use ToIntegerOrInfinity on positions (absent,
undefined and NaN become zero; fractions truncate toward zero). Positions
outside the UTF-16 length give `""`, NaN and undefined respectively.
`charAt` returns exactly one unit, including a lone surrogate; `codePointAt`
combines a high surrogate with the immediately following low surrogate,
otherwise it returns the unit. `String.prototype` itself acts as `""`.
Null/undefined receivers throw catchable TypeError.

`String.fromCharCode(...units)` applies ToNumber and ToUint16 to each
argument: fractions truncate, integers wrap modulo 65536, and
NaN/infinities become zero. `String.fromCodePoint(...points)` applies
ToNumber and requires finite integers in 0..0x10FFFF; otherwise it throws
catchable RangeError. Astral points produce two units; surrogate code
points produce single units. Both accept zero arguments (`""`) and work
through `call`/`apply`. All five functions have `length` 1 and the usual
non-enumerable method properties on the realm intrinsics. Synthetic
fixtures, not light-cljs output:

```js accept
'abc'.charAt(1.9) === 'b' && 'abc'.charAt(-1) === '';
'\u{10ffff}'.codePointAt(0) === 0x10ffff;
String.fromCharCode.apply(null, [65, 66, 67]) === 'ABC';
String.fromCharCode(0xd800).length === 1 && String.fromCharCode(0xd800).charCodeAt(0) === 0xd800;
String.fromCodePoint(0x41, 0x1f600).length === 3 && String.fromCodePoint(0x41, 0x1f600) === 'A\ud83d\ude00';
```

Object receivers use resumable ToPrimitive with string hint before position
conversion with number hint. Static arguments convert in order with number
hint, validating each code point before converting the next argument.
Callbacks may yield, wait, be collected around and migrate without replay.
String and Number wrappers use their observable conversion methods too.

The thrown values are ordinary TypeError and RangeError instances
(clj-quickjs-230).

Bulk primitive-string coercion in the static constructors currently runs
inside a leaf call. Repeated large string arguments can delay yielding and
collection even within the value-stack limit; clj-quickjs-265 tracks bounded,
resumable conversion for this case.

### String substring search (clj-quickjs-144)

`String.prototype.indexOf(search, position)` returns the first matching
UTF-16 code-unit index at or after the clamped position, or -1.
`lastIndexOf(search, position)` returns the last matching start at or before
that position, or -1. Both convert the receiver and search to strings,
then position to a number. Fractions truncate toward zero; negative values
and negative infinity clamp to zero, and positive infinity and positions
past the end clamp to the string length. For `indexOf`, absent, undefined
and NaN positions become zero. For `lastIndexOf`, they become the length.
An empty search returns the clamped position, and absent search means
`"undefined"`. Null/undefined receivers throw catchable TypeError before
argument coercion. String wrappers and generic receivers use their observable
conversion methods through resumable VM callbacks. Long scans yield between
bounded batches. Synthetic fixtures, not light-cljs output:

```js accept
'abcabc'.indexOf('cab') === 2 && 'abcabc'.lastIndexOf('abc') === 3;
'aaa'.indexOf('', Infinity) === 3 && 'aaa'.lastIndexOf('a', NaN) === 2;
'a\u{10000}b'.indexOf('b') === 3 && 'x'.indexOf() === -1;
new String('abc').indexOf({toString: () => 'b'}, {valueOf: () => 1}) === 1;
```

Explicit gaps: other string search methods remain deferred. Bulk primitive
coercion may delay yielding (clj-quickjs-265); substring scanning itself is
budgeted. Engine errors use ordinary Error instances (clj-quickjs-230).

### String extraction and combination (clj-quickjs-145)

`String.prototype.substring(start, end)` truncates numeric positions toward
zero, clamps them to [0, length], and swaps them if start exceeds end.
`slice(start, end)` counts negative positions from the end and returns empty
when start is at or beyond end. NaN positions mean zero; an omitted or
undefined end means length. Results contain UTF-16 units, including lone
surrogates when extraction splits a pair.

`concat(...args)` converts its receiver and each argument to strings in
order. `padStart(maxLength, fill)` and `padEnd(maxLength, fill)` truncate the
target length and return the receiver's string if it is already long enough,
without converting fill. Otherwise fill defaults to one space; an empty fill
returns the receiver's string even for an infinite target. Nonempty fill is
repeated and truncated to the required number of UTF-16 units. Results above
`2^30 - 1` units throw the catchable `RangeError` "string too long" before
allocating the result.

All five methods require a non-null/undefined receiver and otherwise use
generic ToString, including observable wrapper conversion methods. Object
receivers and arguments use resumable ToPrimitive with string or number
hint as appropriate. Each output-copy step processes at most 256 units or
part boundaries, allowing yields, GC and migration. Allocations and bulk
primitive operand conversion are not interruptible (clj-quickjs-265).
Concat retains whole operands as ropes, processing at most 256 operands per
assembly step (clj-quickjs-146), rather than copying their contents.

Synthetic fixtures, not light-cljs output:

```js
'abcd'.substring(3, 1) === 'bc' && 'abcd'.slice(0, -1) === 'abc';
'a'.concat(1, null, undefined) === 'a1nullundefined';
'abc'.padStart(6, '12') === '121abc' && 'abc'.padEnd(5) === 'abc  ';
'abc'.padStart(Infinity, '') === 'abc' && '\u{10000}'.slice(0, 1) === '\ud800';
```

### Number predicates and constants (clj-quickjs-147)

Execution: `isNaN('abc')` is true and `isFinite('1e3')` is true;
`Number.isNaN('abc')` and `Number.isFinite('1e3')` are false.
Global predicates use observable `valueOf`/`toString` callbacks with number
hint and can yield, wait, be collected around and migrate during conversion.
`Number.isInteger(-0)` is true; `Number.isSafeInteger(2**53)` is false.
`Number.EPSILON / 2 + 1 === 1` and `Number.MIN_VALUE / 2 === 0` are true.
The constants' descriptors have all three flags false; strict writes throw
catchable TypeError.

The two globals are task-owned global-object properties (clj-quickjs-104),
and lexical declarations shadow them. Engine-raised errors are ordinary
Error instances (clj-quickjs-230).
`Number.parseInt` and `Number.parseFloat` belong to the parsing pellet.

### Number radix formatting (clj-quickjs-149)

`Number.prototype.toString(radix)` accepts number primitives and Number
wrappers. It validates the receiver before converting the radix. An absent
or `undefined` radix means 10; every other argument uses ToNumber followed
by truncation toward zero. Values outside 2..36, including NaN and infinities,
throw catchable RangeError. Object radixes use resumable OrdinaryToPrimitive
with number hint, including waits, GC and migration. An object returning
`undefined` becomes NaN and throws, rather than selecting the default radix.

Radix 10 keeps the existing decimal layout. Other radixes use lower-case
digits without an exponent: integers exactly, fractions with the shortest
prefix that rounds back to the same binary64, choosing the closest candidate
with ties to even. Both zeros print `"0"`; NaN and infinities keep their usual
spellings. For example, `(255).toString(16)` is `"ff"`, `(0.5).toString(2)`
is `"0.1"`, and `(1.3).toString(7)` is `"1.2046204620462046205"`.

### Math numeric functions (clj-quickjs-152)

Execution: `Math` is an ordinary, non-callable namespace inheriting from
`Object.prototype`. Its non-enumerable, writable, configurable methods are
`floor`, `ceil`, `abs`, `fround` (length 1), `imul`, `hypot` (length 2).
Methods ignore `this`. Fixed-arity methods convert only their required
arguments, with missing arguments treated as undefined. Object arguments
use resumable ToPrimitive with number hint, including forwarded/bound calls
and native callbacks. `imul` multiplies modulo 2^32 and returns a signed
32-bit number; `fround` rounds to binary32, ties to even.
`hypot` converts all arguments in order, even after Infinity or NaN;
Infinity wins over NaN, all-zero inputs yield +0, and scaled compensated
summation avoids spurious overflow/underflow. Conversion and summation are
budgeted across yields, waits, collection and worker migration.

Synthetic fixtures, not light-cljs output:

```js
Math.floor(-0.5) === -1 && 1 / Math.ceil(-0.5) === -Infinity;
Math.imul(-1, 8) === -8 && Math.fround(0.1) === 0.10000000149011612;
Math.hypot(3, 4) === 5 && Math.hypot(NaN, Infinity) === Infinity;
```

The user-approved acceptance correction of 2026-10-24 specifies
`floor(-0.5) === -1`; `floor(-0)` preserves -0.
Explicit gaps: Math constants and other Math methods remain deferred to
their pellets. Global-object integration remains clj-quickjs-104; lexical
bindings shadow `Math`. Like every realm intrinsic, Math is shared and
immutable despite its methods' writable/configurable descriptor flags.
`Math()` and `new Math()` throw catchable TypeError instances (clj-quickjs-230).

### Math.pow (clj-quickjs-164)

Execution: `Math.pow(base, exponent)` converts the first two arguments to
Numbers from left to right, using resumable object conversion. Missing
arguments become undefined; extra arguments are ignored. The result uses
the same Number::exponentiate helper as `**`: `Math.pow(NaN, 0)` is 1,
`Math.pow(1, Infinity)` is NaN, `Math.pow(-0, -1)` is -Infinity and
`Math.pow(-0, 3)` is -0. Its method descriptor is non-enumerable, writable
and configurable; its length is 2 and its name is `"pow"`.
Explicit gaps: other Math methods/constants remain in their own pellets;
the shared intrinsic mutation restriction described above still applies.

### Global object bindings (clj-quickjs-104)

Execution uses a global object owned by the running task, alongside the
shared immutable realm intrinsics. `globalThis` is its writable,
non-enumerable, configurable own self-reference. Top-level `this` reads
the object directly. Script `var` and function declarations create
non-configurable enumerable properties at entry. `let` and `const` stay
lexical. Otherwise free names are looked up at every access; replacing
properties is observed immediately. Missing reads and strict writes are
`ReferenceError`; a `typeof` lookup permits a missing property, and a
sloppy write creates one (clj-quickjs-105).
Read-only global writes throw `TypeError` in strict code and do nothing
in sloppy code. A declaration conflict with lexical bindings is an early
SyntaxError; a restricted global function property is a catchable run-time
`VmErrorKind::SyntaxError` at the declaration's name.
Getter/setter calls receive the global object, including inherited accessors,
and execute in resumable VM frames. `typeof x` invokes a getter too;
assignment returns the assigned value after a setter completes. Callbacks
may yield, wait, be collected around and migrate without repeating effects.

```js accept
globalThis === this;
var a = 1; globalThis.a === 1;
function g() {} globalThis.g === g;
let b = 1; globalThis.b === undefined;
Object.defineProperty(globalThis, "x", {value: 1, writable: true}); x = 2; globalThis.x === 2;
Object = 3; globalThis.Object === 3;
Object.defineProperty(globalThis, "x", {get() { return this === globalThis ? 7 : 0; }}); x === 7 && typeof x === "number";
Object.defineProperty(globalThis, "x", {set(v) { this.saved = v; }}); (x = 3) === 3 && saved === 3;
```

Global objects are task-owned and cannot be published. A published closure
that refers to a global reads the calling task's globals; a closure with a
lexical capture follows the usual value/cell publication rule. A script
arrow capturing `this` captures the task-owned object and cannot publish.
Indirect eval (-106) runs additional scripts in the task's global scope.
Sloppy non-object receiver substitution is still deferred.

### Error constructor and stringification (clj-quickjs-228)

`Error(message, options)` and `new Error(message, options)` create distinct
ordinary task-owned objects with an internal error marker and
`Error.prototype` as prototype. Bound calls and bound construction use the
same intrinsic prototype; subclass syntax and `Reflect.construct` are not
supported. The global binding is a writable task-owned global-object property.
`Error.name` is `"Error"`, its length is 1, and
`Error.prototype.constructor === Error`. The prototype inherits from
`Object.prototype`, has name `"Error"` and message `""`, and is shared and
immutable like the other realm intrinsics.

A non-undefined message undergoes resumable ToString, then becomes an own
writable, configurable, non-enumerable data property. An omitted or undefined
message creates no own property. When options is an object, HasProperty
checks for `cause` (including inherited properties); if present, its value
is read, possibly through a resumable getter, and becomes an own data
property with the same flags. Primitive options are ignored.

`Error.prototype.toString` is generic for object receivers and throws a
catchable TypeError for primitives. It reads and converts `name` before
reading and converting `message`; undefined defaults to `"Error"` and `""`
respectively. It returns the nonempty string if either is empty, otherwise
`name + ": " + message`. All getters and conversions use task-owned native
continuations and ordinary VM calls across yields, waits, GC and migration.
`Object.prototype.toString.call(Error())` returns `"[object Error]"` using
the internal marker, even after a prototype change; inheriting from
`Error.prototype` alone does not confer the marker.

Error instances follow ordinary-object publication rules (`docs/design.md`
§4): `Runtime::publish` rejects them, and publishing one as a dependency
failure via `Completion::Failed` returns `CompleteError::NotShareable`,
leaving the cell pending and its waiters registered. Engine-raised errors
retain their existing immutable native representation.

Synthetic fixtures, not light-cljs output:

```js accept
let error = new Error('text', {cause: 0});
error.message === 'text' && error.cause === 0 && error instanceof Error;
Error(undefined).message === '' && Error(12).message === '12';
Error.prototype.toString.call({name: '', message: 'm'}) === 'm';
try { throw Error('getter'); } catch (caught) { caught.message === 'getter'; }
```

### Error stack strings (clj-quickjs-59)

`Error(...)`, `new Error(...)` and all NativeError constructors capture `stack`
at construction, including when the object is never thrown. Catchable engine
errors capture it at the failing instruction, before unwinding. Reading it later or rethrowing the
object keeps the creation positions. The first line is the creating/failing
frame; each following line is its caller at the call's `(` token. Callback
callers use the JS operation that invoked the native continuation (for
example, the `*` invoking `valueOf`, or `.` invoking a getter/setter).

Each line is `    at name (filename:line:column)` followed by a newline;
there is no message/header line. Names come from the compiled function name
(including inferred names), with `<anonymous>` for an empty name; capture
preserves arbitrary UTF-16 name code units, and never reads user-defined
`name` properties or calls getters. Every function
in a compiled script uses the name passed to `compile_script` as filename.
Lines and columns use the existing source-location convention described in
Errors above. Hand-assembled bytecode has the executable name and omits
`:line:column`. Capture retains the innermost **64 JS frames**, silently
omitting older callers. The immutable string has no heap edges and survives
frame teardown, yields, dependency waits, worker migration and collection.

Every Error instance's stack, including engine errors, is a writable,
configurable, non-enumerable own data property, as in QuickJS (clj-quickjs-230).
Reads, writes, deletion, descriptors and enumeration use ordinary object rules.
Eval runtime diagnostics use the `<eval>` source map; compilation failures
are catchable SyntaxErrors whose messages include `<eval>:line:column`.
Direct eval and function `lineNumber`/`columnNumber` properties remain deferred.

```js accept
Error('hello').stack.indexOf('    at <anonymous> (') === 0;
function make() { return Error(); } make().stack.indexOf('    at make (') === 0;
let stack; try { null(); } catch (e) { stack = e.stack; } typeof stack === 'string';
let e = Error(); let d = Object.getOwnPropertyDescriptor(e, 'stack'); d.writable && d.configurable && !d.enumerable;
let has = false; try { missing; } catch (e) { has = 'stack' in e; } has;
```

### JSON.parse (clj-quickjs-159)

Execution: global `JSON` is an ordinary, non-callable realm namespace with
`parse` (length 2, name `"parse"`). `JSON.parse(text)` applies resumable
ToString with string hint, then recognizes JSON whitespace (SP, TAB, LF,
CR), literals, the JSON decimal grammar, strings, arrays and objects.
Strings preserve UTF-16 units, including escaped or raw lone surrogates;
only JSON escapes are allowed and raw control characters are rejected.
Decimal conversion is correctly rounded, preserves -0 and allows overflow
to Infinity. Arrays and ordinary objects belong to the parsing task.
Duplicate keys replace values without changing the first key's position;
`__proto__` creates an ordinary own data property. Results have the normal
Array/Object prototypes and property attributes. Trailing garbage fails.

Malformed input throws an instance of the realm's SyntaxError with QuickJS
JSON error messages. Its non-enumerable, writable/configurable `stack`
starts with `    at <input>:line:column\n`, then the usual captured JavaScript
frames, starting at the JSON.parse call site (clj-quickjs-162). Input lines
and UTF-16 columns are 1-based; LF, CRLF and CR separate lines. Invalid
escapes point to the offending escape unit; unexpected tokens point to
their start (including malformed literals such as `tru`), and missing
input points one unit past the end. The input location is separate from
the JavaScript call diagnostic and is preserved when the error is rethrown.
Messages no longer embed line/column/offset annotations. Conversion errors
and the unsupported callable-reviver TypeError keep their ordinary stacks.

The parser runs in budgeted native steps, roots all partial containers and
results, and uses an explicit stack with a limit of 512 open containers.
Exceeding the limit throws a catchable SyntaxError, including 100,000 `[`;
no input nesting uses the Rust call stack. Whitespace, string scanning and
decoding, identifier scanning and numeric scanning/conversion yield during
long inputs. Rope inputs are flattened incrementally before parsing, with
flat inputs reused; position tracking never rescans an input on failure.
Object member names are limited to 4096 decoded UTF-16 units so the shared
leaf property-key interner's hashing and equality have a bounded name size.
The limit is checked incrementally before decoding/interning, with a
catchable SyntaxError on the first over-limit unit. Escaped and raw names
count equally; string values retain the normal `2^30 - 1` code-unit limit.

Explicit gaps: a callable reviver throws a catchable TypeError after parsing;
a non-callable second argument is ignored. Reviver traversal,
JSON extensions and separate `fileName`/`lineNumber`/`columnNumber` properties
are deferred. The requested CR/UTF-16/EOF position convention also applies
where the pinned QuickJS diagnostic helper counts only LF/scalar columns
or points an unterminated string back to its opening quote.

Synthetic fixtures, not light-cljs output:

```js accept
let parsed = JSON.parse('{"x":1,"a":[true,null]}');
parsed.x === 1 && parsed.a[0] && parsed.a[1] === null;
Object.is(JSON.parse(' -0 '), -0);
JSON.parse('"\\ud800"').length === 1;
```

## Array construction (clj-quickjs-153)

Execution: `Array()` and `new Array()` create distinct empty arrays.
One Number argument creates that many holes without allocating elements;
only integers in `0 ..= 4294967295` are valid (`-0` becomes `+0`). Invalid
lengths, including NaN and infinities, throw catchable `RangeError`.
A single non-Number, including a Number wrapper, becomes element 0;
two or more arguments become successive elements. No argument is coerced.
Constructed arrays are task-owned, mutable, and inherit `Array.prototype`.

`Array.isArray(v)` tests the array brand, including on `Array.prototype`,
without reading properties or calling JavaScript. It returns false for
persistent vectors, ordinary objects inheriting `Array.prototype`, and
primitives. `Array.length` is 1 and `Array.name` is `"Array"`;
`Array.prototype` is non-writable, non-enumerable and non-configurable.
`Array.prototype.constructor` is `Array`, writable, non-enumerable and
configurable; intrinsic mutation still follows the shared-immutable rule.
`Array.isArray` has length 1 and the usual method attributes.

Explicit gaps: subclassing/new.target, proxies (including proxy array
branding), species and the remaining array methods belong to later work.

### NativeError constructors (clj-quickjs-229)

`TypeError`, `RangeError`, `ReferenceError`, `SyntaxError`, `EvalError`,
`URIError` and `InternalError` are callable and constructible globals with
length 1 and their respective names. `InternalError` is a QuickJS extension,
not an ECMAScript standard constructor. Each constructor inherits from
`Error`; each prototype inherits from `Error.prototype`, has its own
`constructor`, `name` and empty `message`, and inherits `toString`.
The realm intrinsics remain shared and immutable, with writable task-owned
global bindings. AggregateError and SuppressedError remain deferred.

Every constructor uses Error's resumable message/cause handling and stack
capture, and creates a task-owned ordinary object with the internal error
marker. Its instances are instances of both its own constructor and Error; different NativeError
kinds have distinct prototypes. Message conversions, cause getters and
inherited stringification can yield, wait, survive GC and migrate.
Engine-raised failures also use these prototypes (clj-quickjs-230).

Synthetic fixtures, not light-cljs output:

```js accept
let e = new TypeError('text', {cause: 1});
e.message === 'text' && e.cause === 1 && e instanceof TypeError && e instanceof Error;
RangeError('m').toString() === 'RangeError: m';
Object.getPrototypeOf(URIError) === Error && Object.getPrototypeOf(URIError.prototype) === Error.prototype;
InternalError.prototype.toString === Error.prototype.toString;
```

### Math.sqrt and Math.log2 (clj-quickjs-205)

Lexical grammar: no new syntax; both are ordinary Math member calls.
Execution: `sqrt` and `log2` have length 1 and non-enumerable, writable,
configurable descriptors. They ignore the receiver and extra arguments;
missing arguments become undefined. Both use ToNumber, including resumable
number-hint ToPrimitive for objects and catchable conversion errors.
`Math.sqrt` uses correctly rounded binary64 square root, preserves signed
zero, returns NaN for negative inputs and NaN, and preserves +Infinity.
`Math.log2` returns exact integer exponents for all representable positive
powers of two, including subnormals; either signed zero gives -Infinity,
negative inputs and NaN give NaN, +Infinity gives +Infinity, and 1 gives +0.

Synthetic fixtures, not light-cljs output:

```js
Math.sqrt(2) === 1.4142135623730951;
Math.log2(2 ** -1074) === -1074;
Math.log2(10) === 3.321928094887362;
Math.ceil(2000 * Math.log2(10)) === 6644;
```

Explicit gaps: non-power-of-two `log2` uses Rust's platform math operation;
its precision can vary by platform and Rust version and is not guaranteed
correctly rounded. The value for 10 and the digit-sizing expression above
are pinned by source tests on the tested platform. Other Math methods and
constants remain tracked by their own pellets. The existing shared realm
immutability policy also applies to these methods.
Tagged templates (clj-quickjs-58) evaluate the tag reference, obtain the
site's template array, evaluate substitutions left to right without coercion,
and call the tag with that array followed by the substitutions. Member and
computed-member tags preserve `this`; result accesses and further tags chain
normally. The cooked array and its `raw` array are frozen real arrays with
Array.prototype; `raw` is non-enumerable, non-writable and non-configurable.
Malformed escapes in tagged parts produce undefined cooked entries and retain
raw text; malformed escapes in untagged templates remain syntax errors.

GetTemplateObject caches per realm, keyed by retained code identity
and instruction position, including through recursion and distinct closures
of that code and published closures called from other tasks. Separate runtimes
have separate caches. The arrays are
persistent and can be published or captured by published closures; ordinary
Object.freeze arrays remain task-owned. Shared code holds only primitives.
`String.raw` is a realm built-in with length 1, reads callSite.raw then its
ToLength length once, then each raw entry and available substitution in order,
using resumable ToString. Missing substitutions add no text; extra ones are
ignored. Null/undefined callSite or raw throw TypeError. Primitive property
reads follow the existing wrapper model (Boolean wrappers remain unsupported).
Output uses budgeted 256-unit copying and throws RangeError above 2^30 - 1 units.

### Symbol primitive values (clj-quickjs-127)

Syntax uses existing calls and computed property access: `Symbol()` and
`Symbol(description)` create fresh primitives with optional descriptions;
there is no symbol literal syntax. The global binding is a writable property
of each task's global object. `new Symbol()` parses but throws TypeError.
Symbols compare by identity for `==`, `===`, `!=`, `!==` and Object.is, are
truthy, and have typeof "symbol". A non-undefined description undergoes
resumable ToString, including ordinary VM callbacks for object arguments.

Execution: symbols remain distinct from string property names, including
identical descriptions. Computed reads, writes, updates, `in`, `delete`,
descriptors, prototype lookup and accessors accept symbols on objects, arrays
and functions. Internal key order places symbols last in creation order;
Object.keys, Object.getOwnPropertyNames and for-in exclude them. Persistent
map keys and publication through atoms preserve identity. Symbol.iterator,
Symbol.toPrimitive and Symbol.hasInstance are fixed non-enumerable properties,
with identical values in every task. String(symbol) returns
"Symbol(description)" (empty text for an absent description). Implicit
ToString (including templates and concatenation) and ToNumber throw catchable
TypeError. Source positions use the existing operator-token convention.

Symbol.prototype, primitive property reads and wrappers are supported below
(clj-quickjs-129). Explicit gaps: additional well-known symbols and general
protocol dispatch remain deferred. Symbol wrappers consult Symbol.toPrimitive;
other objects and instanceof do not yet consult the corresponding protocols.
Computed keys in object literals execute (clj-quickjs-233).

Synthetic fixtures, not light-cljs output:

```js accept
let key = Symbol('key');
let obj = {};
obj[key] = 2;
obj[key] === 2 && obj.key === undefined && typeof key === 'symbol';
Symbol('key') !== key && Symbol.iterator === Symbol.iterator;
Object.keys(obj).length === 0 && Object.getOwnPropertyNames(obj).length === 0;
```

```js accept
let caught = false;
try { new Symbol(); } catch (e) { caught = true; }
caught && String(Symbol(3)) === 'Symbol(3)';
```

Numeric binary operators finish ToNumber of the left operand before invoking
conversion callbacks on the right, so a symbol on the left immediately throws.
Plus and relational comparisons first obtain both primitive operands.
Inherited symbol-keyed setters run as ordinary resumable calls and retain the
receiver, assigned value and assignment-expression result through waits and GC.

### JSON.stringify (clj-quickjs-160/-161)

Execution: `JSON.stringify(value)` (name `"stringify"`, length 3) produces
compact JSON. Null, booleans, finite numbers and strings serialize normally;
Number::toString supplies the number spelling, including `-0` as `0`, and
non-finite numbers become `null`. Quoting escapes double quotes, backslashes,
controls (short escapes where available, otherwise lower-case `\u00xx`),
and lone UTF-16 surrogates; surrogate pairs remain paired. Number and String
wrappers undergo observable, resumable ToNumber/ToString after `toJSON`.

Objects snapshot their own enumerable string keys in ECMAScript order;
subsequent reads observe getter effects, deletion and inherited replacements.
Arrays snapshot length and read each index, including inherited properties.
Undefined, functions and symbols are omitted from objects, become `null` in arrays,
and yield `undefined` at the top level. Symbol keys are excluded without
reading their values. Object/function `toJSON` lookup runs
before filtering or unwrapping; a callable method receives the original
value as `this` and the string member name (root `""`, array index digits).
A result is serialized without applying `toJSON` again at that same position.
Accessors, methods and coercion may yield, wait, collect or migrate without
replaying effects. A circular active path throws catchable TypeError;
repeated references outside that path are allowed.

`JSON.stringify(value, replacer, space)` accepts `undefined` or `null` as
no replacer. A numeric space supplies its integer number of spaces, clamped
to 0–10 (NaN and negative values give no gap, positive infinity gives 10).
A string space supplies its first 10 UTF-16 code units, even if this splits
a surrogate pair. Number/String wrappers use observable, resumable
ToNumber/ToString before the root's `toJSON`; other space types are ignored
without coercion. A nonempty gap puts each included member/element on a new
line at the container's depth, inserts one space after each object colon,
and puts the closing delimiter on a new line at the enclosing depth. Empty
arrays/objects, including objects whose members are all omitted, stay compact.

Traversal uses an explicit stack with at most 512 open containers (catchable
RangeError beyond it). Each step visits one member/transition or quotes at
most 256 UTF-16 units; indentation emits at most 256 units per step;
final string assembly copies at most 256 units/part
boundaries per step. Output retains the normal `2^30 - 1` code-unit limit.
Shared leaf own-key enumeration is bounded by a limit of 4096 stored
properties per ordinary object, including non-enumerable properties. Member
names read during serialization are limited to 4096 UTF-16 units to bound
leaf key hashing. Strings returned by Number-wrapper ToPrimitive are limited
to 4096 UTF-16 units (including whitespace), checked before the shared leaf
StringToNumber scan; larger results throw RangeError naming the Number
conversion limit. Wrapper getters/methods still run once and resumably before
this check. These limits throw catchable RangeError. Arrays stream indices
and have no corresponding element-count limit. Primitive string values and
String-wrapper results are quoted in chunks without this conversion limit.
The same limited ToNumber path applies to Number space wrappers; String
space wrapper results use a budgeted rope cursor that stops reading after 10
units without flattening or scanning the remaining units. Pending rope-node
references are then released in chunks of at most 256 nodes per step.

Explicit gaps: every replacer other than undefined/null throws TypeError
`"JSON.stringify replacer is not supported"`.
Replacer functions/lists are deferred. Boolean wrapper construction is unavailable in the
underlying value model, so it remains an explicit prerequisite gap;
`new Boolean(true)` fails ReferenceError.
Boolean primitives are supported. BigInt serialization throws TypeError; JSON extensions are deferred.

Synthetic fixtures, not light-cljs output:

```js accept
JSON.stringify({b: 1, a: undefined, 1: 3}) === '{"1":3,"b":1}';
JSON.stringify([undefined, , NaN]) === '[null,null,null]';
JSON.stringify({toJSON(key) { return key; }}) === '""';
JSON.stringify('\ud800') === '"\\ud800"';
JSON.stringify({a: [1]}, null, 2) === '{\n  "a": [\n    1\n  ]\n}';
```

### Symbol registry (clj-quickjs-128)

Syntax: `Symbol.for(key)` and `Symbol.keyFor(sym)` use existing member calls;
no new syntax is introduced. Execution: `for` applies resumable ToString,
then returns one registered symbol per UTF-16 key string across all tasks,
workers and worlds of a runtime. Missing arguments use the key "undefined".
`keyFor` returns that runtime's registered key, or undefined for ordinary and
well-known symbols; non-symbol arguments throw catchable TypeError without
coercion. Both methods have length 1 and ignore their receiver. Constructing
either method throws TypeError. Registry entries survive GC and live until
the runtime heap is dropped. Separate runtimes own separate registries.

Rejected forms and explicit gaps: the existing syntax rejections are unchanged;
Symbol wrappers are supported below (clj-quickjs-129); general well-known
protocol dispatch remains deferred above.
The shared realm's existing immutable-intrinsic policy applies to both methods.

Synthetic fixture, not light-cljs output:

```js accept
let s = Symbol.for(1);
s === Symbol.for('1') && Symbol.keyFor(s) === '1';
Symbol.keyFor(Symbol('1')) === undefined;
```


### Execution: JavaScript Map (clj-quickjs-169)

`new Map()`, `new Map(undefined)` and `new Map(null)` construct an empty,
task-owned Map inheriting from `Map.prototype`. Calling `Map` without `new`
throws a catchable TypeError. `set(key, value)` returns its receiver;
`get(key)` returns the stored value or `undefined`; `has(key)` returns a
boolean; `delete(key)` returns whether an entry existed; `clear()` returns
`undefined`. The inherited, non-enumerable `size` getter counts live entries
and has no setter. These methods/getter require a branded Map receiver and
throw TypeError otherwise. Own properties, prototype reflection, `in`,
`instanceof`, deletion and integrity operations follow ordinary object rules.

Keys use SameValueZero: NaN equals NaN, -0 equals +0 (stored as +0), strings
compare by content and objects by identity. Undefined, null, booleans,
numbers, strings, BigInts (by mathematical value, distinct from Numbers;
clj-quickjs-171) and symbols (by identity) are supported; equal-looking
objects remain distinct. `Map.prototype[Symbol.toStringTag]` is "Map"
(non-writable, non-enumerable, configurable).
Updating a key preserves insertion position; deleting and reinserting
moves it to the end. No user conversion or comparison code is invoked.

Explicit gaps: non-nullish constructor arguments throw TypeError instead of
consuming iterables. Map iteration methods are deferred.
Fresh equal BigInt lookup/update/has/delete and forEach callback deletion
loops are covered for 1000 inline and 1000 larger-than-128-bit keys
(clj-quickjs-171), with each loop leaving an empty Map.
Generic Object.prototype.toString
consumption of Symbol.toStringTag remains deferred with Symbol protocol consumers;
calling it on a Map explicitly fails with UnsupportedOperand.

### Execution: Map forEach (clj-quickjs-170)

`Map.prototype.forEach(callback, thisArg)` validates the Map brand and
callback callability before iteration, calls the callback with `thisArg`
and `(value, key, map)`, and returns `undefined`. The callback follows normal
function `this` rules. Each step re-reads the live insertion-ordered entries:
updates preserve position, deleted entries are skipped, and appended or
reinserted entries are visited later. A callback can delete its current
entry. `clear()` removes all pending entries; entries appended afterward
are still visited. Callback exceptions propagate to the caller, ending the
iteration. Callbacks may yield or wait on `__rt.need`, including across GC
and worker migration. Explicit gaps: Map iterator methods and iterable
construction remain deferred.

## Regular expression pattern parsing API (clj-quickjs-186)

Execution: the Rust `regexp::parse::parse` entry point validates UTF-16
pattern text and flags to an arena IR; it does not execute patterns.
Supported forms include alternatives, captures and named/forward references,
noncapturing and scoped i/m/s groups, assertions and lookaround, greedy/lazy
quantifiers, character classes/ranges, character/class escapes, Annex B
non-Unicode forms, and strict u-mode parsing with surrogate-pair joining.
Errors include messages and UTF-16 offsets (flag errors index the flags).
Quantifier counts above u64::MAX fail explicitly.

Explicit gaps: v patterns and Script/Script_Extensions property escapes remain
deferred. Binary and General_Category property escapes execute in u mode
(clj-quickjs-192). Pattern validation at literal sites remains
clj-quickjs-56; the lexer/AST currently preserves the pattern. RegExp objects and methods execute (clj-quickjs-188);
Unicode matching executes (clj-quickjs-190); case-insensitive matching remains deferred.

### BigInt values and literals (clj-quickjs-196)

Lexical grammar: integer digits followed by lowercase `n` produce exact BigInt
constants: `0n`, `123n`, `0xFFn`, `0o17n`, `0b101n`; radix prefixes may be
uppercase. Decimal digits cannot have leading zeros (except `0n` itself).
Fractions/exponents (`1.5n`, `.5n`, `1e3n`), leading zeros (`01n`, `08n`),
missing prefix digits (`0xn`) and an immediately following identifier or digit
(`1nn`, `1n2`) remain positioned InvalidToken errors. Magnitudes exceeding
1 Mbit fail compilation with TooLarge (`BigInt is too large`). Numeric
separators remain unsupported. Constants are immutable root-free primitives
and use the ordinary validated constant pool and its count limit; the private
BigInt representation enforces the magnitude limit before loading a constant.

Execution: `===`/`!==` compare exact integer values across radix spellings and
never equate a BigInt with a Number. `typeof` is `"bigint"`. Only `0n` is falsy,
including for `if`, `!`, `&&`, `||` and `?:`. BigInts publish across tasks and
survive collection inside persistent vectors/maps. Persistent map keys compare
and hash by integer value; `__rt.get(__rt.map(10n, 1), 10n)` is `1`.
`Object.keys` and `Object.getOwnPropertyNames` report no own keys for a
BigInt primitive; `for-in` over it performs no iterations.

Explicit gaps: wrappers and literal property names remain deferred.
JavaScript string/Number conversions and BigInt property keys are supported
by clj-quickjs-202/-203 below. Available but deferred operators/conversions throw
UnsupportedOperand naming BigInt; mixing BigInt and Number in arithmetic
throws TypeError. `JSON.stringify` rejects a BigInt with TypeError. Embedder
inspection supports exact decimal digits and sign/magnitude independently of
JavaScript conversion support.

## ArrayBuffer (clj-quickjs-165)

Execution: `new ArrayBuffer(length)` creates distinct, zero-initialized task-owned
byte storage. Omitted/undefined/NaN lengths become zero; numeric coercion is
resumable and fractions truncate toward zero. Negative integer lengths,
infinities, and lengths above 16,777,216 bytes throw `RangeError`, as does byte
allocation failure. `ArrayBuffer(length)` without `new` throws `TypeError`.
`ArrayBuffer.prototype.byteLength` and `.detached` are non-enumerable configurable
getters with no setter. They require a branded buffer; byteLength is zero after
detachment. Buffers otherwise support ordinary task-owned properties/prototypes.

`buffer.transfer(newLength?)` allocates a distinct buffer, copies the prefix,
truncates or zero-extends as needed, and detaches the source after successful
allocation. Omitted/undefined newLength preserves the original length. A detached
source throws `TypeError`; invalid lengths or throwing coercions leave an
attached source intact unless the coercion itself detached it. The source keeps
its ordinary properties. `buffer.slice(start?, end?)` copies a clamped range into
a fresh buffer without detaching, with negative indices relative to its length.
Object indices convert left to right through resumable native frames.

Explicit gaps: custom slice constructors/species fail explicitly; resizable
buffers, SharedArrayBuffer and subclassing remain deferred. Integer typed-array
views are supported below.
Publication through atoms or dependency cells rejects buffers, including detached
ones. Foreign tasks cannot read or mutate their state.

Until resizable buffers are supported, an object-valued second constructor
argument fails explicitly with `UnsupportedOperand` after length coercion;
primitive options are ignored. `ArrayBuffer.length` is 1, `transfer.length` is 0,
`slice.length` is 2, and both accessor functions have length 0.

### BigInt comparisons (clj-quickjs-197)

Lexical grammar: comparisons use the existing BigInt literals and operator tokens.
Execution: `<`, `<=`, `>`, `>=`, `==` and `!=` compare BigInts by exact value.
Mixed BigInt/Number comparisons preserve both mathematical values, including
fractional Numbers: `2n < 2.1`, `2.1 < 3n`,
`9007199254740993n > 9007199254740992`, and
`100000000000000000001n != 1e20` are true. NaN makes all relational operators
and `==` false; infinities are ordered beyond every BigInt. `0n == -0` is true.
Booleans convert to Numbers; `1n == true` is true. Null/undefined equal no
BigInt, while relational comparison converts them to 0/NaN. Symbols compare
unequal and throw TypeError for relational operators. Object operands use
resumable ToPrimitive (number hint for relational operators, default for
equality), retaining source conversion order and exceptions.

BigInt/String comparisons use StringToBigInt (clj-quickjs-201), including
strings returned by ToPrimitive.
Strict equality continues to distinguish Number and BigInt.

## Task-owned job execution (clj-quickjs-174)

Execution: after a script body completes normally, engine-enqueued jobs run
in FIFO order as ordinary resumable activations. Jobs appended by a job run
after the existing tail. `Complete` returns the body's original value only
after all jobs drain; an uncaught job exception fails the task and discards
the tail. Jobs share the run's instruction budget and can yield, migrate or
wait on `__rt.need`; a wait suspends the whole task, including queued jobs.
Consumer cancellation discards pending jobs and uses the active activation's
existing finally cleanup. ClearKeptObjects runs on the owning task after
the body and after each completed job (clj-quickjs-176).

Explicit gaps: enqueue is crate-internal, with no new syntax or global API.
Promise reactions, async resumes and host timers use this queue.
FinalizationRegistry cleanup producers remain deferred.
Cross-task delivery transfers shareable arguments to an owner-registered
callable without running JavaScript during publication. Future rejection
reporting waits for quiescence after jobs and timers, not individual jobs.

### Regular expression matcher API (clj-quickjs-187)

Execution: the Rust-only `regexp::matcher::compile(&Pattern)` and
`exec(&Program, input: &[u16], start)` execute pattern IR, including Unicode
mode (clj-quickjs-190). The
result is `Result<Option<Captures>, MatchError>`: a match, no match, or an
explicit failure. All ranges and the match index count UTF-16 code units;
group zero is the full match and undefined captures are `None`. Greedy/lazy
bounded and unbounded repeats follow RepeatMatcher's empty-iteration check
and clear captures inside the atom before each iteration. Positive lookahead
retains captures; negative lookahead discards them. Lookahead is atomic.
Backreferences (including forward/undefined and duplicate-name candidates),
classes/escapes, `^`, `$`, `\b`, `\B`, dot, and scoped m/s are supported.
The m flag changes line assertions, s changes dot, and y restricts the search
to exactly `start`. g/d do not change the core search; capture ranges are
always available. A start beyond the input length returns no match.

Explicit gaps: lookbehind and non-Unicode ignore-case matching
(including scoped i without u) return `MatchError::Unsupported` at compilation.
`MAX_MATCH_STEPS = 1_000_000` bounds a complete search, including assertions,
backtracking and comparisons; exhaustion returns `LimitExceeded`, never
`None`. The RegExp object layer translates it to a catchable
RangeError. Compilation has an independent allowance of the same size.
The object layer supplies JavaScript RegExp objects, exec/test, lastIndex
and literal execution (clj-quickjs-188).

### BigInt additive and multiplicative arithmetic (clj-quickjs-198)

Lexical grammar: existing `+`, `-`, `*`, `++`, `--`, `+=`, `-=`, `*=` and
BigInt literal tokens suffice; unary minus is an operator, not part of a literal.
Execution: two BigInts add, subtract and multiply exactly. Unary minus keeps
`0n` canonical. Prefix/postfix updates preserve BigInt, including bindings,
closure cells and property references; postfix returns the old numeric value.
Compound assignment reuses these operators. Each arithmetic operation costs one
instruction, independent of limb count. Every intermediate result obeys the
1 Mbit magnitude cap; overflow throws catchable `RangeError` before a store.
Mixing BigInt with Number, boolean, undefined or null throws catchable TypeError
(`cannot mix BigInt and other types`); unary plus on BigInt also throws TypeError.
Object operands use resumable ToPrimitive in source order, including waits.
String concatenation involving BigInt uses decimal ToString (clj-quickjs-202).

### BigInt division, remainder and exponentiation (clj-quickjs-199)

Lexical grammar: existing `/`, `%`, `**`, `/=`, `%=` and `**=` tokens and
BigInt literals suffice. The unary-left-operand restriction still makes
`-2n ** 2n` a SyntaxError; `(-2n) ** 2n` and `2n ** -1n` parse.
Execution: two BigInts divide with truncation toward zero and produce a
remainder with the dividend's sign (zero stays canonical). Exponentiation
computes exact powers, with `x ** 0n === 1n` even for zero, and `0n ** k === 0n`
for positive k. A negative exponent or division/remainder by zero throws a
catchable RangeError. Mixed numeric types throw TypeError after resumable
ToNumeric conversion in source order. Compound assignments reuse these
operators and store only after success. Each operation costs one instruction.
Powers reject sizes beyond the 1 Mbit cap with RangeError (`BigInt is too
large`), including oversized intermediate operations later cancelled by zero.
Explicit gaps: the conversion APIs remain assigned to their sibling pellets;
this change adds no new conversions.

## Integer typed arrays (clj-quickjs-166)

Execution: the seven integer constructors `Int8Array`, `Uint8Array`,
`Uint8ClampedArray`, `Int16Array`, `Uint16Array`, `Int32Array`, `Uint32Array`
require `new`. A primitive length undergoes ToIndex; buffer construction
converts byteOffset then optional length resumably, validates alignment and
bounds, and rejects detached buffers with TypeError. Length allocation is
zero-filled and bounded by ArrayBuffer's 16 MiB byte limit.

Canonical numeric string keys (including `-0`, negative/fractional indices,
NaN and infinities) use integer-indexed exotic operations, without prototype
fallback for invalid indices. Invalid reads are undefined, `in` is false,
and writes still convert their values before becoming no-ops. Other keys
remain ordinary properties. Element writes wrap modulo the element width;
Uint8ClampedArray clamps to 0..255 with ties rounded to even. Conversion
callbacks, including property-key and descriptor-value conversion, can wait,
yield, collect and migrate. BigInt and Symbol element values throw TypeError.

The abstract `%TypedArray%` constructor is reached through
`Object.getPrototypeOf(Uint8Array)`; calling or constructing it throws
TypeError. Its shared prototype has branded `buffer`, `length`, `byteLength`,
`byteOffset` getters. Each concrete constructor and prototype exposes fixed
`BYTES_PER_ELEMENT`. A transfer detaches every old view: elements are
undefined and the numeric getters are zero, while `buffer` retains identity.
In-range element descriptors are writable/enumerable/configurable data
properties, but deletion and attribute restrictions fail. A nonempty view
cannot be frozen (preventing extensions still succeeds).

Explicit gaps: BigInt typed arrays, other typed-array methods, resizable
buffers and subclass/species construction remain deferred. Source construction
and set/fill/join/toString are described under clj-quickjs-168 below.

### BigInt bitwise and shift operators (clj-quickjs-200)

Lexical grammar: existing `~`, `&`, `|`, `^`, `<<`, `>>`, `>>>` tokens and
their compound assignment forms suffice; no literal syntax changes.
Execution: BigInt `~`, `&`, `|` and `^` use infinite two's-complement semantics.
BigInt `<<` and `>>` take exact BigInt counts without 32-bit masking; negative
counts reverse direction. Right shifts round toward negative infinity, so
`-5n >> 1n` is `-3n`; arbitrarily large right shifts give `0n` or `-1n`.
Zero stays zero for any shift count. `>>>` with a BigInt and mixed
BigInt/Number pairs throw catchable TypeError. Resumable ToNumeric converts
object operands in source order, including waits, yields and collection.
Each operation costs one instruction. Every result obeys the 1 Mbit magnitude
cap: left shifts check the exact result size before allocation; overflowing
bitwise results also throw RangeError (`BigInt is too large`). Compound stores
occur only on success.
Explicit gaps: BigInt conversion APIs remain assigned to their sibling pellets.

### Execution: Date time values and UTC (clj-quickjs-179)

`new Date(value)` creates a distinct ordinary task-owned object inheriting
from `Date.prototype`, with an own internal `[[DateValue]]` slot. An actual
Date argument copies its slot without running conversion hooks, regardless
of its prototype. Other arguments undergo resumable ToPrimitive with default
hint, then ToNumber; string primitives fail explicitly pending Date parsing.
TimeClip accepts finite milliseconds within ±8.64e15, truncates toward zero,
canonicalizes zero to +0 and otherwise stores NaN. `Date.prototype` is an
unbranded ordinary object without that slot; it and objects inheriting from
it fail Date method receiver checks with TypeError.

`getTime()` and `valueOf()` return the slot; `setTime(value)` validates the
receiver first, converts its first argument to Number resumably, clips, stores
and returns that value. A missing argument becomes NaN. Invalid receivers
throw catchable TypeError; a foreign task's receiver fails ownership checks.
Freezing an instance's ordinary properties leaves its internal slot mutable;
the shared intrinsic prototype follows the realm's immutable policy instead.
Object.prototype.toString reports `[object Date]` for actual Date objects.

`Date.UTC(year, month = 0, date = 1, hour = 0, minute = 0, second = 0, ms = 0)`
converts the first seven supplied arguments to Number in order, including
all later conversions after NaN/Infinity; abrupt conversion failure stops
before later hooks. Missing year gives NaN; explicit undefined does not select
a default. Further arguments are evaluated by the caller but never coerced.
After conversion, integer years 0..99 become 1900..1999; other components
truncate toward zero, normalize and combine in the ECMA-262 floating-point
order before final clipping. Intermediate day/hour/etc. values may exceed
the clip range and cancel. Methods have standard names, lengths and method
attributes (Date length 7, UTC length 2, getTime/valueOf length 0, setTime
length 1). Bound Date construction and forwarded method calls work normally.
Callbacks may yield, wait, survive GC and resume on another worker.

Numeric string coercion for UTC/setTime is capped at 4096 UTF-16 units,
including strings returned by ToPrimitive. Above that limit a catchable
RangeError precedes later field conversions and setTime mutation. This bounds
the shared leaf StringToNumber scan/rope flatten until clj-quickjs-265 supplies
budgeted primitive numeric parsing. Constructor strings fail explicitly as
the parsing gap below, without scanning their contents.

Explicit gaps: `Date()` calls fail with UnsupportedOperand pending formatting.
String construction/parsing, non-ISO formatting and Date's
Symbol.toPrimitive/default conversion remain
separate Date/protocol work. A value lookup reaching Date.prototype for its
deferred toString/toJSON fails explicitly rather than using Object.prototype's
tag or silently serializing a Date as an ordinary JSON object. Presence checks
for these standard methods return true without running formatting. An own
toJSON override (including a getter or non-callable value) remains observable.
Host clock/zone integration is documented at `src/vm/date.rs`:
unrecorded leaf host clock effect, one rule-table zone per runtime, UTC when
no host supplies a zone. Clock and local-time behavior is supplied by clj-quickjs-182 below.

### Execution: Date ISO formatting (clj-quickjs-180)

`Date.prototype.toISOString` (name `toISOString`, length 0, ordinary method
flags) reads its receiver's own Date slot, ignoring arguments and conversion
hooks. Valid values format as `YYYY-MM-DDTHH:mm:ss.sssZ` in UTC; years outside
0..9999 use a sign and six digits, including `-000001` and `+010000`.
Negative milliseconds use the preceding calendar day, so -1 formats as
`1969-12-31T23:59:59.999Z`. The exact TimeClip endpoints format as
`-271821-04-20T00:00:00.000Z` and `+275760-09-13T00:00:00.000Z`.
NaN throws a catchable RangeError with message `Invalid time value`;
unbranded receivers throw TypeError. Foreign task receivers retain the
existing ownership failure. The method is non-constructible.

Explicit gaps: `toJSON`, other Date formats and parsing remain
deferred as above. ISO formatting consumes no clock or zone input.

## Floating-point typed arrays (clj-quickjs-167)

Execution: `new Float16Array`, `new Float32Array` and `new Float64Array` accept
a primitive length or `(buffer, byteOffset?, length?)`, sharing the integer
constructors' resumable ToIndex conversion, alignment/bounds RangeErrors,
detached-buffer TypeErrors and zero initialization. They inherit the same
`%TypedArray%` prototype and expose fixed `BYTES_PER_ELEMENT` of 2, 4 and 8.
Element writes perform resumable ToNumber, then round to binary16/binary32
with ties to even or retain binary64 exactly. Negative zero and subnormals
are preserved, overflow becomes signed infinity, and stores select a positive
quiet NaN representation. Loads widen exactly to Number. The backing bytes
are little-endian; all existing exotic operations and detachment rules apply.

Explicit gaps: `Math.f16round`, BigInt views, other typed-array methods,
resizable buffers and subclass/species construction remain deferred. Source
construction and set/fill/join/toString are described under clj-quickjs-168 below.

### RegExp object execution (clj-quickjs-188)

Execution: literals allocate fresh task-owned objects from immutable compiled
programs. `RegExp(pattern, flags)` and `new RegExp` accept strings or RegExp
objects. `exec` returns null or an array with index/input/groups; unmatched
captures are present undefined elements. Named groups use a null-prototype
object. `test`, generic `toString`, source escaping, all boolean flag getters,
generic `flags` in dgimsuvy order, and own writable/non-enumerable/
non-configurable `lastIndex` execute. Global/sticky matching applies ToLength,
updates lastIndex on success and resets it on failure. Nonglobal matching
reads/converts lastIndex but leaves it unchanged. Empty matches do not advance
it beyond the match end. All positions and captures count UTF-16 code units.
Coercions and getters use resumable frames; matching has the documented
1,000,000-step bound and throws catchable RangeError on exhaustion.

Explicit gaps: constructing plain i or v patterns throws a catchable SyntaxError
naming the unsupported flag. Non-Unicode scoped i and lookbehind are also rejected.
Literal pattern early errors, Symbol.match recognition, symbol-based string
consumers, non-Unicode folding and Unicode sets remain deferred.
Eval is unavailable; the upstream raw NUL case uses constructor/raw literal.

```js accept
let r = /(b+)c/g; let m = r.exec('abbbbbc'); m[1] === 'bbbbb' && m.index === 1;
new RegExp('a/b').source === 'a\\/b';
r = /a/y; r.lastIndex = 1; r.test('ba') && r.lastIndex === 2;
/^.$/u.test('🐱');
```

### BigInt function and StringToBigInt (clj-quickjs-201)

Lexical grammar: string conversion uses StringToBigInt rather than the source
literal grammar. It trims the same StrWhiteSpace and line terminators as
StringToNumber. Empty text is zero. Decimal digits may have one leading `+`
or `-`; unsigned `0x`/`0X`, `0o`/`0O`, and `0b`/`0B` forms require at least
one radix digit. Leading decimal zeros are permitted. Fractions, exponents,
`Infinity`, separators, `n` suffixes, signed radix prefixes, embedded whitespace,
NUL, non-ASCII digits and lone surrogates are invalid.

Execution: the global `BigInt` function has length 1. Integral finite Numbers
convert exactly from their binary64 value, including values outside i64;
`BigInt(9007199254740993)` is `9007199254740992n`. Negative zero becomes `0n`.
Booleans convert to `0n`/`1n`, BigInts remain unchanged, and strings use the
shared parser. Objects use resumable ToPrimitive with hint number. Invalid
strings throw catchable SyntaxError; fractional/non-finite Numbers and
magnitudes exceeding 1 Mbit throw RangeError; undefined, null, symbols and
construction with `new` throw TypeError. String validation precedes size
rejection. `BigInt.prototype` inherits from `Object.prototype`, has a
`constructor` link, and supplies property lookup on BigInt primitives;
`Object.getPrototypeOf(1n)` returns it. Realm intrinsics remain immutable.

BigInt/String loose equality and relational comparisons now parse the string
exactly, including strings returned by ToPrimitive. Invalid strings are unequal
and unordered (all four relational operators return false). Valid strings
above the magnitude cap throw RangeError. Long scans and decimal limb updates
yield through native continuations. Each advance handles at most 256 bounded
scan/limb steps (a decimal group reads at most nine digits); an existing string
rope may first flatten in linear time.

Explicit gaps: BigInt wrapper objects remain deferred. Prototype, Number
conversion and width wrapping are supported below.

### Execution: Date UTC field access (clj-quickjs-181)

`Date.prototype` supplies `getUTCFullYear`, `getUTCMonth`, `getUTCDate`,
`getUTCDay`, `getUTCHours`, `getUTCMinutes`, `getUTCSeconds` and
`getUTCMilliseconds` (length 0). They read the own branded slot, return NaN
on invalid dates, and ignore arguments. Months count from zero; weekdays
count from Sunday = 0.

`setUTCFullYear(y[, m[, d]])`, `setUTCMonth(m[, d])`, `setUTCDate(d)`,
`setUTCHours(h[, m[, s[, ms]]])`, `setUTCMinutes(m[, s[, ms]])`,
`setUTCSeconds(s[, ms])` and `setUTCMilliseconds(ms)` have lengths 3, 2, 1,
4, 3, 2 and 1. All methods have ordinary method flags and reject unbranded
receivers before observable coercion. Setters capture the time before
coercion, run ToNumber on accepted arguments from left to right through
resumable frames, preserve omitted fields, normalize overflows and return
the stored MakeDate/TimeClip result. Explicit undefined or a missing first
argument produces NaN. Years 0..99 stay literal years. Excess arguments are
ignored. Non-finite numbers do not suppress later conversions; a conversion
exception stops immediately and prevents the setter's final slot write.
The shared 4096 UTF-16-unit numeric-string conversion cap still applies.

Invalid original dates still undergo argument coercion. Except for
`setUTCFullYear`, they then return NaN without overwriting slot changes made
by callbacks. `setUTCFullYear` uses +0 as the base for invalid original dates.
Frozen dates remain mutable through these internal-slot setters. Waits,
instruction yields, collection and worker migration preserve pending receivers,
arguments and the original numeric time. Foreign-task access remains rejected.
Explicit gaps: parsing, other formats and `toJSON` remain separate pellets;
local fields and host inputs are implemented by clj-quickjs-182 below.

## Typed-array sources and methods (clj-quickjs-168)

Execution: all Number typed-array constructors accept typed-array sources,
object iterables using `Symbol.iterator`, and array-like objects using
`length` and indexed reads. Typed-array sources take precedence over their
iterator property. Iterable values are collected before element conversion;
array-like values are read and converted one at a time. Symbol assignment
and computed literals can define iterator methods/getters.

`set(source, offset = 0)` uses array-like or typed-array sources, truncates
offset toward zero, and throws RangeError before writing if the source will
not fit. Typed-array sources are snapshotted for overlapping views.
`fill(value, start = 0, end = length)` converts value once, clamps relative
indices, and returns its receiver. `set` returns undefined. Both use internal
view lengths. `join(separator = ',')` reads the internal length and converts
the separator resumably; elements detached during separator conversion become
empty strings. Initially detached receivers throw TypeError. `toString` is
the same function object as `Array.prototype.toString`, including dynamic
`this.join` lookup. Getters, iterator calls and conversions remain resumable;
long element loops and string copying yield to the instruction budget.

Explicit gaps: BigInt views, resizable/shared buffers, species/subclassing,
and other typed-array methods (except `values`/`Symbol.iterator`, clj-quickjs-237) remain
deferred. Allocation and iterable collection are bounded by the existing
16 MiB backing-buffer limit per view; infinite iterables hit that limit.

Default array sources now use the general Array iterator (clj-quickjs-237),
which rereads length before each indexed read; construction then converts
the collected values. Explicit null/undefined iterator overrides, or removal
of Array.prototype from the chain, select array-like read/conversion order.
`set` always uses array-like order for ordinary sources and ignores their
iterator properties.

## Synchronous for-of and built-in iterators (clj-quickjs-237)

Execution: `for (const x of iterable)`, `for (let x of iterable)`,
`for (var x of iterable)` and `for (x of iterable)` obtain `Symbol.iterator`
once, call it with the iterable as receiver, require an object iterator and
cache its `next`. Each iteration calls that method with the iterator as
receiver, requires an object result, reads `done`, and reads `value` only
when `done` is false. Non-iterables throw catchable TypeError. A lexical
head has its own temporal dead zone during iterable evaluation and a fresh
binding per iteration, including closures; `var` retains one hoisted binding.

`Array.prototype.values` and `[Symbol.iterator]` are the same function.
Array iterators read the current length and index each time (holes are
undefined unless inherited); exhaustion is permanent. `values` is generic
over array-like receivers. Typed-array `values`/`[Symbol.iterator]` validate
the view and use its internal length, checking detachment on each step.
`String.prototype[Symbol.iterator]` coerces its receiver to string, then
yields code points: a surrogate pair is one two-unit string and a lone
surrogate remains one unit. Iterators inherit an identity `[Symbol.iterator]`
method and the appropriate `next` and `Symbol.toStringTag`.

`continue` of this loop never closes; exhaustion never closes. Break,
return, body throw, outer continue/labelled jump and consumer cancellation
perform IteratorClose through the existing finally machinery. The `return`
method is optional (null/undefined mean absent); its receiver is the iterator
and its result must be an object. Pending throws retain their original
identity and diagnostic when close fails; other completions are replaced by
close failures. Exceptions from `next`/`done`/`value` do not close.
Iterator, cached next and results are hidden traced frame locals. All
getters, callbacks and string coercions use ordinary resumable frames;
waiting, migration, collection and budget yields preserve exact progress.

Synthetic fixtures, not light-cljs output:

```js accept
let total = 0;
for (const x of [1, 2, 3]) total += x;
let fs = []; for (let x of [1, 2]) fs.push(() => x);
let text = ''; for (let cp of 'A\u{1f600}') text += cp;
let it = { next() { return { value: 1, done: false }; }, return() { return {}; } };
it[Symbol.iterator] = function() { return this; };
for (let x of it) break;
for (var x of new Uint8Array([1, 2])) {}
```

```js reject UnexpectedToken 1:16
for (let x of a, b) {}
```

```js reject Unsupported 1:6
for (obj.x of a) {}
```

### String replacement (clj-quickjs-189)

Execution: `String.prototype.replace(search, replacement)` and `replaceAll`
accept strings and internally branded RegExp objects. String replacement is
first-occurrence/all non-overlapping occurrences respectively; empty string
search visits UTF-16 boundaries, including both ends for `replaceAll`.
Nonglobal regexps replace once; global regexps start at lastIndex zero,
collect all matches before invoking replacers, advance past empty matches,
and reset lastIndex to zero on the final failed match. Sticky/nonglobal
lastIndex behavior follows `exec`. `replaceAll` rejects a nonglobal RegExp
with a catchable TypeError. Captures and offsets use UTF-16 code units;
empty matches advance by code point with u and by code unit otherwise.

String replacements expand `$$`, `$&`, `` $` ``, `$'`, `$n`, `$nn` and
`$<name>`. Unmatched captures expand to empty strings; invalid numbered
references stay literal; named references stay literal without named
captures, and missing names expand to empty strings when names exist.
Function replacers receive `(match, ...captures, offset, input[, groups])`
with undefined `this`; named groups have a null prototype. Returned values
use ToString and are inserted literally. All argument coercions, lastIndex
ToLength and replacer calls/results use resumable VM frames, permitting
yield, dependency waits, collection and migration without replay.

String scans use 256-comparison KMP steps; each regexp search retains the
1,000,000-step matcher bound. Before flattening, receiver/search/replacement
strings and callback result strings are limited to 65,536 code units each.
The same bound applies to lastIndex strings after resumable ToPrimitive,
before numeric conversion scans or flattens them (global regexps overwrite
the original lastIndex with zero first).
An aggregate 1,000,000-unit allowance bounds capture storage, substring
copying, substitution parsing/name comparisons and output across all steps.
Exhaustion throws catchable RangeError, including output expansion.

Explicit gaps/dispatch decision: these methods use internal RegExp slots,
not `Symbol.replace`/`Symbol.match` dispatch. RegExp overrides of `exec`,
`flags` and boolean flag properties are ignored; internal flags select the
loop. Other search objects undergo ToString. Symbol-based protocols and
generic `RegExp.prototype[Symbol.replace]`, `match`/`search`, case
folding and the other deferred RegExp flags remain separate work.

```js accept
'abcabc'.replace('b', 'X') === 'aXcabc';
'ab'.replaceAll('', '-') === '-a-b-';
'abc'.replace(/(?<x>b)/, '<$<x>>') === 'a<b>c';
'ab'.replaceAll('', (match, offset) => offset) === '0a1b2';
```

## Promise core (clj-quickjs-246)

Syntax: existing `new`, calls, method calls and functions express all Promise
operations; no syntax production is added. Execution: `new Promise(executor)`
calls a callable executor synchronously with resolve/reject functions sharing
one already-resolved flag. Calling without `new` or with a non-callable
executor throws TypeError. The first resolution/rejection wins, including over
an executor's later throw. Self-resolution rejects with TypeError.
`Promise.resolve(p)` preserves a Promise's identity; other values produce a
new promise. `Promise.reject(value)` always produces a new rejected promise.
`then` requires a Promise; non-callable handlers act as identity/thrower.
Handler returns resolve and throws reject the derived Promise. `catch` reads
and calls `this.then(undefined, handler)`, including resumable getters.

Reactions run as FIFO jobs on the task that registered them, after synchronous
code and behind already queued jobs. Waiting on a dependency inside an executor,
getter or reaction suspends the whole task; collection and migration preserve
its exact continuation. Settlement on another task only appends jobs and never
wakes dependency waits. Promises and their resolve/reject functions are
shareable. A task-owned object result remains confined to its settling task;
a foreign reaction rejects its derived promise with TypeError instead of
exposing that object. Promise prototypes and resolver properties are immutable
under the existing shared-intrinsic policy.

Resolving with an object reads `then` exactly once, including resumable
getters. A throwing getter rejects; a non-callable `then` fulfills with the
object. A callable `then` (including another Promise's method) is saved and
called in a FIFO resolve-thenable job with the object as `this` and a fresh
resolve/reject pair. The pair's first call wins, including over a later throw
from `then`. Nested thenables are adopted by subsequent jobs. An already
fulfilled Promise adopted by `new Promise(r => r(inner))` adds two jobs before
the outer reaction: the adoption call and the inner forwarding reaction.
Thenable calls can yield, wait, migrate and survive collection like reactions.

Rejected forms: async generators and derived class syntax retain explicit
rejections. Explicit gaps: subclass capabilities,
species, combinators, `finally` and arbitrary Promise own properties are
deferred. Only the intrinsic Promise constructor is
accepted as `this` for static resolve/reject.

Synthetic fixtures, not light-cljs output; callbacks run after the final
synchronous expression:

```js accept
let p = Promise.resolve(1); p.then(x => x + 1); Promise.resolve(p) === p;
let n = 0; new Promise(() => { n++; }); n === 1;
```

```js accept
let n = 0; Promise.reject(1).catch(() => n++); n === 0;
```

```js accept
let n = 0; try { new Promise(1); } catch (e) { n = e instanceof TypeError; } n;
```

Thenable adoption (clj-quickjs-247), synthetic fixture, not light-cljs output:

```js accept
let calls = 0; Promise.resolve({then(resolve) { calls++; resolve(5); }}); calls === 0;
new Promise(resolve => resolve(Promise.resolve(1))).then(value => value + 1);
Promise.resolve({get then() { throw 7; }}).catch(reason => reason);
```

### BigInt string formatting (clj-quickjs-202)

Lexical grammar: existing method calls and template substitutions suffice;
no new token or opcode is needed.

Execution: `BigInt.prototype.toString(radix)` accepts bases 2 through 36,
uses lower-case digits and a minus sign, and defaults to decimal when radix
is absent or undefined. Its length is 0. Radix uses ToIntegerOrInfinity;
invalid bases throw RangeError and BigInt/Symbol radices throw TypeError.
Receiver branding precedes observable radix coercion. `valueOf()` returns
the primitive unchanged. Both methods reject other receivers, including
the unbranded `BigInt.prototype`, with TypeError.

Shared ToString emits decimal digits without an `n` suffix for String calls
and construction, templates, string concatenation, computed property keys,
and other built-ins that apply ToString. Object callbacks retain the original
conversion hint and run once. Long formatting yields in bounded native steps,
including after object conversion; it survives collection and migration.

Explicit gaps: wrapper objects and locale-sensitive `toLocaleString` remain
unsupported; JSON serialization of BigInts still throws TypeError.

### Unicode RegExp execution (clj-quickjs-190)

Execution: `u` is accepted by literals and constructors. Consuming atoms and
search candidates advance over a complete surrogate pair, or one unpaired
surrogate. Literals, dot, positive/inverted classes, escapes, astral ranges,
quantifiers and backreferences use code points through U+10FFFF. A surrogate
literal never matches half a pair. Match indices, captures, callback offsets
and lastIndex still count UTF-16 code units. A global/sticky lastIndex inside
a pair maps back to the lead before matching; an unpaired trail is unchanged.
`exec` preserves empty-match end offsets; global `replace`/`replaceAll` advance
empty matches by a whole code point. Step exhaustion remains catchable RangeError.

Explicit gaps: non-Unicode i folding, Script properties, v grammar and
lookbehind remain deferred. Unicode mode adds no size limits or dependencies.

```js accept
/^.$/u.test('🐱') && !/^.$/.test('🐱');
/[\u{10000}-\u{10FFFF}]/u.test('\u{10400}');
'🐱'.replace(/(?:)/gu, '-') === '-🐱-';
```

### Execution: Date host inputs and local fields (clj-quickjs-182)

`Date.now()` and `new Date()` read epoch milliseconds from an unrecorded leaf
host clock. Without one they throw RangeError; explicit Date values and local
construction continue to work. Multiple-argument `new Date(y, m, ...)` uses
local wall time, the same field defaults/overflow rules as Date.UTC, and the
specified 0–99 year adjustment. Only its first seven arguments are converted.
Eight local getters (`getFullYear`, `getMonth`, `getDate`, `getHours`,
`getMinutes`, `getSeconds`, `getMilliseconds`, `getDay`) and seven setters
(`setFullYear`, `setMonth`, `setDate`, `setHours`, `setMinutes`, `setSeconds`,
`setMilliseconds`) mirror the UTC methods' branding, lengths, invalid handling,
argument order and resumable coercion. `getTimezoneOffset()` returns minutes
west of UTC, preserving fractional minutes from source seconds; invalid dates
return NaN. New methods have ordinary writable/nonenumerable/configurable flags.

One host zone is frozen on the runtime's first local Date operation. NoHost
selects UTC. Validated rule-table zones cover 59 canonical names and 36 aliases
from the pinned QuickJS snapshot; inventory-only and unknown names are rejected
by the embedding API. First/last offsets extend indefinitely without future DST
extrapolation. In overlaps choose the earlier instant; in gaps use the offset
before the jump. Explicit fixed-second offsets are also available to embedders.
Explicit gaps: Date() string calls, parsing, non-ISO formatting, legacy year
methods and Date's default primitive conversion remain deferred. No engine
operation reads the OS clock or zone.

### BigInt to Number and implicit ToNumber (clj-quickjs-203)

Lexical grammar: existing BigInt literals and ordinary calls suffice; no new
syntax. Execution: `Number(bigint)` and `new Number(bigint)` round the exact
integer once to the nearest binary64 value, with ties to even. Carry into the
next binade is supported; crossing the finite rounding boundary returns signed
Infinity. BigInt zero converts to positive zero. Objects first complete
resumable ToPrimitive with number hint, including getters, waits and fallback
to `toString`, then use the same conversion.

Implicit ToNumber continues to throw catchable TypeError for BigInt primitives
and for BigInts returned by ToPrimitive. The currently implemented numeric
intrinsic entry points are:

| Entry point | BigInt rejection |
| --- | --- |
| `Math.floor`, `ceil`, `abs`, `imul`, `fround`, `hypot`, `pow`, `sqrt`, `log2` | every consumed numeric operand, in argument order; even `hypot(Infinity, 1n)` throws |
| global `isNaN`, `isFinite` | first argument; `Number.isNaN`, `isFinite`, `isInteger`, `isSafeInteger` instead return false without coercion |
| `Number.prototype.toString`, `valueOf` | BigInt receiver fails Number branding; `toString` also rejects a BigInt radix |
| `Date` construction, `Date.UTC`, `setTime`, `setUTCFullYear`, `setUTCMonth`, `setUTCDate`, `setUTCHours`, `setUTCMinutes`, `setUTCSeconds`, `setUTCMilliseconds`, `setFullYear`, `setMonth`, `setDate`, `setHours`, `setMinutes`, `setSeconds`, `setMilliseconds` | each numeric field; a setter failure leaves the date unchanged unless argument coercion itself mutates it |
| `Int8Array`, `Uint8Array`, `Uint8ClampedArray`, `Int16Array`, `Uint16Array`, `Int32Array`, `Uint32Array`, `Float32Array`, `Float64Array` | lengths/offsets, source elements, indexed stores, `set` source elements/offset and `fill` value/start/end |
| `ArrayBuffer`, `transfer`, `slice` | byte lengths and slice bounds |
| `String.fromCharCode`, `fromCodePoint`; string `charAt`, `charCodeAt`, `codePointAt`, `indexOf`, `lastIndexOf`, `substring`, `slice`, `padStart`, `padEnd`, `split` | numeric code units/points, positions, target lengths and split limit |
| global/Number `parseInt` | radix; its text argument and `parseFloat` use ToString and accept BigInt |
| `BigInt.prototype.toString` | numeric radix |
| generic array `push`, `join`, `toLocaleString`; function `apply`/`Reflect.apply`; `String.raw`; `Intl.getCanonicalLocales` | array-like `length` through ToLength; array `length` assignment/definition also rejects BigInt |
| RegExp `exec`, `test`, and string replacement using global/sticky RegExp | `lastIndex` through ToLength when consumed |

Ignored extra arguments are not coerced. Unary plus also rejects BigInt;
BigInt arithmetic and comparisons retain their separate ToNumeric/exact-value
rules. `Number` is the explicit numeric conversion exception.

Explicit gaps: BigInt wrapper objects and BigInt typed-array
storage remain deferred. This adds no Number prototype methods or Math methods.

### Decimal NumberFormat (clj-quickjs-213)

Execution: `Intl.NumberFormat(locales, options)` and `new Intl.NumberFormat(...)`
create decimal formatters in standard notation. A cached `format` getter returns
a detached callable; `resolvedOptions()` reports immutable selected settings,
and `supportedLocalesOf(locales, options)` returns supported canonical requests.
Instances inherit `Symbol.toStringTag = 'Intl.NumberFormat'`. Locale lists and
options use resumable property reads/coercions, as do object format arguments.
Numbers, BigInts and decimal strings format with locale symbols/grouping,
negative zero, NaN and infinity. Options cover localeMatcher, numberingSystem,
useGrouping (booleans, auto/always/min2), minimumIntegerDigits (1..21), fraction
digits (0..100), significant digits (1..21), and all nine rounding modes
(default halfExpand); default fraction bounds are 0..3. Invalid listed options
throw catchable RangeErrors; invalid receivers and Symbol numbers throw TypeError.

Explicit gaps: unit style and nonstandard
notations throw RangeError. Numbering-system options and nu extensions use the pinned QuickJS digit mappings;
unknown well-formed systems fall back to the selected system. Both matchers use lookup over the policy's test locales.
Parts/range methods, ChainNumberFormat legacy receivers and general
Object.prototype.toString tag consumption remain deferred. Decimal coefficient
input over 100,000 characters or exponent magnitude over 10,000 throws RangeError
when the numeric syntax is valid. Malformed strings produce localized NaN;
surrounding whitespace follows JavaScript's rules. BigInt decimal conversion
yields through the shared native conversion state.

## Date non-locale formats (clj-quickjs-183)

Execution: `Date.prototype.toString`, `toDateString`, `toTimeString`,
`toUTCString` and `toGMTString` are branded methods with length zero. They
ignore arguments, reject receivers without an own Date slot with TypeError,
and return `Invalid Date` for an invalid slot. Local formats use English
three-letter day/month names and the runtime zone's `GMT±hhmm` offset; UTC
uses `Day, dd Mon yyyy hh:mm:ss GMT`. Negative years have a minus sign and
at least four digits. The two UTC names reference the same function object.
`Date()` ignores its arguments and returns the current host time in the local
`toString` format. Its hostless clock failure is RangeError. These replace
earlier explicit formatting gaps; `String(date)` now uses the Date formatter.

Explicit gaps: parsing, default Date coercion, `toJSON`, and `toLocale*String`
remain deferred. Whole-minute truncation of the offset and local fields in
these strings follows QuickJS, while local field getters retain offset seconds.

### Unicode RegExp ignore-case execution (clj-quickjs-191)

Execution: i with u applies Unicode 17.0.0 simple folding to literals, ranges,
class escapes, word boundaries and backreferences. Scoped i can enable or
disable folding within a u pattern. Classes test case-closed membership before
inverting, so `/[^a]/iu.test('A')` is false. `\w` includes characters folding
to ASCII word characters, including long s and Kelvin sign. Identifiers and
capture names now use the same verified Unicode 17.0.0 version. Expansion
and matching preserve the catchable work limit and UTF-16 result offsets.

Explicit gaps: plain i and scoped i without u or v throw catchable SyntaxError;
Script properties and lookbehind remain deferred.

```js accept
/k/iu.test('\u212A') && /[a-z]/iu.test('\u017F');
'aAbBcC'.replace(/[^A-B]/gui, 'X') === 'aAbBXX';
```

### BigInt width wrapping (clj-quickjs-204)

Lexical grammar: ordinary calls and existing BigInt literals suffice; no new
syntax. Execution: `BigInt.asIntN(bits, value)` and `BigInt.asUintN(bits, value)`
have length 2, with writable, configurable, non-enumerable properties on
`BigInt`. ToIndex truncates `bits` toward zero (`undefined`/NaN become zero),
rejecting negative integers, infinities and indices above 2^53 - 1 with
RangeError; BigInt and Symbol widths throw TypeError. Then ToBigInt accepts
BigInt, boolean and integer strings, rejecting Numbers/null/undefined/Symbol
with TypeError and malformed strings with SyntaxError. Both object arguments
use resumable ToPrimitive with number hint, in order. A zero width still
converts the value. Ignored extra arguments are not coerced.

Unsigned results are modulo 2^bits; signed results interpret the low bits as
two's complement, including partial and complete limb widths. All results are
canonical. Widths up to 2^53 - 1 return already-fitting values without allocating
width-sized storage. In particular huge signed widths return either sign
unchanged, and huge unsigned widths return nonnegative inputs unchanged.
An unsigned conversion of a negative input needs the requested width: above
the runtime's 1 Mbit magnitude cap it throws RangeError (`BigInt is too large`)
before allocation. At exactly the cap it succeeds. This follows QuickJS's
checked-allocation behavior with this runtime's documented size limit.

Explicit gaps: BigInt wrappers and typed-array storage remain deferred.

### Intl.PluralRules (clj-quickjs-212)

Execution: `new Intl.PluralRules(locales, options)` creates an ordinary task-owned
object with immutable internal slots, inheriting `Intl.PluralRules.prototype`.
Calling without `new` throws TypeError. `select(value)` uses resumable ToNumber,
rounds the absolute decimal value with the configured fraction/significant digit
bounds, then evaluates cardinal or ordinal rules. NaN and infinities produce
`other`; BigInt and Symbol throw TypeError. Omitted/undefined values become NaN.
`localeMatcher` accepts `lookup` and `best fit` (both use lookup); `type` accepts
`cardinal` and `ordinal`. Minimum integer digits are 1..21, fraction bounds 0..100,
and significant bounds 1..21. Inconsistent bounds throw RangeError before the
rounding-mode getter runs. Default fraction bounds are 0..3; a larger minimum
increases the default maximum. Significant bounds default to 1..21 when either
is supplied. `roundingMode` defaults to `halfExpand` and accepts the same nine
values as NumberFormat. Every getter and object conversion is a saved native
continuation. Options must be undefined or an object, otherwise TypeError.

`resolvedOptions()` returns fresh ordinary fields in order: locale, type,
minimumIntegerDigits, minimumFractionDigits, maximumFractionDigits, optional
minimumSignificantDigits/maximumSignificantDigits, pluralCategories, roundingMode.
Categories use the canonical zero/one/two/few/many/other order and a fresh array.
`supportedLocalesOf` canonicalizes the requested list and accepts localeMatcher.
Both services share canonical-list processing, lookup/truncation/default fallback,
relevant extension filtering, option getter/coercion helpers, numeric validation,
supported-locale selection and ordered resolved-object construction. PluralRules
has no relevant Unicode extension keys; NumberFormat retains only supported nu.

Explicit gaps: `selectRange`, notation, compact display, rounding increment and
priority, trailingZeroDisplay and additional locale coverage remain deferred.
Unconsumed option properties have no effect. The explicit operand/ordinal
requirements of this pellet take precedence over the pinned QuickJS
`pluralrules.c` shortcuts (which ignore rounding, classify Russian fractions as
integers, and do not implement English ordinal selection). Category names and
the six upstream integer rows retain the QuickJS strings. No existing
NumberFormat output changes. Inherited toStringTag consumption by
Object.prototype.toString remains unsupported as for NumberFormat and Locale.

### Unicode RegExp property escapes (clj-quickjs-192)

Execution: in u or v patterns, `\p{Name}` / `\P{Name}` accept the 53 binary
properties in ECMA-262's Binary Unicode property aliases table, or any of the
38 General_Category values and aliases. Categories also accept `gc=Value`
and `General_Category=Value`. Spellings are exact, including lowercase aliases
such as `digit`, `cntrl`, `punct`, and `space`; loose casing, spaces, Is prefixes,
binary `Name=Yes` and unknown values throw catchable SyntaxError. Property sets
work as standalone atoms and class members, but cannot be range endpoints.
Lone surrogates participate in Cs, Any and other applicable code-point sets.
Without u or v, p/P are identity escapes, so `/\p{L}/.test('p{L}')` is true.

In iu (also scoped i inside u), `\P` complements the property before simple
folding. This differs from class inversion: `/\P{Lower}/iu.test('a')` is true,
while `/[^\p{Lower}]/iu.test('a')` is false. Matching retains the catchable
step budget and UTF-16 offsets.

Explicit gaps: Script, sc, Script_Extensions and scx deliberately throw
SyntaxError. Unicode string properties remain deferred.

```js accept
/\p{General_Category=Uppercase_Letter}/u.test('A');
/\p{Lowercase}/u.test('\u{1D41A}');
/[\p{Nd}x]/u.test('x') && /\P{Lower}/iu.test('a');
```

### Percent and currency NumberFormat (clj-quickjs-214)

Execution: NumberFormat additionally accepts percent and currency styles.
Percent adjusts the decimal exponent before rounding (including BigInt/string
inputs), with default fraction bounds 0..0. Currency defaults follow the pinned
QuickJS minor-unit rules (EUR 2..2, JPY 0..0, CLF 3..3); explicit fraction or
significant-digit options still apply. `currency` must contain exactly three
ASCII letters and is uppercased, including unknown codes. A missing currency
for currency style throws TypeError; malformed codes and invalid display/sign
options throw RangeError, even when supplied with decimal or percent style.
`currencyDisplay` is symbol (default), narrowSymbol, code or name;
`currencySign` is standard (default) or accounting. Currency settings appear in
`resolvedOptions` only for currency style. All new getters and ToString calls
use resumable native continuations, before notation and digit options.

Patterns and symbols come from the test-locale snapshot, including nonbreaking
spaces and Arabic bidi marks. The QuickJS output policy retains four-digit
Spanish grouping; narrowSymbol and name both use the regular currency symbol,
code inserts the literal code without added spacing, and nonfinite values have
no percent/currency affix. Explicit gaps: unit style throws RangeError; distinct
narrow symbols and translated currency names beyond QuickJS remain deferred.

### Execution: Date ISO parsing (clj-quickjs-184)

`Date.parse(value)` (name `parse`, length 1, ordinary method flags) converts
only its first argument with ToString using the string hint. It is not a
constructor. A single-argument `new Date(value)` copies an actual Date slot;
otherwise default-hint ToPrimitive runs, and a resulting string is parsed
rather than converted to Number. Observable getters/methods can wait, yield,
collect, throw, or migrate as ordinary VM frames.

The ISO grammar accepts four-digit years or signed six-digit expanded years
(`-000000` is invalid), optional `-MM` and then `-DD`, and optional `THH:mm`
after any of those date forms. Seconds `:ss` and then a fraction `.f+` are
optional; all fractional digits are validated and only the first three
contribute milliseconds, padding shorter fractions with zeros. A time may
end in `Z`, `±HH:mm` or `±HHmm`; these suffixes also apply directly to a
date-only form. Without a time, omitted month/day are January/1 and the value
uses UTC with any explicit offset applied. A time with no offset uses the
runtime's local zone. There is no 1900 adjustment for parsed years 0–99.
Month/day bounds are 1–12/1–31;
calendar arithmetic balances days within those bounds across month ends.
Hours/minutes/seconds must be at most 24/59/59, with hour 24 requiring zero
minutes, seconds and milliseconds. Offset hours/minutes are at most 23/59.
Trailing units, including NUL and lone surrogates, invalidate the ISO form.
Conversion uses MakeDay/MakeTime/MakeDate, the host's local-to-UTC rule when
needed, the explicit offset when present, and finally TimeClip. Failure is
NaN or an invalid Date object, rather than an unsupported-operation error.

Explicit gaps: these rules supersede earlier ISO parsing gaps. Non-ISO
fallback parsing is described in clj-quickjs-185 below; default Date primitive
conversion, `toJSON`, and locale formatting remain deferred. Parsing introduces
no new input-length cap; the existing 4096-unit Date numeric-field conversion
limit still applies only to numeric conversions.

### Execution: Date fallback parsing (clj-quickjs-185)

When ISO parsing fails, the same entry points try the QuickJS fallback grammar
over the complete string. It recognizes case-insensitive three-letter month
prefixes (with an ignored word suffix), leading weekday/other ignored words,
ASCII spaces, `-/.,` separators, and balanced nested parenthesized comments.
Stray closing parentheses and NUL are rejected, including in ignored words.
Month/day default to January/1 and the fallback year defaults to 2001.
At most three numeric date fields, counting an explicit year and month name,
are permitted. A number with more than two digits is a literal year when no
year has been set; a small number outside 1–31 instead selects a year with
0–49 mapped to 2000–2049 and 50–99 to 1950–1999. Remaining fields follow
month/day/year order without a month name, or day/year with one. Signed years
retain their value and negative zero years are rejected.

An optional `H:mm[:ss[.f]]` time accepts one or two minute/second digits,
`.` or `,` fractions of up to nine digits (truncated to milliseconds), and
case-insensitive AM/PM. Range checks and the `24:00` special case match ISO
parsing. The fixed abbreviations are GMT/UTC/UT/Z, EDT/EST/CDT/CST/MDT/MST/PDT/PST,
and WET/WEST/CET/CEST/EET/EEST. After a time, signed offsets accept hours,
compact hour/minute digits or `H:mm`, optionally following GMT. QuickJS's
non-strict handling drops excess rightmost digit pairs from compact offsets.
Unicode minus U+2212 is recognized. Without an offset or abbreviation, even a
date-only fallback form uses the runtime's local-to-UTC conversion and existing
DST gap/overlap policy. Parsed fields then use the shared calendar arithmetic
and TimeClip. `toString` and `toUTCString` round-trip whole-second values.

Explicit gaps: this supersedes earlier non-ISO parsing gaps. It introduces no
input-length cap or host clock/time-zone reads outside the embedding boundary.
QuickJS's fixed byte-buffer truncation is not reproduced. Default Date primitive
conversion, `toJSON` and locale formatting remain deferred.

## Array and argument spread (clj-quickjs-130)

Syntax: `[a, ...iterable, b]`, `f(a, ...iterable, b)`,
`o.m(...iterable)` and `new F(...iterable)` accept spread operands at
AssignmentExpression precedence. Multiple spread operands and trailing commas
are supported. The ellipsis is part of the element's AST span.

Execution: each spread consumes the existing synchronous iterator protocol,
including user overrides of `Symbol.iterator`, cached `next`, object result
validation, and resumable `done`/`value` getters. Iteration finishes before the
following element or argument is evaluated. Holes yield own `undefined`
elements (or inherited values); array iterators observe the current length.
Method calls preserve their receiver. Errors obtaining or advancing the
iterator are catchable TypeErrors and are located at the ellipsis. Errors
thrown by JavaScript retain their own source location. Exhaustion and
`next`/`done`/`value` failures do not call `return`.

The compiler retains the 65,535 syntactic argument limit (`TooLarge`). Packed
spread arguments are checked after each append; more than 65,535 values throw
a catchable RangeError. A list that cannot fit the task's value stack throws
the existing catchable StackOverflow at the call. The private packed array is
never exposed to JavaScript or re-iterated when the call starts.

Rejected forms: object spread, rest parameters and a standalone
`(...a)` retain their syntax errors. Explicit gaps: compiler-reserved `__rt`
intrinsics require statically supplied arguments and reject spread with
`Unsupported`. Dynamic Function construction retains its explicit rejection.
Direct and indirect eval support ordinary and spread calls.

Synthetic fixtures, not light-cljs output:

```js accept
let a = [1, ...[2, 3], 4];
let holes = [...[,]];
function f(a, b, c) { return a + b * 10 + c * 100; }
f(...[1, 2], 3);
let o = {m(x) { return this === o && x === 1; }};
o.m(...[1]);
new f(...[1, 2, 3]);
```

```js reject Unsupported 1:2
(...a);
```

### RegExp match indices (clj-quickjs-193)

Execution: literals and constructors accept `d`; `hasIndices` reports its
internal flag and `flags` includes it first. Successful `exec` results gain
an own writable/enumerable/configurable `indices` array. Entry zero covers
the whole match; later entries cover numbered captures in the same order as
the match result. Matched entries are two-element `[start, end]` arrays of
UTF-16 offsets, including zero-width matches and captures in lookahead.
Unmatched entries are present undefined elements, never holes.

`indices.groups` is an own writable/enumerable/configurable property:
undefined without named groups, otherwise a null-prototype object. Its
named properties reference the exact numbered pair objects; duplicate names
in disjoint alternatives select the participating capture. Every exec
allocates fresh pairs. Without `d`, the match result has no own `indices`.
Global/sticky lastIndex behavior and Unicode surrogate handling are unchanged.
Indices construction consumes the search's existing work allowance and
exhaustion throws catchable RangeError before result allocation.

Explicit gaps: lookbehind, non-Unicode ignore-case matching
and the previously documented symbol-based dispatch remain deferred.

## Array.from (clj-quickjs-207)

Execution: `Array.from(items[, mapFn[, thisArg]])` validates the mapper first,
then reads `items[Symbol.iterator]` once. A non-nullish method selects the shared
GetIterator protocol; each next result must be an object, and `done` precedes
`value`. Otherwise `ToLength(items.length)` selects indexed array-like reads,
including inherited properties and getters. Each source value creates an own
data element (including undefined for holes). Mapping calls
`mapFn.call(thisArg, value, index)` before appending that element. Iterator
exhaustion does not call `return`; mapper or element-creation exceptions close
once and preserve the original exception over errors/results from `return`.
Failures of next/done/value do not close. All getters and callbacks are ordinary
resumable frames. Each element consumes native steps, including array-like
holes. Null/undefined inputs and noncallable mappers/methods throw TypeError.
Array-like lengths use resumable ToNumber then ToLength, with RangeError above
`2^32 - 1`; iterable output has the same existing array length bound.

Explicit gaps: `Array.from.call(C, ...)` with custom/subclass or bound
constructors fails explicitly with UnsupportedOperand pending subclass support.
Array and nonconstructor receivers return plain arrays. Boolean wrappers
are unavailable; Boolean and Symbol primitives have no
observable length/iterator and produce empty arrays. Async iteration and
`Array.fromAsync` remain unsupported.

### RegExp Unicode sets (clj-quickjs-194)

Execution: `v` selects Unicode mode outside classes, with the same code-point
consumption and UTF-16 indices as `u`. Inside classes it enables nested sets,
union by adjacency, intersection chains (`&&`), subtraction chains (`--`),
and `\q{...|...}` finite strings, including empty strings. Different operations
require nested classes; ranges cannot be operands of intersection/subtraction.
Each class tries longer strings before shorter strings, single code points,
and finally the empty string, retaining alternatives for backtracking.

In `vi`, operands and input use Unicode 17 simple folding before union,
intersection, subtraction and complement. In particular, `\P{Lower}` is the
complement of folded Lower, unlike `iu`'s complement-before-folding rule.
Scoped `i` uses the same set operations. `v` and `u` are mutually exclusive;
`unicodeSets` and `unicode` expose their respective flags.

Unescaped `()[]{}/-|` inside sets and reserved double punctuation throw
catchable SyntaxError. Escaped class syntax and reserved punctuation are
accepted. A negated set is rejected when static MayContainStrings is true:
union combines it with OR, intersection with AND, and subtraction retains
the left operand's value. This includes empty class strings and rejects
negation even if subtraction would remove all actual strings. Parsing and
set traversal use explicit stacks; compilation and matching retain their
catchable RangeError work limits.

Explicit gaps: properties of strings such as `\p{RGI_Emoji}` deliberately
throw SyntaxError. Script/Script_Extensions, lookbehind, non-Unicode folding,
and symbol-based string dispatch retain their existing deferred status.
Literal pattern errors retain the current catchable construction-time policy.

```js accept
/[\q{abc|ab|a}]/v.exec('abcd')[0] === 'abc';
! /[[a-z]--[aeiou]]/v.test('e');
'aAbBcC#4'.replace(/\P{Lower}/gvi, 'X') === 'aAbBcCXX';
```

### Collector weak edges (clj-quickjs-172)

Execution: the collector supports heap-internal weak slots and ephemeron
tables, clears dead conditional edges before sweeping, and hands notifying
slots to observers after the pause with rooted owners. This introduces no
JavaScript syntax or globals and does not change ordinary Map semantics.
WeakRef and its job keep-alives are supported (-176). Explicit gaps:
JavaScript WeakSet and FinalizationRegistry registration and cleanup
scheduling remain deferred.

## Array.prototype.sort (clj-quickjs-208)

Execution: `array.sort([compareFn])` validates a defined comparator before
reading length or elements, collects present own/inherited indices, and stably
sorts the snapshot. Default comparison applies ToString to each compared value
and compares UTF-16 code units. A callback receives `(a, b)` with undefined
`this`; its result undergoes ToNumber, with NaN treated as zero. Undefined
values compare after all defined values without invoking the comparator.
Sorted values are written back with throwing Set operations, then remaining
indices are deleted with throwing Delete operations; length is preserved and
the receiver is returned. Object coercions, accessors and callbacks execute as
resumable VM calls. Array-like object receivers and Number/String/Symbol wrappers
are supported. Long default comparisons flatten ropes and compare in bounded
chunks, beyond the general synchronous rope limitation above.

Explicit gaps: typed-array `sort` and `toSorted` remain deferred. Generic sort
on Boolean/BigInt primitives explicitly rejects unavailable wrappers.

## Generator suspension examples (clj-quickjs-244)

These synthetic fixtures, not light-cljs output, use the saved operand stack
and handlers. Both syntax examples parse through `grammar_doc_examples`; the
source integration tests execute them and check the iterator result rows.

```js accept
function* operands(a, b, c) { return a + b * (yield c); }
let g = operands(2, 3, 4);
let first = g.next();
let last = g.next(10);
first.value === 4 && !first.done && last.value === 32 && last.done
```

```js accept
function* cleanup() {
  try { return 1; }
  finally { yield 2; }
}
let g = cleanup();
let first = g.next();
let last = g.next();
first.value === 2 && !first.done && last.value === 1 && last.done
```

### Intl.Collator root order (clj-quickjs-218)

Execution: `Intl.Collator(locales, options)` and `new Intl.Collator(...)` create
ordinary task-owned instances with immutable internal options. The `compare`
accessor returns the same bound two-argument function on every read; detached
calls convert their arguments to strings in left-to-right order, allowing waits
inside object hooks. `resolvedOptions()` returns locale, usage, sensitivity,
ignorePunctuation, collation (`default` or German `phonebk`), numeric and
caseFirst, in that order.
`Intl.Collator.supportedLocalesOf` shares the canonical locale-list and matcher
helpers. Instances inherit `Symbol.toStringTag = 'Intl.Collator'`.
The detached function is an `Array.prototype.sort` comparator: en/de root
sorting of `['z','a','Z','A']` gives `a,A,z,Z`; equal collation keys retain
their input order, including base-sensitive and canonically equivalent ties.

Options: `localeMatcher` lookup/best fit, `usage` sort/search, `sensitivity`
base/accent/case/variant, `ignorePunctuation` boolean, `numeric` boolean and
`caseFirst` upper/lower/false. The supported `kn` and `kf` Unicode keys provide
defaults; conflicting explicit options remove their keys from the resolved
locale. Malformed collation options and invalid enumerated values throw
RangeError; well-formed unsupported collations resolve to default. Option
objects use the shared undefined-or-object policy. Wrong receiver brands and
symbol compare arguments throw TypeError; foreign task objects are rejected.

Explicit gaps: locales use root rules except the German/Turkish mappings
described below. Primary weights are folded NFD scalar order,
secondary weights preserve the snapshot's combining-mark order, and case-only
tertiary weights use the pinned Unicode Uppercase property, lowercase first
(or explicit upper). Same-case variants remain equal at case sensitivity;
only variant sensitivity uses the final NFD scalar fallback. Canonical combining-class
ordering applies to the full snapshot's NFD data, including Hangul and non-BMP
characters; lone surrogates remain distinct. Punctuation filtering follows the
snapshot's ASCII alphanumeric / non-ASCII ID_Start rule. Numeric runs are ASCII
digits, compared by significant length and digits without integer conversion;
leading zeroes do not break ties. These root rules do not supply full UCA/DUCET
weights. Unlike the snapshot's unused numeric flag and
uncached getter, numeric comparison and function identity follow this pellet's
explicit acceptance requirements. Full folding is completed to an idempotent
closure, including capital sharp S; collation-option rope validation is budgeted.

## Generator delegation examples (clj-quickjs-245)

Synthetic fixtures, not light-cljs output. Yield* keeps iterator protocol state
in traced activation locals, supports arrays and user iterables, preserves result
identity and evaluates to the delegate's final value. Return/throw forwarding
awaits the currently absent generator prototype methods.

```js accept
function* inner() { let received = yield 1; return received + 2; }
function* outer() { return 10 + (yield* inner()); }
let g = outer();
let a = g.next();
let b = g.next(3);
if (a.value !== 1 || a.done || b.value !== 15 || !b.done) throw 'delegation';
```

```js accept
function* array() { return yield* [1, 2]; }
let g = array();
if (g.next().value !== 1 || g.next().value !== 2) throw 'array yields';
let end = g.next();
if (!end.done || end.value !== undefined) throw 'array completion';
```

### Execution: WeakMap (clj-quickjs-173)

`new WeakMap()` and nullish constructor arguments create empty task-owned
WeakMaps. `set(key, value)` accepts objects and non-registered symbols and
returns the map; primitives and `Symbol.for` keys throw catchable TypeErrors.
`get`, `has` and `delete` compare private identities without coercion; absent
or invalid keys return undefined, false and false respectively. Methods
validate receiver brand and task ownership before inspecting the key.
Ordinary own properties, prototypes and integrity operations use the same
rules as Map. Freezing properties does not freeze entry mutation.
The prototype exposes `Symbol.toStringTag` equal to `WeakMap`.

Entries are ephemerons: a reachable map retains a value only while its key is
independently reachable, including symbol keys reached through live values,
property keys, closures, frames or host roots. Dead-key entries and their
otherwise unreachable values disappear in the same collection, including
self cycles and cycles between tables. WeakMaps cannot be published across
tasks, directly or through persistent values or captures.

Explicit gaps: non-nullish iterable construction throws TypeError, matching
Map's current constructor boundary. WeakSet and FinalizationRegistry
remain deferred. WeakMap has no enumeration, size, clear or iterator API;
the host-only entry-count test hook is not a JavaScript property. Generic
Object.prototype.toString symbol-tag consumption remains deferred.

### Intl.Collator German and Turkish tailorings (clj-quickjs-219)

Execution: German `de-u-co-phonebk` or an explicit `{collation:'phonebk'}`
selects phonebook primary/secondary keys: ä/ö/ü become ae/oe/ue, with equivalent
uppercase and canonically decomposed spellings. Variant sensitivity retains
the original NFD fallback. German default sort uses a/o/u followed by the
snapshot's small intervening umlaut weight; German search uses phonebook
weights while reporting collation `default`. Turkish `tr` orders ı before i,
and I before İ at default sensitivity. Other locales retain root ordering.
`resolvedOptions().collation` reports `phonebk` only for supported German sort;
the resolved locale preserves `-u-co-phonebk` when selected by that extension.
Explicit collation options win over the extension. Well-formed unsupported
collations resolve to `default` and drop `co`, preserving independent `kn`/`kf`
resolution. Malformed collation options still throw RangeError.

Explicit gaps: the pinned snapshot's Turkish mapping changes dotless ı only;
I/İ retain root folding, so full locale-specific uppercase equivalences are
deferred. Azerbaijani is not in the agreed locale set. Additional collations
and UCA/DUCET weights remain deferred.

### Intl.DateTimeFormat date fields (clj-quickjs-215)

Execution: `Intl.DateTimeFormat(locales, options)` works with or without `new`.
`format` is a cached bound getter; `format(value)` performs resumable ToNumber
and TimeClip, and `format()` reads the host clock. `resolvedOptions`,
`supportedLocalesOf` and `Symbol.toStringTag` are provided. `weekday` and `era`
accept long/short/narrow; `year` and `day` numeric/2-digit; `month` also accepts
long/short/narrow. Missing date and time fields default to numeric year/month/day.
Locale resolution uses `ca`/`hc`/`nu`, and options include `localeMatcher`,
`calendar`, `numberingSystem`, `hour12`, `hourCycle`, `timeZone`, `formatMatcher`.
The service shares Date's cached host zone and accepts UTC, fixed minute offsets
and the host's named zone/aliases (matched without ASCII case). Date names and numeric digits reproduce the
pinned QuickJS snapshot, including its manual component layout.

Explicit gaps: calendars other than `gregory` and explicit `timeZoneName`
components throw RangeError when supplied, after value validation.
Named zones outside the host selection throw RangeError. No host clock means
`format()` throws RangeError; NaN, infinities and out-of-TimeClip values do too.
Style formatting is described under -217; formatToParts and ranges are deferred.

### Intl.DateTimeFormat time fields (clj-quickjs-216)

Execution: `hour`/`minute`/`second` accept numeric/2-digit;
`fractionalSecondDigits` converts to a number, floors and validates 1–3;
`dayPeriod` accepts long/short/narrow. Any of these suppresses default date fields.
`hourCycle` supports h11 (0–11), h12 (1–12), h23 (0–23), h24 (1–24).
Precedence is `hour12`, explicit `hourCycle`, `hc` extension, snapshot short time
pattern default. Following QuickJS, Japanese `hour12: true` chooses h11 unless
an explicit `hourCycle` was supplied, when it chooses h12; false chooses h23.
The hour cycle and boolean `hour12` are reported only when `hour` is present.
All option getters and conversions can wait and resume.

Manual time fields reproduce QuickJS's colon separators, neighboring-field
padding and trailing locale am/pm/day-period names. Explicit day periods use
the snapshot's five hour buckets (6–11, 12, 13–17, 18–20, otherwise night), with
short/long sharing a pool. en-US uses U+202F before the period; other locale
tags use a space. Fractions truncate milliseconds, use Arabic decimal U+066B
with arab digits and a period otherwise, and support standalone formatting.
Time components use Date's existing host zone and transition rules.
Explicit `timeZoneName`, formatToParts and ranges remain gaps.

### Intl.DateTimeFormat styles (clj-quickjs-217)

Execution: `dateStyle`/`timeStyle` each accept full/long/medium/short. Snapshot
date/time patterns and the date style's combined template determine ordering,
padding, punctuation and quoted literals. Names and numeric digits use the
locale pools. Time styles apply the resolved hour cycle to pattern hours;
24-hour cycles suppress pattern day periods and their preceding separator.
Styles suppress numeric date defaults. Either style with any explicit weekday,
era, year, month, day, dayPeriod, hour, minute, second, fractionalSecondDigits
or timeZoneName throws TypeError after option validation/conversion.
`resolvedOptions()` reports styles, omits component fields, and includes
hourCycle/hour12 when timeStyle is present.

Full/long time styles include snapshot zone output: UTC's short/English long
names, raw fixed-offset identifiers, Vienna's English standard/summer names,
and GMT offsets for other host-selected named zones. DST uses the instant's
offset. Explicit gaps: independent `timeZoneName` components still throw
RangeError; non-Gregorian calendars, other named zones, parts and ranges remain
unsupported as above. Styles use the existing generated pools unchanged.

## Async execution (clj-quickjs-248)

Async declarations, function expressions, arrows and methods execute. Calls create
an intrinsic Promise and run synchronously through parameters and the body to the
first await. `await v` uses intrinsic PromiseResolve and internal PerformPromiseThen;
even non-promises resume in a later FIFO job, on the activation's owning task.
Native promises do not require a `then` property lookup. Thenables follow the
existing resumable getter and resolve-thenable job protocol. Fulfilment is the
expression's value; rejection throws at await. Return resolves (adopting promises
and thenables); uncaught throw rejects. Operands and pending catch/finally
completions survive suspension. Async arrows preserve lexical `this`.

Dependency reads remain whole-task VM waits and run no jobs while Waiting.
Await never parks a task. Once its inbox is empty, a task completes and reports
suspended function names through `Task::suspended_async_activations()`. This list
contains diagnostic strings only and does not retain saved activations.
Explicit gaps: async generators and top-level await are unsupported.

```js accept
(async () => 2 + await 3)().then(x => __rt.effect(0, x));
```

### Unhandled Promise rejections (clj-quickjs-249)

Syntax: existing Promise methods and async functions suffice; no new syntax.
Execution: HostPromiseRejectionTracker's reject/handle transitions record
rejected Promises without reactions and remove them when any `then` reaction
attaches. An absent rejection handler transfers rejection to the derived
Promise. `await` installs an internal reaction, so catching an awaited
rejection does not report it. A throw from `catch` rejects its derived Promise.

The embedding policy is `RunOutcome::Complete(value)` plus the queryable
`Task::unhandled_rejections()` snapshot, checked only at quiescence after jobs
and timers drain. Before completion the query is empty, including during a
budget yield or dependency wait. It holds at most 30 Promise/reason pairs;
`dropped` counts overflow without rooting additional values. Later handling
removes stored records or decrements overflow. The binary reports remaining
rejections on stderr and exits with status 1. Failure/cancellation discard them.
Rejected forms are unchanged. Explicit gaps: no JavaScript-visible rejection
events or host rejection callback; unresolved awaits alone are not rejections.

Synthetic fixtures, not light-cljs output. These complete normally in the
embedding API; only the first has a remaining diagnostic (reason 1):

```js accept
Promise.reject(1); 42;
```

```js accept
let p = Promise.reject(2); Promise.resolve().then(() => p.catch(() => {})); 42;
(async () => { try { await Promise.reject(3); } catch (e) {} })();
```

### Indirect eval (clj-quickjs-106)

EBNF is unchanged: `eval` is an Identifier reference, and `(0, eval)(s)`,
`globalThis.eval(s)` and an alias `const e = eval; e(s)` use ordinary Call
syntax. Syntactic `eval(s)` calls use the caller's scope when their callee
value is the intrinsic eval function (see "Direct eval" below).

Execution: `eval` is a writable, configurable, non-enumerable property of the
task's global object. The function's length is 1 and name is `"eval"`.
A non-string first argument is returned unchanged (including `undefined`
when omitted). String input is compiled as a global script, initially sloppy;
a `"use strict"` directive makes it strict. Its completion value is returned.
Sloppy eval's `var` and top-level function declarations create configurable
global properties; existing non-configurable script globals retain their
flags. Eval `let`/`const` bindings, and strict eval `var`/function declarations,
are local to the eval frame. Closures may retain those local bindings. Caller
function bindings and caller strictness do not enter indirect eval's scope.
Compilation errors become catchable `SyntaxError` instances with positioned
`<eval>` messages; runtime diagnostics use the eval code's source map.

The native continuation prepares UTF-16 source and flattens ropes in budgeted
steps. The synchronous frontend runs on a temporary 2 MiB compiler thread
with no heap or waiter lock, so VM workers can use 64 KiB stacks;
eval input is limited to 16,384 UTF-16 code units (catchable `RangeError` above
that limit), bounding that final compilation step. Raw unpaired surrogates in
source currently throw `SyntaxError`; Unicode escapes within source literals
remain supported. Compiled code crosses the existing immutable `Arc` boundary
and runs in a fresh task-owned VM frame, so instruction yields, dependency
waits, collection and worker migration follow the ordinary VM protocol.

Indirect eval shares the task's global declarative environment: it reads and
writes script `let` bindings, reads `const` bindings, and enforces their TDZ
and const-assignment errors. A non-strict eval `var` or function declaration
conflicting with a script lexical throws SyntaxError. All global declaration
checks precede property creation, function replacement and body effects.
Refused eval object declarations throw TypeError. Eval-local lexical
declarations never become global properties or escape into that environment.
Global lexical bindings cannot be deleted. Immutable const captures retain
their publication behavior; script let captures share mutable binding cells.

Explicit gaps: sloppy direct-eval declaration hoisting and
parameter-environment eval remain -108/-109.

```js accept
const e = eval;
e('1 + 1') === 2 && (0, eval)('if (0) 2; else 3;') === 3;
(0, eval)('var q = 1');
Object.getOwnPropertyDescriptor(globalThis, 'q').configurable;
let outer = 7;
(0, eval)('outer = 9');
outer === 9 && (0, eval)('outer') === 9;
```

```js accept
eval('1 + 1');
```

### Math.sumPrecise (clj-quickjs-158)

Execution: `Math.sumPrecise(items)` consumes a synchronous iterable of Numbers
and rounds its exact sum once to binary64, ties to even. It has length 1,
ignores `this` and extra arguments, and is not a constructor. Empty inputs and
inputs containing only negative zeros return -0; other exact zero sums return
+0. NaN or both signs of
Infinity produce NaN; otherwise an input Infinity determines the result.
Finite intermediate overflow is retained exactly until the final rounding.
Non-Number elements throw TypeError without coercion and close the iterator;
iteration continues after NaN/Infinity to validate the remaining elements.
Getters, next and return may yield, wait, survive GC and migrate. Iterator step
errors propagate without closing; a pending validation error wins over close
errors. At 2^53 elements a RangeError closes the iterator, per the
[Math.sumPrecise specification](https://tc39.es/ecma262/multipage/numbers-and-dates.html#sec-math.sumprecise).
Explicit gaps: async iterables are not accepted by this synchronous method.
Other Math methods/constants retain their existing supported/deferred status.

### Execution: optional simulated host facilities (clj-quickjs-175)

After the embedder calls `Task::install_sim_host_bindings()`, a fixture can
call `std.gc()` to collect synchronously before its next statement, and
`os.setTimeout(callback, delay)` / `os.clearTimeout(id)` to manage task-local
virtual timers. These namespace properties are absent by default. GC returns
undefined; timer registration returns an increasing task-local numeric ID.
Collection notifications are processed before the requesting task resumes.
The script body runs first, then jobs drain, then the earliest timer runs as
a resumable callback job; jobs it enqueues drain before the next timer.
Equal due times preserve registration order. Completion waits for all timers
and preserves the body's result. Timer callbacks can yield, wait and migrate.

Delays use resumable ToNumber conversion, then truncate finite milliseconds
and clamp negative values to zero. Omitted delay is zero. Nonfinite/unsafe
delays or virtual-time overflow throw RangeError; noncallable callbacks throw
TypeError. Clear cancels an existing ID and ignores unknown IDs. Timers and
captures are precise roots; task failure/cancellation discards pending timers.
No timer uses real time or sleeps.

Explicit gaps: these are test/host bindings, not QuickJS module imports or
new default engine globals. Intervals, string callbacks, extra callback
arguments, automatic collection and Date-clock coupling remain deferred.
Collection points inside an enclosing heap scope/native operation/additional
run guard, or reentered from the same collection's observer, fail explicitly.
WeakRef/FinalizationRegistry semantics remain owned by their separate pellets.

### Execution: WeakRef (clj-quickjs-176)

`new WeakRef(target)` accepts objects and symbols other than `Symbol.for`
symbols. Other primitives, a missing argument, and calling `WeakRef` without
`new` throw TypeError. `WeakRef.prototype.deref()` has length 0, checks the
receiver's internal brand and owning task, and returns the live target or
`undefined` after the collector clears its slot. The prototype has a
non-writable, non-enumerable, configurable `Symbol.toStringTag` of `"WeakRef"`.
Construction and successful deref keep the target alive until the end of the
owning task's current script body or job. A yield, worker migration or wait on
`__rt.need` does not end that job. Strong targets remain available across jobs;
collection can clear targets after their last strong reference and kept-object
entry disappear. WeakRef instances and ordinary properties remain task-owned.

Explicit gaps: publishing WeakRefs across tasks is deferred, including frozen
instances. With optional simulated host bindings installed, `std.gc()` preserves
the active job's kept objects, and `os.setTimeout()` starts a later job after
the kept set clears. This adds no FinalizationRegistry API (-177) or automatic
collection trigger.

### Symbol prototype and wrapper execution (clj-quickjs-129)

Syntax uses existing member access and calls: `symbol.toString()`,
`symbol.valueOf()`, `symbol.description`, `symbol[Symbol.toPrimitive](hint)`
and `Object(symbol)`. No grammar or rejected syntax changes are needed;
`new Symbol()` remains accepted syntax that throws TypeError at runtime.

Execution: primitive reads start at the immutable realm `Symbol.prototype`
with primitive `this`. `toString` returns `Symbol(description)` without losing
UTF-16 code units; `valueOf` and `[Symbol.toPrimitive]` return the symbol.
The description getter returns undefined only for a missing description.
These methods also accept branded Symbol wrappers; other receivers, including
Symbol.prototype and objects merely inheriting from it, throw TypeError.
`Object(symbol)` creates distinct mutable task-owned objects with `typeof`
`"object"`, an immutable internal symbol slot and the Symbol prototype.

Conversion on Symbol wrappers reads/calls `[Symbol.toPrimitive]` with the
`string`, `number` or `default` hint in ordinary resumable VM frames. A nullish
method falls back to ordinary conversion; non-callable methods and object
results throw TypeError. `String(symbol)` alone permits direct descriptive
conversion. Implicit ToString/ToNumber of a symbol, including the symbol
returned by default wrapper conversion, throws catchable TypeError at the
existing operator-token position.

Explicit gaps: general object Symbol.toPrimitive dispatch and general
Object.prototype.toString Symbol.toStringTag consumption remain deferred.
Boolean/BigInt wrappers and sloppy-function receiver boxing remain deferred.

Synthetic fixture, not light-cljs output:

```js accept
let sym = Symbol('d');
let boxed = Object(sym);
sym.toString() === 'Symbol(d)' && String(sym) === 'Symbol(d)';
boxed.valueOf() === sym && boxed.description === 'd';
Object.getPrototypeOf(boxed) === Symbol.prototype;
Symbol().description === undefined && typeof boxed === 'object';
```

### Execution: adapted upstream async fixtures (clj-quickjs-250)

`tests/quickjs_async.rs` executes the pinned QuickJS `test_generator`,
`test_async_gc` and `test_async_promise_rejection` cases as source fixtures,
with verbatim case bodies and assertion helpers. No syntax or representation
changes are introduced. The generator fixture preserves `"use strict"`;
the async fixtures use the embedder's opt-in std/os bindings instead of module
imports. Completion drains jobs and virtual timers, and the harness checks
unhandled rejections as well as direct failures. Fixture headers enumerate
extraction ranges and every adaptation; provenance is in `docs/architecture.md`.

Rejected forms and explicit gaps remain as documented above: these fixtures
do not add module import support or generator `return()`/`throw()` methods.
The pinned generator case exercises `next()`, delegation, and operand/handler
preservation across `yield` without calling those methods.

### Direct eval (clj-quickjs-107)

EBNF is unchanged: an Identifier callee `eval` uses ordinary Call syntax,
including parentheses around the identifier and spread arguments. At runtime,
the callee must be the intrinsic eval function to select direct eval. A
shadowing function is called normally with all evaluated arguments. Extra
arguments to intrinsic eval are evaluated and ignored; a missing or non-string
first argument follows the indirect-eval rule above. Aliases, member calls,
bound calls, and comma-expression callees remain indirect.

Execution: runtime compilation inherits caller strictness; a `"use strict"`
directive can also make the eval source strict. Eval reads and writes the
caller's visible lexical and variable bindings, including parameters, implicit
`arguments`, enclosing closures, catch bindings, and the current loop
iteration's `let` cells. `this` comes from the caller, lexically through arrows.
Receiver transport does not evaluate `this`: shadowed calls and non-string
eval inside arrows work without reading the receiver. Eval retains the
originating receiver mode even when its source or an intervening arrow has
a strict directive. Reading a primitive/undefined sloppy receiver keeps the
existing explicit UnsupportedOperand gap for sloppy substitution.
Const writes throw TypeError, uninitialized reads/writes throw ReferenceError,
and a named function expression's immutable name rejects writes with TypeError.
Strict eval's own `var` and function declarations stay in the eval frame;
lexical declarations always do. Its result is the script completion value.

Each call site carries a validated scope descriptor in its executable.
Bindings exposed to eval use task-owned cells, including those only used in
runtime source and those belonging to enclosing functions. Closures over these
cells cannot be published, including closures created by eval. Functions with
no direct eval in their lexical subtree retain the existing capture optimizations.
Descriptors contain no heap references and survive executable spec round trips.
The native compiler continuation traces its captured environment; eval executes
in a fresh task-owned frame and can yield, wait, collect, and migrate, including
on 64 KiB worker stacks. Source preparation and limits match indirect eval.

Rejected forms and explicit gaps: unshadowed direct eval in parameter
expressions is rejected by the compiler with Unsupported (-109). A shadowed
callee named `eval` remains an ordinary call there; passing intrinsic eval
to that call is rejected at runtime before argument evaluation. Sloppy eval
with `var` or top-level function declarations is rejected during runtime
compilation with a catchable SyntaxError until caller hoisting lands (-108);
sloppy eval without those declarations works. `new.target` (-99) and `super`
(-112) remain unsupported everywhere, including runtime eval source.

```js accept
function f() { let x=1; eval('x += 2'); return x; }
f() === 3;
function receiver() { return (() => eval('this'))(); }
receiver.call(5) === 5;
let readers=[];
for (let i=0; i<3; i++) readers.push(eval('() => i'));
readers[0]() === 0 && readers[1]() === 1 && readers[2]() === 2;
```

```js compile-reject Unsupported 1:16
function f(x = eval('1')) {}
```

## Base classes (clj-quickjs-136)

Syntax: named class declarations are lexical declarations in statement lists;
class expressions have an optional name. Class bodies are strict, including
parameters and computed keys. Elements support ordinary, generator and async
methods, static methods, getters and setters, literal and computed property
names, and empty semicolon elements. `static()`, `get()`, `set()` and `async()`
are ordinary instance methods. A non-computed instance `constructor` is the
constructor; a missing constructor has no parameters and returns its new receiver.

Execution: declarations have a mutable outer binding with a temporal dead zone.
A named class also has an independent immutable inner name binding, captured by
its constructor and methods. Computed keys execute in order while this inner
binding remains uninitialized. The binding is initialized after all elements.
Constructors require `new`, including through call/apply/bind and spread calls.
A constructor's `prototype` is non-writable, non-enumerable and non-configurable;
its prototype's `constructor` is writable, non-enumerable and configurable.
Methods and accessors are non-enumerable and configurable; methods are writable.
Their home objects are the instance prototype or the class for static elements.
Ordinary methods and accessors have no constructor capability. All callbacks
use resumable VM frames, including constructors and computed-key conversion.

Rejected forms: duplicate constructors, accessor/async/generator constructors,
and non-computed static methods/accessors named `prototype` are early `Invalid`
SyntaxErrors. Declarations in single-statement positions are `Invalid`.
Inheritance, `super`, fields and static blocks are explicitly unsupported.
Explicit gaps: derived constructors and `super()` (-137), super properties
(-138), instance/static/private fields and static initialization blocks (-139).

```js accept
class Point { constructor(x) { this.x = x; } get y() { return this.x; } static origin() { return new Point(0); } }
let p = new Point(3);
p.y === 3;
```

```js accept
let C = class Inner { static self() { return Inner; } ['m']() { return 7; } };
C.self() === C && new C().m() === 7;
```

```js reject Invalid 1:28
class C { constructor() {} constructor() {} }
```

```js reject Invalid 1:15
class C { get constructor() {} }
```

```js reject Unsupported 1:9
class C extends B {}
```

```js reject Unsupported 1:11
class C { x; }
```
