# JavaScript and native contract

This is the supported subset at the verified baseline `72201c8b` plus this
documentation pass. Accepted features have ordinary JavaScript semantics within
the boundaries below; unsupported syntax and APIs fail explicitly. This is not
full ECMAScript conformance. [design.md](design.md) supplies requirements and
[architecture.md](architecture.md) describes execution, ownership and embedding.

`frontend::compile_script` and `compile_function` default to strict mode;
`*_with(..., ScriptOptions::SLOPPY)` selects ordinary script strictness and
honors directive prologues. `parse_module` is always strict. The CLI uses
`compile_script`, so it also defaults to strict mode. Input is UTF-8; string
values are immutable UTF-16 code units, including lone surrogates. Identifier
properties, case conversion, normalization and RegExp data use the verified
Unicode 17.0.0 tables. The former Unicode 16 identifier limitation is superseded.

The checked examples below are parsed by
`frontend::tests::grammar_doc_examples`. Fences labelled `js accept` are strict,
`sloppy-accept` selects sloppy mode, and `module-accept` uses the module goal.
`reject` fences specify exact error kind and line:column; `compile-reject`
fences parse but are rejected by lowering. A parse-only example is not evidence
of execution; the feature map points to executable source tests. The checked
`tests/fixtures/light-cljs/` corpus is real compiler output; other original
fixtures are synthetic. Selected QuickJS material is
identified in [attribution.md](attribution.md).

## Supported syntax and execution

| Feature family | Supported behavior | Concrete source coverage |
| --- | --- | --- |
| Lexical forms | Unicode/escaped identifiers, decimal/binary/octal/hex numbers and numeric separators, BigInt, quoted strings and escapes, templates/tagged templates, RegExp lexical goal, ASI | `tests/quickjs_language.rs`, `src/frontend/tests.rs`, `tests/js_source.rs` |
| Expressions | Arithmetic/bitwise/shift/exponentiation, comparisons/equality, coercions, comma/conditional/logical/nullish, assignments including logical and compound, updates, `typeof`, `void`, `delete`, `in`, `instanceof`, optional chains | `tests/quickjs_language.rs`; `tests/fixtures/js/logical_assignment.js`; `tests/js_source.rs` |
| Statements and bindings | Blocks, lexical/var hoisting and TDZ, if/switch, while/do/for, labels and abrupt completion, for-in/prototype enumeration, for-of and async iteration, patterns/defaults/rest in supported binding heads | `tests/js_source.rs`, `tests/quickjs_functions.rs`, `tests/async_iteration.rs`, `tests/exception_waits.rs` |
| Functions and scope | Declarations/expressions/arrows, nested calls/recursion/closures, parameters/defaults/rest, lexical this/super/new.target, unmapped arguments, call/apply/bind, strict/sloppy globals, sloppy with, direct/indirect eval and Function compilation; sloppy receiver limits below | `tests/js_functions.rs`, `tests/quickjs_functions.rs`, `tests/closures.rs`, `tests/quickjs_objects.rs` |
| Objects | Named/computed/string/symbol keys, literals/methods/accessors/spread, descriptors, prototypes, extensibility, freeze/seal, own/inherited enumeration, reflection, resumable coercion including Symbol.toPrimitive and Symbol.toStringTag | `tests/quickjs_objects.rs`, `tests/object_statics.rs`, `tests/object_to_string.rs`, `tests/reflect.rs`, `tests/need.rs` |
| Classes | Base/derived construction, super calls/properties, public fields, private fields/methods/accessors and private-in, static blocks, supported built-in subclasses | `tests/fixtures/js/private_classes.js`, `tests/class_static_blocks.rs`, `tests/builtin_subclasses.rs`, `tests/js_source.rs` |
| Exceptions and disposal | throw/try/catch/finally, completion across waits, Error/NativeError/AggregateError/SuppressedError, captured stacks, using/await using, DisposableStack/AsyncDisposableStack | `tests/exceptions.rs`, `tests/exception_waits.rs`, `tests/error_stacks.rs`, `tests/native_unwind.rs` |
| Generators and jobs | next/return/throw, yield/yield*, async functions/await, async generator queued requests/delegation, async-from-sync adapters, task-owned jobs | `tests/fixtures/js/generator_return_throw.js`, `tests/async_generators.rs`, `tests/async_iteration.rs`, `tests/quickjs_async.rs` |
| Static modules | Imports/live cells/re-exports/namespaces/cycles, default expressions/declarations, JSON default values | `tests/js_modules.rs`, `tests/fixtures/modules/default_exports.js` |
| Native collections/intrinsics | Persistent vectors/maps, builders, folds, dependency need/needAll; atoms through embedding/VM bytecode; native Clojure form values (keywords, symbols, lists, vectors, maps with structural keys and metadata) | `tests/collection_intrinsics.rs`, `tests/collections.rs`, `tests/native_fold.rs`, `tests/need.rs`, `tests/atoms.rs`, `tests/forms.rs` |

### Native JavaScript surface

| Family | Implemented surface | Evidence and boundaries |
| --- | --- | --- |
| Object/Function/Reflect | Object construction, assign/is/entries/values/fromEntries/hasOwn/isPrototypeOf/seal/isSealed, descriptor/key/prototype/integrity methods; Function call/apply/bind/constructor; Reflect internal methods including construct | `tests/object_statics.rs`, `tests/reflect.rs`, `tests/js_functions.rs`, `tests/quickjs_objects.rs` |
| Arrays/iteration | Sparse arrays and length, push/pop/shift/unshift/splice/reverse/fill/copyWithin; searches; callback/reduction methods; concat/join/slice/flat/flatMap; Array.from/isArray, spread and iterators, sort | `tests/fixtures/js/array_{mutators,search,callbacks,copy}.js`, `tests/js_source.rs`, `tests/need.rs`; callbacks/coercions can wait without replay |
| Map/Set and weak variants | Iterable constructors with IteratorClose, lookup/mutation, ordered live iterators and forEach; WeakMap/WeakSet ephemeron keys, WeakRef and FinalizationRegistry | `tests/map_iterables.rs`, `tests/sets.rs`, `tests/quickjs_collections.rs`, `tests/gc.rs`; mutable instances are task-owned |
| Real light-cljs output | Current runtime factory initialization; emitted arithmetic, closure capture, persistent map/vector loops, sequence reduction, user macro expansion and protocol dispatch loop | `tests/light_cljs.rs`; sources, expected numbers, shared emitted runtime and compiler revision in `tests/fixtures/light-cljs/` |
| Primitive wrappers | Number, String, Boolean, Symbol, BigInt conversion/boxing, including ordinary property lookup and with environments | `tests/boolean_bigint_wrappers.rs`, `tests/js_source.rs`; Symbol/BigInt reject construction |
| Numbers/BigInt/Math | Predicates/constants, parseInt/parseFloat, number radix/fixed/exponential/precision formatting; in-tree BigInt arithmetic, bitwise/shift/width wrapping and formatting; Math standard numeric surface, sumPrecise, random | `tests/quickjs_bigint.rs`, `tests/math.rs`, `tests/js_source.rs`; numerical policy below |
| Strings | UTF-16 indexing, char/code-point access/construction, search/extraction/combination/padding/repeat/raw, casing/locale casing, normalization/well-formedness, trim/start/end/includes, split/replace/replaceAll and RegExp symbol protocols | `tests/fixtures/js/string_methods.js`, `tests/fixtures/js/regexp_symbols.js`, `tests/js_source.rs`; first rope flatten may run synchronously |
| RegExp | In-tree parser/IR/backtracking matcher, flags d/g/i/m/s/u/v/y, captures/backreferences, positive/negative lookahead, indices, Unicode folding and binary/category/script/string properties, v sets; exec/test/source/flags and symbol methods; constructor pattern observations precede newTarget prototype selection, then source/flags coercion | `tests/js_source.rs`, `tests/builtin_subclasses.rs`, `tests/fixtures/js/regexp_*.js`, `src/regexp/*` tests; bounded leaf matching; lookbehind creation is rejected |
| Binary data | ArrayBuffer detach/slice, integer/float typed-array construction from length/buffer/array-like/iterables, indexed reads/writes and supported set/subarray/slice | `tests/quickjs_builtins.rs`, `tests/js_source.rs`; no shared buffers, Atomics or DataView |
| JSON | parse with positioned errors; stringify with getters/toJSON/wrappers/arrays/objects/indentation/cycles | `tests/js_source.rs`, `tests/js_modules.rs`; callable reviver and non-nullish replacers explicitly rejected |
| Date | Time clipping, UTC/local fields and setters, host clock/zone, ISO and pinned fallback parsing, non-locale formats | `tests/date_host.rs`, `tests/js_source.rs`; clock/zone and conversion policy below |
| Intl | Locale/canonicalization/likely subtags, NumberFormat (decimal/percent/currency/unit), PluralRules, DateTimeFormat, Collator, ListFormat, RelativeTimeFormat, Segmenter, DisplayNames, DurationFormat | `tests/quickjs_intl.rs`, `tests/sim_host.rs`, `tests/js_source.rs`; service/options/data subset below, no blanket Intl compatibility |
| Promises | Shareable promises (including subclasses), cross-task settlement, owner-task reactions, adoption/chaining and supported static combinators; quiescent rejection reporting | `tests/promises.rs`, `tests/quickjs_async.rs`; await suspends an activation, need waits the task |
| Proxy | Construction and forwarding, resumable ownKeys/getOwnPropertyDescriptor traps with invariant checks | `tests/js_source.rs` proxy cases; other present traps explicitly error, revocation deferred |

