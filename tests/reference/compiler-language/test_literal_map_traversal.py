"""Exact source-bound, independently frozen map packets and fail-closed replay."""
import copy
import json
import unittest
from unittest.mock import patch
import literal_map_traversal as literal


class LiteralMapTraversalGuards(unittest.TestCase):
    def test_complete_exact_catalog_and_original_order(self):
        rows=literal.load()
        self.assertEqual(39,len(rows))
        self.assertEqual(literal.ORIGINAL,rows[0]['source'])
        self.assertEqual(literal.ORIGINAL_SHA256,rows[0]['source_sha256'])
        for row in rows:
            self.assertEqual(literal.sci.digest(row['source']),row['source_sha256'])
            self.assertEqual([],row['canonical_expected']['trace'])
            self.assertEqual('',row['canonical_expected']['stdout'])
            for phase in ('application','macro'):
                self.assertIn('(do '+row['source']+')',literal.proof.historical.source(row['source'],phase))
        value=rows[0]['canonical_expected']['outcome']['value']['$vector']
        self.assertEqual(9,value[0])
        self.assertEqual(list('egchbdfia'),[k['$keyword']['name'] for k in value[1]['$vector']])

    def test_thresholds_composites_effects_and_zero_credit_neighbor(self):
        rows={r['id'].removeprefix('literal-map-traversal-'):r for r in literal.load()}
        runtime_map_names={'9-key-effects','9-key-value-effects','16-key-value-effects',
                           'scalar-mix-nine','same-hash-numeric-nine','signed-zero-quoted',
                           'nonfinite-nine','vector-keys-nine','list-keys-nine','map-keys-nine',
                           'set-keys-nine','mixed-composite-keys','metadata-composite-keys',
                           'runtime-hash-map-sequence-nonpassing'}
        self.assertEqual(runtime_map_names,{name for name,row in rows.items()
                                          if not row['canonical_acceptance'] and name!='runtime-set-sequence-nonpassing'})
        self.assertEqual(24,sum(row['canonical_acceptance'] for row in rows.values()))
        for name in ('executable-8-entries','executable-9-entries','executable-16-entries',
                     'quoted-8-entries','quoted-9-entries','quoted-16-entries',
                     '8-value-effects','9-value-effects','16-value-effects',
                     '8-key-effects','9-key-effects','8-key-value-effects',
                     '9-key-value-effects','16-key-value-effects',
                     'scalar-mix-nine','same-hash-strings-nine','same-hash-numeric-nine',
                     'nil-late-nine','signed-zero-quoted','nonfinite-nine',
                     'vector-keys-nine','list-keys-nine','map-keys-nine','set-keys-nine',
                     'mixed-composite-keys','nested-map-values','quoted-metadata-nine',
                     'executable-metadata-nine','metadata-composite-keys',
                     'dynamic-duplicate-keys','dynamic-duplicate-vector-keys',
                     'metadata-8-value-effects','metadata-9-value-effects',
                     '9-key-value-effects-only','16-key-value-effects-only'):
            self.assertEqual(name not in runtime_map_names,rows[name]['canonical_acceptance'],name)
            self.assertTrue(rows[name]['two_pin_agreement'],name)
        witness=rows['sixteen-even-key-operand-effects']['canonical_expected']['outcome']['value']['$vector']
        self.assertEqual(16,witness[0])
        self.assertEqual([14,16,12,30,2,20,18,24,10,26,0,22,8,6,28,4,15,17,13,31,3,21,19,25,11,27,1,23,9,7,29,5],witness[1]['$vector'])
        effects=rows['9-key-value-effects']['canonical_expected']['outcome']['value']['$vector'][-2]['$vector']
        self.assertEqual(['k2','k5','k3','k7','k0','k6','k1','k8','k4','v2','v5','v3','v7','v0','v6','v1','v8','v4'],[x['$keyword']['name'] for x in effects])
        for count,order in ((8,'abcdefgh'),(9,'egchbdfia')):
            value=rows['metadata-'+str(count)+'-value-effects']['canonical_expected']['outcome']['value']['$vector']
            self.assertEqual(count,value[1])
            self.assertEqual(list(order),[k['$keyword']['name'] for k in value[2]['$vector']])
        for count in (9,16):
            full=rows[str(count)+'-key-value-effects']
            effect=rows[str(count)+'-key-value-effects-only']
            self.assertFalse(full['canonical_acceptance'])
            self.assertTrue(effect['canonical_acceptance'])
            self.assertIn('(vec (seq m))',full['source'])
            self.assertNotIn('(vec (seq m))',effect['source'])
            full_value=full['canonical_expected']['outcome']['value']['$vector']
            effect_value=effect['canonical_expected']['outcome']['value']['$vector']
            self.assertEqual([count,0,True],effect_value[:3])
            self.assertEqual(full_value[-2:],effect_value[-2:])
        runtime_map=rows['runtime-hash-map-sequence-nonpassing']
        self.assertFalse(runtime_map['canonical_acceptance'])
        self.assertTrue(runtime_map['two_pin_agreement'])
        for field,order in (('canonical_expected','egchbdfia'),('local_expected','abcdefghi')):
            self.assertEqual(list(order),[p['$vector'][0]['$keyword']['name']
                                        for p in runtime_map[field]['outcome']['value']['$vector']])
        runtime=rows['runtime-set-sequence-nonpassing']
        self.assertFalse(runtime['canonical_acceptance'])
        self.assertTrue(runtime['two_pin_agreement'])
        self.assertIn('Pellet180',runtime['nonpassing_reason'])
        self.assertEqual(list(range(16)),runtime['canonical_expected']['outcome']['value']['$vector'])
        self.assertEqual([14,3,12,9,2,15,5,10,7,0,13,8,1,6,4,11],runtime['local_expected']['outcome']['value']['$vector'])

    def test_no_typed_packet_projection(self):
        row=literal.load()[0]
        packet=copy.deepcopy(row['canonical_expected'])
        packet['outcome']['value']['$vector'][1]={'$seq':packet['outcome']['value']['$vector'][1]['$vector']}
        self.assertEqual('mismatch',literal.exact_observation(row,packet)['status'])
        list_row=next(r for r in literal.load() if r['id'].endswith('list-keys-nine'))
        packet=copy.deepcopy(list_row['canonical_expected'])
        key=packet['outcome']['value']['$vector'][2]['$vector'][0]['$vector'][0]
        key['$seq']=key.pop('$list')
        self.assertEqual('mismatch',literal.exact_observation(list_row,packet)['status'])
        self.assertTrue(literal.proof.report.same_json(literal.sci.projection(packet),literal.sci.projection(list_row['canonical_expected'])))
        packet=copy.deepcopy(row['canonical_expected'])
        packet['trace']=['extra']
        self.assertEqual('mismatch',literal.exact_observation(row,packet)['status'])
        packet=copy.deepcopy(row['canonical_expected'])
        packet['stdout']='extra\n'
        self.assertEqual('mismatch',literal.exact_observation(row,packet)['status'])

    def test_rejected_derivation_remains_nonpassing(self):
        frozen=json.loads(literal.CONTRACTS.read_text())
        controls=frozen['rejected_derivation_controls']
        self.assertEqual(1,len(controls))
        row=controls[0]
        self.assertIn('[1 2] :vector (1 2) :list',row['source'])
        self.assertFalse(row['canonical_acceptance'])
        self.assertEqual(0,row['two_pin_agreement_credit'])
        for kind in ('release','checkout'):
            self.assertEqual('rejected',row['references'][kind]['status'])
            self.assertEqual('Duplicate key: (1 2)',row['references'][kind]['detail'])

    def test_contract_hash_and_source_drift_fail_closed(self):
        with patch.object(literal,'CONTRACTS_SHA256','0'*64):
            with self.assertRaises(ValueError):literal.load()
        with patch.object(literal,'ORIGINAL',literal.ORIGINAL+' '):
            with self.assertRaises(ValueError):literal.load()
        with patch.object(literal,'SOURCE_COUNT',33):
            with self.assertRaises(ValueError):literal.load()

    def test_reconciliation_requires_raw_artifacts(self):
        with self.assertRaises(ValueError):literal.reconcile({'profile':literal.PROFILE,'artifacts_root':'relative'})
        missing=str(literal.Path('/tmp/literal-map-traversal-missing-raw').resolve())
        with self.assertRaises(FileNotFoundError):literal.reconcile({'profile':literal.PROFILE,'artifacts_root':missing})


if __name__=='__main__':unittest.main()
