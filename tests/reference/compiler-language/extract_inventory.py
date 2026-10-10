#!/usr/bin/env python3
"""Lossless source inventory; never evaluates or declares upstream tests passing.

Run from any directory with Python 3.10+; --check regenerates in memory and
compares every byte. See README.md for the counting and source-scope contract.
"""

from __future__ import annotations

import argparse
from bisect import bisect_right
from collections import Counter
from dataclasses import dataclass
import hashlib
import json
import re
from pathlib import Path
import subprocess
import sys


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
UPSTREAM = ROOT / "research/clojurescript"
REVISION = "a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e"
FILES = (
    "src/test/clojure/cljs/analyzer_tests.clj",
    "src/test/clojure/cljs/compiler_tests.clj",
    "src/test/cljs/cljs/core_test.cljs",
    "src/test/cljs/cljs/new_new_test.cljs",
    "src/test/cljs/cljs/collections_test.cljs",
    "src/test/cljs/cljs/hash_map_test.cljs",
    "src/test/cljs/cljs/map_entry_test.cljs",
    "src/test/cljs/cljs/seqs_test.cljs",
    "src/test/cljs/cljs/chunked_seq.cljs",
    "src/test/cljs/cljs/primitives_test.cljs",
    "src/test/cljs/cljs/apply_test.cljs",
    "src/test/cljs/cljs/recur_test.cljs",
    "src/test/cljs/cljs/destructuring_test.cljs",
    "src/test/cljs/cljs/letfn_test.cljs",
    "src/test/cljs/cljs/binding_test.cljs",
    "src/test/cljs/cljs/var_test.cljs",
    "src/test/cljs/cljs/extend_to_native_test.cljs",
    "src/test/cljs/cljs/metadata_test.cljc",
)
FEATURES = (
    "functions-apply", "recursion", "bindings-control", "mutable-state",
    "protocols-types-records", "reader-metadata", "exceptions", "core-helpers",
    "lazy-sequences", "javascript-interop", "namespaces",
)
ASSERTIONS = {"is", "are", "assert", "defspec"}
TEST_FORMS = {"deftest", "defspec"}
DECLARATIONS = TEST_FORMS | {
    "def", "defn", "defn-", "defmacro", "defprotocol", "defrecord", "deftype",
    "defmulti", "defmethod", "ns", "defonce",
}


@dataclass
class Node:
    kind: str
    start: int
    end: int
    children: list[Node]
    token: str = ""


class Reader:
    """Structural, non-evaluating reader retaining every source character span.

    This intentionally does not macroexpand, resolve names, normalize expected
    values, or silently drop reader-discarded forms. Reader conditionals retain
    every branch, with inactive branches identified by the assertion walker.
    """

    def __init__(self, text: str):
        self.text = text
        self.i = 0

    def space(self):
        while self.i < len(self.text):
            c = self.text[self.i]
            if c.isspace() or c == ",":
                self.i += 1
            elif c == ";" or self.text.startswith("#!", self.i):
                p = self.text.find("\n", self.i)
                self.i = len(self.text) if p < 0 else p + 1
            else:
                break

    def all(self) -> list[Node]:
        result = []
        self.space()
        while self.i < len(self.text):
            result.append(self.form())
            self.space()
        return result

    def form(self) -> Node:
        self.space()
        start = self.i
        if start >= len(self.text):
            raise ValueError("Expected a form at end of source")
        c = self.text[self.i]
        if c in ")]}":
            raise ValueError(f"Unexpected closing delimiter {c!r} at {self.i}")
        if c in "([{":
            close = {"(": ")", "[": "]", "{": "}"}[c]
            self.i += 1
            children = []
            while True:
                self.space()
                if self.i >= len(self.text):
                    raise ValueError(f"Unclosed {c!r} at {start}")
                if self.text[self.i] == close:
                    self.i += 1
                    return Node({"(": "list", "[": "vector", "{": "map"}[c],
                                start, self.i, children)
                children.append(self.form())
        if c == '"' or self.text.startswith('#"', self.i):
            self.i += 1 if c == '"' else 2
            while self.i < len(self.text):
                d = self.text[self.i]
                self.i += 1
                if d == "\\":
                    self.i += 1
                elif d == '"':
                    return Node("string" if c == '"' else "regex", start, self.i, [])
            raise ValueError(f"Unclosed string/regex at {start}")
        # A character literal can itself contain a delimiter (e.g. \\) or \\;).
        if c == "\\":
            self.i += 2
            while self.i < len(self.text) and not self.delimiter(self.text[self.i]):
                self.i += 1
            return Node("character", start, self.i, [], self.text[start:self.i])
        prefixes = (("#?@", "conditional-splice", 1), ("#?", "conditional", 1),
                    ("#_", "discard", 1), ("#'", "var", 1), ("~@", "unquote-splice", 1),
                    ("'", "quote", 1), ("`", "syntax-quote", 1), ("~", "unquote", 1),
                    ("@", "deref", 1), ("^", "metadata", 2))
        for prefix, kind, arity in prefixes:
            if self.text.startswith(prefix, self.i):
                self.i += len(prefix)
                children = [self.form() for _ in range(arity)]
                return Node(kind, start, self.i, children, prefix)
        if c == "#":
            self.i += 1
            if self.i < len(self.text) and self.text[self.i] in "({":
                child = self.form()
                return Node("fn-literal" if child.kind == "list" else "set",
                            start, self.i, [child])
            # ##Inf/##NaN are symbolic values, not tagged reader forms.
            if self.i < len(self.text) and self.text[self.i] == "#":
                self.i += 1
                while self.i < len(self.text) and not self.delimiter(self.text[self.i]):
                    self.i += 1
                return Node("symbolic-value", start, self.i, [], self.text[start:self.i])
            tag_start = self.i
            while self.i < len(self.text) and not self.delimiter(self.text[self.i]):
                self.i += 1
            tag = self.text[tag_start:self.i]
            if not tag:
                raise ValueError(f"Unknown dispatch form at {start}")
            child = self.form()
            return Node("tagged", start, self.i, [child], tag)
        while self.i < len(self.text) and not self.delimiter(self.text[self.i]):
            self.i += 1
        if self.i == start:
            raise ValueError(f"Unknown reader form at {start}")
        return Node("atom", start, self.i, [], self.text[start:self.i])

    @staticmethod
    def delimiter(c: str) -> bool:
        # Apostrophes and # are legal inside symbols such as foo2' and x#.
        return c.isspace() or c in ',()[]{}";@^`~\\'


