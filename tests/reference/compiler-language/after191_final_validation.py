import hashlib,json,re,subprocess,sys
from pathlib import Path
sys.path.insert(0,'tests/reference/compiler-language')
import arity_after191_proof as arity,sci_originals_after191 as sci,cljc_rejections_after191 as reader
root=Path('tests/reference/compiler-language')
def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def doc(name):return json.loads((root/name).read_text())
a=doc('arity-after191-context-receipt.json');s=doc('sci-originals-after191-receipt.json');r=doc('cljc-rejections-after191-context-receipt.json')
assert a['frozen_source_snapshot']==arity.frozen_snapshot()
assert a['installed_loader_state']['ordinary_loader']==a['installed_loader_state']['phase_loader']=='passed-exact-own-cohort'
assert s['compiler_build_producer_source_sha256']==sci.snapshot()
assert len(s['source_bindings'])==len(s['frames'])==31
assert sum(len(v) for v in s['frames'].values())==248
assert sum(key.startswith('macro-') for v in s['frames'].values() for key in v)==124
assert all(frame['status']=='passed' for v in s['frames'].values() for frame in v.values())
assert len(a['actual_frames'])==184 and len(a['actual_input_hash_frames'])==92
assert len(a['module_source_emitted_js_and_raw_sha256'])==647
assert r['source_snapshot']==reader.snapshot()
initial=doc('after191-initial-gate-rejected-receipt.json')
independent={}
for name in ('full-inventory.jsonl','full-bodies.jsonl','obligations.jsonl','support-sources.jsonl'):
 assert digest(root/name)==initial['frozen_start']['producer_sha256'][name]
 independent[name]=digest(root/name)
logs={}
for name,path,count in [('report','/tmp/light-cljs-125-after191-report-guards.log',42),('full_ledger','/tmp/light-cljs-125-after191-full-ledger-guards.log',22),('arity','/tmp/light-cljs-125-arity-after191-integrated/guards.log',20)]:
 raw=Path(path).read_text();assert re.search(r'Ran '+str(count)+r' tests in [\d.]+s',raw);assert raw.rstrip().endswith('OK');assert 'skipped=' not in raw
 logs[name]={'path':path,'sha256':digest(path),'tests':count,'skipped':0,'log':raw}
clippy=Path('/tmp/light-cljs-125-after191-final-clippy.log').read_text();assert 'Finished' in clippy and 'warning:' not in clippy and 'error:' not in clippy
raw=doc('arity-after191-raw-guard-receipt.json');assert len(raw['controls'])==17 and raw['restored_pristine_replay']=='passed'
assert all(c['status']=='rejected' for c in raw['controls'])
integer=doc('integer_predicate_after191_raw_controls.json');assert len(integer['controls'])==15
sci_guard=doc('sci-originals-after191-final-validation-receipt.json')
assert sci_guard['tests']==6 and sci_guard['skips']==0 and sci_guard['exit_code']==0
assert doc('cljc-rejections-after191-validation-receipt.json')['tests']==5
proofs={name:digest(root/name) for name in ['arity-after191-context-receipt.json','arity-after191-raw-guard-receipt.json','sci-originals-after191-receipt.json','sci-originals-after191-final-validation-receipt.json','sci-originals-after191-integration-receipt.json','sci-originals-after191-final-binding-after.json','cljc-rejections-after191-context-receipt.json','cljc-rejections-after191-raw-guard-receipt.json','cljc-rejections-after191-validation-receipt.json','cljc-rejections-after191-selected-integration-receipt.json','cljc-rejections-after191-fresh-30-prior-receipt.json','integer_predicate_after191_receipt.json','integer_predicate_after191_raw_controls.json','after191-installed-ledger-receipt.json','after191-initial-gate-rejected-receipt.json']}
rust=set(subprocess.check_output(['git','ls-files','*.rs'],text=True).splitlines())
rust.update(str(p) for p in Path('tests').glob('*after191*.rs'))
validation={'schema_version':1,'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'scope':'Current independent evidence checkpoint; no production repair or comprehensive completion claim','current_source_producer_bindings':'arity, SCI and negative-source frozen bindings exactly match current files','four_independent_full_ledger_files':independent,'installed_131_pairs':a['installed_loader_state'],'cohort_complete_arity_originals':23,'actually_linked_reference_cases':16,'full_original_sci_sources':31,'actual_sci_frames':248,'actual_macro_sci_frames':124,'reader_source_rejections':16,'reader_same_host_recovery_frames':32,'canonical_invalid_reader_ids':3,'canonical_reader_original_execution_credit':0,'exact_integer_diagnostic_local_comparisons':112,'exact_integer_diagnostic_local_failures':14,'integer_public_predicate_focused_owner':'light-cljs-192','proof_receipt_sha256':proofs,'permanent_python_guards':95,'base_and_arity_guard_logs':logs,'reader_guard_validation':doc('cljc-rejections-after191-validation-receipt.json'),'sci_guard_validation':doc('sci-originals-after191-final-validation-receipt.json'),'arity_restored_raw_controls':17,'integer_restored_raw_controls':15,'selected_reader_accounting_controls':7,'clippy':{'command':['cargo','clippy','--locked','--all-targets','--','-D','warnings'],'environment':{'PYTHONDONTWRITEBYTECODE':'1'},'exit_code':0,'log':clippy,'log_sha256':digest('/tmp/light-cljs-125-after191-final-clippy.log')},'rust_input_sha256':{p:digest(p) for p in sorted(rust)},'full_gate':'Initial exact command failed closed before execution; no current comprehensive counts claimed','performance':'No production changes or new performance run claimed','strict_complete':False,'source_strict_complete':False,'156':'Externally blocked after two automatic approval review rejections; no retry/delegation/native-crash work/cancellation or resource-limit tests','owned_bytecode_byproduct_history':['/tmp/light-cljs-125-after191-owned-bytecode-history/manifest.json','/tmp/light-cljs-125-after191-owned-untracked-bytecode-history/manifest.json']}
import arity_after191_discovery as discovery
original=next(row for row in discovery.rows() if row['original_id']=='extension-direct-2')
validation['whitespace_check']={'command':['git','diff','--cached','--check'],'retained_original_source_exception':{'path':original['fixture'],'source_sha256':original['source_sha256'],'original_row_sha256':original['original_row_sha256'],'reason':'The complete original zero-credit native-array source intentionally retains its exact trailing spaces; do not edit originals to silence whitespace checks.'},'all_other_staged_changes':'passed'}
validation['collector']={'command':['python3','-B',str(Path(__file__))],'environment':{'PYTHONDONTWRITEBYTECODE':'1'},'source_sha256':digest(__file__)}
(root/'after191-integration-validation-receipt.json').write_text(json.dumps(validation,indent=2)+'\n')
print('Fresh exact source bindings, immutable original ledgers, complete proofs and guard logs validated.')
