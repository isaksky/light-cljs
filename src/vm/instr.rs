//! The instruction format.
//!
//! A new, compact byte format (not QuickJS-compatible). Each instruction is
//! one opcode byte followed by its fixed-width little-endian operand:
//!
//! | Opcode          | Byte | Operand            | Stack (before → after) | Meaning |
//! |-----------------|------|--------------------|------------------------|---------|
//! | `Const`         | 0x01 | u16 constant index | → v                    | push constant |
//! | `Undefined`     | 0x02 | –                  | → undefined            | push `undefined` |
//! | `GetLocal`      | 0x03 | u16 local index    | → v                    | push local |
//! | `SetLocal`      | 0x04 | u16 local index    | v →                    | pop into local |
//! | `Pop`           | 0x05 | –                  | v →                    | discard |
//! | `Dup`           | 0x06 | –                  | v → v v                | duplicate top |
//! | `Add`           | 0x07 | –                  | a b → a+b              | number addition |
//! | `Sub`           | 0x08 | –                  | a b → a-b              | number subtraction |
//! | `Mul`           | 0x09 | –                  | a b → a*b              | number multiplication |
//! | `Lt`            | 0x0A | –                  | a b → a<b              | number less-than |
//! | `StrictEq`      | 0x0B | –                  | a b → a===b            | JavaScript `===` |
//! | `Not`           | 0x0C | –                  | v → !v                 | JavaScript `!` (ToBoolean) |
//! | `Jump`          | 0x0D | u32 byte offset    | –                      | unconditional branch |
//! | `JumpIfFalse`   | 0x0E | u32 byte offset    | v →                    | branch if ToBoolean(v) is false |
//! | `Return`        | 0x0F | –                  | v →                    | complete the task with v |
//! | `NewObject`     | 0x10 | –                  | → obj                  | allocate a fresh plain object |
//! | `HostEffect`    | 0x11 | u16 host op        | arg → result           | leaf host effect (see [`crate::host`]) |
//! | `NewVector`     | 0x12 | u16 count n        | v1 … vn → vec          | new vector of the n values, in order |
//! | `NewMap`        | 0x13 | u16 entry count n  | k1 v1 … kn vn → map    | new map; a later duplicate key wins |
//! | `Count`         | 0x14 | –                  | coll → n               | element or entry count |
//! | `Nth`           | 0x15 | –                  | vec i → v              | vector element at index `i` |
//! | `Get`           | 0x16 | –                  | coll key → v           | lookup; fails if absent |
//! | `GetOr`         | 0x17 | –                  | coll key d → v         | lookup; `d` if absent |
//! | `Assoc`         | 0x18 | –                  | coll key v → coll'     | persistent assoc |
//! | `Dissoc`        | 0x19 | –                  | map key → map'         | persistent dissoc |
//! | `Conj`          | 0x1A | –                  | vec v → vec'           | persistent append |
//! | `PopLast`       | 0x1B | –                  | vec → vec'             | persistent remove-last |
//! | `NewVectorBuilder` | 0x1C | –               | → b                    | new task-owned vector builder |
//! | `NewMapBuilder` | 0x1D | –                  | → b                    | new task-owned map builder |
//! | `BuilderConj`   | 0x1E | –                  | b v → b                | append to a vector builder in place |
//! | `BuilderAssoc`  | 0x1F | –                  | b key v → b            | set an index or key in a builder in place |
//! | `Freeze`        | 0x20 | –                  | b → coll               | persistent value of a builder; invalidates it |
//! | `Div`           | 0x21 | –                  | a b → a/b              | number division |
//! | `Rem`           | 0x22 | –                  | a b → a%b              | number remainder (sign of the dividend) |
//! | `Neg`           | 0x23 | –                  | a → -a                 | number negation |
//! | `ToNumber`      | 0x24 | –                  | v → n                  | JavaScript ToNumber of a primitive |
//! | `ToNumberPair`  | 0x25 | –                  | a b → n m              | ToNumber of both, `a` first |
//! | `Gt`            | 0x26 | –                  | a b → a>b              | number greater-than |
//! | `Le`            | 0x27 | –                  | a b → a<=b             | number less-than-or-equal |
//! | `Ge`            | 0x28 | –                  | a b → a>=b             | number greater-than-or-equal |
//! | `LooseEq`       | 0x29 | –                  | a b → a==b             | JavaScript `==` |
//! | `ThrowError`    | 0x2A | u16 [`ThrowKind`]  | → (never returns)      | fail with a static JavaScript error |
//! | `Function`      | 0x2B | u16 function index | c1 … cn → f            | new function object (closure) for child code `i` |
//! | `Call`          | 0x2C | u16 argument count n | f a1 … an → r        | call `f` in a new task-owned frame |
//! | `CurrentFunction` | 0x2D | –                | → f                    | the function object of the running frame |
//! | `GetCapture`    | 0x2E | u16 capture index  | → v                    | captured value `i` of the running closure |
//! | `NewCell`       | 0x2F | –                  | → cell                 | new uninitialized binding cell |
//! | `CellInit`      | 0x30 | –                  | cell v →               | initialize (or re-initialize) a cell |
//! | `CellGet`       | 0x31 | –                  | cell → v               | read a cell; ReferenceError if uninitialized |
//! | `CellSet`       | 0x32 | –                  | cell v →               | assign a cell; ReferenceError if uninitialized |
//! | `AtomNew`       | 0x33 | –                  | v → atom               | new shared atom holding `v` |
//! | `AtomLoad`      | 0x34 | –                  | atom → v               | the atom's current value |
//! | `AtomCas`       | 0x35 | –                  | atom e n → w ok        | identity compare-and-set; `w` is the value compared |
//! | `Need`          | 0x36 | –                  | def kind → v           | read a dependency result, or suspend until it is published |
//! | `Throw`         | 0x37 | –                  | v → (never returns)    | raise `v` as a JavaScript exception |
//! | `PushCatch`     | 0x38 | u32 byte offset    | –                      | push a catch handler (target entered with the exception pushed) |
//! | `PushFinally`   | 0x39 | u32 byte offset    | –                      | push a finally handler (target is the finally block) |
//! | `PopCatch`      | 0x3A | –                  | –                      | pop the frame's innermost handler, a catch handler |
//! | `PopFinally`    | 0x3B | –                  | –                      | pop the innermost finally handler, recording a normal completion |
//! | `EndFinally`    | 0x3C | –                  | –                      | resume the pending completion of the finished finally block |
//! | `Reduce`        | 0x3D | –                  | f init vec → acc       | resumable native fold: `acc = f(acc, x)` for each element |
//! | `NeedAll`       | 0x3E | –                  | defs kind → results    | resumable native: a vector of the results of `need` of each definition |
//! | `BitAnd`        | 0x3F | –                  | a b → `a & b`          | ToInt32 of both, bitwise AND |
//! | `BitOr`         | 0x40 | –                  | a b → `a \| b`          | ToInt32 of both, bitwise OR |
//! | `BitXor`        | 0x41 | –                  | a b → `a ^ b`          | ToInt32 of both, bitwise XOR |
//! | `Shl`           | 0x42 | –                  | a b → `a << b`         | ToInt32(a) shifted left by ToUint32(b) & 31 |
//! | `Sar`           | 0x43 | –                  | a b → `a >> b`         | ToInt32(a) shifted right, sign-filling |
//! | `UShr`          | 0x44 | –                  | a b → `a >>> b`        | ToUint32(a) shifted right, zero-filling |
//! | `BitNot`        | 0x45 | –                  | a → `~a`               | bitwise NOT of ToInt32(a) |
//! | `Pow`           | 0x46 | –                  | a b → `a ** b`         | Number::exponentiate |
//! | `CallFinally`   | 0x47 | –                  | –                      | run the innermost finally block, then continue after this instruction |
//! | `DropCompletion`| 0x48 | –                  | –                      | discard the pending completion of the running finally block |
//! | `Plus`          | 0x49 | –                  | a b → a+b              | JavaScript binary `+` on primitives (numbers add, strings concatenate) |
//! | `CompareLt`     | 0x4A | –                  | a b → a<b              | JavaScript `<` on primitives (IsLessThan: strings by code units) |
//! | `CompareGt`     | 0x4B | –                  | a b → a>b              | JavaScript `>` on primitives |
//! | `CompareLe`     | 0x4C | –                  | a b → a<=b             | JavaScript `<=` on primitives |
//! | `CompareGe`     | 0x4D | –                  | a b → a>=b             | JavaScript `>=` on primitives |
//! | `Typeof`        | 0x4E | –                  | v → type               | JavaScript `typeof`: the type name of v as a string |
//! | `GetBuiltinGlobal` | 0x52 | u16 built-in global | → v                  | push the realm's value of a built-in global name (`crate::realm::Global`: 0 is `Object`, 1 `Number`, 2 `String`) |
//! | `CallMethod`    | 0x53 | u16 argument count | this f a1 … an → r     | call `f` with receiver `this` (a method call) |
//! | `NewObjectLiteral` | 0x54 | –               | → obj                  | allocate a fresh ordinary object inheriting from `Object.prototype` (an object literal) |
//! | `ToString`      | 0x55 | –                  | v → s                  | JavaScript ToString on primitives (clj-quickjs-57) |
//! | `GetThis`       | 0x56 | –                  | → this                 | push the running frame's `this` value (the receiver it was called with) |
//! | `Construct`     | 0x5E | u16 argument count | f a1 … an → obj        | `new f(a1, …, an)`: construct with the ordinary function `f` (clj-quickjs-71) |
//! | `NewArguments`  | 0x65 | –                  | → obj                  | consume retained actual arguments into an unmapped ordinary object |
//! | `Method`        | 0x66 | u16 function index | obj c1 … cn → obj f    | allocate a non-constructor closure with immutable home object `obj` |
//! | `DefineGetter`  | 0x67 | u16 name index | obj f → obj | define/merge an enumerable configurable getter |
//! | `DefineSetter`  | 0x68 | u16 name index | obj f → obj | define/merge an enumerable configurable setter |
//! | `GetGlobalObject` | 0x69 | – | → obj | the task-owned global object (script `this`) |
//! | `GetGlobal` | 0x6A | u16 name constant | → v | current global property; missing is ReferenceError |
//! | `GetGlobalOrUndefined` | 0x6B | u16 name constant | → v | global lookup permitting a missing property (`typeof`) |
//! | `PutGlobal` | 0x6C | u16 name constant | v → v | assign global property; sloppy code creates missing names, strict code throws ReferenceError |
//! | `DeclareGlobalVar` | 0x6D | u16 name constant | – | create a non-configurable global var unless already own |
//! | `DeclareGlobalFunction` | 0x6E | u16 name constant | – | validate a global function declaration and prepare its property |
//! | `TemplateObject` | 0x6F | u16 part count n | cooked/raw pairs (2*n) → array | frozen, persistent per-realm/site template object (clj-quickjs-58); cooked must be String/undefined and raw String; code stays primitive-only |
//!
//! `Method` (clj-quickjs-126) takes its capture count from child code,
//! like `Function`. Validation also requires the home-object operand below
//! the captures. It is an allocation poll point. The object is rooted on
//! the operand stack until the new method traces it. `DefineField` then
//! installs the method as an ordinary data property of the literal.
//!
//! `NewArguments` (clj-quickjs-97) is emitted at function entry before
//! defaults. Loading code containing it retains all original actual values
//! in the task-owned frame until the instruction consumes them; other code
//! retains only declared parameters. It allocates an ordinary object with
//! Object.prototype, enumerable writable configurable indices and a
//! non-enumerable writable configurable length, and is a GC poll point.
//!
//! Opcodes `0x12`–`0x20` are the private collection intrinsics and builder
//! operations. Their contract (operand validation, errors, the builder
//! lifecycle) is in [`crate::vm::intrinsics`]. They are VM operations a
//! compiler may lower known collection calls to, not user-visible globals.
//!
//! Opcodes `0x21`–`0x2A` are what the source compiler
//! ([`crate::frontend::compile_script`]) needs beyond the core set.
//! `ToNumber` converts `undefined` to NaN, `null` to `+0` and booleans to 0
//! or 1 and strings by StringToNumber (clj-quickjs-67); objects first use
//! resumable ToPrimitive with hint number (clj-quickjs-70).
//! `ToNumberPair` converts both operands the same way; the compiler emits it
//! before a number-only comparison only when the other operand of `<`, `>`,
//! `<=` or `>=` is statically a number, so two strings never reach it there
//! (they compare with `CompareLt` and its siblings below). `LooseEq` is JavaScript
//! `==` (IsLooselyEqual) for the supported value types: `null ==
//! undefined`, objects compare by identity, two strings by content, a
//! number, boolean or string against another of those compares as numbers
//! (booleans as 0 or 1, strings by StringToNumber), and comparing an object
//! with a primitive other than `null`/`undefined` uses resumable
//! ToPrimitive with hint default. `StrictEq` compares strings
//! by content too. `ThrowError` always
//! raises the error named by its [`ThrowKind`] operand
//! ([`VmErrorKind::ReferenceError`](crate::VmErrorKind::ReferenceError) or
//! [`VmErrorKind::TypeError`](crate::VmErrorKind::TypeError)), a catchable
//! exception since clj-quickjs-11 (`crate::vm::exception`). For stack checking
//! it counts as pushing one value, so it can stand in for an expression.
//!
//! Opcodes `0x2B`–`0x2D` are calls (clj-quickjs-4). An executable carries a
//! table of child function code ([`ExecutableSpec::functions`](crate::ExecutableSpec::functions),
//! shared `Arc` handles, never copied); `Function i` allocates a new function
//! object for child `i` each time it runs, so two evaluations are distinct
//! objects, as with JavaScript function expressions. `Call n` pops the callee
//! and `n` arguments, pushes a task-owned frame for the callee's code and
//! continues there; the callee's `Return` pops its frame and pushes the result
//! onto the caller's operands. Arguments bind to the callee's parameter
//! locals: missing ones are `undefined`, extra ones are ignored, its other
//! locals start `undefined`. There is no `this` or `arguments` object. A
//! callee that is not a function object fails the task with
//! [`VmErrorKind::TypeError`](crate::VmErrorKind::TypeError) (`… is not a
//! function`); a call beyond the task's [`VmLimits`](crate::VmLimits) fails it
//! with [`VmErrorKind::StackOverflow`](crate::VmErrorKind::StackOverflow).
//! `CurrentFunction` pushes the function object of the running frame (for the
//! task's entry frame, one is allocated on first use and reused): the
//! self-reference of a named function expression, like QuickJS's
//! `special_object` with `OP_SPECIAL_OBJECT_THIS_FUNC`. Frames are described in
//! `crate::vm::frames`.
//!
//! Opcodes `0x2E`–`0x32` are lexical closures (clj-quickjs-20); their contract
//! is in `crate::vm::function`. A child function declares how many captured
//! values a closure over it carries
//! ([`ExecutableSpec::captures`](crate::ExecutableSpec::captures), `n`
//! above); `Function i` pops exactly that many (pushed first capture first)
//! and stores them, in order, in the new function object, which is the
//! closure's environment. `GetCapture i` pushes captured value `i` of the
//! running frame's function object; operands are checked against the
//! declaration at load time. A captured value is either an immutable value
//! (captured by value) or a *binding cell*, the shared, task-owned storage of
//! a mutable binding: `NewCell` creates one uninitialized, `CellInit` stores
//! its initial value, `CellGet` and `CellSet` read and assign it and fail
//! with `ReferenceError` (as `ThrowError Uninitialized` does) while it is
//! uninitialized, which is the run-time temporal-dead-zone check of a
//! captured `let`/`const`. Cell operations on another task's cell fail with
//! [`VmErrorKind::ForeignBinding`](crate::VmErrorKind::ForeignBinding); on a
//! value that is not a cell, with `UnsupportedOperand`.
//!
//! Opcodes `0x33`–`0x35` are the private atom intrinsics (clj-quickjs-10),
//! documented in [`crate::vm::intrinsics`] and [`crate::atom`].
//!
//! Opcode `0x36`, `Need`, is the dependency read behind `__rt.need`
//! (clj-quickjs-23): the resumable wait of `docs/design.md` §7, documented in
//! [`crate::task`] ("Dependency waits in the VM"). It pops a definition id
//! and a result kind (non-negative integer numbers; anything else fails with
//! `UnsupportedOperand`) and pushes the shared result value, which may
//! require ending the run suspended and continuing at this instruction when
//! the task is run again.
//!
//! Opcodes `0x37`–`0x3C` are exceptions (clj-quickjs-11), documented in
//! `crate::vm::exception`. `Throw` raises its operand; the four handler
//! instructions maintain the task-owned handler stack, and `Return` itself
//! runs the finally blocks of its frame (the innermost first) before leaving
//! it. A handler target is a branch target; for stack checking a catch
//! target starts one deeper than its `PushCatch` (the exception value) and a
//! finally target at the depth of its `PushFinally`. Whether a run-time
//! handler instruction matches the handler stack (a `PopCatch` whose
//! innermost entry is a catch handler of its frame, and so on) is checked
//! when it runs: a mismatch, possible only in hand-assembled code, fails the
//! task with `Internal`. `ThrowError` failures (and the other JavaScript
//! errors listed there) are catchable.
//!
//! Opcodes `0x3D`–`0x3E` start *resumable native operations*
//! (clj-quickjs-12), documented in `crate::vm::native`: `Reduce` (behind
//! `__rt.reduce`) folds a persistent vector through a JavaScript callback,
//! `NeedAll` (behind `__rt.needAll`) needs one dependency per element of a
//! vector of definition ids. Unlike the leaf intrinsics above, which run to
//! completion inside their instruction, each pops its operands into a
//! task-owned native continuation that the interpreter loop drives one step
//! at a time; the continuation may call JavaScript or demand a dependency,
//! and the whole task may yield or suspend between its steps. The result is
//! pushed when the continuation returns.
//!
//! Opcodes `0x3F`–`0x45` are JavaScript's bitwise and shift operators
//! (clj-quickjs-62). Like the arithmetic opcodes they take Number operands
//! only (the compiler applies ToNumber first). Each applies ToInt32 to its
//! operands (`UShr` applies ToUint32 to its left one): NaN and ±Infinity
//! become 0, other values are truncated toward zero and wrapped modulo
//! 2^32. A shift count is `ToUint32(b) & 31`. Results are 32-bit integers
//! as doubles (unsigned for `UShr`), never `-0`. The names follow QuickJS's
//! `and`/`or`/`xor`/`shl`/`sar`/`shr`/`not` opcodes
//! (`research/quickjs/quickjs-opcode.h`); `UShr` is QuickJS's `shr`.
//!
//! Opcode `0x46` `Pow` is JavaScript's `**` (clj-quickjs-63), Number-only
//! like the arithmetic opcodes. It computes Number::exponentiate: IEEE 754
//! `pow`, except that a non-finite exponent with base `1` or `-1` gives NaN
//! (`pow` gives 1 there, and also for `1 ** NaN`). It follows QuickJS's
//! `pow` opcode and `js_pow` (`research/quickjs/quickjs.c`).
//! Opcodes `0x47`–`0x48` carry `break`/`continue` through finally blocks
//! (clj-quickjs-79), documented in `crate::vm::exception`. `CallFinally`
//! turns the running frame's innermost handler, a finally handler, into a
//! pending *jump* completion whose resume point is the next instruction,
//! and continues at the finally block; that block's `EndFinally` continues
//! at the resume point. It is QuickJS's `gosub` with the return address
//! kept in the task-owned handler stack. `DropCompletion` pops the running
//! frame's innermost entry, a pending completion, without resuming it (a
//! jump out of a finally block overrides it). Neither has an operand: the
//! resume point is the fall-through edge, which the stack check already
//! follows. At run time `CallFinally` requires the operand stack to be at
//! its handler's height and `EndFinally` of a jump completion requires it
//! again, so the resume point is entered at the validated depth; a mismatch
//! (hand-assembled code only) fails the task with `Internal`, like the
//! other handler-stack mismatches above.
//!
//! Opcode `0x49` `Plus` is JavaScript's binary `+` on primitives
//! (clj-quickjs-65, clj-quickjs-66; QuickJS `js_add_slow`): when either
//! operand is a string, the ToString of both concatenate into a new string
//! (immutable storage outside the heap, `crate::value::JsString`; a result
//! longer than 2^30 - 1 code units fails with `OutOfMemory`); otherwise two
//! numbers add and other primitive pairs add their ToNumber values. Any
//! object operand first uses resumable ToPrimitive with hint default,
//! and a string is never converted with ToNumber. It allocates
//! no heap object, so it is not a poll point. The compiler emits it for
//! every `+` whose operands are not both statically numbers; `Add` stays
//! number-only.
//!
//! Opcodes `0x4A`–`0x4D` are JavaScript's relational operators `<`, `>`,
//! `<=`, `>=` on primitives (clj-quickjs-68; ECMAScript IsLessThan, QuickJS
//! `js_relational_slow` and `js_string_compare`, `research/quickjs/quickjs.c`,
//! revision 7d1c13c5d8f8). Two string operands compare lexicographically by
//! UTF-16 code units (a proper prefix is smaller; the `Ord` of
//! [`JsString`](crate::value::JsString)), so `"10" < "9"` and
//! `"\u{10000}" < "\u{FFFF}"` hold. Any other pair of primitives converts
//! numerically (strings by StringToNumber) and compares exactly, preserving
//! BigInts (clj-quickjs-197); BigInt/String pairs explicitly require the
//! deferred StringToBigInt conversion. For the supported numeric pairs,
//! a NaN makes all four false. `CompareGt` is IsLessThan with the operands
//! swapped; `CompareLe` and `CompareGe` negate `CompareGt` and `CompareLt`
//! except that NaN still gives false. Objects first use resumable
//! ToPrimitive with hint number, left before right (before any swap).
//! Conversion steps are poll points; primitive comparison allocates no heap
//! object (mixed BigInt/Number comparison uses bounded temporary limb storage).
//! The compiler emits them unless both operands are statically Numbers;
//! unknown operands may be BigInts. Two static Numbers use the number-only
//! `Lt`, `Gt`, `Le`, `Ge`.
//!
//! Opcode `0x4E`, `Typeof`, is JavaScript `typeof` (clj-quickjs-69; ECMAScript
//! TypeofOperator, QuickJS `js_operator_typeof`, `research/quickjs/quickjs.c`,
//! revision 7d1c13c5d8f8). It replaces the value with its type name, a
//! string: `undefined` is `"undefined"`, `null` is `"object"`, a boolean
//! `"boolean"`, a number `"number"`, a string `"string"`. Of the heap
//! objects, a function object and a built-in function (clj-quickjs-117; the
//! callable kinds, `Call` accepts nothing else) are `"function"`; every
//! other kind is `"object"`: plain
//! objects (including Error instances), arrays, native payload objects, persistent
//! vectors, maps and syntax occurrences, transient builders, atoms and
//! binding cells (which JavaScript code never holds as values). It never
//! fails on a live value and calls no JavaScript. The string lives outside
//! the heap, so it allocates nothing there and is not a poll point. The
//! compiler lowers `typeof name` for a name with no binding to `Undefined`
//! and `Typeof` instead of the ReferenceError a plain read throws.
//!
//! Opcodes `0x4F`–`0x51` (`GetField`, `PutField`, `DefineField`,
//! clj-quickjs-114) and `0x57`–`0x5B` (clj-quickjs-118, with `0x5C`–`0x5D` of
//! clj-quickjs-76) are property access; their contract is in `crate::vm::property`:
//!
//! | Opcode          | Byte | Operand            | Stack (before → after) | Meaning |
//! |-----------------|------|--------------------|------------------------|---------|
//! | `NewArray`      | 0x57 | –                  | → arr                  | allocate a fresh empty array owned by the running task |
//! | `AppendElement` | 0x58 | –                  | arr v → arr            | define index `length` of an array literal as v |
//! | `AppendHole`    | 0x59 | –                  | arr → arr              | skip index `length` of an array literal (a hole) |
//! | `GetElem`       | 0x5A | –                  | o k → v                | `o[k]`: ToPropertyKey(k), then `[[Get]]` |
//! | `PutElem`       | 0x5B | –                  | o k v → v              | `o[k] = v`: ToPropertyKey(k), then `[[Set]]` (strict) |
//! | `Dup2`          | 0x5C | –                  | a b → a b a b          | duplicate the top two (a computed reference read before it is written, clj-quickjs-76) |
//! | `ToPropertyKey` | 0x5D | –                  | o k → o key            | `undefined`/`null` o is a `TypeError` (a read), else ToPropertyKey(k) once: an array index as a number, any other key as a string (clj-quickjs-76) |
//!
//! `NewArray` allocates (and builds the realm on first use, for
//! `Array.prototype`), so it is a poll point, as `NewObjectLiteral` is; the
//! others allocate no heap object.
//!
//! Opcode `0x5F`, `SetProto` (clj-quickjs-121; QuickJS `OP_set_proto`):
//! `obj v → obj`. If v is an object or `null` it becomes the prototype of
//! the object literal obj, with the checks of `[[SetPrototypeOf]]` (another
//! task's object as v is `ForeignObject`; `crate::vm::prototype`); any other
//! v is ignored. The compiler emits it for a literal's `__proto__: value`.
//! It allocates nothing and runs no JavaScript, so it is not a poll point.
//!
//! Opcode `0x60`, `Instanceof` (clj-quickjs-75; QuickJS `OP_instanceof`,
//! `JS_IsInstanceOf`, `JS_OrdinaryIsInstanceOf`): `v f → bool`, JavaScript
//! `v instanceof f` (InstanceofOperator, OrdinaryHasInstance). `f` must be
//! an object and callable, else a catchable `TypeError`; a primitive `v`
//! is `false`; otherwise `f.prototype` must be an object (`TypeError`) and
//! the result is whether it is on `v`'s prototype chain, by identity
//! (`crate::vm::prototype::instance_of`). `Symbol.hasInstance` and bound
//! functions do not exist yet. Reading `f.prototype` may create a
//! function's lazy prototype object and build the realm, so it is a poll
//! point; it runs no JavaScript.
//! Opcode `0x61`, `Delete` (clj-quickjs-124; QuickJS `OP_delete`):
//! `o k → b`, the `delete o[k]` / `delete o.k` operator. An `undefined` or
//! `null` o is a TypeError; then ToPropertyKey(k) and `[[Delete]]` of the
//! own property (`crate::vm::property::delete`): b is true when o has no
//! own property k afterwards (an array element becomes a hole), and a
//! non-configurable own property stays and is a TypeError in strict code,
//! false in sloppy code (the running code's `Code::strict`). A primitive o
//! is treated as its wrapper object (a string's indices below its length
//! and its `length` are non-configurable). Deleting initial function
//! metadata allocates its property record and a constructor's prototype
//! (clj-quickjs-101), so it is a poll point. It runs no JavaScript.
//!
//! Opcode `0x62`, `In` (clj-quickjs-74; QuickJS `OP_in`, `js_operator_in`):
//! `k o → bool`, JavaScript `k in o`. `o` must be an object, else a
//! catchable `TypeError`; then ToPropertyKey(k) (an object `k` uses
//! resumable ToPrimitive with hint string, as for `GetElem`) and
//! HasProperty: an own or inherited property, whatever its value
//! (`crate::vm::property::has_property`); a function's lazy `prototype`
//! is reported without being created. Continuing from a function to
//! `Object.prototype` may build the realm, so it is a poll point; it runs
//! no JavaScript.
//!
//! Opcodes `0x63`–`0x64` run `for`-`in` loops (clj-quickjs-85; QuickJS
//! `OP_for_in_start`, `OP_for_in_next`, `build_for_in_iterator`,
//! `js_for_in_next`), over a task-owned enumerator (`crate::vm::keys`,
//! `ForInIterator`):
//!
//! | Opcode          | Byte | Operand            | Stack (before → after) | Meaning |
//! |-----------------|------|--------------------|------------------------|---------|
//! | `ForInStart`    | 0x63 | –                  | v → it                 | a new enumerator of v's own enumerable string keys (none for `null`, `undefined`, numbers, booleans; a string's indices) |
//! | `ForInNext`     | 0x64 | –                  | it → key more          | the next key still an own enumerable property, and `true`; or `undefined` and `false` when done |
//!
//! `ForInStart` snapshots the keys in OrdinaryOwnPropertyKeys order and
//! allocates the enumerator, so it is a poll point; another task's object
//! is `ForeignObject`. `ForInNext` re-checks each key before returning it,
//! skipping keys deleted (or made non-enumerable) since the snapshot; it
//! allocates no heap object (the key string lives outside the heap) and is
//! not a poll point. The compiler keeps the enumerator in a scratch local,
//! never on the operand stack across the loop body, so `break`,
//! `continue`, `return` and exceptions leave the loop with nothing to pop.
//!
//! Opcode `0x52`, `GetBuiltinGlobal` (clj-quickjs-117), pushes the value of
//! a built-in global name of the runtime's realm (`crate::realm`), building
//! the realm on first use, so it may allocate and is a poll point. Its
//! operand is checked at load time against the built-in global table. The
//! source compiler now uses `GetGlobal` instead (clj-quickjs-104), so it
//! observes writable task-local global bindings. This instruction remains
//! available for hand-assembled bytecode that needs the original intrinsic.
//!
//! Opcode `0x53`, `CallMethod` (clj-quickjs-117), is `Call` with a receiver
//! below the callee: the compiler emits it for a call whose callee is a
//! property reference `o.m(…)`, evaluating `o` once. A built-in function
//! callee (`crate::vm::builtin`) receives `this`; a JavaScript function
//! callee is entered with it as its frame's `this` (clj-quickjs-116). Plain
//! `Call` passes `undefined` as the receiver, to a built-in or a frame
//! alike (strict-mode semantics). Both are poll points.
//!
//! Opcode `0x54`, `NewObjectLiteral` (clj-quickjs-117), is `NewObject` with
//! the realm's `Object.prototype` as the new object's prototype (QuickJS
//! `object`): the compiler emits it for object literals. `NewObject` keeps
//! a null prototype (OrdinaryObjectCreate(null), as `Object.create(null)`
//! will need), and allocating with it never builds the realm.
//!
//! Opcode `0x55`, `ToString`, is JavaScript ToString (clj-quickjs-57;
//! ECMAScript ToString, QuickJS `JS_ToStringInternal`): a string stays the
//! same string, `undefined`, `null` and booleans give their names, and a
//! number its Number::toString in radix 10. Objects first use resumable
//! ToPrimitive with hint string (clj-quickjs-70).
//! The compiler emits it after each substitution of a template literal,
//! before the `Plus` that appends it, so the conversion is ToString rather
//! than the default-hint ToPrimitive of `+`. The string lives outside the
//! heap, so it is not a poll point.
//! Opcode `0x56`, `GetThis` (clj-quickjs-116), pushes the running frame's
//! `this` (`crate::vm::frames`): the receiver of the `CallMethod` that
//! entered it, otherwise `undefined` (a plain `Call`, a native's callback,
//! a task's entry frame). The compiler emits it for `this` inside a
//! non-arrow function. In strict code the value is pushed as is; in sloppy
//! code an `undefined`, `null` or primitive `this` would be replaced by the
//! global object or wrapped (ToObject), neither of which exists yet
//! (clj-quickjs-104, clj-quickjs-72), so it fails the task with
//! `UnsupportedOperand` instead of pushing a wrong value. It never
//! allocates and is not a poll point.
//!
//! Opcode `0x5E`, `Construct` (clj-quickjs-71), is `new f(args)` (QuickJS
//! `OP_call_constructor`, `JS_CallConstructorInternal`, revision
//! 7d1c13c5d8f8). It checks that `f` is a constructor: a function object
//! whose code is an ordinary function (`ExecutableSpec::constructor`, a
//! `function` declaration or expression). Anything else (a primitive, a
//! built-in, an arrow function, a collection) fails with a catchable
//! `TypeError` "… is not a constructor". It then reads `f.prototype`
//! (materializing the lazy prototype object, `crate::vm::function`),
//! allocates an ordinary object owned by the running task whose prototype
//! is that value if it is an object and `Object.prototype` otherwise, and
//! enters `f` in a new frame exactly like `Call`, with the new object as
//! its `this` and the frame marked as a construct frame
//! (`crate::vm::frames`). When that frame returns, a returned object is the
//! result and any other value is replaced by the frame's `this` (the
//! return-override rule). It may allocate, so it is a poll point.
//!
//! Byte `0x00` is deliberately unassigned so zeroed memory never decodes.
//! Branch targets are absolute byte offsets that must land on an instruction
//! boundary. The opcode list is an append-only registry: new opcodes take the
//! next byte and are handled in every exhaustive match.
//!
//! `Add`, `Sub`, `Mul`, `Lt`, `Div`, `Rem`, `Neg`, `Gt`, `Le`, `Ge`, `Pow` and the
//! bitwise and shift opcodes support Number operands only. Other operand types fail the task with
//! [`VmErrorKind::UnsupportedOperand`](crate::VmErrorKind::UnsupportedOperand)
//! rather than applying JavaScript's coercions; a compiler applies them
//! explicitly with `ToNumber`/`ToNumberPair`.
//!
//! The local/branch/return families follow the shape of QuickJS's
//! `get_loc`/`put_loc`, `goto`/`if_false` and `return` opcodes
//! (`research/quickjs/quickjs-opcode.h`, revision 7d1c13c5d8f8); no QuickJS
//! code or encoding is reused. `ThrowError` plays the role of QuickJS's
//! `throw_error` (emitted for assignment to a constant and similar statically
//! known failures). The collection intrinsics follow QuickJS's
//! practice of dedicated opcodes for known operations (for example
//! `array_from`, `get_array_el`, `append` in the same file); their semantics
//! are this project's own collection contract.

