"""Fresh, replayable complete original SCI .cljc reader contexts.

An original grants credit only with its exact reviewed literal in both checked
pins and all eight actual application/macro engine/lifecycle frames. Rejections,
transport failures and originals without a reviewed literal remain nonpassing.
"""
from pathlib import Path
import argparse
import copy
import json
import os
import shutil
import subprocess
import sci_cljc_source_probe as historical
import sci_execution as sci
import sci_source_context as context
import sci_symbol_keyword_context as packets
import report

ROOT = sci.ROOT
PROFILE = 'exact-original-sci-cljc-reader-context-v1'
DRIVER = ROOT/'tests/compiler_sci_cljc_accepted_probe.rs'
ORACLE = Path(__file__).with_suffix('.clj')
CONTRACTS = Path(__file__).with_name('sci-cljc-accepted-contracts.jsonl')
CONTRACTS_SHA256 = 'd506d990b544862324d7bd30ccca8df37f1bb45582b539bf05c2026f992967d5'
TEST = 'accepted_original_cljc_sources_execute_in_fresh_and_reused_macro_hosts'
VARIANTS = historical.VARIANTS


def same(actual, expected, reason):
    if not report.same_json(actual, expected):
        raise ValueError(reason)


def load():
    raw = CONTRACTS.read_bytes()
    same(sci.digest(raw), CONTRACTS_SHA256, 'Unreviewed canonical reader literal catalog')
    contracts = [json.loads(line) for line in raw.decode().splitlines()]
    originals = historical.catalog()
    same(len(contracts), 30, 'Missing complete original reader cohort')
    same(len(originals), 30, 'Missing immutable original reader cohort')
    same(len({row['id'] for row in contracts}), 30, 'Duplicate canonical reader source')
    same([{key: row[key] for key in original} for row, original in zip(contracts, originals)], originals,
         'Changed original source/body/row/typed SCI expectation')
    result = []
    for row in contracts:
        canonical = row['canonical_expected']
        if canonical is not None:
            same(set(canonical), {'id', 'outcome', 'trace', 'stdout'}, 'Incomplete typed canonical packet')
            same(canonical['id'], row['id'], 'Wrong canonical source identity')
            if not isinstance(canonical['trace'], list) or not isinstance(canonical['stdout'], str):
                raise ValueError('Invalid canonical effects/stdout')
        if not row['scope_basis'] or not row['reason']:
            raise ValueError('Missing exact source-bound scope/reason')
        result.append({**row, 'original_expected': copy.deepcopy(row['expected']),
                       'source_context_profile': PROFILE, 'comparison': sci.PROJECTION})
    return result


def reference_inputs():
    checkout=ROOT/'research/clojurescript'
    head=subprocess.check_output(['git','-C',str(checkout),'rev-parse','HEAD'],text=True).strip()
    same(head,report.REVISION,'Current reference checkout revision drift')
    status=subprocess.check_output(['git','-C',str(checkout),'status','--porcelain','--untracked-files=normal'],text=True)
    same(status,'','Current reference checkout source drift')
    paths=sorted(p for p in (checkout/'src/main').rglob('*') if p.is_file())
    paths.append(checkout/'deps.edn')
    jar=Path.home()/'.m2/repository/org/clojure/clojurescript'/report.RELEASE/('clojurescript-'+report.RELEASE+'.jar')
    paths.append(jar)
    return {str(p):sci.digest(p.read_bytes()) for p in paths}


def snapshot():
    paths = [Path(__file__), ORACLE, DRIVER, CONTRACTS,
             Path(__file__).with_name('test_sci_cljc_accepted_context.py'),
             Path(historical.__file__), historical.FROZEN, Path(sci.__file__),
             Path(context.__file__), Path(packets.__file__), Path(report.__file__),
             sci.CORPUS, sci.DISPOSITIONS, ROOT/'tests/reference/execution.clj',
             ROOT/'tests/support/execution.js', ROOT/'tests/support/sci_execution_observation.js']
    return {**context.compiler_sources(), **{str(p.relative_to(ROOT)): sci.digest(p.read_bytes()) for p in paths},
            **reference_inputs()}


