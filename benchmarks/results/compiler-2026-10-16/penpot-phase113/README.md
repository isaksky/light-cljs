# Separate private-store method bodies — not promoted

The profile suggested V8 feedback shared by the `cells`, `views`, `lazyValues`
and `chunks` private-store factory could be costly. This experiment gives each
store its own class/method bodies while retaining private brands and replacement
semantics. It changes only `src/js_runtime.js` relative to phase112.

Three Node workflows pass. Penpot main's application output and macro expansions
are byte-identical after replacing the respective embedded runtime factories.
The full Rust suite was not rerun because the measured candidate was rejected.

Sequential alternating cold main builds:

| Run | Wall | Peak RSS bytes |
| --- | ---: | ---: |
| baseline 1 | 23.44 s | 999,194,624 |
| candidate 1 | 23.36 s | 998,080,512 |
| baseline 2 | 22.60 s | 961,314,816 |
| candidate 2 | 22.35 s | 966,098,944 |

The average paired reduction is **0.165 s (0.7%)**, within the observed timing
variation, with no meaningful memory benefit. That does not justify duplicating
the runtime implementation. Production retains phase112's shared factory.
This result weakens the hypothesis of an easy large gain from private-brand
dispatch specialization; it does not establish that all remaining conversion
and dispatch costs are unavoidable. Patch, scripts, logs and hashes are retained.
