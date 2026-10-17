# Phase92: QuickJS function samples

Diagnostic-only engine instrumentation of production phase87. Interpreter
safepoints capture up to 24 frames when at least 10 ms have elapsed, reading
bytecode names and declaration lines directly without executing JavaScript.
This is safepoint sampling, not a uniformly timed profiler: native calls and
work between safepoints can be underrepresented; truncated stacks and anonymous
functions sharing a name/line can also combine entries. The raw sample is
preserved so ambiguous entries can be inspected in context.

Main none takes 91.24 seconds / 890,830,848 peak RSS bytes and matches phase42
exactly, including its known ESM defect. There are 8,615 observations. Bridge
`visit` accounts for 11.2% of top frames, the compiler's declaration-demand
callback for 7.8%, `publicMethod` for 6.9%, and the private-store `has` and `get`
wrappers for approximately 5.6% and 3.9%. `publicMethod` is present in 32.6% of
stacks inclusively. Inclusive categories overlap and must not be added.

The private-store wrappers are anonymous functions at evaluated runtime lines
46 and 48; these are not compiler root lookup functions. Runtime line numbers
include the evaluator's two-line prefix and exclude manifest annotations.

Scripts, extracted counts, compressed raw samples, patch, logs and hashes are
retained. No full suite was run for this diagnostic-only binary and no engine
instrumentation is promoted.
