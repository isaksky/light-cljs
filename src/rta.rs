//! Rapid type analysis over the program's types and protocols: Phase 2 step 4
//! of docs/advanced-optimization.md (light-cljs-211), at `Advanced` only.
//!
//! [`analyze`] walks the prepared program from its root, as emission will, but
//! enters a method body of a `deftype`, `defrecord`, `reify` or `extend-type`
//! only once the method can run: once its type has an instance and the program
//! calls the method. What it finds reachable can make more types instantiated
//! and more methods called, so the walk runs to the least fixed point, the
//! optimistic analysis that keeps neither of two types whose methods only
//! construct each other. Closure keeps every implementation of a slot once
//! its dispatcher is reachable (`RemoveUnusedCode.java:527-559`).
//!
//! # Instances and calls
//!
//! - A program type is *instantiated* when reachable code constructs it
//!   (`new T`, `->T`, `map->T`), or reads its constructor in any other way than
//!   as the class of `instance?` or the target of `extend-type`: a value read
//!   escapes, so code the analysis cannot see may construct it, and so does an
//!   assignment. A `deftype`/`defrecord` whose value is used, and a raw
//!   `deftype*`, count as reads. A `reify` has an instance whenever it is
//!   reached.
//! - A program protocol method is *dispatched* when reachable code reads its
//!   global (calls it or uses it as a value), so the output defines its
//!   dispatcher; it is *called* when it is dispatched or a call of it is
//!   devirtualized (below). Methods are tracked by name, so a called method
//!   keeps all its arities.
//! - A program protocol is *observed* when reachable code reads its global or
//!   tests it with `satisfies?` or `implements?`, which read the marker slots
//!   of its implementations.
//! - Code that names a protocol slot (`cljs$user$P$m$arity$1`, or the marker
//!   `cljs$user$P$`) anywhere in a property, method, component of a `js/`
//!   path, string or `js*` text may read it on any instance: the method
//!   counts as called, the protocol as observed. A text names a method where
//!   it holds `cljs$user$P$m$arity$`, whatever follows, so no character
//!   around a slot hides it.
//!
//! # What stays installed
//!
//! A method entry stays when it can run:
//!
//! - on a program type (`deftype`, `defrecord`, or `extend-type` of a type
//!   only that form defines and nothing assigns), when the type is
//!   instantiated, and for a program protocol's method when it is called, for
//!   its marker when the protocol is observed;
//! - on a `reify` or with `specify!`, when the method is called or the marker
//!   observed;
//! - on anything else (`extend-type` of `string`, `default`, a runtime type),
//!   when the method is dispatched or the marker observed: devirtualized calls
//!   only reach program types.
//!
//! Core protocol methods (`IEquiv`, `ILookup`, `IFn`, `Object`, ...) stay
//! whenever their type is instantiated, since the runtime dispatches them. An
//! `extend-type` whose value is used keeps every method, because it returns
//! the last one.
//!
//! [`TypeAnalysis::apply`] then rewrites the program: dropped entries leave
//! their method lists; a type without instances that nothing reads loses its
//! `deftype` entirely, and one only tested with `instance?` or extended keeps
//! a bare constructor; an `extend-type` that has no method left disappears
//! when it extends a host type name or a type defined earlier at top level,
//! so that the error of extending a type not yet defined stays; a positional
//! or map factory nothing reads is not defined (the positional one since
//! light-cljs-264); a `defprotocol` defines only the protocol and
//! dispatchers something reads; and a top-level definition that preparation
//! would remove once unread goes when only dropped methods read it. Emission
//! gives a type its static `getBasis` only when reachable code names it, and
//! the runtime's types their upstream fields and methods
//! ([`crate::runtime_slice::UPSTREAM_MEMBERS`]) only when it names one.
//! The runtime installs each core protocol method of records where an
//! operation dispatching it is kept; where reachable code names the slot of
//! one ([`crate::runtime_slice::RECORD_METHODS`]) in the ways it can name a
//! program protocol's slot, emission of a record also keeps a unit that the
//! method waits for. Where it names a protocol mask, with which a host value
//! can declare core protocols, or the marker or a method slot of a protocol
//! of [`crate::runtime_slice::PROTOCOL_ROOTS`], emission keeps the units such
//! a value needs ([`TypeAnalysis::named_protocol_declarations`]). Where a
//! program that keeps light.lazy's `Loadable` names what the runtime gives
//! that type or its instances as a deftype's
//! ([`crate::runtime_slice::LOADABLE_MEMBERS`]), emission keeps a unit that
//! the effect adding it waits for ([`TypeAnalysis::named_loadable_members`]).
//!
//! # Devirtualization
//!
//! A call of a program protocol method whose receiver inference types as an
//! instance of one program type ([`Known::Instance`]) that implements the
//! method at the call's arity reads the implementation from the receiver's
//! slot, `r["ns$P$m$arity$2"](r, x)`, exactly as the dispatcher does
//! (`protocolMethod` in `src/js_runtime.js`), instead of calling the
//! dispatcher. It applies when the method is defined once, never assigned,
//! takes no rest arguments and is not extendable through metadata, and both
//! the protocol and the type are defined at top level, the protocol first, so
//! that the dispatcher is defined wherever an instance exists. A later
//! `extend-type` or `specify!` replaces the slot, which the call then reads.
//!
//! # Soundness
//!
//! Every method entry that can run stays: a program protocol method runs only
//! through its dispatcher or a slot read on an instance (devirtualized, or by a
//! name in the program text), and an instance exists only where reachable code
//! made it or a constructor escaped. Protocol slots are compiler-owned names,
//! not host-facing ones, so host code that calls a slot the program does not
//! name is outside the contract, as is raw JavaScript that builds instances
//! from a prototype it was not given.
use std::collections::HashMap;

