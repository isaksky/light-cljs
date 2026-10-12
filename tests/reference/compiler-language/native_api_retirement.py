"""Exact five historical native-number API rows; no numeric-suite exclusion.

This records the already explicit group boundary. It never supplies semantic
pass evidence and never modifies an original historical row or diagnostic.
"""
from pathlib import Path
import sci_execution as sci

PATH = 'tests/reference/sci/historical/supported-apis.tsv'
BASIS = ('User explicitly excludes native exact integer/ratio/BigDecimal macro behavior, '
         'JVM/GraalVM execution, .clj macro companions, upstream self-hosted compiler '
         'and unchanged application/library compatibility. Native arbitrary-precision '
         '-48 is retired. Explicit JS BigInt interop is an engine operation, not a newly '
         'promised Clojure numeric tower or form-return type.')
REVIEWED = {
    'clojure.core/bigdec': '609f5654cc259cac6fdf7c2f6e8c1e313dd505b5c9de58d9a388c236c7747d64',
    'clojure.core/bigint': 'afd3e196b68f7defffefffaa67559e8c63acdc5e02024895cdecc2afdc791cf2',
    'clojure.core/biginteger': '12c823959df57c35b4d3f24dd22a947c6c2a78956ee0293239800e96c134da01',
    'clojure.core/ratio?': 'c8d46a9de8488dc346281cd7b5760e8d608416b10baa2ef2cf69beef6d8fa751',
    'clojure.core/rational?': '8d060e3bece922bf6d679e5553abd5531dbe20a5c00102a903d152beeda40abe',
}


def review(path, line):
    """Return a precise dialect disposition, or None outside these five rows."""
    name = line.split('\t')[0]
    if path != PATH or name not in REVIEWED:
        return None
    if sci.digest(line) != REVIEWED[name]:
        raise ValueError('Historical native numeric API row drift; renewed scope review required')
    return {'id': 'local::'+PATH+'::'+name, 'source': PATH, 'name': name,
        'body_sha256': REVIEWED[name], 'disposition': 'dialect',
        'reason': 'Exact historical native numeric tower API intentionally retired by the adopted JavaScript Number boundary; original registration and nonpassing compiler diagnostics retained.',
        'scope_basis': BASIS, 'semantic_pass_credit': False}


def load():
    rows = [review(PATH, line) for line in (sci.ROOT/PATH).read_text().splitlines()]
    result = [row for row in rows if row is not None]
    if [r['name'] for r in result] != list(REVIEWED):
        raise ValueError('Missing/duplicate/reordered exact historical native numeric API rows')
    return result


def validate_diagnostics(document, binary):
    """Check retained current rejections without turning them into a pass."""
    binary = Path(binary).resolve()
    if document.get('compiler_binary_sha256') != sci.digest(binary.read_bytes()):
        raise ValueError('Retained diagnostic compiler binary drift')
    rows = document.get('rows', [])
    if [r.get('id') for r in rows] != [r['id'] for r in load()]:
        raise ValueError('Missing/duplicate/reordered original native API diagnostics')
    for row, binding in zip(rows, load()):
        name = binding['name'].split('/')[1]; end = len(name)+1
        source = '('+name+' 1)'
        stderr = ('WARNING: Use of undeclared Var cljs.user/'+name+
            ' [undeclared-var] (source 0, bytes 1..'+str(end)+')\n'+
            'light-cljs: 1:2: unresolved symbol: '+name+' (bytes 1..'+str(end)+')\n')
        expected = {'id': binding['id'], 'historical_row_sha256': binding['body_sha256'],
            'source': source, 'command': [str(binary), '--compile', '--text', source],
            'exit_code': 1, 'stdout': '', 'stderr': stderr,
            'status': 'rejected', 'semantic_pass_credit': False}
        if not sci.report_module().same_json(row, expected):
            raise ValueError('Historical native API unresolved diagnostic changed')
    return {'status': 'retained-nonpassing', 'semantic_pass_credit': False, 'rows': rows}
