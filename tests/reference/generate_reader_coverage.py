#!/usr/bin/env python3
"""Pin complete reader sources and body byte spans, without running a reader oracle."""
import argparse
import hashlib
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE / "compiler-language"))
from extract_inventory import Reader, REVISION, short_head

FILES = (
    "src/test/cljs/cljs/reader_test.cljs",
    "src/test/cljs/cljs/syntax_quote_test.cljs",
    "src/test/cljs/cljs/tagged_literals_test.cljs",
    "src/test/cljs/cljs/metadata_test.cljc",
    "src/test/cljs/data_readers.cljc",
    "src/main/clojure/cljs/vendor/clojure/tools/reader.clj",
    "src/main/clojure/cljs/vendor/clojure/tools/reader/edn.clj",
)
CORPUS = HERE / "reader-sources.tsv"
HARNESS = """(def assertions (atom 0))
(defmacro is [x] (list 'do '(swap! assertions inc) (list 'assert x)))
(defmacro deftest [name & body] (list 'defn name [] (cons 'do body)))
"""


def bodies(data):
    text = data.decode("utf-8")
    return text, Reader(text).all()


def spans(data):
    text, forms = bodies(data)
    return ",".join(f"{len(text[:f.start].encode('utf-8'))}:{len(text[:f.end].encode('utf-8'))}" for f in forms)


def generated(sources):
    rows = ["# Complete EPL-1.0 upstream sources; parser acceptance is not execution.",
            "# https://github.com/clojure/clojurescript", f"# revision {REVISION}",
            "# path\tsha256\ttop-level UTF-8 byte spans\tcomplete source hex"]
    for path in FILES:
        data = sources[path]
        rows.append(f"{path}\t{hashlib.sha256(data).hexdigest()}\t{spans(data)}\t{data.hex()}")
    result = {CORPUS: ("\n".join(rows) + "\n").encode()}
    for file, namespace in [("syntax_quote_test.cljs", "cljs.syntax-quote-test"),
                            ("tagged_literals_test.cljs", "cljs.tagged-literals-test")]:
        text, forms = bodies(sources[f"src/test/cljs/cljs/{file}"])
        tests = [f for f in forms if short_head(f) == "deftest"]
        # Retain the original license and every complete test body. Only the
        # upstream test harness/imports are replaced with executable local ones.
        prefix = text[:text.index("(ns ")]
        requires = "[cljs.test :as test]" if file.startswith("syntax") else "[cljs.reader :as reader] [clojure.set :as set]"
        setup = "" if file.startswith("syntax") else """(reader/register-tag-parser! 'cljs/tag identity)
(reader/register-tag-parser! 'cljs/inc inc)
(reader/register-tag-parser! 'cljs/union set/union)
"""
        program = prefix + f";; Generated from {REVISION}; complete bodies, local harness.\n(ns {namespace} (:require {requires}))\n" + HARNESS + setup
        program += "\n\n".join(text[f.start:f.end] for f in tests) + "\n"
        program += " ".join(f"({f.children[1].token})" for f in tests) + "\n@assertions\n"
        result[ROOT / f"tests/fixtures/reader-coverage/{namespace.replace('.', '/').replace('-', '_')}.cljs"] = program.encode()
    result[ROOT / "tests/fixtures/reader-coverage/cljs/test.cljs"] = b";; Alias target for unchanged syntax-quote bodies; no upstream test runtime.\n(ns cljs.test)\n"
    return result


def stored_sources():
    rows = [line.split("\t") for line in CORPUS.read_text().splitlines() if not line.startswith("#")]
    assert tuple(row[0] for row in rows) == FILES
    sources = {}
    for path, digest, locations, hex_source in rows:
        data = bytes.fromhex(hex_source)
        assert hashlib.sha256(data).hexdigest() == digest, path
        assert spans(data) == locations, path
        sources[path] = data
    # The older complete reader execution fixture must retain every original
    # non-namespace setup/test form, not only a subset of assertion expressions.
    text, forms = bodies(sources[FILES[0]])
    fixture = (ROOT / "tests/fixtures/compiler-reader-upstream.cljs").read_text()
    for form in forms:
        if short_head(form) != "ns":
            assert text[form.start:form.end] in fixture, text[form.start:form.end][:80]
    return sources


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="compare regeneration from pinned Git sources")
    parser.add_argument("--verify", action="store_true", help="verify committed source hashes/spans/fixtures; no research or Java")
    args = parser.parse_args()
    if args.verify:
        sources = stored_sources()
    else:
        checkout = ROOT / "research/clojurescript"
        sources = {p: subprocess.check_output(["git", "-C", str(checkout), "show", f"{REVISION}:{p}"]) for p in FILES}
    outputs = generated(sources)
    for path, data in outputs.items():
        if args.check or args.verify:
            assert path.read_bytes() == data, f"Regenerate {path}"
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    print(f"reader sources: {len(sources)} files, {sum(len(Reader(v.decode()).all()) for v in sources.values())} bodies, {sum(map(len, sources.values()))} bytes; oracle unexecuted")


if __name__ == "__main__":
    main()
