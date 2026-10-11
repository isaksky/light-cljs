"""Fail-closed tests for the exact configured source-reader audit protocol."""
import copy
import json
import unittest
import configured_source_context as adapter

class ConfiguredSourceProtocol(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.case = adapter.load()

    def raw(self):
        rows = []
        for variant in adapter.VARIANTS:
            source = adapter.macro_source(self.case['source']) if variant.startswith('macro') else self.case['source']
            values = [variant, source.encode().hex(), 'const rt = // Original bounded runtime\nnull'.encode().hex(), json.dumps(adapter.EXPECTED).encode().hex(), '31' if variant.endswith('reused') else '-']
            rows.append('\t'.join(values))
        return '\n'.join(rows) + '\n'

    def test_rejects_missing_changed_or_extra_reader_profile(self):
        for config in (None, [], [{'tag': 'cljs/tag', 'operation': 'inc'}], [{'tag': 'cljs/tag', 'operation': 'identity'}, {'tag': 'cljs/inc', 'operation': 'inc'}]):
            case = copy.deepcopy(self.case); case['compiler_data_readers'] = config
            with self.assertRaises(ValueError): adapter.validate(case)
        case = copy.deepcopy(self.case); del case['original_data_reader_configuration']
        with self.assertRaises(ValueError): adapter.validate(case)

    def test_rejects_reduced_original_body_or_changed_literal_expectation(self):
        for source in ('(defn run [] true)', self.case['source'].replace('[1 2 3]))', '[1 2 4]))')):
            case = copy.deepcopy(self.case); case['source'] = source
            with self.assertRaises(ValueError): adapter.validate(case)
        case = copy.deepcopy(self.case); case['expected']['outcome']['value'] = 1
        with self.assertRaises(ValueError): adapter.validate(case)

    def test_ordered_complete_raw_rows_and_actual_warmup_required(self):
        self.assertEqual(len(adapter.parse_raw(self.raw(), self.case)), 4)
        lines = self.raw().splitlines()
        for malformed in (lines[:-1], list(reversed(lines)), lines + [lines[0]], lines[:2] + [lines[1]] + lines[3:]):
            with self.assertRaises(ValueError): adapter.parse_raw('\n'.join(malformed), self.case)
        missing = [line.rsplit('\t', 1)[0] + '\t-' for line in lines]
        with self.assertRaises(ValueError): adapter.parse_raw('\n'.join(missing), self.case)

    def test_complete_phase_source_and_strict_raw_outcome_required(self):
        lines = self.raw().splitlines()
        first = lines[0].split('\t')
        for observed in ({'id': adapter.ID, 'outcome': {'status': 'returned', 'value': True}}, {**adapter.EXPECTED, 'outcome': {'status': 'returned', 'value': 1}}, {**adapter.EXPECTED, 'stdout': 'hidden effect'}, {**adapter.EXPECTED, 'trace': [1]}):
            changed = first.copy(); changed[3] = json.dumps(observed).encode().hex()
            with self.assertRaises(ValueError): adapter.parse_raw('\n'.join(['\t'.join(changed)] + lines[1:]), self.case)
        changed = first.copy(); changed[1] = '(defn run [] true)'.encode().hex()
        with self.assertRaises(ValueError): adapter.parse_raw('\n'.join(['\t'.join(changed)] + lines[1:]), self.case)

    def test_source_compiler_build_producer_drift_rejected(self):
        before = {'src/compiler.rs': 'old', 'Cargo.lock': 'lock', 'producer.py': 'producer'}
        for name in before:
            after = dict(before); after[name] = 'changed'
            with self.assertRaises(ValueError): adapter.unchanged(before, after)
        with self.assertRaises(ValueError): adapter.unchanged(before, {**before, 'unexpected': 'source'})
        adapter.unchanged(before, before.copy())

if __name__ == '__main__': unittest.main()
