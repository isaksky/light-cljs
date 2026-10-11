#!/usr/bin/env python3
"""Check literal original-source receipts without changing SCI expectations.

An explicit version difference records two different upstream contracts. It
does not make either mismatch against the immutable SCI expectation a pass.
"""
from __future__ import annotations
import argparse
import collections
import json
from pathlib import Path
import sci_execution as sci
import sci_source_context as context

RECEIPTS = context.HERE / "sci-source-context-receipts.jsonl"
SETTINGS = {"optimizations": "none", "static-fns": False, "def-emits-var": False}


def source_binding(row):
    return {key: row[key] for key in ("id", "sci_id", "classification", "source",
        "source_sha256", "original_source_sha256", "original_row_sha256",
        "body_sha256", "obligation_id", "expected", "namespace", "source_name",
        "forms", "reference_setup", "setup", "comparison")}


def checked_references(row, layers):
    runner = sci.report_module()
    for kind, identity in (("release", runner.RELEASE), ("checkout", runner.REVISION)):
        layer = layers[kind]
        if (layer.get("reference_identity") != kind + " " + identity or
            not runner.same_json(layer.get("settings"), SETTINGS) or
            layer.get("source_sha256") != row["source_sha256"] or
            layer.get("oracle_source_sha256") != sci.digest((sci.ROOT / context.ORACLE).read_bytes()) or
            layer.get("oracle_helper_source_sha256") != sci.digest((sci.ROOT / "tests/reference/execution.clj").read_bytes()) or
            layer.get("observation_source_sha256") != sci.digest((sci.ROOT / "tests/support/execution.js").read_bytes())):
            raise ValueError("Unverified reference identity, source or settings: " + row["id"])


def classification(row, layers):
    if set(layers) != {"javascript", "release", "checkout"}:
        raise ValueError("Incomplete or extra source-context layers: " + row["id"])
    checked_references(row, layers)
    runner = sci.report_module()
    javascript = layers["javascript"]
    if (javascript.get("status") != "passed" or javascript.get("source_sha256") != row["source_sha256"] or
        javascript.get("test_file") != context.DRIVER or javascript.get("test_name") != context.TEST or
        javascript.get("test_source_sha256") != sci.digest((sci.ROOT / context.DRIVER).read_bytes()) or
        javascript.get("adapter_source_sha256") != sci.digest((sci.ROOT / "tests/support/sci_execution_observation.js").read_bytes())):
        raise ValueError("Original local source not verified: " + row["id"])
    if set(javascript.get("engines", {})) != {"Node", "QuickJS"}:
        raise ValueError("Incomplete or extra local engines: " + row["id"])
    for kind in ("Node", "QuickJS"):
        engine = javascript["engines"][kind]
        if engine.get("status") != "passed" or sci.verify(row,
            {"status": "completed", "observed": engine.get("observed")}, runner.same_json)["status"] != "passed":
            raise ValueError("Original local expectation changed: " + row["id"])
    for kind in ("release", "checkout"):
        layer = layers[kind]
        if layer["status"] in ("passed", "mismatch"):
            if layer.get("reference_phase_effects") != ["[]"]:
                raise ValueError("Unreconciled compiler phase effects: " + row["id"])
            verified = sci.verify(row, {"status": "completed", "observed": layer.get("observed")}, runner.same_json)
            if layer["status"] != verified["status"]:
                raise ValueError("Original reference comparison changed: " + row["id"])
        elif layer["status"] != "rejected":
            raise ValueError("Incomplete reference execution: " + row["id"])
    if all(layers[kind]["status"] == "passed" for kind in ("release", "checkout")):
        return "original-source-agreement"
    if row["classification"] == "pinned-version-difference":
        release, checkout = layers["release"], layers["checkout"]
        if (release["status"] not in ("passed", "mismatch") or
            checkout["status"] not in ("passed", "mismatch") or
            runner.same_json(release["observed"], checkout["observed"])):
            raise ValueError("Version difference lacks distinct executed raw contracts: " + row["id"])
        matching_pins = [kind for kind in ("release", "checkout")
                         if all(runner.same_json(sci.projection(engine["observed"]),
                                                 sci.projection(layers[kind]["observed"]))
                                for engine in javascript["engines"].values())]
        if not matching_pins:
            raise ValueError("Original local contract matches neither reference pin: " + row["id"])
        return "verified-pinned-version-difference"
    return "original-contract-unresolved"


def full_report_evidence(row, layers):
    """Classify freshly supplied full-run evidence; stored receipts grant no pass.

    Both local engines and both pinned source/oracle identities are rechecked.
    A verified pin difference still leaves the original SCI agreement unresolved.
    """
    try:
        reconciliation = classification(row, layers)
    except (ValueError, KeyError, TypeError) as error:
        return {"status": "missing", "reconciliation": "unverified",
                "original_expectation_status": "unresolved", "reason": str(error)}
    status = {"original-source-agreement": "covered",
              "verified-pinned-version-difference": "version-difference",
              "original-contract-unresolved": "missing"}[reconciliation]
    return {"status": status, "reconciliation": reconciliation,
            "original_expectation_status": "passed" if status == "covered" else "unresolved",
            "reason": "Fresh exact original-source Node/QuickJS and pinned reference receipts checked."}


def load():
    receipts = [json.loads(line) for line in RECEIPTS.read_text().splitlines() if line]
    rows = context.load()
    if [row["id"] for row in receipts] != [row["id"] for row in rows]:
        raise ValueError("Missing, duplicate or reordered source-context receipts")
    for row, receipt in zip(rows, receipts):
        if receipt.get("schema_version") != 1 or receipt.get("catalog_sha256") != sci.digest(context.CATALOG.read_bytes()):
            raise ValueError("Receipt schema or catalog changed: " + row["id"])
        if json.dumps(receipt["source_binding"], sort_keys=True) != json.dumps(source_binding(row), sort_keys=True):
            raise ValueError("Original source/expectation receipt changed: " + row["id"])
        if receipt["reconciliation"] != classification(row, receipt["layers"]):
            raise ValueError("Receipt reconciliation changed: " + row["id"])
        if receipt.get("original_expectation_status") != ("passed" if receipt["reconciliation"] == "original-source-agreement" else "unresolved"):
            raise ValueError("Nonagreement promoted to original SCI pass: " + row["id"])
    return receipts


def verify(evidence):
    receipts = load()
    if list(evidence) != [receipt["id"] for receipt in receipts]:
        raise ValueError("Fresh evidence identities changed")
    results = {}
    rows = {row["id"]: row for row in context.load()}
    for receipt in receipts:
        identity = receipt["id"]
        fresh = evidence[identity]
        classification(rows[identity], fresh)
        # Literal outcome, callback order, stdout, rejection and phase receipts
        # remain independent of the original SCI comparison projection.
        for kind in ("release", "checkout"):
            for key in ("status", "observed", "detail", "reference_phase_effects"):
                if json.dumps(fresh[kind].get(key), sort_keys=True) != json.dumps(receipt["layers"][kind].get(key), sort_keys=True):
                    raise ValueError("Pinned raw contract changed: " + identity + "/" + kind + "/" + key)
        results[identity] = receipt["reconciliation"]
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    results = (verify(json.loads(args.evidence.read_text())["evidence"]) if args.evidence
               else {row["id"]: row["reconciliation"] for row in load()})
    print(dict(collections.Counter(results.values())))


if __name__ == "__main__":
    main()
