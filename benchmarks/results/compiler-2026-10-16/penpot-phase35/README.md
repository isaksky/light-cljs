# Current per-macro costs

An isolated phase33 compiler times the stages of each JavaScript macro
expansion, deferred-form analysis and deferred-unit emission. A stack of
timers subtracts nested instrumented stages from each exclusive total.
Uninstrumented work remains charged to its enclosing stage: macro-call
time is therefore not a pure JavaScript CPU measurement. No instrumentation
is retained in production.

Main completed in 117.10 seconds, 116.43 user seconds, with 1,332,068,352
bytes peak RSS. This is a diagnostic run, not a replacement clean benchmark.
Its complete output is byte-identical to phase33 and passes Node syntax
checking. No CPU-heavy work overlapped the run.

| Stage | Calls | Inclusive seconds | Exclusive seconds |
| --- | ---: | ---: | ---: |
| Macro call | 48,216 | 58.973 | 54.669 |
| Deferred analysis | 13,361 | 76.807 | 14.299 |
| Whole form import | 48,216 | 8.155 | 8.155 |
| Result export | 48,216 | 7.257 | 7.257 |
| Environment import | 48,216 | 6.928 | 6.920 |
| Unit emission | 441 | 5.301 | 5.301 |
| Shared argument preparation | 48,216 | 4.266 | 4.266 |

Only exclusive times may be summed; about 16 seconds of this process lie
outside the instrumented stages. Macro transports still cost about 26.6
seconds. Deferred analysis and emission cost about 19.6 seconds after nested
macro work is subtracted.

The largest macro-call exclusive totals are rumext.v2/defc (16.787 seconds,
1,033 calls), instaparse.abnf/precompile-cljs-grammar (12.713 seconds, one
call), rumext.v2/html (4.779 seconds, 418 calls), app.main.style/css (4.316
seconds, 5,131 calls), and app.main.style/css* (3.628 seconds, 5,131 calls).
Together these account for about 77% of macro-call exclusive time. Rumext
defc totals 21.576 seconds including its transports. Detailed totals are in
summary.txt; nested instrumentation means the Instaparse main result is not
directly comparable to the isolated capture replay.

The macro stage is still the largest individual target. However, even
eliminating it entirely would leave roughly a minute of work in this
diagnostic, so reaching the speed goal also requires reducing transport
and native compiler costs. Phase36 samples the current native execution to
locate those costs without changing production behavior.

The isolated executable and emitted JavaScript remain under
`/private/tmp/light-cljs-phase35/`. Production validation remains phase33:
1,399 passing tests, zero failures, strict Clippy and all three Node workflows.