use crate::analyzer::{ExprId, ExprKind, GlobalId, MethodDispatch, Program};
use crate::core_operations::Type;
use crate::inference::{Definition, Inference, Known};

/// The result of [`analyze`]: what [`TypeAnalysis::apply`] rewrites and
/// emission reads.
pub(crate) struct TypeAnalysis {
    /// Expressions the walk reached, by `ExprId`.
    live: Vec<bool>,
    /// Program types with instances, by `GlobalId` of the constructor.
    instantiated: Vec<bool>,
    /// Globals reachable code reads, by `GlobalId`.
    read: Vec<bool>,
    /// Protocol method globals whose dispatcher reachable code reads.
    dispatched: Vec<bool>,
    /// Method entries that stay, by owner expression (a `DefType`, `Reify` or
    /// `ExtendType`), in method order.
    kept: HashMap<ExprId, Vec<bool>>,
    /// Devirtualized calls and the slot each reads.
    direct: HashMap<ExprId, String>,
    /// `extend-type` forms that do nothing once their methods are gone, by
    /// `ExprId` (see `Walker::quiet_extension`).
    quiet: Vec<bool>,
    /// Expressions whose value emission discards, by `ExprId`.
    discarded: Vec<bool>,
    /// Whether reachable code names `getBasis` (see [`names_property`]).
    basis: bool,
    /// The first reachable expression that names an upstream member of the
    /// runtime's types.
    members: Option<ExprId>,
    /// The units to root for the record methods whose slots reachable code
    /// names, each with the first expression naming one.
    record_methods: Vec<(&'static str, ExprId)>,
    /// The units to root for the core protocol declarations reachable code
    /// names, each with the first expression naming one.
    declarations: Vec<(&'static str, ExprId)>,
    /// The units to root for the members of `Loadable` reachable code names,
    /// each with the first expression naming one.
    loadable_members: Vec<(&'static str, ExprId)>,
}

/// What a rewrite removed, for [`crate::ApplicationStats`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Rewrite {
    pub removed_definitions: usize,
    pub uninstantiated_types: usize,
    pub dropped_methods: usize,
    pub devirtualized_calls: usize,
}

/// What must hold for a method entry to stay, besides its owner being reached.
#[derive(Clone, Copy, Debug)]
enum Then {
    Nothing,
    /// The program protocol method with this dense id is called.
    Called(usize),
    /// ... dispatched.
    Dispatched(usize),
    /// The program protocol with this dense id is observed.
    Observed(usize),
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    owner: ExprId,
    index: usize,
    /// The program type that must be instantiated first.
    instance: Option<GlobalId>,
    then: Then,
}

struct Walker<'p> {
    program: &'p Program,
    inference: &'p Inference,
    /// Top-level definitions preparation may still remove: visited only once
    /// their global is read.
    candidates: &'p [bool],
    /// Dense ids of program protocol method keys and of protocol marker keys.
    method_ids: HashMap<&'p str, usize>,
    protocol_ids: HashMap<String, usize>,
    /// By `GlobalId`: the method id of a protocol method global, the protocol
    /// id of a protocol global, and the program type a constructor or factory
    /// global builds.
    method_of: Vec<Option<usize>>,
    protocol_of: Vec<Option<usize>>,
    type_of: Vec<Option<GlobalId>>,
    /// What every slot of a program protocol method starts with
    /// (`cljs$user$P$m$arity$`), and the marker slots (`cljs$user$P$`), each
    /// with the method (none for a marker) and protocol ids, more than one
    /// pair where names munge alike.
    slots: HashMap<String, Vec<(Option<usize>, usize)>>,
    /// The distinct lengths of the keys of `slots`, ascending.
    slot_lengths: Vec<usize>,

    live: Vec<bool>,
    instantiated: Vec<bool>,
    read: Vec<bool>,
    called: Vec<bool>,
    dispatched: Vec<bool>,
    observed: Vec<bool>,
    kept: HashMap<ExprId, Vec<bool>>,
    direct: HashMap<ExprId, String>,
    quiet: Vec<bool>,
    discarded: Vec<bool>,

    pending: Vec<ExprId>,
    on_instance: Vec<Vec<Entry>>,
    on_called: Vec<Vec<Entry>>,
    on_dispatched: Vec<Vec<Entry>>,
    on_observed: Vec<Vec<Entry>>,
    on_read: Vec<Vec<ExprId>>,
}

/// Run the analysis over `program` as prepared, with its inferred types.
/// `candidates` marks the top-level definitions preparation would remove when
/// nothing reads them (by `ExprId`); `exports` are read from outside.
pub(crate) fn analyze(
    program: &Program,
    inference: &Inference,
    candidates: &[bool],
    exports: &[GlobalId],
) -> TypeAnalysis {
    let globals = program.globals.len();
    let mut method_ids: HashMap<&str, usize> = HashMap::new();
    let mut protocol_ids: HashMap<String, usize> = HashMap::new();
    let mut method_of = vec![None; globals];
    let mut protocol_of = vec![None; globals];
    let mut type_of = vec![None; globals];
    let mut slots: HashMap<String, Vec<(Option<usize>, usize)>> = HashMap::new();
    let mut raw_types = Vec::new();
    for expr in &program.exprs {
        match &expr.kind {
            ExprKind::DefProtocol {
                global, methods, ..
            } => {
                let binding = &program.globals[global.0];
                let marker = format!("{}/{}", binding.namespace, binding.name);
                let next = protocol_ids.len();
                let protocol = *protocol_ids.entry(marker.clone()).or_insert(next);
                protocol_of[global.0] = Some(protocol);
                slots
                    .entry(protocol_slot(&marker, 0))
                    .or_default()
                    .push((None, protocol));
                for method in methods {
                    let next = method_ids.len();
                    let id = *method_ids.entry(method.key.as_str()).or_insert(next);
                    method_of[method.global.0] = Some(id);
                    let mut prefix = protocol_slot(&method.key, 1);
                    prefix.pop();
                    slots.entry(prefix).or_default().push((Some(id), protocol));
                }
            }
            ExprKind::DefType {
                global,
                constructor,
                map_constructor,
                ..
            } => {
                for g in std::iter::once(global)
                    .chain(constructor)
                    .chain(map_constructor)
                {
                    type_of[g.0] = Some(*global);
                }
                if constructor.is_none() {
                    raw_types.push(*global);
                }
            }
            _ => (),
        }
    }
    let methods = method_ids.len();
    let protocols = protocol_ids.len();
    let mut slot_lengths: Vec<usize> = slots.keys().map(String::len).collect();
    slot_lengths.sort_unstable();
    slot_lengths.dedup();
    // Emission discards the value of every form of a `do` but the last
    // (the expression's own `context` is how analysis saw it, which for a
    // top-level form is `Return`).
    let mut discarded = vec![false; program.exprs.len()];
    let mut discarded_dos = Vec::new();
    for expr in &program.exprs {
        if let ExprKind::Do(items) = &expr.kind
            && let Some((_, init)) = items.split_last()
        {
            for item in init {
                discarded[item.0] = true;
                discarded_dos.push(*item);
            }
        }
    }
    // ... and of the last form of a `do` whose value it discards.
    while let Some(id) = discarded_dos.pop() {
        if let ExprKind::Do(items) = &program.exprs[id.0].kind
            && let Some(last) = items.last()
            && !std::mem::replace(&mut discarded[last.0], true)
        {
            discarded_dos.push(*last);
        }
    }
    let mut walker = Walker {
        program,
        inference,
        candidates,
        method_ids,
        protocol_ids,
        method_of,
        protocol_of,
        type_of,
        slots,
        slot_lengths,
        live: vec![false; program.exprs.len()],
        instantiated: vec![false; globals],
        read: vec![false; globals],
        called: vec![false; methods],
        dispatched: vec![false; methods],
        observed: vec![false; protocols],
        kept: HashMap::new(),
        direct: HashMap::new(),
        quiet: vec![false; program.exprs.len()],
        discarded,
        pending: vec![program.root],
        on_instance: vec![Vec::new(); globals],
        on_called: vec![Vec::new(); methods],
        on_dispatched: vec![Vec::new(); methods],
        on_observed: vec![Vec::new(); protocols],
        on_read: vec![Vec::new(); globals],
    };
    // A raw `deftype*` builds its type with code the analysis does not follow.
    for global in raw_types {
        walker.instantiate(global);
    }
    for global in exports {
        walker.read_global(*global, false);
    }
    walker.run();
    let Walker {
        live,
        instantiated,
        read,
        dispatched,
        method_of,
        kept,
        direct,
        quiet,
        discarded,
        ..
    } = walker;
    let dispatched = method_of
        .iter()
        .map(|method| method.is_some_and(|id| dispatched[id]))
        .collect();
    let basis = program.exprs.iter().zip(&live).any(|(expr, live)| {
        *live && names_property(program, &expr.kind, |name| name == "getBasis")
    });
    let members = program
        .exprs
        .iter()
        .zip(&live)
        .position(|(expr, live)| {
            *live
                && names_property(program, &expr.kind, |name| {
                    crate::runtime_slice::UPSTREAM_MEMBERS.contains(&name)
                })
        })
        .map(ExprId);
    let record_methods = named_record_methods(program, &live);
    let declarations = named_protocol_declarations(program, &live);
    let loadable_members = named_loadable_members(program, &live);
    TypeAnalysis {
        live,
        instantiated,
        read,
        dispatched,
        kept,
        direct,
        quiet,
        discarded,
        basis,
        members,
        record_methods,
        declarations,
        loadable_members,
    }
}

impl Walker<'_> {
    fn run(&mut self) {
        while let Some(id) = self.pending.pop() {
            self.visit(id);
        }
    }

