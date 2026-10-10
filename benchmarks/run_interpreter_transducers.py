#!/usr/bin/env python3
"""Save paired transducer samples, binaries, exact sources and optional old-path A/B.

The old baseline's archived harness and exact original fixtures define the A/B
scope. The general regression driver's whole-fixture identity check is unchanged.
"""
import argparse
import csv
import io
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import time

from run_interpreter import ROOT, build, capture, distribution, save_json, sha


def measured(out, name, command):
    print("Measuring", name, flush=True)
    try:
        result = capture([str(arg) for arg in command])
    except subprocess.CalledProcessError as error:
        (out / f"{name}.csv").write_text(error.stdout or "")
        (out / f"{name}.log").write_text(error.stderr or "")
        raise
    (out / f"{name}.csv").write_text(result.stdout)
    (out / f"{name}.log").write_text(json.dumps([str(arg) for arg in command]) + "\n" + result.stderr)
    return result.stdout


def summarize(path):
    groups = {}
    per_op = ["elapsed_ns", "allocations", "reallocations", "requested_bytes"]
    absolute = ["live_requested_bytes", "peak_requested_bytes", "heap_values", "names", "sources",
                "executable_units", "namespaces", "vars", "roots", "slots"]
    for row in csv.DictReader(path.open()):
        group = groups.setdefault(row["scenario"] + "/" + row["phase"], {})
        for field in per_op + absolute:
            value = int(row[field]) / (int(row["iterations"]) if field in per_op else 1)
            group.setdefault(field, []).append(value)
    return {key: {field: distribution(values) for field, values in fields.items()}
            for key, fields in groups.items()}


def old_paths(out, baseline):
    """Never pretend the expanded fixture tree matches the old driver manifest.

    Run its unchanged archived harness against both libraries instead. New
    fixtures are excluded by that source's include_strs, not by weakening the
    existing run_interpreter.py equality check.
    """
    manifest = json.loads((baseline / "manifest.json").read_text())
    archived = out / "old-path-source"
    archived.mkdir()
    with tarfile.open(baseline / "sources.tar.gz") as archive:
        archive.extractall(archived, filter="data")
    for path, digest in manifest["sources"].items():
        if sha(archived / path) != digest:
            raise RuntimeError(f"baseline archived source changed: {path}")
    for path, digest in manifest["fixtures"].items():
        if sha(ROOT / path) != digest:
            raise RuntimeError(f"old-path fixture changed: {path}; choose a matching baseline")
    for name, digest in manifest["binaries"].items():
        if sha(baseline / "bin" / name) != digest:
            raise RuntimeError(f"baseline executable changed: {name}")
    result = capture(["cargo", "build", "--locked", "--release", "--lib", "--message-format=json"])
    (out / "old-path-build.log").write_text(result.stderr)
    artifacts = [json.loads(line) for line in result.stdout.splitlines() if line.startswith("{")]
    libraries = [Path(path) for entry in artifacts if entry.get("reason") == "compiler-artifact"
                 and entry["target"]["name"] == "light_cljs"
                 for path in entry["filenames"] if path.endswith(".rlib")]
    if len(libraries) != 1:
        raise RuntimeError(f"expected one current library: {libraries}")
    for instrumented in [False, True]:
        suffix = "allocations" if instrumented else "timings"
        current = out / "bin" / f"old-path-current-{suffix}"
        command = ["rustc", "--edition=2024", "-C", "opt-level=3", str(archived / "benches/interpreter.rs"),
                   "--extern", f"light_cljs={libraries[0]}", "-L", str(libraries[0].parent), "-o", str(current)]
        if instrumented:
            command += ["--cfg", 'feature="count-allocations"']
        capture(command)
        old = out / "bin" / f"old-path-baseline-{suffix}"
        shutil.copy2(baseline / "bin" / ("interpreter-allocations" if instrumented else "interpreter"), old)
    save_json(out / "old-path-identity.json", {
        "baseline": str(baseline), "archive_sha256": sha(baseline / "sources.tar.gz"),
        "harness_sha256": sha(archived / "benches/interpreter.rs"),
        "allocator_sha256": sha(archived / "benches/support/interpreter_allocations.rs"),
        "current_library_sha256": sha(libraries[0]), "exact_fixture_manifest": manifest["fixtures"],
        "scope": "Unchanged archived interpreter harness with its exact original fixtures; new transducer fixtures are a separate measurement. No general-driver manifest override.",
    })