/// One opcode of the instruction format. See the module docs for the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Opcode {
    Const = 0x01,
    Undefined = 0x02,
    GetLocal = 0x03,
    SetLocal = 0x04,
    Pop = 0x05,
    Dup = 0x06,
    Add = 0x07,
    Sub = 0x08,
    Mul = 0x09,
    Lt = 0x0A,
    StrictEq = 0x0B,
    Not = 0x0C,
    Jump = 0x0D,
    JumpIfFalse = 0x0E,
    Return = 0x0F,
    NewObject = 0x10,
    HostEffect = 0x11,
    NewVector = 0x12,
    NewMap = 0x13,
    Count = 0x14,
    Nth = 0x15,
    Get = 0x16,
    GetOr = 0x17,
    Assoc = 0x18,
    Dissoc = 0x19,
    Conj = 0x1A,
    PopLast = 0x1B,
    NewVectorBuilder = 0x1C,
    NewMapBuilder = 0x1D,
    BuilderConj = 0x1E,
    BuilderAssoc = 0x1F,
    Freeze = 0x20,
    Div = 0x21,
    Rem = 0x22,
    Neg = 0x23,
    ToNumber = 0x24,
    ToNumberPair = 0x25,
    Gt = 0x26,
    Le = 0x27,
    Ge = 0x28,
    LooseEq = 0x29,
    ThrowError = 0x2A,
    Function = 0x2B,
    Call = 0x2C,
    CurrentFunction = 0x2D,
    GetCapture = 0x2E,
    NewCell = 0x2F,
    CellInit = 0x30,
    CellGet = 0x31,
    CellSet = 0x32,
    AtomNew = 0x33,
    AtomLoad = 0x34,
    AtomCas = 0x35,
    Need = 0x36,
    Throw = 0x37,
    PushCatch = 0x38,
    PushFinally = 0x39,
    PopCatch = 0x3A,
    PopFinally = 0x3B,
    EndFinally = 0x3C,
    Reduce = 0x3D,
    NeedAll = 0x3E,
    BitAnd = 0x3F,
    BitOr = 0x40,
    BitXor = 0x41,
    Shl = 0x42,
    Sar = 0x43,
    UShr = 0x44,
    BitNot = 0x45,
    Pow = 0x46,
    CallFinally = 0x47,
    DropCompletion = 0x48,
    Plus = 0x49,
    CompareLt = 0x4A,
    CompareGt = 0x4B,
    CompareLe = 0x4C,
    CompareGe = 0x4D,
    Typeof = 0x4E,
    GetField = 0x4F,
    PutField = 0x50,
    DefineField = 0x51,
    GetBuiltinGlobal = 0x52,
    CallMethod = 0x53,
    NewObjectLiteral = 0x54,
    ToString = 0x55,
    GetThis = 0x56,
    NewArray = 0x57,
    AppendElement = 0x58,
    AppendHole = 0x59,
    GetElem = 0x5A,
    PutElem = 0x5B,
    Dup2 = 0x5C,
    ToPropertyKey = 0x5D,
    Construct = 0x5E,
    SetProto = 0x5F,
    Instanceof = 0x60,
    Delete = 0x61,
    In = 0x62,
    ForInStart = 0x63,
    ForInNext = 0x64,
    NewArguments = 0x65,
    Method = 0x66,
    DefineGetter = 0x67,
    DefineSetter = 0x68,
    GetGlobalObject = 0x69,
    GetGlobal = 0x6A,
    GetGlobalOrUndefined = 0x6B,
    PutGlobal = 0x6C,
    DeclareGlobalVar = 0x6D,
    DeclareGlobalFunction = 0x6E,
    /// Pop `2*n` interleaved cooked/raw primitives, push the per-realm/site
    /// persistent frozen template array (clj-quickjs-58).
    TemplateObject = 0x6F,
}

