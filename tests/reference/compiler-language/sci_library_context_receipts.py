#!/usr/bin/env python3
"""Validate bounded original library contracts without promoting mismatches."""
import argparse, collections, json
import sci_library_context as context
import sci_execution as sci

RECEIPTS = context.HERE / 'sci-library-context-receipts.jsonl'


def classification(row, layers):
    runner = sci.report_module()
    if set(layers) != {'javascript', 'release', 'checkout'}:
        raise ValueError('Incomplete library engine/reference layers')
    local = layers['javascript']
    exact = {'source_sha256': row['source_sha256'], 'adapted_source_sha256': row['adapted_source_sha256'],
        'test_file': context.DRIVER, 'test_name': context.TEST,
        'test_source_sha256': sci.digest((sci.ROOT / context.DRIVER).read_bytes()),
        'adapter_source_sha256': sci.digest((sci.ROOT / 'tests/support/sci_execution_observation.js').read_bytes()),
        'inspection_source_sha256': sci.digest((sci.ROOT / context.INSPECTOR).read_bytes()),
        'library_source_sha256': context.local_libraries()}
    if any(local.get(key) != value for key, value in exact.items()) or set(local.get('engines', {})) != {'Node', 'QuickJS'}:
        raise ValueError('Unverified library source/driver/inspection/library hashes')
    local_statuses = []
    for kind in ('Node', 'QuickJS'):
        engine = local['engines'][kind]
        status = sci.verify(row, {'status': 'completed', 'observed': engine.get('observed')}, runner.same_json)['status']
        if status not in {'passed', 'mismatch'} or engine.get('status') != status:
            raise ValueError('Invalid local library observation/comparison')
        local_statuses.append(status)
    if local.get('status') != ('passed' if local_statuses == ['passed', 'passed'] else 'mismatch'):
        raise ValueError('Local library aggregate status changed')
    for kind, pin in (('release', runner.RELEASE), ('checkout', runner.REVISION)):
        layer = layers[kind]
        exact = {'source_sha256': row['source_sha256'], 'adapted_source_sha256': row['adapted_source_sha256'],
            'reference_identity': kind + ' ' + pin,
            'settings': {'optimizations': 'none', 'static-fns': False, 'def-emits-var': False},
            'oracle_source_sha256': sci.digest((sci.ROOT / context.ORACLE).read_bytes()),
            'oracle_helper_source_sha256': sci.digest((sci.ROOT / 'tests/reference/execution.clj').read_bytes()),
            'observation_source_sha256': sci.digest((sci.ROOT / 'tests/support/execution.js').read_bytes()),
            'namespace': row['reference_namespace'], 'isolated': True}
        if any(not runner.same_json(layer.get(key), value) for key, value in exact.items()):
            raise ValueError('Unverified library reference identity/fixture/hashes')
        status = sci.verify(row, {'status': 'completed', 'observed': layer.get('observed')}, runner.same_json)['status']
        if status not in {'passed', 'mismatch'} or layer.get('status') != status:
            raise ValueError('Invalid reference library observation/comparison')
    return 'original-source-agreement' if all(layers[kind]['status'] == 'passed' for kind in layers) else 'original-contract-unresolved'


def load():
    records = [json.loads(line) for line in RECEIPTS.read_text().splitlines() if line]
    rows = context.load()
    if [record['id'] for record in records] != [row['id'] for row in rows]:
        raise ValueError('Missing/extra/reordered library receipts')
    for row, record in zip(rows, records):
        if record.get('schema_version') != 1 or record.get('catalog_sha256') != sci.digest(context.CATALOG.read_bytes()) or json.dumps(record.get('source_binding'), sort_keys=True) != json.dumps(row, sort_keys=True):
            raise ValueError('Original library source/expectation binding changed')
        result = classification(row, record['layers'])
        if record.get('reconciliation') != result or record.get('original_expectation_status') != ('passed' if result == 'original-source-agreement' else 'unresolved'):
            raise ValueError('Library mismatch promoted to passing contract')
    return records


def verify(evidence):
    records = load()
    if list(evidence) != [record['id'] for record in records]:
        raise ValueError('Fresh library evidence identities changed')
    for row, record in zip(context.load(), records):
        classification(row, evidence[row['id']])
        for kind in ('javascript', 'release', 'checkout'):
            for key in ('status', 'observed', 'engines', 'detail'):
                if json.dumps(record['layers'][kind].get(key), sort_keys=True) != json.dumps(evidence[row['id']][kind].get(key), sort_keys=True):
                    raise ValueError('Literal library execution receipt changed')
    return {record['id']: record['reconciliation'] for record in records}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence')
    args = parser.parse_args()
    if args.evidence:
        document = json.loads(open(args.evidence).read())
        if document.get('catalog_sha256') != sci.digest(context.CATALOG.read_bytes()):
            raise ValueError('Fresh library evidence catalog changed')
        result = verify(document['evidence'])
    else:
        result = {record['id']: record['reconciliation'] for record in load()}
    print(dict(collections.Counter(result.values())))

if __name__ == '__main__': main()
