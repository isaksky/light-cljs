#!/usr/bin/env python3
"""Original SCI declarations, user namespace and source-coordinate adapters.

The source catalog preserves immutable original expectations. Reference-only JVM
macro declarations are evaluated sequentially, with original application forms
analyzed in order in user. Production macros remain canonical JavaScript.
"""
from __future__ import annotations
import argparse, collections, json, os, signal, subprocess
from pathlib import Path
import sci_execution as sci
from extract_inventory import Reader, head

HERE = Path(__file__).resolve().parent
CATALOG = HERE / "sci-source-context-catalog.jsonl"
DRIVER = "tests/compiler_sci_source_context.rs"
TEST = "retained_sources_execute_with_original_namespace_and_positions"
ORACLE = "tests/reference/compiler-language/sci_source_context.clj"
CLASSES = {'macro-declaration-context': ['sci-macro-raw-arguments', 'sci-macro-twice-effects', 'sci-macro-env-runtime', 'sci-macro-form', 'sci-macro-local-shadow', 'sci-macro-qualified', 'sci-macro-var-meta', 'sci-macro-variadic', 'sci-macro-multi-arity', 'sci-macro-redefinition', 'sci-macro-emit-defs', 'sci-macro-nested-top-do', 'sci-macro-one-step', 'sci-macro-all-steps', 'sci-macro-not-recursive', 'sci-macro-quoted-no-expansion', 'sci-macro-collection-result', 'sci-macro-nested-meta-override', 'sci-macro-definition-env', 'sci-macro-multi-attrs', 'sci-syntax-quote-gensym-macro-capture', 'sci-syntax-quote-macro-vector', 'sci-syntax-quote-macro-splice', 'sci-syntax-quote-macro-fn', 'sci-syntax-quote-gensym-later-call', 'sci-binding-macro-destructured', 'sci-deferred-lazy-macro-nested', 'sci-deferred-lazy-macro-expands-macro', 'sci-vars-macro-analysis-scope', 'sci-example-custom-macro'], 'namespace-and-source-metadata': ['sci-syntax-quote-symbol', 'sci-syntax-quote-specials', 'sci-syntax-quote-nested-symbol', 'sci-syntax-quote-dotted', 'sci-syntax-quote-qualified', 'sci-syntax-quote-metadata-symbol', 'sci-syntax-quote-metadata-vector', 'sci-syntax-quote-metadata-replace', 'sci-syntax-quote-quote-qualified-local', 'sci-binding-defn-arglist-conditions', 'sci-binding-defn-location', 'sci-string-symbol-var'], 'pinned-version-difference': ['sci-binding-default-effects', 'sci-binding-sibling-default', 'sci-binding-key-default', 'sci-binding-map-as-default', 'sci-binding-initializer-order', 'sci-binding-map-ordered', 'sci-binding-symbol-key-default', 'sci-binding-string-key-default']}


def generated_catalog():
    corpus, dispositions = sci.rows(sci.CORPUS), sci.rows(sci.DISPOSITIONS)
    result = []
    for category, identities in CLASSES.items():
        for identity in identities:
            original_row, original = corpus[identity]
            disposition_row, disposition = dispositions[identity]
            if original != disposition[:7]:
                raise ValueError("Original SCI fields changed: " + identity)
            source = original[2]
            forms = []
            for node in Reader(source).all():
                kind = "macro" if head(node) == "defmacro" else "runtime"
                if identity == "sci-vars-macro-analysis-scope" and head(node) == "def":
                    kind = "host-root"
                forms.append({"kind": kind, "source": source[node.start:node.end],
                              "utf8_byte_start": len(source[:node.start].encode()),
                              "utf8_byte_end": len(source[:node.end].encode())})
            result.append({"id": "sci-execution-" + identity, "sci_id": identity,
                "classification": category, "source": source,
                "source_sha256": sci.digest(source), "original_source_sha256": sci.digest(source),
                "original_row_sha256": sci.digest(original_row), "body_sha256": sci.digest(disposition_row),
                "obligation_id": "local::tests/reference/sci/evaluator-dispositions.tsv::" + identity,
                "expected": sci.expected(original), "namespace": "user", "source_name": "sci-original.cljs",
                "forms": forms, "reference_setup": sci.REFERENCE_SETUP,
                "setup": "Original user namespace and source coordinates; namespace-global observe callback; original top-level forms retain their order. Reference-only JVM macro declarations are executed at compiler phase. The ordinary x root in sci-vars-macro-analysis-scope is separately initialized in reference macro/application heaps.",
                "comparison": sci.PROJECTION,
                "canonical_contract": None})
    return result


def load():
    rows = [json.loads(line) for line in CATALOG.read_text().splitlines() if line]
    if json.dumps(rows, sort_keys=True) != json.dumps(generated_catalog(), sort_keys=True):
        raise ValueError("SCI source-context catalog stale or incomplete")
    return rows


