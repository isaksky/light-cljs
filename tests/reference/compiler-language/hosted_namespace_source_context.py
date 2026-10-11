#!/usr/bin/env python3
"""Audited complete goog3/REPL2 bodies through two exact source-provider profiles.

The goog provider compiles a delegation to the genuine library function. The
REPL provider retains the exact original Error->map source/dependency slice.
These profiles establish no arbitrary Closure, cljs.repl or compiler API parity.
"""
from __future__ import annotations
import argparse, json, os, shutil, zipfile
from pathlib import Path
import report, extract_inventory as inventory
import configured_source_context as transport
import hosted_import_source_context as imports
ROOT=report.ROOT
DRIVER=ROOT/'tests/compiler_hosted_namespace_source_context.rs'
TEST='original_namespace_bodies_execute_in_fresh_and_reused_hosts'
VARIANTS=transport.VARIANTS
digest=transport.digest
GOOG_PROVIDER='(ns goog)\n(defn isArrayLike [x] (js/goog.isArrayLike x))\n'
GOOG_PROVIDER_HASH='841b39e2d568bb869e306b6b5b8cbc70152eca072bad7af9407f10d7336dfe16'
REPL_MODULE='src/main/cljs/cljs/repl.cljs'
REPL_MODULE_HASH='69a5ec0a352a9b05dc9617d38d466d190d2d526501688e08ad247a08a965e49d'
REPL_FUNCTION_HASH='1f215cafe3ea242fccf28eba27dceececc62fdc1c4e63950748c71157d775e44'
REPL_PROVIDER_HASH='482b472d9ac598097e909d77ecf787dd1abe06b03728d55f6d3aea7985490749'
SPECS=(
 ('goog','cljs-hosted-original-goog-context','cljs.ns-test','src/test/cljs/cljs/ns_test.cljs',
  '21243db745474306fdfb034dd99b4ba5cee1a858493e4e8182fc2542968d1f1b',
  'c763df6378d5773a664ce2dfbc8e933b8b7ed96bcf38ce883014c6e5b6262a0d',
  ['src/test/cljs/cljs/ns_test.cljs::test-cljs-1677'],
  ['af46c1e105d03a7bdd7dc8d2bb87a5c6c45bf8e734c7804c3c134d0ad87636a6'],
  '(ns cljs.ns-test (:require [goog :as goog-alias]))','goog.cljs',3),
 ('repl','cljs-hosted-original-repl-context','cljs.repl-test','src/test/cljs/cljs/repl_test.cljs',
  '1bb588aa598ab3b5d9cf1d4367e87af1c352d0c65eeb20a07c1c5359446d05b3',
  'e9aa9c9b9a303d3eddd6bb1fbd3ed9fda031b99aeafccfcbc66b21d03468d4f9',
  ['src/test/cljs/cljs/repl_test.cljs::test-cljs-3017','src/test/cljs/cljs/repl_test.cljs::test-cljs-3019'],
  ['1cc0a036141bae786f333b70e3ffb2eaad297f1c806e306b8aaafa57e773bb31','f1baa0acdf9a01935cacb63095e2ac0af6f165c06cac57dbc2ee9cfa082dffea'],
  '(ns cljs.repl-test (:require [cljs.repl]))','cljs/repl.cljs',2))

def repl_function(text):
    forms=[form for form in inventory.Reader(text).all() if inventory.short_head(form)=='defn' and form.children[1].token=='Error->map']
    if len(forms)!=1:raise ValueError('missing/duplicate original Error->map function')
    text=text[forms[0].start:forms[0].end]
    if digest(text)!=REPL_FUNCTION_HASH:raise ValueError('original Error->map function drift')
    return text

