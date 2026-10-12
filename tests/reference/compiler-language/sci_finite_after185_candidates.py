"""Unadapted finite SCI originals selected after the production185 repairs."""
from pathlib import Path
from collections import Counter
import json
import sci_execution as sci
import sci_gap_return_candidates as historical

IDS = ('sci-collection-equality','sci-meta-list-tail','sci-native-equality',
       'sci-native-comparisons','sci-native-ratio-double','sci-native-conversions',
       'sci-native-numeric-predicates','sci-control-case-map','sci-control-case-set',
       'sci-iteration-doseq-zero-generators','sci-iteration-doseq-prefix-let',
       'sci-iteration-dotimes-fraction','sci-iteration-dotimes-ratio',
       'sci-iteration-dotimes-char')
SOURCES = Path(__file__).with_name('sci-finite-after185-sources.json')


def load():
    frozen=json.loads(SOURCES.read_text())
    if tuple(row['sci_id'] for row in frozen)!=IDS:raise ValueError('Finite original cohort changed')
    rows=sci.rows(sci.DISPOSITIONS); originals=sci.rows(sci.CORPUS); result=[]
    for binding in frozen:
        identity=binding['sci_id'];line,fields=rows[identity];original_line,original=originals[identity]
        if (fields[:7]!=original or fields[2]!=fields[8] or fields[1]!='portable' or fields[7]!='gap'
            or sci.exclusion([*fields[:7],'portable',*fields[8:]])
            or binding != {'sci_id':identity,'row_sha256':sci.digest(line),'original_row_sha256':sci.digest(original_line),'source_sha256':sci.digest(fields[2]),'source':fields[2],'original_expected':sci.expected(original)}):
            raise ValueError('Finite original complete source/profile/expectation drift: '+identity)
        result.append({'id':'sci-execution-'+identity,'sci_id':identity,
            'obligation_id':'local::tests/reference/sci/evaluator-dispositions.tsv::'+identity,
            'body_sha256':binding['row_sha256'],'original_row_sha256':binding['original_row_sha256'],
            'original_source_sha256':binding['source_sha256'],'source_sha256':binding['source_sha256'],
            'source':fields[2],'profile':fields[1],'retained_disposition':fields[7],
            'expected':sci.expected(original),'isolated':True,'reference_isolated':True,
            'semantic_pass_credit':False,'macro_phase_status':'not-run',
            'setup':'Unchanged complete portable source; lexical observe callback, captured stdout and empty stderr; no additional reader/namespace/library fixtures.'})
    return result


def snapshot():
    paths=[Path(__file__),SOURCES,Path(__file__).with_name("sci_finite_after185_context.py"),Path(__file__).with_name("sci-finite-after185-contracts.jsonl"),Path(__file__).with_name("test_sci_finite_after185_context.py")]
    return {**historical.snapshot(),**{str(p.relative_to(sci.ROOT)):sci.digest(p.read_bytes()) for p in paths}}


def run(directory,timeout=600):
    directory=Path(directory).resolve();directory.mkdir(parents=True,exist_ok=False)
    before=snapshot();cases=load();evidence={row['id']:{} for row in cases}
    local=sci.local_cases(cases,directory/'local',timeout)
    for identity,layer in local.items():evidence[identity]['javascript']=layer
    print('Local finite sources',dict(Counter(x['status'] for x in local.values())),flush=True)
    for kind in ('release','checkout'):
        layers=sci.reference_cases(cases,kind,sci.ROOT/'research/clojurescript',directory/kind,timeout)
        for identity,layer in layers.items():evidence[identity][kind]=layer
        print(kind,dict(Counter(x['status'] for x in layers.values())),flush=True)
    if before!=snapshot():raise ValueError('Finite source producer changed during audit')
    manifest={str(p.relative_to(directory)):sci.digest(p.read_bytes()) for p in sorted(directory.rglob('*')) if p.is_file()}
    result={'schema_version':1,'scope':'Unchanged finite equality/metadata/numeric/control/iteration SCI originals after185',
            'cases':cases,'producer_sha256':before,'artifact_sha256':manifest,'evidence':evidence,
            'semantic_pass_credit':False,'macro_phase_status':'not-run'}
    (directory/'evidence.json').write_text(json.dumps(result,indent=2)+'\n');return result