def tree(root, exclude=()):
    return {str(p.relative_to(root)): sci.digest(p.read_bytes()) for p in sorted(root.rglob('*'))
            if p.is_file() and str(p.relative_to(root)) not in exclude}


def executable_paths():
    result = {}
    for name in ('node', 'clojure', 'cargo'):
        path = Path(shutil.which(name) or '').resolve()
        if not path.is_file():
            raise ValueError('Missing actual executable: '+name)
        result[name] = str(path)
    return result


def command(command, directory, timeout, *, source=None, environment=None, record_environment=None, allow_failure=False):
    directory.mkdir(parents=True, exist_ok=False)
    ran = subprocess.run(command, cwd=ROOT, input=source, text=True, capture_output=True,
                         timeout=timeout, env=environment)
    (directory/'stdout').write_text(ran.stdout)
    (directory/'stderr').write_text(ran.stderr)
    record = {'command': command, 'cwd': str(ROOT), 'returncode': ran.returncode,
              'stdin': source, 'environment': record_environment if record_environment is not None else environment or {}}
    (directory/'command.json').write_text(json.dumps(record, indent=2)+'\n')
    if ran.returncode and not allow_failure:
        raise ValueError('Actual producer command failed: '+str(directory))
    return ran


def check_command(path, expected, *, stdin=None, environment=None, allow_failure=False):
    record = json.loads((path/'command.json').read_text())
    same(record.get('command'), expected, 'Wrong exact actual command')
    same(record.get('cwd'), str(ROOT), 'Wrong actual command working directory')
    same(record.get('stdin'), stdin, 'Changed actual command stdin')
    same(record.get('environment'), environment or {}, 'Changed actual command environment binding')
    if type(record.get('returncode')) is not int or (not allow_failure and record['returncode'] != 0):
        raise ValueError('Actual command did not complete successfully')
    return record


def input_text(rows):
    return ''.join(row['id']+'\t'+row['source'].encode().hex()+'\n' for row in rows)


def node_script(root, index):
    return ('process.chdir('+json.dumps(str(root))+');require('+json.dumps(str(root/'reference.js'))+');\n'
            +(root/'contexts.js').read_text()+'\nglobalThis.lightCases=[lightCases['+str(index)+']];'
            'globalThis.lightCaseMetadata=[lightCaseMetadata['+str(index)+']];\n'
            +(ROOT/'tests/support/execution.js').read_text())


def reference_command(kind, root, programs):
    identity = report.RELEASE if kind == 'release' else report.REVISION
    checkout = ROOT/'research/clojurescript'
    dependency = ('{:mvn/version "'+report.RELEASE+'"}' if kind == 'release'
                  else '{:local/root '+json.dumps(str(checkout))+'}')
    return [programs['clojure'], '-Srepro', '-Sdeps', '{:paths [] :deps {org.clojure/clojurescript '+dependency+'}}',
            '-M', str(ORACLE), kind, identity, '-' if kind == 'release' else str(checkout),
            str(root), str(root/'input.tsv')]


def produce_reference(kind, root, rows, programs, timeout):
    root.mkdir()
    (root/'input.tsv').write_text(input_text(rows))
    command(reference_command(kind, root, programs), root/'build', timeout)
    records = [line.split('\t') for line in (root/'contexts.tsv').read_text().splitlines()]
    same([r[0] for r in records], [r['id'] for r in rows], 'Changed actual reference order')
    for index, (row, rec) in enumerate(zip(rows, records)):
        if rec[1] == 'compiled':
            command([programs['node'], '-'], root/'node'/row['id'], 30,
                    source=node_script(root, index), allow_failure=True)


