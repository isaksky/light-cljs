"""Independent intended-language obligations, with no static semantic pass claims.

The pinned tree determines the source universe. Complete bodies are retained even
for boundaries; public declarations are inventoried before consulting local
CoreFnId spellings, metadata or witnesses. See full-scope.md for the contract.
"""
from collections import Counter
import json
from pathlib import Path
import re
import subprocess

import extract_inventory as inv
import private_tap_scope
import inference_optimizer_scope
import invoke_program_scope

HERE, ROOT = inv.HERE, inv.ROOT
COLUMNS = ("parse", "analysis_lowering", "native", "canonical_js", "quickjs_macro", "execution_evidence")
CORE = ("src/main/cljs/cljs/core.cljs", "src/main/clojure/cljs/core.cljc",
        "src/main/cljs/clojure/string.cljs", "src/main/cljs/clojure/set.cljs",
        "src/main/cljs/clojure/walk.cljs", "src/main/cljs/cljs/reader.cljs")
ANALYZER = "src/main/clojure/cljs/analyzer.cljc"
# These exclusions have a scope basis independent of local implementation.
FILE_BOUNDARIES = {
    "async_await_test.cljs": ("ecosystem", "Async/await library parity is outside the adopted synchronous dialect milestone."),
    "proxy_test.cljs": ("ecosystem", "Proxy library parity is outside the adopted synchronous dialect milestone."),
    "npm_deps_test.cljs": ("ecosystem", "npm acquisition/bundling is an explicitly excluded build integration."),
    "clojure_alias_test.cljs": ("ecosystem", "Standalone clojure.test/cljs.spec library alias compatibility is outside the promised namespaces; ordinary namespace aliases/renames and core predicates retain independent executing contracts."),
    "lite_collections_test.cljs": ("lite-profile", "Optional upstream lite collection representations are outside the default profile."),
    "pprint_test.cljs": ("ecosystem", "Standalone cljs.pprint namespace parity is outside the promised standard libraries."),
    "reducers_test.cljs": ("ecosystem", "Standalone reducers namespace parity is outside the promised standard libraries."),
    "spec_test.cljs": ("ecosystem", "Standalone cljs.spec namespace parity is outside the promised standard libraries."),
    "spec_test.cljc": ("ecosystem", "Standalone cljs.spec namespace parity is outside the promised standard libraries."),
    "test_test.cljs": ("ecosystem", "Standalone cljs.test harness parity is not a language obligation."),
    "test_test.cljc": ("ecosystem", "Standalone cljs.test harness parity is not a language obligation."),
    "datafy_test.cljs": ("ecosystem", "Standalone clojure.datafy parity is outside the promised standard libraries."),
    "math_test.cljs": ("ecosystem", "Standalone clojure.math parity is outside the promised standard libraries."),
    "data_test.cljs": ("ecosystem", "Standalone clojure.data parity is outside the promised string/set/walk/EDN libraries."),
}
PROMOTED = {"transient-collections", "transducers", "multimethods-hierarchies", "sorted-collections", "tap-delivery", "library"}
OWNERS = {
    "transient-collections": "light-cljs-117", "transducers": "light-cljs-118",
    "multimethods-hierarchies": "light-cljs-47", "sorted-collections": "light-cljs-119",
    "tap-delivery": "light-cljs-124", "library": "light-cljs-123",
}
NAME_FAMILIES = (
    ("public-iterator-helpers", "light-cljs-144", {"seq-iter", "ranged-iterator"}),
    ("public-protocols", "light-cljs-138", set("Inst inst-ms* inst-ms inst? IUUID uuid? IDerefWithTimeout -deref-with-timeout ASeq".split())),
    ("public-construction", "light-cljs-139", set("Cons ->Cons Reduced ->Reduced Symbol ->Symbol".split())),
    ("public-collection-construction", "light-cljs-141", set("List ->List EmptyList ->EmptyList Keyword ->Keyword Cycle ->Cycle Repeat ->Repeat Iterate ->Iterate PersistentVector ->PersistentVector Subvec ->Subvec ChunkedSeq ->ChunkedSeq PersistentQueue ->PersistentQueue PersistentArrayMap ->PersistentArrayMap PersistentHashMap ->PersistentHashMap PersistentHashSet ->PersistentHashSet".split())),
    ("public-construction-factories", "light-cljs-142", set("->IndexedSeq ->UUID ->Range ->IntegerRange ->PersistentTreeMap ->PersistentTreeSet ->PersistentTreeMapSeq ->Eduction ->MultiFn ->SeqIter ->RangedIterator ->Namespace ->TaggedLiteral".split())),
    ("transient-collections", "light-cljs-117", set("transient persistent! conj! assoc! dissoc! pop! disj! IEditableCollection ITransientCollection ITransientAssociative ITransientMap ITransientVector ITransientSet".split())),
    ("sorted-collections", "light-cljs-119", set("sorted-map sorted-map-by sorted-set sorted-set-by subseq rsubseq ISorted -sorted-seq -sorted-seq-from -entry-key -comparator comparator".split())),
    ("multimethods-hierarchies", "light-cljs-47", set("defmulti defmethod IMultiFn MultiFn remove-all-methods remove-method prefer-method methods get-method prefers dispatch-fn default-dispatch-val make-hierarchy isa? parents ancestors descendants derive underive bases supers".split())),
    ("sequence-transform", "light-cljs-59", {"sequence"}),
    ("eduction", "light-cljs-60", {"eduction", "Eduction"}),
    ("transducers", "light-cljs-118", set("completing transduce ensure-reduced halt-when dedupe random-sample map mapcat map-indexed filter remove keep keep-indexed take drop take-while drop-while take-nth partition-all partition-by replace interpose cat".split())),
    ("hash-equality", "light-cljs-120", set("hash hash-ordered-coll hash-unordered-coll INamed -name -namespace IHash -hash IEquiv -equiv IMeta IWithMeta meta with-meta -meta -with-meta".split())),
    ("lazy-sequences", "light-cljs-121", set("tree-seq flatten chunk chunk-first chunk-rest chunk-next chunk-cons chunked-seq? iterator-seq es6-iterator seq iter IChunk IChunkedSeq IChunkedNext ISeq ISeqable".split())),
    ("regex", "light-cljs-49", set("re-find re-matches re-seq re-pattern regexp?".split())),
    ("utilities", "light-cljs-122", set("subs rand rand-int rand-nth parse-long parse-double parse-uuid parse-boolean clone js-symbol? special-symbol? to-array-2d boolean-array byte-array char-array short-array int-array long-array float-array double-array object-array".split())),
    ("printing-reflection", "light-cljs-124", set("pr pr-str prn prn-str print println print-str println-str printf flush system-time Namespace find-ns create-ns ns-name ns-publics ns-interns ns-imports add-tap remove-tap tap>".split())),
    ("reader", "light-cljs-9", set("read read-string tagged-literal tagged-literal?".split())),
)
# Unmarked upstream implementation classes/helpers need explicit dispositions;
# public constructor/protocol names stay intended unless listed here.
LITE_NAMES = set("VectorLite VectorLiteIterator ObjMap ObjMapIterator HashMapLite SetLite obj-map hash-map-lite set-lite".split())
INTERNAL_NAMES = set("""BitmapIndexedNode ArrayNode HashCollisionNode Box NodeSeq ArrayNodeSeq
VectorNode TransientHashMap TransientArrayMap TransientVector TransientHashSet
BlackNode RedNode tree-map-add tree-map-append tree-map-remove tree-map-replace
balance-left balance-right balance-left-del balance-right-del inode-kv-reduce
create-node create-inode-seq create-array-node-seq edit-and-set ensure-editable
clone-and-set remove-pair bitmap-indexed-node-index bitpos mask caching-hash
NodeIterator ArrayNodeIterator HashMapIter HashSetIter ArrayList NeverEquiv
fast-path-protocols fast-path-protocol-partitions-count import-macros""".split())
MACRO_DIFFERENCES = set("require-macros use-macros import-macros".split())


def pinned(path):
    data = subprocess.check_output(["git", "-C", str(inv.UPSTREAM), "show", f"{inv.REVISION}:{path}"])
    if (inv.UPSTREAM / path).read_bytes() != data:
        raise ValueError(f"Upstream working source differs from pin: {path}")
    return data


def source_paths():
    paths = subprocess.check_output(["git", "-C", str(inv.UPSTREAM), "ls-tree", "-r", "--name-only", inv.REVISION], text=True).splitlines()
    # Include every runtime test/support file, even integrations and unasserted
    # helpers. Whitelist removal cannot hide a new pure core test file.
    return tuple(sorted(set(inv.FILES) | {p for p in paths if p.startswith("src/test/cljs/") and p.endswith((".cljs", ".cljc", ".clj"))}))


def disposition_for(path, category, reason, inactive):
    if inactive:
        return "unrelated", "inactive-upstream", reason
    boundary = FILE_BOUNDARIES.get(Path(path).name)
    if path.startswith("src/test/cljs/") and path.endswith(".clj"):
        boundary = ("reference-harness", "JVM companion/helper source is retained as reference setup; local compiler-emitted JS macro/phase contracts have separate obligations.")
    if "/spec/" in path:
        boundary = ("ecosystem", "Standalone cljs.spec/test library parity is outside the adopted milestone.")
    if boundary:
        return "unrelated", *boundary
    if category in PROMOTED:
        return "in-scope", category, "Intended public language/library behavior; historical batch exclusion is not a dialect boundary."
    if category in {"upstream-internals", "lite-profile"}:
        return "unrelated", category, reason
    return "in-scope", "source-language", "Complete source-language obligation; no execution inferred from inventory."


