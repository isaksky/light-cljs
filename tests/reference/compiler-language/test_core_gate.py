#!/usr/bin/env python3
"""Protocol declarations cannot supply executing public-call evidence."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('core_gate', Path(__file__).parents[1] / 'compiler-core-gate.py')
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)


class CoreCallEvidenceTests(unittest.TestCase):
    def test_protocol_method_declarations_are_not_call_arities(self):
        source = '(reify ISwap (-swap! [_ f] (inc 1)) (-swap! [_ f a b xs] (count xs)))'
        self.assertEqual(gate.calls(source), {'inc': {1}, 'count': {1}})

    def test_only_real_dispatch_calls_supply_protocol_arities(self):
        source = '(let [r (reify ISwap (-swap! [_ f] (f 1)))] [(-swap! r inc) (-swap! r + 1 2 (list 3))])'
        self.assertEqual(gate.calls(source), {'-swap!': {2, 5}, 'list': {1}})

    def test_qualified_workflow_preserves_definitions_and_qualifies_bodies(self):
        source = '(reify ISwap (-swap! [_ f] (inc (f 1))))'
        self.assertEqual(gate.qualify_literal_core_calls(source),
                         '(reify ISwap (-swap! [_ f] (clojure.core/inc (f 1))))')
        self.assertEqual(gate.qualify_literal_core_calls("'(-swap! [_ f] (inc 1))"),
                         "'(-swap! [_ f] (inc 1))")

    def test_all_64_historical_workflows_remain_byte_exact(self):
        new_names = {'literal::print-callback-failure', 'literal::qualified-print-callback-state', 'literal::transducer-stop-and-failure', 'literal::metadata-transform-arities', 'literal::apply-argument-stream', 'literal::qualified-print-callback-failure', 'literal::qualified-transducer-state-and-tail', 'literal::qualified-clone-find-protocol-workflow', 'literal::transducer-init-completion', 'literal::transducer-state-and-tail', 'literal::clone-find-protocol-workflow', 'literal::qualified-transducer-stop-and-failure', 'literal::print-callback-state', 'literal::qualified-metadata-transform-arities', 'literal::qualified-transducer-init-completion', 'literal::qualified-apply-argument-stream'}
        text = gate.FIXTURE.read_text()
        retained = "".join(line for line in text.splitlines(keepends=True)
                           if line.startswith("#") or line.split("\t")[1] not in new_names)
        self.assertEqual(gate.digest(retained), 'a61f6491e18b1cc4d09901294e5612d5f73aed889972cb38c5a774a4e4fb87e5')

    def test_independently_verified_16_whole_sources_and_expectations_are_preserved(self):
        expected = {'transducer-init-completion': ('collections', '46874f9feb73c2998ca8c1f169720c8d012fa1fd74aaead16b10cd33aaaa8470', 'de1b434107974cae95229a03169174da5f9302294294415a9201d7de71261916'), 'transducer-stop-and-failure': ('collections', '32e90aa13134ebc38466cfce07e424ba53e46c56dfa1ca7a3a213dea5e6e6b83', '5cb84c57d24fd0710357554224018d6a83efb54b527f7e8fc06f84672c36c778'), 'transducer-state-and-tail': ('collections', '5a26f6e50535445b2e66ccc5ef1b9ce4360164abcae3b8f113c2ffa4daa66833', 'cc06a751abd66e503fe4dd6b091f27efcc5ac1056c30d27636de3fb5d41902f9'), 'qualified-transducer-init-completion': ('collections', '078e45351895f9d012dde46f71b773b9972b29db9397a9b451b89f8534a7b683', 'de1b434107974cae95229a03169174da5f9302294294415a9201d7de71261916'), 'qualified-transducer-stop-and-failure': ('collections', '095414b79b7403c8dd086e24efeeb01a22ef6801ca5f55febc5fc90e6db63f31', '5cb84c57d24fd0710357554224018d6a83efb54b527f7e8fc06f84672c36c778'), 'qualified-transducer-state-and-tail': ('collections', 'eb2f08548fcd0648f20ad1d8e2a7fbee3a8c3a162f0db7d1d2b70e77aedf3b09', 'cc06a751abd66e503fe4dd6b091f27efcc5ac1056c30d27636de3fb5d41902f9'), 'metadata-transform-arities': ('collections', '0c5a4bc47cccaad7371e7b7437ed1412cdf052107822cd12744ac5bb8c0f54ce', 'e30d9f6b18d27e573ae047b038576d033a11b6888da61ed14150aea8c9cfa998'), 'apply-argument-stream': ('collections', 'c3168a309d7d0a319fedba166b62bf8caef6d3bdd8e330cd074336a0628a7499', '3bb6d5a729534fcc68e60b71ecf86ac3bad64ebaf52e4f7029901ada363d58c2'), 'clone-find-protocol-workflow': ('collections', '876bccc80e42fef066eade3ff2eadb8e4f35a79708143d3ef5351231320ed4b8', '193f1f6e00e0607118eaf3d34db8f1a32b61c90bde34ade5aa46131866702a57'), 'qualified-metadata-transform-arities': ('collections', 'd38c2ca97c7dcb5384c96a31adc6afc7bf9febdc3c78604c1595ff74a938e825', 'e30d9f6b18d27e573ae047b038576d033a11b6888da61ed14150aea8c9cfa998'), 'qualified-apply-argument-stream': ('collections', 'e4b6a52bef36a7b28d9449de2110e051340171b38ce7e3288061a55065499548', '3bb6d5a729534fcc68e60b71ecf86ac3bad64ebaf52e4f7029901ada363d58c2'), 'qualified-clone-find-protocol-workflow': ('collections', '6ce1416fddb5d3f969703d71319a9e8a82b88d67712c444b64902b7532dc8655', '193f1f6e00e0607118eaf3d34db8f1a32b61c90bde34ade5aa46131866702a57'), 'print-callback-state': ('state', '7668631d3faa26cf0fd2d4e26a047c05ca6fe00ad02520793a2e637932a7398a', 'b5bea41b6c623f7c09f1bf24dcae58ebab3c0cdd90ad966bc43a45b44867e12b'), 'qualified-print-callback-state': ('state', 'e46355412b1f935ba1c17405038200c2a595327f3d71ff4eec3e0d03d25ed4d8', 'b5bea41b6c623f7c09f1bf24dcae58ebab3c0cdd90ad966bc43a45b44867e12b'), 'print-callback-failure': ('state', '8e41f609e7778e64ddfde78442bd724c25cb1beb663a3dc7a92eb3066b651707', 'b5bea41b6c623f7c09f1bf24dcae58ebab3c0cdd90ad966bc43a45b44867e12b'), 'qualified-print-callback-failure': ('state', '4dcc0f0db521767109bc4e2f3c3dcf5989bd92b5b1144f06add2238774c9cd10', 'b5bea41b6c623f7c09f1bf24dcae58ebab3c0cdd90ad966bc43a45b44867e12b')}
        found = {}
        for group, identity, source, value, _ in gate.fixture_rows():
            name = identity.removeprefix("literal::")
            if name in expected:
                self.assertNotIn(name, found)
                found[name] = (group, gate.digest(source), gate.digest(value))
        self.assertEqual(found, expected)

    def test_clone_and_find_definitions_cannot_be_counted_as_public_calls(self):
        source = "(reify ICloneable (-clone [_] :copy) IFind (-find [_ k] (find {:a 1} k)))"
        self.assertEqual(gate.calls(source), {"find": {2}})
        self.assertEqual(gate.calls("[(-clone [1]) (cljs.core/-find {:a 1} :a)]"),
                         {"-clone": {1}, "-find": {2}})


if __name__ == '__main__':
    unittest.main()
