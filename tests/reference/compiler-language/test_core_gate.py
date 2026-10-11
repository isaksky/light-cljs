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


if __name__ == '__main__':
    unittest.main()
