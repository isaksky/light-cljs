"""Exact historical reader sources with .cljc filename / :cljs selection.

Diagnostic evidence only. Historical SCI expectations stay immutable; rejected,
malformed active or unsupported-domain forms never become semantic passes.
"""
from pathlib import Path
import argparse, collections, json, os
import report
import sci_execution as sci
import sci_source_context as context
ROOT=report.ROOT
DRIVER=ROOT/'tests/compiler_sci_cljc_source_probe.rs'
ORACLE=Path(__file__).with_suffix('.clj')
TEST='original_cljc_sources_execute_in_fresh_and_reused_macro_hosts'
VARIANTS=('application-fresh','macro-fresh','application-reused','macro-reused')
IDS=('default','discard-no-match-child','discard-quote-no-match','discard-splice','feature-clj','map-splice','metadata-no-match','nested','nil','no-match','no-match-eof','quote-no-match','quote-no-match-child','skipped-arguments','skipped-regex-bigint','skipped-regex-identity','skipped-tag','skipped-tag-identity','splice-empty','splice-list','splice-map','splice-map-entries','splice-map-key','splice-metadata','splice-nested','splice-no-reflatten','splice-quote','splice-scalars','splice-set','splice-vector')

FROZEN=Path(__file__).with_name('sci-cljc-source-probe-catalog.json')
FROZEN_SHA256='3b8cce3f332306327f27c13ffb3f4a81cee67aab9ade6eee0f53aefbf09c25f0'

def generated_catalog():
    corpus=sci.rows(sci.CORPUS);dispositions=sci.rows(sci.DISPOSITIONS);result=[]
    for suffix in IDS:
        identity='sci-context-reader-'+suffix
        original_line,original=corpus[identity];body_line,body=dispositions[identity]
        if body[:7]!=original or sci.wire(original[3])[0]!='return':raise ValueError('original reader row/profile drift')
        result.append({'id':'sci-execution-'+identity,'sci_id':identity,'source':original[2],
            'source_sha256':sci.digest(original[2]),'original_row_sha256':sci.digest(original_line),
            'body_sha256':sci.digest(body_line),'expected':sci.expected(original),
            'source_name':'sci-original.cljc','reader_feature':'cljs',
            'obligation_id':'local::tests/reference/sci/evaluator-dispositions.tsv::'+identity})
    return result

def frozen():
    raw=FROZEN.read_bytes()
    if sci.digest(raw)!=FROZEN_SHA256:raise ValueError('unreviewed source/canonical literal catalog')
    return json.loads(raw)

def catalog():
    original=frozen()['catalog']
    if generated_catalog()!=original:raise ValueError('original source/body/row/expectation changed')
    return original

def classify(rows,observations,references):
    reviewed={r['id']:r for r in frozen()['contracts']}
    gaps={r['id']:r for r in frozen()['focused_gap_contracts']}
    result=[]
    for row in rows:
        identity=row['id'];contract=reviewed.get(identity) or gaps.get(identity)
        expected=contract['canonical_expected'] if contract else None
        pins=[references[k][identity] for k in ('release','checkout')]
        pin_agreement=expected is not None and all(sci.verify({'expected':expected},p,report.same_json)['status']=='passed' for p in pins)
        variants=observations[identity]
        frames=[v[e] for v in variants.values() if v.get('status')!='rejected' for e in ('QuickJS','Node')]
        complete=len(variants)==4 and len(frames)==8 and pin_agreement and all(sci.verify({'expected':expected},f,report.same_json)['status']=='passed' for f in frames)
        original=complete and all(f['status']=='passed' for f in frames+pins)
        gap=identity in gaps and pin_agreement and not complete
        result.append({'id':identity,'complete_eight_local_phase_frames_and_both_pins':complete,'original_agreement':original,'canonical_expected':expected,'classification':'original-agreement' if original else 'canonical-source-feature-difference' if complete else 'confirmed-public-reader-gap' if gap else 'unresolved','original_semantic_pass_credit':0})
    return result

def source(original,phase):
    if phase=='macro':return "(ns user)\n(defmacro upstream [] (list 'quote (do "+original+")))\n(defn run [observe] (upstream))\nrun\n"
    return '(ns user)\n(defn run [observe] (do '+original+'))\nrun\n'

def snapshot():
    files=[Path(__file__),ORACLE,DRIVER,FROZEN,sci.CORPUS,sci.DISPOSITIONS,
           ROOT/'tests/reference/execution.clj',ROOT/'tests/support/execution.js',
           ROOT/'tests/support/sci_execution_observation.js']
    return {**context.compiler_sources(),**{str(p.relative_to(ROOT)):sci.digest(p.read_bytes()) for p in files}}

