#!/usr/bin/env python3
"""Six original import assertions with the real, exact reviewed Closure profile.

This is an explicit embedding of genuine library bytes, not Closure build parity,
JVM macro imports, arbitrary goog namespace support or fake JS constructors.
"""
from __future__ import annotations
import argparse, ast, hashlib, json, os, re, shutil, subprocess, zipfile
from pathlib import Path
import report, extract_inventory as inventory
import configured_source_context as transport
ROOT = report.ROOT
DRIVER = ROOT / 'tests/compiler_hosted_import_source_context.rs'
TEST = 'original_import_body_executes_in_fresh_and_reused_hosts'
ID = 'cljs-hosted-original-import-context'
MODULE = 'src/test/cljs/cljs/import_test.cljs'
MODULE_HASH = '194d7c04722e6a8a4ea750cc4b70144d37628a0ded6111750f55d97964669f94'
BODY_HASH = '3fa2d1eb9c79510bc82847a301a967b9ed0d5c2c520ae29d165a9b19570ec812'
SOURCE_HASH = '721cc1453ee7a160378f61cedb759ee0b177a388b9905ca815c4a47ba1ba9c8f'
VARIANTS = transport.VARIANTS
EXPECTED = {'id':ID,'outcome':{'status':'returned','value':True},'trace':[],'stdout':''}
DEPENDENCIES = {
 'base.js':'5f112f51631fb5b92ef39fabf1427f5c4a4b1f93d3fbf8c799e5cb9486e5c1fc',
 'deps.js':'ccf78ff0c40ec9765f4bf7d052ffc75051ecedd66b22951c6eaad22945fc0606',
 'debug/error.js':'749ac0c7d09cef8a6f6e523fb5150f6a52aabc031b820b903d647821b5cd49ed',
 'dom/nodetype.js':'b5cc73fda1b5028390b464d063a49a8b597c08718dd305fa260afa694704eaae',
 'asserts/asserts.js':'d73a31c49c426b7c491bd443b6683044896ff6837df7a45063bf3281fd1d3869',
 'reflect/reflect.js':'15912d9ee23634e6d21a0321563a0b2faf25e5e55746ce25b9289891534328b5',
 'math/long.js':'9d74e2e306b5d5a0828911dce2653857702dd5ceecf641acded6eb49c2963153',
 'math/math.js':'43eff984882740275ba9277d1eb613ae84f28cae028dd3ddc499646b631da47a',
 'math/coordinate.js':'80eb59b3766531eaf58f952300a60abe4c2adec842c36336c01b5623e1bf33f1',
 'math/vec2.js':'b9eb5c3dfd8115916f1f1411393866dce8b8e79afe5a3098f828a06c2a11b46d',
 'math/coordinate3.js':'ef5de4467663a36432e71b3bc968b6a34b86a1638cd1d9dd87fb1fce7e3b9582',
 'math/vec3.js':'d2c026a351f6e83431064b867cd5c6e315d8202662110e8ffbb1c130e0418fb3',
 'math/integer.js':'d4c01e0a2dfbbad292768445b109ac683b436394f9f3984b3550da96ddb7b28a'}
digest = transport.digest

def load():
    original = (ROOT/'research/clojurescript'/MODULE).read_text()
    if digest(original)!=MODULE_HASH: raise ValueError('original import module drift')
    forms = inventory.Reader(original).all()
    if len(forms)!=2 or inventory.short_head(forms[0])!='ns' or inventory.short_head(forms[1])!='deftest': raise ValueError('original whole source shape drift')
    ns,body=forms
    if digest(original[body.start:body.end])!=BODY_HASH or len(body.children[2:])!=6: raise ValueError('complete original import body drift')
    clause=ns.children[3]
    if inventory.short_head(clause)!=':import': raise ValueError('original imports changed')
    assertions=[original[c.start:c.end] for c in body.children[2:]]
    if any(inventory.short_head(c)!='is' for c in body.children[2:]): raise ValueError('original assertion harness changed')
    program=original[:ns.start]+'(ns cljs.import-test '+original[clause.start:clause.end]+')\n\n(defn run [] (do\n  '+'\n  '.join(a.replace('(is ','(assert ',1) for a in assertions)+'\n  true))\n'
    return validate({'id':ID,'entry':'cljs.import-test','export':'run','source':program,'expected':EXPECTED,'native_stdout':None,
      'hosted_import_profile':'genuine-closure-20250515-f04e4c0e-original-import-six',
      'original_module_sha256':MODULE_HASH,'original_body_sha256':BODY_HASH,
      'original_assertion_sha256':[digest(a) for a in assertions],'closure_dependency_sha256':DEPENDENCIES})

