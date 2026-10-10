#!/usr/bin/env python3
"""Validated compiler/Node measurements, using only the Python standard library.

Save exact binaries and sources outside target. Optional --baseline-ref builds an
unchanged Git archive in a temporary directory without changing any checkout.
"""
import argparse
import csv
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time

from run_interpreter import ROOT, distribution, save_json, sha

FIXTURES = ROOT / 'benchmarks/fixtures/compiler/collections'
PRIMITIVES = ['integer', 'nested-closure', 'js-nested-method']


def capture(command, cwd=ROOT):
    return subprocess.run([str(x) for x in command], cwd=cwd, check=True, text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=600)


def build(out, root, target, counted=False, label='current'):
    command = ['cargo', 'build' if target == 'light-cljs' else 'bench', '--locked']
    command += ['--release', '--bin', target] if target == 'light-cljs' else ['--no-run', '--bench', target]
    command += ['--message-format=json']
    if counted:
        command += ['--features', 'count-allocations']
    result = capture(command, root)
    with (out / 'build.log').open('a') as log:
        log.write(json.dumps(command) + '\n' + result.stderr)
    artifacts = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
    paths = [a['executable'] for a in artifacts if a.get('reason') == 'compiler-artifact'
             and a.get('target', {}).get('name') == target and a.get('executable')]
    if len(paths) != 1:
        raise RuntimeError(f'expected exactly one {target} binary: {paths}')
    destination = out / 'bin' / (label + '-' + target + ('-allocations' if counted else ''))
    shutil.copy2(paths[0], destination)
    return destination


def primitive_rows(path):
    rows = {r[0]: r for line in path.read_text().splitlines() if line and not line.startswith('#')
            for r in [line.split('\t')]}
    return {name: rows[name] for name in PRIMITIVES}


def summarize(rows, fields):
    groups = {}
    for row in rows:
        key = row['scenario'] + '/' + str(row.get('phase', row.get('size', '')))
        for field in fields:
            groups.setdefault(key, {}).setdefault(field, []).append(float(row[field]) / int(row['iterations']))
    return {key: {field: distribution(values) for field, values in group.items()} for key, group in sorted(groups.items())}


def record_bench(out, binary, name, args, extra=()):
    result = capture([binary, '--samples', args.samples, '--iterations', args.iterations, *extra])
    (out / (name + '.csv')).write_text(result.stdout)
    (out / (name + '.log')).write_text(result.stderr)
    rows = list(csv.DictReader(io.StringIO(result.stdout)))
    groups = {}
    for row in rows:
        groups.setdefault((row['scenario'], row['phase']), []).append(row)
    if not groups or any(len(group) != args.samples or {int(r['sample']) for r in group} != set(range(1,args.samples+1)) for group in groups.values()):
        raise RuntimeError('missing/duplicate resident samples')
    return summarize(rows, ['elapsed_ns', 'allocations', 'reallocations', 'requested_bytes'])


def kw(name):
    return {'$keyword': {'namespace': None, 'name': name, 'spelling': name}}


