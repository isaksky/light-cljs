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
