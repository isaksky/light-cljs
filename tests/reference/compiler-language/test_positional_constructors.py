"""Complete-source constructor contracts and fail-closed producer guards."""
import copy
import unittest
from unittest.mock import patch
import positional_constructors as constructors


class PositionalConstructorEvidence(unittest.TestCase):
    def test_original_complete_source_and_two_pin_contracts(self):
        rows, negative = constructors.load()
        self.assertEqual([22, 1], [len(rows), len(negative)])
        self.assertEqual(constructors.ORIGINAL, rows[0]['source'])
        self.assertEqual({'$vector': [2, 1, 2]}, rows[0]['canonical_expected']['outcome']['value'])
        for row in rows + negative:
            self.assertEqual(constructors.sci.digest(row['source']), row['source_sha256'])
            for phase in ('application', 'macro'):
                self.assertIn('(do '+row['source']+')', constructors.proof.historical.source(row['source'], phase))
        self.assertTrue(all(r['two_pin_agreement'] and r['canonical_acceptance'] for r in rows))
        self.assertEqual(22, len({r['id'] for r in rows}))

    def test_constructor_calls_updates_and_effects_are_independently_frozen(self):
        rows, _ = constructors.load()
        value = lambda name: next(r for r in rows if r['id']=='positional-constructor-'+name)['canonical_expected']['outcome']['value']['$vector']
        self.assertEqual([2, 1, 2], value('original'))
        for name in ('saved', 'apply', 'dynamic'):
            self.assertEqual([1, 2, 1], [value(name)[0], value(name)[1], value(name)[3]])
        self.assertEqual([9, 1, 8], [value('nine')[0], value('nine')[1], value('nine')[3]])
        self.assertEqual([8, 2, 9, 8, True, True, 8, 8], value('promotion'))
        self.assertEqual('2\n11\n', value('operand-effects')[-1])
        self.assertEqual({'$vector': []}, value('equality-effects')[-1])
        self.assertEqual([1, False, {'$vector': [True]}], value('set-updates')[2:5])
        self.assertEqual({'$vector': [{'$vector': [True, True]}]}, value('map-updates')[4])

    def test_source_and_contract_drift_reject(self):
        with patch.object(constructors, 'CONTRACTS_SHA256', '0'*64):
            with self.assertRaises(ValueError): constructors.load()
        changed=list(constructors.CONTROLS); changed[0]=(changed[0][0],changed[0][1]+' ')
        with patch.object(constructors, 'CONTROLS', tuple(changed)):
            with self.assertRaises(ValueError): constructors.load()

    def test_reconcile_requires_actual_current_raw_executable_evidence(self):
        with self.assertRaises(ValueError): constructors.reconcile({'profile':'detached'})
        with self.assertRaises(ValueError): constructors.reconcile({'profile':constructors.PROFILE,'artifacts_root':'relative'})
        with self.assertRaises(FileNotFoundError): constructors.reconcile({'profile':constructors.PROFILE,'artifacts_root':'/private/tmp/missing-177-raw'})


if __name__=='__main__': unittest.main()
