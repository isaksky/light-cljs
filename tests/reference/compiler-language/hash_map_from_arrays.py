"""Exact two-pin public static factory contracts and eight actual phase frames."""
from pathlib import Path
from unittest.mock import patch
import argparse
import json
import os
import zipfile
import extract_inventory as inv
import runtime_map_traversal as runtime

proof = runtime.proof
sci = runtime.sci
ROOT = sci.ROOT
PROFILE = 'canonical-hash-map-from-arrays-v1'
DRIVER = ROOT/'tests/compiler_hash_map_from_arrays.rs'
TEST = 'hash_map_from_arrays_sources_execute_in_fresh_and_reused_macro_hosts'
SOURCE = ROOT/'tests/fixtures/compiler-hash-map-from-arrays-original.cljc'
CONTRACTS = Path(__file__).with_name('hash-map-from-arrays-contracts.json')
CONTRACTS_SHA256 = '355e6bc6ef9f80d7ae16e9866d85eba0e7f6f43b38931beeebd3987304efc342'
IDENTITY = 'runtime-map-traversal-public-from-arrays'


def rows():
    raw = CONTRACTS.read_bytes()
    proof.same(sci.digest(raw),CONTRACTS_SHA256,'Changed independently frozen factory contracts')
    catalog = json.loads(raw)
    proof.same(catalog['profile'],PROFILE,'Wrong factory profile')
    proof.same(catalog['comparison'],'exact-typed-packets-no-projection','Factory projection forbidden')
    proof.same(catalog['pins'],{'release':'1.12.134','checkout':'a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e'},'Wrong canonical pins')
    result = catalog['sources']
    proof.same(len(result),12,'Missing complete factory contracts')
    proof.same(len({r['id'] for r in result}),12,'Duplicate factory identity')
    original = next(r for r in runtime.load() if r['id']==IDENTITY)
    for key in ('id','source','source_sha256','canonical_expected','original_expected'):
        proof.same(result[0][key],original[key],'Changed historical whole source/expectation')
    proof.same(SOURCE.read_text(),result[0]['source'],'Changed original fixture')
    for row in result:
        proof.same(sci.digest(row['source']),row['source_sha256'],'Changed exact factory source')
        packet = row['canonical_expected']
        proof.same(set(packet),{'id','outcome','trace','stdout'},'Incomplete factory packet')
        proof.same(packet['id'],row['id'],'Detached factory packet')
        proof.same(packet['outcome']['status'],'returned','Failing whole source cannot earn factory credit')
        proof.same(row['original_expected'],packet,'Changed reference expectation')
    return result


def declarations():
    result = {}
    for kind in ('release','checkout'):
        if kind == 'release':
            jar = Path.home()/'.m2/repository/org/clojure/clojurescript/1.12.134/clojurescript-1.12.134.jar'
            with zipfile.ZipFile(jar) as z:data=z.read('cljs/core.cljs')
        else:data=(ROOT/'research/clojurescript/src/main/cljs/cljs/core.cljs').read_bytes()
        text=data.decode()
        forms=[text[f.start:f.end] for f in inv.Reader(text).all() if f.kind=='list' and text[f.start:f.end].startswith('(set! (.-fromArrays PersistentHashMap)')]
        proof.same(len(forms),1,'Missing canonical static factory declaration')
        result[kind]={'core_source_sha256':sci.digest(data),'declaration':forms[0],'declaration_sha256':sci.digest(forms[0])}
    return result


def snapshot():
    paths=(Path(__file__),DRIVER,SOURCE,CONTRACTS,Path(__file__).with_name('test_hash_map_from_arrays.py'))
    return {**runtime.snapshot(),**{str(p.relative_to(ROOT)):sci.digest(p.read_bytes()) for p in paths}}


