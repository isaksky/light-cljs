//! Budgeted inlining over the prepared IR: the inlining half of Phase 2 step 8
//! of docs/advanced-optimization.md (light-cljs-214), at `Advanced` only.
//! [`crate::folding`] is the other half; `prepare_application` alternates the
//! two with definition removal (see "Phase 2 folding and inlining extension
//! points" in the design document).
//!
//! [`inline`] replaces a call whose target is known at compile time
//! ([`crate::call_shapes`]) with the body of the method it runs when:
//!
//! - the callee is *single-use*: its value never escapes
//!   ([`CallShapes::closed`]) and this is its only call, so the definition
//!   goes away once nothing reads it and the body moves rather than grows; or
//! - the body costs at most [`INLINE_BUDGET`] expressions, is *pure* and
//!   inlining it lowers the program's cost ([`lowers_cost`]). A pure body only
//!   reads its parameters, locals, globals and constants, branches, binds
//!   locals, builds literal collections, looks up keywords and calls core
//!   operations whose row in `src/core_operations.tsv` is `pure` or
//!   `allocates` and calls no function argument, so it can run only the code
//!   its arguments supply (protocol implementations), as the call would. Its
//!   copies spend the program's growth budget ([`growth_budget`]).
//!
//! The cost walk stops as soon as a body exceeds its limit (rule 4 of
//! docs/dos-and-donts.md), [`SINGLE_USE_LIMIT`] for single-use callees.
//! Upstream's Closure Compiler weighs inlining the same way, by the size of the
//! body against the calls it replaces and the definition it removes
//! (`FunctionInjector.java:910-1080` in the research clone).
//!
//! # What is inlined
//!
//! The callee is a global whose sole definition
//! ([`crate::folding::sole_definitions_and_fenced_with_slots`]) is a direct top-level `def`, and the
//! call runs after it: in a later top-level form, or inside the function
//! itself. Its argument count matches the method's fixed parameters (a
//! variadic method may receive more, when its body never reads the rest
//! parameter), and the call has one parent. The method is not `^:async` (its
//! call is a Promise and its body may `await`), captures nothing, is no
//! `recur` target, and its body holds only literals, local, global and host
//! reads, `if`, `do`, `let`, `^boolean` hints, calls, collection literals,
//! property reads, method calls, `new`, `instance?` and metadata: no nested
//! `fn`, loop, `try`, `throw`, `case`, assignment, definition, `js*` raw text,
//! `js-arguments` or `this-as`, whose meaning depends on the function they are
//! in. A callee tagged `^boolean` keeps its calls, whose tests trust the tag
//! (see "Advanced-mode contract"). Calls are visited children first, in
//! source order, so a function's own calls are inlined before it is copied.
//!
//! # Arguments
//!
//! A parameter whose argument is a literal, a core value, an immutable local
//! or a constant collection the pool holds reads a copy of the argument
//! ([`trivial`]): those have no effect and keep their value while the body
//! runs. An unread parameter drops an argument without effect; an argument
//! without effect that reads no global or mutable field moves to its
//! parameter's only read ([`stable`]); and one argument with an effect moves
//! to its parameter's only read when the body reaches that read before
//! anything with an effect, unconditionally ([`first_read`], Closure's direct
//! inlining), unless it awaits: other code runs while it waits and may change
//! what the body reads first. Any other argument is bound to a fresh local by a `let` around
//! the body, in argument order, so it is evaluated once and before the body,
//! as the call did; arguments for an unread rest parameter are bound too when
//! they may have an effect. A `let` in value position would be emitted as a
//! function call, so a call there is inlined only when it needs no binding and
//! its body has no `let` where it produces its value. Locals of the body get
//! fresh identities, so a body inlined twice in one scope does not clash.
use crate::analyzer::{Expr, ExprId, ExprKind, LocalBinding, LocalId, Program};
use crate::call_shapes::CallShapes;
use crate::core_operations::{Calls, Purity};
use crate::folding::{
    BitSet, NOWHERE, bounded_slice_count, constant_like, partition_slices,
    sole_definitions_and_fenced_with_slots,
};
use crate::inference::ProgramFacts;
use crate::program_fragment::visit_children;
use crate::scheduler::{self, Slots};
use std::collections::{HashMap, HashSet};

/// Minimum expression count before parallelizing inlining pre-scans.
const PARALLEL_EXPR_THRESHOLD: usize = 2048;

/// Minimum top-level form count before slicing the inlining pre-walk.
const PARALLEL_FORM_THRESHOLD: usize = 16;

/// The most expressions a body inlined at every call may have.
pub const INLINE_BUDGET: usize = 10;

/// The most expressions a single-use body may have.
pub const SINGLE_USE_LIMIT: usize = 4096;

/// How many expressions inlining may add to a program of `nodes` reachable
/// expressions over all rounds: inlined calls of small pure functions copy
/// their bodies, and the budget bounds the copies.
pub fn growth_budget(nodes: usize) -> usize {
    nodes / 2 + 64
}

/// What a body walk found: its size, whether it is pure, and whether it has a
/// `let` where it produces its value.
#[derive(Clone)]
struct Body {
    nodes: usize,
    pure: bool,
    binds: bool,
    /// The local of every local read, once per read (compact `u32` IDs).
    reads: Vec<u32>,
}

/// How an argument reaches its parameter.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Use {
    /// Read in place at every read ([`trivial`]).
    Value,
    /// Read in place at its only read ([`stable`]).
    Moved,
    /// The parameter is never read and the argument has no effect.
    Dropped,
    /// Bound to a fresh local before the body.
    Bound,
}

/// How a parameter of an inlined method reads.
#[derive(Clone)]
enum Parameter {
    /// A copy of the argument expression at each read ([`trivial`]).
    Value(ExprId),
    /// A fresh local the argument is bound to.
    Local(LocalId),
    /// The argument expression, moved to the parameter's only read.
    Moved(ExprId),
    /// A parameter whose argument was dropped, or the rest parameter: a read
    /// aborts inlining.
    Unread,
}

/// Inline the calls of `program` that qualify, given its call shapes, and
/// return how many were inlined. `growth` is what is left of the program's
/// growth budget, which inlining pure functions at several calls spends.
pub fn inline(
    program: &mut Program,
    shapes: &CallShapes,
    growth: &mut usize,
    built: &mut Vec<bool>,
) -> usize {
    let mut top = Vec::new();
    flatten(program, program.root, &mut top);
    let facts = ProgramFacts::new_light(program, &top);
    inline_with_facts(program, shapes, growth, built, &facts)
}

/// [`inline`] reusing precomputed [`ProgramFacts`].
pub fn inline_with_facts(
    program: &mut Program,
    shapes: &CallShapes,
    growth: &mut usize,
    built: &mut Vec<bool>,
    facts: &ProgramFacts,
) -> usize {
    inline_with_facts_checked(program, shapes, growth, built, facts).0
}