def ownership(name="", category="", path="", words=()):
    if name == "*unchecked-arrays*":
        return "array-compiler-settings", "light-cljs-132"
    if category in OWNERS:
        return category, OWNERS[category]
    if path.startswith(("src/main/cljs/clojure/", "src/main/cljs/cljs/reader")):
        return "library", "light-cljs-123"
    if name.startswith("-persistent") or name in {"-conj!", "-assoc!", "-dissoc!", "-pop!", "-disjoin!", "-as-transient", "-assoc-n!"}:
        return "transient-collections", "light-cljs-117"
    if name.startswith(("PersistentTree", "TreeMap")):
        return "sorted-collections", "light-cljs-119"
    if name:
        for family, owner, names in NAME_FAMILIES:
            if name in names:
                return family, owner
    for family, owner, names in NAME_FAMILIES:
        if set(words) & names:
            # Metadata assertions reached through another public subsystem
            # remain obligations of that subsystem, not hash-family omissions.
            if family == "hash-equality":
                enclosing = {"eval_test.cljs": ("printing-reflection", "light-cljs-124"),
                             "reader_test.cljs": ("reader", "light-cljs-9")}
                if Path(path).name in enclosing:
                    return enclosing[Path(path).name]
            return family, owner
    basename = Path(path).name
    if basename in {"hashing_test.cljs", "hash_map_test.cljs", "hash_set_test.cljs", "set_equiv_test.cljs"}:
        return "hash-equality", "light-cljs-120"
    if basename in {"iterator_test.cljs", "seqs_test.cljs", "chunked_seq.cljs"}:
        return "lazy-sequences", "light-cljs-121"
    if basename in {"printing_test.cljs", "var_test.cljs", "ns_test.cljs", "repl_test.cljs", "eval_test.cljs"}:
        return "printing-reflection", "light-cljs-124"
    if basename in {"reader_test.cljs", "syntax_quote_test.cljs", "tagged_literals_test.cljs"}:
        return "reader", "light-cljs-9"
    if basename in {"parse_test.cljs", "array_access_test.cljs", "array_access_test.cljc", "other_functions_test.cljs"}:
        return "utilities", "light-cljs-122"
    if basename in {"string_test.cljs", "set_test.cljs", "walk_test.cljs"}:
        return "library", "light-cljs-123"
    return "public-residual", "light-cljs-125"


def arity(vector):
    args = [inv.effective(n).token if inv.effective(n).kind == "atom" else "<destructured>" for n in vector.children]
    return {"kind": "variadic" if "&" in args else "fixed", "minimum": args.index("&") if "&" in args else len(args),
            "parameters": args}


def signature_nodes(form):
    # Only direct argument vectors/methods, never vectors nested in function body.
    tail = form.children[2:]
    function_declaration = inv.short_head(form) not in {"def", "defonce"}
    for n in tail:
        n = inv.effective(n)
        if n.kind == "vector" and function_declaration:
            yield n
            return
        if n.kind == "list" and n.children and inv.effective(n.children[0]).kind == "vector":
            yield inv.effective(n.children[0])
        elif inv.short_head(n) in {"fn", "fn*"}:
            yield from signature_nodes(inv.Node("list", n.start, n.end, [n.children[0], inv.Node("atom", 0, 0, [], "anonymous"), *n.children[1:]]))


def declarations(node):
    if node.kind == "discard":
        return
    n = inv.effective(node)
    h = inv.short_head(n)
    if h in {"def", "defonce", "defn", "defn-", "defmacro", "defprotocol", "deftype", "defrecord", "defcurried", "definline", "import-macros"}:
        yield n
        return
    # Descend only structural top-level wrappers. No quoted generated expansions
    # or definitions inside function bodies become public declarations.
    if n.kind in {"conditional", "conditional-splice"}:
        branch_list = n.children[0]
        for child in branch_list.children[1::2]:
            yield from declarations(child)
    elif n.kind == "vector":
        for child in n.children:
            yield from declarations(child)
    elif h == "do":
        for child in n.children[1:]:
            yield from declarations(child)


def public_api():
    rows, source_rows = [], []
    for path in CORE:
        source = inv.Source(path, pinned(path))
        source_rows.append({"path": path, "sha256": inv.digest(source.data), "license_notice": source.text[:source.forms[0].start].rstrip()})
        occurrence = Counter()
        for top in source.forms:
            for form in declarations(top):
                if len(form.children) < 2:
                    raise ValueError(f"unnamed declaration: {path}")
                h, name = inv.short_head(form), inv.effective(form.children[1]).token
                if h == "import-macros":
                    entries = [(inv.effective(n).token, "imported-macro", []) for vector in form.children[2:] if vector.kind == "vector" for n in vector.children]
                else:
                    entries = [(name, h, list(signature_nodes(form)))]
                if h == "defprotocol":
                    entries += [(inv.effective(n.children[0]).token, "protocol-method", [v for v in n.children[1:] if v.kind == "vector"])
                                for n in form.children[2:] if n.kind == "list" and n.children]
                if h in {"deftype", "defrecord"}:
                    fields = next((n for n in form.children[2:] if n.kind == "vector"), None)
                    if fields:
                        entries.append(("->" + name, "generated-positional-constructor", [fields]))
                        if h == "defrecord":
                            entries.append(("map->" + name, "generated-map-constructor", []))
                private = h == "defn-" or bool(re.search(r"\^:private|:private\s+true|:internal\s+true", source.text[form.start:form.children[2].start] if len(form.children) > 2 else ""))
                for api_name, kind, vectors in entries:
                    occurrence[api_name] += 1
                    suffix = f"~{occurrence[api_name]}" if occurrence[api_name] > 1 else ""
                    family, owner = ownership(api_name, path=path)
                    internal = private or api_name.removeprefix("->") in INTERNAL_NAMES
                    lite = api_name.removeprefix("->") in LITE_NAMES
                    reference_import = kind == "imported-macro"
                    disposition = "internal" if internal else "dialect" if lite or reference_import else "intended"
                    reason = ("Pinned declaration is explicitly private/internal." if private else
                              "Upstream storage/helper implementation shape is excluded; public collection/protocol behavior remains required." if internal else
                              "Public upstream name independently retained; mapped local spelling is not arity/execution evidence.")
                    if lite:
                        reason = "Optional upstream lite representation is a separate profile; ordinary persistent collection and transient behavior stays intended."
                    if reference_import:
                        reason = "Upstream JVM macro import plumbing is intentionally replaced by compiler-emitted JS macro phases; public macro declarations in the CLJS branch remain independent obligations."
                    signatures = [arity(v) for v in vectors]
                    if kind == "generated-map-constructor":
                        signatures = [{"kind": "fixed", "minimum": 1, "parameters": ["map"]}]
                    if not signatures:
                        signatures = [{"kind": "unresolved" if kind in {"imported-macro", "defcurried", "definline"} else "value", "minimum": None, "parameters": []}]
                    for index, signature in enumerate(signatures, 1):
                        rows.append({"id": f"api::{path}::{api_name}{suffix}::arity-{index}", "obligation_kind": "public-api",
                                     "name": api_name, "declaration": kind, "signature": signature, "source": path,
                                     "body_id": f"{path}::{name}{suffix}", "body_sha256": source.record(form)["sha256"],
                                     "source_span": source.record(form), "container_span": source.record(top),
                                     "disposition": disposition, "reason": reason, "family": family, "owner": owner,
                                     "dialect_contract": "Local compiler-emitted JS macro/import/phase contract; unchanged .clj companion behavior is not required." if api_name in MACRO_DIFFERENCES or kind in {"defmacro", "imported-macro"} else None})
    # Syntax dispatch comes from the upstream analyzer, independently of CoreFnId.
    source = inv.Source(ANALYZER, pinned(ANALYZER))
    source_rows.append({"path": ANALYZER, "sha256": inv.digest(source.data), "license_notice": source.text[:source.forms[0].start].rstrip()})
    special = next(f for f in source.forms if inv.short_head(f) == "def" and inv.effective(f.children[1]).token == "specials")
    values = next(n for n in inv.nodes(special) if n.kind == "set")
    for n in values.children[0].children:
        name = n.token
        rows.append({"id": "syntax::" + name, "obligation_kind": "syntax", "name": name, "signature": {"kind": "syntax", "minimum": None},
                     "source": ANALYZER, "body_id": "analyzer::specials", "body_sha256": source.record(special)["sha256"], "source_span": source.record(special),
                     "disposition": "intended", "reason": "Pinned public syntax dispatch; contextual analysis and execution require separate evidence.",
                     "family": "reader" if name == "&" else "public-residual", "owner": "light-cljs-9" if name == "&" else "light-cljs-125"})
    return rows, source_rows