def effective(node: Node) -> Node:
    while node.kind in {"metadata", "discard"}:
        node = node.children[-1]
    return node


def head(node: Node) -> str:
    node = effective(node)
    if node.kind != "list" or not node.children:
        return ""
    first = effective(node.children[0])
    return first.token if first.kind == "atom" else ""


def short_head(node: Node) -> str:
    return head(node).rsplit("/", 1)[-1]


def nodes(node: Node):
    yield node
    for child in node.children:
        yield from nodes(child)


def symbols(node: Node) -> set[str]:
    return {n.token.rsplit("/", 1)[-1] for n in nodes(node) if n.kind == "atom"}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


class Source:
    def __init__(self, path: str, data: bytes):
        self.path = path
        self.data = data
        self.text = data.decode("utf-8")
        self.forms = Reader(self.text).all()
        self.byte_offsets = [0]
        self.lines = [0]
        for i, c in enumerate(self.text):
            self.byte_offsets.append(self.byte_offsets[-1] + len(c.encode("utf-8")))
            if c == "\n":
                self.lines.append(i + 1)

    def span(self, start: int, end: int) -> dict:
        data = self.data[self.byte_offsets[start]:self.byte_offsets[end]]
        return {"start_line": bisect_right(self.lines, start),
                "end_line": bisect_right(self.lines, max(start, end - 1)),
                "start_byte": self.byte_offsets[start], "end_byte": self.byte_offsets[end],
                "sha256": digest(data), "text": data.decode("utf-8")}

    def record(self, node: Node) -> dict:
        return self.span(node.start, node.end)


