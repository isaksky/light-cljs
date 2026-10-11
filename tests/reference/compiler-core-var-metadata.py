#!/usr/bin/env python3
"""Reconcile executing metadata witnesses; generated rows never imply a pass."""
from pathlib import Path
import argparse
import hashlib
import json
import re
import sys
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / 'tests/reference/compiler-language'
TEST = 'tests/compiler_core_var_metadata.rs'
SUPPORT = 'tests/fixtures/compiler-core-var-metadata.cljs'
NAME = 'recovered_core_var_metadata_executes_in_applications_and_macros'
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--write', action='store_true')
parser.add_argument('--reference', choices=['release', 'checkout', 'both'])
parser.add_argument('--artifacts', type=Path)
args = parser.parse_args()
names = sorted({line.split('\t')[0] for catalog in ['src/core_public_bindings.tsv', 'src/core_var_metadata.tsv']
                for line in (ROOT / catalog).read_text().splitlines() if '\t' in line})
runtime_path = ROOT / 'src/js_runtime.js'
runtime = runtime_path.read_text()
pattern = r'const readerCoreNames = new Set\("[^"\n]*"\.split\(" "\)\);'
assert len(re.findall(pattern, runtime)) == 1
canonical = 'const readerCoreNames = new Set(' + json.dumps(' '.join(names)) + '.split(" "));'
if '--write' in sys.argv:
    runtime_path.write_text(re.sub(pattern, lambda _: canonical, runtime))
else:
    assert re.search(pattern, runtime).group() == canonical, 'Runtime reader core names drift'
LOCAL_NAMES = {'*1', '*2', '*3', '*assert*', '*print-err-fn*', '-clone',
               'str', 'str_', 'Var', '->Var', 'get'}
# Value declarations are actually used under bindings or invoked.
# Constructor/function metadata alone cannot establish public invocation arities.
PUBLIC_NAMES = {'*1', '*2', '*3', '*assert*', '*print-err-fn*'}
def sha(data):
    return hashlib.sha256(data).hexdigest()
def rows(name):
    return [json.loads(line) for line in (LEDGER / name).read_text().splitlines() if line]
execution = {'test_file': TEST, 'test_name': NAME,
             'test_source_sha256': sha((ROOT / TEST).read_bytes())}
supports = {SUPPORT: sha((ROOT / SUPPORT).read_bytes())}
obligations, phases = [], []
for target in rows('obligations.jsonl'):
    local = target['source'] == 'src/core_var_metadata.tsv' and target['name'] in LOCAL_NAMES
    public = target['source'] == 'src/main/cljs/cljs/core.cljs' and target['name'] in PUBLIC_NAMES
    if target['disposition'] != 'intended' or not (local or public):
        continue
    common = {'obligation_id': target['id'], 'body_sha256': target['body_sha256'],
              'rationale': 'The complete pinned local test and support contract execute literal public Var names, docs, locations, namespace identity and list/vector arglist shapes in Node/QuickJS applications and a QuickJS macro. Dynamic values are observed through bound/dereferenced roots and the saved error sink is invoked; clone/-clone dispatch executes on a persistent vector. Local metadata rows attest actual Var observations, not complete API behavior. Public mappings cover only the dynamic value declarations exercised by this contract; existing clone and Namespace mappings retain their original invocation witnesses.'}
    if target.get('signature'):
        common['signature_sha256'] = sha(json.dumps(target['signature'], sort_keys=True).encode())
    obligations.append({**common, 'local_execution': execution, 'assertion_ids': []})
    phases.append({**common, **execution, 'support_source_sha256': supports})
assert len(obligations) == len(LOCAL_NAMES) + len(PUBLIC_NAMES)
for filename, expected in [('obligation-witnesses.jsonl', obligations), ('phase-witnesses.jsonl', phases)]:
    identities = {row['obligation_id'] for row in expected}
    path = LEDGER / filename
    if '--write' in sys.argv:
        retained = [line for line in path.read_text().splitlines() if json.loads(line)['obligation_id'] not in identities]
        path.write_text('\n'.join(retained + [json.dumps(row, separators=(',', ':')) for row in expected]) + '\n')
    else:
        current = {row['obligation_id']: row for row in rows(filename)}
        for row in expected:
            assert current[row['obligation_id']] == row, (filename, row['obligation_id'])
print(f'{len(obligations)} executing metadata/value/invocation mappings reconciled')
if args.reference:
    assert args.artifacts, '--reference requires --artifacts'
    sys.path.insert(0, str(LEDGER))
    import report
    # Preserve each literal, assertion and expression. Strip local comments and
    # reader whitespace only for the existing one-line TSV oracle transport.
    source = '(do ' + ' '.join(line for line in (ROOT / SUPPORT).read_text().splitlines()
                              if not line.lstrip().startswith(';')) + ' (metadata-contract))'
    case = {'id': 'core-var-metadata', 'source': source,
            'expected': {'id': 'core-var-metadata', 'outcome': {'status': 'returned', 'value': True},
                         'trace': [], 'stdout': ''}}
    for kind in ['release', 'checkout'] if args.reference == 'both' else [args.reference]:
        selected = dict(case)
        if kind == 'release':
            # Explicit Maven 1.12.134 source-version differences. Preserve all
            # assertions and expect its actual declared locations/documentation.
            replacements = {'[213 218 223 71 94]': '[212 217 222 72 93]',
                            "'clone 1271": "'clone 1270", "'-clone 615": "'-clone 614",
                            "'str 3140": "'str 3129", "'str_ 3117": "'str_ 3106",
                            "'Var 1188": "'Var 1187", "'->Var 1188": "'->Var 1187",
                            "'Namespace 12132": "'Namespace 12105", "'get 2032": "'get 2031",
                            json.dumps("When set to logical false, 'assert' will omit assertion checks in\n compiled code. Defaults to true."): 'nil'}
            for original, replacement in replacements.items():
                assert selected['source'].count(original) == 1, original
                selected['source'] = selected['source'].replace(original, replacement)
        observed = report.reference_cases([selected], kind, (ROOT / 'research/clojurescript').resolve(),
                                         args.artifacts.resolve() / kind, 180)
        print(kind, json.dumps(observed), flush=True)
        assert observed[case['id']]['status'] == 'passed'
