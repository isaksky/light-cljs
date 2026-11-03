# Goose source export experiment

From the repository root, run:

```sh
bb crates/big-boy-pants-js/prototypes/goose/regenerate.bb
bb crates/big-boy-pants-js/prototypes/goose/regenerate.bb --check
```

The first command regenerates deterministic source artifacts, independent
V8/Pants expansion and output expectations, bytecode, metadata and SHA-256
manifest. The second checks without changing artifacts. Both validate metadata,
test the Rust exporter, and run Goose format/check/debug/native checks and
inspection of every binary. They require Rust and the installed V8 cache, Node,
Babashka and Goose. Builds stay inside this checkout; temporary native outputs
are removed on success or failure. For the fast Goose-only command:

```sh
bb crates/big-boy-pants-js/prototypes/goose/smoke.bb
```

`inspect.goose` accepts `.gsp` paths as arguments. Scripts resolve their own
locations and also work from the engine package as `prototypes/goose/...`.
Validated Goose: `v2026.3-267-g369c7a2c (runtime 3025e5810379)`.

## Corpus and coverage

`capture.rs` runs the actual compiler with test-only hooks at deferred/native
unit emission and successful expansion. Normal builds have neither the hooks
nor recorders. Factories come from `MacroCode.unit`, with the exact
`rt => rt.core.macroReaderFunction((UNIT)(rt))` wrapper used at
`source_macro_function`; helper factories come from the emitter. Reference
engines invoke the macro bodies. Compiler-owned expansion values and provenance
are compared after both hosts and their engine-owned values have died. Node
separately checks application output.

| Source | Reference work | Goose coverage |
| --- | --- | --- |
| `churn.cljs` | Collection-heavy `benchmarks/native-macros-k.bb` loop at 0/16/100: vector conj, map assoc/get, seq traversal, rest/count; `0 168 5250` | Every emitted function exported and inspected |
| `closure.cljs` | `mapv` invokes a helper closure capturing a computed offset; `[[1 11] [2 12] [3 13]]` | Functions/captures exported and inspected |
| `branches.cljs` | Independent expansions initialize two definitions; a consumer demands both; exactly two producer factories execute | Macro and helper factories exported and inspected |
| `effect.cljs` | One declared compiler-gensym effect; result proves it returned a symbol | Source inspected; effect authority declared |
| Historical arithmetic/control source | Results 27.5 and 7307 | Source-derived entries execute debug/native |
| Generated wide source | 300 constants/locals, code and branch addresses above 255; result 299 | Executes debug/native |
| Generated Unicode source | UTF-8 emoji and a lone UTF-16 surrogate | Inspected; strings excluded from scalar execution |

Historical scripts are copied unchanged from
`7ba4bd6f:tests/fixtures/js/{arithmetic,control_flow}.js`, retaining their
comments. The existing parser's final-expression span inserts an entry-function
`return`; both original and adapted sources are retained. `compile_function`
here proves entry-body execution only. Bootstrap invariants adapt `cc764dbe`,
`88fc62d3`, `99d4f9fb` and `1ddd5633` (repository-root paths at those commits).

**Parsed, exported and executed are distinct.** Per-factory inventories mark
parsed/exported true and Goose-executed false. Export walks every function,
including dead functions: a parsed but unsupported operation, such as a throw
in an unused function, is a positioned error naming its operation and function.
Macros do not yet execute in Goose; the scalar adapter explicitly rejects them
without fallback. The exporter accepts source alone and cannot evaluate jobs
or compute expansions. Reference generation supplies separate expectations.

The captured syntax surface includes arrow/ordinary function expressions,
closures, lexical bindings, labeled while/continue, if/ternary/nullish and logical
branches, assignments/comma expressions, member/index reads, ordinary/method
calls, arrays and scalar/string literals. Native form constructors and form
selectors lower directly. Per-factory inventories enumerate the resulting
operations and full function/instruction counts.

## Runtime and native boundaries

`runtime.edn` records conservative roots found mechanically in compiled string
constants. Core names resolve through `RuntimeManifest`, other exported names
through its exports; `reached_by_uses(Call)` selects transitive units. A data
string matching a runtime name can retain extra units. The broad `core` hub
is excluded as a root. `runtime.js` preserves selected original bodies in
initialization order and is independently instantiated in V8. Unit byte ranges
refer to the hashed `src/js_runtime.js`. Native counterparts are retained in the hashed
`src/bbp_runtime.js` and `src/bbp_reader.js`, named explicitly in the inventory.

The Pants profile uses explicit form-operation leaves (listed in `runtime.edn`)
in place of JavaScript private stores/brands. Remaining core calls, including
`mapv`, `inc`, `into`, `vec`, `gensym`, symbol testing and the reader wrapper,
are identified by selected core bindings. Compiler reader context, definition
demand/`cljReadRoot`, source provenance and gensym authority are declared
boundaries for later Goose implementations. No host evaluates a Goose job.
Selected runtime source is reference material, **not executable Goose export**;
its classes, private stores, protocol/reader machinery and host hooks are outside
the scalar VM. Complete ECMAScript/cljs.core are excluded.

## GSP version 2

