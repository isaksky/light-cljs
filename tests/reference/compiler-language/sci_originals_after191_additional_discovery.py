import sys,json
from pathlib import Path
sys.path.insert(0,'/Users/admin/src/light-cljs/tests/reference/compiler-language')
import sci_execution as sci
from sci_originals_after190_candidates import original_expected
from concurrent.futures import ThreadPoolExecutor
ids=('sci-var-unbound-predicates', 'sci-var-no-init-redefinition', 'sci-var-conditional-definition', 'sci-var-root-reference', 'sci-var-unbound-self-root', 'sci-var-unbound-copy-root', 'sci-var-unbound-self-defonce', 'sci-vars-duplicate-redefs', 'sci-vars-lexical-shadow', 'sci-vars-nil-false', 'sci-vars-def-bound', 'sci-vars-defonce-bound', 'sci-protocol-same-shape', 'sci-protocol-zero-receiver', 'sci-syntax-quote-core-unquote', 'sci-syntax-quote-core-macros', 'sci-syntax-quote-core', 'sci-xf-bounds-empty-invalid')
root=Path('/tmp/light-cljs-125-original-sci-after191-additional-discovery-v1').resolve();root.mkdir(exist_ok=False)
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
