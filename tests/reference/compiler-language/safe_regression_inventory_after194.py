#!/usr/bin/env python3
"""Read-only complete local regression inventory and exact safe-run proposal.

This helper never executes tests. Rust test names come from a completed Cargo
list-only log; current full function bodies supply exclusions and provenance.
"""
from pathlib import Path
import argparse,hashlib,json,re
ROOT=Path(__file__).resolve().parents[3]
HERE=ROOT/'tests/reference/compiler-language'

def digest(data):return hashlib.sha256(data if isinstance(data,bytes)else data.encode()).hexdigest()

def sanitized(source):
    out=list(source);i=0
    raw_pattern=re.compile(r'(?:br|r)(#{0,255})"')
    char_pattern=re.compile(r"'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^'\n\\])'")
    def blank(start,end):
        for k in range(start,end):
            if out[k]!='\n':out[k]=' '
    while i<len(source):
        start=i
        if source.startswith('//',i):
            end=source.find('\n',i);i=len(source)if end<0 else end;blank(start,i);continue
        if source.startswith('/*',i):
            depth=1;i+=2
            while i<len(source)and depth:
                if source.startswith('/*',i):depth+=1;i+=2
                elif source.startswith('*/',i):depth-=1;i+=2
                else:i+=1
            blank(start,i);continue
        raw=raw_pattern.match(source,i)
        if raw:
            terminator='"'+raw.group(1);end=source.find(terminator,i+len(raw.group()));i=len(source)if end<0 else end+len(terminator);blank(start,i);continue
        if source[i]=='"':
            i+=1
            while i<len(source):
                if source[i]=='\\':i+=2
                elif source[i]=='"':i+=1;break
                else:i+=1
            blank(start,i);continue
        char=char_pattern.match(source,i)
        if char:i+=len(char.group());blank(start,i);continue
        i+=1
    return ''.join(out)


def matching(code,start,opening='{',closing='}'):
    depth=0
    for i in range(start,len(code)):
        if code[i]==opening:depth+=1
        elif code[i]==closing:
            depth-=1
            if not depth:return i
    raise ValueError('unbalanced inspected Rust source')


def functions(path):
    original=path.read_text();code=sanitized(original);modules=[]
    for match in re.finditer(r'\bmod\s+([A-Za-z_]\w*)\s*\{',code):
        start=match.end()-1;modules.append((start,matching(code,start),match.group(1)))
    result=[]
    for match in re.finditer(r'\bfn\s+([A-Za-z_]\w*)\s*\(',code):
        end=match.start();attrs=[]
        while True:
            j=end-1
            while j>=0 and code[j].isspace():j-=1
            if j<0 or code[j]!=']':break
            depth=1;k=j-1
            while k>=0 and depth:
                if code[k]==']':depth+=1
                elif code[k]=='[':depth-=1
                k-=1
            if k<0 or code[k]!='#':break
            attrs.append(code[k:j+1]);end=k
        if not any(re.search(r'\[\s*test\s*\]',a)for a in attrs):continue
        start=code.find('{',match.end());finish=matching(code,start)
        nested=[name for a,b,name in modules if a<match.start()<b]
        file=str(path.relative_to(ROOT));prefix=[]
        if file.startswith('src/'):
            parts=list(path.relative_to(ROOT/'src').with_suffix('').parts)
            if parts[-1]in ['mod','lib']:parts=parts[:-1]
            prefix=parts
        name='::'.join([*prefix,*nested,match.group(1)])
        body=original[start:finish+1]
        result.append({'file':file,'name':name,'function_name':match.group(1),'line':original[:match.start()].count('\n')+1,'ignored':any(re.search(r'\[\s*ignore\b',a)for a in attrs),'source_sha256':digest(original),'body_sha256':digest(body),'body':body})
    return result


def source_inventory():
    paths=sorted([*(ROOT/'src').rglob('*.rs'),*(ROOT/'tests').glob('*.rs'),*(ROOT/'tests/support').glob('*.rs')])
    return [row for path in paths for row in functions(path)]


