"""Preserve complete original sources and independently established packets."""
import hashlib
import json
from pathlib import Path
import re
import unittest

import report

HERE = Path(__file__).resolve().parent


class LiteralExecution(unittest.TestCase):
    def setUp(self):
        self.history = json.loads((HERE / 'literal-execution-historical-before193.json').read_text())
        self.receipt = json.loads((HERE / 'literal-execution-reference-receipt.json').read_text())

    def test_whole_sources_and_rejection_controls_survive(self):
        text = (report.ROOT / 'tests/execution.rs').read_text()
        current = []
        for name, original in self.history['historical_complete_test_bodies'].items():
            body = text.split('fn ' + name + '() {', 1)[1].split('\n#[test]', 1)[0]
            current += [(identity, json.loads('"' + source + '"')) for identity, source in re.findall(
                r'id: "([^" ]+)"\.into\(\),\s*source: "((?:\\.|[^"\\])*)"\.into\(\),', body)]
            self.assertTrue(original.startswith('#[test]\nfn ' + name + '() {'))
            self.assertIn('literal_cases_in_both_engines(&cases,', body)
            if name.startswith('set_reader'):
                for source in self.history['reject_sources']:
                    self.assertIn(json.dumps(source)[1:-1], body)
        self.assertEqual(current, [(r['id'], r['source']) for r in self.history['sources']])
        for row in self.history['sources']:
            self.assertEqual(hashlib.sha256(row['source'].encode()).hexdigest(), row['source_sha256'])
        self.assertEqual(self.receipt['sources'], self.history['sources'])
        self.assertEqual(self.receipt['historical_complete_test_bodies'], self.history['historical_complete_test_bodies'])

    def test_current_packets_match_both_independent_pins_exactly(self):
        pins = self.receipt['pins']
        self.assertTrue(report.same_json(pins['release']['packets'], pins['checkout']['packets']))
        for kind, identity in [('release', report.RELEASE), ('checkout', report.REVISION)]:
            self.assertEqual(pins[kind]['identity'], identity)
            self.assertEqual(pins[kind]['settings'], {'optimizations': 'none', 'static-fns': False, 'def-emits-var': False})
            self.assertEqual([r['source'] for r in pins[kind]['diagnostics']], self.history['reject_sources'])
            self.assertEqual([r['status'] for r in pins[kind]['diagnostics']], ['reader-rejected'] * 2)
        text = (report.ROOT / 'tests/execution.rs').read_text()
        current = []
        for name in self.history['historical_complete_test_bodies']:
            body = text.split('fn ' + name + '() {', 1)[1].split('\n#[test]', 1)[0]
            for identity, outcome, trace in re.findall(
                r'id: "([^" ]+)"\.into\(\),.*?outcome: r#"(.*?)"#\.into\(\),\s*trace: (r#".*?"#|".*?")\.into\(\),', body, re.S):
                trace = trace[3:-2] if trace.startswith('r#') else json.loads(trace)
                current.append({'id': identity, 'outcome': json.loads(outcome), 'trace': json.loads(trace), 'stdout': ''})
        self.assertTrue(report.same_json(current, pins['release']['packets']))
        self.assertEqual(len(current), 7)
        self.assertFalse(self.receipt['strict_complete'])
        self.assertEqual(self.receipt['original_site_credit'], 0)


if __name__ == '__main__':
    unittest.main()