/// Operand encoding of an opcode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OperandKind {
    None,
    /// u16 index into the constant pool.
    Const,
    /// u16 index into the frame's locals.
    Local,
    /// u32 absolute byte offset of the branch target.
    Target,
    /// u16 host-defined operation selector, passed through to the host.
    HostOp,
    /// u16 element (vector) or entry (map) count of a constructor.
    Count,
    /// u16 [`ThrowKind`] selector.
    ThrowKind,
    /// u16 index into the executable's child function table.
    Function,
    /// u16 number of call arguments.
    ArgCount,
    /// u16 index into the running closure's captured values.
    Capture,
    /// u16 index into the constant pool naming a string constant: a
    /// property name.
    Name,
    /// u16 built-in global name (`crate::realm::Global`).
    BuiltinGlobal,
}

impl OperandKind {
    pub(crate) fn size(self) -> usize {
        match self {
            OperandKind::None => 0,
            OperandKind::Const
            | OperandKind::Local
            | OperandKind::HostOp
            | OperandKind::Count
            | OperandKind::ThrowKind
            | OperandKind::Function
            | OperandKind::ArgCount
            | OperandKind::Capture
            | OperandKind::Name
            | OperandKind::BuiltinGlobal => 2,
            OperandKind::Target => 4,
        }
    }
}