# Only contracts specific to upstream compiler internals are excluded here.
# Lack of light-cljs support is NEVER an exclusion rule. Source-language errors,
# warnings, AST-backed namespace semantics, new checkout syntax and lazy behavior
# remain in scope. The complete enclosing forms are retained in every case.
INTERNAL_TESTS = {
    "analyzer_tests.clj": {
        "test-cljs-975": "Direct desugar-ns-specs helper output: exact intermediate require/require-macros forms and reload placement; no source namespace is analyzed.",
        "test-rewrite-cljs-aliases": "Direct rewrite-cljs-aliases helper output: exact synthetic cljs.spec.alpha alias forms and list layout; no source namespace is analyzed.",
        "test-cljs-1763": "Direct parse-ns-excludes helper output: exact empty :excludes/:renames schema and set representation.",
        "test-cljs-2292": "Direct parse-ns-excludes helper output: exact :excludes/:renames schema and set representation for a rename.",
        "test-parse-global-refer": "Direct parse-global-refer-spec helper output: exact internal :use/:rename map schema; no source namespace is analyzed.",
        "test-parse-require-global": "Direct parse-global-require-spec helper output: exact internal :require/:use/:rename map schema and helper ABI.",
        "test-canonicalize-specs": "Direct canonicalize-specs helper output: exact intermediate unquoting and vector/list normalization, not require execution.",
        "test-canonicalize-import-specs": "Direct canonicalize-import-specs helper output: exact intermediate unquoting and vector/list normalization, not import execution.",
        "test-cljs-1992": "Upstream compiler namespace-cache :fn-var implementation bit after declare; no invocation or public Var metadata is asserted.",
        "no-warn": "Analyzer warning-handler suppression configuration API.",
        "all-warn": "Analyzer warning-handler enablement configuration API.",
        "test-cljs-1105": "Upstream constant-table identifier allocation API.",
        "test-symbols-munge-cljs-1432": "Upstream constant-table identifier allocation API.",
        "test-unicode-munging-cljs-1457": "Exact upstream generated constant identifier spelling.",
        "test-gen-user-ns": "Build filename SHA-derived synthetic namespace naming algorithm.",
        "test-cljs-1536": "Presence of upstream AST :var-ast implementation field.",
        "test-cljs-2023": "Upstream analyzer inferred :tag representation.",
        "test-cljs-2387": "Upstream analyzer namespace-cache :defs representation for an empty file.",
        "test-locals-mapped-to-sym": "Upstream analyzer API accepting manually constructed :locals environments.",
        "test-cljs-2814": "Exact generated names for npm/global-export module plumbing.",
        "test-cljs-2868": "Upstream return-type inference :tag representation.",
        "test-cljs-3120": "Upstream protocol signature-table metadata schema (:sigs fields).",
        "test-cljs-3133": "Upstream type-narrowing :tag set representation.",
        "test-cljs-3140": "Upstream type-narrowing :tag set representation.",
        "test-cljs-3158": "Upstream loop type-inference :tag representation.",
        "test-analyze-goog-ns": "Closure-library analyzer cache population API.",
        "test-analyze-goog-ns-ctor": "Closure-library constructor analyzer-cache metadata.",
        "test-cljs-3401": "Upstream constant-table identifier allocation API.",
    },
    "compiler_tests.clj": {
        "test-js-negative-infinity": "Exact emitted JavaScript expression spelling -Infinity via emit; no numeric runtime observation.",
        "test-cljs-2352": "Exact emitted JavaScript IEEE constant spellings NaN/Infinity/-Infinity from JVM Double values, not numeric runtime behavior.",
        "test-resolve-dotdot": "Direct resolve-var helper output: exact :name/:ns map schema for the core .. macro, not source macro invocation.",
        "test-goog-ctor-import-gen": "Exact emitted JavaScript constructor reference/return/new substrings for a Closure import; no constructor runtime observation.",
        "fn-scope-munge": "Exact emitted lexical/function identifier spelling via upstream munge API.",
        "test-munge-dotdot": "Exact upstream compiler munge API output spelling.",
        "test-cljs-428": "Generated JSDoc comment formatting and delimiter escaping.",
        "test-cljs-1643": "Emitter API rejection of a JVM function object as an internal constant.",
        "test-optimized-invoke-emit": "Exact optimized JavaScript code shape/dispatch names and temporary bindings.",
        "test-cljs-3077": "Exact JavaScript closure wrapper source formatting, not closure execution.",
        "emit-source-ns*-retains-ns-name": "Build emit-source return metadata for the output namespace.",
        "test-3368-global-shadowing": "Exact generated local variable suffix (__$1).",
        "test-externs-infer-is-nan": "Optimization-specific omission of truth_ after extern type inference.",
        "test-goog-lib-infer-boolean": "Closure-library inference optimization: omission of truth_.",
        "test-goog-module-infer-cljs-3438": "Closure-module inference optimization: omission of truth_.",
    },
    "destructuring_test.cljs": {
        "test-pam-dupes?": "Private PersistentArrayMap construction helper invoked through its Var.",
        "test-pam-new-size": "Private PersistentArrayMap storage-size helper invoked through its Var.",
    },
    "core_test.cljs": {
        "test-816": "clojure.set/rename-keys library behavior, outside the core compiler-language sections.",
        "test-cljs-1590": "clojure.string/split and split-lines library behavior.",
    },
}


