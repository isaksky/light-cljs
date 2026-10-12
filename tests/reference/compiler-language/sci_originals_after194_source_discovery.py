#!/usr/bin/env python3
"""Whole original top-level/user-context diagnostic; no generic fn wrapper.

Established SCI source adapter preserves all exact original forms/source positions
and originalusernamespace. Outcomes remain complete, including rejected profiles.
"""
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
from collections import Counter
import argparse,json
import sci_originals_after194_discovery as originals
import sci_source_context as context
import sci_execution as sci
import extract_inventory as inventory


def load():
    rows=[]
    for row in originals.load():
        source=row['source'];forms=[]
        for node in inventory.Reader(source).all():
            forms.append({'kind':'macro' if inventory.short_head(node)=='defmacro' else 'runtime',
              'source':source[node.start:node.end], 'utf8_byte_start':len(source[:node.start].encode()),'utf8_byte_end':len(source[:node.end].encode())})
        rows.append({**row,'namespace':'user','source_name':'sci-original.cljs','forms':forms})
    return rows


def snapshot():
    paths=[Path(__file__),Path(context.__file__),sci.ROOT/context.DRIVER,sci.ROOT/context.ORACLE]
    return {**originals.snapshot(),**{str(p.relative_to(sci.ROOT)):sci.digest(p.read_bytes()) for p in paths}}


def execute(root):
    root=Path(root).resolve();root.mkdir(parents=True,exist_ok=False);before=snapshot();rows=load();bindings=originals.prior.reference_execution_binding()
    (root/'source-bindings.json').write_text(json.dumps(rows,indent=2)+'\n');evidence={r['id']:{} for r in rows}
    local=context.local_cases(rows,root/'local',600)
    for identity,layer in local.items():evidence[identity]['javascript']=layer
    print('Whole original top-level local '+str(dict(Counter(l['status'] for l in local.values()))),flush=True)
    with ThreadPoolExecutor(max_workers=2) as pool:
        futures={kind:pool.submit(context.reference_cases,rows,kind,root/kind,600) for kind in ('release','checkout')}
        for kind,future in futures.items():
            layers=future.result()
            for identity,layer in layers.items():evidence[identity][kind]=layer
            print('Whole original top-level '+kind+' '+str(dict(Counter(l['status'] for l in layers.values()))),flush=True)
    if snapshot()!=before or originals.prior.reference_execution_binding()!=bindings:raise ValueError('Original top-level source/compiler/executable changed')
    receipt={'schema_version':1,'scope':'31complete unchanged original top-level/usernamespace/position sources; zero acceptance credit',
      'source_bindings':rows,'source_snapshot':before,'reference_execution_binding':bindings,'evidence':evidence,
      'macro_phase_status':'not-run','original_semantic_pass_credit':0,'strict_complete':False,
      'artifact_sha256':{str(p.relative_to(root)):sci.digest(p.read_bytes()) for p in sorted(root.rglob('*')) if p.is_file()}}
    (root/'evidence.json').write_text(json.dumps(receipt,indent=2)+'\n');return receipt

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--execute',type=Path,required=True);args=parser.parse_args();execute(args.execute)
