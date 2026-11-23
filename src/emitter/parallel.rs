//! Parallel single-chunk release emission (`:advanced`, `:prepare`, `:none`)
//! and shared precomputed read-only tables reused by chunked emission
//! (light-cljs-1097).
use super::*;
use crate::scheduler::Slots;

/// Minimum total top-level expression weight before slicing a single-chunk
/// release build across worker threads. Smaller programs stay on the inline
/// serial path with zero pool overhead.
const PARALLEL_EXPR_THRESHOLD: u64 = 2048;

/// Minimum number of non-trivial top-level forms before slicing.
const PARALLEL_FORM_THRESHOLD: usize = 16;

/// Target slices per CPU slot: `4 * slots` gives LPT scheduling room to smooth
/// out heavy namespaces while keeping peak `Resolution` memory in `terse` to
/// at most `slots / (4 * slots) = 25%` of the program.
const SLICES_PER_SLOT: usize = 4;

/// Emitted JavaScript text before [`crate::runtime_key::RuntimeKey`]
/// substitution: either one string (serial or source-mapped) or
/// statement-boundary pieces ready for streaming SHA-256 and parallel
/// placeholder substitution.
pub(crate) enum EmittedCode {
    Single(String),
    Pieces {
        pieces: Vec<String>,
        total_len: usize,
        lkey_counts: Option<crate::runtime_key::StemCounts>,
    },
}

/// Pre-computed read-only tables shared across all slice/chunk [`Emitter`]s so
/// parallel emission avoids `O(P * |program|)` duplicate allocations and scans.
pub(super) struct SharedTables<'a> {
    pub(super) js_exposed_locals: Vec<bool>,
    pub(super) assignable_locals: Vec<bool>,
    pub(super) dispatchers: Option<HashMap<&'a str, GlobalId>>,
    pub(super) scope_flags: Vec<u8>,
    pub(super) constant_classes: Vec<ConstantClass>,
}

impl<'a> SharedTables<'a> {
    pub(super) fn new(program: &'a Program, direct: Option<&[&str]>) -> Self {
        let (js_exposed_locals, assignable_locals) =
            scan_local_flags(program, true, direct.is_some());
        if let Some(replaced) = direct {
            let dispatchers = program
                .protocols
                .values()
                .flatten()
                .map(|method| (method.key.as_str(), method.global))
                .collect();
            let scope_flags = compute_scope_flags(program);
            let constant_classes = compute_constant_classes(program, replaced);
            Self {
                js_exposed_locals,
                assignable_locals,
                dispatchers: Some(dispatchers),
                scope_flags,
                constant_classes,
            }
        } else {
            Self {
                js_exposed_locals,
                assignable_locals,
                dispatchers: None,
                scope_flags: Vec::new(),
                constant_classes: Vec::new(),
            }
        }
    }
}

fn compute_scope_flags(program: &Program) -> Vec<u8> {
    let mut flags = vec![0u8; program.exprs.len()];
    let mut pending = Vec::new();
    for root in 0..program.exprs.len() {
        if flags[root] & SCOPE_DONE != 0 {
            continue;
        }
        pending.push((ExprId(root), false));
        while let Some((id, expanded)) = pending.pop() {
            if flags[id.0] & SCOPE_DONE != 0 {
                continue;
            }
            let own = match program.exprs[id.0].kind {
                ExprKind::JsArguments
                | ExprKind::CopyArguments(_)
                | ExprKind::JsTemplate { .. }
                | ExprKind::ThisAs { .. }
                | ExprKind::Def { test: Some(_), .. } => Some(SCOPE_READ),
                ExprKind::Fn { .. } | ExprKind::Reify { .. } => Some(SCOPE_CLOSURE),
                ExprKind::DefType { .. } | ExprKind::ExtendType { .. } => {
                    Some(SCOPE_READ | SCOPE_CLOSURE)
                }
                _ if program.namespace_entries.contains_key(&id) => Some(SCOPE_READ),
                _ => None,
            };
            if let Some(own) = own {
                flags[id.0] = own | SCOPE_DONE;
                continue;
            }
            if expanded {
                let statement = match program.exprs[id.0].kind {
                    ExprKind::Loop { .. }
                    | ExprKind::Case { default: None, .. }
                    | ExprKind::Throw(_)
                    | ExprKind::Binding { .. }
                    | ExprKind::WithOutStr(_)
                    | ExprKind::Try { .. } => SCOPE_STATEMENT,
                    _ => 0,
                };
                let mut computed = SCOPE_DONE | statement;
                visit_children(&program.exprs[id.0].kind, |child| {
                    computed |= flags[child.0];
                });
                flags[id.0] = computed;
            } else {
                pending.push((id, true));
                visit_children(&program.exprs[id.0].kind, |child| {
                    if flags[child.0] & SCOPE_DONE == 0 {
                        pending.push((child, false));
                    }
                });
            }
        }
    }
    flags
}

fn compute_constant_classes(program: &Program, replaced: &[&str]) -> Vec<ConstantClass> {
    let mut classes = vec![ConstantClass::Unknown; program.exprs.len()];
    for id in 0..program.exprs.len() {
        if classes[id] == ConstantClass::Unknown {
            constant_nodes(program, replaced, &mut classes, ExprId(id));
        }
    }
    classes
}

/// Memoized subtree expression count for load-balancing slices.
pub(super) fn expr_weight(
    program: &Program,
    root: ExprId,
    memo: &mut [u32],
    stack: &mut Vec<(ExprId, bool)>,
) -> u64 {
    if memo[root.0] != 0 {
        return u64::from(memo[root.0]);
    }
    stack.clear();
    stack.push((root, false));
    while let Some((id, expanded)) = stack.pop() {
        if memo[id.0] != 0 {
            continue;
        }
        if expanded {
            let mut sum = 1u32;
            visit_children(&program.exprs[id.0].kind, |child| {
                sum = sum.saturating_add(memo[child.0]);
            });
            memo[id.0] = sum;
        } else {
            stack.push((id, true));
            visit_children(&program.exprs[id.0].kind, |child| {
                if memo[child.0] == 0 {
                    stack.push((child, false));
                }
            });
        }
    }
    u64::from(memo[root.0])
}

