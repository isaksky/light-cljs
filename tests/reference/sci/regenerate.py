#!/usr/bin/env python3
"""Explicit, pinned SCI oracle; check by default, --write to regenerate fixtures."""
import argparse
import os
import re
from pathlib import Path
import signal
import subprocess
import tempfile
import time

REVISION = "5fe89056384418be50d232b4278344cb3a1a526b"
ROOT = Path(__file__).resolve().parents[3]
FIXTURES = ROOT / "tests/fixtures/sci-outcomes.tsv"
DRIVER = Path(__file__).with_name("driver.clj")
METADATA = Path(__file__).with_name("runtime.tsv")
OPTIONS = "{:features #{:clj}, :read-eval false, :user [observe], :out :captured, :err :captured}"


def checked_process(command, cwd, directory, timeout):
    """File-backed output and process-group cleanup also bound a hung launcher."""
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "invocation.txt").write_text(repr(command) + "\ncwd=" + str(cwd) + "\n")
    with (directory / "stdout.txt").open("wb") as out, (directory / "stderr.txt").open("wb") as err:
        try:
            child = subprocess.Popen(command, cwd=cwd, stdin=subprocess.DEVNULL,
                                     stdout=out, stderr=err, start_new_session=True)
        except OSError as error:
            raise RuntimeError(f"Required tool unavailable; artifacts: {directory}: {error}") from error
        try:
            child.wait(timeout=timeout)
        except subprocess.TimeoutExpired as error:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait()
            raise RuntimeError(f"Reference timed out after {timeout}s; artifacts: {directory}") from error
    if child.returncode:
        raise RuntimeError(f"Reference tool failed ({child.returncode}); artifacts: {directory}")
    return (directory / "stdout.txt").read_text()


def checked_checkout(checkout, artifacts):
    checkout = checkout.resolve(strict=True)
    def git(*args):
        return checked_process(["git", "-C", str(checkout), *args], ROOT,
                               artifacts / ("git-" + args[0] + "-" + str(time.monotonic_ns())), 15).strip()
    if git("rev-parse", "--show-toplevel") != str(checkout):
        raise RuntimeError("SCI checkout must be an explicit repository root")
    if git("rev-parse", "HEAD") != REVISION:
        raise RuntimeError(f"SCI checkout revision mismatch: require {REVISION}")
    if git("status", "--porcelain", "--untracked-files=all"):
        raise RuntimeError("SCI checkout must be clean, including untracked files")
    if not (checkout / "deps.edn").is_file():
        raise RuntimeError("SCI checkout deps.edn is missing")
    return checkout


def fixture_rows(text):
    rows = [line.split("\t") for line in text.splitlines() if line and not line.startswith("#")]
    if not rows or any(len(row) != 7 for row in rows):
        raise RuntimeError("Expected nonempty seven-column SCI fixture corpus")
    if any(not re.fullmatch(r"sci-[a-z0-9-]+", row[0])
           or row[1] not in {"portable", "bounded-number", "reader-difference", "reader-bb", "reader-tags", "reader-fallback", "loading", "state-validators"}
           or not row[2] for row in rows):
        raise RuntimeError("Invalid SCI fixture identity/profile/source")
    if len({row[0] for row in rows}) != len(rows):
        raise RuntimeError("Duplicate SCI fixture IDs")
    return rows


def checked_results(output, rows):
    results = [line.split("\t") for line in output.splitlines()]
    if any(len(row) != 5 for row in results):
        raise RuntimeError("Malformed reference output: expected five columns per result")
    wanted = [row[0] for row in rows]
    actual = [row[0] for row in results]
    if actual != wanted:
        raise RuntimeError(f"Missing, duplicate, extra, or reordered reference cases: expected {wanted}, got {actual}")
    if any(not value for row in results for value in row):
        raise RuntimeError("Empty reference result field")
    return [source[:3] + result[1:] for source, result in zip(rows, results)]


