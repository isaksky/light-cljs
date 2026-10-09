#!/usr/bin/env python3
"""Save a reproducible native baseline; optionally rerun saved binaries for A/B.

Standard library only. Build time is excluded. Never overwrite an earlier run.
"""
import argparse
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import sys
import tarfile
import time

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "benchmarks/fixtures/interpreter"
EXPECTED = {"closures_loop.clj": "134400\n", "collections.clj": "2763520\n",
            "lazy.clj": "2796160\n", "main.clj": "62182\n"}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def capture(command):
    return subprocess.run(command, cwd=ROOT, check=True, text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=300)


def save_json(path, data):
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")


def build(out, target, allocations=False):
    command = (["cargo", "build", "--release", "--bin", "light-cljs"] if target == "light-cljs"
               else ["cargo", "bench", "--no-run", "--bench", target])
    command += ["--message-format=json"]
    if allocations:
        command += ["--features", "count-allocations"]
    print("Building", target, "allocations" if allocations else "timing", flush=True)
    result = capture(command)
    with (out / "build.log").open("a") as log:
        log.write(json.dumps(command) + "\n" + result.stderr)
    artifacts = [json.loads(line) for line in result.stdout.splitlines() if line.startswith("{")]
    paths = [a["executable"] for a in artifacts if a.get("reason") == "compiler-artifact"
             and a.get("target", {}).get("name") == target and a.get("executable")]
    if len(paths) != 1:
        raise RuntimeError(f"expected one executable for {target}: {paths}")
    name = target + ("-allocations" if allocations else "")
    destination = out / "bin" / name
    shutil.copy2(paths[0], destination)
    return destination


def run_saved(out, label, bins, args):
    for name in ["interpreter", "tokenizer", "parser"]:
        command = [str(bins / name), "--samples", str(args.samples)]
        command += (["--iterations", str(args.iterations)] if name == "interpreter"
                    else ["--ms", str(args.frontend_ms), *args.frontend_source])
        print("Measuring", label, name, flush=True)
        result = capture(command)
        (out / f"{label}-{name}.csv").write_text(result.stdout)
        (out / f"{label}-{name}.log").write_text(json.dumps(command) + "\n" + result.stderr)
    command = [str(bins / "interpreter-allocations"), "--samples", str(args.samples),
               "--iterations", str(args.iterations), "--allocations"]
    result = capture(command)
    (out / f"{label}-allocations.csv").write_text(result.stdout)
    (out / f"{label}-allocations.log").write_text(json.dumps(command) + "\n" + result.stderr)


def distribution(values):
    median = statistics.median(values)
    return {"samples": len(values), "median": median, "min": min(values), "max": max(values),
            "mad": statistics.median(abs(value - median) for value in values)}


def summarize(path):
    groups = {}
    for row in csv.DictReader(io.StringIO(path.read_text())):
        if "scenario" in row:
            key = row["scenario"] + "/" + row["phase"]
            ns = int(row["elapsed_ns"]) / int(row["iterations"])
        elif "input" in row:
            key = row["input"]
            ns = float(row["seconds"]) * 1e9 / int(row["iterations"])
        else:
            key = row["fixture"]
            ns = int(row["elapsed_ns"])
        groups.setdefault(key, []).append(ns)
    return {key: distribution(values) for key, values in sorted(groups.items())}


