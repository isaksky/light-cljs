//! The dependency records of a compilation's namespaces so far, built one
//! namespace at a time in source order, as the persistent cache's lookup
//! needs them (docs/compiler-cache.md, "Loading, diagnostics, and
//! storage"): a namespace's key covers the keys of earlier namespaces, so
//! each must be known when the next is looked up, long before
//! [`crate::dependency_manifest::build`] assembles the whole manifest.
//!
//! An analyzed namespace's record comes from the loader and from what the
//! observation log recorded while it was the source being analyzed; an
//! installed one's comes from the loader and from the edges, inputs and
//! absent names its artifact's manifest stored, which match the current
//! ones whenever its key does. Both follow the rules of
//! [`crate::dependency_manifest::build`], with two differences that only
//! make a record describe what its namespace's compilation saw:
//!
//! - an observation the log attributes to a namespace after it completed
//!   (none is known today) is left out, since its artifact was captured
//!   before it; the whole manifest still has it;
//! - an input a helper unit records while a later namespace compiles goes to
//!   the later namespaces it reaches, not back to the earlier consumers of
//!   that helper, whose artifacts never saw it.
//!
//! `LAZY` edges are left out: a lazy target may come later in source order
//! and is not a compilation dependency, so no key covers it.

use crate::dependency_manifest::{
    DependencyManifest, EdgeKinds, ExternalInput, InputId, LoaderRecord, Lookup, LookupId,
    NamespaceId, NamespaceRecord, ObservationLog, RequireRecord, RequireTarget, SourceIdentity,
};
use std::collections::HashMap;

/// What a namespace key and its re-validation read of the dependency
/// records: [`DependencyManifest`], or a [`ManifestPrefix`] of one
/// compilation so far.
pub trait NamespaceRecords {
    fn record(&self, id: NamespaceId) -> &NamespaceRecord;
    fn lookup(&self, id: LookupId) -> &Lookup;
    fn input(&self, id: InputId) -> &ExternalInput;
    fn id(&self, name: &str) -> Option<NamespaceId>;
    /// The namespaces `id` depends on directly, ascending, with the kinds.
    fn dependency_row(&self, id: NamespaceId) -> Vec<(NamespaceId, EdgeKinds)>;
    /// The `:namespaces` broad view: every namespace's name and source
    /// identity, in source order. A [`ManifestPrefix`] has only the
    /// namespaces so far, so the persistent cache bypasses a namespace with
    /// that view and its compilation consumers.
    fn inventory(&self) -> crate::cache_key::Digest;
}

impl NamespaceRecords for DependencyManifest {
    fn record(&self, id: NamespaceId) -> &NamespaceRecord {
        DependencyManifest::record(self, id)
    }
    fn lookup(&self, id: LookupId) -> &Lookup {
        DependencyManifest::lookup(self, id)
    }
    fn input(&self, id: InputId) -> &ExternalInput {
        DependencyManifest::input(self, id)
    }
    fn id(&self, name: &str) -> Option<NamespaceId> {
        DependencyManifest::id(self, name)
    }
    fn dependency_row(&self, id: NamespaceId) -> Vec<(NamespaceId, EdgeKinds)> {
        self.dependencies(id).collect()
    }
    fn inventory(&self) -> crate::cache_key::Digest {
        crate::cache_key::inventory(&self.namespaces)
    }
}

/// How far into each list of the observation log the prefix has read.
#[derive(Clone, Copy, Debug, Default)]
struct Cursors {
    loaded: usize,
    named: usize,
    absent: usize,
    executes: usize,
    inputs: usize,
}