def local_inventory():
    import native_api_retirement
    rows, files = [], []
    sci_dispositions = {fields[0]: fields for line in (ROOT / "tests/reference/sci/evaluator-dispositions.tsv").read_text().splitlines()
                        if line and not line.startswith("#") for fields in [line.split("\t")]}
    paths = ("src/analyzer.rs", "src/core_var_metadata.tsv", "tests/reference/sci/runtime.tsv", "tests/reference/sci/evaluator-dispositions.tsv", "tests/reference/sci/historical/supported-apis.tsv", "tests/reference/compiler-language/native_api_retirement.py")
    for path in paths:
        data = (ROOT / path).read_bytes()
        files.append({"path": path, "sha256": inv.digest(data)})
        if not path.endswith(".tsv"):
            continue
        for number, line in enumerate(data.decode().splitlines(), 1):
            if not line or line.startswith(("#", ";")):
                continue
            name = line.split("\t")[0]
            family, owner = ownership(name.rsplit("/", 1)[-1])
            fields = line.split("\t")
            migration = sci_dispositions.get(name)
            runtime_metadata = path == "tests/reference/sci/runtime.tsv" and name in {
                "format", "revision", "clojure", "java", "java-vendor", "options"}
            if runtime_metadata and len(fields) != 2:
                raise ValueError("SCI runtime provenance metadata changed into an executable row")
            retired = "sci/" in path and migration is not None and migration[7] == "retired"
            native_retirement = native_api_retirement.review(path, line)
            native_boundary = name.startswith("light-cljs.types/") or name.startswith("clojure.core/java.lang.") or name == "clojure.core/Throwable." or native_retirement is not None
            rows.append({"id": f"local::{path}::{name}", "obligation_kind": "local-api" if "supported-apis" in path or "metadata" in path else "sci-case",
                         "name": name, "source": path, "line": number, "body_id": f"{path}::{name}", "body_sha256": inv.digest(line.encode()),
                         "source_span": {"text": line, "sha256": inv.digest(line.encode())}, "disposition": "internal" if runtime_metadata or name == "clojure.core/lazy-seq*" else "dialect" if retired or native_boundary else "intended",
                         "reason": "SCI capture provenance metadata (format, revision, JVM runtime or capture options), retained verbatim; this two-column artifact contains no executable source or semantic expectation." if runtime_metadata else "Retired native macroexpansion helper; public lazy-seq remains an independent source/arity/phase obligation." if name == "clojure.core/lazy-seq*" else migration[-1] if retired else native_retirement['reason'] if native_retirement else "Native-only owner/type or exact numeric extension is intentionally retired by the adopted JavaScript dialect." if native_boundary else "Local/API and historical SCI provenance retained; historical/native observations and registration metadata establish no current semantic pass.",
                         **({'scope_basis': native_retirement['scope_basis'], 'semantic_pass_credit': False} if native_retirement else {}),
                         "family": family, "owner": owner})
    return rows, files


def local_registration():
    path = "src/analyzer.rs"
    text = (ROOT / path).read_text()
    start = text.index("pub(crate) fn resolve(name: &str)")
    end = text.index("\n    }", start)
    return {match.group(1): {"source": path, "line": text[:match.start()].count("\n") + 1,
                            "arm_sha256": inv.digest(match.group(0).encode()), "status": "static-registration-only"}
            for match in re.compile(r'"([^"\n]+)"\s*=>').finditer(text, start, end)}


# Exact implementation-storage probes; public metadata paths in these bodies
# remain required and execute through the complete equality fixture.
PRIVATE_EQUALITY_ASSERTIONS = {
    "hash_map_test.cljs::test-cljs-1888::assertion-005": "(is (instance? NodeSeq node-seq))",
    "hash_map_test.cljs::test-cljs-1888::assertion-010": "(is (instance? ArrayNodeSeq array-node-seq))",
}
PRIVATE_TAP_ASSERTIONS = {identity: text for identity, text, _ in private_tap_scope.ASSERTIONS}


# Optional lite storage tests remain excluded even when their original final
# assertion contains a standard transient call. Public transients over ordinary
# persistent collections retain independent intended body/arity/phase obligations.
OPTIONAL_LITE_ASSERTIONS = {
    "collections_test.cljs::test-obj-map::assertion-013":
        "(is (= (transient (obj-map :a 1 :b 2))\n         (obj-map :a 1 :b 2)))",
    "collections_test.cljs::test-hash-map-lite::assertion-013":
        "(is (= (transient (hash-map-lite :a 1 :b 2))\n         (hash-map-lite :a 1 :b 2)))",
}


# Exact residual harness/configuration bodies remain intended and unexecuted.
# Public parsing and checked-array operation contracts have separate execution.
UTILITY_RESIDUAL_BODIES = {
    'src/test/cljs/cljs/parse_test.cljs::test-gen-parse-long': (
        '0e9212db349f5d12c7ec780ac9267d3bc4673ec883692a594731331d5c11c332',
        'light-cljs-125', 'public-residual',
        'Complete external test.check quick-check/large-integer/for-all* harness integration '
        'remains required; the original 100000-case property body is retained unchanged and is '
        'not represented by the independent seeded core parser roundtrip test.'
    ),
    'src/test/cljs/cljs/parse_test.cljs::test-gen-parse-double': (
        'a0821f73f14a68067f674d6138d1935d05632a4bc3d02f8099550c090e766146',
        'light-cljs-125', 'public-residual',
        'Complete external test.check quick-check/double/for-all* harness integration remains '
        'required; the original 100000-case property body is retained unchanged and is not '
        'represented by the independent seeded core parser roundtrip test.'
    ),
    'src/test/cljs/cljs/array_access_test.cljs::cljs-2861-test': (
        '4ef042570cc1e5086b875e3b82bbeb8fea3b6b8cac21cfe4740506732f3e5ccf',
        'light-cljs-132', 'array-compiler-settings',
        'Source-visible *unchecked-arrays* compiler intrinsic/configuration evidence is owned by '
        'light-cljs-132; the canonical compiler currently rejects the default-setting source '
        'probe as unresolved.'
    ),
    'src/test/cljs/cljs/array_access_test.cljs::unchecked-arrays-file-scope-test': (
        'c7bb1a8f16225152068efba94312c554975b198e386cd5a9195a2fa31bd19163',
        'light-cljs-132', 'array-compiler-settings',
        'Complete alpha/beta/helper source-file compiler-setting isolation workflow is owned by '
        'light-cljs-132; no substituted false literal or dropped module setup supplies evidence.'
    ),
    'src/test/cljs/clojure/string_test.cljs::test-cljs-2300': (
        'fa0afa9f22feeca58f9fef2c712d5703b9b39f9ffd1729c7789e497ae9cc08f7',
        'light-cljs-125', 'public-residual',
        'Complete external test.check defspec/for-all/fmap/vector/choose harness integration for '
        'clojure.string/capitalize remains required; its original property contains subs but is '
        'not a missing core substring API, and remains an intended full-body obligation.'
    ),
}


