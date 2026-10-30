//! Native macro tasks on a shared runtime. Mutable registries belong to a
//! task; compiler helper factories cross explicit dependency boundaries.
use crate::{
    CollectionKind as C,
    compiler_data::{
        CompilerLiteralKind as L, Datum, DatumKind as D, ExpansionOrigin, Form, Name, SourceSpan,
    },
    compiler_evaluator::{
        CompilerEvaluator, NativeRunner, NativeSubmit, NativeSubmitter, StringService, UnitService,
        WaitService,
    },
    evaluator::{
        EvalError, EvaluatorOptions, ExecutionControl, MacroEvaluator, NEXT_SESSION, ValueId,
    },
    form_bridge::ConversionLimits,
};
use big_boy_pants_js::{
    Budget, Completion, DependencyId, DependencyKey, Executable, FormKind, JsString, Local,
    ObjectIdentity, Primitive, ProducerRequest, Provenance, ReclamationQueue, ResultKind, Rooted,
    RunOutcome, Runtime, Scope, Task, ValueView, WeakHandle,
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap, HashSet},
    hash::{BuildHasherDefault, Hasher},
    rc::Rc,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU32, AtomicUsize, Ordering},
    },
};

mod catalog;
mod namespaces;

fn error(e: impl std::fmt::Display) -> EvalError {
    EvalError(format!("big-boy-pants-js: {e}"))
}
fn string(scope: &Scope<'_>, units: &[u16]) -> Result<JsString, EvalError> {
    let local = scope
        .try_string_units(units)
        .ok_or_else(|| error("heap byte or string limit exceeded"))?;
    scope
        .string(local)
        .ok_or_else(|| error("expected imported string"))
}

fn literal_tag(kind: L) -> &'static str {
    match kind {
        L::BigInteger => "number",
        L::Regex => "regex",
        L::Instant => "instant",
        L::Uuid => "uuid",
        L::JsArray => "js-array",
        L::JsObject => "js-object",
        L::Queue => "queue",
    }
}

fn validate_js_object_payload(payload: &Datum) -> Result<(), EvalError> {
    let D::Collection {
        kind: C::Map,
        children,
    } = payload.kind()
    else {
        return Err(error("invalid JS object literal payload"));
    };
    if !children.len().is_multiple_of(2) {
        return Err(error("invalid JS object literal payload"));
    }
    for pair in children.as_chunks::<2>().0 {
        if !matches!(pair[0].kind(), D::String(_))
            && !matches!(pair[0].kind(), D::Keyword(name) if name.namespace().is_none())
        {
            return Err(error("Invalid JS object literal key"));
        }
    }
    Ok(())
}

pub type BridgeStats = crate::compile_stats::NativeBridgeCall;

/// The legacy bridge gives newly constructed scalars the latest equal scalar
/// argument's provenance. Keep native occurrence identities during evaluation;
/// apply this call-local compatibility rule only when exporting scalar leaves.
#[derive(Eq, PartialEq, Hash)]
enum ScalarKey {
    Nil,
    Boolean(bool),
    Number(u64),
    String(Arc<[u16]>),
    Keyword(Name),
}
impl ScalarKey {
    fn of(data: &Datum) -> Option<Self> {
        let number = |n: f64| {
            Self::Number(if n.is_nan() {
                f64::NAN.to_bits()
            } else {
                n.to_bits()
            })
        };
        Some(match data.kind() {
            D::Nil => Self::Nil,
            D::Boolean(b) => Self::Boolean(*b),
            D::Integer(n) => number(*n as f64),
            D::Double(n) => number(*n),
            D::Ratio {
                numerator,
                denominator,
            } => number(*numerator as f64 / *denominator as f64),
            D::Character(c) => Self::String(Arc::from([*c])),
            D::String(s) => Self::String(s.clone()),
            D::Keyword(n) => Self::Keyword(n.clone()),
            _ => return None,
        })
    }
}

struct ScalarOrigin {
    data: Datum,
    span: SourceSpan,
}

fn scalar_origins(call: &Form) -> HashMap<ScalarKey, ScalarOrigin> {
    let mut origins = HashMap::new();
    let mut pending: Vec<_> = call.children().into_iter().skip(1).rev().collect();
    while let Some(form) = pending.pop() {
        let data = form.data();
        let big_integer = matches!(
            data.kind(),
            D::CompilerLiteral {
                kind: L::BigInteger,
                ..
            }
        );
        if let Some(key) = ScalarKey::of(&data) {
            origins.insert(
                key,
                ScalarOrigin {
                    data,
                    span: form.compiler_span(),
                },
            );
        }
        // Legacy decoding visits children in order, then metadata.
        pending.extend(form.metadata());
        // BigInteger is encoded directly as a number. Its decimal payload
        // never becomes a decoded string argument in the legacy bridge.
        if !big_integer {
            pending.extend(form.node().compiler_payload.and_then(|id| form.at(id)));
        }
        pending.extend(form.children().into_iter().rev());
    }
    origins
}

struct Imported {
    _live: crate::compile_stats::Live,
    data: Datum,
    retained: usize,
    occurrence: WeakHandle,
    // A metadata map can escape without its occurrence. Reimport it with
    // the same native identity while that raw object remains observable.
    native: Option<WeakHandle>,
}

// These tables contain allocator/heap identities, never user-controlled
// strings or language equality keys. Avoid SipHash on every released edge.
#[derive(Default)]
struct BridgeIdentityHasher(u64);
impl Hasher for BridgeIdentityHasher {
    fn finish(&self) -> u64 {
        // Allocator addresses are aligned; mix their high bits into the low
        // bucket bits as well as the hash table's high-bit control tags.
        self.0 ^ (self.0 >> 32)
    }
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(u64::from(byte));
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
    fn write_u32(&mut self, n: u32) {
        self.write_u64(u64::from(n));
    }
}
type IdentityMap<K, V> = HashMap<K, V, BuildHasherDefault<BridgeIdentityHasher>>;

struct RetainedDatum {
    key: usize,
    children: Vec<usize>,
    // One reference per imported root and per edge from a retained Datum.
    references: usize,
    cost: Option<(usize, usize)>,
}
#[derive(Clone, Copy)]
struct ReaderExportPath<'a> {
    path: &'a str,
    // #js payload containers use the host container's diagnostic paths.
    literal: Option<L>,
}
#[derive(Default)]
struct Bridge {
    control: Option<ExecutionControl>,
    // Retaining the Datum prevents allocator address reuse from aliasing a key.
    imports: IdentityMap<usize, Imported>,
    forms: IdentityMap<usize, Form>,
    stats: BridgeStats,
    next_token: u32,
    collected: u64,
    environment: Option<(Datum, Rooted)>,
    cost_entries: usize,
    // Native metadata is a raw immutable map, so it can emerge without its
    // occurrence wrapper. Map that exact object back without traversing it.
    // Reclamation notifications remove dead identities; liveness handles
    // already belong to Imported, so the lookup need not duplicate them.
    native_objects: IdentityMap<ObjectIdentity, usize>,
    reclaimed: ReclamationQueue,
    // Each identity stays allocated through a surviving Imported Datum tree.
    // When a root dies, hold that Datum until its child links are released.
    retained: IdentityMap<usize, usize>,
    retained_nodes: Vec<Option<RetainedDatum>>,
    free_retained: Vec<usize>,
    // remember() can precede a failed import. Check only newly remembered
    // entries that have not yet become reachable, rather than all forms.
    unretained: HashSet<usize>,
    gensym_counter: u64,
    // Keys include the native service's 'a' request tag. Retain accounted
    // engine strings: generated spellings cannot grow uncharged host buffers.
    generated_names: HashSet<JsString>,
    // A parked initializer may inherit an earlier caller. Gensym uses that
    // caller's counter and capture log, even on the later caller's VM task.
    initializer_callers: HashMap<u64, BTreeMap<u64, u64>>,
    initializer_parents: HashMap<u64, HashSet<u64>>,
    caller_gensyms: HashMap<u64, crate::generated_names::NativeGensyms>,
    initializer_namespaces: HashMap<(u64, u64), (u64, bool)>,
    next_namespace: u64,
    /// The prune walk's and `&env` cache's work, kept only while a trace
    /// records (`--stats`).
    counters: crate::compile_stats::NativeBridgeCounters,
}
impl Bridge {
    /// Observable native objects keep their full compiler-side Datum tree.
    /// Check after a collection, under the caller's no-GC host scope.
    fn prune(&mut self, collected: u64) {
        if self.collected == collected {
            return;
        }
        let counted = crate::compile_trace::any_recording();
        let started = counted.then(std::time::Instant::now);
        let sizes = |b: &Self| b.forms.len() + b.cost_entries + b.native_objects.len();
        let before = sizes(self);
        self.collected = collected;
        let mut visited = 0;
        let mut dead = Vec::new();
        for (identity, key) in self.reclaimed.take() {
            self.native_objects.remove(&identity);
            let std::collections::hash_map::Entry::Occupied(entry) = self.imports.entry(key) else {
                continue;
            };
            let imported = entry.get();
            visited += 1;
            if imported.occurrence.is_live()
                || imported.native.as_ref().is_some_and(WeakHandle::is_live)
            {
                continue;
            }
            dead.push(entry.remove());
        }
        // Batch released roots in one worklist. Keep their Datum allocations
        // alive until all their links have been processed, including shared
        // descendants whose last import was removed earlier in this pass.
        let released = self.release_datums(dead.iter().map(|i| i.retained).collect());
        let removed = dead.len();
        drop(dead);
        // Hash-table iteration visits capacity. Reset the candidate table
        // each pass so a later failed import cannot scan an old large table.
        for key in std::mem::take(&mut self.unretained) {
            if !self.retained.contains_key(&key) {
                self.forms.remove(&key);
            }
        }
        if let Some(started) = started {
            let entries_removed = before - sizes(self);
            let c = &mut self.counters;
            c.prune_passes += 1;
            c.prune_imports_visited += visited;
            c.prune_imports_removed += removed;
            c.prune_references_released += released;
            c.prune_entries_removed += entries_removed;
            c.prune_nanos += started.elapsed().as_nanos();
        }
    }

