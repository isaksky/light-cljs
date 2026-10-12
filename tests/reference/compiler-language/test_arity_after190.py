import copy
from pathlib import Path
import unittest
from unittest import mock
import arity_after190 as cohort


class ArityAfter190Tests(unittest.TestCase):
    def changed_source(self, name, before, after):
        original = Path.read_text
        def changed(path, *args, **kwargs):
            result = original(path, *args, **kwargs)
            return result.replace(before, after) if path.name == name else result
        return mock.patch.object(Path, 'read_text', changed)

    def test_exact_census_and_no_original_site_credit(self):
        import full_ledger
        import report
        exported = cohort.exports()
        rows = exported['obligation-witnesses.jsonl']
        self.assertEqual(94, len(rows))
        self.assertEqual(21, len(cohort.rows()))
        self.assertTrue(all(row['assertion_ids'] == [] for row in rows))
        assertions, obligations, _ = full_ledger.load()
        _, witnesses, _ = report.load_contract('intended-language')
        installed = {
            'obligation-witnesses.jsonl': full_ledger.load_obligation_witnesses(obligations, assertions, witnesses),
            'phase-witnesses.jsonl': full_ledger.load_phase_witnesses(obligations),
        }
        for filename in installed:
            self.assertEqual(exported[filename], [installed[filename][row['obligation_id']] for row in exported[filename]])

    def test_function_test_callback_remains(self):
        with self.changed_source('cljs-definitions-var-test.cljs', '(test cljs-3411-function)', ':ok'), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_var_test_callback_remains(self):
        with self.changed_source('cljs-definitions-var-test.cljs', "(test #'cljs-3411-function)", ':ok'), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_both_original_callback_literals_remain(self):
        with self.changed_source('cljs-definitions-var-test.cljs', '(assert (= (cljs-3411-function 4 4) 8))', 'true'), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_complete_map_entry_reduction_remains(self):
        with self.changed_source('cljs-helpers-map-entry-test-map-entry-interface-tests.cljs', '(-reduce e conj [])', '[:key :val]'), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_complete_transient_persistence_events_remain(self):
        with self.changed_source('arity-receiver-transient.cljs', 'result (-persistent! custom)', 'result []'), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_protocol_receiver_without_actual_methods_gives_no_credit(self):
        selected = next(row for row in cohort.SELECTION if row['evidence_mode'] == 'protocol-receiver')
        with self.assertRaisesRegex(ValueError, 'method/callback demand omitted'):
            cohort.operation_evidence('(satisfies? IMultiFn x)', selected, {'name': 'IMultiFn'})

    def test_complete_multifn_factory_apply_remains(self):
        with self.changed_source('cljs-public-gate-core-factory-multifn.cljs', '(apply ->MultiFn', '(apply vector'), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_missing_demanded_macro_invocation_rejects(self):
        with self.changed_source('compiler_arity_after190.rs', '{invoke}', 'true'), self.assertRaisesRegex(ValueError, 'whole source invocation'):
            cohort.exports()

    def test_original_callback_arity_is_frozen(self):
        reviewed = copy.deepcopy(cohort.REVIEWED)
        reviewed[0]['invoke'] = '(run)'
        with mock.patch.object(cohort, 'REVIEWED', reviewed), self.assertRaisesRegex(ValueError, 'callback invocation arity drift'):
            cohort.exports()

    def test_operation_hash_signature_and_actual_arity_are_frozen(self):
        for field, value, message in [('body_sha256', '0'*64, 'operation/body drift'), ('signature_sha256', '0'*64, 'signature drift'), ('direct_arities', [0], 'actual source calls')]:
            selected = copy.deepcopy(cohort.SELECTION)
            selected[0][field] = value
            with mock.patch.object(cohort, 'SELECTION', selected), self.assertRaisesRegex(ValueError, message):
                cohort.exports()

    def test_protocol_methods_require_authoritative_arity_and_receiver(self):
        selected = next(row for row in cohort.SELECTION if row['evidence_mode'] == 'protocol-method' and row['protocol_name'] == 'ILookup')
        target = {'name': 'ILookup'}
        source = next(row for row in cohort.rows() if row['id'] == selected['workflow'])['source']
        for changes, message in [({'direct_arities': [2]}, 'declared signature drift'), ({'protocol_body_sha256': '0'*64}, 'authority drift'), ({'receiver': 'unused'}, 'demanded protocol method calls')]:
            with self.assertRaisesRegex(ValueError, message):
                cohort.operation_evidence(source, {**selected, **changes}, target)
        with self.assertRaisesRegex(ValueError, 'demanded protocol method calls'):
            cohort.operation_evidence("'(-lookup e 0)", selected, target)

    def test_marker_requires_original_named_receiver_and_consumption(self):
        selected = next(row for row in cohort.SELECTION if row['evidence_mode'] == 'protocol-marker-receiver')
        for source in ['(-seq e)', '(satisfies? ISequential e)', "'(satisfies? ISequential e) (-seq e)"]:
            with self.assertRaisesRegex(ValueError, 'named marker receiver or consumption'):
                cohort.operation_evidence(source, selected, {'name': 'ISequential'})

    def test_printing_root_requires_original_value_and_actual_printer(self):
        selected = next(row for row in cohort.SELECTION if row['evidence_mode'] == 'printing-root' and row['call_name'] == '*print-readably*')
        target = {'name': '*print-readably*'}
        for source in ['(binding [*print-readably* true] (pr-str (with-meta ["x"] {:x true})))', '(binding [*print-readably* false] true)', '(let [name (quote *print-readably*)] true)']:
            with self.assertRaisesRegex(ValueError, 'root value|demanded original printer|demanded printing-root'):
                cohort.operation_evidence(source, selected, target)

    def test_printing_original_sci_fields_and_canonical_literal_remain(self):
        case = cohort.supplemental_reference_cases()[cohort.PRINT_ID]
        provenance = case['canonical_printing_provenance']
        self.assertEqual(provenance['original_fields'], provenance['retained_disposition_fields'][:7])
        self.assertEqual(0, provenance['original_semantic_pass_credit'])
        self.assertEqual('^{:x true} [x]', case['expected']['outcome']['value'])
        with self.changed_source('arity-after190-canonical-print-meta-unreadable.cljs', '*print-readably* false', '*print-readably* true'), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_printer_callback_definitions_alone_give_no_credit(self):
        selected = next(row for row in cohort.SELECTION if row['evidence_mode'] == 'protocol-printer-receiver')
        source = next(row for row in cohort.rows() if row['id'] == selected['workflow'])['source']
        with self.assertRaisesRegex(ValueError, 'callback demand/literal omitted'):
            cohort.operation_evidence(source.replace('(pr-str #js [1 2 3])', '"#array[3]"'), selected, {'name': 'IPrintWithWriter'})

    def test_implemented_callback_without_original_consumer_gives_no_credit(self):
        for protocol in ['IWatchable', 'IEncodeJS', 'IEncodeClojure', 'IHash']:
            selected = next(row for row in cohort.SELECTION if row['evidence_mode'] == 'protocol-implemented-consumer' and row['protocol_name'] == protocol)
            source = next(row for row in cohort.rows() if row['id'] == selected['workflow'])['source']
            consumer = selected['consumer_calls'][0]['source']
            with self.assertRaisesRegex(ValueError, 'original protocol consumer omitted'):
                cohort.operation_evidence(source.replace(consumer, 'true'), selected, {'name': protocol})

    def test_consumer_method_signature_and_independent_literal_are_required(self):
        selected = next(row for row in cohort.SELECTION if row['evidence_mode'] == 'protocol-implemented-consumer' and row['protocol_name'] == 'IEncodeJS')
        source = next(row for row in cohort.rows() if row['id'] == selected['workflow'])['source']
        changed = {**selected, 'demanded_methods': [{'name': '-clj->js', 'arities': [2]}]}
        with self.assertRaisesRegex(ValueError, 'demanded method signature drift'):
            cohort.operation_evidence(source, changed, {'name': 'IEncodeJS'})
        with self.assertRaisesRegex(ValueError, 'equality literal omitted'):
            cohort.operation_evidence(source.replace(selected['root_equality_literal_source'], 'true', 1), selected, {'name': 'IEncodeJS'})

    def test_named_drop_receiver_requires_same_original_consumption(self):
        selected = next(row for row in cohort.SELECTION if row['evidence_mode'] == 'protocol-typed-consumer')
        source = next(row for row in cohort.rows() if row['id'] == selected['workflow'])['source']
        with self.assertRaisesRegex(ValueError, 'named protocol receiver tests omitted'):
            cohort.operation_evidence(source.replace('(satisfies? IDrop (seq "aaaaaaaaaa"))', 'true'), selected, {'name': 'IDrop'})
        with self.assertRaisesRegex(ValueError, 'original protocol consumer omitted'):
            cohort.operation_evidence(source.replace('(drop 5 (seq "aaaaaaaaaa"))', '[]'), {**selected, 'receiver_tests': selected['receiver_tests'][:1]}, {'name': 'IDrop'})

    def test_namespace_setup_requires_actual_original_consumer(self):
        for selected in [row for row in cohort.SELECTION if row['evidence_mode'] == 'namespace-setup-consumer']:
            source = next(row for row in cohort.rows() if row['id'] == selected['workflow'])['source']
            target = {'signature': {'kind': 'variadic', 'minimum': 0}}
            with self.assertRaisesRegex(ValueError, 'namespace setup consumer omitted'):
                cohort.operation_evidence(source.replace(selected['consumer_calls'][0]['source'], 'true'), selected, target)
            with self.assertRaisesRegex(ValueError, 'top-level namespace setup/signature omitted'):
                cohort.operation_evidence(source.replace(selected['setup_source'], '(do '+selected['setup_source']+')'), selected, target)

    def test_namespace_complete_companion_tree_is_frozen(self):
        original = Path.read_bytes
        def changed(path, *args, **kwargs):
            content = original(path, *args, **kwargs)
            return content+b'\ntrue\n' if 'arity-after190/source-context/' in str(path) else content
        with mock.patch.object(Path, 'read_bytes', changed), self.assertRaisesRegex(ValueError, 'namespace companion omitted'):
            cohort.exports()

    def test_actual_frames_require_complete_original_literal_order(self):
        lines = ['arity-after190-observation\t'+row['id']+'\t'+host+'\t'+phase+'\t'+engine+'\t'+cohort.EXPECTED_LITERAL_RAW[row['id']]
            for row in cohort.rows() for host in ['fresh', 'reused'] for phase in ['application', 'macro'] for engine in ['QuickJS', 'Node']]
        self.assertEqual(168, len(cohort.parse_frames('\n'.join(lines))))
        for altered in [lines[:-1], list(reversed(lines)), [*lines, lines[-1]], [line.replace('\ttrue', '\tfalse') for line in lines]]:
            with self.assertRaises(ValueError):
                cohort.parse_frames('\n'.join(altered))


if __name__ == '__main__':
    unittest.main()