/// [`inline_with_facts`] running read-only pre-scans on `slots` CPU work slots
/// (`--jobs N`), keeping the growth-budget commit loop serial and deterministic.
pub fn inline_with_facts_with_slots(
    program: &mut Program,
    shapes: &CallShapes,
    growth: &mut usize,
    built: &mut Vec<bool>,
    facts: &ProgramFacts,
    slots: Slots,
) -> usize {
    inline_with_facts_checked_with_slots(program, shapes, growth, built, facts, slots).0
}

/// [`inline_with_facts`], also reporting whether any inlined call sat in a
/// position that [`ProgramFacts`] inspects directly (assignment target or
/// receiver, raw JS hole, or `extend-type` target), so callers know whether
/// cached facts must refresh before the next fold.
pub(crate) fn inline_with_facts_checked(
    program: &mut Program,
    shapes: &CallShapes,
    growth: &mut usize,
    built: &mut Vec<bool>,
    facts: &ProgramFacts,
) -> (usize, bool) {
    inline_with_facts_checked_with_slots(program, shapes, growth, built, facts, Slots::ONE)
}

const NO_OWNER: u32 = u32::MAX;
const FLAG_BLOCK: u8 = 1;
const FLAG_MULTI_PARENT: u8 = 2;

#[derive(Clone, Copy)]
struct DirectCallInfo {
    id: u32,
    top_index: u32,
    owner: u32,
    flags: u8,
}

impl DirectCallInfo {
    #[inline]
    fn block(self) -> bool {
        (self.flags & FLAG_BLOCK) != 0
    }

    #[inline]
    fn multi_parent(self) -> bool {
        (self.flags & FLAG_MULTI_PARENT) != 0
    }
}

struct InlineScanSlice {
    reached: BitSet,
    direct_calls: Vec<DirectCallInfo>,
    fn_owners: Vec<(u32, u32)>,
    def_positions: Vec<(u32, u32)>,
    fact_sites: Vec<u32>,
    has_shared: bool,
}

fn record_assign_site(program: &Program, target: ExprId, sites: &mut Vec<u32>) {
    sites.push(target.0 as u32);
    if let ExprKind::Property { receiver, .. } | ExprKind::JsIndex { receiver, .. } =
        program.exprs[target.0].kind
    {
        sites.push(receiver.0 as u32);
    }
}

fn scan_inline_slice(
    program: &Program,
    shapes: &CallShapes,
    top: &[ExprId],
    range: std::ops::Range<usize>,
) -> InlineScanSlice {
    let count = program.exprs.len();
    let mut visited = BitSet::new(count);
    let mut pushed = BitSet::new(count);
    let mut multi_parents: HashSet<u32> = HashSet::new();
    let mut direct_calls = Vec::new();
    let mut fn_owners = Vec::new();
    let mut def_positions = Vec::new();
    let mut fact_sites = Vec::new();
    let mut roles = Vec::new();
    let mut pending: Vec<(ExprId, bool, bool, u32)> = Vec::new();
    let mut has_shared = false;

    for index in range {
        let form = top[index];
        if visited.get(form.0) {
            has_shared = true;
            continue;
        }
        if pushed.set(form.0) {
            multi_parents.insert(form.0 as u32);
        }
        pending.push((form, false, true, NO_OWNER));
        while let Some((id, expanded, is_block, cur_owner)) = pending.pop() {
            if expanded {
                if matches!(program.exprs[id.0].kind, ExprKind::Call { .. })
                    && shapes.call(id).is_some()
                {
                    direct_calls.push(DirectCallInfo {
                        id: id.0 as u32,
                        top_index: index as u32,
                        owner: cur_owner,
                        flags: if is_block { FLAG_BLOCK } else { 0 },
                    });
                }
                continue;
            }
            if visited.set(id.0) {
                has_shared = true;
                continue;
            }
            match &program.exprs[id.0].kind {
                ExprKind::SetGlobal { target, .. } => {
                    record_assign_site(program, *target, &mut fact_sites);
                }
                ExprKind::Binding { bindings, .. } => {
                    for (target, _) in bindings {
                        record_assign_site(program, *target, &mut fact_sites);
                    }
                }
                ExprKind::JsTemplate { args, .. } => {
                    for arg in args {
                        record_assign_site(program, *arg, &mut fact_sites);
                    }
                }
                ExprKind::ExtendType { target, .. } => {
                    fact_sites.push(target.0 as u32);
                }
                ExprKind::Def { .. } => {
                    def_positions.push((id.0 as u32, index as u32));
                }
                ExprKind::Fn { .. } => {
                    fn_owners.push((id.0 as u32, cur_owner));
                }
                _ => (),
            }
            pending.push((id, true, is_block, cur_owner));
            roles.clear();
            child_roles(program, id, is_block, &mut roles);
            let inner = if matches!(program.exprs[id.0].kind, ExprKind::Fn { .. }) {
                id.0 as u32
            } else {
                cur_owner
            };
            for &(child, role) in roles.iter().rev() {
                if pushed.set(child.0) {
                    multi_parents.insert(child.0 as u32);
                    has_shared = true;
                }
                if !visited.get(child.0) {
                    pending.push((child, false, role, inner));
                }
            }
        }
    }

    if !multi_parents.is_empty() {
        has_shared = true;
        for call in &mut direct_calls {
            if multi_parents.contains(&call.id) {
                call.flags |= FLAG_MULTI_PARENT;
            }
        }
    }

    InlineScanSlice {
        reached: visited,
        direct_calls,
        fn_owners,
        def_positions,
        fact_sites,
        has_shared,
    }
}

/// Read-only eligibility check for one direct call site, shared by parallel
/// pre-filtering workers and the serial commit loop. Returns:
/// - `Err(true)` if the call cannot be ruled out statically (its callee is
///   itself a `Call` that an earlier inline could rewrite to a `Global`), so
///   the serial commit loop must recheck it;
/// - `Err(false)` if the call is definitely ineligible;
/// - `Ok((function, method_idx, body, limit))` if the call passes all
///   immutable checks and needs body measurement.
#[allow(clippy::too_many_arguments)]
fn check_call_eligibility(
    program: &Program,
    shapes: &CallShapes,
    top: &[ExprId],
    definitions: &[Option<ExprId>],
    def_positions: &HashMap<u32, u32>,
    fn_owners: &HashMap<u32, u32>,
    sites: &HashMap<u32, u32>,
    call: DirectCallInfo,
) -> Result<(u32, u32, ExprId, usize), bool> {
    if call.multi_parent() {
        return Err(false);
    }
    let call_id = ExprId(call.id as usize);
    let Some(direct) = shapes.call(call_id) else {
        return Err(false);
    };
    let ExprKind::Call { callee, args } = &program.exprs[call_id.0].kind else {
        return Err(false);
    };
    let ExprKind::Global(global) = program.exprs[callee.0].kind else {
        // An inner call in callee position may inline into a `Global` before
        // this outer call is visited in post-order.
        return Err(matches!(
            program.exprs[callee.0].kind,
            ExprKind::Call { .. }
        ));
    };
    if program.globals[global.0].boolean_tag {
        return Err(false);
    }
    let Some(definition) = definitions[global.0] else {
        return Err(false);
    };
    let defined = def_positions
        .get(&(definition.0 as u32))
        .copied()
        .unwrap_or(NOWHERE);
    if top.get(defined as usize) != Some(&definition)
        || call.top_index == NOWHERE
        || call.top_index < defined
    {
        return Err(false);
    }
    if call.top_index == defined {
        let mut inside = false;
        let mut function = call.owner;
        while function != NO_OWNER {
            if function == direct.function.0 as u32 {
                inside = true;
                break;
            }
            function = fn_owners.get(&function).copied().unwrap_or(NO_OWNER);
        }
        if !inside {
            return Err(false);
        }
    }
    let ExprKind::Fn { methods, .. } = &program.exprs[direct.function.0].kind else {
        return Err(false);
    };
    let method = &methods[direct.method];
    let fixed = method.params.len();
    if method.asynchronous
        || !method.captures.is_empty()
        || program.recur_targets[method.target.0].used
        || (method.rest.is_none() && args.len() != fixed)
        || args.len() < fixed
    {
        return Err(false);
    }
    let closed = shapes.closed(global);
    let call_count = sites.get(&(direct.function.0 as u32)).copied().unwrap_or(0) as usize;
    let single = closed && call_count == 1;
    let limit = if single {
        SINGLE_USE_LIMIT
    } else {
        INLINE_BUDGET
    };
    Ok((
        direct.function.0 as u32,
        direct.method as u32,
        method.body,
        limit,
    ))
}