def cases(catalog=None):
    return [{"id": row["id"], "source": row["source"], "expected": row["expected"]}
            for row in (load() if catalog is None else catalog)]


def expression_rows(rows, contexts=None):
    """Specialize matching originals without dropping an unrelated expression."""
    contexts = load() if contexts is None else contexts
    by_id = {row["id"]: row for row in contexts}
    for row in rows:
        context = by_id.get(row["id"])
        if context is None:
            continue
        keys = ("obligation_id", "original_source_sha256", "original_row_sha256", "body_sha256", "expected")
        if any(json.dumps(row.get(key), sort_keys=True) != json.dumps(context[key], sort_keys=True) for key in keys):
            raise ValueError("SCI source specialization detached from original expression: " + row["id"])
    return [row for row in rows if row["id"] not in by_id]


def compiler_sources():
    paths = sorted(path for path in (sci.ROOT / "src").rglob("*") if path.is_file())
    paths += [sci.ROOT / name for name in ("Cargo.toml", "Cargo.lock", "build.rs")
              if (sci.ROOT / name).is_file()]
    return {str(path.relative_to(sci.ROOT)): sci.digest(path.read_bytes()) for path in paths}


def adapter_hash():
    return sci.digest(Path(__file__).read_bytes())


def local_cases(rows, directory, timeout):
    production_hashes = compiler_sources()
    adapter_source_hash = adapter_hash()
    directory.mkdir(parents=True, exist_ok=True)
    source, output = directory / "input.tsv", directory / "observations.tsv"
    source.write_text("".join(row["id"] + "\t" + row["source"].encode().hex() + "\n" for row in rows))
    environment = {**os.environ, "LIGHT_CLJS_SCI_CONTEXT_INPUT": str(source.resolve()),
                   "LIGHT_CLJS_SCI_CONTEXT_OUTPUT": str(output.resolve())}
    command = ["cargo", "test", "--locked", "--test", "compiler_sci_source_context", TEST,
               "--", "--ignored", "--exact", "--nocapture"]
    runner = sci.report_module()
    try:
        process = subprocess.Popen(command, cwd=sci.ROOT, env=environment, text=True,
                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        try:
            stdout, stderr = process.communicate(timeout=timeout)
            timed_out = False
        except subprocess.TimeoutExpired:
            timed_out = True; os.killpg(process.pid, signal.SIGTERM)
            try: stdout, stderr = process.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL); stdout, stderr = process.communicate()
    except OSError as error:
        return {row["id"]: {"status": "unavailable", "detail": str(error)} for row in rows}
    (directory / "cargo.stdout").write_text(stdout); (directory / "cargo.stderr").write_text(stderr)
    if timed_out or process.returncode or "running 1 test" not in stdout or "test result: ok. 1 passed; 0 failed; 0 ignored;" not in stdout or not output.is_file():
        return {row["id"]: {"status": "timeout" if timed_out else "failed"} for row in rows}
    if compiler_sources() != production_hashes or adapter_hash() != adapter_source_hash:
        return {row["id"]: {"status": "source-drift"} for row in rows}
    raw = [line.split("\t") for line in output.read_text().splitlines()]
    if [line[0] for line in raw] != [row["id"] for row in rows]:
        raise ValueError("missing, extra, duplicate or reordered local source context observations")
    result = {}
    for row, fields in zip(rows, raw):
        if len(fields) != 4 or fields[1] != "executed":
            result[row["id"]] = {"status": "rejected", "detail": fields[2:]}; continue
        layers = {}
        for engine, observation in zip(("Node", "QuickJS"), fields[2:]):
            try: layers[engine] = sci.verify(row, {"status": "completed", "observed": json.loads(observation)}, runner.same_json)
            except ValueError: layers[engine] = {"status": "invalid-output", "detail": observation}
        result[row["id"]] = {"status": "passed" if all(x["status"] == "passed" for x in layers.values()) else "mismatch",
            "engines": layers, "source_sha256": row["source_sha256"], "test_file": DRIVER,
            "test_name": TEST, "test_source_sha256": sci.digest((sci.ROOT / DRIVER).read_bytes()),
            "adapter_source_sha256": sci.digest((sci.ROOT / "tests/support/sci_execution_observation.js").read_bytes()),
            "source_context_adapter_sha256": adapter_source_hash,
            "compiler_source_sha256": production_hashes}
    return result