def validate(case):
    if (case.get('id')!=ID or case.get('entry')!='cljs.import-test' or case.get('export')!='run'
        or digest(case.get('source',''))!=SOURCE_HASH or not report.same_json(case.get('expected'),EXPECTED)
        or case.get('hosted_import_profile')!='genuine-closure-20250515-f04e4c0e-original-import-six'
        or case.get('original_module_sha256')!=MODULE_HASH or case.get('original_body_sha256')!=BODY_HASH
        or case.get('closure_dependency_sha256')!=DEPENDENCIES): raise ValueError('unreviewed full source/library profile')
    original=(ROOT/'research/clojurescript'/MODULE).read_text()
    if digest(original)!=MODULE_HASH: raise ValueError('original module drift')
    body=inventory.Reader(original).all()[1]
    if case.get('original_assertion_sha256')!=[digest(original[c.start:c.end]) for c in body.children[2:]]: raise ValueError('missing/reduced/reordered original assertions')
    return case

def snapshot(case):
    validate(case)
    paths=[p for p in (ROOT/'src').rglob('*') if p.is_file()]
    paths += [ROOT/n for n in ('Cargo.toml','Cargo.lock','build.rs') if (ROOT/n).is_file()]
    paths += [DRIVER,Path(__file__).resolve(),Path(__file__).with_name('test_hosted_import_source_context.py'),Path(transport.__file__),Path(report.__file__),Path(inventory.__file__),ROOT/'tests/support/sci_execution_observation.js',ROOT/'tests/support/execution.js',ROOT/'tests/reference/execution.clj',ROOT/'research/clojurescript'/MODULE]
    return {str(p.relative_to(ROOT)):digest(p.read_bytes()) for p in sorted(set(paths))}

def dependency_bundle(goog):
    hashes={p:digest((goog/p).read_bytes()) for p in DEPENDENCIES}
    if hashes!=DEPENDENCIES: raise ValueError('genuine Closure dependency bytes drift')
    graph={}
    for line in (goog/'deps.js').read_text().splitlines():
        match=re.fullmatch(r"goog.addDependency\('([^']+)', (\[[^\]]*\]), (\[[^\]]*\]), (.*)\);",line)
        if match:
            path,provides,requires,flags=match.groups()
            for provide in ast.literal_eval(provides):
                if provide in graph: raise ValueError('duplicate genuine Closure namespace')
                graph[provide]=(path,ast.literal_eval(requires))
    ordered=[];visiting=set();seen=set()
    def visit(name):
        if name not in graph: raise ValueError('missing real transitive Closure namespace')
        path,requires=graph[name]
        if path in visiting: raise ValueError('cyclic audited dependency graph')
        if path in seen:return
        visiting.add(path)
        for require in requires:visit(require)
        visiting.remove(path);seen.add(path);ordered.append(path)
    for name in ('goog.math.Long','goog.math.Vec2','goog.math.Vec3','goog.math.Integer'):visit(name)
    if ordered!=[p for p in DEPENDENCIES if p not in ('base.js','deps.js')]: raise ValueError('missing/extra/reordered genuine Closure dependency closure')
    # The compiler output has already wrapped goog.module files. Evaluate these
    # exact bytes once; wrapping goog.loadModule a second time is rejected.
    bundle='globalThis.CLOSURE_NO_DEPS = true;\n(0,eval)('+json.dumps((goog/'base.js').read_text())+');\n'
    for path in ordered:bundle+='(0,eval)('+json.dumps((goog/path).read_text())+');\n'
    return '(function(){'+bundle+'return true;})()',hashes