/// [`inline_with_facts_checked`] running read-only pre-scans on `slots` CPU
/// work slots (`--jobs N`).
pub(crate) fn inline_with_facts_checked_with_slots(
    program: &mut Program,
    shapes: &CallShapes,
    growth: &mut usize,
    built: &mut Vec<bool>,
    facts: &ProgramFacts,
    slots: Slots,
) -> (usize, bool) {
    let count = program.exprs.len();
    let mut top = Vec::new();
    flatten(program, program.root, &mut top);

    let use_parallel =
        slots.get() > 1 && count >= PARALLEL_EXPR_THRESHOLD && top.len() >= PARALLEL_FORM_THRESHOLD;

    // Walk each top-level form in order, children first, recording each
    // direct call's single-parent status, top-level form index, innermost `fn`
    // owner, and whether emission writes it in block/return position.
    let (global_reached, direct_calls, fn_owners, def_positions, fact_sites, has_shared) =
        if use_parallel {
            let target_slices = bounded_slice_count(slots, top.len());
            let weights: Vec<u64> = vec![1; top.len()];
            let ranges = partition_slices(&weights, target_slices);
            let jobs: Vec<(u64, std::ops::Range<usize>)> =
                ranges.into_iter().map(|r| (r.len() as u64, r)).collect();
            let slice_results = scheduler::map(slots, jobs, |range| {
                scan_inline_slice(program, shapes, &top, range)
            });

            let mut global_reached = BitSet::new(count);
            let mut overlaps = false;
            for slice in &slice_results {
                if global_reached.intersects(&slice.reached) {
                    overlaps = true;
                    break;
                }
                global_reached.or_assign(&slice.reached);
            }

            if overlaps {
                // Rare cross-slice shared ExprId: fall back to exact serial pre-walk.
                drop(slice_results);
                let single = scan_inline_slice(program, shapes, &top, 0..top.len());
                let fn_owners: HashMap<u32, u32> = single.fn_owners.into_iter().collect();
                let def_positions: HashMap<u32, u32> = single.def_positions.into_iter().collect();
                let fact_sites: HashSet<u32> = single.fact_sites.into_iter().collect();
                (
                    single.reached,
                    single.direct_calls,
                    fn_owners,
                    def_positions,
                    fact_sites,
                    true,
                )
            } else {
                let any_shared = slice_results.iter().any(|s| s.has_shared);
                let total_calls: usize = slice_results.iter().map(|s| s.direct_calls.len()).sum();
                let total_fns: usize = slice_results.iter().map(|s| s.fn_owners.len()).sum();
                let total_defs: usize = slice_results.iter().map(|s| s.def_positions.len()).sum();
                let mut direct_calls = Vec::with_capacity(total_calls);
                let mut fn_owners = HashMap::with_capacity(total_fns);
                let mut def_positions = HashMap::with_capacity(total_defs);
                let mut fact_sites = HashSet::new();
                for slice in slice_results {
                    direct_calls.extend(slice.direct_calls);
                    fn_owners.extend(slice.fn_owners);
                    def_positions.extend(slice.def_positions);
                    fact_sites.extend(slice.fact_sites);
                }
                (
                    global_reached,
                    direct_calls,
                    fn_owners,
                    def_positions,
                    fact_sites,
                    any_shared,
                )
            }
        } else {
            let single = scan_inline_slice(program, shapes, &top, 0..top.len());
            let fn_owners: HashMap<u32, u32> = single.fn_owners.into_iter().collect();
            let def_positions: HashMap<u32, u32> = single.def_positions.into_iter().collect();
            let fact_sites: HashSet<u32> = single.fact_sites.into_iter().collect();
            (
                single.reached,
                single.direct_calls,
                fn_owners,
                def_positions,
                fact_sites,
                single.has_shared,
            )
        };

    let (definitions, fenced) =
        sole_definitions_and_fenced_with_slots(program, &global_reached, slots);
    drop(global_reached);
    built.resize(count, false);

    let mut sites: HashMap<u32, u32> = HashMap::new();
    for call in &direct_calls {
        if let Some(direct) = shapes.call(ExprId(call.id as usize)) {
            *sites.entry(direct.function.0 as u32).or_insert(0) += 1;
        }
    }

    // Run read-only eligibility filtering and candidate body measurement on
    // workers when above the parallel threshold and free of shared subgraphs.
    // For `Slots::ONE`, small programs, or shared graphs, keep eligibility and
    // measurement inline/lazy during the serial commit loop.
    type MeasuredBodies = HashMap<(u32, u32), (u32, Option<Body>)>;
    let (commit_calls, mut measured_bodies): (Vec<DirectCallInfo>, MeasuredBodies) =
        if use_parallel && !has_shared {
            let (eligible_calls, candidates) = if direct_calls.len() >= PARALLEL_FORM_THRESHOLD {
                let num_chunks = bounded_slice_count(slots, direct_calls.len());
                let chunk_size = direct_calls.len().div_ceil(num_chunks);
                let jobs: Vec<(u64, std::ops::Range<usize>)> = (0..direct_calls.len())
                    .step_by(chunk_size)
                    .map(|start| {
                        let end = (start + chunk_size).min(direct_calls.len());
                        ((end - start) as u64, start..end)
                    })
                    .collect();
                let filtered_chunks = scheduler::map(slots, jobs, |range| {
                    let mut kept_calls = Vec::new();
                    let mut local_seen: HashSet<(u32, u32)> = HashSet::new();
                    let mut local_candidates: Vec<(u32, u32, ExprId, usize)> = Vec::new();
                    for &call in &direct_calls[range] {
                        match check_call_eligibility(
                            program,
                            shapes,
                            &top,
                            &definitions,
                            &def_positions,
                            &fn_owners,
                            &sites,
                            call,
                        ) {
                            Ok((func, method_idx, body, limit)) => {
                                kept_calls.push(call);
                                if local_seen.insert((func, method_idx)) {
                                    local_candidates.push((func, method_idx, body, limit));
                                }
                            }
                            Err(true) => kept_calls.push(call),
                            Err(false) => (),
                        }
                    }
                    (kept_calls, local_candidates)
                });
                drop(direct_calls);
                let total_kept: usize = filtered_chunks.iter().map(|(c, _)| c.len()).sum();
                let mut eligible_calls = Vec::with_capacity(total_kept);
                let mut candidate_seen: HashSet<(u32, u32)> = HashSet::new();
                let mut candidates: Vec<(u32, u32, ExprId, usize)> = Vec::new();
                for (kept_calls, local_candidates) in filtered_chunks {
                    eligible_calls.extend(kept_calls);
                    for cand in local_candidates {
                        if candidate_seen.insert((cand.0, cand.1)) {
                            candidates.push(cand);
                        }
                    }
                }
                (eligible_calls, candidates)
            } else {
                let mut eligible_calls = Vec::with_capacity(direct_calls.len());
                let mut candidate_seen: HashSet<(u32, u32)> = HashSet::new();
                let mut candidates: Vec<(u32, u32, ExprId, usize)> = Vec::new();
                for call in direct_calls {
                    match check_call_eligibility(
                        program,
                        shapes,
                        &top,
                        &definitions,
                        &def_positions,
                        &fn_owners,
                        &sites,
                        call,
                    ) {
                        Ok((func, method_idx, body, limit)) => {
                            eligible_calls.push(call);
                            if candidate_seen.insert((func, method_idx)) {
                                candidates.push((func, method_idx, body, limit));
                            }
                        }
                        Err(true) => eligible_calls.push(call),
                        Err(false) => (),
                    }
                }
                (eligible_calls, candidates)
            };

            let measured = if candidates.len() >= PARALLEL_FORM_THRESHOLD {
                let target_slices = bounded_slice_count(slots, candidates.len());
                let weights: Vec<u64> = candidates.iter().map(|c| c.3 as u64).collect();
                let ranges = partition_slices(&weights, target_slices);
                let jobs: Vec<(u64, std::ops::Range<usize>)> = ranges
                    .into_iter()
                    .map(|r| {
                        let w: u64 = weights[r.clone()].iter().sum();
                        (w, r)
                    })
                    .collect();
                let chunks = scheduler::map(slots, jobs, |range| {
                    let mut out = Vec::with_capacity(range.len());
                    for &(func, method_idx, body_id, limit) in &candidates[range] {
                        out.push((
                            (func, method_idx),
                            (0u32, measure(program, facts, body_id, limit)),
                        ));
                    }
                    out
                });
                drop(candidates);
                let mut map = HashMap::new();
                for chunk in chunks {
                    map.extend(chunk);
                }
                map
            } else {
                let mut map = HashMap::with_capacity(candidates.len());
                for (func, method_idx, body_id, limit) in candidates {
                    map.insert(
                        (func, method_idx),
                        (0u32, measure(program, facts, body_id, limit)),
                    );
                }
                map
            };
            (eligible_calls, measured)
        } else {
            (direct_calls, HashMap::new())
        };

    // Version counter per enclosing `fn`: bumped whenever an inlined call
    // mutates inside that `fn`, invalidating cached `Body` measurements for
    // every method of that `fn`.
    let mut fn_versions: HashMap<u32, u32> = HashMap::new();

    let mut inlined = 0;
    let mut invalidated_facts = false;
    for call_info in commit_calls {
        let call = ExprId(call_info.id as usize);
        let Some(direct) = shapes.call(call) else {
            continue;
        };
        if call_info.multi_parent() {
            continue;
        }
        let ExprKind::Call { callee, args } = &program.exprs[call.0].kind else {
            continue;
        };
        let ExprKind::Global(global) = program.exprs[callee.0].kind else {
            continue;
        };
        if program.globals[global.0].boolean_tag {
            continue;
        }
        // The definition is a direct top-level form that runs before the
        // form holding the call.
        let Some(definition) = definitions[global.0] else {
            continue;
        };
        let defined = def_positions
            .get(&(definition.0 as u32))
            .copied()
            .unwrap_or(NOWHERE);
        // A call inside the function itself runs once the function exists,
        // after its definition.
        let inside = || {
            let mut function = call_info.owner;
            while function != NO_OWNER {
                if function == direct.function.0 as u32 {
                    return true;
                }
                function = fn_owners.get(&function).copied().unwrap_or(NO_OWNER);
            }
            false
        };
        if top.get(defined as usize) != Some(&definition)
            || call_info.top_index == NOWHERE
            || call_info.top_index < defined
            || (call_info.top_index == defined && !inside())
        {
            continue;
        }
        let ExprKind::Fn { methods, .. } = &program.exprs[direct.function.0].kind else {
            continue;
        };
        let method = methods[direct.method].clone();
        let args = args.clone();
        let fixed = method.params.len();
        // A call of an `^:async` method returns a Promise and its body may
        // `await`, which only that method can hold.
        if method.asynchronous
            || !method.captures.is_empty()
            || program.recur_targets[method.target.0].used
            || (method.rest.is_none() && args.len() != fixed)
            || args.len() < fixed
        {
            continue;
        }
        let closed = shapes.closed(global);
        let call_count = sites.get(&(direct.function.0 as u32)).copied().unwrap_or(0) as usize;
        let single = closed && call_count == 1;
        let limit = if single {
            SINGLE_USE_LIMIT
        } else {
            INLINE_BUDGET
        };
        let func32 = direct.function.0 as u32;
        let key = (func32, direct.method as u32);
        let cur_ver = fn_versions.get(&func32).copied().unwrap_or(0);
        let body = if has_shared {
            measure(program, facts, method.body, limit)
        } else if single {
            // Single-use body is needed at most once: take ownership and free immediately.
            match measured_bodies.remove(&key) {
                Some((ver, cached)) if ver == cur_ver => cached,
                _ => measure(program, facts, method.body, limit),
            }
        } else if let Some((ver, cached)) = measured_bodies.get(&key)
            && *ver == cur_ver
        {
            cached.clone()
        } else {
            let fresh_body = measure(program, facts, method.body, limit);
            measured_bodies.insert(key, (cur_ver, fresh_body.clone()));
            fresh_body
        };
        let Some(body) = body else {
            continue;
        };
        if !single
            && (!body.pure
                || *growth < body.nodes
                || !lowers_cost(body.nodes, args.len(), fixed, closed.then_some(call_count)))
        {
            continue;
        }
        if body.binds && !call_info.block() {
            continue;
        }

        // How each parameter reads its argument. Arguments read in place or
        // dropped need no binding; the others are bound by a `let`, which only
        // a statement or returned value may hold.
        let mut uses: Vec<Use> = args
            .iter()
            .enumerate()
            .map(|(index, &arg)| {
                let kind = &program.exprs[arg.0].kind;
                let reads = method.params.get(index).map_or(0, |parameter| {
                    let target = parameter.0 as u32;
                    body.reads.iter().filter(|l| **l == target).count()
                });
                // A literal built at each evaluation is one value per call,
                // which copies at several reads would not be.
                if reads > 0 && trivial(program, kind) && !builds(program, built, arg) {
                    Use::Value
                } else if reads == 0 && crate::folding::effect_free(program, kind) {
                    Use::Dropped
                } else if reads == 1 && stable(program, kind) {
                    Use::Moved
                } else {
                    Use::Bound
                }
            })
            .collect();
        // One argument with an effect may move to its parameter's only read
        // when the body reads it before anything else it evaluates can have an
        // effect or throw: it is then evaluated once, at the same point
        // relative to every effect (Closure's direct inlining).
        if let [bound] = uses
            .iter()
            .enumerate()
            .filter(|(_, use_)| **use_ == Use::Bound)
            .map(|(index, _)| index)
            .collect::<Vec<_>>()[..]
            && let Some(&parameter) = method.params.get(bound)
            && !crate::folding::scoped(program, args[bound])
            // Other code runs while an `await` waits, and may change what
            // the body reads before the parameter.
            && !crate::program_fragment::awaits(program, args[bound])
            && body.reads.iter().filter(|l| **l == parameter.0 as u32).count() == 1
            && first_read(program, method.body, parameter) == Scan::Found
        {
            uses[bound] = Use::Moved;
        }
        if uses.contains(&Use::Bound) && !call_info.block() {
            continue;
        }
        let (exprs, locals) = (program.exprs.len(), program.locals.len());
        let mut parameters: Vec<(LocalId, Parameter)> = Vec::with_capacity(args.len() + 1);
        let mut bindings = Vec::new();
        for (index, (&arg, use_)) in args.iter().zip(uses).enumerate() {
            let parameter = method.params.get(index).copied();
            let read = match use_ {
                Use::Value => Parameter::Value(arg),
                Use::Moved => Parameter::Moved(arg),
                Use::Dropped => Parameter::Unread,
                Use::Bound => {
                    let like = parameter.or(method.rest).expect("a parameter");
                    let local = fresh(program, like);
                    bindings.push((local, arg));
                    Parameter::Local(local)
                }
            };
            if let Some(parameter) = parameter {
                parameters.push((parameter, read));
            }
        }
        if let Some(rest) = method.rest {
            parameters.push((rest, Parameter::Unread));
        }
        let mut copier = Copier {
            program,
            built,
            call,
            parameters,
        };
        // A call that is a map key or set member must not become a constant.
        let Some(copy) = copier.copy(method.body).filter(|copy| {
            !fenced.get(call.0) || !constant_like(program, &program.exprs[copy.0].kind)
        }) else {
            program.exprs.truncate(exprs);
            program.locals.truncate(locals);
            built.truncate(exprs);
            continue;
        };
        let kind = if bindings.is_empty() {
            built[call.0] = built[copy.0];
            program.exprs[copy.0].kind.clone()
        } else {
            built[call.0] = false;
            ExprKind::Let {
                bindings,
                body: copy,
            }
        };
        program.exprs[call.0].kind = kind;
        // Any enclosing `fn` now has a modified body and must be re-measured
        // before a later call copies it.
        let mut enclosing = call_info.owner;
        while enclosing != NO_OWNER {
            *fn_versions.entry(enclosing).or_insert(0) += 1;
            enclosing = fn_owners.get(&enclosing).copied().unwrap_or(NO_OWNER);
        }
        if !single {
            *growth -= body.nodes;
        }
        invalidated_facts |= fact_sites.contains(&call_info.id);
        inlined += 1;
    }
    (inlined, invalidated_facts)
}

