#!/usr/bin/env python3
"""Fresh unchanged historical lazy/iteration/control candidates; no shared credit.

A historical nonpassing disposition cannot prove the present source's behavior.
Every original row and expectation is preserved, including disagreements. These
ordinary observations alone cannot establish complete phase acceptance.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import sci_execution as sci

REVIEWED = {'sci-meta-list-tail': {'row_sha256': 'aa2975c650b51115dc22a8edfab984a6344499f8d845d4a80e12749104df1cbb', 'original_row_sha256': 'efaaeef07f45a47da66b1dc6dfed84d1860524764e373a9742da4327f6e92ead', 'source_sha256': 'f24c1e88ac63141e45fbf31360310357835d0658d04afdae670f327cceba94a4'}, 'sci-deferred-lazy-nested-realized': {'row_sha256': 'c7fbab7fbc279c664351b6507c57cbaa37e5dbb679e854edbb5ad817bedbc2a2', 'original_row_sha256': '23ba932adaac6ff03b3d99871658c93c546623ef13f069c3046d1e574c087c17', 'source_sha256': '754e853ead1b1ef2de37c109dc9986de2cc1ea9e7709deb1bda185ab3e07da16'}, 'sci-deferred-lazy-meta': {'row_sha256': '16252f100d189d5963b6974d1c52ab079ac790cdaa1a1560b24b5e53084dc170', 'original_row_sha256': 'e2c8d7e6510a2845c9758474f76f4da03273c1e4d5d0844bf12ad5dec82138d3', 'source_sha256': 'f1559a50c4f2eabb5d947fc5a340c4a0018ebc32521402b26474977caa83ed45'}, 'sci-deferred-conj-demand': {'row_sha256': '125ce8119c13e9c5eb71840ecee322f141cd3dda98601e994bfeee2ab37bfed8', 'original_row_sha256': '80d0e8f1c85ea72a0092a6e7a05239c02aba800653fbfc5f4a2bea4de17a33b6', 'source_sha256': 'a03e93973a407ce7bcf30fd19e9af1265fc680d0f855092771f48d11dbb30abe'}, 'sci-lazy-partition-all-group-effect': {'row_sha256': '734be653fa299d18ae13853fed14af7da0fa50eb427920819272753450658f60', 'original_row_sha256': 'd4240291a7be340c13604aa9e54c84d1d8516a2a5ef3fcb172517e19da5dd167', 'source_sha256': '517e54bd7de9cab07e1a065c7f335919068fead2f98e020a8eaf9da09e3f87eb'}, 'sci-lazy-counts': {'row_sha256': '1b4ef570175bd18a61af1d3e5f077bce2a792d8108a9e78f8c81bb1f4fa94e80', 'original_row_sha256': '0e1c9f6d74c83ccd2698ad2fe929435e9d5e8496b065b45ba3baebac8d568c0b', 'source_sha256': '2cab35896ce52839f060942f4a7e4b1e7b3fc5ed8b5ac79feba80ed055722543'}, 'sci-lazy-probe-count-fracs': {'row_sha256': 'd478102cd88cf81f2d540e681f9555d36522da9c24a02c791abd8c3c9c3cb7a0', 'original_row_sha256': '7ddea80f6eb61cfe3e35bfbd3d80f12f8dddb11d7a62cc4d73400b4b636d673f', 'source_sha256': 'fdd3368bd2f64a6c3266ee31a403608cc09803e64d5595ebd4e9bdd456192db3'}, 'sci-lazy-probe-range-nan': {'row_sha256': '6e557e8af71ee5dea2ca439d6b467218e7a8d7fc3068139b826f4efc773c1473', 'original_row_sha256': '609c906341a88d199adbe85c95ae7b86c1c6ea2a4f8c9871c3d08ae134948f02', 'source_sha256': 'c5f35c1450cf16b3074e7ecf397ad3069d7a160a6b7d35bc5aa1131e8b4c02d1'}, 'sci-lazy-probe-partition-demand': {'row_sha256': '6289cfbbffb9a35cea29b189ce007fdc2f29183d56b351bba8ca89226cb50727', 'original_row_sha256': 'e9a203a4070034b1cee159f91b315f0932d733327f48c8d7c68893a8886744af', 'source_sha256': '4d44752020485455950d49b12bcb21185a1bec1d5f12c3c12686dd74a836e209'}, 'sci-lazy-probe-repeatedly-bad-count': {'row_sha256': '3fd853d5be73082dbe60c2a2849e565ef91e9248bd45c01c0e4dc2ec05fdef89', 'original_row_sha256': '730ba63bcda771f5fedc779a57ff62b2550f1ffa655f3c4bc387f8d946180e0d', 'source_sha256': '9f1dcbcfdecd851a696c20760ef352cb8cd172ca4b68afc5a617b10579216789'}, 'sci-lazy-probe-partition-all-lookahead': {'row_sha256': '916691746efc2803fbf51d2af6852f1a653b4a691d901da174310efc221a92ec', 'original_row_sha256': 'c7d96b54a9f84ad8b146aabe46a522beaa3e15ba35bc2f7a6611362dde16b0ff', 'source_sha256': 'e4194aaa2d0d6385a918a27d4288484e3741a295d2a94910c751c54f13dbd57b'}, 'sci-lazy-probe-partition-step-lookahead': {'row_sha256': '431fe1adc9d0acf7f9cf3a29751d7bd93460f255d25b887f203c9a265b1532fe', 'original_row_sha256': 'e9bc8ce011596a1c298d79373b660122f6170caa40fb06d985860616ed478b04', 'source_sha256': 'eae0c5397dd6b0c68f03e6048a78c043fa890164896710d588236416425c7c4c'}, 'sci-lazy-probe-nth-traversal-empty': {'row_sha256': '50211c994d11ec5fd0441076e601721bc3bb65db660724a48b0a6be7a9042e5c', 'original_row_sha256': '688661402d7945a41c366c19dec564ad697db3c93b1ef68a3fddae854c646e1b', 'source_sha256': '01459942cbe2b7ef78978c061455ec8fb0e755e969aec5a8b2cbcfbccd9ae5b6'}, 'sci-lazy-probe-nthrest-effects': {'row_sha256': '49ef20ffe02d617c52655d40ed5fc71719288cd0260f4aaf7dc268c85ae13f62', 'original_row_sha256': 'ccdce4f93e4f46983baec45fc0dbb4139e802e2687e4ae80b1ad7236711e1641', 'source_sha256': 'db02a723abd7f52f15e7d938c69099a385097afc1c0449838cb4128ed186867b'}, 'sci-lazy-probe-partition-n-intdouble': {'row_sha256': '995831116cd20f1cd748c54ff23f566d8a543df50475edb7acc75f40fb4bb1de', 'original_row_sha256': 'c38059b66f3598955c5076236418020572af556c77ac406786b74902d9bd539c', 'source_sha256': '7561466854f84bd8191455e642c249feeec41c62ecd79efc5432bb7eb4d33598'}, 'sci-iteration-doseq-zero-generators': {'row_sha256': 'bd7e7a350adca588357c2d6b758e1e0d8ca031a061de421377d673fe05fa6fe9', 'original_row_sha256': '005fabfb0936b7d8457d184e19ce9cddbec4f41aa2aab3ba22448c2ea68523e8', 'source_sha256': '70c62549614dcfbe4ecc9d1d648a08b746d8069e3ff29087ac1953ed006bb72f'}, 'sci-iteration-doseq-prefix-let': {'row_sha256': 'd4636fc1e473ecdce5caf71bf6be3c2aad839c5a8110e217961dced9d5cf5ecc', 'original_row_sha256': 'a6b47f1da492b67f88de91f540f3ad0f271b2fe1532d83ef0464b8d5bd56b264', 'source_sha256': '441f382ed4822f233bc529b69fce8edd84c0bb077058b2c37ba142bf160c7eb5'}, 'sci-iteration-dotimes-fraction': {'row_sha256': 'f07a56fa3f5dae6539cf5401e50f94e9b63d655807876df497e1c4fc12717c6f', 'original_row_sha256': '52b36420abe0925b6c3a19ac248f7fa78343de2add23a868ad1f24b9462d4e86', 'source_sha256': '953f590524fa3cf1d8f0fdf644d7ebe3c6ef4371dfb01c370d632b18f897c339'}, 'sci-iteration-dotimes-char': {'row_sha256': '467debec9a0dc5425868ba2d254943c29590b71888f20e7cfb86a64c0acc24ae', 'original_row_sha256': '636eb3f82ba4b2997b441d5a772385f0d4946f71cb8f02416381437493d6e890', 'source_sha256': '3ccdf3a9d8e146e647b90662eea192888122a76ac2275e2260672cb59ef1af05'}, 'sci-control-case-map': {'row_sha256': '4176a50490a14126e255a9181ad7e0a4af59c10e3656e2a0ffed66ca642dcd51', 'original_row_sha256': '75be93c4de6418a30bb59a170760a8a2b4974def1e564288ac350469a7c0ae73', 'source_sha256': 'a17bd524f827f3d95dcba9f90756b3558eccb4e42f0b02aeecd3cadace1d2d3f'}, 'sci-control-case-set': {'row_sha256': 'b039df80ca3b9d39b1f92a15bff8f234d470176ab54f085547d1d3418c4af1b5', 'original_row_sha256': 'ecb32e0b5617f72c379f7c2c9308338951ed977217c41afcdfa2e3fc4b961263', 'source_sha256': 'b2bbe04f17eb282c1ce6bf625570f8b22b4620ac813afc468762bd8a8d2e0ee6'}}

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
    paths += [Path(__file__), Path(__file__).with_name('sci_lazy_context.py'),
        Path(__file__).with_name('test_sci_lazy_context.py'), Path(__file__).with_name('sci-lazy-canonical-contracts.jsonl'),
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
    result = {'schema_version': 1, 'scope': 'Exact 21 unchanged historical lazy/iteration/control returned candidates',
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