def library_origin(goog):
    # Identify the genuine Maven source archive by the exact base/deps bytes;
    # retain every raw source dependency alongside compiler-produced JS hashes.
    matches=[]
    for archive in sorted((Path.home()/'.m2/repository/org/clojure/google-closure-library').rglob('*.jar')):
        with zipfile.ZipFile(archive) as jar:
            if digest(jar.read('goog/base.js'))==DEPENDENCIES['base.js'] and digest(jar.read('goog/deps.js'))==DEPENDENCIES['deps.js']:
                matches.append({'archive':str(archive),'archive_sha256':digest(archive.read_bytes()),'raw_source_dependency_sha256':{p:digest(jar.read('goog/'+p)) for p in DEPENDENCIES}})
    if len(matches)!=1: raise ValueError('missing/ambiguous genuine Closure source archive')
    return matches[0]

def macro_source(source,loader):
    prefix,body=source.split('\n(defn run [] ',1)
    return prefix+'\n(defmacro upstream [] (do (js* '+json.dumps(loader)+') '+body.rstrip().removesuffix(')')+'))\n(defn run [] (upstream))\n'

def parse_raw(text,case,loader):
    rows=[line.split('\t') for line in text.splitlines()]
    if len(rows)!=4 or any(len(row)!=5 for row in rows) or [row[0] for row in rows]!=list(VARIANTS): raise ValueError('missing/extra/partial/reordered original import observations')
    parsed=[]
    for variant,source_hex,js_hex,raw_hex,warmup in rows:
        source,js,raw=[bytes.fromhex(x).decode() for x in (source_hex,js_hex,raw_hex)]
        if source!=(macro_source(case['source'],loader) if variant.startswith('macro') else case['source']): raise ValueError('reduced/replaced original import phase source')
        if js.count(' = // Original bounded runtime')!=1: raise ValueError('runtime observation anchor drift')
        if (warmup=='-')!=variant.endswith('fresh') or (variant.endswith('reused') and not bytes.fromhex(warmup)): raise ValueError('missing actual reused-host warmup')
        observed=report.observations(raw,[case])[ID]
        if observed['status']!='passed': raise ValueError('invalid/mismatching QuickJS raw original import frame')
        parsed.append((variant,source,js,raw,warmup,observed))
    return parsed

