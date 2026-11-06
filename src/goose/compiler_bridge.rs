//! GFB2 compiler-owned form transport. No engine pointers or printed forms.
//! A source travels as the compiler's identity for it and its byte length,
//! never its text (light-cljs-817): Goose keeps spans against the identity,
//! and a result's spans are checked against the compiler's own [`Source`].
use crate::CollectionKind;
use crate::compiler_data::{Datum, DatumKind, ExpansionOrigin, Form, Name, Source};
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
/// The words of a narrowed graph's omission record (version 3), after the
/// six counts: [`Graph::omitted_nodes`] and the rest, as
/// form_bridge.goose's `Omitted` reads them.
const OMISSION_WORDS: usize = 6;
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
/// A graph's source entry: the compiler's identity for a source (its
/// [`crate::compiler_data::SourceId`]), no text (`start` 0), and its byte
/// length. Goose's own fixtures may carry text (identity -1, `start` into
/// the graph's text), which the compiler never sends and refuses.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SourceEntry {
    identity: i32,
    start: u32,
    count: u32,
}
/// What a call sent, which its result is read against: its input
/// occurrences by origin token, and its sources by identity.
#[derive(Default)]
pub(super) struct Originals {
    inputs: HashMap<i32, Input>,
    sources: HashMap<i32, Source>,
}
#[derive(Default)]
pub(super) struct Graph {
    nodes: Vec<Node>,
    children: Vec<u32>,
    units: Vec<u16>,
    sources: Vec<SourceEntry>,
    roots: Vec<u32>,
    pub originals: Originals,
    /// A decoded result's sources: the call's [`Source`] each entry names.
    resolved: Vec<Source>,
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
    /// The string rows Goose's import would add for the omitted entries
    /// (one a string, one or two a symbol or keyword), and the code units
    /// its validation reads in their symbols and keywords (the namespace's
    /// if any, otherwise the name's): what the omission record carries
    /// besides nodes, child values, text units and depth (light-cljs-820).
    omitted_strings: usize,
    omitted_names: usize,
    /// Whether an omitted entry holds data Goose's validation refuses as
    /// unsupported, which the compiler does not refuse itself: then Goose
    /// is sent the whole environment, so that it refuses it.
    omitted_refused: bool,
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
    strings: usize,
    names: usize,
    size: usize,
    height: usize,
    refused: bool,
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
    /// The bytes of source text the graph carries, for the session's
    /// transport counters: none, since a source travels as its identity.
    pub fn source_bytes(&self) -> usize {
        0
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
            + self.sources.len() * 12
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
        let occurrence = self.originals.inputs.len() as i32;
        self.originals.inputs.insert(
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
                self.omitted_strings += 1;
            }
            DatumKind::Symbol(v) | DatumKind::Keyword(v) => {
                units = v.name().len() + v.namespace().map_or(0, <[u16]>::len);
                c.bytes(units * 2)?;
                self.omitted_charges += 1;
                self.omitted_strings += 1 + usize::from(v.namespace().is_some());
                // What form_bridge.goose's `validate` reads and refuses: a
                // `/` in a namespace, or in a name of more than one unit
                // without one, and an empty namespace with an empty name.
                let slash = |text: &[u16]| text.contains(&47);
                match v.namespace() {
                    Some(ns) => {
                        self.omitted_names += ns.len();
                        self.omitted_refused |= slash(ns) || ns.is_empty() && v.name().is_empty();
                    }
                    None => {
                        self.omitted_names += v.name().len();
                        self.omitted_refused |= v.name().len() != 1 && slash(v.name());
                    }
                }
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
            self.omit_metadata(&meta, depth + 1, limits, c)?;
        }
        self.charge(depth, limits, c)?;
        self.omitted_nodes += 1;
        self.omitted_size += 48;
        Ok(())
    }
    /// [`Self::omit`] for metadata, which Goose refuses unless it is a map.
    fn omit_metadata(
        &mut self,
        meta: &Datum,
        depth: usize,
        limits: ConversionLimits,
        c: &Budget,
    ) -> Result<(), EvalError> {
        self.omitted_refused |= !matches!(
            meta.kind(),
            DatumKind::Collection {
                kind: CollectionKind::Map,
                ..
            }
        );
        self.omit(meta, depth, limits, c)
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
            self.omitted_strings += o.strings;
            self.omitted_names += o.names;
            self.omitted_depth = self.omitted_depth.max(depth + o.height - 1);
            self.omitted_refused |= o.refused;
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
            self.omitted_strings,
            self.omitted_names,
            std::mem::replace(&mut self.omitted_refused, false),
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
            strings: self.omitted_strings - before.7,
            names: self.omitted_names - before.8,
            size: self.omitted_size - before.1,
            height: self.omitted_depth - depth + 1,
            refused: self.omitted_refused,
        };
        self.omitted_depth = self.omitted_depth.max(before.4);
        self.omitted_refused |= before.9;
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
            self.omit_metadata(&meta, depth + 1, limits, c)?;
        }
        let mut n = Node {
            tag: 8,
            ..Default::default()
        };
        n.fields[0] = self.children.len() as i32;
        n.fields[1] = ids.len() as i32;
        self.children.extend(ids);
        self.charge(depth, limits, c)?;
        let occurrence = self.originals.inputs.len() as i32;
        self.originals.inputs.insert(
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
            // The source's identity and length, not its text.
            let identity = i32::try_from(source.id().get()).map_err(|_| {
                EvalError("Goose source identity is past the graph format's range".into())
            })?;
            let count = u32::try_from(source.text().len()).map_err(|_| bad())?;
            let i = self.sources.len();
            self.sources.push(SourceEntry {
                identity,
                start: 0,
                count,
            });
            self.source_ids.insert(source.id(), i);
            self.originals.sources.insert(identity, source.clone());
            i
        };
        let occurrence = self.originals.inputs.len() as i32;
        self.originals.inputs.insert(
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
        // Goose checks, charges and reserves the omitted entries from the
        // graph's omission record (light-cljs-820), but cannot refuse data
        // it is not sent: when an omitted entry holds data Goose's
        // validation refuses, Goose is sent the whole environment, so that
        // it refuses it. This graph charged what the whole one charges.
        if g.omitted_refused {
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
    /// The graph's omission record, which version 3 of the encoding writes
    /// after its counts when a narrowed environment left entries out: their
    /// nodes, child values, text units, string rows, the units Goose's
    /// validation reads in their symbols and keywords, and the deepest
    /// depth among them (a root at 1). Goose checks, polls and charges them
    /// as if they were present and reserves their rows for the rest of the
    /// job (form_bridge.goose `Omitted`, light-cljs-820).
    fn omission(&self) -> Option<[usize; OMISSION_WORDS]> {
        (self.omitted_nodes > 0).then_some([
            self.omitted_nodes,
            self.omitted_children,
            self.omitted_units,
            self.omitted_strings,
            self.omitted_names,
            self.omitted_depth,
        ])
    }
    pub fn encode(&self) -> Result<Vec<u8>, EvalError> {
        if self.size() > MAX_BYTES {
            return Err(bad());
        }
        let omission = self.omission();
        let mut out = Vec::with_capacity(self.encoded_size() + OMISSION_WORDS * 4);
        out.extend(b"GFB2");
        out.extend(if omission.is_some() { 3u32 } else { 2 }.to_le_bytes());
        for n in [
            self.nodes.len(),
            self.children.len(),
            self.units.len(),
            self.sources.len(),
            0,
            self.roots.len(),
        ]
        .into_iter()
        .chain(omission.into_iter().flatten())
        {
            out.extend(u32::try_from(n).map_err(|_| bad())?.to_le_bytes());
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
        for s in &self.sources {
            out.extend(s.identity.to_le_bytes());
            out.extend(s.start.to_le_bytes());
            out.extend(s.count.to_le_bytes());
        }
        for id in &self.roots {
            out.extend(id.to_le_bytes());
        }
        Ok(out)
    }
    /// A result graph for the call that sent `originals`: its sources must
    /// be ones the call sent, by identity and byte length, and each span
    /// must lie inside its source on UTF-8 boundaries, checked against the
    /// compiler's own [`Source`] once per source entry.
    pub fn decode(
        bytes: &[u8],
        originals: &Originals,
        limits: ConversionLimits,
        c: &Budget,
    ) -> Result<Self, EvalError> {
        c.check()?;
        if bytes.len() < 32 || bytes.len() > MAX_BYTES || &bytes[..4] != b"GFB2" {
            return Err(bad());
        }
        let mut at = 4;
        let word = |at: &mut usize| {
            let n = u32::from_le_bytes(bytes[*at..*at + 4].try_into().unwrap());
            *at += 4;
            n
        };
        if word(&mut at) != 2 {
            return Err(bad());
        }
        let counts: [usize; 6] = std::array::from_fn(|_| word(&mut at) as usize);
        let [nodes, children, units, sources, text, roots] = counts;
        // The compiler sends no source text and takes none back.
        if nodes > limits.nodes
            || roots != 1
            || text != 0
            || 32u64
                + nodes as u64 * 48
                + children as u64 * 4
                + units as u64 * 2
                + sources as u64 * 12
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
            c.check()?;
            let entry = SourceEntry {
                identity: word(&mut at) as i32,
                start: word(&mut at),
                count: word(&mut at),
            };
            // A source the call did not send, or with another length, is
            // malformed, as is one that claims text.
            let source = originals
                .sources
                .get(&entry.identity)
                .filter(|s| entry.start == 0 && s.text().len() == entry.count as usize)
                .ok_or_else(bad)?;
            g.sources.push(entry);
            g.resolved.push(source.clone());
        }
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
                let text = g.resolved[n.fields[6] as usize].text();
                if n.fields[7] < 0
                    || n.fields[8] < n.fields[7]
                    || n.fields[8] as usize > text.len()
                    || !text.is_char_boundary(n.fields[7] as usize)
                    || !text.is_char_boundary(n.fields[8] as usize)
                {
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
        originals: &Originals,
        limits: ConversionLimits,
        c: &Budget,
    ) -> Result<Form, EvalError> {
        struct Decode<'a> {
            g: &'a Graph,
            states: Vec<u8>,
            memo: Vec<Option<Datum>>,
            originals: &'a Originals,
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
                if let Some(original) = self.originals.inputs.get(&f[5]) {
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
                        // The same source, by identity: decode checked the
                        // entry against the call's sources once.
                        if self.g.resolved[f[6] as usize].id() != span.source().id() {
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
        let decode = |graph: &Graph, originals: &Originals| {
            Graph::decode(
                &graph.encode().unwrap(),
                originals,
                limits,
                &Budget::new(limits.nodes, control.clone()),
            )
        };
        // Spans are checked against the compiler's source: off a UTF-8
        // boundary, past its end or reversed is malformed.
        for (start, end) in [(1, 6), (3, 6), (4, 6), (5, 6), (0, 7), (2, 0), (0, 5)] {
            graph.nodes[0].fields[7] = start;
            graph.nodes[0].fields[8] = end;
            assert!(decode(&graph, &graph.originals).is_err(), "{start}..{end}");
            assert_eq!(form.span().bytes(), 0..6);
            assert!(
                matches!(form.data().kind(), DatumKind::String(units) if units.last() == Some(&0xd800))
            );
        }
        graph.nodes[0].fields[7] = 0;
        graph.nodes[0].fields[8] = 6;
        let good = graph.encode().unwrap();
        // The wire carries the source's identity and length, not its text.
        assert_eq!(&good[..4], b"GFB2");
        assert!(!good.windows(6).any(|w| w == "é😀".as_bytes()[..6].as_ref()));
        assert_eq!(good[24..28], [0; 4], "no source text");
        let decoded = decode(&graph, &graph.originals).unwrap();
        assert_eq!(decoded.units, [0x00e9, 0xd83d, 0xde00, 0xd800]);
        assert_eq!(decoded.resolved[0].id(), source.id());
        // A source the call did not send, a length that is not its
        // source's, or an entry claiming text is malformed.
        let other = Graph::arguments(
            &[super::super::Stored::Form(
                Form::from_data(
                    Datum::new(DatumKind::Nil),
                    crate::compiler_data::Source::new("other.cljs", "é😀")
                        .span(0..6)
                        .unwrap(),
                    crate::compiler_data::Origin::Source,
                ),
                limits,
            )],
            limits,
            &Budget::new(limits.nodes, control.clone()),
        )
        .unwrap();
        assert!(decode(&graph, &other.originals).is_err());
        assert!(decode(&graph, &Originals::default()).is_err());
        let entry = graph.sources[0];
        for changed in [
            SourceEntry { count: 5, ..entry },
            SourceEntry { count: 7, ..entry },
            SourceEntry { start: 1, ..entry },
            SourceEntry {
                identity: entry.identity + 1,
                ..entry
            },
        ] {
            graph.sources[0] = changed;
            assert!(decode(&graph, &graph.originals).is_err(), "{changed:?}");
        }
        graph.sources[0] = entry;
        let mut text = good.clone();
        text[24] = 1;
        text.push(b'x');
        assert!(
            Graph::decode(
                &text,
                &graph.originals,
                limits,
                &Budget::new(limits.nodes, control.clone())
            )
            .is_err(),
            "a result carrying source text"
        );
        let mut old = good.clone();
        old[..4].copy_from_slice(b"GFB1");
        assert!(
            Graph::decode(
                &old,
                &graph.originals,
                limits,
                &Budget::new(limits.nodes, control)
            )
            .is_err(),
            "GFB1"
        );
        assert_eq!(form.span().source().text(), "é😀");
    }
    /// light-cljs-817: a returned input occurrence names its source by the
    /// identity the call sent and keeps its original form's span and origin,
    /// and a new form takes the call's; a result naming a source the call
    /// did not send, or an occurrence naming another sent source than its
    /// form's, is malformed.
    #[test]
    fn goose_compiler_bridge_returned_inputs_keep_their_forms_by_source_identity() {
        use crate::compiler_reader::{NamespaceView, read_form};
        let read = |name: &str, text: &str| {
            let source = crate::compiler_data::Source::new(name, text);
            read_form(&source, &NamespaceView::new("fixture.core", &source))
                .unwrap()
                .unwrap()
        };
        let call = read("call.cljs", "(m [x \"é\"] y)");
        // As long as the call, so that the call's span lies inside it.
        let other = read("other.cljs", "(zzzzzzzzzzzz)");
        assert_eq!(call.span().bytes(), other.span().bytes());
        let origin = || ExpansionOrigin {
            macro_name: Name::new(None, "m"),
            call: call.span(),
            definition: None,
            helpers: vec![],
        };
        let limits = ConversionLimits::default();
        let control = ExecutionControl::new(&Default::default()).unwrap();
        let budget = || Budget::new(limits.nodes, control.clone());
        let stored = |forms: &[&Form]| {
            forms
                .iter()
                .map(|&f| super::super::Stored::Form(f.clone(), limits))
                .collect::<Vec<_>>()
        };
        let mut args = Graph::arguments(&stored(&[&call, &other]), limits, &budget()).unwrap();
        // A result has one root; `other`'s nodes stay, unreached.
        args.roots.truncate(1);
        assert_eq!(args.sources.len(), 2);
        assert_eq!(args.source_bytes(), 0);
        // The argument graph is the result an identity expansion returns:
        // every node an input occurrence, named by its origin token.
        let bytes = args.encode().unwrap();
        let decoded = Graph::decode(&bytes, &args.originals, limits, &budget()).unwrap();
        let form = decoded
            .expansion(&call, origin(), &args.originals, limits, &budget())
            .unwrap();
        fn same(a: &Form, b: &Form) {
            assert_eq!(a.span().source().id(), b.span().source().id());
            assert_eq!(a.span().bytes(), b.span().bytes());
            assert!(matches!(
                a.node().origin,
                crate::compiler_data::Origin::Source
            ));
            assert!(a.data().value_eq(&b.data()));
            let (ac, bc) = (a.children(), b.children());
            assert_eq!(ac.len(), bc.len());
            for (a, b) in ac.iter().zip(&bc) {
                same(a, b);
            }
        }
        same(&form, &call);
        // A new vector holding the call's second element: the vector takes
        // the call's span, the element keeps its own.
        let mut built = Graph::arguments(&stored(&[&call]), limits, &budget()).unwrap();
        let element = built.children[built.nodes[built.roots[0] as usize].fields[0] as usize + 1];
        let mut fresh = Node {
            tag: 7,
            ..Default::default()
        };
        fresh.fields[0] = built.children.len() as i32;
        fresh.fields[1] = 1;
        built.children.push(element);
        built.roots = vec![built.nodes.len() as u32];
        built.nodes.push(fresh);
        let result = Graph::decode(
            &built.encode().unwrap(),
            &built.originals,
            limits,
            &budget(),
        )
        .unwrap()
        .expansion(&call, origin(), &built.originals, limits, &budget())
        .unwrap();
        assert_eq!(result.span().bytes(), call.span().bytes());
        assert!(matches!(
            result.node().origin,
            crate::compiler_data::Origin::MacroExpansion(_)
        ));
        same(&result.children()[0], &call.children()[1]);
        // `other.cljs` was not sent by a call of `call` alone.
        let alone = Graph::arguments(&stored(&[&call]), limits, &budget()).unwrap();
        assert!(Graph::decode(&bytes, &alone.originals, limits, &budget()).is_err());
        // An occurrence of `call` naming `other.cljs`, with its own span.
        let mut swapped = Graph::arguments(&stored(&[&call, &other]), limits, &budget()).unwrap();
        swapped.roots.truncate(1);
        let root = swapped.roots[0] as usize;
        swapped.nodes[root].fields[6] = 1;
        let decoded = Graph::decode(
            &swapped.encode().unwrap(),
            &swapped.originals,
            limits,
            &budget(),
        )
        .unwrap();
        assert!(
            decoded
                .expansion(&call, origin(), &swapped.originals, limits, &budget())
                .is_err()
        );
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
                    &graph.originals,
                    limits,
                    &Budget::new(limits.nodes, control.clone())
                )
                .is_err(),
                "prefix {end}"
            );
        }
        assert!(
            Graph::decode(
                &bytes,
                &graph.originals,
                limits,
                &Budget::new(limits.nodes, control.clone())
            )
            .is_ok()
        );
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
                &graph.originals,
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
            let decoded =
                Graph::decode(&graph.encode().unwrap(), &graph.originals, limits, &budget).unwrap();
            let form = decoded
                .expansion(&call, origin(), &Originals::default(), limits, &budget)
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
        let decoded =
            Graph::decode(&graph.encode().unwrap(), &graph.originals, limits, &budget).unwrap();
        let form = decoded
            .expansion(&call, origin(), &graph.originals, limits, &budget)
            .unwrap();
        assert!(matches!(form.data().kind(), DatumKind::Integer(7)));
    }

    fn text(s: &str) -> Datum {
        Datum::new(DatumKind::String(
            s.encode_utf16().collect::<Vec<_>>().into(),
        ))
    }
    fn keyword(ns: Option<&str>, name: &str) -> Datum {
        Datum::new(DatumKind::Keyword(Name::new(ns, name)))
    }
    fn symbol(ns: Option<&str>, name: &str) -> Datum {
        Datum::new(DatumKind::Symbol(Name::new(ns, name)))
    }
    fn collection(kind: CollectionKind, children: Vec<Datum>) -> Datum {
        Datum::new(DatumKind::Collection {
            kind,
            children: children.into(),
        })
    }
    /// An environment whose macro inventory, which a narrowed view leaves
    /// out, holds `probe` in a macro's metadata, and the view.
    fn environment_with(probe: Datum) -> (Datum, Datum) {
        use CollectionKind::{Map, Vector};
        let ns = [keyword(None, "ns"), symbol(None, "app.main")];
        let locals = [keyword(None, "locals"), collection(Map, vec![])];
        let meta = collection(
            Map,
            vec![
                keyword(None, "doc"),
                text("λx"),
                keyword(Some("light-cljs"), "probe"),
                probe,
            ],
        );
        let macros = collection(
            Map,
            vec![
                symbol(Some("m"), "a").with_metadata(meta),
                collection(Vector, vec![text(""), symbol(None, "s")]),
            ],
        );
        let mut entries = ns.to_vec();
        entries.extend([keyword(Some("light-cljs"), "macros"), macros]);
        entries.extend(locals.clone());
        let whole = collection(Map, entries)
            .with_metadata(collection(Map, vec![keyword(None, "file"), text("f")]));
        let mut kept = ns.to_vec();
        kept.extend(locals);
        (collection(Map, kept), whole)
    }
    /// A graph's string rows Goose's import adds and the units its
    /// validation reads in names, and its height from `id`.
    fn strings_and_names(g: &Graph) -> (usize, usize) {
        g.nodes.iter().fold((0, 0), |(s, n), node| match node.tag {
            3 => (s + 1, n),
            4 | 5 if node.fields[3] >= 0 => (s + 2, n + node.fields[3] as usize),
            4 | 5 => (s + 1, n + node.fields[1] as usize),
            _ => (s, n),
        })
    }
    fn height(g: &Graph, id: u32) -> usize {
        let n = &g.nodes[id as usize];
        let mut edges = if n.tag >= 6 {
            g.children[n.fields[0] as usize..][..n.fields[1] as usize].to_vec()
        } else {
            vec![]
        };
        edges.extend((n.fields[4] >= 0).then_some(n.fields[4] as u32));
        1 + edges.iter().map(|&c| height(g, c)).max().unwrap_or(0)
    }

    /// A narrowed environment's omission record holds exactly what the
    /// left-out entries add to the whole environment's graph: its nodes,
    /// child values, text units, string rows, name units and depth, after
    /// a walk and from the cached cost alike, and the encoding writes it
    /// as version 3 (light-cljs-820).
    #[test]
    fn goose_compiler_bridge_omission_record_counts_what_the_whole_graph_adds() {
        let limits = ConversionLimits::default();
        let deep = (0..5).fold(text("deep"), |d, _| {
            collection(CollectionKind::Vector, vec![d])
        });
        let (view, whole) = environment_with(collection(
            CollectionKind::Vector,
            vec![keyword(Some("k"), "v"), symbol(None, "/"), deep],
        ));
        let budget = Budget::new(limits.nodes, control(1_000_000));
        let mut full = Graph::default();
        let root = full.datum(&whole, 1, limits, &budget).unwrap();
        let cache = OmitCache::default();
        for _ in 0..2 {
            let mut g = Graph::default();
            let id = g
                .environment(&view, &whole, 1, limits, &budget, &cache)
                .unwrap();
            assert!(!g.omitted_refused);
            assert_eq!(full.nodes.len(), g.nodes.len() + g.omitted_nodes);
            assert_eq!(full.children.len(), g.children.len() + g.omitted_children);
            assert_eq!(full.units.len(), g.units.len() + g.omitted_units);
            let (s, n) = strings_and_names(&full);
            let (ks, kn) = strings_and_names(&g);
            assert_eq!((s, n), (ks + g.omitted_strings, kn + g.omitted_names));
            assert!(g.omitted_depth > height(&g, id));
            assert_eq!(height(&full, root), g.omitted_depth);
            assert_eq!(g.size(), full.encoded_size());
            g.roots.push(id);
            let bytes = g.encode().unwrap();
            let word = |i: usize| u32::from_le_bytes(bytes[i * 4..][..4].try_into().unwrap());
            assert_eq!(word(1), 3);
            assert_eq!(bytes.len(), g.encoded_size() + OMISSION_WORDS * 4);
            let record: Vec<_> = (8..14).map(|i| word(i) as usize).collect();
            assert_eq!(record, g.omission().unwrap());
        }
        // A whole graph stays version 2.
        full.roots.push(root);
        assert_eq!(full.encode().unwrap()[4], 2);
    }

    /// Data Goose's validation refuses as unsupported in a left-out entry
    /// sends the whole environment, which Goose refuses (light-cljs-820).
    #[test]
    fn goose_compiler_bridge_refused_omitted_data_sends_the_whole_environment() {
        let limits = ConversionLimits::default();
        let refused = [
            symbol(None, "a/b"),
            keyword(Some("a/b"), "c"),
            Datum::new(DatumKind::Symbol(Name::from_utf16(Some(&[]), &[]))),
            text("x").with_metadata(collection(CollectionKind::Vector, vec![])),
        ];
        let accepted = [symbol(None, "/"), keyword(Some("a"), "b/c"), text("x")];
        for (probe, refuse) in refused
            .into_iter()
            .map(|p| (p, true))
            .chain(accepted.into_iter().map(|p| (p, false)))
        {
            let (view, whole) = environment_with(probe.clone());
            let budget = Budget::new(limits.nodes, control(1_000_000));
            let mut full = Graph::default();
            full.datum(&whole, 1, limits, &budget).unwrap();
            let g = Graph::arguments(
                &[super::super::Stored::Environment(view, whole, limits)],
                limits,
                &budget,
            )
            .unwrap();
            assert_eq!(g.omission().is_none(), refuse, "{probe:?}");
            assert_eq!(g.nodes.len() == full.nodes.len(), refuse, "{probe:?}");
        }
    }
}
