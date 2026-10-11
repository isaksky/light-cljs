#!/usr/bin/env python3
"""Execute the named migration scope with legacy native execution fenced.

This is an execution gate, not a whole-language completion report. Historical
native adapter suites and parser acceptance are separate. Every subprocess must
succeed; ignored oracle/performance tests remain reported by the Rust harness.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[3]
REVISION = "a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts", type=Path)
    parser.add_argument("--reference", action="store_true", help="require focused pinned release + checkout comparisons")
    parser.add_argument("--checkout", type=Path)
    args = parser.parse_args()
    if args.reference and args.checkout is None:
        parser.error("--reference requires --checkout for the exact pinned source")
    artifacts = args.artifacts or Path(tempfile.mkdtemp(prefix="light-cljs-evaluator-gate-"))
    artifacts.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, LIGHT_CLJS_DISABLE_NATIVE_EVAL="1", PYTHONDONTWRITEBYTECODE="1")
    results = []

    def run(label, command):
        print(f"{label}: {' '.join(map(str, command))}", flush=True)
        log = artifacts / f"{label}.log"
        with log.open("w") as stream:
            result = subprocess.run(command, cwd=ROOT, env=env, stdout=stream, stderr=subprocess.STDOUT)
        results.append({"label": label, "command": list(map(str, command)), "exit_code": result.returncode, "log": str(log)})
        (artifacts / "results.json").write_text(json.dumps({"native_execution_disabled": True, "rust_min_stack_override": env.get("RUST_MIN_STACK"), "scope": "named migration suites only", "checks": results}, indent=2) + "\n")
        if result.returncode:
            print(log.read_text()[-18000:])
            raise SystemExit(f"{label} failed; retained diagnostics: {artifacts}")

    run("tools", ["python3", "-c", "import platform,subprocess; print(platform.platform()); [subprocess.run(c,check=True) for c in [['rustc','--version'],['node','--version']]]"])
    run("default-build", ["cargo", "check", "--locked"])
    run("all-feature-build", ["cargo", "check", "--locked", "--all-features"])
    # Discover every current compiler/evaluation integration suite. New suites
    # enter automatically; only the explicitly historical adapter API tests
    # have their own excluded target until the physical-removal step.
    targets = sorted(p.stem for p in (ROOT / "tests").glob("*.rs") if
                     p.stem.startswith(("compiler_", "eval")) and p.stem != "compiler_native_adapter")
    targets += ["analyzer", "boundaries", "cli", "execution", "namespaces", "provenance", "sci_reference"]
    command = ["cargo", "test", "--locked", "--no-fail-fast"]
    for target in targets:
        command += ["--test", target]
    run("consumers", command)
    for module in ["compiler_bootstrap::tests", "application::tests", "eval_compiler::tests", "active_tests", "compiler_pure_tests", "compiler_default_reader_tests", "program_fragment::tests"]:
        run("unit-" + module.replace("::", "-"), ["cargo", "test", "--locked", "--lib", module])
    for script in ["js_runtime", "js_core_helpers"]:
        run(script, ["node", f"tests/support/{script}.test.js"])
    for suite, pattern in [("compiler-language", "test_*.py"), ("sci", "test_regenerate.py")]:
        run("accounting-" + suite, ["python3", "-B", "-m", "unittest", "discover", "-s", "tests/reference/" + suite, "-p", pattern])
    if args.reference:
        run("inventory", ["python3", "-B", "tests/reference/compiler-language/extract_inventory.py", "--check"])
        env.update(LIGHT_CLJS_REFERENCE="both", LIGHT_CLJS_REFERENCE_CHECKOUT=str(args.checkout.resolve()), LIGHT_CLJS_REFERENCE_REVISION=REVISION)
        for test in ["compiled_helpers_match_pinned_values_effects_and_stdout", "compiled_sequences_match_pinned_values_effects_and_stdout"]:
            run(test, ["cargo", "test", "--locked", "--test", "execution", test, "--", "--ignored", "--exact", "--nocapture"])
        run("transducers-reference", ["cargo", "test", "--locked", "--test", "compiler_transducers", "eager_transducer_fixtures_match_pinned_release_and_checkout", "--", "--ignored", "--exact", "--nocapture"])
    print(f"Passed named migration checks; ignored tests are retained in logs: {artifacts}")


if __name__ == "__main__":
    main()
