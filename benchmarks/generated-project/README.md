# Seeded large projects

`generate.bb` creates a standalone synthetic ClojureScript project for comparing
light-cljs with shadow-cljs. It requires only Babashka. The same seed and size
options produce byte-identical files, independent of the output directory.
Generated projects need no porting, npm packages, or external application code.

From the repository root:

```sh
bb benchmarks/generated-project/generate.bb /tmp/cljs-5k --seed 42 --lines 5k
bb benchmarks/generated-project/generate.bb /tmp/cljs-200k --seed 42 --lines 200k
bb benchmarks/generated-project/generate.bb /tmp/cljs-800k --seed 42 --lines 800k
```

`--lines` is approximate **nonblank, noncomment source lines** across the
canonical `.cljs` and `.cljc` sources, including deliberate waste. Code is laid
out by a small pretty printer at the usual 80 columns, so a line is an ordinary
line of Clojure. There is no padding to reach the target: the last worker
namespace can overshoot by a few hundred lines and the fixed namespaces can leave
a small undershoot. The manifest records exact nonblank and physical lines,
bytes, top-level forms, source hashes and generator hashes. Macro-loading shims
and tooling are excluded.

`--namespace-lines` (default 400, minimum 200) changes how the work is distributed:
smaller values make more namespaces, larger values make larger namespaces.
`--seed` accepts a signed 64-bit integer. The destination must be absent or empty;
the generator never replaces an existing project. Outputs belong in `/tmp` or
the gitignored `research/` directory, not in the repository's source tree.

## Build, execute, and compare

Build the current compiler first, then work in the generated directory:

```sh
cargo build --release --locked --bin light-cljs
export LIGHT="$PWD/target/release/light-cljs"
cd /tmp/cljs-5k

# Independent Babashka interpreter of the generated model:
bb oracle.bb 42 3

# Optimized light-cljs and shadow-cljs applications:
"$LIGHT" release bench
node run-light.mjs 42 3
clojure -M:shadow release bench
node out/shadow.js 42 3

# Report surviving waste markers and output sizes:
bb check-waste.bb --strict out/light/workload.main.js out/shadow.js
```

All three executions should print the same single integer. The arguments are a
runtime **input** (0 through 1,000,002, default 1) and **iterations** (1 through
1,000,000, default 1). These are independent of the generation seed. Each
iteration visits every worker and feeds its result into the next. Increasing
iterations increases runtime work without changing compilation size.

The generated `deps.edn` pins shadow-cljs **3.5.3** and allows an 8 GiB JVM heap.
Shadow needs Java, the Clojure CLI, Node, and Maven access on its first build.
Generation and the oracle need only `bb`. No npm install is required. The
generated `shadow-cljs.edn` turns off the analyzer's `:invalid-arithmetic`
warning: valid code such as `(+ (or (peek v) 3) 1)` trips its conservative nil
inference, a few times per 100k lines.

To compare other optimization levels:

```sh
"$LIGHT" release bench --optimize none
node run-light.mjs 42 3
bb check-waste.bb out/light/workload.main.js

"$LIGHT" release bench --optimize prepare
node run-light.mjs 42 3

clojure -M:shadow compile bench
node out/shadow.js 42 3
bb check-waste.bb out/shadow-dev
```

Light's generated configuration disables its persistent compiler cache and
raises macro budgets for large builds. Shadow retains its normal incremental
cache. For timing experiments distinguish warm/cold builds, JVM startup,
optimization, and execution; the generator itself makes no timing comparison.
`--stats` can be added to the light commands.

`compare.bb` times advanced builds of one generated project with both compilers,
uncached and cached, checks each output against the oracle and the waste checker,
and writes `compare-result.edn` into the project
([results of October 22, 2026](../generated-shadow-vs-light.md)):

```sh
bb benchmarks/generated-project/compare.bb /tmp/cljs-200k --light "$LIGHT" --runs 3 --warm-jvm 4
```

## How a project is made

The generator first builds a **model**: seeded expression trees plus the
protocols, records, types, multimethods, dynamic vars and macros they use. It
then renders the model to ClojureScript, choosing among equivalent spellings
(`(:k m d)`, `(get m :k d)` or `(m :k d)`; `reduce`, an atom with `doseq`, a
volatile with `run!`, or `loop` over `seq`; `(fn [x] ...)`, `#(...)` or a named
`fn`). `model.edn` is the model; `oracle.bb` interprets it. The renderer's
choices never reach the oracle.

Expressions are typed (integers, vectors, keyword maps, strings, keywords,
records, and unary/binary function values) and composed from about 100 node
kinds, so trees nest any operation inside any other of a fitting type. Every
identifier is a seeded pronounceable word, unique within its namespace, never a
`clojure.core` name or a JavaScript reserved word. At 200k lines about 74% of
nonblank lines are distinct even with numbers and indentation ignored, compared
with 244 distinct lines for the earlier fixed-template generator. The manifest
reports `:distinct-normalized-lines` and `:distinct-line-ratio`.