    fn visit(&mut self, id: ExprId) {
        let program = self.program;
        let expr = &program.exprs[id.0];
        // Global reads are leaves; each read counts, wherever it occurs.
        if let ExprKind::Global(global) = expr.kind {
            self.live[id.0] = true;
            self.read_global(global, false);
            return;
        }
        if let ExprKind::Def { global, .. } = expr.kind
            && self.candidates.get(id.0).copied().unwrap_or(false)
            && !self.read[global.0]
        {
            self.on_read[global.0].push(id);
            return;
        }
        if std::mem::replace(&mut self.live[id.0], true) {
            return;
        }
        self.named_slots(&expr.kind);
        match &expr.kind {
            ExprKind::DefType {
                global,
                body,
                masks,
                methods,
                ..
            } => {
                self.pending.extend(body.iter().chain(masks));
                // The value of the form is the type, which then escapes.
                if !self.discarded[id.0] {
                    self.read_global(*global, false);
                }
                self.entries(id, methods, Some(*global), true);
            }
            ExprKind::Reify {
                metadata, methods, ..
            } => {
                self.pending.extend(metadata);
                self.entries(id, methods, None, true);
            }
            ExprKind::ExtendType {
                instance,
                return_target,
                target,
                methods,
            } => {
                // An `extend-type` whose value is used returns its last method.
                let all = !self.discarded[id.0] && !*return_target;
                // The type of a program `deftype` or `defrecord` that nothing
                // else defines or assigns: then the methods are its methods.
                let program_type = match program.exprs[target.0].kind {
                    ExprKind::Global(global) if !*instance => self.type_of[global.0].filter(|t| {
                        *t == global && self.inference.facts().program_type(global).is_some()
                    }),
                    _ => None,
                };
                match program_type {
                    // `apply` leaves out the extension of a type defined
                    // earlier once its methods are gone, and with it the read.
                    Some(global) if !all && self.quiet_extension(id, global) => {
                        self.live[target.0] = true;
                        self.quiet[id.0] = true;
                    }
                    Some(global) => {
                        self.live[target.0] = true;
                        self.read_global(global, true);
                    }
                    None => {
                        // A host type name: without methods, nothing to do.
                        self.quiet[id.0] = !all
                            && !*instance
                            && matches!(program.exprs[target.0].kind, ExprKind::String(_));
                        self.pending.push(*target);
                    }
                }
                if all {
                    // Also when there are none to keep.
                    self.kept
                        .entry(id)
                        .or_insert_with(|| vec![false; methods.len()]);
                    for (index, _) in methods.iter().enumerate() {
                        self.activate(Entry {
                            owner: id,
                            index,
                            instance: None,
                            then: Then::Nothing,
                        });
                    }
                } else {
                    self.entries(
                        id,
                        methods,
                        program_type,
                        program_type.is_some() || *instance,
                    );
                }
            }
            ExprKind::InstanceOf { class, value } => {
                match program.exprs[class.0].kind {
                    ExprKind::Global(global)
                        if self.type_of[global.0].is_some_and(|t| t == global) =>
                    {
                        self.live[class.0] = true;
                        self.read_global(global, true);
                    }
                    _ => self.pending.push(*class),
                }
                self.pending.push(*value);
            }
            ExprKind::ProtocolPredicate {
                protocol,
                value,
                key,
                direct,
            } => {
                if let Some(&protocol) = self.protocol_ids.get(key.as_str()) {
                    self.observe(protocol);
                }
                if !*direct {
                    self.pending.push(*protocol);
                }
                self.pending.push(*value);
            }
            ExprKind::Call { callee, args } => {
                if let Some((method, slot)) = self.devirtualize(id) {
                    self.call(method);
                    self.direct.insert(id, slot);
                } else {
                    self.pending.push(*callee);
                }
                self.pending.extend(args);
            }
            _ => self
                .pending
                .extend(crate::program_fragment::children(program, id)),
        }
    }

