#!/usr/bin/env python3
"""Reject IFn declaration counts, fake receivers and changed full observations."""
import json
from pathlib import Path
import unittest
from unittest.mock import patch
import ifn_public_context as cohort

class IfnPublicContractTests(unittest.TestCase):
 def test_definitions_and_native_functions_supply_no_receiver_demands(self):
  self.assertEqual(cohort.receiver_demands('(reify IFn (-invoke [this x] x))'),[])
  self.assertEqual(cohort.receiver_demands('(let [receiver (fn [x] x) saved receiver] (receiver 1) (saved 2))'),[])
  self.assertEqual(cohort.receiver_demands("'(let [receiver (reify IFn (-invoke [this x] x))] (receiver 1))"),[])
 def test_exact_genuine_receiver_slot_and_saved_copy_demands(self):
  source='(let [receiver (reify IFn (-invoke [this x] x)) copy (with-meta receiver {:a 1}) saved copy] [(receiver 1) (saved 2) (apply copy [3])])'
  demands=cohort.receiver_demands(source)
  self.assertEqual([r['public_receiver_arity'] for r in demands],[2,2,2])
  self.assertEqual([r['alias'] for r in demands],['receiver','saved','copy'])
  self.assertEqual(len({r['method_source_sha256'] for r in demands}),1)
 def test_missing_receiver_method_cannot_supply_a_signature(self):
  with self.assertRaises(ValueError):cohort.receiver_demands('(let [receiver (reify IFn (-invoke [this] :zero))] (receiver 1))')
 def test_terminal_apply_is_bound_to_the_real_rest_slot(self):
  rows=cohort.rows();row=next(r for r in rows if r['id']=='callable-receiver-rest-22')
  self.assertEqual([r['public_receiver_arity'] for r in row['receiver_demands']],[22,22,22,22])
  self.assertEqual([r['user_argument_count'] for r in row['receiver_demands']],[21,21,22,21])
  self.assertEqual(len({r['method_source_sha256'] for r in row['receiver_demands']}),1)
 def test_wrong_explicit_slot_or_formal_count_is_rejected(self):
  for call in ['(.call (aget receiver "cljs$core$IFn$_invoke$arity$1") receiver)', '(.call (aget receiver "cljs$core$IFn$_invoke$arity$2") receiver 1)']:
   with self.assertRaises(ValueError):cohort.receiver_demands('(let [receiver (reify IFn (-invoke [this x] x))] '+call+')')
 def test_nested_terminal_apply_tail_is_rejected_without_relabeling(self):
  row=next(r for r in cohort.rows() if r['id']=='callable-receiver-rest-22')
  source=row['source'].replace('19 20 21 22])','19 20 (list 21 22)])')
  self.assertNotEqual(source,row['source'])
  with self.assertRaises(ValueError):cohort.receiver_demands(source)
 def test_five_whole_sources_and_literals_are_frozen(self):
  rows=cohort.rows();self.assertEqual(len(rows),5)
  original=(cohort.ROOT/cohort.FIXTURE).read_text();lines=original.splitlines();fields=lines[1].split('\t');real=Path.read_text
  for column in [2,3]:
   changed=fields[:];changed[column]='(do 1)'.encode().hex()
   if column==2:changed[4]=json.dumps({'source_sha256':cohort.digest('(do 1)')})
   text='\n'.join([lines[0],'\t'.join(changed),*lines[2:]])+'\n'
   def read(path,*args,**kwargs):return text if path==cohort.ROOT/cohort.FIXTURE else real(path,*args,**kwargs)
   with patch.object(Path,'read_text',read),self.assertRaises(ValueError):cohort.rows()
 def test_all_canonical_slots_have_literal_oracles_and_actual_phase_tests(self):
  exported=cohort.exports();ordinary=exported['obligation-witnesses.jsonl'];phases=exported['phase-witnesses.jsonl'];cases={r['id']:r for r in exported['full-cases.jsonl']}
  self.assertEqual(len(ordinary),25)
  public=[r for r in ordinary if '::-invoke::arity-' in r['obligation_id']]
  self.assertEqual(len(public),22)
  self.assertEqual({r['reference_fixture_id'] for r in ordinary},set(cases))
  for row,phase in zip(ordinary,phases):
   self.assertEqual(row['local_execution'],{k:phase[k] for k in ['test_file','test_name','test_source_sha256']})
   self.assertEqual(phase['test_source_sha256'],cohort.digest((cohort.ROOT/cohort.DRIVER).read_bytes()))
   self.assertEqual(row['reference_source_sha256'],cohort.digest(cases[row['reference_fixture_id']]['source']))
   self.assertIn(cohort.FIXTURE,phase['support_source_sha256'])
 def test_failure_source_preserves_full_metadata_receiver_and_ordered_error_data(self):
  row=cohort.rows()[-1]
  self.assertIn('(throw (ex-info "receiver failure" {:receiver label :value x}))',row['source'])
  self.assertIn('(finally (swap! events conj :finally))',row['source'])
  self.assertIn('(with-meta receiver {:tag :copied})',row['source'])
  self.assertIn('[:dispatch true :initial :fail] :finally [:dispatch true :initial :after]',row['expected_edn'])
if __name__=='__main__':unittest.main()