def generate():
    paths = source_paths()
    generated, manifest = inv.inventory(files=paths, full_scope=True)
    assertions = [json.loads(line) for line in generated["inventory.jsonl"].splitlines()]
    private_taps = {row['id']: row for row in private_tap_scope.validate()}
    for row in assertions:
        if row["id"] in PRIVATE_EQUALITY_ASSERTIONS:
            if row["assertion"]["text"] != PRIVATE_EQUALITY_ASSERTIONS[row["id"]]:
                raise ValueError("Private equality exclusion source drift")
            row.update(disposition="unrelated", category="upstream-internals", status="unrelated",
                       reason="Exact private hash-trie sequence storage class identity; complete public sequence metadata behavior executes in compiler-equality.tsv.")
        if row['id'] in private_taps:
            basis = private_taps[row['id']]
            if (row['assertion']['text'] != basis['assertion']['text']
                    or row['assertion']['sha256'] != basis['assertion']['sha256']
                    or row['body_sha256'] != basis['body_sha256']):
                raise ValueError('Private tap storage exclusion source drift')
            row.update({key: basis[key] for key in ('disposition', 'category', 'status',
                'reason', 'scope_basis', 'scope_basis_record', 'private_declaration_sha256',
                'semantic_pass_credit', 'execution_evidence')})
        if row["id"] in OPTIONAL_LITE_ASSERTIONS:
            if row["assertion"]["text"] != OPTIONAL_LITE_ASSERTIONS[row["id"]]:
                raise ValueError("Optional lite source exclusion drift")
            row.update(disposition="unrelated", category="lite-profile", status="unrelated",
                       reason="Exact optional obj-map/hash-map-lite representation contract; this final source assertion follows the excluded complete lite body. Public ordinary persistent/transient collections remain independently required.")
        if row["body_id"] == "analyzer_tests.clj::test-namespace-metadata":
            if row["body_sha256"] != "1eb396cb35c7a51a58a59c1568c3bef46ccaa16f0675f35b98f0cf6374abc1d7":
                raise ValueError("Analyzer namespace metadata exclusion source drift")
            row.update(disposition="unrelated", category="upstream-internals", status="unrelated",
                       reason="Pinned JVM analyzer *cljs-ns* binding and AST :name metadata representation; upstream analyzer state/AST shape is outside the adopted compiler API. Runtime namespace/Var/value metadata remains independently required.")
        words = inv.symbols(inv.Reader(row["assertion"]["text"]).all()[0])
        family, owner = ownership(category=row["category"], path=row["source"], words=words)
        if row["body_id"] in UTILITY_RESIDUAL_BODIES:
            digest, owner, family, reason = UTILITY_RESIDUAL_BODIES[row["body_id"]]
            if row["body_sha256"] != digest:
                raise ValueError("Utility residual source drift")
            row["reason"] = reason
        row.update(obligation_kind="source-assertion", family=family, owner=owner,
                   historical_scope=row["source"] in inv.FILES,
                   dialect_contract="Local macro/import/phase contracts differ intentionally; unchanged .clj companions do not establish the local contract." if Path(row["source"]).name in {"macro_test.cljs", "import_test.cljs", "clojure_alias_test.cljs"} else None)
    api, api_sources = public_api()
    registration = local_registration()
    for row in api:
        row["local_registration"] = registration.get(row["name"], {"status": "unmapped"})
    local, local_sources = local_inventory()
    obligations = api + local
    bodies = [json.loads(line) for line in generated["bodies.jsonl"].splitlines()]
    optimizer_scopes = {row['id']: row for row in inference_optimizer_scope.validate()}
    invoke_scopes = {row['id']: row for row in invoke_program_scope.validate()}
    for body in bodies:
        if body["kind"] != "test" or body["assertions"]:
            continue
        family, owner = ownership(path=body["source"])
        boundary, category, reason = disposition_for(body["source"], "source-language", "Not an active upstream test: reader-discarded.", "reader-discarded" if body["source_span"]["text"].startswith("#_") else None)
        # These assertion-free JVM harness callers inspect/drive the upstream
        # analyzer API itself. Quoted language sources elsewhere in that file
        # remain independently selected and keep their existing witnesses.
        if body["id"] in {"analyzer_tests.clj::test-cljs-2101", "analyzer_tests.clj::test-refer-global"}:
            exact = {"analyzer_tests.clj::test-cljs-2101": "c36fcfc8e73f3c64d759150fee64c1a46cc9a9fbda4950aa6a742489904e2296", "analyzer_tests.clj::test-refer-global": "c86b3725b575fb529b8bd079dda0a78888a47618a28569013c84e223213f32ee"}
            if body["source_span"]["sha256"] != exact[body["id"]]:
                raise ValueError("JVM assertion-free analyzer boundary source drift")
            boundary, category, reason = "unrelated", "upstream-internals", "Exact JVM cljs.analyzer compiler environment/analyze-form-seq or parsed AST :renames contract; upstream analyzer API and representation are outside the adopted compiler API. Local quoted-source/Var/namespace/rename behavior remains independently required."
        optimizer_scope = optimizer_scopes.get('body::' + body['id'])
        if optimizer_scope:
            if body['source_span']['sha256'] != optimizer_scope['body_sha256']:
                raise ValueError('Exact optimizer body binding drift')
            boundary, category, reason = 'unrelated', optimizer_scope['category'], optimizer_scope['reason']
        obligations.append({"id": "body::" + body["id"], "obligation_kind": "assertion-free-test-body", "name": body["name"],
                            "source": body["source"], "body_id": body["id"], "body_sha256": body["source_span"]["sha256"],
                            "source_span": body["source_span"], "disposition": "intended" if boundary == "in-scope" else "host",
                            "reason": "Complete assertion-free test body requires meaningful execution; it contributes no invented assertion/pass." if boundary == "in-scope" else reason,
                            "family": family, "owner": owner,
                            **({key: optimizer_scope[key] for key in ("category", "scope_basis", "scope_basis_record", "semantic_pass_credit", "local_execution_credit", "macro_phase_credit", "original_predicate_forms")} if optimizer_scope else {})})
    for entry in manifest["files"]:
        if entry["assertions"] or not Path(entry["path"]).stem.endswith("_test"):
            continue
        source = inv.Source(entry["path"], pinned(entry["path"]))
        family, owner = ownership(path=source.path)
        if source.path == "src/test/cljs/cljs/array_access_test.cljc":
            family, owner = "public-residual", "light-cljs-125"
        boundary, category, reason = disposition_for(source.path, "source-language", "", None)
        invoke_scope = invoke_scopes.get('program::' + source.path)
        if invoke_scope:
            if inv.digest(source.data) != invoke_scope['body_sha256']:
                raise ValueError('Exact invocation optimizer program binding drift')
            boundary, category, reason = 'unrelated', invoke_scope['category'], invoke_scope['reason']
        obligations.append({"id": "program::" + source.path, "obligation_kind": "source-program", "name": Path(source.path).stem,
                            "source": source.path, "body_id": "program::" + source.path, "body_sha256": inv.digest(source.data),
                            "source_span": source.span(0, len(source.text)), "disposition": "intended" if boundary == "in-scope" else "host",
                            "reason": "Complete assertion-free test module requires compiler/execution evidence; its calls/effects cannot disappear from accounting." if boundary == "in-scope" else reason,
                            "family": family, "owner": owner,
                            **({key: invoke_scope[key] for key in ("category", "scope_basis", "scope_basis_record", "semantic_pass_credit", "local_execution_credit", "macro_phase_credit", "original_caller", "original_caller_test", "original_caller_body_sha256", "original_emission_assertions")} if invoke_scope else {})})
    if len({r["id"] for r in obligations}) != len(obligations):
        raise ValueError("Duplicate independent obligation ID")
    support = []
    tree = subprocess.check_output(["git", "-C", str(inv.UPSTREAM), "ls-tree", "-r", "--name-only", inv.REVISION], text=True).splitlines()
    for path in tree:
        if path.startswith("src/test/cljs/") and path not in paths:
            data = pinned(path)
            support.append({"source": path, "sha256": inv.digest(data), "bytes": len(data), "text": data.decode(),
                            "disposition": "reference-setup", "reason": "Complete auxiliary test fixture retained for source setup/host/effect provenance; not an executed language assertion."})
    assertion_bytes, obligation_bytes = inv.encode_jsonl(assertions), inv.encode_jsonl(obligations)
    support_bytes = inv.encode_jsonl(support)
    manifest.update(schema_version=2, scope="intended-language", generator_sha256=inv.digest(Path(__file__).read_bytes()),
                    private_tap_scope_sha256=inv.digest(Path(private_tap_scope.__file__).read_bytes()),
                    inference_optimizer_scope_sha256=inv.digest(Path(inference_optimizer_scope.__file__).read_bytes()),
                    invoke_program_scope_sha256=inv.digest(Path(invoke_program_scope.__file__).read_bytes()),
                    inventory_sha256=inv.digest(assertion_bytes), obligations_sha256=inv.digest(obligation_bytes),
                    api_sources=api_sources, local_sources=local_sources, support_sources=len(support), support_sha256=inv.digest(support_bytes), columns=list(COLUMNS),
                    status_policy="Inventory grants no pass. All intended source/name/arity obligations start missing. Native is removed; macro eligibility requires phase-specific execution evidence.")
    manifest["counts"].update(by_status=dict(Counter(r["status"] for r in assertions)),
                              by_category=dict(Counter(r["category"] for r in assertions)), obligations=len(obligations),
                              public_api_arities=sum(r["obligation_kind"] == "public-api" and r["disposition"] == "intended" for r in obligations),
                              intended_assertions=sum(r["disposition"] == "in-scope" for r in assertions))
    manifest["features"] = [] # family/owner totals are authoritative for this scope
    manifest["families"] = dict(Counter(r["family"] for r in [*assertions, *obligations]))
    return {"support-sources.jsonl": support_bytes, "full-inventory.jsonl": assertion_bytes, "full-bodies.jsonl": generated["bodies.jsonl"],
            "obligations.jsonl": obligation_bytes, "full-manifest.json": (json.dumps(manifest, indent=2, ensure_ascii=False) + "\n").encode()}, manifest


def load():
    manifest = json.loads((HERE / "full-manifest.json").read_text())
    for filename, key in (("full-inventory.jsonl", "inventory_sha256"), ("full-bodies.jsonl", "bodies_sha256"), ("obligations.jsonl", "obligations_sha256"), ("support-sources.jsonl", "support_sha256")):
        if inv.digest((HERE / filename).read_bytes()) != manifest[key]:
            raise ValueError(f"Full ledger hash drift: {filename}; regenerate and review")
    if manifest["revision"] != inv.REVISION or manifest["scope"] != "intended-language":
        raise ValueError("Full ledger source revision/scope mismatch")
    for filename, key in (("full_ledger.py", "generator_sha256"), ("extract_inventory.py", "extractor_sha256"),
                          ('private_tap_scope.py', 'private_tap_scope_sha256'),
                          ('inference_optimizer_scope.py', 'inference_optimizer_scope_sha256'),
                          ('invoke_program_scope.py', 'invoke_program_scope_sha256')):
        if inv.digest((HERE / filename).read_bytes()) != manifest[key]:
            raise ValueError(f"Full ledger generator drift: {filename}")
    private_tap_scope.validate()
    inference_optimizer_scope.validate()
    invoke_program_scope.validate()
    for entry in manifest["local_sources"]:
        if inv.digest((ROOT / entry["path"]).read_bytes()) != entry["sha256"]:
            raise ValueError(f"Local surface drift: {entry['path']}; regenerate and review")
    read = lambda name: [json.loads(line) for line in (HERE / name).read_text().splitlines()]
    return read("full-inventory.jsonl"), read("obligations.jsonl"), manifest


def columns(layers=None, status="missing"):
    layers = layers or {}
    result = {name: {"status": "not-run"} for name in COLUMNS}
    result["native"] = {"status": "removed", "reason": "Native evaluator removed in light-cljs-115; retained native evidence is historical."}
    for column, layer in (("parse", "parser"), ("analysis_lowering", "diagnostic"), ("canonical_js", "javascript")):
        if layer in layers:
            result[column] = layers[layer]
    if layers.get("javascript", {}).get("status") == "passed":
        result["analysis_lowering"] = {"status": "accepted-via-compilation", "reason": "Emission/execution occurred; this does not prove all analyses/arities."}
    result["execution_evidence"] = {"status": "verified" if status == "covered" else "not-run" if not layers else status,
                                    "references": {key: layers.get(key, {"status": "not-run"}) for key in ("release", "checkout")}}
    return result


