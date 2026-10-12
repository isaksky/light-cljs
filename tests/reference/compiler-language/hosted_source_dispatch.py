"""Reviewed source embeddings; registration never supplies execution credit."""
import copy
import contextlib
from pathlib import Path
import report
import configured_source_context as configured
import hosted_import_source_context as imports
import hosted_namespace_source_context as namespaces
import local_api_source_context as local_apis
import configured_custom_source_context as custom

PROFILES = {configured.ID: 'configured-reader-identity-v1', imports.ID: 'original-import-real-closure-v1',
            namespaces.SPECS[0][1]: 'original-goog-real-delegation-v1',
            namespaces.SPECS[1][1]: 'original-repl-error-map-v1'}
# The helper knows all 121 reviewed sources. Catalog integration remains the
# separately reviewed subset of 30 exact exported cases, never all known IDs.
LOCAL_API_IDS = frozenset(case['id'] for case in local_apis.cases())
PROFILES.update({identity: local_apis.PROFILE for identity in LOCAL_API_IDS})
PROFILES[custom.ID] = custom.PROFILE
BOUNDARIES = {
 imports.ID: 'Complete original import body with genuine pinned Long/Integer/Vec2/Vec3 dependency closure installed in application and actual MacroHost. No general Closure build/library parity.',
 namespaces.SPECS[0][1]: 'Complete original goog body uses a compiled delegation to genuine pinned Closure isArrayLike. Real base initialization is explicit in both phases. No default goog namespace or foreign-function-root parity.',
 namespaces.SPECS[1][1]: 'Both complete original REPL bodies use the exact original Error->map source function and core Throwable->map dependency. Both references execute canonical cljs.repl. No entire cljs.repl namespace compatibility.'}

def register(case):
    case = copy.deepcopy(case)
    case['hosted_source_adapter'] = {'profile': PROFILES[case['id']], 'schema_version': 1}
    return case

def ordinary_profiles():
    rows = [imports.load(), *namespaces.load()]
    for row in rows:
        row['reference_adaptation'] = BOUNDARIES[row['id']] + ' Every original setup, assertion, expected value and failure path is retained; only the test assertion harness becomes assert.'
        if row['id'] == imports.ID:
            row['module_source'] = imports.MODULE
            row['complete_original_provenance'] = [{'body_id': imports.MODULE+'::test-import', 'source_sha256': imports.BODY_HASH}]
        else:
            spec = next(s for s in namespaces.SPECS if s[1] == row['id'])
            row['module_source'] = spec[3]
            row['complete_original_provenance'] = [{'body_id': key, 'source_sha256': value} for key,value in row['original_complete_body_sha256'].items()]
    return [register(row) for row in rows] + [custom.case()]

def validate_case(case):
    identity = case.get('id')
    marker = case.get('hosted_source_adapter')
    if identity not in PROFILES:
        if (str(identity).startswith('local-api-source-') or marker is not None
                or any(k in case for k in ('compiler_data_readers','hosted_import_profile','hosted_namespace_profile'))):
            raise ValueError('unknown hosted source profile')
        return False
    if not report.same_json(marker, {'profile': PROFILES[identity], 'schema_version': 1}):
        raise ValueError('missing/changed reviewed hosted source dispatch profile')
    if identity in LOCAL_API_IDS:
        if not local_apis.validate(case):
            raise ValueError('unknown original local API source')
        expected = next(row for row in local_apis.cases() if row['id'] == identity)
        if not report.same_json(case, expected):
            raise ValueError('changed original local API source/typed contract')
    elif identity == custom.ID:
        custom.validate_case(case)
    elif identity == configured.ID:
        configured.validate(case)
    else:
        expected = next(row for row in ordinary_profiles() if row['id'] == identity)
        if not report.same_json(case, expected):
            raise ValueError('changed/reduced hosted source registration')
    return True

def validate_witnesses(case, witnesses, inventory):
    if not validate_case(case):return
    if case['id'] in LOCAL_API_IDS:
        # SCI sources have operation witnesses and an exact oracle, not
        # invented upstream assertion/body witnesses. Ordinary mapping
        # eligibility is still checked independently by report/full_ledger.
        if any(row['fixture_id'] == case['id'] for row in witnesses.values()):
            raise ValueError('original local API source cannot claim upstream assertion witnesses')
        return
    origins={row['body_id']:row['source_sha256'] for row in case['complete_original_provenance']}
    expected=[row['id'] for row in inventory.values() if row['body_id'] in origins and row['disposition']=='in-scope']
    actual=[row['assertion_id'] for row in witnesses.values() if row['fixture_id']==case['id']]
    if expected != actual or not expected:
        raise ValueError('missing/extra/reordered original hosted source witnesses')
    for identity in expected:
        if inventory[identity]['body_sha256']!=origins[inventory[identity]['body_id']]:
            raise ValueError('hosted assertion detached from complete original body')

def layers(document, identity):
    observations = document['observations']
    if identity in observations:
        observations = observations[identity]
    expected = [v+'/'+e for v in configured.VARIANTS for e in ('QuickJS','Node')]
    if document.get('status') != 'passed' or list(observations) != expected:
        raise ValueError('incomplete current hosted source evidence')
    for layer in observations.values():
        if (layer.get('status') != 'passed' or not layer.get('raw_observation_sha256')
                or not layer.get('compiler_build_producer_source_sha256')
                or not layer.get('audit_executable_sha256') or not layer.get('compiled_js_sha256')
                or not report.same_json(layer.get('observed'), {'id':identity,'outcome':{'status':'returned','value':True},'trace':[],'stdout':''})):
            raise ValueError('missing current raw hosted source provenance')
    return {'status': 'passed', 'observed': observations[expected[0]]['observed'],
            'hosted_source_adapter': PROFILES[identity], 'observations': observations,
            'generic_cli': 'not claimed'}


