#!/usr/bin/env python3
"""Fresh unchanged historical gap-return candidates; no shared credit.

A historical nonpassing disposition cannot prove the present source's behavior.
Every original row and expectation is preserved, including disagreements. These
ordinary observations alone cannot establish complete phase acceptance.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import sci_execution as sci

REVIEWED = {'sci-coll-contains-value': {'row_sha256': '9f0c1999800d4f2e01fae9e314660805f5aab7f1df4d108c309cf7586e38daca', 'original_row_sha256': 'e5aaa8e1c3d85b54f0f75dfcb1f2c8e2afe35ad7c17a591c931aec25b5d49692', 'source_sha256': 'c4bfc770923fd09c32986e4866c974e74610a87800df7b89ad11c8c24ff489b0'}, 'sci-coll-list-star-kinds': {'row_sha256': 'fe5a3ef8a017b5dc277a987d896e38e19662b5133daf55b768e184609843f2af', 'original_row_sha256': '04f5abf123e8adf1877f82ef969095c0d2753cb3441c11ee8cf0b9b739df22d4', 'source_sha256': '94fdd07c7e472caf01e9314407d3a65ea0ebd0de5e32f1531f73b20b6b0915b2'}, 'sci-coll-conj-seq': {'row_sha256': 'ca89fe324bc078f60083d3ad1d63fe2b4f313d0713c07c53bf72c0850d1eaf8f', 'original_row_sha256': '7a88e209fe45dbb356db7e166e78620e4c4a81a8b4d6aeaa18f69edd2b6e1cf5', 'source_sha256': '0f38ffa77187c0d6396a7220b83ca59bff456d7a3bb4c9eed2f7c7d716274c6c'}, 'sci-coll-predicates': {'row_sha256': '607fef3717c445d7c99eb04250bb5581a3b0ec954741aa3f7ff078a3d48c45ba', 'original_row_sha256': 'f89dbb2f73aeb5d9d6f659d70527aeb55f5beb65078891ca6801980dabdd23d2', 'source_sha256': '0ffd94bdd10d1cdec647f930911bbc66d56e8d975e672810bcb271699c590a98'}, 'sci-coll-predicate-nil': {'row_sha256': 'b763a7dd12273f1b2f52a3e17355eab2a3ea3fad917e1c32e70c03c0936f6197', 'original_row_sha256': 'e2377e249fb0cad823ea6cbc5f42def890192b97bfdebedb0daa90dc39968135', 'source_sha256': '3d6568ef20b1bfe2f760f07846285633b206f99e683c150edf415affa0c70bc7'}, 'sci-coll-lookup-bounds': {'row_sha256': 'd313fc92731adf7ac1dd4f5837df90bfd12e8a89826e2a910ad8379ed13ddb8a', 'original_row_sha256': '74ba91283a3f9758eb22485a89fcf880d90bae6a08912dad55efed8079265295', 'source_sha256': '807c19a795c42d97460ff7cd2540450bb3a4da09aacfdd2d7ff5e724bb772172'}, 'sci-coll-list-star-empty-tails': {'row_sha256': '68c049a8e255be6387422805391c3bdbf457434c56cb79a2bd19b47fc92b3f43', 'original_row_sha256': 'ae3db54c0df19dde99ca2c6ab6d68d913e7610956ff918c2521c410dd206df6f', 'source_sha256': '78e373a7d8400925cd8febfce97e0664989949a35a65178e322748cde07ddff1'}, 'sci-coll-index-narrowing': {'row_sha256': 'eda9fd9ee26f14371572593c45c64e1386d7f8916ac21fc640bd0cf6c2158ae9', 'original_row_sha256': '9cee97a5c727ccf7e89078a8c5c38945fcb51af15bcbf8d758ad5a3eff351f0a', 'source_sha256': '5e4fce10678764f9aa84776a8e4d97c861d09c1a355de383b26a1338222e19bb'}, 'sci-coll-string-numeric-indices': {'row_sha256': '1f9f12f2a3483ba0f109694fa70f8fe0381487039f987961d394e5679f053060', 'original_row_sha256': '514723b9f62fdc1a43aa2059df9b1330dfb20c04b9e17125670832c00b8f68b6', 'source_sha256': 'f4f82e99fd2635cf456aaa80d8ee43fee887467ffe2a3ec6d36eb252c6c4aea1'}, 'sci-coll-nth-fringe': {'row_sha256': '8840ccfc6107ebc8ea3a3b3a87cf507386089f89ec16789a0d5a997c2bda23e3', 'original_row_sha256': '5dba8b838fddd6ca99a74736cfb8ad937174f306ea5f468e296e6dadf71b8759', 'source_sha256': '52b74ef7c6018c51f4d8b37e0e2d3c966e710392a2c8e5079cfb75502cb03516'}, 'sci-coll-select-keys-original-key': {'row_sha256': '36b62a584b238fb55482683c8d691d5ca38ddf4ab5da2d05e350bb6fc25ec2cd', 'original_row_sha256': '82a0054609bef91983079c7b3d5046ac45161389a28293a05dd62b680730c58e', 'source_sha256': '361d1da26c60f814a4a1b2cdbeada981a8737251d4440fb7c8a75afb352deadd'}, 'sci-seq-string-lone': {'row_sha256': 'bdb7a1009114cba16867865f0edfd3f1673550d5d6ffb92039a879570d6bc129', 'original_row_sha256': 'ee67ef64b81d8d133ac3f7a7efe7de3b2ae7d209afe5d888d92eafa2907117a5', 'source_sha256': '27f7f2d6d6103a8006751e80e0613d04af82849ca0b0cd863457f2e7af8dbe80'}, 'sci-seq-cons-kinds': {'row_sha256': '68f73a8b0b8c2eedcc10d59e7eab24be26951242233b8b77f84f82a184d7ed69', 'original_row_sha256': '71fddddfc84e038251c1dba4d60a11aa26ee9d11c7b5467b15fec45a627be014', 'source_sha256': '3ac0c3841ef46a6068c994db2d99dd4852f0be5b1dbe256152c8a28d0b5c1f06'}, 'sci-seq-reduce-utf16': {'row_sha256': '55ac34031764e3b15cfa9be1e4bf9111a5e509d08c9ea0f9cfc5acaee0278898', 'original_row_sha256': 'ae8c9939347d86102c87f4aa9950e0db52d3876d8c2316ee14b5629c4d71c692', 'source_sha256': '4e372dcf7843077eb90db92c177ccbf02368ee070c056d32b1430f7ac97cc57a'}, 'sci-seq-reduce-kv-nested-entry': {'row_sha256': 'ce8f84a70f402dba4c537fdd7d88dbbb25c6518a51a4745e53144cc00ed28532', 'original_row_sha256': '1b978471ffb1985b01ba88000ddb83bfb9b70e7db25e1e4d3300a94e1d9710d0', 'source_sha256': 'dc98b9aef1ab00352258d9247014772b60c45f9cb240485313a0bffc18c84f37'}, 'sci-seq-apply-constant': {'row_sha256': '0780838990f05ca3c39cef7a2dbf7c81f26f39b2a157a15c8c58731b297b2418', 'original_row_sha256': 'c9d0bee0a264bf3886a8bae9889cdc3e5c827a9b9ff03218e872bea98914b15f', 'source_sha256': '2a042e5cf9aa1936ac1af5cf6aeda3850f60c35ffe17f53749a917f9e76ad379'}, 'sci-seq-combinator-basic': {'row_sha256': '853ab3b00a5569796e0ea9b3823ec82e735bd869b391dddf34810c8ba751c844', 'original_row_sha256': '093c4925fa83f510dfe5611adc3f52b214ee8970d80f83ae1189c8dab05a28d4', 'source_sha256': '29c1261265d54e8902447ff361093e3bc3c71c2408ce6df157446dbefbd23f5e'}, 'sci-seq-apply-partial-constructor': {'row_sha256': '6e156608128c69d8a6d5f981d0bdadf7eab93fbde8ba593aa2fcd7a3c7e23d6c', 'original_row_sha256': 'd7403794c79b5a9eb5999a92ceae586725aee0132dd2c554ad79486fa61d2a29', 'source_sha256': '520ef7ab3bb846c0633742dc225a096a7f73467ee39e3c75fb3ea44d03ef68e0'}, 'sci-seq-apply-mapv-empty': {'row_sha256': '4d74c7bf9c80509472c346ea2f383caed1f5981d3a70e418811692a4f7ce2e7b', 'original_row_sha256': '73a201bf60f8ee89a8037528864be6e3f22cd6083c97d1b3f8387cc90e79a4d6', 'source_sha256': 'e0a907824be6de52f6110387000b5a50bdc2a73524df8dfe1edb88d55f121303'}, 'sci-seq-apply-partial-concat-demand': {'row_sha256': 'c1ca03e24fea5c9f2ecd786ec39e6a10dc2a6b7845564a94ccd2d5e8bab16fb0', 'original_row_sha256': 'a8184d4ae72e555d9b6645ba25a3b8599f2bf855fa9e71e4c62aab0cfd5851de', 'source_sha256': 'e8a2ba6b83dcd54dc321805c2b658cc04893c05438c56736dc4a4aeb85797234'}, 'sci-seq-apply-partial-bounded-one': {'row_sha256': '4930350e3430461f0992c93a06c2df2ac6963fa9c3000831db81c3f5f73b4081', 'original_row_sha256': '82d506a530a9235a09ae42b09fa7e914f3a2e979ea062cc96bf1c30f685c764a', 'source_sha256': '94e3c7845d41d4c5fd05840574cebf65f72b48c970f59764a04a152ff0112608'}, 'sci-state-cas-identity': {'row_sha256': '55b4f8ea3b324772bef3d2a4bdfea98aa939c652dc6566ec323d4e1a9539bc6a', 'original_row_sha256': 'e7da47de1d8ee22b032465d7ef93393190910fab35e55a42f241ca2f1efde798', 'source_sha256': 'e6ede60ef8c582ef70b848d9754c2cde25210ce3692949393c93ed57d28d48c5'}, 'sci-state-cas-strings': {'row_sha256': '0c6e0d432d24c28cf505b752dc581cb3d15bcd05b6540930178a9795f6853c24', 'original_row_sha256': '0432c4598b19a9fb4d11aefbf85f9aca60b534d860282844e9897098fc010afe', 'source_sha256': '730da316451881869ca7033cccb25935a30eccbeb8e9d16b4998765cf29cdb13'}, 'sci-state-cell-predicates': {'row_sha256': 'c73c45c8810f552359ef07b3a3bd6e8d519988bf2ccee44580a18b96126d07d0', 'original_row_sha256': '51df92a98863ca43af4cd44d574dd5b6fe2c1d45f56126e63362c7edcd3c8126', 'source_sha256': 'f4502cb1b6078cb42bb3041b95e6797dbb2c3c507e123a6a3ad8c48da10e29f9'}, 'sci-state-atom-meta-options': {'row_sha256': 'b725272d94e0b1ec68a86d4589015cd1e779d1643306ba2864b3f683b8ed81aa', 'original_row_sha256': 'e2f2f81acb93e259eab7ea29174df478933378e37bb94f2008b293548c47750a', 'source_sha256': '720e7631b581c6268be676d30be791e9ee031a49334a52659a63f8642598c8fc'}, 'sci-state-watch-nil': {'row_sha256': '2a9eb2a0b39c26c8f972851f84d475df8d0e1e1b44ddd69244e554f8102d61b2', 'original_row_sha256': 'ac26a770ff6f74abe8fb3c7986626091300a310bc5e2eb6e53ca023ae3af177c', 'source_sha256': '42c7381feded22b585f8f1ae4e62f65ec3849adfb14f4b6442aeeb9a177ee313'}, 'sci-state-swap-retry': {'row_sha256': 'dc2a2241fe4f2981c67e95d5caa4d14280cf90c218015273cbd1a18632e1bb61', 'original_row_sha256': 'b3e2bba50bf7c50f02c854737e562da63fc0a65c6b3fea7c13426b5148a0b6cc', 'source_sha256': 'a38e0bb17c17f08eac6731889192613c72f7b470b70f7d0a1c1342470290d0e3'}, 'sci-state-swap-vals-retry': {'row_sha256': '8986a0b20d8039e9040026e8f4d54674ae9a56c3298a6b05b2f8902cee50724b', 'original_row_sha256': '8b4c5a2c95ac1b391e7ae7be9b627a41ac73cedff3fce27abb71a930ecf0eafb', 'source_sha256': '012ab06cb3acc43f2ca4f0202198810fbd46ee17c1a519103a2dfc9cd36b5f2f'}, 'sci-state-meta-independent': {'row_sha256': 'c17500b8690f1b5b486332a715f35514e7c8dfca81cea32604b5840bda33fb88', 'original_row_sha256': '35e206cab539e4454226ae5ff715084fb121fdd068e92325fd221a53024f68ff', 'source_sha256': '30b44c60896d561269d702e2e8de4aad50d8c2a48ab855ae814dd09829c4be28'}, 'sci-state-validator-reentry-reset': {'row_sha256': '24a728166206f2b5a20a818738a3706fe8fa2fcec7e63f7fd69338f3de20c9bd', 'original_row_sha256': '124909c68f363f3a9bae7b15ccf790afe522ff51185b58f06aea630a605cb951', 'source_sha256': '9403276a896e9518b44eef1768af44cd60f78dbb5478fb0e16b1273c228c6f98'}, 'sci-state-validator-reentry-reset-vals': {'row_sha256': '8a172fd2dc15cad2021aadda4ead690b49ecdad039c8de4d953b3e5db55fabb7', 'original_row_sha256': '11e1c8fe84cadd2ca519e090e8a03c52e38e3ddb261b135b7f9e3075ec0c80d3', 'source_sha256': '02b82d71464b29de9852c049615e6813feab6d9f8929f39af787bfc247155773'}, 'sci-state-validator-reentry-swap': {'row_sha256': '648edc7dce7eddec75cc491e40024c5f4975332c1445d04ea6d99b1c03a3427d', 'original_row_sha256': '4b040dbdbc9b908a59342647548e66909174b35ce9a08d459c175000545a301b', 'source_sha256': 'a4ae8c3ba76fbfbdf96781f8b1c7ac257a1b04b5a9889678327f5d3100005537'}, 'sci-state-validator-reentry-swap-vals': {'row_sha256': '6c522925283af6f768400540a47cb78b421d88946d3fb805be24c8a8e9e5f751', 'original_row_sha256': '7b7bb4dab01845f134f1c7765960d773e6e2cb94b39a0176505c36bfe424e1c0', 'source_sha256': 'd538e9ea8d1813df0221adff9927e4ab472726c2a74d53d9ac586fe337c12952'}, 'sci-state-validator-reentry-cas': {'row_sha256': '54c5d47ef759e2b5665fe22b8ca8cc1555508893e015912afc3d4f5943b2df93', 'original_row_sha256': 'a75f4f59d232a3b4ab67fd2a7151fe30251b0d2a7626bc518acf63d978e79134', 'source_sha256': '969ffb722d2dbe27bec1bc1fa036e77e4ffc775064036bbdbe504cf643615f03'}, 'sci-state-watch-key-remove-nil': {'row_sha256': '25cdc9e5af4d6e985dddff4f2448d0429f4aaea41855b6f4e5010cf57d942d86', 'original_row_sha256': 'b16a81daad3dd51d213dbe579117cfe2fc58d382efa2b0d8576d5f8abf5dcadd', 'source_sha256': '12c9352edf5e727173dbae03d2b88ac500f94b8e6b673dc121c96bfb46bbf3e6'}}

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
    paths += [Path(__file__), sci.CORPUS, sci.DISPOSITIONS, Path(sci.__file__), sci.HERE/'report.py',
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
    result = {'schema_version': 1, 'scope': 'Exact 35 unchanged historical collection/sequence/state returned candidates',
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
