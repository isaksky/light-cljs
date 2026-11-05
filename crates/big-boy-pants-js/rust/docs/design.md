> Integration update, 2026-10-25: this engine now lives in the light-cljs
> repository at `crates/big-boy-pants-js/`. The original specification below
> records its standalone development constraints. Its instructions about an
> unavailable compiler and a separate repository are historical: inspect and
> integrate with the compiler in the same checkout. The runtime architecture
> requirements still apply.

You are starting a standalone Rust project: a custom JavaScript runtime designed for highly concurrent compiler workloads.

Implement the first meaningful architectural slice, with runnable code and tests. Do not stop at a design document.

IMPORTANT: This project is being developed on a separate VM with NO access to the compiler that will eventually use it.

Do not search for, request, clone, inspect, or attempt to reconstruct the light-cljs project. Its absence is intentional, not a blocker.

This specification is your source of requirements. Build a standalone runtime, a minimal embedding API, and a simulated compiler host that demonstrates the required capabilities.

You may consult QuickJS source as implementation material, but this is not a conventional compatibility-focused QuickJS port.

# 1. Purpose

The eventual consumer is light-cljs, a Clojure/ClojureScript-to-JavaScript compiler implemented in Rust.

The compiler already has its own parsing, analysis, macro machinery, and scheduling. We are not implementing those here, and we are not building a second Clojure interpreter.

The runtime’s purpose is to execute compiler-generated JavaScript efficiently, especially macros and helpers, while allowing:

- Many lightweight execution tasks across multiple CPU cores.
- Direct sharing of persistent values between Rust host code and JavaScript execution.
- Shared executable code and eligible immutable state.
- Suspension when a task needs a definition or compiler result that is not available yet.
- Resumption without restarting the computation or repeating prior effects.
- Native persistent collections, atoms, and task-local mutable builders.
- Low task startup costs and little duplication of loaded runtime data.

We control the JavaScript emitted by the eventual compiler. We can introduce private intrinsics and explicit lowering conventions.

We do not need to support arbitrary uses of every JavaScript concurrency primitive. We do need clearly specified behavior for the supported JavaScript subset.

Compiler-generated does not automatically mean pure or static. The design must accommodate closures, exceptions, native callbacks, and deliberately stateful operations.

Emitted applications can continue running on conventional JavaScript engines. This project primarily concerns compile-time execution.

# 2. Scope and working approach

Create a small Rust crate or workspace. Avoid excessive crate boundaries and speculative abstraction layers.

Build the execution architecture before attempting comprehensive ECMAScript coverage.

Use QuickJS as a source of implementation ideas, semantic algorithms, tests, and potentially ported code. If you use upstream material, record its revision and preserve attribution and license requirements.

Do not preserve architectural choices that conflict with this specification merely because QuickJS uses them.

The intended execution core is Rust-native. A wrapper around a C JavaScript engine is not completion of this project.

Do not begin by porting every QuickJS feature. Start with enough genuine JavaScript execution to demonstrate the architecture, and expand from there.

# 3. One runtime, many resumable tasks

The core organization is:

- Runtime: shared heap, executable code, native types, and execution machinery.
- Task: VM frames, operands, lexical state, dynamic execution context, pending operations, and diagnostic information.
- Worker: an OS thread currently executing a task.
- Host: supplies dependency identities, produces requested results, and coordinates scheduling.

Task execution state belongs to the task, not permanently to a worker thread.

A suspended task must be able to resume on another eligible worker. Ensure a task can never execute on two workers simultaneously.

Use explicit VM-managed frames. Do not depend on keeping a Rust or C call stack alive while waiting for compiler work.

A run operation should return outcomes conceptually like:

    Complete(value)
    Failed(error)
    Waiting(wait_key)
    Yielded

Keep the runtime’s execution interface independent of a specific scheduler implementation.

For this standalone project, provide a small reference executor or test scheduler. Later, a compiler should be able to drive the same runtime using its own worker pool.

Do not require an additional runtime-owned thread pool in the embedding contract.

Do not use a global interpreter lock. Short locks for metadata or scheduling are acceptable; serializing all JavaScript execution is not.

# 4. Shared data: use Clojure’s concurrency model

The primary model is:

    Share persistent values.
    Publish changes through atomic references.
    Build intermediate values privately.
    Suspend tasks when results are unavailable.
    Use locks for exceptional shared mutation, not as the default.

