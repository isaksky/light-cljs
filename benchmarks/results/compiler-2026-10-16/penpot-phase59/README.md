# Incomplete V8 backing-store guard — do not promote

Based on validated isolated phase58. A Rust allocator limits ordinary V8
ArrayBuffer backing storage, tracks live bytes and signals terminal memory
failure on refusal. It uses atomics because backing stores may be freed off
the owner thread. The isolate's managed heap and external-memory totals are
also checked after operations and in interrupt callbacks.

The earlier fixed-buffer version passed five focused tests and strict Clippy:
zeroing, reclamation on session drop, terminal refusal despite JavaScript catch,
and the three callback regressions. `clippy-fixed-buffer-version.log` covers that
earlier source, not the final incomplete source. No full suite or Penpot benchmark
was run for this phase. `compile-initial.log` preserves the corrected immutable
isolate-reference compile error.

The added resizable/growable regression fails against the allocator-only guard:
resizable ArrayBuffer bypasses V8's custom allocator. Including V8's external
memory catches that case, but the regression still fails for:

```js
new SharedArrayBuffer(33 * 1024 * 1024, { maxByteLength: 34 * 1024 * 1024 })
```

Under a 32 MiB limit this succeeds. The diagnostic records a 1,347,224-byte
managed heap, zero external bytes and zero custom-allocator bytes. Therefore
neither the allocator counter nor the current aggregate guard bounds growable
shared backing storage. Later cases in that test loop have not run past this
failure. The negative-control and diagnostic logs are preserved.

This is an unresolved backend contract gap, not a passing memory-limit fix.
`buffer-budget-incomplete.patch` reproduces the current source from phase58,
including its failing test and diagnostic. Production and validated phase58
remain unchanged. Stack limits, startup behavior, watchdog behavior and other
V8 adoption requirements also remain outstanding.