    fn datum_children(data: &Datum) -> impl Iterator<Item = Datum> + '_ {
        let children = match data.kind() {
            D::Collection { children, .. } => children.as_ref(),
            D::CompilerLiteral { payload, .. } => std::slice::from_ref(payload),
            _ => &[],
        };
        children.iter().cloned().chain(data.metadata())
    }

    fn retain_reference(&mut self, key: usize) -> (usize, bool) {
        self.unretained.remove(&key);
        match self.retained.entry(key) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                let index = *entry.get();
                self.retained_nodes[index].as_mut().unwrap().references += 1;
                (index, false)
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                let index = self.free_retained.pop().unwrap_or_else(|| {
                    self.retained_nodes.push(None);
                    self.retained_nodes.len() - 1
                });
                self.retained_nodes[index] = Some(RetainedDatum {
                    key,
                    children: Vec::new(),
                    references: 1,
                    cost: None,
                });
                entry.insert(index);
                (index, true)
            }
        }
    }

    fn retain_datum(&mut self, data: &Datum) -> usize {
        let (root, fresh) = self.retain_reference(data.identity());
        let mut pending = Vec::new();
        if fresh {
            pending.push((data.clone(), root));
        }
        while let Some((data, index)) = pending.pop() {
            // Cache reader edges once, including entries omitted from raw
            // maps. Slots are reused only after their last reference dies;
            // neither a live import nor a live parent can refer to a free slot.
            let children = Self::datum_children(&data)
                .map(|child| {
                    let (index, fresh) = self.retain_reference(child.identity());
                    if fresh {
                        pending.push((child, index));
                    }
                    index
                })
                .collect();
            self.retained_nodes[index].as_mut().unwrap().children = children;
        }
        root
    }

    fn cost(&self, key: usize) -> Option<(usize, usize)> {
        let index = *self.retained.get(&key)?;
        self.retained_nodes[index].as_ref()?.cost
    }

    fn release_datums(&mut self, mut pending: Vec<usize>) -> usize {
        let mut walked = 0;
        while let Some(index) = pending.pop() {
            walked += 1;
            let retained = self.retained_nodes[index]
                .as_mut()
                .expect("retained import or parent");
            retained.references -= 1;
            if retained.references != 0 {
                continue;
            }
            // A surviving parent restores its entire original tree, including
            // descendants whose occurrence wrappers no longer exist. Release
            // child edges only when the last parent/import reference dies.
            let retained = self.retained_nodes[index].take().unwrap();
            self.retained.remove(&retained.key);
            self.free_retained.push(index);
            self.cost_entries -= usize::from(retained.cost.is_some());
            pending.extend(retained.children);
            self.forms.remove(&retained.key);
        }
        walked
    }
    fn cached<'s>(&mut self, s: &Scope<'s>, data: &Datum) -> Result<Option<Local<'s>>, EvalError> {
        let Some(imported) = self.imports.get(&data.identity()) else {
            return Ok(None);
        };
        if let Ok(value) = s.upgrade(&imported.occurrence) {
            return Ok(Some(value));
        }
        let native = imported.native.as_ref().and_then(|v| s.upgrade(v).ok());
        native.map(|value| self.retain(s, data, value)).transpose()
    }
    fn initializer_caller(&self, unit: u64) -> Option<u64> {
        self.initializer_context(unit).map(|(_, scope)| scope)
    }
    // The earliest direct or inherited caller, including the unit whose
    // namespace snapshot belongs to that caller. No executing-thread state.
    fn initializer_context(&self, unit: u64) -> Option<(u64, u64)> {
        let mut pending = vec![unit];
        let mut visited = HashSet::new();
        let mut earliest = None;
        while let Some(unit) = pending.pop() {
            if !visited.insert(unit) {
                continue;
            }
            if let Some((&position, &scope)) = self
                .initializer_callers
                .get(&unit)
                .and_then(|callers| callers.first_key_value())
                && earliest.is_none_or(|(prior, _)| position < prior)
            {
                earliest = Some((position, (unit, scope)));
            }
            if let Some(parents) = self.initializer_parents.get(&unit) {
                pending.extend(parents);
            }
        }
        earliest.map(|(_, context)| context)
    }
    fn initializer_namespace(&self, unit: u64) -> Option<u64> {
        // An explicit dynamic binding between two initializer demands stays
        // in force when the parent inherits an earlier caller. Carry the
        // nearest such snapshot along each ancestry path. Prefer a direct
        // caller at the same source position, then a deterministic unit ID.
        let mut pending = vec![(unit, None, 0usize)];
        let mut visited = HashMap::new();
        let mut earliest = None;
        while let Some((unit, bound, depth)) = pending.pop() {
            if visited
                .get(&(unit, bound))
                .is_some_and(|&prior| prior <= depth)
            {
                continue;
            }
            visited.insert((unit, bound), depth);
            let Some((&position, &scope)) = self
                .initializer_callers
                .get(&unit)
                .and_then(|callers| callers.first_key_value())
            else {
                continue;
            };
            let &(id, explicit) = self.initializer_namespaces.get(&(unit, scope))?;
            let key = (
                position,
                depth,
                bound.map_or(unit, |(_, owner)| owner),
                unit,
            );
            if earliest.is_none_or(|(prior, _)| key < prior) {
                earliest = Some((key, bound.map_or(id, |(id, _)| id)));
            }
            let bound = bound.or(explicit.then_some((id, unit)));
            if let Some(parents) = self.initializer_parents.get(&unit) {
                pending.extend(parents.iter().map(|parent| (*parent, bound, depth + 1)));
            }
        }
        earliest.map(|(_, id)| id)
    }
    fn poll(&self) -> Result<(), EvalError> {
        self.control.as_ref().map_or(Ok(()), ExecutionControl::poll)
    }
    fn charge(&self, data: &Datum, depth: usize, remaining: &mut usize) -> Result<(), EvalError> {
        let (nodes, height) = self.cost(data.identity()).expect("imported cost");
        if depth < height || *remaining < nodes {
            return Err(error("form conversion limit exceeded"));
        }
        *remaining -= nodes;
        Ok(())
    }
    fn import<'s>(
        &mut self,
        s: &Scope<'s>,
        d: &Datum,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Local<'s>, EvalError> {
        self.poll()?;
        if let Some(value) = self.cached(s, d)? {
            self.charge(d, depth, remaining)?;
            return Ok(value);
        }
        if depth == 0 || *remaining == 0 {
            return Err(error("form conversion limit exceeded"));
        }
        *remaining -= 1;
        let mut value = match d.kind() {
            D::Nil => s.primitive(Primitive::Null),
            D::Boolean(v) => s.primitive(Primitive::Bool(*v)),
            D::Integer(v) => s.primitive(Primitive::Number(*v as f64)),
            D::Double(v) => s.primitive(Primitive::Number(*v)),
            D::Ratio {
                numerator,
                denominator,
            } => s.primitive(Primitive::Number(*numerator as f64 / *denominator as f64)),
            D::Character(v) => s.primitive(Primitive::String(string(s, &[*v])?)),
            D::String(v) => s.primitive(Primitive::String(string(s, v)?)),
            D::Symbol(n) | D::Keyword(n) => {
                let ns = n.namespace().map(|units| string(s, units)).transpose()?;
                let name = string(s, n.name())?;
                if matches!(d.kind(), D::Symbol(_)) {
                    if n.namespace().is_none() {
                        let mut key = vec![b'a' as u16];
                        key.extend_from_slice(n.name());
                        self.generated_names.insert(string(s, &key)?);
                    }
                    s.new_symbol(ns, name)
                } else {
                    s.new_keyword(ns, name)
                }
                .map_err(error)?
            }
            D::Collection { kind, children } => {
                let values = children
                    .iter()
                    .map(|v| self.import(s, v, depth - 1, remaining))
                    .collect::<Result<Vec<_>, _>>()?;
                match kind {
                    C::List | C::AnonymousFn => s.new_list(&values),
                    C::Vector => s.new_form_vector(&values),
                    C::Map => s.new_literal_map(
                        &values
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|p| (p[0], p[1]))
                            .collect::<Vec<_>>(),
                    ),
                    C::Set => s.new_literal_set(&values),
                }
                .map_err(error)?
            }
            D::CompilerLiteral { kind, payload } => {
                // Memoized exports return their original Datum directly. Only
                // validated object literals may enter that identity cache.
                if *kind == L::JsObject {
                    validate_js_object_payload(payload)?;
                }
                let value = self.import(s, payload, depth - 1, remaining)?;
                if *kind == L::BigInteger {
                    let D::String(text) = payload.kind() else {
                        return Err(error("invalid Number payload"));
                    };
                    let number = String::from_utf16(text)
                        .ok()
                        .and_then(|s| s.parse::<f64>().ok())
                        .ok_or_else(|| error("invalid Number payload"))?;
                    s.primitive(Primitive::Number(number))
                } else {
                    s.new_compiler_literal(
                        string(s, &literal_tag(*kind).encode_utf16().collect::<Vec<_>>())?,
                        value,
                    )
                    .map_err(error)?
                }
            }
        };
        if let Some(meta) = d.metadata() {
            let m = self.import(s, &meta, depth - 1, remaining)?;
            value = s
                .form_with_meta(value, Some(s.form_value(m)))
                .map_err(error)?;
        }
        // Every input node is an occurrence, including numbers and strings.
        // Returning it preserves exact integer/ratio/character representation.
        self.retain(s, d, value)
    }
    fn retain<'s>(
        &mut self,
        s: &Scope<'s>,
        d: &Datum,
        value: Local<'s>,
    ) -> Result<Local<'s>, EvalError> {
        let raw = value;
        let native = s
            .object_identity(raw)
            .map(|identity| (identity, s.downgrade(raw)));
        let token = self.next_token;
        self.next_token = token
            .checked_add(1)
            .ok_or_else(|| error("too many imported occurrences"))?;
        let location = self
            .forms
            .get(&d.identity())
            .map(|f| crate::compiler_data::DiagnosticLocation::from_span(&f.span()));
        let value = s
            .new_occurrence(
                value,
                Provenance {
                    source: token,
                    line: location.as_ref().map_or(0, |l| l.line as u32),
                    column: location.as_ref().map_or(0, |l| l.column as u32),
                },
            )
            .map_err(error)?;
        let occurrence = s.downgrade(value);
        // Publish the raw-object entry only after occurrence allocation
        // succeeds, so failed imports cannot leave an unwatched entry.
        let native = native.map(|(identity, weak)| {
            self.native_objects.insert(identity, d.identity());
            weak
        });
        let identity = s
            .object_identity(value)
            .expect("an occurrence is an object");
        self.native_objects.insert(identity, d.identity());
        let retained = match self.imports.get(&d.identity()) {
            Some(imported) => imported.retained,
            None => self.retain_datum(d),
        };
        s.watch_reclamation(value, &self.reclaimed, d.identity());
        if native.is_some() {
            s.watch_reclamation(raw, &self.reclaimed, d.identity());
        }
        self.imports.insert(
            d.identity(),
            Imported {
                _live: crate::compile_stats::Live::new(&crate::compile_stats::LIVE_BRIDGE_PAYLOADS),
                data: d.clone(),
                retained,
                occurrence,
                native,
            },
        );
        let mut cost = (1usize, 1usize);
        let children = match d.kind() {
            D::Collection { children, .. } => children.as_ref(),
            D::CompilerLiteral { payload, .. } => std::slice::from_ref(payload),
            _ => &[],
        };
        for child in children.iter().cloned().chain(d.metadata()) {
            // An unchanged environment slot can have a freshly constructed
            // scalar keyword key whose native key was already present.
            let (nodes, height) = self.cost(child.identity()).unwrap_or((1, 1));
            cost.0 = cost.0.saturating_add(nodes);
            cost.1 = cost.1.max(height.saturating_add(1));
        }
        let state = self.retained_nodes[retained]
            .as_mut()
            .expect("retained import");
        self.cost_entries += usize::from(state.cost.replace(cost).is_none());
        self.stats.imported += 1;
        Ok(value)
    }
    fn environment<'s>(
        &mut self,
        s: &Scope<'s>,
        data: &Datum,
        limits: ConversionLimits,
    ) -> Result<Local<'s>, EvalError> {
        // Bootstrap functions with an unused &env receive nil. Keep the last
        // real map envelope for incremental imports by subsequent source calls.
        if matches!(data.kind(), D::Nil) {
            return self.import(s, data, limits.depth, &mut limits.nodes.clone());
        }
        let counted = crate::compile_trace::any_recording();
        if let Some(value) = self.cached(s, data)? {
            self.charge(data, limits.depth, &mut limits.nodes.clone())?;
            if counted {
                self.counters.env_hits += 1;
            }
            return Ok(value);
        }
        let mut remaining = limits.nodes;
        let Some((previous, root)) = self.environment.clone() else {
            let value = self.import(s, data, limits.depth, &mut remaining)?;
            self.environment = Some((data.clone(), s.persist(value)));
            if counted {
                self.counters.env_full += 1;
            }
            return Ok(value);
        };
        if counted {
            self.counters.env_misses += 1;
            self.counters.env_evictions += usize::from(previous.identity() != data.identity());
        }
        let before = previous.entries().map_err(error)?;
        let after = data.entries().map_err(error)?;
        let mut value = s.form_value(s.local(&root).map_err(error)?);
        let mut rebuilt = 0;
        // The fixed environment envelope has a handful of keyword slots.
        // Namespace and reader tables are memoized Datum subtrees: their
        // contents are neither walked nor shipped on an unchanged slot.
        for (key, _) in &before {
            if !after.iter().any(|(k, _)| k.value_eq(key)) {
                let k = self.import(s, key, limits.depth, &mut remaining)?;
                value = s.form_dissoc(value, k).map_err(error)?;
                rebuilt += 1;
            }
        }
        for (key, new) in &after {
            if before
                .iter()
                .any(|(k, v)| k.value_eq(key) && v.identity() == new.identity())
            {
                continue;
            }
            let k = self.import(s, key, limits.depth, &mut remaining)?;
            let v = self.import(s, new, limits.depth, &mut remaining)?;
            value = s.form_assoc(value, k, v).map_err(error)?;
            rebuilt += 1;
        }
        if counted {
            self.counters.env_entries_rebuilt += rebuilt;
        }
        let value = self.retain(s, data, value)?;
        self.charge(data, limits.depth, &mut limits.nodes.clone())?;
        self.environment = Some((data.clone(), s.persist(value)));
        Ok(value)
    }
    #[cfg(test)]
    fn export<'s>(
        &mut self,
        s: &Scope<'s>,
        v: Local<'s>,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Datum, EvalError> {
        self.export_with_origins(s, v, depth, remaining, &HashMap::new(), None)
    }
    fn export_with_origins<'s>(
        &mut self,
        s: &Scope<'s>,
        v: Local<'s>,
        depth: usize,
        remaining: &mut usize,
        origins: &HashMap<ScalarKey, ScalarOrigin>,
        reader: Option<ReaderExportPath<'_>>,
    ) -> Result<Datum, EvalError> {
        self.poll()?;
        if let Some(data) = s
            .object_identity(v)
            .and_then(|key| self.native_objects.get(&key))
            .and_then(|key| self.imports.get(key))
            .map(|imported| &imported.data)
        {
            self.charge(data, depth, remaining)?;
            self.stats.reused += 1;
            return Ok(data.clone());
        }
        if depth == 0 || *remaining == 0 {
            return Err(error("form conversion limit exceeded"));
        }
        *remaining -= 1;
        let v = s.form_value(v);
        let kind = match s.form_kind(v) {
            Some(FormKind::Keyword | FormKind::Symbol) => {
                let name = s.form_name(v).map_err(error)?;
                let n = Name::from_utf16(
                    name.namespace.as_ref().map(JsString::code_units),
                    name.name.code_units(),
                );
                if s.form_kind(v) == Some(FormKind::Keyword) {
                    D::Keyword(n)
                } else {
                    D::Symbol(n)
                }
            }
            Some(FormKind::CompilerLiteral) => {
                let (tag, payload) = s.form_compiler_literal(v).map_err(error)?;
                let kind = [
                    L::Regex,
                    L::Instant,
                    L::Uuid,
                    L::JsArray,
                    L::JsObject,
                    L::Queue,
                ]
                .into_iter()
                .find(|kind| tag == literal_tag(*kind))
                .ok_or_else(|| error("invalid compiler literal tag"))?;
                let path = reader.map(|reader| {
                    if matches!(kind, L::JsArray | L::JsObject) {
                        reader.path.to_owned()
                    } else {
                        format!("{}.payload", reader.path)
                    }
                });
                let payload = self.export_with_origins(
                    s,
                    payload,
                    depth - 1,
                    remaining,
                    origins,
                    path.as_deref().map(|path| ReaderExportPath {
                        path,
                        literal: Some(kind),
                    }),
                )?;
                if kind == L::JsObject {
                    validate_js_object_payload(&payload)?;
                }
                D::CompilerLiteral { kind, payload }
            }
            Some(k) => {
                let values = if k == FormKind::Map {
                    s.form_print_items(v)
                        .map_err(error)?
                        .into_iter()
                        .map(|pair| s.form_items(pair).map_err(error))
                        .collect::<Result<Vec<_>, _>>()?
                        .into_iter()
                        .flatten()
                        .collect()
                } else if k == FormKind::Set {
                    s.form_print_items(v).map_err(error)?
                } else {
                    s.form_items(v).map_err(error)?
                };
                let children = values
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(i, x)| {
                        let path = reader
                            .map(|reader| -> Result<_, EvalError> {
                                Ok(if reader.literal == Some(L::JsObject) {
                                    if i % 2 == 0 {
                                        format!("{}.key", reader.path)
                                    } else {
                                        let value = s.form_value(values[i - 1]);
                                        let key = if let Some(key) = s.string(value) {
                                            key
                                        } else {
                                            let name = s.form_name(value).map_err(error)?;
                                            if s.form_kind(value) != Some(FormKind::Keyword)
                                                || name.namespace.is_some()
                                            {
                                                return Err(error("Invalid JS object literal key"));
                                            }
                                            name.name
                                        };
                                        format!("{}.{}", reader.path, key.to_string_lossy())
                                    }
                                } else if k == FormKind::Map {
                                    format!(
                                        "{}.{}[{}]",
                                        reader.path,
                                        if i % 2 == 0 { "key" } else { "value" },
                                        i / 2
                                    )
                                } else {
                                    format!("{}[{i}]", reader.path)
                                })
                            })
                            .transpose()?;
                        self.export_with_origins(
                            s,
                            x,
                            depth - 1,
                            remaining,
                            origins,
                            path.as_deref().map(|path| ReaderExportPath {
                                path,
                                literal: None,
                            }),
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                D::Collection {
                    kind: match k {
                        FormKind::List => C::List,
                        FormKind::Vector => C::Vector,
                        FormKind::Set => C::Set,
                        _ => C::Map,
                    },
                    children: children.into(),
                }
            }
            None => match s.view(v) {
                ValueView::Undefined if let Some(reader) = reader => {
                    return Err(EvalError(format!(
                        "Error: Unsupported macro result undefined at {}",
                        reader.path
                    )));
                }
                ValueView::Null | ValueView::Undefined => D::Nil,
                ValueView::Bool(b) => D::Boolean(b),
                ValueView::Number(n) => D::Double(n),
                ValueView::String => D::String(s.string(v).unwrap().code_units().into()),
                ValueView::Object if let Some(time) = s.date_time_value(v) => {
                    if !time.is_finite() {
                        return Err(EvalError(format!(
                            "Error: Unsupported macro result invalid instant at {}",
                            reader.map_or("$", |reader| reader.path)
                        )));
                    }
                    let path = reader.map(|reader| format!("{}.payload", reader.path));
                    let payload = s.primitive(Primitive::Number(time));
                    D::CompilerLiteral {
                        kind: L::Instant,
                        payload: self.export_with_origins(
                            s,
                            payload,
                            depth - 1,
                            remaining,
                            origins,
                            path.as_deref().map(|path| ReaderExportPath {
                                path,
                                literal: None,
                            }),
                        )?,
                    }
                }
                // A global or sticky regex stays a host RegExp
                // (light-cljs-563); it exports as the legacy regex literal.
                ValueView::Object if let Some(text) = s.regexp_export_text(v).map_err(error)? => {
                    // The payload is one more node, one level down, as for a
                    // `regex` form.
                    let payload = s.primitive(Primitive::String(text));
                    D::CompilerLiteral {
                        kind: L::Regex,
                        payload: self.export_with_origins(
                            s,
                            payload,
                            depth - 1,
                            remaining,
                            origins,
                            reader.map(|reader| ReaderExportPath {
                                path: reader.path,
                                literal: None,
                            }),
                        )?,
                    }
                }
                _ => {
                    return Err(error(
                        "unsupported operation: macro returned a non-form value",
                    ));
                }
            },
        };
        let mut data = Datum::new(kind);
        if let Some(m) = s.form_meta(v) {
            let path = reader.map(|reader| format!("{}.meta", reader.path));
            data = data.with_metadata(self.export_with_origins(
                s,
                m,
                depth - 1,
                remaining,
                origins,
                path.as_deref().map(|path| ReaderExportPath {
                    path,
                    literal: None,
                }),
            )?);
        } else if !matches!(data.kind(), D::Keyword(_))
            && let Some(original) = ScalarKey::of(&data).and_then(|key| origins.get(&key))
        {
            // Runtime keyword construction is fresh; only imported, interned
            // keyword occurrences above share their call's latest provenance.
            self.stats.reused += 1;
            return Ok(original.data.clone());
        }
        self.stats.exported += 1;
        Ok(data)
    }
    fn remember(
        &mut self,
        form: &Form,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<(), EvalError> {
        self.poll()?;
        // A generated symbol (notably an anonymous fn parameter) may share
        // one Datum at several source positions. Legacy retain chooses the
        // last occurrence, including inside an already remembered subtree.
        if depth == 0 || *remaining == 0 {
            return Err(error("form conversion limit exceeded"));
        }
        *remaining -= 1;
        for f in form.children() {
            self.remember(&f, depth - 1, remaining)?;
        }
        if let Some(f) = form.node().compiler_payload.and_then(|id| form.at(id)) {
            self.remember(&f, depth - 1, remaining)?;
        }
        if let Some(f) = form.metadata() {
            self.remember(&f, depth - 1, remaining)?;
        }
        let key = form.data().identity();
        self.forms.insert(key, form.clone());
        if !self.retained.contains_key(&key) {
            self.unretained.insert(key);
        }
        Ok(())
    }

    fn compiler_spans(&self, result: Form, origins: &HashMap<ScalarKey, ScalarOrigin>) -> Form {
        let mut spans = HashMap::new();
        let mut pending = vec![(result.clone(), None::<Form>)];
        while let Some((form, inherited)) = pending.pop() {
            if let Some(original) = inherited {
                if let Some(span) = original.compiler_span_override() {
                    spans.insert(form.id(), span);
                }
                pending.extend(
                    form.children()
                        .into_iter()
                        .zip(original.children())
                        .map(|(f, o)| (f, Some(o))),
                );
                pending.extend(
                    form.metadata()
                        .zip(original.metadata())
                        .map(|(f, o)| (f, Some(o))),
                );
                let payload = |f: &Form| f.node().compiler_payload.and_then(|id| f.at(id));
                pending.extend(
                    payload(&form)
                        .zip(payload(&original))
                        .map(|(f, o)| (f, Some(o))),
                );
                continue;
            }
            if let Some(span) = form.compiler_span_override() {
                spans.insert(form.id(), span);
            }
            let data = form.data();
            if let Some(key) = ScalarKey::of(&data) {
                if matches!(data.kind(), D::Keyword(_))
                    && !self.imports.contains_key(&data.identity())
                {
                    continue;
                }
                if let Some(original) = origins.get(&key) {
                    let span = original.span.clone();
                    if span.bytes() != form.span().bytes() {
                        spans.insert(form.id(), span);
                    }
                }
                continue;
            }
            // An unchanged imported collection restores its complete subtree,
            // including annotations from an earlier expansion. Do not apply
            // the current call's aliases to its leaves.
            if self.imports.contains_key(&data.identity()) {
                if let Some(original) = self
                    .forms
                    .get(&data.identity())
                    .filter(|f| f.has_compiler_spans() || result.has_compiler_spans())
                {
                    spans.remove(&form.id());
                    pending.push((form, Some(original.clone())));
                }
                continue;
            }
            pending.extend(form.children().into_iter().map(|f| (f, None)));
            pending.extend(form.metadata().map(|f| (f, None)));
            pending.extend(
                form.node()
                    .compiler_payload
                    .and_then(|id| form.at(id))
                    .map(|f| (f, None)),
            );
        }
        result.with_compiler_spans(spans)
    }
}

/// The compilation's heap, immutable boot code and aggregate execution limit.
/// Spawning another evaluator does not create another heap or reset its budget.
#[derive(Clone)]
pub struct BbpRuntime(Arc<SharedRuntime>);
struct SharedRuntime {
    _live: crate::compile_stats::Live,
    /// The aggregate budget this runtime was built for ([`heap_policy`]).
    memory_limit: usize,
    counters: Arc<NativeCounters>,
    runtime: Runtime,
    executable: Executable,
    // One canonical immutable builtin tag table for this runtime. The
    // reader recognizes builtin handlers by identity across task migration.
    reader_tags: Rooted,
    /// Canonical callable members, indexed by builtin family or native type
    /// identity. Tasks share their identities and resolve dispatch locally.
    raw_members: Rooted,
    bridge: Arc<Mutex<Bridge>>,
    control: ExecutionControl,
    next_command: AtomicU32,
    definitions: Mutex<HashMap<DependencyKey, u64>>,
    generation: Mutex<u64>,
    changed: Condvar,
    graph: Mutex<big_boy_pants_js::DependencyGraph>,
    /// Reserved initializer claims, before any factory code runs. A later
    /// task can reserve a root first, but the earliest caller must execute
    /// it with its own bindings (including task-owned mutable values).
    pending_initializers: Mutex<HashMap<DependencyKey, PendingInitializer>>,
    /// Compiled unit factories by source ([`SHARED_UNIT_BYTES`]): each
    /// distinct source is compiled once for the whole runtime and every
    /// task calls the one published function. `None` records a source
    /// whose factory cannot be shared, which tasks evaluate themselves.
    factories: Mutex<HashMap<String, Arc<std::sync::OnceLock<Option<Rooted>>>>>,
    catalog: Mutex<catalog::Registry>,
    /// Task contexts that pool slots parked between jobs, for any slot to
    /// borrow ([`BbpEvaluator::park`], [`BbpEvaluator::adopt`]).
    idle: Mutex<Vec<TaskContext>>,
    /// Tasks parked on a dependency whose next step a completion queues on
    /// their pool ([`BbpRuntime::wake`]), by task.
    parked: Mutex<HashMap<big_boy_pants_js::TaskId, Parked>>,
    /// The compilation's effect lanes, which order shared atom accesses
    /// (light-cljs-480); none outside a compilation's catalog.
    lanes: std::sync::OnceLock<Arc<crate::effect_lanes::Lanes>>,
    /// The namespaces its tasks created and the compiler registered, which
    /// `all-ns` and `find-ns` read (light-cljs-589).
    namespaces: Arc<Mutex<namespaces::NamespaceLog>>,
    #[cfg(test)]
    on_wait: Mutex<Option<WaitHook>>,
    #[cfg(test)]
    on_initializer: Mutex<Option<WaitHook>>,
}
/// A task parked on a dependency: its next step, where that step's outcome
/// goes, and the pool that runs it once a completion wakes the task.
struct Parked {
    step: NativeStep,
    reply: std::sync::mpsc::SyncSender<Result<RunOutcome, String>>,
    submit: NativeSubmit,
}
struct PendingInitializer {
    request: ProducerRequest,
    callers: Vec<(big_boy_pants_js::TaskId, crate::effect_lanes::Caller)>,
}
impl PendingInitializer {
    fn owner(
        &self,
        lanes: Option<&Arc<crate::effect_lanes::Lanes>>,
        key: DependencyKey,
    ) -> big_boy_pants_js::TaskId {
        let index = lanes.map_or(0, |lanes| {
            lanes.earliest_caller(
                self.callers.iter().map(|(_, caller)| caller),
                crate::effect_lanes::Target::Cell(crate::effect_lanes::identity(key)),
            )
        });
        self.callers[index].0
    }
}
/// Removes a parked task's entry when its owner stops waiting, so that a
/// later wakeup queues nothing for it.
struct Unpark<'a>(&'a SharedRuntime, big_boy_pants_js::TaskId);
impl Drop for Unpark<'_> {
    fn drop(&mut self) {
        self.0.parked.lock().unwrap().remove(&self.1);
    }
}
#[derive(Default)]
struct NativeCounters {
    compiled: AtomicUsize,
    published: AtomicUsize,
    per_task: AtomicUsize,
    waits: AtomicUsize,
    steps: AtomicUsize,
    migrations: AtomicUsize,
    contexts: AtomicUsize,
    units_compiled: AtomicUsize,
    units_shared: AtomicUsize,
    units_evaluated: AtomicUsize,
    lent: AtomicUsize,
    borrowed: AtomicUsize,
    woken: AtomicUsize,
    log_reads: AtomicUsize,
    log_edges_copied: AtomicUsize,
    log_edges_recorded: AtomicUsize,
    catalog_uses: AtomicUsize,
    catalog_entries: AtomicUsize,
    catalog_nanos: AtomicUsize,
    producers: AtomicUsize,
    producer_replayed: AtomicUsize,
    producer_catalog_entries: AtomicUsize,
    producer_nanos: AtomicUsize,
    #[cfg(test)]
    cleanups: AtomicUsize,
}
#[cfg(test)]
type WaitHook = Arc<dyn Fn(&Task, DependencyKey) + Send + Sync>;

