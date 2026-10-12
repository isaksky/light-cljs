"""Exact declaration value evidence; separate SCI and macro-demand differences."""
from pathlib import Path
import argparse, copy, json, re, subprocess
import sci_execution as sci
import sci_originals_after190_candidates as bindings
import sci_cljc_accepted_context as pin_inputs

BASE = Path(__file__).parent
SOURCES = BASE/'declare-value-sources.json'
SOURCE_DIGEST = '986264d9558b328b6632183faeb2e069de1bdabd0cfb1b8fc86a3966891d3987'
FIXTURE = sci.ROOT/'tests/fixtures/declare-values.tsv'
FIXTURE_DIGEST = 'eecfff7eea455916017544dcb367d1bab38f665612f184e7280f5f3099a486fa'
DIAGNOSTIC = BASE/'sci-declare-value-position-after190-production-gap-diagnostic.json'
FORWARD_MACRO = {'id':'declare191-forward','outcome':{'status':'threw','error':{'category':'error'}},'trace':[],'stdout':''}


def require(value, expected, message):
    if value != expected: raise ValueError(message)


def load():
    require(sci.digest(SOURCES.read_bytes()),SOURCE_DIGEST,'Declaration source/literal mutation')
    require(sci.digest(FIXTURE.read_bytes()),FIXTURE_DIGEST,'Declaration executable fixture mutation')
    require(sci.digest(DIAGNOSTIC.read_bytes()),'c75a3dd09b7278bc5435a1e44f7c3fd3a68616a9d490fddff5fb4054635aa238','Original discovery mutation')
    rows=json.loads(SOURCES.read_text());original=json.loads(DIAGNOSTIC.read_text())['source_binding']
    oldline,old=sci.rows(sci.CORPUS)[original['sci_id']]
    line,fields=sci.rows(sci.DISPOSITIONS)[original['sci_id']]
    require(old,original['original_fields'],'Original seven SCI fields mutation')
    require(fields[:7],old,'SCI disposition original mutation')
    require(sci.digest(oldline),original['original_row_sha256'],'Original SCI row mutation')
    require(sci.digest(line),original['body_sha256'],'SCI disposition row mutation')
    require(rows[0]['source'],original['source'],'Unchanged declaration source mutation')
    require(sci.digest(rows[0]['source']),original['source_sha256'],'Original source hash mutation')
    require(rows[0]['expected']['outcome'],{'status':'returned','value':{'$vector':[False,None,None]}},'Canonical literal mutation')
    require(FIXTURE.read_text(),''.join(r['id']+'\t'+r['source']+'\t'+json.dumps(r['expected'],separators=(',',':'))+'\n' for r in rows),'Executable fixture detached from source')
    return rows,original


def snapshot():
    names=subprocess.check_output(['git','ls-files','src','Cargo.toml','Cargo.lock'],cwd=sci.ROOT,text=True).splitlines()
    names += ['tests/compiler_declare_values.rs','tests/fixtures/declare-values.tsv',
      'tests/support/sci_execution_observation.js','tests/support/execution.js',
      'tests/reference/execution.clj','tests/reference/compiler-language/report.py',
      'tests/reference/compiler-language/sci-analysis.clj','tests/reference/compiler-language/sci_execution.py',
      'tests/reference/compiler-language/declare_values.py','tests/reference/compiler-language/declare-value-sources.json']
    return {p:sci.digest((sci.ROOT/p).read_bytes()) for p in names}


def census(root):
    return {str(p.relative_to(root)):sci.digest(p.read_bytes()) for p in sorted(root.rglob('*')) if p.is_file()}


def tested_executable(log):
    paths=re.findall(r"Running tests/compiler_declare_values.rs \(([^)]+)\)",Path(log).read_text())
    require(len(paths),1,"Missing exact tested declaration executable")
    path=(sci.ROOT/paths[0]).resolve()
    return {"path":str(path),"sha256":sci.digest(path.read_bytes())}


def frames(log,rows):
    text=Path(log).read_text()
    if 'test result: ok. 2 passed; 0 failed; 0 ignored;' not in text:
        raise ValueError('Exact declaration phase/recovery tests did not complete')
    packets={}
    for line in text.splitlines():
        if not line.startswith('declare-value-packet\t'):continue
        _,cycle,phase,session,identity,raw=line.split('\t')
        key=':'.join((cycle,phase,session,identity))
        if key in packets:raise ValueError('Duplicate declaration phase packet')
        packets[key]=json.loads(raw)
    expected={':'.join((str(c),p,s,r['id'])):(FORWARD_MACRO if p=='macro' and r['id']=='declare191-forward' else r['expected'])
      for c in (0,1) for p in ('application','macro') for s in ('fresh','reused') for r in rows}
    require(packets,expected,'Missing/changed exact declaration phase packets')
    return packets