def local(rows,directory):
    directory.mkdir();(directory/'input.tsv').write_text(''.join(r['id']+'\t'+r['source'].encode().hex()+'\n' for r in rows))
    ran=report.invoke(['env','LIGHT_SCI_CLJC_PROBE_ROOT='+str(directory),'cargo','test','--locked','--test',DRIVER.stem,TEST,'--','--ignored','--exact','--nocapture'],directory/'audit',timeout=240)
    if ran['status']!='completed' or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in ran.get('stdout',''):raise ValueError('actual .cljc phase audit failed: '+str(directory))
    raw=[line.split('\t') for line in (directory/'observations.tsv').read_text().splitlines()]
    byid={r['id']:r for r in rows};result={r['id']:{} for r in rows}
    for fields in raw:
        identity,variant,engine,encoded=fields[:4]
        if identity not in byid or variant not in VARIANTS:raise ValueError('unknown original phase identity')
        retained=bytes.fromhex(encoded).decode()
        if retained!=source(byid[identity]['source'],variant.split('-')[0]):raise ValueError('reduced/rewritten original source')
        if engine=='rejected':
            if len(fields)!=5:raise ValueError('invalid rejection')
            result[identity][variant]={'status':'rejected','detail':bytes.fromhex(fields[4]).decode(),'phase_source_sha256':sci.digest(retained)}
        else:
            if engine not in ('QuickJS','Node') or len(fields)!=7:raise ValueError('invalid engine frame')
            js,observed=[bytes.fromhex(v).decode() for v in fields[4:6]]
            if (fields[6]=='-')!=variant.endswith('fresh'):raise ValueError('missing reused host')
            layer={'status':'observation-error','detail':observed} if observed.startswith('HARNESS_ERROR:') else sci.verify(byid[identity],{'status':'completed','observed':json.loads(observed)},report.same_json)
            layer.update(phase_source_sha256=sci.digest(retained),compiled_js_sha256=sci.digest(js),raw_observation_sha256=sci.digest(observed),warmup_sha256=None if fields[6]=='-' else sci.digest(bytes.fromhex(fields[6])))
            if engine in result[identity].setdefault(variant,{}):raise ValueError('duplicate phase frame')
            result[identity][variant][engine]=layer
    for variants in result.values():
        if list(variants)!=list(VARIANTS):raise ValueError('incomplete/reordered phase variants')
        for v in variants.values():
            if v.get('status')!='rejected' and list(v)!=['QuickJS','Node']:raise ValueError('incomplete engine pair')
    return result

def reference(rows,kind,directory):
    directory.mkdir();inputfile=directory/'input.tsv';inputfile.write_text(''.join(r['id']+'\t'+r['source'].encode().hex()+'\n' for r in rows))
    identity=report.RELEASE if kind=='release' else report.REVISION
    checkout=ROOT/'research/clojurescript'
    dependency='{:mvn/version "'+report.RELEASE+'"}' if kind=='release' else '{:local/root '+json.dumps(str(checkout))+'}'
    built=report.invoke(['clojure','-Srepro','-Sdeps','{:paths [] :deps {org.clojure/clojurescript '+dependency+'}}','-M',str(ORACLE),kind,identity,'-' if kind=='release' else str(checkout),str(directory),str(inputfile)],directory/'build',timeout=240)
    if built['status']!='completed' or built.get('stdout','').count('REFERENCE IDENTITY '+kind+' '+identity)!=2 or 'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in built['stdout']:raise ValueError('unverified .cljc reference: '+str(directory))
    records=[line.split('\t') for line in (directory/'contexts.tsv').read_text().splitlines()]
    if [r[0] for r in records]!=[r['id'] for r in rows]:raise ValueError('reference rows changed')
    accepted=[i for i,r in enumerate(records) if r[1]=='compiled'];selected=[rows[i] for i in accepted]
    result={}
    for index,row in zip(accepted,selected):
        script='process.chdir('+json.dumps(str(directory))+');require('+json.dumps(str(directory/'reference.js'))+');\n'+(directory/'contexts.js').read_text()+'\nglobalThis.lightCases=[lightCases['+str(index)+']];globalThis.lightCaseMetadata=[lightCaseMetadata['+str(index)+']];\n'+(ROOT/'tests/support/execution.js').read_text()
        ran=report.invoke(['node','-'],directory/'node'/row['id'],source=script,timeout=30)
        if ran['status']!='completed':
            result[row['id']]={'status':'observation-error','detail':ran};continue
        frames=ran.get('stderr','').splitlines()
        if len(frames)!=2 or not frames[0].startswith('CASE START ') or not frames[1].startswith('CASE END ') or any(json.loads(f.split(' ',2)[2]).get('id')!=row['id'] for f in frames):raise ValueError('reference stderr contamination')
        layer=report.observations(ran['stdout'],[row])[row['id']]
        result[row['id']]=sci.verify(row,layer,report.same_json)
    for row,rec in zip(rows,records):
        if rec[1]=='rejected':result[row['id']]={'status':'rejected','detail':rec[2:]}
        result[row['id']].update(reference_identity=kind+' '+identity,settings={'optimizations':'none','static-fns':False,'def-emits-var':False},original_source_sha256=row['source_sha256'],source_name='sci-original.cljc',oracle_sha256=sci.digest(ORACLE.read_bytes()))
    return result

def execute(directory):
    directory=Path(directory).resolve();directory.mkdir(parents=True,exist_ok=False)
    rows=catalog();before=snapshot()
    observations=local(rows,directory/'local');print('local 30 original sources / actual phase audit finished',flush=True)
    references={}
    for kind in ('release','checkout'):
        references[kind]=reference(rows,kind,directory/kind)
        print(kind,dict(collections.Counter(r['status'] for r in references[kind].values())),flush=True)
    if snapshot()!=before:raise ValueError('source/driver/compiler drift during audit')
    reconciled=classify(rows,observations,references)
    result={'schema_version':1,'catalog':rows,'local':observations,'references':references,'reconciliation':reconciled,'source_producer_sha256':before,'original_semantic_pass_credit':0,'shared_ledger_integration':'not performed'}
    (directory/'evidence.json').write_text(json.dumps(result,indent=2)+'\n')
    print(dict(collections.Counter(r['classification'] for r in reconciled)),flush=True)
    return result
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',required=True,type=Path);a=p.parse_args();execute(a.output)
