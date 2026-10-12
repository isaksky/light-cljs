import unittest
import native_api_retirement as audit


class NativeApiRetirementGuards(unittest.TestCase):
    def test_only_five_exact_original_rows_retire_without_pass_credit(self):
        rows = audit.load()
        self.assertEqual([r['name'] for r in rows], list(audit.REVIEWED))
        self.assertTrue(all(r['disposition'] == 'dialect' and r['semantic_pass_credit'] is False for r in rows))

    def test_historical_original_fields_cannot_be_rewritten_native_only(self):
        originals = (audit.sci.ROOT/audit.PATH).read_text().splitlines()
        line = next(line for line in originals if line.startswith('clojure.core/bigdec\t'))
        for changed in [line.replace('unsupported-explicit', 'native-only'), line+'\textra']:
            with self.assertRaises(ValueError): audit.review(audit.PATH, changed)

    def test_other_path_or_ordinary_predicate_never_retires(self):
        originals = (audit.sci.ROOT/audit.PATH).read_text().splitlines()
        line = next(line for line in originals if line.startswith('clojure.core/ratio?\t'))
        self.assertIsNone(audit.review('src/core_var_metadata.tsv', line))
        for name in ['number?', 'integer?', 'int?', 'float?', 'double?', 'zero?', 'pos?', 'neg?', 'even?', 'odd?', 'pos-int?', 'neg-int?', 'nat-int?', 'js/BigInt']:
            self.assertIsNone(audit.review(audit.PATH, name+'\tfunction\tsci-exact'))

    def test_mixed_original_sci_body_is_not_a_retirement_target(self):
        raw, fields = audit.sci.rows(audit.sci.DISPOSITIONS)['sci-native-numeric-predicates']
        self.assertEqual(fields[7], 'gap')
        self.assertIsNone(audit.review('tests/reference/sci/evaluator-dispositions.tsv', raw))
        self.assertIn('(number? 1/2)', fields[2])
        self.assertIn('(even? -4)', fields[2])

    def test_diagnostic_proof_never_accepts_status_only(self):
        with self.assertRaises((ValueError, OSError)):
            audit.validate_diagnostics({'status': 'passed'}, '/tmp/no-such-numeric-diagnostic-binary')


if __name__ == '__main__': unittest.main()