FILE_FEATURE = {
    "apply_test.cljs": "functions-apply", "recur_test.cljs": "recursion",
    "letfn_test.cljs": "recursion", "binding_test.cljs": "bindings-control", "var_test.cljs": "mutable-state",
    "destructuring_test.cljs": "bindings-control",
    "extend_to_native_test.cljs": "protocols-types-records",
    "metadata_test.cljc": "reader-metadata",
}
TEST_FEATURES = {
    "mutable-state": """test-atoms-and-volatile test-delay test-919-generic-cas
        test-920-watch-ops-return-ref test-cljs-982-var-deref test-cljs-2549
        test-cljs-2552 swap-vals-returns-old-value deref-swap-arities
        deref-reset-returns-old-value reset-on-deref-reset-equality test-cljs-2933
        test-cljs-2934 test-cljs-3406 test-cljs-3472""".split(),
    "protocols-types-records": """test-hierarchy test-multimethods-1 test-multimethods-2
        test-cljs-3367-backward-conflict-prefers test-reify-meta tset-638
        test-cljs-1144 test-cljs-1451 test-cljs-1420 test-cljs-1780 test-cljs-2079
        test-cljs-2083 test-cljs-2133 test-cljs-2407 test-cljs-2787 test-cljs-2960
        test-cljs-3313 test-cljs-3305 test-cljs-3190 test-cljs-3371 test-cljs-1925
        no-warn-on-emit-invoke-protocol-method test-cljs-3120""".split(),
    "functions-apply": """test-fn-with-metadata test-383 test-477 test-580
        test-cljs-1225 test-cljs-1284 test-cljs-2741 test-cljs-3386 test-apply
        test-partial-and-comp test-cljs-2005 test-cljs-2863 test-defn-error
        test-cljs-1871 test-cljs-1992 fn-scope-munge test-optimized-invoke-emit""".split(),
    "recursion": """threading-loop-recur test-812 test-739 test-cljs-2475
        test-cljs-2476 test-cljs-3031 test-cljs-3158 test-cljs-3077""".split(),
    "bindings-control": """some->test some->>test cond->test cond->>test as->test
        test-401-411 test-letfn-shadowing test-case-keyword test-cljs-993
        test-1210 test-cljs-1536 test-const-emission test-cljs-2267 test-cljs-2449
        test-cljs-2283 test-1518 test-case test-constants test-3368-global-shadowing
        test-amp-as-local-clj-2954 or-doesnt-create-bindings keys-bang syms-bang
        strs-bang select-or-defaults""".split(),
    "reader-metadata": """test-metadata test-reader-literals test-symbol-meta
        test-853 test-807 test-921-var-meta-name test-cljs-1046 test-var-arglists
        tagged-literals test-1248 test-cljs-1569 test-cljs-2109 test-cljs-2959
        quote-args-error-test var-args-error-test test-unicode-munging-cljs-1457""".split(),
    "exceptions": "test-try-catch test-data-conveying-exceptions test-Throwable->map test-cljs-3473".split(),
    "lazy-sequences": """test-transducers test-into+halt-when test-map-new-transducers
        test-585 test-587 test-604 test-725 test-724 test-cljs-1209
        test-reductions-obeys-reduced test-cljs-1739 test-cljs-1744 test-cljs-1837
        test-cljs-2028 test-cljs-2113 test-cljs-2455 test-cljs-2799 test-cljs-3270
        test-cljs-3271 test-list-comprehensions test-inext""".split(),
    "javascript-interop": """test-js-primitives test-js-clj-conversions aget-test
        aset-test unchecked-get-test js-invoke-test memfn-test test-495 test-2764
        test-647 test-730 test-cljs-1839 test-cljs-1998 test-cljs-2190
        test-cljs-2278 test-cljs-2282 test-cljs-2585 test-cljs-2991 test-cljs-3395
        test-cljs-3452 test-static-props-methods test-new-method
        test-instance-method-new test-arrays test-make-array test-dot test-type
        test-instance? test-717 test-1556 test-cljs-1785-js-shadowed-by-local
        test-cljs-2148 test-js-negative-infinity test-cljs-2352""".split(),
    "namespaces": """spec-validation test-cljs-975 test-rewrite-cljs-aliases
        test-namespace-metadata test-cljs-1508-rename test-cljs-1274 test-cljs-1702
        test-cljs-1763 test-cljs-2292 test-parse-global-refer test-parse-require-global
        test-canonicalize-specs test-canonicalize-import-specs test-cljs-1346
        test-analyze-refer-global test-gen-user-ns test-cljs-2139 test-cljs-2037
        test-cljs-2182 test-cljs-2814 test-cljs-3239 test-cljs-3320 test-refer-global
        test-require-global test-cljs-2184 test-resolve test-cljs-3333 test-cljs-3334
        test-goog-ctor-import-gen test-resolve-dotdot emit-source-ns*-retains-ns-name""".split(),
}
TEST_FEATURE = {test: feature for feature, tests in TEST_FEATURES.items() for test in tests}
FEATURE_SYMBOLS = {
    "recursion": {"recur", "loop", "loop*", "letfn", "letfn*", "trampoline"},
    "protocols-types-records": {"defprotocol", "deftype", "defrecord", "reify", "extend-type",
                                "extend-protocol", "satisfies?", "implements?", "specify!", "defmulti", "defmethod"},
    "mutable-state": {"atom", "volatile!", "swap!", "swap-vals!", "reset!", "reset-vals!",
                      "compare-and-set!", "vreset!", "vswap!", "set!", "delay", "force", "add-watch"},
    "reader-metadata": {"meta", "with-meta", "vary-meta", "tagged-literal", "tagged-literal?", "var"},
    "bindings-control": {"let", "let*", "if", "if-some", "if-let", "when", "when-let", "case",
                         "cond", "condp", "binding", "with-redefs", "when-not", "doseq", "dotimes"},
    "lazy-sequences": {"lazy-seq", "lazy-cat", "range", "iterate", "repeat", "cycle", "for",
                       "map", "mapcat", "filter", "remove", "take", "drop", "take-while", "drop-while",
                       "concat", "reductions", "partition-all", "partition-by", "sequence", "eduction",
                       "transduce", "keep", "keep-indexed", "map-indexed", "interleave", "take-nth"},
    "functions-apply": {"fn", "fn*", "defn", "apply", "partial", "comp", "juxt", "constantly",
                        "complement", "some-fn", "every-pred"},
    "namespaces": {"ns", "require", "require-macros", "refer-clojure", "ns-publics", "ns-imports",
                   "resolve", "refer-global", "require-global"},
    "javascript-interop": {"js*", "this-as", "js-obj", "js-keys", "js-delete", "js-invoke", "aget", "aset",
                           "array", "make-array", "alength", "aclone", "amap", "areduce", "to-array",
                           "into-array", "js->clj", "clj->js", "exists?", "memfn", "unchecked-get"},
    "exceptions": {"try", "catch", "finally", "throw", "ex-info", "ex-data", "ex-message", "ex-cause", "Throwable->map"},
}

