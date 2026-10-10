# Persistent collection helper output/startup samples

Generated with:

```sh
python3 -B benchmarks/measure_compiler_interop.py --collection-helpers \
  --output benchmarks/results/compiler-2026-10-10/collection-helpers
```

`output-startup.json` retains sources, validated results, binary/runtime hashes,
raw/gzip byte counts and nine fresh Node parse/evaluation samples per program.
The emitted JavaScript files retain exactly what was measured. These timings
exclude Node launch and file reads; OS caches and scheduling are uncontrolled.
See `tests/reference/compiler-collection-helpers.md` for conformance evidence and
comparison with the prior core-protocol milestone.
