# Removing QuickJS and the Pants build

Measured on November 25, 2026, on macOS 15.7.3 (24G419), Apple M3 Ultra
(virtual), 24 vCPUs and 32 GiB of memory. Rust and Cargo were 1.99.1,
Babashka 1.13.229 and Node 26.12.1. Master was `fb7de495`, also the tip
of master at the start of the comparison. The `remove-quickjs` tip was
`50890488`. Master's V8 build includes QuickJS. The branch's V8 build
uses V8 for compile-time macros, data readers and interactive sessions.

Runs were unreserved, with no other benchmark worker. Each pair used the
same machine and inputs, switching which revision went first every round.
Commands completing within 30 seconds used ten runs per revision. Slower
commands used five, and full test suites used one run per build. Tables
give medians and observed minimum–maximum ranges. Load is the one-minute
load average sampled immediately before a command. Wall time includes
process start unless noted. Peak memory is in MiB from macOS
`/usr/bin/time -l`.

Run [remove-quickjs.bb](remove-quickjs.bb) from the branch's worktree.
Prepare a detached worktree for master and set these shell variables.

```sh
branch="$PWD"
master=/tmp/light-cljs-1138-master
logs="$branch/target/remove-quickjs-logs"
git worktree add --detach "$master" fb7de495
bb benchmarks/remove-quickjs.bb build "$logs" "$master" "$branch" 5
bb benchmarks/remove-quickjs.bb summary "$logs/build.edn"
```

## 1. Release compiler build

Both sides used `cargo build --release` with the features enabled by
default on each revision. The build target trees were
`$master/target` and `$branch/target/release-comparison`. Before each
clean build the script deleted that side's target tree. This leaves
Cargo's shared dependency cache intact. A second build in the same tree
followed a timestamp-only change to `src/main.rs`. The incremental
measurement includes the compiler fingerprint build script and Rust
recompilation it triggers. Binary sizes are for the unstripped executables.

| Measurement | Master | Branch |
| --- | ---: | ---: |
| Clean release build (s) | 131.64 [130.44–155.54] | 150.86 [124.60–162.20] |
| Rebuild after touching `src/main.rs` (s) | 120.55 [118.62–143.75] | 113.81 [113.36–131.45] |
| Release binary size (bytes) | 77,768,656 [77,768,624–77,768,656] | 76,106,448 [76,106,448–76,106,448] |
| Clean build load | 5.33 [4.19–13.25] | 4.23 [3.97–6.41] |
| Source-touch rebuild load | 5.40 [4.47–12.39] | 6.46 [4.39–7.22] |

The branch binary is 1,662,208 bytes smaller at the median (2.14%). The
clean-build median increased by 19.22 s, while the source-touch rebuild
median fell by 6.74 s. Each timing range overlaps its counterpart.

## 8. Emitted JavaScript

The script extracts the 32 single-source and four namespace fixtures from
`tests/support/ratchet_fixtures.rs` and compiles the same files with both
release binaries at each optimization level. It compares the JavaScript
with `cmp`. For TodoMVC it uses `release client --no-cache` on the shared
read-only `research/todomvc-port` inputs, with output directed into the
benchmark log tree.

```sh
bb benchmarks/remove-quickjs.bb output "$logs" "$master" "$branch"
```

| Output | Byte-identical | Master bytes | Branch bytes | Load range |
| --- | ---: | ---: | ---: | ---: |
| Size ratchet, none | 0 / 36 | | | 4.10–4.11 |
| Size ratchet, prepare | 0 / 36 | | | 4.10–4.11 |
| Size ratchet, advanced | 21 / 36 | | | 4.10–4.11 |
| TodoMVC startup module, advanced | 0 / 1 | 349,574 | 349,392 | 4.10 |

Every changed advanced ratchet is 44 bytes smaller. The first differing
compiled runtime unit in the advanced vector example loses the
`lightCljsWeakMapStores` switch removed by `b6f143e3`. At none and
prepare the full readable runtime also loses Pants comments and branches
from `36a2de8c`. These changes alter the keyed runtime identifiers
embedded in the output. The branch additionally includes the live
protocol-name fix and stable `satisfies?` optimization (`dbe6511f`,
`fdb245dd`, `646d0f3f`).

**Byte-identity check failed.** light-cljs-1152 tracks attribution of the
remaining byte drift and the decision on whether to accept it. TodoMVC
SHA-256 was `0bc64c35bdd4b5aaa28cfb755f2fc081ffe8e3ce39131bfd12a23a8870e2b8b3`
on master and `cc7681507c8a03e5ca5e8785e47172331f0fb6ec9d433fa46036c7223c623a57`
on the branch.

## 4. Interactive sessions