def old_samples(out, samples, iterations):
    common = ["--samples", str(samples), "--iterations", str(iterations)]
    # Reverse order in a second round; retain all samples and unfavorable results.
    for number, labels in [(1, ["baseline", "current"]), (2, ["current", "baseline"])]:
        outputs = {}
        for label in labels:
            outputs[label] = measured(out, f"old-path-{label}-timings-{number}",
                                      [out / "bin" / f"old-path-{label}-timings", *common])
        def outcomes(text):
            return {(r["scenario"], r["phase"], r["sample"]): r["checksum"]
                    for r in csv.DictReader(io.StringIO(text))
                    if r["phase"] in {"eval", "read_analyze_eval", "invoke", "first_load", "cached_require"}}
        if outcomes(outputs["baseline"]) != outcomes(outputs["current"]):
            raise RuntimeError("old-path language outcomes changed")
    for label in ["baseline", "current"]:
        measured(out, f"old-path-{label}-allocations",
                 [out / "bin" / f"old-path-{label}-allocations", *common, "--allocations"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="new directory outside target")
    parser.add_argument("--baseline", type=Path, help="optional saved baseline from run_interpreter.py")
    parser.add_argument("--samples", type=int, default=9)
    parser.add_argument("--iterations", type=int, default=32)
    args = parser.parse_args()
    if args.samples < 9 or args.iterations < 1:
        parser.error("reports require at least 9 samples and positive iterations")
    out = args.output.expanduser().resolve()
    if out.is_relative_to(ROOT / "target"):
        parser.error("save outside target")
    out.mkdir(parents=True, exist_ok=False)
    (out / "bin").mkdir()
    files = sorted({p for folder in ["src", "benches", "tests", "examples", "docs", "benchmarks/fixtures"]
                    for p in (ROOT / folder).rglob("*") if p.is_file()} |
                   {ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "README.md",
                    Path(__file__).resolve(), ROOT / "benchmarks/run_interpreter.py",
                    ROOT / "benchmarks/interpreter.md", ROOT / "benchmarks/README.md"})
    source_hashes = {str(p.relative_to(ROOT)): sha(p) for p in files}
    with tarfile.open(out / "sources.tar.gz", "w:gz") as archive:
        for path in files:
            archive.add(path, arcname=path.relative_to(ROOT))
    environment = {
        "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "platform": platform.platform(), "machine": platform.machine(), "logical_cpus": os.cpu_count(),
        "rustc": capture(["rustc", "-Vv"]).stdout, "cargo": capture(["cargo", "-V"]).stdout,
        "python": sys.version, "git_head": capture(["git", "rev-parse", "HEAD"]).stdout.strip(),
        "git_status": capture(["git", "status", "--porcelain"]).stdout, "command": sys.argv,
        "build_environment": {k: v for k, v in os.environ.items()
                              if k.startswith(("CARGO_PROFILE_", "CARGO_BUILD_"))
                              or k in {"RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC", "RUSTUP_TOOLCHAIN", "MACOSX_DEPLOYMENT_TARGET"}},
        "method": "sequential single-thread release runs; no affinity; default opt-level=3; setup outside timing; one explicit GC per batch; instrumented elapsed_ns=0; fresh CLI includes startup, read, analysis, print and exit; OS caches uncontrolled",
    }
    if platform.system() == "Darwin":
        environment["hardware"] = capture(["sysctl", "hw.model", "hw.memsize", "hw.ncpu", "machdep.cpu.brand_string"]).stdout
    save_json(out / "environment.json", environment)
    for target in ["light-cljs", "interpreter_transducers", "interpreter", "interpreter_gc"]:
        build(out, target)
    for target in ["interpreter_transducers", "interpreter", "interpreter_gc"]:
        build(out, target, allocations=True)
    if args.baseline:
        old_paths(out, args.baseline.expanduser().resolve())
    # No compilation runs concurrently with timing.
    common = ["--samples", str(args.samples), "--iterations", str(args.iterations)]
    measured(out, "timings", [out / "bin/interpreter_transducers", *common])
    measured(out, "allocations", [out / "bin/interpreter_transducers-allocations", *common, "--allocations"])
    measured(out, "existing-timings", [out / "bin/interpreter", *common])
    measured(out, "existing-allocations", [out / "bin/interpreter-allocations", *common, "--allocations"])
    for name in ["interpreter_gc", "interpreter_gc-allocations"]:
        result = capture([str(out / "bin" / name)])
        (out / f"{name}.log").write_text(result.stdout + result.stderr)
    cli_rows = []
    for sample in range(1, args.samples + 1):
        start = time.perf_counter_ns()
        result = capture([str(out / "bin/light-cljs"), "--eval", str(ROOT / "benchmarks/fixtures/interpreter/transducers/cli.clj")])
        elapsed = time.perf_counter_ns() - start
        if result.stdout != "1001000\n" or result.stderr:
            raise RuntimeError("unexpected native CLI result")
        cli_rows.append([sample, 1, elapsed, 1001000])
    with (out / "fresh-process.csv").open("w", newline="") as file:
        writer = csv.writer(file)
        writer.writerow(["sample", "iterations", "elapsed_ns", "checksum"])
        writer.writerows(cli_rows)
    if args.baseline:
        old_samples(out, args.samples, args.iterations)
    save_json(out / "summary.json", {
        "units": "elapsed_ns/allocations/reallocations/requested_bytes per operation; live/peak requested bytes and all memory counts are absolute; GC operation means one collection",
        "results": {p.stem: summarize(p) for p in sorted(out.glob("*.csv")) if p.name != "fresh-process.csv"},
        "fresh-process_ns": distribution([r[2] for r in cli_rows]),
    })
    if any(sha(ROOT / path) != digest for path, digest in source_hashes.items()):
        raise RuntimeError("source changed during the build or measurements; preserve this run and rerun")
    save_json(out / "manifest.json", {
        "schema": 1, "sources": source_hashes,
        "fixtures": {p: h for p, h in source_hashes.items() if p.startswith("benchmarks/fixtures/")},
        "binaries": {p.name: sha(p) for p in (out / "bin").iterdir()},
        "source_archive_sha256": sha(out / "sources.tar.gz"),
    })
    print("Saved verified measurements:", out, flush=True)


if __name__ == "__main__":
    main()