def cargo_inventory(log):
    raw=Path(log).read_text();entries=[];summaries=[];target=None;target_file=None
    for line in raw.splitlines():
        match=re.search(r'Running (?:unittests|tests/[^ ]+\.rs)\s+(?:(src/[^ ]+\.rs)\s+)?\(',line)
        if match:
            source=match.group(1)or re.search(r'Running (tests/[^ ]+\.rs)',line).group(1)
            target_file=source;target='lib'if source=='src/lib.rs'else 'bin'if source=='src/main.rs'else Path(source).stem
        test=re.fullmatch(r'(.+): test(?: \(ignored\))?',line)
        if test:
            if target is None:raise ValueError('Cargo test without target header')
            entries.append({'target':target,'target_file':target_file,'name':test.group(1),'cargo_ignored':'(ignored)'in line})
        summary=re.fullmatch(r'(\d+) tests?, (\d+) benchmarks?',line)
        if summary:summaries.append((target,int(summary.group(1)),int(summary.group(2))))
    if not summaries or sum(n for _,n,_ in summaries)!=len(entries):raise ValueError('Cargo list-only inventory incomplete or duplicate')
    # Cargo's source compile may emit no final summary; require its last target
    # census to be the final nonblank output so a still-running list is rejected.
    if not re.fullmatch(r'\d+ tests?, \d+ benchmarks?',raw.rstrip().splitlines()[-1]):raise ValueError('Cargo list-only output has not completed')
    if len({(r['target'],r['name'])for r in entries})!=len(entries):raise ValueError('duplicate actual Cargo test identity')
    return entries,summaries,raw

RESOURCE_PATTERNS={
 'cancellation':r'\bcancel(?:lation|led|ler)?\b|\.cancel\s*\(',
 'terminal budget/deadline':r'poll_budget\s*[:=]|poll budget exhausted|deadline exceeded|\.set_interrupt_handler\s*\(',
 'memory/stack exhaustion':r'memory_limit\s*:\s*(?:[1-9]\s*\*|[0-9]{1,7}\b)|memory limit exhausted|stack_limit\s*[:=]|maximum call stack|allocation.limit',
 'expansion/conversion exhaustion':r'max_(?:expansion|conversion|form|result|decode)[A-Za-z_]*\s*[:=]|(?:expansion|conversion|execution|reader|terminal)[_ ]limits?\b|unbounded_results|bounded_and_unbranded|shared_control',
}


