# Two runtime decisions from the captured macro corpus

Inspected repository revision: `677ffce822bbec43cbba2a53614e0e847f0a05f8`
(the completed 726 export). All source/test anchors below refer to that tree.
The corpus manifest records compiler base `49b178ecbf739b276eb93a9d350facad09e50505`
and hashes of the actual capture inputs; that base is not the final exporter revision.
This is source/test reading plus executable storage evidence, with no timing claim.

## 1. What survives a call frame: values or binding cells?

**Observed requirement.** `corpus/closure.cljs` computes `offset = base + 3`,
then passes a closure using it to `mapv`. In `closure-captured.pants.js`, `l7`
captures initialized `const l5`; both it and the enclosing macro also use runtime
objects. The V8 factory has the corresponding `l5` capture and ordinary method
calls. The Pants GSP inventory has six functions/61 instructions including
`function`, `get_capture`, `call` and `call_method`; the V8 one has seven/103.
Neither inventory requires cell operations. `churn-churn.pants.js` additionally
reuses loop locals and calls selectors while its operands and loop state remain
live. These are exported factories, not Goose-executed macros.

**Reference paths and focused tests.**

- The project's QuickJS is the patched **quickjs-ng 0.16.2** bundled in
  `rquickjs-sys 0.14.0`, selected by root `Cargo.toml`; see
  `vendor/rquickjs-sys/PATCHES.md`. Its `quickjs/quickjs.c` blob is
  `2e43c18dec762cfad9d923cc09a72489176e1e43`.
  `JSVarRef` (line 469) points at an open frame slot or its detached owned value.
  `get_var_ref` (17489) reuses a binding's reference, `js_closure2` (17562)
  propagates it through nested closures, and `close_var_ref`/`close_var_refs`
  (17788/17813) detach before frame reclamation. `OP_get_var_ref`/
  `OP_put_var_ref` (18979/18990) dereference the same binding. This preserves
  shared mutation; copying a mutable binding's current value would not.