def execute(case,directory,timeout=240):
    case=validate(case);directory=Path(directory).resolve()
    if directory.exists(): raise ValueError('fresh audit artifacts required')
    before=snapshot(case);directory.mkdir(parents=True)
    reference_source=directory/'original-source';file=reference_source/'cljs/import_test.cljs';file.parent.mkdir(parents=True);file.write_text(case['source'])
    reference_case={**case,'source_root':str(reference_source),'reference_files_sha256':{'cljs/import_test.cljs':SOURCE_HASH}}
    references={};bundles={};origins={}
    for kind in ('release','checkout'):
        references[kind]=report.reference_cases([reference_case],kind,ROOT/'research/clojurescript',directory/kind,timeout)[ID]
        if references[kind]['status']!='passed': raise ValueError('original whole import body pinned reference failed: '+kind)
        artifacts=directory/kind/ID
        bundles[kind]=dependency_bundle(artifacts/'out/goog');origins[kind]=library_origin(artifacts/'out/goog')
        references[kind].update(source_sha256=SOURCE_HASH,dependency_js_sha256=bundles[kind][1],library_origin=origins[kind],raw_observation_sha256=digest((artifacts/'node/stdout').read_bytes()),build_stdout_sha256=digest((artifacts/'build/stdout').read_bytes()),reference_javascript_sha256=digest((artifacts/'reference.js').read_bytes()))
        print(kind,'original six assertions passed',flush=True)
    if bundles['release']!=bundles['checkout'] or origins['release']!=origins['checkout']: raise ValueError('pin Closure embedding profile differs; separate audited profile required')
    loader,dependencies=bundles['release'];(directory/'genuine-closure-loader.js').write_text(loader)
    request=directory/'input.tsv';output=directory/'observations.tsv';request.write_text(ID+'\t'+case['entry']+'\t'+case['source'].encode().hex()+'\t'+loader.encode().hex()+'\t'+macro_source(case['source'],loader).encode().hex()+'\n')
    build,_=transport.run(['cargo','test','--locked','--test','compiler_hosted_import_source_context','--no-run','--message-format=json'],directory/'build',timeout)
    binaries=[Path(row['executable']) for row in map(json.loads,build.splitlines()) if row.get('reason')=='compiler-artifact' and row.get('target',{}).get('name')=='compiler_hosted_import_source_context' and row.get('executable')]
    if len(binaries)!=1: raise ValueError('missing/duplicate actual hosted import audit executable')
    binary=binaries[0];binary_hash=digest(binary.read_bytes());node=Path(shutil.which(os.environ.get('NODE','node')) or '').resolve()
    if not node.is_file(): raise ValueError('Node unavailable')
    node_hash=digest(node.read_bytes());env={**os.environ,'LIGHT_CLJS_HOSTED_IMPORT_INPUT':str(request),'LIGHT_CLJS_HOSTED_IMPORT_OUTPUT':str(output),'LIGHT_CLJS_HOSTED_IMPORT_ROOT':str(directory/'phase-sources')}
    stdout,_=transport.run([str(binary),TEST,'--ignored','--exact','--nocapture'],directory/'audit',timeout,environment=env)
    if 'running 1 test' not in stdout or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in stdout: raise ValueError('hosted import audit not actually executed')
    layers={}
    for variant,source,js,raw,warmup,quickjs in parse_raw(output.read_text(),case,loader):
        target=directory/variant;target.mkdir();(target/'source.cljs').write_text(source);(target/'compiled.js').write_text(js);(target/'quickjs.json').write_text(raw+'\n')
        captured=js.replace(' = // Original bounded runtime',' = globalThis.lightRuntime = // Original bounded runtime',1)
        script='globalThis.lightInspectValue = value => globalThis.lightRuntime?.inspect(value) ?? null;\nglobalThis.lightCases = [observe => { '+loader+'; const run = '+captured+'; return run(observe); }];\nglobalThis.lightCaseMetadata = '+json.dumps([[ID,case['source']]])+';\n'+transport.NODE_OBSERVER.read_text()
        (target/'node-script.js').write_text(script);node_raw,stderr=transport.run([str(node),'-'],target/'node',timeout,source=script);node_layer=report.observations(node_raw,[case])[ID];frames=stderr.splitlines()
        if node_layer['status']!='passed' or len(frames)!=2 or not frames[0].startswith('CASE START ') or not frames[1].startswith('CASE END ') or json.loads(frames[0][11:]).get('id')!=ID or json.loads(frames[1][9:]).get('id')!=ID: raise ValueError('invalid/mismatching raw Node outcome/stderr')
        provenance={'source_sha256':SOURCE_HASH,'original_module_sha256':MODULE_HASH,'original_body_sha256':BODY_HASH,'original_assertion_sha256':case['original_assertion_sha256'],'phase_source_sha256':digest(source),'compiled_js_sha256':digest(js),'closure_loader_sha256':digest(loader),'dependency_js_sha256':dependencies,'library_origin':origins['release'],'compiler_build_producer_source_sha256':before,'audit_executable_sha256':binary_hash,'node_executable_sha256':node_hash,'build_stdout_sha256':digest(build),'warmup_compiled_js_sha256':None if warmup=='-' else digest(bytes.fromhex(warmup))}
        for engine,layer,raw_frame in [('QuickJS',quickjs,raw),('Node',node_layer,node_raw)]:layers[variant+'/'+engine]={**layer,**provenance,'raw_observation_sha256':digest(raw_frame)}
    transport.unchanged(before,snapshot(case))
    for kind in ('release','checkout'):
        if dependency_bundle(directory/kind/ID/'out/goog')!=bundles[kind] or library_origin(directory/kind/ID/'out/goog')!=origins[kind]:raise ValueError('dependency/build source drift')
    if digest(binary.read_bytes())!=binary_hash or digest(node.read_bytes())!=node_hash:raise ValueError('execution binary drift')
    if list(layers)!=[v+'/'+e for v in VARIANTS for e in ('QuickJS','Node')]:raise ValueError('incomplete eight original import observations')
    result={'status':'passed','case':case,'references':references,'observations':layers,'prospective_original_assertion_credit':6,'capability':'explicit audited genuine Closure embedding','generic_Closure_build_parity':'not claimed'}
    (directory/'evidence.json').write_text(json.dumps(result,indent=2)+'\n');return result

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',required=True,type=Path);parser.add_argument('--timeout',default=240,type=int);args=parser.parse_args();execute(load(),args.output,args.timeout);print('all eight hosted import observations passed',flush=True)
if __name__=='__main__':main()
