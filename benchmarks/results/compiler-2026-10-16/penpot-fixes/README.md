# Evidence for the Penpot performance fixes

See [the report](../../../penpot-investigation.md) for the implementation,
measurements and remaining limitations. These runs use the working-tree fixes
on top of `4d2a0d69004dd1c993aab3a65e3080079858e4bc`; the older diagnostic
instrumentation and ablation switches are not present in this binary.

## Reproduce the builds

From the repository root, after building and testing the release binary:

```sh
bb benchmarks/penpot.bb /private/tmp/light-cljs-penpot-rerun
```

The driver uses the existing port under `research/penpot-light` and the existing
upstream shadow project under `research/penpot/frontend`. It does not provision
or patch those projects. It launches fresh processes, sequentially, with the
original source roots and macro limits. Shadow's `.shadow-cljs` compiler cache
and `target/index.js` are removed before each shadow build. OS file caches and
installed dependencies are retained. All timings include process startup.

## Local artifacts

Large emitted JavaScript files, generated fixtures and the raw CPU sample remain
in `/private/tmp/light-cljs-penpot-fixed-20261016`. The untouched baseline binary
for the synthetic comparison is
`/private/tmp/light-cljs-investigation-20261016/baseline`. The original Penpot
outputs and timing logs remain in
`/private/tmp/claude-501/-Users-admin-src-light-cljs/ef390548-9edc-4e71-ba3a-a2837e664e83/scratchpad/bench`.

The first new `main-none` run was sampled for five seconds late in compilation.
Its timing is retained as a diagnostic run; the clean repeat is the result used
in the final comparison. Neither the advanced build nor the clean repeat was
sampled. `sample-stages.bb` attributes each leaf sample to its innermost compiler
phase, avoiding double-counting nested analysis. Its 449 samples cover only a
short interval, not the whole compilation.

Input hashing sorts all regular files under the five port roots, then hashes
each repository-relative UTF-8 path followed by its exact contents, with no
separator. `inputs.edn` records the initial hash and host; `inputs-after.edn`
records the verification after the builds. The port is not modified by this
work.

The verification script removes the single embedded runtime factory before
comparing application code, and also records complete file hashes. It checks
syntax with Node. `verify-fonts.bb` accounts separately for the 1,910 font UUIDs
already present in the current generated port input that differ from the old
benchmark artifacts. It matches catalog IDs, checks current emitted UUIDs
against the source, substitutes only those UUID constructor arguments, and
asserts that the remaining application bodies are identical. The embedded
advanced runtime also matches exactly; none/render runtime growth is 1,344
bytes. These checks do not bundle or execute Penpot and cannot establish the
correctness of the original port. All 22 synthetic modules execute in Node and
return the expected values.

`results.edn` contains all six new Penpot/shadow measurements and marks the
diagnostic main run with `:sampled true`. `probes.edn` and `probe-driver.log`
contain the 22 synthetic measurements and runtime-check completion. The scripts
here are the copies used in the local artifact directory; copy them there to
rerun comparisons against the large outputs. `compare-output.bb` prepares the
main application-body files consumed by `verify-fonts.bb`. Run them from the
repository root. `hashes.txt` identifies the tested binaries, raw sample and
local implementation patch. Large outputs and that patch are kept outside Git.

`suite.log` records the full regular release suite: 1,363 passed, 0 failed,
106 ignored. Optional ignored reference/oracle suites were not enabled.
`clippy.log` and the three `node-*.log` files record the other checks;
`cargo fmt --check` also passed.
