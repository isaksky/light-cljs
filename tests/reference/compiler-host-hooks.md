# Compiler host hooks

The source names `system-time`, `add-tap`, `remove-tap`, `tap>`, and
`load-file` resolve through the compiler and the canonical runtime. Loading is
an embedding capability installed by `EvalSession`; ordinary emitted programs
report that a compiler session and file provider are required.

## Capability contracts

| Capability | Default | Injection and limits |
| --- | --- | --- |
| Clock | `performance.now()` when supplied by the JS host, otherwise `Date.now()` | `EvalOptions.clock` or `rt.core.configureHostHooks({clock})`; callback returns finite milliseconds, with its time origin defined by the host. |
| Random | Host `Math.random()` | `EvalOptions.random` or `configureHostHooks({random})`; callback returns a Number in `[0,1)`. The same hook feeds `rand`, `rand-int`, `rand-nth`, `shuffle`, `random-sample`, and `random-uuid`. |
| Tap scheduling | Synchronous FIFO dispatch, limited to 1024 accepted events per drain | `configureHostHooks({tapSchedule, maxPendingTaps})` or dynamic `*exec-tap-fn*`; scheduler receives an at-most-once task and reports admission by truth value. Pending scheduled events share the bound. |
| Console | Only a `globalThis.console` supplied by the embedding JS host | `enable-console-print!` installs the console methods as output sinks. `EvalOptions.output` and `error_output` are independent explicit Rust sinks, bounded by `max_output_bytes`. |
| Namespace loading | Built-in compiler libraries; other namespaces require `source_provider` | Existing typed `LoadRequest`; application/session cache and cycle detection are shared with runtime file loading. |
| File loading | Denied without a provider | `EvalOptions.file_provider: FileProvider` receives `FileLoadRequest {path, requesting_namespace}` and returns `SourceInput` or no source. Paths are opaque; the session grants no filesystem access. |

Clock and random options configure the application heap. The compiler macro
heap has its own host environment. Fresh sessions reuse capability callbacks
but own independent runtime state, tap sets, and compiler roots. Callbacks can
return `EvalError` to deny individual clock, random, or file requests.

The tap set deduplicates a registered callable; adding and removing return nil.
The listeners are sampled when an admitted event is delivered. A listener's
exception does not prevent another listener receiving the event. Changes made
by a listener affect later deliveries. A synchronous recursive listener can
enqueue more events until the drain bound, after which `tap>` returns false.
The default requires no ambient timer. An injected scheduler owns its delivery
timing and ordering, and must eventually invoke accepted tasks or keep their
bounded pending slots occupied. Null scheduler injection restores the default.

The synchronous default is an explicit policy difference from upstream
[ClojureScript's timer-backed `*exec-tap-fn*`](https://github.com/clojure/clojurescript/blob/a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e/src/main/cljs/cljs/core.cljs#L76-L84).
Registration, removal, delivery-time membership, and listener exception
isolation follow upstream
[tap definitions](https://github.com/clojure/clojurescript/blob/a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e/src/main/cljs/cljs/core.cljs#L11345-L11374).

## Loading and recovery

Both `EvalSession::load_file(path)` and language `(load-file path)` use the
session's incremental `InteractiveCompiler` and application runtime. The host
callback uses the already active `HostCall` context, including nested loads;
it does not create a second evaluator. Forms are read and evaluated in order,
so definitions and effects preceding an error remain visible. Macros retain
their existing separate compiler heap. Namespace directives and `.cljc`
reader conditionals use the existing compiler reader and loader.

The requested `.clj` suffix is rejected before the provider runs, and a returned
`.clj` source is also rejected. The returned source name supplies read, analysis,
and runtime diagnostic locations. Missing providers, missing files, provider
denial, and file cycles are ordinary recoverable errors. Existing source-byte,
engine memory, stack, cancellation, and operation-time limits apply. Source
loads preserve namespace load-cache state and restore the requesting namespace
after loading namespace dependencies. Direct file namespace changes become
the current session namespace and update `*ns*`.

Loaded language exceptions cross the host bridge as their original application
values. `ex-data`, catch behavior, and `last_exception` preserve that value;
the host adds the loaded-file diagnostic context without replacing the value.
The usual session/CLI result-history update remains the caller's responsibility.

## Executable evidence

`tests/compiler_host_hooks.rs` names each contract:

- `clocks_random_sources_and_taps_are_explicit_session_capabilities` checks
  deterministic hooks, duplicate registration, removal, listener isolation,
  and fresh-session isolation.
- `runtime_scheduler_admission_bounded_reentrancy_and_arities` executes the
  same scheduler, membership-timing, random, arity, and drain-bound assertions
  in Node and QuickJS.
- `host_hooks_have_macro_application_phase_witnesses` invokes `probe` inside
  the compiler-backed `phase-snapshot` macro and as an application function,
  checking system time, taps, finite reentrancy, and load-capability denial in
  both heaps; it separately checks application clock denial and recovery.
- `load_file_uses_one_compiler_and_heap_with_macros_namespaces_and_require`
  covers `.cljc`, retained macros/functions, namespace switches, and shared
  namespace-provider caching.
- `file_diagnostics_history_recovery_and_capability_denial_are_stable` checks
  loaded source locations, partial effects, original exception data/history,
  cycles, `.clj` rejection, missing capability, and provider denial.