```sh
bb benchmarks/remove-quickjs.bb sessions "$logs" "$master" "$branch" 10
bb benchmarks/remove-quickjs.bb summary "$logs/sessions.edn"
```

The library workload requires `clojure.string`, `clojure.set`, `clojure.walk`,
`clojure.edn` and `cljs.reader` in one evaluation. The macro script expands
400 calls to a macro that introduces a local binding and adds three
terms. The REPL follows light-cljs-305 and light-cljs-366, with 3,000
`(def vI (vec (range 50)))` forms each followed by
`(reduce + (map inc vI))`. The macro script and REPL both use
`--timeout-ms 120000 --poll-budget 1000000000000`. Every run checked
the expected result and result line count.

| Workload | Master wall (s) | Branch wall (s) | Master peak (MiB) | Branch peak (MiB) | Load range |
| --- | ---: | ---: | ---: | ---: | ---: |
| `--eval --text '(+ 1 2)'` | 0.037 [0.033–0.122] | 0.035 [0.033–0.105] | 32.35 [32.11–33.06] | 31.98 [31.73–32.28] | 3.57 |
| Five libraries | 0.043 [0.042–0.049] | 0.043 [0.042–0.048] | 34.54 [34.45–34.91] | 34.22 [34.02–34.45] | 3.52–3.57 |
| 400 macro calls | 0.167 [0.165–0.194] | 0.081 [0.078–0.099] | 54.45 [54.09–55.19] | 67.51 [63.55–71.67] | 3.52 |
| 6,000 REPL forms | 0.423 [0.418–0.439] | 0.424 [0.419–0.446] | 82.97 [81.39–85.17] | 82.75 [80.83–84.69] | 3.44–3.52 |

The macro-heavy script takes 49% of master's wall time and adds 13.06
MiB (24.0%) at the median. Both changes exceed the observed ranges.
A separate `LIGHT_V8_STATS=1` run showed one V8 isolate on each side for
`(+ 1 2)` and one on master versus two on the branch for the macro
script. Macro execution starts a second V8 isolate on the branch in
place of master's QuickJS runtime. The branch requires at least 16 MiB
for a V8 heap, and these runs use the default 64 MiB setting. The user
must decide whether the macro-heavy memory increase is acceptable.

## 3. Compiler data readers

```sh
bb benchmarks/remove-quickjs.bb readers "$logs" "$master" "$branch" 10
bb benchmarks/remove-quickjs.bb summary "$logs/readers.edn"
```

The script links a small driver against each revision's release library
because configuring a compiler data reader is a library API. It compiles
a namespace with a registered `#bench/named` reader and an unknown
`#bench/fallback` tag handled by the default reader. The small source
contains 16 calls of each kind, and the scaled source contains 3,000
of each. Node checks the emitted program's sum outside the timed command.

| Reader calls | Master wall (s) | Branch wall (s) | Master peak (MiB) | Branch peak (MiB) | Load range |
| --- | ---: | ---: | ---: | ---: | ---: |
| 32 | 0.061 [0.059–0.105] | 0.024 [0.023–0.032] | 20.22 [20.16–23.86] | 26.91 [26.80–27.03] | 4.50–4.62 |
| 6,000 | 0.167 [0.165–0.168] | 0.065 [0.063–0.067] | 27.55 [27.41–27.67] | 49.55 [47.23–51.30] | 4.57–4.62 |

The scaled workload takes 39% of master's wall time and adds 22.00 MiB
(80%) at the median. The small workload also speeds up and adds 6.69 MiB.
Both memory increases exceed the observed ranges. A diagnostic run of
the scaled source reports one V8 isolate on the branch and zero on
master, since the fixture only needs a reader engine. The branch uses
a dedicated V8 reader isolate with a 16 MiB initial allowance and a
64 MiB ceiling at the default limit. The user must decide whether to
accept the higher peak memory for these tagged-literal workloads.

## 2. Clean project compilations

Each run uses `release BUILD --no-cache` with a fresh output directory.
Each project runs once per revision per round with explicit `--jobs 1`,
and again with the jobs flag omitted. Both revisions default to one job.
The generated projects use their checked-in generator settings for macro
limits, including the 2 GiB macro heap. Their inputs were regenerated
because the historical `research/shadow-credibility/projects` directory
was absent. The seed is 42, with 199,129 and 795,693 nonblank lines
respectively.

```sh
for size in 200k 800k
do
  bb benchmarks/generated-project/generate.bb "$logs/projects/workload-$size" --seed 42 --lines "$size"
done
bb benchmarks/remove-quickjs.bb compile "$logs" "$master" "$branch" "$logs/projects/workload-200k" bench 10
bb benchmarks/remove-quickjs.bb compile "$logs" "$master" "$branch" "$logs/projects/workload-800k" bench 10
```