def verify_reference(kind, root, rows, programs):
    same((root/'input.tsv').read_text(), input_text(rows), 'Changed full original reference input')
    check_command(root/'build', reference_command(kind, root, programs))
    build = (root/'build/stdout').read_text()
    identity = kind+' '+(report.RELEASE if kind == 'release' else report.REVISION)
    if build.count('REFERENCE IDENTITY '+identity) != 2 or build.count('REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}') != 1:
        raise ValueError('Missing checked pin/settings/whole-source compiler proof')
    records = [line.split('\t') for line in (root/'contexts.tsv').read_text().splitlines()]
    same([r[0] for r in records], [r['id'] for r in rows], 'Missing/duplicate/reordered original pin rows')
    metadata = 'globalThis.lightCaseMetadata=['+','.join('['+json.dumps(r['id'])+','+json.dumps(r['source'])+']' for r in rows)+'];\n'
    compiled = []
    result = {}
    expected_source_files = set()
    for index, (row, rec) in enumerate(zip(rows, records)):
        if len(rec) != 3 or rec[1] not in {'compiled', 'rejected'}:
            raise ValueError('Malformed original reference source result')
        folder = root/'original-sources'/row['id']
        same((folder/'sci-original.cljc').read_text(), row['source'], 'Changed original .cljc source bytes')
        expected_source_files.add(row['id']+'/sci-original.cljc')
        if rec[1] == 'rejected':
            same((folder/'rejection.txt').read_text(), rec[2], 'Changed actual original reader rejection')
            expected_source_files.add(row['id']+'/rejection.txt')
            compiled.append('null')
            layer = {'status': 'rejected', 'detail': rec[2]}
        else:
            same(rec[2], '', 'Compiled source reported rejection text')
            unit = (folder/'compiled.js').read_text()
            if not unit.startswith('function(observe) { user.observe=observe;\n') or not unit.endswith('\n}'):
                raise ValueError('Invalid full original pin emitted wrapper')
            expected_source_files.add(row['id']+'/compiled.js')
            compiled.append(unit)
            node = root/'node'/row['id']
            record = check_command(node, [programs['node'], '-'], stdin=node_script(root, index), allow_failure=True)
            stdout = (node/'stdout').read_text(); stderr = (node/'stderr').read_text()
            if record['returncode']:
                layer = {'status': 'observation-error', 'detail': {'returncode': record['returncode'], 'stdout': stdout, 'stderr': stderr}}
            else:
                markers = stderr.splitlines()
                if len(markers) != 2 or not markers[0].startswith('CASE START ') or not markers[1].startswith('CASE END '):
                    raise ValueError('Unverified source-bearing pin observation markers')
                same(packets.strict_json(markers[0].removeprefix('CASE START ')), {'id': row['id'], 'source': row['source']}, 'Pin execution detached from full original source')
                same(packets.strict_json(markers[1].removeprefix('CASE END ')), {'id': row['id']}, 'Wrong pin observation end')
                # A finite observer packet must be one typed JSON line.
                lines = stdout.splitlines()
                if len(lines) != 1:
                    layer = {'status': 'observation-error', 'detail': {'stdout': stdout, 'stderr': stderr}}
                else:
                    observed = packets.strict_json(lines[0])
                    same(observed.get('id'), row['id'], 'Wrong pin observation identity')
                    layer = observation(row, observed)
        layer.update(reference_identity=identity, settings={'optimizations':'none','static-fns':False,'def-emits-var':False},
                     original_source_sha256=row['source_sha256'], source_name='sci-original.cljc')
        result[row['id']] = layer
    same(set(tree(root/'original-sources')), expected_source_files, 'Missing/extra original source/emitted pin artifacts')
    same((root/'contexts.js').read_text(), 'globalThis.lightCases=['+','.join(compiled)+'];\n'+metadata,
         'Pin emitted module/source marker census drift')
    required_nodes = {row['id']+'/'+name for row, rec in zip(rows, records) if rec[1]=='compiled'
                      for name in ('stdout','stderr','command.json')}
    same(set(tree(root/'node')), required_nodes, 'Missing/extra actual pin execution artifacts')
    return result


def observation(row, observed):
    same(set(observed), {'id','outcome','trace','stdout'}, 'Invalid complete typed observation packet')
    if not isinstance(observed['outcome'], dict) or not isinstance(observed['trace'], list) or not isinstance(observed['stdout'], str):
        raise ValueError('Invalid typed outcome/effect/stdout packet')
    same(observed['id'], row['id'], 'Wrong actual observation identity')
    canonical = row['canonical_expected']
    return {'status': 'passed' if canonical is not None and report.same_json(sci.projection(observed), sci.projection(canonical)) else 'mismatch',
            'observed': observed, 'original_status': 'passed' if report.same_json(sci.projection(observed), sci.projection(row['original_expected'])) else 'mismatch'}


