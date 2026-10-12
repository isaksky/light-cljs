"""Fifteen unchanged complete original SCI sources with explicit canonical contrasts; no current credit from history."""
from pathlib import Path
from collections import Counter
import json,os,shutil
import sci_execution as sci
import sci_gap_return_candidates as historical
import sci_cljc_accepted_context as pin_inputs

IDS=('sci-seq-reduce-callback-error', 'sci-seq-mapv-error', 'sci-seq-filterv-error', 'sci-seq-juxt-error', 'sci-coll-nth-range', 'sci-xf-bounds-underflow-take', 'sci-xf-bounds-underflow-drop', 'sci-xf-bounds-underflow-drop-after-forward', 'sci-exception-accessors', 'sci-binding-defn-attributes', 'sci-binding-defn-symbol-meta', 'sci-binding-defn-trailing-meta', 'sci-record-numeric-key', 'sci-var-definition-meta-order', 'sci-binding-defn-meta-effects')
SOURCES=Path(__file__).with_name('sci-originals-after190-sources.json')
SOURCES_SHA256='f81d4169ae88acd452180ed4fb4c7a8ae98deb998593687017c252f8fa2ebd33'


def original_expected(fields):
    outcome=sci.wire(fields[3])
    if outcome[0]=='return':return sci.expected(fields)
    if (len(outcome)!=3 or outcome[0]!='error' or outcome[1]!='execution'
        or outcome[2] not in {'arithmetic','type'}):raise ValueError('Changed original SCI structured error')
    effects=sci.value(sci.wire(fields[4]))
    if set(effects)!={'$vector'} or sci.value(sci.wire(fields[6]))!='':raise ValueError('Changed original SCI effects/stderr')
    return {'id':'sci-execution-'+fields[0],
            'outcome':{'status':'sci-error','phase':outcome[1],'category':outcome[2]},
            'trace':effects['$vector'],'stdout':sci.value(sci.wire(fields[5]))}


def load():
    if sci.digest(SOURCES.read_bytes())!=SOURCES_SHA256:raise ValueError('Unreviewed numeric error source table')
    frozen=json.loads(SOURCES.read_text());originals=sci.rows(sci.CORPUS);dispositions=sci.rows(sci.DISPOSITIONS)
    if tuple(r['sci_id'] for r in frozen)!=IDS:raise ValueError('Numeric error source cohort changed')
    result=[]
    for row in frozen:
        identity=row['sci_id'];line,fields=dispositions[identity];original_line,original=originals[identity]
        expected={'id':'sci-execution-'+identity,'sci_id':identity,
          'obligation_id':'local::tests/reference/sci/evaluator-dispositions.tsv::'+identity,
          'body_sha256':sci.digest(line),'original_row_sha256':sci.digest(original_line),
          'source_sha256':sci.digest(fields[2]),'original_source_sha256':sci.digest(fields[2]),
          'source':fields[2],'original_fields':original,'expected':original_expected(original),
          'isolated':True,'reference_isolated':True}
        if (row!=expected or fields[:7]!=original or fields[2]!=fields[8]
            or fields[1]!='portable' or fields[7]!='gap' or fields[5:7]!=['(string u)','(string u)']):
            raise ValueError('Complete numeric original source/profile/expectation drift: '+identity)
        result.append(row)
    return result


def snapshot():
    paths=[Path(__file__),SOURCES,Path(pin_inputs.__file__),
           Path(__file__).with_name('sci_originals_after190.py'),
           Path(__file__).with_name('sci-originals-after190-contracts.json'),
           Path(__file__).with_name('test_sci_originals_after190.py')]
    return {**historical.snapshot(),**pin_inputs.reference_inputs(),
            **{str(p.relative_to(sci.ROOT)):sci.digest(p.read_bytes()) for p in paths}}


def reference_execution_binding():
    programs=pin_inputs.executable_paths()
    for name in ('node','clojure'):
        effective=Path(shutil.which(os.environ.get(name.upper(),name)) or '').resolve()
        if str(effective)!=programs[name]:raise ValueError('Effective reference command differs from bound executable: '+name)
    return {'programs':programs,'executables_sha256':{path:sci.digest(Path(path).read_bytes()) for path in programs.values()}}


def run(directory,timeout=600):
    directory=Path(directory).resolve();directory.mkdir(parents=True,exist_ok=False)
    before=snapshot();program_binding=reference_execution_binding();cases=load();evidence={r['id']:{} for r in cases}
    local=sci.local_cases(cases,directory/'local',min(timeout,300))
    for identity,layer in local.items():evidence[identity]['javascript']=layer
    print('Fresh local numeric originals '+str(dict(Counter(r['status'] for r in local.values()))),flush=True)
    for kind in ('release','checkout'):
        layers=sci.reference_cases(cases,kind,sci.ROOT/'research/clojurescript',directory/kind,timeout)
        for identity,layer in layers.items():evidence[identity][kind]=layer
        print('Fresh '+kind+' numeric originals '+str(dict(Counter(r['status'] for r in layers.values()))),flush=True)
    if before!=snapshot():raise ValueError('Numeric source producer/compiler changed during execution')
    if program_binding!=reference_execution_binding():raise ValueError('Actual numeric reference executable changed during execution')
    manifest={str(p.relative_to(directory)):sci.digest(p.read_bytes()) for p in sorted(directory.rglob('*')) if p.is_file()}
    result={'schema_version':1,'scope':'Fifteen unchanged full SCI sequence/index/transducer/exception/definition metadata/record originals',
            'cases':cases,'producer_sha256':before,'artifact_sha256':manifest,'evidence':evidence,
            'reference_execution_binding':program_binding,
            'semantic_pass_credit':False,'macro_phase_status':'not-run'}
    (directory/'evidence.json').write_text(json.dumps(result,indent=2)+'\n');return result