/// Whether inlining a body of `body` expressions at a call with `arguments`
/// arguments shrinks the program, in expressions: the call costs itself, its
/// callee and its arguments, and a function of `parameters` parameters costs
/// its body and about three expressions more. A function that escapes keeps
/// its definition, so each inlined call must cost no more than the call. A
/// closed one with `sites` calls loses its definition once every call is
/// inlined, which pays for the copies (Closure's `inliningLowersCost`).
fn lowers_cost(body: usize, arguments: usize, parameters: usize, sites: Option<usize>) -> bool {
    let call = 2 + arguments;
    match sites {
        None => body <= call,
        Some(sites) => sites * body <= sites * call + body + 3 + parameters,
    }
}

/// The direct top-level forms of the program.
fn flatten(program: &Program, id: ExprId, top: &mut Vec<ExprId>) {
    if let ExprKind::Do(items) = &program.exprs[id.0].kind {
        for item in items {
            flatten(program, *item, top);
        }
    } else {
        top.push(id);
    }
}

/// The children of `id` and whether emission writes each as a statement or a
/// returned value, given whether it writes `id` so (`block`). A `let`, `loop`
/// or `letfn` body and a function body are always one: in value position the
/// form becomes a function call that returns it. Everything else not listed
/// is a value.
fn child_roles(program: &Program, id: ExprId, block: bool, out: &mut Vec<(ExprId, bool)>) {
    match &program.exprs[id.0].kind {
        ExprKind::Do(items) => out.extend(items.iter().map(|item| (*item, block))),
        ExprKind::If { test, yes, no } => out.extend([(*test, false), (*yes, block), (*no, block)]),
        ExprKind::Let { bindings, body }
        | ExprKind::LetFn { bindings, body }
        | ExprKind::Loop { bindings, body, .. } => {
            out.extend(bindings.iter().map(|(_, init)| (*init, false)));
            out.push((*body, true));
        }
        ExprKind::Fn { methods, .. } => out.extend(methods.iter().map(|m| (m.body, true))),
        _ => visit_children(&program.exprs[id.0].kind, |child| out.push((child, false))),
    }
}

