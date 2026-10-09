#!/usr/bin/env python3
"""Compare exact corpus observations and same-source analysis harnesses.

Extract/rebuild a verified saved source archive without changing the baseline.
All subprocesses have deadlines. Standard library only; output must be new.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[1]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(command, cwd=ROOT):
    return subprocess.run(command, cwd=cwd, check=True, text=True,
                          capture_output=True, timeout=300)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    baseline, out = args.baseline.resolve(), args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    archive = out / "baseline-source"
    archive.mkdir()
    with tarfile.open(baseline / "sources.tar.gz") as source:
        source.extractall(archive, filter="data")
    manifest = json.loads((baseline / "manifest.json").read_text())
    for filename, digest in manifest["sources"].items():
        if sha(archive / filename) != digest:
            raise RuntimeError(f"baseline source hash mismatch: {filename}")
    bins = out / "bin"
    bins.mkdir()
    identity = {"baseline_archive_sha256": sha(baseline / "sources.tar.gz"),
                "toolchain": run(["rustc", "-Vv"]).stdout,
                "harnesses": {}, "libraries": {}, "binaries": {}}
    harnesses = {"analysis": ROOT / "benches/interpreter_reuse.rs",
                 "allocations": ROOT / "benches/interpreter_reuse.rs",
                 "outcomes": ROOT / "benches/support/interpreter_outcomes.rs"}
    for harness in set(harnesses.values()):
        identity["harnesses"][str(harness.relative_to(ROOT))] = sha(harness)
    for label, directory in [("baseline", archive), ("current", ROOT)]:
        built = run(["cargo", "build", "--release", "--lib", "--message-format=json"], directory)
        (out / f"{label}-build.log").write_text(built.stderr)
        artifacts = [json.loads(line) for line in built.stdout.splitlines() if line.startswith("{")]
        libraries = [Path(filename) for entry in artifacts if entry.get("reason") == "compiler-artifact"
                     and entry["target"]["name"] == "light_cljs"
                     for filename in entry["filenames"] if filename.endswith(".rlib")]
        if len(libraries) != 1:
            raise RuntimeError(f"expected exactly one rlib: {libraries}")
        library = libraries[0]
        identity["libraries"][label] = sha(library)
        for name, harness in harnesses.items():
            binary = bins / f"{label}-{name}"
            command = ["rustc", "--edition=2024", "-C", "opt-level=3", str(harness),
                       "--extern", f"light_cljs={library}", "-L", str(library.parent), "-o", str(binary)]
            if name == "allocations":
                command += ["--cfg", 'feature="count-allocations"']
            run(command)
            identity["binaries"][binary.name] = sha(binary)
    for round_number, labels in [(1, ["baseline", "current"]), (2, ["current", "baseline"])]:
        for label in labels:
            for name in ["analysis", "allocations"]:
                result = run([str(bins / f"{label}-{name}")])
                (out / f"{label}-{name}-{round_number}.csv").write_text(result.stdout)
    outcomes = {}
    for label in ["baseline", "current"]:
        result = run([str(bins / f"{label}-outcomes")])
        outcomes[label] = result.stdout
        (out / f"{label}-outcomes.tsv").write_text(result.stdout)
    if outcomes["baseline"] != outcomes["current"]:
        raise RuntimeError("baseline/current corpus observations differ; inspect retained output")
    identity["outcome_rows"] = len(outcomes["current"].splitlines())
    identity["outcomes_sha256"] = sha(out / "current-outcomes.tsv")
    (out / "identity.json").write_text(json.dumps(identity, indent=2, sort_keys=True) + "\n")
    print(f"Matched {identity['outcome_rows']} corpus observations; saved analysis samples in {out}")


if __name__ == "__main__":
    main()