def verify(document):
    rows,original=load()
    require(document['schema_version'],1,'Wrong declaration evidence version')
    require(document['original_semantic_pass_credit'],0,'Original SCI mismatch awarded semantic credit')
    require(document['original_expectation_status'],'canonical-dialect-mismatch','Original SCI dialect mismatch lost')
    require(document['original_source_binding'],original,'Original seven-field source receipt changed')
    require(document['cases'],rows,'Declaration canonical controls changed')
    require(document['producer_sha256'],snapshot(),'Declaration producer/compiler provenance drift')
    require(document['reference_execution_binding'],bindings.reference_execution_binding(),'Pinned executable provenance drift')
    require(document['reference_input_sha256'],pin_inputs.reference_inputs(),'Pinned checkout/release source provenance drift')
    root=Path(document['reference_root'])
    require(document['reference_artifact_sha256'],census(root),'Pinned source/unit/raw artifact drift')
    require(document['test_executable'],tested_executable(document['phase_log']),'Tested declaration executable drift')
    require(document['phase_log_sha256'],sci.digest(Path(document['phase_log']).read_bytes()),'Phase executable raw log drift')
    require(document['phase_packets'],frames(document['phase_log'],rows),'Declaration raw phase packets changed')
    for kind in ('release','checkout'):
        layers=json.loads((root/(kind+'.json')).read_text())
        require(document['references'][kind],layers,'Pinned complete observations changed')
        build=(root/kind/'execution/build/stdout').read_text()
        identity=kind+' '+(sci.report_module().RELEASE if kind=='release' else sci.report_module().REVISION)
        if 'REFERENCE IDENTITY '+identity not in build or 'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in build:
            raise ValueError('Pinned compiler identity/settings missing')
        require((root/kind/'execution/cases.tsv').read_text(),''.join(r['id']+'\t'+(sci.REFERENCE_SETUP+r['source'])+'\n' for r in rows),'Pinned executable complete source drift')
        for index,row in enumerate(rows):
            require(layers[row['id']]['status'],'passed','Pinned declaration failed')
            require(layers[row['id']]['observed'],row['expected'],'Pinned complete typed observation drift')
            raw=(root/kind/'execution'/('node-'+str(index))/'stdout').read_text()
            require(json.loads(raw),row['expected'],'Pinned raw typed packet mutation')
    for kind in ('release','checkout'):
        controls=json.loads((root/'failures'/(kind+'.json')).read_text())
        require(document['reference_failure_controls'][kind],controls,'Canonical failure controls changed')
        for name,category in [('declare191-single-let','javascript-error'),('declare191-invalid','analysis-error')]:
            require(controls[name]['status'],'rejected','Canonical control rejection lost')
            require(controls[name]['observed']['category'],category,'Canonical control phase changed')
    return document


def produce(root,log):
    rows,original=load();root=Path(root).resolve();log=Path(log).resolve()
    document={'schema_version':1,'scope':'One declaration production repair; strict125 and historical SCI fields unchanged',
      'original_semantic_pass_credit':0,'original_expectation_status':'canonical-dialect-mismatch',
      'original_source_binding':original,'cases':rows,'producer_sha256':snapshot(),
      'reference_execution_binding':bindings.reference_execution_binding(),'reference_input_sha256':pin_inputs.reference_inputs(),
      'reference_root':str(root),'reference_artifact_sha256':census(root),
      'references':{kind:json.loads((root/(kind+'.json')).read_text()) for kind in ('release','checkout')},
      'test_executable':tested_executable(log),'phase_log':str(log),'phase_log_sha256':sci.digest(log.read_bytes()),'phase_packets':frames(log,rows),
      'macro_forward_control':'Deliberate unavailable definition cljs.user/y when the uninitialized root is read; canonical application returns undefined. Exact failure and reused-host recovery are asserted.',
      'single_name_let_control':'Both pins independently reject (let [d (declare x)] [(undefined? d) (nil? d)]) as javascript-error; local supports undefined consistently. Original source remains unchanged.',
      'reference_failure_controls':{kind:{name:json.loads((root/'failures'/(kind+'.json')).read_text())[name] for name in ('declare191-single-let','declare191-invalid')} for kind in ('release','checkout')},
      'qualified_names':'Local existing current-namespace ownership accepted; foreign namespace and nonsymbol bindings remain located analysis failures.'}
    return verify(document)


def guards(document):
    mutations=[lambda d:d.update(original_semantic_pass_credit=1),
      lambda d:d['cases'][0].update(source='(declare)'),
      lambda d:d['references']['release']['declare191-original']['observed']['outcome'].update(value=None),
      lambda d:d['phase_packets'].pop(next(iter(d['phase_packets']))),
      lambda d:d['reference_artifact_sha256'].update({'execution.js':'forged'}),
      lambda d:d['producer_sha256'].update({'src/analyzer.rs':'forged'}),
      lambda d:d['original_source_binding']['original_fields'].__setitem__(3,'(return nil)'),
      lambda d:d['reference_input_sha256'].update({'release.jar':'forged'})]
    for mutate in mutations:
        altered=copy.deepcopy(document);mutate(altered)
        try:verify(altered)
        except ValueError:continue
        raise ValueError('Declaration provenance mutation was accepted')
    return len(mutations)


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--reference-root',required=True);parser.add_argument('--phase-log',required=True);parser.add_argument('--output',required=True)
    args=parser.parse_args();document=produce(args.reference_root,args.phase_log)
    document['rejected_mutations']=guards(document)
    Path(args.output).write_text(json.dumps(document,indent=2)+'\n')
    print('Eight both-pin complete controls; 64 exact phase/session packets; eight fail-closed guards; original SCI semantic credit 0')