impl Opcode {
    /// Decode an opcode byte; `None` for unassigned bytes.
    pub fn from_byte(byte: u8) -> Option<Opcode> {
        use Opcode::*;
        Some(match byte {
            0x01 => Const,
            0x02 => Undefined,
            0x03 => GetLocal,
            0x04 => SetLocal,
            0x05 => Pop,
            0x06 => Dup,
            0x07 => Add,
            0x08 => Sub,
            0x09 => Mul,
            0x0A => Lt,
            0x0B => StrictEq,
            0x0C => Not,
            0x0D => Jump,
            0x0E => JumpIfFalse,
            0x0F => Return,
            0x10 => NewObject,
            0x11 => HostEffect,
            0x12 => NewVector,
            0x13 => NewMap,
            0x14 => Count,
            0x15 => Nth,
            0x16 => Get,
            0x17 => GetOr,
            0x18 => Assoc,
            0x19 => Dissoc,
            0x1A => Conj,
            0x1B => PopLast,
            0x1C => NewVectorBuilder,
            0x1D => NewMapBuilder,
            0x1E => BuilderConj,
            0x1F => BuilderAssoc,
            0x20 => Freeze,
            0x21 => Div,
            0x22 => Rem,
            0x23 => Neg,
            0x24 => ToNumber,
            0x25 => ToNumberPair,
            0x26 => Gt,
            0x27 => Le,
            0x28 => Ge,
            0x29 => LooseEq,
            0x2A => ThrowError,
            0x2B => Function,
            0x2C => Call,
            0x2D => CurrentFunction,
            0x2E => GetCapture,
            0x2F => NewCell,
            0x30 => CellInit,
            0x31 => CellGet,
            0x32 => CellSet,
            0x33 => AtomNew,
            0x34 => AtomLoad,
            0x35 => AtomCas,
            0x36 => Need,
            0x37 => Throw,
            0x38 => PushCatch,
            0x39 => PushFinally,
            0x3A => PopCatch,
            0x3B => PopFinally,
            0x3C => EndFinally,
            0x3D => Reduce,
            0x3E => NeedAll,
            0x3F => BitAnd,
            0x40 => BitOr,
            0x41 => BitXor,
            0x42 => Shl,
            0x43 => Sar,
            0x44 => UShr,
            0x45 => BitNot,
            0x46 => Pow,
            0x47 => CallFinally,
            0x48 => DropCompletion,
            0x49 => Plus,
            0x4A => CompareLt,
            0x4B => CompareGt,
            0x4C => CompareLe,
            0x4D => CompareGe,
            0x4E => Typeof,
            0x4F => GetField,
            0x50 => PutField,
            0x51 => DefineField,
            0x52 => GetBuiltinGlobal,
            0x53 => CallMethod,
            0x54 => NewObjectLiteral,
            0x55 => ToString,
            0x56 => GetThis,
            0x57 => NewArray,
            0x58 => AppendElement,
            0x59 => AppendHole,
            0x5A => GetElem,
            0x5B => PutElem,
            0x5C => Dup2,
            0x5D => ToPropertyKey,
            0x5E => Construct,
            0x5F => SetProto,
            0x60 => Instanceof,
            0x61 => Delete,
            0x62 => In,
            0x63 => ForInStart,
            0x64 => ForInNext,
            0x65 => NewArguments,
            0x66 => Method,
            0x67 => DefineGetter,
            0x68 => DefineSetter,
            0x69 => GetGlobalObject,
            0x6A => GetGlobal,
            0x6B => GetGlobalOrUndefined,
            0x6C => PutGlobal,
            0x6D => DeclareGlobalVar,
            0x6E => DeclareGlobalFunction,
            0x6F => TemplateObject,
            _ => return None,
        })
    }

