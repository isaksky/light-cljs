# Architecture and embedding contract

This document describes the runtime at the verified baseline `72201c8b` plus
this documentation pass. [design.md](design.md) remains the requirements source;
[grammar.md](grammar.md) defines source/native coverage and all fixed resource
and policy limits. The proof map below is bounded evidence for the standalone
architectural slice, not a claim of complete JavaScript or compiler integration.

## Embedding

`Runtime` is shared heap and execution machinery (`Send + Sync`), with no owned
worker pool. `frontend::compile_script` / `compile_function` produce validated,
immutable `Executable`s. `Runtime::spawn` or `spawn_in` creates a `Task` with its
own world, frames, operands, environments, handlers and native continuations.
An executable is shared directly; arguments use roots from the same runtime.
Spawn checks runtime membership, not publication: the embedder must call
`Runtime::publish` before handing values across tasks.
See `src/runtime.rs`, `src/task.rs`, and runnable `examples/resumable_counter.rs`.

`Task::run()` claims exclusive execution with an unlimited budget and no host;
host effects then fail the task. Use `Task::run_with(budget, &mut host)` for
bounded execution with a `Host`, as in `examples/resumable_counter.rs`.
Claiming never blocks. A second claimant receives `RunError::AlreadyRunning`; a
finished task receives `Finished`. A run releases its Rust stack and reports:

| Outcome | Embedder responsibility |
| --- | --- |
| `Complete(Rooted)` | Retain the root as long as the result is needed. Engine jobs have drained. Query suspended async activations and unhandled rejections separately. |
| `Failed(VmError)` | Report the saved diagnostic; task execution is terminal. |
| `Yielded` | Enqueue another slice; includes a park that found its wake already delivered. |
| `Waiting(DependencyKey)` | Do not spin; route producer requests and enqueue tasks returned in `Wakeups`. |
| `Cancelled` | Cleanup finished; execution is terminal. |

Budgets count VM instructions and resumable native steps, not wall time.
A task can migrate between slices without moving or reconstructing its values.
Validated code stores poll flags and the length of each primitive instruction
stretch. Dispatch caps a stretch at the remaining budget and charges exactly
the instructions executed, preserving the next instruction even when a slice
ends partway through it. A branch ends a stretch; a backward branch starts a
new one so its poll still precedes the branch. Numeric arithmetic/comparison
and conversion instructions use a guarded primitive path; a nonnumeric operand
leaves the stack and instruction position unchanged for ordinary conversion.
Primitive execution creates no `Fault` result and primitive `Value` clones
copy their payloads without reference counting. Strings, symbols and BigInts
retain their existing ownership.

Derived fusion metadata recognizes two numeric local/constant loads followed
by arithmetic or comparison, optionally including one nested arithmetic
operation and a local assignment or forward conditional branch. All inputs
must already be Numbers; only then can the expression bypass intermediate
operand pushes and tags. The full original instruction count must fit the
current slice, otherwise the unchanged instruction stream executes up to the
exact budget boundary. A failed guard also uses that stream, preserving
conversion order, native continuations and diagnostic locations. Fusion adds
no serialized opcode and never crosses an interior safepoint; heap references
stay in task storage throughout.

Primitive and fused Number remainder uses integer remainder only when both
operands round-trip exactly through `i32` and the divisor is nonzero. The
integer overflow pair (`i32::MIN`, -1) produces zero, and the result takes the
dividend's sign, including negative zero. Fractions, larger Numbers, NaN,
infinities and zero divisors retain the general floating-point remainder;
BigInt and conversion dispatch are unchanged.

Run entry, allocation instructions, calls, returns, native steps and caught
exceptions retain their existing safepoints. A stretch cannot start a native
continuation or cross a poll. The longest stretch without a poll remains
bounded by executable code length. Cancellation is still observed by the task
driver at its documented run/dependency/park/budget boundaries; batching adds
no new unlimited-run cancellation guarantee.

VM calls retain immutable code through `TaskCode`: an `Arc<TaskCodeOwner>`
whose owner holds an `Arc<Code>`.
Each task caches at most 256 distinct handles by code identity; a miss takes
one shared code reference, while calls, returns and dispatch-frame changes
clone only the task's outer handle. Clearing a full cache cannot invalidate
live frames. Code bytes and child function tables remain shared, and tasks
never share their outer handles through function objects. Both ownership
layers support migration; no borrowed code pointer outlives a heap read.

Code ownership and heap rooting are separate. A function object owns its
shared code and traces its constants and captures; each call frame traces its
callee. The entry executable owns and roots entry constants. A detached
generator or async activation owns its `TaskCode` and traces its code constants
and callee even after the original VM state is gone. Suspension preserves
these owners; return, unwind and cancellation release them normally. The
bounded cache alone is not a heap root and cannot make a dead function's
constants live: cached handles can only be retrieved using code already held
by a live function or executable. Ordinary function creation and metadata
operations may still clone a shared code reference; ordinary VM frame
transitions do not.

Leaf `Host::effect` takes/returns root-free primitives, cannot reenter or wait,
and has no heap access; failure becomes a host-effect error. `Task::run` has no
host and rejects effects. Host clock/default-locale calls are unrecorded leaf
effects. `host::sim` supplies synthetic definition/result producers; its
scheduler is example glue, not a compiler implementation.

