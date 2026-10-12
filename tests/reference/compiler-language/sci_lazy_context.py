"""Exact unchanged SCI lazy/iteration/control sources and literal canonical differences.

The complete original expressions, ordered effects and SCI expectations remain
immutable. Positive evidence requires fresh both-pin and demanded macro proof.
"""
import argparse
import json
from pathlib import Path
import sys
import sci_execution as sci
import sci_lazy_candidates as candidates
import sci_gap_canonical_context as proof

PROFILE = 'exact-historical-lazy-iteration-control-canonical-context-v1'
CONTRACTS = Path(__file__).with_name('sci-lazy-canonical-contracts.jsonl')


def load():
    originals = {row['sci_id']: row for row in candidates.load()}
    contracts = [json.loads(line) for line in CONTRACTS.read_text().splitlines()]
    proof.same([r['sci_id'] for r in contracts], list(originals), 'Missing/duplicate/reordered exact lazy/iteration/control contracts')
    if len(contracts) != 21:
        raise ValueError('The reviewed lazy/iteration/control cohort must retain twenty-one originals')
    result = []
    for contract in contracts:
        original = originals[contract['sci_id']]
        for key in ('source_sha256', 'original_row_sha256', 'body_sha256'):
            proof.same(contract[key], original[key], 'Original lazy/iteration/control source/row/disposition binding drift')
        canonical = contract['canonical_expected']
        if (canonical['id'] != original['id'] or set(canonical) != {'id', 'outcome', 'trace', 'stdout'}
                or not isinstance(canonical['trace'], list) or not isinstance(canonical['stdout'], str)
                or not contract.get('scope_basis') or not contract.get('reason')):
            raise ValueError('Incomplete literal canonical lazy/iteration/control/effect/scope contract')
        result.append({**original, 'original_expected': original['expected'],
            'canonical_expected': canonical, 'scope_basis': contract['scope_basis'],
            'reason': contract['reason'], 'focused_owner': None,
            'source_context_profile': PROFILE, 'original_semantic_pass_credit': 0,
            'original_expectation_status': 'unresolved'})
    return result


def snapshot():
    paths = [Path(__file__), Path(candidates.__file__), CONTRACTS,
             Path(__file__).with_name('test_sci_lazy_context.py')]
    return {**proof.snapshot(), **{str(p.relative_to(sci.ROOT)): sci.digest(p.read_bytes()) for p in paths}}


def run(directory, timeout=600):
    return proof.run(directory, timeout, cohort=sys.modules[__name__])


def reconcile(document):
    return proof.reconcile(document, cohort=sys.modules[__name__])


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--timeout', type=int, default=600)
    args = parser.parse_args()
    run(args.output, args.timeout)
