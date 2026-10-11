#!/usr/bin/env python3
"""Extractor checks using synthetic source only; no research checkout required."""

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
import full_ledger
from unittest.mock import patch

sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("language_inventory", Path(__file__).with_name("extract_inventory.py"))
inv = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = inv
SPEC.loader.exec_module(inv)


class InventoryTests(unittest.TestCase):
    def generate(self, text, path="src/test/cljs/cljs/example.cljs"):
        with patch.object(inv, "FILES", (path,)), patch.object(inv.subprocess, "check_output", return_value=text.encode()):
            result, manifest = inv.inventory(check_checkout=False)
        rows = [json.loads(line) for line in result["inventory.jsonl"].splitlines()]
        bodies = [json.loads(line) for line in result["bodies.jsonl"].splitlines()]
        return result, manifest, rows, bodies

    def test_full_bodies_helpers_and_zero_assertion_tests_are_retained(self):
        text = "; License notice\n(ns sample)\n(defn helper [x] (is x))\n(deftest calls-helper (helper true))\n(deftest nested (doseq [x [1 2]] (is (= x x))))"
        _, manifest, rows, bodies = self.generate(text)
        self.assertEqual(len(bodies), 4)
        self.assertEqual(len(rows), 2)
        self.assertEqual(manifest["zero_assertion_tests"], ["example.cljs::calls-helper"])
        self.assertEqual(bodies[2]["source_span"]["text"], "(deftest calls-helper (helper true))")
        self.assertEqual(rows[0]["body_id"], "example.cljs::helper")
        self.assertEqual(rows[1]["context"][0]["head"], "doseq")
        self.assertTrue(all(row["status"] == "missing" for row in rows))

    def test_reader_literals_do_not_create_phantom_assertions(self):
        text = r'''(deftest literals
          ; (is false)
          (is (= "(is false)" "(is false)"))
          (is (= #"[(]is[)]" #"[(]is[)]"))
          (is (= [\) \; \" \\ \space \newline] [\) \; \" \\ \space \newline]))
          (is (= #:foo{:x 1} #::{:x 1}))
          (is (= #js [1] #js [1]))
          (is (= ##Inf ##Inf)))'''
        _, _, rows, _ = self.generate(text)
        self.assertEqual(len(rows), 6)
        for row in rows:
            span = row["assertion"]
            self.assertEqual(text.encode()[span["start_byte"]:span["end_byte"]].decode(), span["text"])

    def test_unicode_offsets_are_bytes_and_expected_forms_are_not_rewritten(self):
        text = '(deftest unicode (is (= "é♯" "é♯")) (is (= \'(1)) (apply f [1])))'
        _, _, rows, _ = self.generate(text)
        self.assertEqual(len(rows), 2)
        span = rows[1]["assertion"]
        self.assertEqual(span["text"], "(is (= '(1)) (apply f [1]))")
        self.assertEqual(text.encode()[span["start_byte"]:span["end_byte"]].decode(), span["text"])
        self.assertEqual(inv.digest(span["text"].encode()), span["sha256"])

    def test_are_expands_rows_and_retains_template_arguments_and_scope(self):
        text = "(deftest rows (are [expected expr] (= expected expr) [] (vec nil) {} (sorted-map)))"
        _, _, rows, _ = self.generate(text)
        self.assertEqual(len(rows), 2)
        self.assertEqual([r["status"] for r in rows], ["missing", "unrelated"])
        self.assertEqual(rows[1]["category"], "sorted-collections")
        self.assertEqual(rows[0]["are"]["template"]["text"], "(= expected expr)")
        self.assertEqual([x["text"] for x in rows[0]["are"]["arguments"]], ["[]", "(vec nil)"])
        self.assertEqual(rows[0]["are"]["row_source"]["text"], "[] (vec nil)")
        self.assertEqual(rows[0]["assertion"], rows[1]["assertion"])
        self.assertNotEqual(rows[0]["id"], rows[1]["id"])
        with self.assertRaisesRegex(ValueError, "Incomplete are row"):
            self.generate("(deftest bad (are [x y] (= x y) 1))")

    def test_inactive_branches_and_nested_thrown_are_counted_honestly(self):
        text = """(deftest active
          (is (thrown? js/Error (f)))
          #_(is false)
          '(is false)
          (comment (is false))
          #?(:clj (is false) :cljs (is true)))"""
        _, manifest, rows, _ = self.generate(text)
        self.assertEqual(len(rows), 6)
        self.assertEqual(manifest["counts"]["active_assertions"], 2)
        self.assertEqual([r["inactive_reason"] for r in rows],
                         [None, "reader-discarded", "quoted-data", "comment-form", "non-cljs-reader-branch", None])

    def test_assert_and_quantified_property_are_not_omitted(self):
        _, _, rows, _ = self.generate("(deftest ordinary (assert (not= 1 2))) (defspec property 10 (prop/for-all [b gen/boolean] (boolean? b)))")
        self.assertEqual([r["kind"] for r in rows], ["assert", "property"])
        self.assertIn("prop/for-all", rows[1]["assertion"]["text"])

    def test_scope_distinguishes_local_dependencies_and_quoted_data(self):
        text = """(defmulti bar identity)
          (deftest mixed
            (let [plain #{1} sorted (sorted-set 1)]
              (is (= plain #{1}))
              (is (= sorted #{1})))
            (is (= (vary-meta #'first assoc :x 'bar) #'first))
            (is (bar 1))
            (is (= [2] (map inc [1])))
            (is (map inc)))"""
        _, _, rows, _ = self.generate(text)
        self.assertEqual([r["category"] for r in rows], ["source-language", "sorted-collections", "source-language",
                                                         "multimethods-hierarchies", "source-language", "transducers"])

    def test_rebound_local_keeps_outer_dependency_without_tainting_siblings(self):
        text = """(deftest bindings
          (let [xs (sorted-set 1) ordinary #{1}]
            (let [old xs xs (conj xs 2) ordinary (conj ordinary 2)]
              (is (= ordinary #{1 2}))
              (is (= xs #{1 2}))
              (is (= old #{1}))))
          (let [m (transient {}) m (persistent! m)] (is (= m {}))))"""
        _, _, rows, _ = self.generate(text)
        self.assertEqual([r["category"] for r in rows],
                         ["source-language", "sorted-collections", "sorted-collections", "transient-collections"])

    def test_iteration_and_destructuring_track_only_selected_components(self):
        text = """(deftest bindings
          (doseq [m [(hash-map) (sorted-map)]] (is (empty? m)))
          (doseq [ctor [hash-map sorted-map]] (is (empty? (ctor))))
          (let [rows [[{} (sorted-map)]]]
            (doseq [[plain ordered] rows :let [alias plain]]
              (is (empty? plain)) (is (empty? ordered)) (is (empty? alias))))
          (let [[plain [ordered] & [other] :as whole] [{} [(sorted-map)] {}]]
            (is (empty? plain)) (is (empty? ordered)) (is (empty? other)) (is (seq whole)))
          (let [[_ absent] [(sorted-map)]] (is (nil? absent)))
          (let [{:keys [plain ordered]} {:plain {} :ordered (sorted-map)}]
            (is (empty? plain)) (is (empty? ordered)))
          (let [{plain :plain ordered :ordered} {:plain {} :ordered (sorted-map)}]
            (is (empty? plain)) (is (empty? ordered)))
          (for [[plain ordered] [[{} (sorted-map)]]]
            (is (empty? plain)) (is (empty? ordered))))"""
        _, _, rows, _ = self.generate(text)
        self.assertEqual([r["category"] for r in rows], [
            "sorted-collections", "sorted-collections",
            "source-language", "sorted-collections", "source-language",
            "source-language", "sorted-collections", "source-language", "sorted-collections",
            "source-language", "source-language", "sorted-collections",
            "source-language", "sorted-collections", "source-language", "sorted-collections"])

    def test_default_lite_false_branches_are_preserved_but_not_counted_active(self):
        text = """(deftest profile
          (if-not ^boolean LITE_MODE (is true) (is false))
          (if cljs.core/LITE_MODE (is false) (is true))
          (when LITE_MODE (is false) (is false))
          (when-not ^boolean LITE_MODE (is true))
          (let [LITE_MODE true] (when LITE_MODE (is true))))"""
        _, manifest, rows, bodies = self.generate(text)
        self.assertEqual([r["active"] for r in rows], [True, False, False, True, False, False, True, True])
        self.assertEqual(manifest["counts"]["assertions"], 8)
        self.assertEqual(manifest["counts"]["active_assertions"], 4)
        self.assertEqual(bodies[0]["source_span"]["text"], text)
        self.assertTrue(all(r["inactive_reason"] == "non-default-lite-mode-branch" for r in rows if not r["active"]))

    def test_lexical_lite_name_is_not_the_default_build_flag(self):
        text = """(deftest profile
          (let [[LITE_MODE] [true]] (when LITE_MODE (is true)))
          (fn [LITE_MODE] (when LITE_MODE (is true)))
          (doseq [x [true] :let [LITE_MODE x]] (when LITE_MODE (is true)))
          (let [LITE_MODE true] (when cljs.core/LITE_MODE (is false))))"""
        _, _, rows, _ = self.generate(text)
        self.assertEqual([r["active"] for r in rows], [True, True, True, False])

    def test_lite_collection_apis_do_not_exclude_ordinary_sibling_assertions(self):
        text = """(deftest test-set-lite
          (is (= #{1} #{1}))
          (let [ordinary #{} specialized (set-lite [])]
            (is (empty? ordinary)) (is (empty? specialized)))
          (is (empty? (obj-map)))
          (is (empty? (hash-map-lite)))
          (is (some? (ObjMap. nil (array) (js-obj))))
          (is (= 'sorted-map 'sorted-map))
          (is (= 'obj-map 'obj-map)))"""
        _, _, rows, _ = self.generate(text)
        self.assertEqual([r["category"] for r in rows],
                         ["source-language", "source-language"] + ["lite-profile"] * 4 + ["source-language"] * 2)
        self.assertTrue(all(r["active"] for r in rows))

    def test_direct_analyzer_helpers_do_not_exclude_public_namespace_semantics(self):
        text = """(deftest test-cljs-975
                    (is (= (ana/desugar-ns-specs '((:require foo))) '((:require foo)))))
                  (deftest test-cljs-1992 (is (:fn-var cached-definition)))
                  (deftest test-cljs-1508-rename
                    (is (= (:renames (analyze env '(ns x (:refer-clojure :rename {map m}))))
                           '{m cljs.core/map})))
                  (deftest spec-validation
                    (is (thrown? Exception (analyze env '(ns x (:require {:x :y}))))))
                  (deftest test-require-global
                    (is (= (:uses parsed-ns) '{createElement React})))"""
        _, _, rows, bodies = self.generate(text, "src/test/clojure/cljs/analyzer_tests.clj")
        self.assertEqual([row["category"] for row in rows],
                         ["upstream-internals", "upstream-internals",
                          "source-language", "source-language", "source-language"])
        self.assertEqual(len(bodies), 5)
        self.assertTrue(all(row["status"] == "missing" for row in rows[2:]))

    def test_rename_storage_absence_does_not_exclude_resolution(self):
        text = """(deftest test-cljs-1508-rename
          (is (nil? (-> parsed-ns :uses (get 'foo))))
          (is (nil? (-> parsed-ns :uses (get 'intersection))))
          (is (some? (-> parsed-ns :renames (get 'foo))))
          (is (= (-> parsed-ns :renames (get 'foo)) 'clojure.set/intersection))
          (is (nil? (-> parsed-ns :uses (get 'other))))
          (is (nil? (resolve 'intersection))))"""
        _, _, rows, bodies = self.generate(text, "src/test/clojure/cljs/analyzer_tests.clj")
        self.assertEqual([row["category"] for row in rows],
                         ["upstream-internals"] * 2 + ["source-language"] * 4)
        self.assertEqual(bodies[0]["source_span"]["text"], text)
        self.assertEqual(len(rows), 6)

    def test_emitted_code_spelling_exclusion_retains_binding_diagnostics(self):
        text = """(deftest test-js-negative-infinity
                    (is (= (with-out-str (emit (analyze env 'js/-Infinity))) "-Infinity")))
                  (deftest test-cljs-2352
                    (are [form expected] (= (emit-string form) expected)
                      Double/NaN "NaN" Double/POSITIVE_INFINITY "Infinity"))
                  (deftest or-doesnt-create-bindings (is (= warning :undeclared-var)))
                  (deftest keys-bang (is (undeclared-warning? form 'b)))
                  (deftest select-or-defaults (is (thrown? Exception (compile-simple-form form))))"""
        _, _, rows, _ = self.generate(text, "src/test/clojure/cljs/compiler_tests.clj")
        self.assertEqual([row["category"] for row in rows],
                         ["upstream-internals"] * 3 + ["source-language"] * 3)

    def test_snapshot_omission_changes_regeneration_and_check_fails(self):
        original, _, _, _ = self.generate("(deftest t (is true) (is false))")
        omitted, _, _, _ = self.generate("(deftest t (is true))")
        self.assertNotEqual(original["inventory.jsonl"], omitted["inventory.jsonl"])
        self.assertNotEqual(original["bodies.jsonl"], omitted["bodies.jsonl"])
        for filename in ("inventory.jsonl", "bodies.jsonl", "manifest.json"):
            with self.subTest(filename=filename), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                for name, data in original.items():
                    (root / name).write_bytes(data)
                (root / filename).write_bytes(omitted[filename])
                with patch.object(inv, "HERE", root), patch.object(inv, "inventory", return_value=(original, {})), patch.object(full_ledger, "generate", return_value=({}, {})), \
                     patch.object(sys, "argv", ["extract_inventory.py", "--check"]), contextlib.redirect_stderr(io.StringIO()) as err:
                    self.assertEqual(inv.main(), 1)
                    self.assertIn(filename, err.getvalue())

    def test_modified_upstream_working_bytes_fail_even_if_snapshot_is_regenerated(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "source.cljs").write_text("(deftest t (is false))")
            with patch.object(inv, "UPSTREAM", root), patch.object(inv, "FILES", ("source.cljs",)), \
                 patch.object(inv.subprocess, "check_output", return_value=b"(deftest t (is true))"):
                with self.assertRaisesRegex(ValueError, "working source differs from pin"):
                    inv.inventory()

    def test_invalid_structural_source_fails_instead_of_losing_tail(self):
        for source in ('(is true', '(is ] true)', '(is "unterminated)', "#", "#?(:cljs)"):
            with self.subTest(source=source), self.assertRaises(ValueError):
                self.generate(source)


if __name__ == "__main__":
    unittest.main()
