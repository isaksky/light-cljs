#!/usr/bin/env python3
"""Independent scope, provenance and failed-evidence accounting regressions."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
import extract_inventory as inv
import full_ledger as full
import report


class FullLedgerTests(unittest.TestCase):
    def test_pinned_tree_selection_has_no_selected_file_whitelist(self):
        tree = "src/test/cljs/cljs/new_core_test.cljs\nsrc/test/cljs/cljs/helpers.clj\nsrc/test/cljs/clojure/set_test.cljs\nsrc/test/cljs/cljs/reader_test.cljs\nsrc/main/cljs/ignored.cljs\n"
        with patch.object(full.subprocess, "check_output", return_value=tree):
            paths = full.source_paths()
        self.assertTrue(set(inv.FILES) <= set(paths))
        for name in tree.splitlines()[:-1]:
            self.assertIn(name, paths)
        self.assertNotIn(tree.splitlines()[-1], paths)

    def test_public_names_arities_protocol_methods_and_syntax_come_from_source(self):
        core = b'''(ns cljs.core)
        (defn f ([x] x) ([x y & more] more))
        (defn- hidden [x] x)
        (defn ^:private also-hidden [x] x)
        (def data [1 2])
        (defprotocol P (-p [this] [this x]))
        (defrecord R [x])
        #?(:cljs (defmacro m [x & more] x) :clj (import-macros clojure.core [m]))
        (defn outer [] '(defn phantom [x] x))
        #_(defn discarded [x] x)
        #?@(:cljs [(defn spliced [x] x)])'''
        analyzer = b"(def specials '#{if fn* &})"
        with patch.object(full, "CORE", ("core.cljs",)), patch.object(full, "ANALYZER", "analyzer.cljc"), \
             patch.object(full, "pinned", side_effect=lambda path: analyzer if path == "analyzer.cljc" else core):
            rows, sources = full.public_api()
        names = {r["name"] for r in rows}
        self.assertNotIn("phantom", names)
        self.assertNotIn("discarded", names)
        self.assertIn("spliced", names)
        by_name = lambda name: [r for r in rows if r["name"] == name]
        self.assertEqual([(r["signature"]["kind"], r["signature"]["minimum"]) for r in by_name("f")], [("fixed", 1), ("variadic", 2)])
        self.assertEqual([r["signature"]["minimum"] for r in by_name("-p")], [1, 2])
        self.assertEqual(by_name("data")[0]["signature"]["kind"], "value")
        self.assertEqual(by_name("map->R")[0]["signature"]["minimum"], 1)
        self.assertTrue(all(r["disposition"] == "internal" for r in by_name("hidden") + by_name("also-hidden")))
        self.assertEqual({r["signature"]["kind"] for r in by_name("m")}, {"variadic", "unresolved"})
        self.assertEqual({r["name"] for r in rows if r["obligation_kind"] == "syntax"}, {"if", "fn*", "&"})
        for row in rows:
            self.assertEqual(inv.digest(row["source_span"]["text"].encode()), row["body_sha256"])
        self.assertEqual(len(sources), 2)

    def test_standard_historical_exclusions_become_intended_without_erasing_bodies(self):
        source = b"(ns sample) (def xs [1 2]) (deftest behavior (doseq [x xs] (is (= x (first (persistent! (conj! (transient []) x)))))))"
        path = "src/test/cljs/cljs/new_core_test.cljs"
        with patch.object(inv.subprocess, "check_output", return_value=source):
            generated, manifest = inv.inventory(check_checkout=False, files=(path,), full_scope=True)
        rows = [json.loads(line) for line in generated["inventory.jsonl"].splitlines()]
        bodies = [json.loads(line) for line in generated["bodies.jsonl"].splitlines()]
        self.assertEqual(rows[0]["disposition"], "in-scope")
        self.assertEqual(rows[0]["category"], "transient-collections")
        self.assertIn("doseq", bodies[-1]["source_span"]["text"])
        self.assertEqual(len(bodies), 3)
        for category in full.PROMOTED:
            self.assertEqual(full.disposition_for(path, category, "historical", None)[0], "in-scope")
        self.assertEqual(full.disposition_for(path, "transducers", "discard", "reader-discarded")[0], "unrelated")
        self.assertEqual(full.ownership("-persistent!"), ("transient-collections", "light-cljs-117"))

    def test_committed_full_contract_preserves_history_and_new_required_sources(self):
        rows, obligations, manifest = full.load()
        historical, witnesses, _ = report.load_contract()
        complete, all_witnesses, _ = report.load_contract("intended-language")
        self.assertEqual(len(historical), 3237)
        self.assertEqual(sum(r["disposition"] == "in-scope" for r in historical.values()), 2535)
        paths = {r["path"] for r in manifest["files"]}
        for name in ("hashing_test.cljs", "hash_map_test.cljs", "hash_set_test.cljs", "set_equiv_test.cljs", "invoke_test.cljs", "iterator_test.cljs", "printing_test.cljs", "parse_test.cljs", "walk_test.cljs", "reader_test.cljs", "var_test.cljs", "macro_test.cljs"):
            self.assertTrue(any(p.endswith("/" + name) for p in paths), name)
        self.assertTrue(any(r["obligation_kind"] == "source-program" and r["source"].endswith("invoke_test.cljs") for r in obligations))
        self.assertGreater(len(rows), len(historical))
        self.assertEqual(set(complete), {r["id"] for r in rows})
        for identity, old in historical.items():
            current = complete[identity]
            for key in ("assertion", "body_id", "body_sha256", "context"):
                self.assertEqual(old[key], current[key])
            if identity in full.PRIVATE_EQUALITY_ASSERTIONS:
                self.assertEqual(current["assertion"]["text"], full.PRIVATE_EQUALITY_ASSERTIONS[identity])
                self.assertEqual(current["disposition"], "unrelated")
                self.assertEqual(current["category"], "upstream-internals")
                self.assertNotIn(identity, all_witnesses)
            elif old["category"] in full.PROMOTED and old["active"]:
                self.assertEqual(current["disposition"], "in-scope")
        self.assertTrue(all(r["owner"].startswith("light-cljs-") for r in rows + obligations))
        self.assertEqual(manifest["columns"], list(full.COLUMNS))
        self.assertTrue(all(r["status"] == "static-registration-only" for r in full.local_registration().values()))

    def test_utility_residual_harness_and_compiler_settings_remain_required(self):
        rows, obligations, _ = full.load()
        for body, (digest, owner, family, reason) in full.UTILITY_RESIDUAL_BODIES.items():
            selected = [row for row in rows if row["body_id"] == body]
            self.assertTrue(selected, body)
            for row in selected:
                self.assertEqual(row["disposition"], "in-scope")
                self.assertEqual(row["body_sha256"], digest)
                self.assertEqual((row["owner"], row["family"]), (owner, family))
                self.assertEqual(row["reason"], reason)
        setting = next(row for row in obligations if row["name"] == "*unchecked-arrays*")
        self.assertEqual((setting["disposition"], setting["owner"]), ("intended", "light-cljs-132"))

    def test_all_six_columns_and_no_parser_or_registration_semantic_credit(self):
        columns = full.columns({"parser": {"status": "accepted"}, "evaluation": {"status": "passed"}})
        self.assertEqual(set(columns), set(full.COLUMNS))
        self.assertEqual(columns["parse"]["status"], "accepted")
        self.assertEqual(columns["native"]["status"], "removed")
        for key in ("analysis_lowering", "canonical_js", "quickjs_macro"):
            self.assertEqual(columns[key]["status"], "not-run")
        self.assertNotEqual(columns["execution_evidence"]["status"], "verified")

    def test_subset_and_every_failed_semantic_layer_never_complete_full_scope(self):
        source = {"id": "one", "disposition": "in-scope", "reason": "original", "feature": "core-helpers", "source": "core_test.cljs", "test": "one", "assertion": {"start_line": 1, "sha256": "hash"}, "obligation_kind": "source-assertion", "family": "public-residual", "owner": "light-cljs-125", "body_id": "body", "body_sha256": "body-hash"}
        witness = {"fixture_id": "fixture"}
        historical = {"counts": {key: 0 for key in ("covered", "verified-rejection", "verified-warning", "missing", "version-difference", "unrelated")}, "strict_complete": True, "boundary_strict_complete": True}
        evidence = {"fixture": {layer: {"status": "passed"} for layer in report.SEMANTIC_LAYERS}}
        with patch.object(full, "load", return_value=([source], [], {"counts": {}})), \
             patch.object(full, "load_obligation_witnesses", return_value={}), \
             patch.object(full, "load_phase_witnesses", return_value={}):
            self.assertTrue(full.expand_report(historical, {"one": witness}, evidence)["strict_complete"])
            selected = full.expand_report(historical, {"one": witness}, evidence, identities=("one",))
            self.assertFalse(selected["strict_complete"])
            self.assertFalse(selected["selection"]["full_scope_selected"])
            for layer in report.SEMANTIC_LAYERS:
                for status in ("skipped", "rejected", "timed-out", "unavailable", "not-run", "reference-unavailable"):
                    changed = copy.deepcopy(evidence)
                    changed["fixture"][layer]["status"] = status
                    result = full.expand_report(historical, {"one": witness}, changed)
                    self.assertFalse(result["strict_complete"])
                    self.assertEqual(result["counts"]["covered"], 0)

    def test_public_arity_completion_requires_bound_macro_phase_execution(self):
        source = {"id": "a", "disposition": "in-scope", "reason": "source", "feature": "core-helpers", "source": "core_test.cljs", "test": "a", "assertion": {"start_line": 1, "sha256": "assertion"}, "obligation_kind": "source-assertion", "family": "public-residual", "owner": "light-cljs-125", "body_id": "test-body", "body_sha256": "test-hash"}
        api = {"id": "api", "disposition": "intended", "reason": "public arity", "obligation_kind": "public-api", "source": "core.cljs", "name": "f", "source_span": {}, "family": "public-residual", "owner": "light-cljs-125"}
        historical = {"counts": {key: 0 for key in ("covered", "verified-rejection", "verified-warning", "missing", "version-difference", "unrelated")}, "strict_complete": True, "boundary_strict_complete": True}
        evidence = {"fixture": {layer: {"status": "passed"} for layer in report.SEMANTIC_LAYERS}}
        with patch.object(full, "load", return_value=([source], [api], {"counts": {}})), \
             patch.object(full, "load_obligation_witnesses", return_value={"api": {"assertion_ids": ["a"]}}), \
             patch.object(full, "load_phase_witnesses", return_value={"api": {"test_source_sha256": "phase-source"}}):
            for state in ("not-run", "failed", "timed-out", "unavailable", "passed"):
                result = full.expand_report(historical, {"a": {"fixture_id": "fixture"}}, evidence,
                                            phase_evidence={"api": {"status": state, "test_source_sha256": "phase-source"}})
                self.assertEqual(result["strict_complete"], state == "passed")
            result = full.expand_report(historical, {"a": {"fixture_id": "fixture"}}, evidence,
                                        phase_evidence={"api": {"status": "passed", "test_source_sha256": "stale"}})
            self.assertFalse(result["strict_complete"])

    def test_metadata_consumers_keep_their_public_subsystem_owner(self):
        for filename, owner in [("eval_test.cljs", "light-cljs-124"), ("reader_test.cljs", "light-cljs-9")]:
            self.assertEqual(full.ownership(path=filename, words={"meta"})[1], owner)
        self.assertEqual(full.ownership(path="metadata_test.cljc", words={"meta"})[1], "light-cljs-120")

    def test_local_execution_witness_cannot_pass_without_current_named_execution(self):
        api = {"id": "api", "disposition": "intended", "reason": "public contract", "obligation_kind": "public-api", "source": "core.cljs", "name": "f", "source_span": {}, "family": "hash-equality", "owner": "light-cljs-120"}
        test = {"test_file": "tests/compiler_equality.rs", "test_name": "example", "test_source_sha256": "current"}
        historical = {"counts": {key: 0 for key in ("covered", "verified-rejection", "verified-warning", "missing", "version-difference", "unrelated")}, "strict_complete": True, "boundary_strict_complete": True}
        with patch.object(full, "load", return_value=([], [api], {"counts": {}})), \
             patch.object(full, "load_obligation_witnesses", return_value={"api": {"local_execution": test}}), \
             patch.object(full, "load_phase_witnesses", return_value={"api": test}):
            for state, digest, expected in [("not-run", "current", False), ("failed", "current", False), ("passed", "stale", False), ("passed", "current", True)]:
                result = full.expand_report(historical, {}, {}, phase_evidence={"api": {"status": state, "test_source_sha256": digest}})
                self.assertEqual(result["strict_complete"], expected)
                columns = result["assertions"][0]["columns"]
                self.assertEqual(columns["execution_evidence"]["references"],
                                 {"release": {"status": "not-run"}, "checkout": {"status": "not-run"}})

    def test_arity_witness_rejects_hash_drift_unknown_arity_and_selected_body_subset(self):
        target = {"id": "api::map", "disposition": "intended", "body_sha256": "body", "signature": {"kind": "fixed", "minimum": 2}}
        sources = [{"id": "a", "disposition": "in-scope", "body_id": "full-body"}, {"id": "b", "disposition": "in-scope", "body_id": "full-body"}]
        mapping = {"obligation_id": target["id"], "body_sha256": "body", "signature_sha256": inv.digest(json.dumps(target["signature"], sort_keys=True).encode()), "assertion_ids": ["a", "b"], "rationale": "Original full body invokes this public arity and asserts values/effects."}
        with tempfile.TemporaryDirectory() as temp, patch.object(full, "HERE", Path(temp)):
            path = Path(temp) / "obligation-witnesses.jsonl"
            path.write_text(json.dumps(mapping) + "\n")
            self.assertIn(target["id"], full.load_obligation_witnesses([target], sources, {"a": {}, "b": {}}))
            for change in ({"body_sha256": "changed"}, {"signature_sha256": "wrong"}, {"assertion_ids": ["a"]}, {"rationale": ""}, {"assertion_ids": []}):
                path.write_text(json.dumps({**mapping, **change}) + "\n")
                with self.assertRaises(ValueError):
                    full.load_obligation_witnesses([target], sources, {"a": {}, "b": {}})

    def test_macro_phase_execution_rejects_zero_tests_skips_and_failed_runs(self):
        item = {"test_file": "tests/compiler_macros.rs", "test_name": "example", "test_source_sha256": "complete-source"}
        valid = "running 1 test\ntest example ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.01s\n"
        with tempfile.TemporaryDirectory() as temp, patch.object(full, "load_phase_witnesses", return_value={"api": item}):
            for observed, expected in (({"status": "completed", "stdout": valid}, "passed"),
                                       ({"status": "completed", "stdout": valid.replace("test example ... ok", "test other ... ok")}, "invalid-output"),
                                       ({"status": "completed", "stdout": "running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored;"}, "invalid-output"),
                                       ({"status": "completed", "stdout": valid.replace("0 ignored", "1 ignored")}, "invalid-output"),
                                       ({"status": "timed-out"}, "timed-out"), ({"status": "unavailable"}, "unavailable"), ({"status": "failed"}, "failed")):
                with patch.object(report, "invoke", return_value=observed) as call:
                    result = full.run_phase_witnesses([], Path(temp), 1)
                    self.assertEqual(result["api"]["status"], expected)
                    self.assertEqual(call.call_args.args[0], ["cargo", "test", "--locked", "--test", "compiler_macros", "--", "--exact", "example", "--nocapture"])

    def test_macro_phase_source_and_exact_public_arity_are_pinned(self):
        target = {"id": "api", "disposition": "intended", "body_sha256": "core-body", "signature": {"kind": "fixed", "minimum": 1}}
        source = b"fn helper() {}\n#[test]\nfn example() { helper(); }\n"
        item = {"obligation_id": "api", "body_sha256": "core-body", "signature_sha256": inv.digest(json.dumps(target["signature"], sort_keys=True).encode()),
                "test_file": "tests/compiler_macros.rs", "test_name": "example", "test_source_sha256": inv.digest(source), "rationale": "Complete macro call setup and original operation arity retained."}
        with tempfile.TemporaryDirectory() as temp, patch.object(full, "HERE", Path(temp)), patch.object(full, "ROOT", Path(temp)):
            root = Path(temp); (root / "tests").mkdir(); (root / item["test_file"]).write_bytes(source)
            path = root / "phase-witnesses.jsonl"
            path.write_text(json.dumps(item) + "\n")
            self.assertIn("api", full.load_phase_witnesses([target]))
            for change in ({"body_sha256": "stale"}, {"signature_sha256": "stale"}, {"test_source_sha256": "stale"}, {"test_name": "absent"}, {"test_file": "../outside.rs"}, {"rationale": ""}):
                path.write_text(json.dumps({**item, **change}) + "\n")
                with self.assertRaises(ValueError):
                    full.load_phase_witnesses([target])

    def test_default_strict_cli_reports_gaps_historical_scope_retains_exact_counts(self):
        for scope in ("intended-language", "historical-selected"):
            result = subprocess.run([sys.executable, "-B", str(full.HERE / "report.py"), "--scope", scope, "--strict", "--json"], capture_output=True, text=True)
            self.assertEqual(result.returncode, 1, result.stderr)
            data = json.loads(result.stdout)
            self.assertEqual(data["scope"], scope)
            self.assertGreater(data["counts"]["missing"], 0)
            self.assertEqual(data["counts"]["covered"], 0)
            if scope == "intended-language":
                self.assertGreater(data["counts"]["missing"], 2535)
                self.assertEqual(data["historical_scope"]["counts"]["missing"], 2535)
                self.assertTrue(all(set(row["columns"]) == set(full.COLUMNS) for row in data["assertions"]))
            else:
                self.assertEqual(data["counts"]["missing"], 2535)
                self.assertEqual(data["counts"]["unrelated"], 702)


if __name__ == "__main__":
    unittest.main()
