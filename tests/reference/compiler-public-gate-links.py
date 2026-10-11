#!/usr/bin/env python3
"""Bind existing public application/macro contracts to exact pinned oracles.

The reviewed table selects complete literal fixtures, never registrations or
constructor spellings alone. Existing test/body/signature identities stay intact.
Generated catalogs are evidence bindings; only the report's current executions
can establish passes. --propose writes separate additions for shared-catalog
integration, while --write applies precisely those additions after review.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / "tests/reference/compiler-language"
sys.path.insert(0, str(HERE))
import full_ledger
import report

PREFIX = "cljs-public-gate-"
TESTS = {
    "protocols": "tests/compiler_public_protocols.rs",
    "constructors": "tests/compiler_public_constructors.rs",
    "factories": "tests/compiler_core_factories.rs",
    "iterators": "tests/compiler_iterator_helpers.rs",
}
# Each selected constructor fixture explicitly supplies every positional slot,
# invokes direct/new construction and saved/apply factory calls, and checks a
# literal ordinary result plus nominal identity. The protocol fixture executes
# both saved methods at their exact arities and every retained marker/predicate.
PROTOCOL_NAMES = set("ASeq IDerefWithTimeout -deref-with-timeout Inst inst-ms* inst-ms inst? IUUID uuid?".split())
CONSTRUCTOR_NAMES = set("Cons Reduced Symbol List EmptyList Keyword Cycle Repeat Iterate PersistentVector Subvec ChunkedSeq PersistentQueue PersistentArrayMap PersistentHashMap PersistentHashSet".split())
FACTORY_NAMES = set("IndexedSeq UUID Range IntegerRange PersistentTreeMap PersistentTreeSet PersistentTreeMapSeq Eduction MultiFn SeqIter RangedIterator Namespace TaggedLiteral".split())


def digest(value):
    return hashlib.sha256(value.encode() if isinstance(value, str) else value).hexdigest()


def read(name):
    return report.jsonl(HERE / name)


def fixture_rows(path):
    result = {}
    for line in (ROOT / path).read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        identity, source, expected = line.split("\t")
        assert expected == "true", (path, identity, expected)
        assert identity not in result
        result[identity] = (source, digest(line))
    return result


def oracle_contracts():
    rows, selections = {}, {}
    for group, path in (
        ("protocols", "tests/fixtures/compiler-public-protocols.tsv"),
        ("constructors", "tests/fixtures/compiler-public-constructors.tsv"),
        ("factories", "tests/fixtures/compiler-core-factories.tsv"),
    ):
        rows[group] = (path, fixture_rows(path))
    selections["protocols"] = {name: "public-protocol-types-saved" for name in PROTOCOL_NAMES}
    selections["constructors"] = {}
    for name in CONSTRUCTOR_NAMES:
        identity = ("public-saved-applied-factories" if name in {"Cons", "Reduced", "Symbol"}
                    else "public-" + name.lower() + "-construction")
        for spelling in (name, "->" + name):
            selections["constructors"][spelling] = identity
    selections["factories"] = {}
    for name in FACTORY_NAMES:
        for spelling in (name, "->" + name):
            selections["factories"][spelling] = "core-factory-" + name.lower()
    path = "tests/fixtures/compiler-iterator-helpers.cljs"
    original = (ROOT / path).read_text()
    # Remove only semicolon-only comments, preserving every executable form,
    # metadata assertion, loop, lazy effect and boundary check in the contract.
    source = "(do " + " ".join(line for line in original.splitlines()
                               if not line.lstrip().startswith(";")) + " (iterator-helper-contract))"
    rows["iterators"] = (path, {"iterator-helper-contract": (source, digest(original))})
    selections["iterators"] = {name: "iterator-helper-contract" for name in ("seq-iter", "ranged-iterator")}
    return rows, selections


def attach_reference(mapping):
    """Preserve this oracle overlay when a family generator regenerates a row.

    Family generators can call this on their generated ordinary mappings. The
    exact local execution test remains their responsibility; this function
    changes only the three source/literal reference identity fields.
    """
    group = next((key for key, path in TESTS.items()
                  if path == mapping.get("local_execution", {}).get("test_file")), None)
    if group is None:
        return mapping
    target = next(row for row in read("obligations.jsonl") if row["id"] == mapping["obligation_id"])
    rows, selections = oracle_contracts()
    path, sources = rows[group]
    source, _ = sources[selections[group][target["name"]]]
    fixture_id = PREFIX + selections[group][target["name"]]
    expected = {"id": fixture_id, "outcome": {"status": "returned", "value": True},
                "trace": [], "stdout": ""}
    return {**mapping, "reference_fixture_id": fixture_id,
            "reference_source_sha256": digest(source),
            "reference_contract_sha256": digest(json.dumps(expected, sort_keys=True))}


def generated_links():
    obligations = {row["id"]: row for row in read("obligations.jsonl")}
    phases = full_ledger.load_phase_witnesses(list(obligations.values()))
    rows, selections = oracle_contracts()
    groups = {path: group for group, path in TESTS.items()}
    cases, updates, receipts = {}, [], []
    for mapping in read("obligation-witnesses.jsonl"):
        local = mapping.get("local_execution", {})
        group = groups.get(local.get("test_file"))
        if not group:
            continue
        target = obligations[mapping["obligation_id"]]
        phase = phases[target["id"]]
        assert target["disposition"] == "intended"
        assert mapping["body_sha256"] == target["body_sha256"]
        assert local == {key: phase[key] for key in ("test_file", "test_name", "test_source_sha256")}
        assert digest((ROOT / local["test_file"]).read_bytes()) == local["test_source_sha256"]
        signature = target.get("signature")
        if signature:
            assert signature["kind"] != "unresolved"
            assert mapping["signature_sha256"] == digest(json.dumps(signature, sort_keys=True))
            span = target["source_span"]
            assert full_ledger.pinned(target["source"])[span["start_byte"]:span["end_byte"]].decode() == span["text"]
        identity = selections[group][target["name"]]
        path, sources = rows[group]
        source, row_hash = sources[identity]
        assert digest((ROOT / path).read_bytes()) == phase["support_source_sha256"][path]
        fixture_id = PREFIX + identity
        case = {"id": fixture_id, "source": source,
                "expected": {"id": fixture_id, "outcome": {"status": "returned", "value": True},
                             "trace": [], "stdout": ""}, "native_stdout": "true\n"}
        assert fixture_id not in cases or cases[fixture_id] == case
        cases[fixture_id] = case
        fields = {"reference_fixture_id": fixture_id,
                  "reference_source_sha256": digest(source),
                  "reference_contract_sha256": digest(json.dumps(case["expected"], sort_keys=True))}
        # Add exact oracle identity without rewriting the retained local rationale
        # or pretending adjacent whole-body source tests call a constructor.
        updated = attach_reference(mapping)
        assert updated == {**mapping, **fields}
        updates.append(updated)
        receipts.append({"obligation_id": target["id"], "body_sha256": target["body_sha256"],
                         "signature": signature, "fixture_path": path, "fixture_row_sha256": row_hash,
                         **fields, "local_execution": local,
                         "rationale": "Complete existing literal application/macro fixture; retain every setup, direct/saved/apply call, positional argument, assertion, metadata/identity observation, iteration and effect. The separate complete upstream body witnesses remain unchanged and are not counted as constructor calls."})
    assert len(updates) == 131, len(updates)
    assert len(cases) == 29, len(cases)
    return updates, list(cases.values()), receipts


def text(rows):
    return "".join(json.dumps(row, separators=(",", ":")) + "\n" for row in rows)


def replace_owned(existing, generated, belongs):
    result, inserted = [], False
    for row in existing:
        if belongs(row):
            if not inserted:
                result.extend(generated)
                inserted = True
        else:
            result.append(row)
    if not inserted:
        result.extend(generated)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--propose", type=Path, help="write additions without changing shared catalogs")
    parser.add_argument("--write", action="store_true", help="apply reviewed catalog additions")
    parser.add_argument("--check", action="store_true", help="verify installed catalog additions")
    parser.add_argument("--run", action="store_true", help="execute exact Node/QuickJS and both checked pins")
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/light-cljs")
    parser.add_argument("--artifacts", type=Path)
    args = parser.parse_args()
    updates, cases, receipts = generated_links()
    if args.propose:
        args.propose.mkdir(parents=True, exist_ok=True)
        for name, value in (("obligation-updates.jsonl", updates), ("case-additions.jsonl", cases),
                            ("public-gate-link-receipts.jsonl", receipts)):
            (args.propose / name).write_text(text(value))
    if args.write or args.check:
        by_update = {row["obligation_id"]: row for row in updates}
        expected = [by_update.get(row["obligation_id"], row) for row in read("obligation-witnesses.jsonl")]
        current_cases = read("full-cases.jsonl")
        for name, value in (("obligation-witnesses.jsonl", expected), ("full-cases.jsonl", replace_owned(current_cases, cases, lambda row: row["id"].startswith(PREFIX))),
                            ("public-gate-link-receipts.jsonl", receipts)):
            path = HERE / name
            if args.write:
                path.write_text(text(value))
            elif name == "full-cases.jsonl":
                assert [row for row in current_cases if row["id"].startswith(PREFIX)] == cases, name + " public oracle link drift"
            else:
                assert path.read_text() == text(value), name + " public oracle link drift"
    print(json.dumps({"public_local_rows": len(updates), "literal_oracles": len(cases)}), flush=True)
    if args.run:
        assert args.artifacts, "--run requires a fresh --artifacts directory"
        args.artifacts.mkdir(parents=True, exist_ok=False)
        local = {case["id"]: report.local_layers(case, str(args.binary.resolve()),
                  args.artifacts / "local" / case["id"], 120) for case in cases}
        (args.artifacts / "local.json").write_text(json.dumps(local, indent=2) + "\n")
        print("local", json.dumps({key: row["javascript"]["status"] for key, row in local.items()}), flush=True)
        failures = {"local:" + key: row for key, row in local.items()
                    if row["javascript"]["status"] != "passed" or row["evaluation"]["status"] != "passed"}
        for kind in ("release", "checkout"):
            observed = report.reference_cases(cases, kind, ROOT / "research/clojurescript", args.artifacts / kind, 300)
            (args.artifacts / (kind + ".json")).write_text(json.dumps(observed, indent=2) + "\n")
            print(kind, json.dumps({key: row["status"] for key, row in observed.items()}), flush=True)
            failures.update({kind + ":" + key: row for key, row in observed.items() if row["status"] != "passed"})
        assert not failures, failures


if __name__ == "__main__":
    main()