/// Partition `weights` into up to `target_slices` contiguous, weight-balanced
/// non-empty ranges covering `0..weights.len()`.
pub(super) fn partition_slices(
    weights: &[u64],
    target_slices: usize,
) -> Vec<std::ops::Range<usize>> {
    let n = weights.len();
    if n == 0 {
        return Vec::new();
    }
    let slices = target_slices.clamp(1, n);
    if slices == 1 {
        return std::iter::once(0..n).collect();
    }
    let mut remaining_weight: u64 = weights.iter().sum();
    let mut ranges = Vec::with_capacity(slices);
    let mut start = 0usize;
    for remaining_slices in (1..=slices).rev() {
        if start >= n {
            break;
        }
        if remaining_slices == 1 {
            ranges.push(start..n);
            break;
        }
        let target = remaining_weight / remaining_slices as u64;
        let limit = n - (remaining_slices - 1);
        let mut acc = 0u64;
        let mut end = start;
        while end < limit {
            let w = weights[end];
            let next = acc + w;
            if end > start && next > target && next - target > target.saturating_sub(acc) {
                break;
            }
            acc = next;
            end += 1;
            if acc >= target && acc > 0 {
                break;
            }
        }
        if end == start {
            acc = weights[end];
            end += 1;
        }
        ranges.push(start..end);
        remaining_weight = remaining_weight.saturating_sub(acc);
        start = end;
    }
    ranges
}

/// Remap nested `ConstantPart::Entry(local)` references in `key` using `remap`.
pub(super) fn remap_constant_key<'a>(key: &ConstantKey<'a>, remap: &[usize]) -> ConstantKey<'a> {
    let remap_parts = |parts: &[ConstantPart<'a>]| -> Box<[ConstantPart<'a>]> {
        parts
            .iter()
            .map(|&part| match part {
                ConstantPart::Entry(local) => ConstantPart::Entry(remap[local]),
                other => other,
            })
            .collect()
    };
    match key {
        ConstantKey::Keyword { namespace, name } => ConstantKey::Keyword {
            namespace: *namespace,
            name: name.clone(),
        },
        ConstantKey::Symbol { namespace, name } => ConstantKey::Symbol {
            namespace: *namespace,
            name,
        },
        ConstantKey::Composite { constructor, parts } => ConstantKey::Composite {
            constructor: *constructor,
            parts: remap_parts(parts),
        },
        ConstantKey::Shape { set, keys } => ConstantKey::Shape {
            set: *set,
            keys: remap_parts(keys),
        },
        ConstantKey::Method { function, method } => ConstantKey::Method {
            function: *function,
            method: *method,
        },
    }
}

/// Rewrite slice-local `k{slice_id}_{local}` references in `text` (outside
/// `raw` segments) to their canonical `k{canon}` spellings.
pub(super) fn rewrite_slice_k_names(
    text: &str,
    raw: &[std::ops::Range<usize>],
    k_prefix: &str,
    remap: &[usize],
) -> (String, Vec<(usize, usize)>) {
    crate::terse::rename_compiler_names(text, raw, |word| {
        if let Some(rest) = word.strip_prefix(k_prefix)
            && let Ok(local) = rest.parse::<usize>()
            && let Some(&canon) = remap.get(local)
        {
            format!("k{canon}")
        } else {
            word.to_owned()
        }
    })
}

/// One flattened top-level item of `program.root`.
#[derive(Clone, Copy)]
enum TopItem {
    NamespaceEntry(ExprId),
    Form { id: ExprId, context: ExprContext },
    ReturnNull(ExprId),
}

fn flatten_top(program: &Program, id: ExprId, context: ExprContext, out: &mut Vec<TopItem>) {
    if let ExprKind::Do(body) = &program.exprs[id.0].kind {
        if program.namespace_entries.contains_key(&id) {
            out.push(TopItem::NamespaceEntry(id));
        }
        if body.is_empty() {
            if context == ExprContext::Return {
                out.push(TopItem::ReturnNull(id));
            }
            return;
        }
        for (index, &child) in body.iter().enumerate() {
            let child_context = if index + 1 == body.len() {
                context
            } else {
                ExprContext::Discard
            };
            flatten_top(program, child, child_context, out);
        }
    } else {
        out.push(TopItem::Form { id, context });
    }
}

/// Output of one parallel AST → JS slice in Stage 1.
struct SliceOutput<'a> {
    output: String,
    raw_segments: Vec<std::ops::Range<usize>>,
    marks: Option<Marks>,
    runtime: RuntimeRoots,
    core_cells: Vec<(&'static str, RuntimeName)>,
    imports: Vec<PackageImport>,
    declared_in_place: HashSet<String>,
    operands_declared: usize,
    record_kit_sites: Vec<RecordKitSite>,
    constants: Vec<LocalConstant<'a>>,
    has_raw_js: bool,
    needs_runtime: bool,
    needs_host: bool,
    needs_package: bool,
    truth_used: bool,
}