SCOPE_EXCLUSIONS = (
    ({"sorted-map", "sorted-map-by", "sorted-set", "sorted-set-by", "PersistentTreeMap", "PersistentTreeSet"},
     "sorted-collections", "Requires a sorted collection implementation, outside the named persistent helper scope."),
    ({"transient", "persistent!", "assoc!", "dissoc!", "conj!", "disj!", "pop!"},
     "transient-collections", "Requires transient collection mutation, outside the named persistent helper scope."),
    ({"transduce", "sequence", "eduction", "halt-when"},
     "transducers", "Requires transducer construction/execution, outside the collection-taking lazy sequence scope."),
    ({"derive", "underive", "parents", "ancestors", "descendants", "isa?", "make-hierarchy",
      "defmulti", "defmethod", "prefer-method", "prefers", "methods", "remove-method",
      "remove-all-methods", "dispatch-fn", "default-dispatch-val", "-get-method"},
     "multimethods-hierarchies", "Multimethod/hierarchy dispatch is separate from the named protocol/type/record features."),
    ({"add-tap", "remove-tap", "tap>", "tapset"},
     "tap-delivery", "Tap delivery/registration is outside the named synchronous atom/watch/state APIs."),
    ({"obj-map", "hash-map-lite", "set-lite", "set-lite-check",
      "ObjMap", "ObjMap.", "HashMapLite", "HashMapLite.", "SetLite", "SetLite.",
      "cljs.core.ObjMap", "cljs.core.ObjMap.", "cljs.core.HashMapLite", "cljs.core.HashMapLite.",
      "cljs.core.SetLite", "cljs.core.SetLite."},
     "lite-profile", "Direct lite-mode collection implementation/API, outside the default LITE_MODE=false persistent helper profile."),
)


def dependency_nodes(source: Source, assertion: Node, context):
    """Follow named local/global fixture references for scope, without evaluation.

    This keeps mixed bodies: (not= record hash-map-local) remains in scope even
    when another local in that same let constructs an unrelated sorted map.
    Every source form is still preserved verbatim regardless of disposition.
    """
    bindings = {}
    for form in source.forms:
        form = effective(form)
        if short_head(form) in {"def", "defonce", "defn", "defn-", "defmulti"} and len(form.children) > 2:
            name = effective(form.children[1]).token
            bindings[name] = [form] if short_head(form) == "defmulti" else form.children[2:]
    locals_ = set()

    def capture(node):
        # Freeze local references before rebinding the same name. An inner
        # [xs (conj xs x)] must retain the outer xs producer, not form a cycle.
        if node.kind in {"quote", "syntax-quote", "discard"} or short_head(node) == "quote":
            return node
        if node.kind == "atom" and node.token in locals_:
            return Node("bound-value", node.start, node.end, bindings[node.token])
        if node.kind == "metadata":
            return Node(node.kind, node.start, node.end,
                        [node.children[0], capture(node.children[-1])], node.token)
        return Node(node.kind, node.start, node.end, [capture(c) for c in node.children], node.token)

    def possible_values(values):
        for value in values:
            value = effective(value)
            if value.kind == "bound-value":
                yield from possible_values(value.children)
            else:
                yield value

    def literal_key(node):
        node = effective(node)
        if node.kind == "quote" and len(node.children) == 1 and node.children[0].kind == "atom":
            return ("symbol", node.children[0].token)
        if short_head(node) == "quote" and len(node.children) == 2 and node.children[1].kind == "atom":
            return ("symbol", node.children[1].token)
        if node.kind == "string":
            return ("string", json.loads(source.text[node.start:node.end]))
        if node.kind == "atom" and (node.token.startswith(":") or node.token in {"nil", "true", "false"}
                                    or node.token[:1].isdigit()):
            return ("atom", node.token)
        return None

    def map_values(values, key):
        projected = []
        for value in values:
            if value.kind == "map" and key is not None:
                pairs = list(zip(value.children[::2], value.children[1::2]))
                # Only literal keys permit precise sibling-independent lookup.
                if all(literal_key(k) is not None for k, _ in pairs):
                    projected.extend(v for k, v in pairs if literal_key(k) == key)
                    continue
            projected.append(value)
        return projected

    def bind_pattern(pattern, values):
        pattern = effective(pattern)
        values = list(possible_values(values))
        if pattern.kind == "atom":
            if pattern.token not in {"&", "nil", "true", "false"} and not pattern.token.startswith(":"):
                bindings[pattern.token] = values
                locals_.add(pattern.token)
        elif pattern.kind == "vector":
            # A literal row lets each destructured local follow only its own
            # component. Unknown producers conservatively remain whole inputs.
            index = 0
            parts = iter(pattern.children)
            for part in parts:
                if part.token == "&":
                    rest = next(parts, None)
                    if rest:
                        tail = [Node("vector", value.start, value.end, value.children[index:])
                                if value.kind == "vector" else value for value in values]
                        bind_pattern(rest, tail)
                    continue
                if part.token == ":as":
                    alias = next(parts, None)
                    if alias:
                        bind_pattern(alias, values)
                    continue
                projected = [value.children[index] for value in values
                             if value.kind == "vector" and index < len(value.children)]
                projected.extend(value for value in values if value.kind != "vector")
                bind_pattern(part, projected)
                index += 1
        elif pattern.kind == "map":
            for key, value in zip(pattern.children[::2], pattern.children[1::2]):
                if key.token == ":as":
                    bind_pattern(value, values)
                elif key.token in {":keys", ":syms", ":strs"}:
                    for local in value.children:
                        token = effective(local).token
                        key_value = (("atom", ":" + token.lstrip(":")) if key.token == ":keys"
                                     else ("symbol", token) if key.token == ":syms"
                                     else ("string", token))
                        name = Node("atom", local.start, local.end, [], token.split("/")[-1].lstrip(":"))
                        bind_pattern(name, map_values(values, key_value))
                elif not key.token.startswith(":"):
                    bind_pattern(key, map_values(values, literal_key(value)))

    for form in context:
        h = short_head(form)
        if h in {"let", "let*", "loop", "loop*", "binding", "with-redefs", "doseq", "for"}:
            if len(form.children) > 1 and form.children[1].kind == "vector":
                pairs = form.children[1].children
                for name, value in zip(pairs[::2], pairs[1::2]):
                    if h in {"doseq", "for"}:
                        if name.token == ":let":
                            for local, init in zip(value.children[::2], value.children[1::2]):
                                bind_pattern(local, [capture(init)])
                        elif not name.token.startswith(":"):
                            inputs = [child for producer in possible_values([capture(value)])
                                      for child in (producer.children if producer.kind == "vector" else [producer])]
                            bind_pattern(name, inputs)
                    else:
                        bind_pattern(name, [capture(value)])
    pending, seen = [assertion], set()
    while pending:
        node = pending.pop()
        yield node
        for n in reference_atoms(node):
            if n.kind == "atom" and n.token in bindings and n.token not in seen:
                seen.add(n.token)
                pending.extend(bindings[n.token])