    /// Mnemonic used in diagnostics.
    pub fn name(self) -> &'static str {
        use Opcode::*;
        match self {
            Const => "const",
            Undefined => "undefined",
            GetLocal => "get_local",
            SetLocal => "set_local",
            Pop => "pop",
            Dup => "dup",
            Add => "add",
            Sub => "sub",
            Mul => "mul",
            Lt => "lt",
            StrictEq => "strict_eq",
            Not => "not",
            Jump => "jump",
            JumpIfFalse => "jump_if_false",
            Return => "return",
            NewObject => "new_object",
            HostEffect => "host_effect",
            NewVector => "new_vector",
            NewMap => "new_map",
            Count => "count",
            Nth => "nth",
            Get => "get",
            GetOr => "get_or",
            Assoc => "assoc",
            Dissoc => "dissoc",
            Conj => "conj",
            PopLast => "pop_last",
            NewVectorBuilder => "new_vector_builder",
            NewMapBuilder => "new_map_builder",
            BuilderConj => "builder_conj",
            BuilderAssoc => "builder_assoc",
            Freeze => "freeze",
            Div => "div",
            Rem => "rem",
            Neg => "neg",
            ToNumber => "to_number",
            ToNumberPair => "to_number_pair",
            Gt => "gt",
            Le => "le",
            Ge => "ge",
            LooseEq => "loose_eq",
            ThrowError => "throw_error",
            Function => "function",
            Call => "call",
            CurrentFunction => "current_function",
            GetCapture => "get_capture",
            NewCell => "new_cell",
            CellInit => "cell_init",
            CellGet => "cell_get",
            CellSet => "cell_set",
            AtomNew => "atom_new",
            AtomLoad => "atom_load",
            AtomCas => "atom_cas",
            Need => "need",
            Throw => "throw",
            PushCatch => "push_catch",
            PushFinally => "push_finally",
            PopCatch => "pop_catch",
            PopFinally => "pop_finally",
            EndFinally => "end_finally",
            Reduce => "reduce",
            NeedAll => "need_all",
            BitAnd => "bit_and",
            BitOr => "bit_or",
            BitXor => "bit_xor",
            Shl => "shl",
            Sar => "sar",
            UShr => "ushr",
            BitNot => "bit_not",
            Pow => "pow",
            CallFinally => "call_finally",
            DropCompletion => "drop_completion",
            Plus => "plus",
            CompareLt => "compare_lt",
            CompareGt => "compare_gt",
            CompareLe => "compare_le",
            CompareGe => "compare_ge",
            Typeof => "typeof",
            GetField => "get_field",
            PutField => "put_field",
            DefineField => "define_field",
            GetBuiltinGlobal => "get_builtin_global",
            CallMethod => "call_method",
            NewObjectLiteral => "new_object_literal",
            ToString => "to_string",
            GetThis => "get_this",
            NewArray => "new_array",
            AppendElement => "append_element",
            AppendHole => "append_hole",
            GetElem => "get_elem",
            PutElem => "put_elem",
            Dup2 => "dup2",
            ToPropertyKey => "to_property_key",
            Construct => "construct",
            SetProto => "set_proto",
            Instanceof => "instanceof",
            Delete => "delete",
            In => "in",
            ForInStart => "for_in_start",
            ForInNext => "for_in_next",
            NewArguments => "new_arguments",
            Method => "method",
            DefineGetter => "define_getter",
            DefineSetter => "define_setter",
            GetGlobalObject => "get_global_object",
            GetGlobal => "get_global",
            GetGlobalOrUndefined => "get_global_or_undefined",
            PutGlobal => "put_global",
            DeclareGlobalVar => "declare_global_var",
            DeclareGlobalFunction => "declare_global_function",
            TemplateObject => "template_object",
        }
    }

    pub(crate) fn operand(self) -> OperandKind {
        use Opcode::*;
        match self {
            Const => OperandKind::Const,
            GetLocal | SetLocal => OperandKind::Local,
            Jump | JumpIfFalse | PushCatch | PushFinally => OperandKind::Target,
            HostEffect => OperandKind::HostOp,
            NewVector | NewMap | TemplateObject => OperandKind::Count,
            ThrowError => OperandKind::ThrowKind,
            Function | Method => OperandKind::Function,
            Call | CallMethod | Construct => OperandKind::ArgCount,
            GetBuiltinGlobal => OperandKind::BuiltinGlobal,
            GetCapture => OperandKind::Capture,
            GetGlobal
            | GetGlobalOrUndefined
            | PutGlobal
            | DeclareGlobalVar
            | DeclareGlobalFunction
            | GetField
            | PutField
            | DefineField
            | DefineGetter
            | DefineSetter => OperandKind::Name,
            Undefined | Pop | Dup | Add | Sub | Mul | Lt | StrictEq | Not | Return | NewObject
            | Count | Nth | Get | GetOr | Assoc | Dissoc | Conj | PopLast | NewVectorBuilder
            | NewMapBuilder | BuilderConj | BuilderAssoc | Freeze | Div | Rem | Neg | ToNumber
            | ToNumberPair | Gt | Le | Ge | LooseEq | CurrentFunction | NewCell | CellInit
            | CellGet | CellSet | AtomNew | AtomLoad | AtomCas | Need | Throw | PopCatch
            | PopFinally | EndFinally | Reduce | NeedAll | BitAnd | BitOr | BitXor | Shl | Sar
            | UShr | BitNot | Pow | CallFinally | DropCompletion | Plus | CompareLt | CompareGt
            | CompareLe | CompareGe | Typeof | NewObjectLiteral | ToString | GetThis | NewArray
            | AppendElement | AppendHole | GetElem | PutElem | Dup2 | ToPropertyKey | SetProto
            | Instanceof | Delete | In | ForInStart | ForInNext | NewArguments => OperandKind::None,
            GetGlobalObject => OperandKind::None,
        }
    }
}

