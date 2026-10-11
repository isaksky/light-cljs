#!/usr/bin/env python3
"""Reproduce complete promised library bodies; static generation establishes no pass.

Upstream Copyright (c) Rich Hickey; Eclipse Public License 1.0. Original notices
remain in research/clojurescript, including epl-v10.html, and must be retained.
"""
from pathlib import Path
import hashlib
import importlib.util
import json
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / "tests/reference/compiler-language"
sys.path.insert(0, str(HERE))
import extract_inventory as inv
spec = importlib.util.spec_from_file_location("sequence_adapter", ROOT / "tests/reference/compiler-sequence-completion.py")
adapter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adapter)
BASE_ADAPT = adapter.adapt
PREFIX = "cljs-library-body-"
SOURCES = {"src/test/cljs/clojure/set_test.cljs", "src/test/cljs/clojure/string_test.cljs",
           "src/test/cljs/clojure/walk_test.cljs", "src/test/cljs/cljs/walk_test.cljs"}
RATIONALE = ("Retain the complete pinned deftest body, helpers/records, every original expected value/message, "
             "setup, iteration, metadata and ordered effect path. is/are become assert and testing becomes do; "
             "library alias symbols are fully qualified to their original namespace. The whole body returns true only "
             "after all original assertions execute. Pinned bytes and complete body/setup hashes are independently checked.")
META_BODY = "src/test/cljs/cljs/walk_test.cljs::retain-meta"

def retain_meta_paths(source):
    ordinal = iter(range(1, 7))
    def rewrite(node):
        if inv.short_head(node) == "assert":
            return "(library-check " + json.dumps("assertion-" + str(next(ordinal)).zfill(3)) + " " + source[node.children[1].start:node.children[1].end] + ")"
        parts, offset = [], node.start
        for child in node.children:
            parts.extend([source[offset:child.start], rewrite(child)])
            offset = child.end
        return "".join(parts) + source[offset:node.end]
    result = rewrite(inv.Reader(source).all()[0])
    assert next(ordinal, None) is None, "All six original metadata paths must be retained"
    return "(let [library-failures (atom []) library-check (fn [id check] (when-not check (swap! library-failures conj id)))] " + result + " (if (empty? @library-failures) true @library-failures))"

def adapt(text, node):
    if not node.children:
        atom = text[node.start:node.end]
        if node.kind == "atom":
            for alias, namespace in (("set/", "clojure.set/"), ("s/", "clojure.string/"), ("w/", "clojure.walk/")):
                if atom.startswith(alias): return namespace + atom[len(alias):]
        return atom
    return BASE_ADAPT(text, node)
adapter.adapt = adapt

