#!/usr/bin/env python3
"""Preserve native printing bodies and verify explicit outcomes with both CLJS pins."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / "tests/fixtures/compiler-native-printing.tsv"
PROVENANCE = ROOT / "tests/reference/compiler-native-printing-provenance.json"
CORPUS = ROOT / "tests/fixtures/sci-outcomes.tsv"
REVISION = "a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e"


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()


def trace(identity):
    if identity == "sci-string-pr-lazy-effects":
        return [{"$keyword": {"namespace": None, "name": name, "spelling": name}} for name in ("a", "b")]
    return []


def bodies():
    selected = []
    for line in CORPUS.read_text().splitlines():
        fields = line.split("\t")
        identity = fields[0]
        if identity.startswith("sci-string-pr-") or identity == "sci-string-str-ignores-bindings":
            source = fields[2]
            adapted = source.replace("-1/2", "(/ -1 2)").replace("3/2", "(/ 3 2)").replace("1/2", "(/ 1 2)")
            selected.append((identity, source, adapted))
    assert len(selected) == 58
    return selected


def provenance(selected, expected):
    return {
        "source": "tests/fixtures/sci-outcomes.tsv",
        "references": {"release": "1.12.134", "checkout": REVISION},
        "settings": {"optimizations": "none", "static-fns": False, "def-emits-var": False},
        "adaptation": "Only ratio reader literals become numeric division; complete source bodies are retained.",
        "observation": "pr-str of [:returned body], or [:error] at the original runtime boundary; original native outcomes remain in the corpus.",
        "cases": [{"id": identity, "source_sha256": digest(source), "adapted_sha256": digest(adapted),
                   "expected_sha256": digest(expected[identity]), "trace": trace(identity)} for identity, source, adapted in selected],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", action="store_true", help="execute both pinned CLJS references")
    parser.add_argument("--write", action="store_true", help="save outcomes only after both references agree")
    parser.add_argument("--artifacts", type=Path)
    args = parser.parse_args()
    if args.write and not args.run:
        parser.error("--write requires --run")
    selected = bodies()
    expected = {}
    if FIXTURE.exists():
        rows = [line.split("\t") for line in FIXTURE.read_text().splitlines() if line and not line.startswith("#")]
        assert [(r[0], r[1]) for r in rows] == [(identity, adapted) for identity, _, adapted in selected]
        expected = {r[0]: r[2] for r in rows}
    if args.run:
        sys.path.insert(0, str(ROOT / "tests/reference/compiler-language"))
        spec = importlib.util.spec_from_file_location("printing_report", ROOT / "tests/reference/compiler-language/report.py")
        report = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(report)
        cases = [{"id": identity, "source": "(pr-str (try [:returned (do " + adapted + ")] (catch :default e [:error])))",
                  "expected": {"id": identity, "outcome": {"status": "returned", "value": expected.get(identity)}, "trace": trace(identity), "stdout": ""}}
                 for identity, _, adapted in selected]
        artifacts = args.artifacts or Path(tempfile.mkdtemp(prefix="light-cljs-native-printing-reference-"))
        artifacts.mkdir(exist_ok=True, parents=True)
        observed = {kind: report.reference_cases(cases, kind, ROOT / "research/clojurescript", artifacts / kind, 180)
                    for kind in ("release", "checkout")}
        (artifacts / "observations.json").write_text(json.dumps(observed, indent=2) + "\n")
        verified = {}
        for identity, _, _ in selected:
            results = [observed[kind][identity] for kind in ("release", "checkout")]
            assert all("observed" in result for result in results), (identity, results)
            values = [result["observed"] for result in results]
            assert values[0] == values[1], (identity, values)
            assert values[0]["outcome"]["status"] == "returned", (identity, values)
            assert values[0]["trace"] == trace(identity) and values[0]["stdout"] == "", (identity, values)
            verified[identity] = values[0]["outcome"]["value"]
            if not args.write:
                assert all(result["status"] == "passed" for result in results), (identity, results)
        if args.write:
            expected = verified
            FIXTURE.write_text("# id\tcomplete adapted source\tcanonical CLJS outcome literal\n" + "".join(
                identity + "\t" + adapted + "\t" + expected[identity] + "\n" for identity, _, adapted in selected))
            PROVENANCE.write_text(json.dumps(provenance(selected, expected), indent=2) + "\n")
        print(f"Both pinned references agree on all 58 complete native printing bodies; artifacts: {artifacts}")
    assert len(expected) == 58
    assert json.loads(PROVENANCE.read_text()) == provenance(selected, expected)
    print("All 58 complete native printing bodies, canonical expectations, and source hashes are preserved")


if __name__ == "__main__":
    main()