/// The statically known JavaScript error a `ThrowError` instruction raises
/// (its u16 operand). The compiler emits these where JavaScript throws at run
/// time for a reason known at compile time, so the error still happens only
/// if and when that code runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum ThrowKind {
    /// ReferenceError: a reference to a name with no binding (strict mode).
    Unresolvable = 0,
    /// ReferenceError: a `let`/`const` binding used or assigned before its
    /// declaration ran (temporal dead zone).
    Uninitialized = 1,
    /// TypeError: assignment to a `const` binding.
    ConstAssignment = 2,
}

impl ThrowKind {
    /// Decode an operand; `None` for unassigned values.
    pub fn from_u16(code: u16) -> Option<ThrowKind> {
        Some(match code {
            0 => ThrowKind::Unresolvable,
            1 => ThrowKind::Uninitialized,
            2 => ThrowKind::ConstAssignment,
            _ => return None,
        })
    }
}

/// A decoded, validated instruction. Branch targets are instruction indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Instr {
    Const(u16),
    Undefined,
    GetLocal(u16),
    SetLocal(u16),
    Pop,
    Dup,
    Add,
    Sub,
    Mul,
    Lt,
    StrictEq,
    Not,
    Jump(u32),
    JumpIfFalse(u32),
    Return,
    NewObject,
    HostEffect(u16),
    /// Element count.
    NewVector(u16),
    /// Entry count (pops twice as many values).
    NewMap(u16),
    Count,
    Nth,
    Get,
    GetOr,
    Assoc,
    Dissoc,
    Conj,
    PopLast,
    NewVectorBuilder,
    NewMapBuilder,
    BuilderConj,
    BuilderAssoc,
    Freeze,
    Div,
    Rem,
    Neg,
    ToNumber,
    ToNumberPair,
    Gt,
    Le,
    Ge,
    LooseEq,
    ThrowError(ThrowKind),
    /// Index into the child function table, and the number of captured
    /// values the child declares (copied from it at load time, so the stack
    /// effect is known without the table).
    Function(u16, u16),
    /// Argument count.
    Call(u16),
    CurrentFunction,
    /// Index into the running closure's captured values.
    GetCapture(u16),
    NewCell,
    CellInit,
    CellGet,
    CellSet,
    AtomNew,
    AtomLoad,
    AtomCas,
    Need,
    Throw,
    /// Handler target (an instruction index).
    PushCatch(u32),
    /// Handler target (an instruction index).
    PushFinally(u32),
    PopCatch,
    PopFinally,
    EndFinally,
    Reduce,
    NeedAll,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Sar,
    UShr,
    BitNot,
    Pow,
    CallFinally,
    DropCompletion,
    Plus,
    CompareLt,
    CompareGt,
    CompareLe,
    CompareGe,
    Typeof,
    /// Constant index of the property name (a string).
    GetField(u16),
    /// Constant index of the property name (a string).
    PutField(u16),
    /// Constant index of the property name (a string).
    DefineField(u16),
    /// Built-in global operand (checked at load time).
    GetBuiltinGlobal(u16),
    /// Argument count (the receiver and callee are below the arguments).
    CallMethod(u16),
    NewObjectLiteral,
    ToString,
    GetThis,
    NewArray,
    AppendElement,
    AppendHole,
    GetElem,
    PutElem,
    Dup2,
    ToPropertyKey,
    /// Argument count (the constructor is below the arguments).
    Construct(u16),
    SetProto,
    Instanceof,
    Delete,
    In,
    ForInStart,
    ForInNext,
    NewArguments,
    /// Allocate a non-constructor closure with the object below its captures
    /// as its immutable home object; keep that object on the stack.
    Method(u16, u16),
    DefineGetter(u16),
    DefineSetter(u16),
    GetGlobalObject,
    GetGlobal(u16),
    GetGlobalOrUndefined(u16),
    PutGlobal(u16),
    DeclareGlobalVar(u16),
    DeclareGlobalFunction(u16),
    TemplateObject(u16),
}

