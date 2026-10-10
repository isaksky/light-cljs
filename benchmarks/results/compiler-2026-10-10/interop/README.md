# Compiler interop output and startup

Generated with `python3 -B benchmarks/measure_compiler_interop.py --output benchmarks/results/compiler-2026-10-10/interop`.
The JSON records source, binary/runtime/output hashes, sizes, expected numeric
results and nine fresh Node process samples per case. Each sample times V8
compilation plus execution; value validation follows timing. Emitted snapshots
are retained alongside the report. Process launch, source reading and browser
startup are not measured. Historical comparisons are not paired trials.
