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