/// Emit a prepared single-chunk application using `slots` CPU work slots
/// (`--jobs N`). Keeps `Slots::ONE` and small programs on the inline serial
/// path with zero pool overhead.
pub(crate) fn emit_application_with_slots(
    program: &Program,
    application: Application<'_>,
    module: bool,
    mapped: bool,
    count_lkeys: bool,
    slots: Slots,
) -> (EmittedCode, EmittedRuntime, Option<SourceMapping>) {
    if slots == Slots::ONE {
        let (code, runtime, mapping) = emit_application(program, application, module, mapped);
        return (EmittedCode::Single(code), runtime, mapping);
    }
    emit_application_parallel(
        program,
        application,
        module,
        mapped,
        count_lkeys,
        slots,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn emit_application_parallel<'a>(
    program: &'a Program,
    application: Application<'a>,
    module: bool,
    mapped: bool,
    count_lkeys: bool,
    slots: Slots,
    force: bool,
) -> (EmittedCode, EmittedRuntime, Option<SourceMapping>) {
    let direct = application.direct;
    let slice = direct.is_some();
    let terse = slice && application.terse;

    if !matches!(program.exprs[program.root.0].kind, ExprKind::Do(_)) {
        let (code, runtime, mapping) = emit_application(program, application, module, mapped);
        return (EmittedCode::Single(code), runtime, mapping);
    }

    let mut items = Vec::new();
    flatten_top(program, program.root, ExprContext::Return, &mut items);

    let mut memo = vec![0u32; program.exprs.len()];
    let mut stack = Vec::new();
    let mut weights = Vec::with_capacity(items.len());
    let mut total_weight = 0u64;
    let mut active_items = 0usize;
    for &item in &items {
        let w = match item {
            TopItem::NamespaceEntry(_) | TopItem::ReturnNull(_) => 1,
            TopItem::Form { id, context } => {
                if slice
                    && context == ExprContext::Discard
                    && !program.namespace_entries.contains_key(&id)
                    && matches!(
                        program.exprs[id.0].kind,
                        ExprKind::Nil
                            | ExprKind::Def {
                                init: None,
                                test: None,
                                once: false,
                                ..
                            }
                    )
                {
                    0
                } else {
                    expr_weight(program, id, &mut memo, &mut stack)
                }
            }
        };
        if w > 0 {
            active_items += 1;
            total_weight += w;
        }
        weights.push(w);
    }
    drop(memo);
    drop(stack);

    if !force && (total_weight < PARALLEL_EXPR_THRESHOLD || active_items < PARALLEL_FORM_THRESHOLD)
    {
        let (code, runtime, mapping) = emit_application(program, application, module, mapped);
        return (EmittedCode::Single(code), runtime, mapping);
    }

    let target_slices = (slots.get() * SLICES_PER_SLOT).min(active_items).max(1);
    let slice_ranges = partition_slices(&weights, target_slices);
    if slice_ranges.len() <= 1 {
        let (code, runtime, mapping) = emit_application(program, application, module, mapped);
        return (EmittedCode::Single(code), runtime, mapping);
    }

    let slice_costs: Vec<u64> = slice_ranges
        .iter()
        .map(|range| weights[range.clone()].iter().sum())
        .collect();
    drop(weights);

    // Pre-compute read-only shared tables once before Stage 1 fan-out.
    let shared_tables = SharedTables::new(program, direct);
    let function_names = mapped.then(|| FunctionNames::new(program, 0));

    let run_stage1 = |register_namespaces: bool| -> Vec<SliceOutput<'a>> {
        let jobs: Vec<(u64, (usize, std::ops::Range<usize>))> = slice_ranges
            .iter()
            .cloned()
            .enumerate()
            .map(|(idx, range)| (slice_costs[idx], (idx, range)))
            .collect();
        crate::scheduler::map(slots, jobs, |(slice_idx, range)| {
            let mut emitter = Emitter::new(
                program,
                Setup {
                    demand_roots: false,
                    application_roots: false,
                    module,
                    mapped,
                    application: Some(application),
                    direct,
                    function_names: function_names.as_ref(),
                    js_exposed_locals: None,
                    unit_root: None,
                    shared_tables: Some(&shared_tables),
                    constant_slice: slice.then_some(slice_idx),
                },
                register_namespaces,
            );
            if slice {
                emitter.declared_in_place = Some(HashSet::new());
            }
            emitter.operands = Some(Operands {
                hoists: false,
                ..Operands::default()
            });
            for &item in &items[range] {
                match item {
                    TopItem::NamespaceEntry(id) => {
                        let prev_site = emitter.site.replace(id);
                        emitter.mark(Some(id));
                        emitter.namespace_entry(id);
                        emitter.site = prev_site;
                    }
                    TopItem::ReturnNull(id) => {
                        let prev_site = emitter.site.replace(id);
                        emitter.mark(Some(id));
                        emitter.indentation();
                        emitter.return_open();
                        emitter.output.push_str("null");
                        emitter.return_close();
                        emitter.output.push_str(";\n");
                        emitter.site = prev_site;
                    }
                    TopItem::Form { id, context } => {
                        if slice {
                            emitter.top_statement = true;
                        }
                        emitter.emit(id, context);
                        emitter.top_statement = false;
                    }
                }
            }
            let operands_declared = emitter.operands.take().map_or(0, |o| o.declared);
            SliceOutput {
                output: emitter.output,
                raw_segments: emitter.raw_segments,
                marks: emitter.marks,
                runtime: emitter.runtime,
                core_cells: emitter.core_cells,
                imports: emitter.imports.unwrap_or_default(),
                declared_in_place: emitter.declared_in_place.unwrap_or_default(),
                operands_declared,
                record_kit_sites: emitter.record_kit_sites,
                constants: emitter.constants.map(|c| c.entries).unwrap_or_default(),
                has_raw_js: emitter.has_raw_js,
                needs_runtime: emitter.needs_runtime,
                needs_host: emitter.needs_host,
                needs_package: emitter.needs_package,
                truth_used: emitter.truth_used,
            }
        })
    };

    let merge_runtime_and_app_roots = |slice_outputs: &[SliceOutput<'a>]| -> RuntimeRoots {
        let mut merged = RuntimeRoots::default();
        for s in slice_outputs {
            merged.extend(&s.runtime);
        }
        if slice && !merged.is_empty() {
            record_application_roots_into(&mut merged, application);
        }
        merged
    };

    // Stage 1: Parallel AST → JS emission, with reflection check at `:advanced`.
    let mut register_namespaces = !slice;
    let mut slice_outputs = run_stage1(register_namespaces);
    let mut merged_runtime = merge_runtime_and_app_roots(&slice_outputs);
    if slice && merged_runtime.reach_reflection() {
        register_namespaces = true;
        slice_outputs = run_stage1(true);
        merged_runtime = merge_runtime_and_app_roots(&slice_outputs);
    }

    // Drop Stage 1 read-only tables before building constants and entering Stage 2.
    drop(shared_tables);
    drop(function_names);
    drop(items);

    // Ordered merge of slice metadata `0..P`.
    let mut merged_core_cells: Vec<(&'static str, RuntimeName)> = Vec::new();
    let mut merged_imports: Vec<PackageImport> = Vec::new();
    let mut merged_declared_in_place: HashSet<String> = HashSet::new();
    let mut max_operands = 0usize;
    let mut has_raw_js = false;
    let mut needs_runtime = false;
    let mut needs_host = false;
    let mut needs_package = false;
    let mut truth_used = false;
    for s in &mut slice_outputs {
        for &(name, initial) in &s.core_cells {
            if !merged_core_cells
                .iter()
                .any(|(existing, _)| *existing == name)
            {
                merged_core_cells.push((name, initial));
            }
        }
        if module {
            for import in std::mem::take(&mut s.imports) {
                let index = Emitter::package_import(&mut merged_imports, &import.package);
                merged_imports[index].namespace |= import.namespace;
                for export in import.exports {
                    if !merged_imports[index].exports.contains(&export) {
                        merged_imports[index].exports.push(export);
                    }
                }
            }
        }
        merged_declared_in_place.extend(std::mem::take(&mut s.declared_in_place));
        max_operands = max_operands.max(s.operands_declared);
        has_raw_js |= s.has_raw_js;
        needs_runtime |= s.needs_runtime;
        needs_host |= s.needs_host;
        needs_package |= s.needs_package;
        truth_used |= s.truth_used;
    }

    // Ordered canonical merge of per-slice constant tables (`0..P`).
    let mut canon_numbers: HashMap<(ConstantKey<'a>, bool), usize> = HashMap::new();
    let mut canon_lazy: Vec<bool> = Vec::new();
    let mut canon_composites = 0usize;
    let mut canon_functions = 0usize;
    let mut canon_declarations = String::new();
    let mut canon_marks: Vec<Mark> = Vec::new();
    let mut canon_record_kit_sites: Vec<RecordKitSite> = Vec::new();
    let mut remaps: Vec<Vec<usize>> = Vec::with_capacity(slice_outputs.len());

    if slice {
        for (slice_idx, s) in slice_outputs.iter_mut().enumerate() {
            let mut entries = std::mem::take(&mut s.constants);
            let mut remap = Vec::with_capacity(entries.len());
            let mut new_entries: Vec<(usize, usize)> = Vec::new();
            for (local_id, entry) in entries.iter().enumerate() {
                let canon_key = remap_constant_key(&entry.key, &remap);
                let dedup_key = (canon_key, entry.lazy);
                if let Some(&cid) = canon_numbers.get(&dedup_key) {
                    remap.push(cid);
                } else {
                    let cid = canon_lazy.len();
                    canon_lazy.push(entry.lazy);
                    canon_composites +=
                        usize::from(matches!(entry.key, ConstantKey::Composite { .. }));
                    canon_functions += usize::from(matches!(entry.key, ConstantKey::Method { .. }));
                    canon_numbers.insert(dedup_key, cid);
                    remap.push(cid);
                    new_entries.push((local_id, cid));
                }
            }
            // Emit new canonical entries in finalization order so composite
            // children precede their parents and hoisted method bodies follow
            // the constants they reference.
            new_entries.sort_unstable_by_key(|&(local_id, _)| entries[local_id].finalized_order);
            let k_prefix = format!("k{slice_idx}_");
            for (local_id, cid) in new_entries {
                let entry = &mut entries[local_id];
                let needs_rewrite = matches!(
                    entry.key,
                    ConstantKey::Composite { .. }
                        | ConstantKey::Shape { .. }
                        | ConstantKey::Method { .. }
                ) && entry.value.contains(&k_prefix);
                if needs_rewrite {
                    let (rewritten, anchors) =
                        rewrite_slice_k_names(&entry.value, &[], &k_prefix, &remap);
                    if mapped {
                        for mark in &mut entry.marks {
                            mark.output = crate::terse::renamed_offset(&anchors, mark.output);
                        }
                    }
                    for site in &mut entry.record_kit_sites {
                        site.range.start = crate::terse::renamed_offset(&anchors, site.range.start);
                        site.range.end = crate::terse::renamed_offset(&anchors, site.range.end);
                    }
                    entry.value = rewritten;
                }
                let decl = if entry.lazy {
                    format!("  let c{cid}; const k{cid} = () => c{cid} ??= ")
                } else {
                    format!("  const k{cid} = ")
                };
                let at = canon_declarations.len() + decl.len();
                if mapped {
                    canon_marks.extend(entry.marks.drain(..).map(|mark| Mark {
                        output: mark.output + at,
                        ..mark
                    }));
                }
                for mut site in entry.record_kit_sites.drain(..) {
                    site.range.start += at;
                    site.range.end += at;
                    canon_record_kit_sites.push(site);
                }
                writeln!(canon_declarations, "{decl}{};", entry.value).unwrap();
            }
            remaps.push(remap);
        }
    } else {
        remaps.resize_with(slice_outputs.len(), Vec::new);
    }
    // Record kit splicing: check merged roots once, intern field keyword
    // constants, then splice only into affected slices (in parallel) and into
    // hoisted methods in `canon_declarations`.
    let any_record_sites = !canon_record_kit_sites.is_empty()
        || slice_outputs.iter().any(|s| !s.record_kit_sites.is_empty());
    if slice && any_record_sites {
        let kits = record_kit_parts(merged_runtime.record_kits(), application.inference);
        if kits.any() {
            let mut field_keywords: HashMap<&'a str, usize> = HashMap::new();
            if kits.lookup || kits.assoc {
                for site in slice_outputs
                    .iter()
                    .flat_map(|s| s.record_kit_sites.iter())
                    .chain(&canon_record_kit_sites)
                {
                    let ExprKind::DefType { layout, .. } = &program.exprs[site.expr.0].kind else {
                        unreachable!("a record definition")
                    };
                    for field in &layout.fields {
                        if field_keywords.contains_key(field.as_str()) {
                            continue;
                        }
                        let units: Vec<u16> = field.encode_utf16().collect();
                        let dedup_key = (
                            ConstantKey::Keyword {
                                namespace: None,
                                name: Cow::Owned(units),
                            },
                            false,
                        );
                        let cid = if let Some(&number) = canon_numbers.get(&dedup_key) {
                            number
                        } else {
                            let number = canon_lazy.len();
                            let kw = record_direct_runtime_read(
                                &mut merged_runtime,
                                "keyword",
                                Some(site.expr),
                            );
                            write!(canon_declarations, "  const k{number} = {kw}(null, ").unwrap();
                            write_string(&mut canon_declarations, field.encode_utf16(), true);
                            canon_declarations.push_str(");\n");
                            canon_lazy.push(false);
                            canon_numbers.insert(dedup_key, number);
                            number
                        };
                        field_keywords.insert(field.as_str(), cid);
                    }
                }
            }
            let splice_jobs: Vec<(u64, SliceOutput<'a>)> = slice_outputs
                .into_iter()
                .map(|s| (s.output.len() as u64, s))
                .collect();
            let spliced = crate::scheduler::map(slots, splice_jobs, |mut s| {
                let mut added_roots = RuntimeRoots::default();
                if !s.record_kit_sites.is_empty() {
                    let sites = std::mem::take(&mut s.record_kit_sites);
                    splice_record_kits_into(
                        program,
                        &mut s.output,
                        &mut s.raw_segments,
                        s.marks.as_mut().map(|m| m.list.as_mut_slice()),
                        sites,
                        kits,
                        &field_keywords,
                        &mut added_roots,
                    );
                }
                (s, added_roots)
            });
            slice_outputs = Vec::with_capacity(spliced.len());
            for (s, added) in spliced {
                merged_runtime.extend(&added);
                slice_outputs.push(s);
            }
            if !canon_record_kit_sites.is_empty() {
                splice_record_kits_into(
                    program,
                    &mut canon_declarations,
                    &mut [],
                    (!canon_marks.is_empty()).then_some(canon_marks.as_mut_slice()),
                    canon_record_kit_sites,
                    kits,
                    &field_keywords,
                    &mut merged_runtime,
                );
            }
        }
    }
    drop(canon_numbers);

    #[cfg(debug_assertions)]
    {
        for s in &slice_outputs {
            check_runtime_reads(
                &s.output,
                &s.raw_segments,
                &merged_runtime,
                &merged_core_cells,
            );
        }
        check_runtime_reads(
            &canon_declarations,
            &[],
            &merged_runtime,
            &merged_core_cells,
        );
    }

    // Decide whether the output embeds a runtime.
    let embed = if slice {
        !merged_runtime.is_empty()
    } else {
        !program.namespace_entries.is_empty() || needs_runtime
    };
    debug_assert!(
        embed || merged_runtime.is_empty(),
        "emitted code reads the runtime without embedding it"
    );

    // Emit namespace registration (when embedded and needed) BEFORE slicing
    // the runtime or writing `core_cells`, so registration roots & core cells
    // are included in the slice.
    let registration = if embed {
        if slice {
            merged_runtime.record(
                RuntimeName::direct(RUNTIME_KEY_UNIT, None),
                None,
                NameUse::Keep,
            );
        }
        if register_namespaces {
            let mut reg_emitter = Emitter::new(
                program,
                Setup {
                    demand_roots: false,
                    application_roots: false,
                    module: false,
                    mapped: false,
                    application: Some(application),
                    direct,
                    function_names: None,
                    js_exposed_locals: None,
                    unit_root: None,
                    shared_tables: None,
                    constant_slice: None,
                },
                true,
            );
            reg_emitter.runtime = std::mem::take(&mut merged_runtime);
            reg_emitter.core_cells = std::mem::take(&mut merged_core_cells);
            let retained = |global: usize| !slice || application.globals[global];
            let reg = reg_emitter.registration(None, &[], true, retained, None);
            merged_runtime = reg_emitter.runtime;
            merged_core_cells = reg_emitter.core_cells;
            reg
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Build header slice `H` (without `PRELUDE_OPEN` / `package_imports`).
    let mut header = String::new();
    let mut header_marks = mapped.then(Marks::default);
    if application.inference.is_none() || truth_used {
        header.push_str(PRELUDE_TRUTH);
    }

    let mut embedding = RuntimeEmbedding::None;
    let mut runtime_bytes = 0usize;
    let mut unit_bytes: Option<Vec<usize>> = None;
    let mut header_runtime_segment: Option<std::ops::Range<usize>> = None;
    let mut header_runtime_statement: std::ops::Range<usize> = 0..0;
    let mut parallel_runtime_kept: Option<Vec<bool>> = None;
    let mut parallel_runtime_bindings: Option<Vec<&'static str>> = None;

    if embed {
        if slice {
            embedding = RuntimeEmbedding::Slice;
            if terse {
                parallel_runtime_kept = Some(crate::runtime_slice::sliced_units(&merged_runtime));
                parallel_runtime_bindings = Some(merged_runtime.direct_bindings());
            } else {
                // Readable `:advanced --pseudo-names` runtime factory inside `H`.
                let runtime_start = header.len();
                let (factory, bindings, bytes) =
                    crate::runtime_slice::sliced_runtime(&merged_runtime, false);
                unit_bytes = bytes.map(crate::runtime_slice::written_unit_bytes);
                runtime_bytes = crate::runtime_key::written_len(factory.trim());
                let pattern = bindings
                    .iter()
                    .map(|binding| format!("rt${binding}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let statement = format!("  const [{pattern}] = {};\n", factory.trim());
                let factory_offset = statement
                    .find("(() =>")
                    .or_else(|| statement.find("(()=>"))
                    .unwrap_or_default();
                if let Some(marks) = &mut header_marks {
                    let at = statement.len() - 2 - factory.trim_end().len();
                    let mut runtime_marks = Marks::default();
                    runtime_marks.readable_slice(&factory, at);
                    marks.absorb(runtime_marks, runtime_start);
                }
                header.push_str(&statement);
                header_runtime_segment =
                    Some(runtime_start + factory_offset..runtime_start + statement.len() - 2);
                header_runtime_statement = runtime_start..runtime_start + statement.len();
            }
        } else {
            embedding = RuntimeEmbedding::Whole;
            let runtime_start = header.len();
            let whole = crate::runtime_manifest::RUNTIME.trim();
            runtime_bytes = crate::runtime_key::written_len(whole);
            let statement = format!("  const rt = {whole};\n");
            let factory_offset = statement
                .find("(() =>")
                .or_else(|| statement.find("(()=>"))
                .unwrap_or_default();
            if let Some(marks) = &mut header_marks {
                let mut runtime_marks = Marks::default();
                runtime_marks.whole_runtime("  const rt = ".len());
                marks.absorb(runtime_marks, runtime_start);
            }
            header.push_str(&statement);
            header_runtime_segment =
                Some(runtime_start + factory_offset..runtime_start + statement.len() - 2);
            header_runtime_statement = runtime_start..runtime_start + statement.len();
        }
    }

    let after_runtime = header.len();
    for (name, initial) in &merged_core_cells {
        writeln!(header, "  let core${name} = {initial};").unwrap();
    }
    let constants_at = header.len();
    header.push_str(&canon_declarations);
    let registration_at = header.len();
    header.push_str(&registration);
    let after_registration = header.len();
    if needs_host || needs_package {
        header.push_str(&helpers(needs_host, needs_package));
    }
    // Top-level `let g<n>;` declarations for globals not declared in place.
    let shapes = application.shapes.filter(|_| slice);
    let mut declare = |name: String| {
        if !merged_declared_in_place.contains(&name) {
            writeln!(header, "  let {name};").unwrap();
        }
    };
    for global in 0..program.globals.len() {
        if !application.globals[global] {
            continue;
        }
        if let Some(FunctionShape::Entries {
            function,
            dispatcher,
            ..
        }) = shapes.map(|shapes| shapes.function(GlobalId(global)))
        {
            if dispatcher {
                declare(format!("g{global}"));
            }
            let ExprKind::Fn { methods, .. } = &program.exprs[function.0].kind else {
                unreachable!("method entries of a fn")
            };
            let shapes = shapes.expect("entries come from call shapes");
            for index in 0..methods.len() {
                if shapes.entry_used(GlobalId(global), index) {
                    declare(format!("g{global}_{index}"));
                }
            }
            continue;
        }
        declare(format!("g{global}"));
    }
    if max_operands > 0 {
        header.push_str("  let ");
        for index in 0..max_operands {
            if index > 0 {
                header.push_str(", ");
            }
            write!(header, "operand{index}").unwrap();
        }
        header.push_str(";\n");
    }

    // Source-map marks for `H` and per-slice compiler-name original names.
    if let Some(marks) = &mut header_marks {
        marks.place(&canon_marks, constants_at);
        for at in [after_runtime, registration_at, after_registration] {
            marks.unmapped(at);
        }
        let skipped = if !header_runtime_statement.is_empty() {
            vec![header_runtime_statement.clone()]
        } else {
            Vec::new()
        };
        for (range, word) in crate::terse::scanned_names(&header, &skipped) {
            if let Some(name) = original_name(program, word) {
                marks.named(range.start, None, name);
            }
        }
    }
    if mapped {
        let name_jobs: Vec<(u64, SliceOutput<'a>)> = slice_outputs
            .into_iter()
            .map(|s| (s.output.len() as u64, s))
            .collect();
        slice_outputs = crate::scheduler::map(slots, name_jobs, |mut s| {
            if let Some(marks) = &mut s.marks {
                for (range, word) in crate::terse::scanned_names(&s.output, &s.raw_segments) {
                    if let Some(name) = original_name(program, word) {
                        marks.named(range.start, None, name);
                    }
                }
            }
            s
        });
    }

    let constants_pool = ConstantPool {
        entries: canon_lazy.len(),
        composites: canon_composites,
        functions: canon_functions,
        lazy: canon_lazy.iter().filter(|lazy| **lazy).count(),
        bytes: canon_declarations.len(),
    };
    drop(canon_declarations);
    drop(canon_marks);
    drop(merged_declared_in_place);

    let package_imports_text = if module {
        package_imports(&merged_imports)
    } else {
        String::new()
    };

    // Stage 2: Parallel terse rewriting (or parallel hygiene / k-remapping).
    let (emitted_code, mapping) = if terse {
        let canon_k_names: Vec<String> = (0..canon_lazy.len()).map(|i| format!("k{i}")).collect();
        let k_prefixes: Vec<String> = (0..slice_outputs.len())
            .map(|idx| format!("k{idx}_"))
            .collect();
        let mut terse_slices = Vec::with_capacity(slice_outputs.len() + 1);
        terse_slices.push(crate::terse::TerseSlice {
            source: &header,
            raw: &[],
            marks: header_marks,
            k_prefix: None,
            remap: &[],
        });
        for (idx, s) in slice_outputs.iter_mut().enumerate() {
            terse_slices.push(crate::terse::TerseSlice {
                source: &s.output,
                raw: &s.raw_segments,
                marks: s.marks.take(),
                k_prefix: Some(&k_prefixes[idx]),
                remap: &remaps[idx],
            });
        }
        let parallel_rt = parallel_runtime_kept
            .as_deref()
            .zip(parallel_runtime_bindings.as_deref())
            .map(|(kept, bindings)| crate::terse::ParallelRuntime {
                kept,
                bindings,
                can_nest: module,
            });
        let terse_out = crate::terse::terse_program_slices(
            terse_slices,
            &canon_k_names,
            parallel_rt,
            &package_imports_text,
            module,
            mapped,
            count_lkeys,
            slots,
        );
        runtime_bytes = terse_out.runtime_bytes;
        unit_bytes = terse_out.unit_bytes;
        drop(slice_outputs);
        drop(header);
        if mapped {
            let mut joined = String::with_capacity(terse_out.total_len);
            for piece in terse_out.pieces {
                joined.push_str(&piece);
            }
            let mapping = terse_out.marks.map(|marks| marks.finish(&joined));
            (EmittedCode::Single(joined), mapping)
        } else {
            (
                EmittedCode::Pieces {
                    pieces: terse_out.pieces,
                    total_len: terse_out.total_len,
                    lkey_counts: Some(terse_out.lkey_counts),
                },
                None,
            )
        }
    } else {
        // Non-terse path: `:none`, `:prepare`, or `:advanced --pseudo-names`.
        let mut open_prefix = String::new();
        if module {
            open_prefix.push_str(&package_imports_text);
            open_prefix.push_str("export default ");
        }
        open_prefix.push_str(PRELUDE_OPEN);
        let close_suffix = if module { "})();\n" } else { "})()\n" };

        let (final_header, final_header_marks, final_slices) = if has_raw_js {
            let hy_prefix = crate::terse::raw_hygiene_prefix(
                slice_outputs
                    .iter()
                    .flat_map(|s| s.raw_segments.iter().map(|r| &s.output[r.clone()]))
                    .chain(header_runtime_segment.iter().map(|r| &header[r.clone()])),
            );
            enum HygieneJob {
                Header {
                    source: String,
                    skipped: Vec<std::ops::Range<usize>>,
                    marks: Option<Marks>,
                },
                Body {
                    idx: usize,
                    source: String,
                    raw: Vec<std::ops::Range<usize>>,
                    marks: Option<Marks>,
                },
            }
            let mut jobs = Vec::with_capacity(slice_outputs.len() + 1);
            jobs.push((
                header.len() as u64,
                HygieneJob::Header {
                    source: header,
                    skipped: header_runtime_segment.into_iter().collect(),
                    marks: header_marks,
                },
            ));
            for (idx, s) in slice_outputs.into_iter().enumerate() {
                jobs.push((
                    s.output.len() as u64,
                    HygieneJob::Body {
                        idx,
                        source: s.output,
                        raw: s.raw_segments,
                        marks: s.marks,
                    },
                ));
            }
            let mut results = crate::scheduler::map(slots, jobs, |job| match job {
                HygieneJob::Header {
                    source,
                    skipped,
                    mut marks,
                } => {
                    let (renamed, anchors) =
                        crate::terse::rename_compiler_names(&source, &skipped, |word| {
                            format!("{hy_prefix}{word}")
                        });
                    if let Some(marks) = marks.as_mut() {
                        marks.relocate(|offset| {
                            Some(crate::terse::renamed_offset(&anchors, offset))
                        });
                    }
                    (renamed, marks)
                }
                HygieneJob::Body {
                    idx,
                    source,
                    raw,
                    mut marks,
                } => {
                    let k_prefix = slice.then(|| format!("k{idx}_"));
                    let remap = &remaps[idx];
                    let (renamed, anchors) =
                        crate::terse::rename_compiler_names(&source, &raw, |word| {
                            if let Some(kp) = k_prefix.as_deref()
                                && let Some(rest) = word.strip_prefix(kp)
                                && let Ok(local) = rest.parse::<usize>()
                                && let Some(&canon) = remap.get(local)
                            {
                                format!("{hy_prefix}k{canon}")
                            } else {
                                format!("{hy_prefix}{word}")
                            }
                        });
                    if let Some(marks) = marks.as_mut() {
                        marks.relocate(|offset| {
                            Some(crate::terse::renamed_offset(&anchors, offset))
                        });
                    }
                    (renamed, marks)
                }
            });
            let (h_text, h_marks) = results.remove(0);
            (h_text, h_marks, results)
        } else if slice {
            // `:advanced --pseudo-names` without raw `js*`: canonicalize
            // `k{s}_{local}` -> `k{canon}` in parallel per slice.
            let rewrite_jobs: Vec<(u64, (usize, SliceOutput<'a>))> = slice_outputs
                .into_iter()
                .enumerate()
                .map(|(idx, s)| (s.output.len() as u64, (idx, s)))
                .collect();
            let rewritten = crate::scheduler::map(slots, rewrite_jobs, |(idx, mut s)| {
                let remap = &remaps[idx];
                if !remap.is_empty() {
                    let k_prefix = format!("k{idx}_");
                    let (text, anchors) =
                        rewrite_slice_k_names(&s.output, &s.raw_segments, &k_prefix, remap);
                    if let Some(marks) = s.marks.as_mut() {
                        marks.relocate(|offset| {
                            Some(crate::terse::renamed_offset(&anchors, offset))
                        });
                    }
                    (text, s.marks)
                } else {
                    (s.output, s.marks)
                }
            });
            (header, header_marks, rewritten)
        } else {
            let body = slice_outputs
                .into_iter()
                .map(|s| (s.output, s.marks))
                .collect();
            (header, header_marks, body)
        };

        if mapped {
            let total_len = open_prefix.len()
                + final_header.len()
                + final_slices.iter().map(|(s, _)| s.len()).sum::<usize>()
                + close_suffix.len();
            let mut joined = String::with_capacity(total_len);
            let mut all_marks = Marks::default();
            joined.push_str(&open_prefix);
            if let Some(hm) = final_header_marks {
                all_marks.absorb(hm, joined.len());
            }
            joined.push_str(&final_header);
            for (text, marks) in final_slices {
                if let Some(marks) = marks {
                    all_marks.absorb(marks, joined.len());
                }
                joined.push_str(&text);
            }
            joined.push_str(close_suffix);
            let mapping = all_marks.finish(&joined);
            (EmittedCode::Single(joined), Some(mapping))
        } else {
            let mut pieces = Vec::with_capacity(final_slices.len() + 3);
            let mut total_len = open_prefix.len();
            pieces.push(open_prefix);
            if !final_header.is_empty() {
                total_len += final_header.len();
                pieces.push(final_header);
            }
            for (text, _) in final_slices {
                if !text.is_empty() {
                    total_len += text.len();
                    pieces.push(text);
                }
            }
            total_len += close_suffix.len();
            pieces.push(close_suffix.to_owned());
            (
                EmittedCode::Pieces {
                    pieces,
                    total_len,
                    lkey_counts: None,
                },
                None,
            )
        }
    };

    let runtime = EmittedRuntime {
        roots: merged_runtime,
        embedding,
        bytes: runtime_bytes,
        unit_bytes,
        constants: constants_pool,
    };
    (emitted_code, runtime, mapping)
}

/// Record the same application-level runtime roots as
/// [`Emitter::record_application_roots`] directly into `runtime`.
fn record_application_roots_into(runtime: &mut RuntimeRoots, application: Application<'_>) {
    if application.direct.is_none() || runtime.is_empty() {
        return;
    }
    for &(unit, site) in application.protocol_declarations {
        runtime.record(RuntimeName::direct(unit, None), Some(site), NameUse::Keep);
    }
    for &(unit, site) in application.loadable_members {
        runtime.record(RuntimeName::direct(unit, None), Some(site), NameUse::Keep);
    }
    for &(unit, site) in application.member_units {
        runtime.record(RuntimeName::direct(unit, None), Some(site), NameUse::Keep);
    }
    if let Some(site) = application.type_static {
        runtime.record(
            RuntimeName::direct(crate::runtime_slice::TYPE_STATICS_UNIT, None),
            Some(site),
            NameUse::Keep,
        );
    }
    if let Some(site) = application
        .inference
        .and_then(|inference| inference.facts().core_method_write())
    {
        runtime.record(
            RuntimeName::direct("extendType", None),
            Some(site),
            NameUse::Keep,
        );
        runtime.record(
            RuntimeName::direct("slotWrites", None),
            Some(site),
            NameUse::Keep,
        );
    }
    if let Some(site) = application.multi_tables {
        runtime.record(
            RuntimeName::direct(crate::runtime_slice::MULTI_TABLES_UNIT, None),
            Some(site),
            NameUse::Keep,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply_emitted_key(
        prepared: &crate::PreparedApplication,
        emitted: EmittedCode,
        slots: Slots,
    ) -> String {
        let short_names = prepared.level().advanced() && !prepared.short_names().is_empty();
        match emitted {
            EmittedCode::Single(code) => {
                let key = crate::runtime_key::RuntimeKey::release(
                    [code.as_str()],
                    prepared.level(),
                    None,
                );
                let key = if short_names {
                    key.shortened([code.as_str()])
                } else {
                    key
                };
                key.apply(&code)
            }
            EmittedCode::Pieces {
                pieces,
                total_len,
                lkey_counts,
            } => {
                let joined: String = pieces.concat();
                assert_eq!(joined.len(), total_len);
                let key = crate::runtime_key::RuntimeKey::release_pieces(
                    pieces.iter().map(String::as_str),
                    total_len,
                    prepared.level(),
                    None,
                );
                let key = if short_names {
                    let counts =
                        lkey_counts.unwrap_or_else(|| crate::runtime_key::StemCounts::of(&joined));
                    key.shortened_from_counts(counts)
                } else {
                    key
                };
                let applied = key.apply_pieces(pieces, slots);
                let whole_key = crate::runtime_key::RuntimeKey::release(
                    [joined.as_str()],
                    prepared.level(),
                    None,
                );
                let whole_key = if short_names {
                    whole_key.shortened([joined.as_str()])
                } else {
                    whole_key
                };
                assert_eq!(applied, whole_key.apply(&joined));
                applied
            }
        }
    }

    fn eval_node_js(js: &str) -> String {
        let output = std::process::Command::new("node")
            .arg("-e")
            .arg(format!("process.stdout.write(JSON.stringify({js}));"))
            .output()
            .expect("node executable");
        assert!(
            output.status.success(),
            "node failed:\nstderr: {}\njs:\n{}",
            String::from_utf8_lossy(&output.stderr),
            &js[..js.len().min(2000)]
        );
        String::from_utf8(output.stdout).unwrap()
    }

    /// Force the parallel single-chunk release emitter onto small programs
    /// across all optimization levels, module/js modes, pseudo-names, raw js*,
    /// source maps, records, hoisted methods, lazy constants, and reflection.
    #[test]
    fn forced_parallel_single_chunk_matches_serial_runtime_and_source_maps() {
        // Large constant (> 1024 nodes) to exercise lazy constant cells
        // (`let c<n>; const k<n> = () => c<n> ??= ...`) plus eager/lazy shared
        // sub-constants across slices.
        let mut big_vec = String::from("[");
        for i in 0..1100 {
            if i > 0 {
                big_vec.push(' ');
            }
            write!(big_vec, ":kw{i}").unwrap();
        }
        big_vec.push(']');
        let lazy_source = format!(
            "(def eager-kw :kw42)
             (def eager-vec [:kw0 :kw1 :kw42])
             (def big-a {big_vec})
             (def big-b {big_vec})
             (def use-both (fn [] (array (count big-a) (nth big-b 42) (= eager-kw (nth big-a 42)) (count eager-vec))))
             (use-both)"
        );

        let cases: &[(&str, &str)] = &[
            (
                "records-and-hoisted",
                r#"(defrecord Point [x y])
                   (defn dist [p q] (+ (.-x p) (.-y q)))
                   (let [make (fn
                                ([x]
                                  (defrecord Inner [v])
                                  (let [r (->Inner x)]
                                    (when (nil? r) (throw "after-hoisted-record"))
                                    r))
                                ([x y] (+ x y)))]
                     (def p1 (->Point 3 4))
                     (def p2 (->Point 3 4))
                     (def i1 (make 9))
                     (def i2 (make 9))
                     (array (dist p1 p2) (= p1 p2) (= (hash p1) (hash p2))
                            (= i1 i2) (= (hash i1) (hash i2)) (make 5 6) "after-main-record"))"#,
            ),
            (
                "records-lookup-assoc",
                r#"(defrecord Box [a b-c])
                   (defrecord Empty [])
                   (let [make (fn
                                ([x]
                                  (defrecord Nested [unmentioned])
                                  (->Nested x))
                                ([x y] (+ x y)))]
                     (def b @(atom (->Box 1 2)))
                     (def e @(atom (->Empty)))
                     (def n @(atom (make 7)))
                     (def b2 (assoc b :a 10))
                     (def b3 (assoc b (keyword "b-c") 20))
                     (def b4 (assoc b :ext 30))
                     (def n2 (assoc n (keyword "unmentioned") 9))
                     (array (:a b) (get b (keyword "b-c")) (:a b2) (:b-c b3) (:ext b4)
                            (get (assoc e :z 4) :z) ((keyword "unmentioned") n)
                            (get n2 (keyword "unmentioned")) (make 2 3)))"#,
            ),
            (
                "raw-js-and-hygiene",
                r#"(def a 10)
                   (def b (js* "((x) => x + 5)(~{})" a))
                   (def c (let [v 7] (js* "~{} = ~{} + 3" v v) v))
                   (def d (+ a b c))
                   (array a b c d)"#,
            ),
            (
                "reflection-current-ns",
                r#"(def x 1)
                   (def y 2)
                   (def z (str *ns*))
                   (def w (+ x y))
                   (array x y z w)"#,
            ),
            ("lazy-and-eager-constants", &lazy_source),
            (
                "raw-comments-and-top-let",
                r#"(def k1 :alpha)
                   (def k2 :beta)
                   (def v1 (js* "1 + /* keep */ 2"))
                   (let [a 3 b 4] (def from-let (+ a b)))
                   (def v2 (+ from-let 10))
                   (array (name k1) (name k2) v1 v2 from-let)"#,
            ),
        ];

        let line_col = |text: &str, byte: usize| -> (u32, u32) {
            let prefix = &text[..byte];
            let line = prefix.bytes().filter(|&b| b == b'\n').count() as u32;
            let tail = prefix.rsplit('\n').next().unwrap_or(prefix);
            let col = tail.encode_utf16().count() as u32;
            (line, col)
        };

        for &(label, source) in cases {
            let program = crate::analyze(source, &crate::parse(source).unwrap()).unwrap();
            for level in crate::OptimizationLevel::ALL {
                for pseudo_names in [false, true] {
                    if !level.advanced() && pseudo_names {
                        continue;
                    }
                    let mut opts = crate::ApplicationOptions::new(level);
                    opts.pseudo_names = pseudo_names;
                    let prepared = crate::prepare_application(&program, opts);
                    let app = prepared.application_for_test();
                    let count_lkeys = level.advanced() && !pseudo_names;

                    // Serial ground truth for JS evaluation.
                    let serial_js = prepared.emit_js();
                    let expected_eval = eval_node_js(&serial_js);

                    for module in [false, true] {
                        let (emitted1, rt1, _) = emit_application_parallel(
                            prepared.program(),
                            app,
                            module,
                            false,
                            count_lkeys,
                            Slots::new(4).unwrap(),
                            true,
                        );
                        let (emitted2, rt2, _) = emit_application_parallel(
                            prepared.program(),
                            app,
                            module,
                            false,
                            count_lkeys,
                            Slots::new(4).unwrap(),
                            true,
                        );
                        let final1 = apply_emitted_key(&prepared, emitted1, Slots::new(4).unwrap());
                        let final2 = apply_emitted_key(&prepared, emitted2, Slots::new(4).unwrap());
                        // Deterministic across repeated runs at the same slot count.
                        assert_eq!(
                            final1, final2,
                            "{label} {level} pseudo={pseudo_names} mod={module}"
                        );
                        assert_eq!(rt1.bytes, rt2.bytes);
                        assert_eq!(rt1.constants.entries, rt2.constants.entries);
                        // No slice-local constant prefixes may leak into final JS.
                        assert!(!final1.contains("k0_"), "{label}: leaked k0_");
                        assert!(!final1.contains("k1_"), "{label}: leaked k1_");

                        if !module {
                            let actual_eval = eval_node_js(&final1);
                            assert_eq!(
                                actual_eval, expected_eval,
                                "runtime mismatch for {label} {level} pseudo={pseudo_names}"
                            );
                        }

                        // Mapped mode: valid source map and exact sentinel lookup.
                        let (mapped_emitted, _, mapping) = emit_application_parallel(
                            prepared.program(),
                            app,
                            module,
                            true,
                            false,
                            Slots::new(4).unwrap(),
                            true,
                        );
                        let EmittedCode::Single(mapped_raw) = mapped_emitted else {
                            unreachable!("mapped returns Single");
                        };
                        let mut mapping = mapping.expect("mapped returns SourceMapping");
                        let key = crate::runtime_key::RuntimeKey::release(
                            [mapped_raw.as_str()],
                            level,
                            None,
                        );
                        let key = if count_lkeys {
                            key.shortened([mapped_raw.as_str()])
                        } else {
                            key
                        };
                        let mapped_final = key.apply_mapped(&mapped_raw, &mut mapping);
                        assert_eq!(
                            mapped_final, final1,
                            "mapped text diverged from unmapped for {label} {level} pseudo={pseudo_names} mod={module}"
                        );
                        if label == "records-lookup-assoc" && level.advanced() && pseudo_names {
                            // Verify parallel emission splices per-type lookup (both arities) and
                            // assoc kits into each defineType call, including the hoisted Nested record.
                            for name in [
                                "\"cljs.user/Box a b-c\"",
                                "\"cljs.user/Empty\"",
                                "\"cljs.user/Nested unmentioned\"",
                            ] {
                                let start = final1
                                    .find(name)
                                    .unwrap_or_else(|| panic!("missing {name} in {final1}"));
                                let end = final1[start..]
                                    .find('\n')
                                    .map(|offset| start + offset)
                                    .unwrap_or(final1.len());
                                let def = &final1[start..end];
                                for expected in [
                                    "cljs$core$ILookup$_lookup$arity$2$",
                                    "cljs$core$ILookup$_lookup$arity$3$",
                                    "cljs$core$IAssociative$_assoc$arity$3$",
                                ] {
                                    assert!(
                                        def.contains(expected),
                                        "{name} missing {expected} ({level} mod={module}): {def}"
                                    );
                                }
                            }
                        }
                        if label == "records-and-hoisted" {
                            for needle in ["\"after-hoisted-record\"", "\"after-main-record\""] {
                                let expected_byte = source.find(needle).unwrap();
                                let generated_byte = mapped_final.find(needle).unwrap();
                                let (line, col) = line_col(&mapped_final, generated_byte);
                                assert_eq!(
                                    mapping.original(line, col),
                                    Some((0, expected_byte)),
                                    "source-map mismatch for {needle} ({level} pseudo={pseudo_names} mod={module})"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