# Whole test functions are excluded: safe subparts are never extracted or run.
EXPLICIT_RESOURCE_EXCLUSIONS={
 ('src/compiler_bootstrap.rs','cancelled_demand_discards_roots_before_reusing_host'):'Cancellation demand path.',
 ('src/compiler_bootstrap.rs','cancelled_extension_bundle_rolls_back_before_reusing_host'):'Cancellation bundle path.',
 ('src/evaluator.rs','unit_service_cancellation_is_sticky_even_when_js_catches'):'Sticky cancellation service path.',
 ('src/evaluator.rs','backtracking_deadline_is_terminal_and_fresh_operations_recover'):'Backtracking deadline exhaustion.',
 ('tests/compiler_bootstrap.rs','bootstrap_failure_and_expansion_limits_leave_reused_host_clean'):'Zero poll budget and expansion exhaustion.',
 ('tests/compiler_data_libraries.rs','traversal_limits_and_session_recovery'):'Walk depth exhaustion and infinite postwalk under a low budget.',
 ('tests/compiler_edn.rs','reader_limits_and_cancelled_providers_share_sticky_session_control'):'Reader budget exhaustion and cancelled provider.',
 ('tests/compiler_eduction_contracts.rs','cancellation_in_eduction_construction_demand_and_completion_is_terminal'):'Cancellation at eduction boundaries.',
 ('tests/compiler_eduction_contracts.rs','abandoned_prefixes_and_reducer_cycles_are_collected_in_both_engines'):'Low-memory bounded GC retention suite; conservatively outside this safe proposal.',
 ('tests/compiler_macros.rs','recursive_and_growing_expansions_obey_depth_count_size_and_execution_limits'):'Expansion depth/count/node and execution-budget exhaustion.',
 ('tests/compiler_macros.rs','exhausted_expansion_depth_does_not_run_macro_effects'):'Expansion depth exhaustion.',
 ('tests/compiler_macros.rs','collection_macro_compilation_obeys_shared_limits_and_recovers_with_the_same_host'):'Zero macro poll budget.',
 ('tests/compiler_macros.rs','unsupported_and_unbounded_results_are_located_and_fresh_sessions_recover'):'Unbounded expansion-result realization and generated-node exhaustion.',
 ('tests/compiler_macros.rs','quoted_js_literal_export_is_bounded_and_unbranded_nested_values_are_rejected'):'Explicit node/depth conversion exhaustion, mixed with ordinary object rejection.',
 ('tests/compiler_macros.rs','caught_nested_conversion_limits_cannot_reset_the_session_budget'):'Unbounded macroexpand conversion under explicit node/depth controls.',
 ('tests/compiler_macros.rs','nested_execution_limit_is_terminal_and_a_clean_host_can_be_reused'):'Infinite macro execution under a short deadline.',
 ('tests/compiler_macros.rs','terminal_limit_in_owner_cannot_be_caught_and_host_recovers'):'Infinite initializer under a short deadline.',
 ('tests/compiler_regex.rs','built_in_backtracking_observes_budget_deadline_and_cancellation'):'Regex budget/deadline/cancellation suite.',
 ('tests/compiler_sci_service_probes.rs','expansion_limits_and_finite_macro_returns_recover'):'Explicit expansion-depth exhaustion.',
 ('tests/compiler_sequence_completion.rs','shared_lazy_roots_are_phase_separated_and_failed_compilations_reset'):'Whole mixed test includes an infinite tree-seq under a 100ms deadline.',
 ('tests/compiler_sequence_contracts.rs','cancellation_in_sequence_construction_demand_and_completion_is_terminal'):'Cancellation at sequence boundaries.',
 ('tests/compiler_sequence_contracts.rs','abandoned_prefixes_and_reducer_cycles_are_collected_in_both_engines'):'Low-memory bounded GC retention suite; conservatively outside this safe proposal.',
 ('tests/compiler_transducer_contracts.rs','cancellation_at_each_boundary_stops_exactly_and_keeps_session_terminal'):'Cancellation at transducer boundaries.',
 ('tests/eval_cli.rs','runtime_and_rendering_limits_fail_without_partial_results'):'CLI poll/memory/output/deadline/source exhaustion.',
 ('tests/eval_cli.rs','repl_eof_reports_incomplete_input_once_and_errors_do_not_shift_history'):'Whole mixed test includes infinite REPL execution under a low poll budget.',
 ('tests/eval_cli.rs','repl_compacts_consumed_lines_and_preserves_original_locations'):'Explicit source-limit compaction suite; conservatively outside this safe proposal.',
 ('tests/evaluator_conformance.rs','every_historical_sci_case_has_an_executable_evaluator_disposition'):'Whole historical census calls support/sci_evaluator::run, which cancels a bounded-effect probe and applies terminal deadlines to two lazy-cycle originals.',
 ('tests/eval_sessions.rs','cancellation_and_limits_require_reset_but_host_compilation_recovers'):'Session cancellation and budget recovery.',
 ('tests/eval_sessions.rs','source_output_limits_and_oom_are_explicit'):'Source/output/OOM exhaustion.',
 ('tests/evaluator.rs','terminal_controls_survive_catch_and_fresh_host_sessions_recover'):'Evaluator cancellation and budget exhaustion.',
 ('tests/evaluator.rs','deadline_allocation_and_panics_produce_owned_failures'):'Deadline/allocation exhaustion mixed with ordinary callback panic.',
 ('tests/parser.rs','nesting_limit_includes_collections_prefixes_and_discards'):'Structural parser nesting-bound check; conservatively excluded from this proposal.',
 ('tests/sci_reference.rs','protocol_rejects_lossy_unknown_malformed_and_unbounded_results'):'Malformed/unbounded result protocol validation; conservatively excluded from this proposal.',
}
EXPLICIT_SAFE_REVIEWS={
 ('tests/compiler_macros.rs','finite_lazy_results_realize_under_shared_control_and_handles_stay_session_owned'):'Finite range40, ordinary GC and foreign-handle rejection; no budget/deadline/cancellation/exhaustion.',
 ('tests/eval_cli.rs','number_js_macros_and_clj_migration_are_explicit'):'Retired resource flags reject before language execution; ordinary literal/macro and migration diagnostics.',
 ('tests/eval_cli.rs','evaluation_options_are_validated_and_help_lists_both_families'):'Argument/help validation only; no resource exhaustion operation.',
 ('tests/compiler_printing_completion.rs','javascript_object_prefixes_survive_depth_limits_in_both_phases_and_sessions'):'Ordinary print-level/print-length language behavior is explicitly allowed.',
 ('tests/compiler_ifn_public_context.rs','ifn_receiver_terminal_rest_slot_executes_in_both_phases'):'Terminal denotes IFn rest-slot position; ordinary argument ABI, not terminal execution control.',
 ('tests/benchmark_variants.rs','compact_source_limit_is_checked_before_narrowing'):'Scanner offset representation validation; no evaluator resource operation.',
}


