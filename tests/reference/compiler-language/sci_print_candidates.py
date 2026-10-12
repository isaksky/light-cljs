#!/usr/bin/env python3
"""Fresh unchanged historical printing candidates; no shared credit.

A historical nonpassing disposition cannot prove the present source's behavior.
Every original row and expectation is preserved, including disagreements. These
ordinary observations alone cannot establish complete phase acceptance.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import sci_execution as sci

REVIEWED = {'sci-string-pr-characters': {'row_sha256': '8e3d682dc580654eaea0ce1dc550a4897c67aec3ece72ab3b308bc1ae074f75e', 'original_row_sha256': '345b5c551200fde67f63afa094a0fa75cd7ecc256059503185b4305548a676c8', 'source_sha256': '2c87cb63bb1d524b024530fad22fa943e3fb54ae04c18394b1fd52face71ea08'}, 'sci-string-pr-collections': {'row_sha256': 'b96e1a7ba923df8df786e1a5c8cb67fb7ed8c60bcb932280304b16ff71b16637', 'original_row_sha256': 'c7b5bea882743d72a1ed2d26d7ff491f993b5097783d9d0d6b205d4db0a26155', 'source_sha256': 'd1aca10f5747c5adb70542130b540cb266857409786126c83e9ce6f884e8239c'}, 'sci-string-pr-level-false': {'row_sha256': '153dccaeb5b57b0fa91edc556ca11be30e4eb58808a13c6ed36be6cbbf32b715', 'original_row_sha256': '7d183cd786a7ee3c260ee7e1638a638c76909ff943bab2940a86ba389b186626', 'source_sha256': 'dc5bcaef3625d5f19bde820282d8db6d34b1f715bf5ea246dcf298281a62ca66'}, 'sci-string-pr-meta-length-zero': {'row_sha256': '89d230f1633b87ee4c443195e851d5f16ec6c3f71d53cc8b2a2b12ff351ceba4', 'original_row_sha256': '057f7d0cc7696d198765f75538a1c914132247627eea90a873122bcc70056dac', 'source_sha256': '88806e1128857fa40ecea5833f3b186e9a20ea923d1bad97fedf2628e3740459'}, 'sci-string-pr-meta-level-one': {'row_sha256': '045dfd9cd63f56b801282194ff8a27cda5660fe070affed33240ef5019380e03', 'original_row_sha256': '83bfe18000963e566fbf77ff1aa0d1f0a393421fa6df494306031276b92588fd', 'source_sha256': '99e4f759feb2b5911ee5a48d7f4b58240d83c39056cd9b3c459b09b8b2fe4dc1'}, 'sci-string-pr-meta-level-zero': {'row_sha256': '2607ff57a9d5829c3fb6d2c157f8ac28c9e06986061e093d9a7b8fa2c5f3c3ec', 'original_row_sha256': '13b21dcbc70922586e9d49039b53d677c2cca448836102529e71f68756debe7f', 'source_sha256': 'abd3f8771c376d2ce842447785b71b26b0208432e290dc6e6261e385b329d54b'}, 'sci-string-pr-meta-tag': {'row_sha256': '8474b8c11e5d3163e8296ce3e1fac6e791c913d82b0ff3a7c48569e3bf0122e5', 'original_row_sha256': 'b478679b3efa9a87944bf8cb73cfbfe9b6d6a0d343a5dcac00d87d68f4d33a53', 'source_sha256': 'ff920bfe821af91a8d22af53ce395699e28955b72f94f813f08997dce246f710'}, 'sci-string-pr-meta-unreadable': {'row_sha256': 'a302512af7453d441296f36b5ca04b4de62f5163f2bc20173bc7177af67a6b9b', 'original_row_sha256': '389d612ee341dbe2ce63d222edef02e380432f0134949e066fabb3da4c90fd51', 'source_sha256': '04876926079dacc7d995a80304ca21884ed6c7d8c8009dfa24f3257a16800ccd'}, 'sci-string-pr-tag-integer': {'row_sha256': '9166e1ebc938049210e9c15b03328a6a5da390d307257a272b0e8a3152938a78', 'original_row_sha256': '59b4fc10b4f82e732c7897a89012335268953dfa93f7a385c8b0286dedc6da14', 'source_sha256': '1281bb921e74c9df2b26ff08d85f7779dd81ed3328e3fd18526353462db4ad52'}, 'sci-string-pr-tag-keyword': {'row_sha256': 'cc8fb56904012a09a896a27812cb78bc0ef2cb32e58ba3402196a2c6e9a726ea', 'original_row_sha256': '793a76d30c6cd000259e399f43196761ac047b6458af7d2819aa9ed41fcb3881', 'source_sha256': 'c1e7c84d173bdeebdcf433a7f35c01efa0ae2c43be68eb9e6c8af249242f1f3e'}, 'sci-string-pr-tag-string': {'row_sha256': 'cb515d613db2b3ad71375bfa573f4e83da3bba8456b0eeb5b58526ae257d6f84', 'original_row_sha256': '102f489b98a229f62de30d4fd300def41d54e584f7b648ad38899216bc38c18c', 'source_sha256': '462c7691b916c3d69aaa0bfb1a8c9ebf7048c77c69e3cd773de56ae1d123f637'}, 'sci-string-pr-tag-true': {'row_sha256': '32e6d8b3bce176ac6769c531ca43c4f17d85e21ff48bcf7b0fb649d21ac688a2', 'original_row_sha256': '1f004eda95a14e0b610350046f066c542dece0b0b947a02b731393570d8cd3fe', 'source_sha256': '4f1094acb766aa945173657ad4ef28ce97f572600dc03ce8bb0a1905cc7c4359'}, 'sci-string-str-collections': {'row_sha256': '45adf6f7aef31e321a5cab31e50e3b1d0fa03354c5e85f0598d914839d20f83a', 'original_row_sha256': '2092017e6f0e41ca9aa466b7e2290798288d29f14b6fc5163b0fdb61919a9e13', 'source_sha256': '6efc2cc97582c86331358a46d4b0fa39d768c8f54931992e1c5a9b9ffc4f3338'}, 'sci-string-str-ignores-bindings': {'row_sha256': '6a02fb9a4d3575f3c7d6be54303061a042a1fde2d43bd4bddab7775a6e7b3e6e', 'original_row_sha256': 'a86b3230a5c3b6a978d0e7088d92776c4976ea5113b857d1673c571d256fab36', 'source_sha256': '96db297ae31f511d1ea681aab4d0769823abc4ebc707ccec0010346d05052c35'}}

def load():
    rows = sci.rows(sci.DISPOSITIONS)
    originals = sci.rows(sci.CORPUS)
    result = []
    for identity, binding in REVIEWED.items():
        line, fields = rows[identity]
        original_line, original = originals[identity]
        if (fields[:7] != original or sci.digest(line) != binding['row_sha256']
                or sci.digest(original_line) != binding['original_row_sha256']
                or sci.digest(fields[2]) != binding['source_sha256']
                or fields[7] != 'gap' or fields[2] != fields[8]
                or sci.exclusion([*fields[:7], 'portable', *fields[8:]])):
            raise ValueError('unchanged original candidate row/profile drift: '+identity)
        result.append({'id': 'sci-execution-'+identity, 'sci_id': identity,
            'obligation_id': 'local::tests/reference/sci/evaluator-dispositions.tsv::'+identity,
            'body_sha256': binding['row_sha256'],
            'original_row_sha256': binding['original_row_sha256'],
            'original_source_sha256': binding['source_sha256'],
            'source_sha256': binding['source_sha256'], 'source': fields[2],
            'profile': fields[1], 'retained_disposition': fields[7],
            'expected': sci.expected(original), 'isolated': True,
            'reference_isolated': True, 'semantic_pass_credit': False,
            'macro_phase_status': 'not-run',
            'setup': 'Unchanged complete portable source with lexical observe and explicit empty stderr/output capture; no library/reader/namespace fixture is inferred.'})
    return result

def snapshot():
    paths = list((sci.ROOT/'src').rglob('*'))
    paths += [sci.ROOT/name for name in ('Cargo.toml','Cargo.lock','build.rs', sci.TEST_FILE,
        'tests/support/sci_execution_observation.js','tests/reference/execution.clj')]
    paths += [Path(__file__), Path(__file__).with_name('sci_print_context.py'),
        Path(__file__).with_name('test_sci_print_context.py'), Path(__file__).with_name('sci-print-canonical-contracts.jsonl'),
        Path(__file__).with_name('sci_gap_canonical_context.py'), sci.CORPUS, sci.DISPOSITIONS, Path(sci.__file__), sci.HERE/'report.py',
        sci.ROOT/'tests/support/execution.js', sci.HERE/'configured_source_context.py',
        sci.HERE/'sci-analysis.clj', sci.ROOT/'tests/reference/analysis_boundaries.clj']
    return {str(p.relative_to(sci.ROOT)): sci.digest(p.read_bytes()) for p in sorted(set(paths)) if p.is_file()}

def run(directory, timeout):
    directory.mkdir(parents=True, exist_ok=False)
    before = snapshot()
    cases = load()
    evidence = {case['id']: {} for case in cases}
    local = sci.local_cases(cases, directory/'local', timeout)
    for identity, layer in local.items(): evidence[identity]['javascript'] = layer
    print('Local', dict(Counter(x['status'] for x in local.values())), flush=True)
    for kind in ('release','checkout'):
        refs = sci.reference_cases(cases, kind, sci.ROOT/'research/clojurescript', directory/kind, timeout)
        for identity, layer in refs.items(): evidence[identity][kind] = layer
        print(kind, dict(Counter(x['status'] for x in refs.values())), flush=True)
    if before != snapshot(): raise ValueError('production/source/build/producer drift during candidate audit')
    artifact_hashes = {str(p.relative_to(directory)): sci.digest(p.read_bytes())
        for p in sorted(directory.rglob('*')) if p.is_file()}
    result = {'schema_version': 1, 'scope': 'Exact 14 unchanged historical printing returned candidates',
        'cases': cases, 'producer_sha256': before, 'artifact_sha256': artifact_hashes,
        'evidence': evidence, 'semantic_pass_credit': False, 'macro_phase_status': 'not-run'}
    (directory/'evidence.json').write_text(json.dumps(result, indent=2, ensure_ascii=True)+'\n')
    agreeing = [case['sci_id'] for case in cases if all(evidence[case['id']][layer]['status']=='passed'
        for layer in ('javascript','release','checkout'))]
    print('Original agreements in both engines and pins:', agreeing, flush=True)
    return result

if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--timeout', type=int, default=300)
    args=parser.parse_args()
    run(args.output, args.timeout)