/// Whether `id` or a literal in it is marked built at each evaluation
/// ([`crate::folding::built_literals`]): folding made its items constants,
/// but each copy would still be a value of its own.
fn builds(program: &Program, built: &[bool], id: ExprId) -> bool {
    let mut pending = vec![id];
    while let Some(id) = pending.pop() {
        if built.get(id.0).copied().unwrap_or(false) {
            return true;
        }
        if matches!(
            program.exprs[id.0].kind,
            ExprKind::Vector(_)
                | ExprKind::List(_)
                | ExprKind::Map(_)
                | ExprKind::Set(_)
                | ExprKind::LiteralMap(_)
                | ExprKind::LiteralSet(_)
                | ExprKind::WithMeta { .. }
        ) {
            visit_children(&program.exprs[id.0].kind, |child| pending.push(child));
        }
    }
    false
}

/// Whether an argument of this kind may be read in place of its parameter:
/// it has no effect and its value cannot change while the body runs. A
/// constant collection literal qualifies because `Advanced` declares it once
/// in the constant pool, so every read is the one value the call would pass;
/// not one that [`builds`] at each evaluation.
fn trivial(program: &Program, kind: &ExprKind) -> bool {
    match kind {
        ExprKind::Nil
        | ExprKind::Bool(_)
        | ExprKind::Number(_)
        | ExprKind::String(_)
        | ExprKind::Keyword(_)
        | ExprKind::CoreFn(_) => true,
        // A mutable field may be assigned while the body runs.
        ExprKind::Local(local) => !program
            .field_bindings
            .get(local)
            .is_some_and(|(_, mutable)| *mutable),
        _ => pooled(program, kind),
    }
}

