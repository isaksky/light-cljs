#!/usr/bin/env python3
"""Accounting tests intentionally inject lost, skipped and failing observations."""
import contextlib
import copy
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
import report


class AccountingTests(unittest.TestCase):
    def setUp(self):
        self.row = {"disposition": "in-scope", "reason": "source language assertion"}
        self.witness = {"fixture_id": "example"}
        self.complete = {"example": {layer: {"status": "passed"} for layer in report.SEMANTIC_LAYERS}}
        self.case = {"id": "example", "source": "true", "expected": {"id": "example", "outcome": {"status": "returned", "value": True},
                                                     "trace": [], "stdout": ""}}

    def test_only_complete_current_compiler_and_both_references_cover(self):
        self.assertEqual(report.classify(self.row, self.witness, self.complete)[0], "covered")
        self.assertEqual(report.classify(self.row, None, self.complete)[0], "missing")
        self.assertEqual(report.classify(self.row, self.witness, {})[0], "missing")
        self.assertEqual(report.classify(self.row, self.witness, {"example": {
            "lexer": {"status": "accepted"}, "parser": {"status": "accepted"},
            "native": {"status": "passed"}}})[0], "missing")

    def test_eval_cli_does_not_relabel_historical_native_observations(self):
        case = {**self.case, "native_stdout": "true\n"}
        def invoke(command, directory, **kwargs):
            if "--compile" in command:
                return {"status": "failed", "stderr": "unimplemented fixture"}
            return {"status": "completed", "stdout": "true\n", "stderr": ""}
        with tempfile.TemporaryDirectory() as directory, patch.object(report, "invoke", side_effect=invoke):
            layers = report.local_layers(case, "compiler", Path(directory), 1)
        self.assertEqual(layers["native"]["status"], "historical-not-executed")
        self.assertEqual(layers["native"]["observed"], "true\n")
        self.assertEqual(layers["evaluation"]["status"], "passed")
        self.assertEqual(report.classify(self.row, self.witness, {"example": layers})[0], "missing")

    def test_every_nonpass_status_stays_missing_in_every_required_layer(self):
        for layer in report.SEMANTIC_LAYERS:
            for status in ("missing", "rejected", "skipped", "not-run", "timed-out", "reference-unavailable",
                           "invalid-reference", "invalid-output", "unavailable", "failed", "mismatch", "accepted"):
                with self.subTest(layer=layer, status=status):
                    evidence = {"example": {key: dict(value) for key, value in self.complete["example"].items()}}
                    evidence["example"][layer] = {"status": status}
                    self.assertEqual(report.classify(self.row, self.witness, evidence)[0], "missing")
            evidence = {"example": {key: value for key, value in self.complete["example"].items() if key != layer}}
            self.assertEqual(report.classify(self.row, self.witness, evidence)[0], "missing")

    def test_version_differences_and_unrelated_assertions_never_increase_passing_count(self):
        witness = {**self.witness, "version_difference": {"reason": "verified release/checkpoint difference"}}
        self.assertEqual(report.classify(self.row, witness, self.complete)[0], "version-difference")
        self.assertEqual(report.classify(self.row, witness, {})[0], "missing")
        self.assertEqual(report.classify({"disposition": "unrelated", "reason": "upstream AST shape"},
                                         self.witness, self.complete)[0], "unrelated")

    def test_compile_differences_require_observed_rejection_and_executed_checkout(self):
        witness = {**self.witness, "compile_difference": {"reason": "checkout-only macro expansion",
                    "release": {"category": "analysis-error", "detail_contains": "specific unsupported directive"}}}
        evidence = copy.deepcopy(self.complete)
        self.assertEqual(report.classify(self.row, witness, evidence)[0], "missing")
        evidence["example"]["release"] = {"status": "verified-compile-difference"}
        self.assertEqual(report.classify(self.row, witness, evidence)[0], "version-difference")
        evidence["example"]["checkout"] = {"status": "failed"}
        self.assertEqual(report.classify(self.row, witness, evidence)[0], "missing")
        self.assertEqual(report.classify(self.row, self.witness, evidence)[0], "missing")

    def test_compile_difference_rejects_wrong_phase_message_identity_and_missing_rows(self):
        case = {**self.case, "compile_difference": {"reason": "explicit checkout-only directive",
                "release": {"category": "analysis-error", "detail_contains": "specific directive"}}}
        verified = ("REFERENCE IDENTITY release 1.12.134\n"
                    "REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for output, expected in [
                ("example\tanalysis-error\t\tspecific directive is invalid\n", "verified-compile-difference"),
                ("example\taccepted\t\tspecific directive\n", "mismatch"),
                ("example\tanalysis-error\t\tother failure\n", "mismatch"),
                ("example\tanalysis-error\t\tspecific directive\nexample\tanalysis-error\t\tspecific directive\n", "invalid-output"),
                ("", "invalid-output"),
            ]:
                (root / "analysis.tsv").write_text(output)
                with patch.object(report, "invoke", return_value=report.result("completed", stdout=verified)):
                    observed = report.reference_compile_differences([case], "release", root, root, 1)
                    self.assertEqual(observed["example"]["status"], expected)
            with patch.object(report, "invoke", return_value=report.result("completed", stdout="")):
                self.assertEqual(report.reference_compile_differences([case], "release", root, root, 1)["example"]["status"], "invalid-reference")
            with patch.object(report, "invoke", return_value=report.result("failed", "JVM crash")):
                self.assertEqual(report.reference_compile_differences([case], "release", root, root, 1)["example"]["status"], "failed")

    def test_outcomes_require_exact_ids_order_count_and_fields(self):
        one = json.dumps(self.case["expected"])
        second = {"id": "next", "expected": {**self.case["expected"], "id": "next"}}
        two = json.dumps(second["expected"])
        for output in ("", one, one + "\n" + one, two + "\n" + one,
                       one + "\n" + two + "\n" + one, "crashed", "null", "42"):
            with self.subTest(output=output):
                values = report.observations(output, [self.case, second])
                self.assertFalse(any(row["status"] == "passed" for row in values.values()))
        values = report.observations(one + "\n" + two + "\n", [self.case, second])
        self.assertEqual([row["status"] for row in values.values()], ["passed", "passed"])
        partial = json.dumps({"id": "example", "outcome": self.case["expected"]["outcome"]})
        self.assertEqual(report.observations(partial, [self.case])["example"]["status"], "invalid-output")
        for invalid in (one.replace('"value": true', '"value": false, "value": true'),
                        one.replace('"value": true', '"value": NaN')):
            self.assertEqual(report.observations(invalid, [self.case])["example"]["status"], "invalid-output")

    def test_expected_failures_are_compared_as_outcomes_not_passing_by_exception(self):
        case = {"id": "throws", "expected": {"id": "throws", "outcome": {
            "status": "threw", "error": {"category": "type-error"}}, "trace": [1], "stdout": "before\n"}}
        self.assertEqual(report.observations(json.dumps(case["expected"]), [case])["throws"]["status"], "passed")
        for change in ({"trace": []}, {"stdout": ""}, {"outcome": {"status": "returned", "value": None}}):
            self.assertEqual(report.observations(json.dumps({**case["expected"], **change}), [case])["throws"]["status"],
                             "mismatch")

    def test_boolean_number_confusion_cannot_pass(self):
        actual = {**self.case["expected"], "outcome": {"status": "returned", "value": 1}}
        self.assertEqual(report.observations(json.dumps(actual), [self.case])["example"]["status"], "mismatch")
        self.assertFalse(report.same_json([True], [1]))
        self.assertTrue(report.same_json(1, 1.0))

    def test_process_failure_timeout_and_missing_executable_are_not_success(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, command, status in (
                ("crash", [sys.executable, "-c", "print('partial'); raise SystemExit(3)"], "failed"),
                ("timeout", [sys.executable, "-c", "import time; print('partial', flush=True); time.sleep(10)"], "timed-out"),
                ("absent", [str(root / "nonexistent-tool")], "unavailable"),
            ):
                observed = report.invoke(command, root / name, timeout=0.1)
                self.assertEqual(observed["status"], status)
                self.assertTrue((root / name / "invocation.json").exists())

    def test_process_output_preserves_crlf_and_rejects_invalid_utf8(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            crlf = report.invoke([sys.executable, "-c", "import sys; sys.stdout.buffer.write(b'first\\r\\nlast\\r\\n')"],
                                 root / "crlf")
            self.assertEqual(crlf["stdout"], "first\r\nlast\r\n")
            invalid = report.invoke([sys.executable, "-c", "import sys; sys.stdout.buffer.write(b'\\xff')"],
                                    root / "invalid")
            self.assertEqual(invalid["status"], "invalid-output")

    def test_reference_success_requires_pinned_identity_effective_options_and_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            # A successful launcher with omitted build evidence is not a reference.
            with patch.object(report, "invoke", return_value=report.result("completed", stdout="", stderr="")):
                values = report.reference_cases([self.case], "release", root, root / "run", 1)
                self.assertEqual(values["example"]["status"], "invalid-reference")
            with patch.object(report, "invoke", return_value=report.result("unavailable", "Clojure missing")):
                values = report.reference_cases([self.case], "release", root, root / "absent", 1)
                self.assertEqual(values["example"]["status"], "reference-unavailable")
            (root / "reference.js").write_text("", encoding="utf-8")
            verified = ("REFERENCE IDENTITY release 1.12.134\n"
                        "REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}")
            with patch.object(report, "invoke", side_effect=[report.result("completed", stdout=verified),
                              report.result("completed", stdout=json.dumps(self.case["expected"]))]) as invoke:
                self.assertEqual(report.reference_cases([self.case], "release", root, root, 7)["example"]["status"],
                                 "passed")
                self.assertEqual(invoke.call_args_list[-1].kwargs["timeout"], 7)

    def test_isolated_native_witnesses_have_separate_node_invocations(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "reference.js").write_text("", encoding="utf-8")
            cases = [{**self.case, "id": name, "isolated": isolated,
                      "expected": {**self.case["expected"], "id": name}}
                     for name, isolated in [("ordinary", False), ("mutates", True), ("after", True)]]
            verified = ("REFERENCE IDENTITY release 1.12.134\n"
                        "REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}")
            replies = [report.result("completed", stdout=verified)] + [
                report.result("completed", stdout=json.dumps(case["expected"])) for case in cases]
            with patch.object(report, "invoke", side_effect=replies) as invoke:
                results = report.reference_cases(cases, "release", root, root, 7)
                self.assertTrue(all(value["status"] == "passed" for value in results.values()))
                calls = invoke.call_args_list[1:]
                self.assertEqual(len(calls), 3)
                for index, call in enumerate(calls):
                    self.assertIn(f"const selected = [{index}];", call.kwargs["source"])
                    self.assertIn("require(", call.kwargs["source"])
                self.assertEqual(len({call.args[1] for call in calls}), 3)

    def test_namespace_program_requires_observed_results_and_verified_build(self):
        case = {**self.case, "source_root": "tests/fixtures/compiler-namespaces/qualified", "entry": "cljs.core-test", "export": "run"}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for status, expected in (("unavailable", "reference-unavailable"), ("timed-out", "timed-out"), ("failed", "failed")):
                with patch.object(report, "invoke", return_value=report.result(status, "build failed")):
                    self.assertEqual(report.reference_program(case, "release", root, root, 1)[case["id"]]["status"], expected)
            with patch.object(report, "invoke", return_value=report.result("completed", stdout="")):
                self.assertEqual(report.reference_program(case, "release", root, root, 1)[case["id"]]["status"], "invalid-reference")
            (root / "reference.js").write_text("")
            verified = "REFERENCE IDENTITY release 1.12.134\nREFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}"
            for output, expected in (("", "invalid-output"), (json.dumps(case["expected"]), "passed")):
                with patch.object(report, "invoke", side_effect=[report.result("completed", stdout=verified), report.result("completed", stdout=output)]):
                    self.assertEqual(report.reference_program(case, "release", root, root, 1)[case["id"]]["status"], expected)


class DiagnosticTests(unittest.TestCase):
    def setUp(self):
        self.probe = {"id": "probe-1", "mode": "expr", "source": "(quote)",
                      "local": {"category": "analysis-error", "detail": "quote requires one argument"},
                      **{kind: {"category": "analysis-error", "detail": "Wrong number of args to quote at line 1 ",
                                "warnings": "", "exception_class": "clojure.lang.ExceptionInfo"}
                         for kind in ("release", "checkout")}}
        self.case = {"id": "negative", "diagnostic": {"body_id": "body", "body_sha256": "body-sha",
                    "assertion_ids": ["assertion"], "rationale": "Complete original diagnostic body",
                    "probes": [self.probe]}}
        self.row = {"disposition": "in-scope", "reason": "original expected rejection"}
        self.witness = {"fixture_id": "negative", "expected_rejection": True}
        self.evidence = {"negative": {layer: {"status": "verified-rejection"} for layer in report.REJECTION_LAYERS}}
        self.verified = ("REFERENCE IDENTITY release 1.12.134\n"
                         "REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}")

    def test_expected_rejections_have_separate_accounting_and_need_all_three_layers(self):
        self.assertEqual(report.classify(self.row, self.witness, self.evidence)[0], "verified-rejection")
        for layer in report.REJECTION_LAYERS:
            for status in ("passed", "accepted", "rejected", "failed", "mismatch", "not-run", "timed-out",
                           "verified-compile-difference", "verified-diagnostic-difference"):
                observed = copy.deepcopy(self.evidence)
                observed["negative"][layer]["status"] = status
                self.assertEqual(report.classify(self.row, self.witness, observed)[0], "missing")
        self.assertEqual(report.classify(self.row, {"fixture_id": "negative"}, self.evidence)[0], "missing")

    def test_local_requires_parser_acceptance_exact_analyzer_message_and_exit_one(self):
        accepted = report.result("completed", stdout="", stderr="", returncode=0)
        rejected = report.result("failed", stdout="", returncode=1,
                                  stderr="light-cljs: 1:1: quote requires one argument (bytes 0..7)\n")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for update, expected in (({}, "verified-rejection"), ({"returncode": -11}, "mismatch"),
                                      ({"returncode": 2}, "mismatch"), ({"stdout": "partial"}, "mismatch"),
                                      ({"stderr": "light-cljs: 1:1: unsupported construct: quote (bytes 0..7)\n"}, "mismatch"),
                                      ({"stderr": "light-cljs: 1:1: other error (bytes 0..7)\n"}, "mismatch"),
                                      ({"status": "completed"}, "mismatch"), ({"status": "timed-out"}, "timed-out")):
                with patch.object(report, "invoke", side_effect=[accepted, accepted, {**rejected, **update}]):
                    layers = report.local_layers(self.case, "compiler", root, 1)
                    self.assertEqual(layers["diagnostic"]["status"], expected)
                    self.assertEqual(layers["javascript"]["status"], "not-applicable")
            with patch.object(report, "invoke", side_effect=[accepted, rejected]) as invoke:
                self.assertEqual(report.local_layers(self.case, "compiler", root, 1)["diagnostic"]["status"], "mismatch")
                self.assertEqual(invoke.call_count, 2)

    def test_every_loop_probe_must_match_and_observations_must_be_complete(self):
        case = copy.deepcopy(self.case)
        case["diagnostic"]["probes"] += [{**self.probe, "id": "probe-2"}]
        lines = ["negative--probe-1\tanalysis-error\t\tWrong number of args to quote at line 1 \n",
                 "negative--probe-2\tanalysis-error\t\tWrong number of args to quote at line 1 \n"]
        classes = [f"negative--probe-{index}\tclojure.lang.ExceptionInfo\n" for index in (1, 2)]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            mutations = [(lines, classes, "verified-rejection"),
                         (lines[:1], classes, "invalid-output"),
                         (lines[::-1], classes, "invalid-output"),
                         ([lines[0], lines[0]], classes, "invalid-output"),
                         (lines, classes[:1], "invalid-output"),
                         (lines, classes[::-1], "invalid-output"),
                         (lines, [classes[0], classes[1].replace("clojure.lang.ExceptionInfo", "java.lang.Error")], "mismatch"),
                         ([lines[0], lines[1].replace("analysis-error", "reader-error")], classes, "mismatch"),
                         ([lines[0], lines[1].replace("Wrong number", "Unexpected number")], classes, "mismatch"),
                         ([lines[0], lines[1].replace("analysis-error\t\t", "analysis-error\tundeclared-var\t")], classes, "mismatch")]
            for rows, types, expected in mutations:
                (root / "analysis.tsv").write_text("".join(rows))
                (root / "exception-classes.tsv").write_text("".join(types))
                with patch.object(report, "invoke", return_value=report.result("completed", stdout=self.verified)):
                    self.assertEqual(report.reference_diagnostics([case], "release", root, root, 1)["negative"]["status"], expected)
            for output in ("", self.verified.replace("1.12.134", "1.0"), self.verified.replace(":static-fns false", ":static-fns true")):
                with patch.object(report, "invoke", return_value=report.result("completed", stdout=output)):
                    self.assertEqual(report.reference_diagnostics([case], "release", root, root, 1)["negative"]["status"], "invalid-reference")
            for status in ("failed", "timed-out", "unavailable"):
                with patch.object(report, "invoke", return_value=report.result(status)):
                    self.assertEqual(report.reference_diagnostics([case], "release", root, root, 1)["negative"]["status"],
                                     "reference-unavailable" if status == "unavailable" else status)

    def test_namespace_uses_original_file_and_distinguishes_header_error_from_missing_file(self):
        case = copy.deepcopy(self.case)
        probe = case["diagnostic"]["probes"][0]
        probe.update(mode="ns", entry="foo.bar", source="(ns foo.bar (:unless []))",
                     local={"category": "namespace-error", "detail": "unsupported namespace clause"})
        accepted = report.result("completed", stdout="", stderr="", returncode=0)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = (root / "probe-1/source/foo/bar.cljs").resolve()
            for message, expected in [(f"{path}:1:13: unsupported namespace clause (bytes 12..24)", "verified-rejection"),
                                      ("missing namespace file: foo.bar", "mismatch"),
                                      (f"{path}:1:13: unsupported construct: ns (bytes 12..24)", "mismatch"),
                                      (f"{path.with_name('wrong.cljs')}:1:13: unsupported namespace clause (bytes 12..24)", "mismatch")]:
                rejected = report.result("failed", returncode=1, stdout="", stderr=f"light-cljs: {message}\n")
                with patch.object(report, "invoke", side_effect=[accepted, accepted, rejected]) as invoke:
                    self.assertEqual(report.local_layers(case, "compiler", root, 1)["diagnostic"]["status"], expected)
                    self.assertEqual(path.read_text(), probe["source"])
                    self.assertEqual(invoke.call_args.args[0], ["compiler", "--compile", "--entry", "foo.bar", "--source-root", str(root / "probe-1/source")])

    def test_checked_release_acceptance_is_only_an_explicit_version_difference(self):
        witness = {**self.witness, "diagnostic_difference": "Release accepts; checkout validates binding"}
        self.assertEqual(report.classify(self.row, witness, self.evidence)[0], "missing")
        observed = copy.deepcopy(self.evidence)
        observed["negative"]["release"]["status"] = "verified-diagnostic-difference"
        self.assertEqual(report.classify(self.row, witness, observed)[0], "version-difference")
        self.assertEqual(report.classify(self.row, self.witness, observed)[0], "missing")
        observed["negative"]["checkout"]["status"] = "accepted"
        self.assertEqual(report.classify(self.row, witness, observed)[0], "missing")
        case = copy.deepcopy(self.case)
        case["diagnostic"]["release_difference"] = witness["diagnostic_difference"]
        case["diagnostic"]["probes"][0]["release"] = {"category": "warning", "warnings": "undeclared-var", "detail": "", "exception_class": ""}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "exception-classes.tsv").write_text("negative--probe-1\t\n")
            for category, warnings, expected in [("warning", "undeclared-var", "verified-diagnostic-difference"),
                                                  ("warning", "fn-arity", "mismatch"), ("accepted", "", "mismatch")]:
                (root / "analysis.tsv").write_text(f"negative--probe-1\t{category}\t{warnings}\t\n")
                with patch.object(report, "invoke", return_value=report.result("completed", stdout=self.verified)):
                    self.assertEqual(report.reference_diagnostics([case], "release", root, root, 1)["negative"]["status"], expected)

    def test_mixed_body_subset_retains_order_hash_and_independent_missing_assertions(self):
        inventory = {key: {"disposition": "in-scope"} for key in ("positive", "assertion", "later")}
        bodies = {"body": {"source_span": {"sha256": "body-sha"}, "assertions": ["positive", "assertion", "later"]}}
        with self.assertRaises(ValueError):report.validate_diagnostic(self.case, inventory, bodies)
        case = copy.deepcopy(self.case)
        case["diagnostic"].update(assertion_subset=True, split_rationale="Positive namespace source executes separately; retain original negative assertion.")
        report.validate_diagnostic(case, inventory, bodies)
        self.assertEqual(report.classify(inventory["positive"], None, self.evidence)[0], "missing")
        for ids in (["later", "assertion"], ["assertion", "assertion"], ["unknown"], []):
            changed = copy.deepcopy(case);changed["diagnostic"]["assertion_ids"] = ids
            with self.assertRaises(ValueError):report.validate_diagnostic(changed, inventory, bodies)
        case["diagnostic"].pop("split_rationale")
        with self.assertRaises(ValueError):report.validate_diagnostic(case, inventory, bodies)

    def test_contract_rejects_stale_partial_generic_or_permissive_diagnostic_mappings(self):
        inventory = {"assertion": {"disposition": "in-scope"}}
        bodies = {"body": {"source_span": {"sha256": "body-sha"}, "assertions": ["assertion"]}}
        report.validate_diagnostic(self.case, inventory, bodies)
        mutations = [("body_sha256", "stale"), ("assertion_ids", []), ("probes", []),
                     ("probes", [self.probe, self.probe]), ("release_difference", "")]
        for key, value in mutations:
            case = copy.deepcopy(self.case)
            case["diagnostic"][key] = value
            with self.assertRaises(ValueError):
                report.validate_diagnostic(case, inventory, bodies)
        for layer, update in [("local", {"detail": "unsupported construct: quote"}),
                              ("local", {"category": "reader-error"}), ("release", {"category": "accepted"}),
                              ("checkout", {"exception_class": "java.lang.Error"})]:
            case = copy.deepcopy(self.case)
            case["diagnostic"]["probes"][0][layer].update(update)
            with self.assertRaises(ValueError):
                report.validate_diagnostic(case, inventory, bodies)


class InventoryTests(unittest.TestCase):
    def test_live_contract_contains_complete_bodies_and_traceable_witnesses(self):
        inventory, witnesses, cases = report.load_contract()
        self.assertGreater(len(inventory), len(witnesses))
        self.assertTrue(cases)
        for witness in witnesses.values():
            self.assertIn(witness["fixture_id"], cases)

    def test_current_report_is_missing_until_execution_and_strict_fails(self):
        inventory, witnesses, _ = report.load_contract()
        assembled = report.assemble(inventory, witnesses, {})
        self.assertEqual(assembled["counts"]["covered"], 0)
        self.assertGreater(assembled["counts"]["missing"], 0)
        self.assertFalse(assembled["strict_complete"])
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(report.main(["--strict"]), 1)

    def test_invalid_selection_cannot_appear_complete(self):
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(report.main(["--strict", "--id", "not-an-assertion"]), 2)
            self.assertEqual(report.main(["--feature", "not-a-feature"]), 2)

    def test_version_expectations_preserve_types_and_shared_fixture_contracts(self):
        _, witnesses, cases = report.load_contract()
        original_jsonl = report.jsonl

        def load_with(rows):
            def data(path):
                return rows if path.name == "witnesses.jsonl" else original_jsonl(path)
            with patch.object(report, "jsonl", side_effect=data):
                return report.load_contract()

        rows = copy.deepcopy(list(witnesses.values()))
        expected = copy.deepcopy(cases[rows[0]["fixture_id"]]["expected"])
        release = copy.deepcopy(expected)
        release["outcome"]["value"] = 1
        rows[0]["version_difference"] = {"reason": "injected explicit numeric versus boolean contract",
                                         "release": release, "checkout": expected}
        # true and 1 are a real contract difference, not Python-equal values.
        load_with(rows)
        bad_checkout = copy.deepcopy(rows)
        bad_checkout[0]["version_difference"] = {"reason": "injected wrong checkout expectation",
                                                 "release": expected, "checkout": release}
        with self.assertRaisesRegex(ValueError, "checkout must remain"):
            load_with(bad_checkout)
        # An unselected witness cannot silently overwrite another assertion's
        # pinned reference expectations through a shared fixture.
        rows[1]["fixture_id"] = rows[0]["fixture_id"]
        with self.assertRaisesRegex(ValueError, "inconsistent reference expectations"):
            load_with(rows)

    def test_no_warning_witnesses_fail_on_compiler_or_reference_warnings(self):
        case = {"id": "quiet", "source": "true", "native_stdout": None, "no_warnings": True, "expected": {"id": "quiet", "outcome": {"status": "returned", "value": True}, "trace": [], "stdout": ""}}
        completed = {"status": "completed", "stdout": "true", "stderr": "WARNING: protocol call warning"}
        with tempfile.TemporaryDirectory() as directory, patch.object(report, "invoke", return_value=completed):
            root = Path(directory)
            local = report.local_layers(case, "compiler", root / "local", 1)
            self.assertEqual(local["javascript"]["status"], "mismatch")
            reference = report.reference_cases([case], "release", root, root / "reference", 1)
            self.assertEqual(reference[case["id"]]["status"], "mismatch")
            program = {**case, "source_root": "tests/fixtures/compiler-namespaces/qualified", "entry": "cljs.core-test", "export": "run"}
            for kind in ("release", "checkout"):
                reference = report.reference_program(program, kind, root, root / kind, 1)
                self.assertEqual(reference[case["id"]]["status"], "mismatch")

    def test_signature_and_recur_rejections_are_missing_capabilities_not_negative_tests(self):
        boundaries = {row["id"]: row for row in report.boundary_inventory()}
        for identity in ("fn-variadic", "fn-multiple", "special-recur", "special-loopstar"):
            row = boundaries[identity]
            self.assertEqual(row["disposition"], "accepted-boundary" if row["local_category"] == "accepted"
                             else "missing-capability")
        for identity in ("if-empty", "fn-variadic-missing-rest", "reader-map-duplicate", "def-missing"):
            self.assertEqual(boundaries[identity]["disposition"], "negative-test")


class WarningAccountingTests(unittest.TestCase):
    def setUp(self):
        self.trace = [["undeclared-var", "Use of undeclared Var cljs.user/x"]]
        self.reference = {"category": "warning", "warnings": "undeclared-var", "detail": "", "warning_trace": self.trace}
        self.probe = {"id": "probe", "source": "x", "mode": "expr", "settings": {"static-fns": False, "checked-arrays": False},
                      "local": {"category": "accepted", "warning_trace": self.trace}, "release": self.reference, "checkout": self.reference}
        self.case = {"id": "warning", "warning": {"body_id": "body", "body_sha256": "hash", "assertion_ids": ["one"],
                      "rationale": "complete warning source assertion", "probes": [self.probe]}}
        self.witness = {"fixture_id": "warning", "expected_warning": True}
        self.row = {"disposition": "in-scope"}

    def test_warning_classification_is_separate_and_requires_all_live_layers(self):
        evidence = {"warning": {layer: report.result("verified-warning") for layer in report.REJECTION_LAYERS}}
        self.assertEqual(report.classify(self.row, self.witness, evidence)[0], "verified-warning")
        for layer in report.REJECTION_LAYERS:
            for status in ("passed", "accepted", "rejected", "verified-rejection", "mismatch", "not-run", "timed-out"):
                changed = copy.deepcopy(evidence);changed["warning"][layer] = report.result(status)
                self.assertEqual(report.classify(self.row, self.witness, changed)[0], "missing")
        difference = {**self.witness, "warning_difference": "checked old warning semantics"}
        self.assertEqual(report.classify(self.row, difference, evidence)[0], "missing")
        evidence["warning"]["release"] = report.result("verified-warning-difference")
        self.assertEqual(report.classify(self.row, difference, evidence)[0], "version-difference")

    def test_warning_contract_rejects_changed_settings_dropped_assertions_and_unproven_difference(self):
        inventory = {"one": {"disposition": "in-scope"}}
        bodies = {"body": {"source_span": {"sha256": "hash"}, "assertions": ["one"]}}
        report.validate_warning(self.case, inventory, bodies)
        for mutation in (lambda c: c["warning"].update(assertion_ids=[]),
                         lambda c: c["warning"]["probes"][0]["settings"].update({"static-fns": True}),
                         lambda c: c["warning"].update(release_difference="not actually different"),
                         lambda c: c["warning"]["probes"][0]["local"].update(category="analysis-error"),
                         lambda c: c["warning"]["probes"][0]["local"].update(warning_trace=[])):
            changed = copy.deepcopy(self.case);mutation(changed)
            with self.assertRaises(ValueError):report.validate_warning(changed, inventory, bodies)

    def test_local_warning_requires_exact_trace_and_successful_compilation(self):
        expected = "WARNING: Use of undeclared Var cljs.user/x [undeclared-var] (source 0, bytes 0..1)\n"
        with tempfile.TemporaryDirectory() as directory:
            for stderr, status, expected_status in [(expected, "completed", "verified-warning"),
                    ("", "completed", "mismatch"), (expected + expected, "completed", "mismatch"),
                    (expected + "unexpected stderr\n", "completed", "mismatch"),
                    (expected, "failed", "mismatch"), (expected, "timed-out", "timed-out")]:
                def invoke(command, *_args, **_kwargs):
                    return report.result(status if "--compile" in command else "completed", stdout="true", stderr=stderr if "--compile" in command else "")
                with patch.object(report, "invoke", side_effect=invoke):
                    observed = report.local_warning(self.case, "compiler", Path(directory), 1)
                self.assertEqual(observed["diagnostic"]["status"], expected_status)
                self.assertEqual(observed["javascript"]["status"], "not-applicable")

    def test_warning_oracle_checks_settings_ids_order_and_exact_messages(self):
        verified = "REFERENCE IDENTITY release 1.12.134\nREFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}\n"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "analysis.tsv").write_text("probe\twarning\tundeclared-var\t\n")
            (root / "warnings.tsv").write_text("probe\t" + json.dumps(self.trace) + "\n")
            (root / "warning-settings.tsv").write_text("probe\tfalse\tfalse\n")
            with patch.object(report, "invoke", return_value=report.result("completed", stdout=verified)) as invoke:
                values = report.checked_analysis([self.probe], "release", root, root, 1, warning_settings=True)
                self.assertEqual(values["probe"]["observed"], self.reference)
                self.assertIn("tests/reference/compiler-language/warnings.clj", invoke.call_args.args[0])
                for content in ("", "probe\ttrue\tfalse\n", "probe\tfalse\tfalse\nprobe\tfalse\tfalse\n"):
                    (root / "warning-settings.tsv").write_text(content)
                    self.assertEqual(report.checked_analysis([self.probe], "release", root, root, 1, warning_settings=True)["probe"]["status"], "invalid-output")
                (root / "warning-settings.tsv").write_text("probe\tfalse\tfalse\n")
                (root / "warnings.tsv").write_text("other\t[]\n")
                self.assertEqual(report.checked_analysis([self.probe], "release", root, root, 1, warning_settings=True)["probe"]["status"], "invalid-output")

    def test_node_bootstrap_override_requires_live_options_evidence_and_standard_loader(self):
        outcome = {"id": "program", "outcome": {"status": "returned", "value": True}, "trace": [], "stdout": ""}
        case = {"id": "program", "source": "(ns sample)", "source_root": "tests/fixtures/compiler-namespaces/qualified", "entry": "sample", "export": "run", "nodejs_rt": False, "expected": outcome}
        identity = "REFERENCE IDENTITY release 1.12.134\nREFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}\n"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory);(root / "reference.js").write_text("unused browser loader")
            for checked in (False, True):
                def invoke(command, *_args, **kwargs):
                    if "-M" in command:
                        self.assertEqual(command[-1], "false")
                        return report.result("completed", stdout=identity + ("REFERENCE NODEJS_RT false\n" if checked else ""))
                    self.assertIn("out/goog/bootstrap/nodejs.js", kwargs["source"])
                    self.assertIn("out/cljs_deps.js", kwargs["source"])
                    self.assertIn("goog.require('semantic.program')", kwargs["source"])
                    return report.result("completed", stdout=json.dumps(outcome) + "\n")
                with patch.object(report, "invoke", side_effect=invoke):
                    value = report.reference_program(case, "release", root, root, 1)["program"]
                    self.assertEqual(value["status"], "passed" if checked else "invalid-reference")
                    if checked:self.assertIs(value["settings"]["nodejs-rt"], False)

    def test_source_static_true_context_must_match_in_addition_to_pinned_false(self):
        self.probe["supplemental_static_fns"] = True
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for bad in (False, True):
                def checked(cases, *_args, **_kwargs):
                    return {p["id"]: report.result("completed", observed={**self.reference, "warning_trace": []}
                            if bad and p["settings"]["static-fns"] else self.reference) for p in cases}
                with patch.object(report, "checked_analysis", side_effect=checked) as call:
                    value = report.reference_warnings([self.case], "release", root, root, 1)["warning"]
                    self.assertEqual(value["status"], "mismatch" if bad else "verified-warning")
                    self.assertFalse(call.call_args_list[0].args[0][0]["settings"]["static-fns"])
                    self.assertTrue(call.call_args_list[1].args[0][0]["settings"]["static-fns"])


class SupplementalBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.cases = report.boundary_support_contract()
        self.by_id = {case["id"]: case for case in self.cases}

    def contract_with(self, changed):
        original = report.jsonl
        with patch.object(report, "jsonl", side_effect=lambda path: changed if path.name == "boundary-support.jsonl" else original(path)):
            return report.boundary_support_contract()

    def test_catalog_retains_all_original_sources_and_exact_public_equivalents(self):
        self.assertEqual(len(self.cases), 27)
        self.assertEqual(sum(len(case.get("public_equivalents", [])) for case in self.cases), 4)
        for mutate in (lambda rows: rows.pop(),
                       lambda rows: rows[0].update(source="true"),
                       lambda rows: rows[0].update(source_sha256="stale"),
                       lambda rows: rows[0].update(local={"category": "accepted", "warning_trace": []}),
                       lambda rows: rows[1]["public_equivalents"][0].update(source="(let [x 1] x)"),
                       lambda rows: rows[1]["public_equivalents"][0]["checkout"].update(category="accepted", detail="")):
            changed = copy.deepcopy(self.cases);mutate(changed)
            with self.assertRaises(ValueError):self.contract_with(changed)

    def test_supported_cases_need_exact_warning_contract_and_runtime_source_tree(self):
        for identity, mutate in (
            ("unqualified-host", lambda case: case.pop("runtime")),
            ("unknown-alias", lambda case: case["local"].update(warning_trace=[])),
            ("ns-as-duplicate", lambda case: case["local_files"].update({"boundary.cljs": "(ns boundary)"})),
            ("reader-conditional", lambda case: case["runtime"][0].update(files_sha256={})),
            ("fn-empty-method", lambda case: case.update(runtime=self.by_id["unqualified-host"]["runtime"])),
        ):
            changed = copy.deepcopy(self.cases)
            mutate(next(case for case in changed if case["id"] == identity))
            with self.assertRaises(ValueError):self.contract_with(changed)

    def test_macro_phase_difference_preserves_reference_hashes_and_needs_both_phase_proofs(self):
        case = self.by_id["macro-defmacro"]
        self.assertEqual(case["kind"], "intentional-difference")
        self.assertEqual(case["source"], "(defmacro m [] 1)")
        self.assertEqual(case["source_sha256"], "e20d17315c3f9c14e5bb05f2956be9ce618660d49254ccddfbba8c0425482082")
        self.assertEqual([proof["source_sha256"] for proof in case["reference_runtime"]], [
            "6c958a7b0fa93e84f93f2aa7bef93f4ba1b035024fee765266db0c5ff7ce43af",
            "fa55e075c1c7cf662ec5f098c6bf46d1606d97daca4620fac6591670f48f36eb",
            "0c4e379d1d2cd04c36803865586788ef26c6962fb8fb72643b78a61dfd47b046"])
        for mutate in (
            lambda case: case.pop("local_runtime"),
            lambda case: case.pop("reference_runtime"),
            lambda case: case["reference_runtime"].pop(),
            lambda case: case["local_probes"].pop(),
            lambda case: case["local_probes"][0].update(source_sha256="stale"),
            lambda case: case["local_probes"][0].update(local={"category": "accepted", "warning_trace": []}),
            lambda case: case["local_probes"][2].update(local={"category": "analysis-error", "detail": "stale Var identity rejection"}),
            lambda case: case.update(kind="public-runtime", runtime=case["reference_runtime"]),
        ):
            changed = copy.deepcopy(self.cases)
            mutate(next(case for case in changed if case["id"] == "macro-defmacro"))
            with self.assertRaises(ValueError):self.contract_with(changed)

    def test_duplicate_macro_aliases_preserve_both_complete_phase_contracts(self):
        case = self.by_id["ns-as-duplicate"]
        self.assertEqual(case["kind"], "intentional-difference")
        local = case["local_runtime"][0]
        reference = case["reference_runtime"][0]
        self.assertEqual(reference["source_sha256"], "7b4fbf2c2540ccb097b6501e4ea6bc1fd49c0e6b5414558fa68040ae48cbe387")
        self.assertIn(":require-macros", reference["source"])
        self.assertNotIn(":require-macros", local["source"])
        self.assertEqual(local["source"].count("(:require "), 1)
        self.assertEqual(local["source"].count("[boundary.macros :as m :refer [twice]]"), 2)
        self.assertIn("boundary/macros.cljs", local["files_sha256"])
        self.assertEqual(local["expected"]["outcome"], reference["expected"]["outcome"])
        for key in ("local_runtime", "reference_runtime"):
            changed = copy.deepcopy(self.cases)
            next(value for value in changed if value["id"] == case["id"]).pop(key)
            with self.assertRaises(ValueError):
                self.contract_with(changed)

    def test_macro_phase_runner_executes_only_the_declared_phase_and_detects_either_failure(self):
        case = self.by_id["macro-defmacro"]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for failure in (None, "local-runtime", "local-phase", "release", "checkout"):
                def local(probe, *_args):
                    if probe is case:return report.result("verified-warning")
                    expected = "verified-rejection" if probe["local"]["category"] == "analysis-error" else "verified-warning"
                    return report.result("mismatch" if failure == "local-phase" else expected)
                def local_runtime(proof, *_args, **_kwargs):
                    self.assertEqual(proof["id"], "boundary-defmacro-compile-time-expansion")
                    return {"javascript": report.result("mismatch" if failure == "local-runtime" else "passed")}
                def analyzed(probes, kind, *_args, **_kwargs):
                    self.assertEqual([probe["id"] for probe in probes], ["macro-defmacro"])
                    return {case["id"]: report.result("completed", observed=case[kind])}
                def reference_runtime(proofs, kind, *_args):
                    self.assertEqual([proof["id"] for proof in proofs], [proof["id"] for proof in case["reference_runtime"]])
                    return {proof["id"]: report.result("mismatch" if failure == kind else "passed") for proof in proofs}
                with patch.object(report, "local_boundary_probe", side_effect=local), \
                     patch.object(report, "local_layers", side_effect=local_runtime) as local_execution, \
                     patch.object(report, "checked_analysis", side_effect=analyzed), \
                     patch.object(report, "reference_cases", side_effect=reference_runtime) as reference_execution:
                    evidence = report.run_boundary_support([case], "compiler", root, root, 1, 1, "both")
                self.assertEqual(local_execution.call_count, 1)
                self.assertEqual(reference_execution.call_count, 2)
                self.assertEqual(report.boundary_support_status(case, evidence[case["id"]]),
                                 "verified-intentional-difference" if failure is None else "mismatch")

    def test_supplemental_checks_never_inflate_source_counts_or_pass_when_unexecuted(self):
        inventory, witnesses, _ = report.load_contract()
        before = report.assemble(inventory, witnesses, {})
        evidence = {case["id"]: {layer: report.result("verified-boundary") for layer in ("local", "release", "checkout")} for case in self.cases}
        after = report.assemble(inventory, witnesses, {}, boundary_evidence=evidence)
        self.assertEqual(before["counts"], after["counts"])
        self.assertFalse(before["boundary_strict_complete"])
        self.assertTrue(after["boundary_strict_complete"])
        self.assertFalse(after["strict_complete"])
        for status in ("passed", "accepted", "rejected", "not-run", "mismatch", "timed-out", "invalid-reference"):
            changed = copy.deepcopy(evidence[self.cases[0]["id"]]);changed["release"]["status"] = status
            self.assertEqual(report.boundary_support_status(self.cases[0], changed), "mismatch")
        self.assertEqual(report.boundary_support_status(self.cases[0], {}), "not-run")

    def test_runner_checks_runtime_wrong_warnings_and_public_rejections_independently(self):
        selected = [self.by_id[key] for key in ("unqualified-host", "unknown-alias", "fn-empty-method")]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for failure in (None, "runtime", "warnings", "public", "local"):
                def local(probe, *_args):
                    return report.result("mismatch" if failure == "local" else "verified-rejection" if probe["local"]["category"] == "analysis-error" else "verified-warning")
                def analyzed(probes, kind, *_args, **_kwargs):
                    result = {}
                    for probe in probes:
                        observed = copy.deepcopy(probe[kind])
                        if failure == "warnings" and probe["id"] == "unknown-alias":observed["warning_trace"] = []
                        if failure == "public" and probe["id"] == "public-fn-empty":observed.update(category="accepted", detail="")
                        result[probe["id"]] = report.result("completed", observed=observed)
                    return result
                def executed(cases, *_args):
                    return {case["id"]: report.result("mismatch" if failure == "runtime" else "passed", observed=case["expected"]) for case in cases}
                with patch.object(report, "local_boundary_probe", side_effect=local), \
                     patch.object(report, "local_layers", return_value={"javascript": report.result("passed")}), \
                     patch.object(report, "checked_analysis", side_effect=analyzed), \
                     patch.object(report, "reference_cases", side_effect=executed):
                    evidence = report.run_boundary_support(selected, "compiler", root, root, 1, 1, "both")
                passed = all(report.boundary_support_status(case, evidence[case["id"]]).startswith("verified-") for case in selected)
                self.assertEqual(passed, failure is None)

    def test_quiet_public_sources_use_default_cli_and_warning_cases_use_explicit_api(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for identity, warnings in (("unqualified-host", False), ("unknown-alias", True)):
                case = self.by_id[identity]
                stderr = "".join(f"WARNING: {message} [{kind}] (source 0, bytes 0..1)\n" for kind, message in case["local"]["warning_trace"])
                def invoke(command, *_args, **_kwargs):
                    if "--compile" in command:self.assertEqual("--warnings" in command, warnings)
                    return report.result("completed", stdout="true", stderr=stderr if "--compile" in command else "")
                with patch.object(report, "invoke", side_effect=invoke):
                    self.assertEqual(report.local_boundary_probe(case, "compiler", root, 1)["status"], "verified-warning")


if __name__ == "__main__":
    unittest.main()
