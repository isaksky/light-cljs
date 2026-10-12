"""Two exact private tap-registry assertions; no public tap or execution credit.

validate() reads the pinned checkout only. The recorded release evidence was
inspected separately; inventory generation does not acquire or open Maven JARs.
"""
from __future__ import annotations

import subprocess
from typing import TypedDict

import extract_inventory as inv

REVISION = "a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e"
CORE_PATH = "src/main/cljs/cljs/core.cljs"
TEST_PATH = "src/test/cljs/cljs/core_test.cljs"
SOURCE_SHA256 = {
    CORE_PATH: "9734617a341b58f65bdf0786d895d920c83b5ca5b5ca4863e768ec65053927ca",
    TEST_PATH: "9371b1c18ea3ad5b8f0f1eab540ffbdb7241b24a6e00acfbd6bd036addba1dd8",
}
DECLARATION = '(defonce ^{:jsdoc ["@type {*}"] :private true}\n  tapset nil)'
DECLARATION_SHA256 = "35e0d01a09454fd1d4e308cd81b33fe9ade1d0957b553b233fc4dda87b26094a"
BODY_ID = "core_test.cljs::test-cljs-2549"
BODY = """(deftest test-cljs-2549
  (let [tap (fn [_])]
    (add-tap tap)
    (is (set? @tapset))
    (is (contains? @tapset tap))
    (remove-tap tap)))"""
BODY_SHA256 = "0c5c65c2832893c54f613b4851f936d1062518fce1004acc29a250eda25c752e"
ASSERTIONS = (
    (BODY_ID + "::assertion-001", "(is (set? @tapset))",
     "806b993a37887e8989048f8b1c57439cd83c76e39f92476fed1d40fbac328d98"),
    (BODY_ID + "::assertion-002", "(is (contains? @tapset tap))",
     "d055bc96f584c4e4e7b0f0a6c05279d526b4c78cd83010cb76b93d9b60b0434f"),
)
PUBLIC_DECLARATIONS = {
    "add-tap": "43829ea3b9249a1a742819505cec053c19b559c0a2c5f3a1b0d1b93ef57406ed",
    "remove-tap": "98b373283d86b8843b31c71cf76c8a5607f331dd90efac36973874f8af5f3412",
    "tap>": "112110c2b4f938f41bc23e972d9c2366c0561feadcef04edafc8b090f53535cc",
}
PUBLIC_TEST = "(deftest test-cljs-2552\n  (is (boolean? (tap> nil))))"
PUBLIC_TEST_SHA256 = "6b054aa8be721437bb5252704832aebd9f0d51560157d615766a4a50b9c84968"
SCOPE_BASIS = (
    "User-adopted compiler-language-completion group (#186, revision 2) excludes "
    "exact upstream private storage/ABI, while public constructor/protocol/core "
    "behavior remains required. The group explicitly says private aliases/internal "
    "storage classes must not be falsely counted as missing public features. "
    "Applying that boundary to only these two assertions is justified by their "
    "direct dereference of the explicitly private cljs.core/tapset registry. "
    "Public add-tap/remove-tap/tap> and test-cljs-2552 remain intended."
)
GROUP_CONTEXT_SHA256 = "f3e25d6a72914bb406675ce959d086d5112288d036622c224d1fd38b1c2f151a"
REASON = (
    "Exact assertion probes the representation or contents of the explicitly "
    "private upstream tapset registry. Complete original setup/body/cleanup is "
    "retained; public tap registration, removal and delivery remain independently "
    "required. This scope disposition grants no semantic or execution pass."
)


class ReleaseEvidence(TypedDict):
    version: str
    jar_sha256: str
    source_entry: str
    source_sha256: str
    declaration_sha256: str
    declaration_start_line: int
    inspection: str