def checked_metadata(text):
    rows = [line.split("\t") for line in text.splitlines()]
    if any(len(row) != 2 for row in rows) or len(dict(rows)) != len(rows):
        raise RuntimeError("Malformed or duplicate reference metadata")
    metadata = dict(rows)
    expected = {"format": "sci-outcomes-v1", "revision": REVISION, "clojure": "1.12.4", "options": OPTIONS}
    if any(metadata.get(key) != value for key, value in expected.items()):
        raise RuntimeError("Missing or mismatched reference runtime/options metadata")
    if not metadata.get("java") or not metadata.get("java-vendor"):
        raise RuntimeError("Missing Java runtime identity")
    return metadata


def edn_string(text):
    if any(ord(c) < 32 for c in text):
        raise RuntimeError("Checkout path must not contain control characters")
    return '"' + text.replace('\\', '\\\\').replace('"', '\\"') + '"'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checkout", type=Path, required=True,
                        help=f"Clean SCI source checkout at {REVISION}; no release fallback")
    parser.add_argument("--fixtures", type=Path, default=FIXTURES, help="Existing sci-outcomes-v1 corpus, including exported seed/reduction replay files")
    parser.add_argument("--evaluator", action="store_true", help="After exact SCI comparison, execute canonical evaluator integration suites with the same timeout and crash enforcement")
    parser.add_argument("--write", action="store_true", help="Update committed fixtures and runtime.tsv after all cases succeed")
    parser.add_argument("--timeout", type=float, default=180, help="Positive JVM process timeout in seconds (default 180)")
    args = parser.parse_args()
    if not 0 < args.timeout < float("inf"):
        parser.error("--timeout must be finite and positive")
    artifacts = Path(tempfile.mkdtemp(prefix="light-cljs-sci-reference-"))
    print(f"SCI reference artifacts: {artifacts}", flush=True)
    checkout = checked_checkout(args.checkout, artifacts)
    fixture_path = args.fixtures.resolve(strict=True)
    if args.evaluator and fixture_path != FIXTURES:
        parser.error("--evaluator uses the committed corpus; run custom files as oracle-only comparisons")
    original = fixture_path.read_text()
    rows = fixture_rows(original)
    # An empty cwd plus -Srepro prevents project/user aliases from replacing the oracle.
    work = artifacts / "work"
    work.mkdir()
    deps = '{:deps {org.clojure/clojure {:mvn/version "1.12.4"} org.babashka/sci {:local/root ' + edn_string(str(checkout)) + '}}}'
    input_path = artifacts / "input.tsv"
    input_path.write_text(original)
    metadata_path = artifacts / "runtime.tsv"
    command = [os.environ.get("CLOJURE", "clojure"), "-Srepro", "-Sdeps", deps,
               "-M", str(DRIVER), str(checkout), str(input_path), str(metadata_path)]
    output = checked_process(command, work, artifacts / "driver", args.timeout)
    metadata = metadata_path.read_text()
    checked_metadata(metadata)
    # Check again so a concurrently changed checkout cannot be recorded as pinned.
    checked_checkout(checkout, artifacts)
    actual = checked_results(output, rows)
    if fixture_path.read_text() != original:
        raise RuntimeError("Fixture source changed during reference execution; refusing replacement")
    if args.write:
        header = "\n".join(line for line in original.splitlines() if line.startswith("#"))
        fixture_path.write_text(header + "\n" + "\n".join("\t".join(row) for row in actual) + "\n")
        if fixture_path == FIXTURES:
            METADATA.write_text(metadata)
        print(f"Wrote {len(rows)} verified SCI cases and runtime/options metadata")
    else:
        for expected, result in zip(rows, actual):
            if expected != result:
                raise RuntimeError(f"SCI mismatch for {expected[0]} ({expected[2]}):\nexpected {expected[3:]}\nactual {result[3:]}")
        print(f"Matched all {len(rows)} SCI reference cases; runtime/options in {metadata_path}")
    if args.evaluator:
        targets = ["evaluator_conformance", "eval_sessions", "eval_cli", "sci_reference", "provenance"]
        command = ["cargo", "test", "--locked"]
        for target in targets:
            command.extend(["--test", target])
        evaluator_output = checked_process(command, ROOT, artifacts / "evaluator", args.timeout)
        print(evaluator_output, end="")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError) as error:
        raise SystemExit(str(error)) from error