    /// Register the method entries of `owner`. `instance` is the program type
    /// whose instances the methods belong to, if any; `called` says whether a
    /// program protocol method needs to be called (a slot an instance of a
    /// program type may be devirtualized to) or dispatched.
    fn entries(
        &mut self,
        owner: ExprId,
        methods: &[(MethodDispatch, crate::analyzer::FnMethod)],
        instance: Option<GlobalId>,
        called: bool,
    ) {
        self.kept
            .entry(owner)
            .or_insert_with(|| vec![false; methods.len()]);
        for (index, (dispatch, method)) in methods.iter().enumerate() {
            let then = match dispatch {
                MethodDispatch::Protocol(key) => match self.method_ids.get(key.as_str()) {
                    Some(&id) if called => Then::Called(id),
                    Some(&id) => Then::Dispatched(id),
                    None => Then::Nothing,
                },
                MethodDispatch::Builtin(key)
                    if method.params.is_empty() && method.rest.is_none() =>
                {
                    match self.protocol_ids.get(key.as_str()) {
                        Some(&id) => Then::Observed(id),
                        None => Then::Nothing,
                    }
                }
                MethodDispatch::Builtin(_) => Then::Nothing,
            };
            self.require(Entry {
                owner,
                index,
                instance,
                then,
            });
        }
    }

