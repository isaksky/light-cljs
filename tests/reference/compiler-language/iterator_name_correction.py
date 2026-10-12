"""Replay the unchanged iterator-seq source: invalid canonical capability premise."""
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
PROFILE = 'invalid-canonical-iterator-name-premise-v1'
DRIVER = ROOT/'tests/compiler_iterator_name.rs'
TEST = 'iterator_name_original_rejects_in_fresh_and_reused_macro_hosts'
SOURCE = ROOT/'tests/fixtures/compiler-iterator-name-original.cljc'
IDENTITY = 'runtime-map-traversal-iterators'


def rows():
    row = next(r for r in runtime.load() if r['id'] == IDENTITY)
    proof.same(SOURCE.read_text(), row['source'], 'Changed complete original source')
    proof.same(row['canonical_expected'], {'id':IDENTITY,'outcome':{'status':'threw','error':{'category':'type-error'}},'trace':[],'stdout':''}, 'Converted thrown original into returned credit')
    proof.same(row['two_pin_agreement'], True, 'Unclassified pin disagreement')
    return [row]


def declarations():
    result = {}
    for kind in ('release','checkout'):
        if kind == 'release':
            jar = Path.home()/'.m2/repository/org/clojure/clojurescript/1.12.134/clojurescript-1.12.134.jar'
            with zipfile.ZipFile(jar) as z:
                data = z.read('cljs/core.cljs')
        else:
            data = (ROOT/'research/clojurescript/src/main/cljs/cljs/core.cljs').read_bytes()
        text = data.decode()
        public = [f for f in inv.Reader(text).all() if f.kind == 'list' and len(f.children)>1 and f.children[0].token in {'defn','defn-','def','deftype'}]
        names = [f.children[1].token for f in public]
        proof.same('iterator-seq' in names, False, 'New canonical declaration requires reassessment')
        helper = [text[f.start:f.end] for f in public if f.children[1].token == 'es6-iterator-seq']
        proof.same(len(helper),1,'Missing canonical ES6 helper declaration')
        result[kind] = {'core_source_sha256':sci.digest(data),'iterator_seq_declared':False,'canonical_helper':helper[0],'canonical_helper_sha256':sci.digest(helper[0])}
    return result


def snapshot():
    return {**runtime.snapshot(),**{str(p.relative_to(ROOT)):sci.digest(p.read_bytes()) for p in (Path(__file__),DRIVER,SOURCE,Path(__file__).with_name('test_iterator_name_correction.py'))}}


def verified_document(directory):
    root = Path(directory).resolve()
    before = snapshot()
    binding = json.loads((root/'binding.json').read_text())
    proof.same(binding['producer_source_sha256'],before,'Producer/compiler drift')
    proof.same(binding['programs'],proof.executable_paths(),'Executable path drift')
    proof.same(set(binding['executables_sha256']),set(binding['programs'].values())|{binding['audit_executable']},'Missing executable hashes')
    for path,digest in binding['executables_sha256'].items():
        proof.same(sci.digest(Path(path).read_bytes()),digest,'Executable drift')
    proof.same(binding['audit_environment'],{'LIGHT_SCI_CLJC_ACCEPTED_ROOT':str(root/'phases'),'NODE':binding['programs']['node']},'Audit environment drift')
    proof.same(binding['artifact_sha256'],proof.tree(root,('binding.json','evidence.json')),'Raw artifact census/hash drift')
    refs = {}
    with patch.object(proof,'observation',runtime.exact_observation):
        for kind in ('release','checkout'):
            refs[kind] = proof.verify_reference(kind,root/kind,rows(),binding['programs'])
            observed = refs[kind][IDENTITY]
            proof.same(observed['status'],'passed','Pin observation differs from frozen thrown packet')
            proof.same(observed['observed']['outcome']['status'],'threw','Thrown source cannot earn returned credit')
            warnings = (root/kind/'build/stderr').read_text().splitlines()
            expected_warnings = ['WARNING: Use of undeclared Var user/iterator-seq at line 1 sci-original.cljc']*4
            expected_warnings += ['WARNING: user is a single segment namespace at line 1 '+str(root/kind/'src/user.cljs')]
            proof.same(warnings,expected_warnings,'Missing exact undeclared-name evidence')
    with patch.object(proof,'DRIVER',DRIVER),patch.object(proof,'TEST',TEST):
        frames = proof.verify_local(root/'phases',rows(),binding)
    local = frames[IDENTITY]
    proof.same(list(local),[v+'/rejected' for v in proof.VARIANTS],'Missing exact four source rejection contexts')
    for frame in local.values():
        if 'unresolved symbol: iterator-seq' not in frame['detail']:
            raise ValueError('Wrong source rejection category')
    proof.same(before,snapshot(),'Producer changed during replay')
    return {'schema_version':1,'profile':PROFILE,'artifacts_root':str(root),'execution_binding':binding,'source_bindings':rows(),'declarations':declarations(),'references':refs,'frames':frames,'resolution':'invalid-not-applicable','original_semantic_pass_credit':0,'full_language_completion_credit':0}


def reconcile(document):
    proof.same(document['profile'],PROFILE,'Wrong correction profile')
    proof.same(document,verified_document(document['artifacts_root']),'Summary differs from raw/executable proof')


def run(directory):
    root = Path(directory).resolve()
    root.mkdir(parents=True,exist_ok=False)
    before = snapshot()
    programs = proof.executable_paths()
    hashes = {p:sci.digest(Path(p).read_bytes()) for p in programs.values()}
    for kind in ('release','checkout'):
        proof.produce_reference(kind,root/kind,rows(),programs,180)
        print('Fresh '+kind+' unchanged complete source compiled/executed',flush=True)
    phases = root/'phases'
    phases.mkdir()
    (phases/'input.tsv').write_text(proof.input_text(rows()))
    build = proof.command([programs['cargo'],'test','--locked','--test',DRIVER.stem,'--no-run','--message-format=json'],phases/'build',180)
    binaries = [r['executable'] for r in map(json.loads,build.stdout.splitlines()) if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')==DRIVER.stem and r.get('executable')]
    proof.same(len(binaries),1,'Missing exact Cargo audit executable')
    binary = binaries[0]
    hashes[binary] = sci.digest(Path(binary).read_bytes())
    env = {'LIGHT_SCI_CLJC_ACCEPTED_ROOT':str(phases),'NODE':programs['node']}
    proof.command([binary,TEST,'--ignored','--exact','--nocapture'],phases/'audit',120,environment={**os.environ,**env},record_environment=env)
    proof.same(before,snapshot(),'Producer changed during fresh proof')
    binding = {'producer_source_sha256':before,'programs':programs,'executables_sha256':hashes,'audit_executable':binary,'audit_environment':env,'artifact_sha256':proof.tree(root)}
    (root/'binding.json').write_text(json.dumps(binding,indent=2)+'\n')
    document = verified_document(root)
    (root/'evidence.json').write_text(json.dumps(document,indent=2)+'\n')
    reconcile(document)
    print('Invalid premise confirmed: two exact thrown pin packets, four local rejections, zero semantic credit',flush=True)
    return document


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',required=True,type=Path)
    run(parser.parse_args().output)
