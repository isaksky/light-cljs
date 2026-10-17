# Function.call ownership and frame diagnostics

Production remains at phase33. Neither experiment is retained.

The ownership variant preserves the native frame, realm, nested interrupt
checks and ordinary method lookup, but passes through interpreter-owned
arguments instead of forcing the child call to copy them. External calls
retain COPY_ARGV. The before/after semantic probes both pass 4,019 checks,
including escaped/mapped arguments, mutation, exceptions, bound functions,
Proxies, custom .call lookup and observable native stack frames.

The deliberately invalid upper-bound variant removes the native call frame.
It fails the stack-frame probe as expected and cannot be used in production.

| Replay | Invocation milliseconds |
| --- | --- |
| Baseline, alternating runs | 9221, 9247, 9248, 9301 |
| Ownership variant, alternating runs | 9235, 9319, 9309, 9334 |
| Invalid frame-removal upper bound | 9170, 9290 |
| Final baseline | 9376, 9402 |

Every replay reproduces the exact captured encoded macro result. The valid
variant shows no useful gain; even the unsafe variant improves only about
1–2% relative to the final baseline. Frequent Function.call frames in the
native profile do not establish their overhead as a major bottleneck.

Scripts and logs are archived here; frozen C/runtime sources, executables
and the large phase31 capture remain under /private/tmp/light-cljs-phase38
and /private/tmp/light-cljs-phase31. No production edits or new full-suite
claims follow from these rejected experiments.