Use one logical shared heap.

Distinguish:

1. Shareable immutable/persistent values.
2. Task-owned mutable objects and transient builders.
3. Explicit concurrent references, such as atoms and dependency cells.

General-purpose shared mutable JavaScript object graphs are not required for the first implementation.

Persistent values must be directly accessible to both Rust host code and the VM without serialization or per-worker reconstruction.

For the standalone demo, construct a persistent form-like value in Rust, pass it to multiple VM tasks, and allow those tasks to return the same underlying structure.

Returning an unchanged subtree must preserve structural sharing. Do not merely deep-copy it and compare the results for equality.

Start with a small useful set of persistent types, such as vectors and records/maps, rather than implementing every collection before demonstrating sharing.

Keep these concepts distinct:

- Value equality.
- Object/reference identity.
- Syntax occurrence and provenance.

The same scalar value can occur in multiple source locations. Do not attach provenance by assuming equal values represent the same occurrence.

A persistent collection containing an ordinary mutable object is not automatically safe to share. Define and enforce publication rules.

Transient builders belong to an exclusively executing task, not permanently to its original OS thread.

Ordinary reads of shared persistent values should not acquire per-node locks.

# 5. Native operations and internal bookkeeping

Provide native Rust operations for persistent values.

The eventual compiler may lower known collection operations directly to private intrinsics. Do not require all host data to be wrapped in ordinary JavaScript objects and reconstructed through generic property operations.

Keep internal compiler/runtime bookkeeping separate from user-extensible operations.

Dependency tables, task registries, and private maps should use internal identifiers and operations that cannot unexpectedly invoke user hashing, equality, getters, or protocol callbacks.

Explicit language-level operations must follow their documented behavior.

For now, document the native collection contract you implement. Do not claim compatibility with an unavailable light-cljs collection implementation.

Do not create a universal reflection-heavy conversion framework in the hot path.

# 6. Atoms and atomic publication

Implement native atomic-reference machinery suitable for shared persistent values.

Distinguish:

- Single-writer publication.
- Multi-writer compare-and-swap.
- Single-assignment dependency completion.

Define comparison semantics explicitly. Do not use user-defined equality inside an atomic update or synchronization operation.

Use a stable reference/handle representation and safe lifetime rules. A reader must retain a valid value even if another task replaces the atom’s current contents.

Use Rust atomic synchronization primitives for publication. Do not use read_volatile/write_volatile as interthread synchronization.

Begin with conservative, documented memory ordering.

Retrying atom updates must not repeat macro execution or external effects. Expensive or effectful computation belongs outside the CAS loop.

Do not put all shared runtime/compiler state behind one global atom.

General-purpose add-watch is not required initially. Dependency waiting uses the narrower mechanism below.

# 7. Dependency cells

The central waiting primitive is a native, single-assignment dependency cell.

Conceptually:

    DependencyKey:
        world/version
        definition or query identity
        requested result kind

    DependencyCell:
        producer identity
        outcome
        waiting task handles

    Outcome:
        Pending
        Ready(value)
        Failed(error)
        Cancelled

A cell completes once for a given key/version.

Do not reset a completed cell to Pending while existing tasks still reference it.

The VM need not understand namespaces or compiler analysis. It understands result keys, cells, and suspended tasks. The simulated host supplies their meaning.

Provide a private compiler-facing intrinsic conceptually equivalent to:

    const helper = __rt.need(DEF_HELPER, RESULT_CALLABLE);

The task carries its world/version as execution context.

Lower this to an explicit VM operation when appropriate.

The operation must:

1. Record the dependency, even when the result is already ready.
2. Return the shared value immediately when ready.
3. Propagate a completed failure appropriately.
4. Otherwise ensure one producer is scheduled.
5. Register the consumer and suspend it.
6. Continue from the suspended operation when the result arrives.

Do not restart the task or replay the macro.

Do not turn every function into an ordinary JavaScript Promise chain.

Private intrinsic names and binding rules can be reserved by the compiler/runtime contract. Specify that contract rather than trying to recognize arbitrary JavaScript functions by name.

# 8. Notifications and lost-wakeup correctness

A dependency waiter is a task ID or compact wake handle, not an arbitrary JavaScript callback.

Completing a cell enqueues eligible tasks. It must not execute dependent JavaScript inline inside the publication operation.

Support multiple waiters.