def reference_atoms(node: Node):
    if node.kind in {"quote", "syntax-quote", "discard"} or short_head(node) == "quote":
        return
    if node.kind == "metadata":
        yield from reference_atoms(node.children[-1])
        return
    if node.kind == "atom":
        yield node
    for child in node.children:
        yield from reference_atoms(child)


def feature_for(source: Source, test: str, assertion: Node) -> tuple[str, list[str]]:
    words = symbols(assertion)
    found = {feature for feature, vocabulary in FEATURE_SYMBOLS.items() if words & vocabulary}
    if any(n.kind == "tagged" and n.token == "js" for n in nodes(assertion)):
        found.add("javascript-interop")
    preferred = FILE_FEATURE.get(Path(source.path).name) or TEST_FEATURE.get(test)
    if preferred:
        found.add(preferred)
    if not found:
        found.add("core-helpers")
    primary = preferred or next(f for f in FEATURE_SYMBOLS if f in found) if found != {"core-helpers"} else "core-helpers"
    return primary, [f for f in FEATURES if f in found]


def assertion_nodes(node: Node, inactive: str | None = None, context=()):
    """Yield assertion macro calls once; `thrown?` is part of its enclosing is.

    Preserve doseq/helper call-site contexts without claiming to enumerate a
    runtime iteration count. Quotes are data, but their assertion-looking forms
    are retained as inactive entries so they cannot be silently mistaken for
    tested behavior. Metadata's value is data; its target is walked normally.
    """
    if node.kind == "discard":
        yield from assertion_nodes(node.children[0], inactive or "reader-discarded", context)
        return
    if node.kind in {"quote", "syntax-quote", "var"}:
        for child in node.children:
            yield from assertion_nodes(child, inactive or "quoted-data", context)
        return
    if node.kind == "metadata":
        yield from assertion_nodes(node.children[0], inactive or "metadata-data", context)
        yield from assertion_nodes(node.children[1], inactive, context)
        return
    if node.kind in {"conditional", "conditional-splice"}:
        pairs = node.children[0].children
        if len(pairs) % 2:
            raise ValueError(f"Malformed reader conditional at {node.start}")
        selected = False
        for feature, branch in zip(pairs[::2], pairs[1::2]):
            active = not selected and feature.token in {":cljs", ":default"}
            selected |= active
            yield from assertion_nodes(branch, inactive if active else (inactive or "non-cljs-reader-branch"), context)
        return
    h = short_head(node)
    # Both checked reference builds use the default non-lite profile. Retain
    # the other branch verbatim, but never claim its assertions were executed.
    if h in {"if", "if-not", "when", "when-not"} and len(node.children) > 1:
        condition = effective(node.children[1])
        def shadows_lite(form):
            form_head = short_head(form)
            if form_head in {"let", "let*", "loop", "loop*", "doseq", "for", "binding", "with-redefs"}:
                if len(form.children) > 1 and form.children[1].kind == "vector":
                    pairs = form.children[1].children
                    for local, value in zip(pairs[::2], pairs[1::2]):
                        if "LITE_MODE" in symbols(local):
                            return True
                        if local.token == ":let" and any("LITE_MODE" in symbols(n) for n in value.children[::2]):
                            return True
            if form_head in {"fn", "fn*"}:
                parts = form.children[1:]
                if parts and effective(parts[0]).kind == "atom":
                    if effective(parts[0]).token == "LITE_MODE":
                        return True
                    parts = parts[1:]
                parameters = ([parts[0]] if parts and parts[0].kind == "vector" else
                              [part.children[0] for part in parts if part.kind == "list" and part.children])
                return any("LITE_MODE" in symbols(params) for params in parameters)
            return False

        shadowed = any(shadows_lite(form) for form in context)
        if condition.token == "cljs.core/LITE_MODE" or (condition.token == "LITE_MODE" and not shadowed):
            yes_active = h in {"if-not", "when-not"}
            for index, branch in enumerate(node.children[2:]):
                active = yes_active if h.startswith("when") or index == 0 else not yes_active
                yield from assertion_nodes(branch, inactive if active else
                                           (inactive or "non-default-lite-mode-branch"), context)
            return
    if h == "comment":
        inactive = inactive or "comment-form"
    if h in ASSERTIONS:
        yield node, inactive, context
        # Nested is/are/assert forms in expression position are still assertions;
        # defspec property body is represented by its whole quantified contract.
        if h == "defspec":
            return
    if h in {"testing", "doseq", "dotimes", "for", "let", "let*", "loop", "loop*", "letfn", "binding", "with-redefs", "fn", "fn*"}:
        context = (*context, node)
    for child in node.children:
        yield from assertion_nodes(child, inactive, context)