RELEASE_EVIDENCE: ReleaseEvidence = {
    "version": "1.12.134",
    "jar_sha256": "d019b73177e4805748f4ee995c4cfcd14ebf28bff685a84059813fbe4e8c878f",
    "source_entry": "cljs/core.cljs",
    "source_sha256": "35344afe7d6a56a610d85dd759a96eb84a0aac398faa80d31eb81ecdbd127e80",
    "declaration_sha256": DECLARATION_SHA256,
    "declaration_start_line": 12034,
    "inspection": "Read-only source inspection; no tap/host execution. Recorded evidence only; validate() requires no Maven artifact.",
}


def _pinned(path: str) -> bytes:
    data = subprocess.check_output([
        "git", "-C", str(inv.UPSTREAM), "show", f"{REVISION}:{path}"])
    if (inv.UPSTREAM / path).read_bytes() != data:
        raise ValueError("Private tap scope working source differs from reviewed pin: " + path)
    return data


def _named(source: inv.Source, head: str, name: str) -> inv.Node:
    found = [node for node in source.forms
             if node.kind == "list" and inv.short_head(node) == head
             and len(node.children) > 1
             and inv.effective(node.children[1]).token == name]
    if len(found) != 1:
        raise ValueError("Private tap scope needs one active exact declaration/body: " + name)
    return found[0]


def _private_true(node: inv.Node) -> bool:
    name = node.children[1]
    if name.kind != "metadata" or name.children[0].kind != "map":
        return False
    fields = name.children[0].children
    return any(key.token == ":private" and value.kind == "atom" and value.token == "true"
               for key, value in zip(fields[::2], fields[1::2]))


def _exact(source: inv.Source, node: inv.Node, text: str | None, digest: str, label: str):
    span = source.record(node)
    if span["sha256"] != digest or (text is not None and span["text"] != text):
        raise ValueError("Private tap scope " + label + " drift; renewed scope review required")
    return span


def validate() -> list[dict]:
    """Return only two frozen source-assertion dispositions, never a pass."""
    if inv.REVISION != REVISION:
        raise ValueError("Private tap scope checkout revision drift")
    sources = {}
    for path, expected in SOURCE_SHA256.items():
        data = _pinned(path)
        if inv.digest(data) != expected:
            raise ValueError("Private tap scope pinned source hash drift: " + path)
        sources[path] = inv.Source(path, data)
    core, tests = sources[CORE_PATH], sources[TEST_PATH]
    declaration = _named(core, "defonce", "tapset")
    if not _private_true(declaration):
        raise ValueError("Private tap scope declaration no longer has explicit :private true")
    _exact(core, declaration, DECLARATION, DECLARATION_SHA256, "declaration")
    body = _named(tests, "deftest", "test-cljs-2549")
    _exact(tests, body, BODY, BODY_SHA256, "complete body")
    assertions = [node for node in inv.nodes(body)
                  if node.kind == "list" and inv.short_head(node) in inv.ASSERTIONS]
    if len(assertions) != len(ASSERTIONS):
        raise ValueError("Private tap scope original assertion count drift")
    # Public neighbors are guarded, never returned as scope exclusions.
    for name, expected in PUBLIC_DECLARATIONS.items():
        public = _named(core, "defn", name)
        if _private_true(public):
            raise ValueError("Private tap scope public declaration became private")
        _exact(core, public, None, expected, "public " + name)
    _exact(tests, _named(tests, "deftest", "test-cljs-2552"),
           PUBLIC_TEST, PUBLIC_TEST_SHA256, "public tap> test")
    result = []
    for node, (identity, text, expected) in zip(assertions, ASSERTIONS):
        assertion = _exact(tests, node, text, expected, "original assertion")
        result.append({
            "id": identity, "source": TEST_PATH, "body_id": BODY_ID,
            "body_sha256": BODY_SHA256, "assertion": assertion,
            "disposition": "unrelated", "category": "upstream-internals",
            "status": "unrelated", "reason": REASON, "scope_basis": SCOPE_BASIS,
            "scope_basis_record": {"group": "compiler-language-completion", "id": 186,
                                   "revision": 2, "context_sha256": GROUP_CONTEXT_SHA256},
            "private_declaration_sha256": DECLARATION_SHA256,
            "source_revision": REVISION, "semantic_pass_credit": False,
            "execution_evidence": "not-run",
        })
    return result