def compare_results(out, summaries):
    comparison = {}
    for bench in ["interpreter", "tokenizer", "parser", "fresh-process"]:
        before, after = summaries[f"baseline-{bench}"], summaries[f"current-{bench}"]
        if before.keys() != after.keys():
            raise RuntimeError(f"comparison scenario mismatch: {bench}")
        comparison[bench] = {
            key: {"baseline_ns": before[key]["median"], "current_ns": after[key]["median"],
                  "change_percent": 100 * (after[key]["median"] / before[key]["median"] - 1)}
            for key in before
        }
    # Compare language outcomes, not optimized IR sizes or amounts reclaimed.
    def outcomes(label):
        return {(r["scenario"], r["phase"], r["sample"]): r["checksum"]
                for r in csv.DictReader((out / f"{label}-interpreter.csv").open())
                if r["phase"] in {"eval", "read_analyze_eval", "invoke", "first_load", "cached_require"}}
    if outcomes("baseline") != outcomes("current"):
        raise RuntimeError("baseline/current interpreter outcomes differ")
    save_json(out / "comparison.json", comparison)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path, help="new directory outside target; never overwritten")
    parser.add_argument("--compare", type=Path, help="saved baseline directory from this driver")
    parser.add_argument("--samples", type=int, default=9)
    parser.add_argument("--iterations", type=int, default=32)
    parser.add_argument("--frontend-ms", type=int, default=100)
    parser.add_argument("--frontend-source", action="append", default=[])
    args = parser.parse_args()
    if min(args.samples, args.iterations, args.frontend_ms) < 1:
        parser.error("sample counts, iterations and duration must be positive")
    out = args.output.expanduser().resolve()
    if out.is_relative_to(ROOT / "target"):
        parser.error("save outside target so cargo clean cannot remove the baseline")
    out.mkdir(parents=True, exist_ok=False)
    (out / "bin").mkdir()
    files = sorted([p for directory in ["src", "benches", "tests", "examples", "docs", "benchmarks/fixtures"]
                    for p in (ROOT / directory).rglob("*") if p.is_file()] +
                   [ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "README.md", Path(__file__).resolve(),
                    ROOT / "benchmarks/compare_interpreter_reuse.py"])
    hashes = {str(p.relative_to(ROOT)): sha(p) for p in files}
    fixtures = {p: digest for p, digest in hashes.items() if p.startswith("benchmarks/fixtures/")}
    baseline = args.compare.expanduser().resolve() if args.compare else None
    if baseline:
        prior = json.loads((baseline / "manifest.json").read_text())
        if prior["fixtures"] != fixtures:
            raise RuntimeError("baseline fixtures changed; choose a matching baseline")
        for name, digest in prior["binaries"].items():
            if sha(baseline / "bin" / name) != digest:
                raise RuntimeError(f"saved baseline binary changed: {name}")
        if prior["frontend_sources"] != {p: sha(ROOT / p) for p in args.frontend_source}:
            raise RuntimeError("baseline frontend source inputs changed")
    environment = {
        "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "platform": platform.platform(), "machine": platform.machine(),
        "python": sys.version, "logical_cpus": os.cpu_count(),
        "rustc": capture(["rustc", "-Vv"]).stdout,
        "cargo": capture(["cargo", "-V"]).stdout,
        "git_head": capture(["git", "rev-parse", "HEAD"]).stdout.strip(),
        "git_status": capture(["git", "status", "--porcelain"]).stdout,
        "build_environment": {key: value for key, value in os.environ.items()
                              if key.startswith(("CARGO_PROFILE_", "CARGO_BUILD_"))
                              or key in {"RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC", "RUSTUP_TOOLCHAIN", "MACOSX_DEPLOYMENT_TARGET"}},
        "command": sys.argv,
        "method": "single thread; fresh process, OS caches uncontrolled; resident phases primed; no affinity; release opt-level=3 unless environment/config overrides",
    }
    if platform.system() == "Darwin":
        environment["hardware"] = capture(["sysctl", "hw.model", "hw.memsize", "hw.ncpu", "machdep.cpu.brand_string"]).stdout
    elif Path("/proc/cpuinfo").exists():
        environment["hardware"] = Path("/proc/cpuinfo").read_text()
        environment["memory"] = Path("/proc/meminfo").read_text()
    save_json(out / "environment.json", environment)
    # Save exact sources, including uncommitted interpreter work; HEAD alone
    # cannot describe a baseline made during a sequence of implementation tasks.
    with tarfile.open(out / "sources.tar.gz", "w:gz") as archive:
        for file in files:
            archive.add(file, arcname=file.relative_to(ROOT))
    for target in ["light-cljs", "interpreter", "tokenizer", "parser"]:
        build(out, target)
    build(out, "interpreter", allocations=True)
    manifest = {"schema": 1, "sources": hashes, "fixtures": fixtures,
                "frontend_sources": {p: sha(ROOT / p) for p in args.frontend_source},
                "binaries": {p.name: sha(p) for p in (out / "bin").iterdir()},
                "baseline": str(baseline) if baseline else None}
    save_json(out / "manifest.json", manifest)
    labels = [("baseline", baseline / "bin")] if baseline else []
    labels.append(("current", out / "bin"))
    for label, bins in labels:
        run_saved(out, label, bins, args)
    # Alternate order each round when comparing. Each child starts a new
    # interpreter, reads files, prints its result and exits. No build is timed.
    cold = {label: [] for label, _ in labels}
    for sample in range(1, args.samples + 1):
        for filename, expected in EXPECTED.items():
            for label, bins in labels[::1 if sample % 2 else -1]:
                command = [str(bins / "light-cljs"), "--eval", "--source-root", str(FIXTURES), str(FIXTURES / filename)]
                started = time.perf_counter_ns()
                result = capture(command)
                elapsed = time.perf_counter_ns() - started
                if result.stdout != expected or result.stderr:
                    raise RuntimeError(f"{label}/{filename}: unexpected output {result.stdout!r} {result.stderr!r}")
                cold[label].append([filename, sample, elapsed, result.stdout.strip()])
    for label, rows in cold.items():
        with (out / f"{label}-fresh-process.csv").open("w", newline="") as file:
            writer = csv.writer(file)
            writer.writerow(["fixture", "sample", "elapsed_ns", "result"])
            writer.writerows(rows)
    diagnostic = capture([str(out / "bin/interpreter"), "--samples", "1", "--iterations", "2", "--diagnostics"])
    (out / "diagnostics.log").write_text(diagnostic.stderr)
    (out / "diagnostics.csv").write_text(diagnostic.stdout)
    summaries = {f"{label}-{name}": summarize(out / f"{label}-{name}.csv")
                 for label, _ in labels for name in ["interpreter", "tokenizer", "parser", "fresh-process"]}
    save_json(out / "summary.json", {"unit": "ns/op", "variation": "MAD and min/max, not confidence intervals", "results": summaries})
    if baseline:
        compare_results(out, summaries)
    print(f"Saved verified raw samples, summaries, sources and executables in {out}", flush=True)


if __name__ == "__main__":
    main()