impl Instr {
    pub(crate) fn opcode(self) -> Opcode {
        match self {
            Instr::Const(_) => Opcode::Const,
            Instr::Undefined => Opcode::Undefined,
            Instr::GetLocal(_) => Opcode::GetLocal,
            Instr::SetLocal(_) => Opcode::SetLocal,
            Instr::Pop => Opcode::Pop,
            Instr::Dup => Opcode::Dup,
            Instr::Add => Opcode::Add,
            Instr::Sub => Opcode::Sub,
            Instr::Mul => Opcode::Mul,
            Instr::Lt => Opcode::Lt,
            Instr::StrictEq => Opcode::StrictEq,
            Instr::Not => Opcode::Not,
            Instr::Jump(_) => Opcode::Jump,
            Instr::JumpIfFalse(_) => Opcode::JumpIfFalse,
            Instr::Return => Opcode::Return,
            Instr::NewObject => Opcode::NewObject,
            Instr::HostEffect(_) => Opcode::HostEffect,
            Instr::NewVector(_) => Opcode::NewVector,
            Instr::NewMap(_) => Opcode::NewMap,
            Instr::Count => Opcode::Count,
            Instr::Nth => Opcode::Nth,
            Instr::Get => Opcode::Get,
            Instr::GetOr => Opcode::GetOr,
            Instr::Assoc => Opcode::Assoc,
            Instr::Dissoc => Opcode::Dissoc,
            Instr::Conj => Opcode::Conj,
            Instr::PopLast => Opcode::PopLast,
            Instr::NewVectorBuilder => Opcode::NewVectorBuilder,
            Instr::NewMapBuilder => Opcode::NewMapBuilder,
            Instr::BuilderConj => Opcode::BuilderConj,
            Instr::BuilderAssoc => Opcode::BuilderAssoc,
            Instr::Freeze => Opcode::Freeze,
            Instr::Div => Opcode::Div,
            Instr::Rem => Opcode::Rem,
            Instr::Neg => Opcode::Neg,
            Instr::ToNumber => Opcode::ToNumber,
            Instr::ToNumberPair => Opcode::ToNumberPair,
            Instr::Gt => Opcode::Gt,
            Instr::Le => Opcode::Le,
            Instr::Ge => Opcode::Ge,
            Instr::LooseEq => Opcode::LooseEq,
            Instr::ThrowError(_) => Opcode::ThrowError,
            Instr::Function(..) => Opcode::Function,
            Instr::Call(_) => Opcode::Call,
            Instr::CurrentFunction => Opcode::CurrentFunction,
            Instr::GetCapture(_) => Opcode::GetCapture,
            Instr::NewCell => Opcode::NewCell,
            Instr::CellInit => Opcode::CellInit,
            Instr::CellGet => Opcode::CellGet,
            Instr::CellSet => Opcode::CellSet,
            Instr::AtomNew => Opcode::AtomNew,
            Instr::AtomLoad => Opcode::AtomLoad,
            Instr::AtomCas => Opcode::AtomCas,
            Instr::Need => Opcode::Need,
            Instr::Throw => Opcode::Throw,
            Instr::PushCatch(_) => Opcode::PushCatch,
            Instr::PushFinally(_) => Opcode::PushFinally,
            Instr::PopCatch => Opcode::PopCatch,
            Instr::PopFinally => Opcode::PopFinally,
            Instr::EndFinally => Opcode::EndFinally,
            Instr::Reduce => Opcode::Reduce,
            Instr::NeedAll => Opcode::NeedAll,
            Instr::BitAnd => Opcode::BitAnd,
            Instr::BitOr => Opcode::BitOr,
            Instr::BitXor => Opcode::BitXor,
            Instr::Shl => Opcode::Shl,
            Instr::Sar => Opcode::Sar,
            Instr::UShr => Opcode::UShr,
            Instr::BitNot => Opcode::BitNot,
            Instr::Pow => Opcode::Pow,
            Instr::CallFinally => Opcode::CallFinally,
            Instr::DropCompletion => Opcode::DropCompletion,
            Instr::Plus => Opcode::Plus,
            Instr::CompareLt => Opcode::CompareLt,
            Instr::CompareGt => Opcode::CompareGt,
            Instr::CompareLe => Opcode::CompareLe,
            Instr::CompareGe => Opcode::CompareGe,
            Instr::Typeof => Opcode::Typeof,
            Instr::GetField(_) => Opcode::GetField,
            Instr::PutField(_) => Opcode::PutField,
            Instr::DefineField(_) => Opcode::DefineField,
            Instr::GetBuiltinGlobal(_) => Opcode::GetBuiltinGlobal,
            Instr::CallMethod(_) => Opcode::CallMethod,
            Instr::NewObjectLiteral => Opcode::NewObjectLiteral,
            Instr::ToString => Opcode::ToString,
            Instr::GetThis => Opcode::GetThis,
            Instr::NewArray => Opcode::NewArray,
            Instr::AppendElement => Opcode::AppendElement,
            Instr::AppendHole => Opcode::AppendHole,
            Instr::GetElem => Opcode::GetElem,
            Instr::PutElem => Opcode::PutElem,
            Instr::Dup2 => Opcode::Dup2,
            Instr::ToPropertyKey => Opcode::ToPropertyKey,
            Instr::Construct(_) => Opcode::Construct,
            Instr::SetProto => Opcode::SetProto,
            Instr::Instanceof => Opcode::Instanceof,
            Instr::Delete => Opcode::Delete,
            Instr::In => Opcode::In,
            Instr::ForInStart => Opcode::ForInStart,
            Instr::ForInNext => Opcode::ForInNext,
            Instr::NewArguments => Opcode::NewArguments,
            Instr::Method(..) => Opcode::Method,
            Instr::DefineGetter(_) => Opcode::DefineGetter,
            Instr::DefineSetter(_) => Opcode::DefineSetter,
            Instr::GetGlobalObject => Opcode::GetGlobalObject,
            Instr::GetGlobal(_) => Opcode::GetGlobal,
            Instr::GetGlobalOrUndefined(_) => Opcode::GetGlobalOrUndefined,
            Instr::PutGlobal(_) => Opcode::PutGlobal,
            Instr::DeclareGlobalVar(_) => Opcode::DeclareGlobalVar,
            Instr::DeclareGlobalFunction(_) => Opcode::DeclareGlobalFunction,
            Instr::TemplateObject(_) => Opcode::TemplateObject,
        }
    }