    fn require(&mut self, entry: Entry) {
        if let Some(global) = entry.instance
            && !self.instantiated[global.0]
        {
            self.on_instance[global.0].push(entry);
            return;
        }
        match entry.then {
            Then::Called(id) if !self.called[id] => self.on_called[id].push(entry),
            Then::Dispatched(id) if !self.dispatched[id] => self.on_dispatched[id].push(entry),
            Then::Observed(id) if !self.observed[id] => self.on_observed[id].push(entry),
            _ => self.activate(entry),
        }
    }

    fn activate(&mut self, entry: Entry) {
        let program = self.program;
        let methods = match &program.exprs[entry.owner.0].kind {
            ExprKind::DefType { methods, .. }
            | ExprKind::Reify { methods, .. }
            | ExprKind::ExtendType { methods, .. } => methods,
            _ => unreachable!("method entries belong to types, reify and extensions"),
        };
        let kept = self
            .kept
            .entry(entry.owner)
            .or_insert_with(|| vec![false; methods.len()]);
        if !std::mem::replace(&mut kept[entry.index], true) {
            self.pending.push(methods[entry.index].1.body);
        }
    }

    /// A read of `global`; `benign` for the class of `instance?` and the
    /// target of `extend-type`, which use a program type without making one.
    fn read_global(&mut self, global: GlobalId, benign: bool) {
        if !std::mem::replace(&mut self.read[global.0], true) {
            let deferred = std::mem::take(&mut self.on_read[global.0]);
            self.pending.extend(deferred);
        }
        if let Some(program_type) = self.type_of[global.0]
            && (program_type != global || !benign)
        {
            self.instantiate(program_type);
        }
        if let Some(method) = self.method_of[global.0] {
            self.call(method);
            if !std::mem::replace(&mut self.dispatched[method], true) {
                for entry in std::mem::take(&mut self.on_dispatched[method]) {
                    self.require(entry);
                }
            }
        }
        if let Some(protocol) = self.protocol_of[global.0] {
            self.observe(protocol);
        }
    }

    /// A protocol slot the program names anywhere in one of the [`texts`] of
    /// an expression may be read on any instance: its method counts as
    /// called, and a marker's protocol as observed. Every key of `slots` ends
    /// in `$`, so each `$` of a text ends the keys the text holds there.
    fn named_slots(&mut self, kind: &ExprKind) {
        if self.slots.is_empty() {
            return;
        }
        let (slots, lengths) = (&self.slots, &self.slot_lengths);
        let mut named = Vec::new();
        texts(self.program, kind, |text| {
            for (position, _) in text.match_indices('$') {
                let end = position + 1;
                for &length in lengths.iter().take_while(|&&length| length <= end) {
                    let key = text.get(end - length..end);
                    if let Some(found) = key.and_then(|key| slots.get(key)) {
                        named.extend_from_slice(found);
                    }
                }
            }
        });
        for (method, protocol) in named {
            self.observe(protocol);
            if let Some(method) = method {
                self.call(method);
            }
        }
    }

    fn instantiate(&mut self, global: GlobalId) {
        if !std::mem::replace(&mut self.instantiated[global.0], true) {
            for entry in std::mem::take(&mut self.on_instance[global.0]) {
                self.require(entry);
            }
        }
    }

    fn call(&mut self, method: usize) {
        if !std::mem::replace(&mut self.called[method], true) {
            for entry in std::mem::take(&mut self.on_called[method]) {
                self.require(entry);
            }
        }
    }

    fn observe(&mut self, protocol: usize) {
        if !std::mem::replace(&mut self.observed[protocol], true) {
            for entry in std::mem::take(&mut self.on_observed[protocol]) {
                self.require(entry);
            }
        }
    }

    /// Whether the `extend-type` `extension` of the program type `global` runs
    /// after the type's only definition: both are top-level forms, the
    /// type's first. Without methods it then does nothing, so leaving it out
    /// cannot hide the error of extending a type not yet defined.
    fn quiet_extension(&self, extension: ExprId, global: GlobalId) -> bool {
        let facts = self.inference.facts();
        let Some((definition, _)) = facts.program_type(global) else {
            return false;
        };
        matches!(
            self.program.exprs[definition.0].kind,
            ExprKind::DefType {
                constructor: Some(_),
                ..
            }
        ) && matches!(
            (facts.top_position(definition), facts.top_position(extension)),
            (Some(definition), Some(extension)) if definition < extension
        )
    }

