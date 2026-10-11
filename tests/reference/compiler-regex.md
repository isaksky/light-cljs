# Canonical ECMAScript regex evidence

Regex literals and `regexp?`, `re-pattern`, `re-find`, `re-matches`, and `re-seq`
execute the existing first-party JavaScript runtime in application and macro
phases. The obsolete native Rust regex-engine proposal is superseded; no JVM
regex subset or Rust regex engine is introduced. `clojure.string` uses the same
ECMAScript engine; its broader contracts and intentional corrections are in
[compiler-strings.md](compiler-strings.md).

## Complete source and phase witnesses

`tests/fixtures/compiler-upstream-regex-tests.cljs` retains the complete
`primitives_test.cljs::test-regexps` and `core_test.cljs::test-cljs-2453` bodies
from checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, including all let setup,
regex literals, sequence traversal and assertion paths. The original EPL notice
is retained. The executable reference module replaces only `deftest` and
`testing` with `do`, and `is` with a single evaluation appended to an observation
atom. All 23 assertions execute; the module asserts their count and outcomes.
A regular provenance check compares the retained source bodies with the checkout.

The shared `regex-contract` exercises all five public names at their declared
arities, saved calls and apply, RegExp identity/source/flags, full/partial/no
matches, optional captures, global lastIndex state, finite zero-width progression,
Unicode flag behavior, UTF-16 units and lone surrogates, invalid pattern/flags and
non-string inputs. It also checks replacement callback capture vectors and
ordered calls, replacement group tokens, replace-first and split overloads.
The module's literal result is `[[23 true] true]`. Exactly the same module runs
as a required macro helper and in QuickJS/Node applications. Public and local
API obligations retain exact declaration/signature and test-source hashes in
`compiler-language/{obligation-witnesses,phase-witnesses}.jsonl`.

The explicit ignored reference test compiles that module using Maven CLJS
1.12.134 and the clean exact checkout. Both reference identities and the
reference harness settings are checked; both produce `[[23 true] true]` in
Node. This is canonical CLJS evidence, not SCI/JVM regex equivalence.

A generated RegExp retains its source and `imsu` flags across the in-process
form bridge. A macro computes regex results without printing or serializing
engine values and returns finite quoted data. Borrowed regex/Date/UUID/#js reader
forms retain their literal tags and lone-surrogate contents. An arbitrary
constructed host object produces a located `Unsupported macro result` diagnostic
with macro name and result path. The same MacroHost succeeds after regex failure.
Newly created host arrays/objects require the documented explicit form bridge;
raw `#js` execution values are not automatically borrowed reader forms.

The existing incomplete `ns-publics 'cljs.core` snapshot can omit core bindings;
this is owned by live light-cljs-124's public reflection acceptance. Saved direct
regex function values execute here. Regex behavior itself has no native-engine
backlog.

## Bounds, controls and recovery

The local tests enforce a 1024-byte source limit and 64-byte output limit, reject
oversized source and printed regex-sequence output, retain a session after an
ordinary invalid pattern, and reset its global RegExp root/lastIndex state.
A 29-unit pathological backtracking input tests actual native RegExp VM work,
using a 100-poll budget, 40 ms deadline, and cancellation from another thread.
Caught regex failure cannot restore a terminal session; the same configured host
creates a successful fresh evaluator.

rquickjs-sys 0.14.0's bundled `libregexp.c` calls `lre_check_timeout` from its
matching/backtracking VM, wired in `quickjs.c` to the runtime interrupt handler.
These tests establish cooperative interruption of that regex execution, not a
hard wall-clock bound for every string builtin. Parsing/compiling a pattern,
individual string copies and other builtins without interrupt polls may return
before the post-call control check detects deadline/cancellation. Allocation
limits remain enforced by the evaluator allocator, while source/output limits
belong to EvalSession. Node emitted applications use their embedding's controls;
QuickJS session limits are not emitted as Node resource guards.

## Executed checks

- `cargo test --locked --test compiler_regex --test compiler_reader --test compiler_strings --test compiler_string_fixtures --test evaluator -- --nocapture`
- `cargo test --locked --test compiler_regex regex_contract_passes_both_pinned_references -- --ignored --nocapture`
- `cargo test --locked --test compiler_string_fixtures full_upstream_bodies_pass_both_pinned_cljs_references -- --ignored --nocapture`
- `node tests/support/js_runtime.test.js`
- `cargo bench --locked --bench evaluator -- --iterations 1`
- `python3 -B tests/reference/compiler-language/extract_inventory.py --check`
- `python3 -B -m unittest discover -s tests/reference/compiler-language -p 'test_*.py'`

The five focused integration suites passed 27 ordinary tests. The explicit
regex reference run passed both oracles; the string reference rerun passed 95 API
assertions and 1,000 seeded capitalization trials for each oracle. All 72 Python
ledger checks, inventory validation, the runtime Node suite and `git diff --check`
passed. The final regex suite with `--include-ignored` passed all six tests.

The one-iteration evaluator smoke benchmark validated all phases: fresh session
14,176,209 ns, reused session 111,833 ns, and bounded-loop/reset/recovery
62,432,083 ns. These are observations, not performance thresholds; this change
adds evidence and documentation without altering production regex execution.
The standalone ignored evaluator startup measurement was not run. The broader
intended-language report remains incomplete and was only validated without
execution; these tests do not claim whole-language completion. No browser regex
engine or JVM regex equivalence is claimed.