    /// Operand-stack effect as (values popped, values pushed). Constructors
    /// pop as many values as their count operand says.
    pub(crate) fn stack_effect(self) -> (usize, usize) {
        use Instr::*;
        match self {
            Const(_) | Undefined | GetLocal(_) | NewObject | NewVectorBuilder | NewMapBuilder
            | ThrowError(_) | CurrentFunction | GetCapture(_) | NewCell | GetBuiltinGlobal(_)
            | NewObjectLiteral | GetThis | NewArray | NewArguments => (0, 1),
            GetGlobalObject | GetGlobal(_) | GetGlobalOrUndefined(_) => (0, 1),
            SetLocal(_) | Pop | JumpIfFalse(_) | Return | Throw => (1, 0),
            PutGlobal(_) => (1, 1),
            DeclareGlobalVar(_) | DeclareGlobalFunction(_) => (0, 0),
            CellInit | CellSet => (2, 0),
            Dup => (1, 2),
            Add | Sub | Mul | Lt | StrictEq | Nth | Get | Dissoc | Conj | BuilderConj | Div
            | Rem | Gt | Le | Ge | LooseEq | Need | NeedAll | BitAnd | BitOr | BitXor | Shl
            | Sar | UShr | Pow | Plus | CompareLt | CompareGt | CompareLe | CompareGe
            | Instanceof | In => (2, 1),
            ForInStart => (1, 1),
            ForInNext => (1, 2),
            ToNumberPair => (2, 2),
            AtomNew | AtomLoad => (1, 1),
            AtomCas => (3, 2),
            GetOr | Assoc | BuilderAssoc | Reduce => (3, 1),
            Not | HostEffect(_) | Count | PopLast | Freeze | Neg | ToNumber | CellGet | BitNot
            | Typeof | GetField(_) | ToString | AppendHole => (1, 1),
            PutField(_) | DefineField(_) | DefineGetter(_) | DefineSetter(_) | AppendElement
            | GetElem | SetProto | Delete => (2, 1),
            PutElem => (3, 1),
            Dup2 => (2, 4),
            ToPropertyKey => (2, 2),
            Jump(_) | PushCatch(_) | PushFinally(_) | PopCatch | PopFinally | EndFinally
            | CallFinally | DropCompletion => (0, 0),
            NewVector(n) => (usize::from(n), 1),
            NewMap(n) | TemplateObject(n) => (2 * usize::from(n), 1),
            Call(n) => (usize::from(n) + 1, 1),
            Construct(n) => (usize::from(n) + 1, 1),
            CallMethod(n) => (usize::from(n) + 2, 1),
            Function(_, captures) => (usize::from(captures), 1),
            Method(_, captures) => (usize::from(captures) + 1, 2),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_assigned_byte_round_trips_and_zero_is_invalid() {
        assert_eq!(Opcode::from_byte(0), None);
        let mut assigned = 0;
        for byte in 0..=u8::MAX {
            if let Some(op) = Opcode::from_byte(byte) {
                assert_eq!(op as u8, byte);
                assigned += 1;
            }
        }
        assert_eq!(assigned, 111);
    }
}