### What a worker namespace contains

Each worker namespace requires `clojure.string`, `clojure.set`, the common
namespace, one macro namespace, and up to three earlier workers, all under
seeded aliases. It defines, in varying numbers and shapes:

- One or two protocols with one to three methods of one or two arities, some
  through a macro that expands to `defprotocol`.
- One or two records with one to four fields, implementing their own protocols
  and protocols from dependencies inline, sometimes `IFn` too, some through a
  macro that expands to `defrecord`.
- Sometimes a `deftype` with a `^:mutable` field updated by `set!`, implementing
  `IFn`, `ICounted` and `ILookup`.
- Sometimes a multimethod (plain or macro-defined `defmulti`) over namespaced
  keywords with a `derive` hierarchy and a `:default` method.
- Sometimes a `^:dynamic` var, rebound with `binding` by this and later namespaces.
- `extend-protocol`/`extend-type` of dependency protocols to `number`, `string`
  and `default`, and of visible protocols to dependency records.
- `defmethod`s on dependency multimethods, including methods for new keywords
  that `derive` from the dependency's (`::alias/tag` or fully qualified).
- Private helpers (`defn-`) that may call later helpers through `declare`.
- Public functions with one arity, defaulting multiple arities, or `& rest`,
  some defined by a macro that expands to `defn`, and `def` aliases of
  dependency functions.
- Kernels, which call all of the above across namespaces, pass functions as
  values (`partial`, `comp`, `juxt`), construct dependency records with `->R`,
  `map->R` and `new`, and dispatch on numbers, strings and records.

Expression nodes cover arithmetic; map construction, lookup with defaults,
`assoc`/`dissoc`/`update`/`update-in`/`merge`/`merge-with`/`select-keys`/
`zipmap`, maps past the 8-entry array-map limit and transients; vectors with
`conj`, `assoc`, `take`, `drop`, `peek`, `last`, `sort`/`sort-by`,
`distinct`, `interleave`, `partition-all`, `map-indexed`, `mapcat`, `keep`,
`reductions`, `iterate`, a common `lazy-seq` generator, sets and `clojure.set`;
`group-by` and `frequencies`; `reduce`, `reduce-kv` over sorted maps, early
`reduced`, transducers via `transduce`, `sequence` and `into`; `for` with `:let`
and `:when`; `let`, vector and map destructuring (`:keys`, `:or`, `:as`, `&`),
`if`/`if-not`/`cond`/`case` on integers, keywords and strings/`condp`/`cond->`/
`some->`/`if-some`/`when-let`/`if-let`; `loop`/`recur` and mutually recursive
`letfn`; atoms with watches, volatiles, `delay`, `binding`; `try` with `ex-info`,
`ex-data`, `ex-message` and `finally`; metadata; `satisfies?` and `isa?`;
`clojure.string` and `name`/`keyword`/`str` reduced to integers through `count`,
`index-of` and `compare`.

### Macros

Each group of eight workers shares a seeded `.cljc` macro namespace. Its macros
use syntax quote, auto-gensyms, destructured macro arguments, `&form` (an argument
count), `&env` (`(contains? (:locals &env) sym)`), nested syntax quote, a
compile-time helper function, a macro that expands to itself until its arguments
are used up, and macros that expand to `defn`, `defprotocol`, `defrecord` and
`defmulti`. Both compilers read identical macro bodies.

Shadow additionally reads tiny `.cljs` files under `shadow-support/` that
self-require their corresponding macro namespace. This enables upstream macro
inference from ordinary runtime `:require` aliases. Light loads macros directly
through those ordinary requires and excludes `shadow-support/` from its source
roots. There are no compiler-specific computation branches or implementations.

### Runtime stays linear

Bodies that run once per element add an iteration level; calling a user
function, protocol method or multimethod costs one level. Kernels allow two
levels and every other body one, and protocol, multimethod and type bodies
call no user functions. Helpers only call later helpers, once. So each kernel
does a bounded amount of work and total runtime grows linearly with project
size. Dependency edges from worker `n` only point into the first half of earlier
workers.

Every integer expression stays in `[0, 1000003)`; products of two such values
stay far below JavaScript's exact-integer limit. Hash-ordered iteration only
feeds commutative sums, ordered folds use sorted maps, and `compare` results are
normalized. Floating point, platform hashes and JVM-only semantics never reach
the result.

## Waste is always generated

There is no switch that disables unused code. Every payload carries a unique
string sentinel recorded in `manifest.edn` with its category and what an
optimizer may legally do with it:

| Category | Where | Expectation |
| --- | --- | --- |
| `helper` | Unreferenced `def` table and function in each worker | `:removable` |
| `kernel` | Unreferenced functions over full expression trees, calling live code | `:removable` |
| `record` | Unreferenced `defrecord` | `:removable` |
| `chain` | Unused function calling the previous one in a dependency, a waste-only function and live code | `:removable` |
| `waste-only` | Ordinary-looking function reached only from `chain` | `:removable` |
| `common` | Unused functions in the common namespace | `:removable` |
| `idle` | Namespace required by each group but never called | `:removable` |
| `arity` | Extra arity of a function that is called with other arities | `:best-effort` |
| `protocol-impl` | Live record implementing a protocol nothing calls | `:best-effort` |
| `extension` | `extend-protocol` of that protocol to `string` | `:best-effort` |
| `defmethod` | Method for a keyword nothing dispatches on | `:retained` |
| `macro` | Macros nobody invokes | `:compile-time` |
| `orphan` | Namespace outside the require graph | `:never-loaded` |

A `:best-effort` payload is legal to drop but needs arity or property analysis:
a multi-arity function's dispatcher refers to every arity, and protocol methods
are prototype properties. A `defmethod` mutates a live multimethod when the
namespace loads; a compiler cannot know the keyword is never dispatched, so
both compilers keep it. Macro bodies never reach JavaScript.

`check-waste.bb` scans a JS file or a directory recursively (ignoring source
maps) and reports survivors per category. It exits nonzero if an orphan marker
appears; `--strict` also fails for any surviving `:removable` marker. An
unoptimized build should retain every `:removable`, `:best-effort` and
`:retained` marker; an advanced build should keep no `:removable` one.

String sentinels let us measure payload elimination, but their absence does
**not** prove that every associated instruction disappeared. Compare emitted
sizes and inspect generated code when investigating an optimizer regression.

## Reference model and tests

`oracle.bb` interprets `model.edn` node by node with ClojureScript semantics
where they differ from the JVM: `compare` returns -1, 0 or 1, and `satisfies?`
also counts a `default` extension. Function values receive the caller's
dynamic bindings. The oracle never reads generated source or compiler output,
and neither model nor oracle is loaded by the compiled application. The checksum
can collide, so test several seeds and inputs and treat agreement as evidence,
not proof.

```sh
# Determinism, size, line variety, readable sources (edamame), every node kind
# live at 30k lines, graph bounds, unique classified markers, output protection,
# standalone oracle, and waste-checker positive/negative controls:
bb benchmarks/generated-project/test.bb

# Also compile two seeds at light none/prepare/advanced and shadow dev/release,
# compare four input/iteration pairs against the oracle, check waste
# expectations, and fail on shadow warnings:
bb benchmarks/generated-project/test.bb --light target/release/light-cljs --shadow
```

The end-to-end checks use fresh temporary projects and remove them on exit.
Compiler regressions fail the test; the generator does not silently change its
language subset to make a failing compiler pass.

Local validation on October 21, 2026 (Apple Silicon, 32 GB; light-cljs release
binary built from `ac88cb2`; cold shadow builds, wall time including JVM start,
with shadow's own build time in parentheses):

| Seed | Requested | Code lines | Bytes | Workers | Distinct lines | Checksum (42, 3) |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| 42 | 5k | 5,046 | 256,252 | 11 | 3,915 (0.776) | `305050`: oracle, light none/prepare/advanced, shadow dev/release |
| 42 | 200k | 199,129 | 9,966,430 | 450 | 147,512 (0.741) | `484264`: oracle, light advanced, shadow release |
| 20261021 | 800k | 795,632 | 39,712,538 | 1,814 | 586,737 (0.737) | `255125`: oracle, light advanced |

| Size | Generate | Oracle | Light advanced | Shadow release | Light output (gzip) | Shadow output (gzip) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 5k | 0.24 s | 0.04 s | 0.39 s, 0.14 GB | 10.1 s (5.0 s), 1.45 GB | 214,145 (66,729) | 263,193 (58,203) |
| 200k | 7.2 s | 0.68 s | 25.4 s, 1.19 GB | 40.7 s (35.5 s), 3.44 GB | 4,097,518 (1,269,750) | 4,067,637 (1,013,132) |
| 800k | 30 s | 2.7 s | 267.7 s, 6.18 GB | not run | 16,104,527 (4,945,555) | — |

Both advanced builds removed every `:removable` marker at 5k and 200k; light did
at 800k. At 200k light kept 86 and shadow 51 of 204 unused arities, both removed
all unused protocol implementations and string extensions, and both kept all
406 unreachable `defmethod`s. Twenty further seeds (12 at 8k lines, 8 at 50k)
matched the oracle at light none and advanced. These are correctness and
elimination checks, not a controlled performance comparison.