    /// The method id and receiver slot of a call that can read the
    /// implementation from its receiver (see the module docs).
    fn devirtualize(&self, call: ExprId) -> Option<(usize, String)> {
        let program = self.program;
        let ExprKind::Call { callee, args } = &program.exprs[call.0].kind else {
            return None;
        };
        let ExprKind::Global(global) = program.exprs[callee.0].kind else {
            return None;
        };
        let method_id = self.method_of[global.0]?;
        let facts = self.inference.facts();
        let Definition::Protocol(protocol) = facts.definition(global) else {
            return None;
        };
        let ExprKind::DefProtocol {
            global: protocol_global,
            methods,
            via_metadata: false,
        } = &program.exprs[protocol.0].kind
        else {
            return None;
        };
        let method = methods.iter().find(|method| method.global == global)?;
        if *protocol_global == global
            || method.variadic.is_some()
            || !method.arities.contains(&args.len())
        {
            return None;
        }
        let receiver = self.inference.value(*args.first()?);
        let Known::Instance(program_type) = receiver.known else {
            return None;
        };
        if receiver.ty.is_never() || !Type::RECORD.union(Type::OBJECT).contains(receiver.ty) {
            return None;
        }
        let (definition, _) = facts.program_type(program_type)?;
        let ExprKind::DefType {
            methods: implementations,
            ..
        } = &program.exprs[definition.0].kind
        else {
            return None;
        };
        let implemented = implementations.iter().any(|(dispatch, implementation)| {
            matches!(dispatch, MethodDispatch::Protocol(key) if *key == method.key)
                && implementation.rest.is_none()
                && implementation.params.len() == args.len()
        });
        let ordered = matches!(
            (facts.top_position(protocol), facts.top_position(definition)),
            (Some(protocol), Some(definition)) if protocol < definition
        );
        (implemented && ordered).then(|| (method_id, protocol_slot(&method.key, args.len())))
    }
}

impl TypeAnalysis {
    /// The slot a devirtualized call reads from its receiver.
    pub(crate) fn direct_calls(&self) -> &HashMap<ExprId, String> {
        &self.direct
    }

    /// Whether reachable code names `getBasis`, the static method of
    /// `deftype` and `defrecord` types. Only then can it be called, as Closure
    /// assumes when it removes properties nothing reads.
    pub(crate) fn names_basis(&self) -> bool {
        self.basis
    }

    /// The first reachable expression that names an upstream field or method
    /// of the runtime's types ([`crate::runtime_slice::UPSTREAM_MEMBERS`]),
    /// which only then can read one.
    pub(crate) fn names_upstream_member(&self) -> Option<ExprId> {
        self.members
    }