def load():
    bodies={row['id']:row for row in map(json.loads,(ROOT/'tests/reference/compiler-language/full-bodies.jsonl').read_text().splitlines())}
    library=(ROOT/'research/clojurescript'/REPL_MODULE).read_text()
    if digest(library)!=REPL_MODULE_HASH:raise ValueError('original REPL module drift')
    provider='(ns cljs.repl)\n'+repl_function(library)+'\n'
    rows=[]
    for label,identity,entry,module,module_hash,source_hash,body_ids,body_hashes,header,path,count in SPECS:
        text=(ROOT/'research/clojurescript'/module).read_text();ns=inventory.Reader(text).all()[0]
        if digest(text)!=module_hash:raise ValueError('original caller module drift')
        parts=[];assertions=[]
        for body_id,body_hash in zip(body_ids,body_hashes):
            body=bodies[body_id]['source_span']['text']
            if digest(body)!=body_hash:raise ValueError('complete original caller body drift')
            form=inventory.Reader(body).all()[0]
            parts.append('(do '+' '.join(body[c.start:c.end].replace('(is ','(assert ') for c in form.children[2:])+' true)')
            def walk(node):
                if inventory.short_head(node)=='is':assertions.append(digest(body[node.start:node.end]))
                for child in node.children:walk(child)
            walk(form)
        source=text[:ns.start]+header+'\n\n(defn run [] (do '+' '.join(parts)+' true))\n'
        if len(assertions)!=count:raise ValueError('missing/reordered original caller assertions')
        row={'id':identity,'entry':entry,'export':'run','source':source,'expected':{'id':identity,'outcome':{'status':'returned','value':True},'trace':[],'stdout':''},'native_stdout':None,
         'hosted_namespace_profile':label,'original_module_sha256':module_hash,'original_complete_body_sha256':dict(zip(body_ids,body_hashes)),
         'original_assertion_sha256':assertions,'provider_path':path,'provider_source':GOOG_PROVIDER if label=='goog' else provider,'provider_source_sha256':GOOG_PROVIDER_HASH if label=='goog' else REPL_PROVIDER_HASH,
         'dependency_source_sha256':{'goog/base.js':imports.DEPENDENCIES['base.js']} if label=='goog' else {REPL_MODULE:REPL_MODULE_HASH,'Error->map':REPL_FUNCTION_HASH}}
        if digest(source)!=source_hash or digest(row['provider_source'])!=row['provider_source_sha256']:raise ValueError('reviewed source/provider drift')
        rows.append(row)
    return rows

def validate(rows):
    expected=load()
    if len(rows)!=2 or not report.same_json(rows,expected):raise ValueError('missing/extra/reordered/changed audited namespace profile')
    return rows

def snapshot(rows):
    validate(rows);paths=[p for p in (ROOT/'src').rglob('*') if p.is_file()]
    paths += [ROOT/n for n in ('Cargo.toml','Cargo.lock','build.rs') if (ROOT/n).is_file()]
    paths += [DRIVER,Path(__file__).resolve(),Path(__file__).with_name('test_hosted_namespace_source_context.py'),Path(transport.__file__),Path(imports.__file__),Path(report.__file__),Path(inventory.__file__),ROOT/'tests/support/sci_execution_observation.js',ROOT/'tests/support/execution.js',ROOT/'tests/reference/execution.clj',ROOT/'tests/reference/compiler-language/full-bodies.jsonl',ROOT/'research/clojurescript'/REPL_MODULE]
    paths += [ROOT/'research/clojurescript'/spec[3] for spec in SPECS]
    return {str(p.relative_to(ROOT)):digest(p.read_bytes()) for p in sorted(set(paths))}

def goog_loader(artifacts):
    base=artifacts/'out/goog/base.js';data=base.read_text()
    if digest(data)!=imports.DEPENDENCIES['base.js']:raise ValueError('genuine Closure base drift')
    origin=imports.library_origin(artifacts/'out/goog')
    # The original body uses only the predicate provided in base.js. Real base
    # initialization needs no additional namespace; its deps autoload is disabled.
    loader='(function(){globalThis.CLOSURE_NO_DEPS=true;(0,eval)('+json.dumps(data)+');return true;})()'
    return loader,{'goog/base.js':digest(data),'raw_library_archive':origin['archive_sha256']},origin

def repl_origin(kind,artifacts):
    if kind=='checkout':
        text=(ROOT/'research/clojurescript'/REPL_MODULE).read_text();origin={'module_sha256':digest(text),'pin':report.REVISION}
    else:
        archive=Path.home()/'.m2/repository/org/clojure/clojurescript'/report.RELEASE/('clojurescript-'+report.RELEASE+'.jar')
        with zipfile.ZipFile(archive) as jar:text=jar.read('cljs/repl.cljs').decode()
        origin={'module_sha256':digest(text),'archive_sha256':digest(archive.read_bytes()),'archive':str(archive),'pin':report.RELEASE}
    function=repl_function(text)
    return {'original_function_sha256':digest(function),'module_origin':origin,'reference_compiled_module_sha256':digest((artifacts/'out/cljs/repl.js').read_bytes())}

