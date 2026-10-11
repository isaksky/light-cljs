#!/usr/bin/env python3
"""Validate bounded original library contracts without promoting mismatches."""
import argparse, collections, json
from pathlib import Path
import sci_library_context as context
import sci_execution as sci

RECEIPTS = context.HERE / 'sci-library-context-receipts.jsonl'


def classification(row, layers):
    runner = sci.report_module()
    if set(layers) != {'javascript', 'release', 'checkout'}:
        raise ValueError('Incomplete library engine/reference layers')
    local = layers['javascript']
    exact = {'source_sha256': row['source_sha256'], 'adapted_source_sha256': row['adapted_source_sha256'],
        'library_context_adapter_sha256': context.adapter_hash(),
        'compiler_source_sha256': context.compiler_sources(),
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
        raw_observation = context.observations(engine.get('raw_output', ''), [row])[row['id']]
        if (raw_observation.get('status') not in {'passed', 'mismatch'} or
                not runner.same_json(raw_observation.get('observed'), engine.get('observed'))):
            raise ValueError('Raw local library observation changed')
        status = sci.verify(row, {'status': 'completed', 'observed': engine.get('observed')}, runner.same_json)['status']
        if status not in {'passed', 'mismatch'} or engine.get('status') != status:
            raise ValueError('Invalid local library observation/comparison')
        local_statuses.append(status)
    if local.get('status') != ('passed' if local_statuses == ['passed', 'passed'] else 'mismatch'):
        raise ValueError('Local library aggregate status changed')
    for kind, pin in (('release', runner.RELEASE), ('checkout', runner.REVISION)):
        layer = layers[kind]
        if context.reference_walk_source(kind) != context.WALK_SOURCE_HASHES[kind]:
            raise ValueError('Pinned walk source changed after reviewed execution')
        exact = {'source_sha256': row['source_sha256'], 'adapted_source_sha256': row['adapted_source_sha256'],
            'walk_source_sha256': context.WALK_SOURCE_HASHES[kind],
            'library_context_adapter_sha256': context.adapter_hash(),
            'reference_identity': kind + ' ' + pin,
            'settings': {'optimizations': 'none', 'static-fns': False, 'def-emits-var': False},
            'oracle_source_sha256': sci.digest((sci.ROOT / context.ORACLE).read_bytes()),
            'oracle_helper_source_sha256': sci.digest((sci.ROOT / 'tests/reference/execution.clj').read_bytes()),
            'observation_source_sha256': sci.digest((sci.ROOT / 'tests/support/execution.js').read_bytes()),
            'namespace': row['reference_namespace'], 'isolated': True}
        if any(not runner.same_json(layer.get(key), value) for key, value in exact.items()):
            raise ValueError('Unverified library reference identity/fixture/hashes')
        build_stdout = layer.get('reference_build_stdout', '')
        if ('REFERENCE IDENTITY ' + kind + ' ' + pin not in build_stdout or
                'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in build_stdout or
                not isinstance(layer.get('reference_build_stderr'), str)):
            raise ValueError('Unchecked library reference build identity/settings')
        raw_observation = context.observations(layer.get('raw_stdout', ''), [row])[row['id']]
        if raw_observation.get('status') not in {'passed', 'mismatch'} or not runner.same_json(raw_observation.get('observed'), layer.get('observed')):
            raise ValueError('Raw reference library observation changed')
        frames = layer.get('raw_stderr', '').removesuffix('\n').split('\n')
        if (len(frames) != 2 or not frames[0].startswith('CASE START ') or not frames[1].startswith('CASE END ') or
                json.loads(frames[0][11:]) != {'id': row['id'], 'source': row['source']} or
                json.loads(frames[1][9:]) != {'id': row['id']}):
            raise ValueError('Raw reference library identity/stderr frames changed')
        status = sci.verify(row, {'status': 'completed', 'observed': layer.get('observed')}, runner.same_json)['status']
        if status not in {'passed', 'mismatch'} or layer.get('status') != status:
            raise ValueError('Invalid reference library observation/comparison')
    reviewed_contract = context.version_contract(row['sci_id'])
    if reviewed_contract is not None and row.get('classification') != 'pinned-version-difference':
        raise ValueError('Reviewed library version classification changed')
    if row.get('classification') == 'pinned-version-difference':
        contract = reviewed_contract
        if (contract is None or not runner.same_json(row.get('canonical_contract'), contract) or
                not runner.same_json(row['expected'], contract['checkout'])):
            raise ValueError('Unreviewed library pinned-version classification')
        if not all(runner.same_json(layers[kind]['observed'], contract[kind]) for kind in ('release', 'checkout')):
            raise ValueError('Library version difference changed literal pinned contracts')
        if runner.same_json(layers['release']['observed'], layers['checkout']['observed']):
            raise ValueError('Library version difference lacks distinct executed pinned contracts')
        matching_pin = contract['matching_pin']
        if not all(runner.same_json(engine['observed'], layers[matching_pin]['observed'])
                   for engine in local['engines'].values()):
            raise ValueError('Both local library engines must match the same reviewed pin')
        return 'verified-pinned-version-difference'
    if all(layers[kind]['status'] == 'passed' for kind in layers):
        return 'original-source-agreement'
    return 'original-contract-unresolved'


def full_report_evidence(row, layers):
    """Check fresh layers only; stored receipts never grant report coverage."""
    try:
        reconciliation = classification(row, layers)
    except (ValueError, KeyError, TypeError, AttributeError, OSError, context.zipfile.BadZipFile) as error:
        return {'status': 'missing', 'reconciliation': 'unverified',
                'original_expectation_status': 'unresolved', 'reason': str(error)}
    status = {'original-source-agreement': 'covered',
              'verified-pinned-version-difference': 'version-difference',
              'original-contract-unresolved': 'missing'}[reconciliation]
    return {'status': status, 'reconciliation': reconciliation,
            'original_expectation_status': 'passed' if status == 'covered' else 'unresolved',
            'reason': 'Fresh complete original-library Node/QuickJS and checked pinned reference observations verified.'}


def load():
    records = [json.loads(line) for line in RECEIPTS.read_text().splitlines() if line]
    rows = context.load()
    if [record['id'] for record in records] != [row['id'] for row in rows]:
        raise ValueError('Missing/extra/reordered library receipts')
    for row, record in zip(rows, records):
        if (record.get('schema_version') != 1 or record.get('catalog_sha256') != sci.digest(context.CATALOG.read_bytes()) or
                record.get('receipt_validator_sha256') != sci.digest(Path(__file__).read_bytes()) or
                json.dumps(record.get('source_binding'), sort_keys=True) != json.dumps(row, sort_keys=True)):
            raise ValueError('Original library source/expectation binding changed')
        result = classification(row, record['layers'])
        if record.get('reconciliation') != result or record.get('original_expectation_status') != ('passed' if result == 'original-source-agreement' else 'unresolved'):
            raise ValueError('Library mismatch promoted to passing contract')
    return records


def record_evidence(document, evidence_path, prior_records=()):
    """Record complete fresh layers, retaining earlier active and raw history."""
    catalog = context.load()
    catalog_hash = sci.digest(context.CATALOG.read_bytes())
    if document.get('catalog_sha256') != catalog_hash:
        raise ValueError('Fresh library evidence catalog changed')
    evidence = document.get('evidence', {})
    if list(evidence) != [row['id'] for row in catalog]:
        raise ValueError('Fresh library evidence identities changed')
    previous = {record['id']: record for record in prior_records}
    if len(previous) != len(prior_records):
        raise ValueError('Duplicate prior library receipt identity')
    records = []
    evidence_hash = sci.digest(Path(evidence_path).read_bytes())
    validator_hash = sci.digest(Path(__file__).read_bytes())
    for row in catalog:
        layers = evidence[row['id']]
        result = classification(row, layers)
        record = {'schema_version': 1, 'id': row['id'], 'catalog_sha256': catalog_hash,
                  'receipt_validator_sha256': validator_hash,
                  'source_binding': row, 'layers': layers, 'reconciliation': result,
                  'original_expectation_status': 'passed' if result == 'original-source-agreement' else 'unresolved',
                  'evidence_path': str(evidence_path), 'evidence_sha256': evidence_hash}
        prior = previous.get(row['id'])
        if prior:
            record['prior_evidence'] = dict(prior)
        records.append(record)
    return records


def verify(evidence):
    records = load()
    if list(evidence) != [record['id'] for record in records]:
        raise ValueError('Fresh library evidence identities changed')
    for row, record in zip(context.load(), records):
        classification(row, evidence[row['id']])
        for kind in ('javascript', 'release', 'checkout'):
            for key in ('status', 'observed', 'engines', 'detail', 'raw_stdout', 'raw_stderr', 'reference_build_stdout', 'reference_build_stderr'):
                if json.dumps(record['layers'][kind].get(key), sort_keys=True) != json.dumps(evidence[row['id']][kind].get(key), sort_keys=True):
                    raise ValueError('Literal library execution receipt changed')
    return {record['id']: record['reconciliation'] for record in records}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--evidence')
    mode.add_argument('--write-evidence', type=Path)
    args = parser.parse_args()
    if args.write_evidence:
        document = json.loads(args.write_evidence.read_text())
        prior = [json.loads(line) for line in RECEIPTS.read_text().splitlines() if line] if RECEIPTS.exists() else []
        records = record_evidence(document, args.write_evidence, prior)
        temporary = RECEIPTS.with_suffix('.jsonl.tmp')
        temporary.write_text(''.join(json.dumps(record, separators=(',', ':')) + '\n' for record in records))
        temporary.replace(RECEIPTS)
        result = {record['id']: record['reconciliation'] for record in load()}
    elif args.evidence:
        document = json.loads(open(args.evidence).read())
        if document.get('catalog_sha256') != sci.digest(context.CATALOG.read_bytes()):
            raise ValueError('Fresh library evidence catalog changed')
        result = verify(document['evidence'])
    else:
        result = {record['id']: record['reconciliation'] for record in load()}
    print(dict(collections.Counter(result.values())))

if __name__ == '__main__': main()