def local_api_layers(case, layer):
    """Validate actual local/phase metadata independently of source registration."""
    variants = ('application-fresh', 'application-reused', 'macro-fresh', 'macro-reused')
    expected = [variant+'/'+engine for variant in variants for engine in ('QuickJS', 'Node')]
    observations = layer.get('observations', {})
    if (layer.get('status') != 'passed' or list(observations) != expected
            or layer.get('hosted_source_adapter') != local_apis.PROFILE
            or layer.get('generic_cli') != 'not claimed'
            or not report.same_json(layer.get('observed'), case['expected'])):
        raise ValueError('incomplete current original local API source evidence')
    producers = local_apis.snapshot()
    for slot, observed in observations.items():
        variant, engine = slot.split('/')
        phase, host = variant.split('-')
        frame = {'id': case['id'], 'phase': phase, 'host': host, 'engine': engine, 'raw': 'true'}
        if (observed.get('status') != 'passed'
                or not report.same_json(observed.get('observed'), case['expected'])
                or not report.same_json(observed.get('actual_frame'), frame)
                or observed.get('raw_observation_sha256') != local_apis.digest('true')
                or not report.same_json(observed.get('compiler_build_producer_source_sha256'), producers)
                or any(not isinstance(observed.get(key), str) or len(observed[key]) != 64
                       or any(ch not in '0123456789abcdef' for ch in observed[key])
                       for key in ('audit_executable_sha256', 'compiled_js_sha256', 'module_source_sha256'))):
            raise ValueError('missing current raw original local API source provenance')
    return layer


def _execute(cases, directory, timeout, reference='both', checkout=None):
    if reference not in ('none','release','checkout','both'):
        raise ValueError('unknown reference selection')
    if checkout is not None and Path(checkout).resolve() != (report.ROOT/'research/clojurescript').resolve():
        raise ValueError('hosted profiles require the reviewed exact checkout')
    selected = {case['id']:case for case in cases if validate_case(case)}
    if len(selected) != len(cases):
        raise ValueError('generic source routed into hosted dispatch')
    directory=Path(directory); results={}
    if configured.ID in selected:
        case=selected[configured.ID]; doc=configured.local_cases(case,directory/'configured'/'local',timeout)
        results[case['id']]={'javascript': layers(doc,case['id'])}
        for kind in ('release','checkout'):
            if reference in (kind,'both'):
                results[case['id']][kind]=report.reference_cases([case],kind,report.ROOT/'research/clojurescript',directory/'configured'/kind,timeout)[case['id']]
    if imports.ID in selected:
        doc=imports.execute(imports.load(),directory/'imports',timeout)
        results[imports.ID]={'javascript':layers(doc,imports.ID)}
        for kind in ('release','checkout'):
            if reference in (kind,'both'):results[imports.ID][kind]=doc['references'][kind]
    namespace_ids=set(selected)&{s[1] for s in namespaces.SPECS}
    if namespace_ids:
        doc=namespaces.execute(namespaces.load(),directory/'namespaces',timeout)
        for identity in namespace_ids:
            results[identity]={'javascript':layers(doc,identity)}
            for kind in ('release','checkout'):
                if reference in (kind,'both'):results[identity][kind]=doc['references'][kind][identity]
    if custom.ID in selected:
        result = custom.execute([selected[custom.ID]], directory/'custom', timeout, reference, checkout)
        if set(result) != {custom.ID}:
            raise ValueError('missing/extra current custom-reader hosted result')
        layers({'status': result[custom.ID]['javascript'].get('status'),
                'observations': result[custom.ID]['javascript'].get('observations', {})}, custom.ID)
        results.update(result)
    api_cases = [case for identity, case in selected.items() if identity in LOCAL_API_IDS]
    if api_cases:
        # The exact helper always executes the complete original 121-source
        # cohort, then returns only the requested source identities.
        result = local_apis.execute(api_cases, directory/'local-api', timeout, reference, checkout)
        if set(result) != {case['id'] for case in api_cases}:
            raise ValueError('missing/extra current original local API source results')
        for case in api_cases:
            result[case['id']]['javascript'] = local_api_layers(case, result[case['id']]['javascript'])
        results.update(result)
    return results

def execute(cases, directory, timeout, reference='both', checkout=None):
    if reference not in ('none','release','checkout','both') or any(not validate_case(c) for c in cases):
        raise ValueError('invalid hosted source selection')
    if checkout is not None and Path(checkout).resolve() != (report.ROOT/'research/clojurescript').resolve():
        raise ValueError('hosted profiles require the reviewed exact checkout')
    paths = [Path(__file__), Path(__file__).with_name('test_hosted_source_dispatch.py'),
             report.ROOT/'tests/reference/compiler-residual-source-context.py']
    before = {str(p.relative_to(report.ROOT)): configured.digest(p.read_bytes()) for p in paths}
    directory=Path(directory)
    if directory.exists():raise ValueError('fresh hosted dispatch artifacts required')
    directory.mkdir(parents=True)
    with (directory/'progress.log').open('w') as progress, contextlib.redirect_stdout(progress):
        results=_execute(cases,directory,timeout,reference,checkout)
    after = {str(p.relative_to(report.ROOT)): configured.digest(p.read_bytes()) for p in paths}
    configured.unchanged(before,after)
    for result in results.values():result['javascript']['dispatch_producer_source_sha256']=before
    return results