- QuickJS's `test_closure1` setter/getter case is retained with its license in
  `tests/fixtures/js/quickjs_test_closure.js` under the engine package. That
  fixture comes from Bellard/Gordon QuickJS revision
  `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, **not** the vendored ng revision.
  The adapted `quickjs_closure.js` is exercised by
  `tests/js_functions.rs::fixtures_run_calls_recursion_and_closures_through_vm_instructions`.
  These fixtures are behavioral references, not evidence of running ng's test suite.
  The project's current QuickJS-host tests in
  `tests/evaluation/evaluator.rs::compiler_units_execute_with_identical_expected_results_in_quickjs_and_node`
  cover higher-order invocation through `apply` and lazy `map`.
- Pants `src/frontend/lower.rs::Facts::repr` (447) selects cells for captures
  that can precede initialization or observe assignment; initialized immutable
  bindings use values. `capture_access`/`capture_place` (1970/2010) carry the
  selected representation. `src/vm/dispatch.rs::closure_step` (1375) copies
  captures into a function object; `GetCapture` (3145) retrieves that value,
  while `NewCell`, `CellInit`, `CellGet` and `CellSet` operate on binding objects.
  `tests/js_functions.rs::closures_over_immutable_bindings_capture_values_and_are_publishable`,
  `closures_see_assignments_and_keep_bindings_alive`, and
  `closures_check_the_temporal_dead_zone_at_run_time` distinguish those cases.
  GSP v2 retains these opcode distinctions (`export.rs::supported`).

**Options.** (a) Store Goose references into frame-owned locals: convenient for
live calls, but cannot survive local reclamation or become a transport image.
(b) Give every captured binding an indexed cell: viable, at the cost of cells
and indirection even for this corpus's immutable captures.
(c) Preserve the lowerer's value/cell distinction in flat owned tables.

**Decision: (c).** A closure record holds a function ID and a range in a
fixed-stride capture-value table. A captured object value copies its ID, not
the object's contents. Binding-cell values refer to a separate table containing
payload, initialization and mutability state. Captures never borrow a frame
slot. Explicit call frames hold code/PC, callee, receiver and operand/local
bases; returning truncates frame scratch without truncating captures or cells.
Do not infer capture modes again in Goose or replace source calls with native
macro bodies. Implement supported cell instructions with TDZ/const checks, or
reject them explicitly; their presence in the export schema alone is not execution.
The probe's shared-cell cases test this representation, not an additional
mutable-capture requirement invented for the captured macros.

Goose root structs cannot be copied as byte images, and owned dynamic arrays
inside records are variable-stride. Separate owners of fixed-stride records
plus integer ranges fit the actual compiler's indexed-storage rules. This
choice simplifies lifetime reasoning; its time/space advantage is unmeasured.

## 2. How should persistent versions and source occurrences live?

**Observed requirement.** `corpus/churn.cljs` invokes `conj` and `assoc` at
0/16/100 iterations, then traverses the vector with `seq`/`next`/`first` and
reads the map with `get`. Its Pants source uses `cljConj`, `cljAssoc`,
`cljCount`, `cljRest` and form-vector construction; the GSP inventory has
five functions/167 instructions. `closure.cljs` returns vectors containing
the original `x` occurrences alongside computed numbers. Its companion
`closure.pants.jobs.edn` retains input/expected datum IDs, child ranges,
origins and metadata separately. Equal runtime numbers cannot identify the
input occurrence that must survive that bridge.

**Reference paths and focused tests.**

- QuickJS does not supply Clojure persistent collections. In the actual
  reference runtime, `src/js_runtime.js::vectorConj` (3498) copies the tail
  and path as needed; `vectorAssocNode`/`vectorAssoc` (3527/3534) copy the
  changed path/tail and preserve other child objects. The ng engine implements
  the required shallow array copying in `quickjs.c::js_array_slice` (44329),
  including borrowed fast-array elements through
  `JS_CreateDataPropertyUint32Const` (11611). `js_dup` (1663) increments an
  object's reference count rather than cloning it. Persistence comes from
  runtime operations; lifetime comes from engine references and its collector.
  `tests/evaluation/evaluator.rs::complete_collection_fixture_preserves_typed_values_and_printing_in_both_engines`
  checks the QuickJS collection path, and
  `src/evaluator.rs::shared_macro_arguments_restore_provenance_after_environment_import`
  specifically shows why scalar-value lookup loses the original input identity.
- Pants `src/collections/vector.rs::PVector` uses immutable `Arc` trie nodes,
  path-copying updates and identity-preserving child `Value`s.
  `src/collections/form.rs` distinguishes Clojure form equality from the generic
  collection API's SameValueZero map keys; do not copy that generic key contract
  into macro forms. `src/collections.rs` defines occurrence wrappers separately
  from value equality. `tests/collections.rs::vector_updates_keep_old_versions_and_share_untouched_subtrees_across_collection`
  asserts old versions and unchanged aliases, and
  `equal_scalar_occurrences_keep_distinct_provenance` checks equal scalars with
  different origins. This is useful semantic evidence, not a demand for Goose GC.

**Options.** (a) Deep-copy child values on every update: viable for value
results but erases aliases unless supplemented with identity memoization.
(b) Indexed path-copying trie/HAMT nodes: viable and more scalable, with more
implementation and validation surface. (c) Bounded flat child-ID ranges,
copying the changed container's range and sharing all untouched child IDs.

**Decision: start with (c).** Store immutable form/container records, child-ID
pools, UTF-16 pools and explicit occurrence/origin records. Use checked IDs;
never intern equal scalar occurrences. A vector update copies IDs, with assoc
at count appending; a bounded map initially uses ordered key/value ranges and
current form equality, preserving literal input/evaluation order separately
from lookup semantics. A sequence can be an immutable range cursor. Keep JS
mutable arrays/objects distinct from these persistent forms. Retain metadata
and source references independently from runtime value equality.

Use append-only session heap tables with enforced node/child/byte budgets and
no slot reuse in this serial slice; reclaim call scratch on return and the
heap at whole-session teardown. External handles validate their session identity.
Do not rewind a job heap while an initialized helper, closure or retained value
can still reach it. This intentionally retains dead intermediate versions;
finite budgets must fail explicitly, with no general collector required.
At 100 successive vector appends, whole-range copying alone writes 5,050 child
IDs; this is an operation-count consequence, **not** a measured allocation or
timing result. Repeated session calls also consume the cumulative budget.
If measured serial costs justify a trie or a safe lifetime partition, change
the layout behind the same semantics later. Independent bridge output must
copy/relocate data and retain compiler source owners before session teardown.

## Executable evidence and implementation mapping

`layout_probe.goose` is an original bounded storage probe, not adapted engine
code, bytecode dispatch, an alternative VM, or a translation of a macro. It
uses integer payloads to isolate storage: real tagged JS values, graph bridge
relocation, source retention, structural map equality, TDZ reads, exceptions
and runtime dispatch remain implementation work.

The probe checks capture-table and cell-table growth, reuse of the exact frame
slots after return, immutable capture stability, two captures sharing one
mutable cell, and rejection of uninitialized/immutable/out-of-range writes.
It also checks old container versions, repeated child aliases, distinct origins
for equal scalar values, assoc-at-count, pool growth and independently owned
scalar output after the private storage owner has ended. It does not prove
full graph export or a macro expansion.

From the repository root, `bb crates/big-boy-pants-js/scripts/smoke.bb`
now includes `goose format --check layout_probe.goose`, `goose check`, debug
execution, native build and execution, alongside existing VM/source/20-image
checks. All passed with `goose v2026.3-267-g369c7a2c` (runtime `3025e5810379`)
and Babashka `1.12.218`. Standalone commands (from this directory) are:

```sh
goose format --check layout_probe.goose
goose check layout_probe.goose
goose layout_probe.goose --debug
goose build layout_probe.goose -o .layout-probe
./.layout-probe
rm .layout-probe
```

Debug/native output reports both `layout captures: ... passed` and
`layout collections: ... passed`. No performance measurement selects either
design. Existing Rust tests above were read as references, not rerun: no Rust,
export format or compiler boundary changes in this investigation.

- **727:** extend the existing VM with explicit frames, capture values/cells,
  checked heap identity, supported call/property semantics and cumulative budgets.
- **728:** implement the selected real runtime/native leaves, persistent
  forms and occurrence-aware owned bridge; verify actual expansion results,
  aliases, multiple calls after one boot and initial diagnostic timings.
- **729/730:** explicit snapshot copying and validated immutable relocation
  must copy the separate tables/allocator metadata and preserve aliases;
  the present root struct is never a transport object.
- **738/735:** measured serial-cost investigation and final equivalent-work
  comparisons decide later performance changes, not this layout probe.

The complete 738-record queue/history was scanned, including closed/deferred
work, and the existing Goose implementation sequence covers these findings.
No new pellets were created (0 of the authorized 3 used). Remaining limitations
are the stated implementation boundaries, not missing investigation acceptance.
