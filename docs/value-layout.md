# Value layout: classes with secret-key protocol slots

Design of light-cljs-1008 (2026-11-14), for the output-speed-parity group
(pl group 1661): every later pellet of the group follows it. It replaces the
runtime's private-field payloads (`payloads`, `privateStore`, `kindedStore`
and the `WeakMap` stores at the top of `src/js_runtime.js`) and its layered
dispatch (`behindProtocol`, `publicMethod`, kind tables) with upstream-style
classes: data in the value's own fields, protocol methods on the prototype,
mask bits. What keeps a runtime's values apart from host objects and from
other runtimes is a secret short string in every property name the runtime
gives a value or a prototype, as the user chose on 2026-11-14.

It is read against research/output-speed/investigation-6ceeb23b/REPORT.md
(the causes and the representation-versus-dispatch prototype),
docs/advanced-optimization.md (Phase 2 builtin types) and upstream's
`cljs/core.cljs` (research/clojurescript). The key measurements come from
the prototype in research/output-speed/value-layout-1008/ (section 1).

## Decisions for the user

No implementation pellet (light-cljs-1009 to 1019) starts before the user
answers these.

1. **Two copies of one build on a page.** With the per-build key that
   section 1 recommends, two loads of the *same* output file share the key,
   so each recognises the other's collections and seqs as its own (and
   operates on them correctly, since their runtime text is identical);
   two *different* builds never do. Keeping two copies of one build apart
   needs a per-load key, which costs about 6 ns (48%) on every megamorphic
   slot read and 3 ns on every protocol test of a host value, so the generic
   operations would stay slower than shadow-cljs's. Today's private fields
   keep two copies apart. **Question: do you accept that two copies of the
   same build on one page recognise each other's values (different builds
   never do), in exchange for a per-build key that measures as fast as
   shadow-cljs's names?** Recommended: yes. Anyone who needs one program
   twice in isolation builds it twice with different `:runtime-key`s
   (section 1).

## The rules in brief

- Every runtime value is an instance of a class of its kind (section 2). Its
  data are own fields; nothing in it is frozen or held in a side table.
- Protocol slots, protocol masks, protocol markers and the kind number are
  properties of the class prototype whose names end in `$<key>`, the
  per-build key (section 1). The value's own fields carry the key too.
- A core operation reads the keyed mask bit or marker and calls the keyed
  slot (section 3). `extend-type` and `specify!` overwrite slots, so no
  operation checks whether a slot was replaced.
- Release, dev builds and the target-side REPL use this layout and nothing
  else. The compile-time runtime (macro host, evaluator) uses it too and
  adds a per-load brand for the compiler form bridge; the Pants build keeps
  its engine forms for the kinds it swaps today (section 4).
- Deliberate reflection (reading the prototype's keys, `Object.create` of a
  runtime prototype, copying fields onto one) can forge a value. Nothing an
  accident can produce does (section 5).
- Each conversion deletes its kind's payload stores in the same pellet; the
  types not yet converted keep today's path (section 6).

## 1. The secret key

### What carries it

Every property name that recognises or dispatches a value carries the key as
a suffix:

- protocol method slots, `cljs$core$ICounted$_count$arity$1$<key>`, for the
  runtime's classes, program types, `reify`, `extend-type`, `extend-protocol`
  and `specify!` alike;
- protocol markers, `cljs$core$ISeqable$<key>`;
- the two mask properties, `cljs$lang$protocol_mask$partition0$<key>` and
  `...partition1$<key>`;
- the kind number, `cljs$lang$kind$<key>` (section 2);
- the runtime classes' own data fields, `cnt$<key>`, `tail$<key>`.

Not keyed: the functions' arity convention (`cljs$core$IFn$_invoke$arity$N`,
`cljs$lang$maxFixedArity`, `cljs$lang$applyTo`), which host callers and
other runtimes call through `.call` anyway, and the `IFn` invoke entries and
`call`/`apply` of a type implementing `IFn` (its `IFn` marker and mask bit
are keyed); `Object` protocol members (`toString`, `equiv`, `indexOf`);
well-known symbols (`Symbol.iterator`); and a program type's or record's own
fields, which keep their munged source names (`(.-x p)`), and a record's
`__meta`, `__extmap` and `__hash`.

The upstream field names of the runtime's types (`(.-cnt v)`, `(.-tail v)`,
`(.-root v)`) stay the public accessors they are today (`publicFields`): an
accessor on the prototype that reads the keyed field, so a read answers as
today and a write to an immutable field still makes a shadow property rather
than changing the value.

Source that names a keyed member, `(.-cljs$lang$protocol_mask$partition0$ x)`,
`(.cljs$core$ILookup$_lookup$arity$2 m k)` or a marker, is keyed by the
compiler at every level (the analyzer recognises the three shapes, with the
protocol's namespace, as it munges members today), so interop that names
slots keeps working. A name built at run time (`(aget x "cljs$core$...")`,
`goog.object/get`) is not keyed and sees nothing.

### Measured

`node run.mjs 7 21` in research/output-speed/value-layout-1008/ (`gen.mjs`
generates one runtime-like script per variant: the same eleven classes, the
same generic operations, the same benches; `bench.mjs` measures one bench in
a fresh process, so each bench's inline caches see only its own receivers;
`run.mjs` alternates the variants). Node v26.12.1 (V8), 24 CPUs, load
average 3.6-5.1 (unreserved); ns per operation, median of 7 rounds, range in
brackets (`results-key.json`, `table-key.md`).

- **U**: upstream names, dotted, as shadow-cljs's output has them (the
  baseline).
