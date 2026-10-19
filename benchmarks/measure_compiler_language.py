#!/usr/bin/env python3
"""Measure the integrated compiled program, validating its complete observation."""
import argparse
from datetime import datetime, timezone
import gzip
import hashlib
import json
from pathlib import Path
import platform
import statistics
import subprocess

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
subprocess.run(["cargo", "build", "--locked", "--quiet"], cwd=ROOT, check=True, timeout=180)
binary = ROOT / "target/debug/light-cljs"
fixture = ROOT / "tests/fixtures/namespaces/integrated"
javascript = subprocess.check_output(
    [str(binary), "--compile", "--no-cache", "--entry", "integrated.main", "--source-root", str(fixture)],
    cwd=ROOT, timeout=30)
marker = b" = // Original bounded runtime"
assert javascript.count(marker) == 1, "namespace program must embed exactly one runtime"
output = args.output / "integrated.js"
output.write_bytes(javascript)
script = r'''
const fs = require("node:fs"), vm = require("node:vm");
const source = fs.readFileSync(process.argv[1], "utf8").replace(
  " = // Original bounded runtime",
  " = globalThis.lightProgramRuntime = // Original bounded runtime");
const measurements = {};
globalThis.lightInspectValue = value => lightProgramRuntime.inspect(value);
globalThis.lightCaseMetadata = [["integrated-language", "integrated.main/run"]];
globalThis.lightCases = [observe => {
  const start = performance.now(), compiled = new vm.Script(source);
  const parsed = performance.now(), run = compiled.runInThisContext();
  const loaded = performance.now(), result = run(observe), finished = performance.now();
  measurements.parse_ms = parsed - start;
  measurements.initialize_ms = loaded - parsed;
  measurements.execute_ms = finished - loaded;
  measurements.total_ms = finished - start;
  return result;
}];
vm.runInThisContext(fs.readFileSync(process.argv[2], "utf8"));
process.stderr.write(JSON.stringify(measurements) + "\n");
'''
expected = (fixture / "expected.json").read_text()
samples = []
for _ in range(9):
    result = subprocess.run(["node", "-e", script, str(output.resolve()),
                             str(ROOT / "tests/support/execution.js")],
                            capture_output=True, text=True, check=True, timeout=30)
    assert result.stdout == expected, "benchmark changed value, effects, or stdout"
    samples.append(json.loads(result.stderr.rstrip().split("\n")[-1]))
sha = lambda data: hashlib.sha256(data).hexdigest()
# The embedded runtime: the source without its `//@` manifest header lines (see build.rs).
runtime = b"".join(line for line in (ROOT / "src/js_runtime.js").read_bytes().splitlines(keepends=True)
                   if not line.lstrip().startswith(b"//@"))
small_programs = []
for label, source, expected_value in [("primitive", "42", 42), ("core-call", "(+ 1 2)", 3)]:
    emitted = subprocess.check_output([str(binary), "--compile", "--text", source], cwd=ROOT, timeout=30)
    small_path = args.output / (label + ".js")
    small_path.write_bytes(emitted)
    observed = subprocess.check_output(
        ["node", "-e", "const fs=require('node:fs'),vm=require('node:vm');process.stdout.write(JSON.stringify(vm.runInThisContext(fs.readFileSync(process.argv[1],'utf8'))));", str(small_path.resolve())],
        text=True, timeout=30)
    assert json.loads(observed) == expected_value, "small program result changed"
    small_programs.append({"name": label, "source": source, "value": expected_value,
                           "bytes": len(emitted), "gzip_bytes": len(gzip.compress(emitted, mtime=0)),
                           "runtime_preludes": emitted.count(marker), "sha256": sha(emitted)})
report = {
    "measured_at": datetime.now(timezone.utc).isoformat(),
    "node": subprocess.check_output(["node", "--version"], text=True).strip(),
    "platform": platform.platform(), "machine": platform.machine(),
    "binary_sha256": sha(binary.read_bytes()), "runtime_sha256": sha(runtime),
    "method": "Nine fresh Node processes; vm.Script parse, namespace initialization, and integrated run timed separately. Process launch, source reads, and result encoding excluded. Complete value/effects/stdout checked on every sample. OS caches/scheduling uncontrolled; these are Node measurements, not browser measurements.",
    "program": "integrated.main", "source_root": str(fixture.relative_to(ROOT)),
    "fixture_sha256": {str(path.relative_to(fixture)): sha(path.read_bytes())
                       for path in sorted(fixture.rglob("*")) if path.is_file()},
    "harness_sha256": sha((ROOT / "tests/support/execution.js").read_bytes()),
    "sha256": sha(javascript), "bytes": len(javascript),
    "gzip_bytes": len(gzip.compress(javascript, mtime=0)),
    "runtime_bytes": len(runtime), "runtime_gzip_bytes": len(gzip.compress(runtime, mtime=0)),
    "small_programs": small_programs,
    "samples": samples,
    "median_ms": {name: statistics.median(sample[name] for sample in samples) for name in samples[0]},
    "expected": json.loads(expected),
}
(args.output / "output-startup.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps({key: report[key] for key in ("bytes", "gzip_bytes", "median_ms")}))
