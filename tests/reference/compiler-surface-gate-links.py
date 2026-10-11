#!/usr/bin/env python3
"""Add exact utility/regex/printing literal oracle links; shared writes opt-in."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import sys
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / 'tests/reference/compiler-language'
sys.path.insert(0, str(HERE))
import extract_inventory as inv
import full_ledger
import report
PREFIX = 'cljs-surface-gate-'
SUPPORT = 'tests/fixtures/compiler-surface-gate'
TESTS = {'utilities': 'tests/compiler_utilities_completion.rs',
         'regex': 'tests/compiler_regex.rs', 'printing': 'tests/compiler_printing_completion.rs'}

def digest(value):
    return hashlib.sha256(value.encode() if isinstance(value, str) else value).hexdigest()

def read(name):
    return report.jsonl(HERE / name)

def expected(identity, value=True):
    return {'id': identity, 'outcome': {'status': 'returned', 'value': value}, 'trace': [], 'stdout': ''}

def literal_sources():
    spec = importlib.util.spec_from_file_location('utility_contracts', ROOT / 'tests/reference/compiler-utilities-completion.py')
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    identity, source, value, provenance = module.literal_contracts()[0]
    assert identity == 'cljs-utilities-contract-0' and value == 'true'
    assert [line.split('\t') for line in (ROOT / 'tests/fixtures/compiler-utilities-contracts.tsv').read_text().splitlines()
            if line and not line.startswith('#')][0] == [identity, source, value, provenance]
    cases = {'utilities': {'id': PREFIX + 'utilities', 'source': source,
             'expected': expected(PREFIX + 'utilities'), 'native_stdout': 'true\n'}}
    originals = {'utilities': {'path': 'tests/fixtures/compiler-utilities-contracts.tsv', 'source_sha256': digest(source), 'provenance': provenance}}
    files = {}
    regex_path = 'tests/fixtures/compiler-regex-reference/reference/main.cljs'
    regex = (ROOT / regex_path).read_text()
    # Keep the complete original namespace, every definition and assertion. An
    # observation-argument export wraps the existing zero-argument literal run.
    source = regex + '\n(defn oracle-run [observe] (run))\noracle-run\n'
    relative = SUPPORT + '/regex/reference/main.cljs'; files[relative] = source
    cases['regex'] = {'id': PREFIX + 'regex', 'source_root': SUPPORT + '/regex',
        'entry': 'reference.main', 'export': 'oracle-run', 'source': source,
        'expected': expected(PREFIX + 'regex', {'$vector': [{'$vector': [23, True]}, True]}), 'native_stdout': None}
    originals['regex'] = {'path': regex_path, 'source_sha256': digest(regex)}
    text = (ROOT / TESTS['printing']).read_text()
    for group, name in [('printing-values', 'printing-witness'), ('printing-output', 'output-witness')]:
        matches = [m.group(1) for m in re.finditer(r'r##"(.*?)"##', text, re.S) if '(defn ' + name + ' ' in m.group(1)]
        assert len(matches) == 1
        original = matches[0]; forms = inv.Reader(original).all()
        # Retain every executable declaration and the entire literal witness;
        # only the compiler-executed same-file defmacro and duplicate invocation
        # are omitted from this canonical application oracle.
        assert inv.short_head(forms[-2]) == 'defmacro' and forms[-1].kind == 'vector'
        assert original[forms[-1].start:forms[-1].end] == f'[({name}) ({"printing" if name == "printing-witness" else "output"}-macro-witness)]'
        retained = original[forms[0].start:forms[-3].end]
        call = '(' + name + ')'
        source = '(ns cljs.user)\n' + retained + '\n(defn oracle-run [observe] ' + call + ')\noracle-run\n'
        relative = SUPPORT + '/' + group + '/cljs/user.cljs'; files[relative] = source
        cases[group] = {'id': PREFIX + group, 'source_root': SUPPORT + '/' + group, 'entry': 'cljs.user',
            'export': 'oracle-run', 'source': source, 'expected': expected(PREFIX + group), 'native_stdout': None}
        originals[group] = {'path': TESTS['printing'], 'source_sha256': digest(original), 'retained_source_sha256': digest(retained)}
    # The previously reviewed TaggedLiteral factory fixture supplies exact 2/1
    # calls plus nominal/keyword observations; reuse its source/literal identity.
    tagged = next(c for c in read('full-cases.jsonl') if c['id'] == 'cljs-public-gate-core-factory-taggedliteral')
    cases['tagged'] = tagged
    return cases, originals, files

def attach_reference(mapping):
    """Preserve safe exact links when an existing family regenerates a row."""
    local = mapping.get('local_execution', {})
    group = {'tests/compiler_utilities_completion.rs': 'utilities', 'tests/compiler_regex.rs': 'regex'}.get(local.get('test_file'))
    if local.get('test_file') == 'tests/compiler_reader.rs':
        target = next(o for o in read('obligations.jsonl') if o['id'] == mapping['obligation_id'])
        if target['name'] in {'tagged-literal', 'tagged-literal?'}:
            group = 'tagged'
    if group is None:
        return mapping
    cases, _, _ = literal_sources()
    case = cases[group]
    return {**mapping, 'reference_fixture_id': case['id'],
        'reference_source_sha256': digest(case['source']),
        'reference_contract_sha256': digest(json.dumps(case['expected'], sort_keys=True))}

def generated():
    cases, originals, files = literal_sources()
    obligations = {o['id']: o for o in read('obligations.jsonl')}
    phases = full_ledger.load_phase_witnesses(list(obligations.values()))
    updates, receipts = [], []
    for mapping in read('obligation-witnesses.jsonl'):
        local = mapping.get('local_execution', {})
        path = local.get('test_file')
        target = obligations[mapping['obligation_id']]
        group = next((key for key, value in TESTS.items() if value == path), None)
        if path == 'tests/compiler_reader.rs' and target['name'] in {'tagged-literal', 'tagged-literal?'}:
            group = 'tagged'
        if not group:
            continue
        if group == 'printing':
            group = {'length_level_readability_metadata_and_option_overrides': 'printing-values',
                     'output_sinks_capture_nesting_flush_and_restoration': 'printing-output'}[local['test_name']]
        if group in {'printing-values', 'printing-output'}:
            continue  # Complete originals have separately recorded pinned differences.
        phase = phases[target['id']]
        assert local == {key: phase[key] for key in ('test_file', 'test_name', 'test_source_sha256')}
        assert digest((ROOT / path).read_bytes()) == local['test_source_sha256']
        assert target['body_sha256'] == mapping['body_sha256'] and target['disposition'] == 'intended'
        signature = target.get('signature')
        if signature:
            assert signature['kind'] != 'unresolved'
            assert mapping['signature_sha256'] == digest(json.dumps(signature, sort_keys=True))
            if 'source_span' in target:
                span = target['source_span']
                assert full_ledger.pinned(target['source'])[span['start_byte']:span['end_byte']].decode() == span['text']
        case = cases[group]
        fields = {'reference_fixture_id': case['id'], 'reference_source_sha256': digest(case['source']),
                  'reference_contract_sha256': digest(json.dumps(case['expected'], sort_keys=True))}
        assert not mapping.get('reference_fixture_id') or all(mapping[key] == value for key, value in fields.items())
        updated = attach_reference(mapping)
        assert updated == {**mapping, **fields}
        updates.append(updated)
        receipts.append({'obligation_id': target['id'], 'body_sha256': target['body_sha256'],
            'signature': signature, 'original_contract': originals.get(group, {'reused_fixture': case['id']}),
            'local_execution': local, **fields,
            'rationale': 'Retain the complete literal application witness and setup. Existing exact macro/application phase and declaration/signature identities remain unchanged. Printing keeps cljs.user for its literal record tags; no zero-argument calls are inferred from positive variadic calls. Reader hosted read APIs remain unlinked.'})
    assert len(updates) == 51, len(updates)
    return updates, [c for key, c in cases.items() if key not in {'tagged', 'printing-values', 'printing-output'}], receipts, files

def records(rows):
    return ''.join(json.dumps(row, separators=(',', ':')) + '\n' for row in rows)

def replace_cases(existing, cases):
    result, inserted = [], False
    for row in existing:
        if row['id'].startswith(PREFIX):
            if not inserted:
                result.extend(cases)
                inserted = True
        else:
            result.append(row)
    if not inserted:
        result.extend(cases)
    return result

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures-write', action='store_true')
    parser.add_argument('--propose', type=Path)
    parser.add_argument('--write', action='store_true')
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--run', action='store_true')
    parser.add_argument('--artifacts', type=Path)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/light-cljs')
    args = parser.parse_args()
    updates, cases, receipts, files = generated()
    for relative, source in files.items():
        path = ROOT / relative
        if args.fixtures_write:
            path.parent.mkdir(parents=True, exist_ok=True); path.write_text(source)
        else:
            assert path.read_text() == source, relative + ' complete contract drift'
    if args.propose:
        args.propose.mkdir(parents=True, exist_ok=True)
        for name, rows in [('obligation-updates.jsonl', updates), ('case-additions.jsonl', cases), ('surface-gate-link-receipts.jsonl', receipts)]:
            (args.propose / name).write_text(records(rows))
    if args.write or args.check:
        index = {r['obligation_id']: r for r in updates}
        current_cases = read('full-cases.jsonl')
        for name, rows in [('obligation-witnesses.jsonl', [index.get(r['obligation_id'], r) for r in read('obligation-witnesses.jsonl')]),
            ('full-cases.jsonl', replace_cases(current_cases, cases)),
            ('surface-gate-link-receipts.jsonl', receipts)]:
            if args.write:
                (HERE / name).write_text(records(rows))
            elif name == 'full-cases.jsonl':
                assert [c for c in current_cases if c['id'].startswith(PREFIX)] == cases, name + ' overlay drift'
            else:
                assert (HERE / name).read_text() == records(rows), name + ' overlay drift'
    print(json.dumps({'local_rows': len(updates), 'new_literal_oracles': len(cases), 'reused_literal_oracles': 1}), flush=True)
    if args.run:
        assert args.artifacts
        args.artifacts.mkdir(parents=True, exist_ok=False)
        local = {}
        for case in cases:
            directory = args.artifacts / 'local' / case['id']
            result = report.local_layers(case, str(args.binary.resolve()), directory, 120)
            if 'source_root' in case:
                # The namespace bundle returns the export function. Hosted
                # evaluation invokes that same complete source/export directly.
                source = case['source'].removesuffix('oracle-run\n') + '(oracle-run nil)\n'
                run = report.invoke([str(args.binary.resolve()), '--eval', '-'], directory / 'hosted-export', source=source, timeout=120)
                literal = '[[23 true] true]\n' if case['id'].endswith('regex') else 'true\n'
                result['evaluation'] = {'status': 'passed' if run['status'] == 'completed' and run['stdout'] == literal else 'mismatch', 'observed': run}
            local[case['id']] = result
        (args.artifacts / 'local.json').write_text(json.dumps(local, indent=2) + '\n')
        print('local', json.dumps({key: {layer: row[layer]['status'] for layer in ('javascript', 'evaluation')} for key, row in local.items()}), flush=True)
        failures = {key: row for key, row in local.items() if any(row[layer]['status'] != 'passed' for layer in ('javascript', 'evaluation'))}
        for kind in ('release', 'checkout'):
            observed = report.reference_cases(cases, kind, ROOT / 'research/clojurescript', args.artifacts / kind, 300)
            (args.artifacts / (kind + '.json')).write_text(json.dumps(observed, indent=2) + '\n')
            print(kind, json.dumps({key: row['status'] for key, row in observed.items()}), flush=True)
            failures.update({kind + ':' + key: row for key, row in observed.items() if row['status'] != 'passed'})
        assert not failures, failures

if __name__ == '__main__':
    main()