/// Unit sources at least this long are compiled once per runtime and
/// shared by its tasks ([`BbpRuntime::factory`]); a shorter one, such as a
/// root registration, is cheaper to evaluate in the task than to call.
const SHARED_UNIT_BYTES: usize = 512;

/// How many registrations a producer task replays per evaluation.
const REPLAY_BLOCK: usize = 64;

/// One unit that evaluates `registrations` in order, each as evaluating it
/// alone would: called with the task's native runtime.
fn replay_source(registrations: &[String]) -> String {
    let mut source = String::from("rt => { ");
    for registration in registrations {
        source.push('(');
        source.push_str(registration);
        source.push_str(")(rt); ");
    }
    source.push_str("return null; }");
    source
}

const BINDING: ResultKind = ResultKind::new(2);
const HELPER: ResultKind = ResultKind::new(3);
const INITIALIZER: ResultKind = ResultKind::new(4);
/// A lane cell: a task parks on one before it touches a shared atom, and
/// the compilation's effect lanes complete it once its lane opens
/// ([`crate::effect_lanes`], light-cljs-480).
const LANE: ResultKind = ResultKind::new(5);
/// The host effect that asks the effect lanes about an atom access: with a
/// negative argument, the token of the task's current activity (which owns
/// the atoms it makes); else, given the atom's owner token, 0 to go on or a
/// lane cell to park on (`bbp_runtime.js`).
const LANE_EFFECT: u16 = 65532;
/// Compiler-owned generated names, independent of the worker running a step.
const GENSYM_EFFECT: u16 = 65531;
const GENSYM_PREFIX: ResultKind = ResultKind::new(6);
/// Immutable namespace snapshots: a name symbol or an explicitly bound value.
const NAMESPACE: ResultKind = ResultKind::new(7);
const NAMESPACE_EFFECT: u16 = 65530;
/// The compilation's namespace registry ([`namespaces`]): `r` and a name
/// records its creation by the task's activity, `f` and a name asks whether
/// the task sees it, `v` gives the names it sees, each as its length, `:`
/// and its text. A reader asks its lane first.
const NAMESPACES_EFFECT: u16 = 65529;

/// The runtime-wide collection and admission policy for `options`.
///
/// `options.memory_limit` is the compilation's one aggregate budget, shared
/// by every task and worker of the runtime; this is the only place it may
/// reach the engine. It becomes the engine's accounted-byte admission limit
/// (`HeapPolicy::hard_limit_bytes`): the booted realm, the runtime's boot
/// code, every task's objects, strings, forms and saved continuations count
/// against it, and allocation pressure triggers collections before an
/// allocation is denied. It is a managed-heap cap, not a process RSS cap
/// (docs/native-macros.md, "Memory limit").
fn heap_policy(options: &EvaluatorOptions) -> big_boy_pants_js::HeapPolicy {
    big_boy_pants_js::HeapPolicy {
        hard_limit_bytes: Some(options.memory_limit),
        ..Default::default()
    }
}

/// Adapt the canonical reader at native task assembly, leaving the emitted
/// application runtime byte-for-byte unchanged. The parser/control flow stays
/// shared; only scalar occurrence boundaries and native callable/tag adapters
/// differ. Require every replacement to match so a parser edit cannot silently
/// drop an adapter (the source-macro differential table covers their behavior).
fn native_reader_source(manifest: &crate::runtime_manifest::RuntimeManifest<'_>) -> String {
    let unit = manifest.unit(manifest.unit_named("reader").unwrap());
    let source = &manifest.source()[unit.body_range.clone()];
    let (body, _) = source
        .split_once("})({keyword: freshKeyword,")
        .expect("canonical reader adapter");
    let mut source = format!("{body}}};");
    for (from, to) in [
        ("const reader = (rt => {", "const createReader = rt => {"),
        (
            "const inspect = rt.inspect;",
            "const inspect = rt.inspect, valueOf = rt.unwrap;",
        ),
        (
            "function stringReader(text, name = '<string-reader>') {",
            "function stringReader(text, name = '<string-reader>') { text = valueOf(text); name = valueOf(name);",
        ),
        (
            "function inputProvider(name, provider) {",
            "function inputProvider(name, provider) { name = valueOf(name);",
        ),
        ("s.provider()", "valueOf(s.provider())"),
        (
            "function jsLiteral(s, value) {",
            "function jsLiteral(s, value) { value = valueOf(value);",
        ),
        (
            "if (typeof key !== 'string'",
            "if (typeof valueOf(key) !== 'string'",
        ),
        (
            "pairs.push(typeof key === 'string' ? key : name.name, value);",
            "pairs.push(typeof valueOf(key) === 'string' ? key : name.name, value);",
        ),
        (
            "Object.prototype.hasOwnProperty.call(escapes, escaped)",
            "rt.hasOwn(escapes, escaped)",
        ),
        (
            "function timestampFields(value) {",
            "function timestampFields(value) { value = valueOf(value);",
        ),
        (
            "value != null && typeof value.call !== 'function'",
            "valueOf(value) != null && !rt.callable(value)",
        ),
        (
            "result.fallback = value;",
            "result.fallback = valueOf(value) == null ? valueOf(value) : value;",
        ),
        (
            "if (value == null) continue;",
            "if (valueOf(value) == null) continue;",
        ),
        (
            "handler != null && typeof handler.call !== 'function'",
            "valueOf(handler) != null && !rt.callable(handler)",
        ),
        (
            "function visit(value, d) {",
            "function visit(value, d) { value = valueOf(value);",
        ),
        (
            "typeof metadata === 'string'",
            "typeof valueOf(metadata) === 'string'",
        ),
        ("if (handler != null) {", "if (valueOf(handler) != null) {"),
        (
            "if (handler === builtinTags.get(tag)) {",
            "if (handler === builtinTags.get(tag)) { if (tag !== 'js') return rt.readBuiltin(tag, value);",
        ),
        (
            "handler.call(null, value)",
            "rt.callReader(handler, [value])",
        ),
        (
            "opts.fallback.call(null, symbol, value)",
            "rt.callReader(opts.fallback, [symbol, value])",
        ),
        (
            "args[1] !== false && args[1] != null",
            "valueOf(args[1]) !== false && valueOf(args[1]) != null",
        ),
        (
            "const text = args[args.length - 1];",
            "const text = valueOf(args[args.length - 1]);",
        ),
        (
            "defaultTag = handler;",
            "defaultTag = valueOf(handler) == null ? valueOf(handler) : handler;",
        ),
        (
            "for (const [tag, handler] of builtinTags) tags.set(tag,handler);",
            "for (const [tag, handler] of rt.builtinTags) builtinTags.set(tag,handler); for (const [tag, handler] of builtinTags) tags.set(tag,handler);",
        ),
    ] {
        assert!(
            source.contains(from),
            "native reader adapter missing: {from}"
        );
        source = source.replace(from, to);
    }
    source
}

impl BbpRuntime {
    /// Build a compilation's native runtime. Every runtime-wide limit comes
    /// from `options` here, once: evaluators spawned from the result share
    /// its heap, policy and execution budget and never configure their own.
    pub fn new(options: &EvaluatorOptions) -> Result<Self, EvalError> {
        Self::with_heap_policy(options, heap_policy(options))
    }