def macro_source(source,loader):
    prefix,body=source.split('\n(defn run [] ',1)
    if loader=='true':return prefix+'\n(defmacro upstream [] '+body+'\n(defn run [] (upstream))\n'
    return prefix+'\n(defmacro upstream [] (do (js* '+json.dumps(loader)+') '+body.rstrip().removesuffix(')')+'))\n(defn run [] (upstream))\n'

def parse_raw(text,rows,loaders):
    raw=[line.split('\t') for line in text.splitlines()];identities=[(row['id'],variant) for row in rows for variant in VARIANTS]
    if len(raw)!=8 or any(len(line)!=6 for line in raw) or [(line[0],line[1]) for line in raw]!=identities:raise ValueError('missing/extra/partial/reordered complete namespace observations')
    parsed=[]
    for fields,(identity,variant) in zip(raw,identities):
        row=next(row for row in rows if row['id']==identity);source,js,frame=[bytes.fromhex(x).decode() for x in fields[2:5]];warmup=fields[5]
        if source!=(macro_source(row['source'],loaders[identity]) if variant.startswith('macro') else row['source']):raise ValueError('changed/reduced original namespace phase source')
        if js.count(' = // Original bounded runtime')!=1:raise ValueError('runtime observation anchor drift')
        if (warmup=='-')!=variant.endswith('fresh') or (variant.endswith('reused') and not bytes.fromhex(warmup)):raise ValueError('missing prior actual reused-host macro execution')
        observed=report.observations(frame,[row])[identity]
        if observed['status']!='passed':raise ValueError('invalid/mismatching raw QuickJS namespace frame')
        parsed.append((row,variant,source,js,frame,warmup,observed))
    return parsed

