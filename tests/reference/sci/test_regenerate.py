"""Failure-enforcement tests; no Java, network, or research checkout required."""
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import regenerate as reference


class ReferenceContract(unittest.TestCase):
    def test_missing_duplicate_extra_reordered_and_malformed_results_fail(self):
        rows = [["sci-a", "portable", "1", "(return (int 1))", "(vector)", "(string u)", "(string u)"],
                ["sci-b", "portable", "2", "(return (int 2))", "(vector)", "(string u)", "(string u)"]]
        lines = ["\t".join([row[0]] + row[3:]) for row in rows]
        valid = "\n".join(lines) + "\n"
        self.assertEqual(reference.checked_results(valid, rows), rows)
        for output in ["", lines[0], valid + lines[0], "\n".join(reversed(lines)),
                       lines[0] + "\n" + lines[0], valid + "\n", valid.replace("\t(vector)", "\t")]:
            with self.subTest(output=output), self.assertRaises(RuntimeError):
                reference.checked_results(output, rows)

    def test_missing_or_mismatched_runtime_and_options_fail(self):
        metadata = (f"format\tsci-outcomes-v1\nrevision\t{reference.REVISION}\n"
                    f"clojure\t1.12.4\njava\ttest\njava-vendor\ttest\noptions\t{reference.OPTIONS}\n")
        self.assertEqual(reference.checked_metadata(metadata)["revision"], reference.REVISION)
        for invalid in ["", metadata.replace(reference.REVISION, "HEAD"),
                        metadata.replace("1.12.4", "1.12.0"), metadata.replace(":clj", ":cljs"),
                        metadata.replace("java\ttest\n", ""), metadata + "java\ttest\n"]:
            with self.subTest(invalid=invalid), self.assertRaises(RuntimeError):
                reference.checked_metadata(invalid)

    def test_wrong_dirty_missing_or_nested_checkout_never_falls_back(self):
        with tempfile.TemporaryDirectory() as tmp:
            checkout = Path(tmp).resolve()
            (checkout / "deps.edn").write_text("{}")
            for replies in [[str(checkout), "0" * 40],
                            [str(checkout), reference.REVISION, " M src/sci/core.cljc"],
                            [str(checkout), reference.REVISION, "?? rogue.clj"],
                            [str(checkout.parent)]]:
                with self.subTest(replies=replies), patch.object(reference, "checked_process", side_effect=replies), self.assertRaises(RuntimeError):
                    reference.checked_checkout(checkout, checkout)
            with patch.object(reference, "checked_process", side_effect=[str(checkout), reference.REVISION, ""]):
                self.assertEqual(reference.checked_checkout(checkout, checkout), checkout)
            (checkout / "deps.edn").unlink()
            with patch.object(reference, "checked_process", side_effect=[str(checkout), reference.REVISION, ""]), self.assertRaises(RuntimeError):
                reference.checked_checkout(checkout, checkout)
            with self.assertRaises(FileNotFoundError):
                reference.checked_checkout(checkout / "missing", checkout)

    def test_crash_timeout_and_unavailable_driver_preserve_diagnostics(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name, script, timeout, message in [
                ("crash", "import sys; print('output before failure', flush=True); print('CASE START sci-crash', file=sys.stderr, flush=True); sys.exit(7)", 3, "failed (7)"),
                ("hang", "import sys,time; print('CASE START sci-hang', file=sys.stderr, flush=True); time.sleep(60)", .2, "timed out"),
            ]:
                with self.subTest(name=name), self.assertRaisesRegex(RuntimeError, message.replace("(", r"\(").replace(")", r"\)")):
                    reference.checked_process([sys.executable, "-c", script], root, root / name, timeout)
                self.assertIn("CASE START sci-" + name, (root / name / "stderr.txt").read_text())
                self.assertTrue((root / name / "invocation.txt").is_file())
                if name == "crash":
                    self.assertEqual((root / name / "stdout.txt").read_text(), "output before failure\n")
            with self.assertRaisesRegex(RuntimeError, "unavailable"):
                reference.checked_process([str(root / "missing-tool")], root, root / "missing", 1)

    @unittest.skipUnless(os.name == "posix", "reference launcher uses POSIX process groups")
    def test_timeout_kills_launcher_descendants(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            marker = root / "descendant-survived"
            script = ("import subprocess,sys,time; subprocess.Popen([sys.executable,'-c',"
                      + repr("import time; from pathlib import Path; time.sleep(.5); Path(" + repr(str(marker)) + ").write_text('bad')")
                      + "]); time.sleep(60)")
            with self.assertRaisesRegex(RuntimeError, "timed out"):
                reference.checked_process([sys.executable, "-c", script], root, root / "driver", .2)
            import time
            time.sleep(.5)
            self.assertFalse(marker.exists(), "timed-out reference left a descendant running")

    def test_fixture_input_and_edn_paths_are_strict(self):
        for invalid in ["", "sci-a\tportable\t1", "# comments only\n"]:
            with self.assertRaises(RuntimeError):
                reference.fixture_rows(invalid)
        row = "sci-a\tportable\t1\tnil\tnil\tnil\tnil"
        with self.assertRaises(RuntimeError):
            reference.fixture_rows(row + "\n" + row)
        self.assertEqual(reference.edn_string('a"b\\c'), '"a\\"b\\\\c"')
        with self.assertRaises(RuntimeError):
            reference.edn_string("a\nb")


if __name__ == "__main__":
    unittest.main()
