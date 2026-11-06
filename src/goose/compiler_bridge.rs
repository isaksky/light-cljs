//! GFB1 compiler-owned form transport. No engine pointers or printed forms.
use crate::CollectionKind;
use crate::compiler_data::{Datum, DatumKind, ExpansionOrigin, Form, Name};
use crate::evaluator::{EvalError, ExecutionControl};
use crate::form_bridge::ConversionLimits;
use std::{cell::Cell, collections::HashMap};

pub(super) struct Budget {
    remaining: Cell<usize>,
    bytes: Cell<usize>,
    pending: Cell<u64>,
    control: ExecutionControl,
    /// Whether checks and charges count; a budget that does not only
    /// rebuilds a graph whose conversion was already charged.
    metered: bool,
}
// No fuel is reserved. At most 127 logical checks per active Budget can be
// unpaid between checkpoints; all are paid before VM use/publication. Thus
// concurrent/nested conversions cannot strand credits or exhaust on somebody
// else's unused reservation. A checkpoint observes cancellation/deadline every
// 128 node/work/scalar checks, in addition to all transport boundaries. Existing
// node/depth/64MiB byte-work limits still bound the individual operations.
const POLL_BATCH: u64 = 128;
impl Budget {
    pub fn new(nodes: usize, control: ExecutionControl) -> Self {
        Self {
            remaining: Cell::new(nodes),
            bytes: Cell::new(MAX_BYTES * 4),
            pending: Cell::new(0),
            control,
            metered: true,
        }
    }
    /// A budget that charges nothing, for rebuilding a graph whose
    /// conversion was charged in full ([`Graph::arguments_omitting`]).
    fn unmetered(control: ExecutionControl) -> Self {
        Self {
            remaining: Cell::new(usize::MAX),
            bytes: Cell::new(usize::MAX),
            pending: Cell::new(0),
            control,
            metered: false,
        }
    }
    fn poll(&self) -> Result<(), EvalError> {
        if !self.metered {
            return Ok(());
        }
        let pending = self.pending.get();
        if pending == 0 {
            self.control.poll_many(0)?;
        }
        self.pending.set(pending + 1);
        if pending + 1 == POLL_BATCH {
            self.finish()?;
        }
        Ok(())
    }
    pub fn finish(&self) -> Result<(), EvalError> {
        self.control.poll_many(self.pending.replace(0))
    }
    pub fn settle<T>(&self, result: Result<T, EvalError>) -> Result<T, EvalError> {
        self.finish().and(result)
    }
    fn check(&self) -> Result<(), EvalError> {
        self.poll()?;
        let Some(next) = self.remaining.get().checked_sub(1) else {
            return Err(EvalError("Goose conversion limit exceeded".into()));
        };
        self.remaining.set(next);
        Ok(())
    }
    /// Charge `checks` node checks, `polls` polls and `bytes` bytes at
    /// once, as that many [`Self::check`]s and byte charges would, if they
    /// all fit; `false`, charging nothing, if one would fail.
    fn debit(&self, checks: usize, polls: usize, bytes: usize) -> Result<bool, EvalError> {
        if self.remaining.get() < checks || self.bytes.get() < bytes {
            return Ok(false);
        }
        self.control.poll_many(0)?;
        self.pending.set(self.pending.get() + polls as u64);
        self.finish()?;
        self.remaining.set(self.remaining.get() - checks);
        self.bytes.set(self.bytes.get() - bytes);
        Ok(true)
    }
    fn bytes(&self, count: usize) -> Result<(), EvalError> {
        self.poll()?;
        let Some(left) = self.bytes.get().checked_sub(count) else {
            return Err(EvalError(
                "Goose conversion byte/work limit exceeded".into(),
            ));
        };
        self.bytes.set(left);
        Ok(())
    }
}
impl Drop for Budget {
    fn drop(&mut self) {
        // Error exits may abandon a partial graph. Charge the actual tail too;
        // a failed debit leaves the shared terminal state sticky.
        let _ = self.finish();
    }
}

