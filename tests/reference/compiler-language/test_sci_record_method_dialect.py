import copy
import json
import unittest
import sci_record_method_dialect as audit


class RecordMethodDialectGuards(unittest.TestCase):
    def raw(self):
        records = []
        for row in audit.load():
            for variant in audit.VARIANTS:
                for engine in ('QuickJS', 'Node'):
                    raw = json.dumps(row['canonical_expected'], separators=(',', ':'))+ ('\n' if engine == 'Node' else '')
                    records.append('\t'.join((row['id'], variant, engine,
                        audit.module(row, variant.split('-')[0]).encode().hex(),
                        ' = // Original bounded runtime'.encode().hex(), raw.encode().hex(),
                        '-' if variant.endswith('fresh') else '74727565')))
        return records

    def test_originals_and_canonical_literals_remain_separate(self):
        rows = audit.load(); audit.validate(rows)
        self.assertTrue(all(r['original_expectation_status'] == 'unresolved' for r in rows))
        for row in rows: self.assertNotEqual(row['original_expected'], row['canonical_expected'])

    def test_no_changed_original_or_canonical_expectation_or_partial_source(self):
        for field, value in [('source', 'true'), ('original_expected', {}), ('canonical_expected', {}), ('setup_source', '')]:
            rows = copy.deepcopy(audit.load()); rows[0][field] = value
            with self.assertRaises(ValueError): audit.validate(rows)
        rows = copy.deepcopy(audit.load()); rows[0]['source_context_profile']['schema_version'] = True
        with self.assertRaises(ValueError): audit.validate(rows)

    def test_all16_actual_ordered_frames_required(self):
        raw = self.raw(); self.assertEqual(len(audit.parse_raw('\n'.join(raw), audit.load())), 16)
        for wrong in [raw[:-1], raw+[raw[0]], raw[::-1]]:
            with self.assertRaises(ValueError): audit.parse_raw('\n'.join(wrong), audit.load())

    def test_original_expected_substitution_does_not_pass_canonical_audit(self):
        raw = self.raw(); fields = raw[0].split('\t')
        fields[-2] = json.dumps(audit.load()[0]['original_expected']).encode().hex(); raw[0] = '\t'.join(fields)
        with self.assertRaises(ValueError): audit.parse_raw('\n'.join(raw), audit.load())

    def test_receiving_recur_removed_or_shortened_source_rejected(self):
        for substitution in [('(p (->R 3) 5)', '(p (->R 3) 1)'), ('(recur (assoc this :x (inc x)) (dec n))', 'x')]:
            raw = self.raw(); fields = raw[0].split('\t')
            fields[3] = bytes.fromhex(fields[3]).decode().replace(*substitution).encode().hex(); raw[0] = '\t'.join(fields)
            with self.assertRaises(ValueError): audit.parse_raw('\n'.join(raw), audit.load())

    def test_both_pin_original_status_must_remain_mismatch(self):
        row = audit.load()[0]; r = audit.sci.report_module()
        layer = {'status': 'mismatch', 'observed': row['canonical_expected'], 'reference_identity': 'release '+r.RELEASE,
            'settings': {'optimizations': 'none', 'static-fns': False, 'def-emits-var': False}}
        audit.checked_reference(row, layer, 'release')
        for key, value in [('status', 'passed'), ('observed', row['original_expected']), ('reference_identity', 'release unverified')]:
            wrong = copy.deepcopy(layer); wrong[key] = value
            with self.assertRaises(ValueError): audit.checked_reference(row, wrong, 'release')

    def test_status_only_sidecar_never_grants_original_or_canonical_credit(self):
        with self.assertRaises(ValueError): audit.reconcile({'status': 'passed'}, '/tmp/no-record-method-raw-proof')

    def test_reused_phase_must_retain_completed_actual_warmup(self):
        raw = self.raw(); fields = raw[4].split('\t'); fields[-1] = '-'; raw[4] = '\t'.join(fields)
        with self.assertRaises(ValueError): audit.parse_raw('\n'.join(raw), audit.load())


if __name__ == '__main__': unittest.main()