    /// The units an `Advanced` slice of a program with records roots so that
    /// the runtime installs the record methods whose slots reachable code
    /// names ([`crate::runtime_slice::RECORD_METHODS`]), each with the first
    /// expression naming one of its slots.
    pub(crate) fn named_record_methods(&self) -> &[(&'static str, ExprId)] {
        &self.record_methods
    }

    /// The units an `Advanced` slice roots because reachable code names a core
    /// protocol declaration, each with the first expression naming it. A host
    /// value the program builds can declare core protocols by a protocol mask
    /// (`cljs$lang$protocol_mask$partition0$`), which then needs the protocol
    /// hooks that `installMethods`' effects install and every unit of
    /// [`crate::runtime_slice::PROTOCOL_ROOTS`], and by the marker of one of
    /// those protocols, which naming it or one of its method slots
    /// (`cljs$core$ISeqable$`) roots. A name computed at run time does not
    /// count, as for protocol slots.
    pub(crate) fn named_protocol_declarations(&self) -> &[(&'static str, ExprId)] {
        &self.declarations
    }

    /// The units an `Advanced` slice of a program that keeps light.lazy's
    /// `Loadable` roots because reachable code names what the runtime gives
    /// the type or its instances as a deftype's
    /// ([`crate::runtime_slice::LOADABLE_MEMBERS`]): its IDeref marker or a
    /// slot, a protocol mask, `cljs$lang$type` or `cljs$lang$ctorStr`. Each
    /// comes with the first expression naming it. The effects adding these
    /// otherwise wait for operations observing them, which a direct read such
    /// as `(.cljs$core$IDeref$_deref$arity$1 l l)` does not keep.
    pub(crate) fn named_loadable_members(&self) -> &[(&'static str, ExprId)] {
        &self.loadable_members
    }

    /// Rewrite `program` as the analysis decided (see the module docs) and
    /// update `retained`, the globals emission declares.
    pub(crate) fn apply(
        &self,
        program: &mut Program,
        retained: &mut [bool],
        candidates: &[bool],
    ) -> Rewrite {
        let mut rewrite = Rewrite {
            devirtualized_calls: self.direct.len(),
            ..Rewrite::default()
        };
        // Preparation retains every global its walk saw, including those that
        // only dropped method bodies read; start over from what is reachable.
        let mut keep = vec![false; program.globals.len()];
        for (global, read) in self.read.iter().enumerate() {
            keep[global] |= *read;
        }
        for index in 0..program.exprs.len() {
            let id = ExprId(index);
            if candidates.get(index).copied().unwrap_or(false) && !self.live[index] {
                // A definition only dropped methods read.
                if !matches!(program.exprs[index].kind, ExprKind::Nil) {
                    program.exprs[index].kind = ExprKind::Nil;
                    rewrite.removed_definitions += 1;
                }
                continue;
            }
            if !self.live[index] {
                continue;
            }
            let discard = self.discarded[index];
            match &mut program.exprs[index].kind {
                ExprKind::Def { global, .. } => keep[global.0] = true,
                ExprKind::DefType {
                    record,
                    global,
                    constructor,
                    body,
                    masks,
                    map_constructor,
                    methods,
                    ..
                } => {
                    if constructor.is_some() && !self.instantiated[global.0] {
                        rewrite.uninstantiated_types += 1;
                        rewrite.dropped_methods += methods.len();
                        if !self.read[global.0] && discard && body.is_none() && masks.is_none() {
                            program.exprs[index].kind = ExprKind::Nil;
                            continue;
                        }
                        // Only `instance?` and `extend-type` read it: a bare
                        // constructor without methods or factories.
                        *record = false;
                        *constructor = None;
                        *map_constructor = None;
                        methods.clear();
                    } else {
                        rewrite.dropped_methods += retain(methods, &self.kept[&id]);
                        // `map->T` builds through `recordFromMap`, which keeps
                        // the generic core: define it only when something reads it.
                        if map_constructor.is_some_and(|factory| !self.read[factory.0]) {
                            *map_constructor = None;
                        }
                    }
                    // A factory stays where something reads it, as `read`
                    // keeps it, and emission defines only the kept ones
                    // (light-cljs-264).
                    keep[global.0] = true;
                }
                ExprKind::Reify { methods, .. } => {
                    rewrite.dropped_methods += retain(methods, &self.kept[&id]);
                }
                ExprKind::ExtendType { methods, .. } => {
                    rewrite.dropped_methods += retain(methods, &self.kept[&id]);
                    if methods.is_empty() && discard && self.quiet[index] {
                        program.exprs[index].kind = ExprKind::Nil;
                    }
                }
                ExprKind::DefProtocol {
                    global, methods, ..
                } => {
                    let protocol = *global;
                    methods.retain(|method| self.dispatched[method.global.0]);
                    for method in methods.iter() {
                        keep[method.global.0] = true;
                    }
                    let unread = !self.read[protocol.0];
                    if unread && methods.is_empty() && discard {
                        program.exprs[index].kind = ExprKind::Nil;
                    }
                }
                _ => (),
            }
        }
        for (global, keep) in keep.iter().enumerate() {
            retained[global] = *keep;
        }
        rewrite
    }
}

/// Call `text` with each text in which an expression of this kind may name a
/// host property: the property it reads, the method it calls, each component
/// of the path of a JavaScript global, a string, and raw JavaScript. A name
/// computed at run time does not count.
fn texts(program: &Program, kind: &ExprKind, mut text: impl FnMut(&str)) {
    match kind {
        ExprKind::Property { property: name, .. }
        | ExprKind::MethodCall { method: name, .. }
        | ExprKind::QualifiedMethod {
            method: Some(name), ..
        } => text(name),
        ExprKind::JsGlobal(global) => program.js_globals[global.0]
            .path
            .iter()
            .for_each(|name| text(name)),
        ExprKind::JsTemplate { segments, .. } => segments.iter().for_each(|segment| text(segment)),
        ExprKind::String(string) => text(&String::from_utf16_lossy(&program.strings[string.0])),
        _ => (),
    }
}

/// Whether an expression of this kind names a host property that `named`
/// accepts, an identifier such as `getBasis`: one of its [`texts`] is the
/// name, or raw JavaScript holds it as an identifier or a string.
fn names_property(program: &Program, kind: &ExprKind, mut named: impl FnMut(&str) -> bool) -> bool {
    match kind {
        ExprKind::JsTemplate { segments, .. } => segments.iter().any(|segment| {
            segment
                .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
                .any(&mut named)
        }),
        _ => {
            let mut found = false;
            texts(program, kind, |text| found = found || named(text));
            found
        }
    }
}

/// For a program with records, the [`crate::runtime_slice::RECORD_METHODS`]
/// whose slots a reachable expression names anywhere in one of its [`texts`],
/// as the unit to root and the first such expression, in table order.
fn named_record_methods(program: &Program, live: &[bool]) -> Vec<(&'static str, ExprId)> {
    let reached = || {
        program
            .exprs
            .iter()
            .enumerate()
            .filter(|(index, _)| live[*index])
    };
    let records = reached().any(|(_, expr)| {
        matches!(
            expr.kind,
            ExprKind::DefType {
                record: true,
                constructor: Some(_),
                ..
            }
        )
    });
    if !records {
        return Vec::new();
    }
    let methods = crate::runtime_slice::RECORD_METHODS;
    let slots: Vec<(String, usize)> = methods
        .iter()
        .enumerate()
        .flat_map(|(method, (key, arities, _))| {
            arities
                .iter()
                .map(move |&arity| (protocol_slot(key, arity), method))
        })
        .collect();
    let mut first = vec![None; methods.len()];
    for (index, expr) in reached() {
        texts(program, &expr.kind, |text| {
            if !text.contains('$') {
                return;
            }
            for (slot, method) in &slots {
                if text.contains(slot.as_str()) {
                    first[*method].get_or_insert(ExprId(index));
                }
            }
        });
    }
    methods
        .iter()
        .zip(first)
        .filter_map(|((_, _, unit), site)| Some((*unit, site?)))
        .collect()
}

/// See [`TypeAnalysis::named_protocol_declarations`]: the units to root, in
/// [`crate::runtime_slice::PROTOCOL_ROOTS`] order and then `installMethods`,
/// each with the first reachable expression naming it anywhere in one of its
/// [`texts`]: a mask, or the marker (`cljs$core$ISeqable$`) with which every
/// slot of the protocol starts.
fn named_protocol_declarations(program: &Program, live: &[bool]) -> Vec<(&'static str, ExprId)> {
    const MASK: &str = "cljs$lang$protocol_mask$partition";
    let roots = crate::runtime_slice::PROTOCOL_ROOTS;
    let markers: Vec<String> = roots.iter().map(|root| protocol_slot(root, 0)).collect();
    let mut first = vec![None; roots.len() + 1];
    for (index, expr) in program.exprs.iter().enumerate() {
        if !live[index] {
            continue;
        }
        texts(program, &expr.kind, |text| {
            let mask = text.contains(MASK);
            for (unit, site) in first.iter_mut().enumerate() {
                let declares = markers
                    .get(unit)
                    .is_some_and(|marker| text.contains(marker.as_str()));
                if mask || declares {
                    site.get_or_insert(ExprId(index));
                }
            }
        });
    }
    roots
        .iter()
        .copied()
        .chain(["installMethods"])
        .zip(first)
        .filter_map(|(unit, site)| Some((unit, site?)))
        .collect()
}

/// See [`TypeAnalysis::named_loadable_members`]: for a program in which a
/// reachable expression reads the runtime's `Loadable`
/// ([`ExprKind::LoadableType`]), the units of the
/// [`crate::runtime_slice::LOADABLE_MEMBERS`] whose names a reachable
/// expression holds anywhere in one of its [`texts`], each with the first
/// such expression, in table order.
fn named_loadable_members(program: &Program, live: &[bool]) -> Vec<(&'static str, ExprId)> {
    let reached = || {
        program
            .exprs
            .iter()
            .enumerate()
            .filter(|(index, _)| live[*index])
    };
    if !reached().any(|(_, expr)| matches!(expr.kind, ExprKind::LoadableType)) {
        return Vec::new();
    }
    let members = crate::runtime_slice::LOADABLE_MEMBERS;
    let mut first = vec![None; members.len()];
    for (index, expr) in reached() {
        texts(program, &expr.kind, |text| {
            for ((names, _), site) in members.iter().zip(&mut first) {
                if names.iter().any(|name| text.contains(name)) {
                    site.get_or_insert(ExprId(index));
                }
            }
        });
    }
    members
        .iter()
        .zip(first)
        .filter_map(|((_, unit), site)| Some((*unit, site?)))
        .collect()
}

/// Keep the method entries `kept` marks; how many were dropped.
fn retain<T>(methods: &mut Vec<T>, kept: &[bool]) -> usize {
    let before = methods.len();
    let mut index = 0;
    methods.retain(|_| {
        index += 1;
        kept[index - 1]
    });
    before - methods.len()
}

/// The property holding a protocol method's implementation of `arity`
/// (receiver included) on an instance: `protocolSlot` in `src/js_runtime.js`,
/// whose munging is [`crate::analyzer::munge_js_member`].
pub(crate) fn protocol_slot(key: &str, arity: usize) -> String {
    let munge = crate::analyzer::munge_js_member;
    let slash = key.find('/');
    let qualified = match slash {
        Some(slash) if arity > 0 => key[slash + 1..].contains('/'),
        Some(_) => true,
        None => false,
    };
    let (namespace, remainder) = match slash {
        Some(slash) if qualified => (&key[..slash], &key[slash + 1..]),
        _ => ("cljs.core", key),
    };
    let separator = if remainder.starts_with("//") {
        Some(1)
    } else {
        remainder.find('/')
    };
    let prefix = format!(
        "{}${}$",
        munge(namespace).replace('.', "$"),
        munge(if arity == 0 {
            remainder
        } else {
            separator.map_or(remainder, |separator| &remainder[..separator])
        })
    );
    if arity == 0 {
        return prefix;
    }
    let method = separator.map_or(remainder, |separator| &remainder[separator + 1..]);
    let method = if method == "/" {
        "_SLASH_".to_owned()
    } else {
        munge(method)
    };
    format!("{prefix}{method}$arity${arity}")
}

#[cfg(test)]
mod tests {
    use super::protocol_slot;

    #[test]
    fn protocol_slots_follow_the_runtime() {
        // Expected values are what `protocolSlot` in src/js_runtime.js returns;
        // `rta::devirtualized_calls_read_the_runtime_slot` in tests/compiler
        // checks the runtime itself.
        assert_eq!(
            protocol_slot("cljs.user/Shape/area", 1),
            "cljs$user$Shape$area$arity$1"
        );
        assert_eq!(protocol_slot("my.ns/P/-m?", 3), "my$ns$P$_m_QMARK_$arity$3");
        assert_eq!(protocol_slot("my.ns/P//", 2), "my$ns$P$_SLASH_$arity$2");
        assert_eq!(
            protocol_slot("ISeq/-first", 1),
            "cljs$core$ISeq$_first$arity$1"
        );
        assert_eq!(protocol_slot("my.ns/P", 0), "my$ns$P$");
        assert_eq!(protocol_slot("ISeq", 0), "cljs$core$ISeq$");
    }
}
