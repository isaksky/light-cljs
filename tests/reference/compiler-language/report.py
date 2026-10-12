#!/usr/bin/env python3
"""Executable assertion inventory; no dependencies and no language implementation.

The committed inventory is source evidence, never a passing result. Only this
invocation's complete compiler + checked release/checkout observations can cover
an assertion. Parser acceptance, historical native observations and current evaluation have independent columns.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
REVISION = "a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e"
RELEASE = "1.12.134"
LAYERS = ("lexer", "parser", "native", "evaluation", "javascript", "diagnostic", "release", "checkout")
SEMANTIC_LAYERS = ("javascript", "release", "checkout")
REJECTION_LAYERS = ("diagnostic", "release", "checkout")


def jsonl(path):
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def unique(rows, key, label):
    result = {}
    for row in rows:
        identity = row[key]
        if identity in result:
            raise ValueError(f"duplicate {label}: {identity}")
        result[identity] = row
    return result


def fixtures(scope="historical-selected"):
    result = []
    path = ROOT / "tests/fixtures/compiler-language.tsv"
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        identity, source, outcome, trace, stdout, native = line.split("\t")
        result.append({
            "id": identity, "source": source,
            "expected": {"id": identity, "outcome": json.loads(outcome),
                         "trace": json.loads(trace), "stdout": json.loads(stdout)},
            "native_stdout": None if native == "-" else json.loads(native),
        })
    for case in jsonl(HERE / "namespace-cases.jsonl"):
        entry = case["entry"].replace(".", "/").replace("-", "_")
        case["source"] = (ROOT / case["source_root"] / (entry + ".cljs")).read_text(encoding="utf-8")
        result.append(case)
    result.extend(jsonl(HERE / "diagnostic-cases.jsonl"))
    result.extend(jsonl(HERE / "warning-cases.jsonl"))
    if scope == "intended-language":
        result.extend(jsonl(HERE / "full-cases.jsonl"))
    return unique(result, "id", "fixture")


def validate_reference_source(case):
    if "reference_source_root" in case:
        reference_root = Path(case["reference_source_root"])
        if ("source_root" not in case or reference_root.is_absolute()
                or not reference_root.parts or ".." in reference_root.parts
                or not case.get("reference_adaptation", "").strip()):
            raise ValueError(f"invalid explicit reference source root: {case['id']}")
        root = ROOT / reference_root
        hashes = {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
                  for path in sorted(root.rglob("*")) if path.is_file()}
        if not hashes or hashes != case.get("reference_files_sha256"):
            raise ValueError(f"reference source tree drift: {case['id']}")


def validate_diagnostic(case, inventory, bodies):
    """Negative witnesses retain complete bodies, every probe, and exact reasons.

    These are restricted to analysis/header validation of readable source.
    Missing namespace files and accepted-but-unimplemented programs cannot enter
    through this contract. Every non-rejecting release needs an explicit difference.
    """
    diagnostic = case["diagnostic"]
    body = bodies.get(diagnostic.get("body_id"))
    if (body is None or diagnostic.get("body_sha256") != body["source_span"]["sha256"]
            or not diagnostic.get("rationale", "").strip()):
        raise ValueError(f"diagnostic detached from complete body: {case['id']}")
    assertions = diagnostic.get("assertion_ids", [])
    expected = [identity for identity in body["assertions"] if inventory[identity]["disposition"] == "in-scope"]
    subset = diagnostic.get("assertion_subset")
    if subset is not None and (subset is not True or not diagnostic.get("split_rationale", "").strip()):
        raise ValueError(f"diagnostic subset needs an explicit split rationale: {case['id']}")
    if not expected or not assertions or assertions != ([value for value in expected if value in assertions] if subset else expected):
        raise ValueError(f"diagnostic must retain every selected in-scope body assertion in order: {case['id']}")
    probes = diagnostic.get("probes")
    if not isinstance(probes, list) or not probes:
        raise ValueError(f"diagnostic needs nonempty complete probes: {case['id']}")
    unique(probes, "id", "diagnostic probe")
    difference = diagnostic.get("release_difference")
    if difference is not None and (not isinstance(difference, str) or not difference.strip()):
        raise ValueError(f"diagnostic difference needs a rationale: {case['id']}")
    for probe in probes:
        if (not re.fullmatch(r"[a-zA-Z0-9-]+", probe["id"])
                or probe.get("mode") not in {"expr", "ns"} or not probe.get("source", "").strip()
                or any(char in probe["source"] for char in "\r\n\t")):
            raise ValueError(f"invalid diagnostic source or mode: {case['id']}")
        if probe["mode"] == "ns" and (not re.fullmatch(r"[a-zA-Z][a-zA-Z0-9-]*(?:\.[a-zA-Z][a-zA-Z0-9-]*)*", probe.get("entry", ""))
                                        or not probe["source"].startswith("(ns " + probe["entry"] + " ")):
            raise ValueError(f"namespace diagnostic needs its original declared entry: {case['id']}")
        if "local_source" in probe:
            local_source = probe["local_source"]
            if (not isinstance(local_source, str) or not local_source.strip()
                    or any(char in local_source for char in "\r\n\t")
                    or not probe.get("local_adaptation", "").strip()
                    or probe.get("local_source_sha256") != hashlib.sha256(local_source.encode()).hexdigest()
                    or (probe["mode"] == "ns" and not local_source.startswith("(ns " + probe["entry"] + " "))):
                raise ValueError(f"diagnostic local adaptation needs hashed source, original entry and rationale: {case['id']}")
        for layer in ("local", "release", "checkout"):
            contract = probe.get(layer, {})
            keys = {"category", "detail"} if layer == "local" else {"category", "detail", "warnings", "exception_class"}
            if difference and layer == "release":
                if (set(contract) != keys or contract["category"] not in {"accepted", "warning"}
                        or contract["detail"] != "" or contract["exception_class"] != ""
                        or not isinstance(contract["warnings"], str)
                        or (contract["category"] == "warning") != bool(contract["warnings"])):
                    raise ValueError(f"diagnostic difference needs exact accepted/warning release behavior: {case['id']}")
            elif (set(contract) != keys or contract["category"] != ("namespace-error" if layer == "local" and probe["mode"] == "ns" else "analysis-error")
                    or not isinstance(contract["detail"], str) or not contract["detail"].strip()
                    or (layer == "local" and probe["mode"] == "expr" and "unsupported" in contract["detail"].lower())
                    or (layer == "local" and "unsupported construct:" in contract["detail"].lower())
                    or (layer != "local" and (not isinstance(contract["warnings"], str)
                                              or contract["exception_class"] != "clojure.lang.ExceptionInfo"))):
                raise ValueError(f"diagnostic requires exact {layer} rejection reason and phase: {case['id']}")


def valid_warning_trace(trace):
    return isinstance(trace, list) and all(isinstance(row, list) and len(row) == 2
        and all(isinstance(value, str) and value for value in row) for row in trace)


def validate_warning(case, inventory, bodies):
    contract = case["warning"]
    body = bodies.get(contract.get("body_id"))
    if (body is None or contract.get("body_sha256") != body["source_span"]["sha256"]
            or not contract.get("rationale", "").strip()):
        raise ValueError(f"warning detached from complete source body: {case['id']}")
    expected = [identity for identity in body["assertions"] if inventory[identity]["disposition"] == "in-scope"]
    if not expected or contract.get("assertion_ids") != expected:
        raise ValueError(f"warning must retain every body assertion: {case['id']}")
    probes = contract.get("probes")
    if not isinstance(probes, list) or not probes:
        raise ValueError(f"warning needs complete source probes: {case['id']}")
    unique(probes, "id", "warning probe")
    differs = False
    for probe in probes:
        if (not re.fullmatch(r"[a-zA-Z0-9-]+", probe["id"]) or probe.get("mode") not in {"expr", "ns"}
                or not probe.get("source", "").strip() or any(char in probe["source"] for char in "\r\n\t")
                or set(probe.get("settings", {})) != {"static-fns", "checked-arrays"}
                or probe["settings"]["static-fns"] is not False
                or type(probe["settings"]["checked-arrays"]) is not bool):
            raise ValueError(f"warning needs source and pinned primary settings: {case['id']}")
        if probe.get("supplemental_static_fns") not in (None, True):
            raise ValueError(f"invalid supplemental warning settings: {case['id']}")
        local = probe.get("local", {})
        if (set(local) != {"category", "warning_trace"} or local["category"] != "accepted"
                or not valid_warning_trace(local["warning_trace"])):
            raise ValueError(f"warning requires accepted local compilation and exact trace: {case['id']}")
        for kind in ("release", "checkout"):
            expected = probe.get(kind, {})
            if (set(expected) != {"category", "detail", "warnings", "warning_trace"}
                    or not valid_warning_trace(expected["warning_trace"])
                    or expected["warnings"] != ",".join(sorted({row[0] for row in expected["warning_trace"]}))
                    or expected["category"] not in {"accepted", "warning", "analysis-error"}
                    or (expected["category"] == "analysis-error" and (not (probe.get("exception_tolerant") or (kind == "release" and contract.get("release_difference"))) or not expected["detail"].strip()))
                    or (expected["category"] != "analysis-error" and (expected["detail"] != ""
                        or (expected["category"] == "warning") != bool(expected["warning_trace"])))):
                raise ValueError(f"warning needs precise reference acceptance and diagnostic order: {case['id']}")
        if not same_json(local["warning_trace"], probe["checkout"]["warning_trace"]):
            raise ValueError(f"local warnings must implement the checkout source contract: {case['id']}")
        differs |= not same_json(probe["release"], probe["checkout"])
        if probe["mode"] == "ns":
            if (not isinstance(probe.get("local_files"), dict) or not probe["local_files"]
                    or not re.fullmatch(r"[a-zA-Z][a-zA-Z0-9-]*(?:\.[a-zA-Z][a-zA-Z0-9-]*)*", probe.get("entry", ""))
                    or not probe.get("local_adaptation", "").strip()):
                raise ValueError(f"namespace warning needs explicit dependency adaptation: {case['id']}")
            for path, source in probe["local_files"].items():
                if Path(path).is_absolute() or ".." in Path(path).parts or not path.endswith(".cljs") or not source.strip():
                    raise ValueError(f"invalid warning namespace file: {case['id']}")
    if differs != bool(contract.get("release_difference")):
        raise ValueError(f"warning version differences require exact differing observations and rationale: {case['id']}")


def local_warning(case, binary, directory, timeout):
    checks, readers = [], {"lexer": [], "parser": []}
    for probe in case["warning"]["probes"]:
        failure = None
        for layer, flags in (("lexer", []), ("parser", ["--parse"])):
            run = invoke([binary, *flags, "-"], directory / probe["id"] / layer, source=probe["source"], timeout=timeout)
            status = "accepted" if run["status"] == "completed" else "mismatch" if run["status"] == "failed" else run["status"]
            readers[layer].append(result(status, id=probe["id"]))
            if status != "accepted":
                failure = result(status, f"{layer} did not accept original warning source", id=probe["id"])
                break
        if failure:
            checks.append(failure)
            continue
        flags = (["--checked-arrays-warn" if probe["settings"]["checked-arrays"] else "--warnings"]
                 if probe.get("local_api", "warnings") in {"warnings", "namespace-warnings"} else []) + ["--compile"]
        if probe["mode"] in {"ns", "cljc"}:
            root = (directory / probe["id"] / "source").resolve()
            for name, source in probe["local_files"].items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source, encoding="utf-8")
            run = invoke([binary, *flags, "--entry", probe["entry"], "--source-root", str(root)],
                         directory / probe["id"] / "compile", timeout=timeout)
        else:
            run = invoke([binary, *flags, "-"], directory / probe["id"] / "compile", source=probe["source"], timeout=timeout)
        trace = []
        valid = run["status"] == "completed" and bool(run.get("stdout", "").strip())
        for line in run.get("stderr", "").splitlines():
            match = re.fullmatch(r"WARNING: (.+) \[([a-z-]+)\] \(source ([0-9]+), bytes ([0-9]+)\.\.([0-9]+)\)", line)
            if not match or int(match[4]) > int(match[5]):
                valid = False
                continue
            trace.append([match[2], match[1]])
        observed = {"category": "accepted", "warning_trace": trace} if valid else None
        if valid:
            syntax = invoke([os.environ.get("NODE", "node"), "--check"], directory / probe["id"] / "javascript-syntax",
                            source=run["stdout"], timeout=timeout)
            valid = syntax["status"] == "completed"
        matches = valid and same_json(observed, probe["local"])
        checks.append(result("verified-warning" if matches else "mismatch" if run["status"] in ("completed", "failed") else run["status"],
                             id=probe["id"], observed=observed, **({"process": run} if not matches else {})))
    return {**{layer: aggregate_probes(values, "accepted") for layer, values in readers.items()},
            "diagnostic": aggregate_probes(checks, "verified-warning"),
            "native": result("not-applicable", "compiler warning assertion"),
            "javascript": result("not-applicable", "compiled and syntax checked; diagnostic source is not executed")}


def reference_warnings(cases, kind, checkout, directory, timeout):
    probes = [{**probe, "id": case["id"] + "--" + probe["id"]} for case in cases for probe in case["warning"]["probes"]]
    observations = checked_analysis(probes, kind, checkout, directory / "primary", timeout, warning_settings=True)
    supplemental = [{**probe, "settings": {**probe["settings"], "static-fns": True}}
                    for probe in probes if probe.get("supplemental_static_fns")]
    supplemental_observations = checked_analysis(supplemental, kind, checkout, directory / "static-fns-true", timeout,
                                                warning_settings=True) if supplemental else {}
    results = {}
    for case in cases:
        checks = []
        success = "verified-warning-difference" if kind == "release" and case["warning"].get("release_difference") else "verified-warning"
        for probe in case["warning"]["probes"]:
            key = case["id"] + "--" + probe["id"]
            for configuration, values in (("primary", observations), ("static-fns-true", supplemental_observations)):
                if configuration != "primary" and not probe.get("supplemental_static_fns"):
                    continue
                value = values[key]
                if value["status"] == "completed":
                    value = {**value, "status": success if same_json(value["observed"], probe[kind]) else "mismatch"}
                checks.append({**value, "id": probe["id"], "configuration": configuration})
        results[case["id"]] = aggregate_probes(checks, success)
    return results


def load_contract(scope="historical-selected"):
    prefix = "full-" if scope == "intended-language" else ""
    manifest = json.loads((HERE / (prefix + "manifest.json")).read_text(encoding="utf-8"))
    if manifest["revision"] != REVISION:
        raise ValueError("inventory has a different source revision")
    for filename, field in ((prefix + "inventory.jsonl", "inventory_sha256"), (prefix + "bodies.jsonl", "bodies_sha256"),
                            ("extract_inventory.py", "extractor_sha256")):
        if hashlib.sha256((HERE / filename).read_bytes()).hexdigest() != manifest[field]:
            raise ValueError(f"inventory drift: {filename}; regenerate and review source dispositions")
    inventory = unique(jsonl(HERE / (prefix + "inventory.jsonl")), "id", "assertion")
    bodies = unique(jsonl(HERE / (prefix + "bodies.jsonl")), "id", "body")
    cases = fixtures(scope)
    witness_rows = jsonl(HERE / "witnesses.jsonl")
    if scope == "intended-language":
        from full_ledger import PRIVATE_EQUALITY_ASSERTIONS, PRIVATE_TAP_ASSERTIONS
        # Preserve historical evidence, while the intended ledger explicitly
        # excludes only the pinned private storage identity probes.
        excluded = set(PRIVATE_EQUALITY_ASSERTIONS) | set(PRIVATE_TAP_ASSERTIONS) | {
            f"analyzer_tests.clj::test-namespace-metadata::assertion-{i:03}" for i in range(1, 9)}
        removed_cases = {w["fixture_id"] for w in witness_rows if w["assertion_id"] in excluded}
        witness_rows = [w for w in witness_rows if w["assertion_id"] not in excluded]
        witness_rows.extend(jsonl(HERE / "full-witnesses.jsonl"))
        linked = {w["fixture_id"] for w in witness_rows}
        for identity in removed_cases - linked:
            cases.pop(identity, None)
    witnesses = unique(witness_rows, "assertion_id", "witness")
    if not inventory or not bodies or not cases:
        raise ValueError("empty inventory, body catalog, or fixture set")
    if len(inventory) != manifest["counts"]["assertions"] or len(bodies) != manifest["counts"]["bodies"]:
        raise ValueError("inventory count drift")
    body_assertions = []
    for body in bodies.values():
        span = body["source_span"]
        if hashlib.sha256(span["text"].encode()).hexdigest() != span["sha256"]:
            raise ValueError(f"changed complete test body: {body['id']}")
        body_assertions.extend(body["assertions"])
    if len(body_assertions) != len(set(body_assertions)) or set(body_assertions) != set(inventory):
        raise ValueError("omitted or duplicated body assertions")
    for identity, row in inventory.items():
        if row["body_id"] not in bodies:
            raise ValueError(f"unknown complete body for {identity}")
        if row["disposition"] not in ("in-scope", "unrelated"):
            raise ValueError(f"invalid disposition for {identity}")
        if not row["feature"] or not row["reason"]:
            raise ValueError(f"missing feature or rationale for {identity}")
        assertion = row["assertion"]
        if hashlib.sha256(assertion["text"].encode()).hexdigest() != assertion["sha256"]:
            raise ValueError(f"changed assertion text for {identity}")
        body = bodies[row["body_id"]]
        span = body["source_span"]
        offset = assertion["start_byte"] - span["start_byte"]
        length = assertion["end_byte"] - assertion["start_byte"]
        if (row["body_sha256"] != span["sha256"] or row["source"] != body["source"]
                or body["source_span"]["text"].encode()[offset:offset + length] != assertion["text"].encode()
                or identity not in body["assertions"]):
            raise ValueError(f"assertion detached from original complete body: {identity}")
    linked_cases = set()
    reference_contracts = {}
    for case in cases.values():
        import hosted_source_dispatch
        hosted_source_dispatch.validate_case(case)
        import namespace_results
        namespace_results.validate(case)
        if "nodejs_rt" in case and (case["nodejs_rt"] is not False or "source_root" not in case):
            raise ValueError(f"invalid explicit Node bootstrap setting: {case['id']}")
        validate_reference_source(case)
        if "diagnostic" in case:
            validate_diagnostic(case, inventory, bodies)
        if "warning" in case:
            validate_warning(case, inventory, bodies)
    for identity, witness in witnesses.items():
        row = inventory.get(identity)
        if row is None or row["disposition"] != "in-scope":
            raise ValueError(f"witness must reference an in-scope assertion: {identity}")
        if witness["assertion_sha256"] != row["assertion"]["sha256"]:
            raise ValueError(f"stale assertion witness: {identity}")
        if witness["fixture_id"] not in cases or not witness["rationale"].strip():
            raise ValueError(f"missing witness fixture or adaptation rationale: {identity}")
        case = cases[witness["fixture_id"]]
        if "warning" in case or "expected_warning" in witness:
            if (witness.get("expected_warning") is not True or "warning" not in case
                    or identity not in case["warning"]["assertion_ids"]
                    or witness.get("warning_difference") != case["warning"].get("release_difference")
                    or any(key in witness for key in ("version_difference", "compile_difference", "no_warnings", "isolated", "expected_rejection"))):
                raise ValueError(f"invalid warning witness: {identity}")
            linked_cases.add(witness["fixture_id"])
            continue
        if "diagnostic" in case or "expected_rejection" in witness:
            if (witness.get("expected_rejection") is not True or "diagnostic" not in case
                    or identity not in case["diagnostic"]["assertion_ids"]
                    or witness.get("diagnostic_difference") != case["diagnostic"].get("release_difference")
                    or any(key in witness for key in ("version_difference", "compile_difference", "no_warnings", "isolated"))):
                raise ValueError(f"invalid expected rejection witness: {identity}")
            linked_cases.add(witness["fixture_id"])
            continue
        compile_difference = witness.get("compile_difference")
        if compile_difference:
            if (witness.get("version_difference") or set(compile_difference) != {"reason", "release"}
                    or not compile_difference["reason"].strip()):
                raise ValueError(f"invalid compile difference: {identity}")
            rejection = compile_difference["release"]
            if (set(rejection) != {"category", "detail_contains"}
                    or rejection["category"] not in {"reader-error", "analysis-error", "emit-error", "javascript-error"}
                    or not rejection["detail_contains"].strip()):
                raise ValueError(f"compile difference needs an exact rejection contract: {identity}")
            previous = cases[witness["fixture_id"]].setdefault("compile_difference", compile_difference)
            if not same_json(previous, compile_difference):
                raise ValueError(f"inconsistent compile difference: {identity}")
        linked_cases.add(witness["fixture_id"])
        if witness.get("isolated"):
            cases[witness["fixture_id"]]["isolated"] = True
        if witness.get("no_warnings"):
            cases[witness["fixture_id"]]["no_warnings"] = True
        # A difference is a verified pair of versioned expectations, never an
        # expected compiler failure or a shortcut around the release oracle.
        difference = witness.get("version_difference")
        if difference:
            if not difference.get("reason") or same_json(difference.get("release"), difference.get("checkout")):
                raise ValueError(f"unsubstantiated version difference: {identity}")
            for kind in ("release", "checkout"):
                if not isinstance(difference.get(kind), dict):
                    raise ValueError(f"missing {kind} expected outcome: {identity}")
            if not same_json(difference["checkout"], cases[witness["fixture_id"]]["expected"]):
                raise ValueError(f"checkout must remain the fixture contract: {identity}")
        contract = {kind: difference[kind] if difference else cases[witness["fixture_id"]]["expected"]
                    for kind in ("release", "checkout")}
        previous = reference_contracts.setdefault(witness["fixture_id"], contract)
        if not same_json(previous, contract):
            raise ValueError(f"inconsistent reference expectations for shared fixture: {witness['fixture_id']}")
    if scope == "intended-language":
        linked_cases.update(item["reference_fixture_id"] for item in jsonl(HERE / "obligation-witnesses.jsonl")
                            if item.get("local_execution") and item.get("reference_fixture_id") in cases)
    if linked_cases != set(cases):
        raise ValueError(f"unmapped fixtures: {sorted(set(cases) - linked_cases)}")
    for case in cases.values():
        hosted_source_dispatch.validate_witnesses(case, witnesses, inventory)
    for case in cases.values():
        if "diagnostic" in case and any(witnesses.get(identity, {}).get("fixture_id") != case["id"]
                                       for identity in case["diagnostic"]["assertion_ids"]):
            raise ValueError(f"partially mapped diagnostic body: {case['id']}")
    for case in cases.values():
        if "warning" in case and any(witnesses.get(identity, {}).get("fixture_id") != case["id"]
                                    for identity in case["warning"]["assertion_ids"]):
            raise ValueError(f"partially mapped warning body: {case['id']}")
    return inventory, witnesses, cases


def result(status, detail="", **extra):
    return {"status": status, **({"detail": detail} if detail else {}), **extra}


def invoke(command, directory, *, source="", timeout=30, cwd=ROOT):
    """File-backed output retains crashes and hangs without pipe descendants.

    Kill the owned process group on POSIX; never kill by executable name.
    """
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "invocation.json").write_text(json.dumps([str(arg) for arg in command]), encoding="utf-8")
    (directory / "stdin").write_bytes(source.encode("utf-8"))
    started = time.monotonic()
    with (directory / "stdin").open("rb") as stdin, \
            (directory / "stdout").open("wb") as stdout, \
            (directory / "stderr").open("wb") as stderr:
        try:
            process = subprocess.Popen(command, cwd=cwd, stdin=stdin, stdout=stdout,
                                       stderr=stderr, start_new_session=os.name == "posix")
        except OSError as error:
            return result("unavailable", str(error))
        try:
            code = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            if os.name == "posix":
                os.killpg(process.pid, signal.SIGKILL)
            else:
                process.kill()
            process.wait()
            return result("timed-out", f"exceeded {timeout}s; artifacts: {directory}")
    try:
        output = (directory / "stdout").read_bytes().decode("utf-8")
    except UnicodeDecodeError:
        return result("invalid-output", f"stdout is not UTF-8; artifacts: {directory}")
    return result("completed" if code == 0 else "failed", returncode=code,
                  stdout=output,
                  stderr=(directory / "stderr").read_bytes().decode("utf-8", errors="replace"),
                  elapsed_seconds=round(time.monotonic() - started, 3))


def observations(output, expected):
    """Require exactly one ordered complete outcome for each expected ID."""
    def object_fields(pairs):
        fields = {}
        for key, value in pairs:
            if key in fields:
                raise ValueError(f"duplicate JSON field: {key}")
            fields[key] = value
        return fields

    def invalid_number(value):
        raise ValueError(f"non-JSON numeric value: {value}")

    try:
        lines = [json.loads(line, object_pairs_hook=object_fields, parse_constant=invalid_number)
                 for line in output.splitlines()]
        if [line.get("id") for line in lines] != [case["id"] for case in expected]:
            raise ValueError("missing, duplicate, reordered, or extra outcome")
        if any(set(line) != {"id", "outcome", "trace", "stdout"} for line in lines):
            raise ValueError("incomplete outcome protocol")
    except (ValueError, AttributeError, TypeError) as error:
        return {case["id"]: result("invalid-output", str(error)) for case in expected}
    return {case["id"]: result("passed" if same_json(line, case["expected"]) else "mismatch",
                              observed=line)
            for case, line in zip(expected, lines)}


def same_json(left, right):
    # Python considers True == 1, whereas the JavaScript outcome contract does
    # not. JSON numbers share a representation, but booleans never do.
    if isinstance(left, bool) != isinstance(right, bool):
        return False
    if isinstance(left, dict) and isinstance(right, dict):
        return left.keys() == right.keys() and all(same_json(left[key], right[key]) for key in left)
    if isinstance(left, list) and isinstance(right, list):
        return len(left) == len(right) and all(same_json(a, b) for a, b in zip(left, right))
    if type(left) is not type(right) and not (type(left) in (int, float) and type(right) in (int, float)):
        return False
    return left == right


def local_layers(case, binary, directory, timeout, *, warning_mode=False):
    import hosted_source_dispatch
    import namespace_results
    if namespace_results.validate(case):
        return namespace_results.local(case, binary, directory, timeout)
    if hosted_source_dispatch.validate_case(case):
        raise ValueError('reviewed hosted source requires actual embedding dispatch; generic CLI is not evidence')
    if "warning" in case:
        return local_warning(case, binary, directory, timeout)
    if "diagnostic" in case:
        return local_diagnostic(case, binary, directory, timeout)
    # Retain the fixture's old native observation separately. --eval now runs
    # EvalSession and cannot establish native evidence.
    layers = {"native": result("historical-not-executed", "legacy fixture observation; native adapter is retired for CLI evaluation", observed=case["native_stdout"])}
    for layer, flags in (("lexer", []), ("parser", ["--parse"]), ("evaluation", ["--eval"])):
        if layer == "evaluation" and case["native_stdout"] is None:
            layers[layer] = result("not-applicable", "no compatible CLI stdout expectation; compiler-only host behavior")
            continue
        roots = (["--source-root", ROOT / case["source_root"]]
                 if layer == "evaluation" and "source_root" in case else [])
        run = invoke([binary, *flags, *roots, "-"], directory / layer, source=case["source"], timeout=timeout)
        if run["status"] == "completed":
            if layer == "evaluation":
                status = "passed" if run["stdout"] == case["native_stdout"] else "mismatch"
            else:
                status = "accepted"
            layers[layer] = result(status, observed=run["stdout"] if layer == "evaluation" else None)
        else:
            layers[layer] = result("rejected" if run["status"] == "failed" else run["status"],
                                   run.get("detail", run.get("stderr", "")))
    if "source_root" in case:
        compiled = invoke([binary, *(["--warnings"] if warning_mode else []), "--compile", "--entry", case["entry"], "--source-root", ROOT / case["source_root"]],
                          directory / "compile", timeout=timeout)
    else:
        compiled = invoke([binary, *(["--warnings"] if warning_mode else []), "--compile", "-"], directory / "compile",
                          source=f"(fn* [observe] {case['source']})", timeout=timeout)
    if compiled["status"] != "completed":
        layers["javascript"] = result("rejected" if compiled["status"] == "failed" else compiled["status"],
                                      compiled.get("detail", compiled.get("stderr", "")))
        return layers
    if case.get("no_warnings") and "WARNING:" in compiled.get("stderr", ""):
        layers["javascript"] = result("mismatch", "unexpected compiler warning: " + compiled["stderr"])
        return layers
    # Same test-only lexical runtime adapter as tests/execution.rs; production
    # JavaScript has no global brand or inspection API.
    javascript = compiled["stdout"].replace(
        " = // Original bounded runtime", " = globalThis.lightRuntime = // Original bounded runtime", 1)
    if "source_root" in case:
        javascript = f"observe => {{ const run = {javascript}; return run(observe); }}"
    script = ("globalThis.lightInspectValue = value => globalThis.lightRuntime?.inspect(value) ?? null;\n"
              f"globalThis.lightCases = [{javascript}];\n"
              f"globalThis.lightCaseMetadata = {json.dumps([[case['id'], case['source']]])};\n"
              + (ROOT / "tests/support/execution.js").read_text(encoding="utf-8"))
    ran = invoke([os.environ.get("NODE", "node"), "-"], directory / "javascript", source=script,
                 timeout=timeout)
    layers["javascript"] = (observations(ran["stdout"], [case])[case["id"]]
                            if ran["status"] == "completed" else ran)
    return layers


def local_diagnostic(case, binary, directory, timeout):
    """Locate failure by running the parser before the analyzer on every probe.

    Only the ordinary CLI's exit 1, one located diagnostic, and the entire exact
    expected message count. A crash, parser error, missing command, unsupported
    form, or successful compilation never substitutes for the expected error.
    """
    probes = []
    readers = {"lexer": [], "parser": []}
    for probe in case["diagnostic"]["probes"]:
        failure = None
        local_source = probe.get("local_source", probe["source"])
        for layer, flags in (("lexer", []), ("parser", ["--parse"])):
            run = invoke([binary, *flags, "-"], directory / probe["id"] / layer,
                         source=local_source, timeout=timeout)
            status = "accepted" if run["status"] == "completed" else (
                "mismatch" if run["status"] == "failed" else run["status"])
            readers[layer].append(result(status, id=probe["id"]))
            if status != "accepted":
                failure = result(status, f"{layer} did not accept the diagnostic source", id=probe["id"])
                break
        if failure is not None:
            probes.append(failure)
            continue
        if probe["mode"] == "ns":
            source_root = directory / probe["id"] / "source"
            path = source_root / (probe["entry"].replace(".", "/").replace("-", "_") + ".cljs")
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(local_source, encoding="utf-8")
            run = invoke([binary, "--compile", "--entry", probe["entry"], "--source-root", str(source_root)],
                         directory / probe["id"] / "namespace", timeout=timeout)
            prefix = "light-cljs: " + re.escape(str(path.resolve())) + ":"
            category = "namespace-error"
        else:
            run = invoke([binary, "--analyze", "-"], directory / probe["id"] / "analyzer",
                         source=local_source, timeout=timeout)
            prefix = "light-cljs: "
            category = "analysis-error"
        message = re.fullmatch(prefix + r"[0-9]+:[0-9]+: (.+) \(bytes [0-9]+\.\.[0-9]+\)\n",
                               run.get("stderr", ""))
        observed = {"category": category, "detail": message[1]} if message else None
        matches = (run["status"] == "failed" and run.get("returncode") == 1
                   and run.get("stdout") == "" and same_json(observed, probe["local"]))
        status = ("verified-rejection" if matches else "mismatch"
                  if run["status"] in ("completed", "failed") else run["status"])
        probes.append(result(status, id=probe["id"], observed=observed,
                             **({"process": run} if not matches else {})))
    layers = {layer: aggregate_probes(values, "accepted") for layer, values in readers.items()}
    layers.update({"diagnostic": aggregate_probes(probes, "verified-rejection"),
                   "native": result("not-applicable", "expected analyzer rejection"),
                   "javascript": result("not-applicable", "expected analyzer rejection")})
    return layers


def aggregate_probes(probes, success):
    if not probes:
        return result("invalid-output", "missing diagnostic probes")
    failed = next((probe for probe in probes if probe["status"] != success), None)
    return result(failed["status"] if failed else success, probes=probes)


def reference_cases(cases, kind, checkout, directory, timeout):
    warnings = [case for case in cases if "warning" in case]
    if warnings:
        ordinary = [case for case in cases if "warning" not in case]
        results = reference_cases(ordinary, kind, checkout, directory / "expressions", timeout) if ordinary else {}
        results.update(reference_warnings(warnings, kind, checkout, directory / "warnings", timeout))
        return results
    diagnostics = [case for case in cases if "diagnostic" in case]
    if diagnostics:
        ordinary = [case for case in cases if "diagnostic" not in case]
        results = reference_cases(ordinary, kind, checkout, directory / "expressions", timeout) if ordinary else {}
        results.update(reference_diagnostics(diagnostics, kind, checkout, directory / "diagnostics", timeout))
        return results
    differences = [case for case in cases if kind in case.get("compile_difference", {})]
    if differences:
        ordinary = [case for case in cases if case not in differences]
        results = reference_cases(ordinary, kind, checkout, directory / "expressions", timeout) if ordinary else {}
        results.update(reference_compile_differences(differences, kind, checkout, directory / "compile-differences", timeout))
        return results
    quiet = [case for case in cases if case.get("no_warnings")]
    if quiet and len(quiet) != len(cases):
        ordinary = [case for case in cases if not case.get("no_warnings")]
        return {**reference_cases(ordinary, kind, checkout, directory / "ordinary", timeout),
                **reference_cases(quiet, kind, checkout, directory / "no-warnings", timeout)}
    programs = [case for case in cases if "source_root" in case]
    if programs:
        ordinary = [case for case in cases if "source_root" not in case]
        results = reference_cases(ordinary, kind, checkout, directory / "expressions", timeout) if ordinary else {}
        for case in programs:
            results.update(reference_program(case, kind, checkout, directory / case["id"], timeout))
        return results
    directory.mkdir(parents=True, exist_ok=True)
    input_path = directory / "cases.tsv"
    input_path.write_text("".join(f"{case['id']}\t{case['source']}\n" for case in cases), encoding="utf-8")
    identity = RELEASE if kind == "release" else REVISION
    dependency = (f'{{:mvn/version "{RELEASE}"}}' if kind == "release"
                  else "{:local/root " + json.dumps(str(checkout)) + "}")
    command = [os.environ.get("CLOJURE", "clojure"), "-Srepro", "-Sdeps",
               "{:paths [] :deps {org.clojure/clojurescript " + dependency + "}}",
               "-M", "tests/reference/execution.clj", kind, identity,
               "-" if kind == "release" else str(checkout), str(directory), "cases", str(input_path)]
    compiled = invoke(command, directory / "build", timeout=timeout)
    if compiled["status"] != "completed":
        status = "reference-unavailable" if compiled["status"] == "unavailable" else compiled["status"]
        failure = result(status, compiled.get("detail", compiled.get("stderr", "")))
        return {case["id"]: failure for case in cases}
    if quiet and "WARNING:" in compiled.get("stderr", ""):
        return {case["id"]: result("mismatch", "unexpected reference warning: " + compiled["stderr"]) for case in cases}
    settings = "REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}"
    if (f"REFERENCE IDENTITY {kind} {identity}" not in compiled["stdout"]
            or settings not in compiled["stdout"] or not (directory / "reference.js").is_file()):
        return {case["id"]: result("invalid-reference", "unverified identity/options or missing output")
                for case in cases}
    # Native extension tests mutate cljs.core tables shared by namespaces.
    # Compile once, then execute isolated witnesses in fresh Node processes.
    batches = [[case] for case in cases if case.get("isolated")]
    ordinary = [case for case in cases if not case.get("isolated")]
    if ordinary:
        batches.insert(0, ordinary)
    results = {}
    for index, batch in enumerate(batches):
        indices = [cases.index(case) for case in batch]
        script = (f"process.chdir({json.dumps(str(directory))});\n"
                  f"require({json.dumps(str(directory / 'reference.js'))});\n"
                  f"const selected = {json.dumps(indices)};\n"
                  "globalThis.lightCases = selected.map(i => lightCases[i]);\n"
                  "globalThis.lightCaseMetadata = selected.map(i => lightCaseMetadata[i]);\n"
                  + (ROOT / "tests/support/execution.js").read_text(encoding="utf-8"))
        ran = invoke([os.environ.get("NODE", "node"), "-"], directory / f"node-{index}", source=script, timeout=timeout)
        if ran["status"] != "completed":
            results.update({case["id"]: ran for case in batch})
        else:
            results.update({case_id: {**observation, "reference_identity": kind + " " + identity,
                                      "settings": {"optimizations": "none", "static-fns": False, "def-emits-var": False}}
                            for case_id, observation in observations(ran["stdout"], batch).items()})
    return results


def checked_analysis(cases, kind, checkout, directory, timeout, *, exception_classes=False, warning_settings=False, driver=None):
    """Use the identity/options-checked boundary oracle, retaining ordered rows."""
    directory.mkdir(parents=True, exist_ok=True)
    input_path = directory / "cases.tsv"
    input_path.write_text("".join(f"{case['id']}\t{case['mode']}\t\t{case['source']}\t\n"
                                  for case in cases), encoding="utf-8")
    identity = RELEASE if kind == "release" else REVISION
    dependency = (f'{{:mvn/version "{RELEASE}"}}' if kind == "release"
                  else "{:local/root " + json.dumps(str(checkout)) + "}")
    command = [os.environ.get("CLOJURE", "clojure"), "-Srepro", "-Sdeps",
               "{:paths [] :deps {org.clojure/clojurescript " + dependency + "}}",
               "-M", driver or ("tests/reference/compiler-language/diagnostics.clj" if exception_classes else "tests/reference/execution.clj"), kind, identity,
               "-" if kind == "release" else str(checkout), str(directory), "analysis", str(input_path)]
    if warning_settings:
        settings_path = directory / "warning-settings.edn"
        settings_path.write_text("{" + " ".join(json.dumps(case["id"]) + " {:static-fns "
            + str(case["settings"]["static-fns"]).lower() + " :checked-arrays "
            + str(case["settings"]["checked-arrays"]).lower() + "}" for case in cases) + "}", encoding="utf-8")
        command[5] = "tests/reference/compiler-language/warnings.clj"
        command.append(str(settings_path))
    compiled = invoke(command, directory / "build", timeout=timeout)
    if compiled["status"] != "completed":
        status = "reference-unavailable" if compiled["status"] == "unavailable" else compiled["status"]
        return {case["id"]: result(status, compiled.get("detail", compiled.get("stderr", ""))) for case in cases}
    settings = "REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}"
    if (f"REFERENCE IDENTITY {kind} {identity}" not in compiled["stdout"]
            or settings not in compiled["stdout"] or not (directory / "analysis.tsv").is_file()):
        return {case["id"]: result("invalid-reference", "unverified identity/options or missing rejection evidence") for case in cases}
    rows = [line.split("\t", 3) for line in (directory / "analysis.tsv").read_text(encoding="utf-8").split("\n") if line]
    if len(rows) != len(cases) or any(len(row) != 4 or row[0] != case["id"] for row, case in zip(rows, cases)):
        return {case["id"]: result("invalid-output", "missing, duplicate, reordered or incomplete compile observations") for case in cases}
    results = {case["id"]: result("completed",
                observed={"category": category, "warnings": warnings, "detail": detail},
                reference_identity=kind + " " + identity,
                settings={"optimizations": "none", "static-fns": False, "def-emits-var": False})
            for case, (_, category, warnings, detail) in zip(cases, rows)}
    if exception_classes:
        path = directory / "exception-classes.tsv"
        classes = [line.split("\t") for line in path.read_text(encoding="utf-8").splitlines()] if path.is_file() else []
        if len(classes) != len(cases) or any(len(row) != 2 or row[0] != case["id"] for row, case in zip(classes, cases)):
            return {case["id"]: result("invalid-output", "missing, duplicate, reordered or incomplete exception classes") for case in cases}
        for case, (_, name) in zip(cases, classes):
            results[case["id"]]["observed"]["exception_class"] = name
    if warning_settings:
        try:
            warning_rows = [line.split("\t", 1) for line in (directory / "warnings.tsv").read_text(encoding="utf-8").splitlines()]
            settings_rows = [line.split("\t") for line in (directory / "warning-settings.tsv").read_text(encoding="utf-8").splitlines()]
            if (len(warning_rows) != len(cases) or len(settings_rows) != len(cases)
                    or any(len(row) != 2 or row[0] != case["id"] for row, case in zip(warning_rows, cases))
                    or any(row != [case["id"], str(case["settings"]["static-fns"]).lower(), str(case["settings"]["checked-arrays"]).lower()]
                           for row, case in zip(settings_rows, cases))):
                raise ValueError("missing, reordered, duplicate or invalid warning/settings observations")
            for case, (_, encoded) in zip(cases, warning_rows):
                trace = json.loads(encoded)
                if not valid_warning_trace(trace):
                    raise ValueError("invalid warning trace")
                results[case["id"]]["observed"]["warning_trace"] = trace
                results[case["id"]]["settings"].update(case["settings"])
        except (ValueError, OSError) as error:
            return {case["id"]: result("invalid-output", str(error)) for case in cases}
    return results


def reference_compile_differences(cases, kind, checkout, directory, timeout):
    probes = [{"id": case["id"], "mode": "expr", "source": f"(fn* [observe] {case['source']})"}
              for case in cases]
    observations = checked_analysis(probes, kind, checkout, directory, timeout)
    results = {}
    for case in cases:
        value = observations[case["id"]]
        if value["status"] == "completed":
            expected = case["compile_difference"][kind]
            observed = value["observed"]
            matches = observed["category"] == expected["category"] and expected["detail_contains"] in observed["detail"]
            value = {**value, "status": "verified-compile-difference" if matches else "mismatch"}
        results[case["id"]] = value
    return results


def reference_diagnostics(cases, kind, checkout, directory, timeout):
    probes = [{**probe, "id": case["id"] + "--" + probe["id"]}
              for case in cases for probe in case["diagnostic"]["probes"]]
    observations = checked_analysis(probes, kind, checkout, directory, timeout, exception_classes=True)
    results = {}
    for case in cases:
        checked = []
        success = ("verified-diagnostic-difference" if kind == "release" and case["diagnostic"].get("release_difference")
                   else "verified-rejection")
        for probe in case["diagnostic"]["probes"]:
            value = observations[case["id"] + "--" + probe["id"]]
            if value["status"] == "completed":
                matches = same_json(value["observed"], probe[kind])
                value = {**value, "status": success if matches else "mismatch"}
            checked.append({**value, "id": probe["id"]})
        results[case["id"]] = aggregate_probes(checked, success)
    return results


def reference_program(case, kind, checkout, directory, timeout):
    """Use the same checked program oracle as the Rust namespace comparisons."""
    import namespace_results
    namespace_results.validate(case)
    validate_reference_source(case)
    directory.mkdir(parents=True, exist_ok=True)
    identity = RELEASE if kind == "release" else REVISION
    dependency = (f'{{:mvn/version "{RELEASE}"}}' if kind == "release"
                  else "{:local/root " + json.dumps(str(checkout)) + "}")
    root = ROOT / case.get("reference_source_root", case["source_root"])
    command = [os.environ.get("CLOJURE", "clojure"), "-Srepro", "-Sdeps",
               "{:paths [" + json.dumps(str(root)) + "] :deps {org.clojure/clojurescript " + dependency + "}}",
               "-M", "tests/reference/execution.clj", kind, identity,
               "-" if kind == "release" else str(checkout), str(directory), "program", str(root), case["entry"], case["export"]]
    if case.get("nodejs_rt") is False:
        command.append("false")
    compiled = invoke(command, directory / "build", timeout=timeout)
    if compiled["status"] != "completed":
        return {case["id"]: result("reference-unavailable" if compiled["status"] == "unavailable" else compiled["status"],
                                  compiled.get("detail", compiled.get("stderr", "")))}
    if case.get("nodejs_rt") is False and "REFERENCE NODEJS_RT false" not in compiled.get("stdout", "").splitlines():
        return {case["id"]: result("invalid-reference", "unverified explicit Node bootstrap setting")}
    if case.get("no_warnings") and "WARNING:" in compiled.get("stderr", ""):
        return {case["id"]: result("mismatch", "unexpected reference warning: " + compiled["stderr"])}
    settings = "REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}"
    if (f"REFERENCE IDENTITY {kind} {identity}" not in compiled["stdout"] or settings not in compiled["stdout"]
            or not (directory / "reference.js").is_file()):
        return {case["id"]: result("invalid-reference", "unverified identity/options or missing output")}
    loader = (f"require({json.dumps(str(directory / 'out/goog/bootstrap/nodejs.js'))}); "
              f"require({json.dumps(str(directory / 'out/cljs_deps.js'))}); goog.require('semantic.program'); "
              if case.get("nodejs_rt") is False else f"require({json.dumps(str(directory / 'reference.js'))}); ")
    script = (f"globalThis.lightCases = [observe => {{ process.chdir({json.dumps(str(directory))}); "
              f"{loader}return globalThis.lightProgramRun(observe); }}];\n"
              "globalThis.lightInspectValue = value => globalThis.lightProgramInspectValue(value);\n"
              f"globalThis.lightCaseMetadata = {json.dumps([[case['id'], case['source']]])};\n"
              + (ROOT / "tests/support/execution.js").read_text(encoding="utf-8"))
    ran = invoke([os.environ.get("NODE", "node"), "-"], directory / "node", source=script, timeout=timeout)
    if ran["status"] != "completed":
        return {case["id"]: ran}
    namespace_results.validate(case)
    return {key: {**observed, "reference_identity": kind + " " + identity,
                  "settings": {"optimizations": "none", "static-fns": False, "def-emits-var": False,
                               **({"nodejs-rt": False} if case.get("nodejs_rt") is False else {})}}
            for key, observed in observations(ran["stdout"], [case]).items()}


def classify(row, witness, evidence):
    if row["disposition"] == "unrelated":
        return "unrelated", row["reason"]
    if witness is None:
        return "missing", "no assertion-faithful executable witness"
    observed = evidence.get(witness["fixture_id"], {})
    if witness.get("expected_warning") is True:
        for layer in REJECTION_LAYERS:
            status = observed.get(layer, {}).get("status", "not-run")
            expected = ("verified-warning-difference" if layer == "release" and witness.get("warning_difference") else "verified-warning")
            if status != expected:
                return "missing", f"{layer}: {status}"
        if witness.get("warning_difference"):
            return "version-difference", witness["warning_difference"]
        return "verified-warning", "all original warning assertions match local compilation and both checked references"
    if witness.get("expected_rejection") is True:
        for layer in REJECTION_LAYERS:
            status = observed.get(layer, {}).get("status", "not-run")
            expected = ("verified-diagnostic-difference" if layer == "release" and witness.get("diagnostic_difference")
                        else "verified-rejection")
            if status != expected:
                return "missing", f"{layer}: {status}"
        if witness.get("diagnostic_difference"):
            return "version-difference", witness["diagnostic_difference"]
        return "verified-rejection", "all original diagnostic probes match the local analyzer and both pinned references"
    for layer in SEMANTIC_LAYERS:
        status = observed.get(layer, {}).get("status", "not-run")
        if layer in witness.get("compile_difference", {}) and status == "verified-compile-difference":
            continue
        if status != "passed":
            return "missing", f"{layer}: {status}"
    if witness.get("compile_difference"):
        # An unexpected successful release build is not proof of the declared
        # incompatibility and must not become covered through the normal path.
        for layer in ("release",):
            if observed.get(layer, {}).get("status") != "verified-compile-difference":
                return "missing", "declared compile difference was not observed"
        return "version-difference", witness["compile_difference"]["reason"]
    if witness.get("version_difference"):
        return "version-difference", witness["version_difference"]["reason"]
    return "covered", "generated JavaScript and both pinned references match the assertion witness"


def boundary_inventory():
    """Keep former accepted-source restrictions separate from negative tests."""
    rows = []
    for line in (ROOT / "tests/fixtures/analysis-boundaries.tsv").read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        identity, mode, before, rejected, after, category, diagnostic, release, checkout = line.split("\t")
        if category == "accepted":
            disposition = "accepted-boundary"
        elif release in ("accepted", "warning") or checkout in ("accepted", "warning"):
            disposition = "missing-capability"
        else:
            disposition = "negative-test"
        if identity in ("ns-package", "ns-import", "special-nsstar"):
            disposition = "unrelated-build-loading"
        feature = ("functions-apply" if identity.startswith("fn-") else
                   "recursion" if (identity in ("special-loopstar", "special-letfnstar", "special-recur") or identity.startswith("recur-")) else
                   "exceptions" if identity in ("special-throw", "special-try") else
                   "reader-metadata" if identity.startswith("reader-") else
                   "namespaces" if identity.startswith("ns-") else "language-boundaries")
        rows.append({"id": identity, "feature": feature, "disposition": disposition,
                     "source": before + rejected + after, "mode": mode,
                     "local_category": category, "diagnostic": diagnostic,
                     "release": release, "checkout": checkout})
    return rows


# Supplemental legacy boundary evidence is deliberately outside assertion witnesses.
# Fixing a formerly rejected source must be demonstrated using its actual public
# API and checked diagnostics; no marker here contributes an assertion pass.
BOUNDARY_SUPPORT_IDS = set("""let-binding-amp let-binding-keyword fn-empty-method fn-binding-keyword
self-init forward-local self-closure leaked-local leaked-param leaked-fn-name forward-global
unknown-global unqualified-host unknown-alias macro-env reader-metadata reader-conditional
macro-defmacro def-dot def-amp host-empty-path member-qualified ns-as-duplicate ns-as-js
fn-malformed-empty-arity fn-overload-duplicate fn-overload-fixed-too-large""".split())


def boundary_support_contract():
    cases = unique(jsonl(HERE / "boundary-support.jsonl"), "id", "supplemental boundary")
    matrix = {row["id"]: row for row in boundary_inventory()}
    if set(cases) != BOUNDARY_SUPPORT_IDS:
        raise ValueError("supplemental boundary catalog must retain all 27 audited rows")
    runtime_ids = set()
    for identity, case in cases.items():
        original = matrix.get(identity)
        # Source macro declarations are no longer a rejected-source test. Keep
        # their exact legacy provenance after retiring that rejection row.
        if original is None and identity == "macro-defmacro" and case.get("kind") == "intentional-difference":
            original = {"source": "(defmacro m [] 1)", "mode": "expr"}
        if original is None:
            raise ValueError(f"supplemental boundary has no retained original source: {identity}")
        if (case.get("source") != original["source"] or case.get("mode") != original["mode"]
                or case.get("source_sha256") != hashlib.sha256(original["source"].encode()).hexdigest()
                or not case.get("rationale", "").strip()):
            raise ValueError(f"supplemental boundary detached from original source: {identity}")
        kind = case.get("kind")
        if kind not in {"public-runtime", "warning-api", "reader-api", "primitive-permissiveness", "version-difference", "intentional-difference"}:
            raise ValueError(f"invalid supplemental disposition: {identity}")
        local_probes = case.get("local_probes", [])
        if kind == "intentional-difference":
            phase_contracts = {
                "macro-defmacro": (["boundary-defmacro-phase-value", "boundary-defmacro-phase-apply", "boundary-defmacro-phase-var"],
                                   ["boundary-defmacro-compile-time-expansion"],
                                   ["boundary-defmacro-metadata-hidden-args", "boundary-defmacro-methods-apply-receiver", "boundary-defmacro-dispatch-abi"]),
                "ns-as-duplicate": ([], ["boundary-duplicate-alias-local-macro-runtime"], ["boundary-duplicate-alias-runtime"]),
            }
            contract = phase_contracts.get(identity)
            if (contract is None or case.get("runtime")
                    or [probe["id"] for probe in local_probes] != contract[0]
                    or [proof["id"] for proof in case.get("local_runtime", [])] != contract[1]
                    or [proof["id"] for proof in case.get("reference_runtime", [])] != contract[2]):
                raise ValueError(f"macro phase difference needs separate local and retained reference proofs: {identity}")
        elif any(case.get(key) for key in ("local_probes", "local_runtime", "reference_runtime")):
            raise ValueError(f"separate phase proofs require an intentional difference: {identity}")
        probes = [case, *case.get("public_equivalents", []), *case.get("supplemental_probes", []), *local_probes]
        if kind in {"primitive-permissiveness", "version-difference"} and case.get("local", {}).get("category") != "analysis-error":
            raise ValueError(f"boundary difference requires the actual local rejection: {identity}")
        if kind == "primitive-permissiveness":
            equivalents = case.get("public_equivalents", [])
            expected_public = case["source"].replace("(fn*", "(fn", 1).replace("(let*", "(let", 1)
            if (len(equivalents) != 1 or equivalents[0].get("source") != expected_public
                    or any(case.get(layer, {}).get("category") not in {"accepted", "warning"} for layer in ("release", "checkout"))):
                raise ValueError(f"primitive difference must retain its exact public spelling: {identity}")
        if kind == "version-difference" and (case.get("release", {}).get("category") != "accepted"
                or case.get("checkout", {}).get("category") != "analysis-error"):
            raise ValueError(f"versioned boundary requires the declared accepted/rejected split: {identity}")
        if (kind == "primitive-permissiveness") != bool(case.get("public_equivalents")):
            raise ValueError(f"primitive difference needs checked public syntax: {identity}")
        if kind in {"public-runtime", "reader-api"} and not case.get("runtime"):
            raise ValueError(f"public support needs executed runtime proof: {identity}")
        if kind in {"primitive-permissiveness", "version-difference"} and case.get("runtime"):
            raise ValueError(f"primitive/version diagnostics cannot masquerade as runtime passes: {identity}")
        for probe in probes:
            if (probe in case.get("supplemental_probes", []) or probe in local_probes) and (
                    probe.get("source_sha256") != hashlib.sha256(probe["source"].encode()).hexdigest()
                    or not probe.get("rationale", "").strip()):
                raise ValueError(f"supplemental diagnostic needs hashed source and rationale: {identity}")
            if (not re.fullmatch(r"[a-zA-Z0-9-]+", probe["id"])
                    or probe.get("mode") not in {"expr", "statement", "ns", "cljc"}
                    or not probe.get("source", "").strip() or any(char in probe["source"] for char in "\r\n\t")
                    or probe.get("settings") != {"static-fns": False, "checked-arrays": False}
                    or probe.get("local_api") not in {"default", "warnings", "namespace", "namespace-warnings", "cljc"}):
                raise ValueError(f"invalid supplemental probe API/settings: {identity}")
            local = probe.get("local", {})
            reject = local.get("category") == "analysis-error"
            if reject:
                if set(local) != {"category", "detail"} or not local["detail"].strip():
                    raise ValueError(f"supplemental rejection needs exact reason: {identity}")
            elif set(local) != {"category", "warning_trace"} or local["category"] != "accepted" or not valid_warning_trace(local["warning_trace"]):
                raise ValueError(f"supplemental support requires compilation and precise warnings: {identity}")
            if probe in local_probes:
                var_identity = identity == "macro-defmacro" and probe["id"] == "boundary-defmacro-phase-var"
                if reject == var_identity or any(layer in probe for layer in ("release", "checkout")):
                    raise ValueError(f"local phase proof requires its independent identity/rejection contract: {identity}")
                continue
            for layer in ("release", "checkout"):
                ref = probe.get(layer, {})
                if (set(ref) != {"category", "warnings", "detail", "warning_trace"}
                        or ref["category"] not in {"accepted", "warning", "analysis-error"}
                        or not valid_warning_trace(ref["warning_trace"])
                        or ref["warnings"] != ",".join(sorted({row[0] for row in ref["warning_trace"]}))
                        or (ref["category"] == "analysis-error") != bool(ref["detail"])
                        or (ref["category"] != "analysis-error" and (ref["category"] == "warning") != bool(ref["warning_trace"]))):
                    raise ValueError(f"supplemental reference needs exact phase and ordered warnings: {identity}")
            if kind not in {"primitive-permissiveness", "version-difference"}:
                if reject or any(probe[layer]["category"] == "analysis-error" or not same_json(local["warning_trace"], probe[layer]["warning_trace"]) for layer in ("release", "checkout")):
                    raise ValueError(f"supplemental support does not implement checked public behavior: {identity}")
            elif probe is not case and (not reject or any(probe[layer]["category"] != "analysis-error" for layer in ("release", "checkout"))):
                raise ValueError(f"primitive difference requires rejected public equivalents: {identity}")
            if "local_files" in probe:
                entry_file = probe.get("entry", "").replace(".", "/").replace("-", "_") + (".cljc" if probe["mode"] == "cljc" else ".cljs")
                original_source = probe["source"]
                if probe["mode"] == "cljc":
                    original_source = "(ns " + probe.get("entry", "") + ") " + original_source
                if probe["local_files"].get(entry_file) != original_source:
                    raise ValueError(f"supplemental namespace must retain the exact original source: {identity}")
                if not probe.get("local_adaptation", "").strip() or not probe.get("entry"):
                    raise ValueError(f"namespace supplemental probe needs explicit harness: {identity}")
                for name, source in probe["local_files"].items():
                    if Path(name).is_absolute() or ".." in Path(name).parts or not name.endswith((".cljs", ".cljc", ".clj")) or not source.strip():
                        raise ValueError(f"invalid supplemental namespace file: {identity}")
        for runtime in [*case.get("runtime", []), *case.get("local_runtime", []), *case.get("reference_runtime", [])]:
            if (runtime["id"] in runtime_ids or runtime.get("native_stdout", "missing") is not None
                    or not runtime.get("source", "").strip() or not runtime.get("rationale", "").strip()
                    or runtime.get("source_sha256") != hashlib.sha256(runtime["source"].encode()).hexdigest()
                    or set(runtime.get("expected", {})) != {"id", "outcome", "trace", "stdout"}
                    or runtime["expected"]["id"] != runtime["id"]
                    or runtime.get("local_warning_mode") not in (None, True)):
                raise ValueError(f"incomplete supplemental runtime contract: {identity}")
            runtime_ids.add(runtime["id"])
            if "source_root" in runtime:
                if Path(runtime["source_root"]).is_absolute() or ".." in Path(runtime["source_root"]).parts:
                    raise ValueError(f"invalid supplemental runtime source root: {identity}")
                root = ROOT / runtime["source_root"]
                hashes = {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
                          for path in sorted(root.rglob("*")) if path.is_file()}
                if not hashes or hashes != runtime.get("files_sha256"):
                    raise ValueError(f"supplemental runtime source tree drift: {identity}")
    return list(cases.values())


def local_boundary_probe(probe, binary, directory, timeout):
    if probe["local"]["category"] == "analysis-error":
        return local_diagnostic({"diagnostic": {"probes": [probe]}}, binary, directory, timeout)["diagnostic"]
    # Use the warning runner's exact stderr parsing, source-reader checks and
    # emitted-JavaScript syntax validation for quiet default APIs as well.
    return local_warning({"warning": {"probes": [probe]}}, binary, directory, timeout)["diagnostic"]


def boundary_support_status(case, evidence):
    required = {"local", "release", "checkout"}
    if any(value.get("status") != "verified-boundary" for value in evidence.values()):
        return "mismatch"
    if set(evidence) != required:
        return "not-run"
    return "verified-" + case["kind"]


def run_boundary_support(cases, binary, checkout, directory, timeout, reference_timeout, references):
    evidence = {case["id"]: {} for case in cases}
    runtime = [value for case in cases for value in [*case.get("runtime", []), *case.get("reference_runtime", [])]]
    for case in cases:
        checks = []
        for probe in [case, *case.get("public_equivalents", []), *case.get("supplemental_probes", []), *case.get("local_probes", [])]:
            value = local_boundary_probe(probe, binary, directory / "local" / probe["id"], timeout)
            expected = "verified-rejection" if probe["local"]["category"] == "analysis-error" else "verified-warning"
            checks.append({**value, "status": "verified-boundary" if value["status"] == expected else value["status"], "id": probe["id"]})
        for proof in [*case.get("runtime", []), *case.get("local_runtime", [])]:
            value = local_layers(proof, binary, directory / "local-runtime" / proof["id"], timeout,
                                 warning_mode=proof.get("local_warning_mode", False))["javascript"]
            checks.append({**value, "status": "verified-boundary" if value["status"] == "passed" else value["status"], "id": proof["id"], "execution": True})
        evidence[case["id"]]["local"] = aggregate_probes(checks, "verified-boundary")
    probes = [probe for case in cases for probe in [case, *case.get("public_equivalents", []), *case.get("supplemental_probes", [])]]
    for kind in ("release", "checkout"):
        if references not in (kind, "both"):
            continue
        analyzed = checked_analysis(probes, kind, checkout, directory / kind / "analysis", reference_timeout, warning_settings=True)
        executed = reference_cases(runtime, kind, checkout, directory / kind / "runtime", reference_timeout) if runtime else {}
        for case in cases:
            checks = []
            for probe in [case, *case.get("public_equivalents", []), *case.get("supplemental_probes", [])]:
                value = analyzed[probe["id"]]
                checks.append({**value, "id": probe["id"], "status": ("verified-boundary" if same_json(value["observed"], probe[kind]) else "mismatch") if value["status"] == "completed" else value["status"]})
            for proof in [*case.get("runtime", []), *case.get("reference_runtime", [])]:
                value = executed[proof["id"]]
                checks.append({**value, "id": proof["id"], "execution": True, "status": "verified-boundary" if value["status"] == "passed" else value["status"]})
            evidence[case["id"]][kind] = aggregate_probes(checks, "verified-boundary")
    return evidence


def assemble(inventory, witnesses, evidence, *, features=(), identities=(), boundary_evidence=None):
    rows = []
    for identity, row in inventory.items():
        if features and row["feature"] not in features:
            continue
        if identities and identity not in identities:
            continue
        witness = witnesses.get(identity)
        status, reason = classify(row, witness, evidence)
        rows.append({"id": identity, "feature": row["feature"], "status": status,
                     "reason": reason, "source": row["source"], "test": row["test"],
                     "line": row["assertion"]["start_line"], "fixture_id": witness["fixture_id"] if witness else None,
                     "layers": evidence.get(witness["fixture_id"], {}) if witness else {}})
    if not rows:
        raise ValueError("selection contains no assertions")
    counts = Counter(row["status"] for row in rows)
    sections = defaultdict(Counter)
    for row in rows:
        sections[row["feature"]][row["status"]] += 1
    boundary_rows = boundary_inventory()
    support = boundary_support_contract()
    support_rows = [{"id": case["id"], "kind": case["kind"], "local_api": case["local_api"],
                     "status": boundary_support_status(case, (boundary_evidence or {}).get(case["id"], {})),
                     "rationale": case["rationale"], "layers": (boundary_evidence or {}).get(case["id"], {})} for case in support]
    support_by_id = {row["id"]: row for row in support_rows}
    for row in boundary_rows:
        if row["id"] in support_by_id:
            row["disposition"] = "supplemental-" + support_by_id[row["id"]]["kind"]
            row["verification"] = support_by_id[row["id"]]["status"]
    boundary_complete = all(row["status"].startswith("verified-") for row in support_rows)
    return {"schema_version": 1, "upstream_revision": REVISION, "release": RELEASE,
            "scope": "assertions in the explicitly inventoried source files; not the upstream suite",
            "selection": {"features": list(features), "ids": list(identities)},
            "counts": {key: counts[key] for key in ("covered", "verified-rejection", "verified-warning", "missing", "version-difference", "unrelated")},
            "features": {key: dict(value) for key, value in sorted(sections.items())},
            "boundary_counts": dict(Counter(row["disposition"] for row in boundary_rows)),
            "boundaries": boundary_rows, "assertions": rows,
            "boundary_support": support_rows, "boundary_strict_complete": boundary_complete,
            "source_strict_complete": counts["missing"] == 0,
            "strict_complete": counts["missing"] == 0 and boundary_complete}


def text_report(report, show_missing):
    print("Compiler language assertions — live observations only (" + report["scope"] + ")")
    if "columns" in report:
        print("Evidence columns: " + ", ".join(report["columns"]))
        for column in report["columns"]:
            counts = Counter(row["columns"][column]["status"] for row in report["assertions"])
            print(column + ": " + ", ".join(f"{status}={count}" for status, count in sorted(counts.items())))
    print("feature\tcovered\tverified-rejection\tverified-warning\tmissing\tversion-difference\tunrelated")
    for feature, counts in report["features"].items():
        print(feature + "\t" + "\t".join(str(counts.get(key, 0)) for key in
                                               ("covered", "verified-rejection", "verified-warning", "missing", "version-difference", "unrelated")))
    print("TOTAL\t" + "\t".join(str(value) for value in report["counts"].values()))
    layers = {layer: Counter(row["layers"].get(layer, {}).get("status", "not-run")
                             for row in report["assertions"] if row["fixture_id"]) for layer in LAYERS}
    for layer, counts in layers.items():
        print(f"{layer}: " + ", ".join(f"{key}={value}" for key, value in sorted(counts.items())))
    print("Boundary diagnostics (not passing semantic assertions): " + json.dumps(report["boundary_counts"], sort_keys=True))
    for row in report["boundaries"]:
        if row["id"] in ("fn-variadic", "fn-multiple", "special-recur"):
            print(f"  {row['id']}: {row['disposition']} ({row['diagnostic']})")
    if show_missing:
        for row in report["assertions"]:
            if row["status"] == "missing":
                print(f"MISSING {row['id']} [{row['feature']}] owner={row.get('owner', 'historical-selected')}: {row['reason']}")
    print("Supplemental boundary verification: " + json.dumps(dict(Counter(row["status"] for row in report["boundary_support"])), sort_keys=True))
    print("Strict completion: " + ("complete" if report["strict_complete"] else "INCOMPLETE"))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", choices=("intended-language", "historical-selected"), default="intended-language", help="full independent ledger (default) or preserved 18-file batch")
    parser.add_argument("--run", action="store_true", help="execute witnesses; absent witnesses remain missing")
    parser.add_argument("--strict", action="store_true", help="require every selected source assertion and every supplemental boundary check")
    parser.add_argument("--feature", action="append", default=[], help="exact feature section; repeatable")
    parser.add_argument("--id", action="append", default=[], help="exact stable assertion ID; repeatable")
    parser.add_argument("--fixture", action="append", default=[], help="execute only these exact fixture IDs; all other obligations remain in the report and strict gate")
    parser.add_argument("--reference", choices=("none", "release", "checkout", "both"), default="both")
    parser.add_argument("--checkout", type=Path, default=ROOT / "research/clojurescript")
    parser.add_argument("--binary", type=Path, help="prebuilt executable (requires --no-build)")
    parser.add_argument("--no-build", action="store_true", help="use an explicitly prebuilt binary")
    parser.add_argument("--timeout", type=float, default=30, help="seconds per local case/layer")
    parser.add_argument("--reference-timeout", type=float, default=180)
    parser.add_argument("--json", action="store_true", help="emit full machine-readable report")
    parser.add_argument("--output", type=Path, help="also save full JSON report")
    parser.add_argument("--show-missing", action="store_true")
    parser.add_argument("--artifacts", type=Path, help="fresh directory for invocations, outputs and report")
    args = parser.parse_args(argv)
    if args.timeout <= 0 or args.reference_timeout <= 0:
        parser.error("timeouts must be positive")
    if args.binary and not args.no_build:
        parser.error("--binary requires --no-build; otherwise use the executable reported by Cargo")
    try:
        historical_inventory, historical_witnesses, historical_cases = load_contract()
        inventory, witnesses, cases = load_contract(args.scope) if args.scope == "intended-language" else (historical_inventory, historical_witnesses, historical_cases)
        if set(args.fixture) - set(cases):
            raise ValueError("unknown execution fixture ID")
        from full_ledger import expand_report, load as load_full, sci_execution_failed
        full_rows, obligations, _ = load_full() if args.scope == "intended-language" else ([], [], {})
        all_features = {row["feature"] for row in [*inventory.values(), *full_rows]} | {row["family"] for row in [*full_rows, *obligations]}
        if set(args.feature) - all_features or set(args.id) - (set(inventory) | {row["id"] for row in full_rows}):
            raise ValueError("unknown feature or assertion ID")
        historical_filters = {"features": args.feature, "identities": args.id} if args.scope == "historical-selected" else {}
        selected = assemble(historical_inventory, historical_witnesses, {}, **historical_filters)
        if args.scope == "intended-language":
            selected = expand_report(selected, witnesses, {}, features=args.feature, identities=args.id)
        selected_ids = {row["fixture_id"] for row in selected["assertions"] if row["fixture_id"]}
        selected_cases = [case for identity, case in cases.items() if identity in selected_ids]
        if args.fixture:
            if set(args.fixture) - {case['id'] for case in selected_cases}:
                raise ValueError("execution fixture outside selected report scope")
            selected_cases = [case for case in selected_cases if case['id'] in args.fixture]
        evidence = {}
        boundary_evidence = {}
        phase_evidence = {}
        sci_evidence = {}
        library_dialect_evidence = None
        reviewed_sci_context_evidence = None
        gap_sci_context_evidence = None
        print_sci_context_evidence = None
        cljc_sci_context_evidence = None
        lazy_sci_context_evidence = None
        case_sci_context_evidence = None
        artifacts = None
        binary_identity = None
        if args.run:
            if args.artifacts:
                artifacts = args.artifacts.resolve()
                artifacts.mkdir(parents=True, exist_ok=False)
            else:
                artifacts = Path(tempfile.mkdtemp(prefix="light-cljs-language-"))
            if not args.no_build:
                build = invoke(["cargo", "build", "--locked", "--bin", "light-cljs", "--message-format=json"],
                               artifacts / "cargo", timeout=180)
                if build["status"] != "completed":
                    raise ValueError(f"compiler build failed; artifacts: {artifacts}")
                built = [row["executable"] for row in (json.loads(line) for line in build["stdout"].splitlines())
                         if row.get("reason") == "compiler-artifact" and row["target"]["name"] == "light-cljs"
                         and row.get("executable")]
                if len(built) != 1:
                    raise ValueError(f"Cargo did not identify exactly one compiler executable; artifacts: {artifacts}")
                binary = str(Path(built[0]).resolve())
            else:
                binary = str((args.binary or ROOT / "target/debug/light-cljs").resolve())
            binary_identity = {"path": binary, "sha256": hashlib.sha256(Path(binary).read_bytes()).hexdigest()}
            import hosted_source_dispatch
            hosted_cases = [case for case in selected_cases if hosted_source_dispatch.validate_case(case)]
            ordinary_cases = [case for case in selected_cases if case not in hosted_cases]
            if hosted_cases:
                evidence.update(hosted_source_dispatch.execute(hosted_cases, artifacts / 'hosted-source',
                                max(240, args.reference_timeout), args.reference, args.checkout.resolve()))
            for case in ordinary_cases:
                evidence[case["id"]] = local_layers(case, binary, artifacts / "local" / case["id"], args.timeout)
            for kind in ("release", "checkout"):
                if args.reference not in (kind, "both") or not ordinary_cases:
                    continue
                expected = {identity: case["expected"] for identity, case in cases.items() if "expected" in case}
                for witness in witnesses.values():
                    if witness.get("version_difference"):
                        expected[witness["fixture_id"]] = witness["version_difference"][kind]
                reference_inputs = [case if "diagnostic" in case or "warning" in case else {**case, "expected": expected[case["id"]]}
                                    for case in ordinary_cases]
                for identity, observed in reference_cases(reference_inputs, kind, args.checkout.resolve(),
                                                           artifacts / kind, args.reference_timeout).items():
                    evidence[identity][kind] = observed
            boundary_evidence = run_boundary_support(boundary_support_contract(), binary, args.checkout.resolve(),
                                                     artifacts / "boundary-support", args.timeout, args.reference_timeout, args.reference)
        if args.run and args.scope == "intended-language":
            from full_ledger import run_phase_witnesses
            phase_evidence = run_phase_witnesses(obligations, artifacts / "macro-phase", args.timeout,
                reference_fixtures=args.fixture or None)
            # SCI rows are independent source obligations. Execute the original
            # portable cohort through both local engines and checked pins; a
            # historical evaluator disposition never supplies a present pass.
            if not args.feature and not args.id and not args.fixture:
                import sci_execution
                import sci_source_context
                import sci_library_context
                source_rows = sci_source_context.load()
                library_rows = sci_library_context.load()
                expression_rows = sci_source_context.expression_rows(sci_execution.load_all(), source_rows)
                import reviewed_sci_context_dispatch
                ordinary_sci_rows, _ = reviewed_sci_context_dispatch.partition(
                    sci_library_context.expression_rows(expression_rows, library_rows))
                sci_cases = sci_execution.cases(ordinary_sci_rows)
                sci_evidence = {case["id"]: {} for case in sci_cases}
                local = sci_execution.local_cases(sci_cases, artifacts / "sci" / "local", max(180, args.reference_timeout))
                for identity, observed in local.items():
                    sci_evidence[identity]["javascript"] = observed
                for kind in ("release", "checkout"):
                    if args.reference in (kind, "both"):
                        observed = sci_execution.reference_cases(sci_cases, kind, args.checkout.resolve(), artifacts / "sci" / kind, args.reference_timeout)
                        for identity, layer in observed.items():
                            sci_evidence[identity][kind] = layer
                (artifacts / "sci" / "evidence.json").write_text(json.dumps(sci_evidence, indent=2) + "\n")
                source_evidence = {row["id"]: {} for row in source_rows}
                local = sci_source_context.local_cases(source_rows, artifacts / "sci-source-context" / "local", max(180, args.reference_timeout))
                for identity, layer in local.items():
                    source_evidence[identity]["javascript"] = layer
                for kind in ("release", "checkout"):
                    if args.reference in (kind, "both"):
                        observed = sci_source_context.reference_cases(source_rows, kind, artifacts / "sci-source-context" / kind, args.reference_timeout)
                        for identity, layer in observed.items():
                            source_evidence[identity][kind] = layer
                if set(sci_evidence) & set(source_evidence):
                    raise ValueError("SCI expression/source-context execution identities overlap")
                sci_evidence.update(source_evidence)
                (artifacts / "sci-source-context" / "evidence.json").write_text(json.dumps({
                    "catalog_sha256": sci_execution.digest(sci_source_context.CATALOG.read_bytes()),
                    "evidence": source_evidence}, indent=2) + "\n")
                library_evidence = {row["id"]: {} for row in library_rows}
                local = sci_library_context.local_cases(library_rows, artifacts / "sci-library-context" / "local", max(180, args.reference_timeout))
                for identity, layer in local.items():
                    library_evidence[identity]["javascript"] = layer
                for kind in ("release", "checkout"):
                    if args.reference in (kind, "both"):
                        observed = sci_library_context.reference_cases(library_rows, kind, artifacts / "sci-library-context" / kind, args.reference_timeout)
                        for identity, layer in observed.items():
                            library_evidence[identity][kind] = layer
                if set(sci_evidence) & set(library_evidence):
                    raise ValueError("SCI expression/source/library-context execution identities overlap")
                sci_evidence.update(library_evidence)
                library_document = {
                    "catalog_sha256": sci_execution.digest(sci_library_context.CATALOG.read_bytes()),
                    "evidence": library_evidence}
                (artifacts / "sci-library-context" / "evidence.json").write_text(json.dumps(library_document, indent=2) + "\n")
                if len(library_rows) == 191 and args.reference == 'both' and all(
                        set(library_evidence[row['id']]) == {'javascript','release','checkout'} for row in library_rows):
                    import sci_library_reconciliation
                    local = sci_library_reconciliation.run_local(artifacts / 'sci-library-reconciliation')
                    sidecar = sci_library_reconciliation.reconcile(local, library_document)
                    sidecar['local_evidence'] = local
                    sidecar['full_evidence'] = library_document
                    (artifacts / 'sci-library-reconciliation' / 'evidence.json').write_text(json.dumps(sidecar, indent=2) + '\n')
                    library_dialect_evidence = sidecar
                if args.reference == 'both':
                    reviewed_sci_context_evidence = reviewed_sci_context_dispatch.run(
                        artifacts / 'reviewed-sci-context', max(240, args.reference_timeout))
                    for proof in reviewed_sci_context_dispatch.reconcile(reviewed_sci_context_evidence).values():
                        if proof['id'] in sci_evidence:
                            raise ValueError('Reviewed SCI context overlaps expression/source/library execution')
                        sci_evidence[proof['id']] = proof['layers']
                    import sci_gap_canonical_context
                    gap_sci_context_evidence = sci_gap_canonical_context.run(
                        artifacts / 'gap-sci-context', max(240, args.reference_timeout))
                    for proof in sci_gap_canonical_context.reconcile(gap_sci_context_evidence).values():
                        if proof['id'] in sci_evidence:
                            raise ValueError('Canonical gap context overlaps another fresh execution adapter')
                        sci_evidence[proof['id']] = proof['layers']
                    import sci_print_context
                    print_sci_context_evidence = sci_print_context.run(
                        artifacts / 'print-sci-context', max(240, args.reference_timeout))
                    for proof in sci_print_context.reconcile(print_sci_context_evidence).values():
                        if proof['id'] in sci_evidence:
                            raise ValueError('Exact printing context overlaps another fresh execution adapter')
                        sci_evidence[proof['id']] = proof['layers']
                    import sci_cljc_accepted_context
                    cljc_sci_context_evidence = sci_cljc_accepted_context.run(
                        artifacts / 'cljc-sci-context', max(240, args.reference_timeout))
                    for proof in sci_cljc_accepted_context.reconcile(cljc_sci_context_evidence).values():
                        if proof['id'] in sci_evidence:
                            raise ValueError('Exact CLJC reader context overlaps another fresh execution adapter')
                        sci_evidence[proof['id']] = proof['layers']
                    import sci_lazy_context
                    lazy_sci_context_evidence = sci_lazy_context.run(
                        artifacts / 'lazy-sci-context', max(240, args.reference_timeout))
                    for proof in sci_lazy_context.reconcile(lazy_sci_context_evidence).values():
                        if proof['id'] in sci_evidence:
                            raise ValueError('Exact lazy/iteration/control context overlaps another fresh execution adapter')
                        sci_evidence[proof['id']] = proof['layers']
                    import case_char_string_accepted
                    case_sci_context_evidence = case_char_string_accepted.run(
                        artifacts / 'case-char-string-context', max(240, args.reference_timeout))
                    for proof in case_char_string_accepted.reconcile(case_sci_context_evidence).values():
                        if proof['id'] in sci_evidence:
                            raise ValueError('Exact character/string case context overlaps another fresh execution adapter')
                        sci_evidence[proof['id']] = proof['layers']
        report = assemble(historical_inventory, historical_witnesses, evidence, **historical_filters, boundary_evidence=boundary_evidence)
        report["scope"] = "historical-selected"
        if args.scope == "intended-language":
            report = expand_report(report, witnesses, evidence, features=args.feature, identities=args.id, phase_evidence=phase_evidence, sci_evidence=sci_evidence, library_dialect_evidence=library_dialect_evidence, reviewed_sci_context_evidence=reviewed_sci_context_evidence, gap_sci_context_evidence=gap_sci_context_evidence, print_sci_context_evidence=print_sci_context_evidence, cljc_sci_context_evidence=cljc_sci_context_evidence, lazy_sci_context_evidence=lazy_sci_context_evidence, case_sci_context_evidence=case_sci_context_evidence)
        report["execution_selection"] = {"fixture_ids": args.fixture, "all_selected_fixtures": not args.fixture}
        if artifacts:
            report["artifacts"] = str(artifacts)
            report["compiler_binary"] = binary_identity
            (artifacts / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        encoded = json.dumps(report, indent=2) + "\n"
        if args.output:
            args.output.write_text(encoded, encoding="utf-8")
        if args.json:
            print(encoded, end="")
        else:
            text_report(report, args.show_missing)
            if artifacts:
                print(f"Artifacts: {artifacts}")
        # Focused implementation runs may reject missing features. A changed
        # value, invalid harness output, timeout or missing reference is a real
        # run failure, even outside strict mode. Only explicitly mapped expected
        # diagnostics can become verified-rejection; they never become passes.
        failure_statuses = {"mismatch", "invalid-output", "invalid-reference", "failed",
                            "unavailable", "reference-unavailable", "timed-out", "timeout",
                            "rejected", "stale-build-input", "unverified"}
        failed = any(layer.get("status") in failure_statuses for result_layers in evidence.values()
                     for layer in result_layers.values())
        failed |= any(layer.get("status") != "verified-boundary" for layers in boundary_evidence.values() for layer in layers.values())
        failed |= any(value.get("status") != "passed" for value in phase_evidence.values())
        failed |= sci_execution_failed(report, sci_evidence, failure_statuses)
        return 1 if failed or (args.strict and not report["strict_complete"]) else 0
    except (ValueError, KeyError, OSError) as error:
        print(f"coverage contract error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
