"""Minimal Clojure source reader shared by the reference generators: nodes keep exact byte spans."""
from __future__ import annotations
from dataclasses import dataclass

REVISION = "a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e"


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