## Compiler execution evidence

The real compiler gate is bounded execution evidence. It does not establish
native sharing of compiler forms, on-demand helper scheduling, automatic GC,
performance targets, or compatibility with every ClojureScript program. Macro
expansion happens when the fixtures are generated; the engine executes the
resulting JavaScript. `Map.prototype.entries` is also `@@iterator`; keys,
values and entries use branded live cursors that observe deletion, clear and
later insertion until exhausted. The regressions cover spread, for-of,
destructuring, direct/borrowed next calls, overrides, IteratorClose and GC
between resumed steps. `Object.getOwnPropertyDescriptors` includes symbol and
non-enumerable keys without invoking property getters; seal/isSealed preserve
writable data values and use the supported Proxy descriptor/own-key protocols.
Realm intrinsics remain immutable and present unsupported Proxy traps still
raise an explicit error.

## Modules

The embedder loads the full static graph before task creation through
`module::ModuleLoader` and `ModuleHost`; relative `./x` resolves against the
referrer's directory, and an unresolved request raises ReferenceError
`could not load module`. Records cache per world/version; instances/cells belong
to each task. Imports are not recorded host dependency edges. Host module kind
classifies JSON (the supplied loader's policy uses `.json`); it exposes a default
export. Default exports evaluate once, preserve function/class naming and hoisting,
and remain live through import/re-export. Dynamic import, import.meta, string
import/export names, attributes and module top-level await are absent. The CLI
is script-only. `tests/js_modules.rs` supplies executable examples of the host API.

## Ownership and deliberate deviations

Ordinary objects, arrays, globals, generator/async activations, wrappers and
mutable collections belong to their task. `Object.freeze` does not grant
cross-task ownership. Realm intrinsics are shared and immutable; attempts to
change them fail, although task global bindings can be replaced. Published
functions cannot gain mutable property records. Promises/guards (including
subclasses) and atoms are explicit concurrent references. Subclass Promise own
properties and prototype access remain task-owned under ordinary-object rules;
sharing their settlement state grants no foreign access to that state.
Promise private fields, methods and accessors use traced task-owned storage;
foreign private reads, writes, initialization and private-in throw `TypeError`.
This includes native Promises stamped by a returning super constructor, whose
private storage belongs to the first task that initializes a private element.
See [architecture.md](architecture.md#publication-and-native-values).

`undefined`, `NaN` and `Infinity` are reserved literal bindings and cannot be
shadowed. `await`/`yield` remain reserved as identifiers. Block functions stay
lexical; sloppy Annex B block-function hoisting and admitting later independent
scripts into an existing global environment are absent. Constructor errors use
the source callee label instead of consulting a mutable function name.
Non-standard QuickJS function lineNumber/columnNumber properties are absent;
error diagnostics/stacks retain source locations. Accepted syntax can still
raise a catchable unsupported-API error at execution (for example Proxy traps,
JSON revivers/replacers and unsupported Intl options).

Sloppy calls substitute the task's global object for omitted or nullish
receivers, including lexical arrows and direct eval. Object receivers retain
their identity. Primitive receiver boxing is absent: reading `this` with a
primitive receiver fails with UnsupportedOperand.
Strict functions retain their supplied receiver unchanged. `arguments` is always
an unmapped copy, even in sloppy functions: parameter writes and indexed argument
writes do not alias each other. These are deliberate subset boundaries, not full
sloppy-mode semantics. Implementation: `src/vm/dispatch.rs::checked_this` and
`new_arguments`; evidence: `tests/js_functions.rs::call_apply_and_reflect_apply`
and `sloppy_nullish_receivers_use_the_task_global_object`, plus
`arguments_object_holds_every_argument`.

Lookbehind patterns parse into RegExp IR but are rejected during RegExp creation
(including literal execution) with a catchable SyntaxError. Only positive and
negative lookahead execute. The native matcher reports Unsupported("lookbehind");
`src/regexp/matcher/tests.rs::unsupported_features_fail_explicitly` asserts this
boundary.

## Resource and policy limits

This is the single documentation reference for implementation-selected caps and
policy choices. Caps are part of this slice's contract, not ECMAScript limits.
A budget is cooperative, not a wall-clock deadline; synchronous leaf work and
first rope flattening can delay yield and collection. Allocation failure may
also terminate an operation. No aggregate runtime heap quota is implemented.

| Boundary | Current limit or policy | Implementation pointer |
| --- | --- | --- |
| Frontend depth | Weighted nesting budget and AST height each 256; TooDeep on excess. Constructs have different costs (parentheses/calls/functions charge 3 plus contents; operators/statements charge 1). Host parsing/compilation requires a 2 MiB native stack. | `src/frontend.rs::MAX_NESTING`, `src/frontend/parser.rs::nesting`, `src/frontend/tests.rs` depth tests |
| Compiler tables | 65,535 live bindings, call arguments and captures; 65,536 constants/functions per function; TooLarge on excess | `src/frontend/lower.rs` MAX constants |
| VM frames/values | Defaults: 10,000 frames and 1,048,576 reserved stack values; embedder-configurable `VmLimits`; checked StackOverflow | `src/vm/frames.rs::VmLimits` |
| Automatic collection | Default 16,384 allocations, growing with the last live-object count; runtime-wide `HeapPolicy`. Requests run at safe VM boundaries; scopes and long native/host work can delay them. Explicit collection remains available. | `src/heap/mod.rs::HeapPolicy`, `src/heap/gc.rs` |
| Heap object admission | Optional runtime-wide `hard_limit_objects`, shared across workers; default unlimited. Rejected task allocations raise catchable `HeapLimit` with primitive string value `heap object limit exceeded`. Counts allocated objects until swept, not payload bytes or RSS. | `src/heap/mod.rs::HeapPolicy`, `src/vm/dispatch.rs` |
| Reference executor | Default two workers and 10,000 instructions/steps per run, configurable by embedder; shutdown deadlines are caller-supplied. Unlimited budgets or blocking leaf work can prevent timely shutdown. | `src/exec.rs::ExecutorConfig` |
| Simulated host invocation | Default 10,000 activations per invocation, configurable with set_activation_limit; a host-model bound separate from VM recursion | `src/host/sim.rs` |
| Opt-in virtual timers | Finite truncated millisecond delays; negative becomes zero; delays, due times and sequence IDs must stay within 2^53−1; overflow rejects rather than wraps | `src/host/sim/facilities.rs` |
| Runtime eval/Function | At most 16,384 UTF-16 source units, including assembled Function parameters/body; compile on a joined 2 MiB compiler thread stack; this leaf compilation does not yield | `src/vm/native/eval.rs` |
| Strings | At most 2^30−1 UTF-16 units; ropes cache their first contiguous flatten and retain rope plus flat storage until release | `src/value/string.rs::MAX_STRING_LEN` |
| BigInt | At most 1 Mbit magnitude; canonical inline-small or heap-large representation; RangeError `BigInt is too large`; one instruction budget unit per arithmetic operation | `src/value/bigint.rs::MAX_BIGINT_BITS` |
| JSON.parse | 512 open containers and 4096 decoded member-name units; catchable SyntaxError; callable revivers rejected after parsing | `src/vm/json.rs::MAX_DEPTH`, `MAX_MEMBER_NAME_UNITS` |
| JSON.stringify | 512 open containers, 4096 own properties before filtering (including non-enumerable/symbol/virtual properties, Proxy lists and target snapshots), 4096-unit member names and Number-wrapper conversion strings; catchable RangeError. Arrays stream indices without a matching element-count cap. | `src/vm/json.rs::MAX_OBJECT_PROPERTIES`, native JSON continuations |
| ArrayBuffer/typed arrays | 16 MiB per backing buffer; total memory remains proportional to live buffers; typed-array source accumulation obeys the same bound | `src/object/array_buffer.rs::MAX_BYTE_LENGTH` |
| String replace and RegExp string protocols | 65,536 units per receiver/search/replacement/callback-result or limited numeric string, plus 1,000,000 aggregate work/output units; catchable RangeError | `src/vm/native/string_replace.rs`, `string_regexp.rs` |
| RegExp matching | 1,000,000 steps per complete search, including assertions/backtracking; catchable RangeError. Quantifier counts beyond u64 capacity error at execution. | `src/regexp/matcher.rs::MAX_MATCH_STEPS`, `src/regexp/parse.rs` |
| Date numeric conversion | 4096 UTF-16 units for bounded leaf numeric conversion strings; existing TimeClip range is ±8.64e15 ms | `src/vm/date.rs`, Date native continuations |
| Number formatting counts | toFixed/toExponential/toPrecision count-string conversion at most 4096 units; RangeError before unbounded leaf scan; ordinary digit ranges still apply | `src/vm/builtin.rs::number_digit_count` |
| Dynamic argument lists/spread | At most 65,535 arguments, additionally subject to VM value limits | `src/vm/dispatch.rs`, `src/frontend/lower.rs` |
| Error stacks | Snapshot/output cap 16,384 UTF-16 units; metadata fields at most 256 units; at most 64 JS callers for eval/Function compilation errors | `src/vm/frames.rs`, `src/vm/native/eval.rs` |
| Prototype lookup/Proxy forwarding | Leaf cursors have a 4096-step bound; Proxy forwarding counts internal cursor work, not just target hops | `src/vm/property.rs::lookup`, `src/vm/proxy.rs::forward_cursor` |
| Proxy classification | Separate 512-step bound, including the final non-Proxy inspection; exhaustion throws TypeError. A 600-Proxy chain therefore fails tag classification even when forwarding is within its bound. | `src/vm/proxy.rs::unwrapped` |
| Map/Set insertion positions | Monotonic u64 positions; exhaustion is a catchable RangeError even if deletions made room; no wraparound | `src/object/map.rs` |
| Math.random | Fixed-seed per-task SplitMix64; each task starts the same deterministic sequence, no host entropy or cryptographic guarantee | `src/vm/frames.rs`, `src/vm/native.rs` |
| Math transcendental boundary | cbrt/asinh/acosh/atanh use host C libm results; platform rounding can vary. Other numerical algorithms retain their tested signed-zero/NaN behavior. | `src/vm/builtin.rs`, `tests/math.rs` |
| Promise diagnostics | At quiescence retain up to 30 unhandled-rejection entries; later entries increment a dropped counter. Suspended awaits are reported separately on task completion. | `src/task.rs`, `src/job.rs` |
| Intl locales | en, en-US, en-GB, de, fr, es, ja, zh, zh-Hant, zh-Hans-CN, ar, ru, pt-BR, tr, with zh-TW resolution; default from host, en-US without host, unrecorded leaf effect | `src/intl/mod.rs`, `tests/sim_host.rs` |
| Locale tags | Fewer than 512 ASCII bytes and fewer than 64 hyphen-separated subtags; invalid explicit tags throw RangeError in JavaScript | `src/intl/tag.rs::LanguageTag::parse` |
| String locale casing | Only the pinned QuickJS Turkish/Azeri and Lithuanian context rules are claimed; other languages use root casing. Omitted/undefined/empty locale lists use the host default. Host locale strings of 512 or more UTF-16 units silently use root casing, including those beginning with tr. | `src/vm/native/string_methods.rs::State::language`, `case_step`, `lookahead`; [host-default test](../tests/sim_host.rs) (`string_locale_case_uses_host_default`), [String fixture expectations](../tests/fixtures/js/string_methods.js) (`tests/js_source.rs::string_methods_semantic_fixture`) |
| NumberFormat decimal strings | Mantissa at most 100,000 ASCII bytes and exponent magnitude at most 10,000 after syntax validation; RangeError on excess | `src/number.rs::Decimal::parse` |
| Intl options/data | Only implemented service/options and pinned test outputs are claimed. NumberFormat range methods, unsupported numbering systems/calendars/time zones and unimplemented options reject explicitly. Segmenter is the tested data-driven subset, not full UAX #29. | `src/intl/`, `tests/quickjs_intl.rs` |
| Date clock/zone | Host clock is an unrecorded leaf effect; reruns can differ. One zone per runtime, UTC without host. Generated snapshot lists 598 names but only 59 transition records; names do not imply transition coverage. | `src/host/timezone/`, `tests/date_host.rs` |
| Generated data | Pinned QuickJS snapshot and verified Unicode 17.0.0 inputs; no verified CLDR/tzdb release inferred from that snapshot, no runtime download/generation/C wrapper | `scripts/quickjs-data/`, provenance and licenses in [attribution.md](attribution.md) |

The String locale-casing qualification refers to
`research/quickjs/quickjs.c::js_string_toLowerCase` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, using this runtime's verified
Unicode tables. The linked fixture fixes the expected combining-dot and
case-ignorable behavior; it does not establish complete Unicode locale
SpecialCasing conformance. Source attribution is in
[attribution.md](attribution.md#string-case-and-normalization-clj-quickjs-300).

Service-specific Intl boundaries are part of that same data policy:

| Service | Coverage boundary | Test pointer |
| --- | --- | --- |
| DateTimeFormat | Gregorian calendar; UTC, fixed offsets and the configured runtime zone. Other named zones/calendars reject. Component widths, time fields, styles and zone-name forms follow pinned snapshot patterns. | `tests/sim_host.rs` date_time_format tests |
| Collator | Root folded-NFD scalar ordering plus German/Turkish tailorings, ASCII numeric runs and tested case/diacritic options; not full UCA/DUCET weights. Well-formed unsupported collations resolve to default. | `tests/js_source.rs` intl_collator tests |
| Segmenter | Grapheme and word heuristics from the snapshot, with corrected regional-indicator pairing; sentence granularity and complete UAX #29 absent. | `tests/quickjs_intl.rs`, `tests/js_source.rs` intl_segmenter tests |
| DisplayNames | Localized display names use en/fr with English fallback; unnamed components preserve canonical codes. | `tests/js_source.rs` intl_display_names tests |
| DurationFormat/unit patterns | Duration objects and digital fields; no ISO duration strings or Temporal private slots. Missing CLDR unit patterns use the pinned QuickJS literal unit-identifier fallback. | `tests/js_source.rs` intl_duration_format and intl_number_format tests |

The Date, RegExp, BigInt, Intl, Promise, async and module choices above follow the
run's user decisions. Other caps are implementation-selected resource policies.
There is no independent fixed async-generator request-queue limit; existing
allocation/frame/value limits and cooperative budgets apply.

### Nesting limit

The frontend's 256-unit weighted nesting budget and 256-node AST-height
limit cover parsing, recursive lowering and syntax-tree teardown on a 2 MiB
host thread stack, including unoptimized builds. Shared recursive parser and
lowerer frames must stay small: assignment construction and return/with
statement scratch values live in separate helpers so unrelated syntax does
not pay their stack cost at every level. Exceeding the budget reports a
positioned `TooDeep` syntax error, including for truncated input.

`frontend::tests::nesting_at_the_limit_fits_a_2_mib_stack` checks every accepted
depth for the covered constructs. The focused parameter-default and for-of
lowering tests cover macOS aarch64 debug overflows at depths 62 and 124,
respectively. The 2 MiB host requirement does not apply to execution workers:
the existing 64 KiB worker tests exercise explicit VM continuations, migration,
eval and teardown. Runtime eval/Function compilation uses its own joined
2 MiB compiler thread, including AST cleanup, without retaining a JavaScript
continuation on that thread.

## Explicit gaps and rejection

JSON reviver traversal and stringify replacers, Proxy revocation/other traps,
DataView/shared buffers/Atomics, module dynamic features and top-level await,
unsupported Intl options, browser/Node/shell APIs, full Clojure collection
protocols and general shared mutable JS graphs are outside the contract.
The source rejections below are exercised by `tests/malformed_input.rs` and the
checked examples. Runtime API rejections are asserted in `tests/js_source.rs`,
`tests/js_modules.rs`, `tests/promises.rs` and feature-specific tests.

| Form | Kind |
|------|------|
| `using` declarations at script top level (strict or sloppy), directly in a `case`/`default` clause, as a single-statement body, with binding patterns or without initializers (clj-quickjs-236) | Invalid at `using` |
| `for (using x in o)`, `for (using [x] of a)`, `for (using {x} of a)` (clj-quickjs-238) | Invalid at `using` |
| classic `for (using x = r; ;)` heads | Unsupported at `using` |
| `for await` or `for (await using ... of ...)` outside async functions | Unsupported at `await`; module top-level await is absent |
| a property as a for-of target (`for (a.b of o)`), `debugger` | Unsupported |
| `with` in strict code (clj-quickjs-110) | Invalid at `with` |
| `await using` outside async functions, including module top level; patterns, missing initializers, switch clauses or single-statement bodies | SyntaxError (Invalid or Unsupported according to the rejected token) |
| `import`/`export` in scripts, dynamic `import` | Unsupported |
| private names outside their declaring lexical class scope | Invalid |
| module string-literal import/export names, import attributes (including `type: 'json'` and `type: 'json5'`), `import.meta`, top-level `await` | Unsupported |
| `new.target` outside a non-arrow function, including a top-level arrow or indirect eval (clj-quickjs-99) | Invalid |
| initialized for-in heads in strict code or with `let`/`const` or a binding pattern; initialized for-of heads in either mode (clj-quickjs-90) | Invalid |
| an optional chain as an assignment/update target (`a?.b = 1`, `a?.b++`), a `new` callee (`new a?.b()`), or before a tagged template in the same chain | Invalid |
| in object literals: computed shorthand `{[k]}` | UnexpectedToken |
| misplaced numeric separators (`1__0`, `1_`, `0x_1`, `1_n`, `0_1`) | InvalidToken at the `_` |
| `super.x` outside a method/accessor or field initializer context, bare `super`, `super?.x` | Invalid |
| `delete` of an identifier reference in strict code (`delete x`, ECMAScript 13.5.1.1), reported at the `delete` | Invalid |
| a reserved word as a shorthand property (`{if}`) | ReservedWord |
| a string or numeric key as a shorthand property (`{1}`) | UnexpectedToken |
| a shorthand initializer outside a destructuring pattern (`({a = 1})`) | Invalid |
| a second `__proto__: v` (or `"__proto__": v`) in one object literal (clj-quickjs-121; a shorthand `{__proto__}` does not count) | Invalid |
| reserved word as a binding name or reference; binding, assigning (also with a compound operator: `undefined += 1`), incrementing or decrementing `undefined`/`NaN`/`Infinity`; binding or assigning/updating `eval`/`arguments` in strict code, including a function name or parameter checked again because the function's body starts with a `"use strict"` directive | ReservedWord |
| computed access to the reserved intrinsic binding (`__rt[x]`), assignment to one of its intrinsics (`__rt.count = f`), `++`/`--` on one (`__rt.count++`) or `delete` of one (`delete __rt.count`) | ReservedWord |
| an arrow function as an operand, callee or member object without parentheses (`1 + x => x`, `() => {}()`), a line break before `=>` | UnexpectedToken |
| `return` outside a function, `break` outside a loop or `switch` of the same function, `continue` outside a loop of the same function, a second `default` in a `switch`, `const` without initializer, duplicate parameter (arrow functions included), declaration as an `if`/`while`/`do`/`for` or labelled body, a `break`/`continue` label that no enclosing statement of the same function has, `continue` to a label of a statement that is not a loop, a label inside a statement with the same label, `const` without initializer in a `for` head, an initializer or more than one binding in a for-in head (`for (let k = 1 in o)`, `for (let a, b in o)`, `for (var k = 1 in o)` in strict code), a for-in target that is not a reference (`for (1 in o)`), invalid assignment target (`1 = 2`, `1 += 2`, `f() += 1`), an operand of `++`/`--` that is not an identifier binding or a property `o.p` / `o[k]` (`1++`, `f()++`, `++a++`), a line break between `throw` and its expression, a `let`/`const`/function declaration in a catch body that repeats the catch parameter, a prefix-operator expression as the base of `**` (`-a ** b`) | Invalid |
| nesting over the `MAX_NESTING` budget (256 units), or a syntax tree more than 256 nodes high; see "Nesting limit" below | TooDeep |
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
| `__rt.formList(x…)`, `__rt.formVector(x…)`, `__rt.formMap(k, v, …)` | `new_form_list n`, `new_form_vector n`, `new_form_map n` | new native Clojure form list, vector or map (light-cljs-466); map keys are structural, a later equal key wins |
| `__rt.formKeyword(ns, name)`, `__rt.formSymbol(ns, name)` and the other `__rt.form*` operations | `form_op k` | one form-value operation (`crate::vm::intrinsics`, "Form intrinsics"; contract in `crate::collections::form`) |

Atoms additionally have VM opcodes `atom_new`, `atom_load`, `atom_cas`; they are
exposed to Rust by `Runtime`/`Scope`, not source `__rt` names. Native maps use
SameValueZero keys with objects by identity; form maps (`__rt.formMap`) are a
separate kind whose keys compare by ClojureScript value equality. Structural `value_equals`, identity
and provenance are different operations. Persistent updates share unchanged
subtrees; builders are owned by task ID, can migrate and freeze once. Builder
misuse fails with WrongType/NotOwner/Frozen/OwnerFinished; collection indices
must be integral nonnegative numbers, with no string coercion. No JavaScript
hash/equality callbacks run in these operations. See `src/collections.rs` and
`src/vm/intrinsics.rs` for the exact Rust error and operand contracts.

## Checked grammar examples

These existing examples are retained verbatim through this consolidation;
their fences are machine-checked. Execution evidence is in the feature map.
Selected adapted QuickJS spellings retain their [attribution](attribution.md).

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

```js accept
function gather(a, ...rest) { return a + rest.length; }
gather(1, 2, 3) === 3 && gather.length === 1;
let copied = {a: 1, ...{b: 2}, ...null}; copied.b === 2;
let values = []; for (let [a] of [[1], [2]]) values.push(a);
for (const {length: size} in {abc: 1}) values.push(size);
```

```js sloppy-accept
const o = { x: 1 };
with (o) { var x = 2; (() => x); }
```

```js reject Invalid 1:1
with (o) {}
```

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

```js accept
+" 12\n" === 12 && +"0x1f" === 31 && +"" === 0;
("12345" | 0) === 12345 && ("0x12345" >>> 0) === 0x12345;
"6" * "7" === 42 && "2" ** "10" === 1024;
"" == 0 && "123" == 123 && "1" == true && "abc" != NaN;
```

```js accept
"b" > "a" && "a" < "aa" && "" < "a" && "B" < "a" && "a" <= "a";
"10" < "9" && !("10" < 9) && !("abc" < 1) && !("abc" >= 1);
"\u{10000}" < "\uFFFF" && !("\uFFFF" < "\u{10000}");
```

```js accept
typeof 1 === "number" && typeof null === "object" && typeof typeof 1 === "string";
typeof nope === "undefined" && typeof (nope) === "undefined" && typeof undefined === "undefined";
typeof (() => 1) === "function" && typeof __rt.vector() === "object" && typeof "" === "string";
```

```js reject Invalid 1:10
typeof a ** 2;
```

```js accept
void 0 === undefined && void void 1 === undefined && typeof void 0 === "undefined";
```

```js accept
let a = {x: 1, y: 1}; delete a.x && a.x === undefined && delete a.missing && delete a["y"];
let arr = [1, 2]; delete arr[0] && arr.length === 2 && arr[0] === undefined;
let r = false; try { delete null.a; } catch (e) { r = true; } r;
let o = {}; Object.defineProperty(o, "k", {value: 1}); let r = false; try { delete o.k; } catch (e) { r = o.k === 1; } r;
```

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

```js accept
let p = {x: 1, m() { return this.y; }}; let o = {__proto__: p, y: 2, f() { return super.x + super.m(); }}; o.f() === 3;
```

```js accept
const x = 1;
function f(a, b = a + 1, g = () => x) { var x = 2; return b * 10 + g(); }
f(1) === 21 && f(1, undefined) === 21 && f(1, 5) === 51 && ((a = 1, c) => a + c)(undefined, 2) === 3;
```

```js accept
[1, 2, 3][2] === 3 && [1, 2,].length === 2 && [ , ].length === 1 && [].length === 0;
[1, , 3].length === 3 && [1, , 3][1] === undefined && [] !== [] && typeof [] === "object";
let a = [1]; a[3] = 4; a.length === 4 && a["0"] === 1 && a[2] === undefined;
```

```js accept
let o = {}; o[1] = "one"; o["1"] === "one" && o[1.5] === undefined;
let p = {}; p[-0] = 0; p[true] = 1; p["0"] === 0 && p.true === 1 && p[undefined] === undefined;
let q = {}; let k = "y"; q[k] = 7; q.y === 7 && ({})["toString"] === Object.prototype.toString;
```

```js accept
let a = 1, b = 2;
[a, b] = [b, a];
let [first, , third = 3, ...tail] = [1, 2, undefined, 4, 5];
let [[nested]] = [[1]];
function sum([x, y]) { return x + y; }
(([x, y]) => x + y)([1, 2]);
try { throw [7]; } catch ([error]) { error; }
```

```js reject Invalid 1:2
[a + 1] = x;
```

```js reject UnexpectedToken 1:10
let [...r, b] = x;
```

```js accept
let {a, b: c, d = 4, ...r} = {a: 1, b: 2, e: 5};
let o = {}; ({x: o.y} = {x: 3});
let {['x']: [{y = 7}]} = {x: [{}]};
(({a, b}) => a + b)({a: 1, b: 2});
try { throw {message: 'ok'}; } catch ({message}) { message; }
```

```js reject Invalid 1:6
({a: 1} = x);
```

```js reject UnexpectedToken 1:10
let {...r,} = x;
```

```js accept
[1, 2].toString() === "1,2";
```

```js accept
function F() {} F.prototype.constructor === F && F.prototype === F.prototype;
let f = function () {}; f.x = 1; f.x === 1 && f.y === undefined && (() => 1).prototype === undefined;
```

```js accept
function add(a, b) { return a + b; } add.call(null, 1, 2) === 3 && add.apply(null, [1, 2]) === 3 && Reflect.apply(add, null, [1, 2]) === 3;
function self() { return this; } self.call(7) === 7 && self.apply(8) === 8 && self.call() === undefined && Object.getPrototypeOf(self) === Function.prototype;
let r = false; try { Reflect.apply(function () {}, null, undefined); } catch (e) { r = true; } r;
```

```js accept
function f(a, b) { return this + a + b; } const g = f.bind(1, 2); g(3) === 6 && g.length === 1 && g.name === 'bound f';
function C(x) { this.x = x; } const B = C.bind(null, 7); const o = new B(); o.x === 7 && o instanceof B && o instanceof C;
let r = false; try { Function.prototype.bind.call(1); } catch (e) { r = true; } r;
```

```js accept
(function (a, b) {}).length === 2 && ((a, b = 1, c) => {}).length === 1 && (() => {}).length === 0;
function f() {} const g = () => 1; let h; h = function () {}; f.name === "f" && g.name === "g" && h.name === "h" && (function () {}).name === "";
const f = () => 0; const d = Object.getOwnPropertyDescriptor(f, "name"); !d.writable && !d.enumerable && d.configurable && Object.getOwnPropertyNames(f)[0] === "length";
function f() {} delete f.name && f.name === "" && Object.getOwnPropertyDescriptor(f, "name") === undefined;
```

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

```js accept
function f() {
  try { return 1; } finally { cleanup(); }
}
try { f(); } catch (e) { throw e; } finally {}
```

```js accept
let refOK = false; try { nope; } catch (e) { refOK = e instanceof ReferenceError && e instanceof Error && e.message === 'nope is not defined' && e.toString() === 'ReferenceError: nope is not defined'; } refOK;
let typeOK = false; try { null(); } catch (e) { typeOK = e instanceof TypeError && e.name === 'TypeError'; } typeOK;
let rangeOK = false; try { let a = []; a.length = -1; } catch (e) { rangeOK = e instanceof RangeError && e.message === 'invalid array length'; } rangeOK;
let lengthOK = false; let finalized = false; try { let s = 'x'; for (let i = 0; i < 31; i++) s = s + s; } catch(e) { lengthOK = e instanceof InternalError && e.message === 'string too long'; } finally { finalized = true; } lengthOK && finalized;
```

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

```js accept
let disposed = 0;
{ using a = null, b = { [Symbol.dispose]() { disposed++; } }; }
disposed === 1;
function resource() { using of = undefined; return 42; }
```

```js accept
async function resource() {
  await using x = { async [Symbol.asyncDispose]() { await null; } };
  await using empty = null;
  return 42;
}
resource().then(x => __rt.effect(0, x));
async function expressions(using) { await using; await using
x = 1; }
```

```js reject Unsupported 1:1
await using x = null;
```

```js module-reject Unsupported 1:1
await using x = null;
```

```js reject Invalid 1:32
async function f() { if (true) await using x = null; }
```

```js accept
let disposed = 0, fs = [];
for (using x of [{n: 1, [Symbol.dispose]() { disposed++; }}]) {
  fs.push(() => x.n);
}
disposed === 1 && fs[0]() === 1;
let using;
for (using of [1, 2]) {}
using = [42];
for (using/*comment*/[0]; false;) {}
async function f(using) { for (await using; false;) {} }
```

```js reject Invalid 1:6
for (using x in o) {}
```

```js reject Invalid 1:6
for (using [x] of a) {}
```

```js reject Unsupported 1:6
for (using x = r; ;) {}
```

```js reject Unsupported 1:6
for (await using x of a) {}
```

```js module-accept
using resource = { [Symbol.dispose]() {} };
export { resource };
```

```js reject Invalid 1:1
using resource = null;
```

```js reject Invalid 1:3
{ using resource; }
```

```js reject Invalid 1:3
{ using [resource] = null; }
```

```js reject Invalid 1:22
switch (0) { case 0: using resource = null; }
```

```js reject Invalid 1:11
if (true) using resource = null;
```

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

```js accept
function F(x) { this.x = x; } const b = new F(2); b.x === 2 && (new F).x === undefined && new F(1) !== new F(1);
function* G() {} let caught = false; try { new G(); } catch(e) { caught = e instanceof TypeError && e.message === 'G is not a constructor'; } caught;
let o = {m() {}}; let caught = false; try { new o.m(); } catch(e) { caught = e instanceof TypeError && e.message === 'o.m is not a constructor'; } caught;
function G() { this.y = 1; return {y: 2}; } function H() { this.z = 1; return 5; } new G().y === 2 && new H().z === 1;
function P(v) { this.v = v; } P.prototype.twice = function () { return this.v * 2; }; new P(3).twice() === 6 && new P(3).constructor === P;
let o = new Object(); o.constructor === Object && Object.getPrototypeOf(o) === Object.prototype;
const a = new Object, b = new Object(null); a !== b && Object.getPrototypeOf(a) === Object.prototype;
const o = {x: 1}; new Object(o) === o && new Object(42).valueOf() === 42 && new Object('abc')[1] === 'b';
```

```js accept
function f() { return new.target; }
function F() { return () => new.target; } new F()() === F && F()() === undefined;
function F(a = new.target) { this.target = a; } new F().target === F;
function F() { return (() => eval('new.target'))(); } new F() === F && F() === undefined;
```

```js reject Invalid 1:1
new.target;
```

```js reject Invalid 1:17
const a = () => new.target;
```

```js accept
let o = {}; Object.defineProperty(o, "k", {value: 1}) === o && Object.getOwnPropertyDescriptor(o, "k").writable === false;
let p = {}; Object.defineProperty(p, "k", {value: 1}); let r = false; try { p.k = 2; } catch (e) { r = p.k === 1; } r;
let d = Object.getOwnPropertyDescriptor({x: 1}, "x"); d.writable && d.enumerable && d.configurable && Object.getOwnPropertyDescriptor({}, "x") === undefined;
```

```js accept
let a = {x: 1}; let b = Object.create(a); Object.getPrototypeOf(b) === a && b.x === 1 && Object.getPrototypeOf(a) === Object.prototype;
let c = {}; let e = Object.create(c); let r1 = false; try { Object.setPrototypeOf(c, e); } catch (err) { r1 = true; } r1;
let n = Object.create(null); n.toString === undefined && Object.getPrototypeOf(Object.prototype) === null;
let q = {y: 2}; let s = {__proto__: q}; let t = {}; t.__proto__ = q; s.y === 2 && t.__proto__ === q && ({}).__proto__ === Object.prototype;
```

```js accept
let k = Object.keys({b: 1, a: 2, 1: 3, 0: 4}); k.length === 4 && k[0] === "0" && k[1] === "1" && k[2] === "b" && k[3] === "a";
let o = {a: 1}; Object.defineProperty(o, "h", {value: 2}); Object.keys(o).length === 1 && Object.getOwnPropertyNames(o)[1] === "h";
let n = Object.getOwnPropertyNames([ , 1]); n.length === 2 && n[0] === "1" && n[1] === "length";
```

```js reject Invalid 2:17
// Only one `__proto__: v` may set a literal's prototype.
({__proto__: 1, __proto__: 2});
```

```js accept
let a = {x: 1}; Object.preventExtensions(a) === a && !Object.isExtensible(a) && Object.isExtensible({});
let b = {x: 1}; Object.preventExtensions(b); let r = false; try { b.y = 2; } catch (e) { r = b.y === undefined; } r;
let f = Object.freeze({x: 1}); let s = false; try { f.x = 2; } catch (e) { s = f.x === 1; } s && Object.isFrozen(f) && !Object.isFrozen({});
let q = Object.freeze([1, 2]); let t = false; try { q[0] = 3; } catch (e) { t = q[0] === 1; } t && Object.isFrozen(q);
Object.isExtensible(1) === false && Object.preventExtensions(1) === 1 && Object.isFrozen(__rt.vector());
```

```js accept
function F() {} function G() {} G.prototype = new F(); new G() instanceof F && new G() instanceof G && !(new F() instanceof G);
function F() {} ({}) instanceof Object && !(1 instanceof F) && !(Object.create(null) instanceof Object) && Object.create(F.prototype) instanceof F;
let r = false; try { ({}) instanceof {}; } catch (e) { r = true; } r;
```

```js accept
'x' in {x: undefined} && 'toString' in {} && !('y' in {x: 2}) && 1 in {1: 0};
0 in [1] && !(1 in [1]) && !('toString' in Object.create(null));
let r = false; try { 'x' in 1; } catch (e) { r = true; } r;
```

```js accept
(19686109595169230000).toString() === "19686109595169230000" && (1.5).toString() === "1.5" && (1).valueOf() === 1;
"abc".toString() === "abc" && (1).toString === Number.prototype.toString && (1).constructor === Number;
let r = 0; try { null.toString(); } catch (e) { r = 1; } try { (1).x = 2; } catch (e) { r = r + 10; } r === 11;
```

```js accept
"abc".length === 3 && "abc"[1] === "b" && "abc"["1"] === "b" && "abc"[3] === undefined;
"\u{10ffff}".length === 2 && "\u{10ffff}"[0] === "\u{dbff}" && Object.getOwnPropertyNames("ab")[2] === "length";
```

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

```js accept
/aaa]/; /(?<n>a)\k<n>/u; /[\]]/u; /[\]]/v;
```

```js accept
false && /\p{Script=Greek}/u;
false && /\p{Script_Extensions=Grek}/u;
false && /\p{RGI_Emoji}/v;
false && /a{18446744073709551616}/;
/a{18446744073709551616,00018446744073709551616}/u;
```

```js reject Invalid 1:1
/a{18446744073709551617,18446744073709551616}/u
```

```js reject Invalid 1:1
/\p{Script=NotAScript}/u
```

```js reject Invalid 1:1
/\p{RGI_Emoji}/u
```

```js reject Invalid 1:1
/\P{RGI_Emoji}/v
```

```js reject Invalid 1:1
/[^\p{Basic_Emoji}]/v
```

```js reject Invalid 2:3

  /aaa]/u
```

```js reject Invalid 1:1
/aaa]/v
```

```js reject Invalid 1:1
/(/
```

```js reject Invalid 1:1
/a{2,1}/
```

```js reject Invalid 1:1
/[b-a]/
```

```js reject Invalid 1:1
/\u{110000}/u
```

```js reject Invalid 1:1
/(?<n>a)(?<n>b)/
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

```js accept
a&&=b
```

```js accept
h ??= 1;
```

```js accept
a.b &&= 1;
```

```js accept
// Synthetic fixture, not light-cljs output.
let x = null; x ??= 3; x &&= 4; x ||= 5;
null ?? (false || 7);
```

```js reject Invalid 1:11
null ?? 0 || 1;
```

```js reject Invalid 1:12
false && 0 ?? 1;
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

```js accept
for (a.x in o) ;
for (a[0] in o) ;
for ((a.x) in o) ;
for (obj().k in {a:1, b:1, c:1}) ;
for (using /* target */ [0] in o) ;
```

```js reject Invalid 1:6
for (f() in o) ;
```

```js reject ReservedWord 1:6
for (__rt.count in o) ;
```

```js reject Unsupported 1:6
for (a.b of o) {}
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

```js sloppy-accept
for (var k = 2 in o) ;
for (var k = ("x" in o) in o) ;
```

```js sloppy-reject Invalid 1:16
for (let k = 2 in o) ;
```

```js sloppy-reject Invalid 1:16
for (var k = 2 of o) ;
```

```js sloppy-reject Invalid 1:19
for (var [k] = [] in o) ;
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

```js accept
function* g() { yield 1; yield* xs; }
```

```js accept
async function f() { await 1; }
```

```js accept
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

```js accept
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

```js accept
let a = [1,2,3]; a.length = '2'; a.length === 2 && a[2] === undefined;
let a = []; a[4294967294] = 1; a.length = 0; a.length === 0;
let a = [1,2]; let caught = false; try { a.length = 1.5; } catch(e) { caught = true; } caught && a.length === 2;
```

```js accept
let a = [1, 2]; a.push(3, 4) === 4 && a[2] === 3 && a[3] === 4 && a.length === 4;
[].push() === 0 && Array.prototype.push.length === 1;
let o = {length: 1}; Array.prototype.push.call(o, "x") === 2 && o[1] === "x";
```

```js accept
Number('0x10') === 16 && Number() === 0 && String(null) === 'null' && String(1e21) === '1e+21';
let n = new Number(5); typeof n === 'object' && n !== 5 && n + 1 === 6 && n instanceof Number;
let s = new String('a'); s + "b" === "ab" && s.toString() === 'a' && s instanceof String;
(new Number()).valueOf() === 0 && (new String()).valueOf() === '';
```

```js accept
new Number(1) == 1 && 2 == new Number(2) && new String('abc') == 'abc';
Number({valueOf: function () { return 7; }}) === 7 && String({toString: function () { return 's'; }}) === 's';
```

```js accept
'abc'.charAt(1.9) === 'b' && 'abc'.charAt(-1) === '';
'\u{10ffff}'.codePointAt(0) === 0x10ffff;
String.fromCharCode.apply(null, [65, 66, 67]) === 'ABC';
String.fromCharCode(0xd800).length === 1 && String.fromCharCode(0xd800).charCodeAt(0) === 0xd800;
String.fromCodePoint(0x41, 0x1f600).length === 3 && String.fromCodePoint(0x41, 0x1f600) === 'A\ud83d\ude00';
```

```js accept
'abcabc'.indexOf('cab') === 2 && 'abcabc'.lastIndexOf('abc') === 3;
'aaa'.indexOf('', Infinity) === 3 && 'aaa'.lastIndexOf('a', NaN) === 2;
'a\u{10000}b'.indexOf('b') === 3 && 'x'.indexOf() === -1;
new String('abc').indexOf({toString: () => 'b'}, {valueOf: () => 1}) === 1;
```

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

```js accept
let error = new Error('text', {cause: 0});
error.message === 'text' && error.cause === 0 && error instanceof Error;
Error(undefined).message === '' && Error(12).message === '12';
Error.prototype.toString.call({name: '', message: 'm'}) === 'm';
try { throw Error('getter'); } catch (caught) { caught.message === 'getter'; }
```

```js accept
Error('hello').stack.indexOf('    at <anonymous> (') === 0;
function make() { return Error(); } make().stack.indexOf('    at make (') === 0;
let stack; try { null(); } catch (e) { stack = e.stack; } typeof stack === 'string';
let e = Error(); let d = Object.getOwnPropertyDescriptor(e, 'stack'); d.writable && d.configurable && !d.enumerable;
let has = false; try { missing; } catch (e) { has = 'stack' in e; } has;
```

```js accept
let parsed = JSON.parse('{"x":1,"a":[true,null]}');
parsed.x === 1 && parsed.a[0] && parsed.a[1] === null;
Object.is(JSON.parse(' -0 '), -0);
JSON.parse('"\\ud800"').length === 1;
```

```js accept
let e = new TypeError('text', {cause: 1});
e.message === 'text' && e.cause === 1 && e instanceof TypeError && e instanceof Error;
RangeError('m').toString() === 'RangeError: m';
Object.getPrototypeOf(URIError) === Error && Object.getPrototypeOf(URIError.prototype) === Error.prototype;
InternalError.prototype.toString === Error.prototype.toString;
```

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

```js accept
typeof Symbol.dispose === 'symbol' && Symbol.dispose !== Symbol.asyncDispose;
Symbol.dispose.description === 'Symbol.dispose';
String(Symbol.asyncDispose) === 'Symbol(Symbol.asyncDispose)';
Symbol.for('Symbol.dispose') !== Symbol.dispose;
let resource = {};
let cleanup = function() { return 7; };
resource[Symbol.dispose] = cleanup;
resource[Symbol.dispose] === cleanup && resource[Symbol.dispose]() === 7;
```

```js accept
JSON.stringify({b: 1, a: undefined, 1: 3}) === '{"1":3,"b":1}';
JSON.stringify([undefined, , NaN]) === '[null,null,null]';
JSON.stringify({toJSON(key) { return key; }}) === '""';
JSON.stringify('\ud800') === '"\\ud800"';
JSON.stringify({a: [1]}, null, 2) === '{\n  "a": [\n    1\n  ]\n}';
```

```js accept
let s = Symbol.for(1);
s === Symbol.for('1') && Symbol.keyFor(s) === '1';
Symbol.keyFor(Symbol('1')) === undefined;
```

```js accept
let r = /(b+)c/g; let m = r.exec('abbbbbc'); m[1] === 'bbbbb' && m.index === 1;
new RegExp('a/b').source === 'a\\/b';
r = /a/y; r.lastIndex = 1; r.test('ba') && r.lastIndex === 2;
/^.$/u.test('🐱');
```

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

```js accept
'abcabc'.replace('b', 'X') === 'aXcabc';
'ab'.replaceAll('', '-') === '-a-b-';
'abc'.replace(/(?<x>b)/, '<$<x>>') === 'a<b>c';
'ab'.replaceAll('', (match, offset) => offset) === '0a1b2';
```

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

```js accept
let calls = 0; Promise.resolve({then(resolve) { calls++; resolve(5); }}); calls === 0;
new Promise(resolve => resolve(Promise.resolve(1))).then(value => value + 1);
Promise.resolve({get then() { throw 7; }}).catch(reason => reason);
```

```js accept
/^.$/u.test('🐱') && !/^.$/.test('🐱');
/[\u{10000}-\u{10FFFF}]/u.test('\u{10400}');
'🐱'.replace(/(?:)/gu, '-') === '-🐱-';
```

```js accept
/k/iu.test('\u212A') && /[a-z]/iu.test('\u017F');
'aAbBcC'.replace(/[^A-B]/gui, 'X') === 'aAbBXX';
```

```js accept
/\p{General_Category=Uppercase_Letter}/u.test('A');
/\p{Lowercase}/u.test('\u{1D41A}');
/[\p{Nd}x]/u.test('x') && /\P{Lower}/iu.test('a');
```

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

```js accept
/[\q{abc|ab|a}]/v.exec('abcd')[0] === 'abc';
! /[[a-z]--[aeiou]]/v.test('e');
'aAbBcC#4'.replace(/\P{Lower}/gvi, 'X') === 'aAbBcCXX';
```

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

```js accept
(async () => 2 + await 3)().then(x => __rt.effect(0, x));
```

```js accept
Promise.reject(1); 42;
```

```js accept
let p = Promise.reject(2); Promise.resolve().then(() => p.catch(() => {})); 42;
(async () => { try { await Promise.reject(3); } catch (e) {} })();
```

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

```js accept
let sym = Symbol('d');
let boxed = Object(sym);
sym.toString() === 'Symbol(d)' && String(sym) === 'Symbol(d)';
boxed.valueOf() === sym && boxed.description === 'd';
Object.getPrototypeOf(boxed) === Symbol.prototype;
Symbol().description === undefined && typeof boxed === 'object';
```

```js accept
function f() { let x=1; eval('x += 2'); return x; }
f() === 3;
function receiver() { return (() => eval('this'))(); }
receiver.call(5) === 5;
let readers=[];
for (let i=0; i<3; i++) readers.push(eval('() => i'));
readers[0]() === 0 && readers[1]() === 1 && readers[2]() === 2;
```

```js sloppy-accept
function caller() {
  const read = () => injected;
  eval('var injected = 2; function twice() { return injected * 2; }');
  return read() === 2 && twice() === 4 && delete injected;
}
caller();
eval('var configurable = 1');
delete configurable;
function conflict() {
  let blocked;
  try { eval('var blocked = 1'); } catch (e) { return e instanceof SyntaxError; }
}
conflict();
```

```js accept
function f(x = eval('1')) { return x; }
f() === 1;
```

```js sloppy-accept
function f(a = eval('var c = 1'), probe = () => c) {
  var c = 2;
  return c === 2 && probe() === 1;
}
f();
function conflict(a = eval('var arguments')) {}
let caught = false;
try { conflict(); } catch (e) { caught = e instanceof SyntaxError; }
caught;
```

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

```js accept
class B {} class C extends B {}
new C() instanceof B;
```

```js accept
class C { x; }
let c = new C(); Object.getOwnPropertyNames(c).join(',') === 'x' && c.x === undefined;
```

```js accept
class A { constructor(a, b) { this.s = a + b; } }
class B extends A {}
class C extends B { constructor() { super(...[1, 2]); this.z = 20; } }
let c = new C(); c.s === 3 && c.z === 20;
```

```js accept
class A { constructor() { this.target = new.target; } }
class B extends A { constructor() { let f = () => new.target; super(); this.same = f() === new.target; } }
let b = new B(); b.target === B && b.same;
```

```js reject Invalid 1:32
class C extends Object { m() { super(); } }
```

```js module-accept
import {fib} from './fib_module.js';
import message from './message.json';
throw new Error('loading must not execute this body');
```

```js reject Unsupported 1:1
import './dependency.js';
```

```js module-accept
import {a, b as c} from 'named';
import d from 'default';
import * as ns from 'namespace';
import e, {f} from 'combined';
import g, * as other from 'combined-namespace';
import 'side-effect';
export {a, c as renamed, d, ns};
```

```js module-accept
export function f(x) { return x + 1; }
export let a = 1;
export const b = 2;
export var c;
export {a as alias, b as default};
```

```js module-accept
export default function named() { return 1; }
export {named};
```

```js module-accept
export default function() { return 1; }
```

```js module-accept
export default {answer: 42};
```

```js module-accept
export class C { method(){return 1;} }
```

```js module-accept
export default class Named {}
export {Named};
```

```js module-accept
export default class {}
```

```js module-reject Invalid 2:14
import {a} from 's';
import {b as a} from 't';
```

```js module-reject Invalid 2:7
import a from 's';
const a = 1;
```

```js module-reject Invalid 2:14
const a = 1; export {a};
export {a as a};
```

```js module-reject Invalid 1:9
export {missing};
```

```js module-reject Invalid 1:14
import {x as for} from 's';
```

```js module-reject Invalid 1:3
{ import 's'; }
```

```js module-reject Invalid 1:15
function f(){ export const x = 1; }
```

```js module-accept
export * from 's';
```

```js module-accept
export * as ns from 's';
```

```js module-accept
export {x} from 's';
```

```js module-reject Unsupported 1:9
import {'x' as x} from 's';
```

```js module-reject Unsupported 1:14
export {x as 'name'};
```

```js module-reject Unsupported 1:12
import 's' with {type: 'json'};
```

```js module-reject Unsupported 1:1
await 1;
```

```js module-reject Unsupported 1:1
import('s');
```

```js module-reject Unsupported 1:1
import.meta;
```

```js module-accept
import {g as importedG} from './self.js';
export function g(x) { return x + 1; }
__rt.effect(0, 2 * importedG(1));
```

```js module-accept
import {n, inc} from './counter.js';
inc(); inc();
__rt.effect(0, n);
```

```js module-accept
import msg from './message.json';
import * as ns from './message.json';
__rt.effect(0, ns.default === msg && msg.x === 1 && msg.tab[2] === 3);
```

```js module-reject Unsupported 1:25
import './message.json' with {type: 'json5'};
```

```js module-accept
/* example of JS and JSON modules */

import { fib } from "./fib_module.js";
import msg from "./message.json";

console.log("Hello World");
console.log("fib(10)=", fib(10));
console.log("msg=", msg);
```

```js accept
let e = new SuppressedError(1, 2, 'm');
e.error === 1 && e.suppressed === 2 && e.toString() === 'SuppressedError: m';
e instanceof SuppressedError && e instanceof Error;
Object.getPrototypeOf(SuppressedError) === Error && SuppressedError.length === 3;
Object.getOwnPropertyDescriptor(SuppressedError(1, 2), 'message') === undefined;
SuppressedError(1, 2, undefined, {get cause() { throw 42; }}).cause === undefined;
```

```js accept
class C { f() { return 1; } static F() { return -1; } } class D extends C { h() { return super.f(); } static H() { return super["F"](); } } let o = new D(); o.h() === 1 && D.H() === -1;
let p = {}; let o = {__proto__: p, f() { super.z = 5; }}; o.f(); o.z === 5 && p.z === undefined;
let o = {f() { delete super.x; }}; let ok = false; try { o.f(); } catch(e) { ok = e instanceof ReferenceError; } ok;
```

```js reject Invalid 1:5
x = super.y;
```

```js reject Invalid 1:23
function g() { return super.x; }
```

```js module-accept
import * as counter from './counter.js';
export {counter};
counter.inc();
__rt.effect(0, counter.n);
export * as sameCounter from './counter.js';
```

```js accept
class S { static x = 42; static y = S.x; static z = this.x; }
S.x === 42 && S.y === 42 && S.z === 42;
class P { get = () => '123'; static() { return 42; } }
new P().get() === '123' && new P().static() === 42;
```

```js accept
class B { constructor() { this.x = 7; } f() { return this.x; } }
class C extends B { y = super.f(); z = eval('super.f()'); }
let c = new C(); c.y === 7 && c.z === 7;
```

```js reject Invalid 1:11
class R { constructor = 1; }
```

```js reject Invalid 1:15
class R { x = arguments; }
```

```js accept
class R { static {} }
```

```js accept
class R { #x; }
```

```js accept
class C { static x = 1; static { this.x += 2; var local = 7; this.f = () => local; } static y = this.x; }
C.x === 3 && C.y === 3 && C.f() === 7;
```

```js reject Invalid 1:20
class C { static { return; } }
```

```js reject Invalid 1:20
class C { static { arguments; } }
```

```js accept
const p = {f() { return this; }};
const o = {__proto__: p, g() { return () => eval('super.f()'); }};
o.g()() === o;
class B {} class C extends B { x = 7; constructor() { eval('super()'); } }
new C().x === 7;
```

```js reject Invalid 1:17
const a = () => super.x;
```

```js reject Invalid 1:22
const a = (x = () => super.x) => x;
```

```js accept
let s = new DisposableStack(), log = '';
s.defer(() => { log += 'a'; });
s.adopt(42, v => { log += v; });
s.dispose(); s.disposed && log === '42a';
s[Symbol.dispose] === s.dispose;
```

```js accept
let n = 0;
{ using s = new DisposableStack(); s.defer(() => n++); }
n === 1;
```

```js accept
(async () => {
    let log = '', s = new AsyncDisposableStack();
    s.defer(async () => { log += 'a'; await null; log += 'A'; });
    s.adopt(42, async v => { log += v; });
    let result = await s.disposeAsync();
    return s.disposed && result === undefined && log === '42aA';
})();
```

```js accept
(async () => {
    let n = 0;
    { await using s = new AsyncDisposableStack(); s.defer(() => n++); }
    return n === 1;
})();
```

```js accept
new Function('a', 'b', 'return a + b')(2, 3) === 5;
Function('return 7')() === 7;
Function('eval', 'return eval(1, 2)')((a, b) => a + b) === 3;
Function('a', 'b', '').length === 2 && Function().name === 'anonymous';
let caught = false; try { Function('a){', ''); } catch(e) { caught = e instanceof SyntaxError; } caught;
```

```js reject UnexpectedEnd 1:14
new Function(
```

```js sloppy-accept
const f = function m() { const v = (m = 5); return v === 5 && m === f; }; f();
const g = function n() { (() => { n = 1; })(); return n; }; g() === g;
```

```js accept
const f = function m() { "use strict"; m = 5; };
let caught = false; try { f(); } catch (e) { caught = e instanceof TypeError; } caught;
```

```js accept
const f = function m() {
  "use strict";
  let caught = false;
  try { (() => { m = 1; })(); } catch (e) { caught = e instanceof TypeError; }
  return caught && m === f;
};
f();
```

```js accept
let p = new Proxy({}, {
    ownKeys() { return ['a', 'b']; },
    getOwnPropertyDescriptor(target, key) {
        return {value: key, enumerable: true, configurable: true};
    }
});
let names = ''; for (let key in p) names += key;
names === 'ab';
```

```js accept
class Counter {
  #value = 1;
  #step() { return ++this.#value; }
  get #current() { return this.#value; }
  next() { return this.#step(); }
  static has(o) { return #value in o; }
}
let counter = new Counter();
counter.next() === 2 && Counter.has(counter) && !Counter.has({});
```

```js reject Invalid 1:21
class C { #x; m() { delete this.#x; } }
```

```js accept
async function sum(values) {
  let total = 0;
  for await (const x of values) total += x;
  return total;
}
async function resources(values) {
  for (await using resource of values) { await null; }
  for await (await using resource of values) { break; }
}
```

```js reject Invalid 1:22
async function f() { for await (const x in a) {} }
```

```js reject Unsupported 1:5
for await (const x of a) {}
```

```js accept
/\p{Script=Greek}/u.test('α');
/\p{scx=Grek}/v.test('\u0342');
/[\p{scx=Hira}&&\p{scx=Kana}]/v.test('ー');
/\P{sc=Latin}/u.test('α');
```
