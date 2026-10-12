"""Diagnostic-only original collections; no completion or source credit."""
import sys,json
from pathlib import Path
sys.path.insert(0,'/Users/admin/src/light-cljs/tests/reference/compiler-language')
import sci_execution as sci
def original_expected(fields):
 outcome=sci.wire(fields[3])
 if outcome[0]=='return':return sci.expected(fields)
 assert len(outcome)==3 and outcome[0]=='error' and outcome[1] in {'execution','analysis','read'}
 effects=sci.value(sci.wire(fields[4]));assert set(effects)=={'$vector'} and sci.value(sci.wire(fields[6]))==''
 return {'id':'sci-execution-'+fields[0],'outcome':{'status':'sci-error','phase':outcome[1],'category':outcome[2]},'trace':effects['$vector'],'stdout':sci.value(sci.wire(fields[5]))}
from concurrent.futures import ThreadPoolExecutor
ids=('sci-coll-count-invalid', 'sci-coll-empty-p-invalid', 'sci-coll-keys-invalid', 'sci-coll-vals-invalid', 'sci-coll-select-keys-invalid', 'sci-coll-merge-invalid', 'sci-coll-merge-with-invalid', 'sci-coll-peek-invalid', 'sci-coll-pop-invalid', 'sci-coll-conj-vector-sequence-invalid', 'sci-coll-conj-entry-length-invalid', 'sci-coll-call-vector-arity', 'sci-coll-call-set-arity', 'sci-coll-call-keyword-arity', 'sci-coll-call-map-arity', 'sci-coll-call-symbol-arity', 'sci-coll-call-vector-type', 'sci-coll-call-vector-bounds', 'sci-coll-nth-bounds', 'sci-coll-nth-type', 'sci-coll-assoc-bounds', 'sci-coll-assoc-type', 'sci-coll-pop-vector-empty', 'sci-coll-pop-list-empty')
root=Path('/tmp/light-cljs-125-original-sci-after191-collection-discovery-v3').resolve();root.mkdir(exist_ok=False)
originals=sci.rows(sci.CORPUS);dispositions=sci.rows(sci.DISPOSITIONS);rows=[]
for identity in ids:
 line,f=dispositions[identity];ol,o=originals[identity]
 assert f[:7]==o and f[2]==f[8] and f[1]=='portable' and f[7]=='gap'
 rows.append(dict(id='sci-execution-'+identity,sci_id=identity,obligation_id='local::tests/reference/sci/evaluator-dispositions.tsv::'+identity,body_sha256=sci.digest(line),original_row_sha256=sci.digest(ol),source_sha256=sci.digest(f[2]),original_source_sha256=sci.digest(f[2]),source=f[2],original_fields=o,expected=original_expected(o),isolated=True,reference_isolated=True))
(root/'sources.json').write_text(json.dumps(rows,indent=2)+'\n')
evidence={r['id']:{} for r in rows}
local=sci.local_cases(rows,root/'local',300)
for i,l in local.items():evidence[i]['javascript']=l
print('Local completed',flush=True)
with ThreadPoolExecutor(max_workers=2) as pool:
 futures={kind:pool.submit(sci.reference_cases,rows,kind,sci.ROOT/'research/clojurescript',root/kind,600) for kind in ('release','checkout')}
 for kind,f in futures.items():
  for i,l in f.result().items():evidence[i][kind]=l
  print(kind+' completed',flush=True)
(root/'evidence.json').write_text(json.dumps(dict(cases=rows,evidence=evidence),indent=2)+'\n')
for r in rows:
 print(r['sci_id'])
 for k,l in evidence[r['id']].items():print(k,l.get('status'),l.get('observed'),l.get('diagnostic'))