/// The records of a compilation's namespaces `0..len()`, by
/// [`NamespaceId`].
#[derive(Debug, Default)]
pub struct ManifestPrefix {
    namespaces: Vec<NamespaceRecord>,
    /// Each namespace's edges, ascending by target, without `LAZY`.
    rows: Vec<Vec<(NamespaceId, EdgeKinds)>>,
    /// Whether each namespace declares macros, which makes a require of it
    /// a `MACRO_PROVIDER` edge.
    macros: Vec<bool>,
    /// The loader's lookups, by [`LookupId`], copied as they appear.
    lookups: Vec<Lookup>,
    /// By [`InputId`], each distinct input once.
    inputs: Vec<ExternalInput>,
    interned: HashMap<ExternalInput, u32>,
    ids: HashMap<String, u32>,
    /// The inputs recorded while helper units or data readers ran, with
    /// those units' sources, in the order recorded.
    helper_inputs: Vec<(Vec<u32>, InputId)>,
    cursors: Cursors,
}

impl NamespaceRecords for ManifestPrefix {
    fn record(&self, id: NamespaceId) -> &NamespaceRecord {
        &self.namespaces[id.0 as usize]
    }
    fn lookup(&self, id: LookupId) -> &Lookup {
        &self.lookups[id.0 as usize]
    }
    fn input(&self, id: InputId) -> &ExternalInput {
        &self.inputs[id.0 as usize]
    }
    fn id(&self, name: &str) -> Option<NamespaceId> {
        self.ids.get(name).map(|&id| NamespaceId(id))
    }
    fn dependency_row(&self, id: NamespaceId) -> Vec<(NamespaceId, EdgeKinds)> {
        self.rows[id.0 as usize].clone()
    }
    fn inventory(&self) -> crate::cache_key::Digest {
        crate::cache_key::inventory(&self.namespaces)
    }
}

impl ManifestPrefix {
    pub fn len(&self) -> usize {
        self.namespaces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.namespaces.is_empty()
    }

    /// The edges of namespace `id`, ascending by target.
    pub fn row(&self, id: NamespaceId) -> &[(NamespaceId, EdgeKinds)] {
        &self.rows[id.0 as usize]
    }