def proposal(log,output):
    actual,summaries,raw=cargo_inventory(log);sources=source_inventory();byfile={}
    for row in sources:byfile.setdefault(row['file'],[]).append(row)
    phases={(Path(r['test_file']).stem,r['test_name'])for r in map(json.loads,(HERE/'phase-witnesses.jsonl').read_text().splitlines())}
    reviewed=[]
    for item in actual:
        choices=([r for r in sources if r['file'].startswith('src/')and r['name']==item['name']]if item['target']=='lib'else [r for r in byfile.get(item['target_file'],[])if r['name']==item['name']])
        if not choices and item['target']!='lib':
            target_source=(ROOT/item['target_file']).read_text()
            for imported in re.finditer(r'#\[path\s*=\s*"([^"]+)"\]\s*(?:#\[[^\]]*\]\s*)*mod\s+([A-Za-z_]\w*)\s*;',target_source):
                relative,module=imported.groups()
                file=str((ROOT/item['target_file']).parent.joinpath(relative).relative_to(ROOT))
                for candidate in byfile.get(file,[]):
                    if module+'::'+candidate['name']==item['name']:choices.append({**candidate,'name':item['name']})
        if len(choices)!=1:raise ValueError('actual Cargo name has no unique exact source body: '+str(item))
        source=choices[0];text=source['body']+'\n'+source['name'];reasons=[name for name,pattern in RESOURCE_PATTERNS.items()if re.search(pattern,text,re.I)]
        key=(source['file'],source['function_name'])
        manual=EXPLICIT_RESOURCE_EXCLUSIONS.get(key)or EXPLICIT_SAFE_REVIEWS.get(key)
        if key in EXPLICIT_SAFE_REVIEWS:reasons=[]
        if key in EXPLICIT_RESOURCE_EXCLUSIONS:reasons=[manual]
        if reasons and key not in EXPLICIT_RESOURCE_EXCLUSIONS:raise ValueError('unreviewed resource signal: '+str(item))
        status='ignored-unexecuted'if source['ignored']or item['cargo_ignored']else 'excluded-resource-control'if reasons else 'safe-ordinary-proposed'
        if item['target']=='bin':status='no-tests'
        reviewed.append({**item,**{k:v for k,v in source.items()if k!='body'},'status':status,'manual_review_basis':manual,'resource_review_signals':reasons,'registered_full_gate_phase':(item['target'],item['name'])in phases,'command':['cargo','test','--locked','--lib'if item['target']=='lib'else '--test',*([]if item['target']=='lib'else[item['target']]),'--','--exact',item['name'],'--nocapture']})
    out={'source':'completed Cargo --locked --lib --tests -- --list plus complete current Rust test bodies','executed_regressions':0,'strict_complete':False,'inventory_log':str(Path(log).resolve()),'inventory_log_sha256':digest(raw),'cargo_target_summaries':summaries,'registered_phase_test_count':len(phases),'test_count':len(reviewed),'source_file_sha256':{r['file']:r['source_sha256']for r in reviewed},'rows':reviewed,'manual_review_required':'Resource signals are conservative source leads. Review helper-mediated resource paths and phase overlaps before any execution. No command here has run.'}
    out['counts']={status:sum(r['status']==status for r in reviewed)for status in ['safe-ordinary-proposed','excluded-resource-control','ignored-unexecuted']}
    out['safe_ordinary_not_registered_in_full_gate']=sum(r['status']=='safe-ordinary-proposed'and not r['registered_full_gate_phase']for r in reviewed)
    commands=[]
    for target,_,_ in summaries:
        rows=[r for r in reviewed if r['target']==target]
        safe=[r for r in rows if r['status']=='safe-ordinary-proposed']
        if not safe:continue
        command=['cargo','test','--locked','--lib'if target=='lib'else '--test',*([]if target=='lib'else[target]),'--']
        for r in rows:
            if r['status']=='excluded-resource-control':command.extend(['--skip',r['name']])
        commands.append({'target':target,'command':command,'expected_active_safe_tests':len(safe),'ignored_not_executed':sum(r['status']=='ignored-unexecuted'for r in rows),'excluded_exact_tests':[r['name']for r in rows if r['status']=='excluded-resource-control']})
    out['proposed_target_commands']=commands
    out['never_execute']=['benches/evaluator.rs','tests/support/eval_cli_terminal.py','all excluded whole tests','ignored tests without individual authorization/context and a separate safe audit']
    Path(output).write_text(json.dumps(out,indent=2)+'\n');return out

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--list-log',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();proposal(a.list_log,a.output)