def sci_execution_failed(report, evidence, failure_statuses):
    """Only a verified exact canonical reconciliation can explain SCI mismatch.

    Original observations and statuses remain intact. The full report already
    requires both engines, both pins, exact hashes and the macro phase before
    granting this explicit difference; every other execution failure still fails.
    """
    import report as runner
    reconciled = {row["canonical_reconciliation"]["canonical_expected"]["id"]
                  for row in report.get("assertions", [])
                  if row.get("status") == "version-difference"
                  and row.get("canonical_reconciliation", {}).get("status") == "verified"
                  and row["canonical_reconciliation"].get("disposition") == "canonical-dialect-difference"}
    reconciled.update(row["source_context_reconciliation"]["id"]
                      for row in report.get("assertions", [])
                      if row.get("status") == "version-difference"
                      and row.get("source_context_reconciliation", {}).get("reconciliation") == "verified-pinned-version-difference"
                      and row["source_context_reconciliation"].get("original_expectation_status") == "unresolved")
    reconciled.update(row["library_context_reconciliation"]["id"]
                      for row in report.get("assertions", [])
                      if row.get("status") == "version-difference"
                      and row.get("library_context_reconciliation", {}).get("reconciliation") == "verified-pinned-version-difference"
                      and row["library_context_reconciliation"].get("original_expectation_status") == "unresolved")
    reconciled.update(row["library_dialect_reconciliation"]["id"]
                      for row in report.get("assertions", [])
                      if row.get("status") == "version-difference"
                      and row.get("library_dialect_reconciliation", {}).get("reconciliation") in {
                          "verified-canonical-sci-difference", "verified-adopted-strict-edn-dialect-difference"}
                      and row["library_dialect_reconciliation"].get("original_expectation_status") == "unresolved"
                      and row["library_dialect_reconciliation"].get("actual_macro_contract_status") == "passed"
                      and row["library_dialect_reconciliation"].get("checked_pin_contract_status") == {"release": "passed", "checkout": "passed"}
                      and row.get("columns", {}).get("quickjs_macro", {}).get("status") == "passed"
                      and row.get("layers") == evidence.get(row["library_dialect_reconciliation"]["id"]))
    reconciled.update(row['reviewed_sci_context_reconciliation']['id']
                      for row in report.get('assertions', [])
                      if row.get('status') == 'version-difference'
                      and row.get('reviewed_sci_context_reconciliation', {}).get('reconciliation') == 'verified-canonical-sci-difference'
                      and row['reviewed_sci_context_reconciliation'].get('original_expectation_status') == 'unresolved'
                      and row['reviewed_sci_context_reconciliation'].get('original_semantic_pass_credit') == 0
                      and row['reviewed_sci_context_reconciliation'].get('canonical_contract_status') == 'passed'
                      and row.get('columns', {}).get('quickjs_macro', {}).get('status') == 'passed'
                      and row.get('layers') == evidence.get(row['reviewed_sci_context_reconciliation']['id']))
    reconciled.update(row['gap_sci_context_reconciliation']['id']
                      for row in report.get('assertions', [])
                      if row.get('status') == 'version-difference'
                      and row.get('gap_sci_context_reconciliation', {}).get('reconciliation') == 'verified-canonical-sci-difference'
                      and row['gap_sci_context_reconciliation'].get('original_expectation_status') == 'unresolved'
                      and row['gap_sci_context_reconciliation'].get('original_semantic_pass_credit') == 0
                      and row['gap_sci_context_reconciliation'].get('canonical_contract_status') == 'passed'
                      and row['gap_sci_context_reconciliation'].get('actual_macro_contract_status') == 'passed'
                      and row['gap_sci_context_reconciliation'].get('checked_pin_contract_status') == {'release': 'passed', 'checkout': 'passed'}
                      and row.get('columns', {}).get('quickjs_macro', {}).get('status') == 'passed'
                      and runner.same_json(row.get('layers'), evidence.get(row['gap_sci_context_reconciliation']['id'])))
    reconciled.update(verified_transport_observer_ids(report, evidence))
    import cljc_rejection_after191_accounting
    source_rejections = cljc_rejection_after191_accounting.explained_ids(report, evidence)
    return any(layer.get("status") in failure_statuses
               and not (identity in reconciled and layer.get("status") == "mismatch")
               and not (identity in source_rejections and layer.get("status") in {"mismatch", "rejected"})
               for identity, layers in evidence.items() for layer in layers.values())


def verified_transport_observer_ids(report, evidence):
    """Link an already replayed typed proof to its retained failed observer.

    This only explains an original mismatch. It never suppresses other failure
    statuses, grants original SCI credit, or replaces the original observation.
    """
    import report as runner
    import sci_cljc_transport_after185 as transport
    frozen = {row['id']: row for row in transport.load()}
    result = set()
    for row in report.get('assertions', []):
        proof = row.get('gap_sci_context_reconciliation', {})
        original = frozen.get(proof.get('id'))
        prior = proof.get('prior_observer_reconciliation', {})
        if not original or row.get('status') != 'version-difference':
            continue
        if not (proof.get('intermediate_value_transport_profile') == transport.PROFILE
                and proof.get('reconciliation') == 'verified-canonical-sci-difference'
                and proof.get('original_expectation_status') == 'unresolved'
                and proof.get('original_semantic_pass_credit') == 0
                and proof.get('canonical_contract_status') == 'passed'
                and proof.get('actual_macro_contract_status') == 'passed'
                and proof.get('checked_pin_contract_status') == {'release': 'passed', 'checkout': 'passed'}
                and row.get('columns', {}).get('quickjs_macro', {}).get('status') == 'passed'
                and prior.get('status') == 'missing'
                and prior.get('source_binding', {}).get('canonical_expected') is None
                and runner.same_json(prior.get('layers'), evidence.get(proof['id']))
                and runner.same_json(row.get('layers'), proof.get('layers'))):
            continue
        keys = ('source', 'source_sha256', 'original_row_sha256', 'body_sha256', 'original_expected')
        if all(runner.same_json(proof.get('source_binding', {}).get(key), original.get(key))
               and runner.same_json(prior.get('source_binding', {}).get(key), original.get(key))
               for key in keys):
            result.add(proof['id'])
    return result


def verified_library_dialects(document, current_evidence):
    """A sidecar must reproduce its current full-source and actual phase proof."""
    if document is None:
        return {}, None
    import sci_library_reconciliation as audit
    verified = audit.reconcile(document["local_evidence"], document["full_evidence"])
    if not audit.same(document.get("reconciliations"), verified["reconciliations"]):
        raise ValueError("Library dialect sidecar changed its verified original-source proof")
    for row in verified["reconciliations"]:
        if not audit.same(row["layers"], current_evidence.get(row["id"])):
            raise ValueError("Library dialect proof detached from this report's fresh execution")
    return {row["obligation_id"]: row for row in verified["reconciliations"]}, document["local_evidence"]


def verified_cljc_transport(document, prior_contexts):
    """Supplement only three unresolved observers; preserve their original evidence."""
    if document is None:
        return {}
    import copy
    import sci_cljc_transport_after185 as transport
    contexts = transport.reconcile(document)
    expected = {row['obligation_id'] for row in transport.load()}
    if set(contexts) != expected:
        raise ValueError('Incomplete or extra original CLJC transport cohort')
    for identity, proof in contexts.items():
        prior = prior_contexts.get(identity)
        if not prior or prior.get('status') != 'missing':
            raise ValueError('Transport proof must supplement an actual unresolved whole-source observer')
        binding = prior['source_binding']
        if binding.get('canonical_expected') is not None:
            raise ValueError('Transport proof cannot replace an existing canonical source literal')
        for key in ('source', 'source_sha256', 'original_row_sha256', 'body_sha256', 'original_expected'):
            if binding.get(key) != proof['source_binding'].get(key):
                raise ValueError('Transport proof changed original source or expectation: ' + key)
        if proof.get('status') != 'version-difference' or proof.get('original_semantic_pass_credit') != 0:
            raise ValueError('Intermediate transport must retain original SCI differences and zero pass credit')
        proof['prior_observer_reconciliation'] = copy.deepcopy(prior)
        proof['intermediate_value_transport_profile'] = transport.PROFILE
    return contexts


