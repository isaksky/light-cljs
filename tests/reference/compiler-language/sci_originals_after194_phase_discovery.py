#!/usr/bin/env python3
"""Execute every disjoint whole original class-family source in actual phases.

Each original runs independently so a located compile or transport rejection
cannot hide later sources. Every raw full/partial frame and failure stays visible;
failed/rejected contexts grant no source or SCI pass. No source is weakened.
"""
from pathlib import Path
import argparse,json,os,subprocess
import sci_originals_after194_discovery as originals
import sci_gap_canonical_context as phase
import sci_execution as sci


def snapshot():
    return {**originals.snapshot(),str(Path(__file__).relative_to(sci.ROOT)):sci.digest(Path(__file__).read_bytes())}


def execute(root):
    root=Path(root).resolve();root.mkdir(parents=True,exist_ok=False);before=snapshot();rows=originals.load()
    built=subprocess.run(['cargo','test','--locked','--test',phase.DRIVER.stem,'--no-run','--message-format=json'],cwd=sci.ROOT,capture_output=True,text=True,timeout=600)
    (root/'build.stdout').write_text(built.stdout);(root/'build.stderr').write_text(built.stderr)
    if built.returncode:raise ValueError('Exact existing whole-source actual phase driver unavailable')
    artifacts=[json.loads(line) for line in built.stdout.splitlines() if line.startswith('{')]
    binaries=[a['executable'] for a in artifacts if a.get('reason')=='compiler-artifact' and a.get('target',{}).get('name')==phase.DRIVER.stem and a.get('executable')]
    if len(binaries)!=1:raise ValueError('Exact unique source/phase audit executable missing')
    binary=binaries[0];binary_hash=sci.digest(Path(binary).read_bytes());result={}
    for row in rows:
        folder=root/row['sci_id'];folder.mkdir();(folder/'input.tsv').write_text(row['id']+'\t'+row['source'].encode().hex()+'\n')
        command=[binary,phase.TEST,'--ignored','--exact','--nocapture']
        environment={**os.environ,'PYTHONDONTWRITEBYTECODE':'1','LIGHT_SCI_GAP_PHASE_ROOT':str(folder)}
        process=subprocess.run(command,cwd=sci.ROOT,env=environment,capture_output=True,text=True,timeout=300)
        (folder/'stdout').write_text(process.stdout);(folder/'stderr').write_text(process.stderr);(folder/'command.json').write_text(json.dumps(command)+'\n')
        raw=(folder/'observations.tsv').read_text();frames=[]
        for line in raw.splitlines():
            identity,variant,engine,source_hex,js_hex,packet_hex,warmup_hex=line.split('\t')
            source=bytes.fromhex(source_hex).decode();expected=phase.module(row,variant.split('-')[0])
            if identity!=row['id'] or source!=expected:raise ValueError('Whole original actual context source modified/detached')
            packet=json.loads(bytes.fromhex(packet_hex).decode())
            frames.append({'variant':variant,'engine':engine,'whole_phase_source':source,
              'whole_phase_source_sha256':sci.digest(source),'emitted_js':bytes.fromhex(js_hex).decode(),
              'emitted_js_sha256':sci.digest(bytes.fromhex(js_hex)),'raw_packet':packet,
              'warmup_js_sha256':None if warmup_hex=='-' else sci.digest(bytes.fromhex(warmup_hex))})
        expected_order=[(v,e) for v in phase.VARIANTS for e in ('QuickJS','Node')]
        observed_order=[(f['variant'],f['engine']) for f in frames]
        if observed_order!=expected_order[:len(frames)]:raise ValueError('Whole original actual frame order/partial boundary changed')
        passed=process.returncode==0 and process.stdout.count('test result: ok. 1 passed; 0 failed; 0 ignored;')==1 and observed_order==expected_order
        result[row['id']]={'source_binding':row,'status':'observed-all-eight-contexts' if passed else 'original-context-failed',
          'process_returncode':process.returncode,'complete_raw_frames':frames,'expected_frames':8,'observed_frames':len(frames),
          'unexecuted_contexts':expected_order[len(frames):],'original_semantic_pass_credit':0,
          'failure_stdout':process.stdout if not passed else '', 'failure_stderr':process.stderr if not passed else ''}
        print(row['sci_id'],result[row['id']]['status'],len(frames),flush=True)
    if snapshot()!=before or sci.digest(Path(binary).read_bytes())!=binary_hash:raise ValueError('Whole original actual source/compiler/executable changed')
    receipt={'schema_version':1,'scope':'Every31disjoint complete original source; actual context diagnosis, zero acceptance credit',
      'source_snapshot':before,'actual_executable':binary,'actual_executable_sha256':binary_hash,'evidence':result,
      'original_semantic_pass_credit':0,'strict_complete':False,
      'artifact_sha256':{str(p.relative_to(root)):sci.digest(p.read_bytes()) for p in sorted(root.rglob('*')) if p.is_file()}}
    (root/'evidence.json').write_text(json.dumps(receipt,indent=2)+'\n');return receipt

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--execute',type=Path,required=True);args=parser.parse_args();execute(args.execute)