def records():
    bodies = adapter.read("full-bodies.jsonl")
    cases, witnesses, rows = [], [], []
    by_body = {}
    for body in bodies:
        if body["source"] not in SOURCES or body["form"] != "deftest": continue
        # Preserve complete setup each original body actually references.
        needed = set()
        if body["source"].endswith("clojure/set_test.cljs") and body["name"] in {"test-project", "test-rename", "test-join"}:
            needed = {"compositions"}
        elif body["source"].endswith("cljs/walk_test.cljs"):
            if body["name"] == "test-walk": needed = {"Foo", "get-comparator", "get-comparator~2", "get-comparator~3"}
            elif body["name"] == "retain-meta": needed = {"RM"}
        elif body["source"].endswith("clojure/walk_test.cljs") and body["name"] == "test-api":
            needed = {"Rec1", "inc-leaf"}
        setups = [b for b in bodies if b["source"] == body["source"] and b["id"].rsplit("::", 1)[-1] in needed]
        forms, provenance = [], []
        pinned = subprocess.check_output(["git", "-C", str(ROOT / "research/clojurescript"), "show", inv.REVISION + ":" + body["source"]])
        for original in setups + [body]:
            span = original["source_span"]; text = span["text"]
            assert hashlib.sha256(text.encode()).hexdigest() == span["sha256"]
            assert pinned[span["start_byte"]:span["end_byte"]].decode() == text
            forms.append(adapt(text, inv.Reader(text).all()[0]))
            provenance.append(original["id"] + ";sha256=" + span["sha256"])
        source = "(do " + " ".join(forms) + ")"
        if body["id"] == META_BODY:
            source = retain_meta_paths(source)
        source = source.replace("\n", " ").replace("\t", " ")
        identity = PREFIX + body["id"].replace("/", "-").replace(".cljs::", "-").replace("_", "-").replace("?", "-p")
        rows.append([identity, source, "true", "|".join(provenance)])
        by_body[body["id"]] = identity
        entry = "librarybody.case" + str(len(rows) - 1)
        root = "tests/fixtures/compiler-library-bodies"
        namespace = "clojure.set" if "set_test" in body["source"] else "clojure.string" if "string_test" in body["source"] else "clojure.walk"
        program = (pinned.decode().split("(ns ")[0]
                   + "(ns " + entry + " (:require [" + namespace + "]))\n"
                   + "(defn run [] " + source + ")\n")
        path = ROOT / root / (entry.replace(".", "/") + ".cljs")
        if "--write" in sys.argv:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(program)
        else:
            assert path.read_text() == program
        cases.append({"id": identity, "source": program, "source_root": root, "entry": entry,
                      "export": "run", "expected": {"id": identity, "outcome": {"status": "returned", "value": True}, "trace": [], "stdout": ""}, "native_stdout": None})
    existing = {w["assertion_id"] for name in ("witnesses.jsonl", "full-witnesses.jsonl") for w in adapter.read(name) if not w["fixture_id"].startswith(PREFIX)}
    for row in adapter.read("full-inventory.jsonl"):
        if row["body_id"] in by_body and row["disposition"] == "in-scope" and row["id"] not in existing:
            witness = {"assertion_id": row["id"], "assertion_sha256": row["assertion"]["sha256"], "fixture_id": by_body[row["body_id"]], "rationale": RATIONALE}
            if row["body_id"] == META_BODY:
                identity = witness["fixture_id"]
                common = {"id": identity, "trace": [], "stdout": ""}
                witness["rationale"] += " This body accumulates every failed original assertion ID without stopping subsequent paths."
                witness["version_difference"] = {"reason": "Release 1.12.134 loses postwalk metadata for original list (001) and lazy sequence (005) inputs. The complete six-path body returns precisely those failed IDs. Checkout a5e94c9 and local engines preserve every path and return true.",
                    "release": {**common, "outcome": {"status": "returned", "value": {"$vector": ["assertion-001", "assertion-005"]}}},
                    "checkout": {**common, "outcome": {"status": "returned", "value": True}}}
            witnesses.append(witness)
    linked = {w["fixture_id"] for w in witnesses}
    return [case for case in cases if case["id"] in linked], witnesses, rows

