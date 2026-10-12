"""Compile the exact retained ordinary literal regression sources in both default pins."""
import argparse,json,re,hashlib
from pathlib import Path
import report
ROOT=report.ROOT
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output',required=True,type=Path)
root=parser.parse_args().output.resolve();root.mkdir(exist_ok=False)
receipt=json.loads(Path(__file__).with_name('literal-execution-historical-before193.json').read_text())
rows=receipt['sources'];rejects=receipt['reject_sources']
text=(ROOT/'tests/execution.rs').read_text()
current=[]
for name in receipt['historical_complete_test_bodies']:
    body=text.split('fn '+name+'() {',1)[1].split('\n#[test]',1)[0]
    current += [(identity,json.loads('"'+source+'"')) for identity,source in re.findall(r'id: "([^" ]+)"\.into\(\),\s*source: "((?:\\.|[^"\\])*)"\.into\(\),',body)]
    if name.startswith('set_reader'):
        assert all(json.dumps(source)[1:-1] in body for source in rejects)
assert current==[(row['id'],row['source']) for row in rows], 'Changed complete original sources'
assert len(rows)==7 and len(rejects)==2
(root/'sources.tsv').write_text(''.join(r['id']+'\t'+r['source']+'\n' for r in rows))
(root/'historical-originals.json').write_text(json.dumps(receipt,indent=2)+'\n')
for kind,identity in [('release',report.RELEASE),('checkout',report.REVISION)]:
    dependency='{:mvn/version "'+report.RELEASE+'"}' if kind=='release' else '{:local/root '+json.dumps(str(ROOT/'research/clojurescript'))+'}'
    base=['clojure','-Srepro','-Sdeps','{:paths [] :deps {org.clojure/clojurescript '+dependency+'}}','-M','tests/reference/execution.clj',kind,identity,'-' if kind=='release' else str(ROOT/'research/clojurescript')]
    output=root/kind
    build=report.invoke([*base,str(output),'cases',str(root/'sources.tsv')],output/'build',timeout=600)
    assert build['status']=='completed', build
    assert build['stdout'].count('REFERENCE IDENTITY '+kind+' '+identity)==2
    assert 'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' in build['stdout']
    script='process.chdir('+json.dumps(str(output))+');require('+json.dumps(str(output/'reference.js'))+');\n'+(ROOT/'tests/support/execution.js').read_text()
    run=report.invoke(['node','-'],output/'node',source=script,timeout=60)
    assert run['status']=='completed',run
    packets=[json.loads(line) for line in run['stdout'].splitlines()]
    assert [r['id'] for r in packets]==[r['id'] for r in rows]
    diagnostics=[]
    for index,source in enumerate(rejects):
        fixture=root/('reject'+str(index)+'.tsv');fixture.write_text('reject'+str(index)+'\t'+source+'\n')
        folder=output/('reject'+str(index))
        result=report.invoke([*base,str(folder),'cases',str(fixture)],folder/'build',timeout=600)
        assert result['status']=='failed' and 'Duplicate key:' in result['stderr'], result
        diagnostics.append({'source':source,'source_sha256':hashlib.sha256(source.encode()).hexdigest(),'status':'reader-rejected','stderr':result['stderr']})
    receipt['pins'][kind]={'identity':identity,'settings':{'optimizations':'none','static-fns':False,'def-emits-var':False},'packets':packets,'diagnostics':diagnostics}
    print(kind,json.dumps(packets),flush=True)
receipt['raw_artifact_sha256']={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(root.rglob('*')) if p.is_file()}
receipt['producer_source_sha256']=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
receipt['producer_path']=str(Path(__file__).resolve())
receipt['artifacts_root']=str(root)
receipt['comparison']='complete-typed-packets-exact-trace-and-stdout'
receipt['acceptance_basis']='Fresh independent unchanged sources in both default pins; existing 176/177/178/179/184 owners.'
(root/'reference-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
assert receipt['pins']['release']['packets']==receipt['pins']['checkout']['packets']
print('All nine unchanged sources verified in both pins',flush=True)
