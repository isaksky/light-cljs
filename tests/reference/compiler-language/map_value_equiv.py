"""Exact two-pin map value equivalence contracts and eight actual phase frames."""
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
PROFILE = 'canonical-map-value-equiv-v1'
DRIVER = ROOT/'tests/compiler_map_value_equiv.rs'
TEST = 'map_value_equiv_sources_execute_in_fresh_and_reused_macro_hosts'
SOURCE = ROOT/'tests/fixtures/compiler-map-value-equiv-original.cljc'
CONTRACTS = Path(__file__).with_name('map-value-equiv-contracts.json')
CONTRACTS_SHA256 = '8747934bed39dac7c9bb2dc41c66fc71094436b8f257c06afbf9d5b8fd086569'
IDENTITY = 'runtime-map-traversal-custom-value-equality-effects'


def rows():
    raw = CONTRACTS.read_bytes()
    proof.same(sci.digest(raw),CONTRACTS_SHA256,'Changed independently frozen map value equivalence contracts')
    catalog = json.loads(raw)
    proof.same(catalog['profile'],PROFILE,'Wrong map value equivalence profile')
    proof.same(catalog['comparison'],'exact-typed-packets-no-projection','Map value projection forbidden')
    proof.same(catalog['pins'],{'release':'1.12.134','checkout':'a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e'},'Wrong canonical pins')
    result = catalog['sources']
    proof.same(len(result),32,'Missing complete map value equivalence contracts')
    proof.same(len({r['id'] for r in result}),32,'Duplicate map value equivalence identity')
    original = next(r for r in runtime.load() if r['id']==IDENTITY)
    for key in ('id','source','source_sha256','canonical_expected','original_expected'):
        proof.same(result[0][key],original[key],'Changed historical whole source/expectation')
    proof.same(SOURCE.read_text(),result[0]['source'],'Changed original fixture')
    for row in result:
        proof.same(sci.digest(row['source']),row['source_sha256'],'Changed exact map value equivalence source')
        packet = row['canonical_expected']
        proof.same(set(packet),{'id','outcome','trace','stdout'},'Incomplete map value equivalence packet')
        proof.same(packet['id'],row['id'],'Detached map value equivalence packet')
        proof.same(packet['outcome']['status'],'returned','Failing whole source cannot earn map value equivalence credit')
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
        forms=[text[f.start:f.end] for f in inv.Reader(text).all() if f.kind=='list' and text[f.start:f.end].startswith('(defn equiv-map')]
        proof.same(len(forms),1,'Missing canonical map equivalence declaration')
        array=text[text.index('(deftype PersistentArrayMap ['):]
        array=array[array.index('  IEquiv\n'):array.index('  IHash\n')]
        proof.same(array.count('(-equiv [coll other]'),1,'Missing array-map equivalence implementation')
        result[kind]={'core_source_sha256':sci.digest(data),'declaration':forms[0],'declaration_sha256':sci.digest(forms[0]),
                      'array_map_equivalence':array,'array_map_equivalence_sha256':sci.digest(array)}
    return result


def snapshot():
    paths=(Path(__file__),DRIVER,SOURCE,CONTRACTS,Path(__file__).with_name('test_map_value_equiv.py'))
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
            for row in rows():proof.same(refs[kind][row['id']]['status'],'passed','Source macro differs from independently frozen pin')
        with patch.object(proof,'DRIVER',DRIVER),patch.object(proof,'TEST',TEST):
            frames=proof.verify_local(root/'phases',rows(),binding)
    for row in rows():
        local=frames[row['id']]
        proof.same(list(local),[v+'/'+e for v in proof.VARIANTS for e in ('QuickJS','Node')],'Missing exact eight-frame map value equivalence census')
        for frame in local.values():proof.same(frame['status'],'passed','Whole map value equivalence source differs from canonical packet')
    proof.same(before,snapshot(),'Producer changed during replay')
    return {'schema_version':1,'profile':PROFILE,'artifacts_root':str(root),'execution_binding':binding,'source_bindings':rows(),'declarations':declarations(),'references':refs,'frames':frames,'status':'passed','accepted_source_count':32,'accepted_engine_frame_count':256,'original_semantic_pass_credit':0,'full_language_completion_credit':0}


def reconcile(document):
    proof.same(document['profile'],PROFILE,'Wrong map value equivalence proof profile')
    proof.same(document,verified_document(document['artifacts_root']),'Summary differs from executable/raw proof')


def run(directory):
    root=Path(directory).resolve();root.mkdir(parents=True,exist_ok=False)
    before=snapshot();programs=proof.executable_paths()
    hashes={p:sci.digest(Path(p).read_bytes()) for p in programs.values()}
    for kind in ('release','checkout'):
        proof.produce_reference(kind,root/kind,rows(),programs,240)
        print('Fresh '+kind+' complete map value equivalence sources compiled/executed',flush=True)
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
    print('Verified 64 exact pin packets and 256 actual source engine frames',flush=True)
    return document


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',required=True,type=Path)
    run(parser.parse_args().output)