def classify(source: Source, test: str, node: Node, inactive: str | None, context):
    if inactive:
        return "unrelated", "inactive-upstream", f"Not an active upstream assertion: {inactive}."
    basename = Path(source.path).name
    reason = INTERNAL_TESTS.get(basename, {}).get(test)
    if reason:
        category = "library" if "library behavior" in reason else "upstream-internals"
        return "unrelated", category, reason
    # This test is an exhaustive upstream AST-schema contract. The one direct
    # namespace acceptance assertion (no AST projection) remains in scope.
    if basename == "analyzer_tests.clj" and test == "analyze-ops":
        if not (head(node.children[1]) == "binding" and "my.ns.foo" in symbols(node)):
            return "unrelated", "upstream-internals", "Exact upstream analyzer AST schema, field values, or node representation."
    # Rename resolution is public, but these two exact negative :uses
    # projections only prescribe which upstream cache map stores that binding.
    # The adjacent :renames presence/identity and source diagnostics stay in scope.
    if basename == "analyzer_tests.clj" and test == "test-cljs-1508-rename":
        projection = re.sub(r"\s+", " ", source.text[node.start:node.end])
        if projection in {"(is (nil? (-> parsed-ns :uses (get 'foo))))",
                          "(is (nil? (-> parsed-ns :uses (get 'intersection))))"}:
            return "unrelated", "upstream-internals", "Exact absence from upstream :uses storage after rename; actual renamed binding presence, identity, exclusions and diagnostics remain source-language contracts."
    # JVM analyzer/compiler assertions use Clojure atoms, sets and library helpers
    # as test infrastructure; only apply source-runtime exclusions to cljs files.
    if path_is_runtime(source.path):
        dependencies = list(dependency_nodes(source, node, context))
        words = {n.token.rsplit("/", 1)[-1] for dependency in dependencies
                 for n in reference_atoms(dependency)}
        for excluded, category, reason in SCOPE_EXCLUSIONS:
            if words & excluded:
                return "unrelated", category, reason
        forms = [n for dependency in dependencies for n in nodes(dependency)]
        if any(n.kind == "atom" and n.token in {"set/union", "set/rename-keys", "s/split", "s/split-lines"} for n in forms):
            return "unrelated", "library", "Standalone clojure.set/clojure.string library operation, outside compiler-language core features."
        if any(n.kind == "list" and len(n.children) == 2 and short_head(n) in {
            "map", "mapcat", "map-indexed", "filter", "remove", "keep", "keep-indexed", "take", "drop",
            "take-while", "drop-while", "take-nth", "partition-all", "partition-by", "replace", "interpose"
        } for n in forms):
            return "unrelated", "transducers", "Uses a transducer-constructor arity; this is not a collection-taking sequence overload."
    return "in-scope", "source-language", "Source-language assertion retained in full; no executable coverage claimed by inventory."


def path_is_runtime(path):
    return path.endswith((".cljs", ".cljc"))


def row_records(source: Source, assertion: Node):
    h = short_head(assertion)
    if h != "are":
        yield {"kind": "property" if h == "defspec" else h,
               "assertion": source.record(assertion)}
        return
    children = [n for n in assertion.children if n.kind != "discard"]
    if len(children) < 3 or children[1].kind != "vector":
        raise ValueError(f"Malformed are form in {source.path}:{assertion.start}")
    params = children[1].children
    width = len(params)
    args = children[3:]
    if not width or len(args) % width:
        raise ValueError(f"Incomplete are row in {source.path}:{assertion.start}")
    for row, offset in enumerate(range(0, len(args), width), 1):
        values = args[offset:offset + width]
        yield {"kind": "are-row", "assertion": source.record(assertion),
               "_scope_node": Node("list", assertion.start, assertion.end,
                                   children[:3] + values),
               "are": {"row": row, "rows": len(args) // width,
                       "parameters": [source.text[n.start:n.end] for n in params],
                       "template": source.record(children[2]),
                       "arguments": [source.record(n) for n in values],
                       "row_source": source.span(values[0].start, values[-1].end)}}