    /// The external inputs of namespace `id`, in its record's order.
    pub fn external_inputs(&self, id: NamespaceId) -> impl Iterator<Item = &ExternalInput> + '_ {
        self.namespaces[id.0 as usize]
            .external_inputs
            .iter()
            .map(|&input| &self.inputs[input.0 as usize])
    }

    fn intern(&mut self, input: ExternalInput) -> InputId {
        let next = self.inputs.len() as u32;
        let id = *self.interned.entry(input).or_insert_with_key(|input| {
            self.inputs.push(input.clone());
            next
        });
        InputId(id)
    }

    /// The record of the next namespace as the loader resolved it, with its
    /// require edges, before anything analysis observes.
    fn loaded(
        &mut self,
        loader: &LoaderRecord,
    ) -> (NamespaceRecord, Vec<(NamespaceId, EdgeKinds)>) {
        self.lookups
            .extend_from_slice(&loader.lookups[self.lookups.len()..]);
        let id = self.namespaces.len();
        let source = &loader.sources[id];
        let mut rows = Vec::new();
        let mut requires = Vec::with_capacity(source.requires.len());
        for (written, target, form, lookups) in &source.requires {
            let target = if matches!(target.as_str(), "cljs.core" | "clojure.core") {
                RequireTarget::Core
            } else if let Some(&target) = self.ids.get(target) {
                let mut kinds = EdgeKinds::RUNTIME;
                if self.macros[target as usize] {
                    kinds |= EdgeKinds::MACRO_PROVIDER;
                }
                rows.push((NamespaceId(target), kinds));
                RequireTarget::Namespace(NamespaceId(target))
            } else {
                RequireTarget::Host
            };
            requires.push(RequireRecord {
                written: written.clone(),
                target,
                form: *form,
                lookups: lookups.clone(),
            });
        }
        let record = NamespaceRecord {
            name: source.name.clone(),
            source: SourceIdentity {
                file: source.file.clone(),
                kind: source.kind,
                origin: source.origin,
                lookup: source.lookup,
            },
            requires,
            absent: Vec::new(),
            coarse: false,
            external_inputs: Vec::new(),
        };
        (record, rows)
    }

    fn append(
        &mut self,
        record: NamespaceRecord,
        mut rows: Vec<(NamespaceId, EdgeKinds)>,
        macros: bool,
    ) -> NamespaceId {
        merge(&mut rows);
        let id = NamespaceId(self.namespaces.len() as u32);
        self.ids.insert(record.name.clone(), id.0);
        self.namespaces.push(record);
        self.rows.push(rows);
        self.macros.push(macros);
        id
    }

    /// Append the next namespace as its installed artifact's manifest
    /// describes it: the loader's resolution with the stored edges, inputs,
    /// absent names and coarse flag. A stored edge to a namespace that is
    /// not an earlier one of this compilation is dropped, so the key the
    /// lookup computes then differs from the stored one in its dependencies
    /// or, for a coarse edge, its view, after any earlier component that
    /// differs too.
    pub(crate) fn push_stored(
        &mut self,
        loader: &LoaderRecord,
        fields: &crate::artifact_codec::ManifestFields,
        macros: bool,
    ) -> NamespaceId {
        let (mut record, _) = self.loaded(loader);
        let mut rows = Vec::with_capacity(fields.dependencies.len());
        for dependency in &fields.dependencies {
            let kinds = EdgeKinds(dependency.kinds & !EdgeKinds::LAZY.0);
            if kinds == EdgeKinds::default() {
                continue;
            }
            if let Some(&target) = self.ids.get(&dependency.namespace) {
                rows.push((NamespaceId(target), kinds));
            }
        }
        record.coarse = fields.coarse;
        record.absent = fields.absent.clone();
        record.external_inputs = fields
            .inputs
            .iter()
            .map(|input| self.intern(input.clone()))
            .collect();
        self.append(record, rows, macros)
    }

    /// Record whether namespace `id` declares macros, once it is installed.
    pub fn set_macros(&mut self, id: NamespaceId, macros: bool) {
        self.macros[id.0 as usize] = macros;
    }

    /// Remove the last namespace, which [`Self::push_stored`] appended for
    /// a lookup whose key then did not match.
    pub fn pop(&mut self) {
        if let Some(record) = self.namespaces.pop() {
            self.ids.remove(&record.name);
            self.rows.pop();
            self.macros.pop();
        }
    }

    /// Skip what the observation log recorded while an installed namespace
    /// was current: nothing, since installing observes nothing.
    pub(crate) fn skip_observations(&mut self, log: &ObservationLog) {
        self.cursors = Cursors {
            loaded: log.loaded.len(),
            named: log.named.len(),
            absent: log.absent.len(),
            executes: log.executes.len(),
            inputs: self.cursors.inputs,
        };
        self.read_inputs(log, None);
    }

    /// Intern the inputs the log recorded since the last read, returning
    /// those recorded with `own` as the source being analyzed.
    fn read_inputs(&mut self, log: &ObservationLog, own: Option<u32>) -> Vec<InputId> {
        let mut owned = Vec::new();
        for record in &log.inputs[self.cursors.inputs..] {
            let id = {
                let next = self.inputs.len() as u32;
                *self
                    .interned
                    .entry(record.input.clone())
                    .or_insert_with_key(|input| {
                        self.inputs.push(input.clone());
                        next
                    })
            };
            if Some(record.own) == own {
                owned.push(InputId(id));
            }
            if !record.helpers.is_empty() {
                self.helper_inputs
                    .push((record.helpers.clone(), InputId(id)));
            }
        }
        self.cursors.inputs = log.inputs.len();
        owned
    }

    /// Append the next namespace, just analyzed: the loader's resolution and
    /// what the log recorded while it was the source being analyzed.
    /// `program_namespaces` maps the log's loaded positions to names, and
    /// `macros` says whether the namespace declares macros.
    pub(crate) fn push_analyzed(
        &mut self,
        loader: &LoaderRecord,
        log: &ObservationLog,
        program_namespaces: &crate::arena::Arena<String>,
        macros: bool,
    ) -> NamespaceId {
        let (mut record, mut rows) = self.loaded(loader);
        let id = self.namespaces.len() as u32;
        let mut absent = Vec::new();
        for &(observer, position) in &log.loaded[self.cursors.loaded..] {
            if observer != id {
                continue;
            }
            let observed = program_namespaces
                .get(position as usize)
                .and_then(|name| self.ids.get(name));
            if let Some(&observed) = observed
                && observed < id
            {
                rows.push((NamespaceId(observed), EdgeKinds::OBSERVED));
            }
        }
        for (observer, observed, kinds) in &log.named[self.cursors.named..] {
            if *observer != id {
                continue;
            }
            match self.ids.get(observed) {
                Some(&observed) => {
                    rows.push((NamespaceId(observed), *kinds | EdgeKinds::OBSERVED));
                }
                None if *observed == record.name => {}
                None => absent.push(observed.clone()),
            }
        }
        for (observer, name) in &log.absent[self.cursors.absent..] {
            if *observer == id {
                absent.push(name.clone());
            }
        }
        let coarse = log.executes[self.cursors.executes..].contains(&id);
        self.cursors.loaded = log.loaded.len();
        self.cursors.named = log.named.len();
        self.cursors.absent = log.absent.len();
        self.cursors.executes = log.executes.len();
        merge(&mut rows);
        // Helpers: what each macro provider depends on directly through its
        // requires and observations.
        let helper_sources = EdgeKinds::RUNTIME | EdgeKinds::OBSERVED;
        let mut helpers = Vec::new();
        for &(provider, kinds) in &rows {
            if kinds.contains(EdgeKinds::MACRO_PROVIDER) {
                helpers.extend(
                    self.rows[provider.0 as usize]
                        .iter()
                        .filter(|(_, kinds)| kinds.intersects(helper_sources))
                        .map(|&(target, _)| (target, EdgeKinds::HELPER)),
                );
            }
        }
        rows.append(&mut helpers);
        if coarse {
            record.coarse = true;
            rows.extend((0..id).map(|earlier| (NamespaceId(earlier), EdgeKinds::COARSE)));
        }
        merge(&mut rows);
        absent.sort_unstable();
        absent.dedup();
        record.absent = absent;
        // The configuration, the namespace's own inputs, then those of the
        // helper units of it and of its closure.
        let mut inputs = Vec::new();
        if let Some(sha256) = log.configuration {
            inputs.push(self.intern(ExternalInput::Configuration { sha256 }));
        }
        inputs.extend(self.read_inputs(log, Some(id)));
        if !self.helper_inputs.is_empty() {
            let reached = self.reached(&rows, id);
            for (sources, input) in &self.helper_inputs {
                if sources
                    .iter()
                    .any(|&source| reached.get(source as usize).copied().unwrap_or(false))
                {
                    inputs.push(*input);
                }
            }
        }
        inputs.sort_unstable();
        inputs.dedup();
        record.external_inputs = inputs;
        self.append(record, rows, macros)
    }

    /// Which namespaces, by id, the namespace `id` with edges `rows`
    /// depends on through compilation edges, itself included, by id up to
    /// `id`.
    fn reached(&self, rows: &[(NamespaceId, EdgeKinds)], id: u32) -> Vec<bool> {
        let mut reached = vec![false; id as usize + 1];
        reached[id as usize] = true;
        let mut stack: Vec<u32> = Vec::new();
        let mut visit = |target: NamespaceId, kinds: EdgeKinds, stack: &mut Vec<u32>| {
            if kinds.intersects(EdgeKinds::COMPILATION)
                && !std::mem::replace(&mut reached[target.0 as usize], true)
            {
                stack.push(target.0);
            }
        };
        for &(target, kinds) in rows {
            visit(target, kinds, &mut stack);
        }
        while let Some(next) = stack.pop() {
            for &(target, kinds) in &self.rows[next as usize] {
                visit(target, kinds, &mut stack);
            }
        }
        reached
    }
}

/// Sort a row by target and merge the kinds of duplicate targets.
fn merge(row: &mut Vec<(NamespaceId, EdgeKinds)>) {
    row.sort_unstable_by_key(|&(target, _)| target.0);
    row.dedup_by(|next, kept| {
        if next.0 == kept.0 {
            kept.1 |= next.1;
            true
        } else {
            false
        }
    });
}