/// Whether a literal of this kind is a non-empty constant collection that the
/// constant pool holds (see "Constant pool extension points" in
/// docs/advanced-optimization.md): vectors and lists of constants, and maps
/// and sets whose keys and members are scalars. Empty literals read their
/// type's `EMPTY` where they are evaluated.
fn pooled(program: &Program, kind: &ExprKind) -> bool {
    let scalar = |id: &ExprId| {
        matches!(
            program.exprs[id.0].kind,
            ExprKind::Nil
                | ExprKind::Bool(_)
                | ExprKind::Number(_)
                | ExprKind::String(_)
                | ExprKind::Keyword(_)
                | ExprKind::Symbol(_)
        )
    };
    let constant = |id: &ExprId| scalar(id) || pooled(program, &program.exprs[id.0].kind);
    match kind {
        ExprKind::Vector(items) | ExprKind::List(items) => {
            !items.is_empty() && items.iter().all(constant)
        }
        ExprKind::Map(items) | ExprKind::LiteralMap(items) => {
            !items.is_empty()
                && items
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .all(|[key, value]| scalar(key) && constant(value))
        }
        ExprKind::Set(items) | ExprKind::LiteralSet(items) => {
            !items.is_empty() && items.iter().all(scalar)
        }
        _ => false,
    }
}

/// Whether an argument of this kind may be read in place of its parameter at
/// its one read: it has no effect ([`crate::folding::effect_free`]) and reads
/// no global or mutable field, whose value the body might change before that
/// read.
fn stable(program: &Program, kind: &ExprKind) -> bool {
    if !crate::folding::effect_free(program, kind) {
        return false;
    }
    match kind {
        ExprKind::Global(_) => false,
        ExprKind::Local(_) => trivial(program, kind),
        ExprKind::Vector(items)
        | ExprKind::List(items)
        | ExprKind::Map(items)
        | ExprKind::LiteralMap(items)
        | ExprKind::Set(items)
        | ExprKind::LiteralSet(items) => items
            .iter()
            .all(|item| stable(program, &program.exprs[item.0].kind)),
        _ => true,
    }
}

/// What [`first_read`] found in an expression, scanning in evaluation order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scan {
    /// The read of the parameter, reached before anything with an effect.
    Found,
    /// Evaluated completely without an effect or a throw, and no read.
    Clean,
    /// Something that may have an effect or throw, or a conditional or
    /// repeated evaluation, comes first.
    Dirty,
}

/// Whether evaluating `id` reaches a read of `parameter` before anything
/// that may have an effect or throw, unconditionally: through calls,
/// `do`, `let` and the test of an `if`, never into a branch.
fn first_read(program: &Program, id: ExprId, parameter: LocalId) -> Scan {
    // Children in evaluation order: the first that is not clean decides.
    let sequence = |items: &mut dyn Iterator<Item = ExprId>| {
        for item in items {
            match first_read(program, item, parameter) {
                Scan::Clean => (),
                scan => return scan,
            }
        }
        Scan::Clean
    };
    // A form that runs code of its own after its operands.
    let operation = |scan: Scan| {
        if scan == Scan::Found {
            Scan::Found
        } else {
            Scan::Dirty
        }
    };
    match &program.exprs[id.0].kind {
        ExprKind::Local(local) if *local == parameter => Scan::Found,
        ExprKind::Nil
        | ExprKind::Bool(_)
        | ExprKind::Number(_)
        | ExprKind::String(_)
        | ExprKind::Keyword(_)
        | ExprKind::Symbol(_)
        | ExprKind::Local(_)
        | ExprKind::Global(_)
        | ExprKind::CoreFn(_) => Scan::Clean,
        ExprKind::Do(items) => sequence(&mut items.iter().copied()),
        ExprKind::Let { bindings, body } => sequence(
            &mut bindings
                .iter()
                .map(|(_, init)| *init)
                .chain(std::iter::once(*body)),
        ),
        ExprKind::BooleanHint(inner) => first_read(program, *inner, parameter),
        ExprKind::If { test, .. } => operation(first_read(program, *test, parameter)),
        // Building a vector or list has no effect and cannot throw.
        ExprKind::Vector(items) | ExprKind::List(items) => sequence(&mut items.iter().copied()),
        ExprKind::Call { callee, args } => operation(sequence(
            &mut std::iter::once(*callee).chain(args.iter().copied()),
        )),
        ExprKind::MethodCall { receiver, args, .. } => operation(sequence(
            &mut std::iter::once(*receiver).chain(args.iter().copied()),
        )),
        ExprKind::Property { receiver, .. } => operation(first_read(program, *receiver, parameter)),
        ExprKind::Construct { callee, args } => operation(sequence(
            &mut std::iter::once(*callee).chain(args.iter().copied()),
        )),
        _ => Scan::Dirty,
    }
}

/// A fresh local named like `like`.
fn fresh(program: &mut Program, like: LocalId) -> LocalId {
    let binding: LocalBinding = program.locals[like.0].clone();
    program.locals.push(binding);
    LocalId(program.locals.len() - 1)
}

/// Walk `body` and say what it is, or `None` when it holds something that is
/// never inlined or exceeds `limit` expressions; the walk stops there.
fn measure(program: &Program, facts: &ProgramFacts, body: ExprId, limit: usize) -> Option<Body> {
    let mut result = Body {
        nodes: 0,
        pure: true,
        binds: false,
        reads: Vec::new(),
    };
    // Each entry says whether the expression produces the body's value.
    let mut pending = vec![(body, true)];
    while let Some((id, produces)) = pending.pop() {
        result.nodes += 1;
        if result.nodes > limit {
            return None;
        }
        let kind = &program.exprs[id.0].kind;
        match kind {
            ExprKind::Local(local) => result.reads.push(local.0 as u32),
            ExprKind::Nil
            | ExprKind::Bool(_)
            | ExprKind::Number(_)
            | ExprKind::String(_)
            | ExprKind::Keyword(_)
            | ExprKind::Symbol(_)
            | ExprKind::Global(_)
            | ExprKind::CoreFn(_) => (),
            // A constant collection is one read of its pool entry.
            kind if pooled(program, kind) => (),
            ExprKind::Do(items) => pending.extend(items.iter().map(|item| (*item, produces))),
            ExprKind::If { test, yes, no } => {
                pending.extend([(*test, false), (*yes, produces), (*no, produces)])
            }
            ExprKind::Let { bindings, body } => {
                result.binds |= produces;
                pending.extend(bindings.iter().map(|(_, init)| (*init, false)));
                pending.push((*body, produces));
            }
            ExprKind::BooleanHint(inner) => pending.push((*inner, produces)),
            ExprKind::List(items)
            | ExprKind::Vector(items)
            | ExprKind::Map(items)
            | ExprKind::Set(items)
            | ExprKind::LiteralMap(items)
            | ExprKind::LiteralSet(items) => {
                pending.extend(items.iter().map(|item| (*item, false)))
            }
            ExprKind::Call { callee, args } => {
                result.pure &= match program.exprs[callee.0].kind {
                    ExprKind::CoreFn(core) => {
                        let operation = core.operation();
                        !facts.replaced(core)
                            && operation.purity <= Purity::Allocates
                            && operation.calls != Calls::Invoke
                    }
                    // A keyword looks itself up.
                    ExprKind::Keyword(_) => true,
                    _ => false,
                };
                pending.push((*callee, false));
                pending.extend(args.iter().map(|arg| (*arg, false)));
            }
            ExprKind::JsGlobal(_)
            | ExprKind::Property { .. }
            | ExprKind::MethodCall { .. }
            | ExprKind::JsIndex { .. }
            | ExprKind::Construct { .. }
            | ExprKind::InstanceOf { .. }
            | ExprKind::WithMeta { .. } => {
                result.pure = false;
                visit_children(&program.exprs[id.0].kind, |child| {
                    pending.push((child, false))
                });
            }
            _ => return None,
        }
    }
    Some(result)
}