def verified_document(directory):
    root=Path(directory).resolve()
    before=snapshot()
    binding=json.loads((root/'binding.json').read_text())
    proof.same(binding['producer_source_sha256'],before,'Producer/compiler drift')
    proof.same(binding['programs'],proof.executable_paths(),'Executable path drift')
    proof.same(set(binding['executables_sha256']),set(binding['programs'].values())|{binding['audit_executable']},'Missing executable hashes')
    for path,digest in binding['executables_sha256'].items():proof.same(sci.digest(Path(path).read_bytes()),digest,'Executable drift')
    proof.same(binding['audit_environment'],{'LIGHT_SCI_CLJC_ACCEPTED_ROOT':str(root/'phases'),'NODE':binding['programs']['node']},'Audit environment drift')
    proof.same(binding['artifact_sha256'],proof.tree(root,('binding.json','evidence.json')),'Raw artifact census/hash drift')
    refs={}
    with patch.object(proof,'observation',runtime.exact_observation):
        for kind in ('release','checkout'):
            refs[kind]=proof.verify_reference(kind,root/kind,rows(),binding['programs'])
            for row in rows():proof.same(refs[kind][row['id']]['status'],'passed','Factory differs from independently frozen pin')
        with patch.object(proof,'DRIVER',DRIVER),patch.object(proof,'TEST',TEST):
            frames=proof.verify_local(root/'phases',rows(),binding)
    for row in rows():
        local=frames[row['id']]
        proof.same(list(local),[v+'/'+e for v in proof.VARIANTS for e in ('QuickJS','Node')],'Missing exact eight-frame factory census')
        for frame in local.values():proof.same(frame['status'],'passed','Whole factory source differs from canonical packet')
    proof.same(before,snapshot(),'Producer changed during replay')
    return {'schema_version':1,'profile':PROFILE,'artifacts_root':str(root),'execution_binding':binding,'source_bindings':rows(),'declarations':declarations(),'references':refs,'frames':frames,'status':'passed','accepted_source_count':12,'accepted_engine_frame_count':96,'original_semantic_pass_credit':0,'full_language_completion_credit':0}


def reconcile(document):
    proof.same(document['profile'],PROFILE,'Wrong factory proof profile')
    proof.same(document,verified_document(document['artifacts_root']),'Summary differs from executable/raw proof')


def run(directory):
    root=Path(directory).resolve();root.mkdir(parents=True,exist_ok=False)
    before=snapshot();programs=proof.executable_paths()
    hashes={p:sci.digest(Path(p).read_bytes()) for p in programs.values()}
    for kind in ('release','checkout'):
        proof.produce_reference(kind,root/kind,rows(),programs,240)
        print('Fresh '+kind+' complete factory sources compiled/executed',flush=True)
    phases=root/'phases';phases.mkdir()
    (phases/'input.tsv').write_text(proof.input_text(rows()))
    build=proof.command([programs['cargo'],'test','--locked','--test',DRIVER.stem,'--no-run','--message-format=json'],phases/'build',180)
    binaries=[r['executable'] for r in map(json.loads,build.stdout.splitlines()) if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')==DRIVER.stem and r.get('executable')]
    proof.same(len(binaries),1,'Missing actual Cargo audit executable')
    binary=binaries[0];hashes[binary]=sci.digest(Path(binary).read_bytes())
    env={'LIGHT_SCI_CLJC_ACCEPTED_ROOT':str(phases),'NODE':programs['node']}
    proof.command([binary,TEST,'--ignored','--exact','--nocapture'],phases/'audit',180,environment={**os.environ,**env},record_environment=env)
    proof.same(before,snapshot(),'Producer changed during fresh proof')
    binding={'producer_source_sha256':before,'programs':programs,'executables_sha256':hashes,'audit_executable':binary,'audit_environment':env,'artifact_sha256':proof.tree(root)}
    (root/'binding.json').write_text(json.dumps(binding,indent=2)+'\n')
    document=verified_document(root)
    (root/'evidence.json').write_text(json.dumps(document,indent=2)+'\n')
    reconcile(document)
    print('Verified 24 exact pin packets and 96 actual factory engine frames',flush=True)
    return document


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',required=True,type=Path)
    run(parser.parse_args().output)