def local_script(identity, unit):
    anchor = ' = // Original bounded runtime'
    if unit.count(anchor) != 1:
        raise ValueError('Missing exact compiled namespace runtime/export anchor')
    captured = unit.replace(anchor, ' = globalThis.lightRuntime = // Original bounded runtime', 1)
    return "(() => {const run="+captured+";return ("+(ROOT/'tests/support/sci_execution_observation.js').read_text()+")(globalThis.lightRuntime)('"+identity+"',observe=>run(observe));})()"


def verify_local(root, rows, binding):
    same((root/'input.tsv').read_text(), input_text(rows), 'Changed complete phase input')
    build_command = [binding['programs']['cargo'], 'test','--locked','--test',DRIVER.stem,'--no-run','--message-format=json']
    check_command(root/'build', build_command)
    artifacts = [json.loads(line) for line in (root/'build/stdout').read_text().splitlines() if line.startswith('{')]
    binaries = [r['executable'] for r in artifacts if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')==DRIVER.stem and r.get('executable')]
    same(binaries, [binding['audit_executable']], 'Wrong exact Cargo-built audit executable')
    check_command(root/'audit', [binaries[0],TEST,'--ignored','--exact','--nocapture'], environment=binding['audit_environment'])
    log = (root/'audit/stdout').read_text()
    if 'running 1 test' not in log or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in log:
        raise ValueError('Missing actual exact audit test execution')
    same((root/'audit/stderr').read_text(), '', 'Actual phase audit stderr contamination')
    warmup = (root/'warmup.js').read_text()
    if not warmup.startswith('(() => {\n') or not warmup.endswith('})()\n') or '  return 42;\n' not in warmup:
        raise ValueError('Missing real demanded-macro reused host warmup')
    records = [line.split('\t') for line in (root/'observations.tsv').read_text().splitlines()]
    cursor = 0; result = {}; required = {'input.tsv','warmup.js','observations.tsv'}
    required |= {prefix+'/'+name for prefix in ('build','audit') for name in ('stdout','stderr','command.json')}
    for row in rows:
        frames = {}
        for variant in VARIANTS:
            folder = root/row['id']/variant; prefix = row['id']+'/'+variant+'/'
            source = historical.source(row['source'], variant.split('-')[0])
            same((folder/'user.cljc').read_text(), source, 'Reduced/changed original actual phase .cljc module')
            required.add(prefix+'user.cljc')
            if cursor >= len(records):
                raise ValueError('Missing original actual phase frame')
            fields = records[cursor]
            same(fields[:2], [row['id'],variant], 'Duplicate/reordered original phase frame')
            same(bytes.fromhex(fields[3]).decode(), source, 'Actual source frame detached from namespace source')
            if fields[2] == 'rejected':
                same(len(fields), 5, 'Malformed actual phase rejection')
                detail = bytes.fromhex(fields[4]).decode()
                same((folder/'rejection.txt').read_text(), detail, 'Changed actual compile rejection')
                required.add(prefix+'rejection.txt')
                frames[variant+'/rejected'] = {'status':'rejected','detail':detail,'phase_source_sha256':sci.digest(source)}
                cursor += 1
                continue
            unit = (folder/'compiled.js').read_text(); script = local_script(row['id'], unit)
            same((folder/'script.js').read_text(), script, 'Changed whole-source typed observer script')
            same((folder/'node.stdin').read_text(), 'console.log('+script+');', 'Changed actual Node input')
            required |= {prefix+name for name in ('compiled.js','script.js','quickjs.raw','node.stdin','node.stdout','node.stderr','node.exit')}
            stdout = (folder/'node.stdout').read_text(); stderr = (folder/'node.stderr').read_text()
            exit_code = int((folder/'node.exit').read_text())
            expected_node = stdout.strip() if exit_code==0 and not stderr else 'HARNESS_ERROR:'+stderr
            for engine in ('QuickJS','Node'):
                if cursor >= len(records): raise ValueError('Missing actual engine pair')
                fields = records[cursor]; cursor += 1
                same(fields[:3], [row['id'],variant,engine], 'Duplicate/reordered actual engine pair')
                same(len(fields),7,'Malformed actual source frame')
                same(bytes.fromhex(fields[3]).decode(),source,'Changed actual phase source')
                same(bytes.fromhex(fields[4]).decode(),unit,'Engine detached from actual compiled namespace')
                raw = bytes.fromhex(fields[5]).decode()
                same(raw,(folder/'quickjs.raw').read_text() if engine=='QuickJS' else expected_node,'Changed raw engine outcome')
                same(fields[6],'-' if variant.endswith('fresh') else warmup.encode().hex(),'Missing/wrong actual reused warmup')
                if raw.startswith('HARNESS_ERROR:'):
                    layer={'status':'observation-error','detail':raw}
                else:
                    layer=observation(row, packets.strict_json(raw))
                layer.update(phase_source_sha256=sci.digest(source), compiled_js_sha256=sci.digest(unit),
                             raw_observation_sha256=sci.digest(raw), warmup_compiled_js_sha256=None if variant.endswith('fresh') else sci.digest(warmup))
                frames[variant+'/'+engine]=layer
        result[row['id']]=frames
    same(cursor,len(records),'Extra actual phase/engine frames')
    same(set(tree(root)),required,'Missing/extra actual local source/raw/build/audit artifacts')
    return result


def verify_binding(root, binding):
    same(binding['producer_source_sha256'], snapshot(), 'Current source/compiler/producer drift')
    same(binding['programs'], executable_paths(), 'Actual executable path drift')
    same(set(binding['executables_sha256']), set(binding['programs'].values())|{binding['audit_executable']}, 'Missing actual Cargo/audit/Node/reference executable hashes')
    for name, expected in binding['executables_sha256'].items():
        same(sci.digest(Path(name).read_bytes()), expected, 'Current actual executable hash drift')
    same(binding['audit_environment'], {'LIGHT_SCI_CLJC_ACCEPTED_ROOT':str(root/'phases'),'NODE':binding['programs']['node']}, 'Wrong actual phase source/Node environment')
    same(binding['artifact_sha256'],tree(root,('binding.json','evidence.json')),'Missing/extra/changed complete raw artifact census')


def verified_document(root):
    root=Path(root).resolve(); before=snapshot(); rows=load()
    binding=json.loads((root/'binding.json').read_text()); verify_binding(root,binding)
    references={kind:verify_reference(kind,root/kind,rows,binding['programs']) for kind in ('release','checkout')}
    frames=verify_local(root/'phases',rows,binding)
    summaries=[]; layers={}
    for row in rows:
        identity=row['id']; local=frames[identity]
        keys=[v+'/'+e for v in VARIANTS for e in ('QuickJS','Node')]
        local_ok=list(local)==keys and all(f['status']=='passed' for f in local.values())
        pin_status={kind:references[kind][identity]['status'] for kind in references}
        pin_ok=all(value=='passed' for value in pin_status.values())
        accepted=row['canonical_expected'] is not None and local_ok and pin_ok
        original=accepted and report.same_json(sci.projection(row['original_expected']),sci.projection(row['canonical_expected']))
        summaries.append({'id':identity,'obligation_id':row['obligation_id'],'status':('covered' if original else 'version-difference') if accepted else 'missing',
                          'original_expectation_status':'passed' if original else 'unresolved','original_semantic_pass_credit':1 if original else 0,
                          'canonical_contract_status':'passed' if accepted else 'unresolved','checked_pin_contract_status':pin_status,
                          'actual_macro_contract_status':'passed' if local_ok else 'unresolved'})
        ordinary=[local.get('application-fresh/'+engine,{}) for engine in ('QuickJS','Node')]
        # The ordinary layers retain immutable SCI expectation comparison; the
        # source proof separately owns the verified canonical dialect difference.
        local_layer={'status':'passed' if all(f.get('original_status')=='passed' for f in ordinary) else 'mismatch',
                     'engines':{engine:copy.deepcopy(frame) for engine,frame in zip(('QuickJS','Node'),ordinary)}}
        layers[identity]={'javascript':local_layer}
        for kind in references:
            layer=copy.deepcopy(references[kind][identity])
            if layer.get('observed') is not None:layer['status']=layer['original_status']
            layers[identity][kind]=layer
    same(before,snapshot(),'Current source/producer changed during full replay')
    return {'schema_version':1,'profile':PROFILE,'artifacts_root':str(root),'source_bindings':rows,
            'compiler_build_producer_source_sha256':before,'execution_binding':binding,'references':references,
            'frames':frames,'summaries':summaries,'evidence':layers,
            'original_semantic_pass_credit':sum(r['original_semantic_pass_credit'] for r in summaries)}


def reconcile(document):
    same(document.get('schema_version'),1,'Reader reconciliation schema drift')
    same(document.get('profile'),PROFILE,'Reader reconciliation profile drift')
    same(document.get('source_bindings'),load(),'Original reader source bindings drift')
    same(document.get('compiler_build_producer_source_sha256'),snapshot(),'Current reader producer drift')
    root=document.get('artifacts_root')
    if not isinstance(root,str) or str(Path(root).resolve())!=root:raise ValueError('Missing actual normalized reader artifact root')
    replay=verified_document(root)
    same(document,replay,'Supplied reader document differs from entire current raw/executable replay')
    summaries={r['id']:r for r in replay['summaries']}
    result={}
    for row in replay['source_bindings']:
        summary=summaries[row['id']]; accepted=summary['status']!='missing'
        result[row['obligation_id']]={**copy.deepcopy(summary),'source_binding':copy.deepcopy(row),
            'layers':copy.deepcopy(replay['evidence'][row['id']]),'actual_phase_frames':copy.deepcopy(replay['frames'][row['id']]),
            'reconciliation':('verified-original-source-agreement' if summary['status']=='covered' else 'verified-canonical-sci-difference') if accepted else 'original-contract-unresolved'}
    return result


def run(directory, timeout=600):
    """Normal fresh producer; cached receipts are never a CLI fallback."""
    directory=Path(directory).resolve();directory.mkdir(parents=True,exist_ok=False)
    before=snapshot();rows=load();programs=executable_paths()
    executable_hashes={path:sci.digest(Path(path).read_bytes()) for path in programs.values()}
    phases=directory/'phases';phases.mkdir();(phases/'input.tsv').write_text(input_text(rows))
    build=command([programs['cargo'],'test','--locked','--test',DRIVER.stem,'--no-run','--message-format=json'],phases/'build',timeout)
    artifacts=[json.loads(line) for line in build.stdout.splitlines() if line.startswith('{')]
    binaries=[r['executable'] for r in artifacts if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')==DRIVER.stem and r.get('executable')]
    if len(binaries)!=1:raise ValueError('Missing exact Cargo-built reader audit executable')
    binary=binaries[0];executable_hashes[binary]=sci.digest(Path(binary).read_bytes())
    environment={'LIGHT_SCI_CLJC_ACCEPTED_ROOT':str(phases),'NODE':programs['node']}
    # Record only task-specific environment, retain the actual process environment.
    process_env={**os.environ,**environment}
    audit=command([binary,TEST,'--ignored','--exact','--nocapture'],phases/'audit',min(timeout,300),environment=process_env,record_environment=environment)
    print('Fresh actual local .cljc application/macro audit completed',flush=True)
    for kind in ('release','checkout'):
        produce_reference(kind,directory/kind,rows,programs,timeout)
        print('Fresh '+kind+' whole-original .cljc reader build/execution completed',flush=True)
    same(before,snapshot(),'Source/compiler/producer drift during fresh audit')
    for path,expected in executable_hashes.items():same(sci.digest(Path(path).read_bytes()),expected,'Executable drift during fresh audit')
    binding={'producer_source_sha256':before,'programs':programs,'audit_executable':binary,
             'executables_sha256':executable_hashes,'audit_environment':environment,'artifact_sha256':tree(directory)}
    (directory/'binding.json').write_text(json.dumps(binding,indent=2)+'\n')
    result=verified_document(directory)
    (directory/'evidence.json').write_text(json.dumps(result,indent=2)+'\n')
    reconcile(result)
    return result


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',required=True,type=Path)
    parser.add_argument('--timeout',type=int,default=600)
    args=parser.parse_args()
    result=run(args.output,args.timeout)
    print('Verified',len(result['summaries']),'whole originals;',sum(r['status']!='missing' for r in result['summaries']),'accepted;',sum(r['status']=='missing' for r in result['summaries']),'nonpassing')
