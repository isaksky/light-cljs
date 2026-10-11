#!/usr/bin/env python3
"""Pinned public source metadata probes, retaining actual fixture locations."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("report", Path(__file__).parent / "compiler-language/report.py")
report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(report)
parser = argparse.ArgumentParser()
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
original = next(line.split("\t")[2] for line in Path("tests/fixtures/sci-outcomes.tsv").read_text().splitlines() if line.startswith("sci-var-reserved-metadata\t"))
definition = original.split(" [(get", 1)[0]
projection = definition + ' [(= "fake" (get (meta (var x)) :file)) (get (meta (var x)) :line) (get (meta (var x)) :column) (get (meta (var x)) :doc)]'
path_probe = definition + ' [(string? (:file (meta (var x)))) (boolean (re-find #"semantic/case2.cljs$" (:file (meta (var x)))))]'
def symbol(name):
    return {"$symbol": {"namespace": None, "name": name, "spelling": name}}
def case(identity, source, value):
    return {"id": identity, "source": source, "isolated": True, "expected": {"id": identity, "outcome": {"status": "returned", "value": {"$vector": value}}, "trace": [], "stdout": ""}}
cases = [case("historical-source", original, [symbol("x"), "semantic.case0", False, 3, 25, "new"]), case("same-definition-projection", projection, [False, 3, 25, "new"]), case("source-path", path_probe, [True, True])]
cases.append(case("generated-factories", '(defrecord R [x]) [(string? (:file (meta (var ->R)))) (:line (meta (var ->R))) (:column (meta (var ->R))) (string? (:file (meta (var map->R)))) (:line (meta (var map->R))) (:column (meta (var map->R)))]', [True, 3, 25, True, 3, 25]))
receipts = {"historical_source_sha256": hashlib.sha256(original.encode()).hexdigest(), "projection_sha256": hashlib.sha256(projection.encode()).hexdigest(), "cases": cases, "references": {}}
for kind in ["release", "checkout"]:
    result = report.reference_cases(cases, kind, Path("research/clojurescript").resolve(), args.output / kind, 180)
    receipts["references"][kind] = result
    # Location belongs to the generated original source, not an arbitrary baseline.
    for index in range(len(cases)):
        source = args.output / kind / "src/semantic" / f"case{index}.cljs"
        text = source.read_text()
        lines = text.splitlines()
        assert lines[2].index(cases[index]["source"].split(" [", 1)[0] if index < 3 else "(defrecord R [x])") + 1 == 25
        receipts.setdefault("fixture_sha256", {})[f"{kind}/case{index}.cljs"] = hashlib.sha256(source.read_bytes()).hexdigest()
    assert all(row["status"] == "passed" for row in result.values()), result
args.output.mkdir(parents=True, exist_ok=True)
(args.output / "receipts.json").write_text(json.dumps(receipts, indent=2) + "\n")
print("Four source metadata probes passed on both pinned references")
