# Essentially complete JavaScript on the Goose VM: architecture decisions

November 5, 2026 (light-cljs-761). This document fixes how bbp's Goose VM
(`src/`) grows from the macro subset it runs today to essentially complete
JavaScript, measured case by case. It records seven decisions, the evidence
for each and the alternatives rejected. The foundation and harness pellets
(light-cljs-782 to -790, group `goose-javascript`) implement them; the area
planners (light-cljs-762 to -775) build on them. Pellets holds the work; this
file holds the reasons.

Verified with Goose `v2026.3-267-g369c7a2c` (runtime 3025e5810379). Every
Goose rule relied on below was checked with a small `goose check` or run
program (the probes, summarised in [the appendix](#appendix-probes)), not
taken from documentation alone.

## Starting point

- The Rust frontend (bbp's Rust implementation, `rust/`) parses and compiles
  JavaScript to the bytecode of `rust/src/vm/instr.rs`: 198 opcodes in an
  append-only registry (0x01 to 0xC6). Its Goose exporter
  (`rust/src/frontend/goose.rs`, `supported`) accepts 93 of them and refuses
  the whole program on any other, even on a dead path. It also refuses Symbol
  and BigInt constants, direct `eval`, and every constructor kind except
  ordinary functions.
- `src/source_vm.goose` (1,119 lines) runs that bytecode. A value is a 40-byte
  struct (`tag`, `number`, `a`, `b`, `session`); a heap node is a 202-byte
  struct addressed by an `i64` index; a property is an 88-byte record in a
  linked list threaded through one global table by `i64` index; array
  elements are string-keyed properties found by walking that list; an
  instruction is a 48-byte struct dispatched by `match` on an integer. Frames,
  handlers and operands are already data on job-owned stacks, so a
  JavaScript call does not recurse in Goose.
- Every value carries its heap's session identity, so restoring a snapshot
  rewrites every value (`snapshot::restore`, `relocated`). On the 200k project
  restores copy 995,474,716 bytes per session
  ([evidence](../evidence/generated-200k-performance.md)).
- The empty `src/{intl,regexp,object,...}` directories left by the move are
  not coverage.

## The Goose rules this design follows

From the user's requirement in light-cljs-761 and the Goose references:
memory follows scopes, links are typed and position independent, data stays
inline and compact, dispatch is enums and `match`, the interpreter loop does
not recurse per JavaScript call, workers share nothing and exchange flat
values, performance is checked with Goose's own tools, and stack settings are
recorded. Four facts about the current compiler decide most of what follows:

1. **Pool-relative links are not available to workers.** A `thread_fn` may
   reach only flat globals; a global array whose elements hold
   `T&<u32 in pool>` links is rejected ("thread programs may access only flat
   globals ... objects holds references", probe P2), and root structs such as
   `slot_pool` cannot be worker globals at all. bbp runs JavaScript on
   workers, so `&<u32 in pool.items>` cannot link its heap.
2. **Self-relative links do not survive an interpreter's value flow.** They
   work for straight-line code over one array of variable-mode cells (probe
   P5: verified `to_bytes`/`from_bytes` across a worker, in-place relinking
   inside a payload). They do not fit a fixed-mode value (P1: "a fixed-mode
   Cell binds its payloads only by value"), and in a probe interpreter whose
   values pass through operand stacks, frames and helper functions the
   checker could not prove the exact root that a relinking store needs: a
   store into an object reached through a value popped from the operand stack
   and a copy of links into a regrown run were both rejected ("this
   reference's root is not known exactly, only that it outlives heap", P7),
   and a helper taking a cell reference as a parameter cannot relink at all
   ("rooted at target"). Variable-element arrays also cannot be truncated.
3. **Index types fit, and tie reclamation to the thread program.**
   `type ObjId = u32 in objects` over a flat global table works inside a
   worker, each worker gets its own copy of the tables at spawn, reads through
   an `ObjId` need no bounds check (P3, P7 `--bce-lines`), and `from_bytes`
   verifies arrays of index types against their table (P8). An array named by
   an index type can never shrink (P4: "an index type names objects, so its
   length never goes down"). So the scope that frees such a heap in one step
   is the thread program that owns it.
4. **Spawning is cheap.** Spawning and joining an empty worker costs about
   17 microseconds (P8, native build).

## Decision 1: bytecode and the exporter

**Decision.** The exporter exports every opcode the Rust compiler emits, with
its existing resource bounds and source provenance; the `supported` filter is
removed. The Goose loader validates control flow for all opcodes from a
stack-effect table generated from the Rust registry into
`src/export_schema.goose` (as `export_descriptor` and `export_form_arity` are
today), then decodes each instruction row into a fixed-mode `Ins` enum with
jump targets resolved to instruction indices. An opcode the VM does not
implement decodes to `Ins.Unsupported { opcode }` and fails when executed
with a fault that names the opcode. Such a fault is a VM outcome, never a
JavaScript exception, so `catch` cannot swallow it and the harness always
sees it as explicit-unsupported; the same holds for any unimplemented part of
a built-in. Constants gain Symbol and BigInt tags and
functions gain the class and generator constructor kinds as their areas need
them. **Direct `eval`** runs in the caller's scope (light-cljs-858). The
exporter marks the call of each direct-eval site (descriptor bit 24, GSP
version 3; image metadata, not an opcode) and, since GSP version 6, exports
the site's scope: the capture sources the compiler recorded for it (every
caller binding the eval code may reach, then the receiver, new.target, the
dynamic environments nearest first, the super home and the derived
constructor's field initializer, each a local, a capture, the running
function, `this`, the global object or new.target) as validated rows, and the
whole `EvalSite` as an opaque record (`encode_eval_site`). The loader checks
one site per marked call, the sources in capture order and within the calling
function's locals and captures; snapshot validation checks the same. The VM
decodes each marked call to `Ins.DirectEval`. When its callee is the realm's
%eval% itself (a Proxy or a bound copy is an ordinary call, bbp-282), a
non-string first argument is the result and extra arguments were evaluated
and are ignored; a string goes to the run-time compiler below with the site's
record, which compiles it against that scope as the Pants engine does
(`compile_direct_eval`: strict or sloppy as the caller, its bindings with
their const and function-name rules, the temporal dead zone, the lexical
names a sloppy `var` may not redeclare, new.target and super only where the
caller has them). The caller's values for the sources, read from its frame
at the call, become the eval closure's captures (a binding cell is the cell,
so the eval code shares the caller's variable); the closure runs in a new
frame of the dispatch loop with no receiver of its own, and its completion
value is the call's result. Eval code reads a sloppy caller's receiver
through `CheckSloppyThis` (undefined and null are the global object, another
primitive is boxed, as a sloppy function's own `this` is). Sloppy eval code
declares its `var` and function bindings through the caller's variable
environment, a binding cell holding a null-prototype object (light-cljs-855),
so closures made before or after the eval see them, or, at script level, as
configurable globals; in parameter expressions that environment is the
parameter scope's. A `super` reference in eval code stops at the super
opcodes the objects-classes area implements. No fallback: a call without a
site, or eval code whose capture count is not its site's, fails
UnsupportedOperand by name. Calling the global `eval` any other way is an
indirect eval (below, light-cljs-856).

**Run-time compilation** (light-cljs-856). Indirect eval, and after it the
Function constructor (light-cljs-857) and direct eval (light-cljs-858), hand
the VM JavaScript while a job runs. It is compiled by the same Rust frontend
and Goose exporter that made the job's image, through an in-process C
boundary: `rust/examples/goose_frontend.rs`, a `cdylib` example target of
the existing package (no new crate), exports `bbp_goose_compile` and
`bbp_goose_free`; `src/runtime_compiler.h`, a `c_headers` adapter in each VM
program's `.build` sidecar, implements `runtime_compile.goose`'s `extern fn
compile_runtime` over it. The input is the source's exact UTF-16 code units
(lone surrogates carried as the Pants engine's eval carries them, bbp-279:
`frontend::push_utf16`, which the Pants engine now shares), the mode
(indirect eval; the Function constructor's source with its parameter
boundary; direct eval with its site's opaque record, light-cljs-858) and the
strictness; the output is a GSP image
(`frontend::goose::export_runtime`) or a compile error with its message and
position, which becomes a catchable SyntaxError. The frontend runs on a
helper thread with a 2 MiB stack, as the Pants engine's eval does, because
its nesting bound is proven for that stack and a Goose worker may have less.
The boundary returns bytes and never runs code. The VM validates the image
(`program::install`) and checks its flow (`load`) as it does a loaded
program's, then appends its functions to the job's code (`append_code`,
`Job.writable_code`), above the image's function count, with its constants
in the heap's constant rows; the eval script's root function is marked as
eval code (`Function.eval`), so its `var` and function declarations make
configurable global properties and a refused one is a TypeError
(EvalDeclarationInstantiation). The instruction rows count against the job's
`Limits.code` (a MemoryLimit) and the source and instructions against its
work budget. The eval script runs in a new bytecode frame of the one
dispatch loop (decision 4), so budgets, limits and cancellation apply inside
it. Decision 3 keeps: the worker's reset truncates the code tables to the
image with every other table, so the next job's code is a fresh worker's; a
saved continuation or captured snapshot carries the whole code tables, eval
code included, and validates them on restore (`snapshot::code_valid`), and a
callable publication carries its code suffix (`callable.goose`).
Initializers compile too (light-cljs-863): a definition's factory runs in
its preparer, a thread program over a copy of the base image, so its eval
code joins the prepared definition's code above the image, and the
definition snapshot carries it to every job that installs it
(`snapshot::extend`); a helper's initializer runs in its production job on
a job worker, and a closure helper's callable publication carries the eval
code as part of its code suffix, relocated behind whatever code the
consuming worker already holds (a data helper leaves no code behind). In
the serial measurement control, whose one heap keeps every definition, the
code stays in that heap. The coordinator's image never changes: no job runs
there. A job without writable code fails such a compilation by name
(UnsupportedOperand), as does a program `goose run` executes in-process
(TinyCC has neither `dlfcn.h` nor the C library's symbols). Code that does
not compile at run time is unaffected: a job's writable code is one
reference, read only when it compiles.

*The Function constructor* (light-cljs-857). `Function(...)` and `new
Function(...)` run as the DynamicFunction native frame
(`runtime_compile.goose`): the arguments convert by ToString in order, an
object's in a Convert frame above it (decision 5), then the frame assembles
`function anonymous(` + the parameters joined by `,` + `\n) {\n` + the body
+ `\n}`, compiles it in the dynamic-function mode with the parameter list's
end as its boundary, and makes the root function a closure with no
captures, so it sees the global scope and never its caller's. Unlike eval
code it is not marked `Function.eval`. A newTarget other than Function
(Reflect.construct) gives it that constructor's `prototype` after the
compilation. Generator, async and async generator functions made at run
time are the generators area's (light-cljs-768): so that no lookup reaches
the ordinary constructor by accident, those closures now inherit realm rows
38 to 40 (%GeneratorFunction.prototype% and its siblings, `fixed_rows()` 41),
whose `constructor`s are pending properties that fail by name.

*The library and its cost.* The header opens the library with `dlopen` on
the first compilation in the process, never at start-up: a program that
never compiles at run time neither loads nor needs it, and its start-up
and peak memory are unchanged: the generated 200k project (seed 42, `compare.bb
--phase check`, alternating rounds against the master runner, outputs
byte-identical) took a median 6.95 s against 6.90 s at one Goose worker
(four rounds; peak memory 0.967 GB for both) and 5.04 s against 5.05 s at
four (five rounds under load; 0.978 and 0.977 GB). The library is
`$BBP_FRONTEND_LIBRARY`, or by default `target/release/examples/` of the
repository the header is in; the scripts that run VM programs build it
(`cargo build --release -p big-boy-pants-js --example goose_frontend`) and
set the variable. Measured on the conformance runner (macOS, release
library, median of 15 runs under load): a script without eval 4.6 ms and
2.1 MB peak memory; one eval 4.8 ms and 3.6 MB (the first call opens the
1.7 MB library); 1,000 evals 36 ms, about 31 µs an eval (helper thread,
compilation, validation and decoding). No source-length cap beyond the
exporter's 8 MiB of UTF-8: the Pants engine's 16,384-unit RangeError
(`MAX_SOURCE_UNITS`), which QuickJS does not have, is not ported, nor is any
row that depends on it.

*Rejected.* *A request to the host through the compiler session's pipes*:
the conformance runner and the Goose test programs have no host, and every
eval would become a suspension of the job and a round trip. *A subprocess
per compilation*: a process spawn per eval and a tool path at run time.
*Linking the frontend statically into every VM program*: every `goose build`
of a VM program (scripts, benchmarks, the 200k gate, the compiler tests)
would need a prebuilt Rust static library and a linker search path, and
every binary would carry the frontend. *Goose's `extern "library"` block*:
it opens the library at program start and fails the program when it is
missing, so the 200k runner would pay for it at every start. *A JavaScript
parser in Goose*: forbidden (no second parser).

**Form opcodes.** The light-cljs collection opcodes (`NewFormList`,
`FormOp`, `NewVector` and the rest) stay in the one append-only registry and
decode into the same `Ins` enum; their execution stays in the form module. If
Goose ever needs an opcode of its own, it is appended to `instr.rs` like any
other, and the Rust VM gets only a refusal arm (bbp's AGENTS.md forbids
extending that VM). There is no Goose-private numbering.

**Evidence.** The probe interpreter decodes the real exported map benchmark
(114 instructions, 23 opcodes) into 1,026 bytes, 9 bytes an instruction,
against 5,472 bytes (48 each) for the same program in today's `Op` table.
Dispatch is `match` on the enum (rule 4). Instruction fetch keeps one bounds
check, which a `Pc = u32 in ins` index type removes (rule 7).

**Rejected.** *Growing the filter*: one unsupported opcode anywhere, even
unreachable, refuses the whole file, so the harness would report whole files
as unsupported and hide which cases already pass, and the filter is a second
list to keep in step with the VM. *A Goose bytecode of its own*: a second
compiler, which the group forbids. *Failing at load*: correct programs whose
unimplemented paths never run would be refused. Failing at execution is still
explicit, with no fallback; the 200k project's macros use only implemented
opcodes, so its path does not change.

## Decision 2: values and objects

**Decision.** A JavaScript value is a fixed-mode enum of 9 bytes:

```goose
type ObjId = u32;   // a plain row ID (light-cljs-797); and StrId, SymId ...
enum Value { Undefined, Null, Bool { v: bool }, Number { v: f64 },
             String { s: StrId }, Symbol { s: SymId }, BigInt { b: BigId },
             Object { o: ObjId }, Empty }   // Empty: holes and TDZ, never escapes
```

The heap is a set of flat tables in the thread program that runs the job,
linked only by plain `u32` row IDs (amended below):

- `objects`: fixed-size object headers holding the prototype (a `Value`), a
  shape, four inline property slots, an optional overflow run in `slots`, and
  the object kind as a fixed-mode enum whose payload carries the kind's
  internal slots (array length and element run; function ID and environment;
  bound target; Proxy target and handler; typed-array buffer, offset, length
  and element type; Map/Set entry run; Date time value; RegExp program;
  Promise state and reaction run; generator continuation; module namespace
  bindings; error data).
- `shapes`: immutable layout records (parent shape, key, attributes, slot)
  with transition entries, so an object stores its keys once per shape.
  Writable, enumerable and configurable live in the shape entry. Objects
  with deletions or very many keys switch to dictionary mode (a key and value
  run of their own). An accessor property's slot holds an index into an
  `accessors` table of getter and setter values.
- `slots`: value runs for properties and dense array elements. Holes are
  `Empty`. Sparse or huge arrays use dictionary mode.
- `strings` and two unit tables: a string header holds its encoding
  (Latin-1 or UTF-16), start and length in `latin1: u8[>..]` or
  `utf16: u16[>..]`. Property keys are interned once per image, so shapes and
  lookups compare key IDs. Exact UTF-16 strings stay separate from source byte
  spans, as bbp's AGENTS.md requires. The string area may add a rope variant.
- `symbols`, `bigints` with a `limbs: u32[>..]` table, `envs` (closure
  environments: parent and slot run), `accessors`, `conts` (detached
  continuations, decision 4).

A closure is data: a function object whose kind payload is the function ID
and its environment (rule 4). Object identity is the `ObjId`.

**How it follows the rules.** Values are a compact fixed-mode enum (rule 3);
links are validated index types, position independent because they count
from their table's start (rule 2); every table is flat, so the image crosses
workers by copy (rule 6). Object kinds dispatch by `match` on the kind enum
(rule 4). Reads through an index type are free of bounds checks: in probe P7,
`objects[r.o]` lines report "2 elided, 0 kept", while slot-run reads keep one
check until runs use an index type too (rule 7).

*Amended by light-cljs-797 (the user's decision, November 5, 2026):* rows
link by plain `u32` row IDs, not index types. Goose refuses any index-typed
value crossing threads ("cannot pass between threads: it holds an index
type", light-cljs-785's probe and P17), and VM values cross threads in every
snapshot, saved continuation and message. Row reads keep Goose's bounds
check; an ID from another thread program is checked once against the
receiving table's length; tables may be truncated (decision 3). light-cljs-785
landed the values with `u32` payloads and light-cljs-800 finishes the tables.

*As landed by light-cljs-800:* node, string-header, source-range and function
rows link by `u32`; row reads go through helpers that name their table
(`source::node`, `slot`, `run`, `property_row`, `header`, `constant`,
`source_range`) and fail by that name, which also lets Goose drop the read's
own bounds check. Each table grows through one allocation function whose
limit counts rows above the heap's image tops. Strings are Latin-1 (one byte a
unit) in their own table when every unit fits and UTF-16 otherwise, chosen
canonically so equal strings share an encoding. Object headers keep **two**
inline property slots, not four, so a header (121 bytes) stays below its
size before the change (125 bytes) while most form nodes, which have no
properties, pay for them; shapes and attributes remain light-cljs-787's.

*As landed by light-cljs-787 (the property model):* `source_vm.goose` adds
three tables, all plain `u32` rows like the others. `shapes` holds
immutable layout records (parent, interned key, attribute bits, property
count) with their transitions as a newest-child link and an older-sibling
link, so objects built with the same keys in the same order share one
shape. `accessors` holds getter/setter pairs; an accessor property's slot is
the internal `Value.Accessor` naming its pair, never written in place (a
merge makes a new pair). `keys` is an open-addressed index of interned key
headers (`Str.key`); string constants are interned when code is installed,
so a named access compares one `u32` per shape entry and hashes nothing.
An ordinary object stores its shape, its prototype (`Null`, an object, or
`Empty` for its kind's realm prototype, light-cljs-788, decision 6) and its values: two inline, the
rest in an overflow run of `values`. A delete, an attribute change or more
than 64 keys moves it to dictionary mode, records of key, value and
attributes in `properties`. The internal methods (`own_entry`, `lookup`,
`assign`, `define_property`, `has_property`, `delete_property`, `own_keys`)
dispatch on the object kind with `match`; arrays and functions' `call`
override the ordinary ones (arguments objects are ordinary, light-cljs-854),
other kinds fail by name. A getter runs as a frame entered from the read; a
setter runs under a `Setter` native frame that drops its return value.
Reused workers: a job that adds a transition to an image shape or a key to
the index marks the image written, and the reset copies both back from the twin
(`snapshot::truncate`), so a job's shapes and keys vanish with its rows.

*As landed by light-cljs-804 (key enumeration):* an ordinary object keeps
its index-keyed data properties with every attribute set in an element run
of `values` (its header's `start` and `count`, holes where absent, no
length), as an array does, so `o[i]` with a number reads and writes the slot
without building a key string. An index key with other attributes, an
accessor, or an index far past the run stays a shape key or record; a key
lives in one place, and `own_keys` sorts both kinds of index key first. A
name pays one header read (`element_key` decodes only digit-led keys).
for-in, object rest and spread enumerate through a *key list*, a node of
kind 16 whose records in `properties` hold own keys with the attribute bits
they had when listed, the object the walk is on, the first record of its
keys and the next record: the Pants engine's `ForInIterator` as table rows a
job allocates and its frames alone hold, truncated with the job.
`ForInNext` re-checks each key against the object's current attributes,
lists a prototype's keys when the walk reaches it, skips names an object
nearer the operand had, and stops at `Null` or the pending
%Object.prototype%. `OwnKeys`, `IsEnumerable`, `ToPropertyKey` and
`RequireCoercible` run object rest; `CopyDataProperties` runs as a
`CopyData` native frame, one key a step, a getter entered above it.

*As landed by light-cljs-805 (arrays' and functions' named properties):* a
dense array keeps its named properties in a shape beside its element run,
as an ordinary object does (the header's key fields were free); a
dictionary array keeps them as records after its index records, and the
move to dictionary mode (a far write, a delete, an attribute change, more
than 64 names) carries them along, as a shorter `length` keeps them. A
closure keeps its own properties the same way beside its captures. An
ordinary function's `prototype` (constructor kind 1) is made on its first
observation, a read of `prototype` (on the function or inherited from it)
or the first property added, as the Pants engine's `vm/function.rs` does:
an ordinary object whose non-enumerable `constructor` is the function,
stored as a writable, non-enumerable, non-configurable property that comes
first and never goes, so a function without own properties has not made it
yet; `in` reports it and a delete refuses it without making it. Only the
job that allocated the function makes it; reading another's fails
`UnsupportedOperand`, as the Pants engine fails for a published function. A
function's `length` and `name`, a class constructor's or generator's
`prototype` and a function's key list still fail by name, as do the names and symbol keys
%Array.prototype% and %Function.prototype% will define (a lookup reaching an
array's or function's pending prototype). ToPrimitive runs a `valueOf` or
`toString` getter in its native frame (pc 2), then calls the method it
returned. Publication rejects a closure with own properties as mutable.

*As landed by light-cljs-845 (Proxy for for-in):* `src/proxy.goose` adds
node kind 24 (`node_kind::Proxy`), whose internal slots are its target
(`value`, always an earlier row) and handler (`namespace_value`). A
proxy's [[OwnPropertyKeys]], [[GetOwnProperty]] and [[GetPrototypeOf]]
run as a `Proxy` native frame (decision 4: every trap getter, trap call
and read of a trap result is a frame above it) that forwards to its target
without a trap and checks the specification's invariants; a key list is
their list result and a descriptor is a key list whose records are its
fields. for-in pushes a `ForIn` frame only when its walk meets a proxy.
Every other internal method of a proxy fails `UnsupportedOperand` naming
it (`Proxy: [[Get]]`, `Heap.proxy_method`), for light-cljs-766.

*As landed by light-cljs-890 (Proxy get, set, has and deleteProperty):*
the same `Proxy` frame runs [[Get]], [[Set]], [[HasProperty]] and
[[Delete]] with their invariants, forwarding without a trap to the
target's ordinary method (a proxy on the target's prototype chain
restarts the frame on it, a bounded number of times). No new table and
no new check on the ordinary path: own_entry's proxy arm already faulted
by name, and only that cold fault path changed. `lookup` turns it into a
*proxy hit*, returning the proxy as the getter with the proxy and key in
two transient heap fields (`Heap.proxy_at`, `proxy_key`), so every
reader that runs getters (the dispatch loop's GetNamed and GetIndexed,
`call_accessor` and every built-in's `run_getter`) runs [[Get]] traps with
its receiver; `has_property`, `assign` and `delete_property` keep the
fault, relabelled with the proxy and key recorded, and each caller that
supports the method (`in`, `delete`, assignments, super and Reflect.set,
has and deleteProperty, Object.assign, push, map and forEach presence,
ToPropertyDescriptor, Error's `cause`) starts the frame instead. A
trapless [[Set]] that defines on the proxy receiver fails by name at its
[[DefineOwnProperty]] (light-cljs-891).

*As landed by light-cljs-891 (the remaining traps and Proxy.revocable):*
the `Proxy` frame runs every internal method: [[Call]] and [[Construct]]
(the arguments become an array the frame holds; without a trap the frame
becomes a `Forward` frame that runs the target as any forwarded call, a
construction keeping its newTarget), [[DefineOwnProperty]] (the trap gets
FromPropertyDescriptor's object), [[IsExtensible]], [[PreventExtensions]],
[[SetPrototypeOf]] and [[GetPrototypeOf]] everywhere, each with the
specification's invariants, a proxy target's [[IsExtensible]] and
[[GetPrototypeOf]] for those invariants running as a nested Proxy frame.
Calls reach it where they would push a closure's frame (`push_frame`,
`construct`, `call_accessor`), so ordinary calls test nothing new; the
methods with a boolean answer carry an answer mode (TypeError on false,
the object, or undefined), so Object.preventExtensions, Object and
Reflect.setPrototypeOf and the `__proto__` setter deliver their results
from the frame. Those built-ins, Object and Reflect.isExtensible and
Object.prototype.isPrototypeOf became conditional natives; instanceof's
and isPrototypeOf's walks share `prototype_walk`, which waits for a
proxy's [[GetPrototypeOf]] at a `HasInstance` frame's pc 5; the
`Descriptor` frame runs a proxy target's [[DefineOwnProperty]] at its pc
4; a trapless [[Set]] with a proxy receiver (an assignment, Reflect.set,
a super assignment) reads its [[GetOwnProperty]] and then defines through
its [[DefineOwnProperty]]. Proxy.revocable's revoke function is a job row
of a new built-in naming its proxy; revoking sets the proxy's revoked flag
(its node's `initialized`: proxy node state, decision 2; target and
handler stay, so an operation past its handler read keeps it), after
which every internal method,
Array.isArray and Object.prototype.toString's tag are a TypeError while
`typeof` stays. No new table. Still named failures: Object.seal, freeze,
isSealed and isFrozen of a proxy and a proxy target's [[OwnPropertyKeys]]
in the ownKeys invariants (light-cljs-897), getOwnPropertyDescriptor
through proxies (light-cljs-895), bind of a proxy (light-cljs-861), and the
plain built-ins run to completion where no frame can start.

*As landed by light-cljs-897 (integrity levels of proxies, proxy targets
in the ownKeys invariants):* Object.seal, freeze, isSealed and isFrozen
became conditional natives: of a proxy, SetIntegrityLevel and
TestIntegrityLevel (ECMAScript 7.3.15 and 7.3.16) run in light-cljs-884's
Statics frame, whose key list and cursor walk the proxy's keys, each of
the proxy's internal methods a Proxy frame above it: [[PreventExtensions]]
(seal, freeze; false is a TypeError) or [[IsExtensible]] (the tests, read
first, as the specification orders it), [[OwnPropertyKeys]], then per key
[[DefineOwnProperty]] of {configurable: false} (seal), [[GetOwnProperty]]
and a definition that also clears `writable` on a data property (freeze),
or [[GetOwnProperty]] until a configurable (or, frozen, writable data)
property answers false (the tests); any other argument is the plain
built-in. The ownKeys trap's invariants read a proxy target's
[[IsExtensible]], [[OwnPropertyKeys]] and each of its keys'
[[GetOwnProperty]] as Proxy frames above the [[OwnPropertyKeys]] frame
(its pc 12, 14 and 15), each read key's row of the target's key list
marked configurable or not; any other target is read at once as before.
No new table, node field or native kind (decision 2): the frames' fields
and job rows hold the state, which snapshot validation checks
(corpus/serial/proxy-integrity-wait). [[DefineOwnProperty]] of a row the
job did not allocate (decision 3) now accepts a definition that changes
nothing, as SetIntegrityLevel of such a row already did, so sealing or
freezing a proxy of an image object already at that level succeeds; any
other definition there still faults NativeBoundary before anything
changes, and the compiler's protected rows are not read. Three steps where QuickJS departs
from the specification (and V8) follow the specification: the tests read
IsExtensible before the keys, freeze skips a key whose descriptor is gone,
and freeze defines {writable: false} on a read-only data property too.

*As landed by light-cljs-860 (mapped arguments objects):* a sloppy
function with a simple parameter list gets a mapped arguments object
(ECMAScript 10.4.4.7 CreateMappedArgumentsObject; QuickJS
`js_build_mapped_arguments`) when it can observe the aliasing, and bbp's
Rust lowering decides that statically, only when it lowers for Goose
(`Backend::Goose`, which the Goose exporter and the Goose run-time compiler
use; the Pants command line and the Pants engine's eval lower for
`Backend::Pants` and keep today's bytecode, so the Pants oracle is
unchanged). A function that refers to its `arguments` only as the base of
property reads (`arguments.length`, `arguments[i]`), assigns no parameter
and has no direct eval cannot tell the two apart, so it keeps NewArguments
and plain parameter locals at no cost; strict functions, arrows, functions
that never mention `arguments` and the compiler path (whose ClojureScript
output Goose exports as strict code) never change. Otherwise the lowering
keeps every parameter in a binding cell and emits NewMappedArguments
(opcode 199, appended to the shared registry; the Rust VM refuses it). On
Goose it makes the arguments object exactly as NewArguments does and turns
each parameter local into a cell: below the argument count a mapped cell
(node kind 27, `node_kind::MappedBinding`: the object and the index), past
it an ordinary binding cell. The object's own property is the truth while
an index is mapped: the cell's reads and writes go to it (an element, or a
shape key or record after a redefinition that kept the mapping), so every
other operation on the object, the fast element paths and fnRest included,
is an ordinary object's. The object's `namespace_value` names an argument
map (node kind 26, `node_kind::ArgumentMap`) whose run lists each mapped
index's cell; [[Delete]] and a redefinition as an accessor or as
non-writable unmap an index: the map entry becomes `Undefined` and the
cell an ordinary binding holding the parameter's value, as data. Links are
plain node rows, so snapshots validate them (an object's map, each entry's
cell naming that object and index, each mapped cell listed by its
object's map), in-job reclamation remaps them (a cell's link to its
object is weak there; see below), a generator's or async function's
detached activation keeps the cells as any locals, and direct eval reaches
the parameters through the same cells. A duplicate parameter name leaves
its earlier occurrences' cells unread, so only the last occurrence
aliases. One limit of the static decision: a getter on Object.prototype
reached by `arguments[i]` past the argument count receives the object as
its receiver and could write a parameter through it while the function
has an unmapped object; installing such a getter needs
Object.defineProperty, which Goose does not have yet (light-cljs-766).
*Rejected:* Goose alone aliasing the indices to the frame's parameter
slots, or to the cells the lowering already made for captured
parameters, and detaching them when the frame returns. A parameter that a
closure captures by value (never assigned in the source) would then miss
writes through the object, which only a lowering change can see, and the
detach would need a pass over every returning frame's arguments object,
the hot return path every function takes.

*Amended by light-cljs-867 (a kept parameter no longer keeps its arguments
object):* a mapped cell names its whole arguments object, so a closure that
kept one parameter kept every argument, the map and the other cells, where
QuickJS keeps only the parameter's variable reference. In-job reclamation
(decision 3, `src/reclaim.goose` "Mapped parameter cells") now treats that
link as weak: tracing lists each mapped cell above the job's floor instead
of marking its object and marks the cell's current value in its place
(which can reach the object after all, as when a parameter holds its own
arguments object, so marking repeats until every listed cell is resolved).
A cell whose object is still unmarked then becomes an ordinary binding
holding that value, exactly as unmapping leaves it, before the rows slide,
and the object, its map and whatever only they held are discarded; an
object some root reaches keeps its cells mapped. The VM's own paths are
unchanged, so strict functions, arrows and functions without the mapped
form cost what they did, the snapshot link validation is untouched (a
detached cell is an ordinary binding) and the collection pays only for the
mapped cells it meets. `corpus/serial/sloppy-mapped-reclaim` keeps 300 such
closures while discarding the 4,000-element arrays their arguments objects
held and completes within the job's 1,048,576-value limit, which it passes
without this. *Rejected:* inverting the link (cells own their values, the
map lists the cells and the object's mapped elements read and write
through them): every element read, write, delete, definition and
enumeration of an arguments object, fnRest's run and the fast element paths
would have to consult the map, and the snapshot and reclamation link rules
would change with them, for a retention that only a collection can observe.

*As landed by light-cljs-881 (property descriptors):* `src/descriptors.goose`
adds Object.defineProperty, Object.defineProperties, Reflect.defineProperty,
Reflect.getOwnPropertyDescriptor, Reflect.deleteProperty and Annex B's
`__defineGetter__`, `__defineSetter__`, `__lookupGetter__` and
`__lookupSetter__`. Attributes stay in shapes and dictionary records; no
table is added. ValidateAndApplyPropertyDescriptor (`define_descriptor`)
reads the current property through `own_entry`, decides the complete new
property (a data value or an accessor pair, a fresh accessor row) and
writes it through `define_property` with `half::Complete`, which every
kind already dispatches; a definition that changes nothing writes nothing,
so a String wrapper's exotic properties, a frozen template array and a
non-configurable property accept only that. An attribute change of a
shape-mode object moves it to dictionary mode (light-cljs-787's rule; no
shape transition was measured better, since definitions are rare on the
paths the generated project takes). An array element with other
attributes, or an accessor, moves the array to dictionary mode
(light-cljs-805's records) and an array's two flags live in its node's
otherwise unused `function` field (`array_flag::ReadOnlyLength`,
`array_flag::Attributed`): an array with either flag leaves the element
fast paths (`dense`), so an array no definition touched costs one more
field test there, and assignment checks the element record and the
read-only `length` (ArraySetLength, which also stops a shortening at the
last non-configurable element). ToPropertyDescriptor and
defineProperties's reads run as a `Descriptor` native frame (decision 4),
sharing proxy.goose's field reader; every getter and setter call outside a
closure (built-ins, bound functions) goes through the general call path
(`call_accessor`, a Forward frame).

*As landed by light-cljs-882 (extensibility and integrity levels):*
[[Extensible]] is node state, a `non_extensible` byte of the object row,
which every kind with own properties carries and only the allocating job
(or the realm, for %ThrowTypeError%) sets; `src/integrity.goose` adds
Object.preventExtensions, isExtensible, seal, isSealed, freeze, isFrozen
and Reflect.isExtensible and preventExtensions as plain built-ins. A
non-extensible object refuses a new property in assignment and definition
and another prototype; existing properties keep their attributes.
SetIntegrityLevel needs no table either: shape keys get a new chain of
transitions with the restricted attributes (values keep their slots, and
objects frozen alike share shapes), dictionary records change in place,
an ordinary or arguments object's elements become keys (freezing unmaps a
mapped arguments object's indices first; sealing keeps them mapped), and a
dense array keeps its run, with two more flags in its node's `function`
field (`array_flag::Sealed`, `array_flag::Frozen`) giving its elements'
attributes, so a frozen array stays dense and costs nothing an element; a
move to dictionary mode turns the flags into its records' attributes.
light-cljs-862's template objects are such frozen arrays, so the
`immutable` marker on arrays, the template-only definition path and the
`sealed()` special case for %ThrowTypeError% are gone, and proxy
invariants read real extensibility. *Rejected:* moving a sealed or frozen
array to dictionary mode, which turns every element into a record found
by a linear search (a frozen array of n elements would cost n records and
a quadratic walk to iterate). Forms (light-cljs runtime values) fail by
name: they take no properties here, while the V8 build's runtime makes
them ordinary extensible objects. Objects that never stop being
extensible pay one byte test where a property is added; the dense-array
fast paths test the byte where they tested the template marker.

*As landed by light-cljs-883 (prototypes):* the prototype stays the
object row's `proto` field, Empty meaning the kind's realm prototype, and
[[SetPrototypeOf]] (`set_prototype` in `src/source_vm.goose`) writes it for
every JavaScript object kind, keeping the kind (an array stays an array).
It accepts the prototype an object has with no write, refuses a cycle (the
walk stops at a proxy), a non-extensible object and a new prototype for
%Object.prototype% (an immutable-prototype exotic object), faults
NativeBoundary for a row the job did not allocate, and stores a prototype
equal to the kind's realm one as Empty. Shapes hold no prototype, so a
change invalidates nothing. The one shortcut that assumed the realm
prototype is the dense-array path, which reads a hole as undefined because
no realm prototype holds an element in a job: it now also requires Empty,
so an array with another prototype takes the general path, where a hole
inherits and an inherited read-only element or setter decides a write.
Objects whose prototype never changes pay one more field test there.
`src/prototypes.goose` adds Object.create (a native frame when it has a
properties argument, which runs Object.defineProperties's Descriptor frame
on the new object), Object.setPrototypeOf, Object.prototype.isPrototypeOf
and Reflect.getPrototypeOf and setPrototypeOf as built-ins, and
Object.prototype's `__proto__` becomes a realm accessor pair (accessor row
2, checked by `realm_valid`) whose getter and setter are built-ins; the
literal `__proto__: v` stays SetProto. Object.getPrototypeOf,
Reflect.getPrototypeOf and the `__proto__` getter run a proxy's
[[GetPrototypeOf]] as light-cljs-845's Proxy frame; a proxy's
[[SetPrototypeOf]] and a proxy on isPrototypeOf's walk run as Proxy
frames since light-cljs-891; forms fail by name. A realm row never changes its prototype,
even in a runner whose jobs may write realm rows.

*As landed by light-cljs-884 (Object statics):* the built-ins that expose
own keys (Object.keys, values, entries, getOwnPropertyNames,
getOwnPropertySymbols, getOwnPropertyDescriptors, assign, fromEntries and
groupBy, Object.prototype.propertyIsEnumerable and toLocaleString,
Reflect.ownKeys and has) take their keys from `own_property_keys` into
light-cljs-804's key lists, so no new table exists, and run as one
resumable native frame, Native.Statics with the built-in as payload
(`src/object_statics.goose`); a call that meets no getter, setter,
callback, iterator, key conversion or proxy pushes and pops it in one step,
as a plain function would. EnumerableOwnProperties and Object.assign
re-read each key's [[GetOwnProperty]] just before using it. Object.fromEntries
and Object.groupBy iterate through `get_iterator` and hold a Catch handler
on their own frame while an item is processed, so an abrupt completion
there closes the iterator (its `return`'s exception dropped, the original
rethrown) and an exception from `next`, `done` or `value` does not; the
handler is plain handler-stack data, saved with a suspended stack like any
other. A proxy's [[OwnPropertyKeys]] and [[GetOwnProperty]] run as
light-cljs-845's Proxy frame above it, as do its [[Get]], [[Set]] and
[[HasProperty]] since light-cljs-890. Object.prototype.toString,
Object.hasOwn and hasOwnProperty stay plain built-ins, but every caller
that can start a native frame (`conditional_native`, formerly the error
built-ins' `error_native`) runs a @@toStringTag getter or a key object's
ToPrimitive in the same frame.

*As landed by light-cljs-885 (Symbol values):* a symbol a job makes is a
row of the node table, `node_kind::JsSymbol` (its description, a string or
undefined; whether Symbol.for registered it; and the symbol registered
before it), not a table of its own, so the node table's limit, truncation,
snapshot copy and reclamation cover it. A `Value.Symbol` payload below
`symbol_floor()` (64) is still a well-known symbol's index, the same in
every program and taking no row; one at or above it is the floor plus the
symbol's node row. Values stay the 9-byte enum; `===`, `Object.is`,
SameValueZero and property keys (shapes and records compare key words)
compare the payload, so a symbol is its own identity and no comparison
path changed. The Symbol.for registry is a chain through the registered
rows whose head is a heap state field (`Heap.registry`, carried in the
snapshot header): the worker reset restores it with the heap's other
state, saved continuations and definition snapshots carry it, and
reclamation keeps and moves every registered symbol; Symbol.for walks the
chain, so no lookup structure lives outside the rows (decision 3).
Reclamation also moves every symbol payload above its floor in values,
shapes, records, frames and continuations (`reclaim.goose` relink).
*Identity across jobs:* a symbol never leaves the job that made it. A
publication (a prepared definition or produced helper) whose value is or
reaches one fails by name (`symbol-unshareable`, `cells.goose`
publish_cells; `callable.goose` refuses one too), and a cell value holding
any symbol already failed (`a symbol or bigint value`). A registered
symbol is refused as well: comparing registered symbols by key across jobs
would need either key comparison in every symbol `===` and key lookup or a
shared registry, and no workload needs it yet; a well-known symbol, an
index, publishes as before. %Symbol.prototype% is realm row 41, an
ordinary object (its methods plain built-ins in `src/symbols.goose`; the
description conversion of Symbol(o) and Symbol.for(o) runs in the call
instruction's Convert frame, as String(o)'s does), the [[Prototype]] of
Symbol wrappers (`node_kind::Wrapper` holding a symbol) and where a symbol
primitive's property reads and writes start (`primitive_prototype`).

*As landed by light-cljs-886 (base classes):* a class is a closure row
(no new table or node kind) whose code's constructor kind is
`constructor::BaseClass` or a derived kind. Unlike any other closure it
is never lazy: `closure` makes its `length`, `name` and `prototype` when it
makes the closure, the `prototype` a new ordinary object with a
non-enumerable `constructor`, and itself neither writable, enumerable nor
configurable. A computed class name (`{ [k]: class {} }`) replaces the
made `name` value (`set_computed_name`); snapshot validation refuses a
class closure without properties. Class elements are own properties of
their home object, defined by `Ins.DefineClass` (DefineClassMethod,
DefineClassGetter, DefineClassSetter) through `define_field`'s ordinary
[[DefineOwnProperty]] with class attributes (not enumerable; configurable;
methods writable; getter and setter halves merging). [[Call]] of a class
is a TypeError in `push_frame`, which enters class code only with a
new.target; [[Construct]] of a base class is an ordinary function's
(`construct`, Reflect.construct's Forward frame included; a newTarget
whose `prototype` is an accessor runs its getter above a `Construct`
native frame, which then enters the constructor, for ordinary functions
too, decision 4), and the
instance-field initializer the Rust lowering keeps in the class's
`*instance-fields*` cell runs as an ordinary method frame of `this` at the
start of the body, so fields, static fields and static blocks need no VM
state of their own: they are closures, cells and DefineField/DefineElem on
`this`. Derived classes (`InitHeritage`, `CheckHeritage`, `ConstructSuper`
and the rest, light-cljs-887), `super` properties (light-cljs-888) and
private names (light-cljs-889) still fail by name. *Home object:* the
`Method` opcode's home object is not stored yet; light-cljs-888 adds it
(see its note below).
The intended place is the method closure's `Node.value` (unused by
closures), which reclamation (`mark`/`relink` of every node's `value`) and
snapshot validation (`value` of every node) already cover; 888 must also
decide what a publication does with a method whose home object is a
mutable object (today a lazy method publishes; reaching its home object
would refuse it).

*As landed by light-cljs-887 (derived classes):* no new table, node kind or
frame field. `InitHeritage` sets the class row's `proto` to the parent and
its `prototype` object's to the parent's `prototype` (null for `extends
null`). A derived class's [[Construct]] makes no object: its frame's
receiver is a new uninitialized `node_kind::Binding` row, the Rust
lowering's "receiver cell", which `BindThis` initializes once with the
result of super() and `ResolveThis` reads (a ReferenceError while
unbound); arrows and eval code capture the cell as they capture any
binding, and because the frame names it as its receiver, waits, saved
continuations, reclamation and snapshot validation carry it with no new
rule. `return_value` gives the bound `this` for an undefined return (a
ReferenceError if super() never ran) and a TypeError for another
non-object. `ConstructSuper` and `ConstructSuperSpread` are `construct`
with the shared newTarget, so every parent kind (class, ordinary
function, bound function, built-in; a proxy since light-cljs-891)
resumes as a `new` does; `ForwardArguments` decodes to
`Rest { skip: 0 }`. Constructor built-ins with a newTarget other than
themselves read its `prototype` first (`construct_builtin`; Number and
String convert their argument before it, the primitive replacing the
argument): an accessor
there runs its getter above the same `Construct` native frame, now also
for a built-in callee (snapshot-validated as a constructor built-in), and
the Function constructor reads it after compiling, at a new pc 1 of its
`DynamicFunction` frame.

*As landed by light-cljs-888 (super properties, Reflect.get and
Reflect.set):* no new table, node kind or frame field. A method's
[[HomeObject]] is its closure row's `value` (unused by closures before):
`Ins.Closure` carries whether its opcode was `Method`, whose home object
is the operand below the captures (an object literal, a class's prototype
or constructor), and a class constructor's is the `prototype` made with
it. It is a plain row link like any node's value, so the worker reset,
in-job reclamation (`mark`/`relink` of every node's value), saved
continuations and snapshot validation (which now also requires a
closure's value to be undefined or an object) carry it with no rule of
their own. `GetSuperBase` reads the home object's [[Prototype]] at each
access; `GetSuperElem` is [[Get]] on that base with the method's `this` as
the getter's receiver (`get_property` takes the receiver); `PutSuperElem`
and `PutSuperElemStrict` are `receiver_set`, OrdinarySetWithOwnDescriptor
with a separate receiver (the base's chain decides first; then the
receiver's own writable data property is written or a new one defined, a
primitive, an own accessor, a read-only property or a non-extensible
receiver refusing it, a TypeError in strict code; an array receiver's
`length` set to an object converts it first, as an assignment does), a
setter running with the receiver in the `Setter` native frame;
`ToSuperKey` and an object key at either instruction convert in a Convert
frame and the instruction runs again, after a null base has been refused.
`corpus/serial/super-reflect-wait.js` carries every one of these frames
across saved continuations. Reflect.get and Reflect.set are Statics native frames
(`object_statics.goose`, frame `receiver` the receiver, `output`
Reflect.set's value): a receiver equal to the target takes the ordinary
`write`, a separate one `receiver_set`; ordinary GetNamed, GetIndexed,
SetNamed and SetIndexed are unchanged. *Publication:* the home object is
an edge of the closure row, so the callable walk (`cells.goose`
callable_unshareable), the publication graph (`callable.goose` graph) and
artifact export follow it as they follow a captured value: a method whose
home object is a job's object (every object literal, class prototype or
constructor) is refused as "a mutable JavaScript object", as a closure
capturing that object is, and one whose home object is an image row
publishes as before. A method that never uses `super` is refused too
(the lowering does not say which do); neither the generated 200k project
nor the compiler tests publish one (ClojureScript emits `function`
expressions, which take no home object), and the 200k output is
unchanged.

*As landed by light-cljs-889 (private class elements):* a private name is
a symbol row (light-cljs-885), fresh and unregistered at each evaluation of
a class body (`NewPrivateName`, its description the name's source text),
held in the const binding the Rust lowering makes for it, so methods,
arrows and direct eval code reach it as they reach any binding. An
object's private elements are not properties: they are dictionary records
(key, value or Accessor, and `attr_writable()` for a field only) of a node
of their own, `node_kind::PrivateElements` (kind 29), which the object's
`metadata` names. A JavaScript object never had Clojure metadata (forms keep
theirs there, always a map, and `meta` of a JavaScript object stays nil),
so the field was free, and the Pants engine's per-object private record
(`vm/private.rs`) becomes rows rather than a map. Any JavaScript object
with own properties (`snapshot::named`) and a proxy can hold them; a form
cannot (a TypeError). `src/private.goose` runs the eight instructions:
GetPrivate, PutPrivate and the four Define instructions look only at the
receiver's own records (the brand check; a proxy has its own, never its
target's, and no trap runs), a getter runs as a frame entered from the
read and a setter under the `Setter` native frame (decision 4), a method
or getter-only element refuses PutPrivate and a setter-only one GetPrivate,
a second definition is a TypeError, `#x in o` is a TypeError for a
primitive, and a write to a row the job did not allocate faults
NativeBoundary. *Rejected:* private symbol keys in shapes and records with
an attribute bit every key path skips (QuickJS's representation): own
keys, key lists, for-in, light-cljs-845's proxy key checks, descriptors,
integrity levels (freezing must leave private fields writable) and JSON
would each need the skip, and a shape-mode object adding a private field
would share transitions with public keys. With a separate node no key path
changes, objects without private elements pay nothing, and integrity levels
and extensibility leave private elements alone by construction. The node
is a plain `u32` row: the worker reset truncates it, in-job reclamation
marks and relinks `metadata` and record runs as it does for every node
(`snapshot::record_run` includes the kind), saved continuations carry it,
and snapshot validation requires the node to hold nothing but records
whose keys are symbol rows and whose only attribute is a field's writable
bit, and each such node to be named by the `metadata` of exactly one
holder, an earlier row (the owner exists before its first private
element). Publication refuses
an object with private elements as before (it is a job's mutable object,
or a lazy closure whose `metadata` edge reaches a node kind no image
accepts), and a method using a private name captures a symbol row
(`symbol-unshareable`).

**Evidence.** Probe P4 measures the value enum at 9 bytes (three values, 27
bytes); today's `source::Value` is 40. The probe interpreter (P7) runs the
real exported bytecode with this model; its map benchmark result is in
decision 5. Today's VM needs 1,073 nanoseconds an element on the same
benchmark at 100 elements, because every element access builds a decimal key
string and walks the object's property list (38,419 string units and 10,101
property records for one 100 by 100 run).

**Rejected.** *Pool-relative links* (`&<u32 in pool.items>`): unusable from
workers (fact 1). *Self-relative links in one image array*: verified for
straight-line code but rejected by the checker in interpreter-shaped code
(fact 2), and a fixed-mode value cannot hold them. *Plain `u32` or `i64`
indices into job-local root structs*: truncatable, but unchecked by the type
system, the raw-ID heap the user ruled out. *Plain 8-byte references*: not
position independent, so snapshots would need relocation. *NaN-boxing or a
separate small-integer tag*: the enum is 9 bytes whatever the number payload,
and Goose has no unions to box into.

## Decision 3: memory

**Decision.** Memory is reclaimed by thread program.

*Amended by light-cljs-792:* a job worker now serves many jobs and is reset
to the image before each one; see
[State isolation](#state-isolation-comparison-with-shadow-cljs). *Amended by
light-cljs-797:* that reset reclaims garbage only; compile-time state
persists for the build in cells outside every worker's tables. The bullets
below describe the one-worker-per-job design that light-cljs-784 built.

- A **session worker** owns the session image: the realm (decision 6), loaded
  namespaces and their initialized definitions, in its tables. They only grow
  for the session's life. (The memory pellet may instead give this role to
  today's coordinator; what matters is that the image owner is the program
  that spawns the job workers.)
- Each JavaScript job runs in a fresh **job worker** spawned by the session
  worker. The spawn copies the session worker's tables into the job worker
  (probe P10: a job worker spawned by an image-owning worker sees its rows
  and links, mutates its own copy, and leaves the owner's rows unchanged):
  that copy is the snapshot restore. It is typed and valid by construction,
  so it needs no verification and no relocation, and there is no session
  identity in any value. The job appends above the copied image; its results
  leave as flat messages (forms); its exit frees everything it allocated in
  one step.
- Within a job nothing is reused: tables are append-only and an object whose
  run outgrows its capacity gets a new run (the probe regrew element runs 707
  times in one job without a free list). `slot_pool` and `slice_pool` remain
  the tool where reuse is needed in a non-global root, such as scheduler
  tables; they cannot be worker globals (fact 1).
- Images that arrive from elsewhere (a continuation resumed on another
  worker, a cached realm) are loaded as plain twin rows with `from_bytes`,
  then converted to index types in one checking pass with forward links
  patched after all rows exist. Probe P9 shows why: `from_bytes` checks a
  self-linked index-typed table against the live, empty destination table and
  refuses it. The pass checks every link once; it rewrites nothing.
- **Collection inside a job is evacuation, only if measured.** If a job's
  tables exceed its budget, the worker writes its live graph (reachable from
  frames, the promise job queue, the global object and the realm) as a
  compacted image and sends it to a program whose VM tables are empty (the
  coordinator when a session worker owns the image, otherwise a dedicated
  clean spawner), then exits. That program spawns the successor, which starts
  with empty tables and restores the image through the checking conversion
  below. The old worker must not spawn its own successor: the spawn would copy
  its full, unshrinkable tables, garbage included. During evacuation the old
  heap, the image and the successor's copy coexist, and that peak is charged
  to the retention budget. Images from elsewhere (below) likewise go only to
  workers spawned by a program with empty VM tables. The memory pellet
  first measures unreachable rows at job end on the 200k project and on the
  harness; evacuation is built only if a target workload needs it. There is
  no mark and sweep over a global table. (*Amended by light-cljs-847:* the
  workload came, and since reused workers truncate their tables the
  evacuation happens in the job's own tables; see "Reclamation inside a
  job" below.)
- **Accounting.** Each table has a row limit and the job a byte limit
  (today's `Limits`, per table). A job worker reports its peak rows and bytes.
  Waiting job workers count against a retention budget below the thread cap;
  over it, the session worker asks a waiting job to write its continuation
  image and exit.
- **`trim_memory()`.** Job workers never call it: their exit returns their
  stacks. The long-lived session, input and supervisor programs call it after
  a namespace batch only if the memory pellet measures a benefit. bbp calls it
  nowhere today.

**Implemented (light-cljs-784, `src/compiler_session.goose`).** Measurements
are in [the evidence](../evidence/generated-200k-performance.md), section
"One job worker per job".

- *Image owner: the coordinator* (the runner's `main`), not a dedicated
  session worker. It already holds the realm, the base snapshot, the prepared
  definition snapshots and the identity authority, and P9 requires the
  spawning program to own the tables; a separate session worker would add a
  hop to every dispatch. It is the only program that spawns job workers.
  Probe P11 shows a spawn copies only the globals the spawned thread program
  reaches, so when light-cljs-785 makes the realm the coordinator's flat
  global tables, the input thread and the reapers (which reach none of them)
  copy nothing, and each job worker copies the image as its restore.
- *One contained job worker per dispatch* (`run_job`): a start, a helper
  production and a resume of a saved continuation each get a fresh worker
  with empty tables, which restores today's flat `snapshot::Snapshot` from
  its spawn arguments. Nothing is reset or reused between jobs.
- *Reapers* (`supervise`, `limit + retention` of them) only join job workers
  and report how each ended. The coordinator keeps at most that many workers
  live, so every worker's join is taken at once and a contained abort is
  reported while its peers run. Abort, cancellation, the input/readiness
  protocol and the queue limits are as before.
- *Dependency waits block in place by default.* A worker that needs a helper
  publishes the wait and blocks with its state in its own thread program; the
  coordinator answers it through the `Answer` queue, routed by worker address
  with the re-queue pattern of `cancelled()`. A blocked worker counts against
  the *retention budget* (`BBP_RETAINED_WORKERS`, default 32, at most 128);
  over it the coordinator answers "save", and the worker writes its
  continuation and exits as before. Measured on the 200k project, blocking
  beats saving at 1/2/4/8 workers and copies 154 MB instead of 996 MB per
  session.
- *Accounting*: at most `limit` workers execute; blocked workers do not, but
  hold a retention slot until answered, cancelled or aborted; live workers
  never exceed the reaper count. The peak thread count is `2 + 2 × (limit +
  retention)` at most (274 at limit 8 and retention 128), below the 354-worker
  cap `goose check --stacks` reports. The `JOBS` trace line reports the
  totals, and the tests assert no worker or slot outlives the session.
- *Spawn cost*: a job worker has 46 data stacks, and Goose sets up and tears
  down each region per thread (probe P12: about 7.5 microseconds a data stack,
  345 microseconds for a 44-stack worker against 26 for one stack). That, not
  the 17 microseconds of P8, is the price of the scope, and it makes the 200k
  project slower; see the evidence. light-cljs-791 works on that cost.
- *Collection*: the census (`src/census.goose`, instrumented runs only)
  found no target workload near its job limits, so no evacuation is built.
- *`trim_memory()`*: measured after each idle batch in instrumented runs
  only; see the evidence for why nothing calls it otherwise.

**Why it fits compile-time use.** The 200k project runs 5,226 source macro
entries per session. At 17 microseconds a spawn that is about 0.09 s of
spawning, against 0.15 s of today's restore intervals, and the copy replaces
today's 995 MB of copying and identity relocation. The copy grows with the
session image, so the memory pellet measures image size and spawn cost on the
200k project before the old restore path is removed. Long-running JavaScript
(full programs, the harness) is the case evacuation exists for.

**Rejected.** *Long-lived workers that reset between jobs*: index-typed
tables cannot shrink (fact 3), so a reset is impossible without raw IDs.
(Withdrawn by light-cljs-792: a reset rewrites rows and moves an allocation
top back, and nothing shrinks.)
*A per-session append-only heap shared by all jobs*: jobs would see each
other's mutations. *Copy-on-write of realm objects into a job overlay*: two
kinds of object link and a lookup on every access. *Reference counting or a
tracing collector over the tables*: ruled out by the user.

### Reclamation inside a job (light-cljs-847)

light-cljs-840 found a 150,000-iteration loop allocating `{i}` ending
`MemoryLimit` at the 100,000-node job limit, which V8 and QuickJS run to
completion. The user approved collection inside a job on one condition:
jobs that do not need it must not pay for it.

**Choice: evacuation in place, past a soft limit** (`src/reclaim.goose`).
Every table's allocation function already compares its rows above the
image against its limit. While a job runs, the comparison is against the
limit less a margin (`Heap.slack`), half of what was free when the job
started. Passing it takes the check's cold path, which, if the rows still
fit the real limit, zeroes that table's margin, asks for a collection
(`Heap.reclaim`) and lets the allocation go on. Until then the cost is that
one subtraction and a flag read when the dispatch loop refuels: no write
barrier, no mark bits, no allocation counter.
`run_claimed`, at the top of its loop where the job's whole state is in its
frames, stacks and rows, runs the collection. The request also empties the
dispatch loop's fuel (`fuel` reads the flag it already sits beside), so
execute returns there after the asking instruction; bytecode also runs in
slices of at most 4,096 instructions.

The collection keeps every row a root reaches above the job's floor (each
table's length when the job started or resumed), slides the kept rows down
over the dead ones in table order and rewrites every link through the
remap; rows below the floor never move, so values the host holds across a
run stay valid. Roots: every row below the floor, every shape's key, the
code constants, the heap's global, call method and pending key, and the
job's frames, locals, operands, handlers, result, lexicals, template
objects (light-cljs-862), root receiver, entry, effect arguments and frame, promise queue, unhandled rejections,
cell rows and foreign ranges (whole, so a publication's identities stay
consecutive and its range a range) and the write log's keys. The write
log's objects, its values (unless the host reads them: `Job.logged_values`,
execute.goose's typed outcome) and the interned-key index are weak. The links
followed are the ones `snapshot::validate` checks, and a compacted heap
still validates as a snapshot (`serial_tests.goose` checks it after a
collection at every budget cut). Afterwards each margin is half of what is
left, or none once fewer than 4,096 rows are left, so a job whose live set
stays that near a limit ends `MemoryLimit` as before, and one whose live
set only came near it collects again once that set is dropped.

It follows decision 3's rules: rows stay plain `u32` IDs, nothing is freed
into a list (the tables are compacted, as evacuation compacts an image),
and the reset after the job is unchanged. Two details are Goose-shaped.
Shapes are a transition cache and are kept whole. The Latin-1 table is not
shrunk but keeps a spare tail (`Heap.latin1_spare`) that `add_latin1`
writes over: Goose's shrink check cannot follow `Job.heap` (a reference in
a root struct), so shrinking a byte array through it would be refused at
every caller of `run` that holds a byte view (argv, a directory name).

*Rejected.* *Evacuating into a successor program*, as this decision first
planned: it existed because tables could not shrink; reused workers
truncate them now, and a spawn would copy the whole image per collection.
*Mark and sweep with a free list*: forbidden (rule on free lists), and row
reuse out of order would break the earlier-row invariants (a proxy's
target). *A trigger counter or write barrier*: a cost on every allocation
or write, which the user's condition rules out.

*Measured* (November 7, 2026, the seed-42 200k project, `compare.bb
--phase check --runs 3` with master's runner in the same rounds): no job
reaches a margin (an instrumented 4-worker run's JOBS line ends with 0
collections over 5,283 jobs; the largest job adds 624 node rows against a
margin of about 50,000), and Goose wall times are within noise of master's
(medians, 1 worker: 6.98 s against 6.79 s, rounds ranging 6.69 to 7.54 s;
4 workers: 4.44 s against 4.41 s; two runs before the review fixes gave
7.42 and 7.34 s against 7.53 and 7.30 s, and 4.84 and 4.66 s against 4.80
and 4.73 s).

## State isolation: comparison with shadow-cljs

November 5, 2026 (light-cljs-792). Does one fresh job worker per job
(light-cljs-784) isolate more state than light-cljs needs? **Yes.** Every job
must start from the clean image, but nothing needs a new thread program per
job, which is what costs 5,283 sets of region setup a 200k session. This
section amends decision 3; light-cljs-795 implements it (-796 measured its
optional caches and declined them), -785, -787,
-788, -790 and -791 carry "Amended by light-cljs-792" notes, and -786, -789
and -794 (one image per thread program) stand as written. light-cljs-797
revisited it without byte-identical output across worker counts, which the
user does not require (last subsection).

### What shadow-cljs shares

Paths are under `research/shadow-cljs/src/main/shadow/` (3.5.5) unless named.

- **Macro namespaces load once per process.** `build/macros.clj`
  `load-macros` (86-126) requires each macro namespace under one global
  `require-lock` (84), only if it is not yet in the process-wide
  `active-macros-ref` (20): "since we can't isolate macros they will be
  reloaded for all builds" (13-19). Watch mode reloads a changed macro file
  once for every build (`cljs/devtools/server/reload_classpath.clj` 66-108).
- **Macro state survives everything.** Expansion calls the macro's var in
  the one heap of the Java virtual machine (`research/clojurescript/src/main/
  clojure/cljs/analyzer.cljc`, `macroexpand-1*`, 4293-4309), so
  macro-namespace vars, atoms and `defonce` values persist across calls,
  namespaces and builds. `gensym` is the process-wide counter `RT.nextID`
  (`research/clojure/src/clj/clojure/core.clj` 607-614,
  `src/jvm/clojure/lang/RT.java` 274 and 500); shadow's `reify` type names
  use it (`build/cljs_hacks.cljc` 784).
- **One compiler environment per build, shared by threads.**
  `with-compiler-env` binds one atom (`build/cljs_bridge.clj` 123-142);
  `par-compile-cljs-sources` (`build/compiler.clj` 1278-1361) gives one task
  per namespace to a pool of `availableProcessors` threads
  (`cljs/devtools/server/common.clj` 76-83), conveying that atom with
  `bound-fn` (1338); a task polls every 5 ms for its requires (1239-1262).
  Per namespace only thread-bound analyzer variables are rebound
  (`do-compile-cljs-resource`, 731-775: `*file-defs*`, warnings, and
  `*unchecked-if*` "so it is thread bound, since the analyzer may set! it").
- **Determinism is partial.** Outputs are reassembled by resource
  (1353-1360) and warnings kept per namespace, but nothing orders macro side
  effects or the gensym counter across threads. Namespaces that rely on macro
  side effects, and their direct dependents, skip the cache
  (`:cache-blockers`, default `clara.rules` and `clara.macros`: `build/api.clj`
  110-117, `build/compiler.clj` 933-942). Upstream ClojureScript's
  `:parallel-build` also shares one environment, with a global `load-mutex`
  (`cljs/analyzer.cljc` 639, `cljs/closure.clj` 1048-1100).

So shadow-cljs has no clean world per namespace, still less per macro call:
one world per process for macro state, one environment per build.

### What light-cljs requires

Rewritten by light-cljs-797. light-cljs-792 put "byte-identical output at
every worker and slot count" first, from `docs/compiler-concurrency.md`
("Semantics and determinism"); the user does not require it ("That isn't a
requirement from me", November 5, 2026).

1. **shadow-cljs's compile-time semantics.** Macro namespaces and their
   state (atoms, `defonce`, caches, gensym counters) persist for the build
   and are shared by every worker, as above. A macro whose result depends on
   shared mutable state and on scheduling may give scheduling-dependent
   output, as in shadow-cljs. Goose has no such state today: runtime
   `gensym` numbers in the compiler's generated-name scopes
   (light-cljs-798, below), the runtime has no atoms,
   writes into the realm fault, and so, since light-cljs-795, do writes into a
   prepared definition's rows (`goose_compiler_every_job_runs_in_its_own_worker`).
2. **Correct output.** Deterministic projects (the 200k project, the
   expanded fixture) still match V8 byte for byte, so a pure job's result,
   steps and limit outcome may not depend on which worker ran it or what ran
   there before.
3. **Root visibility as V8 had it** (light-cljs-776). The compiler resolves
   `__lightCompilerRoots[name]` at the job's visibility cursor and the job
   writes the answer into the roots object. A world reused across jobs may
   cache a helper by its unit key, never by name.
4. **Contained failures.** A VM fault, a limit or a cancellation ends one
   job and corrupts later calls no more than shadow-cljs would; a contained
   Goose abort ends the session (`supervise` in `src/compiler_session.goose`).

The minimum: **nothing a job allocates is reachable from a later job except
the compile-time state the build keeps**, and no fresh thread program per
job.

### What Goose's memory model requires

- No collector: garbage goes only when a scope ends. A 200k job leaves 98.7%
  of its nodes unreachable (median 5,841 rows; evidence, "One job worker per
  job"), so a world never reset grows without bound; the retained serial
  control's cumulative session work quota already failed at the fifteenth
  call of the heavier expanded workload (`evidence/expanded-results.md`).
- An index type names only a global array (P13), which never shrinks (P4,
  P14), so a nested Goose scope cannot free a job's rows; a thread program's
  exit can, and so can rewriting them in place later (P14). With plain row
  IDs (light-cljs-797) a table can be truncated to a mark.
- Every growable array a thread program reaches, global tables included, is
  a data-stack region mapped and unmapped at each spawn and exit (P12, P15);
  making the tables globals (light-cljs-785) keeps the region count.

A fresh thread program per job is a design choice. Decision 3 rejected
long-lived workers because "a reset is impossible without raw IDs", but a
reset needs only rewritten rows and an allocation point moved back.

### Options

200k costs: 5,283 dispatches a session (5,226 macro starts, 57 helper
productions; light-cljs-784's JOBS lines); per job from P16. These rows
compare ways to reclaim a job's garbage; light-cljs-797 compares where
compile-time state lives.

| Option | Persists across jobs | Determinism and failures | Spawns a session | Isolation cost a job |
| --- | --- | --- | --- | --- |
| (a) a fresh worker per job (784) | nothing | by construction; exit frees all | 5,283 | 361 µs at 1 worker (132 spawn to first instruction + 229 last publication to join), 2.7 ms at 8; P16 388-397 µs |
| (a') (a) with fewer regions (791) | nothing | as (a) | 5,283 | about 7.5 µs a region, and every table and job-local array is one (P15), so still over 100 µs a job for the tens of tables decision 2 adds |
| (b) a long-lived worker per slot, job tables in a nested scope | the image | needs the image rows restored | at most limit + retention | P16's truncated control: 6-24 µs; 792 rejected it because index types cannot name local tables (P13), but decision 2 now uses plain row IDs: light-cljs-797 adopts it |
| (c) a worker per namespace or batch | the image and the batch's garbage | state carried across a batch's jobs is neither shared, as in shadow-cljs, nor isolated, since the scheduler does not keep a namespace's jobs (deferred units, helper productions) in one worker; with a reset it is (d) with more spawns | about 500 (199,129 lines at the generator's 400 a namespace) | a tenth of (a), plus garbage that grows with the batch |
| **(d) a reused worker with a job arena** | the image | the image is restored before every job, so each job sees what a fresh worker would, at the same rows; a VM fault's state goes at the next reset; an abort ends the session as now | at most limit + retention (784 measured peak live workers 2/3/8/16), plus retirements | P16: 13 µs (the queue round trip), plus the prefix restore (about 17 µs for a 1.28 MB image); pages reused |
| (e) one shared world without reset (shadow-cljs) | everything | not expressible: every world is one thread program's copy (P17); per worker it is light-cljs-797's (b); garbage unbounded | at most limit | none, but no collector reclaims its garbage |

Row (a) is the 784 regression: +1.945/+1.419/+0.664/+1.366 s at 1/2/4/8
workers, system CPU 3.03 to 5.76 s at one and 4.88 to 22.14 s at eight. Under
(d) it projects to about 5,283 × 15 µs, 0.1 s a session, against 1.9 s.

### Decision

Decision 3 is amended: the thread program still owns an image and frees its
tables at exit, but a worker serves many jobs, and **a job is reclaimed by
resetting a job arena** (light-cljs-797 truncates the tables instead of
moving allocation tops, and links are plain row IDs):

- The coordinator, the image owner, spawns contained job workers on demand,
  at most `limit + retention` live, and gives each job to an idle one. Its VM
  tables do not change after `READY`, so every worker, whenever spawned,
  inherits the same image (P10, P11).
- A worker copies its image prefix (each table's rows up to the image
  length, and one worker-global VM state struct) into local pristine twins
  once. Before each job it restores both from the twins and sets every
  table's allocation top back to the image length; the job appends at the
  top, overwriting rows a previous job left (P14, P16), and within a job
  nothing is reused. Frames, operands, bindings and bridge buffers are
  locals of the per-job function and go when it returns.
- Limits count the rows a job added above its starting tops. A worker retires
  after a job once its tables exceed a byte cap; a cap of zero retires it
  after every job, which is (a), the differential control.
- This is reclamation by the job's scope, not a heap allocator: the top is a
  bump pointer reset when the job ends, like a Goose data stack; no free
  list, per-object reuse or sweep, and links stay index types. A job's rows
  start at the coordinator's image length in every worker, so its heap
  layout is a fresh worker's, and a saved continuation restores into any
  reset worker without rebasing (its checking conversion validates links
  against the tops).
- **What may persist:** the image only (the realm, the runtime leaves, and
  the roots and helper-cache objects in their initial state). Prepared
  definitions and installed helpers may stay loaded in a worker if they are
  keyed by definition or unit key, out of JavaScript's reach, and added
  between jobs; light-cljs-796 measured that it does not pay on the 200k
  project (below, "Measured (light-cljs-796)"), so today the image is all
  that persists. If a later workload makes it pay: extended prefixes
  differ between workers, so a cache hit charges the job the rows and work
  its installation recorded (limit outcomes match a cold install), and a
  saved continuation carries every row above the coordinator's image it
  reached, cached ones included, with its cache bindings, and resumes on a
  worker reset to the coordinator's image. Nothing keyed by root name, no
  write to image rows and no row a job added survives the job in a worker's
  tables; compile-time state survives it in cells (light-cljs-797).
- **Rules for the representation pellets** (light-cljs-785, -787, -788,
  -790): every piece of VM state a job can change is a table row, a field of
  the one worker-global state struct, or job-local; no lookup structure
  (interned keys, shape transitions) keeps a job's additions anywhere the
  prefix restore does not cover; the VM reads tops, not physical table
  lengths. In tests a reset check compares the prefix and the struct with the
  twins after every reset (light-cljs-795).

### Revisited without byte identity (light-cljs-797)

November 5, 2026, on the four requirements above. The 200k instrument phase
(`compare.bb --phase instrument --runs 1`, passing) at Goose 1: `JOBS 0 32
5283 5283 88601000 718709000 1368937000 155892823 153906415 2 1 529 0 529 0
0` (5,283 spawns, 136 µs spawn to first instruction and 259 µs last
publication to join each); CENSUS gives a median job 5,841 nodes over a
28-node image, 98.7% unreachable at exit. A probe runner counting each macro
job's writes by target row: 123,397 writes by 5,226 jobs, none into a row
the job did not allocate. The 200k project keeps no compile-time state, so
each option costs it only its garbage reclamation.

| Option | Persists across jobs | Garbage | Failures | Requirement 1 | 200k cost | 795 and 796 |
| --- | --- | --- | --- | --- | --- | --- |
| (a) 792's decision: reused workers, pristine restore | the image (and 796's caches) | job arena reset | a fault's writes go at the next reset | fails: no state survives, so an atom would be undone at every reset and a counter would return 1 forever | about 15 µs a job, at most limit + retention spawns (792's projection) against 5,283 above | as filed |
| (b) one persistent world per worker, as V8's isolates | what its jobs wrote, per worker | reset after promoting every row a persistent write reaches (an export and re-import, as for a result) or undoing the write, at every job end, faults included | a failed job's writes stay, as in shadow-cljs | exact at one worker; at N each worker has its own atoms, so a counter repeats values and a registry misses what other workers' jobs registered even when the compiler finished the dependency first (V8's light-cljs-429 contract, which needs `--cache-blocker` affinity) | as (a): nothing to promote | harder: retiring a worker discards its state unless its whole live world is evacuated, and a job's outcome depends on its worker's history by design |
| (c) one world shared by all workers | — | — | — | not expressible: every world is one thread program's copy (P17); a row ID in a message names a row of the receiver's own copy, and an index-typed one cannot be sent | — | — |
| (c1) one world in one thread program running every job | everything | as (b); the never-reset serial control failed its work quota at the fifteenth call of the expanded workload | as (b) | exact, value identity included | macro execution on one core; today's runner, which spawns per job, gives 9.52 s with one Goose worker against 8.15 s with four at 4 compiler slots and 10.36 against 9.31 s with eight at 8 (one check sample each), an observation, not a measurement of (c1) | moot |
| **(c2) state in process-wide cells outside every world** | the image and 796's caches in each worker; atoms, volatiles and delays reachable from published values, as flat values with process-wide object identities in one owner | job reset (795, by truncation); cells live for the session within a budget | a cell operation is atomic at the owner, so a failed job's completed swaps stay, as in shadow-cljs; nothing to undo | as shadow-cljs at every worker count: one cell per published atom, object identity kept across jobs, `swap!` reruns its function only when another write replaced the value it read, a delay realizes once | none: no cell operations | unchanged; 795 truncates and gains one write rule |

Cells cost P18: with a reply queue private to each worker, a `swap!` (a
load and a compare-and-set, two round trips) takes 9.6 to 9.8 µs with a
64-byte value and 29 to 34 µs with 64 KB at one worker; one owner serves
about 200,000 messages a second, so 8 workers on their own cells complete a
swap every 10 to 12 µs between them, and 8 workers on one cell rerun the
function 7 times a swap. With one reply queue routed by address, as the
`Answer` queue is, replies bounced 0.9 to 9.8 million times per 16,000 swaps.
Other shadow-cljs features need no shared world: Goose prepares each
definition and produces each helper once a session (macro namespaces load
once), `gensym` belongs to the compiler's generated-name scopes (unique in
the build, independent of scheduling), the cache's skipped effects are the
`--cache-blocker` rule, and the compiler environment is the Rust compiler's.

**Integer row IDs** (the user's decision, recorded in decision 2) reopen
792's option (b) (job tables in a nested scope, not this table's (b)),
rejected only because index types cannot name local tables. Separate job tables in a nested scope would split the ID space (an
ID below the image length names the worker's table, any other the job's),
a branch on every row access; one table per kind truncated to the image
length after each job frees the same rows without it. P16 measured that at
5.6-6.0, 10.7-12.9 and 20.9-24.2 µs a job (no rows, 6,000, 18,000) against
the arena's 13.0-13.3, 22.5-22.9 and 51-52. Truncation also drops the
arena's allocation tops: the VM reads lengths, and no stale row sits above
a top. JavaScript cannot write rows a job did not allocate (below), so the
only image rows a job changes are those compiler services write (the roots
object, the helper cache), restored from the twins; the reset needs no
write log.

**Decision: (c2), with jobs reclaimed by truncation.** (c1) is exact but
executes every macro on one core and, like (b), must promote and eventually
collect its world; (b) is wrong at more than one worker. (c2) keeps parallel
execution and gives up only mutable JavaScript objects as shared state,
which shadow-cljs's macros, running on the JVM, cannot have either.
Decision 3 changes: **truncating a reused worker's tables reclaims a job's
garbage, and compile-time state lives in cells outside every worker's
tables.** light-cljs-795 (amended) truncates and adds the write rule,
light-cljs-799 builds the cells, light-cljs-798 the runtime `gensym`.

- An atom, volatile or delay reachable from a published value (a prepared
  definition, a produced helper) becomes a cell when published, one cell per
  object, like the Pants build's one atom per Var
  (`docs/compiler-concurrency.md`, "Effect lanes"); a job's own atoms stay
  rows. A cell holds the flat encoding of a shareable value (forms,
  functions, cell references); a mutable JavaScript object fails with a
  named error, and so do watches and validators.
- Objects keep their identity across jobs: an object that leaves a job in a
  publication or a cell value gets a process-wide identity when it first
  leaves, encodings refer to it, and a job imports each identity once, so
  `identical?` answers what one shared heap would.
- Cell operations are effects. `deref` loads, `reset!` stores and
  `compare-and-set!` compares by `=` (cljs.core). `swap!` runs its function
  on the value it loaded and stores the result if the cell still holds that
  value's identity, rerunning only when another write replaced it, as
  Clojure's `Atom.swap` (`research/clojure/src/jvm/clojure/lang/Atom.java`),
  so a function that resets the cell to the value it read still ends. A
  delay realizes once a build: the first force claims it at the owner, other
  forces wait for its value, and a thunk that throws makes every force throw
  that error, as Clojure's `Delay`.
- The owner is a program with no VM tables (the coordinator or one it
  spawns), so no worker's image changes; replies go on a queue private to
  the worker's execution slot (a generic reply struct with one marker type
  per slot, P18). Cells and identities live for the session: one a job
  creates and later drops stays until the session ends, counted against a
  session budget that fails with a named error. The owner compacts its cell
  graph (tracing from publications and live jobs) only if a measured
  workload needs it, as decision 3 says of evacuation.
- A JavaScript-visible write into a row the job did not allocate (the realm,
  a restored definition, an installed helper) faults with `NativeBoundary`
  before it mutates, as writes into the realm do today, instead of being
  undone silently by the reset. No 200k job makes one.
- Unchanged: 795's reuse and retirement, the conditions on 796's caches
  should they ever be built, among them cold and warm equality
  (requirement 2), root reads at the cursor, `gensym` as the
  compiler's (the runtime leaf numbers in the expansion's generated-name
  scope, as V8's native mode does). Cell effects follow scheduling; the
  owner is where effect lanes (light-cljs-480) could order them if that is
  ever required.

**Implemented (light-cljs-795, `src/compiler_session.goose`,
`src/snapshot.goose`).** Measurements are in
[the evidence](../evidence/generated-200k-performance.md), section "Reused
job workers".

- *The image.* light-cljs-785's coordinator-global tables did not come, so a
  worker inherits the image as the base snapshot its spawn copies (the same
  flat `snapshot::Snapshot` every worker gets; the coordinator never changes
  its tables after `READY`, which checked runs assert by digest). The worker
  restores it once and keeps the spawn argument as its pristine twin.
- *A job* arrives on the one `Work` queue every idle worker takes from; the
  coordinator spawns a worker only when none is idle and at most `limit +
  retention` are live. `snapshot::extend` validates the job's snapshot (the
  prepared definition, the base for a helper production, or a saved
  continuation) as `restore` does and appends only its rows above the image,
  copying the service rows (the roots object, the helper cache and their
  image key runs) a continuation's compiler services wrote. The tables equal
  a fresh restore's row for row, with the job's image tops at its lengths, so
  limits count what the job adds, as before.
- *The reset* (`snapshot::truncate`) cuts every table, code tables included,
  to the twin's length and restores the service rows and the heap's state
  fields from the twin: about 0.3 microseconds a job on the 200k project,
  whose image is 28 nodes. Frames, operands and bridge buffers are locals of
  `serve`. In the test modes and with `BBP_RESET_CHECK=1` (compiler.bb sets
  it) every reset is compared with the twin byte for byte
  (`snapshot::pristine`) and every installed snapshot's prefix with the image
  (`snapshot::over_image`); a difference aborts the session.
- *The write rule.* `Job.owned_nodes` (the node table's length when the job
  started) and `Job.foreign` (node ranges installed from publications during
  the job) bound the rows JavaScript may write: a property, element or
  closure-cell store into any other row faults `NativeBoundary` before it
  mutates (`source::writable`). A root read whose receiver is neither the
  compiler roots object nor such a row faults the same way, since the
  compiler's answer is written into its receiver. A continuation carries
  both bounds.
- *Retirement.* A worker whose tables end a job above
  `BBP_WORKER_TABLE_BYTES` (default 16 MiB) exits instead of resetting; 0
  retires after every job, light-cljs-784's one worker per job, the
  differential control. A reused worker keeps its data stacks' high water, so
  the cap is what bounds idle workers' memory. light-cljs-795 chose 1.25 MiB,
  which retired a quarter of the workers' jobs on the 200k project and kept
  the runner's peak memory within 5% of the control; the user then chose
  16 MiB (light-cljs-791), which no 200k job reaches: a session spawns one
  worker per execution slot and retires none, Goose 1/2/4/8 take 0.36 to
  0.85 s less, and the runner holds up to a fifth more peak memory (6 MB at
  one worker).
- *Unchanged:* blocking waits within the retention budget, the `Answer`
  routing by job address, cancellation, the input and readiness protocol,
  `supervise` (a contained abort still ends the session), root reads in every
  job. The `JOBS` line adds jobs run, workers retired and reset nanoseconds.

**Measured (light-cljs-796, November 5, 2026): no per-worker definition or
helper cache.** The cache would let a reused worker keep the prepared
definitions it restored and the helpers it installed, so a later job skips
`snapshot::extend` or `callable::decode_install`. light-cljs-796 set the bar
before building it: if the two together take under 3% of Goose wall at one
worker on the 200k check gate, record the figures and leave the runtime
alone. The `JOBS` line gained four fields for this (the workers' snapshot
installs above the image, their helper installs and their count, and the
coordinator's time copying a prepared definition into a job and queueing
it). On the 200k project, traced sessions at the default cap (evidence,
"Definition and helper installs in reused workers"):

| Workers | Snapshot installs (5,283) | Helper installs (529) | Coordinator hand-off | Share of Goose wall |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 78.0 ms (14.8 µs each) | 3.8 ms (7.3 µs each) | 43.9 ms | 0.52% (0.80% with the hand-off) |
| 8 | 110.5 ms across the workers | 4.5 ms | 73.2 ms | hand-off 0.94%, on the coordinator |

Together they are 0.08 s of 15.8 s at one worker, under a fifth of the 3% bar
(0.46 s), so the cache is not built. A perfect cache saves at most the two
worker-side columns: the hand-off stays unless jobs are routed to the worker
that holds their definition, which the one `Work` queue deliberately does
not do. Against that saving the cache costs the conditions above (limit
charges recorded at the first install, saved continuations carrying cached
rows and bindings between workers, cold and warm equality tests) and makes a
worker's tables depend on its history. The larger per-job cost left is the
queue-to-first-instruction latency, mostly the spawns of retired workers'
successors (0.31 s summed at one worker, 0.93 s at eight; their joins, 0.34 s
and 2.46 s, are off the critical path), which is light-cljs-791's; it
removed them by raising the default cap to 16 MiB ("Stack settings").
light-cljs-792's premise of 155.9 MB of spawn arguments and 153.9 MB of
restores a session was a worker per job; with 795's reuse they are 131.8 MB
(1,282 base images and the jobs' snapshots) and 102.6 MB (only rows above
the image). Revisit only if a workload's definitions are much larger than
the 200k project's (19 KB of restore a job).

**Implemented, first slice (light-cljs-799, `src/cells.goose`,
`src/cell_owner.goose`).** Atoms and volatiles; delays, functions as cell
values and the identity of published values are the follow-up pellets'.

- *Rows.* `atom`, `deref`, `reset!`, `reset-vals!`, `swap!`, `swap-vals!`,
  `compare-and-set!`, `volatile!` and `vreset!` are `rt.core` leaves
  (Builtin variants, decision 5), and so are `derefProtocol` and
  `vresetProtocol`, which `vswap!` expands to. A job's own atom or volatile
  is a node of kind 17 (state in `value`, flavor in `length`) and follows
  the V8 build's runtime: `swap!` calls its function once and `swap-vals!`
  pairs the state it replaced at the store. `swap!` and `swap-vals!` are a
  native frame (`Native.Swap`) that calls the function with a null `this`,
  so it may be a closure that suspends or a native-frame built-in (`swap!`
  itself). `compare-and-set!` on a cell compares before it shares its
  replacement. A volatile cell's `vswap!` reads and then writes, so two
  jobs at once may lose an increment, as in Clojure. Watches, validators
  and atom options fail by name.
- *Publication.* The preparer and a helper production call
  `source::publish_cells` before they encode: every atom the published value
  reaches through any link becomes a cell, its row a reference of kind 18
  (the cell in `function`), which `callable::graph` and the snapshots accept
  and GFB2 carries as tag 10 inside the session only (`bridge::Control.cells`;
  a job's result for the compiler refuses one, as the V8 build's form bridge
  refuses an atom).
- *Values.* A cell holds a primitive or a form graph: forms, cell references
  and atoms, which become cells first. Anything else fails the operation by
  name before it changes anything (`cannot share a mutable JavaScript
  object`, `a function (functions are not shareable yet)`). A stored graph
  loses its call's compiler occurrences and spans: a later job sees it as a
  value built afresh.
- *Identity.* The owner numbers a graph the first time it is stored; the job
  records every row it loaded or stored with its cell or identity
  (`Job.cell_rows`, saved with a continuation) and imports each once, so a
  value stored back keeps its identity and two loads are one object.
  `swap!` on a cell compare-and-sets against the identity of the value it
  read (for a primitive, the same primitive), so a function that resets the
  cell to that value still ends without a rerun. Only the root of a stored
  graph has an identity in this slice (every object has one since
  light-cljs-808, below).
- *Owner.* `cell_owner` is a thread program the coordinator spawns before
  any other, with no VM tables, holding each cell's encoding in a
  `slice_pool<u8>`. A requester takes one of eight slot tokens and receives
  on `Reply<S0>`..`Reply<S7>`; a cancelled or poisoned session posts `Halt`,
  which releases every waiter. Cells and identities count against
  `BBP_CELL_BUDGET` (default 1,048,576) and their bytes against 256 MiB;
  past either an operation fails `cell budget`. The THREADS line counts the
  owner, and the JOBS line ends with its cells, identities, operations,
  refused compare-and-sets, delay waits and serving nanoseconds.

**Implemented, second slice (light-cljs-808): shared delays and the
identity of published values.** Functions as cell values and the cell
census remain light-cljs-809's.

- *Delays.* `rt.core.Delay` (what the compiler emits for `(delay ...)`:
  `new rt.core.Delay(thunk, null)`; Construct, opcode 94, now runs for this
  one built-in constructor and still fails `Unsupported` for any other
  callee), `force`, `realized` and `isDelay` are leaves, and `deref` and
  `derefProtocol` of a delay force it. A job's own delay is a node of kind
  19 (thunk in `value`, the realized value in `namespace_value`) and
  follows the V8 build's `deferredValue`: a thunk that throws stays, and a
  later force calls it again. A force is a native frame (`Native.Force`),
  so a thunk that suspends on a helper resumes in any worker.
- *Delay cells.* Publication makes a delay a cell of flavor 2 whose
  reference keeps the thunk (a value that reaches a pending delay is
  published as a callable publication, the transport that carries
  functions; a delay realized before publication is stored realized). The
  first force claims it at the owner (op 5) and runs the thunk in its job
  under a catch handler of the force frame; the value, or what the thunk
  threw (a realm error keeps its type, fault code and message, and has an
  identity, so repeated forces in a job throw one object), settles the
  claim (op 6), and every later force takes it, as Clojure's `Delay`. A
  delay forced as a built-in's callback (`mapv`, `swap!`, an array
  callback) runs in its own force frame too. A
  force while another job holds the claim gets the claim's epoch and waits
  as a dependency wait does (event 11, a `WAIT id cell` line; within the
  retention budget in place, otherwise as a saved continuation), and the
  owner posts `Settled` when a claim with waiters ends, which the
  coordinator turns into a resume (Answer or Work mode 5) that forces again.
  A job that ends holding claims (a fault its thunk could not catch, a
  cancellation) releases them (op 7), and so does the coordinator for a
  saved continuation it refuses (retained or waiter budget), so the waiters
  force again and one claims. A delay reference that reaches a job only
  through a cell value has no thunk unless the job also installed its
  publication; forcing it while pending and unclaimed fails by name. The owner follows wait edges from the claim's holder: a force that
  would close a cycle of claims fails `deref: a cycle of delay claims`
  (uncatchable, so the job ends and releases its claims; in a two-job cycle
  both jobs fail). The holder is the macro job's compiler id; a preparation
  or a helper production claims nothing and fails by name on a pending
  delay cell. The JOBS line's delay waits count the forces that waited.
- *Identity.* Every object of a stored graph has an identity now (GFB node
  identities travel beside the bytes, numbered by the owner on first store,
  -1 for primitives, and a cell reference's derived from its cell, so a
  compare-and-set of an atom holding an atom compares cells), and the
  objects of a publication
  have one from the moment they leave their job, with no owner message:
  `publication_identity(publication, place)`, the publication a helper's
  unit or a definition's key and the place the object's row offset in it
  (the rows a form helper's graph, a callable install or a definition
  snapshot installs keep their order in every job). `Job.cell_rows` holds a
  publication as one range entry. A publication stores its cells' initial
  states only once it knows those places (`store_initial`), so an object
  that is both in the publication and in a cell's initial content has one
  identity. Every path that installs rows
  (`snapshot::extend` of a definition, `callable::decode_install` through
  `published`, a form helper's import, a cell value's import) registers
  its cell references and identities and maps an object or cell the job
  held before to the row it holds, rewriting the new rows' links (a held
  delay reference without a thunk takes the installed one's). So a
  published vector stored in an atom is `identical?` to the helper's value
  in a later job on a fresh worker, whichever it reads first, and so is the
  same vector inside a stored vector; a cell captured by a callable helper
  and the same cell inside a cell value are one object in a job.

**Implemented, third slice (light-cljs-809): functions in cells, and the
cell census.**

- *Functions.* A value that reaches a function or a job's own delay goes
  by the callable transport (`cells.goose` `encode_callable`, cell value
  kind 7): `callable::encode_cell` publishes it as `callable::encode`
  publishes a helper, against the session's image header (`cell_base`,
  which the coordinator sets before it spawns any program), from the job's
  heap while the storing built-in runs; its atoms and delays become cells
  first, a delay's reference keeping its thunk. Each installed slot has an
  identity (a cell reference its cell's, a publication's object its place,
  otherwise the owner numbers it). A loading job installs it as
  `callable::decode_install` installs a helper, once per identity, with
  the rows it holds already mapped (`decode_callable`, as
  `install_publication` maps a publication's) and the new rows foreign to
  its writes. A function of the image (a built-in such as `inc`) is its
  row (kind 8). The owner compares the two graph kinds by identity, so
  `swap!` of a function value stores without a rerun. Before anything
  changes, a store fails by name for a mutable JavaScript object, a
  function with own properties and a function capturing a variable it may
  assign (`callable_unshareable`, callable::graph's rules). A definition's
  or helper's atom may start out holding a function (`store_initial`).
  Once stored, the job's closures are foreign to its own writes too
  (`protect`), so no job sees a function its cell's encoding does not
  hold; captured forms lose their call's occurrences and spans, as a form
  value does; a delay reference that keeps its thunk travels by this
  transport, so a job's pending delay stored in a cell forces in any job.
- *Code while a job runs.* Loading a function appends its code, so the
  dispatch loop takes its instruction view per frame activation, not per
  run, and `source::decode` only grows the instruction tables. A load
  outside a macro expansion (a preparation, a helper production) fails by
  name: only a macro job holds its code writable (`Job.cell_code`).
- *Limits.* The publication is the whole job heap's tables above the
  image, as a helper's is, so storing a function copies the job's source
  text, strings and code above the image, and each first load in a job
  installs that code again. A function with own properties (light-cljs-805)
  cannot be stored, as it cannot be published as a callable helper.
- *Encoding cost.* A form value's export (`bridge::export_rows` with
  `spans` off) no longer copies the job's source text and spans, which the
  cell encoding dropped anyway. Measured by `goose_compiler_cell_swap_cost`
  (evidence, "Cell census and swap cost").


### Runtime gensym in the compiler's scopes (light-cljs-798, November 5, 2026)

`rt.core.gensym` on the Goose path gives the name V8's native mode gives:
`str(prefix)` (or `G__`), the scope's prefix `D_` and the next number of the
count of the generated-name scope the macro expands in
(`src/generated_names.rs`). `GooseEvaluator::gensym_scope` records the scope
`sync_gensym_scope` puts it in; each job takes the scope's prefix and next
number with it (`{id}.definition` holds `key prefix next`), numbers in its
own thread program, and gives the count back on its `DONE` line, which
moves the compiler's count past the numbers it took (`GensymWindow`), so a
core macro expanded natively in the scope numbers on after them. The count
travels with every wait: in the blocked-wait event and its answer, and in a
saved continuation. A helper the job demands is produced in the job's
scope (its initializer's `gensym` numbers on from the job's count), and so
is the preparation of the job's definition. Nothing else numbers in the
scope while its macro runs, since the analyzing thread waits for the job
and a demanded helper is analyzed in a scope of its own; closing the window
refuses a count that moved meanwhile, and refuses to advance while the
compilation logs names for artifact capture (the Goose path requires
`--no-cache`). Outside every scope, and for a data reader's runtime scope
(`native` false), `gensym` stays an explicit `NativeBoundary` fault, as do
two arguments, a prefix that is not a string, symbol or keyword (a string
read from a form is its scalar occurrence, unwrapped), and a name with a
slash.

The other option was one compiler effect per `gensym`, answered as a root
read is. Measured with one worker on a release build, 20 jobs each: an
empty macro job took 595 µs, one making 200 gensyms 717 µs (0.6 µs a
name), one making 200 root reads the compiler answers from produced
helpers 74,656 µs (370 µs a round trip). Numbering in the job costs one
line field per job; the effect would cost a round trip per name.

Tests (`src/goose/compiler_tests.rs`): `goose_compiler_gensym_numbers_in_compiler_scopes`
(eight namespaces of deferred units, two expansions a unit, a helper
function that gensyms: the V8 build's JavaScript at slots 1/2/4/8),
`goose_compiler_gensym_helper_production_numbers_in_its_own_scope`
(waits in place and saved continuations; since light-cljs-802 a helper is
produced in its own frozen initializer scope, not the demander's) and the scope case of
`goose_compiler_assembly_leaf_boundaries`. Since light-cljs-807 a compiled
`def` initializer that calls `gensym` runs on Goose:
`goose_compiler_helper_initializer_gensym_names_alike_at_every_slot_count`
compiles such a helper namespace, used from eight namespaces, to the V8
build's JavaScript at slots 1/2/4/8 and under the scheduler's stress seeds.

## Decision 4: control flow

**Decision.** Everything the interpreter suspends or unwinds is data in the
job worker, and the dispatch loop is one non-recursive function (rule 5):

- **Frames** (function, pc, local base, operand base, receiver, `new.target`,
  construct flag) live on job stacks. Native built-ins that run JavaScript
  push a native frame with explicit state and return to the loop (decision 5).
  The call depth limit is a VM limit, not the native stack: 10,000 frames and
  1,048,576 values, the Pants engine's `VmLimits::DEFAULT`
  (`rust/src/vm/frames.rs`), failing as that engine does.
- **Exceptions and finally.** The handler stack holds catch and finally
  entries (frame, pc, operand height). `finally` follows the Pants opcodes:
  `PushFinally`, `PopFinally`, `EndFinally`, `CallFinally`,
  `DropCompletion`, `PendingThrow`, `PendingException`, with completion
  records as data. Unwinding crosses native frames, which clean up their own
  state.
- **Generators and async functions.** A yield or await detaches the
  activation: its frames, locals, operands and handlers are copied into a
  continuation record (`conts` plus a slot run); resumption copies them back.
  This is the Pants engine's "movable VM activation" (`vm/generator.rs`,
  `DetachedActivation`), and today's helper-suspension continuation
  generalised. The record is flat and index-linked, so it moves with the
  image.
- **Promise jobs.** A FIFO table of reactions in the job worker, drained when
  the frame stack empties. A job completes when its entry returns and the
  queue is empty; an entry that returns a promise completes when that promise
  settles.
- **Dependency suspension.** A job that needs a compiler dependency saves its
  continuation in place and blocks on its queue; its state stays in its own
  thread program. Over the retention budget it writes a continuation image
  instead (decision 3). Message routing reuses the existing re-queue pattern
  (`compiler_session.goose`, `cancelled`).
- **Cancellation** arrives at poll points (budget slices) and unwinds as an
  abrupt completion that `catch` cannot intercept but that runs `finally`
  blocks and `using` disposal, the Pants `PendingCancel` contract; then the
  worker exits.

**As implemented (light-cljs-786, November 5, 2026).** `source_vm.goose`
keeps catch and finally handlers and pending completions (Normal, Return,
Throw, Cancel, Jump) as one handler stack of plain `Handler` rows, and runs
the seven finally opcodes and `PendingCancel` by the Pants contracts; a
`return` runs its frame's finally blocks first. TypeError, ReferenceError,
HostEffect and StackLimit faults are catchable, raised as a minimal error
object (node kind 13 labelled with its fault; since light-cljs-788 the
realm's error object of its type, with a `message`, decision 6), and an
uncaught one reports its fault again. The limits are 10,000 frames and
1,048,576 values (locals and operands together). `cancel()` delivers a
cancellation between runs and the compiler session's poll point uses it.
Native frames are frames of function -1 whose `Native` enum names the
operation (`Mapv`, `MapSeq`, and `ToPrimitive`, the resumable
OrdinaryToPrimitive of `+` on an ordinary object); the delivery protocol is
documented at `Native`. Not yet: generators and async functions, promise
jobs, and catching natives (none needs one yet).

**As implemented (light-cljs-790, November 6, 2026).** `continuations.goose`
holds the mechanism; its header is the contract. The continuation table
`heap.conts` is one table of `Cont` rows, an enum of four flat records
linked by plain `u32` row IDs: `Activation` (a detached frame row, its
locals and operands in a run of `heap.values`, its handlers in a run of
`Saved` rows with heights relative to the frame's bases), `Saved`,
`Promise` (state, result, its pending reactions' first and last rows and
an async function's activation) and `Reaction`. Generator objects (node
kind 20) and promises (kind 21) are ordinary objects whose `function`
names their row. GeneratorInitialYield, GeneratorYield and Await *detach*
the top frame into its activation row, reusing the row and its runs when
they fit (a run that no longer fits is replaced by one twice as large), so
a long generator or await loop adds no value rows per step; next, return
and throw push a `Resume` native frame that *attaches* the activation and
receives what it yields or returns, and the bytecode's own paths run
return and throw. An async function's completion boundary is a handler of
kind `Async` holding its promise, so an exception that reaches it rejects
the promise, a return resolves it, and both deliver the promise. The
promise job queue is the job's FIFO (`Job.queue`), drained one task a step
when the frame stack is empty after the entry finished; resolving a
promise with a promise takes the specification's extra job, so job order
is QuickJS's (bbp/async_await_order). A job ends when the queue is empty:
`Fault.UnhandledRejection` if a promise was rejected with no reaction ever
registered (as QuickJS and the Pants engine report), otherwise the entry's
value or, for an entry that returned a promise, its value, its reason
thrown, or `Fault.Unsettled` if it is still pending. A saved continuation
carries the activations in its snapshot (`snapshot.goose` validates every
`Cont` link) and the queue in `Saved`; the worker reset truncates
`heap.conts` with every table, so nothing a job left suspended reaches the
next job. Not yet (the generators area, light-cljs-768): `yield*`, async
generators, the Promise constructor and methods, thenable objects (a named
UnsupportedOperand), %GeneratorFunction.prototype% (generator functions
inherit Function.prototype).

**As implemented (light-cljs-813, November 6, 2026).** An `Activation` is
a run of frames, so one record serves a generator, an async function and
the compiler session's helper-suspension continuation. Its rows: a run of
`Frame` rows (a fifth `Cont` variant holding a frame's fields, native
frames' `Native` state, input, output and index included, with bases
relative to the run's), its locals and operands in a run of `heap.values`,
and its handlers as `Saved` rows that name their frame in the run. A yield
or await detaches a run of one frame; a dependency wait that saves its
continuation detaches the job's whole stack (`suspend_stack`) into one
row named by `Saved.stack` in `compiler_session.goose`, which replaces
`Saved`'s copies of the job's frames, locals, operands and handlers. The
restoring worker attaches it at the heights it left (`resume_stack`), and
the job's next save reuses its rows. A save charges no work (it is the
session's step), but its rows count against the job's limits: a save past
them ends the job with that fault at its wait. `snapshot.goose`
(`activation_valid`) checks every activation's runs, frame order and
bases, bytecode frames' code, pc and locals, native frames' operation and
handlers' frames and heights.

**Rejected.** *Goose recursion per call with `exception::try` for throw*:
the native stack is 8 MB on the macOS main thread, a handler cannot be
re-entered recursively, and suspension would need the native stack saved.
*Running generators in separate worker programs*: a message round trip per
`next()` and a heap split across programs.

## Decision 5: built-ins

**Measurement.** Array map built both ways on the probe interpreter (P7),
over the real Rust-frontend bytecode of this function, with a callback
`(x, i, o) => x * 2`:

```js
function map(a, f) {
  const n = a.length; const out = [];
  for (let i = 0; i < n; i++) out[i] = f(a[i], i, a);
  return out;
}
```

| Engine, map variant | 100 elements | 1,000 elements |
| --- | ---: | ---: |
| Probe, map in JavaScript on Goose | 47.1 ns | 43.2–43.7 ns |
| Probe, map as a Goose native frame | 20.3 ns | 17.0–17.4 ns |
| Today's source VM, map in JavaScript | 1,072.8 ns | not run (quadratic) |
| V8 (node 24), map in JavaScript | 8.64 ns | 3.35 ns |
| V8 (node 24), `Array.prototype.map` | 6.19 ns | 5.58 ns |

Nanoseconds an element, each run 100 rounds of map; the best run of three
(Goose at 100 elements) or five (Goose at 1,000, V8) in one process, the
1,000-element probe rows giving the range over three processes. Goose rows
are native release builds, the probe with one job worker per run; the load
average was about 1.5 on an otherwise idle machine, so these are diagnostic
figures, not a quiet measurement. Both probe variants return checksum 199,800 (1,000) and
19,800 (100). The native frame calls the same JavaScript callback through the
same dispatch loop. Writing map in JavaScript costs 2.5 times the native
frame, because its loop runs about 24 interpreted instructions an element
around the callback.

**Decision.** Built-ins are written in Goose. Each built-in is a variant of
a `Builtin` enum dispatched by `match` or case functions (rule 4); a built-in
function object is an object whose kind payload is its `Builtin`, with
`name` and `length` installed by the realm builder. The rules areas follow:

1. A built-in that can run user code (callbacks, getters, `valueOf` and
   `toString`, Proxy traps, iterator protocol, species constructors) is a
   resumable native frame with explicit state, re-entering the dispatch loop.
   It never calls the interpreter recursively.
2. A built-in that cannot run user code is a plain Goose function over the
   tables.
3. Port semantics from the Pants engine's native built-ins
   (`rust/src/vm/native.rs` and `vm/native/`), whose `NativeState` enum
   already is this resumable design, and cite the QuickJS case that tests it.
4. A step a built-in does not implement yet (species, a Proxy trap, a
   locale) is a named unsupported VM fault, never a placeholder `TypeError`
   that user code could catch (decision 1).
5. Self-hosting a built-in in JavaScript is an exception an area must justify
   with a measurement and a reason (for example a large, cold algorithm); it
   needs the private intrinsic opcodes that self-hosted code would require,
   which the exporter does not have today.

**Template (light-cljs-789).** `Array.prototype.map` and `forEach` are the
reference for rule 1: `src/array_callbacks.goose`, with
`array_callbacks_step` as the step function. An area adding a built-in that
runs user code follows it:

- a `Builtin` variant appended to the enum and `BUILTINS` (codes are
  positions, so existing codes never move), installed by a `REALM_FUNCTIONS`
  entry, its pending name removed from `PENDING`;
- `native_builtin` names it and `native_start` pushes its frame from the
  dispatch loop's `call`, after `Function.prototype.call` has been unwrapped;
  `call_builtin` gives it a named unsupported step, so a caller that runs
  built-ins to completion (ToPrimitive, another native frame's callback)
  cannot run it by mistake;
- a `Native` variant carrying its `Builtin`, its state in the frame's fields
  (`pc` the phase awaiting a result, `index` the position, `input`, `output`,
  `callee`, `receiver`, and `argument_count` for a length), and a `match` arm
  in `native_step`;
- each phase that runs JavaScript enters the function with `push_frame` (a
  getter, the callback) or pushes another native frame (`to_primitive` with
  operator 8, `v - 0`, is ToNumber of an ordinary object), sets the phase that
  consumes the result and returns; a step visits at most one element, holes
  included, so every budget cut, limit and suspension falls between
  callbacks (serial_tests.goose checks every cut of
  `corpus/serial/array-callbacks.js`);
- a fast path reads what a general step would read without user code (an own
  data element of a dense array) and re-reads it every visit; anything else
  takes the specification's steps.

Measured on the ported map (native release build of `execute.goose`, one
map of the benchmark's array a job, best of five, array setup subtracted):
`Array.prototype.map` 65 to 66 nanoseconds an element at 100 and 1,000
elements, `forEach` 79 to 91, the same map written in JavaScript 111 to 112.
The ported map is 1.7 times faster than the JavaScript one but 3.8 times the
probe's 17.2: the probe's interpreter had none of the general VM's per-call
work. Each element costs a full call and return of the callback in the
dispatch loop (about 20 nanoseconds, light-cljs-801's figure, plus about 2.4
an instruction), and, because a native caller ends the loop's in-place run,
a return through `run` to `native_step`, with its step accounting and frame
row copies. Keeping a callback's return in the loop when its caller is a
native frame is the next cut.

**Rejected.** *Self-hosting by default*: 2.5 times slower on the measured
case; it needs private intrinsics hidden from ordinary code, a second compile
path for the realm, and `Function.prototype.toString` and stack traces that
pretend the code is native; and it would rewrite semantics that the Pants
engine already specifies as resumable native code.

## Decision 6: realm setup

**Decision.** The realm builder is Goose code that runs once, in the session
worker, before any job: the global object, `Object.prototype`,
`Function.prototype`, the Error constructors, `Array.prototype`, the
well-known symbols and every intrinsic the areas add, as the first rows of
the tables. Job workers inherit it through the spawn copy (decision 3, probe P10), so
"build once and snapshot" costs one table copy a job, consistent with today's
initialized-snapshot design. Large immutable data (Unicode and Intl tables)
is static `let` data rather than table rows, so it is never part of the
mutable image; whether static data is shared rather than copied at spawn is
unverified, and the realm pellet measures it. The realm pellet records the
realm's rows, bytes and build time, and the spawn cost they add.

**Rejected.** *Building the realm lazily per job*, as the Pants engine does
on first use: every job would pay for it. *Building it in JavaScript*:
decision 5.

**As landed (light-cljs-788, `src/realm.goose`).** `source::initialize`
builds the realm into the empty heap, after the code strings and constants,
so only the program that initializes a heap builds one: the compiler
session's coordinator, before `READY`, and the conformance and test runners.
Job workers restore the coordinator's image (`snapshot::restore`) and never
build it. The layout comes from fixed-size static tables (`BUILTINS`,
`REALM_FUNCTIONS`, `PENDING`, `RUNTIME_LEAVES`), so the realm's node rows are
fixed and `snapshot::validate` checks them (`source::realm_valid`): row 0 the
global object, 1 `Object.prototype` (null prototype), 2
`Function.prototype` (a built-in that returns undefined), 3
`Array.prototype` (an array of length 0), 4 to 11 `Error.prototype` and the
prototypes of `EvalError`, `RangeError`, `ReferenceError`, `SyntaxError`,
`TypeError`, `URIError` and QuickJS's `InternalError` (the stack overflow
error), 12 to 22 their constructors and `Object`, `Function` and `Array`,
then one built-in function per method: `Function.prototype.call`,
`Object.getPrototypeOf`, `Object.is`, `Object.hasOwn`,
`Object.prototype.hasOwnProperty`, `valueOf` and `toString`,
`Array.isArray`, `Error.prototype.toString` and `Symbol`, whose
non-writable, non-configurable properties are the fifteen well-known symbols
(`Value.Symbol`, the exporter's fixed indices, now admitted as values;
calling `Symbol` is unsupported). Each built-in, the runtime leaves too, has
`length` and `name` (configurable only), a constructor its `prototype` (no
attributes) and the prototype its `constructor`; the global object has the
constructors, `globalThis`, `NaN`, `Infinity` and `undefined`.

- *Built-ins (decision 5).* A built-in function is a node of kind 4 whose
  `function` field is its code, the position of its `Builtin` variant in
  `BUILTINS`; `call_builtin` dispatches on the variant with one `match`, the
  same path for a call, a method call, `Function.prototype.call`, a native
  frame's callback (`builtin_callback`) and ToPrimitive's methods. The
  light-cljs runtime leaves (`rt.core.inc`, `mapv`, `gensym` and the rest)
  are variants 0 to 20 of the same enum at their old codes, installed by
  `boot_runtime` from `RUNTIME_LEAVES` once in the coordinator, with the
  same identities and behaviour. Construct (`new`) is still the
  `Unsupported:construct` opcode for every callee.
- *Prototypes.* A node whose `proto` is `Empty` has its kind's realm
  prototype (`source::prototype`): ordinary objects and arguments objects
  `Object.prototype`, arrays `Array.prototype`, closures and built-ins
  `Function.prototype`, error objects `Error.prototype`; literals therefore
  get the realm's prototypes without a write. A heap built without
  `initialize` (hand-made test heaps) has none.
- *Pending properties.* A name the specification puts on a realm object and
  this VM does not implement yet (`Array.prototype.map`, `Object.keys`,
  `Math`, `JSON`, `Symbol.for`, `Function.prototype.bind`, `@@iterator`:
  126 entries of `PENDING`) is an accessor property whose slot names accessor row 0,
  the realm's pending pair. `in` and `hasOwnProperty` see it; reading,
  assigning or defining through it fails `UnsupportedOperand` (decision 1),
  so a case that needs it is explicit-unsupported, not wrong. An area
  replaces its entries by built-ins. A built-in step not implemented (a
  primitive wrapper, a `@@toStringTag` getter, dynamic `Function`) fails
  `UnsupportedOperand` and names the built-in in `Job.detail`, as a pending
  property names its key (`Heap.pending`); `describe_fault` appends the
  name to the fault, so compiler diagnostics carry it
  (`Fault.UnsupportedOperand:pending-map`), and the conformance runner
  prints it.
- *Errors.* A catchable VM fault (TypeError, ReferenceError, the new
  RangeError of `Array(-1)`, HostEffect, a stack overflow) raises an error
  object (kind 13, an ordinary object for properties) with its type's
  prototype and an own `message`: the faulting operation's (`x is not
  defined`, a built-in's message) or the QuickJS wording for the
  instruction (`not a function`, `stack overflow`). Uncaught, it still
  reports its fault; an error JavaScript made (`TypeError("m")`, called as a
  function) is a thrown value. `typeof` (opcode 78) is implemented here.
- *Script declarations and global names* (light-cljs-824). Every global
  opcode runs (`Ins.GlobalOp`, `global_op` in `source_vm.goose`), with the
  Pants engine's semantics (`vm/dispatch.rs` global_step, `realm.rs`
  declare_global_with and check_global_declaration). The object record is
  the global object, read and written on every access, so accessors (a
  getter as a frame, a setter in a `Setter` native frame) and later
  redefinitions are seen. The declarative record is `Job.lexicals`: a
  dictionary-mode node the job allocates, one record per script `let`,
  `const` or `class` name whose value is the binding's cell and whose
  writable attribute marks `let`; a job state field, so the reset drops it,
  and a suspended compiler job's continuation keeps its row
  (`compiler_session.goose` Saved). `GetGlobal` reads it first (an
  uninitialized binding is a ReferenceError, also under `typeof`), then the
  global object. The declaration checks are a catchable SyntaxError
  (`Fault.SyntaxError`, new here): a var or function name a lexical holds,
  a lexical over a non-configurable global property, a function over a
  non-configurable property that is not a writable enumerable data
  property; every global object is extensible until
  `Object.preventExtensions` exists. The compiler emits no preflight for
  lexical names, so a lexical's check runs at its own declaration, after
  the var and function properties declared before it; nothing observes
  them, since a declaration's SyntaxError ends the job before any of the
  script's statements runs and one job runs one script. `ThrowError` (42) raises its
  `ThrowKind` error with the Pants engine's message, naming the binding
  from the instruction's source span. In a compiler job, whose global object
  is protected, every one of these faults `NativeBoundary` before it
  changes anything, so no compiler script declares a binding and its
  `GetGlobal` keeps the protected path. The checks have no harness slice:
  the Pants engine reserves `undefined`, `NaN` and `Infinity` at compile
  time where QuickJS throws at run time, so `serial_tests.goose` checks them
  over global properties it installs.
- *Compiler jobs.* The realm rows are image rows below `Job.owned_nodes`, so
  a job's write into an intrinsic (`Array.prototype.leak = 1`) faults
  `NativeBoundary` before it mutates (decision 3, light-cljs-797), and its
  state is its rows, which the reset restores like any image row; a job
  never sees another's change. A job reads realm objects other than the
  global through their prototype chains; the global object keeps the rule of
  protected rows (a name it lacks is a `NativeBoundary`, since the V8 build
  has many more globals).

Measured on a native release build (November 5, 2026, one process, the
calls image of `corpus/serial`, no code strings, two processes): the realm
is 33 node rows (3,828 bytes), 488 value rows (4,392 bytes; overflow runs
with their spare capacity), 174 shapes (4,524 bytes), 1 accessor pair (18
bytes), 334 string headers (3,340 bytes), 1,438 Latin-1 units and a
512-slot key index (4,096 bytes), no property records: about 22 KB, and
6,681 work units. `initialize` takes 8.2 to 8.7 µs, mean of 2,000. With
`boot_runtime` the image is 51 nodes and 28,322 bytes of payload. Each
reused job worker copies the realm once with its spawn (about 22 KB more per
spawn; a 200k session at the 16 MiB cap spawns 2 to 8 workers); a restore of
the 28 KB image took 81 to 86 µs. The per-job reset (`snapshot::truncate`)
takes 47 to 50 ns when the job wrote no image row and 416 to 422 ns when it
did (a transition from an image shape or a new interned key, which copies
the twin's 192 shapes and 512 key slots back), mean of 20,000 each; before the realm the image had 28 nodes and few
shapes, so the realm adds at most about 0.4 µs to a job's reset, under
2 ms over the 200k project's 5,283 jobs.

*Static data at spawn (item 4).* A probe with a 1 MiB static `let` table of
`u32` (262,144 entries) read by 1, 16 and 63 live workers, native release
build, peak memory from `time -l`: 7.7, 40.0 and 141.3 MB with the
variable-size form `u32[]`, 8.6, 41.2 and 143.5 MB with the fixed-size form
`u32[262144]`, about 2.15 MB a worker in both (the worker's own stacks), not
the 1 MiB more a copy would add; spawn to first read took 0.11 to 0.13 ms a
worker either way. Static data is therefore shared by every thread program,
not copied at spawn. The forms differ in address space: `goose check
--stacks` gives a variable-size `let` one global stack region in every
program that reads it (a reused job worker took 5 more regions for the
realm's tables written that way, lowering the runner's thread cap from 291
to 262), a fixed-size one none. Areas write large immutable data (Unicode
and Intl tables) as fixed-size `let` arrays, as `realm.goose` does. With
them, `goose check --stacks` on the runner (`src/compiler_session.goose`):
main 63 data stacks plus 1 global stack (master: 62 plus 1), `run_worker`
57 (56, the `Job.detail` message buffer), `preparer` 41, `profile_restore`
26, `supervise` 2, `input` 2; thread cap 286 workers (291). The
conformance runner is 40 plus 1 (master: 39 plus 1) and
`src/execute.goose` 38 plus 1 (37 plus 1). The defaults stand.

The 200k check gate (`compare.bb --phase check --runs 1`, November 5, 2026)
passes: Goose 1/2/4/8 15.4/10.7/7.2/7.3 s, and 15.5/10.6/7.5/7.4 s after the
review's changes, V8 6.0 s, against 15.8/10.7/7.4/7.3 s at light-cljs-805:
within run-to-run noise.

## Decision 7: conformance harness

**Decision.** A Babashka runner, `crates/big-boy-pants-js/scripts/conformance.bb`,
runs each case on Goose, the Pants engine and QuickJS and prints a per-case
ledger grouped by the 14 areas of bbp-34 to bbp-47 (light-cljs-762 to -775):

- **Cases.** The Pants engine corpus (`rust/tests/fixtures/js`, 73 files and
  `quickjs_modules/`); its QuickJS-derived files, labelled from their headers
  ("Adapted rows: none" is unmodified text with an added header); and
  unmodified `~/src/quickjs/tests/test_*.js` files, run one test function at
  a time with the file's own `assert` helpers. `test_std.js`, `test_worker*.js`,
  `test_rw_handler.js` and `test_bjson.js` are excluded as QuickJS host
  facilities, following bbp-33; `microbench.js` is a benchmark. Pants fixtures
  that exercise its scheduler host (`concurrency_*`, `need_*`, `fold_*` and
  the like) are labelled host-specific with their reason, not counted as a
  JavaScript area. Every exclusion is listed in the ledger with its reason.
  A Pants fixture is one case, so a fixture that tests many methods cannot
  show a foundation pellet's partial progress; bbp-authored cases under
  `crates/big-boy-pants-js/conformance/cases/`, labelled as such with the
  fixture lines they derive from, cover those slices, and the full fixture
  stays in the ledger for its area.
- **Area map.** A checked-in EDN file assigns each case to one area, taken
  from the case lists in bbp-34 to bbp-47.
- **Outcomes.** Goose: pass, fail (throw, wrong result or disagreement with
  the oracles) or explicit-unsupported (the named opcode or built-in, at
  export or at execution). Pants engine and QuickJS: pass or fail. A case that
  the oracles disagree on is flagged rather than scored.
- **Regression check.** `--check` compares Goose passes with a checked-in
  list and fails on any regression, so every area pellet can name the cases
  its acceptance requires. The ledger itself is generated output in `target/`,
  not a backlog.

**Rejected.** *A test262 campaign now*: a later, separately queued tier.
*A Rust test runner*: harness scripts are Babashka (AGENTS.md).

## Stack settings

Defaults are kept: 2 GB reserved per data stack (`--stack-reserve`), at most
1,024 data stacks per thread program (`GS_MAX_STACKS`), 32 TB for all thread
programs (`GS_STACK_BUDGET`), 1 MB guard gap. Reservations are address space
until used.

`goose check --stacks` on today's runner (`src/compiler_session.goose`):
the main program uses 50 data stacks plus 1 global stack, `worker` 47,
`input` 2, `supervise` 2; the thread cap is 347 workers. The probe
interpreter (P7) needs 18 regions for its main program and 8 for a job worker
(cap 2,044).

On the 200k project, a runner built with `-DGS_STACK_STATS=1` ran the
`compare.bb --backend all --phase check --runs 1` gate (every check passed;
Goose 1/2/4/8 took 14.7/10.2/7.2/7.3 s and V8 5.9 s, not a timing run). In
this Goose version the report gives stacks and regions per thread program,
not bytes used. The 8 runner processes (warmup and sample of each Goose cell)
reported 50 data stacks and 51 regions for the main program, 30 job workers
of 47 stacks (1 + 2 + 4 + 8 per process pair) and 38 workers of 2 stacks; no
thread program came near `GS_MAX_STACKS` or the 347-worker cap. Peak memory
in bytes is the measurement phase's job (`time -l`), not this report's.

With one job worker per job (light-cljs-784), `goose check --stacks` on the
runner: main 51 data stacks plus 1 global stack (52 regions), `run_job` 46,
`supervise` (now a reaper) 2, `input` 2; the thread cap is 354 workers. A
runner built with `-DGS_STACK_STATS=1` compiled the 200k project once at
each of 1/2/4/8 workers (November 5, 2026, plain mode, retention 32): every
run's main program reported 51 data stacks and 52 regions, each of its 5,283
job workers 46 stacks and 46 regions (21,132 reports), and each input thread
and reaper (147 reports) 2. At most 2 + 2 × (limit + retention) threads are
alive at once, at most 274 with the caps, so nothing comes near
`GS_MAX_STACKS` or the thread cap and the defaults stand. The spawn-per-job
model does pay per region at every spawn and exit (probe P12); a smaller
reserve saved only a fifth of that, so it does not justify a change either.

With reused job workers (light-cljs-795), `goose check --stacks` on the
runner: main 59 data stacks plus 1 global stack (60 regions), `run_worker`
53 (the base snapshot it keeps as its twin and the job scratch of `serve`),
`preparer` 38, `supervise` 2, `input` 2; the thread cap is 307 workers. At
most 2 + 2 × (limit + retention) threads are alive, 274 with the caps, still
below it. Workers are spawned on demand, so the 200k project starts 1,282
job workers a session at the default cap instead of 5,283 (2 to 8 with a cap
no job reaches); the region cost per spawn is unchanged and paid that much
less often. The defaults stand.

With the 16 MiB default table cap (light-cljs-791, November 5, 2026),
`goose check --stacks` on the runner is unchanged: main 59 data stacks plus
1 global stack (60 regions), `run_worker` 53, `preparer` 38,
`profile_restore` 21, `supervise` 2, `input` 2; thread cap 307 workers. On
the 200k project a session spawns 2 / 3 / 5 / 8 job workers at 1/2/4/8
execution workers and retires none, within light-cljs-791's bound of
`limit + retention + 64` (97 to 104). The measured per-spawn cost, from the
1.25 MiB sessions that retired 1,282 workers: 15 microseconds of spawn call
in the coordinator, about 177 microseconds more from queueing to first
instruction per successor at one worker (its 53 regions and image restore),
and a join of 259 microseconds at one worker and 1,987 at eight. At 16 MiB,
spawn calls and joins sum to 0.03 to 0.15 milliseconds a session, under
0.003% of Goose wall at every worker count (the bar was 1%), so neither
fewer data stacks nor a region-reuse proposal for the Goose maintainers is
needed, and the defaults stand. Evidence: "Reused job workers at a 16 MiB
cap".

With data-only control flow (light-cljs-786: finally blocks, cancellation,
native frames, the 10,000-frame limit), `goose check --stacks` is unchanged
for the runner (main 59 data stacks plus 1 global stack, `run_worker` 53,
`preparer` 38, `profile_restore` 21, `supervise` 2, `input` 2; thread cap
307), the conformance runner (36 plus 1) and `src/execute.goose` (32 plus 1).
A recursion of 9,999 frames above the entry frame runs in the dispatch loop
without native recursion (`corpus/serial/depth.js`, `serial_tests.goose`),
so the JavaScript depth limit is independent of the 8 MB native stack.

With script declarations and global names (light-cljs-824), `goose check
--stacks` is unchanged from master: the runner's main program 93 data
stacks plus 1 global stack, `run_worker` 90, `preparer` 71,
`profile_restore` 27, `cell_owner` 9, `input` 4, `supervise` 2 (thread cap
180), and the conformance runner 72 plus 1.

The conformance harness (light-cljs-782, decision 7) runs one Goose runner
process per case, `src/conformance.goose`, at most `--jobs` (default 4) at
once. `goose check --stacks` on it: the main program uses 25 data stacks plus
1 global stack, 26 regions, and starts no workers, so it sets no thread cap.
A runner built with `-DGS_STACK_STATS=1` ran the whole ledger once
(`conformance.bb --stack-stats`, November 5, 2026: 181 cases, 147 run on
all three engines, 34 excluded). Only 11 cases got past the exporter; each of
those runner processes reported the same 25 data stacks and 26 regions. The
other 136 were refused at export by name and never started the runner, so
these figures describe today's VM only; `--stack-stats` measures them again
once more cases run. Nothing came near `GS_MAX_STACKS`; the defaults stand.

The design adds no Goose recursion per JavaScript call (decision 4), so the
8 MB native main-thread stack bounds only the remaining recursive functions
(form equality, validation), all depth-limited. Nothing measured justifies a
non-default setting. A job worker's own stacks are released when it exits, so
the spawn-per-job model raises the number of reservations made, not the
number held; the memory pellet records the counts for job workers and the
harness pellet records the harness's peaks.

## Un-Goose risks

Parts that still resemble a conventional heap, and why they stay:

- **Row IDs are object IDs.** Plain `u32` since light-cljs-797 (decision
  2): Goose bounds-checks each use, and the check of incoming IDs at thread
  boundaries is the trust boundary. A stale ID cannot outlive its job, since
  the reset truncates the job's rows and nothing keeps their IDs past it.
- **Append-only tables are a bump allocator per job.** Garbage is freed only
  at worker exit. Long-running JavaScript grows until evacuation, which is a
  whole-image copying step into a fresh worker, built only if measured.
- **Shapes and dictionary-mode objects** are the conventional engine
  structures; they keep keys once per shape and make property order and
  attributes explicit. Inline slots keep small objects in their header.
- **Weak references and finalization.** No collection happens inside a job,
  so a `WeakRef` target never dies and a `FinalizationRegistry` callback
  never runs during one. The specification permits this, but tests that
  expect collection will differ; light-cljs-769 owns the classification.
- **The job arena reset** (light-cljs-792). Any VM state the reset misses
  would leak one job's writes into the next on the same worker. The reset
  restores whole table prefixes and one state struct rather than logging
  writes, and a reset check compares them with their pristine twins in tests.
- **Per-spawn image copies** grow with the session image (since
  light-cljs-792 once per worker, with a prefix restore per job). If it reaches
  hundreds of megabytes, the copy dominates job cost; static data and a
  measured image budget are the mitigations.
- **Process-wide typed queues** need routing of answers to the right job
  worker; the existing re-queue pattern is kept, not a mailbox layer.

## Appendix: probes

Throwaway programs, run with the Goose above; each line gives the rule
checked and the result.

- **P1.** A fixed-mode enum with a `Cell&<u32>?` payload is rejected: "a
  fixed-mode Cell binds its payloads only by value (§3.5): use it in variable
  mode (Cell..), whose payloads bind by reference, or make the references
  `in pool`".
- **P2.** A worker using a global `Obj[>..]` whose elements hold
  `Obj&<u32 in objects>?` is rejected: "thread programs may access only flat
  globals, and `let` globals whose references lead only to static data
  (§11.2): objects holds references and is a var".
- **P3.** `type ObjId = u32 in objects` over flat globals works in a worker;
  the worker sees its own copy of main's rows taken at spawn plus rows it
  restores with `from_bytes`; main's table is unchanged.
- **P4.** Three `Value`s (f64 payload) take 27 bytes; an enum with an `i32`
  payload takes 5. Truncating a flat array to a mark works;
  `objects.resize(mark)` on an index-typed table is rejected: "an index type
  names objects, so its length never goes down (§3.11)".
- **P5.** One `Cell..[>..]` image with self-relative links: built in main,
  sent as `to_bytes` bytes, verified with `from_bytes` in a worker, extended
  with cells linking into the restored prefix, relinked in place inside a
  payload's slot run, returned and verified again. `heap.resize` on it is
  rejected: "Cell..[>..] is sequential".
- **P6.** A runtime-length run of slots holding self-relative links can be
  built by a function returning a pushed local (`resize` with a fill is
  rejected: "copying a value of type Slot, which contains self-relative
  references, is not supported").
- **P7.** The probe interpreter: `source::load` decodes the exported
  benchmark, a fixed-mode `Ins` enum and `Value`, frames as data, a native
  map frame, one job worker per run. With self-relative links every
  relinking store in helpers and operand-stack flows was rejected ("rooted at
  target", "root is not known exactly, only that it outlives heap"); with
  index types it compiles and runs. `--bce-lines`: 38 of 90 index checks
  elided; every `objects[id]` line keeps none.
- **P8.** `from_bytes` of a table of index-type links verifies them in the
  worker; 200 sequential spawn-and-join cycles of an empty worker average
  16,860 ns.
- **P9.** `from_bytes` of a table whose rows link into the same table, into a
  worker whose table is empty, returns false, so cross-worker images are
  restored through plain twin rows and a checking conversion.
- **P10.** A worker that owns an index-typed table (two rows, one linking to
  the other) spawns a job worker. The job worker sees both rows and the link,
  changes a row and appends one; after it exits the owner still has its two
  rows, unchanged.
- **P11** (light-cljs-784). A spawn copies only the globals the spawned
  thread program reaches: with an 88 MB global array in main, spawning and
  joining a worker that never names it took 23.6 microseconds, one that reads
  it 17.2 ms (an 8 MB array: 17.0 microseconds and 1.39 ms).
- **P12** (light-cljs-784). Spawn-and-join cost grows with the worker's data
  stacks: 26 to 33 microseconds for a contained worker with one data stack,
  340 to 351 microseconds for one with 44 (200 cycles, three rounds, native
  build). With `--stack-reserve 64M` the 44-stack worker still took 278
  microseconds, so the cost is per region, not per reserved byte.
- **P13** (light-cljs-792). An index type over a local array is rejected:
  "the array of an index type must be a global variable; a local or
  parameter has no name where a type is declared (§3.11)". A job heap in a
  nested scope cannot be linked by index types.
- **P14** (light-cljs-792). `clear()` and `pop()` on an index-typed global
  table are rejected ("cannot clear objects: an index type names objects, so
  its length never goes down (§3.11)"), but rows can be overwritten: three
  jobs each wrote image rows, linked one to the row they appended, and
  restoring the two image rows from a pristine local copy before each job
  undid every write, while the table kept all five rows.
- **P15** (light-cljs-792). `goose check --stacks` on a worker that reaches
  20 global growable tables and keeps 24 job-local arrays: "24 data stacks +
  20 argument and global stacks = 44 regions". A global table costs a region
  per worker, as a local array does.
- **P16** (light-cljs-792). 1,000 jobs of 64-byte rows, native build, three
  rounds; microseconds a job, including the queue round trip. A fresh
  contained 44-region worker per job: 388-397 with no rows, 410-414 appending
  6,000 rows (384 KB), 470-510 appending 18,000 (1.15 MB), 481-490 with a
  1.28 MB image. One 48-region worker reused, restoring a 28-row image prefix
  before each job and letting rows accumulate: 13.3-13.5, 41, 102-126. The
  same with a job arena (allocation tops reset to the image length, rows
  overwritten, in an index-typed table): 13.0-13.3, 22.5-22.9, 51-52, and
  28.7-33.2 restoring a 1.28 MB prefix. Tables without index types truncated
  after each job, option (b): 5.6-6.0, 10.7-12.9, 20.9-24.2.
- **P17** (light-cljs-797). Main fills an index-typed global table and
  spawns two workers: one writes a row and appends one (it sees `77`, three
  rows), the other then reads the table and sees main's rows (`1`, two
  rows), and so does main. A message struct holding an index-typed link is
  refused: "Link cannot pass between threads: it holds an index type, and
  each thread program's copy of the array it names grows on its own".
- **P18** (light-cljs-797). An owner thread holds cells as flat bytes and
  serves load and version compare-and-set messages; W workers each run 2,000
  `swap!`s (load, update locally, compare-and-set, rerun on a conflict),
  native build, two rounds. With replies on one queue routed by address,
  every worker count above one bounced replies (0.9 to 9.8 million times).
  With one reply queue per worker (`Reply<T>` with a marker type per
  worker selects a distinct queue; a generic `thread_fn` cannot be spawned,
  so each slot has a one-line wrapper): µs a swap of wall, 64 B / 64 KB
  values, 1 worker 9.6-9.8 / 28.8-33.8, 8 on their own cells 10.1-11.9 /
  15.0-17.0, 8 on one cell 46.6-51.2 / 85.8-86.8 with 111,700 reruns.