Implement a reliable read-or-subscribe protocol.

Explicitly handle:

- Completion between checking Pending and registering the waiter.
- Completion after registration but before the task has finished parking.
- Duplicate wake-ups.
- Cancellation while registration or completion is in progress.
- Preventing concurrent execution of the same task.

Start with an atomic completed-result fast path and a small per-cell lock for pending waiter bookkeeping, unless there is a clear reason to choose something else.

Do not make a sophisticated lock-free waiter list a prerequisite.

Do not call host code or execute JavaScript while holding internal waiter-list locks.

Cancelling one consumer must not automatically cancel a producer still needed by other consumers.

Producer failure must wake waiters just as producer success does.

# 9. Dependency graph semantics

Keep two relationships distinct:

- A recorded dependency: computation A used result B.
- An active wait: A cannot currently proceed until B completes.

Record dependencies even for already-ready reads.

Only actual unresolved execution waits should be treated as potential deadlocks. A recursive reference graph is not automatically a wait cycle.

Do not model every definition with one coarse “ready” flag.

The simulated host should demonstrate that these can be separate results:

- Binding discovery.
- Callable availability.
- Initializer completion.
- An analysis result.

Constructing a function that refers to another binding does not necessarily require executing that binding’s initializer.

Demonstrate legitimate recursion without manufacturing a dependency deadlock.

Also demonstrate a genuine active dependency cycle and report it clearly.

An absent definition is not the same as an unfinished definition. The simulated host must eventually return a result or an error; an unresolved name must not silently become an eternal wait.

World/version is part of identity. A result from world 2 must not satisfy a task waiting for the corresponding result from world 1.

Full incremental invalidation is not part of the first milestone. Preserve the ability to add it later by keeping completed results stable.

# 10. Resumption, exceptions, and native callbacks

Task suspension must retain:

- Instruction position.
- Operand and local values.
- Closure references.
- Exception-handler state.
- Dynamic execution context.
- Pending native-operation state.
- Diagnostic context.

A dependency wait is not an exception. It must not prematurely run finally blocks.

It is also not automatically a JavaScript await or a new promise/microtask checkpoint.

Native operations that invoke JavaScript or demand dependencies must use a resumable protocol.

For example:

    Native operation
        → invokes JavaScript callback
            → callback needs unavailable definition
                → whole task suspends
                → callback resumes
        → native operation continues

Do not leave arbitrary Rust stack frames live to represent this continuation.

Leaf native operations that cannot suspend or reenter may remain ordinary Rust calls.

Do not hold Rust references into mutable VM storage across operations that may reenter, collect, grow storage, or suspend.

Define cancellation and safepoints explicitly.

Do not suspend while holding ordinary internal locks that producer tasks may need.

# 11. Memory management

The initial working choice is a precise, nonmoving, stop-the-world tracing collector.

Use worker-local allocation buffers where appropriate, without turning them into isolated worker heaps.

Do not mechanically replace every QuickJS reference count with an atomic reference count.

Do not represent every VM object as Arc<Mutex<...>>.

Coarse Arc ownership for independently managed immutable host storage may be reasonable, but document the lifetime and tracing boundary.

The collector must see:

- Running task frames.
- Suspended task frames.
- Compiler/host roots.
- Published dependency results.
- Atom contents.
- Closure environments.
- Native payload edges.

A Rust local or unmanaged pointer is not automatically a collector-visible root.

Define scoped embedding access and persistent rooted handles.

Specify how native/host code participates in safepoints. Stopping JavaScript execution is insufficient if native code can still access heap state unsafely.

The collector must not reclaim an old atom value while a reader is still using it.

Do not reclaim all of a task’s allocations when it completes; returned structures may be retained elsewhere.

Keep unsafe code limited and document its invariants.

Defer generational collection, concurrent marking, compaction, and exotic value packing until the basic model is correct and measurable.

# 12. Standalone simulated compiler host

Implement a small host that exercises the runtime without becoming a compiler project itself.

It should provide:

- A few definition/result IDs.
- Immutable world/version identifiers.
- Producers that make requested results available.
- A simple dependency graph.
- Shared persistent input values.
- A scheduler that can run at least two actual worker threads.
- Event recording for tests and diagnostics.

Some producers can perform Rust work. At least one should produce a callable implemented by the new VM.

Use JavaScript fixtures written to resemble compiler output.

