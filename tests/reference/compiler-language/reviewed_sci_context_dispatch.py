"""Fresh exact-nine SCI contexts: four agreements and five canonical differences.

Whole original sources and outcomes stay bound. No stored sidecar lookup,
registration credit, generic source fallback or broad record exclusion exists.
"""
from pathlib import Path
import sci_reviewed_source_profiles as agreements
import sci_record_method_dialect as differences
import sci_record_declaration_context as declarations

sci = agreements.sci
ROOT = sci.ROOT
PROFILE = 'reviewed-nine-original-sci-contexts-v1'


def same(actual, expected, message):
    agreements.same(actual, expected, message)


def bindings():
    rows = agreements.load()
    rows += [{**row, 'expected': row['original_expected']} for row in differences.load()]
    rows += [{**row, 'expected': row['original_expected']} for row in declarations.load()]
    if len(rows) != 9 or len({r['id'] for r in rows}) != 9:
        raise ValueError('Exact reviewed source domains overlap or change')
    return rows


def partition(rows):
    reviewed = {r['sci_id']: r for r in bindings()}
    ordinary, selected = [], []
    for row in rows:
        expected = reviewed.get(row.get('sci_id'))
        if expected is None:
            ordinary.append(row)
            continue
        for key in ('id', 'obligation_id', 'source_sha256', 'original_source_sha256',
                    'original_row_sha256', 'body_sha256', 'expected'):
            same(row.get(key), expected[key], 'Original source/typed outcome binding drift')
        if 'source' in row:
            same(row['source'], expected['source'], 'Reduced original reviewed source')
        selected.append(expected)
    if len({r['id'] for r in selected}) != len(selected):
        raise ValueError('Duplicate reviewed original source')
    return ordinary, selected


def snapshot():
    paths = (Path(__file__), Path(__file__).with_name('test_reviewed_sci_context_dispatch.py'))
    return {str(p.relative_to(ROOT)): sci.digest(p.read_bytes()) for p in paths}


def reconcile(document):
    """Replay the freshly supplied raw proofs, keeping SCI mismatches explicit."""
    same(document.get('schema_version'), 1, 'Dispatcher schema drift')
    same(document.get('profile'), PROFILE, 'Dispatcher profile drift')
    same(document.get('source_bindings'), bindings(), 'Original nine sources drift')
    same(document.get('producer_source_sha256'), snapshot(), 'Current dispatcher drift')
    if document.get('status') != 'verified':
        raise ValueError('Incomplete fresh reviewed source execution')
    result = agreements.reconcile(document['agreements'])
    canonical = differences.reconcile(document['differences'])
    extra = declarations.reconcile(document['declarations'])
    if set(canonical) & set(extra):
        raise ValueError('Canonical difference source domains overlap')
    canonical.update(extra)
    for identity, entry in canonical.items():
        if identity in result:
            raise ValueError('Overlapping agreement/difference proof')
        row = entry['source_binding']; frames = entry['actual_phase_frames']
        engines = {}
        for engine in ('QuickJS', 'Node'):
            observed = frames['application-fresh/'+engine]['observed']
            same(observed, row['canonical_expected'], 'Canonical application packet drift')
            if sci.report_module().same_json(observed, row['original_expected']):
                raise ValueError('Canonical difference collapsed into SCI agreement')
            engines[engine] = {'status': 'mismatch', 'observed': observed,
                'original_expectation_status': 'unresolved'}
        layers = {'javascript': {'status': 'mismatch', 'engines': engines,
            'source_sha256': row['source_sha256'], 'observed': row['canonical_expected'],
            'actual_phase_frames': frames, 'generic_cli': 'not claimed'},
            **entry['references']}
        for kind in ('release', 'checkout'):
            if layers[kind].get('status') != 'mismatch':
                raise ValueError('Original SCI mismatch hidden')
            same(layers[kind].get('observed'), row['canonical_expected'],
                 'Required-pin canonical difference drift')
        result[identity] = {**entry, 'status': 'version-difference',
            'reconciliation': differences.STATUS, 'id': row['id'], 'layers': layers}
    same(list(result), [r['obligation_id'] for r in bindings()], 'Missing/extra source proof')
    return result


def run(directory, timeout=240):
    directory = Path(directory).resolve()
    directory.mkdir(parents=True, exist_ok=False)
    before = snapshot()
    document = {'schema_version': 1, 'profile': PROFILE, 'source_bindings': bindings(),
        'producer_source_sha256': before,
        'agreements': agreements.run(directory/'agreements', timeout),
        'differences': differences.run(directory/'differences', timeout),
        'declarations': declarations.run(directory/'declarations', timeout),
        'status': 'verified'}
    same(snapshot(), before, 'Dispatcher changed during execution')
    reconcile(document)
    (directory/'evidence.json').write_text(sci.json.dumps(document, indent=2)+'\n')
    return document
