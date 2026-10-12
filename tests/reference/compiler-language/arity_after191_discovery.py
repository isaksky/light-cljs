#!/usr/bin/env python3
"""Independent original state/callable/protocol/interop discovery; no pass credit."""
from pathlib import Path
import json
from arity_resume_candidates import digest
ROOT=Path(__file__).resolve().parents[3]
HERE=ROOT/'tests/reference/compiler-language'
CATALOG=HERE/'arity-after191-discovery-catalog.json'
CATALOG_SHA256='381dbc17d7cf9f6a7815773beaf9ed3e176a37dbee4b675f8e0a51f40979feac'
PRINT_SUPPORT=[]
CONTEXT_SUPPORT=[]


def rows():
    if digest(CATALOG.read_bytes())!=CATALOG_SHA256:raise ValueError('original discovery catalog drift')
    catalog=json.loads(CATALOG.read_text())
    originals={l.split('\t')[0]:l for l in (ROOT/'tests/fixtures/execution-outcomes.tsv').read_text().splitlines() if l and not l.startswith('#')}
    output=[]
    for row in catalog['reviewed']:
        raw=originals[row['original_id']];f=raw.split('\t');case={'id':row['id'],'source':f[1],'expected':{'id':row['id'],'outcome':json.loads(f[2]),'trace':json.loads(f[3]),'stdout':json.loads(f[4])},'native_stdout':None}
        if digest(raw)!=row['original_row_sha256'] or digest(f[1])!=row['source_sha256'] or (ROOT/row['fixture']).read_text()!=f[1] or case!=row['case'] or digest(json.dumps(case,sort_keys=True))!=row['case_sha256']:raise ValueError('original complete discovery row/source/expectation drift')
        output.append({**row,'source':f[1]})
    return output