TodoMVC is the available `research/todomvc-port`. Simple and Flow were
regenerated from a copy of the re-frame examples at `1a1bf1d` using the
local porter and shadow-cljs 3.5.3. Their inputs are shared between the two
release binaries. The existing porter needed the Flow fixes previously
described in the re-frame comparison. Treat vector and list namespace
clauses alike, remove the redundant reader-conditional `:include-macros`
from rewrite-clj's libspec, expose the SCI `deftime` bodies, replace
SCI's analyzer-initialization form with `nil`, and have the unused
self-hosted `copy-ns` branch report its analyzer dependency.
[re-frame benchmark port notes](re-frame-vs-shadow.md#port-edits-by-cause)

```sh
mkdir -p "$logs/ported"
cp -R research/re-frame research/shadow-port-tool "$logs/ported/"
porter="$logs/ported/shadow-port-tool"
port_deps="{:aliases {:light-cljs/port {:extra-deps {light-cljs/port {:local/root \"$porter\"}}}}}"
for app in simple flow
do
  (
    cd "$logs/ported/re-frame/examples/$app" &&
    npm install --offline --no-audit --no-fund --ignore-scripts &&
    bb -e '(let [p "deps.edn"] (spit p (clojure.string/replace (slurp p) "\"3.2.0\"" "\"3.5.3\"")))' &&
    if [ "$app" = flow ]; then
      bb -e '(let [p (first *command-line-args*)] (spit p (clojure.string/replace (slurp p) "(= :list (z/tag inner))" "(#{:list :vector} (z/tag inner))")))' "$porter/src/light_cljs/port.clj"
    fi &&
    clojure -Sdeps "$port_deps" -M:light-cljs/port -m shadow.cljs.devtools.cli run light-cljs.port/main :client "$logs/ported/$app-port"
  )
done
bb benchmarks/remove-quickjs.bb repair-flow "$logs/ported/flow-port"
for port in "$logs/ported/simple-port" "$logs/ported/flow-port" "$branch/research/todomvc-port"
do
  bb benchmarks/remove-quickjs.bb compile "$logs" "$master" "$branch" "$port" client 10
done
bb benchmarks/remove-quickjs.bb summary "$logs/compile.edn"
```

| Project | Jobs flag | Master wall (s) | Branch wall (s) | Master peak (MiB) | Branch peak (MiB) | Load range |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Generated 200k | 1 | 5.104 [4.988–5.277] | 5.096 [4.998–5.229] | 884.65 [859.08–908.31] | 874.09 [863.95–893.38] | 3.71–4.63 |
| Generated 200k | default | 5.025 [4.986–5.101] | 4.997 [4.951–5.099] | 877.67 [866.17–895.41] | 886.92 [872.70–904.39] | 3.65–4.58 |
| Generated 800k | 1 | 23.123 [22.783–79.194] | 22.783 [22.410–25.301] | 2764.20 [1976.42–2933.36] | 2820.23 [2508.53–2920.02] | 3.62–16.73 |
| Generated 800k | default | 22.961 [22.261–66.730] | 22.826 [22.449–23.515] | 2493.41 [2031.52–2877.77] | 2517.41 [2191.19–2857.47] | 3.76–22.88 |
| TodoMVC | 1 | 0.181 [0.175–0.192] | 0.180 [0.174–0.189] | 86.99 [83.70–90.17] | 86.52 [83.28–90.89] | 7.86–8.11 |
| TodoMVC | default | 0.180 [0.174–0.186] | 0.178 [0.174–0.198] | 87.05 [82.78–91.91] | 86.95 [83.22–94.52] | 7.86–8.11 |
| Simple | 1 | 0.159 [0.155–0.241] | 0.158 [0.152–0.242] | 84.26 [80.08–89.03] | 84.20 [77.25–85.33] | 8.11–8.29 |
| Simple | default | 0.159 [0.155–0.163] | 0.159 [0.154–0.162] | 84.23 [81.59–87.34] | 81.34 [76.72–84.77] | 8.11–8.29 |
| Flow | 1 | 0.901 [0.879–1.049] | 0.892 [0.879–0.920] | 265.29 [263.92–274.95] | 268.88 [264.67–277.20] | 4.25–4.66 |
| Flow | default | 0.895 [0.880–0.915] | 0.889 [0.874–0.919] | 271.16 [259.92–274.91] | 268.31 [266.80–273.50] | 4.25–4.53 |

Wall and peak-memory ranges overlap for every project and jobs setting.
The longest 800k runs occurred during the unreserved series and remain
in the reported ranges. Typical runs took 23 s, so both 800k settings
used ten pairs.