/// Copies a body for one call, mapping its parameters and giving its locals
/// fresh identities.
struct Copier<'p> {
    program: &'p mut Program,
    /// By expression: literals built at each evaluation
    /// ([`crate::folding::built_literals`]); copies carry the mark.
    built: &'p mut Vec<bool>,
    /// The call being replaced: a body that contains it calls itself.
    call: ExprId,
    parameters: Vec<(LocalId, Parameter)>,
}

impl Copier<'_> {
    fn parameter(&self, local: LocalId) -> Option<&Parameter> {
        self.parameters
            .iter()
            .rev()
            .find(|(parameter, _)| *parameter == local)
            .map(|(_, parameter)| parameter)
    }

    fn all(&mut self, ids: &[ExprId]) -> Option<Vec<ExprId>> {
        ids.iter().map(|id| self.copy(*id)).collect()
    }

    /// A copy of `id`, or `None` when it reads a local the copy cannot map or
    /// contains the call being replaced.
    fn copy(&mut self, id: ExprId) -> Option<ExprId> {
        if id == self.call {
            return None;
        }
        let kind = match self.program.exprs[id.0].kind.clone() {
            kind @ (ExprKind::Nil
            | ExprKind::Bool(_)
            | ExprKind::Number(_)
            | ExprKind::String(_)
            | ExprKind::Keyword(_)
            | ExprKind::Symbol(_)
            | ExprKind::Global(_)
            | ExprKind::CoreFn(_)
            | ExprKind::JsGlobal(_)) => kind,
            ExprKind::Local(local) => match self.parameter(local)? {
                Parameter::Value(argument) => {
                    let argument = *argument;
                    return Some(self.duplicate(argument));
                }
                Parameter::Local(local) => ExprKind::Local(*local),
                Parameter::Moved(argument) => return Some(*argument),
                Parameter::Unread => return None,
            },
            ExprKind::Let { bindings, body } => {
                let mut copies = Vec::with_capacity(bindings.len());
                for (local, init) in bindings {
                    let init = self.copy(init)?;
                    let copy = fresh(self.program, local);
                    self.parameters.push((local, Parameter::Local(copy)));
                    copies.push((copy, init));
                }
                ExprKind::Let {
                    bindings: copies,
                    body: self.copy(body)?,
                }
            }
            ExprKind::If { test, yes, no } => ExprKind::If {
                test: self.copy(test)?,
                yes: self.copy(yes)?,
                no: self.copy(no)?,
            },
            ExprKind::Do(items) => ExprKind::Do(self.all(&items)?),
            ExprKind::BooleanHint(inner) => ExprKind::BooleanHint(self.copy(inner)?),
            ExprKind::List(items) => ExprKind::List(self.all(&items)?),
            ExprKind::Vector(items) => ExprKind::Vector(self.all(&items)?),
            ExprKind::Map(items) => ExprKind::Map(self.all(&items)?),
            ExprKind::Set(items) => ExprKind::Set(self.all(&items)?),
            ExprKind::LiteralMap(items) => ExprKind::LiteralMap(self.all(&items)?),
            ExprKind::LiteralSet(items) => ExprKind::LiteralSet(self.all(&items)?),
            ExprKind::Call { callee, args } => {
                // A host path such as `js/Promise.resolve` called in place
                // keeps its object as `this`; the parameter's call had none.
                if let ExprKind::Local(local) = self.program.exprs[callee.0].kind
                    && let Some(Parameter::Value(argument) | Parameter::Moved(argument)) =
                        self.parameter(local)
                    && let ExprKind::JsGlobal(global) = self.program.exprs[argument.0].kind
                    && self.program.js_globals[global.0].path.len() > 1
                {
                    return None;
                }
                ExprKind::Call {
                    callee: self.copy(callee)?,
                    args: self.all(&args)?,
                }
            }
            ExprKind::Property { receiver, property } => ExprKind::Property {
                receiver: self.copy(receiver)?,
                property,
            },
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => ExprKind::MethodCall {
                receiver: self.copy(receiver)?,
                method,
                args: self.all(&args)?,
            },
            ExprKind::JsIndex { receiver, indices } => ExprKind::JsIndex {
                receiver: self.copy(receiver)?,
                indices: self.all(&indices)?,
            },
            ExprKind::Construct { callee, args } => ExprKind::Construct {
                callee: self.copy(callee)?,
                args: self.all(&args)?,
            },
            ExprKind::InstanceOf { class, value } => ExprKind::InstanceOf {
                class: self.copy(class)?,
                value: self.copy(value)?,
            },
            ExprKind::WithMeta { value, metadata } => ExprKind::WithMeta {
                value: self.copy(value)?,
                metadata: self.copy(metadata)?,
            },
            _ => return None,
        };
        // Map keys and set members that are not constants must not become
        // ones (fenced positions).
        let program = &*self.program;
        let keys = |kind: &ExprKind| -> Vec<ExprId> {
            match kind {
                ExprKind::Map(items) | ExprKind::LiteralMap(items) => {
                    items.iter().step_by(2).copied().collect()
                }
                ExprKind::Set(items) | ExprKind::LiteralSet(items) => items.clone(),
                _ => Vec::new(),
            }
        };
        let constant = |id: &ExprId| constant_like(program, &program.exprs[id.0].kind);
        if keys(&kind)
            .iter()
            .zip(&keys(&program.exprs[id.0].kind))
            .any(|(copy, original)| constant(copy) && !constant(original))
        {
            return None;
        }
        Some(self.push(id, kind))
    }

    /// A copy of the trivial argument `id`, its collection items included.
    fn duplicate(&mut self, id: ExprId) -> ExprId {
        let mut kind = self.program.exprs[id.0].kind.clone();
        if let ExprKind::Vector(items)
        | ExprKind::List(items)
        | ExprKind::Map(items)
        | ExprKind::Set(items)
        | ExprKind::LiteralMap(items)
        | ExprKind::LiteralSet(items) = &mut kind
        {
            for item in items.iter_mut() {
                *item = self.duplicate(*item);
            }
        }
        self.push(id, kind)
    }

    /// A new expression of `kind` where `like` was.
    fn push(&mut self, like: ExprId, kind: ExprKind) -> ExprId {
        let original = &self.program.exprs[like.0];
        let expr = Expr {
            kind,
            span: original.span.clone(),
            context: original.context,
            source_index: original.source_index,
        };
        self.program.exprs.push(expr);
        let copy = ExprId(self.program.exprs.len() - 1);
        let mark = self.built.get(like.0).copied().unwrap_or(false);
        self.built.resize(self.program.exprs.len(), false);
        self.built[copy.0] = mark;
        copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call_shapes;
    use crate::folding::built_literals;

    #[test]
    fn inline_remeasures_same_slice_shared_body() {
        let mut source = String::from(
            "(defn leaf [] 7)\n\
             (defn a [] [(leaf)])\n\
             (defn b [] [(leaf)])\n\
             (defn use-all [] [(a) (b) (b)])\n",
        );
        for i in 0..160 {
            source.push_str(&format!(
                "(defn pad{i} [x] (+ x {i} 1 2 3))\n(pad{i} {i})\n"
            ));
        }
        source.push_str("(use-all)\n");
        let mut base = crate::analyze(&source, &crate::parse(&source).unwrap()).unwrap();
        assert!(base.exprs.len() >= PARALLEL_EXPR_THRESHOLD);

        // Give `a` and `b` the exact same method body ExprId (within the same
        // early slice) so inlining `leaf` while visiting `a` mutates `b`'s body.
        let ExprKind::Do(ref items) = base.exprs[base.root.0].kind else {
            panic!("expected Do root");
        };
        let def_a = items[1];
        let def_b = items[2];
        let unwrap_fn = |program: &Program, def_id: ExprId| -> ExprId {
            let mut cur = match program.exprs[def_id.0].kind {
                ExprKind::Def {
                    init: Some(init), ..
                } => init,
                ref other => panic!("expected Def, got {other:?}"),
            };
            while let ExprKind::Do(ref inner) = program.exprs[cur.0].kind {
                cur = *inner.last().expect("non-empty Do");
            }
            assert!(
                matches!(program.exprs[cur.0].kind, ExprKind::Fn { .. }),
                "expected Fn, got {:?}",
                program.exprs[cur.0].kind
            );
            cur
        };
        let a_fn = unwrap_fn(&base, def_a);
        let b_fn = unwrap_fn(&base, def_b);
        let a_body = match &base.exprs[a_fn.0].kind {
            ExprKind::Fn { methods, .. } => methods[0].body,
            _ => unreachable!(),
        };
        if let ExprKind::Fn {
            ref mut methods, ..
        } = base.exprs[b_fn.0].kind
        {
            methods[0].body = a_body;
        }

        let run = |n: usize| {
            let mut program = base.clone();
            let mut built = built_literals(&program);
            let mut top = Vec::new();
            flatten(&program, program.root, &mut top);
            let facts = ProgramFacts::new_light(&program, &top);
            let shapes = call_shapes::analyze(&program, None, &[]);
            let mut growth = growth_budget(program.exprs.len());
            let inlined = inline_with_facts_with_slots(
                &mut program,
                &shapes,
                &mut growth,
                &mut built,
                &facts,
                Slots::new(n).unwrap(),
            );
            (inlined, growth)
        };

        let (inlined1, growth1) = run(1);
        for n in [2, 4, 8] {
            let (inlinedn, growthn) = run(n);
            assert_eq!(inlinedn, inlined1, "inlined count mismatch at slots={n}");
            assert_eq!(growthn, growth1, "remaining growth mismatch at slots={n}");
        }
    }

    #[test]
    fn small_inline_program_keeps_measurement_inline() {
        let mut source = String::new();
        for i in 0..16 {
            source.push_str(&format!("(defn f{i} [x] (+ x {i}))\n"));
        }
        source.push('[');
        for i in 0..16 {
            source.push_str(&format!("(f{i} {i}) "));
        }
        source.push(']');
        let base = crate::analyze(&source, &crate::parse(&source).unwrap()).unwrap();
        assert!(
            base.exprs.len() < PARALLEL_EXPR_THRESHOLD,
            "fixture must stay below parallel expr threshold: {}",
            base.exprs.len()
        );

        let run = |slots: Slots| {
            let mut program = base.clone();
            let mut built = built_literals(&program);
            let mut top = Vec::new();
            flatten(&program, program.root, &mut top);
            let facts = ProgramFacts::new_light(&program, &top);
            let shapes = call_shapes::analyze(&program, None, &[]);
            let mut growth = growth_budget(program.exprs.len());
            let inlined = inline_with_facts_with_slots(
                &mut program,
                &shapes,
                &mut growth,
                &mut built,
                &facts,
                slots,
            );
            (inlined, growth)
        };

        assert_eq!(run(Slots::new(8).unwrap()), run(Slots::ONE));
    }

    #[test]
    fn parallel_inline_eligibility_matches_serial() {
        let mut source = String::from(
            "(declare later)\n\
             (defn before-use [] (later 1))\n\
             (defn later [x] (+ x 1))\n\
             (defn escaped [x] (+ x 2))\n\
             (def esc-ref escaped)\n\
             (defn fixed2 [x y] (+ x y))\n\
             (defn with-cap [x] (let [f (fn [y] (+ x y))] (f 3)))\n",
        );
        for i in 0..160 {
            source.push_str(&format!(
                "(defn ok{i} [x] (+ x {i}))\n\
                 (defn caller{i} [x] [(ok{i} x) (ok{i} 1) (escaped x) (fixed2 x 1) (with-cap x) (later x)])\n"
            ));
        }
        source.push_str("[(before-use) esc-ref ");
        for i in 0..160 {
            source.push_str(&format!("(caller{i} {i}) "));
        }
        source.push(']');

        let base = crate::analyze(&source, &crate::parse(&source).unwrap()).unwrap();
        assert!(base.exprs.len() >= PARALLEL_EXPR_THRESHOLD);

        let run = |slots: Slots| {
            let mut program = base.clone();
            let mut built = built_literals(&program);
            let mut top = Vec::new();
            flatten(&program, program.root, &mut top);
            let facts = ProgramFacts::new_light(&program, &top);
            let shapes = call_shapes::analyze(&program, None, &[]);
            let mut growth = growth_budget(program.exprs.len());
            let inlined = inline_with_facts_with_slots(
                &mut program,
                &shapes,
                &mut growth,
                &mut built,
                &facts,
                slots,
            );
            (inlined, growth)
        };

        let expected = run(Slots::ONE);
        assert!(
            expected.0 > 160,
            "expected eligible calls to inline: {expected:?}"
        );
        for n in [2, 4, 8] {
            assert_eq!(
                run(Slots::new(n).unwrap()),
                expected,
                "eligibility mismatch at slots={n}"
            );
        }
    }
}
