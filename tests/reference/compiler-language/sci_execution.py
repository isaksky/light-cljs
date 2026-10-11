#!/usr/bin/env python3
"""Faithful expression adapters for independently retained SCI source rows.

The catalog is source identity, never execution evidence. The only projection is
the existing documented Number/character/finite list-vs-seq bridge. Profile
setup and context-dependent namespace cases stay outside this initial cohort.
Use the existing report reference oracle; raw local observations come from the
named Rust Node/QuickJS driver. A failed original or canonical execution stays
missing. No evaluator disposition or public API obligation is changed here.
"""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import signal
import struct
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
CATALOG = HERE / "sci-execution-catalog.jsonl"
CORPUS = ROOT / "tests/fixtures/sci-outcomes.tsv"
DISPOSITIONS = ROOT / "tests/reference/sci/evaluator-dispositions.tsv"
TEST_FILE = "tests/compiler_sci_execution.rs"
TEST_NAME = "retained_sci_sources_execute_in_node_and_quickjs"
PROJECTION = "SCI Number/character/finite list-vs-seq projection v1"
REFERENCE_SETUP = ('(set! *print-newline* true) '
    '(set-print-fn! (fn [text] (.write (.-stdout js/process) text))) '
    '(set-print-err-fn! (fn [text] (.write (.-stderr js/process) text))) ')


def digest(value):
    return hashlib.sha256(value if isinstance(value, bytes) else value.encode()).hexdigest()


def rows(path):
    result = {}
    for line in path.read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        fields = line.split("\t")
        if fields[0] in result:
            raise ValueError(f"duplicate source identity: {fields[0]}")
        result[fields[0]] = (line, fields)
    return result


def wire(text):
    """Decode SCI's bounded ASCII transport, without evaluating any source."""
    tokens = re.findall(r"[()]|[^ ()]+", text)
    index = 0

    def read():
        nonlocal index
        token = tokens[index]
        index += 1
        if token != "(":
            return token
        result = []
        while tokens[index] != ")":
            result.append(read())
        index += 1
        return result

    result = read()
    if index != len(tokens):
        raise ValueError("trailing SCI wire data")
    return result


def string(unit):
    if not re.fullmatch(r"u(?:[0-9a-f]{4})*", unit):
        raise ValueError("invalid UTF16 wire string")
    return bytes.fromhex(unit[1:]).decode("utf-16-be", errors="surrogatepass")


def json_order(value):
    # Match JavaScript JSON.stringify's UTF16 lexicographic ordering.
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-16-be", errors="surrogatepass")


def value(tree):
    if tree == "nil":
        return None
    if not isinstance(tree, list):
        raise ValueError(f"unsupported wire value: {tree}")
    tag, *children = tree
    if tag == "bool":
        return children[0] == "true"
    if tag == "int":
        number = int(children[0])
        if abs(number) > 9007199254740991:
            raise ValueError("SCI integer outside portable Number range")
        return number
    if tag == "float":
        number = struct.unpack(">d", bytes.fromhex(children[0]))[0]
        if math.isnan(number):
            return {"$number": "NaN"}
        if math.isinf(number):
            return {"$number": "+Infinity" if number > 0 else "-Infinity"}
        if number == 0 and math.copysign(1, number) < 0:
            return {"$number": "-0"}
        return number
    if tag == "string":
        return string(children[0])
    if tag == "char":
        return string("u" + children[0])
    if tag in {"symbol", "keyword"}:
        namespace = None if children[0] == "-" else string(children[0])
        name = string(children[1])
        return {"$" + tag: {"namespace": namespace, "name": name,
                            "spelling": (namespace + "/" if namespace else "") + name}}
    if tag in {"vector", "list", "seq"}:
        return {"$" + tag: [value(child) for child in children]}
    if tag == "set":
        return {"$set": sorted([value(child) for child in children], key=json_order), "count": len(children)}
    if tag == "map":
        if any(child[0] != "entry" or len(child) != 3 for child in children):
            raise ValueError("invalid SCI map entry")
        return {"$map": sorted([[value(child[1]), value(child[2])] for child in children], key=json_order), "count": len(children)}
    raise ValueError(f"unsupported SCI wire category: {tag}")