    fn with_heap_policy(
        options: &EvaluatorOptions,
        policy: big_boy_pants_js::HeapPolicy,
    ) -> Result<Self, EvalError> {
        let runtime = Runtime::try_with_heap_policy(
            big_boy_pants_js::VmLimits {
                max_stack_values: (options.stack_limit / 16).max(1),
                ..Default::default()
            },
            policy,
        )
        .map_err(|e| match e {
            big_boy_pants_js::RuntimeStartError::HeapLimit => EvalError(format!(
                "macro memory limit of {} bytes cannot hold the native macro runtime's realm \
                 (about 1.2 MB of accounted heap before any macro runs); raise --macro-memory-limit",
                options.memory_limit
            )),
            e => error(e),
        })?;
        let keys = crate::CoreFnId::ALL
            .iter()
            .map(|k| format!("{:?}", k.runtime_key()))
            .collect::<Vec<_>>()
            .join(",");
        // Run the same bounded first-party reader on native constructors. Only
        // this unit is included, not the legacy runtime or an evaluator fallback.
        let manifest = crate::runtime_manifest::RuntimeManifest::embedded();
        let reader = native_reader_source(manifest);
        let reader = include_str!("bbp_reader.js").replace("/* READER */", &reader);
        let source = include_str!("bbp_runtime.js")
            .replace("/* NATIVE_READER */", &reader)
            .replace("/* CORE_KEYS */", &keys);
        let executable = big_boy_pants_js::frontend::compile_function(&format!(
            "function nativeMacroTask(commandKind, beginKind, finishKind, readerTagsCell, rawMembersCell, stringKind) {{ {source} }}"
        ))
        .map_err(error)?;
        let control = ExecutionControl::new(options)?;
        let reader_tags = runtime
            .new_atom(&runtime.root(Primitive::Null))
            .map_err(error)?;
        let raw_members = runtime
            .new_atom(&runtime.root(Primitive::Null))
            .map_err(error)?;
        Ok(Self(Arc::new(SharedRuntime {
            _live: crate::compile_stats::Live::new(&crate::compile_stats::LIVE_ENGINES),
            memory_limit: options.memory_limit,
            counters: Arc::new(NativeCounters::default()),
            runtime,
            executable,
            reader_tags,
            raw_members,
            bridge: Arc::new(Mutex::new(Bridge {
                control: Some(control.clone()),
                ..Default::default()
            })),
            control,
            next_command: AtomicU32::new(16),
            definitions: Mutex::new(HashMap::new()),
            generation: Mutex::new(0),
            changed: Condvar::new(),
            graph: Mutex::new(big_boy_pants_js::DependencyGraph::new()),
            pending_initializers: Mutex::new(HashMap::new()),
            factories: Mutex::new(HashMap::new()),
            catalog: Mutex::new(catalog::Registry::default()),
            idle: Mutex::new(Vec::new()),
            parked: Mutex::new(HashMap::new()),
            lanes: std::sync::OnceLock::new(),
            namespaces: Arc::default(),
            #[cfg(test)]
            on_wait: Mutex::new(None),
            #[cfg(test)]
            on_initializer: Mutex::new(None),
        })))
    }