def driver(js, name, source):
    if js.count('\n  const rt = ') > 1:
        raise RuntimeError('duplicate runtime')
    js = js.replace('\n  const rt = ', '\n  const rt = globalThis.benchRuntime = ', 1)
    return ('globalThis.lightInspectValue = v => globalThis.benchRuntime ? benchRuntime.inspect(v) : null;\n'
            + 'globalThis.lightCaseMetadata = ' + json.dumps([[name, source]]) + ';\n'
            + 'globalThis.lightCases = [() => { return ' + js + '; }];\n'
            + (ROOT / 'tests/support/execution.js').read_text())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--samples', type=int, default=9)
    parser.add_argument('--iterations', type=int, default=32)
    parser.add_argument('--baseline-ref', help='immutable commit/ref to archive and build; compares the three unchanged primitive fixtures')
    args = parser.parse_args()
    if args.samples < 9 or args.iterations < 1:
        parser.error('require >=9 samples and positive iterations')
    out = args.output.expanduser().resolve()
    if out.is_relative_to(ROOT / 'target'):
        parser.error('save outside target')
    out.mkdir(parents=True, exist_ok=False)
    for directory in ['bin','emitted','drivers']:
        (out / directory).mkdir()
    node = os.environ.get('NODE', 'node')
    paths = sorted({p for folder in ['src','benches','tests','benchmarks/fixtures','benchmarks/support']
                    for p in (ROOT / folder).rglob('*') if p.is_file()} |
                   {ROOT / p for p in ['Cargo.toml','Cargo.lock','benchmarks/run_compiler.py','benchmarks/run_interpreter.py']})
    hashes = {str(p.relative_to(ROOT)): sha(p) for p in paths}
    with tarfile.open(out / 'sources.tar.gz', 'w:gz') as archive:
        for path in paths:
            archive.add(path, arcname=path.relative_to(ROOT))
    environment = {
        'utc': time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),
        'platform': platform.platform(), 'machine': platform.machine(), 'logical_cpus': os.cpu_count(),
        'rustc': capture(['rustc','-Vv']).stdout, 'cargo': capture(['cargo','-V']).stdout,
        'node': capture([node,'--version']).stdout.strip(), 'node_path': shutil.which(node), 'python': sys.version,
        'git_head': capture(['git','rev-parse','HEAD']).stdout.strip(),
        'git_status': capture(['git','status','--porcelain']).stdout, 'command': sys.argv,
        'build_environment': {k:v for k,v in os.environ.items() if k.startswith(('CARGO_PROFILE_','CARGO_BUILD_')) or k in {'RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','RUSTC','RUSTUP_TOOLCHAIN','MACOSX_DEPLOYMENT_TARGET','NODE_OPTIONS'}},
        'options': {'samples':args.samples,'iterations':args.iterations,'warmup_batches':2,'rust_profile':'release opt-level=3 unless overridden','node_flags':[]},
        'method':'Sequential single-thread phases; no CPU affinity; resident source warmups; fresh CLI/Node processes with uncontrolled OS caches; fresh Node includes shared typed validator and output through process exit. No browser extrapolation; allocations are a distinct build with elapsed_ns=0.',
    }
    if platform.system() == 'Darwin':
        environment['hardware'] = capture(['sysctl','hw.model','hw.memsize','hw.ncpu','machdep.cpu.brand_string']).stdout
    save_json(out / 'environment.json', environment)
    print('Building current timing, CLI and allocation binaries', flush=True)
    cli = build(out, ROOT, 'light-cljs')
    bench = build(out, ROOT, 'compiler')
    counted = build(out, ROOT, 'compiler', counted=True)
    result = {'resident': record_bench(out,bench,'resident',args,['--dump',out/'emitted']),
              'allocations': record_bench(out,counted,'allocations',args,['--allocations'])}
    current_rows = primitive_rows(ROOT/'tests/fixtures/execution.tsv')
    baseline_cli = None
    if args.baseline_ref:
        ref = capture(['git','rev-parse',args.baseline_ref+'^{commit}']).stdout.strip()
        print('Building unchanged primitive baseline', ref, flush=True)
        archive_path = out / 'baseline-source.tar'
        capture(['git','archive','--format=tar','--output',archive_path,ref])
        baseline_identity = {'commit':ref,'git_archive_sha256':sha(archive_path),'harness_sources':{p:h for p,h in hashes.items() if p.startswith(('benches/compiler.rs','benches/support/allocations.rs','benchmarks/fixtures/compiler/'))}}
        with tempfile.TemporaryDirectory(prefix='light-cljs-compiler-baseline-') as directory:
            base = Path(directory)
            with tarfile.open(archive_path) as archive:
                archive.extractall(base, filter='data')
            old_rows = primitive_rows(base/'tests/fixtures/execution.tsv')
            if old_rows != current_rows:
                raise RuntimeError('primitive source/expected fixtures differ from baseline')
            shutil.copy2(ROOT/'benches/compiler.rs',base/'benches/compiler.rs')
            shutil.copy2(ROOT/'benches/support/allocations.rs',base/'benches/support/allocations.rs')
            shutil.copytree(FIXTURES,base/'benchmarks/fixtures/compiler/collections')
            with (base/'Cargo.toml').open('a') as file:
                file.write('\n[[bench]]\nname = "compiler"\nharness = false\n')
            baseline_cli = build(out,base,'light-cljs',label='baseline')
            baseline_bench = build(out,base,'compiler',label='baseline')
            result['primitive-baseline'] = record_bench(out,baseline_bench,'primitive-baseline',args,['--primitive-only'])
        save_json(out/'baseline-identity.json',baseline_identity)
        # Keep the exact original archive compressed after building it.
        with archive_path.open('rb') as src, gzip.GzipFile(filename=str(out/'baseline-source.tar.gz'),mode='wb',mtime=0) as dest:
            shutil.copyfileobj(src,dest)
        archive_path.unlink()

    scenarios = []
    for name,row in current_rows.items():
        scenarios.append((name,['--text',row[1]],json.loads(row[2]),row[1]))
    scenarios += [('empty',['--text',''],None,''),
                  ('collections',[FIXTURES/'value.cljs'],{'$vector':[{'$vector':[1,2]},{'$vector':[1,2,3]},kw('pair'),True,6]},'value.cljs'),
                  ('expanded-macro',['--text','[1 2 3]'],{'$vector':[1,2,3]},'[1 2 3]')]
    for entry in ['bench.single','bench.main','bench.macro','bench.macro-main']:
        scenarios.append((entry,['--entry',entry,'--source-root',FIXTURES],6,entry))
    outputs, expected, sizes = {}, {}, []
    for name,command,value,source in scenarios:
        labels = [('current',cli)] + ([('baseline',baseline_cli)] if baseline_cli and name in PRIMITIVES else [])
        for label,binary in labels:
            js_result = capture([binary,'--compile',*command])
            if js_result.stderr:
                raise RuntimeError(js_result.stderr)
            js = js_result.stdout
            if label == 'current' and (out/'emitted'/f'{name}.js').read_text() != js:
                raise RuntimeError('CLI/API emitted output mismatch: '+name)
            data = js.encode('utf8')
            runtime_count = js.count('\n  const rt = ')
            if runtime_count != (0 if name in PRIMITIVES or name == 'empty' else 1):
                raise RuntimeError('unexpected runtime inclusion: '+name)
            outputs[label,name] = js
            (out/'emitted'/f'{label}-{name}.js').write_text(js)
            path = out/'drivers'/f'{label}-{name}.js'
            path.write_text(driver(js,name,source))
            expected[label,name] = {'id':name,'outcome':{'status':'returned','value':value},'trace':[],'stdout':''}
            check = capture([node,path])
            if json.loads(check.stdout) != expected[label,name]:
                raise RuntimeError('incorrect typed benchmark value: '+check.stdout)
            sizes.append({'label':label,'scenario':name,'utf8_bytes':len(data),'gzip_bytes':len(gzip.compress(data,mtime=0)),
                          'sha256':hashlib.sha256(data).hexdigest(),'runtime_count':runtime_count,
                          'truth_helpers':js.count('const truth = '),'host_helpers':js.count('const host = ')})
    if baseline_cli:
        for name in PRIMITIVES:
            if outputs['current',name] != outputs['baseline',name]:
                raise RuntimeError('unchanged primitive output differs from baseline')
    save_json(out/'output-sizes.json',sizes)

    print('Measuring fresh processes, validating every result', flush=True)
    process_rows = []
    jobs = [(label,binary,name,command) for name,command,_,_ in scenarios
            for label,binary in [('current',cli)] + ([('baseline',baseline_cli)] if baseline_cli and name in PRIMITIVES else [])]
    # Equal two fresh-process warmups per compiler and scenario.
    for sample in range(-1,args.samples+1):
        order = jobs[sample % len(jobs):] + jobs[:sample % len(jobs)]
        if sample % 2 == 0:
            order.reverse()
        for label,binary,name,command in order:
            start = time.perf_counter_ns()
            compiled = capture([binary,'--compile',*command])
            elapsed = time.perf_counter_ns() - start
            if compiled.stdout != outputs[label,name] or compiled.stderr:
                raise RuntimeError('fresh CLI output mismatch')
            if sample > 0:
                process_rows.append({'scenario':label+'/'+name,'phase':'cli-to-output','sample':sample,'iterations':1,'elapsed_ns':elapsed})
            start = time.perf_counter_ns()
            usable = capture([node,out/'drivers'/f'{label}-{name}.js'])
            elapsed = time.perf_counter_ns() - start
            if json.loads(usable.stdout) != expected[label,name]:
                raise RuntimeError('fresh Node typed value mismatch')
            if sample > 0:
                process_rows.append({'scenario':label+'/'+name,'phase':'node-through-result','sample':sample,'iterations':1,'elapsed_ns':elapsed})
    with (out/'fresh-process.csv').open('w',newline='') as file:
        writer = csv.DictWriter(file,fieldnames=list(process_rows[0])); writer.writeheader(); writer.writerows(process_rows)
    result['fresh-process'] = summarize(process_rows,['elapsed_ns'])

    operations_js = out/'emitted/operations.js'
    operations_compile = capture([cli,'--compile',FIXTURES/'operations.cljs'])
    if operations_compile.stderr:
        raise RuntimeError(operations_compile.stderr)
    operations_js.write_text(operations_compile.stdout)
    print('Measuring validated resident operations', flush=True)
    operations = capture([node,ROOT/'benchmarks/support/compiler_operations.js',operations_js,args.samples,args.iterations])
    if operations.stderr:
        raise RuntimeError(operations.stderr)
    raw = json.loads(operations.stdout)
    groups = {}
    for row in raw['rows']:
        groups.setdefault((row['scenario'],row['size']),[]).append(row['sample'])
    if len(groups) != 142 or any(sorted(v) != list(range(1,args.samples+1)) for v in groups.values()):
        raise RuntimeError('incomplete operation sample matrix: '+str(len(groups)))
    save_json(out/'operations.json',raw)
    result['operations'] = summarize(raw['rows'],['elapsed_ns'])
    if baseline_cli:
        result['primitive-comparison'] = {
            key:{'baseline_median_ns':before['elapsed_ns']['median'],
                 'current_median_ns':result['resident'][key]['elapsed_ns']['median'],
                 'change_percent':100*(result['resident'][key]['elapsed_ns']['median']/before['elapsed_ns']['median']-1)}
            for key,before in result['primitive-baseline'].items()}
    save_json(out/'summary.json',{'units':'nanoseconds, allocation calls and requested bytes per iteration; no instrumented timing', 'results':result})
    if any(sha(ROOT/path) != digest for path,digest in hashes.items()):
        raise RuntimeError('source changed during run; retained artifacts are not a valid report')
    save_json(out/'manifest.json',{'schema':1,'sources':hashes,'fixtures':{p:h for p,h in hashes.items() if p.startswith('benchmarks/fixtures/compiler/')},
              'primitive_fixture_rows':current_rows,'binaries':{p.name:sha(p) for p in (out/'bin').iterdir()},
              'source_archive_sha256':sha(out/'sources.tar.gz')})
    print('Saved validated report:',out,flush=True)


if __name__ == '__main__':
    main()