def projection(observed):
    """Normalize only comparison data; retain raw evidence separately."""
    if isinstance(observed, list):
        return [projection(item) for item in observed]
    if isinstance(observed, dict):
        result = {"$list" if key == "$seq" else key: projection(item) for key, item in observed.items()}
        for kind in ("$map", "$set"):
            if kind in result:
                result[kind] = sorted(result[kind], key=json_order)
        return result
    return observed


def expected(fields):
    outcome = wire(fields[3])
    if outcome[0] != "return" or len(outcome) != 2:
        raise ValueError("initial SCI adapter cohort requires a returned observation")
    effects = value(wire(fields[4]))
    if set(effects) != {"$vector"}:
        raise ValueError("SCI effects must be a vector")
    if value(wire(fields[6])) != "":
        raise ValueError("initial adapter cohort cannot capture stderr in the reference runner")
    return {"id": "sci-execution-" + fields[0], "outcome": {"status": "returned", "value": value(outcome[1])},
            "trace": effects["$vector"], "stdout": value(wire(fields[5]))}


def exclusion(fields):
    if fields[7] != "portable":
        return "Original SCI row has no reviewed matching portable observation."
    if fields[1] != "portable":
        return "Original source profile requires explicit fixture setup beyond expression execution."
    if fields[6] != "(string u)":
        return "Original stderr requires a separate captured stderr oracle."
    if fields[2] != fields[8]:
        return "Existing evaluator source adaptation requires a separate reviewed source contract."
    if any(name in fields[2] for name in ("clojure.string/", "clojure.set/", "clojure.walk/", "clojure.edn/", "#t/", "#x/")):
        return "Source requires namespace/reader setup not supplied by the expression oracle."
    if "::" in fields[2] or re.search(r"\((?:ns|in-ns|require|refer|use|alias|resolve|ns-resolve|all-ns|find-ns|the-ns)\b", fields[2]):
        return "Namespace or auto-resolution context needs an explicit source-specific namespace adapter."
    return None


def generated_catalog():
    originals = rows(CORPUS)
    catalog = []
    for identity, (line, fields) in rows(DISPOSITIONS).items():
        original_line, original = originals[identity]
        if fields[:7] != original:
            raise ValueError(f"SCI disposition changed original columns: {identity}")
        reason = exclusion(fields)
        if reason:
            continue
        try:
            observation = expected(original)
            # Independently validate the old evaluator portable bridge, without
            # granting a present execution pass from the old observation.
            projected = expected([*fields[:3], *fields[9:13]])
            if projection(observation) != projection(projected):
                raise ValueError("historical portable bridge differs")
        except ValueError:
            continue
        catalog.append({"id": observation["id"], "sci_id": identity,
            "obligation_id": f"local::tests/reference/sci/evaluator-dispositions.tsv::{identity}",
            "body_sha256": digest(line), "original_row_sha256": digest(original_line),
            "original_source_sha256": digest(original[2]), "source_sha256": digest(original[2]),
            "reference_source_sha256": digest(REFERENCE_SETUP + original[2]),
            "reference_setup": REFERENCE_SETUP,
            "reference_isolated": bool(re.search(r"\(defrecord\s+(?:Box|Empty)\b", original[2])),
            "profile": original[1], "setup": "Expression runners provide a lexical observe callback and captured output streams. Canonical-only visible prefix sets SCI's streaming print callbacks and print-newline=true instead of the Node console printer; local helper supplies the equivalent stream callbacks. No reader/namespace/library setup.",
            "dialect": "ClojureScript expression source; source bytes unchanged; " + PROJECTION,
            "expected": observation})
    return catalog


def load():
    catalog = [json.loads(line) for line in CATALOG.read_text().splitlines() if line]
    if catalog != generated_catalog():
        raise ValueError("SCI execution catalog is stale or incomplete for its declared cohort")
    return catalog


def cases(catalog=None):
    originals = rows(CORPUS)
    return [{"id": row["id"], "source": originals[row["sci_id"]][1][2],
             "expected": row["expected"], "native_stdout": None,
             "isolated": row["reference_isolated"]} for row in (catalog if catalog is not None else load())]