For static modules, `module::ModuleHost` resolves/fetches the complete graph
before spawning. Keep a `ModuleLoader` per runtime and host; immutable records
are cached by world/version, while instances and live binding cells belong to
the task. Imports are not dependency edges. Relative resolution, missing-module
errors, JSON classification and script-only CLI behavior are explicit in
[grammar.md](grammar.md#modules). See `tests/js_modules.rs` for complete host glue.

## Heap, roots and safepoints

There is one precise nonmoving stop-the-world tracing heap. Private object IDs
are slot plus generation; reuse increments the generation and an exhausted
generation retires the slot. Objects are boxed and never relocate. `Rooted` is
a persistent host root, `Local` belongs to `Runtime::scope`, and `WeakHandle`
does not retain a value. A Rust local containing a raw VM value is not a root.
Scopes register their locals in the heap; do not carry a scope across task
suspension. Root a value before leaving scoped or registered heap access.

`heap/storage.rs` contains the heap's unsafe boundary. Slot headers live in
fixed 256-slot segments; a lazy three-level `OnceLock` directory publishes their
addresses without moving or removing them during the heap's lifetime. Each
thread in mutator access reserves up to 64 empty slots under the object-table
lock. Ordinary and persistent allocation initialize these slots without that
lock; a refill is the only allocation-table critical section. A reservation
belongs exclusively to that thread and heap. Nested mutator tokens share it;
the last token returns unused slots, including on unwind. Tokens borrow the
heap, so the thread-local reservation cannot outlive it or alias a later heap
at the same address. Idle thread registrations alone do not retain reservations.

The slot's atomic word combines generation, liveness, known-shareable,
concurrent-reference and frozen flags. Initialization writes the boxed body
before a Release store makes the slot live; lookup Acquires that word and
compares the generation before accessing the body. Persistent allocation checks
only these atomic headers, including the concurrent-reference flag, without
reading another object's body. A live slot is never reinitialized.

Existing objects are no longer reached through the object-table lock
(light-cljs-471). There are two access classes per slot:

- **Frozen** bodies never change while live and are read with no lock and no
  atomic read-modify-write. Persistent vectors, maps, occurrences and atom
  handles are frozen at allocation. Functions, bound functions, built-ins and
  realm intrinsic objects/arrays become frozen when marked known shareable
  (realm installation or a successful publication check). Each freezable type
  is statically asserted `Sync` and has no conditional (weak) storage. A frozen
  body is never lent mutably. The mutable API hands a caller a private copy:
  it only inspects that copy in order to reject the change, for example a
  property write to an intrinsic or a `Map` method on a vector, and anything
  written to it is discarded.
- **Mutable** bodies (task-owned objects, cells, builders, promises, native
  payloads and functions not yet known shareable) are read and changed only
  while holding the slot's one-byte access word, a per-object spin lock. One
  holder at a time gets a reference, which never escapes the closure.

Ownership is not the soundness argument. Owner checks and `ForeignObject`
faults remain exactly where they were, inside the access closures. But memory
safety would hold even with duplicate task IDs, concurrent host callers or a
foreign task, because those can only contend for the word. Ownership and
scheduling make it fast: only the owning task's run changes an owned object,
and a task runs on one worker at a time. The word is therefore uncontended and
its cache line stays with that worker, and nothing on the access path is
shared between tasks. A lock-free ownership protocol without the word would
instead need biased-lock revocation for every foreign or host access, so it was
not adopted.

Marking takes the word, and the frozen flag is set while holding it. So the
last mutation happens-before the Release that freezes a body, and a writer that
raced the marking sees the flag once it holds the word. Publication replaces
the old table-lock exclusion with an explicit per-object protocol.
`mark_shareable_unless` takes the words of every expanded object in ascending
slot order, re-checks that no function gained its own-property record since the
walk, and marks them all before releasing. A record attach checks the
known-shareable bit under the same function's word. Whichever holds the word
first wins: an attach either precedes the re-check (the check walks again and
rejects) or follows the marking (the attach is refused). The sticky
known-shareable bit therefore keeps its contract. A thread holding a word
never waits for another, except during that ordered multi-word marking.
`objects` may be held while taking words (GC, statistics), never the reverse.
No heap lock or safepoint is taken while a word is held. All unsafe accesses
and the `Sync` implementation are confined to `heap/storage.rs`, which states
these rules with local `SAFETY` comments.

The runtime-wide property-key table keeps its leaf `RwLock`, but name lookups
first consult a bounded thread-local cache. Entries are tagged with the table's
never-reused identity and the sweep epoch read under the lock; a sweep advances
the epoch under the write lock before mutators resume. Concurrent tasks
therefore do not write the lock's shared reader count on every property access.

Sweep holds the stopped-world rendezvous, the object-table lock and each
reclaimed slot's access word: no allocator can be initializing a slot, and no
table reader or body borrow can survive into sweep. A lock-free frozen read
comes from an unparked registered mutator (which delays the rendezvous), from a
thread that keeps the object reachable from a root, or from a thread holding
the table lock. None of them can see an unreachable body being swept. The
collector traces mutable bodies under their words. It skips the conditional
pass for frozen bodies, which have no weak storage. It clears dead bodies, increments generations (retiring exhausted
ones), and returns the reclaimed slots to the free list in one pause. Empty
reserved slots remain exclusively reserved across collections; they are not
objects or roots. Reclaimed bodies are dropped after all heap locks are
released. Crate-internal setup/tests without a mutator token use a single-slot
allocation under the table lock. Diagnostics count previously used slots and
include reserved segment/directory capacity in table bytes; counts during
concurrent allocation are approximate, and quiescent counts are exact.

`Runtime::collect` refuses an unsafe rendezvous with `MutatorActive`;
`request_collection` requests one and waits. Running tasks poll at execution
and allocation boundaries, report their frames, and park for collection.
Host scopes delay collection until closed or released. Use `Scope::released`
around a host wait and `Scope::safepoint` in long scoped work; locals remain
rooted while access is released. Atom operations cannot safepoint internally.
Long leaf effects delay rendezvous. Threads holding multiple run guards cannot
park normally; embedders should avoid that arrangement.

The collector traces running/saved task frames, handlers and pending completions,
native continuations, closure environments, task globals/jobs, module cells,
host roots, intrinsic roots, dependency outcomes, atoms and native payloads.
`Trace` implementations report strong edges without executing user code.
Weak slots and ephemerons are resolved to a least fixpoint before sweep;
notifications root the owner and are delivered only after heap locks are released.
WeakMap/WeakSet do not strongly retain keys. WeakRef has job keep-alives;
FinalizationRegistry callbacks are owner-task jobs, never collector callbacks.

Immutable executable code, strings/ropes and persistent trie nodes use coarse
`Arc` ownership outside the object table. Trie-owning heap objects trace every
contained heap edge; `Arc` does not substitute for tracing. Persistent reads
resolve a frozen body with no lock, then walk it without per-node locks.
Strings own no GC object edges. Atom cells use a separate immutable-box lifetime
boundary described below. Short metadata/object locks exist; there is no global
interpreter lock and no lock held across ordinary bytecode execution or a wait.
The default `HeapPolicy` requests automatic collection after 16,384 allocations.
After each collection the threshold is the larger of that floor and the last
live-object count. Allocation only records pressure: the next VM boundary returns
to the task's collection driver with every identity rooted. The driver releases
its mutator token, retaining execution ownership and its remaining instruction
budget, and uses the same stop-the-world rendezvous as explicit collection.
Enclosing scopes, native operations and nested runs defer automatic collection
until a safe driver boundary. Host-only allocation still needs an explicit
collection after its scope closes. `Runtime::with_heap_policy` configures the
floor or disables automatic requests with `allocation_threshold: None`.
Explicit `collect` and `request_collection` remain available.

`HeapPolicy::hard_limit_objects` optionally caps allocated objects across all
workers with one atomic admission counter. Sweeping returns capacity; unused
slot reservations do not consume it. The automatic threshold is also capped at
half the remaining object headroom. A rejected task allocation raises
`VmErrorKind::HeapLimit`, catchable as the primitive string
`heap object limit exceeded`; raising it requires no further heap object.
Its catch block can release references and return or continue after collection.
The operation is not replayed. The cap includes unreachable unswept objects and
can reject a multi-allocation instruction before its next safepoint. Scope users
can use `try_new_object` instead of the legacy panicking `new_object` API.
The object limit does not bound payload bytes; the byte limit below does.

### Byte accounting

`HeapPolicy::hard_limit_bytes` is one aggregate admission counter
(`src/heap/budget.rs`) shared by every task, worker and host scope of the
runtime. Storage is charged **before** it is allocated, with checked arithmetic,
by an RAII `ByteReservation` kept by the storage's owner and released when that
storage is released (a sweep, a shrink, or the last alias of an immutable
payload). A denial therefore leaves the requesting operation's inputs intact:
a failed `transfer`, freeze, push or concatenation changes nothing. Without a
byte limit nothing is measured and reserving costs no atomic.

Charged, each conservatively (overcharging is allowed, undercharging a
data-sized payload is not):

- every object slot plus its boxed body and fixed boxed internal slots (Intl
  records, iterator cursors, error records; `HeapObject::boxed_bytes`), and the
  object table's segment growth;
- ordinary-object properties (512 bytes per entry, covering capacity, index and
  shape metadata), array dense capacity and sparse/attribute entries, Map/Set
  and weak tables, private elements, atom versions and the property-key table;
- string bodies (two bytes per unit plus 256 per body, ropes including their
  possible flattened copy; a formatter's accumulated output charges each
  node once and the whole output's flattened copy, its shared first piece
  included, at the extension that makes it a rope, and the returned body keeps
  that credit for its cached copy; a string index read's one-unit result,
  the Date and Intl formatters' outputs (`DateTimeFormat`, `NumberFormat`,
  `RelativeTimeFormat` and `DurationFormat` text and part values,
  `DisplayNames.prototype.of`), the data-dependent `resolvedOptions()`
  fields (locale, time zone, unit, currency), `Intl.Locale` tags, RegExp
  `flags`, the index keys of `Object.entries` and of `toJSON` calls, a
  converted computed key's fresh spelling and computed function names, each
  charged before its body is made, while presence and attribute checks of a
  string's index make no string), BigInt limbs, ArrayBuffer stores, native persistent
  vectors/maps/forms and builders, RegExp programs and captures;
- saved continuations: detached activations, closure captures, bound arguments,
  promise reactions, queued jobs, disposable resources, async-generator queues;
- retained native scratch: sort buffers, join/raw/JSON parts, string search,
  split and normalization state, Intl NumberFormat digits, ListFormat items and
  output, DisplayNames codes, locale lists, and own-key snapshots
  (`vm::keys::SNAPSHOT_KEY_BYTES` per key, charged before a string's or typed
  array's virtual keys are listed and per element while a Proxy `ownKeys` trap
  result is read). A for-in walk keeps its snapshot's charge until its visited
  names are released; fresh index spellings are charged string bodies.

Not counted: executable code and constants, the realm's Rust-side tables,
allocator overhead and fragmentation, Rust and VM stacks of running frames,
transient values that exist only within one leaf step (bounded by the step's
inputs), and the embedder's own data. Accounted bytes therefore approximate and
do not bound RSS; `docs/measurement.md` records the realm's accounted footprint.
Fixed-text leaf results are process-wide constants like executable constants:
`typeof`'s type names, `Boolean.prototype.toString`, the TypedArray
`Symbol.toStringTag` getter, `Intl.PluralRules.prototype.select`, the fixed
spellings of Intl `resolvedOptions()` fields, Intl part `type`s and `unit`s,
ListFormat separators, native `formKind` tags, the `Symbol.toPrimitive` hint, default `Error` names,
the empty RegExp source and promise resolving functions' names each return
one shared body per spelling (`JsString::fixed`, `FixedText`), made on first
use and never released, so retaining one allocates nothing. Every empty
string shares one body, and the two heap-limit exception messages share one
uncharged body each.

A Proxy trap's key argument is imported, so a converted primitive key's
fresh spelling reaches the trap as a charged copy. An audit of the
fresh-body constructors (light-cljs-494) found the other remaining fresh,
uncharged string bodies not retained per call: transient scratch within one
leaf step (property-lookup names,
option and code spellings parsed and dropped, pieces a charged builder or
JSON's `import` copies, `Function` constructor source handed to the
compiler); bodies made once per realm, executable or module (intrinsic
names, function names and source constants, JSON module text, export names,
well-known symbol descriptions); and embedder-supplied text. Property-key
spellings are charged by the key table when interned.

A denial fails the requesting operation with `VmErrorKind::HeapLimit`,
catchable as the primitive string `heap byte limit exceeded` (no Error object
is allocated at the limit). Leaf helpers with boolean or `Option` results record
the denial in a thread-local flag that the VM converts at the same instruction
or native-step boundary. Allocation never parks, so there is no last-resort
collection at the allocation itself: a denial instead requests a collection at
the next safe boundary (where a handler may already have released data), and
until a collection leaves an eighth of the limit free, every run slice (unless `allocation_threshold` is `None`, which disables all automatic collection) begins
with one, so a task started after others released their data succeeds. The
collection a denial requests usually runs before its handler has released
anything, so data a handler drops is reclaimed only by such a later
collection; `Runtime::byte_starved` lets an embedder that allocates in host
scopes between runs request one first, as light-cljs's native macro evaluator
does. Between
denials, collections are also requested once the bytes admitted since the last
collection reach half the headroom the last collection left. Not a fraction of
the live bytes: one operation can admit as much as the live set (a large
buffer or string), and a live-proportional trigger collected every few such
operations (light-cljs-495). Ordinary object churn is still collected in
proportion to the live objects by the allocation-count trigger above. The
limit includes unswept garbage and the realm. Persistent vector and map trie
nodes each carry the charge for their own allocation, taken before the node
is built, so an update is charged for the path it copies and a node shared by
several versions stays charged until the last of them is freed
(light-cljs-498).
`Runtime::try_with_heap_policy` builds the realm and one probe global before
returning, and reports `RuntimeStartError::HeapLimit` instead of failing the
first task; a sweep of limits around the realm footprint never panics.

`HeapStats` reports collection count, last live-object count, total pause and
maximum pause, and under a byte limit the managed, peak and last-live bytes, the
total admitted volume and the configured limit. Pause timing covers tracing and
sweeping while the world is stopped; it excludes waiting for mutators, deferred
payload destruction and observer callbacks. `bench gc` reports these at one and
eight workers. The automatic eight-worker root-inventory tests, under an object
cap and under the byte limit alone (admitting several times the limit in
total), retain suspended tasks, native forms, atoms and completed dependency
outcomes without requesting collections themselves.
Moving/generational/concurrent collection remains deferred.

## Ordinary objects and named property caches

Ordinary objects retain their creation-ordered property vector and its existing
large-object dictionary index. They also carry a shared immutable shape describing
the sequence of slot keys. Adding a property follows a shared shape transition;
deletion or more than 64 properties switches that object to dictionary mode.
Bounded worker-local transition caches avoid locking on repeated construction.
Their direct front cache avoids repeated hashing, and its initialization goes
directly into heap storage to preserve the 64 KiB VM-worker stack bound.
Each local shape handle wraps the shared shape in a second owning `Arc`, so
repeated allocation does not update a global reference count. Both layers are
safe to migrate between workers. The bounded shared transition table holds weak
references; none of these caches makes heap values or property keys live.

`Executable::load` classifies named property constants once. Executables remain
runtime independent, so each constant's runtime key is interned lazily at first
use. Its atomic binding packs the key with a globally unique runtime/collection
stamp. The stopped collector changes that stamp before key slots can be reused.
Subsequent named reads, writes and literal definitions neither encode nor hash
the name. Reusing an executable in another runtime or after collection safely
rebinds it. Exhausting the stamp space disables binding-cache hits.

Each instruction has a monomorphic atomic shape/slot cache, shared by concurrent
tasks running the same executable. A single atomic word prevents torn pairs;
replacement only changes hit rate. Hits validate the live slot key as well as
the shape. They read the current value, accessor and flags, so descriptor
changes and freezing need no stale-descriptor invalidation protocol. The first
slot's live key directly proves a hit without consulting the cache, including
for a single-property record. Prototype lookups still walk the current chain
and check each receiver's ownership and
exotic behavior; they can use the same slot cache when reaching the defining
ordinary store. This also accelerates the named read preceding a method call.
No cache holds a value, prototype or object handle, and no validation takes a
cache lock. Deletion invalidates the shape; prototype mutation cannot bypass
the live chain walk. Computed properties retain the generic path.

`tests/property_caches.rs` exercises shape changes, dictionary fallback,
deletion/order, accessors and freezing, prototype/method replacement, concurrent
tasks with different shapes, and repeated collection/runtime key rebinding.

Presence lookup stops at a Proxy, including one on the prototype chain. `in`,
`Reflect.has`, and internal presence checks in array methods, descriptor readers,
locale lists and Error options enter the same traced native continuation. A
`has` getter or trap can yield or wait without replaying the walk or advancing
an element twice. A falsy trap result checks the target's own descriptor, then
its extensibility only for an existing configurable property, following
[ECMA-262 10.5.7](https://tc39.es/ecma262/multipage/ordinary-and-exotic-objects-behaviours.html#sec-proxy-object-internal-methods-and-internal-slots-hasproperty-p).
Those invariant probes can themselves call target Proxy traps. Their saved
target, handler, key, descriptor and callback values remain traced across
collection and worker migration. Descriptor presence callbacks are exercised
through `Object.defineProperty`, `Object.defineProperties`,
`Reflect.defineProperty` and Proxy descriptor results; extensibility callbacks
are exercised through enumeration and reflection as well as `has` invariants.

Property reads likewise stop at a Proxy and retain the original receiver,
including inherited reads and `Reflect.get` with an explicit receiver. Deferred
reads share the accessor continuation protocol throughout native built-ins,
conversion and bytecode. A `get` trap checks the target's own descriptor after
the call, enforcing SameValue for fixed data properties and undefined for a
fixed accessor without a getter. Its result, target and receiver stay traced
while target descriptor callbacks run. Callable Proxy dispatch stops before
unwrapping its target, including after bound/Reflect forwarding and in native
callbacks. The `apply` continuation reads the trap once, builds a fresh ordinary
argument array in budgeted steps and calls the trap with its handler as `this`.
Absent traps forward the original receiver and arguments. These operations
follow [ECMA-262 10.5.8 and 10.5.12](https://tc39.es/ecma262/multipage/ordinary-and-exotic-objects-behaviours.html#sec-proxy-object-internal-methods-and-internal-slots-get-p-receiver).
Transparent forwarding also retains conversion lookup mode, including the
supported fallback past unavailable Function.prototype conversion methods.
JSON array classification follows Proxy targets, then reads `length` on the
original receiver and converts it with resumable ToLength. The Node-checked
`tests/fixtures/js/proxy_builtin_reads.js` table verifies read order, virtual
lengths and failures across JSON, array built-ins, spread, Object.assign and
string conversion. `tests/proxy_get.rs` repeats it with 113 dependency waits,
collection and worker migration.
Other present traps retain their explicit unsupported errors; direct
`Object.isExtensible`/`Reflect.isExtensible` traps remain outside the supported
surface.

### Static engine-name conversion audit

The static-name audit distinguishes repeated execution from installation and
data-dependent text. The categories below cover the Rust-to-JS string/key
construction sites in the engine; error-message construction is unchanged.
Only sites observed in the array/object/string/Map microbenchmarks or native
protocol macro workload are candidates for a hot-path change.

Native iterator names use `vm::property::FixedName`: one lazily initialized
string, the same `NamedProperty` binding as executable constants, and a live
slot cache. Its non-interning read binding caches only successful lookups;
absence still checks ownership and consumes no key-table bytes. A collection
or another heap invalidates the binding through the existing global stamp.
These hints root no keys or objects, and cache neither values nor prototypes.
Result-object construction interns exactly the same keys as before. Fixed
form-kind results and function-name comparisons use `FixedText` directly,
without the process-wide interning table's per-call hash and lock.

| Names and construction sites | Handling |
| --- | --- |
| Source property constants (`vm::executable`, `vm::property::NamedProperty`) | Classified once on executable load; runtime/collection-stamped bindings and live intrinsic/ordinary slot caches avoid repeated encoding and hashing. |
| Builtin method/global tables, prototype installation, builtin `name`/`length` (`realm`) | Constructed during realm installation, or task-global initialization. Existing keys are interned; no changes to these cold setup conversions. |
| Builtin dispatch and equality (`vm::builtin::Builtin`) | Enum discriminants and scalar payloads, including object identities; no string names, UTF-16 encoding or string hashing in equality. Rust `name()` results serve installation and diagnostics. |
| Function special-name classification (`vm::function::special`) and native call helpers | Six `FixedText` spellings compare exact UTF-16 units without re-encoding the Rust names. `instanceof`'s `prototype` and `apply`'s argument-list `length` use `FixedName` reads. Metadata record reads, bind and constructor helpers retain their existing conversions; these were not sampled as hot sites. |
| Native form-kind tags (`vm::intrinsics`, `collections::form::FormKind`) | Each tag, including `record` and `occurrence`, has a `FixedText` body. Repeated protocol predicates neither allocate a string nor consult a string interning table. |
| Protocol dispatch (`collections::clj`) | The common primitive/form kind names share an existing `OnceLock` table; the default `_` key and less common kind names still construct strings when used. These conversions did not appear among the sampled hot sites. |
| Iterator result/record fields and reads (`vm::native::iterator`), array callback preparation | `value`, `done`, `iterator`, `next` and `length` use `FixedName`. Implicit array/string length classification also compares a fixed UTF-16 body. User getters and Proxy reads retain the deferred continuation path. |
| Well-known symbols (`value::symbol::well_known`) | Rust-name match selects a `OnceLock`; each symbol and its formatted description are made once process-wide. Comparisons use symbol identity. |
| Fixed leaf results (`value::string::FixedText`, `JsString::fixed`) | Existing shared bodies for typeof, boolean text, typed-array tags, conversion hints, error defaults, regexp defaults and Intl option/part spellings. Empty strings already share a body. |
| Intl field names, option readers, resolved-options objects and bound formatter metadata (`intl::service`, `vm::native::intl*`, formatter modules) | Option values already use fixed bodies. Field keys and option names still encode when constructing or reading a service result; these paths are outside the measured workloads. Data-dependent locales, currencies and formatted output remain byte-accounted text. |
| Date and JSON (`vm::date`, `vm::json`) | Date output and JSON input/output remain data-dependent, byte-accounted text. Static JSON `toJSON`/`length`, punctuation and fixed output words are outside the measured hot paths; unchanged. The unsupported Date `toJSON` probe also remains unchanged. |
| Proxy trap names and descriptor fields (`vm::proxy`, `vm::descriptor`) | The resumable Get/Has/own-keys/descriptor/extensibility trap lookup uses `FixedName`, preserving the current handler, getters and callbacks. Leaf forwarding, callable-Proxy trap names and descriptor-field conversion remain outside the sampled hot sites. No trap value or descriptor is cached. |
| Iterator records, generator/promise metadata, regexp fields, reflection, disposal and other native readers (`vm::native/*`, `vm::keys`, `vm::promise`, `regexp::object`) | Initialization and generic fallback names remain as written unless identified separately by a profile. Index spellings are data-dependent and stay on the existing charged/interned path. |
| Compiler-runtime bridge and module names (`host::sim::facilities`, `module`, scope APIs) | Simulated namespaces install once; module/export names and embedder strings are data-dependent. Named property accesses in the compiler's JS bridge use executable bindings; free global identifiers retain their existing generic lookup. Neither path re-encodes a Rust name on each call. |

## Publication and native values

`Runtime::publish` walks the transitive graph and reports a path on rejection;
it does not serialize, clone or freeze an ordinary object. Dependency completion, atom installation and Promise settlement also validate
publication; spawn leaves cross-task handoff validation to the embedder.

| Class | Contract |
| --- | --- |
| Immutable/shareable | Primitives, persistent collections/occurrences, native form values and eligible functions whose captures and other edges pass publication. Unchanged subtrees preserve identity. |
| Task-owned | Ordinary objects/arrays, property records, lexical mutation cells, builders, generators and async activations; freezing JS properties does not make these publishable. |
| Explicit concurrent reference | Atoms and all Promises/settlement guards; installs validate their contents rather than allowing arbitrary shared mutable graphs. |

A closure with mutable captures or a task-owned home/property record cannot
publish. Published functions are frozen against later property mutation; globals
remain those of the executing task. Derived Promises and their resolving guards
are shareable and permit cross-task settlement. Subclass own properties and
prototype access follow ordinary-object ownership rules, including property
protocols such as constructor/species lookup; publishing a Promise grants no
foreign access to that state. Publication checks and
property mutation coordinate to avoid caching a stale shareability decision.
Promise private elements are likewise opaque task-owned storage, traced even
when the Promise is shared. Foreign private access (including private-in and
initialization) throws `TypeError`; publication never publishes private payloads.
Native Promises can acquire private elements through a returning super without
gaining public property storage; the first private initialization owns that store.
Module namespaces remain task-owned even when empty or frozen. Any
Promise can retain a private result in an opaque envelope tagged with the
settling task; foreign reactions reject rather than reading it. Sharing the
Promise therefore does not publish that private graph.
Tests: `tests/publication.rs`, `tests/promises.rs`, `tests/builtin_subclasses.rs`,
`src/task/job_tests.rs::derived_promises_publish_and_settle_across_tasks`.

Native collections are this project's own contract (`src/collections.rs`), with
persistent radix-trie vectors, HAMT maps and provenance occurrences. Updates
path-copy and share unchanged values; no-op updates preserve the input identity.
Map keys use private SameValueZero comparison (objects by identity; strings and
BigInts by value), with no user hashing or equality. `Scope::value_equals` is
structural; object identity and source occurrence/provenance remain distinct.
Builders belong to a task, survive migration, freeze once and cannot publish.
JS `Map`/`Set` are separate task-owned mutable collections; their iterators use
monotonic insertion positions, delete/clear eagerly release entries, and weak
variants use conditional tracing. See the [native contract](grammar.md#reserved-intrinsic-binding).

Native Clojure form values (light-cljs-466, `src/collections/form.rs`) are a
separate frozen kind for macro inputs and outputs: keywords and symbols with
exact UTF-16 namespace/name, lists, vectors and maps, and metadata. A name is
accepted only when its spelling splits back into the same namespace and name at
its first `/` (the reader's split; `AmbiguousName` otherwise), because the
runtime compares names by spelling but hashes namespace and name, so
`(keyword nil "a/b")` is `=` to `:a/b` with another hash. They reuse
the vector trie and the HAMT storage (the trie operations take key hash and
equality as parameters; the SameValueZero map API is unchanged). The `seq`,
`rest` and `next` of a form list or vector (`src/collections/clj.rs`) are lists
over the same trie with a hidden prefix (`PVector::drop_first`, light-cljs-502):
O(1), no node allocated or charged, and the hidden elements are never read,
traced, published or compared. Equality and
the 32-bit hash are the light-cljs runtime's `=`/`hash` for these values: lists
and vectors are sequentially equal, map keys are structural, metadata and
provenance never take part, and the walks use explicit heap stacks (also across
nested keys with colliding hashes, with backtracking) and run no JavaScript;
equality decides each distinct pair of forms once.
Identity stays separate: a task returns an unchanged subtree as the same object,
and equal scalars at different locations are distinct occurrences. Exact reader
literals that no Number represents keep their text in the host's table, keyed by
the provenance of the occurrence a macro returns unchanged. Forms are
`Persistent` for publication (elements, keys, values and metadata are walked,
with `[i]`, `{:key}` and `.meta` path steps) and traced as `FormEdges`.
Rust builds and reads them through `Scope::new_keyword` … `Scope::form_value`;
JavaScript through `__rt.form*`. Sets (light-cljs-477) are a sixth kind with
structural members, the runtime's `hashUnorderedColl` hash and `__rt.formSet`/
`formDisj`. The `__rt.clj*` selectors of the same `form_op` instruction
(`crate::collections::clj`) are ClojureScript core operations for compiled
macro code: occurrence-transparent, `nil`-punning, sequences as new lists, and
catchable `TypeError`/`RangeError` diagnostics for unsupported operands.
Structs (light-cljs-478) are records and immutable deftype instances: a
struct type form is the type's identity, records follow the runtime's record
methods, equality and hash (cached in the form's atomic slot), and
`__rt.cljDispatch` reads a protocol table, a SameValueZero persistent map from
a type identity or kind string to the method. A task can install its
core-protocol resolver with `__rt.cljCoreResolver`. Core operations on structs
first call that resolver and then its selected method through `CallArray`
native frames. Both calls retain traced operands across suspension, collection
and migration. A missing entry resumes the original leaf operation; lazy
realization retains that resolution instead of running it again. Two-argument
lookup has its own `cljGet2` selector. Sequence operations also resolve a struct
exposed by a realized lazy wrapper; the resolver receives the realized receiver
and a flag identifying that wrapper, while its callback retains the original
operands. A realized record keeps its original receiver as well as its built-in
sequence in the cell; both are traced and checked for publication. Structural
operations read the built-in sequence and never resolve a protocol. Persistent form comparison and hashing,
including map keys, set members and stored values, remain structural and never
call the resolver; protocol and dependency tables also never call it.
Compiler fixed-call preparation uses `cljCallable(v)` and
`cljCallMember(v, multimethodEntry)`. The latter unwraps occurrences and either
selects the shared multimethod entry or starts the ordinary `.call` property
read. `cljReadRoot(roots, context, name)` reads the task's overlay Map and live
root descriptor. Its context is a persistent vector of the original Proxy,
target, overlay Map and compiler-product getter registry, allocated once per
task; its mutable members retain ordinary task ownership/publication rules.
Only an exact registered compiler getter for a ready bundle bypasses that
getter's JavaScript state check. It still reads the bundle's current product
data property and retains the getter's captured name. Deferred reads, including
product accessors and Proxies, run through the original root Proxy so its get
invariants check the descriptor after user code returns. Modified descriptors,
pending bundles and exotic bookkeeping retain ordinary property execution. These
bounded leaves release all heap borrows before handing accessors or Proxies to
traced property continuations; they never execute JavaScript on the Rust stack.
Sorted maps, shared seq views of lists, and the compiler `Datum` bridge
(light-cljs-462) are not part of this surface. The module
docs carry a runnable example. Tests: `tests/forms.rs`, `collections::form::tests`,
`collections::clj::tests`, `form_edges_retain`/`form_edges_reclaim`.

Lazy sequences (light-cljs-479, `crate::collections::lazy`) are a form kind
whose body is a once-realized cell: a dependency cell local to a value. A clj
operation that meets an unrealized cell does not complete; its instruction
starts a `Realize` native frame holding its operands, which claims the cell
and calls the thunk in the claiming task, installs the value under the cell's
leaf lock and executes the instruction again (intrinsics are pure). A task
that meets a cell another task is realizing subscribes its wake handle and
parks (`Exit::LazyWait`, then `RunOutcome::Waiting` with a `LAZY_REALIZATION`
key); the settlement drains the waiters under the lock and wakes them with no
lock held, queuing the runnable ones for `Runtime::take_lazy_wakeups`, and
`Runtime::cell_status` reports the key pending until they are woken. A thunk
that throws leaves the cell unrealized, as ClojureScript's `LazySeq` does; the
readers parked on that attempt receive the failure (the thrown value when it
is shareable, an `Error` with its message otherwise), and a later reader
runs the thunk again. A dropped or cancelled realizing frame gives its claim up.
The cell is persistent for publication through its thunk (`.thunk`) or its
value (`.realized`); a successful publication check marks it shared, after
which a realized value must be shareable. Equality and hashing record a
realization request in a thread-local slot instead of returning one, and the
instruction takes it. Cons cells (realized elements before an unrealized tail)
and `__rt.cljChunk*` operations give chunked sequences; `__rt.cljRealizeAll`
replaces every lazy sequence inside a form with a list. Tests:
`tests/lazy_seqs.rs`.

## Atomic references

`Runtime::new_atom` creates a CAS-updated shared atom;
`new_single_writer_atom` returns an atom plus a unique, non-clone `Publisher`.
`Publisher::publish` may repeatedly replace its value; CAS on that atom is
rejected. Dependency cells are a third, single-assignment mechanism.

CAS uses SameValue (`Object.is`): object identity, string/BigInt value, NaN
matching itself and signed zeros distinct. A conflict returns a rooted current
value; it never reruns user computation. Compute effectful work before CAS.
An A→B→A value change may match A; slot generations prevent storage reuse from
creating a false object match. No cross-atom total order is promised.

Each atom has an `AtomicPtr` to an immutable box, Acquire loads and AcqRel
replacement (Acquire on CAS failure). Replaced boxes retire until a collection
rendezvous establishes a grace period; reads root the loaded graph inside their
mutator registration before a writer can make it unreferenced. This is the
unsafe pointer boundary, documented with `SAFETY` invariants in `src/atom.rs`.
A loaded `Rooted` survives replacement and collection until released.
Tests: `tests/atoms.rs` and `src/atom.rs` unit tests.

## Dependencies, notifications and cancellation

A `DependencyKey` contains world/version, definition/query ID and result kind.
`RunGuard::need` / source `__rt.need` records every use, including a ready read.
Pending reads claim at most one producer request, subscribe a compact task wake
handle and park at the saved operation. `take_producer_requests` transfers requests
to the embedder; the runtime does not invent compiler scheduling. The simulated
host distinguishes binding, callable, initializer and analysis results, returns
errors for missing definitions, and separates recorded edges from active waits.
Recursion of references is valid; only unresolved execution waits form deadlocks.

A cell settles exactly once: Ready, Failed or Cancelled, with immutable rooted
outcomes. It is never reset for a new world. `Runtime::complete` validates producer
and value, installs the outcome, and delivers wake handles after releasing waiter
locks. Enqueue returned `Wakeups` through the scheduler; publication executes no
JavaScript or host callbacks. Acquire/Release result publication and a per-cell
pending lock implement read-or-subscribe. Task state/park epochs deduplicate
notifications and cover completion before subscription, during registration and
before parking ends. Consumer cancellation does not cancel a producer needed
by others and retains the consumer's recorded dependency.

`Task::cancel()` only requests cancellation. If it returns
`Cancel::Runnable(task)`, the task was parked and the caller **must enqueue it**
for cleanup. `Cancel::Requested` means its existing runner/queue will observe the
request; no extra enqueue is needed. `AlreadyRequested` and `Finished` change
nothing. `Executor::cancel` performs this handoff for its tasks. Ordinary wake
handles become stale once cancellation is requested; later cell completion
cannot substitute for that required enqueue. The task's subsequent run removes
the active waiter and releases saved roots as it performs terminal cleanup.
See `src/task/cancel_tests.rs::cancelled_parked_consumer_leaves_the_cell_when_its_run_cleans_up`.

A dependency wait preserves handler and finally state; it is neither an exception
nor an await checkpoint. Completed dependency failure/cancellation raises at the
need with its original diagnostics and is catchable. Consumer cancellation is
forced completion: finally/disposal cleanup runs under budgets, skips surrounding
catch handlers, cannot revive the task and cannot wait for another dependency.
An await reached during cancellation cleanup ends cleanup. See
`tests/exception_waits.rs`, `tests/native_unwind.rs` and `src/task/cancel_tests.rs`.

Resumable natives save progress, operands and results in traced task-owned
continuations. They request VM calls, coercions or dependency reads through the
trampoline, without keeping Rust frames or mutable VM borrows across reentry.
Each native step participates in budgets/safepoints. The same mechanism supports
folds, getters, iterator consumers and built-in methods. Leaf bounded algorithms
still have latency limits listed in [grammar.md](grammar.md#resource-and-policy-limits).

Promises are shareable with cross-task settlement; reaction JavaScript runs only
as jobs of the registering/owning task. Await suspends an async activation and
allows other task work; `__rt.need` instead waits the whole task. At quiescence,
`Complete` retains queryable unhandled rejections and reports suspended async
activations. Jobs, finalizers and opt-in virtual timers are task-owned execution;
foreign settlement only enqueues. See `tests/promises.rs`, `tests/quickjs_async.rs`,
`tests/async_generators.rs` and `tests/sim_facilities.rs`.

## Requirement map

**Proved** means an asserted runnable case covers the stated requirement in this
slice. **Partial** names narrower evidence; **deferred** means no implementation
claim. Test names below are searchable exact identifiers in the linked file.
The commands and results are in [measurement.md](measurement.md#verification-run).

### Design §13: end-to-end construction and eight proofs

The setup is [`examples/e2e/scenario.rs`](../examples/e2e/scenario.rs), using
`consumer.js`, `producer.js`, `work.js` and `companion.js`. Rust creates the shared
form and delays one helper producer until consumers park; JavaScript implements
the consumers, helper and useful work. All rows below refer to
[`tests/e2e.rs`](../tests/e2e.rs).

| Requirement | Status | Concrete proof |
| --- | --- | --- |
| Rust input, two references, pre-need JS, unavailable helper, park, work, VM helper publication, invoke, same subtree | proved | `events_follow_the_scenario_order`; `returned_subtrees_keep_the_input_identity` |
| 1. Parallel OS workers | proved | `consumers_overlap_on_distinct_workers_and_share_one_producer` |
| 2. Shared data without per-worker imports | proved | `returned_subtrees_keep_the_input_identity` |
| 3. Wait frees a worker | proved | `waiting_consumers_leave_their_workers_free_for_useful_work` |
| 4. Multiple consumers, one result | proved | `consumers_overlap_on_distinct_workers_and_share_one_producer` |
| 5. No replay of effects | proved | `resumed_consumers_migrate_and_replay_nothing` |
| 6. Migration after wait | proved | `resumed_consumers_migrate_and_replay_nothing` |
| 7. Collection during suspension | proved | `collection_while_suspended_keeps_task_values_live` |
| 8. Completion, failure, cancellation | proved | `producer_failure_wakes_both_consumers_and_their_catch_observes_it`; `cancelling_the_claimant_leaves_the_producer_and_the_other_consumer_able_to_complete` plus normal path |
| Extension: nested call, live closure/local, finally, native→JS reentry | proved | `companion_fold_suspends_in_its_callback_and_completes_after_migrating` |

### Design §15: every test item

| Requirement | Status | Concrete test pointer |
| --- | --- | --- |
| Completion before registration | proved | `src/task/need_tests.rs::completion_before_subscription_is_read_in_the_same_run` |
| Completion during registration | proved | `src/task/handoff_tests.rs::completion_between_pending_read_and_subscription_is_read_by_the_recheck`; `tests/dependency.rs::needs_racing_completion_are_never_lost_or_woken_twice` |
| Completion while parking | proved | `src/task/need_tests.rs::completion_inside_the_park_window_yields_and_resumes_at_the_need` |
| Many waiters | proved | `tests/wakeup.rs::many_waiters_each_resume_exactly_once_on_success_and_failure` |
| One producer per key | proved | `tests/dependency.rs::concurrent_first_needs_claim_exactly_one_producer` |
| Producer failure | proved | `tests/dependency.rs::producer_failure_wakes_every_waiter_and_stays_failed` |
| Consumer cancellation | proved | `tests/cancellation.rs::consumer_cancellation_races_settlement_without_lost_or_double_enqueue` |
| Old/new worlds | proved | `tests/sim_host.rs::worlds_and_result_kinds_name_distinct_cells_and_old_results_stay_stable` |
| Already-ready dependency recorded | proved | `tests/dependency.rs::an_already_ready_read_returns_the_shared_value_and_records_the_edge` |
| Legitimate recursion vs real cycle | proved | `tests/sim_host.rs::mutually_recursive_callables_run_without_waiting_for_initializers`; `a_genuine_wait_cycle_is_reported_and_settles_every_waiter` |
| Migration | proved | `tests/wakeup.rs::parked_frames_survive_collection_and_resume_on_another_worker` |
| Shared identity | proved | `tests/e2e.rs::returned_subtrees_keep_the_input_identity`; `tests/collections.rs` |
| Suspended frames and native references survive GC | proved | `tests/executor_gc.rs::a_worker_requested_collection_sees_running_queued_parked_and_native_roots`; root inventory below |
| Exception/finally across wait | proved | `tests/exception_waits.rs::waits_in_try_catch_and_finally_run_no_handler_while_parked` |
| Native callback wait | proved | `tests/e2e.rs::companion_fold_suspends_in_its_callback_and_completes_after_migrating`; `tests/native_fold.rs` |
| Atom load/replacement safety | proved | `tests/atoms.rs::a_loaded_old_value_survives_replacement_and_collection_until_released`; `readers_see_fully_initialized_published_graphs` |

### Design §15: every measurement item

All rows are **partial** measurements of these harness workloads, rather than
production compiler costs or isolated contention/latency guarantees.
[`examples/bench/workloads.rs`](../examples/bench/workloads.rs) implements them;
[`tests/measurement.rs`](../tests/measurement.rs) asserts small instances.

| Requirement | Status | Harness row/group |
| --- | --- | --- |
| Task startup | partial | `startup.spawn`, `startup.first_run_to_complete` |
| Already-ready reads | partial | `reads.js_need_loop`, `reads.rust_runguard_need`, baseline difference |
| Suspension/wake costs | partial | `park.full_cycle`, `park.complete_and_wake`, `work.publish_to_all_resumed` |
| Shared storage vs task count | partial | `memory`, input/code bytes and identity counts |
| Retained suspended-task memory | partial | `memory`, spawn/park/kept bytes and shallow VM estimates, including native fold |
| Native persistent operations | partial | `persistent`, vector/map reads and updates |
| Useful work during waits | partial | `work.alone`, `work.while_consumers_parked`, still-parked counts |

## Known limitations and next concrete step

The standalone source-driven end-to-end proofs are implemented. Production
compiler integration, full ECMAScript compatibility and incremental invalidation
are **deferred**, not implied by those proofs. GC scheduling is explicit and
leaf work can delay a slice or collection. The e2e host uses controlled placement
and synthetic effects; the benchmarks do not model a real compiler or prove
scaling under contention. The [language contract](grammar.md) records the
remaining API gaps and worker-chosen limits in one place.

The next concrete step is to implement budgeted `JSON.parse` reviver traversal
using the existing native continuation protocol, with callbacks that wait, migrate
and survive collection. This is a scoped extension of a currently explicit API
rejection, not an integration claim. Future work is tracked in Pellets only.

## Root inventory

Every place a heap value can be held so that the collector must keep it
(design.md §11, "The collector must see"), the `RootCategory` it is
reached through, and its retention test (the value survives forced
collections while reachable only through that holder) and reclamation test
(released, it is reclaimed and every handle to it is rejected as stale,
also after its slot is reused). Tests are in `tests/gc_roots.rs` unless
marked *(lib)*: those are in `src/heap/inventory.rs`, for holders the
public API cannot reach.

An explicitly weak dependency completion (`Completion::WeakReady`) is not a
root source for its payload. Its producer roots the payload until consumption;
the consumer's handle or VM operand then owns reachability. A reclaimed weak
outcome reads as cancellation and its cell remains terminal. Ordinary `Ready`
and `Failed` cells remain roots. `weak_completions_transfer_payload_reachability_to_the_consumer`
in `tests/need.rs` forces collection before and after that transfer, and after
release and slot reuse; `weak_completions_keep_publication_checks_and_ordinary_cells_keep_roots`
checks the unchanged publication and ordinary outcome contracts.

| Holder | Root category (via object kind) | Retention | Reclamation |
|--------|---------------------------------|-----------|-------------|
| `Rooted` host handle | `PersistentHandle` (plain object) | `persistent_handle_retains` | `persistent_handle_reclaims` |
| open scope local | `ScopeLocal` | `scope_local_retains` | `scope_local_reclaims` |
| published task result | `PersistentHandle` | `task_result_retains` | `task_result_reclaims` |
| running frames (run parked at a VM safepoint) | `TaskFrame`, reported by the runner | `running_frame_retains` | `running_frame_reclaims` |
| suspended (yielded) frames | `TaskFrame`, task root source | `suspended_frame_retains` | `suspended_frame_reclaims` |
| task global object and its properties | `TaskFrame` (`PlainObject`) | `task_global_object_retains_and_reclaims` | `finished_task_releases_global_object` |
| task global declarative environment | `TaskFrame` (`Cell`) | `task_global_lexicals_survive_job_replacement_and_reclaim` *(lib)* | `task_global_lexicals_survive_job_replacement_and_reclaim` *(lib)* |
| module linking continuation and environment cells | `TaskFrame` (`Cell`) | `module_linker_cells_retain_and_reclaim` (`tests/js_modules.rs`) | `module_linker_cells_retain_and_reclaim` |
| module namespace export cells and their values | `PersistentHandle` (`ModuleNamespace` → `Cell`) | `namespace_cells_retain_and_reclaim` *(lib, module::namespace)* | `namespace_cells_retain_and_reclaim` |
| frames suspended at a pending `__rt.need` | `TaskFrame` | `pending_need_frame_retains` | `pending_need_frame_reclaims` |
| executor ready queue | `TaskFrame` (queued task) | `ready_queue_retains` | `ready_queue_reclaims` |
| executor parked set | `TaskFrame` (parked task) | `executor_parked_task_retains` | `executor_parked_task_reclaims` |
| `finally` return/throw completions (handler stack), also across a dependency wait | `TaskFrame` | `handler_completion_retains` | `handler_completion_reclaims` |
| resumable native frames (`__rt.reduce` accumulator) | `TaskFrame` | `native_frame_retains` | `native_frame_reclaims` |
| transient builder drafts | `TaskFrame` (`Builder`) | `transient_builder_retains` | `transient_builder_reclaims` |
| dependency `Ready` value / `Failed` error | `DependencyOutcome` (`Vector`) | `dependency_outcome_retains` | `dependency_outcome_reclaims_with_its_cell` *(lib)* |
| atom current value | `PersistentHandle` (`Atom`) | `atom_current_value_retains` | `atom_current_value_reclaims` |
| atom auxiliary bookkeeping slot | `PersistentHandle` (`Atom` → `Atom`) | `atom_auxiliary_retains` | `atom_auxiliary_reclaims` |
| atom replaced value held by a reader | `PersistentHandle` (load-to-root) | `atom_retained_old_value_retains` | `atom_retained_old_value_reclaims` |
| closure environment | `PersistentHandle` (`Function`) | `closure_environment_retains` | `closure_environment_reclaims` |
| captured mutable binding | `PersistentHandle` (`Function` → `Cell`) | `binding_cell_retains` | `binding_cell_reclaims` |
| persistent collection edges | `PersistentHandle` (`Map` → `Vector` → `Occurrence`) | `persistent_collection_edges_retain` | `persistent_collection_edges_reclaim` |
| native form edges (elements, map keys and values, metadata) | `PersistentHandle` (`Form` list → vector, map key and value, metadata map, occurrence) | `form_edges_retain` | `form_edges_reclaim` |
| executable object constants | `ExecutableConstant` | `executable_constant_retains` *(lib)* | `executable_constant_reclaims` *(lib)* |
| native payload edges | `PersistentHandle` (`Native`) | `native_payload_edges_retain` *(lib)* | `native_payload_edges_reclaim` *(lib)* |
| ephemeron values, conditional on reached owner and key | `PersistentHandle` (`Native`, conditional storage) | `ephemeron_value_lives_only_with_key` *(lib, heap::gc)* | `ephemeron_value_lives_only_with_key` *(lib, heap::gc)* |
| unconsumed cleared-slot notification owner and held data | `WeakNotification` (`Native`) | `cleared_weak_slots_are_notified_after_the_pause` *(lib, heap::gc)* | `cleared_weak_slots_are_notified_after_the_pause` *(lib, heap::gc)* |
| ordinary object property values (and prototype) | `TaskFrame` (`PlainObject` → `PlainObject` → argument) | `object_property_retains` | `object_property_reclaims` |
| realm intrinsics (`Object.prototype`, built-in functions and their properties) | `Intrinsic` (`PlainObject`, `Builtin`) | `realm_intrinsics_retain` | `realm_intrinsics_reclaim_with_their_heap` *(lib)* |
| realm template-site cache | `Intrinsic` (`Array`, cooked → raw) | `template_cache_retains_and_reclaims` *(lib)* | `template_cache_retains_and_reclaims` *(lib)* |

| JavaScript Map keys and values | `PersistentHandle` (`JsMap` → key and value objects) | `map_entries_are_traced` | `map_entries_are_traced` |
| array elements | `TaskFrame` (`Array` → `Array` → argument) | `array_element_retains` | `array_element_reclaims` |
| function own-property record (clj-quickjs-125) and its values, the lazy `prototype` object among them | `PersistentHandle` (`Function` → `PlainObject` → `PlainObject` → argument) | `function_properties_retain` | `function_properties_reclaim` |
| bound function target, bound receiver, bound arguments and immutable metadata (clj-quickjs-103) | `PersistentHandle` (`BoundFunction` → target closure / receiver / arguments / metadata) | `bound_function_retains` | `bound_function_reclaims` |
| concise method home object (clj-quickjs-126), including literal → method cycles | `PersistentHandle` (`Function` → `PlainObject` → argument) | `method_home_object_retains` | `method_home_object_reclaims` |
| engine Error's original internal cause, after replacing or deleting its own cause property (clj-quickjs-230) | `PersistentHandle` (`PlainObject` → cause) | `engine_error_cause_retains_and_reclaims` *(lib)* | `engine_error_cause_retains_and_reclaims` *(lib)* |

| typed-array backing buffer, ordinary properties and prototype | `PersistentHandle` (`TypedArray` → `ArrayBuffer`, property/prototype objects) | `typed_array_retains_buffer_and_properties` *(tests/gc.rs)* | `typed_array_retains_buffer_and_properties` *(tests/gc.rs)* |
| built-in iterator source internal slot | `PersistentHandle` (`PlainObject` cursor → source array) | `builtin_iterator_source_retains_and_reclaims` *(lib)* | `builtin_iterator_source_retains_and_reclaims` *(lib)* |
| Segments and Segment Iterator internal segmenter slots | `PersistentHandle` (`PlainObject` → Segmenter service) | `segmenter_slots_retain_and_reclaim` *(lib)* | `segmenter_slots_retain_and_reclaim` *(lib)* |
| DisposableStack resources, ordinary properties and prototype | `PersistentHandle` (`DisposableStack`) | `disposable_stack_resources_retain_and_reclaim` *(lib)* | `disposable_stack_resources_retain_and_reclaim` *(lib)* |
| bounded unhandled Promise/reason queue | `TaskFrame` (task inbox, including after completion) | `unhandled_rejection_roots_are_retained_and_reclaimed` *(tests/promises.rs)* | `unhandled_rejection_roots_are_retained_and_reclaimed` *(tests/promises.rs)* |
| WeakRef ordinary properties and prototype | `PersistentHandle` (`WeakRef`) | `weak_ref_properties_and_prototype_are_traced` *(tests/gc.rs)* | `weak_ref_properties_and_prototype_are_traced` *(tests/gc.rs)* |
| task kept objects for the active job | `TaskFrame`, running or suspended task state | `kept_objects_survive_need_wait` *(tests/gc.rs)* | `weak_ref_cleared_after_job_and_collection` *(tests/gc.rs)* |

- **Cross-check.** `src/heap/inventory.rs` maps every `RootCategory` and
  every `ObjectKind` to its holders with exhaustive matches, so a new
  category or kind does not compile until it is mapped, and its test
  checks that every holder's two tests exist and are named in this table.
- **Multi-worker.** `every_holder_retains_while_other_workers_allocate_and_collect`
  keeps every public holder (and a scope local, released while waiting)
  alive while worker 0 runs an allocating loop without a budget and worker
  1 runs allocating tasks and requests every collection; each collection
  must report worker 0 stopped at a VM safepoint mid-run. A collection
  observer checks after each collection that nothing held was reclaimed,
  then every holder is released and everything it held is reclaimed.
  `crate_internal_holders_retain_while_other_workers_allocate_and_collect`
  *(lib)* does the same for object constants and native payload edges.
  `heap::storage::tests::concurrent_allocations_keep_unique_ids_and_correct_objects_across_collections`
  checks four mutators across segment growth, reservation retention and slot
  reuse: no identity repeats, each retained ID resolves to its stamped payload,
  and every reclaimed ID remains stale. The storage lock-exclusion test also
  allocates ordinary and persistent objects while another thread holds the
  table lock. Repeated and ThreadSanitizer results are in `measurement.md`.
- **Not reclaimable while the runtime lives.** A dependency cell is never
  removed, so a published outcome stays rooted until the runtime and every
  task are gone; the reclamation test drops a cell table directly. Likewise
  the realm (clj-quickjs-117) lives as long as its heap; its reclamation
  test drops the realm slot directly.

Additional holders registered by later features (same retention/reclamation
contract; `src/heap/inventory.rs::coverage` is the exhaustive cross-check):

| Holder | Test location | Retention and reclamation tests |
| --- | --- | --- |
| ArrayBuffer properties | `tests/gc_roots.rs` | `array_buffer_properties_retained_and_reclaimed` |
| Promise reactions | `tests/promises.rs` | `pending_promise_reactions_are_traced` |
| Promise results and resolver guards | `tests/promises.rs` | `settled_promise_results_and_resolvers_are_traced`; `thenable_resolving_functions_are_traced` |
| WeakMap/WeakSet conditional entries | `tests/gc.rs` | `weak_map_value_kept_while_key_reachable`; `weak_map_drops_entries_of_unreachable_keys`; `weak_set_keys_are_weak_and_properties_strong` |
| Suspended async activation | `tests/gc.rs` | `test_async_gc_suspended_activation` |
| Suspended generator | `tests/gc.rs` | `suspended_generator_is_traced_and_reclaimed` |
| FinalizationRegistry edges and active held values | `tests/gc.rs` | `finalization_registry_properties_and_callback_are_traced`; `finalization_held_value_is_strong` |
| Proxy and private class elements | `src/heap/inventory.rs` | `proxy_edges_retain_and_reclaim`; `private_elements_retain_and_reclaim` |
| Set strong entries | `tests/gc.rs` | `set_entries_are_traced_and_removed_entries_reclaimed` |

## QuickJS references and attribution

The frontend/VM use QuickJS as a semantic reference, not as a linked engine.
The exact revision, source paths, per-fixture adaptations and complete retained
MIT/Unicode notices are in [attribution.md](attribution.md). That document
supersedes historical per-pellet attribution paragraphs in this file.