Integers are little endian. An image owns one exact immutable UTF-8 source
buffer (source ID 0), fixed-stride tables and variable payload pools. There are
no host pointers, owning roots or runtime handles. Runtime strings/names are
exact UTF-16 code-unit ranges, including lone surrogates, separate from source
UTF-8. Source spans are half-open **byte** ranges. Diagnostic line/column work
is outside ordinary execution.

The 32-byte header is `GSP\0`, `u32 version=2`, then six u32 lengths: source
bytes, functions, constants, child IDs, instructions, UTF-16 units. Sections
follow in that order:

| Table | Record | Fields |
| --- | --- | --- |
| Functions | 13 × u32, 52 bytes | instruction start/count; constant start/count; child start/count; params; locals; captures; name-unit start/count; JS length; flags |
| Constants | 3 × u32, 12 bytes | tag and two words: undefined=0, null=1, boolean=2, f64 bits=3, UTF-16 range=4 |
| Child IDs | u32 | Absolute function IDs; function operands index their parent's child range |
| Instructions | 5 × u32, 20 bytes | descriptor; operand; source-byte start/end; original function-code byte offset |
| UTF-16 | u16 | Runtime string/name code units |

Descriptor bits 0..7 are the explicitly allowed engine opcode, 8..15 the operand
category (none, constant/name, local, byte target, child, capture, selector/count),
16..23 the original operand width (0/2/4). Flags contain strictness in bit 0 and
constructor kind (none/ordinary) in bits 8..15. `export_schema.goose` is compiled
reader code generated from the exporter's checked opcode/selector surface; an
input image cannot supply its own schema. Eval sites, unsupported operations,
Symbol/BigInt constants fail explicitly. No source behavior is dropped.

The existing lowerer supplies u16 constant/local/capture/child indices and u32
byte addresses. GSP table IDs/ranges are u32. Budgets: 8 MiB source, 65,536
functions, 1,048,576 records per other pool, function-tree depth 256. Constants
and child tables retain the lowerer's 65,536-entry bound; per-function
locals/parameters/captures are at most 65,535. IDs survive table growth.

The Goose inspector checks framing, known descriptors/selectors, UTF-8 and span
boundaries, ranges/references, child ownership/depth, instruction coverage and
byte offsets, and branch boundaries before installation. Failure preserves
every byte of an already valid image. Inspection is separate from execution:
engine lowering validates stack flow; the scalar adapter additionally runs the
Goose scalar VM's own stack validator before execution.

## Companion forms, definitions and jobs

Each `*.jobs.edn` is a version-2 metadata owner containing:

- `sources`: exact UTF-8 bytes once per compiler source identity; IDs are assigned
  by first occurrence and process-local SourceIds never escape.
- `data`: indexed occurrence nodes, flat child-ID and UTF-16 pools; tags preserve
  i64 integers, f64 bits, ratios, characters, names, compiler literals and metadata.
  Equal values do not erase occurrence IDs.
- `definitions`: macro name/generation, source/span and factory;
  `definition-uses`: analyzed/emitted generations;
  `producer-executions`: actual compile-time producer executions.
- `jobs`: correlated IDs, macro names, call/environment datum IDs, argument IDs
  into the input form table (after the operator), the form/env ABI, and separate
  reference expectations. Hidden arguments are the full input form and environment.
  Form records retain root IDs, flat child ranges,
  metadata/payload IDs, source IDs, byte spans and origins.
- `helper-factories` and `declared-effects`: demanded helper count and ordered
  compiler-gensym authority requirement. The original branch source declares
  the producer definitions and the consumer that reads them.

`regenerate.bb` validates references/ranges, acyclic bounded form/data graphs,
scalar types, argument occurrence IDs, source boundaries and UTF-16 values.
Negative cases reject cycles, unknown tags and overflow before narrowing.
Metadata budgets are 100,000 datum nodes, depth 128 and 1,048,576 child
IDs/code units. Records are compiler-owned independently of any evaluator or
Goose session. The manifest pins the starting compiler revision and content
hashes; current Git HEAD is deliberately excluded so committing regeneration
outputs does not change them.

## Scalar migration and ownership

Version 2 widens scalar constant/local operands to **u16** and branches to
**u32**; code is bounded at 16 MiB, tables at 65,536. `emit` checks wide integers
before narrowing or mutation. `load(..., version=1)` explicitly rejects obsolete
encoding. `encoding.goose` migrates only the historical hand-assembled test
notation and never loads source images.

The 17 bootstrap operations retain numeric/identity/resumption/effect behavior.
Source smoke adds division/remainder, negation, explicit numeric checks and
comparisons. String/object coercion fails explicitly. `scalar.goose` rejects
calls, closures, strings/forms and source host selectors before installation;
bootstrap effect opcode 17 cannot alias a source host-operation selector.

Program owns instructions/constants; Job borrows it immutably and owns locals,
operands, effects, PC, terminal status, result, fault and claim. Budget-zero
checks cover all fields and the allocator. Round-robin runs preserve exactly-once
effects and terminal-failure isolation. Session object IDs are monotonic and
meaningful only in their originating session. This slice has no collector,
snapshot publication, cross-session identity, concurrent jobs or macro timing claim.