def reference_cases(rows, kind, directory, timeout):
    directory.mkdir(parents=True, exist_ok=True)
    input_path = directory / "originals.tsv"
    records = []
    for row in rows:
        forms = "[" + " ".join("[" + json.dumps(form["kind"]) + " " + json.dumps(form["source"], ensure_ascii=True) + "]" for form in row["forms"]) + "]"
        records.append(row["id"] + "\t" + row["source"].encode().hex() + "\t" + forms.encode().hex() + "\n")
    input_path.write_text("".join(records))
    runner = sci.report_module()
    identity = runner.RELEASE if kind == "release" else runner.REVISION
    checkout = sci.ROOT / "research/clojurescript"
    dependency = '{:mvn/version "' + runner.RELEASE + '"}' if kind == "release" else "{:local/root " + json.dumps(str(checkout)) + "}"
    command = [os.environ.get("CLOJURE", "clojure"), "-Srepro", "-Sdeps", "{:paths [] :deps {org.clojure/clojurescript " + dependency + "}}",
               "-M", ORACLE, kind, identity, "-" if kind == "release" else str(checkout), str(directory.resolve()), str(input_path.resolve())]
    built = runner.invoke(command, directory / "build", timeout=timeout)
    settings = "REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}"
    if built["status"] != "completed" or "REFERENCE IDENTITY " + kind + " " + identity not in built.get("stdout", "") or settings not in built.get("stdout", ""):
        return {row["id"]: {"status": built["status"] if built["status"] != "completed" else "invalid-reference", "detail": built} for row in rows}
    records = [line.split("\t") for line in (directory / "contexts.tsv").read_text().splitlines()]
    if [line[0] for line in records] != [row["id"] for row in rows]:
        raise ValueError("Reference source context identities changed")
    result = {}
    for row, record in zip(rows, records):
        provenance = {"reference_identity": kind + " " + identity,
            "settings": {"optimizations": "none", "static-fns": False, "def-emits-var": False},
            "source_sha256": row["source_sha256"],
            "oracle_source_sha256": sci.digest((sci.ROOT / ORACLE).read_bytes()),
            "oracle_helper_source_sha256": sci.digest((sci.ROOT / "tests/reference/execution.clj").read_bytes()),
            "observation_source_sha256": sci.digest((sci.ROOT / "tests/support/execution.js").read_bytes())}
        if record[1] != "compiled":
            result[row["id"]] = {"status": "rejected", "detail": record[2:], **provenance}; continue
        script = "process.chdir(" + json.dumps(str(directory.resolve())) + ");\nrequire(" + json.dumps(str((directory / "reference.js").resolve())) + ");\n" + (directory / "case-js" / (row["id"] + ".js")).read_text() + "\n" + (sci.ROOT / "tests/support/execution.js").read_text()
        ran = runner.invoke([os.environ.get("NODE", "node"), "-"], directory / row["id"], source=script, timeout=timeout)
        if ran["status"] != "completed":
            result[row["id"]] = ran; continue
        observed = runner.observations(ran["stdout"], [row])[row["id"]]
        layer = sci.verify(row, observed, runner.same_json)
        frames = [line for line in ran["stderr"].splitlines() if line.startswith("CASE START ") or line.startswith("CASE END ")]
        extras = [line for line in ran["stderr"].splitlines() if line not in frames]
        if len(frames) != 2 or not frames[0].startswith("CASE START ") or not frames[1].startswith("CASE END ") or json.loads(frames[0][11:])["id"] != row["id"] or json.loads(frames[1][9:])["id"] != row["id"]:
            layer.update(status="invalid-output", detail="Missing or invalid source stderr identity frames")
        if extras: layer.update(status="mismatch", detail="Original empty stderr differs", stderr=extras)
        if len(record) != 4 or record[3] != "[]":
            layer.update(status="phase-effects-require-reconciliation", phase_effects=record[3:] if len(record) > 3 else None)
        layer.update(**provenance, reference_phase_effects=record[3:] if len(record)>3 else None)
        result[row["id"]] = layer
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write-catalog", action="store_true")
    parser.add_argument("--ids")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--timeout", type=int, default=240)
    parser.add_argument("--local", action="store_true")
    parser.add_argument("--references", action="store_true")
    args = parser.parse_args()
    if args.write_catalog: CATALOG.write_text("".join(json.dumps(row, separators=(",", ":")) + "\n" for row in generated_catalog()))
    rows = load()
    if args.ids:
        wanted = set(args.ids.split(",")); rows = [row for row in rows if row["sci_id"] in wanted]
        if {row["sci_id"] for row in rows} != wanted: raise ValueError("Unknown source context identity")
    print("Validated original source contexts:", len(rows), flush=True)
    if not (args.local or args.references): return
    if args.output is None: parser.error("execution requires --output")
    evidence = {row["id"]: {} for row in rows}
    if args.local:
        observed = local_cases(rows, args.output / "local", args.timeout)
        for identity, layer in observed.items(): evidence[identity]["javascript"] = layer
        print("local", dict(collections.Counter(x["status"] for x in observed.values())), flush=True)
    if args.references:
        for kind in ("release", "checkout"):
            observed = reference_cases(rows, kind, args.output / kind, args.timeout)
            for identity, layer in observed.items(): evidence[identity][kind] = layer
            print(kind, dict(collections.Counter(x["status"] for x in observed.values())), flush=True)
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / "evidence.json").write_text(json.dumps({"catalog_sha256": sci.digest(CATALOG.read_bytes()), "evidence": evidence}, indent=2) + "\n")

if __name__ == "__main__": main()