Label them as synthetic fixtures, not actual light-cljs output.

Do not implement a Clojure parser, analyzer, macro expander, or namespace system to make these demonstrations work.

Do not hard-code fixture behavior into Rust and present that as JavaScript execution. JavaScript functions must actually execute through the new VM.

The host may implement native collection operations and explicit host services, but must not secretly replace the tested program’s control flow.

# 13. First end-to-end demonstration

Create a demonstration resembling this:

- Rust constructs a persistent form-like input.
- Two VM tasks reference the same input graph.
- They execute some JavaScript before requesting a helper.
- The helper is not yet available.
- The tasks suspend.
- Workers perform other useful work.
- A producer makes the VM-implemented helper callable available.
- Both consumers resume and invoke it.
- An unchanged input or subtree is returned with shared identity intact.

The example should prove:

1. Actual parallel execution on multiple OS workers.
2. Shared persistent data without per-worker import/reconstruction.
3. Suspension without blocking a worker.
4. Multiple consumers of one dependency result.
5. Resumption without replaying preceding effects.
6. Migration of a suspended task to another worker.
7. Collection while suspended task values remain live.
8. Correct completion, failure, and cancellation behavior.

Extend the slice to exercise a nested call, a live local or closure, exception/finally state, and native-to-JavaScript reentry.

Do not claim the architectural milestone is complete merely because a straight-line arithmetic program runs.

# 14. JavaScript coverage

Start with a documented subset sufficient for the demonstration.

Preserve ordinary JavaScript semantics for the features you claim to support. Reject unsupported syntax or behavior explicitly instead of silently interpreting it differently.

Source-independent bytecode fixtures are acceptable for early VM unit tests.

However, the demonstration should progress to JavaScript source being compiled and executed by the new runtime. Do not present hand-built bytecode alone as JavaScript frontend support.

A complete ECMAScript frontend is not required in the first iteration.

Do not require:

- QuickJS C API compatibility.
- qjs/qjsc compatibility.
- Legacy QuickJS bytecode compatibility.
- A JIT.
- Arbitrary shared mutable JavaScript object graphs.
- Full browser or Node.js APIs.
- Every ECMAScript feature.
- Full Clojure collection/protocol compatibility.
- A production incremental compiler.

Keep supported behavior and deferred behavior explicit.

# 15. Testing and measurement

Use deterministic barriers or scheduler hooks to exercise races. Do not rely on sleeps to establish correctness.

Test:

- Completion before registration.
- Completion during registration.
- Completion while a task is parking.
- Many waiters.
- Exactly one producer scheduled per result key.
- Producer failure.
- Consumer cancellation.
- Old-world/new-world separation.
- Already-ready reads recording dependencies.
- Legitimate recursion versus real wait cycles.
- Task migration.
- Shared-value identity.
- Collection with suspended frames and native references.
- Exception/finally behavior across suspension.
- Native callback suspension.
- Safe atom loads and replacement.

Measure what the standalone implementation can actually demonstrate:

- Task startup.
- Already-ready dependency reads.
- Suspension and wake-up costs.
- Shared input storage versus task count.
- Retained memory for suspended tasks.
- Native persistent operations.
- Useful work completed while consumers wait.

Do not claim improvements over light-cljs, V8, or QuickJS without actual matched comparisons. Those integrations and benchmarks are future work.

# 16. Implementation sequence and deliverables

Keep planning brief and immediately useful.

First:

- Create the crate/workspace.
- Write a concise architecture document.
- Define the value/rooting, task, dependency-cell, and embedding boundaries.
- Implement the smallest executable slice that exercises those boundaries.

Then extend that same slice rather than building unrelated subsystems.

Maintain a short, ordered backlog with concrete acceptance criteria. Do not create dozens of speculative tickets before producing runnable code.

Deliver:

- Rust source.
- Unit and integration tests.
- A runnable standalone example.
- Exact build/test/example commands.
- A concise description of supported behavior.
- An honest list of remaining gaps.
- The next concrete implementation step.

The first working example is an architectural proof, not a finished production JavaScript engine. Do not overclaim.

Make reasonable implementation decisions without asking for access to light-cljs or stopping for optional clarification.

If the full milestone cannot be completed in the current work session, leave the largest coherent runnable portion, record exactly which tests passed, and identify the next step. Do not replace implementation with another broad proposal.

Begin implementation now.