- **S**: the per-build string, dotted: `o.cljs$core$ICounted$_count$arity$1$q7Kx()`.
- **Y**: a per-load `Symbol` per slot, computed: `o[s_count1]()`.
- **L**: a per-load random string per slot, computed (the constant built at
  load).
- **YF**: Y with the fields also per-load `Symbol`s.

| op | U | S | Y | L | YF |
|---|---:|---:|---:|---:|---:|
| count, one class (monomorphic) | 4.3 [4.3-4.4] | 4.3 [4.3-4.4] | 4.3 [0.8-4.5] | 4.3 [4.3-4.4] | 4.3 [4.2-4.4] |
| count, four classes (polymorphic) | 3.3 [3.3-3.3] | 3.3 [3.3-3.3] | 3.3 [3.3-3.3] | 3.3 [3.2-3.3] | 3.3 [3.3-3.3] |
| count, ten classes (megamorphic) | 13.3 [13.0-13.4] | 13.1 [13.0-13.5] | 19.6 [19.5-19.9] | 19.6 [19.5-20.0] | 19.4 [19.3-19.5] |
| get of a keyword, small map | 8.9 | 8.9 | 8.9 | 8.9 | 9.0 |
| direct slot call (light-cljs-1016's shape) | 8.8 | 8.8 | 8.8 | 8.8 | 8.8 |
| assoc, small map | 22.4 | 22.5 | 22.4 | 22.4 | 22.4 |
| 64 conj onto a vector | 1387 [1348-1511] | 1456 [1371-1500] | 1464 [1370-1522] | 1471 [1379-1499] | 1470 [1372-1494] |
| seq walk, vector of 1000 | 7577 | 7503 | 7506 | 7515 | 7549 |
| seq walk, four seq kinds | 10027 [8077-10344] | 9935 [9848-9996] | 10602 [10337-10705] | 10640 [9707-10817] | 10670 [10498-10830] |
| = of two 10-item vectors | 36.4 | 36.4 | 36.4 | 36.5 | 36.4 |
| satisfies? ICounted of host values (miss) | 9.3 [9.2-9.5] | 9.3 [9.2-9.5] | 12.3 [12.2-12.7] | 12.6 [12.6-12.9] | 12.4 [12.3-12.6] |
| startup: load and first call of 400 functions (ms, 21 runs) | 7.64 [6.69-7.97] | 7.52 [7.18-7.81] | 7.61 [6.99-7.82] | 7.65 [7.16-7.80] | 7.67 [7.06-7.85] |
| emitted-program text, bytes / gzip | 352,448 / 6,145 | 378,698 / 6,273 | 213,098 / 5,397 | 213,098 / 5,397 | 213,098 / 5,397 |

A second run (`node run.mjs 5 11 results-fields.json U,S,SF`, load
4.4-5.0) added **SF**, the per-build string with `Symbol`-keyed fields: equal
to S everywhere except the four-kind seq walk, 10,860 [10,742-10,983] against
S's 10,011 (+8.5%).

What the numbers say:

- A per-build string costs nothing: S is U within noise everywhere. Dotted
  names are what V8's named inline caches and its megamorphic stub cache are
  built for, whatever the name.
- A computed key costs nothing where a site sees one to four classes, and
  6.3 ns (48%) where it sees more, which is exactly the generic operations'
  case (`count`, `first`, `next`, `get` in a real program see every
  collection and seq kind), plus 3 ns on every protocol test of a host value.
  The cost is the keyed access, not the `Symbol`: the per-load string L costs
  the same. Megamorphic keyed loads take V8's generic keyed path where named
  loads take the stub cache directly.
- Startup does not separate them: building the prototypes and compiling the
  call sites costs the same in every variant.
- A computed key's text is smaller (a local name per slot, which minifies),
  13% less gzip on the emitted part. A per-build suffix adds 2% gzip there.
  Size is recorded, not decisive (group rules); light-cljs-1016 may later
  give `:advanced` short keyed names from a per-build table.

### Choice: a per-build string, dotted

The key is a per-build string: 5 characters of `[0-9a-z]`, appended as
`$<key>`. It is the user's choice and the fast one; a per-load key would
leave the generic operations 48% above shadow-cljs's at their megamorphic
sites, which the group's goal forbids. Switching to a `Symbol` is not
recommended. Fields use string names too (SF's seq walk).

**Derivation.** `key = base36(hash(runtime text of the output's slice,
optimization level, build identity, :runtime-key))[0..5]`, where the build
identity is the build id (light-cljs.edn) or else the entry namespaces, and
`:runtime-key` is an optional build option (any string; default none). It is
deterministic, so the same inputs give byte-identical output on every
machine, and it differs between builds whose runtime slices differ, so two
builds that would lay out or select different slots never share a key: a
release build and a dev build of one app, two versions of one app with
different slices, two apps. The runtime text uses a placeholder suffix
(`$LKEY`, which `src/runtime_manifest.rs` checks appears only as a keyed
name's suffix), which embedding replaces; the raw `src/js_runtime.js` that
tests evaluate runs with the literal placeholder as its key. Compiled
namespace artifacts in the compile cache carry the emitter's placeholder for
the key and the output writer substitutes it (light-cljs-1009 picks a
placeholder that no emitted string literal can contain, for example one
written only through an escape the emitter always applies), so the key is
not part of a namespace's cache key.

**Dev builds, the REPL and hot reload.** A dev build's slice is the whole dev
runtime, and its key depends on neither the sources nor the edit, so every
recompiled namespace, every hot reload and every form the REPL compiles
against the running page uses the key the page loaded with; the dev session
keeps it with the build. A change to the runtime text (a light-cljs upgrade)
changes the key, and a page with the old runtime then needs a full reload,
which it needs anyway.

### Two runtimes on one page

- **Two different builds** (including a dev and a release build of one app):
  different keys. Neither recognises the other's values: to each, the
  other's vector is a host object (`vector?` false, `count` throws as for
  `{}`, printing shows a host object). An `extend-type` of a host constructor
  (`js/Date`, `js/Object`) in one writes keyed names the other never reads,
  where upstream's would clash. `extend-type` of `object`, `default` or
  `number` goes to each runtime's own tables, as today. What they share is
  only the page: globals the program itself writes.
- **The same build loaded twice** (the question under "Decisions for the
  user"): the same key and the same runtime text, so each recognises the
  other's values and calls their slots, which are the other copy's methods
  over the other copy's fields, and answers correctly. State stays per copy:
  atoms, the keyword intern table (keywords from the two copies are equal,
  never identical: equality is by keyed kind and `fqn`, never `instanceof`,
  section 3), protocol extensions of each copy's own classes, `*print-fn*`
  and the rest. An `extend-type` of a host constructor is shared by name,
  and both copies install the same code. A direct field read of another
  value is guarded by `instanceof` of this copy's class, never by the kind
  alone, so a copy only reaches the other's fields through the other's
  slots.
- **Compile time**: each compile-time runtime has a fixed key and a per-load
  brand (section 4); it never shares a realm with output.

light-cljs-1018 tests these statements in Node and headless Chromium and
records the final guarantee here.

## 2. The classes

## 3. Dispatch

## 4. Profiles

| profile | layout | key | brand |
|---|---|---|---|
| release (`:advanced` and the other levels' output) | classes | per build | none |
| dev builds (and the target-side REPL, hot reload) | classes | per build, stable across reloads | none |
| macro host, compile-time runtime (V8 build) | classes | fixed (`$LKEY`) | per-load private field |
| evaluator: `--eval`, `--repl`, folding (V8 build) | classes | fixed | per-load private field |
| QuickJS reference (`lightCljsWeakMapStores`) | classes | fixed | per-load `WeakSet` |
| Pants build, shared-heap profile (`src/js_shared_profile.js`) | engine forms for the kinds it swaps; classes for the kinds it inherits | fixed | engine forms; per-load brand for inherited classes |

**Release and dev builds** adopt the class layout and nothing else: after
light-cljs-1015 no payload store, kind table, `behindProtocol`,
`publicMethod` or brand class is in their units. Release is the gate.

**The compile-time runtime** (the macro host's `compilerFormBridge` and the
evaluator, which load the same factory with the `compile-time` units) adopts
the class layout too: keeping two layouts would keep every slow path alive
and double every collection's code, and compile speed matters as much as
output speed. It needs one thing release does not: the form bridge must tell
a runtime value from an arbitrary host value without reading any property of
the host value, so that rejecting a macro result never runs a getter,
`toString` or a Proxy trap (docs/evaluator.md, kept unchanged, and
`macros::rejection_never_calls_arbitrary_getters_or_to_string`). A keyed
read is a property read: a Proxy's `get` trap runs for it. So in the
compile-time runtime each runtime class's constructor stamps its instance
with a per-load brand:

- every runtime class constructor ends with a marker comment
  (`/*@brand*/`) that the release and dev embeddings drop and that
  `compiler_runtime` (build.rs), which already inserts the compile-time
  hooks, replaces with `stamp(this);`;
- `stamp` is a compile-time unit: a return-override class adding one private
  field in V8 (the cost today's stores pay, about 9 ns per value, so no
  regression for the macro host), a `WeakSet` add where the host states
  `lightCljsWeakMapStores` (QuickJS, where private fields cost more than
  entries, light-cljs-292);
- the bridge, `inspect` and the compile-time printer test `stamped(value)`
  first, which reads no property, and only then the keyed kind.

**The one emitter.** Program types already have the switch: `TypeBrands`
(src/emitter.rs). Release and dev emission use `TypeBrands::Unread`, no
brand, at every level, and `defineType`/`makeType` set the keyed kind (type,
record or reify) on the prototype, which `record?`, `map?` of a record and
the other record tests read instead of `TypeBrand.type`. Compile-time
emission keeps `Shared` or `Own` by today's rules, so `TypeBrand`,
`ClassBrand` and `defineClass` stay, as compile-time units, with their tests
(section 5). `makeType`'s constructor, which dev builds and the lower levels
use, carries the `/*@brand*/` marker in place of its `new TypeBrand(...)`.

**The Pants build** keeps its engine forms (`__rt` intrinsics) for the kinds
`src/js_shared_profile.js` swaps today (vectors and map entries, maps, sets,
keywords, symbols, list cells), and takes the class layout for every kind it
inherits from the V8 profile. Its boot seals the prototypes, so `extend-type`
or `specify!` of a runtime kind keeps throwing there, as it does today
(`sharedProfileBoot`, until light-cljs-649). A generic operation's
slot-first path finds no slot on an engine form and falls through to the
host-value fallback (section 3), where the profile's units answer engine
forms; so a converted operation keeps a hook (as `kindHooks` are today) that
only `src/js_shared_profile.js` fills. Where a conversion changes an
interface a swapped unit or one of build.rs's pattern rewrites
(`shared_owned_allocations`, `shared_persistent_updates`,
`shared_scalar_reads`) relies on, the converting pellet changes them in the
same commit; an old-layout unit the profile still needs moves into
`src/js_shared_profile.js` rather than staying in `src/js_runtime.js`. Every
conversion runs `cargo nextest run --locked --no-default-features --features
bbp-macros --test bbp_macros`.

## 5. Tests

The rule: a test that a value no accident can produce is no language value
stays ("never by accident"). A test that pins what happens to a value made
by deliberate reflection over the runtime's prototypes or keys, which the
user gave up, is rescoped. Here *deliberate reflection* is
`Object.create` of a runtime prototype (or of a value), `Object.setPrototypeOf`
to one, copying a value's own properties onto a runtime prototype, reading
the prototype's keys (`Object.getOwnPropertyNames`/`Symbols`) and writing a
keyed name. Each rescoping lands in the pellet that converts the kind, with
its line in the commit message.

### Stay

- `builtin_types::value_kind_predicates_answer_as_the_brand_stores_did`: the
  rows of runtime values, host values (`string` to `fn`) and the Proxy, and
  the pollution loop, extended with the unkeyed upstream names
  (`cljs$lang$protocol_mask$partition0$`, `cljs$core$ICounted$`,
  `cljs$core$ICounted$_count$arity$1`, `cljs$lang$kind# Value layout: classes with secret-key protocol slots

Design of light-cljs-1008 (2026-11-14), for the output-speed-parity group
(pl group 1661): every later pellet of the group follows it. It replaces the
runtime's private-field payloads (`payloads`, `privateStore`, `kindedStore`
and the `WeakMap` stores at the top of `src/js_runtime.js`) and its layered
dispatch (`behindProtocol`, `publicMethod`, kind tables) with upstream-style
classes: data in the value's own fields, protocol methods on the prototype,
mask bits. What keeps a runtime's values apart from host objects and from
other runtimes is a secret short string in every property name the runtime
gives a value or a prototype, as the user chose on 2026-11-14.

It is read against research/output-speed/investigation-6ceeb23b/REPORT.md
(the causes and the representation-versus-dispatch prototype),
docs/advanced-optimization.md (Phase 2 builtin types) and upstream's
`cljs/core.cljs` (research/clojurescript). The key measurements come from
the prototype in research/output-speed/value-layout-1008/ (section 1).

## Decisions for the user

No implementation pellet (light-cljs-1009 to 1019) starts before the user
answers these.

1. **Two copies of one build on a page.** With the per-build key that
   section 1 recommends, two loads of the *same* output file share the key,
   so each recognises the other's collections and seqs as its own (and
   operates on them correctly, since their runtime text is identical);
   two *different* builds never do. Keeping two copies of one build apart
   needs a per-load key, which costs about 6 ns (48%) on every megamorphic
   slot read and 3 ns on every protocol test of a host value, so the generic
   operations would stay slower than shadow-cljs's. Today's private fields
   keep two copies apart. **Question: do you accept that two copies of the
   same build on one page recognise each other's values (different builds
   never do), in exchange for a per-build key that measures as fast as
   shadow-cljs's names?** Recommended: yes. Anyone who needs one program
   twice in isolation builds it twice with different `:runtime-key`s
   (section 1).

## The rules in brief

- Every runtime value is an instance of a class of its kind (section 2). Its
  data are own fields; nothing in it is frozen or held in a side table.
- Protocol slots, protocol masks, protocol markers and the kind number are
  properties of the class prototype whose names end in `$<key>`, the
  per-build key (section 1). The value's own fields carry the key too.
- A core operation reads the keyed mask bit or marker and calls the keyed
  slot (section 3). `extend-type` and `specify!` overwrite slots, so no
  operation checks whether a slot was replaced.
- Release, dev builds and the target-side REPL use this layout and nothing
  else. The compile-time runtime (macro host, evaluator) uses it too and
  adds a per-load brand for the compiler form bridge; the Pants build keeps
  its engine forms for the kinds it swaps today (section 4).
- Deliberate reflection (reading the prototype's keys, `Object.create` of a
  runtime prototype, copying fields onto one) can forge a value. Nothing an
  accident can produce does (section 5).
- Each conversion deletes its kind's payload stores in the same pellet; the
  types not yet converted keep today's path (section 6).

## 1. The secret key

### What carries it

Every property name that recognises or dispatches a value carries the key as
a suffix:

- protocol method slots, `cljs$core$ICounted$_count$arity$1$<key>`, for the
  runtime's classes, program types, `reify`, `extend-type`, `extend-protocol`
  and `specify!` alike;
- protocol markers, `cljs$core$ISeqable$<key>`;
- the two mask properties, `cljs$lang$protocol_mask$partition0$<key>` and
  `...partition1$<key>`;
- the kind number, `cljs$lang$kind$<key>` (section 2);
- the runtime classes' own data fields, `cnt$<key>`, `tail$<key>`.

Not keyed: the functions' arity convention (`cljs$core$IFn$_invoke$arity$N`,
`cljs$lang$maxFixedArity`, `cljs$lang$applyTo`), which host callers and
other runtimes call through `.call` anyway, and the `IFn` invoke entries and
`call`/`apply` of a type implementing `IFn` (its `IFn` marker and mask bit
are keyed); `Object` protocol members (`toString`, `equiv`, `indexOf`);
well-known symbols (`Symbol.iterator`); and a program type's or record's own
fields, which keep their munged source names (`(.-x p)`), and a record's
`__meta`, `__extmap` and `__hash`.

The upstream field names of the runtime's types (`(.-cnt v)`, `(.-tail v)`,
`(.-root v)`) stay the public accessors they are today (`publicFields`): an
accessor on the prototype that reads the keyed field, so a read answers as
today and a write to an immutable field still makes a shadow property rather
than changing the value.

Source that names a keyed member, `(.-cljs$lang$protocol_mask$partition0$ x)`,
`(.cljs$core$ILookup$_lookup$arity$2 m k)` or a marker, is keyed by the
compiler at every level (the analyzer recognises the three shapes, with the
protocol's namespace, as it munges members today), so interop that names
slots keeps working. A name built at run time (`(aget x "cljs$core$...")`,
`goog.object/get`) is not keyed and sees nothing.

### Measured

`node run.mjs 7 21` in research/output-speed/value-layout-1008/ (`gen.mjs`
generates one runtime-like script per variant: the same eleven classes, the
same generic operations, the same benches; `bench.mjs` measures one bench in
a fresh process, so each bench's inline caches see only its own receivers;
`run.mjs` alternates the variants). Node v26.12.1 (V8), 24 CPUs, load
average 3.6-5.1 (unreserved); ns per operation, median of 7 rounds, range in
brackets (`results-key.json`, `table-key.md`).

- **U**: upstream names, dotted, as shadow-cljs's output has them (the
  baseline).
- **S**: the per-build string, dotted: `o.cljs$core$ICounted$_count$arity$1$q7Kx()`.
- **Y**: a per-load `Symbol` per slot, computed: `o[s_count1]()`.
- **L**: a per-load random string per slot, computed (the constant built at
  load).
- **YF**: Y with the fields also per-load `Symbol`s.

| op | U | S | Y | L | YF |
|---|---:|---:|---:|---:|---:|
| count, one class (monomorphic) | 4.3 [4.3-4.4] | 4.3 [4.3-4.4] | 4.3 [0.8-4.5] | 4.3 [4.3-4.4] | 4.3 [4.2-4.4] |
| count, four classes (polymorphic) | 3.3 [3.3-3.3] | 3.3 [3.3-3.3] | 3.3 [3.3-3.3] | 3.3 [3.2-3.3] | 3.3 [3.3-3.3] |
| count, ten classes (megamorphic) | 13.3 [13.0-13.4] | 13.1 [13.0-13.5] | 19.6 [19.5-19.9] | 19.6 [19.5-20.0] | 19.4 [19.3-19.5] |
| get of a keyword, small map | 8.9 | 8.9 | 8.9 | 8.9 | 9.0 |
| direct slot call (light-cljs-1016's shape) | 8.8 | 8.8 | 8.8 | 8.8 | 8.8 |
| assoc, small map | 22.4 | 22.5 | 22.4 | 22.4 | 22.4 |
| 64 conj onto a vector | 1387 [1348-1511] | 1456 [1371-1500] | 1464 [1370-1522] | 1471 [1379-1499] | 1470 [1372-1494] |
| seq walk, vector of 1000 | 7577 | 7503 | 7506 | 7515 | 7549 |
| seq walk, four seq kinds | 10027 [8077-10344] | 9935 [9848-9996] | 10602 [10337-10705] | 10640 [9707-10817] | 10670 [10498-10830] |
| = of two 10-item vectors | 36.4 | 36.4 | 36.4 | 36.5 | 36.4 |
| satisfies? ICounted of host values (miss) | 9.3 [9.2-9.5] | 9.3 [9.2-9.5] | 12.3 [12.2-12.7] | 12.6 [12.6-12.9] | 12.4 [12.3-12.6] |
| startup: load and first call of 400 functions (ms, 21 runs) | 7.64 [6.69-7.97] | 7.52 [7.18-7.81] | 7.61 [6.99-7.82] | 7.65 [7.16-7.80] | 7.67 [7.06-7.85] |
| emitted-program text, bytes / gzip | 352,448 / 6,145 | 378,698 / 6,273 | 213,098 / 5,397 | 213,098 / 5,397 | 213,098 / 5,397 |

A second run (`node run.mjs 5 11 results-fields.json U,S,SF`, load
4.4-5.0) added **SF**, the per-build string with `Symbol`-keyed fields: equal
to S everywhere except the four-kind seq walk, 10,860 [10,742-10,983] against
S's 10,011 (+8.5%).

What the numbers say:

- A per-build string costs nothing: S is U within noise everywhere. Dotted
  names are what V8's named inline caches and its megamorphic stub cache are
  built for, whatever the name.
- A computed key costs nothing where a site sees one to four classes, and
  6.3 ns (48%) where it sees more, which is exactly the generic operations'
  case (`count`, `first`, `next`, `get` in a real program see every
  collection and seq kind), plus 3 ns on every protocol test of a host value.
  The cost is the keyed access, not the `Symbol`: the per-load string L costs
  the same. Megamorphic keyed loads take V8's generic keyed path where named
  loads take the stub cache directly.
- Startup does not separate them: building the prototypes and compiling the
  call sites costs the same in every variant.
- A computed key's text is smaller (a local name per slot, which minifies),
  13% less gzip on the emitted part. A per-build suffix adds 2% gzip there.
  Size is recorded, not decisive (group rules); light-cljs-1016 may later
  give `:advanced` short keyed names from a per-build table.

### Choice: a per-build string, dotted

The key is a per-build string: 5 characters of `[0-9a-z]`, appended as
`$<key>`. It is the user's choice and the fast one; a per-load key would
leave the generic operations 48% above shadow-cljs's at their megamorphic
sites, which the group's goal forbids. Switching to a `Symbol` is not
recommended. Fields use string names too (SF's seq walk).

**Derivation.** `key = base36(hash(runtime text of the output's slice,
optimization level, build identity, :runtime-key))[0..5]`, where the build
identity is the build id (light-cljs.edn) or else the entry namespaces, and
`:runtime-key` is an optional build option (any string; default none). It is
deterministic, so the same inputs give byte-identical output on every
machine, and it differs between builds whose runtime slices differ, so two
builds that would lay out or select different slots never share a key: a
release build and a dev build of one app, two versions of one app with
different slices, two apps. The runtime text uses a placeholder suffix
(`$LKEY`, which `src/runtime_manifest.rs` checks appears only as a keyed
name's suffix), which embedding replaces; the raw `src/js_runtime.js` that
tests evaluate runs with the literal placeholder as its key. Compiled
namespace artifacts in the compile cache carry the emitter's placeholder for
the key and the output writer substitutes it (light-cljs-1009 picks a
placeholder that no emitted string literal can contain, for example one
written only through an escape the emitter always applies), so the key is
not part of a namespace's cache key.

**Dev builds, the REPL and hot reload.** A dev build's slice is the whole dev
runtime, and its key depends on neither the sources nor the edit, so every
recompiled namespace, every hot reload and every form the REPL compiles
against the running page uses the key the page loaded with; the dev session
keeps it with the build. A change to the runtime text (a light-cljs upgrade)
changes the key, and a page with the old runtime then needs a full reload,
which it needs anyway.

### Two runtimes on one page

- **Two different builds** (including a dev and a release build of one app):
  different keys. Neither recognises the other's values: to each, the
  other's vector is a host object (`vector?` false, `count` throws as for
  `{}`, printing shows a host object). An `extend-type` of a host constructor
  (`js/Date`, `js/Object`) in one writes keyed names the other never reads,
  where upstream's would clash. `extend-type` of `object`, `default` or
  `number` goes to each runtime's own tables, as today. What they share is
  only the page: globals the program itself writes.
- **The same build loaded twice** (the question under "Decisions for the
  user"): the same key and the same runtime text, so each recognises the
  other's values and calls their slots, which are the other copy's methods
  over the other copy's fields, and answers correctly. State stays per copy:
  atoms, the keyword intern table (keywords from the two copies are equal,
  never identical: equality is by keyed kind and `fqn`, never `instanceof`,
  section 3), protocol extensions of each copy's own classes, `*print-fn*`
  and the rest. An `extend-type` of a host constructor is shared by name,
  and both copies install the same code. A direct field read of another
  value is guarded by `instanceof` of this copy's class, never by the kind
  alone, so a copy only reaches the other's fields through the other's
  slots.
- **Compile time**: each compile-time runtime has a fixed key and a per-load
  brand (section 4); it never shares a realm with output.

light-cljs-1018 tests these statements in Node and headless Chromium and
records the final guarantee here.

## 2. The classes

## 3. Dispatch

## 4. Profiles

| profile | layout | key | brand |
|---|---|---|---|
| release (`:advanced` and the other levels' output) | classes | per build | none |
| dev builds (and the target-side REPL, hot reload) | classes | per build, stable across reloads | none |
| macro host, compile-time runtime (V8 build) | classes | fixed (`$LKEY`) | per-load private field |
| evaluator: `--eval`, `--repl`, folding (V8 build) | classes | fixed | per-load private field |
| QuickJS reference (`lightCljsWeakMapStores`) | classes | fixed | per-load `WeakSet` |
| Pants build, shared-heap profile (`src/js_shared_profile.js`) | engine forms for the kinds it swaps; classes for the kinds it inherits | fixed | engine forms; per-load brand for inherited classes |

**Release and dev builds** adopt the class layout and nothing else: after
light-cljs-1015 no payload store, kind table, `behindProtocol`,
`publicMethod` or brand class is in their units. Release is the gate.

**The compile-time runtime** (the macro host's `compilerFormBridge` and the
evaluator, which load the same factory with the `compile-time` units) adopts
the class layout too: keeping two layouts would keep every slow path alive
and double every collection's code, and compile speed matters as much as
output speed. It needs one thing release does not: the form bridge must tell
a runtime value from an arbitrary host value without reading any property of
the host value, so that rejecting a macro result never runs a getter,
`toString` or a Proxy trap (docs/evaluator.md, kept unchanged, and
`macros::rejection_never_calls_arbitrary_getters_or_to_string`). A keyed
read is a property read: a Proxy's `get` trap runs for it. So in the
compile-time runtime each runtime class's constructor stamps its instance
with a per-load brand:

- every runtime class constructor ends with a marker comment
  (`/*@brand*/`) that the release and dev embeddings drop and that
  `compiler_runtime` (build.rs), which already inserts the compile-time
  hooks, replaces with `stamp(this);`;
- `stamp` is a compile-time unit: a return-override class adding one private
  field in V8 (the cost today's stores pay, about 9 ns per value, so no
  regression for the macro host), a `WeakSet` add where the host states
  `lightCljsWeakMapStores` (QuickJS, where private fields cost more than
  entries, light-cljs-292);
- the bridge, `inspect` and the compile-time printer test `stamped(value)`
  first, which reads no property, and only then the keyed kind.

**The one emitter.** Program types already have the switch: `TypeBrands`
(src/emitter.rs). Release and dev emission use `TypeBrands::Unread`, no
brand, at every level, and `defineType`/`makeType` set the keyed kind (type,
record or reify) on the prototype, which `record?`, `map?` of a record and
the other record tests read instead of `TypeBrand.type`. Compile-time
emission keeps `Shared` or `Own` by today's rules, so `TypeBrand`,
`ClassBrand` and `defineClass` stay, as compile-time units, with their tests
(section 5). `makeType`'s constructor, which dev builds and the lower levels
use, carries the `/*@brand*/` marker in place of its `new TypeBrand(...)`.

**The Pants build** keeps its engine forms (`__rt` intrinsics) for the kinds
`src/js_shared_profile.js` swaps today (vectors and map entries, maps, sets,
keywords, symbols, list cells), and takes the class layout for every kind it
inherits from the V8 profile. Its boot seals the prototypes, so `extend-type`
or `specify!` of a runtime kind keeps throwing there, as it does today
(`sharedProfileBoot`, until light-cljs-649). A generic operation's
slot-first path finds no slot on an engine form and falls through to the
host-value fallback (section 3), where the profile's units answer engine
forms; so a converted operation keeps a hook (as `kindHooks` are today) that
only `src/js_shared_profile.js` fills. Where a conversion changes an
interface a swapped unit or one of build.rs's pattern rewrites
(`shared_owned_allocations`, `shared_persistent_updates`,
`shared_scalar_reads`) relies on, the converting pellet changes them in the
same commit; an old-layout unit the profile still needs moves into
`src/js_shared_profile.js` rather than staying in `src/js_runtime.js`. Every
conversion runs `cargo nextest run --locked --no-default-features --features
bbp-macros --test bbp_macros`.

## 5. Tests

The rule: a test that a value no accident can produce is no language value
stays ("never by accident"). A test that pins what happens to a value made
by deliberate reflection over the runtime's prototypes or keys, which the
user gave up, is rescoped. Here *deliberate reflection* is
`Object.create` of a runtime prototype (or of a value), `Object.setPrototypeOf`
to one, copying a value's own properties onto a runtime prototype, reading
the prototype's keys (`Object.getOwnPropertyNames`/`Symbols`) and writing a
keyed name. Each rescoping lands in the pellet that converts the kind, with
its line in the commit message.

### Stay

- `builtin_types::value_kind_predicates_answer_as_the_brand_stores_did`: the
  rows of runtime values, host values (`string` to `fn`) and the Proxy, and
  the pollution loop, extended with the unkeyed upstream names
  (`cljs$lang$protocol_mask$partition0$`, `cljs$core$ICounted$`,
) defined on
  `Object.prototype`: a host that loaded upstream ClojureScript, or
  another light-cljs build, must change no answer.
- `builtin_types::protocol_hooks_answer_runtime_values_as_the_full_lookup`:
  the rows of runtime, host and `specify!`-ed values and the Proxy row, for
  both extension sets.
- `macros::rejection_never_calls_arbitrary_getters_or_to_string`, all four
  cases, unchanged: the compile-time brand keeps it (section 4).
- `bbp_macros::shared_profile::shared_profile_class_brands_are_task_local`,
  unchanged: `defineClass`, `ClassBrand` and `TypeBrand` stay compile-time
  units with task-local flags.
- tests/support/execution.js:71, the harness rule "never recognize language
  values by a public property": a keyed name is not public (no host code
  writes it by accident), so the harness keeps it as written.
- In tests/support/js_runtime.test.js: the host-object checks (`host`, `{...host}`,
  `[10, 20, 30]`, `{val: v}`, `{}`, arrays, functions, `Object.create(null)`),
  the `TypeBrand.type` loop at 1410-1414 (it tests the compile-time unit
  itself), the copies of a record's fields onto `{}` (1377: own fields
  without the prototype have no kind), and "a pipeline writes no WeakMap
  entry" (1592-1600), which the class layout keeps trivially.

### Rescoped to deliberate reflection

Each item is removed, or keeps only its host-object candidates; the reason
is the same for all, so the line says what each forges.

`builtin_types.rs`:

1. `value_kind_predicates...`: the 16 `forged-*` and `copied-*` rows
   (`Object.create` of the value's prototype, alone or with its fields): a
   value made from a runtime prototype now type-tests as its kind.
2. `value_kind_predicates...`: the Proxy line keeps its rule (only keyed
   names, which the test filters as `get:cljs$`, well-known symbols and the
   prototype are asked), and its expected text is updated to what the class
   layout asks; it may lose `proto`, since a record is told by its keyed kind
   rather than by walking the prototype chain for a brand.
3. `protocol_hooks...`: the `forged-*` rows, both extension sets.
4. `protocol_hooks...`: the expected list of the Proxy's reads: the same
   kinds of names, now keyed (`get:cljs$core$ICounted$<key>`); the key is
   normalised to `<key>` before comparing. A rename, not a semantic change.

tests/support/js_runtime.test.js (line numbers at master 6ceeb23b):

5. 183: `inspect` of `Object.create` of a keyword's prototype.
6. 184: the same of a vector's prototype.
7. 272: the same of each list, seq and cons value (the loop's
   `Object.create` line; its frozen-prototype lines stay, see below).
8. 415: the same of a hash map's prototype.
9. 607: the same of a hash set's prototype.
10. 784: `isReduced` of `Object.create` of a reduced's prototype.
11. 800-802: writing `val` on, and `isReduced` of, the same forged reduced.
12. 1376: `inspect` of `Object.create(Record.prototype)`.
13. 1378: a record's fields copied onto `Object.create(Record.prototype)`.
14. 1379-1381 (a record whose prototype is changed keeps its brand): a
    record moved to `Object.prototype` (`Object.setPrototypeOf`) is a host
    object to release and dev builds.
15. 1416: `inspect(Object.create(Record.prototype))` after the brand loop.
16. 1539: `conjBang` of `Object.create` of a transient's prototype.
17. 1579-1581: writing a public field on, and `inspect` of,
    `Object.create(Type.prototype)` for the 14 types listed: a forged value
    is accepted by the field accessor.
18. 1605-1610: the forged and copied candidates of lazy, chunked, chunk,
    vector seq, cons and list values.
19. 1674-1679: the forged and copied candidates of the fresh runtime's
    built values.
20. 1736-1741: the same in the `lightCljsWeakMapStores` runtime; the
    section's heading claim ("keeps its payloads in WeakMaps") becomes "a
    host that states WeakMap stores brands compile-time values with a
    WeakSet" when its kinds convert.
21. 1779-1809 (kind tables are private): removed with the kind tables
    (light-cljs-1015); every candidate holds a kind under a key read by
    reflection (`Object.getOwnPropertySymbols` of a prototype).
22. 1810-1838 (vectors', maps', sets' and queues' kind numbers): the same.

What stays of their neighbours: the frozen-shared-base checks
(`Object.isFrozen(Object.getPrototypeOf(Object.getPrototypeOf(value)))`,
`Object.getPrototypeOf(m0).call = ...` throws), because the class prototypes
keep inheriting from today's frozen bases that hold `call`, `equiv`,
`toString`, `indexOf` and `lastIndexOf` (section 3).

Not counted as forgery but changed by the layout, and therefore listed:
`Object.keys`, `for...in`, `JSON.stringify` and spreading of a runtime
value now list its keyed fields, as upstream ClojureScript's list its fields
(today they list nothing). `Symbol`-keyed fields would hide them but cost
8.5% on a mixed seq walk (section 1). No test pins the old answer; any found
during the conversions is rescoped with this reason.

### Documentation rules that change

docs/evaluator.md keeps its rules: the macro host is still "not an untrusted
code sandbox" (line 154), and rejecting a macro result still does "not call
arbitrary getters or `toString`" (278-285).

docs/compiler.md, each changed by the pellet that converts the kinds it
names (the last by light-cljs-1015):

- 1028: "They preserve JavaScript Number, UTF-16 and private collection-brand
  semantics." becomes "They preserve JavaScript Number and UTF-16 semantics
  and recognise only values of this runtime's classes (docs/value-layout.md)."
- 1077-1083: "Owned arrays/cells and base callable storage are frozen;
  per-type extension prototypes and public value wrappers remain extensible
  for protocol and JavaScript property writes. Those properties do not
  replace the private collection payloads; private brands prevent host
  arrays or objects from impersonating language values." becomes "A runtime
  value is an instance of its kind's class: its data are own fields, its
  protocol methods, masks and kind are properties of the class prototype,
  and every such name ends in the build's secret key
  (docs/value-layout.md), so no host array or object is a language value by
  accident. Nothing in a value is frozen; the shared bases holding `call`,
  `toString` and `equiv` are."
- 1089-1092: "Its brand and store are a private class field rather than a
  WeakMap entry, which no host object can have or copy either, and its trie
  nodes stay private" becomes "Its count, shift, root, tail, metadata and
  hash are its own keyed fields, and its trie nodes stay private".
- 1183-1191 (`lightCljsWeakMapStores` keeps "all of these stores as
  `WeakMap`s instead ..., as unforgeable a brand") becomes "a host that
  states `globalThis.lightCljsWeakMapStores` before a runtime loads, as the
  evaluator does in its QuickJS sessions, has its compile-time values
  branded by a `WeakSet` rather than a private field".
- 1192-1204 (the `payloads` brand class and `payloadStore`) is removed; its
  replacement is one sentence: "A kind test reads the value's keyed kind,
  one property of its prototype."
- 1205-1213: "Unforgeability is kept everywhere: every kind test still reads
  a private field that no host object can have, inherit or copy, so a value
  forged with `Object.create` of a runtime type's prototype, or by copying a
  value's own properties, is no vector, map, seq or map entry, as before"
  becomes "No host value is a language value by accident: every kind test
  reads a property whose name ends in the build's secret key, which no host
  object holds or inherits, and copying a value's own fields onto another
  object copies no kind. Deliberate reflection can forge a value
  (`Object.create` of a runtime prototype, reading the prototype's keys);
  as in upstream ClojureScript, that is outside the guarantee
  (docs/value-layout.md, section 5)."
- 1222-1240 (kind numbers "under a private symbol", the kind tables and
  their wrappers) is replaced by section 3's dispatch in two sentences:
  "`seq`, `first`, `rest`, `next` and `count` call the value's keyed slot;
  `extend-type` and `specify!` overwrite slots."
- 1254-1257: "An array map ... stores a frozen entry array, and its kind, in
  class-private fields" becomes "An array map ... holds its keys and values
  in one array of its own, unfrozen, as upstream's".
- 1284-1287: "A hash map keeps its trie in a class-private field rather than
  a `WeakMap` entry: as unforgeable a brand" becomes "A hash map holds its
  root, count and nil entry in keyed fields".
- 2878-2906 (what makes an instance a type instance is `TypeBrand`) becomes
  "What makes an instance a type instance, and a record's type, is its
  prototype's keyed kind. Below `Advanced` the runtime's `makeType` makes
  the constructor; at `Advanced` each type's constructor is a function of its
  own. The compile-time runtime also brands each instance (`TypeBrand`, or a
  class's private field where every constructor runs only through `new`), so
  that the compiler form bridge tells it from a host value without reading a
  property", keeping the rest of the paragraph (the light-cljs-346 class
  `TypeError` applies only where the compile-time `Own` brand is used).
- 2975-2977: "Built-in types have distinct public extension prototypes while
  their immutable payloads and private brands remain protected." becomes
  "Built-in types are classes whose prototypes hold their protocol methods;
  `extend-type` replaces a method in place."

## 6. Migration order

## 7. Startup