def expand_report(historical, witnesses, evidence, *, features=(), identities=(), phase_evidence=None, sci_evidence=None, library_dialect_evidence=None, reviewed_sci_context_evidence=None, gap_sci_context_evidence=None, print_sci_context_evidence=None, cljc_sci_context_evidence=None, lazy_sci_context_evidence=None, case_sci_context_evidence=None, transport_sci_context_evidence=None, numeric_sci_context_evidence=None, original_sci_after190_context_evidence=None, original_sci_after191_context_evidence=None, cljc_source_rejection_evidence=None, integer_source_context_evidence=None):
    import report as runner
    assertions, obligations, manifest = load()
    obligation_witnesses = load_obligation_witnesses(obligations, assertions, witnesses)
    phases = load_phase_witnesses(obligations)
    phase_evidence = phase_evidence or {}
    from sci_execution import load_all as load_sci, digest as sci_digest, canonical_contract, canonical_status, TEST_FILE, TEST_NAME
    from sci_source_context import load as load_source_context, expression_rows
    from sci_source_context_receipts import full_report_evidence as source_context_evidence
    from sci_library_context import load as load_library_context, expression_rows as library_expression_rows
    from sci_library_context_receipts import full_report_evidence as library_context_evidence
    source_context_mappings = {item["obligation_id"]: item for item in load_source_context()}
    library_context_mappings = {item["obligation_id"]: item for item in load_library_context()}
    if set(source_context_mappings) & set(library_context_mappings):
        raise ValueError("SCI source/library-context obligation identities overlap")
    expression_mappings = expression_rows(load_sci(), list(source_context_mappings.values()))
    sci_mappings = {item["obligation_id"]: item for item in library_expression_rows(expression_mappings, list(library_context_mappings.values()))}
    sci_evidence = sci_evidence or {}
    reviewed_sci_contexts = {}
    if reviewed_sci_context_evidence is not None:
        import reviewed_sci_context_dispatch as reviewed
        reviewed_sci_contexts = reviewed.reconcile(reviewed_sci_context_evidence)
        for proof in reviewed_sci_contexts.values():
            if not runner.same_json(proof['layers'], sci_evidence.get(proof['id'])):
                raise ValueError('Reviewed whole-source context detached from fresh report execution')
    gap_sci_contexts = {}
    if gap_sci_context_evidence is not None:
        import sci_gap_canonical_context as gaps
        gap_sci_contexts = gaps.reconcile(gap_sci_context_evidence)
        if set(gap_sci_contexts) & (set(reviewed_sci_contexts) | set(source_context_mappings) | set(library_context_mappings) | set(sci_mappings)):
            raise ValueError('Canonical gap contexts overlap another execution adapter')
        for proof in gap_sci_contexts.values():
            if not runner.same_json(proof['layers'], sci_evidence.get(proof['id'])):
                raise ValueError('Canonical gap context detached from fresh report execution')
    if print_sci_context_evidence is not None:
        import sci_print_context as printing
        printing_contexts = printing.reconcile(print_sci_context_evidence)
        if set(printing_contexts) & (set(gap_sci_contexts) | set(reviewed_sci_contexts) | set(source_context_mappings) | set(library_context_mappings) | set(sci_mappings)):
            raise ValueError('Exact printing context overlaps another execution adapter')
        for proof in printing_contexts.values():
            if not runner.same_json(proof['layers'], sci_evidence.get(proof['id'])):
                raise ValueError('Exact printing context detached from fresh report execution')
        gap_sci_contexts.update(printing_contexts)
    for document, module_name, label in (
            (cljc_sci_context_evidence, 'sci_cljc_accepted_context', 'Exact CLJC reader'),
            (lazy_sci_context_evidence, 'sci_lazy_context', 'Exact lazy/iteration/control'),
            (case_sci_context_evidence, 'case_char_string_accepted', 'Exact character/string case'),
            (numeric_sci_context_evidence, 'sci_numeric_error_after188', 'Exact original numeric/error'),
            (original_sci_after190_context_evidence, 'sci_originals_after190', 'Exact original after190'),
            (original_sci_after191_context_evidence, 'sci_originals_after191', 'Exact original after191')):
        if document is None:
            continue
        import importlib
        contexts = importlib.import_module(module_name).reconcile(document)
        if set(contexts) & (set(gap_sci_contexts) | set(reviewed_sci_contexts) | set(source_context_mappings) | set(library_context_mappings) | set(sci_mappings)):
            raise ValueError(label + ' context overlaps another execution adapter')
        for proof in contexts.values():
            if not runner.same_json(proof['layers'], sci_evidence.get(proof['id'])):
                raise ValueError(label + ' context detached from fresh report execution')
        gap_sci_contexts.update(contexts)
    gap_sci_contexts.update(verified_cljc_transport(transport_sci_context_evidence, gap_sci_contexts))
    import cljc_rejection_after191_accounting
    canonical_source_rejections = cljc_rejection_after191_accounting.verified(
        cljc_source_rejection_evidence, gap_sci_contexts, sci_evidence or {})
    library_dialects, library_phase_document = verified_library_dialects(library_dialect_evidence, sci_evidence)
    integer_sources = {}
    if integer_source_context_evidence is not None:
        import integer_source_context_after194 as integer_original
        integer_sources = integer_original.reconcile(integer_source_context_evidence)
        if set(integer_sources) != {integer_original.SITE}:
            raise ValueError('Complete integer source proof changed its sole original site')
    selected = bool(features or identities)
    rows = []
    for row in assertions:
        witness = witnesses.get(row["id"])
        if (features and not ({row["feature"], row["family"]} & set(features))) or (identities and row["id"] not in identities):
            continue
        status, reason = runner.classify(row, witness, evidence)
        layers = evidence.get(witness["fixture_id"], {}) if witness else {}
        rows.append({"id": row["id"], "obligation_kind": row["obligation_kind"], "feature": row["feature"],
                     "source": row["source"], "test": row["test"], "line": row["assertion"]["start_line"],
                     "family": row["family"], "owner": row["owner"], "status": status, "reason": reason,
                     "dialect_contract": row.get("dialect_contract"),
                     "fixture_id": witness["fixture_id"] if witness else None, "layers": layers, "columns": columns(layers, status),
                     "body_id": row["body_id"], "body_sha256": row["body_sha256"], "assertion_sha256": row["assertion"]["sha256"]})
        integer_proof = integer_sources.get(row['id'])
        if integer_proof is not None:
            mapping = integer_proof['mapping']
            if (witness is not None or row['disposition'] != 'in-scope'
                    or mapping['assertion_id'] != row['id']
                    or mapping['assertion_sha256'] != row['assertion']['sha256']
                    or mapping['body_id'] != row['body_id']
                    or mapping['body_sha256'] != row['body_sha256']
                    or integer_proof['status'] != 'covered'
                    or integer_proof['original_assertion_semantic_pass_credit'] != 1
                    or integer_proof['new_public_or_local_arity_links'] != 0):
                raise ValueError('Complete integer proof detached from its original assertion')
            frames = integer_proof['actual_phase_frames']
            references = {kind: {'status': 'passed', 'whole_original_namespace': proof}
                          for kind, proof in integer_proof['references'].items()}
            current = rows[-1]
            current.update(status='covered',
                reason='Complete original table and loop execute all112comparisons; both pins and all actual application/macro lifecycles independently replay.',
                fixture_id=integer_proof['fixture_id'],
                layers={**references, 'javascript': {'status': 'passed', 'actual_frames': frames}},
                integer_source_context_reconciliation=integer_proof)
            current['columns'] = columns(current['layers'], 'covered')
            current['columns']['quickjs_macro'] = {
                'status': 'passed', 'actual_phase': 'macro',
                'hosts': ['fresh', 'reused'], 'engines': ['QuickJS', 'Node'],
                'original_assertion_semantic_pass_credit': 1,
                'frames': {key: value for key, value in frames.items() if key.startswith('macro-')}}
    # Independent API/arity obligations are always required by the full gate,
    # even when a source subset is selected for execution.
    for row in obligations:
        status = "unrelated" if row["disposition"] in {"internal", "dialect", "host"} else "missing"
        mapping = obligation_witnesses.get(row["id"])
        layers = {}
        reconciliation = None
        source_reconciliation = None
        library_reconciliation = None
        library_dialect = None
        reviewed_sci_context = None
        gap_sci_context = None
        if status == "missing" and mapping and not mapping.get("local_execution"):
            source_statuses = [runner.classify(by_source, witnesses.get(by_source["id"]), evidence)[0]
                               for by_source in assertions if by_source["id"] in mapping["assertion_ids"]]
            status = "covered" if source_statuses and all(value == "covered" for value in source_statuses) else "missing"
            if row["obligation_kind"] == "public-api" and (row["id"] not in phases or phase_evidence.get(row["id"], {}).get("status") != "passed"
                    or phase_evidence.get(row["id"], {}).get("test_source_sha256") != phases[row["id"]]["test_source_sha256"]):
                status = "missing"
            observed = [evidence.get(witnesses[identity]["fixture_id"], {}) for identity in mapping["assertion_ids"]]
            layers = {layer: {"status": "passed" if all(value.get(layer, {}).get("status") == "passed" for value in observed) else "not-run"}
                      for layer in runner.SEMANTIC_LAYERS}
        if mapping and mapping.get("local_execution"):
            observed = phase_evidence.get(row["id"], {})
            verified = (observed.get("status") == "passed" and
                        observed.get("test_source_sha256") == mapping["local_execution"]["test_source_sha256"])
            layers = {"javascript": {"status": "passed" if verified else "not-run",
                                      "engines": ["Node", "QuickJS"]}}
            oracle = mapping.get("reference_fixture_id")
            if oracle:
                oracle_layers = evidence.get(oracle, {})
                layers.update({kind: oracle_layers.get(kind, {"status": "not-run"}) for kind in ("release", "checkout")})
                import namespace_results
                if oracle in namespace_results.IDS:
                    case = runner.fixtures('intended-language')[oracle]
                    complete = oracle_layers.get('javascript', {})
                    verified = verified and namespace_results.verified_local(case, complete)
                    layers['javascript'] = complete or {'status': 'not-run'}
            # Application/macro tests cannot manufacture pinned compatibility.
            # Only explicit source/phase dialect contracts may omit that oracle.
            references = bool(oracle) and all(layers.get(kind, {}).get("status") == "passed" for kind in ("release", "checkout"))
            status = "covered" if verified and (references or row.get("dialect_contract")) else "missing"
            # Exact local protocol/macro contracts provide local execution, not
            # pinned-reference observations. Never infer release/checkout runs.
        # A freshly replayed whole-source adapter owns its exact observation
        # shape. Do not first run that proof through the flattened expression
        # adapter, whose engine/provenance fields describe a different run.
        sci_mapping = None if row['id'] in reviewed_sci_contexts else sci_mappings.get(row["id"])
        if sci_mapping:
            if row["body_sha256"] != sci_mapping["body_sha256"]:
                raise ValueError("SCI execution mapping detached from original obligation")
            layers = sci_evidence.get(sci_mapping["id"], {})
            local = layers.get("javascript", {})
            exact_local = (local.get("source_sha256") == sci_mapping["source_sha256"]
                           and local.get("test_file") == TEST_FILE and local.get("test_name") == TEST_NAME
                           and local.get("test_source_sha256") == sci_digest((ROOT / TEST_FILE).read_bytes())
                           and local.get("adapter_source_sha256") == sci_digest((ROOT / "tests/support/sci_execution_observation.js").read_bytes())
                           and set(local.get("engines", {})) == {"Node", "QuickJS"})
            exact_references = all(
                layers.get(kind, {}).get("source_sha256") == sci_mapping["reference_source_sha256"]
                and layers[kind].get("original_source_sha256") == sci_mapping["original_source_sha256"]
                and layers[kind].get("isolated") == sci_mapping["reference_isolated"]
                and layers[kind].get("reference_identity") == kind + " " + (runner.RELEASE if kind == "release" else runner.REVISION)
                and layers[kind].get("settings") == {"optimizations": "none", "static-fns": False, "def-emits-var": False}
                for kind in ("release", "checkout"))
            historical_pass = (local.get("status") == "passed"
                               and all(engine.get("status") == "passed" for engine in local.get("engines", {}).values())
                               and all(layers.get(kind, {}).get("status") == "passed" for kind in ("release", "checkout")))
            status = "covered" if exact_local and historical_pass and exact_references else "missing"
            contract = canonical_contract(sci_mapping)
            if contract:
                canonical_engines = {kind: canonical_status(contract, local.get("engines", {}).get(kind, {}), runner.same_json)
                                     for kind in ("Node", "QuickJS")}
                canonical_references = {kind: canonical_status(contract, layers.get(kind, {}), runner.same_json)
                                        for kind in ("release", "checkout")}
                phase = phase_evidence.get(row["id"], {})
                phase_pass = (row["id"] in phases and phase.get("status") == "passed"
                              and phase.get("test_source_sha256") == phases[row["id"]]["test_source_sha256"])
                canonical_pass = (exact_local and local.get("status") in {"passed", "mismatch"}
                                  and exact_references and phase_pass
                                  and all(layers.get(kind, {}).get("stderr") == "" for kind in ("release", "checkout"))
                                  and all(value == "passed" for value in (*canonical_engines.values(), *canonical_references.values())))
                reconciliation = {**contract, "status": "verified" if canonical_pass else "missing",
                                  "engines": canonical_engines, "references": canonical_references,
                                  "quickjs_macro": "passed" if phase_pass else "not-run"}
                # Canonical agreement is an explicit dialect difference from
                # SCI, never a pass of the retained original [0 2] expectation.
                status = "version-difference" if canonical_pass else "missing"
        source_context_mapping = source_context_mappings.get(row["id"])
        if source_context_mapping:
            if row["body_sha256"] != source_context_mapping["body_sha256"]:
                raise ValueError("SCI source-context mapping detached from original obligation")
            layers = sci_evidence.get(source_context_mapping["id"], {})
            source_reconciliation = {"id": source_context_mapping["id"],
                                     **source_context_evidence(source_context_mapping, layers)}
            status = source_reconciliation["status"]
        library_context_mapping = library_context_mappings.get(row["id"])
        if library_context_mapping:
            if row["body_sha256"] != library_context_mapping["body_sha256"]:
                raise ValueError("SCI library-context mapping detached from original obligation")
            layers = sci_evidence.get(library_context_mapping["id"], {})
            library_reconciliation = {"id": library_context_mapping["id"],
                                      **library_context_evidence(library_context_mapping, layers)}
            status = library_reconciliation["status"]
            library_dialect = library_dialects.get(row["id"])
            if library_dialect:
                if library_reconciliation["reconciliation"] != "original-contract-unresolved":
                    raise ValueError("Library dialect credit would overwrite original SCI evidence")
                status = "version-difference"
        reviewed_sci_context = reviewed_sci_contexts.get(row['id'])
        if reviewed_sci_context:
            binding = reviewed_sci_context['source_binding']
            if (row['body_sha256'] != binding['body_sha256'] or row['disposition'] != 'intended'
                    or row['obligation_kind'] != 'sci-case'):
                raise ValueError('Reviewed whole-source proof detached from original intended SCI obligation')
            layers = reviewed_sci_context['layers']
            status = reviewed_sci_context['status']
        gap_sci_context = gap_sci_contexts.get(row['id'])
        if gap_sci_context:
            binding = gap_sci_context['source_binding']
            if (row['body_sha256'] != binding['body_sha256'] or row['disposition'] != 'intended'
                    or row['obligation_kind'] != 'sci-case'):
                raise ValueError('Canonical gap proof detached from original intended SCI obligation')
            layers = gap_sci_context['layers']
            status = gap_sci_context['status']
        observed_columns = columns(layers, status)
        observed_columns["quickjs_macro"] = phase_evidence.get(row["id"], {"status": "not-run"}) if row["id"] in phases else {"status": "not-run"}
        if library_dialect:
            import sci_library_reconciliation as audit
            observed_columns["quickjs_macro"] = {
                "status": "passed", "test_file": audit.DRIVER, "test_name": audit.TEST,
                "test_source_sha256": inv.digest((ROOT / audit.DRIVER).read_bytes()),
                "actual_phase": "macro", "hosts": ["fresh", "reused"], "engines": ["QuickJS", "Node"],
                "frames": [frame for frame in library_phase_document["frames"]
                           if frame["id"] == library_dialect["id"] and frame["phase"] == "macro"]}
        if reviewed_sci_context:
            binding = reviewed_sci_context['source_binding']
            frames = reviewed_sci_context['actual_phase_frames']
            observed_columns['quickjs_macro'] = {'status': 'passed',
                'actual_phase': 'macro', 'hosts': ['fresh', 'reused'], 'engines': ['QuickJS', 'Node'],
                'source_context_profile': binding['source_context_profile'],
                'frames': [{'variant': key, **value} for key, value in frames.items() if key.startswith('macro-')]}
        gap_reason = (('Fresh whole original source observations retained; this original source/phase contract remains unresolved.' if status == 'missing' else 'Fresh whole original source and literal canonical result/effects checked in both pins and all application/macro lifecycles; original SCI differences and current production discrepancies stay separate.') if gap_sci_context else None)
        if gap_sci_context:
            binding = gap_sci_context['source_binding']
            frames = gap_sci_context['actual_phase_frames']
            observed_columns['quickjs_macro'] = {
                'status': gap_sci_context['actual_macro_contract_status'],
                'actual_phase': 'macro', 'hosts': ['fresh', 'reused'], 'engines': ['QuickJS', 'Node'],
                'source_context_profile': binding['source_context_profile'],
                'frames': [{'variant': key, **value} for key, value in frames.items() if key.startswith('macro-')]}
        canonical_source_rejection = canonical_source_rejections.get(row['id'])
        if canonical_source_rejection:
            binding = canonical_source_rejection['source_binding']
            if row['body_sha256'] != binding['body_sha256'] or row['obligation_kind'] != 'sci-case':
                raise ValueError('Canonical source rejection detached from original obligation')
            status = 'verified-rejection'
            layers = canonical_source_rejection['layers']
            observed_columns = columns(layers, status)
            observed_columns['quickjs_macro'] = {
                'status': 'rejected-before-original-computation',
                'actual_phase': 'macro', 'hosts': ['fresh', 'reused'],
                'original_macro_execution_pass_credit': 0,
                'frames': [frame for frame in canonical_source_rejection['actual_phase_frames'].values()
                           if frame['variant'].startswith('macro-')]}
            gap_reason = canonical_source_rejection['reason']
        rows.append({**row, "feature": row["family"], "test": row["name"], "line": row["source_span"].get("start_line", row.get("line")),
                     "status": status, "fixture_id": mapping.get("reference_fixture_id") if mapping else None, "layers": layers, "columns": observed_columns,
                     **({"canonical_reconciliation": reconciliation} if reconciliation else {}),
                     **({"source_context_reconciliation": source_reconciliation} if source_reconciliation else {}),
                     **({"library_context_reconciliation": library_reconciliation} if library_reconciliation else {}),
                     **({"library_dialect_reconciliation": library_dialect} if library_dialect else {}),
                     **({'reviewed_sci_context_reconciliation': reviewed_sci_context} if reviewed_sci_context else {}),
                     **({'gap_sci_context_reconciliation': gap_sci_context} if gap_sci_context else {}),
                     **({'canonical_source_rejection_reconciliation': canonical_source_rejection} if canonical_source_rejection else {}),
                     "completion_reason": gap_reason or ("Fresh complete original source, literal outcome/effects and actual application/macro contexts verified; original SCI agreement or exact canonical difference remains separately reported." if reviewed_sci_context else "Exact source-specific canonical/strict-EDN difference verified in both pins and both application/macro phases; original SCI expectation remains unresolved." if library_dialect else "QuickJS macro eligibility not run; ordinary body evidence cannot prove expansion-phase operation support." if row["obligation_kind"] == "public-api" and mapping and observed_columns["quickjs_macro"]["status"] != "passed" else row["reason"])})
    counts = Counter(row["status"] for row in rows)
    sections = {}
    for row in rows:
        sections.setdefault(row["feature"], Counter())[row["status"]] += 1
    missing = counts["missing"]
    return {**historical, "schema_version": 2, "scope": "intended-language", "columns": list(COLUMNS),
            "historical_scope": {"name": "historical-selected", "counts": historical["counts"], "strict_complete": historical["strict_complete"]},
            "selection": {"features": list(features), "ids": list(identities), "full_scope_selected": not selected},
            "counts": {key: counts[key] for key in historical["counts"]}, "features": {key: dict(value) for key, value in sorted(sections.items())},
            "assertions": rows, "source_strict_complete": not selected and missing == 0,
            "strict_complete": not selected and missing == 0 and historical["boundary_strict_complete"],
            "inventory_counts": manifest["counts"], "evidence_policy": "Parser corpus acceptance, local metadata/name registration and historical/native evidence never establish public API/arity or macro completion."}


