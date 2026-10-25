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
resolve a body under the object-table lock, then walk it without per-node locks.
Strings own no GC object edges. Atom cells use a separate immutable-box lifetime
boundary described below. Short metadata/object locks exist; there is no global
interpreter lock and no lock held across ordinary bytecode execution or a wait.
Automatic collection triggers, moving/generational/concurrent GC and worker-local
allocation buffers are deferred. Embedders must schedule collection explicitly.

## Publication and native values

`Runtime::publish` walks the transitive graph and reports a path on rejection;
it does not serialize, clone or freeze an ordinary object. Dependency completion, atom installation and Promise settlement also validate
publication; spawn leaves cross-task handoff validation to the embedder.

| Class | Contract |
| --- | --- |
| Immutable/shareable | Primitives, persistent collections/occurrences and eligible functions whose captures and other edges pass publication. Unchanged subtrees preserve identity. |
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
| atom replaced value held by a reader | `PersistentHandle` (load-to-root) | `atom_retained_old_value_retains` | `atom_retained_old_value_reclaims` |
| closure environment | `PersistentHandle` (`Function`) | `closure_environment_retains` | `closure_environment_reclaims` |
| captured mutable binding | `PersistentHandle` (`Function` → `Cell`) | `binding_cell_retains` | `binding_cell_reclaims` |
| persistent collection edges | `PersistentHandle` (`Map` → `Vector` → `Occurrence`) | `persistent_collection_edges_retain` | `persistent_collection_edges_reclaim` |
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