def execute(rows,directory,timeout=240):
    rows=validate(rows);directory=Path(directory).resolve()
    if directory.exists():raise ValueError('fresh artifacts required')
    before=snapshot(rows);directory.mkdir(parents=True);cases=[]
    for row in rows:
        root=directory/'original-source'/row['id'];file=root/(row['entry'].replace('.','/').replace('-','_')+'.cljs');file.parent.mkdir(parents=True);file.write_text(row['source']);cases.append({**row,'source_root':str(root),'reference_files_sha256':{str(file.relative_to(root)):digest(row['source'])}})
    references={};loaders={};dependencies={};origins={}
    for kind in ('release','checkout'):
        references[kind]=report.reference_cases(cases,kind,ROOT/'research/clojurescript',directory/kind,timeout)
        for row in rows:
            identity=row['id'];reference=references[kind][identity];artifacts=directory/kind/identity
            if reference['status']!='passed':raise ValueError('complete original namespace pinned reference failed')
            if row['hosted_namespace_profile']=='goog':
                loader,dependency,origin=goog_loader(artifacts)
                if identity in loaders and (loader!=loaders[identity] or dependency!=dependencies[identity] or origin!=origins[identity]):raise ValueError('pin genuine goog profile differs')
                loaders[identity]=loader;dependencies[identity]=dependency;origins[identity]=origin
            else:
                loaders[identity]='true';dependencies[identity]=row['dependency_source_sha256'];origins.setdefault(identity,{})[kind]=repl_origin(kind,artifacts)
            reference.update(source_sha256=digest(row['source']),provider_source_sha256=row['provider_source_sha256'],raw_observation_sha256=digest((artifacts/'node/stdout').read_bytes()),build_stdout_sha256=digest((artifacts/'build/stdout').read_bytes()),reference_javascript_sha256=digest((artifacts/'reference.js').read_bytes()))
        print(kind,'original goog3/REPL2 passed',flush=True)
    request=directory/'input.tsv';output=directory/'observations.tsv';request.write_text(''.join(row['id']+'\t'+row['entry']+'\t'+row['source'].encode().hex()+'\t'+macro_source(row['source'],loaders[row['id']]).encode().hex()+'\t'+row['provider_path']+'\t'+row['provider_source'].encode().hex()+'\t'+loaders[row['id']].encode().hex()+'\n' for row in rows))
    build,_=transport.run(['cargo','test','--locked','--test','compiler_hosted_namespace_source_context','--no-run','--message-format=json'],directory/'build',timeout)
    binaries=[Path(record['executable']) for record in map(json.loads,build.splitlines()) if record.get('reason')=='compiler-artifact' and record.get('target',{}).get('name')=='compiler_hosted_namespace_source_context' and record.get('executable')]
    if len(binaries)!=1:raise ValueError('missing/duplicate actual audit executable')
    binary=binaries[0];binary_hash=digest(binary.read_bytes());node=Path(shutil.which(os.environ.get('NODE','node')) or '').resolve()
    if not node.is_file():raise ValueError('Node unavailable')
    node_hash=digest(node.read_bytes());env={**os.environ,'LIGHT_CLJS_HOSTED_NAMESPACE_INPUT':str(request),'LIGHT_CLJS_HOSTED_NAMESPACE_OUTPUT':str(output),'LIGHT_CLJS_HOSTED_NAMESPACE_ROOT':str(directory/'phase-sources')}
    stdout,_=transport.run([str(binary),TEST,'--ignored','--exact','--nocapture'],directory/'audit',timeout,environment=env)
    if 'running 1 test' not in stdout or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in stdout:raise ValueError('actual exact namespace audit not executed')
    layers={row['id']:{} for row in rows}
    for row,variant,source,js,frame,warmup,quickjs in parse_raw(output.read_text(),rows,loaders):
        identity=row['id'];target=directory/identity/variant;target.mkdir(parents=True);(target/'source.cljs').write_text(source);(target/'compiled.js').write_text(js);(target/'quickjs.json').write_text(frame+'\n');(target/'provider.cljs').write_text(row['provider_source'])
        captured=js.replace(' = // Original bounded runtime',' = globalThis.lightRuntime = // Original bounded runtime',1);loader=loaders[identity]
        script='globalThis.lightInspectValue = value => globalThis.lightRuntime?.inspect(value) ?? null;\nglobalThis.lightCases = [observe => { '+loader+'; const run = '+captured+'; return run(observe); }];\nglobalThis.lightCaseMetadata = '+json.dumps([[identity,row['source']]])+';\n'+transport.NODE_OBSERVER.read_text();(target/'node-script.js').write_text(script)
        raw_node,stderr=transport.run([str(node),'-'],target/'node',timeout,source=script);node_layer=report.observations(raw_node,[row])[identity];frames=stderr.splitlines()
        if node_layer['status']!='passed' or len(frames)!=2 or not frames[0].startswith('CASE START ') or not frames[1].startswith('CASE END ') or json.loads(frames[0][11:]).get('id')!=identity or json.loads(frames[1][9:]).get('id')!=identity:raise ValueError('invalid raw Node outcome/stderr identity')
        provenance={'source_sha256':digest(row['source']),'phase_source_sha256':digest(source),'compiled_js_sha256':digest(js),'original_module_sha256':row['original_module_sha256'],'original_complete_body_sha256':row['original_complete_body_sha256'],'original_assertion_sha256':row['original_assertion_sha256'],'provider_source_sha256':row['provider_source_sha256'],'dependency_source_sha256':dependencies[identity],'dependency_pin_origins':origins[identity],'closure_loader_sha256':digest(loader),'compiler_build_producer_source_sha256':before,'audit_executable_sha256':binary_hash,'node_executable_sha256':node_hash,'build_stdout_sha256':digest(build),'warmup_compiled_js_sha256':None if warmup=='-' else digest(bytes.fromhex(warmup))}
        for engine,layer,raw_frame in [('QuickJS',quickjs,frame),('Node',node_layer,raw_node)]:layers[identity][variant+'/'+engine]={**layer,**provenance,'raw_observation_sha256':digest(raw_frame)}
    transport.unchanged(before,snapshot(rows))
    for kind in ('release','checkout'):
        for row in rows:
            identity=row['id'];artifacts=directory/kind/identity
            if row['hosted_namespace_profile']=='goog':
                loader,dependency,origin=goog_loader(artifacts)
                if loader!=loaders[identity] or dependency!=dependencies[identity] or origin!=origins[identity]:raise ValueError('dependency origin/build/source drift')
            elif repl_origin(kind,artifacts)!=origins[identity][kind]:raise ValueError('original REPL dependency origin/build/source drift')
    if digest(binary.read_bytes())!=binary_hash or digest(node.read_bytes())!=node_hash:raise ValueError('execution binary drift')
    if any(list(layer)!=[v+'/'+e for v in VARIANTS for e in ('QuickJS','Node')] for layer in layers.values()):raise ValueError('missing raw eight-frame profile evidence')
    result={'status':'passed','cases':rows,'references':references,'observations':layers,'prospective_original_assertion_credit':{'goog':3,'repl':2},'generic_library_or_default_goog_parity':'not claimed'}
    (directory/'evidence.json').write_text(json.dumps(result,indent=2)+'\n');return result

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--timeout',type=int,default=240);args=parser.parse_args();execute(load(),args.output,args.timeout);print('sixteen actual namespace observations passed',flush=True)
if __name__=='__main__':main()