def encode_jsonl(rows) -> bytes:
    return ("".join(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n" for row in rows)).encode("utf-8")


def inventory(check_checkout: bool = True):
    assertions, bodies, files = [], [], []
    for path in FILES:
        # Read pinned git objects, then reject dirty/different working source.
        # A checkout can be on another HEAD only if the pinned object is present
        # and every inventoried file is still byte-identical to that revision.
        data = subprocess.check_output(["git", "-C", str(UPSTREAM), "show", f"{REVISION}:{path}"])
        if check_checkout and (UPSTREAM / path).read_bytes() != data:
            raise ValueError(f"Upstream working source differs from pin: {path}")
        source = Source(path, data)
        basename = Path(path).name
        source_assertions = []
        seen_names = Counter()
        source_bodies = []
        for form_index, form in enumerate(source.forms, 1):
            actual = effective(form)
            h = short_head(actual)
            named = h in DECLARATIONS and len(actual.children) >= 2
            name_node = effective(actual.children[1]) if named else None
            name = source.text[name_node.start:name_node.end] if name_node else f"top-level-{form_index:03}"
            seen_names[name] += 1
            # Repeated defmethod/def names are distinct complete source forms.
            unique_name = name if seen_names[name] == 1 else f"{name}~{seen_names[name]}"
            body_id = f"{basename}::{unique_name}"
            body_assertions = []
            ordinal = 0
            for node, inactive, context in assertion_nodes(form):
                for record in row_records(source, node):
                    scope_node = record.pop("_scope_node", node)
                    feature, features = feature_for(source, name, scope_node)
                    disposition, category, reason = classify(source, name, scope_node, inactive, context)
                    ordinal += 1
                    row = {"id": f"{body_id}::assertion-{ordinal:03}",
                           "source": path, "test": name, "ordinal": ordinal,
                           "body_id": body_id, "body_sha256": source.record(form)["sha256"],
                           "feature": feature, "features": features,
                           "disposition": disposition, "category": category,
                           "reason": reason, "status": "missing" if disposition == "in-scope" else "unrelated",
                           "active": inactive is None, "inactive_reason": inactive,
                           "context": [{"head": head(n), "start_byte": source.byte_offsets[n.start],
                                        "end_byte": source.byte_offsets[n.end]} for n in context], **record}
                    assertions.append(row)
                    source_assertions.append(row)
                    body_assertions.append(row["id"])
            body = {"id": body_id, "source": path, "name": name, "form": h,
                    "kind": "test" if h in TEST_FORMS else "support",
                    "ordinal": form_index, "assertions": body_assertions,
                    "source_span": source.record(form)}
            bodies.append(body)
            source_bodies.append(body)
        files.append({"path": path, "sha256": digest(data), "bytes": len(data),
                      "lines": len(data.splitlines()), "top_level_forms": len(source.forms),
                      "test_bodies": sum(b["kind"] == "test" for b in source_bodies),
                      "assertions": len(source_assertions),
                      "active_assertions": sum(r["active"] for r in source_assertions),
                      "by_status": dict(sorted(Counter(r["status"] for r in source_assertions).items())),
                      "license_notice": source.text[:source.forms[0].start].rstrip()})
    assert len({r["id"] for r in assertions}) == len(assertions), "Duplicate assertion IDs"
    assert len({r["id"] for r in bodies}) == len(bodies), "Duplicate body IDs"
    assertion_bytes, body_bytes = encode_jsonl(assertions), encode_jsonl(bodies)
    manifest = {
        "schema_version": 1, "upstream": "https://github.com/clojure/clojurescript",
        "revision": REVISION, "extractor_sha256": digest(Path(__file__).read_bytes()),
        "inventory_sha256": digest(assertion_bytes), "bodies_sha256": digest(body_bytes),
        "counting_unit": "Source assertion sites; each are argument row is a separate assertion. Dynamic loop/helper invocations are retained in complete bodies, not multiplied or claimed executed.",
        "recognized_assertion_macros": sorted(ASSERTIONS),
        "source_scope": "Whole files, including fixtures, helpers, inactive forms, and zero-assertion tests. No source-language exclusion based on current implementation limits.",
        "expectation_policy": "Verbatim original is/assert/property forms and are template/argument spans. No inference that an is message expression is an asserted predicate; no rewritten expectations.",
        "status_policy": "All active in-scope assertions initially missing. Only executable evidence outside this inventory may establish passing, failing, or justified version-difference status.",
        "counts": {"files": len(files), "bodies": len(bodies), "assertions": len(assertions),
                   "active_assertions": sum(r["active"] for r in assertions),
                   "by_status": dict(sorted(Counter(r["status"] for r in assertions).items())),
                   "by_kind": dict(sorted(Counter(r["kind"] for r in assertions).items())),
                   "by_category": dict(sorted(Counter(r["category"] for r in assertions).items()))},
        "features": [{"id": f, "assertions": sum(r["feature"] == f for r in assertions),
                      "by_status": dict(sorted(Counter(r["status"] for r in assertions if r["feature"] == f).items()))}
                     for f in FEATURES],
        "zero_assertion_tests": [r["id"] for r in bodies if r["kind"] == "test" and not r["assertions"]],
        "files": files,
    }
    return {"inventory.jsonl": assertion_bytes, "bodies.jsonl": body_bytes,
            "manifest.json": (json.dumps(manifest, indent=2, ensure_ascii=False) + "\n").encode("utf-8")}, manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Fail on any source/content/accounting drift; write nothing")
    args = parser.parse_args()
    try:
        generated, manifest = inventory()
        if args.check:
            changed = [name for name, data in generated.items() if not (HERE / name).exists() or (HERE / name).read_bytes() != data]
            if changed:
                raise ValueError("Inventory drift: " + ", ".join(changed) + "; regenerate with extract_inventory.py")
        else:
            for name, data in generated.items():
                (HERE / name).write_bytes(data)
        print(json.dumps({"result": "verified" if args.check else "generated", **manifest["counts"]}, sort_keys=True))
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"inventory error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
