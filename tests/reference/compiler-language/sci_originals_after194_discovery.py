#!/usr/bin/env python3
"""Disjoint full original protocol/record/reify/extension source diagnostics.

This discovers all remaining portable gap sources in these complete families,
without a cohort cap. Exact originalsevenfields stay bound; no semantic credit,
source rewriting, source projection, skips-as-pass or disposition mutation.
"""
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
from collections import Counter
import argparse,json,os,subprocess,sys
import sci_execution as sci
import sci_gap_canonical_context as phase
import sci_originals_after191_candidates as prior

FAMILIES=('sci-protocol-','sci-reify-','sci-record-','sci-extension-')
SOURCES=Path(__file__).with_name('sci-originals-after194-discovery-sources.json')


def occupied():
    result=set();inputs={}
    for path in sorted(sci.HERE.glob('*')):
        if path.name.endswith(('sources.json','catalog.jsonl')) and path!=SOURCES:
            try:
                data=json.loads(path.read_text()) if path.suffix=='.json' else [json.loads(line) for line in path.read_text().splitlines()]
                if isinstance(data,list):
                    ids={row['sci_id'] for row in data if isinstance(row,dict) and 'sci_id' in row}
                    if ids:result.update(ids);inputs[str(path.relative_to(sci.ROOT))]=sci.digest(path.read_bytes())
            except json.JSONDecodeError:raise ValueError('Cannot check previous source cohort '+str(path))
    return result,inputs


def original_expected(fields):
    outcome=sci.wire(fields[3])
    if outcome[0]=='return':return sci.expected(fields)
    if len(outcome)!=3 or outcome[0]!='error':raise ValueError('Unreviewed original structured error')
    effects=sci.value(sci.wire(fields[4]));stderr=sci.value(sci.wire(fields[6]))
    if set(effects)!={'$vector'} or stderr!='':raise ValueError('Unreviewed original stdout/effect/stderr contract')
    return {'id':'sci-execution-'+fields[0],'outcome':{'status':'sci-error','phase':outcome[1],'category':outcome[2]},'trace':effects['$vector'],'stdout':sci.value(sci.wire(fields[5]))}


def candidates():
    existing,_=occupied();originals=sci.rows(sci.CORPUS);rows=[]
    for identity,(line,fields) in sci.rows(sci.DISPOSITIONS).items():
        if fields[1]!='portable' or fields[7]!='gap' or not identity.startswith(FAMILIES) or identity in existing:continue
        original_line,original=originals[identity]
        if fields[:7]!=original:raise ValueError('Original seven fields/source modified '+identity)
        rows.append({'id':'sci-execution-'+identity,'sci_id':identity,'obligation_id':'local::tests/reference/sci/evaluator-dispositions.tsv::'+identity,
          'body_sha256':sci.digest(line),'original_row_sha256':sci.digest(original_line),'source_sha256':sci.digest(fields[2]),
          'original_source_sha256':sci.digest(original[2]),'source':original[2],'original_fields':original,'expected':original_expected(original),
          'isolated':True,'reference_isolated':True})
    return rows


def load():
    frozen=json.loads(SOURCES.read_text());fresh=candidates()
    if frozen!=fresh:raise ValueError('Exact whole-source original disjoint cohort changed')
    return fresh


def snapshot():
    paths=set(subprocess.check_output(['git','ls-files','src','build.rs','Cargo.toml','Cargo.lock'],cwd=sci.ROOT,text=True).splitlines())
    paths.update([str(Path(__file__).relative_to(sci.ROOT)),str(SOURCES.relative_to(sci.ROOT)),
        str(sci.CORPUS.relative_to(sci.ROOT)),str(sci.DISPOSITIONS.relative_to(sci.ROOT)),sci.TEST_FILE,
        'tests/compiler_sci_gap_phase_probe.rs','tests/support/sci_execution_observation.js',
        'tests/reference/compiler-language/sci_execution.py','tests/reference/compiler-language/sci-analysis.clj',
        'tests/reference/compiler-language/sci_gap_canonical_context.py','tests/reference/execution.clj'])
    return {**{name:sci.digest((sci.ROOT/name).read_bytes()) for name in sorted(paths)},**occupied()[1]}


def execute(root):
    root=Path(root).resolve();root.mkdir(parents=True,exist_ok=False)
    before=snapshot();rows=load();binding=prior.reference_execution_binding();evidence={r['id']:{} for r in rows}
    (root/'source-bindings.json').write_text(json.dumps(rows,indent=2)+'\n')
    local=sci.local_cases(rows,root/'local',600)
    for identity,layer in local.items():evidence[identity]['javascript']=layer
    print('Whole original local source diagnostic '+str(dict(Counter(layer['status'] for layer in local.values()))),flush=True)
    with ThreadPoolExecutor(max_workers=2) as pool:
        futures={kind:pool.submit(sci.reference_cases,rows,kind,sci.ROOT/'research/clojurescript',root/kind,600) for kind in ('release','checkout')}
        for kind,future in futures.items():
            layers=future.result()
            for identity,layer in layers.items():evidence[identity][kind]=layer
            print('Whole original '+kind+' diagnostic '+str(dict(Counter(layer['status'] for layer in layers.values()))),flush=True)
    if before!=snapshot() or binding!=prior.reference_execution_binding():raise ValueError('Whole original discovery source/compiler/executable changed')
    receipt={'schema_version':1,'scope':'All remaining disjoint portable gap original protocol/reify/record/extension sources; ordinary diagnostic only',
      'source_cohort':rows,'prior_cohort_inputs':occupied()[1],'source_snapshot':before,'reference_execution_binding':binding,
      'evidence':evidence,'original_semantic_pass_credit':0,'macro_phase_status':'not-run','strict_complete':False,
      'artifact_sha256':{str(p.relative_to(root)):sci.digest(p.read_bytes()) for p in sorted(root.rglob('*')) if p.is_file()}}
    (root/'evidence.json').write_text(json.dumps(receipt,indent=2)+'\n')
    return receipt

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--freeze',action='store_true');parser.add_argument('--execute',type=Path);args=parser.parse_args()
    if args.freeze:
        if SOURCES.exists():raise ValueError('Never overwrite frozen source cohort')
        SOURCES.write_text(json.dumps(candidates(),indent=2)+'\n')
    if args.execute:execute(args.execute)
