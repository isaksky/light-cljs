# Second Penpot optimization pass

Production source starts at `415620c`, which includes the first performance pass.
The benchmark executable includes the uncommitted second-pass changes described
in `benchmarks/penpot-investigation.md`. All builds are sequential cold processes.
The port's 996 input files have the same path/content hash as the first pass.

Raw generated JavaScript and full test logs are in
`/private/tmp/light-cljs-phase2f`. The release executable and implementation patch
are saved there too; `name_index.rs` is saved separately because it was a new,
untracked file when the patch was captured. The large output files are not
checked in. `verify.bb OUTPUT-DIRECTORY` compares application output against the
first-pass output, excluding the embedded runtime, and runs `node --check`.

Use `bb benchmarks/penpot.bb OUTPUT-DIRECTORY` from the repository root after
`cargo build --release` to repeat the five production/shadow jobs. This driver
uses the existing port and does not regenerate it.

The separate diagnostic build adds exclusive wall-time scopes. It does not
change compiler behavior and is not used for clean benchmark timings. From the
repository root, run `bb benchmarks/results/compiler-2026-10-16/penpot-phase2/prepare-profile.bb`,
then `cargo build --release --manifest-path /private/tmp/light-cljs-phase2-profile/Cargo.toml`,
then `bb benchmarks/results/compiler-2026-10-16/penpot-phase2/profile-run.bb`.
The recorded diagnostic render completes in 44.59 seconds at 823,869,440 bytes
peak RSS. Timing instrumentation has overhead; its buckets describe that run.

The production full suite passed 1,372 tests, with 0 failed and 106 explicitly
ignored tests. Strict Clippy, formatting, and all three Node runtime/helper/
harness workflows passed. Penpot output equivalence and syntax checks do not
establish that the original port runs correctly; it has not been bundled/run.

The final cold results are 45.04 s / 0.759 GiB for render, 481.76 s / 2.977 GiB
for main-none, and 481.44 s / 3.027 GiB for main-advanced. Fresh shadow release is
60.38 s / 4.323 GiB; fresh shadow development is 31.80 s / 2.122 GiB.
`verification.edn` records unchanged application bodies, identical advanced
output, and successful syntax checks. `inputs-after.edn` verifies unchanged port
inputs; `probe-verification.log` records the three executed synthetic outcomes.