pub(super) const MAX_BYTES: usize = 16 * 1024 * 1024;
/// Goose's default per-job graph nodes, value rows, text units and bridge
/// work (form_bridge.goose `Control`, source_vm.goose `Limits`).
const GOOSE_NODES: usize = 100_000;
const GOOSE_VALUES: usize = 1_048_576;
const GOOSE_UNITS: usize = 1_048_576;
const GOOSE_WORK: usize = 10_000_000;
fn bad() -> EvalError {
    EvalError("Goose malformed or unsupported form graph".into())
}
#[derive(Clone)]
struct Node {
    tag: u32,
    number: f64,
    fields: [i32; 9],
}
impl Default for Node {
    fn default() -> Self {
        Self {
            tag: 0,
            number: 0.,
            fields: [0, 0, 0, -1, -1, -1, -1, 0, 0],
        }
    }
}
#[derive(Default)]
pub(super) struct Graph {
    nodes: Vec<Node>,
    children: Vec<u32>,
    units: Vec<u16>,
    sources: Vec<(u32, u32)>,
    bytes: Vec<u8>,
    roots: Vec<u32>,
    pub originals: HashMap<i32, Input>,
    memo: HashMap<(usize, usize), u32>,
    source_ids: HashMap<crate::compiler_data::SourceId, usize>,
    /// The nodes and encoded bytes the environment entries a narrowed
    /// environment leaves out would have added ([`Self::omit`]), which the
    /// graph's limits still count.
    omitted_nodes: usize,
    omitted_size: usize,
    /// The byte charges of the omitted entries, their bytes, and the
    /// deepest depth an omission reached, which [`Omitted`] summarizes.
    omitted_charges: usize,
    omitted_bytes: usize,
    omitted_children: usize,
    omitted_units: usize,
    omitted_depth: usize,
}
/// What omitting one collection cost when an exact walk ([`Graph::omit`])
/// passed: its budget checks, polls and bytes, its nodes, encoded size and
/// height, which depend on its data alone.
#[derive(Clone, Copy)]
pub(super) struct Omitted {
    checks: usize,
    polls: usize,
    bytes: usize,
    nodes: usize,
    children: usize,
    units: usize,
    size: usize,
    height: usize,
}
/// The [`Omitted`] costs of the collections a session's narrowed
/// environments left out most recently, by identity, most recently used
/// last, with the collection kept so that its identity is not reused: the
/// macro inventory is the same collection call after call.
pub(super) type OmitCache = std::cell::RefCell<Vec<(Datum, Omitted)>>;
/// How many collections an [`OmitCache`] keeps.
const OMIT_CACHE: usize = 8;
#[derive(Clone)]
pub(super) struct Input {
    data: Datum,
    form: Option<Form>,
}
fn compatible(
    a: &Datum,
    b: &Datum,
    c: &Budget,
    depth: usize,
    limits: ConversionLimits,
) -> Result<bool, EvalError> {
    c.check()?;
    if depth > limits.depth {
        return Err(EvalError("Goose conversion limit exceeded".into()));
    }
    use DatumKind::*;
    let same = match (a.kind(), b.kind()) {
        (Integer(a), Integer(b)) => a == b,
        (Double(a), Double(b)) => a.to_bits() == b.to_bits(),
        (Integer(a), Double(b)) | (Double(b), Integer(a)) => (*a as f64).to_bits() == b.to_bits(),
        (Nil, Nil) => true,
        (Boolean(a), Boolean(b)) => a == b,
        (String(a), String(b)) => {
            c.bytes((a.len() + b.len()) * 2)?;
            a == b
        }
        (Symbol(a), Symbol(b)) | (Keyword(a), Keyword(b)) => a == b,
        (
            Collection {
                kind: a,
                children: ac,
            },
            Collection {
                kind: b,
                children: bc,
            },
        ) => {
            if a != b || ac.len() != bc.len() {
                false
            } else {
                let mut same = true;
                for (a, b) in ac.iter().zip(bc) {
                    if !compatible(a, b, c, depth + 1, limits)? {
                        same = false;
                        break;
                    }
                }
                same
            }
        }
        _ => false,
    };
    if !same {
        return Ok(false);
    }
    match (a.metadata(), b.metadata()) {
        (None, None) => Ok(true),
        (Some(a), Some(b)) => compatible(&a, &b, c, depth + 1, limits),
        _ => Ok(false),
    }
}
fn tag(d: &Datum) -> Result<u32, EvalError> {
    Ok(match d.kind() {
        DatumKind::Nil => 0,
        DatumKind::Boolean(_) => 1,
        DatumKind::Integer(_) | DatumKind::Double(_) => 2,
        DatumKind::String(_) => 3,
        DatumKind::Symbol(_) => 4,
        DatumKind::Keyword(_) => 5,
        DatumKind::Collection { kind, .. } => match kind {
            CollectionKind::List => 6,
            CollectionKind::Vector => 7,
            CollectionKind::Map => 8,
            CollectionKind::Set => 9,
            _ => return Err(bad()),
        },
        _ => return Err(bad()),
    })
}
impl Graph {
    /// The graph's nodes, for the session's transport counters.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    /// The bytes of source text the graph carries, which its encoding
    /// includes, for the session's transport counters.
    pub fn source_bytes(&self) -> usize {
        self.bytes.len()
    }
    fn charge(
        &self,
        depth: usize,
        limits: ConversionLimits,
        control: &Budget,
    ) -> Result<(), EvalError> {
        control.check()?;
        if depth > limits.depth
            || self.nodes.len() + self.omitted_nodes >= limits.nodes
            || self.size() > MAX_BYTES
        {
            return Err(EvalError("Goose conversion limit exceeded".into()));
        }
        Ok(())
    }
    /// The encoding's size, and what the entries a narrowed environment
    /// leaves out would have added to it.
    fn size(&self) -> usize {
        self.encoded_size() + self.omitted_size
    }
    fn encoded_size(&self) -> usize {
        32 + self.nodes.len() * 48
            + self.children.len() * 4
            + self.units.len() * 2
            + self.sources.len() * 8
            + self.bytes.len()
            + self.roots.len() * 4
    }
    fn text(&mut self, text: &[u16]) -> (i32, i32) {
        let start = self.units.len() as i32;
        self.units.extend(text);
        (start, text.len() as i32)
    }
    fn datum(
        &mut self,
        d: &Datum,
        depth: usize,
        limits: ConversionLimits,
        c: &Budget,
    ) -> Result<u32, EvalError> {
        self.charge(depth, limits, c)?;
        let mut n = Node {
            tag: tag(d)?,
            ..Default::default()
        };
        match d.kind() {
            DatumKind::Boolean(v) => n.number = if *v { 1. } else { 0. },
            DatumKind::Integer(v) => {
                if *v as f64 >= i64::MAX as f64 || (*v as f64) as i64 != *v {
                    return Err(bad());
                }
                n.number = *v as f64;
            }
            DatumKind::Double(v) => n.number = *v,
            DatumKind::String(v) => {
                c.bytes(v.len() * 2)?;
                let (s, l) = self.text(v);
                n.fields[0] = s;
                n.fields[1] = l;
            }
            DatumKind::Symbol(v) | DatumKind::Keyword(v) => {
                c.bytes((v.name().len() + v.namespace().map_or(0, <[u16]>::len)) * 2)?;
                let (s, l) = self.text(v.name());
                n.fields[0] = s;
                n.fields[1] = l;
                if let Some(ns) = v.namespace() {
                    let (s, l) = self.text(ns);
                    n.fields[2] = s;
                    n.fields[3] = l;
                }
            }
            DatumKind::Collection { children, .. } => {
                let ids = children
                    .iter()
                    .map(|d| self.datum(d, depth + 1, limits, c))
                    .collect::<Result<Vec<_>, _>>()?;
                n.fields[0] = self.children.len() as i32;
                n.fields[1] = ids.len() as i32;
                self.children.extend(ids);
            }
            _ => {}
        }
        if let Some(meta) = d.metadata() {
            n.fields[4] = self.datum(&meta, depth + 1, limits, c)? as i32;
        }
        self.charge(depth, limits, c)?;
        let occurrence = self.originals.len() as i32;
        self.originals.insert(
            occurrence,
            Input {
                data: d.clone(),
                form: None,
            },
        );
        n.fields[5] = occurrence;
        let id = self.nodes.len() as u32;
        self.nodes.push(n);
        Ok(id)
    }
    /// What [`Self::datum`] does for `d`, without the node: the same
    /// budget checks, byte charges and refusals in the same order, and the
    /// nodes and bytes it would add counted against the graph's limits, so
    /// that leaving an entry out of a narrowed environment neither hides a
    /// conversion failure nor frees budget for the rest of the call
    /// (light-cljs-815).
    fn omit(
        &mut self,
        d: &Datum,
        depth: usize,
        limits: ConversionLimits,
        c: &Budget,
    ) -> Result<(), EvalError> {
        self.omitted_depth = self.omitted_depth.max(depth);
        self.charge(depth, limits, c)?;
        tag(d)?;
        let mut units = 0;
        match d.kind() {
            DatumKind::Integer(v) => {
                if *v as f64 >= i64::MAX as f64 || (*v as f64) as i64 != *v {
                    return Err(bad());
                }
            }
            DatumKind::String(v) => {
                c.bytes(v.len() * 2)?;
                units = v.len();
                self.omitted_charges += 1;
            }
            DatumKind::Symbol(v) | DatumKind::Keyword(v) => {
                units = v.name().len() + v.namespace().map_or(0, <[u16]>::len);
                c.bytes(units * 2)?;
                self.omitted_charges += 1;
            }
            DatumKind::Collection { children, .. } => {
                for child in children.iter() {
                    self.omit(child, depth + 1, limits, c)?;
                }
                self.omitted_size += children.len() * 4;
                self.omitted_children += children.len();
            }
            _ => {}
        }
        self.omitted_size += units * 2;
        self.omitted_bytes += units * 2;
        self.omitted_units += units;
        if let Some(meta) = d.metadata() {
            self.omit(&meta, depth + 1, limits, c)?;
        }
        self.charge(depth, limits, c)?;
        self.omitted_nodes += 1;
        self.omitted_size += 48;
        Ok(())
    }
    /// [`Self::omit`] for an entry of a narrowed environment: a collection
    /// whose cost `cache` knows is charged at once when it fits what is
    /// left of every limit, as each of its checks would have passed, and
    /// otherwise walked, so that it fails where and as the walk fails.
    fn omit_entry(
        &mut self,
        d: &Datum,
        depth: usize,
        limits: ConversionLimits,
        c: &Budget,
        cache: &OmitCache,
    ) -> Result<(), EvalError> {
        if !matches!(d.kind(), DatumKind::Collection { .. }) {
            return self.omit(d, depth, limits, c);
        }
        let known = {
            let mut cache = cache.borrow_mut();
            let found = cache.iter().position(|(e, _)| e.identity() == d.identity());
            found.map(|i| {
                let entry = cache.remove(i);
                let cost = entry.1;
                cache.push(entry);
                cost
            })
        };
        if let Some(o) = known
            && depth + o.height <= limits.depth
            && self.nodes.len() + self.omitted_nodes + o.nodes < limits.nodes
            && self.size() + o.size <= MAX_BYTES
            && c.debit(o.checks, o.polls, o.bytes)?
        {
            self.omitted_nodes += o.nodes;
            self.omitted_size += o.size;
            self.omitted_children += o.children;
            self.omitted_units += o.units;
            return Ok(());
        }
        let before = (
            self.omitted_nodes,
            self.omitted_size,
            self.omitted_charges,
            self.omitted_bytes,
            std::mem::replace(&mut self.omitted_depth, depth),
            self.omitted_children,
            self.omitted_units,
        );
        self.omit(d, depth, limits, c)?;
        let nodes = self.omitted_nodes - before.0;
        let charges = self.omitted_charges - before.2;
        let cost = Omitted {
            checks: 2 * nodes,
            polls: 2 * nodes + charges,
            bytes: self.omitted_bytes - before.3,
            nodes,
            children: self.omitted_children - before.5,
            units: self.omitted_units - before.6,
            size: self.omitted_size - before.1,
            height: self.omitted_depth - depth + 1,
        };
        self.omitted_depth = self.omitted_depth.max(before.4);
        if known.is_none() {
            let mut cache = cache.borrow_mut();
            if cache.len() == OMIT_CACHE {
                cache.remove(0);
            }
            cache.push((d.clone(), cost));
        }
        Ok(())
    }
    /// The map `view` that [`crate::compiler_bootstrap`]'s reader view
    /// narrows the environment `whole` to: converted as [`Self::datum`]
    /// converts `whole`, entry by entry in its order, except that an entry
    /// whose key `view` lacks, and the metadata, are [`Self::omit`]ted.
    fn environment(
        &mut self,
        view: &Datum,
        whole: &Datum,
        depth: usize,
        limits: ConversionLimits,
        c: &Budget,
        cache: &OmitCache,
    ) -> Result<u32, EvalError> {
        let (
            DatumKind::Collection {
                kind: CollectionKind::Map,
                children: kept,
                ..
            },
            DatumKind::Collection {
                kind: CollectionKind::Map,
                children,
                ..
            },
        ) = (view.kind(), whole.kind())
        else {
            return self.datum(whole, depth, limits, c);
        };
        self.charge(depth, limits, c)?;
        let mut ids = Vec::with_capacity(kept.len());
        for entry in children.chunks(2) {
            let keep = kept
                .iter()
                .step_by(2)
                .any(|key| crate::form_bridge::exact(key, &entry[0]));
            for d in entry {
                if keep {
                    ids.push(self.datum(d, depth + 1, limits, c)?);
                } else {
                    self.omit_entry(d, depth + 1, limits, c, cache)?;
                }
            }
        }
        self.omitted_size += (children.len() - ids.len()) * 4;
        self.omitted_children += children.len() - ids.len();
        if let Some(meta) = whole.metadata() {
            self.omit(&meta, depth + 1, limits, c)?;
        }
        let mut n = Node {
            tag: 8,
            ..Default::default()
        };
        n.fields[0] = self.children.len() as i32;
        n.fields[1] = ids.len() as i32;
        self.children.extend(ids);
        self.charge(depth, limits, c)?;
        let occurrence = self.originals.len() as i32;
        self.originals.insert(
            occurrence,
            Input {
                data: view.clone(),
                form: None,
            },
        );
        n.fields[5] = occurrence;
        let id = self.nodes.len() as u32;
        self.nodes.push(n);
        Ok(id)
    }
    fn form(
        &mut self,
        f: &Form,
        depth: usize,
        l: ConversionLimits,
        c: &Budget,
    ) -> Result<u32, EvalError> {
        let key = (f.nodes().as_ptr() as usize, f.id().0);
        if let Some(id) = self.memo.get(&key) {
            return Ok(*id);
        }
        self.charge(depth, l, c)?;
        if f.node().compiler_payload.is_some() {
            return Err(bad());
        }
        let d = f.data();
        let mut n = Node {
            tag: tag(&d)?,
            ..Default::default()
        };
        if n.tag >= 6 {
            let ids = f
                .children()
                .iter()
                .map(|f| self.form(f, depth + 1, l, c))
                .collect::<Result<Vec<_>, _>>()?;
            n.fields[0] = self.children.len() as i32;
            n.fields[1] = ids.len() as i32;
            self.children.extend(ids);
        } else {
            let id = self.datum(&d, depth, l, c)?;
            n = self.nodes[id as usize].clone();
            self.nodes.pop();
        }
        if let Some(meta) = f.metadata() {
            n.fields[4] = self.form(&meta, depth + 1, l, c)? as i32;
        }
        let span = f.span();
        let source = span.source();
        let source_index = if let Some(i) = self.source_ids.get(&source.id()) {
            *i
        } else {
            c.bytes(source.text().len())?;
            let i = self.sources.len();
            self.sources
                .push((self.bytes.len() as u32, source.text().len() as u32));
            self.bytes.extend(source.text().as_bytes());
            self.source_ids.insert(source.id(), i);
            i
        };
        let occurrence = self.originals.len() as i32;
        self.originals.insert(
            occurrence,
            Input {
                data: f.data(),
                form: Some(f.clone()),
            },
        );
        n.fields[5] = occurrence;
        n.fields[6] = source_index as i32;
        n.fields[7] = span.bytes().start as i32;
        n.fields[8] = span.bytes().end as i32;
        self.charge(depth, l, c)?;
        let id = self.nodes.len() as u32;
        self.nodes.push(n);
        self.memo.insert(key, id);
        Ok(id)
    }
    /// [`Self::arguments_omitting`] without a cache, for fixtures.
    #[cfg(test)]
    pub fn arguments(
        forms: &[super::Stored],
        limits: ConversionLimits,
        c: &Budget,
    ) -> Result<Self, EvalError> {
        Self::arguments_omitting(forms, limits, c, &OmitCache::default())
    }
    /// [`Self::arguments`], with the omission costs `cache` knows.
    pub fn arguments_omitting(
        forms: &[super::Stored],
        limits: ConversionLimits,
        c: &Budget,
        cache: &OmitCache,
    ) -> Result<Self, EvalError> {
        let mut g = Self::default();
        for form in forms {
            let id = match form {
                super::Stored::Form(f, _) => g.form(f, 1, limits, c)?,
                super::Stored::Data(d, _) => g.datum(d, 1, limits, c)?,
                super::Stored::Environment(view, whole, _) => {
                    g.environment(view, whole, 1, limits, c, cache)?
                }
                _ => return Err(bad()),
            };
            g.roots.push(id);
        }
        // Goose checks its own capacities when it imports the graph, against
        // a job heap that may already hold the macro's definitions: near
        // them, Goose is sent the whole environment, so that it fails where
        // it failed. This graph charged what the whole one charges.
        if g.omitted_nodes > 0 && g.near_goose_capacity() {
            let whole = forms
                .iter()
                .map(|form| match form {
                    super::Stored::Environment(_, whole, l) => {
                        super::Stored::Data(whole.clone(), *l)
                    }
                    form => form.clone(),
                })
                .collect::<Vec<_>>();
            let unmetered = Budget::unmetered(c.control.clone());
            return Self::arguments_omitting(&whole, limits, &unmetered, cache);
        }
        Ok(g)
    }
    /// Whether the graph, with the entries a narrowed environment left
    /// out, would take more than half of a capacity Goose checks when it
    /// decodes and imports a job's arguments: its nodes, child values and
    /// text units, its work and its bytes (`Control` in form_bridge.goose,
    /// `Limits` in source_vm.goose).
    fn near_goose_capacity(&self) -> bool {
        let nodes = self.nodes.len() + self.omitted_nodes;
        let children = self.children.len() + self.omitted_children;
        let units = self.units.len() + self.omitted_units;
        let work =
            nodes + children + units + self.bytes.len() + self.sources.len() + self.roots.len();
        2 * nodes > GOOSE_NODES
            || 2 * children > GOOSE_VALUES
            || 2 * units > GOOSE_UNITS
            || 2 * work > GOOSE_WORK
            || 2 * self.size() > MAX_BYTES
    }
    pub fn encode(&self) -> Result<Vec<u8>, EvalError> {
        if self.size() > MAX_BYTES {
            return Err(bad());
        }
        let mut out = Vec::with_capacity(self.encoded_size());
        out.extend(b"GFB1");
        out.extend(1u32.to_le_bytes());
        for n in [
            self.nodes.len(),
            self.children.len(),
            self.units.len(),
            self.sources.len(),
            self.bytes.len(),
            self.roots.len(),
        ] {
            out.extend((n as u32).to_le_bytes());
        }
        for n in &self.nodes {
            out.extend(n.tag.to_le_bytes());
            out.extend(n.number.to_le_bytes());
            for f in n.fields {
                out.extend(f.to_le_bytes());
            }
        }
        for id in &self.children {
            out.extend(id.to_le_bytes());
        }
        for u in &self.units {
            out.extend(u.to_le_bytes());
        }
        for (s, n) in &self.sources {
            out.extend(s.to_le_bytes());
            out.extend(n.to_le_bytes());
        }
        out.extend(&self.bytes);
        for id in &self.roots {
            out.extend(id.to_le_bytes());
        }
        Ok(out)
    }
    pub fn decode(bytes: &[u8], limits: ConversionLimits, c: &Budget) -> Result<Self, EvalError> {
        c.check()?;
        if bytes.len() < 32 || bytes.len() > MAX_BYTES || &bytes[..4] != b"GFB1" {
            return Err(bad());
        }
        let mut at = 4;
        let word = |at: &mut usize| {
            let n = u32::from_le_bytes(bytes[*at..*at + 4].try_into().unwrap());
            *at += 4;
            n
        };
        if word(&mut at) != 1 {
            return Err(bad());
        }
        let counts: [usize; 6] = std::array::from_fn(|_| word(&mut at) as usize);
        let [nodes, children, units, sources, text, roots] = counts;
        if nodes > limits.nodes
            || roots != 1
            || 32u64
                + nodes as u64 * 48
                + children as u64 * 4
                + units as u64 * 2
                + sources as u64 * 8
                + text as u64
                + roots as u64 * 4
                != bytes.len() as u64
        {
            return Err(bad());
        }
        let mut g = Self::default();
        for _ in 0..nodes {
            c.check()?;
            let tag = word(&mut at);
            let number = f64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
            at += 8;
            let fields = std::array::from_fn(|_| word(&mut at) as i32);
            g.nodes.push(Node {
                tag,
                number,
                fields,
            });
        }
        for _ in 0..children {
            g.children.push(word(&mut at));
        }
        for _ in 0..units {
            g.units
                .push(u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()));
            at += 2;
        }
        for _ in 0..sources {
            g.sources.push((word(&mut at), word(&mut at)));
        }
        g.bytes.extend(&bytes[at..at + text]);
        at += text;
        for _ in 0..roots {
            g.roots.push(word(&mut at));
        }
        // Validate every edge, including unreachable nodes, before recursive conversion.
        for n in &g.nodes {
            if n.tag > 9
                || n.fields[4] < -1
                || n.fields[4] >= nodes as i32
                || n.fields[6] < -1
                || n.fields[6] >= sources as i32
                || n.fields[5] < -1
                || (n.tag == 4 || n.tag == 5) && n.fields[3] < -1
                || n.tag == 1 && n.number != 0. && n.number != 1.
            {
                return Err(bad());
            }
            if n.tag >= 6 {
                g.segment(&g.children, n.fields[0], n.fields[1])?;
            } else if n.tag >= 3 {
                g.segment(&g.units, n.fields[0], n.fields[1])?;
                if n.tag >= 4 && n.fields[3] != -1 {
                    g.segment(&g.units, n.fields[2], n.fields[3])?;
                }
            }
            if n.tag == 8 && n.fields[1] % 2 != 0 {
                return Err(bad());
            }
            if n.fields[4] >= 0 && g.nodes[n.fields[4] as usize].tag != 8 {
                return Err(bad());
            }
            if n.fields[6] >= 0 {
                let (start, len) = g.sources[n.fields[6] as usize];
                if n.fields[7] < 0 || n.fields[8] < n.fields[7] || n.fields[8] as u32 > len {
                    return Err(bad());
                }
                let text = g.segment(&g.bytes, start as i32, len as i32)?;
                // Sources are UTF-8 validated once below. Checking only the
                // boundary bytes here avoids rescanning a source per node.
                if [n.fields[7], n.fields[8]].iter().any(|&offset| {
                    text.get(offset as usize)
                        .is_some_and(|byte| byte & 0xc0 == 0x80)
                }) {
                    return Err(bad());
                }
            }
        }
        if g.children
            .iter()
            .chain(&g.roots)
            .any(|&id| id as usize >= nodes)
        {
            return Err(bad());
        }
        for &(s, n) in &g.sources {
            c.bytes(n as usize)?;
            std::str::from_utf8(g.segment(&g.bytes, s as i32, n as i32)?).map_err(|_| bad())?;
        }
        let mut states = vec![0u8; nodes];
        let mut heights = vec![0usize; nodes];
        for root in 0..nodes {
            if states[root] == 2 {
                continue;
            }
            let mut stack = vec![(root, 0usize)];
            while let Some((id, next)) = stack.last_mut() {
                c.check()?;
                let id = *id;
                let node = &g.nodes[id];
                states[id] = 1;
                let children = if node.tag >= 6 {
                    g.segment(&g.children, node.fields[0], node.fields[1])?
                } else {
                    &[]
                };
                let count = children.len() + usize::from(node.fields[4] >= 0);
                if *next < count {
                    let child = if *next < children.len() {
                        children[*next] as usize
                    } else {
                        node.fields[4] as usize
                    };
                    *next += 1;
                    if states[child] == 1 {
                        return Err(bad());
                    }
                    if states[child] == 0 {
                        if stack.len() >= limits.depth {
                            return Err(bad());
                        }
                        stack.push((child, 0));
                    }
                } else {
                    let mut height = 1;
                    for &child in children {
                        height = height.max(heights[child as usize] + 1);
                    }
                    if node.fields[4] >= 0 {
                        height = height.max(heights[node.fields[4] as usize] + 1);
                    }
                    if height > limits.depth {
                        return Err(bad());
                    }
                    heights[id] = height;
                    states[id] = 2;
                    stack.pop();
                }
            }
        }
        Ok(g)
    }
    fn segment<'a, T>(&self, v: &'a [T], start: i32, count: i32) -> Result<&'a [T], EvalError> {
        if start < 0 || count < 0 {
            return Err(bad());
        }
        v.get(
            start as usize
                ..(start as usize)
                    .checked_add(count as usize)
                    .ok_or_else(bad)?,
        )
        .ok_or_else(bad)
    }
    pub fn expansion(
        &self,
        call: &Form,
        origin: ExpansionOrigin,
        originals: &HashMap<i32, Input>,
        limits: ConversionLimits,
        c: &Budget,
    ) -> Result<Form, EvalError> {
        struct Decode<'a> {
            g: &'a Graph,
            states: Vec<u8>,
            memo: Vec<Option<Datum>>,
            originals: &'a HashMap<i32, Input>,
            reuse: HashMap<usize, Form>,
            limits: ConversionLimits,
            c: &'a Budget,
            visits: usize,
        }
        impl Decode<'_> {
            fn visit(&mut self, id: usize, depth: usize) -> Result<Datum, EvalError> {
                self.c.check()?;
                self.visits += 1;
                if depth > self.limits.depth || self.visits > self.limits.nodes {
                    return Err(EvalError("Goose conversion limit exceeded".into()));
                }
                if self.states[id] == 1 {
                    return Err(bad());
                }
                if let Some(d) = &self.memo[id] {
                    return Ok(d.clone());
                }
                self.states[id] = 1;
                let n = &self.g.nodes[id];
                let f = n.fields;
                let value = match n.tag {
                    0 => DatumKind::Nil,
                    1 => DatumKind::Boolean(n.number != 0.),
                    // The executable runtime's scalar is an ECMAScript
                    // Number, as in the V8 bridge: preserve its exact f64 bits,
                    // including negative zero, rather than narrowing integers.
                    2 => DatumKind::Double(n.number),
                    3 => {
                        self.c.bytes(f[1] as usize * 2)?;
                        DatumKind::String(self.g.segment(&self.g.units, f[0], f[1])?.into())
                    }
                    4 | 5 => {
                        self.c.bytes((f[1] as usize + f[3].max(0) as usize) * 2)?;
                        let ns = if f[3] < 0 {
                            None
                        } else {
                            Some(self.g.segment(&self.g.units, f[2], f[3])?)
                        };
                        let name = Name::from_utf16(ns, self.g.segment(&self.g.units, f[0], f[1])?);
                        if n.tag == 4 {
                            DatumKind::Symbol(name)
                        } else {
                            DatumKind::Keyword(name)
                        }
                    }
                    6..=9 => {
                        let children = self
                            .g
                            .segment(&self.g.children, f[0], f[1])?
                            .iter()
                            .map(|&id| self.visit(id as usize, depth + 1))
                            .collect::<Result<Vec<_>, _>>()?;
                        DatumKind::Collection {
                            kind: match n.tag {
                                6 => CollectionKind::List,
                                7 => CollectionKind::Vector,
                                8 => CollectionKind::Map,
                                _ => CollectionKind::Set,
                            },
                            children: children.into(),
                        }
                    }
                    _ => return Err(bad()),
                };
                let mut d = Datum::new(value);
                if f[4] >= 0 {
                    d = d.with_metadata(self.visit(f[4] as usize, depth + 1)?);
                }
                if let Some(original) = self.originals.get(&f[5]) {
                    if !compatible(&d, &original.data, self.c, depth, self.limits)? {
                        return Err(bad());
                    }
                    // Restore reader scalar spelling/flavour while retaining
                    // a distinct Datum identity for each wire occurrence.
                    if let DatumKind::Integer(value) = original.data.kind() {
                        let meta = d.metadata();
                        d = Datum::new(DatumKind::Integer(*value));
                        if let Some(meta) = meta {
                            d = d.with_metadata(meta);
                        }
                    }
                    if let Some(form) = &original.form {
                        let span = form.span();
                        if f[6] < 0
                            || f[7] as usize != span.bytes().start
                            || f[8] as usize != span.bytes().end
                        {
                            return Err(bad());
                        }
                        let (start, len) = self.g.sources[f[6] as usize];
                        if self.g.segment(&self.g.bytes, start as i32, len as i32)?
                            != span.source().text().as_bytes()
                        {
                            return Err(bad());
                        }
                        self.reuse.insert(d.identity(), form.clone());
                    } else if f[6] != -1 {
                        return Err(bad());
                    }
                } else if f[5] >= 0 {
                    return Err(bad());
                }
                self.states[id] = 2;
                self.memo[id] = Some(d.clone());
                Ok(d)
            }
        }
        let mut d = Decode {
            g: self,
            states: vec![0; self.nodes.len()],
            memo: vec![None; self.nodes.len()],
            originals,
            reuse: HashMap::new(),
            limits,
            c,
            visits: 0,
        };
        let data = d.visit(self.roots[0] as usize, 1)?;
        Ok(Form::from_bridge_expansion(data, call, origin, &d.reuse))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluator::EvaluatorOptions;
    use std::sync::{Arc, Barrier};

    fn control(polls: u64) -> ExecutionControl {
        ExecutionControl::new(&EvaluatorOptions {
            poll_budget: polls,
            timeout: std::time::Duration::from_secs(60),
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn goose_compiler_bridge_shared_exact_debits_and_exhaustion() {
        const COST: u64 = POLL_BATCH * 3 + 17;
        for consumers in [1, 2, 4, 8] {
            for shortage in [0, 1] {
                let c = control(COST * consumers - shortage);
                let barrier = Arc::new(Barrier::new(consumers as usize));
                let outcomes = std::thread::scope(|scope| {
                    (0..consumers)
                        .map(|_| {
                            let c = c.clone();
                            let barrier = barrier.clone();
                            scope.spawn(move || {
                                let budget = Budget::new(COST as usize, c);
                                barrier.wait();
                                let result = (|| {
                                    for _ in 0..COST {
                                        budget.check()?;
                                    }
                                    Ok(())
                                })();
                                budget.settle(result)
                            })
                        })
                        .collect::<Vec<_>>()
                        .into_iter()
                        .map(|thread| thread.join().unwrap())
                        .collect::<Vec<_>>()
                });
                if shortage == 0 {
                    assert!(outcomes.iter().all(Result::is_ok), "{outcomes:?}");
                    assert_eq!(c.remaining_polls(), 0);
                    assert!(c.poll_many(0).is_ok(), "zero fuel permits no-cost checks");
                    assert!(c.poll().unwrap_err().0.contains("budget exhausted"));
                    assert_eq!(c.remaining_polls(), 0);
                } else {
                    assert!(outcomes.iter().any(Result::is_err));
                    assert!(c.remaining_polls() < COST, "no underflow/wraparound");
                }
                assert!(c.poll_many(0).unwrap_err().0.contains("budget exhausted"));
                assert!(c.check().is_err(), "caught exhaustion remains terminal");
            }
        }
    }

    #[test]
    fn goose_compiler_bridge_nested_tails_and_error_cleanup() {
        let reentrant = control(3);
        let shared = Budget::new(1000, reentrant.clone());
        shared.check().unwrap();
        let nested_result = shared.bytes(1);
        shared.settle(nested_result).unwrap();
        assert_eq!(reentrant.remaining_polls(), 1);
        shared.check().unwrap();
        shared.finish().unwrap();
        assert_eq!(reentrant.remaining_polls(), 0);

        let c = control(POLL_BATCH + 2);
        let outer = Budget::new(1000, c.clone());
        outer.check().unwrap();
        // A nested converter takes only its actual cost; the outer's unpaid
        // tail holds no reservation and does not cause premature exhaustion.
        let nested = Budget::new(1000, c.clone());
        for _ in 0..POLL_BATCH {
            nested.check().unwrap();
        }
        nested.finish().unwrap();
        assert_eq!(c.remaining_polls(), 2);
        outer.finish().unwrap();
        assert_eq!(c.remaining_polls(), 1);
        // Failed immutable work is charged even if its error is caught.
        let failed = Budget::new(0, c.clone());
        assert!(failed.check().is_err());
        drop(failed);
        assert_eq!(c.remaining_polls(), 0);
        assert!(c.poll_many(0).is_ok());
        outer.check().unwrap();
        assert!(outer.finish().unwrap_err().0.contains("budget exhausted"));
        assert!(outer.finish().is_err());
        assert!(control(1).poll().is_ok(), "fresh session recovers");
    }

    #[test]
    fn goose_compiler_bridge_terminal_observation_is_bounded() {
        for cancel in [false, true] {
            let c = control(1_000_000);
            let budget = Budget::new(1_000_000, c.clone());
            for _ in 0..POLL_BATCH - 1 {
                budget.check().unwrap();
            }
            if cancel {
                c.cancel();
            } else {
                c.expire_for_test();
            }
            let expected = if cancel { "cancelled" } else { "deadline" };
            assert!(budget.check().unwrap_err().0.contains(expected));
            assert!(budget.finish().unwrap_err().0.contains(expected));
            assert!(c.poll().unwrap_err().0.contains(expected));
            assert!(c.check().is_err(), "caught terminal failure stays sticky");
            assert!(control(1).poll().is_ok());
        }
        // Publication checks the terminal state even for a one-check tail.
        let c = control(100);
        let budget = Budget::new(100, c.clone());
        budget.bytes(1).unwrap();
        c.cancel();
        assert!(budget.settle(Ok(())).unwrap_err().0.contains("cancelled"));
    }

    #[test]
    fn goose_compiler_bridge_large_graph_charges_and_interrupts() {
        let limits = ConversionLimits::default();
        let nil = Datum::new(DatumKind::Nil);
        // A scalar datum costs its entry and final node/work check. Exercise
        // actual graph construction, including the non-multiple-of-128 tail.
        let c = control(20_002);
        let budget = Budget::new(limits.nodes, c.clone());
        let mut graph = Graph::default();
        for _ in 0..10_001 {
            graph.datum(&nil, 1, limits, &budget).unwrap();
        }
        budget.finish().unwrap();
        assert_eq!(graph.nodes.len(), 10_001);
        assert_eq!(c.remaining_polls(), 0);
        for cancel in [false, true] {
            let c = control(100_000);
            let budget = Budget::new(limits.nodes, c.clone());
            let mut graph = Graph::default();
            for _ in 0..5000 {
                graph.datum(&nil, 1, limits, &budget).unwrap();
            }
            if cancel {
                c.cancel();
            } else {
                c.expire_for_test();
            }
            let failure = loop {
                match graph.datum(&nil, 1, limits, &budget) {
                    Ok(_) => assert!(graph.nodes.len() < 5000 + POLL_BATCH as usize),
                    Err(error) => break error,
                }
            };
            assert!(
                failure
                    .0
                    .contains(if cancel { "cancelled" } else { "deadline" })
            );
            assert!(
                budget.settle(Ok(graph)).is_err(),
                "partial graph stays private"
            );
            assert!(c.poll_many(0).is_err());
        }
    }
    #[test]
    fn goose_compiler_bridge_source_bytes_and_runtime_units_are_distinct() {
        let source = crate::compiler_data::Source::new("unicode.cljs", "é😀");
        let form = Form::from_data(
            Datum::new(DatumKind::String(
                vec![0x00e9, 0xd83d, 0xde00, 0xd800].into(),
            )),
            source.span(0..6).unwrap(),
            crate::compiler_data::Origin::Source,
        );
        let limits = ConversionLimits::default();
        let control = ExecutionControl::new(&Default::default()).unwrap();
        let mut graph = Graph::arguments(
            &[super::super::Stored::Form(form.clone(), limits)],
            limits,
            &Budget::new(limits.nodes, control.clone()),
        )
        .unwrap();
        for offset in [1, 3, 4, 5] {
            graph.nodes[0].fields[7] = offset;
            assert!(
                Graph::decode(
                    &graph.encode().unwrap(),
                    limits,
                    &Budget::new(limits.nodes, control.clone())
                )
                .is_err()
            );
            assert_eq!(form.span().bytes(), 0..6);
            assert!(
                matches!(form.data().kind(), DatumKind::String(units) if units.last() == Some(&0xd800))
            );
        }
        graph.nodes[0].fields[7] = 0;
        let good = graph.encode().unwrap();
        let decoded =
            Graph::decode(&good, limits, &Budget::new(limits.nodes, control.clone())).unwrap();
        assert_eq!(decoded.units, [0x00e9, 0xd83d, 0xde00, 0xd800]);
        graph.bytes[0] = 255;
        assert!(
            Graph::decode(
                &graph.encode().unwrap(),
                limits,
                &Budget::new(limits.nodes, control)
            )
            .is_err()
        );
        assert_eq!(form.span().source().text(), "é😀");
    }
    #[test]
    fn goose_compiler_bridge_rejects_truncations_and_unreachable_cycles() {
        let source = crate::compiler_data::Source::new("graph.cljs", "nil");
        let form = Form::from_data(
            Datum::new(DatumKind::Nil),
            source.span(0..3).unwrap(),
            crate::compiler_data::Origin::Source,
        );
        let control = ExecutionControl::new(&Default::default()).unwrap();
        let limits = ConversionLimits::default();
        let mut graph = Graph::arguments(
            &[super::super::Stored::Form(form, limits)],
            limits,
            &Budget::new(limits.nodes, control.clone()),
        )
        .unwrap();
        let bytes = graph.encode().unwrap();
        for end in 0..bytes.len() {
            assert!(
                Graph::decode(
                    &bytes[..end],
                    limits,
                    &Budget::new(limits.nodes, control.clone())
                )
                .is_err(),
                "prefix {end}"
            );
        }
        assert!(Graph::decode(&bytes, limits, &Budget::new(limits.nodes, control.clone())).is_ok());
        let id = graph.nodes.len() as u32;
        let start = graph.children.len() as i32;
        graph.children.push(id);
        let mut node = Node {
            tag: 7,
            ..Default::default()
        };
        node.fields[0] = start;
        node.fields[1] = 1;
        graph.nodes.push(node);
        assert!(
            Graph::decode(
                &graph.encode().unwrap(),
                limits,
                &Budget::new(limits.nodes, control)
            )
            .is_err(),
            "unreachable cycle must be rejected"
        );
    }
    #[test]
    fn goose_compiler_bridge_preserves_number_bits_and_borrowed_flavours() {
        let source = crate::compiler_data::Source::new("numbers.cljs", "nil");
        let call = Form::from_data(
            Datum::new(DatumKind::Nil),
            source.span(0..3).unwrap(),
            crate::compiler_data::Origin::Source,
        );
        let origin = || ExpansionOrigin {
            macro_name: Name::new(None, "wire"),
            call: call.span(),
            definition: None,
            helpers: vec![],
        };
        let limits = ConversionLimits::default();
        for number in [-0.0, 1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let control = ExecutionControl::new(&Default::default()).unwrap();
            let budget = Budget::new(limits.nodes, control);
            let graph = Graph {
                nodes: vec![Node {
                    tag: 2,
                    number,
                    ..Default::default()
                }],
                roots: vec![0],
                ..Default::default()
            };
            let decoded = Graph::decode(&graph.encode().unwrap(), limits, &budget).unwrap();
            let form = decoded
                .expansion(&call, origin(), &HashMap::new(), limits, &budget)
                .unwrap();
            let data = form.data();
            let DatumKind::Double(value) = data.kind() else {
                panic!("computed JS Number was narrowed")
            };
            assert_eq!(value.to_bits(), number.to_bits());
        }
        let control = ExecutionControl::new(&Default::default()).unwrap();
        let budget = Budget::new(limits.nodes, control);
        let graph = Graph::arguments(
            &[super::super::Stored::Data(
                Datum::new(DatumKind::Integer(7)),
                limits,
            )],
            limits,
            &budget,
        )
        .unwrap();
        let decoded = Graph::decode(&graph.encode().unwrap(), limits, &budget).unwrap();
        let form = decoded
            .expansion(&call, origin(), &graph.originals, limits, &budget)
            .unwrap();
        assert!(matches!(form.data().kind(), DatumKind::Integer(7)));
    }
}
