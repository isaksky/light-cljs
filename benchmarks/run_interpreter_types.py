#!/usr/bin/env python3
"""Measure validated native type costs; reuse existing build, statistics and A/B tools."""
import argparse
import csv
import json
import os
from pathlib import Path
import platform
import sys
import tarfile
import time

from run_interpreter import ROOT, build, capture, distribution, save_json, sha
from run_interpreter_transducers import measured, old_paths, old_samples, summarize


def type_summary(path):
    result = summarize(path)
    counts = {}
    for row in csv.DictReader(path.open()):
        key = row['scenario'] + '/' + row['phase']
        counts.setdefault(key, []).append(int(row['gc_collections']))
    for key, values in counts.items():
        result[key]['gc_collections_per_sample'] = distribution(values)
    return result


def reuse_summary(path):
    groups = {}
    for row in csv.DictReader(path.open()):
        for field in ['elapsed_ns', 'allocations', 'reallocations', 'bytes']:
            groups.setdefault(row['phase'], {}).setdefault(field, []).append(
                int(row[field]) / int(row['iterations']))
    return {phase: {field: distribution(values) for field, values in fields.items()}
            for phase, fields in groups.items()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True, help='new directory outside target')
    parser.add_argument('--baseline', type=Path, help='saved matching old-fixture baseline')
    parser.add_argument('--samples', type=int, default=9)
    parser.add_argument('--iterations', type=int, default=32)
    args = parser.parse_args()
    parser.error("native CLI benchmark retired after evaluator migration; use the pinned historical binaries/results or the evaluator startup measurement")
    if args.samples < 9 or args.iterations < 1:
        parser.error('reports require at least nine samples and positive iterations')
    out = args.output.expanduser().resolve()
    if out.is_relative_to(ROOT / 'target'):
        parser.error('save outside target')
    out.mkdir(parents=True, exist_ok=False)
    (out / 'bin').mkdir()
    files = sorted({p for folder in ['src', 'benches', 'tests', 'examples', 'docs', 'benchmarks/fixtures']
                    for p in (ROOT / folder).rglob('*') if p.is_file()} |
                   {ROOT / path for path in ['Cargo.toml', 'Cargo.lock', 'README.md',
                    'benchmarks/run_interpreter.py', 'benchmarks/run_interpreter_transducers.py',
                    'benchmarks/run_interpreter_types.py', 'benchmarks/interpreter.md', 'benchmarks/README.md']})
    hashes = {str(p.relative_to(ROOT)): sha(p) for p in files}
    with tarfile.open(out / 'sources.tar.gz', 'w:gz') as archive:
        for path in files:
            archive.add(path, arcname=path.relative_to(ROOT))
    environment = {
        'utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
        'platform': platform.platform(), 'machine': platform.machine(), 'logical_cpus': os.cpu_count(),
        'rustc': capture(['rustc', '-Vv']).stdout, 'cargo': capture(['cargo', '-V']).stdout,
        'python': sys.version, 'git_head': capture(['git', 'rev-parse', 'HEAD']).stdout.strip(),
        'git_status': capture(['git', 'status', '--porcelain']).stdout, 'command': sys.argv,
        'build_environment': {k: v for k, v in os.environ.items()
                              if k.startswith(('CARGO_PROFILE_', 'CARGO_BUILD_'))
                              or k in {'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC', 'RUSTUP_TOOLCHAIN', 'MACOSX_DEPLOYMENT_TARGET'}},
        'method': 'sequential single-thread release runs; no affinity; default opt-level=3; two warmups; one GC per batch except explicit method-callback GC; instrumented elapsed_ns=0; CLI includes startup/read/analyze/evaluate/print/exit; OS caches uncontrolled',
    }
    if platform.system() == 'Darwin':
        environment['hardware'] = capture(['sysctl', 'hw.model', 'hw.memsize', 'hw.ncpu', 'machdep.cpu.brand_string']).stdout
    save_json(out / 'environment.json', environment)
    for target in ['light-cljs', 'interpreter_types', 'interpreter', 'interpreter_gc', 'interpreter_reuse']:
        build(out, target)
    for target in ['interpreter_types', 'interpreter', 'interpreter_gc', 'interpreter_reuse']:
        build(out, target, allocations=True)
    if args.baseline:
        old_paths(out, args.baseline.expanduser().resolve())
    common = ['--samples', str(args.samples), '--iterations', str(args.iterations)]
    measured(out, 'timings', [out / 'bin/interpreter_types', *common])
    measured(out, 'allocations', [out / 'bin/interpreter_types-allocations', *common, '--allocations'])
    measured(out, 'existing-timings', [out / 'bin/interpreter', *common])
    measured(out, 'existing-allocations', [out / 'bin/interpreter-allocations', *common, '--allocations'])
    for name in ['interpreter_gc', 'interpreter_gc-allocations']:
        result = capture([str(out / 'bin' / name)])
        (out / f'{name}.log').write_text(result.stdout + result.stderr)
    for name in ['interpreter_reuse', 'interpreter_reuse-allocations']:
        measured(out, name, [out / 'bin' / name])
    cli_rows = []
    names = ['map', 'record', 'external', 'reify', 'closure', 'mixed']
    for sample in range(1, args.samples + 1):
        # Rotate/reverse process order; preserve all samples, including first runs.
        order = names[sample % len(names):] + names[:sample % len(names)]
        if sample % 2 == 0:
            order.reverse()
        for name in order:
            start = time.perf_counter_ns()
            result = capture([str(out / 'bin/light-cljs'), '--eval',
                              str(ROOT / f'benchmarks/fixtures/interpreter/types/cli-{name}.clj')])
            elapsed = time.perf_counter_ns() - start
            if result.stdout != '499500\n' or result.stderr:
                raise RuntimeError(f'unexpected {name} CLI result: {result.stdout} {result.stderr}')
            cli_rows.append([name, sample, 1, elapsed, 499500])
    with (out / 'fresh-process.csv').open('w', newline='') as file:
        writer = csv.writer(file)
        writer.writerow(['scenario', 'sample', 'iterations', 'elapsed_ns', 'checksum'])
        writer.writerows(cli_rows)
    if args.baseline:
        old_samples(out, args.samples, args.iterations)
    summaries = {'timings': type_summary(out / 'timings.csv'),
                 'allocations': type_summary(out / 'allocations.csv')}
    for path in out.glob('*.csv'):
        if path.stem.startswith(('existing-', 'old-path-')):
            summaries[path.stem] = summarize(path)
        elif path.stem.startswith('interpreter_reuse'):
            summaries[path.stem] = reuse_summary(path)
    save_json(out / 'summary.json', {
        'units': 'elapsed_ns/allocations/reallocations/requested_bytes per operation; live/peak requested bytes and all memory counts are absolute; invoke/analyzed_sum/construct_1000 operation contains 1000 items; GC operation means one collection; gc_collections_per_sample is absolute',
        'results': summaries,
        'fresh-process_ns': {name: distribution([r[3] for r in cli_rows if r[0] == name]) for name in names},
    })
    if any(sha(ROOT / path) != digest for path, digest in hashes.items()):
        raise RuntimeError('source changed during measurements; preserve this run and rerun')
    save_json(out / 'manifest.json', {
        'schema': 1, 'sources': hashes,
        'fixtures': {p: h for p, h in hashes.items() if p.startswith('benchmarks/fixtures/')},
        'binaries': {p.name: sha(p) for p in (out / 'bin').iterdir()},
        'source_archive_sha256': sha(out / 'sources.tar.gz'),
    })
    print('Saved verified measurements:', out, flush=True)


if __name__ == '__main__':
    main()