def main():
    cases, witnesses, rows = records()
    fixture = ROOT / "tests/fixtures/compiler-library-bodies.tsv"
    text = "# Copyright (c) Rich Hickey. Eclipse Public License 1.0.\n# Retain this notice; see research/clojurescript/epl-v10.html.\n# id\tsource\texpected\tcomplete body/setup provenance\n" + "".join("\t".join(row) + "\n" for row in rows)
    if "--write" in sys.argv: fixture.write_text(text)
    else: assert fixture.read_text() == text
    for name, expected, key in (("full-cases.jsonl", cases, "id"), ("full-witnesses.jsonl", witnesses, "fixture_id")):
        actual = adapter.read(name)
        if "--write" in sys.argv:
            (HERE / name).write_text("".join(json.dumps(x, separators=(",", ":")) + "\n" for x in actual if not x[key].startswith(PREFIX)) + "".join(json.dumps(x, separators=(",", ":")) + "\n" for x in expected))
        else: assert [x for x in actual if x[key].startswith(PREFIX)] == expected
    obligations, phases, mappings = adapter.read("obligations.jsonl"), [], []
    inventory = adapter.read("full-inventory.jsonl")
    for target in obligations:
        if target["disposition"] != "intended": continue
        body_id = None
        if target["source"] == "src/main/cljs/clojure/set.cljs":
            # The complete upstream test-join body exercises only [xrel yrel].
            # A name occurrence cannot prove the keyed [xrel yrel km] arity.
            if target["name"] == "join" and target["signature"]["minimum"] == 3:
                continue
            body_id = "src/test/cljs/clojure/set_test.cljs::test-" + target["name"]
        elif target["source"] == "src/main/cljs/clojure/string.cljs":
            body_id = "src/test/cljs/clojure/string_test.cljs::test-api"
        elif target["source"] == "src/main/cljs/clojure/walk.cljs":
            body_id = {"walk": "src/test/cljs/cljs/walk_test.cljs::test-walk",
                       "prewalk": "src/test/cljs/clojure/walk_test.cljs::test-api",
                       "postwalk": "src/test/cljs/clojure/walk_test.cljs::test-api",
                       "stringify-keys": "src/test/cljs/cljs/walk_test.cljs::t-stringify-keys",
                       "prewalk-replace": "src/test/cljs/cljs/walk_test.cljs::t-prewalk-replace",
                       "postwalk-replace": "src/test/cljs/cljs/walk_test.cljs::t-postwalk-replace"}.get(target["name"])
        if not body_id: continue
        assertion_ids = [r["id"] for r in inventory if r["body_id"] == body_id and r["disposition"] == "in-scope"]
        assert assertion_ids
        rationale = ("The complete pinned " + body_id + " body executes the exact public " + target["name"]
                     + " source arity " + json.dumps(target["signature"], sort_keys=True)
                     + " with unchanged inputs/expected values, complete setup and all original assertion paths. "
                     + "Its canonical application executes both checked pins; its complete adapter independently executes Node/QuickJS applications and macro computations. "
                     + "Original literal assertions retain reduction, metadata, collection type, regex, ordering and effect distinctions; no registration/name occurrence is execution evidence.")
        common = {"obligation_id": target["id"], "body_sha256": target["body_sha256"],
                  "signature_sha256": inv.digest(json.dumps(target["signature"], sort_keys=True).encode()), "rationale": rationale}
        mappings.append({**common, "assertion_ids": assertion_ids})
        phases.append({**common, "test_file": "tests/compiler_library_bodies.rs", "test_name": "complete_promised_library_bodies_execute_in_both_phases",
                       "test_source_sha256": inv.digest((ROOT / "tests/compiler_library_bodies.rs").read_bytes()),
                       "support_source_sha256": {str(fixture.relative_to(ROOT)): inv.digest(text.encode())}})
    for name, generated in (("obligation-witnesses.jsonl", mappings), ("phase-witnesses.jsonl", phases)):
        old = adapter.read(name)
        def owned(x):
            return x.get("test_file") == "tests/compiler_library_bodies.rs" or x.get("rationale", "").startswith("The complete pinned src/test/cljs/") and "complete adapter independently executes Node/QuickJS applications and macro computations" in x["rationale"]
        existing = {x["obligation_id"] for x in old if not owned(x)}
        replacements = {x["obligation_id"]: x for x in generated if x["obligation_id"] not in existing}
        result = []
        for row in old:
            if not owned(row): result.append(row)
            elif row["obligation_id"] in replacements: result.append(replacements.pop(row["obligation_id"]))
        result.extend(replacements.values())
        output = "".join(json.dumps(x, separators=(",", ":")) + "\n" for x in result)
        if "--write" in sys.argv: (HERE / name).write_text(output)
        else: assert (HERE / name).read_text() == output, name + " library mappings drift"
    print(f"{len(rows)} complete promised library bodies; {len(witnesses)} retained assertion mappings (unexecuted until runner)")
if __name__ == "__main__": main()