def validate_arity_cohort(result, filename, obligations, module_name):
    import importlib
    cohort = importlib.import_module(module_name)
    target_ids = {row['id'] for row in obligations}
    expected = [row for row in cohort.exports()[filename] if row['obligation_id'] in target_ids]
    identities = {row['obligation_id'] for row in expected}
    actual = [row for identity, row in result.items() if identity in identities]
    if json.dumps(actual, sort_keys=True) != json.dumps(expected, sort_keys=True):
        raise ValueError('Complete ' + module_name + ' cohort missing, changed or detached')


def validate_arity_after171(result, filename, obligations):
    validate_arity_cohort(result, filename, obligations, 'arity_after171')


def validate_arity_after172(result, filename, obligations):
    validate_arity_cohort(result, filename, obligations, 'arity_after172')


def load_obligation_witnesses(obligations, assertions, witnesses):
    """Require source/arity identity and complete, execution-backed test bodies.

    The arity-faithfulness rationale remains reviewable, just as it does for
    assertion adapters. A registration/metadata row cannot be the witness.
    """
    targets = {row["id"]: row for row in obligations}
    sources = {row["id"]: row for row in assertions}
    body_ids = {}
    for row in assertions:
        if row["disposition"] == "in-scope":
            body_ids.setdefault(row["body_id"], []).append(row["id"])
    result = {}
    reference_fixtures = None
    phase_mappings = None
    for line in (HERE / "obligation-witnesses.jsonl").read_text().splitlines():
        item = json.loads(line)
        target = targets.get(item.get("obligation_id"))
        if not target or target["disposition"] != "intended" or not item.get("rationale", "").strip():
            raise ValueError("Invalid public obligation witness target/rationale")
        if target["id"] in result or item.get("body_sha256") != target["body_sha256"]:
            raise ValueError("Duplicate/stale public obligation witness")
        signature = target.get("signature")
        if signature and (signature["kind"] == "unresolved" or item.get("signature_sha256") != inv.digest(json.dumps(signature, sort_keys=True).encode())):
            raise ValueError("Public witness needs the exact resolved source arity")
        if item.get("local_execution"):
            # Local public protocol contracts need direct execution evidence when
            # upstream has no corresponding test body. Reuse the exact hashed,
            # named phase test, which must also execute both application engines.
            if phase_mappings is None:
                phase_mappings = load_phase_witnesses(obligations)
            phase = phase_mappings.get(target["id"])
            if (not phase or item["local_execution"] != {key: phase[key] for key in
                    ("test_file", "test_name", "test_source_sha256")} or item.get("assertion_ids")):
                raise ValueError("Local obligation witness needs its exact application/macro execution test")
            if item.get("reference_fixture_id"):
                import report as runner
                if reference_fixtures is None:
                    reference_fixtures = runner.fixtures("intended-language")
                oracle = reference_fixtures.get(item["reference_fixture_id"])
                if (not oracle or item.get("reference_source_sha256") != inv.digest(oracle["source"].encode())
                        or item.get("reference_contract_sha256") != inv.digest(json.dumps(oracle["expected"], sort_keys=True).encode())):
                    raise ValueError("Local operation oracle detached from exact source/literal expectation")
            result[target["id"]] = item
            continue
        ids = item.get("assertion_ids", [])
        if not ids or len(ids) != len(set(ids)) or any(identity not in sources or identity not in witnesses for identity in ids):
            raise ValueError("Public witness needs complete source assertion execution mappings")
        for identity in ids:
            source = sources[identity]
            if any(value not in ids for value in body_ids[source["body_id"]]):
                raise ValueError("Public witness cannot omit setup/body assertion paths")
        result[target["id"]] = item
    validate_arity_after171(result, 'obligation-witnesses.jsonl', obligations)
    validate_arity_after172(result, 'obligation-witnesses.jsonl', obligations)
    validate_arity_cohort(result, 'obligation-witnesses.jsonl', obligations, 'arity_after185')
    validate_arity_cohort(result, 'obligation-witnesses.jsonl', obligations, 'arity_after188')
    validate_arity_cohort(result, 'obligation-witnesses.jsonl', obligations, 'arity_after190')
    validate_arity_cohort(result, 'obligation-witnesses.jsonl', obligations, 'arity_after191')
    return result