    /// Order this runtime's shared atom accesses by `lanes`, its
    /// compilation's.
    pub(crate) fn set_lanes(&self, lanes: Arc<crate::effect_lanes::Lanes>) {
        let _ = self.0.lanes.set(lanes);
    }
    fn lanes(&self) -> Option<&Arc<crate::effect_lanes::Lanes>> {
        self.0.lanes.get()
    }
    /// Complete lane cell `key` for its parked task, whose lane opened.
    fn open_lane(&self, key: DependencyKey, producer: big_boy_pants_js::ProducerId) {
        let value = self.0.runtime.root(Primitive::Null);
        if let Ok(wakeups) = self
            .0
            .runtime
            .complete(key, producer, Completion::Ready(&value))
        {
            self.wake(wakeups);
        }
        *self.0.generation.lock().unwrap() += 1;
        self.0.changed.notify_all();
    }
    /// The aggregate memory budget `new` received; spawned evaluators share it.
    pub fn memory_limit(&self) -> usize {
        self.0.memory_limit
    }
    pub fn evaluator(&self) -> Result<BbpEvaluator, EvalError> {
        BbpEvaluator::in_runtime(self.clone(), 0)
    }
    /// Readers share executable code and the aggregate budget, while their
    /// imported forms, generated spellings and namespace inventory stay
    /// apart from macro tasks. Readers have no compilation namespace floor.
    pub(crate) fn data_reader_evaluator(&self) -> Result<BbpEvaluator, EvalError> {
        let mut evaluator = BbpEvaluator::shell(self.clone(), 0);
        evaluator.bridge = Arc::new(Mutex::new(Bridge {
            control: Some(self.0.control.clone()),
            ..Default::default()
        }));
        evaluator.private_namespaces = Some(Arc::default());
        evaluator.boot()?;
        Ok(evaluator)
    }
    /// A pool slot's evaluator: on an idle task context another slot parked,
    /// with the state of the view that parked it, or, if none is idle, on a
    /// fresh one.
    pub(crate) fn lent_evaluator(&self) -> Result<(BbpEvaluator, Option<LentState>), EvalError> {
        let evaluator = BbpEvaluator::shell(self.clone(), 0);
        match evaluator.adopt() {
            Some(state) => Ok((evaluator, Some(state))),
            None => {
                evaluator.boot()?;
                Ok((evaluator, None))
            }
        }
    }
    /// Queue on its pool the next step of each task `wakeups` made runnable
    /// that parked there ([`BbpEvaluator::park_until_woken`]); a task its
    /// owner still runs itself is not parked and is left to it.
    fn wake(&self, wakeups: big_boy_pants_js::Wakeups) {
        self.wake_tasks(wakeups.into_runnable());
    }
    fn wake_tasks(&self, tasks: impl IntoIterator<Item = Task>) {
        for task in tasks {
            let parked = {
                let mut parked = self.0.parked.lock().unwrap();
                // The owner may have polled the old settlement itself and
                // reached a newer dependency before this wake was drained.
                // Hold the registration lock while checking readiness, so
                // that old notification cannot take a newer parked step.
                if task.status() == big_boy_pants_js::TaskStatus::Ready {
                    parked.remove(&task.id())
                } else {
                    None
                }
            };
            if let Some(Parked {
                step,
                reply,
                submit,
            }) = parked
            {
                self.0.counters.woken.fetch_add(1, Ordering::Relaxed);
                submit(step, reply);
            }
        }
    }
    /// The idle task context with the most catalog registrations installed.
    fn take_idle(&self) -> Option<TaskContext> {
        let mut idle = self.0.idle.lock().unwrap();
        let best = (0..idle.len()).max_by_key(|&i| idle[i].lent.borrow().installed)?;
        let cx = idle.swap_remove(best);
        drop(idle);
        self.0.counters.borrowed.fetch_add(1, Ordering::Relaxed);
        Some(cx)
    }
    /// Drop the idle task contexts, once no pool slot will borrow one.
    pub(crate) fn clear_idle(&self) {
        let idle = std::mem::take(&mut *self.0.idle.lock().unwrap());
        drop(idle);
    }
    pub(crate) fn cancel(&self) {
        self.0.control.cancel();
        self.0.changed.notify_all();
    }
    pub fn helper_counts(&self) -> crate::compile_stats::NativeHelperCounts {
        let c = &self.0.counters;
        let heap = self.0.runtime.heap_stats();
        crate::compile_stats::NativeHelperCounts {
            heap: Some(crate::compile_stats::NativeHeapStats {
                collections: heap.collections,
                live_objects: heap.last_live_objects,
                managed_bytes: heap.managed_bytes,
                peak_managed_bytes: heap.peak_managed_bytes,
                live_bytes: heap.last_live_bytes,
                admitted_bytes: heap.admitted_bytes_total,
                byte_limit: heap.byte_limit.unwrap_or(0),
            }),
            compiled: c.compiled.load(Ordering::Relaxed),
            published: c.published.load(Ordering::Relaxed),
            per_task: c.per_task.load(Ordering::Relaxed),
            waits: c.waits.load(Ordering::Relaxed),
            steps: c.steps.load(Ordering::Relaxed),
            migrations: c.migrations.load(Ordering::Relaxed),
            contexts: c.contexts.load(Ordering::Relaxed),
            units_compiled: c.units_compiled.load(Ordering::Relaxed),
            units_shared: c.units_shared.load(Ordering::Relaxed),
            units_evaluated: c.units_evaluated.load(Ordering::Relaxed),
            contexts_lent: c.lent.load(Ordering::Relaxed),
            contexts_borrowed: c.borrowed.load(Ordering::Relaxed),
            woken: c.woken.load(Ordering::Relaxed),
            bridge: self.0.bridge.lock().unwrap().counters,
            log_reads: c.log_reads.load(Ordering::Relaxed),
            log_edges_copied: c.log_edges_copied.load(Ordering::Relaxed),
            log_edges_recorded: c.log_edges_recorded.load(Ordering::Relaxed),
            catalog_uses: c.catalog_uses.load(Ordering::Relaxed),
            catalog_entries: c.catalog_entries.load(Ordering::Relaxed),
            catalog_nanos: c.catalog_nanos.load(Ordering::Relaxed),
            producers: c.producers.load(Ordering::Relaxed),
            producer_replayed: c.producer_replayed.load(Ordering::Relaxed),
            producer_catalog_entries: c.producer_catalog_entries.load(Ordering::Relaxed),
            producer_nanos: c.producer_nanos.load(Ordering::Relaxed),
        }
    }
    /// The runtime's one published factory for unit `source`, compiled now
    /// if no task has asked for it yet; `None` if it cannot be shared
    /// (a syntax error, or a value that does not publish), in which case
    /// the asking task evaluates the source itself and so reports exactly
    /// what evaluation reports. Concurrent first requests compile it once:
    /// the others wait for that compilation, which runs no task code that
    /// could wait in turn.
    fn factory(&self, source: &str) -> Result<Option<Rooted>, EvalError> {
        let cell = self
            .0
            .factories
            .lock()
            .unwrap()
            .entry(source.to_owned())
            .or_default()
            .clone();
        let mut failure = None;
        let factory = cell.get_or_init(|| match self.compile_factory(source) {
            Ok(factory) => {
                if factory.is_some() {
                    self.0
                        .counters
                        .units_compiled
                        .fetch_add(1, Ordering::Relaxed);
                }
                factory
            }
            Err(error) => {
                failure = Some(error);
                None
            }
        });
        match failure {
            // A cancelled or exhausted build: no task may evaluate it later.
            Some(error) => Err(error),
            None => Ok(factory.clone()),
        }
    }
    fn compile_factory(&self, source: &str) -> Result<Option<Rooted>, EvalError> {
        // As indirect eval does: sloppy global code, named `<eval>` in
        // diagnostics, compiled on a bounded helper stack.
        let text = format!("({source})");
        let compiled = std::thread::Builder::new()
            .name("bbp-unit-compiler".into())
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                use big_boy_pants_js::frontend::{self, ScriptOptions, ast};
                // Only a function literal: evaluating it runs no code, so
                // the factory a fresh task makes is the one any task would.
                // Another expression may read the evaluating task's state.
                let literal =
                    frontend::parse_script_with(&text, ScriptOptions::SLOPPY).is_ok_and(|script| {
                        matches!(
                            script.body.as_slice(),
                            [ast::Stmt {
                                kind: ast::StmtKind::Expr(ast::Expr {
                                    kind: ast::ExprKind::Function(_),
                                    ..
                                }),
                                ..
                            }]
                        )
                    });
                literal
                    .then(|| frontend::compile_script_with("<eval>", &text, ScriptOptions::SLOPPY))
                    .and_then(Result::ok)
            })
            .map_err(error)?
            .join()
            .map_err(|_| error("unit compiler panicked"))?;
        let Some(executable) = compiled else {
            return Ok(None);
        };
        let producer = self.0.runtime.spawn(&executable, &[]).map_err(error)?;
        loop {
            self.0.control.poll()?;
            match producer
                .run_with(Budget::instructions(10_000), &mut RejectHost)
                .map_err(error)?
            {
                RunOutcome::Complete(value) => {
                    return Ok(self.0.runtime.publish(&value).is_ok().then_some(value));
                }
                RunOutcome::Yielded => {}
                _ => return Ok(None),
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn on_wait(&self, hook: WaitHook) {
        *self.0.on_wait.lock().unwrap() = Some(hook);
    }
}

/// One task of the runtime with what the compiler keeps about it: its
/// boundaries, value handles and registrations, and, while it waits idle
/// for a pool slot to borrow it ([`BbpRuntime::lent_evaluator`]), the state
/// of the worker view that used it last. It is `Send`, so an idle context
/// moves between slots; one evaluator on one thread uses it at a time.
pub(crate) struct TaskContext {
    task: Task,
    command_kind: ResultKind,
    begin_kind: ResultKind,
    finish_kind: ResultKind,
    string_kind: ResultKind,
    string_answer: RefCell<Option<Rooted>>,
    initializers: RefCell<HashMap<DependencyKey, ProducerRequest>>,
    /// The lane entry of each initializer it runs, which inherits the
    /// position of what waits for it ([`crate::effect_lanes`]).
    initializer_lanes: RefCell<HashMap<DependencyKey, crate::effect_lanes::EntryGuard>>,
    /// Candidate callers lend their position to a reserved initializer
    /// until its source-order lane selects the task that will execute it.
    initializer_waits: RefCell<HashMap<DependencyKey, Option<crate::effect_lanes::WaitGuard>>>,
    resumable: Cell<bool>,
    per_task: Arc<AtomicUsize>,
    last_thread: Arc<Mutex<std::thread::ThreadId>>,
    registrations: RefCell<Vec<String>>,
    catalog_snapshot: RefCell<Option<Rooted>>,
    session: u64,
    next: Cell<u64>,
    values: RefCell<HashMap<u64, Rooted>>,
    /// Source macro declarations materialized in this task, keyed by code
    /// identity. A producer cannot use its parked parent's function handles;
    /// this cache also travels with a context borrowed by another pool slot.
    macro_functions: RefCell<HashMap<usize, ValueId>>,
    /// This task's handle on its `globalThis.__lightNativeRuntime`, which
    /// shared unit factories are called with; made on first use.
    native_runtime: Cell<Option<ValueId>>,
    gensym_scope: Cell<u64>,
    /// The catalog frontier this task populated from shared immutable data
    /// ([`MacroEvaluator::catalog_cursor`]).
    catalog: Cell<Option<usize>>,
    /// The last namespace table it registered and where: a table that
    /// extends it in the same activity, no earlier, records only its tail.
    namespaces: RefCell<(
        crate::effect_lanes::NamespaceTag,
        crate::arena::Arena<String>,
    )>,
    /// Whether a worker view prepared it (its settings and service stubs
    /// evaluated), which a view that borrows it then does not repeat.
    prepared: Cell<bool>,
    /// The borrowing view's state that names this task's values or
    /// registrations, kept here while the context is idle.
    lent: RefCell<LentState>,
    /// How many of the task's dependency-log edges the dependency graph
    /// has been given ([`BbpEvaluator::run_inner`]).
    logged: Cell<usize>,
}
const _: () = {
    const fn send<T: Send>() {}
    send::<TaskContext>();
};
/// What a worker view knows about the task it borrowed: how many catalog
/// registrations it installed and the reader declarations it sent
/// (`EngineBinding::reader_declared`). Source macro functions belong to the
/// context itself, including those a helper's producer task materializes.
#[derive(Default)]
pub(crate) struct LentState {
    pub(crate) installed: usize,
    pub(crate) reader_declared: HashMap<String, (usize, usize)>,
}
/// A compile-time engine on a [`BbpRuntime`]: the services of the thread
/// that uses it, over one task context, which a pool slot's view returns to
/// the runtime between jobs and borrows again ([`Self::park`]).
pub struct BbpEvaluator {
    shared: BbpRuntime,
    runtime: Runtime,
    control: ExecutionControl,
    service_depth: usize,
    service: RefCell<Option<UnitService>>,
    string_services: RefCell<HashMap<String, StringService>>,
    help: RefCell<Option<WaitService>>,
    runner: RefCell<Option<NativeRunner>>,
    submitter: RefCell<Option<NativeSubmitter>>,
    bridge: Arc<Mutex<Bridge>>,
    /// Compiler readers keep their inventory apart from compilation tasks.
    private_namespaces: Option<Arc<Mutex<namespaces::NamespaceLog>>>,
    stats: Cell<BridgeStats>,
    context: RefCell<Option<Rc<TaskContext>>>,
}
impl BbpEvaluator {
    pub fn new(options: EvaluatorOptions) -> Result<Self, EvalError> {
        BbpRuntime::new(&options)?.evaluator()
    }
    fn in_runtime(shared: BbpRuntime, service_depth: usize) -> Result<Self, EvalError> {
        let this = Self::shell(shared, service_depth);
        this.boot()?;
        Ok(this)
    }
    /// An evaluator without a task context yet.
    fn shell(shared: BbpRuntime, service_depth: usize) -> Self {
        Self {
            runtime: shared.0.runtime.clone(),
            control: shared.0.control.clone(),
            bridge: shared.0.bridge.clone(),
            private_namespaces: None,
            shared,
            service_depth,
            service: RefCell::new(None),
            string_services: RefCell::new(HashMap::new()),
            help: RefCell::new(None),
            runner: RefCell::new(None),
            submitter: RefCell::new(None),
            stats: Cell::new(BridgeStats::default()),
            context: RefCell::new(None),
        }
    }
    fn namespace_log(&self) -> &Arc<Mutex<namespaces::NamespaceLog>> {
        self.private_namespaces
            .as_ref()
            .unwrap_or(&self.shared.0.namespaces)
    }
    fn namespace_lanes(&self) -> Option<&Arc<crate::effect_lanes::Lanes>> {
        if self.private_namespaces.is_some() {
            None
        } else {
            self.shared.lanes()
        }
    }
    /// Give this evaluator a fresh task and run it to its first boundary.
    fn boot(&self) -> Result<(), EvalError> {
        let runtime = &self.runtime;
        let kind = self
            .shared
            .0
            .next_command
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |kind| {
                kind.checked_add(4)
            })
            .map_err(|_| error("native task identity exhausted"))?;
        let argument = runtime.root(Primitive::Number(kind as f64));
        let begin = runtime.root(Primitive::Number((kind + 1) as f64));
        let finish = runtime.root(Primitive::Number((kind + 2) as f64));
        let string = runtime.root(Primitive::Number((kind + 3) as f64));
        let task = runtime
            .spawn(
                &self.shared.0.executable,
                &[
                    &argument,
                    &begin,
                    &finish,
                    &self.shared.0.reader_tags,
                    &self.shared.0.raw_members,
                    &string,
                ],
            )
            .map_err(error)?;
        *self.context.borrow_mut() = Some(Rc::new(TaskContext {
            task,
            command_kind: ResultKind::new(kind),
            begin_kind: ResultKind::new(kind + 1),
            finish_kind: ResultKind::new(kind + 2),
            string_kind: ResultKind::new(kind + 3),
            string_answer: RefCell::new(None),
            initializers: RefCell::new(HashMap::new()),
            initializer_waits: RefCell::new(HashMap::new()),
            initializer_lanes: RefCell::new(HashMap::new()),
            resumable: Cell::new(false),
            per_task: Arc::new(AtomicUsize::new(0)),
            last_thread: Arc::new(Mutex::new(std::thread::current().id())),
            registrations: RefCell::new(Vec::new()),
            catalog_snapshot: RefCell::new(None),
            session: NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
            next: Cell::new(1),
            values: RefCell::new(HashMap::new()),
            macro_functions: RefCell::new(HashMap::new()),
            native_runtime: Cell::new(None),
            gensym_scope: Cell::new(0),
            catalog: Cell::new(None),
            namespaces: RefCell::default(),
            prepared: Cell::new(false),
            lent: RefCell::new(LentState::default()),
            logged: Cell::new(0),
        }));
        self.shared
            .0
            .counters
            .contexts
            .fetch_add(1, Ordering::Relaxed);
        let booted = self.run();
        if booted.is_err() {
            self.context.borrow_mut().take();
        }
        booted
    }
    /// The task context this evaluator runs on.
    fn cx(&self) -> Result<Rc<TaskContext>, EvalError> {
        self.context
            .borrow()
            .clone()
            .ok_or_else(|| error("the evaluator has no task context"))
    }
    /// How many registrations its task holds.
    #[cfg(test)]
    pub(crate) fn registration_count(&self) -> usize {
        self.cx().unwrap().registrations.borrow().len()
    }
    /// Whether it has a task context: a pool slot's evaluator has none
    /// between jobs once it parked its own ([`Self::park`]).
    pub(crate) fn has_context(&self) -> bool {
        self.context.borrow().is_some()
    }
    /// Whether its task context was prepared by a worker view
    /// ([`TaskContext::prepared`]): a view that borrows it does not
    /// evaluate its settings and services again.
    pub(crate) fn prepared(&self) -> bool {
        self.context
            .borrow()
            .as_ref()
            .is_some_and(|cx| cx.prepared.get())
    }
    /// Return the task context to the runtime's idle contexts with the state
    /// of the view that used it, for any pool slot to borrow
    /// ([`Self::adopt`]). The evaluator has no context afterwards. A context
    /// still referenced or mid-initialization is dropped instead; false then.
    pub(crate) fn park(&self, state: LentState) -> bool {
        let Some(cx) = self.context.borrow_mut().take() else {
            return false;
        };
        let Ok(cx) = Rc::try_unwrap(cx) else {
            return false;
        };
        if !cx.initializers.borrow().is_empty() || self.control.check().is_err() {
            return false;
        }
        cx.prepared.set(true);
        *cx.lent.borrow_mut() = state;
        self.shared.0.idle.lock().unwrap().push(cx);
        self.shared.0.counters.lent.fetch_add(1, Ordering::Relaxed);
        true
    }
    /// Borrow an idle task context of the runtime, the one with the most
    /// catalog registrations installed, if this evaluator has none and one
    /// is idle: the state of the view that parked it, for this one to take.
    pub(crate) fn adopt(&self) -> Option<LentState> {
        if self.has_context() {
            return None;
        }
        let cx = self.shared.take_idle()?;
        let state = std::mem::take(&mut *cx.lent.borrow_mut());
        *self.context.borrow_mut() = Some(Rc::new(cx));
        Some(state)
    }
    pub fn bridge_stats(&self) -> BridgeStats {
        let mut stats = self.stats.get();
        if let Some(cx) = self.context.borrow().as_ref() {
            stats.helper_per_task += cx.per_task.load(Ordering::Relaxed);
        }
        // Collections are runtime-wide: every worker's tasks share this heap.
        let gc = self.runtime.heap_stats();
        BridgeStats {
            collections: gc.collections,
            gc_pause_nanos: gc.pause_total.as_nanos(),
            gc_pause_max_nanos: gc.pause_max.as_nanos(),
            gc_live_objects: gc.last_live_objects,
            gc_managed_bytes: gc.managed_bytes,
            gc_peak_managed_bytes: gc.peak_managed_bytes,
            gc_live_bytes: gc.last_live_bytes,
            gc_admitted_bytes: gc.admitted_bytes_total,
            gc_byte_limit: gc.byte_limit.unwrap_or(0),
            ..stats
        }
    }
    fn note(&self, update: impl FnOnce(&mut BridgeStats)) {
        let mut stats = self.stats.get();
        let before = stats;
        update(&mut stats);
        let c = &self.shared.0.counters;
        for (counter, delta) in [
            (&c.compiled, stats.helper_compiled - before.helper_compiled),
            (
                &c.published,
                stats.helper_published - before.helper_published,
            ),
            (&c.per_task, stats.helper_per_task - before.helper_per_task),
            (&c.waits, stats.helper_waits - before.helper_waits),
        ] {
            if delta != 0 {
                counter.fetch_add(delta, Ordering::Relaxed);
            }
        }
        self.stats.set(stats);
    }
    fn with_bridge<T>(&self, work: impl FnOnce(&mut Bridge) -> T) -> T {
        let mut bridge = self.bridge.lock().unwrap();
        let before = bridge.stats;
        let result = work(&mut bridge);
        let after = bridge.stats;
        self.note(|s| {
            s.imported += after.imported - before.imported;
            s.exported += after.exported - before.exported;
            s.reused += after.reused - before.reused;
        });
        result
    }
    pub(crate) fn preload(&self, name: &str, source: &str) -> Result<(), EvalError> {
        let value = self.evaluate_unit(source)?;
        let escaped = name
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r");
        let install = self.evaluate_unit(&format!(
            "rt => f => {{
                Object.defineProperty(globalThis.__lightCompilerRoots, \"{escaped}\",
                    {{value:f,writable:true,configurable:true}});
                return null;
            }}"
        ))?;
        let result = self.call(install, &[value]);
        let _ = self.release(value);
        let _ = self.release(install);
        self.release(result?)
    }
    fn run(&self) -> Result<(), EvalError> {
        let cx = self.cx()?;
        let result = self.run_inner(&cx);
        if let Err(failure) = &result {
            self.shared.0.graph.lock().unwrap().withdraw(cx.task.id());
            self.fail_initializers(&cx, &failure.0);
            let _cancel = cx.task.cancel();
            // The VM bounds cancellation cleanup independently of the spent
            // aggregate fuel. Finally blocks finish on saved task frames.
            while matches!(
                cx.task.run_with(
                    Budget::instructions(10_000),
                    &mut NativeHost(
                        &cx.per_task,
                        &self.shared.0.counters,
                        None,
                        &crate::generated_names::NativeGensyms::capture(),
                        &self.bridge,
                        self.namespace_log(),
                        Some(&self.runtime),
                    )
                ),
                Ok(RunOutcome::Yielded)
            ) {}
        }
        result
    }
    fn step(&self, cx: &TaskContext) -> NativeStep {
        NativeStep {
            task: cx.task.clone(),
            per_task: cx.per_task.clone(),
            counters: self.shared.0.counters.clone(),
            lanes: self.namespace_lanes().cloned(),
            last_thread: cx.last_thread.clone(),
            admitted: Arc::new(Mutex::new(true)),
            gensyms: crate::generated_names::NativeGensyms::capture(),
            bridge: self.bridge.clone(),
            namespaces: self.namespace_log().clone(),
            runtime: self.runtime.clone(),
        }
    }
    fn run_inner(&self, cx: &TaskContext) -> Result<(), EvalError> {
        // The step a pool worker already ran after a wakeup.
        let mut woken = None;
        loop {
            self.control.poll()?;
            // Its host effects see this thread's current activity, on
            // whichever worker its steps run.
            if let Some(lanes) = self.shared.lanes() {
                lanes.sync_task(crate::effect_lanes::identity(cx.task.id()));
            }
            let outcome = match woken.take() {
                Some(outcome) => outcome,
                None => {
                    let step = self.step(cx);
                    // Before the first helper wait, the worker already owns
                    // this runnable task. Only suspended continuations need
                    // pool handoff.
                    let runner = cx
                        .resumable
                        .get()
                        .then(|| self.runner.borrow().clone())
                        .flatten();
                    match runner {
                        Some(run) => run(step),
                        None => step.run(),
                    }
                    .map_err(error)?
                }
            };
            // Lazy once-cells settle inside VM steps, independently of host
            // dependency completion. Their readers need the same pool handoff
            // as helper readers, including when the realizer parked in a cold
            // protocol/helper call before finishing the thunk.
            self.shared.wake_tasks(self.runtime.take_lazy_wakeups());
            {
                let mut graph = self.shared.0.graph.lock().unwrap();
                // The log only grows, and the graph keeps every use it was
                // given: hand it the edges recorded since the last step.
                // The coordinator's task lives for the whole build, so
                // rereading its full log made each step cost the build's
                // length so far (light-cljs-575).
                let edges = cx.task.dependency_edges_since(cx.logged.get());
                cx.logged.set(cx.logged.get() + edges.len());
                let mut recorded = 0;
                for key in &edges {
                    if [BINDING, HELPER, INITIALIZER].contains(&key.kind()) {
                        graph.record_use(cx.task.id(), *key);
                        recorded += 1;
                    }
                }
                if crate::compile_trace::any_recording() {
                    let c = &self.shared.0.counters;
                    c.log_reads.fetch_add(1, Ordering::Relaxed);
                    c.log_edges_copied.fetch_add(edges.len(), Ordering::Relaxed);
                    c.log_edges_recorded.fetch_add(recorded, Ordering::Relaxed);
                }
                graph.withdraw(cx.task.id());
                if let RunOutcome::Waiting(key) = outcome
                    && [HELPER, INITIALIZER].contains(&key.kind())
                {
                    graph
                        .wait(
                            cx.task.id(),
                            key,
                            big_boy_pants_js::DemandSite::new("native compiler helper"),
                        )
                        .map_err(error)?;
                }
            }
            match outcome {
                RunOutcome::Waiting(key) if key.kind() == cx.command_kind => {
                    cx.string_answer.borrow_mut().take();
                    return Ok(());
                }
                RunOutcome::Waiting(key) => woken = self.demand(cx, key)?,
                RunOutcome::Yielded => {}
                RunOutcome::Failed(e) => return Err(error(e)),
                RunOutcome::Cancelled => return Err(error("macro task cancelled")),
                other => return Err(error(format!("unexpected session outcome: {other:?}"))),
            }
        }
    }
    /// Wait for `key`, producing what this task was asked to produce first.
    /// In a pool, a task that waits for another's result is parked: the
    /// completion that wakes it queues its next step on the pool
    /// ([`BbpRuntime::wake`]), whose outcome this returns.
    fn demand(
        &self,
        cx: &TaskContext,
        key: DependencyKey,
    ) -> Result<Option<RunOutcome>, EvalError> {
        if [HELPER, INITIALIZER, LANE, NAMESPACE].contains(&key.kind()) {
            cx.resumable.set(true);
        }
        // What produces `key` inherits this task's lane position while it
        // waits ([`crate::effect_lanes`]).
        let lane_wait = self.shared.lanes().and_then(|lanes| {
            if key.kind() == HELPER || key.kind() == INITIALIZER {
                Some(lanes.wait_for(crate::effect_lanes::Target::Cell(
                    crate::effect_lanes::identity(key),
                )))
            } else {
                self.runtime.lazy_realizer(key).and_then(|task| {
                    lanes.wait_for_task(crate::effect_lanes::identity(task), key.dependency().get())
                })
            }
        });
        #[cfg(test)]
        let hook = self.shared.0.on_wait.lock().unwrap().clone();
        #[cfg(test)]
        if let Some(hook) = hook {
            hook(&cx.task, key);
        }
        if key.kind() == HELPER || key.kind() == INITIALIZER {
            self.note(|s| s.helper_waits += 1);
        }
        let shared = &self.shared.0;
        let requests = cx.task.take_producer_requests();
        for request in requests {
            if request.key().kind() == LANE {
                self.attach_lane(&request);
                continue;
            }
            let result = self.produce(cx, &request);
            if let Err(failure) = &result {
                self.fail_initializers(cx, &failure.0);
            }
            let value = match &result {
                Ok(value) => value.clone(),
                Err(error) => self.failure_root(&error.to_string()),
            };
            let completion = match result {
                Ok(_) if request.key().kind() == cx.string_kind => {
                    // Keep only the current response through its consuming VM
                    // step. Older mailbox results must not retain resources.
                    *cx.string_answer.borrow_mut() = Some(value.clone());
                    Completion::WeakReady(&value)
                }
                Ok(_) => Completion::Ready(&value),
                Err(_) => Completion::Failed(&value),
            };
            // Completing wakes saved VM frames; it never executes JS inline.
            shared.graph.lock().unwrap().resolve(request.key());
            let wakeups = self
                .runtime
                .complete(request.key(), request.producer(), completion)
                .map_err(error)?;
            self.shared.wake(wakeups);
            *shared.generation.lock().unwrap() += 1;
            shared.changed.notify_all();
        }
        let runner = cx.resumable.get() && self.runner.borrow().is_some();
        if let Some(submit) = runner
            .then(|| self.submitter.borrow().clone())
            .flatten()
            .and_then(|submitter| submitter())
        {
            return self.park_until_woken(cx, key, submit, lane_wait.as_ref());
        }
        let mut generation = shared.generation.lock().unwrap();
        // A completion publishes its cell before delivering the task's wake.
        // Only that wake makes this saved continuation runnable.
        while cx.task.status() == big_boy_pants_js::TaskStatus::Waiting {
            self.control.poll()?;
            if cx.task.cancel_requested() {
                return Err(error("macro task cancelled"));
            }
            drop(generation);
            self.lane_waiting(key, lane_wait.as_ref());
            generation = shared.generation.lock().unwrap();
            if let Some(help) = self.help.borrow().clone() {
                drop(generation);
                let helped = help().map_err(error)?;
                generation = shared.generation.lock().unwrap();
                if helped {
                    continue;
                }
                // The wake could have arrived while this thread helped.
                if cx.task.status() != big_boy_pants_js::TaskStatus::Waiting {
                    break;
                }
            }
            generation = shared
                .changed
                .wait_timeout(generation, std::time::Duration::from_millis(20))
                .unwrap()
                .0;
        }
        Ok(None)
    }
    /// Park this task until a completion of `key` wakes it, then return the
    /// outcome of the step that woke it, which `submit` queued on the pool
    /// for any worker, this one included while it helps. If `key`'s wake
    /// arrived before pool registration, nothing was queued and this returns
    /// `None`: the caller runs the step.
    /// While a task waits for `key`: refresh the position what produces it
    /// inherits, and for a lane cell, have a starved earliest unit analyzed
    /// ([`crate::effect_lanes::Lanes::rescue`]).
    fn lane_waiting(&self, key: DependencyKey, wait: Option<&crate::effect_lanes::WaitGuard>) {
        if let Some(wait) = wait {
            wait.refresh();
        }
        if key.kind() == LANE
            && let Some(lanes) = self.shared.lanes()
        {
            lanes.poll();
            lanes.rescue();
        }
    }
    /// Give the compilation's effect lanes the lane cell this task's need
    /// claimed, which they complete once its lane opens.
    fn attach_lane(&self, request: &ProducerRequest) {
        let (key, producer) = (request.key(), request.producer());
        let runtime = self.shared.clone();
        let open = Box::new(move || runtime.open_lane(key, producer));
        match self.shared.lanes() {
            Some(lanes) => lanes.attach(key.dependency().get(), open),
            None => open(),
        }
    }
    fn park_until_woken(
        &self,
        cx: &TaskContext,
        key: DependencyKey,
        submit: NativeSubmit,
        lane_wait: Option<&crate::effect_lanes::WaitGuard>,
    ) -> Result<Option<RunOutcome>, EvalError> {
        let shared = &self.shared.0;
        let id = cx.task.id();
        let step = self.step(cx);
        // Until the woken step's outcome arrives, unwinding first takes the
        // task back from a queued or running step (`NativeHandoff`).
        let mut handoff = step.handoff();
        let (reply, outcome) = std::sync::mpsc::sync_channel(1);
        shared.parked.lock().unwrap().insert(
            id,
            Parked {
                step,
                reply,
                submit,
            },
        );
        let _unpark = Unpark(shared, id);
        // A wake delivered before pool registration found nothing to queue.
        // Cell settlement alone is insufficient: it precedes wake delivery.
        // Check readiness under the registration lock, just like wake_tasks,
        // so either the owner reclaims this step or the wake queues it.
        let reclaimed = {
            let mut parked = shared.parked.lock().unwrap();
            cx.task.status() == big_boy_pants_js::TaskStatus::Ready && parked.remove(&id).is_some()
        };
        if reclaimed {
            handoff.finish();
            return Ok(None);
        }
        loop {
            match outcome.try_recv() {
                Ok(result) => {
                    handoff.finish();
                    return result.map(Some).map_err(error);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(error("native task worker stopped"));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
            self.control.poll()?;
            if cx.task.cancel_requested() {
                return Err(error("macro task cancelled"));
            }
            self.lane_waiting(key, lane_wait);
            if let Some(help) = self.help.borrow().clone()
                && help().map_err(error)?
            {
                continue;
            }
            match outcome.recv_timeout(std::time::Duration::from_millis(20)) {
                Ok(result) => {
                    handoff.finish();
                    return result.map(Some).map_err(error);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(error("native task worker stopped"));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
    fn produce(&self, cx: &TaskContext, request: &ProducerRequest) -> Result<Rooted, EvalError> {
        let key = request.key();
        if key.kind() == cx.string_kind {
            // The VM has parked before this thread-local callback runs. A
            // pooled step carries no Rc, analyzer borrow or resource provider.
            let text = |name| {
                cx.task
                    .global_value(name)
                    .map_err(error)?
                    .and_then(|value| value.string().map(|s| s.to_string_lossy()))
                    .ok_or_else(|| error("invalid string service request"))
            };
            let name = text("__lightStringServiceName")?;
            let request = text("__lightStringServiceRequest")?;
            let callback = self
                .string_services
                .borrow()
                .get(&name)
                .cloned()
                .ok_or_else(|| error("string service is not installed"))?;
            let answer = callback(&request);
            cx.string_answer.borrow_mut().take();
            let import = || {
                self.runtime.scope(|scope| {
                    let text = scope.try_primitive(Primitive::string(&answer))?;
                    // A weak completion of a primitive string still owns its
                    // backing Arc. Put it in a GC-managed message instead.
                    let message = scope.new_vector(&[text]).ok()?;
                    Some(scope.persist(message))
                })
            };
            return import()
                .or_else(|| {
                    // Retry allocation after collecting, retaining the exact
                    // answer rather than replaying the resource read.
                    let _ = self.runtime.request_collection();
                    import()
                })
                .ok_or_else(|| error("heap byte limit exceeded"));
        }
        if key.kind() == cx.begin_kind || key.kind() == cx.finish_kind {
            return self.initialize(cx, key.kind() == cx.begin_kind);
        }
        if key.kind() != HELPER {
            return Err(error(format!("unsupported compiler result: {key}")));
        }
        if self.service_depth >= 16 {
            return Err(error("compiler helper nesting limit exceeded"));
        }
        let id = *self
            .shared
            .0
            .definitions
            .lock()
            .unwrap()
            .get(&key)
            .ok_or_else(|| error("unregistered compiler helper"))?;
        let callback = self
            .service
            .borrow()
            .clone()
            .ok_or_else(|| error("compiler helper service is not installed"))?;
        // The helper's production inherits this task's lane position, and
        // the position of whatever else waits for it.
        let _lane = self
            .shared
            .lanes()
            .map(|lanes| lanes.enter_cell(crate::effect_lanes::identity(key)));
        let active = self.spawn_producer(cx, Some(callback.clone()), |task| {
            self.shared
                .0
                .graph
                .lock()
                .unwrap()
                .bind_producer(key, request.producer(), task);
        })?;
        let source = callback(&active, id);
        active.note_catalog_applied();
        let source = source.map_err(error)?;
        // Only the factory expression runs here. Its initializer runs in each
        // consumer's task, preserving task-owned mutable values.
        let executable =
            big_boy_pants_js::frontend::compile_script("compiler helper", &format!("({source})"))
                .map_err(error)?;
        let producer = self.runtime.spawn(&executable, &[]).map_err(error)?;
        loop {
            self.control.poll()?;
            match producer
                .run_with(Budget::instructions(10_000), &mut RejectHost)
                .map_err(error)?
            {
                RunOutcome::Complete(value) => {
                    self.runtime.publish(&value).map_err(error)?;
                    self.note(|s| s.helper_compiled += 1);
                    return Ok(value);
                }
                RunOutcome::Yielded => {}
                other => {
                    return Err(error(format!(
                        "unexpected helper factory outcome: {other:?}"
                    )));
                }
            }
        }
    }
    /// A producer for this task's helper demand. Inherit its services and
    /// immutable catalog data, populating a fresh ordinary root object. Only
    /// standalone registrations outside the catalog still use source replay.
    fn spawn_producer(
        &self,
        cx: &TaskContext,
        service: Option<UnitService>,
        bind: impl FnOnce(big_boy_pants_js::TaskId),
    ) -> Result<Self, EvalError> {
        // The requesting task is parked. Compiler registration and nested
        // macro expansion use independent mutable state on the same heap.
        let active = Self::in_runtime(self.shared.clone(), self.service_depth + 1)?;
        let producer_cx = active.cx()?;
        bind(producer_cx.task.id());
        *active.service.borrow_mut() = service;
        *active.string_services.borrow_mut() = self.string_services.borrow().clone();
        *active.help.borrow_mut() = self.help.borrow().clone();
        *active.runner.borrow_mut() = self.runner.borrow().clone();
        *active.submitter.borrow_mut() = self.submitter.borrow().clone();
        let started = crate::compile_trace::any_recording().then(std::time::Instant::now);
        let registrations = cx.registrations.borrow().clone();
        // Services and standalone registrations only. The compilation catalog
        // is imported below without parsing or evaluating registration source.
        for block in registrations.chunks(REPLAY_BLOCK) {
            let value = active.evaluate_unit(&replay_source(block))?;
            active.release(value)?;
        }
        *producer_cx.registrations.borrow_mut() = registrations;
        let replayed = producer_cx.registrations.borrow().len();
        if let Some(snapshot) = cx.catalog_snapshot.borrow().clone() {
            // Adopted, not applied: the task registers a name or protocol of
            // the snapshot when it first uses it (light-cljs-590).
            active.use_catalog(snapshot, 0, cx.catalog.get().unwrap_or(0), true)?;
        }
        producer_cx.catalog.set(cx.catalog.get());
        if let Some(started) = started {
            let c = &self.shared.0.counters;
            c.producers.fetch_add(1, Ordering::Relaxed);
            c.producer_replayed.fetch_add(replayed, Ordering::Relaxed);
            c.producer_nanos
                .fetch_add(started.elapsed().as_nanos() as usize, Ordering::Relaxed);
        }
        drop(producer_cx);
        Ok(active)
    }
    fn initialize(&self, cx: &TaskContext, begin: bool) -> Result<Rooted, EvalError> {
        let unit = cx
            .task
            .global_value("__lightInitializerUnit")
            .map_err(error)?
            .ok_or_else(|| error("missing initializer identity"))?;
        let ValueView::Number(unit) = unit.view() else {
            return Err(error("invalid initializer identity"));
        };
        let key = DependencyKey::new(
            self.runtime.world(),
            DependencyId::new(unit as u64),
            INITIALIZER,
        );
        if begin
            && cx
                .task
                .global_value("__lightInitializerSelect")
                .map_err(error)?
                .is_some_and(|v| matches!(v.view(), ValueView::Bool(true)))
        {
            let request = {
                let mut pending = self.shared.0.pending_initializers.lock().unwrap();
                if pending
                    .get(&key)
                    .is_some_and(|p| p.owner(self.shared.lanes(), key) == cx.task.id())
                {
                    pending.remove(&key).map(|p| p.request)
                } else {
                    // This task will consume the result instead of trying
                    // selection again. It is no longer an eligible owner.
                    if let Some(initializer) = pending.get_mut(&key) {
                        initializer
                            .callers
                            .retain(|(task, _)| *task != cx.task.id());
                    }
                    None
                }
            };
            cx.initializer_waits.borrow_mut().remove(&key);
            if let Some(request) = request {
                self.shared.0.graph.lock().unwrap().bind_producer(
                    key,
                    request.producer(),
                    cx.task.id(),
                );
                cx.initializers.borrow_mut().insert(key, request);
                return Ok(self.runtime.root(Primitive::Bool(true)));
            }
            cx.initializer_lanes.borrow_mut().remove(&key);
            return Ok(self.runtime.root(Primitive::Bool(false)));
        }
        if begin {
            let parent = cx
                .task
                .global_value("__lightInitializerParent")
                .map_err(error)?
                .and_then(|value| match value.view() {
                    ValueView::Number(n) => Some(n as u64),
                    _ => None,
                });
            let serial = cx.gensym_scope.get();
            let namespace = cx
                .task
                .global_value("__lightInitializerNamespace")
                .map_err(error)?
                .ok_or_else(|| error("missing initializer namespace"))?;
            let bound = cx
                .task
                .global_value("__lightInitializerNamespaceBound")
                .map_err(error)?
                .is_some_and(|v| matches!(v.view(), ValueView::Bool(true)));
            let position = self
                .shared
                .lanes()
                .and_then(|lanes| {
                    lanes.caller_position(crate::effect_lanes::identity(cx.task.id()))
                })
                .unwrap_or(0);
            let namespace_claim = {
                let mut bridge = self.bridge.lock().unwrap();
                if let Some(scope) = crate::generated_names::NativeGensyms::capture_scope(serial) {
                    bridge.caller_gensyms.insert(serial, scope);
                }
                bridge
                    .initializer_callers
                    .entry(unit as u64)
                    .or_default()
                    .entry(position)
                    .or_insert(serial);
                if let Some(parent) = parent {
                    bridge
                        .initializer_parents
                        .entry(unit as u64)
                        .or_default()
                        .insert(parent);
                }
                if bridge
                    .initializer_namespaces
                    .contains_key(&(unit as u64, serial))
                {
                    None
                } else {
                    bridge.next_namespace += 1;
                    let id = bridge.next_namespace;
                    let key =
                        DependencyKey::new(self.runtime.world(), DependencyId::new(id), NAMESPACE);
                    // Reserve before another task can observe the snapshot
                    // ID through the caller table and demand its cell.
                    let request = self.runtime.claim(key).map_err(error)?;
                    bridge
                        .initializer_namespaces
                        .insert((unit as u64, serial), (id, bound));
                    Some((key, request))
                }
            };
            if let Some((namespace_key, request)) = namespace_claim {
                let wakeups = self
                    .runtime
                    .complete(
                        namespace_key,
                        request.producer(),
                        Completion::Ready(&namespace),
                    )
                    .map_err(error)?;
                self.shared.wake(wakeups);
            }
            let caller = self
                .shared
                .lanes()
                .map(|lanes| lanes.caller())
                .unwrap_or_default();
            let candidate = {
                let mut pending = self.shared.0.pending_initializers.lock().unwrap();
                if let Some(initializer) = pending.get_mut(&key) {
                    initializer.callers.push((cx.task.id(), caller));
                    true
                } else if let Ok(request) = self.runtime.claim(key) {
                    pending.insert(
                        key,
                        PendingInitializer {
                            request,
                            callers: vec![(cx.task.id(), caller)],
                        },
                    );
                    true
                } else {
                    false
                }
            };
            if candidate {
                let wait = self.shared.lanes().map(|lanes| {
                    let target = crate::effect_lanes::identity(key);
                    let wait = lanes.wait_for(crate::effect_lanes::Target::Cell(target));
                    let entry = lanes.enter_cell(target);
                    cx.initializer_lanes.borrow_mut().insert(key, entry);
                    wait
                });
                cx.initializer_waits.borrow_mut().insert(key, wait);
            }
            #[cfg(test)]
            if candidate {
                let hook = self.shared.0.on_initializer.lock().unwrap().clone();
                if let Some(hook) = hook {
                    hook(&cx.task, key);
                }
            }
            return Ok(self.runtime.root(Primitive::Bool(candidate)));
        }
        let producer = cx
            .initializers
            .borrow()
            .get(&key)
            .ok_or_else(|| error("initializer completed without its producer claim"))?
            .producer();
        let failed = cx
            .task
            .global_value("__lightInitializerFailure")
            .map_err(error)?
            .ok_or_else(|| error("missing initializer outcome"))?;
        let value = cx
            .task
            .global_value("__lightInitializerResult")
            .map_err(error)?
            .ok_or_else(|| error("missing initializer result"))?;
        let failure = failed.string().is_some();
        let mut shared_value = false;
        let outcome = if failure {
            failed
        } else {
            let published = match self.runtime.publish(&value) {
                Ok(()) => true,
                Err(e @ big_boy_pants_js::PublishError::NotShareable { .. }) => {
                    // A multimethod's constructor and registration effects
                    // must never take the per-task initializer replay path.
                    // Its immutable native state must publish as one value.
                    if cx
                        .task
                        .global_value("__lightInitializerRequireShared")
                        .map_err(error)?
                        .is_some_and(|v| matches!(v.view(), ValueView::Bool(true)))
                    {
                        return Err(error(format!(
                            "unsupported operation: native multimethod captures task-owned state: {e}"
                        )));
                    }
                    false
                }
                Err(e) => return Err(error(e)),
            };
            shared_value = published;
            self.host_scope(|s| {
                let status = s.primitive(Primitive::Bool(published));
                let value = if published {
                    s.local(&value).map_err(error)?
                } else {
                    s.primitive(Primitive::Null)
                };
                Ok::<_, EvalError>(s.persist(s.new_vector(&[status, value]).map_err(error)?))
            })?
        };
        let completion = if failure {
            Completion::Failed(&outcome)
        } else {
            Completion::Ready(&outcome)
        };
        self.shared.0.graph.lock().unwrap().resolve(key);
        let wakeups = self
            .runtime
            .complete(key, producer, completion)
            .map_err(error)?;
        self.shared.wake(wakeups);
        cx.initializers.borrow_mut().remove(&key);
        let entry = cx.initializer_lanes.borrow_mut().remove(&key);
        drop(entry);
        if !failure {
            self.note(|s| {
                if shared_value {
                    s.helper_published += 1;
                } else {
                    s.helper_per_task += 1;
                }
            });
        }
        *self.shared.0.generation.lock().unwrap() += 1;
        self.shared.0.changed.notify_all();
        Ok(self.runtime.root(Primitive::Null))
    }
    fn fail_initializers(&self, cx: &TaskContext, message: &str) {
        let mut requests = std::mem::take(&mut *cx.initializers.borrow_mut());
        let waits = std::mem::take(&mut *cx.initializer_waits.borrow_mut());
        {
            let mut pending = self.shared.0.pending_initializers.lock().unwrap();
            for key in waits.keys() {
                if pending
                    .get(key)
                    .is_some_and(|p| p.owner(self.shared.lanes(), *key) == cx.task.id())
                {
                    requests.insert(*key, pending.remove(key).unwrap().request);
                } else if let Some(initializer) = pending.get_mut(key) {
                    initializer
                        .callers
                        .retain(|(task, _)| *task != cx.task.id());
                }
            }
        }
        drop(waits);
        let entries = std::mem::take(&mut *cx.initializer_lanes.borrow_mut());
        drop(entries);
        if requests.is_empty() {
            return;
        }
        let failure = self.failure_root(message);
        for (key, request) in requests {
            self.shared.0.graph.lock().unwrap().resolve(key);
            if let Ok(wakeups) =
                self.runtime
                    .complete(key, request.producer(), Completion::Failed(&failure))
            {
                self.shared.wake(wakeups);
            }
        }
        *self.shared.0.generation.lock().unwrap() += 1;
        self.shared.0.changed.notify_all();
    }
    /// A host scope on this evaluator's thread, outside any run. After a
    /// byte-limit denial it first collects: the denial's own collection ran
    /// before the denied macro released its data, and only a run's start
    /// collects again (`Runtime::byte_starved`). Inside a run or with
    /// another scope open the request fails at once and the scope proceeds.
    fn host_scope<R>(&self, f: impl for<'s> FnOnce(&Scope<'s>) -> R) -> R {
        if self.runtime.byte_starved() {
            let _ = self.runtime.request_collection();
        }
        self.runtime.scope(f)
    }
    /// The failure value a producer or initializer completes its waiters
    /// with. It is charged to the memory limit when it fits; at the limit
    /// the bounded diagnostic is rooted uncharged instead, because dropping
    /// it would leave the waiters without their outcome.
    fn failure_root(&self, message: &str) -> Rooted {
        self.runtime
            .try_root(Primitive::string(message))
            .unwrap_or_else(|| self.runtime.root(Primitive::string(message)))
    }
    fn id(&self) -> Result<ValueId, EvalError> {
        let cx = self.cx()?;
        let index = cx.next.get();
        cx.next.set(index + 1);
        Ok(ValueId {
            session: cx.session,
            index,
        })
    }
    fn check(&self, id: ValueId) -> Result<(), EvalError> {
        let cx = self.cx()?;
        if id.session != cx.session || !cx.values.borrow().contains_key(&id.index) {
            Err(error("invalid value handle"))
        } else {
            Ok(())
        }
    }
    /// One command to the task, as engine time of the compile trace: the
    /// Pants build's counterpart of `v8_evaluator::invoke`. A unit the
    /// command's JavaScript demands opens its own frame, which pauses it.
    fn request(&self, op: u32, id: ValueId, input: Rooted) -> Result<(), EvalError> {
        let traced = crate::compile_trace::engine_enter();
        let result = self.request_untraced(op, id, input);
        let service = match op {
            0 => "evaluate",
            1 => "adopt",
            2 => "call",
            3 => "release",
            _ => "useCatalog",
        };
        crate::compile_trace::engine_exit(traced, service);
        result
    }
    fn request_untraced(&self, op: u32, id: ValueId, input: Rooted) -> Result<(), EvalError> {
        self.control.check()?;
        let cx = self.cx()?;
        // Built before the boundary is taken: a denied message leaves the
        // task waiting at the same boundary for the next request.
        let message = self.host_scope(|s| {
            let op = s.primitive(Primitive::Number(op as f64));
            let id = s.primitive(Primitive::Number(id.index as f64));
            let message = s.new_vector(&[op, id, s.local(&input).map_err(error)?]);
            Ok::<_, EvalError>(s.persist(message.map_err(error)?))
        })?;
        let request = cx
            .task
            .take_producer_requests()
            .pop()
            .ok_or_else(|| error("macro task has no request boundary"))?;
        let _wakeups = self
            .runtime
            .complete(
                request.key(),
                request.producer(),
                // This command is read once. Keep its payload rooted here
                // through every helper wait until the task reaches its next
                // command boundary; completed mailboxes need not retain it.
                Completion::WeakReady(&message),
            )
            .map_err(error)?;
        self.run()?;
        let failed = cx
            .task
            .global_value("__lightFailure")
            .map_err(error)?
            .ok_or_else(|| error("missing result boundary"))?;
        if let Some(message) = failed.string() {
            return Err(error(message.to_string_lossy()));
        }
        // Only evaluate, adopt and call give a value a caller holds.
        if matches!(op, 0..=2) {
            let value = cx
                .task
                .global_value("__lightResult")
                .map_err(error)?
                .ok_or_else(|| error("missing macro result"))?;
            cx.values.borrow_mut().insert(id.index, value);
        }
        Ok(())
    }
    fn evaluate(&self, source: &str) -> Result<ValueId, EvalError> {
        let source = self.host_scope(|s| {
            s.try_primitive(Primitive::string(source))
                .map(|local| s.persist(local))
                .ok_or_else(|| error("heap byte limit exceeded"))
        })?;
        let id = self.id()?;
        self.request(0, id, source)?;
        Ok(id)
    }
    fn import(
        &self,
        data: &Datum,
        form: Option<&Form>,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        self.control.poll()?;
        let started = std::time::Instant::now();
        let traced = crate::compile_trace::engine_enter();
        let value = self.host_scope(|s| {
            let collected = self.runtime.heap_stats().collections;
            let v = self.with_bridge(|b| {
                b.prune(collected);
                if let Some(form) = form {
                    b.remember(form, limits.depth, &mut limits.nodes.clone())?;
                }
                b.import(s, data, limits.depth, &mut limits.nodes.clone())
            })?;
            Ok::<_, EvalError>(s.persist(v))
        });
        crate::compile_trace::engine_exit(traced, "importForm");
        let value = value?;
        self.note(|s| s.nanos += started.elapsed().as_nanos());
        let id = self.id()?;
        self.request(1, id, value)?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests;
struct RejectHost;
impl big_boy_pants_js::Host for RejectHost {
    fn effect(&mut self, call: big_boy_pants_js::HostCall) -> Result<Primitive, String> {
        Err(format!("unsupported operation: host effect {}", call.op))
    }
}
/// A bounded, owned VM step. It carries no analyzer or thread-local callback.
pub(crate) struct NativeStep {
    task: Task,
    per_task: Arc<AtomicUsize>,
    counters: Arc<NativeCounters>,
    lanes: Option<Arc<crate::effect_lanes::Lanes>>,
    last_thread: Arc<Mutex<std::thread::ThreadId>>,
    admitted: Arc<Mutex<bool>>,
    gensyms: crate::generated_names::NativeGensyms,
    bridge: Arc<Mutex<Bridge>>,
    namespaces: Arc<Mutex<namespaces::NamespaceLog>>,
    runtime: Runtime,
}
impl NativeStep {
    pub(crate) fn handoff(&self) -> NativeHandoff {
        NativeHandoff {
            task: self.task.clone(),
            admitted: self.admitted.clone(),
            finished: false,
        }
    }
    #[cfg(test)]
    pub(crate) fn task_id(&self) -> big_boy_pants_js::TaskId {
        self.task.id()
    }
    pub(crate) fn run(self) -> Result<RunOutcome, String> {
        // The per-step permit also lets an abandoned wait reclaim ownership.
        // It never excludes another task, and spans only this bounded VM step.
        let admitted = self.admitted.lock().unwrap();
        if !*admitted {
            return Err("native task handoff cancelled".into());
        }
        let current = std::thread::current().id();
        let previous = std::mem::replace(&mut *self.last_thread.lock().unwrap(), current);
        self.counters.steps.fetch_add(1, Ordering::Relaxed);
        if previous != current {
            self.counters.migrations.fetch_add(1, Ordering::Relaxed);
        }
        let outcome = self.task.run_with(
            Budget::instructions(10_000),
            &mut NativeHost(
                &self.per_task,
                &self.counters,
                self.lanes.as_deref(),
                &self.gensyms,
                &self.bridge,
                &self.namespaces,
                Some(&self.runtime),
            ),
        );
        // The settled realizations the step read count at the task's place
        // before a later activity reads the registry (light-cljs-601).
        // Drain even without lanes: standalone evaluators reuse a task,
        // and each fresh realization must not accumulate between commands.
        let consumed = self.runtime.take_lazy_consumed(self.task.id());
        if let Some(lanes) = &self.lanes {
            lanes.floor_consumed(crate::effect_lanes::identity(self.task.id()), &consumed);
        }
        outcome.map_err(|e| e.to_string())
    }
}
/// A waiting analyzer must regain exclusive ownership before unwinding its
/// saved task. A queued step is invalidated; an executing step finishes first.
pub(crate) struct NativeHandoff {
    task: Task,
    admitted: Arc<Mutex<bool>>,
    finished: bool,
}
impl NativeHandoff {
    pub(crate) fn finish(&mut self) {
        self.finished = true;
    }
}
impl Drop for NativeHandoff {
    fn drop(&mut self) {
        if !self.finished {
            *self.admitted.lock().unwrap() = false;
            let _cancel = self.task.cancel();
        }
    }
}
struct NativeHost<'a>(
    &'a AtomicUsize,
    &'a NativeCounters,
    Option<&'a crate::effect_lanes::Lanes>,
    &'a crate::generated_names::NativeGensyms,
    &'a Mutex<Bridge>,
    &'a Mutex<namespaces::NamespaceLog>,
    Option<&'a Runtime>,
);
impl big_boy_pants_js::Host for NativeHost<'_> {
    fn current_time_millis(&mut self) -> Result<f64, String> {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs_f64() * 1000.0)
            .map_err(|error| error.to_string())
    }
    fn effect(&mut self, call: big_boy_pants_js::HostCall) -> Result<Primitive, String> {
        if call.op == NAMESPACE_EFFECT {
            let Primitive::Number(unit) = call.arg else {
                return Err("invalid initializer namespace request".into());
            };
            let bridge = self.4.lock().unwrap();
            let id = bridge
                .initializer_namespace(unit as u64)
                .ok_or("initializer namespace unavailable")?;
            return Ok(Primitive::Number(id as f64));
        }
        if call.op == GENSYM_EFFECT {
            let Primitive::String(request) = call.arg else {
                return Err("invalid native gensym request".into());
            };
            let units = request.code_units();
            let Some((&op, argument)) = units.split_first() else {
                return Err("empty native gensym request".into());
            };
            return match op as u8 {
                b'g' => {
                    let serial = String::from_utf16_lossy(argument)
                        .parse()
                        .map_err(|_| "invalid gensym scope")?;
                    let number = self
                        .4
                        .lock()
                        .unwrap()
                        .caller_gensyms
                        .get(&serial)
                        .and_then(|scope| scope.next(serial))
                        .or_else(|| self.3.next(serial))
                        .ok_or("native gensym scope unavailable")?;
                    Ok(Primitive::Number(number as f64))
                }
                b'L' => {
                    self.3.log(argument);
                    Ok(Primitive::Null)
                }
                b's' => {
                    let request = String::from_utf16_lossy(argument);
                    let (unit, serial) = request.split_once(':').ok_or("invalid gensym caller")?;
                    let unit = unit.parse::<u64>().map_err(|_| "invalid initializer")?;
                    let serial = serial
                        .parse::<u64>()
                        .map_err(|_| "invalid gensym caller scope")?;
                    let bridge = self.4.lock().unwrap();
                    let selected = bridge.initializer_caller(unit).unwrap_or(serial);
                    Ok(Primitive::Number(selected as f64))
                }
                b'C' => {
                    let split = argument
                        .iter()
                        .position(|unit| *unit == b':' as u16)
                        .ok_or("invalid gensym capture")?;
                    let serial = String::from_utf16_lossy(&argument[..split])
                        .parse::<u64>()
                        .map_err(|_| "invalid gensym capture scope")?;
                    let bridge = self.4.lock().unwrap();
                    if let Some(scope) = bridge.caller_gensyms.get(&serial) {
                        scope.log(&argument[split + 1..]);
                    } else {
                        self.3.log(&argument[split + 1..]);
                    }
                    Ok(Primitive::Null)
                }
                b't' => Ok(Primitive::Number(
                    self.4.lock().unwrap().gensym_counter as f64,
                )),
                b'r' => {
                    let floor = String::from_utf16_lossy(argument)
                        .parse()
                        .map_err(|_| "invalid gensym floor")?;
                    let mut bridge = self.4.lock().unwrap();
                    bridge.gensym_counter = bridge.gensym_counter.max(floor);
                    Ok(Primitive::Null)
                }
                b'n' => {
                    let mut bridge = self.4.lock().unwrap();
                    bridge.gensym_counter = bridge
                        .gensym_counter
                        .checked_add(1)
                        .ok_or("gensym identities exhausted")?;
                    Ok(Primitive::Number(bridge.gensym_counter as f64))
                }
                b'a' => Ok(Primitive::Bool(
                    self.4.lock().unwrap().generated_names.insert(request),
                )),
                _ => Err("unknown native gensym request".into()),
            };
        }
        if call.op == NAMESPACES_EFFECT {
            let Primitive::String(request) = call.arg else {
                return Err("invalid namespace registry request".into());
            };
            let units = request.code_units();
            let Some((&op, name)) = units.split_first() else {
                return Err("empty namespace registry request".into());
            };
            let task = crate::effect_lanes::identity(call.task);
            let log = &self.5;
            let no_floors = HashMap::new();
            // The settled realizations this run read count at its place
            // before anything it does next (light-cljs-601).
            if let (Some(lanes), Some(runtime)) = (self.2, self.6) {
                lanes.floor_consumed(task, &runtime.take_lazy_consumed(call.task));
            }
            return match op as u8 {
                b'r' => {
                    let mut tag = self
                        .2
                        .map(|lanes| lanes.namespace_tag(task))
                        .unwrap_or_default();
                    // The shared lazy cells it realizes: work an earlier
                    // reader may wait for, or read once it has settled.
                    if let Some(runtime) = self.6 {
                        let realizing = runtime.attribute_realizing(call.task);
                        tag.owners
                            .extend(realizing.into_iter().map(crate::effect_lanes::Target::Lazy));
                    }
                    log.lock().unwrap().record(name, &tag);
                    Ok(Primitive::Null)
                }
                b'f' | b'v' => {
                    let view = self
                        .2
                        .map_or(crate::effect_lanes::NamespaceView::ALL, |lanes| {
                            lanes.namespace_view(task)
                        });
                    let log = log.lock().unwrap();
                    let read = |floors: &HashMap<_, _>| {
                        if op as u8 == b'f' {
                            return Ok(Primitive::Bool(log.has(name, &view, floors)));
                        }
                        let mut text: Vec<u16> = Vec::new();
                        for name in log.names(&view, floors) {
                            text.extend(name.len().to_string().encode_utf16());
                            text.push(u16::from(b':'));
                            text.extend_from_slice(name);
                        }
                        big_boy_pants_js::JsString::from_code_units(text)
                            .map(Primitive::String)
                            .ok_or_else(|| "namespace registry too large".to_string())
                    };
                    match self.2 {
                        Some(lanes) => lanes.namespace_floors(read),
                        None => read(&no_floors),
                    }
                }
                _ => Err("unknown namespace registry request".into()),
            };
        }
        if call.op == LANE_EFFECT {
            let task = crate::effect_lanes::identity(call.task);
            let argument = match call.arg {
                Primitive::Number(n) => n,
                _ => 0.0,
            };
            let answer = match self.2 {
                None => 0,
                Some(lanes) if argument < 0.0 => lanes.token(task),
                Some(lanes) => lanes.check(task, argument as u64),
            };
            return Ok(Primitive::Number(answer as f64));
        }
        #[cfg(test)]
        if call.op == 65534 {
            self.1.cleanups.fetch_add(1, Ordering::Relaxed);
            return Ok(Primitive::Null);
        }
        if call.op == 65535 {
            self.0.fetch_add(1, Ordering::Relaxed);
            self.1.per_task.fetch_add(1, Ordering::Relaxed);
            Ok(Primitive::Null)
        } else {
            Err(format!(
                "unsupported operation: native host effect {}",
                call.op
            ))
        }
    }
}
impl MacroEvaluator for BbpEvaluator {
    fn native_shell(&self) -> Option<&BbpEvaluator> {
        Some(self)
    }
    fn gensym_scope(
        &self,
        serial: u64,
        prefix: &str,
        retired: &str,
        native: bool,
    ) -> Result<(), EvalError> {
        self.cx()?.gensym_scope.set(serial);
        if serial != 0 && native {
            let key = DependencyKey::new(
                self.runtime.world(),
                DependencyId::new(serial),
                GENSYM_PREFIX,
            );
            if let Ok(request) = self.runtime.claim(key) {
                let prefix = self.host_scope(|s| {
                    let local = s
                        .try_string_units(&prefix.encode_utf16().collect::<Vec<_>>())
                        .ok_or_else(|| error("heap byte limit exceeded"))?;
                    Ok::<_, EvalError>(s.persist(local))
                })?;
                let wakeups = self
                    .runtime
                    .complete(key, request.producer(), Completion::Ready(&prefix))
                    .map_err(error)?;
                self.shared.wake(wakeups);
            }
        }
        debug_assert!(prefix.bytes().all(|b| b.is_ascii_digit() || b == b'_'));
        debug_assert!(retired.bytes().all(|b| b.is_ascii_digit() || b == b','));
        let value = self.evaluate_unit(&format!(
            "rt => rt.compilerForms.gensymScope({serial}, \"{prefix}\", \"{retired}\", {native})"
        ))?;
        self.release(value)
    }
    fn register_compiler_root(
        &self,
        name: &str,
        unit: Option<usize>,
        once: bool,
        location: &str,
        test: Option<usize>,
    ) -> Result<(), EvalError> {
        self.define_root(unit)?;
        let source = crate::evaluator::compiler_root_source(name, unit, once, location, test);
        let value = self.evaluate_unit(&source)?;
        self.release(value)?;
        self.cx()?.registrations.borrow_mut().push(source);
        Ok(())
    }
    /// One evaluation for all of `roots`, as a producer task replays them
    /// ([`replay_source`]): a full catalog block has the same text in every
    /// task, so its factory compiles once for the runtime.
    fn register_compiler_roots(
        &self,
        roots: &[crate::evaluator::CompilerRoot<'_>],
    ) -> Result<(), EvalError> {
        let mut sources = Vec::with_capacity(roots.len());
        for root in roots {
            self.define_root(root.unit)?;
            sources.push(crate::evaluator::compiler_root_source(
                root.name,
                root.unit,
                root.once,
                root.location,
                root.test,
            ));
        }
        let value = self.evaluate_unit(&replay_source(&sources))?;
        self.release(value)?;
        self.cx()?.registrations.borrow_mut().extend(sources);
        Ok(())
    }
    /// A declaration bundle's unit is a helper result like a root's: the
    /// runtime forces it with `__rt.need` when a product or an extension
    /// of a dispatched protocol is read (light-cljs-478). Nested producers
    /// replay the registration with the roots'.
    fn register_compiler_bundle(&self, source: &str, unit: usize) -> Result<(), EvalError> {
        self.define_root(Some(unit))?;
        let value = self.evaluate_unit(source)?;
        self.release(value)?;
        self.cx()?
            .registrations
            .borrow_mut()
            .push(source.to_owned());
        Ok(())
    }
    fn native_bridge_stats(&self) -> Option<BridgeStats> {
        Some(self.bridge_stats())
    }
    fn needs_reader_registration(&self) -> bool {
        false
    }
    fn register_namespaces(
        &self,
        namespaces: &crate::arena::Arena<String>,
    ) -> Result<(), EvalError> {
        self.record_namespaces(namespaces)
    }
    fn catalog_cursor(&self) -> Option<usize> {
        self.context.borrow().as_ref()?.catalog.get()
    }
    fn set_catalog_cursor(&self, end: usize) {
        if let Some(cx) = self.context.borrow().as_ref() {
            cx.catalog.set(Some(end));
        }
    }
    fn control(&self) -> ExecutionControl {
        self.control.clone()
    }
    fn heap(&self) -> usize {
        // The task's: a pool view's evaluator changes heaps as it borrows
        // contexts, which [`crate::generated_names`] is told of.
        self.context
            .borrow()
            .as_ref()
            .map_or(0, |cx| cx.session as usize)
    }
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {
        if source.len() >= SHARED_UNIT_BYTES
            && let Some(factory) = self.shared.factory(source)?
        {
            return self.call_factory(factory);
        }
        self.shared
            .0
            .counters
            .units_evaluated
            .fetch_add(1, Ordering::Relaxed);
        self.evaluate(&format!("({source})(globalThis.__lightNativeRuntime)"))
    }
    fn cached_source_macro_function(&self, identity: usize) -> Option<ValueId> {
        self.context
            .borrow()
            .as_ref()?
            .macro_functions
            .borrow()
            .get(&identity)
            .copied()
    }
    fn source_macro_function(&self, identity: usize, source: &str) -> Result<ValueId, EvalError> {
        let cx = self.cx()?;
        if let Some(&function) = cx.macro_functions.borrow().get(&identity) {
            return Ok(function);
        }
        let function = self.evaluate_unit(source)?;
        cx.macro_functions.borrow_mut().insert(identity, function);
        Ok(function)
    }
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError> {
        self.check(function)?;
        for a in arguments {
            self.check(*a)?;
        }
        let input = self.host_scope(|s| {
            let values = std::iter::once(function)
                .chain(arguments.iter().copied())
                .map(|v| s.primitive(Primitive::Number(v.index as f64)))
                .collect::<Vec<_>>();
            Ok::<_, EvalError>(s.persist(s.new_vector(&values).map_err(error)?))
        })?;
        let id = self.id()?;
        self.request(2, id, input)?;
        Ok(id)
    }
    fn import_form(&self, form: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.import(&form.data(), Some(form), limits)
    }
    fn import_data(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.import(data, None, limits)
    }
    fn import_environment(
        &self,
        data: &Datum,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        self.control.poll()?;
        let started = std::time::Instant::now();
        let traced = crate::compile_trace::engine_enter();
        let value = self.host_scope(|s| {
            let heap = self.runtime.heap_stats();
            let v = self.with_bridge(|b| {
                b.prune(heap.collections);
                let env_started =
                    crate::compile_trace::any_recording().then(std::time::Instant::now);
                let value = b.environment(s, data, limits);
                if let Some(env_started) = env_started {
                    let admitted = self.runtime.heap_stats().admitted_bytes_total;
                    b.counters.env_bytes_rebuilt +=
                        admitted.saturating_sub(heap.admitted_bytes_total);
                    b.counters.env_nanos += env_started.elapsed().as_nanos();
                }
                value
            })?;
            Ok::<_, EvalError>(s.persist(v))
        });
        crate::compile_trace::engine_exit(traced, "importEnvironment");
        let value = value?;
        self.note(|s| s.nanos += started.elapsed().as_nanos());
        let id = self.id()?;
        self.request(1, id, value)?;
        Ok(id)
    }
    fn export_form(
        &self,
        id: ValueId,
        call: &Form,
        origin: ExpansionOrigin,
        limits: ConversionLimits,
    ) -> Result<Form, EvalError> {
        self.check(id)?;
        let started = std::time::Instant::now();
        let v = self.cx()?.values.borrow().get(&id.index).unwrap().clone();
        let origins = scalar_origins(call);
        let traced = crate::compile_trace::engine_enter();
        let data = self.host_scope(|s| {
            self.with_bridge(|b| {
                b.export_with_origins(
                    s,
                    s.local(&v).map_err(error)?,
                    limits.depth,
                    &mut limits.nodes.clone(),
                    &origins,
                    None,
                )
            })
        });
        let result = data.map(|data| {
            self.with_bridge(|b| {
                let result = Form::from_bridge_expansion(data, call, origin, &b.forms);
                b.compiler_spans(result, &origins)
            })
        });
        crate::compile_trace::engine_exit(traced, "exportForm");
        let result = result?;
        self.note(|s| s.nanos += started.elapsed().as_nanos());
        Ok(result)
    }
    fn release(&self, id: ValueId) -> Result<(), EvalError> {
        self.check(id)?;
        self.request(3, id, self.runtime.root(Primitive::Null))?;
        self.cx()?.values.borrow_mut().remove(&id.index);
        Ok(())
    }
}
impl CompilerEvaluator for BbpEvaluator {
    fn export_data(&self, id: ValueId, limits: ConversionLimits) -> Result<Datum, EvalError> {
        self.check(id)?;
        let value = self.cx()?.values.borrow()[&id.index].clone();
        self.host_scope(|s| {
            self.with_bridge(|bridge| {
                bridge.export_with_origins(
                    s,
                    s.local(&value).map_err(error)?,
                    limits.depth,
                    &mut limits.nodes.clone(),
                    &HashMap::new(),
                    Some(ReaderExportPath {
                        path: "$",
                        literal: None,
                    }),
                )
            })
        })
    }
    fn install_wait_service(&self, callback: WaitService) {
        *self.help.borrow_mut() = Some(callback);
    }
    fn install_native_runner(&self, callback: NativeRunner) {
        *self.runner.borrow_mut() = Some(callback);
    }
    fn install_native_submitter(&self, callback: NativeSubmitter) {
        *self.submitter.borrow_mut() = Some(callback);
    }
    fn native_forms(&self) -> bool {
        true
    }
    fn evaluate_js(&self, expression: &str) -> Result<ValueId, EvalError> {
        self.evaluate(expression)
    }
    fn install_compiler_service(
        self: Rc<Self>,
        name: &str,
        callback: UnitService,
    ) -> Result<(), EvalError> {
        if name == "__lightCompileUnit" {
            *self.service.borrow_mut() = Some(callback);
        }
        // Helper demand calls the Rust service from a parked continuation.
        // Preserve explicit rejection of direct legacy JS service calls.
        self.unsupported_service(name)
    }
    fn install_string_service(&self, name: &str, callback: StringService) -> Result<(), EvalError> {
        if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(error("invalid service name"));
        }
        self.string_services
            .borrow_mut()
            .insert(name.to_owned(), callback);
        self.install_registration(format!(
            "rt => {{ globalThis.{name} = request => globalThis.__lightCallStringService('{name}', request); return null; }}"
        ))
    }
    fn install_build_object(&self, caller: &str) -> Result<(), EvalError> {
        self.install_registration(format!(
            "rt => {{ globalThis.lightCljsBuild = ({})(globalThis.__lightBuildService, {caller}); return null; }}",
            crate::build_services::BUILD_OBJECT
        ))
    }
    fn value_roots(&self) -> Result<usize, EvalError> {
        // The task's own runtime handle is not a value its callers hold.
        let held = self.context.borrow().as_ref().map_or(0, |cx| {
            cx.values.borrow().len() - usize::from(cx.native_runtime.get().is_some())
        });
        Ok(held + usize::from(self.bridge.lock().unwrap().environment.is_some()))
    }
    fn collect_garbage(&self) {
        let _ = self.runtime.collect();
    }
    #[cfg(all(test, light_v8))]
    fn number(&self, id: ValueId) -> Result<f64, EvalError> {
        self.check(id)?;
        match self.cx()?.values.borrow()[&id.index].view() {
            ValueView::Number(n) => Ok(n),
            _ => Err(error("not a number")),
        }
    }
}
impl BbpEvaluator {
    /// Record root `unit`'s helper definition and complete its binding cell.
    fn define_root(&self, unit: Option<usize>) -> Result<(), EvalError> {
        let Some(unit) = unit else {
            return Ok(());
        };
        let id = DependencyId::new(unit as u64);
        let key = DependencyKey::new(self.runtime.world(), id, HELPER);
        self.shared
            .0
            .definitions
            .lock()
            .unwrap()
            .insert(key, unit as u64);
        let binding = DependencyKey::new(self.runtime.world(), id, BINDING);
        if let Ok(request) = self.runtime.claim(binding) {
            let value = self.runtime.root(Primitive::Null);
            let wakeups = self
                .runtime
                .complete(binding, request.producer(), Completion::Ready(&value))
                .map_err(error)?;
            self.shared.wake(wakeups);
        }
        Ok(())
    }
    /// Evaluate a unit through its shared factory: the factory enters this
    /// task as a value and is called with this task's runtime, as the
    /// evaluated source would have called it.
    fn call_factory(&self, factory: Rooted) -> Result<ValueId, EvalError> {
        let cx = self.cx()?;
        let runtime = match cx.native_runtime.get() {
            Some(id) => id,
            None => {
                let id = self.evaluate("globalThis.__lightNativeRuntime")?;
                cx.native_runtime.set(Some(id));
                id
            }
        };
        drop(cx);
        let function = self.id()?;
        self.request(1, function, factory)?;
        let result = self.call(function, &[runtime]);
        let released = self.release(function);
        let result = result?;
        released?;
        self.shared
            .0
            .counters
            .units_shared
            .fetch_add(1, Ordering::Relaxed);
        Ok(result)
    }
    fn unsupported_service(&self, name: &str) -> Result<(), EvalError> {
        if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(error("invalid service name"));
        }
        let source = format!(
            "rt => {{ globalThis.{name} = () => {{ throw new Error('unsupported operation: native compiler service {name}'); }}; return null; }}"
        );
        self.install_registration(source)
    }
    fn install_registration(&self, source: String) -> Result<(), EvalError> {
        // A borrowed context has its services from the view that
        // prepared it ([`TaskContext::prepared`]).
        let cx = self.cx()?;
        if cx.prepared.get() && cx.registrations.borrow().contains(&source) {
            return Ok(());
        }
        drop(cx);
        let value = self.evaluate_unit(&source)?;
        self.release(value)?;
        self.cx()?.registrations.borrow_mut().push(source);
        Ok(())
    }
}
