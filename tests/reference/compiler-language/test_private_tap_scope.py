import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import extract_inventory as inv
import private_tap_scope as audit


class PrivateTapScopeGuards(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.sources = {path: audit._pinned(path) for path in audit.SOURCE_SHA256}

    def changed_source(self, path, original, changed):
        sources = dict(self.sources)
        self.assertIn(original.encode(), sources[path])
        sources[path] = sources[path].replace(original.encode(), changed.encode(), 1)
        # Reach structural/frozen-span checks after deliberate whole-file change.
        hashes = {name: inv.digest(data) for name, data in sources.items()}
        return sources, hashes

    def reject_changed(self, path, original, changed, message):
        sources, hashes = self.changed_source(path, original, changed)
        with patch.object(audit, '_pinned', side_effect=sources.__getitem__), \
             patch.object(audit, 'SOURCE_SHA256', hashes):
            with self.assertRaisesRegex(ValueError, message):
                audit.validate()

    def test_only_exact_two_private_assertions_without_pass_credit(self):
        rows = audit.validate()
        self.assertEqual([row['id'] for row in rows], [a[0] for a in audit.ASSERTIONS])
        for row, (_, text, digest) in zip(rows, audit.ASSERTIONS):
            self.assertEqual(row['assertion']['text'], text)
            self.assertEqual(row['assertion']['sha256'], digest)
            self.assertEqual(row['body_sha256'], audit.BODY_SHA256)
            self.assertEqual(row['disposition'], 'unrelated')
            self.assertEqual(row['category'], 'upstream-internals')
            self.assertIs(row['semantic_pass_credit'], False)
            self.assertEqual(row['execution_evidence'], 'not-run')
            self.assertEqual(row['scope_basis'], audit.SCOPE_BASIS)

    def test_checkout_revision_and_whole_file_hash_are_frozen(self):
        with patch.object(inv, 'REVISION', 'unreviewed-pin'):
            with self.assertRaisesRegex(ValueError, 'revision drift'):
                audit.validate()
        sources = dict(self.sources)
        sources[audit.CORE_PATH] += b'\n'
        with patch.object(audit, '_pinned', side_effect=sources.__getitem__):
            with self.assertRaisesRegex(ValueError, 'pinned source hash drift'):
                audit.validate()

    def test_working_checkout_must_match_git_pin(self):
        with tempfile.TemporaryDirectory() as folder:
            upstream = Path(folder)
            path = upstream / audit.CORE_PATH
            path.parent.mkdir(parents=True)
            path.write_bytes(b'unreviewed working source')
            with patch.object(inv, 'UPSTREAM', upstream), \
                 patch.object(audit.subprocess, 'check_output', return_value=self.sources[audit.CORE_PATH]):
                with self.assertRaisesRegex(ValueError, 'working source differs'):
                    audit._pinned(audit.CORE_PATH)

    def test_private_metadata_cannot_be_removed_or_false(self):
        for changed in ('^{}\n  tapset nil', '^:dynamic\n  tapset nil',
                        '^{:jsdoc ["@type {*}"] :private false}\n  tapset nil'):
            self.reject_changed(audit.CORE_PATH,
                '^{:jsdoc ["@type {*}"] :private true}\n  tapset nil', changed,
                'explicit :private true')

    def test_private_declaration_and_complete_setup_cleanup_are_frozen(self):
        self.reject_changed(audit.CORE_PATH, audit.DECLARATION,
            audit.DECLARATION.replace('tapset nil', 'tapset (atom #{})'), 'declaration drift')
        for before, after in (('(add-tap tap)', '(identity tap)'),
                              ('(remove-tap tap)', '(identity tap)'),
                              ('(fn [_])', '(fn [_] :changed)')):
            self.reject_changed(audit.TEST_PATH, audit.BODY,
                audit.BODY.replace(before, after), 'complete body drift')

    def test_assertion_literals_count_and_order_are_frozen(self):
        for body in (audit.BODY.replace('(set? @tapset)', '(set? #{})'),
                     audit.BODY.replace('(contains? @tapset tap)', '(contains? #{} tap)'),
                     audit.BODY.replace('    (is (set? @tapset))\n', ''),
                     audit.BODY.replace('    (remove-tap tap)', '    (is true)\n    (remove-tap tap)'),
                     audit.BODY.replace('(is (set? @tapset))', '(is (contains? @tapset tap))', 1)):
            self.reject_changed(audit.TEST_PATH, audit.BODY, body, 'complete body drift')

    def test_discarded_or_duplicate_body_is_not_an_active_exact_target(self):
        for replacement in ('#_' + audit.BODY, audit.BODY + '\n' + audit.BODY):
            self.reject_changed(audit.TEST_PATH, audit.BODY, replacement,
                                'one active exact declaration/body')

    def test_public_tap_declarations_and_public_assertion_are_guarded(self):
        self.reject_changed(audit.CORE_PATH, '(defn add-tap', '(defn- add-tap',
                            'one active exact declaration/body')
        self.reject_changed(audit.TEST_PATH, audit.PUBLIC_TEST,
                            audit.PUBLIC_TEST.replace('(tap> nil)', 'true'), 'public tap> test drift')
        obligations = [json.loads(line) for line in (inv.HERE / 'obligations.jsonl').read_text().splitlines()]
        public = [row for row in obligations if row['name'] in audit.PUBLIC_DECLARATIONS]
        self.assertEqual(len(public), 6)
        self.assertTrue(all(row['disposition'] == 'intended' for row in public))
        inventory = [json.loads(line) for line in (inv.HERE / 'full-inventory.jsonl').read_text().splitlines()]
        tap_delivery = next(row for row in inventory if row['id'] == 'core_test.cljs::test-cljs-2552::assertion-001')
        self.assertEqual(tap_delivery['disposition'], 'in-scope')
        self.assertNotIn(tap_delivery['id'], {row['id'] for row in audit.validate()})

    def test_release_receipt_is_recorded_without_reading_maven_artifact(self):
        self.assertEqual(audit.RELEASE_EVIDENCE['declaration_sha256'], audit.DECLARATION_SHA256)
        self.assertEqual(audit.RELEASE_EVIDENCE['version'], '1.12.134')
        with patch.object(audit, '_pinned', side_effect=self.sources.__getitem__) as pinned, \
             patch.object(Path, 'open', side_effect=AssertionError('unexpected artifact/file dependency')):
            self.assertEqual(len(audit.validate()), 2)
            self.assertEqual([call.args[0] for call in pinned.call_args_list],
                             list(audit.SOURCE_SHA256))


if __name__ == '__main__':
    unittest.main()