def load_phase_witnesses(obligations):
    """Phase eligibility requires a pinned, named local execution test.

    Macro/import contracts differ intentionally from upstream. Retain the whole
    local test source (including all helper/setup/effect paths), target body and
    arity hashes, and a reviewed rationale connecting its operation to the test.
    No test-name discovery or registration is itself a passing observation.
    """
    targets = {row["id"]: row for row in obligations}
    result = {}
    for line in (HERE / "phase-witnesses.jsonl").read_text().splitlines():
        item = json.loads(line)
        target = targets.get(item.get("obligation_id"))
        path = Path(item.get("test_file", ""))
        name = item.get("test_name", "")
        if (not target or target["disposition"] != "intended" or target["id"] in result
                or item.get("body_sha256") != target["body_sha256"] or not item.get("rationale", "").strip()
                or path.is_absolute() or ".." in path.parts or len(path.parts) != 2
                or path.parts[0] != "tests" or path.suffix != ".rs" or not re.fullmatch(r"[A-Za-z_][A-Za-z_0-9]*", name)):
            raise ValueError("Invalid macro phase witness target/source/rationale")
        signature = target.get("signature")
        if signature and (signature["kind"] == "unresolved" or item.get("signature_sha256") != inv.digest(json.dumps(signature, sort_keys=True).encode())):
            raise ValueError("Macro phase witness needs exact resolved source arity")
        data = (ROOT / path).read_bytes()
        if item.get("test_source_sha256") != inv.digest(data) or not re.search(r"#\[test\]\s*(?:pub\s+)?fn\s+" + re.escape(name) + r"\s*\(", data.decode()):
            raise ValueError("Macro phase witness detached from complete local test source")
        supports = item.get("support_source_sha256", {})
        if not isinstance(supports, dict):
            raise ValueError("Invalid macro phase support source map")
        for support, digest in supports.items():
            catalog_support = support == "src/core_var_metadata.tsv"
            support = Path(support)
            if (support.is_absolute() or ".." in support.parts or not support.parts
                    or (support.parts[0] != "tests" and not catalog_support)
                    or not (ROOT / support).is_file()
                    or inv.digest((ROOT / support).read_bytes()) != digest):
                raise ValueError("Macro phase witness detached from support source")
        result[target["id"]] = item
    validate_arity_after171(result, 'phase-witnesses.jsonl', obligations)
    validate_arity_after172(result, 'phase-witnesses.jsonl', obligations)
    validate_arity_cohort(result, 'phase-witnesses.jsonl', obligations, 'arity_after185')
    validate_arity_cohort(result, 'phase-witnesses.jsonl', obligations, 'arity_after188')
    validate_arity_cohort(result, 'phase-witnesses.jsonl', obligations, 'arity_after190')
    validate_arity_cohort(result, 'phase-witnesses.jsonl', obligations, 'arity_after191')
    return result


def run_phase_witnesses(obligations, artifacts, timeout, *, reference_fixtures=None):
    import report as runner
    mappings = load_phase_witnesses(obligations)
    if reference_fixtures is not None:
        linked = {item['obligation_id'] for item in map(json.loads, (HERE / 'obligation-witnesses.jsonl').read_text().splitlines())
                  if item.get('reference_fixture_id') in reference_fixtures}
        mappings = {identity: item for identity, item in mappings.items() if identity in linked}
    results, cache = {}, {}
    for identity, item in mappings.items():
        key = (item["test_file"], item["test_name"])
        if key not in cache:
            target = Path(item["test_file"]).stem
            observed = runner.invoke(["cargo", "test", "--locked", "--test", target, "--", "--exact", item["test_name"], "--nocapture"],
                                     artifacts / target / item["test_name"], timeout=timeout)
            if observed["status"] == "completed":
                text = observed["stdout"]
                passed = ("running 1 test\n" in text and "test " + item["test_name"] + " ... ok" in text
                          and "test result: ok. 1 passed; 0 failed; 0 ignored;" in text)
                observed = runner.result("passed" if passed else "invalid-output", "Exact single macro phase execution test; zero-test/skipped runs fail.",
                                         test_file=item["test_file"], test_name=item["test_name"], test_source_sha256=item["test_source_sha256"])
            cache[key] = observed
        results[identity] = cache[key]
    return results
