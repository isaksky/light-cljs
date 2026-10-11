#!/usr/bin/env python3
"""Source identity and execution failures must not grant SCI coverage."""
import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

import sci_execution as sci


class SCIExecutionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.catalog = sci.load()
        cls.case = sci.cases(cls.catalog[:1])[0]
        cls.runner = sci.report_module()

    def test_catalog_preserves_original_source_and_independent_row_hashes(self):
        corpus, dispositions = sci.rows(sci.CORPUS), sci.rows(sci.DISPOSITIONS)
        self.assertGreater(len(self.catalog), 1000)
        for row, case in zip(self.catalog, sci.cases(self.catalog)):
            self.assertEqual(row["original_row_sha256"], sci.digest(corpus[row["sci_id"]][0]))
            self.assertEqual(row["body_sha256"], sci.digest(dispositions[row["sci_id"]][0]))
            self.assertEqual(case["source"], corpus[row["sci_id"]][1][2])
            self.assertEqual(row["source_sha256"], sci.digest(case["source"]))
        isolated = [case for case in sci.cases(self.catalog) if case["isolated"]]
        self.assertEqual({case["id"] for case in isolated}, {
            "sci-execution-sci-record-constructors", "sci-execution-sci-record-empty",
            "sci-execution-sci-record-method-acceptance"})

    def test_stale_catalog_identity_and_expectation_fail_closed(self):
        for key in ("original_source_sha256", "body_sha256", "expected"):
            changed = copy.deepcopy(self.catalog)
            changed[0][key] = "changed"
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "catalog.jsonl"
                path.write_text("".join(json.dumps(row) + "\n" for row in changed))
                with mock.patch.object(sci, "CATALOG", path):
                    with self.assertRaisesRegex(ValueError, "stale or incomplete"):
                        sci.load()

    def test_canonical_nonpositive_contract_retains_exact_historical_expectation(self):
        mapping = next(row for row in self.catalog if row["sci_id"] == "sci-reader-anonymous-nonpositive")
        original = copy.deepcopy(mapping)
        contract = sci.canonical_contract(mapping)
        self.assertEqual(mapping, original)
        self.assertEqual(contract["historical_expected"]["outcome"]["value"], {"$vector": [0, 2]})
        self.assertEqual(contract["canonical_expected"]["outcome"]["value"], {"$vector": [2, 2]})
        for key in ("source_sha256", "original_source_sha256", "original_row_sha256", "body_sha256", "expected"):
            with self.assertRaisesRegex(ValueError, "detached"):
                sci.canonical_contract({**mapping, key: "stale"})
        changed = copy.deepcopy(mapping); changed["expected"]["outcome"]["value"]["$vector"][0] = False
        with self.assertRaisesRegex(ValueError, "detached"):
            sci.canonical_contract(changed)
        evidence = {"status": "mismatch", "observed": copy.deepcopy(contract["canonical_expected"])}
        self.assertEqual(sci.canonical_status(contract, evidence, self.runner.same_json), "passed")
        self.assertEqual(evidence["status"], "mismatch")
        for key, value in (("trace", [1]), ("stdout", "extra"), ("id", "other"),
                           ("outcome", {"status": "returned", "value": {"$vector": [0, 2]}})):
            changed = copy.deepcopy(evidence); changed["observed"][key] = value
            self.assertEqual(sci.canonical_status(contract, changed, self.runner.same_json), "mismatch")
        for state in ("not-run", "failed", "timed-out", "rejected", "invalid-output"):
            self.assertNotEqual(sci.canonical_status(contract, {**evidence, "status": state}, self.runner.same_json), "passed")

    def test_sequence_projection_preserves_vectors_numbers_booleans_and_named_values(self):
        seq = {"$seq": [1, {"$keyword": {"namespace": "n", "name": "x", "spelling": "n/x"}}]}
        self.assertEqual(sci.projection(seq), {"$list": seq["$seq"]})
        self.assertNotEqual(sci.projection(seq), sci.projection({"$vector": seq["$seq"]}))
        self.assertNotEqual(sci.projection({"$number": "-0"}), 0)
        self.assertFalse(self.runner.same_json(sci.projection(True), sci.projection(1)))
        self.assertNotEqual(sci.value(sci.wire("(symbol - u0078)")), sci.value(sci.wire("(keyword - u0078)")))

    def test_reference_historical_mismatch_still_requires_and_retains_stderr(self):
        mapping = next(row for row in self.catalog if row["sci_id"] == "sci-reader-anonymous-nonpositive")
        case = sci.cases([mapping])[0]
        canonical = sci.canonical_contract(mapping)["canonical_expected"]
        for stderr in (None, "", "extra\n"):
            with tempfile.TemporaryDirectory() as directory:
                def raw(cases, kind, checkout, output, timeout):
                    if stderr is not None:
                        path = output / "node-0" / "stderr"
                        path.parent.mkdir(parents=True)
                        path.write_text('CASE START ' + json.dumps({"id": case["id"]}) + '\n'
                                        + stderr + 'CASE END ' + json.dumps({"id": case["id"]}) + '\n')
                    return {case["id"]: {"status": "completed", "observed": canonical}}
                preflight = {case["id"]: {"status": "completed", "observed": {"category": "accepted"}}}
                with mock.patch.object(sci, "report_module", return_value=self.runner), \
                     mock.patch.object(self.runner, "checked_analysis", return_value=preflight), \
                     mock.patch.object(self.runner, "reference_cases", side_effect=raw):
                    if stderr is None:
                        with self.assertRaisesRegex(ValueError, "missing canonical stderr observation"):
                            sci.reference_cases([case], "release", Path(directory), Path(directory), 1)
                    else:
                        layer = sci.reference_cases([case], "release", Path(directory), Path(directory), 1)[case["id"]]
                        self.assertEqual(layer["status"], "mismatch")
                        self.assertEqual(layer["stderr"], "" if stderr == "" else ["extra"])

    def test_missing_incomplete_or_false_observations_never_pass(self):
        for evidence in ({}, {"status": "passed"}, {"status": "passed", "observed": {}},
                         {"status": "not-run"}, {"status": "timeout"}):
            self.assertNotEqual(sci.verify(self.case, evidence, self.runner.same_json)["status"], "passed")
        wrong = copy.deepcopy(self.case["expected"])
        wrong["trace"] = ["invented"]
        self.assertEqual(sci.verify(self.case, {"status": "completed", "observed": wrong}, self.runner.same_json)["status"], "mismatch")

    def test_driver_zero_tests_failed_or_timed_out_never_pass(self):
        for stdout, returncode in (("test result: ok. 0 passed; 0 failed; 0 ignored", 0),
                                   ("running 1 test\ntest result: FAILED. 0 passed; 1 failed", 1),
                                   ("running 1 test\ntest result: ok. 0 passed; 0 failed; 1 ignored", 0)):
            process = mock.Mock(returncode=returncode)
            process.communicate.return_value = (stdout, "")
            with tempfile.TemporaryDirectory() as directory, mock.patch.object(sci.subprocess, "Popen", return_value=process) as started:
                observed = sci.local_cases([self.case], Path(directory), 1)
                self.assertEqual(observed[self.case["id"]]["status"], "failed")
                self.assertIn("--locked", started.call_args.args[0])
        process = mock.Mock(returncode=-15, pid=12345)
        process.communicate.side_effect = [subprocess.TimeoutExpired("cargo", 1), ("", "")]
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(sci.subprocess, "Popen", return_value=process), mock.patch.object(sci.os, "killpg") as killed:
            observed = sci.local_cases([self.case], Path(directory), 1)
            self.assertEqual(observed[self.case["id"]]["status"], "timeout")
            killed.assert_called_once_with(12345, sci.signal.SIGTERM)

    def test_infrastructure_failures_cannot_project_into_success(self):
        # Even a valid old observation cannot be accepted after the current
        # process failed or timed out. Such data is stale, not execution.
        for status in ("failed", "timeout", "not-run", "reference-unavailable"):
            evidence = {"status": status, "observed": self.case["expected"]}
            self.assertNotEqual(sci.verify(self.case, evidence, self.runner.same_json)["status"], "passed")


if __name__ == "__main__":
    unittest.main()