def verify(case, evidence, same_json):
    if evidence.get("status") not in {"passed", "mismatch", "completed"}:
        return {**evidence, "status": evidence.get("status", "not-run")}
    observed = evidence.get("observed")
    if not isinstance(observed, dict) or set(observed) != {"id", "outcome", "trace", "stdout"}:
        return {**evidence, "status": evidence.get("status", "not-run") if evidence.get("status") not in ("passed", "mismatch") else "invalid-output"}
    return {**evidence, "status": "passed" if same_json(projection(observed), projection(case["expected"])) else "mismatch",
            "comparison_projection": PROJECTION}


def report_module():
    spec = importlib.util.spec_from_file_location("sci_expression_report", HERE / "report.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def local_cases(selected, directory, timeout=180):
    """Execute exact selected source rows through compiler-emitted Node and rquickjs."""
    directory.mkdir(parents=True, exist_ok=True)
    input_path, output_path = directory / "input.tsv", directory / "observations.tsv"
    input_path.write_text("".join(f"{case['id']}\t{case['source'].encode().hex()}\n" for case in selected))
    environment = {**os.environ, "LIGHT_CLJS_SCI_EXECUTION_INPUT": str(input_path.resolve()),
                   "LIGHT_CLJS_SCI_EXECUTION_OUTPUT": str(output_path.resolve())}
    runner = report_module()
    command = ["cargo", "test", "--locked", "--test", "compiler_sci_execution", TEST_NAME, "--", "--ignored", "--exact", "--nocapture"]
    try:
        process = subprocess.Popen(command, cwd=ROOT, env=environment, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, start_new_session=True)
    except OSError as error:
        return {case["id"]: {"status": "unavailable", "detail": str(error)} for case in selected}
    timed_out = False
    try:
        stdout, stderr = process.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        os.killpg(process.pid, signal.SIGTERM)
        try:
            stdout, stderr = process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            stdout, stderr = process.communicate()
    (directory / "cargo.stdout").write_text(stdout)
    (directory / "cargo.stderr").write_text(stderr)
    exact_run = ("running 1 test" in stdout and
        re.search(r"test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured;", stdout) is not None)
    if timed_out or process.returncode != 0 or not exact_run or not output_path.is_file():
        return {case["id"]: {"status": "timeout" if timed_out else "failed", "detail": "named SCI execution driver failed; see cargo logs"} for case in selected}
    actual_rows = [line.split("\t") for line in output_path.read_text().splitlines()]
    if [row[0] for row in actual_rows] != [case["id"] for case in selected]:
        raise ValueError("missing/duplicate/reordered source execution rows")
    result = {}
    for case, row in zip(selected, actual_rows):
        if row[1] != "executed" or len(row) != 4:
            result[case["id"]] = {"status": "rejected", "detail": row[2:]}
            continue
        engines = {}
        for engine, observation in zip(("Node", "QuickJS"), row[2:]):
            try:
                raw = json.loads(observation)
                engines[engine] = verify(case, {"status": "completed", "observed": raw}, runner.same_json)
            except (ValueError, TypeError):
                engines[engine] = {"status": "invalid-output", "detail": observation}
        result[case["id"]] = {"status": "passed" if all(layer["status"] == "passed" for layer in engines.values()) else "mismatch",
            "engines": engines, "source_sha256": digest(case["source"]),
            "test_file": TEST_FILE, "test_name": TEST_NAME,
            "test_source_sha256": digest((ROOT / TEST_FILE).read_bytes()),
            "adapter_source_sha256": digest((ROOT / "tests/support/sci_execution_observation.js").read_bytes())}
    return result


def reference_cases(selected, kind, checkout, directory, timeout=180):
    runner = report_module()
    originals = {case["id"]: case for case in selected}
    selected = [{**case, "source": REFERENCE_SETUP + case["source"]} for case in selected]
    # A reader rejection in one independently retained SCI expression must not
    # abort all unrelated case namespaces in the reference compiler batch.
    probes = [{"id": case["id"], "mode": "expr", "source": f"(fn* [observe] {case['source']})"} for case in selected]
    analyzed = runner.checked_analysis(probes, kind, checkout, directory / "preflight", timeout,
                                       driver="tests/reference/compiler-language/sci-analysis.clj")
    accepted = [case for case in selected if analyzed[case["id"]].get("status") == "completed" and
                analyzed[case["id"]].get("observed", {}).get("category") in {"accepted", "warning"}]
    results = {case["id"]: {**analyzed[case["id"]], "status": "rejected" if analyzed[case["id"]].get("status") == "completed" else analyzed[case["id"]]["status"]}
               for case in selected if case not in accepted}
    if accepted:
        raw = runner.reference_cases(accepted, kind, checkout, directory / "execution", timeout)
        verified = {case["id"]: verify(case, raw[case["id"]], runner.same_json) for case in accepted}
        # The ordinary runner's stderr contains only source identity frames.
        # Assert the original empty-stderr contract rather than silently losing
        # writes to *print-err-fn* or a host error stream.
        batches = [[case] for case in accepted if case.get("isolated")]
        ordinary = [case for case in accepted if not case.get("isolated")]
        if ordinary:
            batches.insert(0, ordinary)
        for index, batch in enumerate(batches):
            path = directory / "execution" / f"node-{index}" / "stderr"
            if not path.is_file():
                if any(verified[case["id"]]["status"] == "passed" for case in batch):
                    raise ValueError("missing canonical stderr observation")
                continue
            current = None
            extras = {}
            seen = []
            for line in path.read_text().splitlines():
                if line.startswith("CASE START "):
                    metadata = json.loads(line.removeprefix("CASE START "))
                    current = metadata["id"]
                    seen.append(current)
                elif line.startswith("CASE END "):
                    metadata = json.loads(line.removeprefix("CASE END "))
                    if metadata["id"] != current:
                        extras.setdefault(None, []).append(line)
                    current = None
                else:
                    extras.setdefault(current, []).append(line)
            if current is not None or seen != [case["id"] for case in batch]:
                extras.setdefault(None, []).append("incomplete or reordered stderr identity frames")
            for case in batch:
                identity, layer = case["id"], verified[case["id"]]
                emitted = extras.get(identity, []) + extras.get(None, [])
                if emitted and layer["status"] == "passed":
                    layer.update(status="mismatch", detail="original SCI empty stderr contract differs", stderr=emitted)
                elif layer["status"] == "passed":
                    layer["stderr"] = ""
        results.update(verified)
    for case in selected:
        results[case["id"]]["source_sha256"] = digest(case["source"])
        results[case["id"]]["original_source_sha256"] = digest(originals[case["id"]]["source"])
        results[case["id"]]["isolated"] = case.get("isolated", False)
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write-catalog", action="store_true")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--ids", help="comma-separated original SCI ids")
    parser.add_argument("--local", action="store_true")
    parser.add_argument("--references", action="store_true")
    parser.add_argument("--timeout", type=int, default=180)
    args = parser.parse_args()
    if args.write_catalog:
        CATALOG.write_text("".join(json.dumps(row, ensure_ascii=True, separators=(",", ":")) + "\n" for row in generated_catalog()))
    catalog = load()
    selected = cases(catalog)
    if args.ids:
        identities = set(args.ids.split(","))
        selected = [case for case in selected if case["id"].removeprefix("sci-execution-") in identities]
        if len(selected) != len(identities):
            raise ValueError("requested SCI ids are absent from the declared cohort")
    print(f"Validated SCI cohort: {len(catalog)} original source rows; selected {len(selected)}", flush=True)
    if not (args.local or args.references):
        return
    if args.output is None:
        parser.error("execution requires --output")
    args.output.mkdir(parents=True, exist_ok=True)
    evidence = {case["id"]: {} for case in selected}
    if args.local:
        local = local_cases(selected, args.output / "local", args.timeout)
        for identity, layer in local.items():
            evidence[identity]["javascript"] = layer
        print("Local", dict(Counter(layer["status"] for layer in local.values())), flush=True)
    if args.references:
        for kind in ("release", "checkout"):
            observed = reference_cases(selected, kind, ROOT / "research/clojurescript", args.output / kind, args.timeout)
            for identity, layer in observed.items():
                evidence[identity][kind] = layer
            print(kind, dict(Counter(layer["status"] for layer in observed.values())), flush=True)
    output = {"catalog_sha256": digest(CATALOG.read_bytes()), "cohort_count": len(catalog),
              "selected_count": len(selected), "evidence": evidence}
    (args.output / "evidence.json").write_text(json.dumps(output, ensure_ascii=True, indent=2) + "\n")


if __name__ == "__main__":
    main()
