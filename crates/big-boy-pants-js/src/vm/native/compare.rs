//! Exact comparison messages, built in bounded, byte-accounted VM steps.
//! The operands remain roots until formatting finishes. A shared form DAG
//! may describe exponentially much text; no collection snapshot or expanded
//! string is allocated before admission against the aggregate heap budget.
use super::{Action, Fault};
use crate::collections::form::{self, FormBody, FormMap};
use crate::collections::{ComparisonOperands, PVector};
use crate::heap::Heap;
use crate::heap::budget::{ByteReservation, GrowthCharge, reserve_elements};
use crate::heap::gc::{Trace, TraceEdge};
use crate::value::string::{MAX_STRING_LEN, StringFlatten};
use crate::value::{JsString, Value};
use crate::vm::{frames::VmState, property};
use std::collections::{BTreeMap, btree_map};

type Order = (bool, [u8; 7], usize);
type Entries = BTreeMap<Order, (Value, Value)>;

const STRING_HASH_STEP_UNITS: usize = 128;
const NAMESPACE_STEP_UNITS: usize = 128;

#[derive(Clone, Copy, Debug)]
struct Context {
    nested: bool,
    level: Option<f64>,
}

impl Context {
    fn collection(self) -> Option<Self> {
        let level = self.level.map(|level| level - 1.0);
        if level.is_some_and(|level| level < 0.0) {
            None
        } else {
            Some(Self {
                nested: true,
                level,
            })
        }
    }
}

#[derive(Debug, Default)]
struct Options {
    length: Option<f64>,
    level: Option<f64>,
    namespace_maps: bool,
}

impl Options {
    fn for_task(heap: &Heap, state: &VmState) -> Result<Self, Fault> {
        let Some(global) = state.global_object else {
            return Ok(Self::default());
        };
        let get = |object: &Value, name: &str| {
            property::get(
                heap,
                state.diag.task,
                object,
                &property::Key::Name(JsString::from(name)),
            )
            .map(|value| form::unwrap(heap, value))
        };
        // The compiler's runtime and its dynamic printing bindings belong to
        // this task. Engine-only callers keep the ordinary printer defaults.
        let runtime = get(&Value::Object(global), "__lightNativeRuntime")?;
        if !matches!(runtime, Value::Object(_)) {
            return Ok(Self::default());
        }
        let core = get(&runtime, "core")?;
        if !matches!(core, Value::Object(_)) {
            return Ok(Self::default());
        }
        let number = |value| {
            if let Value::Number(n) = value {
                Some(n)
            } else {
                None
            }
        };
        let truth = |value| !matches!(value, Value::Null | Value::Undefined | Value::Bool(false));
        Ok(Self {
            length: number(get(&core, "printLength")?),
            level: number(get(&core, "printLevel")?),
            namespace_maps: truth(get(&core, "printNamespaceMaps")?),
        })
    }
}

#[derive(Debug)]
enum Namespace {
    Invalid,
    First,
    Observed(Option<JsString>),
    Comparing(Box<NamespaceComparison>),
}

impl Namespace {
    fn new(enabled: bool) -> Self {
        if enabled { Self::First } else { Self::Invalid }
    }

    fn observe(&mut self, heap: &Heap, key: &Value) -> Result<(), Fault> {
        if matches!(self, Self::Invalid) {
            return Ok(());
        }
        let key = form::unwrap(heap, key.clone());
        let object = form::form_object(heap, &key);
        let Some(FormBody::Keyword(name) | FormBody::Symbol(name)) =
            object.as_deref().map(|f| f.body())
        else {
            *self = Self::Invalid;
            return Ok(());
        };
        match self {
            Self::First => *self = Self::Observed(name.namespace.clone()),
            Self::Observed(current) => match (current.as_ref(), name.namespace.as_ref()) {
                (Some(left), Some(right)) if left.same_body(right) => (),
                (Some(left), Some(right)) if left.len() == right.len() => {
                    *self = Self::Comparing(NamespaceComparison::new(heap, left, right)?);
                }
                (None, None) => (),
                _ => *self = Self::Invalid,
            },
            _ => unreachable!("finish pending namespace comparisons before observing another key"),
        }
        Ok(())
    }

    fn advance(&mut self) {
        let Self::Comparing(comparing) = self else {
            unreachable!("namespace comparison is pending");
        };
        if let Some(equal) = comparing.advance() {
            *self = if equal {
                Self::Observed(Some(comparing.name.clone()))
            } else {
                Self::Invalid
            };
        }
    }

    fn finish(self) -> Option<JsString> {
        match self {
            Self::Observed(Some(name)) if !name.is_empty() => Some(name),
            _ => None,
        }
    }
}

#[cfg(test)]
thread_local! {
    static HASHED_STRING_UNITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static COMPARED_NAMESPACE_UNITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Debug)]
struct NamespaceComparison {
    // Keep the observed spelling while the enum holds only this pointer.
    name: JsString,
    flatten: [Option<StringFlatten>; 2],
    text: [Option<JsString>; 2],
    index: usize,
    _charge: ByteReservation,
}

impl NamespaceComparison {
    fn new(heap: &Heap, left: &JsString, right: &JsString) -> Result<Box<Self>, Fault> {
        let charge = reserve_elements(heap, 1, std::mem::size_of::<Self>())?;
        Ok(Box::new(Self {
            name: left.clone(),
            flatten: [
                Some(StringFlatten::new_accounted(heap, left.clone())?),
                Some(StringFlatten::new_accounted(heap, right.clone())?),
            ],
            text: [None, None],
            index: 0,
            _charge: charge,
        }))
    }

    fn advance(&mut self) -> Option<bool> {
        for side in 0..2 {
            if let Some(flatten) = &mut self.flatten[side] {
                self.text[side] = flatten.advance(NAMESPACE_STEP_UNITS);
                if self.text[side].is_some() {
                    self.flatten[side] = None;
                }
                // Flatten each operand and compare in separate VM steps.
                return None;
            }
        }
        let [Some(left), Some(right)] = &self.text else {
            unreachable!("namespace operands have been flattened");
        };
        let end = (self.index + NAMESPACE_STEP_UNITS).min(left.len());
        for index in self.index..end {
            #[cfg(test)]
            COMPARED_NAMESPACE_UNITS.with(|count| count.set(count.get() + 1));
            if left.code_units()[index] != right.code_units()[index] {
                return Some(false);
            }
        }
        self.index = end;
        (end == left.len()).then_some(true)
    }
}

#[derive(Debug)]
struct StringKey {
    pair: (Value, Value),
    flatten: Option<StringFlatten>,
    text: Option<JsString>,
    index: usize,
    hash: i32,
    _charge: ByteReservation,
}

impl StringKey {
    fn new(heap: &Heap, pair: (Value, Value), text: JsString) -> Result<Box<Self>, Fault> {
        // Keep the optional hash continuation out of every traversal slot.
        let charge = reserve_elements(heap, 1, std::mem::size_of::<Self>())?;
        Ok(Box::new(Self {
            pair,
            flatten: Some(StringFlatten::new_accounted(heap, text)?),
            text: None,
            index: 0,
            hash: 0,
            _charge: charge,
        }))
    }

    fn advance(&mut self) -> Option<u32> {
        if let Some(flatten) = &mut self.flatten {
            self.text = flatten.advance(STRING_HASH_STEP_UNITS);
            self.text.as_ref()?;
            self.flatten = None;
            // Flattening and hashing each get their own bounded VM steps.
            return None;
        }
        let text = self.text.as_ref().unwrap();
        let end = (self.index + STRING_HASH_STEP_UNITS).min(text.len());
        // StringFlatten supplies a flat leaf, never a synchronous rope walk.
        for &unit in &text.code_units()[self.index..end] {
            self.hash = self.hash.wrapping_mul(31).wrapping_add(i32::from(unit));
            #[cfg(test)]
            HASHED_STRING_UNITS.with(|count| count.set(count.get() + 1));
        }
        self.index = end;
        if end < text.len() {
            return None;
        }
        // Finish scalar_hash's hash_int(string_hash(text)) (collections::form).
        // Keep this constant-time finalizer here so scans need no synchronous
        // string hash and no changes to the collection hashing API.
        let mut h = self.hash as u32;
        if h != 0 {
            let k = h
                .wrapping_mul(0xcc9e_2d51)
                .rotate_left(15)
                .wrapping_mul(0x1b87_3593);
            h = k.rotate_left(13).wrapping_mul(5).wrapping_add(0xe654_6b64) ^ 4;
            h = (h ^ (h >> 16)).wrapping_mul(0x85eb_ca6b);
            h = (h ^ (h >> 13)).wrapping_mul(0xc2b2_ae35);
            h ^= h >> 16;
        }
        Some(h)
    }
}

#[derive(Debug)]
enum Items {
    Positional(PVector, usize),
    Ordered(btree_map::IntoIter<Order, (Value, Value)>, ByteReservation),
}

impl Items {
    fn next(&mut self) -> Option<(Value, Value)> {
        match self {
            Self::Positional(v, i) => {
                let key = v.get(*i)?;
                let value = v.get(*i + 1).unwrap();
                *i += 2;
                Some((key, value))
            }
            Self::Ordered(items, charge) => {
                let _ = charge; // Retain the snapshot's admission until consumed.
                items.next().map(|(_, pair)| pair)
            }
        }
    }
}

#[derive(Debug)]
enum Part {
    Value(Value, Context),
    Text(&'static str),
    String {
        flatten: Option<StringFlatten>,
        text: Option<JsString>,
        index: usize,
        readable: bool,
    },
    Sequence(PVector, usize, &'static str, Context),
    Namespace {
        pairs: PVector,
        index: usize,
        namespace: Namespace,
        context: Context,
    },
    Scan {
        cursor: crate::collections::map::Cursor,
        entries: Entries,
        charge: ByteReservation,
        set: bool,
        hashing: Option<Box<StringKey>>,
        namespace: Namespace,
        context: Context,
    },
    Map(Items, bool, usize, Context, bool),
}

#[derive(Debug)]
pub(super) struct Diagnostic {
    roots: ComparisonOperands,
    options: Options,
    pending: Vec<Part>,
    pending_charge: GrowthCharge,
    units: Vec<u16>,
    output_charge: GrowthCharge,
    header_charge: ByteReservation,
    host: String,
    host_index: usize,
    host_charge: GrowthCharge,
    host_header: ByteReservation,
}

impl Trace for Diagnostic {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        // Every pending value comes from these immutable source graphs.
        self.roots.0.trace(visit);
        self.roots.1.trace(visit);
    }
}

impl Diagnostic {
    #[cfg(test)]
    pub(super) fn new(heap: &Heap, roots: ComparisonOperands) -> Result<Self, Fault> {
        Self::with_options(heap, roots, Options::default())
    }

    pub(super) fn for_task(
        heap: &Heap,
        state: &VmState,
        roots: ComparisonOperands,
    ) -> Result<Self, Fault> {
        Self::with_options(heap, roots, Options::for_task(heap, state)?)
    }

    fn with_options(
        heap: &Heap,
        roots: ComparisonOperands,
        options: Options,
    ) -> Result<Self, Fault> {
        let context = Context {
            nested: false,
            level: options.level,
        };
        let mut out = Self {
            roots,
            options,
            pending: Vec::new(),
            pending_charge: GrowthCharge::default(),
            units: Vec::new(),
            output_charge: GrowthCharge::default(),
            header_charge: JsString::reserve_owned(heap, 0, 1)?,
            host: String::new(),
            host_index: 0,
            host_charge: GrowthCharge::default(),
            host_header: reserve_elements(heap, 1, 256)?,
        };
        out.push(heap, Part::Value(out.roots.1.clone(), context))?;
        out.push(heap, Part::Text(" to "))?;
        out.push(heap, Part::Value(out.roots.0.clone(), context))?;
        out.push(heap, Part::Text("Cannot compare "))?;
        Ok(out)
    }

    fn push(&mut self, heap: &Heap, part: Part) -> Result<(), Fault> {
        self.pending_charge
            .admit(heap, self.pending.len(), std::mem::size_of::<Part>())?;
        if self.pending.len() == self.pending.capacity() {
            self.pending
                .try_reserve_exact(self.pending.len().max(8))
                .map_err(|_| Fault::OutOfMemory("comparison traversal"))?;
        }
        self.pending.push(part);
        Ok(())
    }

    fn append(&mut self, heap: &Heap, units: &[u16]) -> Result<(), Fault> {
        self.append_with_limit::<MAX_STRING_LEN>(heap, units)
    }

    fn append_with_limit<const LIMIT: usize>(
        &mut self,
        heap: &Heap,
        units: &[u16],
    ) -> Result<(), Fault> {
        if self
            .units
            .len()
            .checked_add(units.len())
            .is_none_or(|len| len > LIMIT)
        {
            return Err(Fault::StringTooLong);
        }
        for &unit in units {
            self.output_charge.admit(heap, self.units.len(), 2)?;
            if self.units.len() == self.units.capacity() {
                self.units
                    .try_reserve_exact(self.units.len().max(8))
                    .map_err(|_| Fault::OutOfMemory("comparison message"))?;
            }
            self.units.push(unit);
        }
        Ok(())
    }

    fn text(&mut self, heap: &Heap, text: &str) -> Result<(), Fault> {
        // All callers pass bounded ASCII punctuation or number spellings.
        for unit in text.encode_utf16() {
            self.append(heap, &[unit])?;
        }
        Ok(())
    }

    fn string(&mut self, heap: &Heap, text: JsString, readable: bool) -> Result<(), Fault> {
        if readable {
            self.text(heap, "\"")?;
        }
        self.push(
            heap,
            Part::String {
                flatten: Some(StringFlatten::new_accounted(heap, text)?),
                text: None,
                index: 0,
                readable,
            },
        )
    }

    fn named(&mut self, heap: &Heap, value: &Value, strip_namespace: bool) -> Result<(), Fault> {
        let value = form::unwrap(heap, value.clone());
        let object = form::form_object(heap, &value).unwrap();
        let (FormBody::Keyword(name) | FormBody::Symbol(name)) = object.body() else {
            unreachable!("namespace scanning validates named keys");
        };
        if matches!(object.body(), FormBody::Keyword(_)) {
            self.text(heap, ":")?;
        }
        self.string(heap, name.name.clone(), false)?;
        if !strip_namespace && let Some(ns) = &name.namespace {
            self.push(heap, Part::Text("/"))?;
            self.string(heap, ns.clone(), false)?;
        }
        Ok(())
    }

    fn begin_map(
        &mut self,
        heap: &Heap,
        items: Items,
        set: bool,
        context: Context,
        namespace: Option<JsString>,
    ) -> Result<(), Fault> {
        self.push(heap, Part::Map(items, set, 0, context, namespace.is_some()))?;
        if let Some(namespace) = namespace {
            self.text(heap, "#:")?;
            self.push(heap, Part::Text("{"))?;
            self.string(heap, namespace, false)?;
        } else {
            self.text(heap, if set { "#{" } else { "{" })?;
        }
        Ok(())
    }

    pub(super) fn step(&mut self, heap: &Heap) -> Result<Action, Fault> {
        let Some(part) = self.pending.pop() else {
            // UTF-8 is only a host diagnostic. Decode in bounded steps too,
            // preserving the previous lossy display of lone UTF-16 surrogates.
            for _ in 0..128 {
                let Some(&unit) = self.units.get(self.host_index) else {
                    break;
                };
                self.host_index += 1;
                let c = if (0xd800..=0xdbff).contains(&unit) {
                    if let Some(&low) = self.units.get(self.host_index)
                        && (0xdc00..=0xdfff).contains(&low)
                    {
                        self.host_index += 1;
                        char::from_u32(
                            0x10000 + ((u32::from(unit) - 0xd800) << 10) + u32::from(low) - 0xdc00,
                        )
                        .unwrap()
                    } else {
                        char::REPLACEMENT_CHARACTER
                    }
                } else {
                    char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER)
                };
                let needed = self.host.len() + c.len_utf8();
                self.host_charge.admit(heap, needed - 1, 1)?;
                if needed > self.host.capacity() {
                    let capacity = needed.max(self.host.capacity().saturating_mul(2)).max(8);
                    self.host
                        .try_reserve_exact(capacity - self.host.len())
                        .map_err(|_| Fault::OutOfMemory("comparison host diagnostic"))?;
                }
                self.host.push(c);
            }
            if self.host_index < self.units.len() {
                return Ok(Action::Continue);
            }
            let mut charge = std::mem::take(&mut self.header_charge);
            charge.merge(std::mem::take(&mut self.output_charge).into_reservation());
            let message = JsString::from_owned_code_units(std::mem::take(&mut self.units))
                .ok_or(Fault::StringTooLong)?
                .with_reservation(charge.is_charged().then(|| std::sync::Arc::new(charge)));
            let mut host_charge = std::mem::take(&mut self.host_header);
            host_charge.merge(std::mem::take(&mut self.host_charge).into_reservation());
            let diagnostic =
                crate::error::Message::accounted(std::mem::take(&mut self.host), host_charge);
            return Err(Fault::ExactError(message, diagnostic));
        };
        match part {
            Part::Text(text) => self.text(heap, text)?,
            Part::String {
                mut flatten,
                mut text,
                mut index,
                readable,
            } => {
                if let Some(build) = &mut flatten {
                    text = build.advance(128);
                    if text.is_some() {
                        flatten = None;
                    }
                }
                if let Some(text) = &text {
                    let end = (index + 128).min(text.len());
                    for &unit in &text.code_units()[index..end] {
                        let escape = if readable {
                            match unit {
                                34 => Some("\\\""),
                                92 => Some("\\\\"),
                                8 => Some("\\b"),
                                12 => Some("\\f"),
                                10 => Some("\\n"),
                                13 => Some("\\r"),
                                9 => Some("\\t"),
                                _ => None,
                            }
                        } else {
                            None
                        };
                        if let Some(escape) = escape {
                            self.text(heap, escape)?;
                        } else {
                            self.append(heap, &[unit])?;
                        }
                    }
                    index = end;
                    if index == text.len() {
                        if readable {
                            self.text(heap, "\"")?;
                        }
                        return Ok(Action::Continue);
                    }
                }
                self.push(
                    heap,
                    Part::String {
                        flatten,
                        text,
                        index,
                        readable,
                    },
                )?;
            }
            Part::Sequence(items, index, end, context) => {
                if let Some(value) = items.get(index) {
                    if index > 0 {
                        self.text(heap, " ")?;
                    }
                    if self.options.length == Some(index as f64) {
                        self.text(heap, "...")?;
                        self.text(heap, end)?;
                    } else {
                        self.push(heap, Part::Sequence(items, index + 1, end, context))?;
                        self.push(heap, Part::Value(value, context))?;
                    }
                } else {
                    self.text(heap, end)?;
                }
            }
            Part::Scan {
                mut cursor,
                mut entries,
                charge,
                set,
                mut hashing,
                mut namespace,
                context,
            } => {
                let ready = if matches!(namespace, Namespace::Comparing(_)) {
                    namespace.advance();
                    None
                } else if let Some(mut string) = hashing.take() {
                    if let Some(hash) = string.advance() {
                        Some((string.pair, hash, false))
                    } else {
                        hashing = Some(string);
                        None
                    }
                } else if let Some((key, value)) = cursor.next() {
                    match form::unwrap(heap, key.clone()) {
                        Value::String(text) => {
                            hashing = Some(StringKey::new(heap, (key, value), text)?);
                            None
                        }
                        unwrapped => {
                            let hash = form::hash(heap, &key) as u32;
                            let nil = matches!(unwrapped, Value::Null | Value::Undefined);
                            Some(((key, value), hash, nil))
                        }
                    }
                } else {
                    self.begin_map(
                        heap,
                        Items::Ordered(entries.into_iter(), charge),
                        set,
                        context,
                        namespace.finish(),
                    )?;
                    return Ok(Action::Continue);
                };
                if let Some((pair, hash, nil)) = ready {
                    namespace.observe(heap, &pair.0)?;
                    entries.insert(
                        (
                            !nil,
                            std::array::from_fn(|i| ((hash >> (i * 5)) & 31) as u8),
                            entries.len(),
                        ),
                        pair,
                    );
                }
                self.push(
                    heap,
                    Part::Scan {
                        cursor,
                        entries,
                        charge,
                        set,
                        hashing,
                        namespace,
                        context,
                    },
                )?;
            }
            Part::Namespace {
                pairs,
                mut index,
                mut namespace,
                context,
            } => {
                if matches!(namespace, Namespace::Comparing(_)) {
                    namespace.advance();
                } else if let Some(key) = pairs.get(index) {
                    namespace.observe(heap, &key)?;
                    index += 2;
                } else {
                    self.begin_map(
                        heap,
                        Items::Positional(pairs, 0),
                        false,
                        context,
                        namespace.finish(),
                    )?;
                    return Ok(Action::Continue);
                }
                self.push(
                    heap,
                    Part::Namespace {
                        pairs,
                        index,
                        namespace,
                        context,
                    },
                )?;
            }
            Part::Map(mut items, set, printed, context, strip_namespace) => {
                if let Some((key, value)) = items.next() {
                    if printed > 0 {
                        self.text(heap, if set { " " } else { ", " })?;
                    }
                    if self.options.length == Some(printed as f64) {
                        self.text(heap, "...}")?;
                    } else {
                        self.push(
                            heap,
                            Part::Map(items, set, printed + 1, context, strip_namespace),
                        )?;
                        if !set {
                            self.push(heap, Part::Value(value, context))?;
                            self.push(heap, Part::Text(" "))?;
                        }
                        if strip_namespace {
                            self.named(heap, &key, true)?;
                        } else {
                            self.push(heap, Part::Value(key, context))?;
                        }
                    }
                } else {
                    self.text(heap, "}")?;
                }
            }
            Part::Value(value, context) => match form::unwrap(heap, value) {
                Value::Null | Value::Undefined => {
                    self.text(heap, if context.nested { "nil" } else { "" })?
                }
                Value::Bool(value) => self.text(heap, if value { "true" } else { "false" })?,
                // Collection toString uses readable strings even when the
                // enclosing task binds *print-readably* to false.
                Value::String(value) => self.string(heap, value, context.nested)?,
                Value::Number(value) => self.text(
                    heap,
                    &if context.nested && value.is_nan() {
                        "##NaN".into()
                    } else if context.nested && value == f64::INFINITY {
                        "##Inf".into()
                    } else if context.nested && value == f64::NEG_INFINITY {
                        "##-Inf".into()
                    } else if context.nested && value == 0.0 && value.is_sign_negative() {
                        "-0.0".into()
                    } else {
                        crate::value::string::number_to_string(value)
                    },
                )?,
                value => {
                    let object = form::form_object(heap, &value);
                    match object.as_deref().map(|f| f.body()) {
                        Some(FormBody::Keyword(_) | FormBody::Symbol(_)) => {
                            self.named(heap, &value, false)?;
                        }
                        Some(FormBody::Vector(items) | FormBody::List(items)) => {
                            let vector =
                                matches!(object.as_ref().unwrap().body(), FormBody::Vector(_));
                            let Some(context) = context.collection() else {
                                self.text(heap, "#")?;
                                return Ok(Action::Continue);
                            };
                            self.text(heap, if vector { "[" } else { "(" })?;
                            self.push(
                                heap,
                                Part::Sequence(
                                    items.clone(),
                                    0,
                                    if vector { "]" } else { ")" },
                                    context,
                                ),
                            )?;
                        }
                        Some(FormBody::Map(map) | FormBody::Set(map)) => {
                            let set = matches!(object.as_ref().unwrap().body(), FormBody::Set(_));
                            let Some(context) = context.collection() else {
                                self.text(heap, "#")?;
                                return Ok(Action::Continue);
                            };
                            let namespace = Namespace::new(self.options.namespace_maps && !set);
                            match map {
                                FormMap::Positional(pairs) => {
                                    if self.options.namespace_maps && !set {
                                        self.push(
                                            heap,
                                            Part::Namespace {
                                                pairs: pairs.clone(),
                                                index: 0,
                                                namespace,
                                                context,
                                            },
                                        )?;
                                    } else {
                                        self.begin_map(
                                            heap,
                                            Items::Positional(pairs.clone(), 0),
                                            set,
                                            context,
                                            None,
                                        )?;
                                    }
                                }
                                FormMap::Hashed(map) => {
                                    // A one-entry tree already allocates a full node.
                                    // Admit a complete twelve-slot node (including
                                    // edges/header) per entry, before any insertion.
                                    // Each step visits one entry or a bounded
                                    // chunk of a primitive string key's hash.
                                    let node_bytes =
                                        12 * std::mem::size_of::<(Order, (Value, Value))>() + 256;
                                    let charge = reserve_elements(heap, map.len(), node_bytes)?;
                                    self.push(
                                        heap,
                                        Part::Scan {
                                            cursor: crate::collections::map::Cursor::new(map),
                                            entries: Entries::new(),
                                            charge,
                                            set,
                                            hashing: None,
                                            namespace,
                                            context,
                                        },
                                    )?;
                                }
                            }
                        }
                        _ => self.text(heap, "[object Object]")?,
                    }
                }
            },
        }
        Ok(Action::Continue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::compile_function;
    use crate::host::NoHost;
    use crate::{Budget, Primitive, RunOutcome, Runtime};

    #[test]
    fn hashed_map_snapshot_is_admitted_before_allocating_tree_nodes() {
        for len in [1, 64] {
            let heap = Heap::with_test_byte_headroom(1 << 20, false);
            let pairs: Vec<_> = (0..len)
                .map(|i| (Value::Number(i as f64), Value::Null))
                .collect();
            let map = form::new_map(&heap, &pairs).unwrap();
            let budget = heap.byte_budget();
            let operands = budget.used();
            let node_bytes = 12 * std::mem::size_of::<(Order, (Value, Value))>() + 256;
            for denied in [true, false] {
                let mut diagnostic =
                    Diagnostic::new(&heap, ComparisonOperands(Value::Number(1.0), map.clone()))
                        .unwrap_or_else(|_| panic!("diagnostic setup must fit"));
                while !matches!(diagnostic.pending.last(), Some(Part::Value(value, _)) if value.same_object(&map))
                {
                    diagnostic
                        .step(&heap)
                        .unwrap_or_else(|_| panic!("prefix must fit"));
                }
                let before = budget.used();
                if denied {
                    budget.set_limit(before + len * node_bytes - 1);
                }
                let result = diagnostic.step(&heap);
                if denied {
                    assert!(matches!(result, Err(Fault::HeapLimit)));
                    assert_eq!(
                        budget.used(),
                        before,
                        "denied snapshots allocate no storage"
                    );
                } else {
                    assert!(matches!(result, Ok(Action::Continue)));
                    assert_eq!(budget.used() - before, len * node_bytes);
                    assert!(
                        matches!(diagnostic.pending.last(), Some(Part::Scan { entries, .. }) if entries.is_empty())
                    );
                }
                drop(diagnostic);
                assert_eq!(
                    budget.used(),
                    operands,
                    "dropping a scan releases all diagnostic storage"
                );
                budget.set_limit(usize::MAX);
            }
            heap.collect().unwrap();
            assert!(
                budget.used() < operands,
                "collection releases the unrooted operands"
            );
        }
    }

    #[test]
    fn string_hash_continuation_charge_is_admitted_and_released() {
        let heap = Heap::with_test_byte_headroom(1 << 20, false);
        let text = JsString::copy_units(&heap, &vec![97; STRING_HASH_STEP_UNITS * 4]).unwrap();
        let budget = heap.byte_budget();
        let occupied = budget.used();
        let bytes = std::mem::size_of::<StringKey>();
        budget.set_limit(occupied + bytes - 1);
        assert!(matches!(
            StringKey::new(
                &heap,
                (Value::String(text.clone()), Value::Null),
                text.clone()
            ),
            Err(Fault::HeapLimit)
        ));
        assert_eq!(budget.used(), occupied);
        budget.set_limit(usize::MAX);
        for cancel in [false, true] {
            let mut hashing = StringKey::new(
                &heap,
                (Value::String(text.clone()), Value::Null),
                text.clone(),
            )
            .unwrap_or_else(|_| panic!("hash continuation must fit"));
            assert!(hashing.advance().is_none()); // Flat input becomes ready.
            assert!(hashing.advance().is_none()); // Pause during hashing.
            assert_eq!(budget.used(), occupied + bytes);
            if !cancel {
                while hashing.advance().is_none() {}
                assert_eq!(
                    budget.used(),
                    occupied + bytes,
                    "the owner retains admission until dropped"
                );
            }
            drop(hashing);
            assert_eq!(
                budget.used(),
                occupied,
                "completion and cancellation release admission"
            );
        }
        drop(text);
        heap.collect().unwrap();
        assert!(budget.used() < occupied);
    }

    #[test]
    fn string_key_hash_matches_language_hash() {
        let heap = Heap::default();
        for len in [0, 1, 2, 127, 128, 129, 255, 256, 257, 1025] {
            // Include zero, non-ASCII units, paired and lone surrogates, and
            // enough units for repeated signed accumulator overflow.
            let units: Vec<u16> = [0, 97, 0xffff, 0xd800, 0xdc00, 0xdc01]
                .into_iter()
                .cycle()
                .take(len)
                .collect();
            let flat = JsString::from_code_units(units.clone()).unwrap();
            let rope = if len > 32 {
                JsString::from_code_units(units[..len / 2].to_vec())
                    .unwrap()
                    .concat(&JsString::from_code_units(units[len / 2..].to_vec()).unwrap())
                    .unwrap()
            } else {
                flat.clone()
            };
            for text in [flat, rope] {
                let key = Value::String(text.clone());
                let mut hashing = StringKey::new(&heap, (key.clone(), Value::Null), text)
                    .unwrap_or_else(|_| panic!("string hash setup must fit"));
                let before = HASHED_STRING_UNITS.get();
                let hash = loop {
                    let hashed = HASHED_STRING_UNITS.get();
                    let copied = JsString::copied_code_units();
                    let result = hashing.advance();
                    assert!(HASHED_STRING_UNITS.get() - hashed <= STRING_HASH_STEP_UNITS);
                    assert!(
                        JsString::copied_code_units() - copied <= STRING_HASH_STEP_UNITS as u64
                    );
                    if let Some(hash) = result {
                        break hash;
                    }
                };
                assert_eq!(HASHED_STRING_UNITS.get() - before, len);
                assert_eq!(hash, form::hash(&heap, &key) as u32, "length {len}");
            }
        }
    }

    #[test]
    fn sorted_comparison_string_key_hashing_is_budgeted() {
        let text = "a".repeat(STRING_HASH_STEP_UNITS * 256 + 17);
        let code = compile_function(
            "function f(key) {try {return __rt.formGet(__rt.formSortedMap(__rt.formVector(1,0)),key,null);} catch(e) {return e.message;}}",
        ).unwrap();
        for wrapped in [false, true] {
            for cancel in [false, true] {
                let rt = Runtime::try_with_heap_policy(
                    Default::default(),
                    crate::HeapPolicy {
                        hard_limit_bytes: Some(16 << 20),
                        ..Default::default()
                    },
                )
                .unwrap();
                // Hash the key during preparation, outside the measured runs.
                // Its containing map has hashed storage even with one entry.
                let (key, weak) = rt.scope(|s| {
                    let mut key = s.primitive(Primitive::String(JsString::from(text.as_str())));
                    if wrapped {
                        key = s.new_occurrence(key, Default::default()).unwrap();
                    }
                    let value = s.primitive(Primitive::Number(42.0));
                    let map = s.new_form_map(&[(key, value)]).unwrap();
                    assert!(matches!(
                        form::form_object(rt.heap(), &s.local_value(map))
                            .unwrap()
                            .body(),
                        FormBody::Map(FormMap::Hashed(_))
                    ));
                    (s.persist(map), s.downgrade(map))
                });
                let task = rt.spawn(&code, &[&key]).unwrap();
                drop(key);
                let before = HASHED_STRING_UNITS.get();
                let mut hashing_steps = 0;
                let result = 'run: {
                    for _ in 0..10_000 {
                        let hashed = HASHED_STRING_UNITS.get();
                        let occupied = rt.heap_stats().managed_bytes;
                        let outcome = task.run_with(Budget::instructions(1), &mut NoHost).unwrap();
                        let work = HASHED_STRING_UNITS.get() - hashed;
                        if work > 0 && HASHED_STRING_UNITS.get() - before == text.len() {
                            assert_eq!(
                                occupied - rt.heap_stats().managed_bytes,
                                std::mem::size_of::<StringKey>(),
                                "the completed string hash must release its continuation charge"
                            );
                        }
                        assert!(
                            work <= STRING_HASH_STEP_UNITS,
                            "one run hashed {work} units"
                        );
                        if work > 0 {
                            hashing_steps += 1;
                            assert!(matches!(outcome, RunOutcome::Yielded));
                            if hashing_steps == 2 {
                                let total = HASHED_STRING_UNITS.get() - before;
                                assert_eq!(total, 2 * STRING_HASH_STEP_UNITS);
                                assert!(total < text.len(), "must yield during hashing");
                                rt.collect().unwrap();
                                assert!(weak.is_live(), "the pending entry must stay rooted");
                                if cancel {
                                    let occupied = rt.heap_stats().managed_bytes;
                                    let _ = task.cancel(); // Drive cancellation cleanup below.
                                    assert!(matches!(
                                        task.run_with(Budget::instructions(1), &mut NoHost)
                                            .unwrap(),
                                        RunOutcome::Cancelled
                                    ));
                                    assert_eq!(HASHED_STRING_UNITS.get() - before, total);
                                    assert!(
                                        occupied - rt.heap_stats().managed_bytes
                                            >= std::mem::size_of::<StringKey>(),
                                        "cancellation must release the pending hash continuation"
                                    );
                                    rt.collect().unwrap();
                                    assert!(
                                        !weak.is_live(),
                                        "cancellation must release the operand"
                                    );
                                    break 'run None;
                                }
                            }
                        }
                        match outcome {
                            RunOutcome::Yielded => (),
                            RunOutcome::Complete(value) => break 'run Some(value),
                            other => {
                                panic!("expected yield or caught comparison error, got {other:?}")
                            }
                        }
                    }
                    panic!("comparison failed to finish or enter string hashing");
                };
                if let Some(result) = result {
                    assert!(!cancel);
                    assert_eq!(HASHED_STRING_UNITS.get() - before, text.len());
                    assert_eq!(hashing_steps, text.len().div_ceil(STRING_HASH_STEP_UNITS));
                    assert_eq!(
                        result.string(),
                        Some(JsString::from(
                            format!("Cannot compare 1 to {{\"{text}\" 42}}").as_str()
                        ))
                    );
                    rt.collect().unwrap();
                    assert!(!weak.is_live(), "completion releases the map operand");
                }
            }
        }
    }

    #[test]
    fn namespace_comparison_charge_is_admitted_and_released() {
        let heap = Heap::with_test_byte_headroom(1 << 20, false);
        let left = JsString::copy_units(&heap, &vec![97; NAMESPACE_STEP_UNITS * 4]).unwrap();
        let right = JsString::copy_units(&heap, left.code_units()).unwrap();
        let budget = heap.byte_budget();
        let occupied = budget.used();
        let bytes = std::mem::size_of::<NamespaceComparison>();
        budget.set_limit(occupied + bytes - 1);
        assert!(matches!(
            NamespaceComparison::new(&heap, &left, &right),
            Err(Fault::HeapLimit)
        ));
        assert_eq!(budget.used(), occupied);
        budget.set_limit(usize::MAX);
        for cancel in [false, true] {
            let mut comparing = NamespaceComparison::new(&heap, &left, &right)
                .unwrap_or_else(|_| panic!("namespace continuation must fit"));
            for _ in 0..3 {
                assert!(comparing.advance().is_none());
            }
            assert_eq!(budget.used(), occupied + bytes);
            if !cancel {
                while comparing.advance().is_none() {}
            }
            drop(comparing);
            assert_eq!(
                budget.used(),
                occupied,
                "continuation admission is released"
            );
        }
    }

    #[test]
    fn namespace_comparison_preserves_exact_units_and_bounds_rope_work() {
        let heap = Heap::default();
        for len in [0, 1, 127, 128, 129, 257] {
            let units: Vec<u16> = [0, 97, 0xffff, 0xd800, 0xdc00, 0xdc01]
                .into_iter()
                .cycle()
                .take(len)
                .collect();
            for equal in [false, true] {
                let left = JsString::from_code_units(units.clone()).unwrap();
                let mut other = units.clone();
                if !equal && let Some(last) = other.last_mut() {
                    *last ^= 1;
                }
                for rope in [false, true] {
                    let right = if rope && len > 32 {
                        JsString::from_code_units(other[..len / 2].to_vec())
                            .unwrap()
                            .concat(&JsString::from_code_units(other[len / 2..].to_vec()).unwrap())
                            .unwrap()
                    } else {
                        JsString::from_code_units(other.clone()).unwrap()
                    };
                    assert!(len == 0 || !left.same_body(&right));
                    let mut comparing = NamespaceComparison::new(&heap, &left, &right)
                        .unwrap_or_else(|_| panic!("namespace setup must fit"));
                    let before = COMPARED_NAMESPACE_UNITS.get();
                    let result = loop {
                        let compared = COMPARED_NAMESPACE_UNITS.get();
                        let copied = JsString::copied_code_units();
                        let result = comparing.advance();
                        let work = COMPARED_NAMESPACE_UNITS.get() - compared;
                        let copies = JsString::copied_code_units() - copied;
                        assert!(work + copies as usize <= NAMESPACE_STEP_UNITS);
                        if let Some(result) = result {
                            break result;
                        }
                    };
                    assert_eq!(result, equal || len == 0);
                    assert_eq!(COMPARED_NAMESPACE_UNITS.get() - before, len);
                }
            }
        }
    }

    #[test]
    fn sorted_comparison_namespace_scanning_is_budgeted() {
        let text = "a".repeat(NAMESPACE_STEP_UNITS * 256 + 17);
        let code = compile_function(
            "function f(key) {globalThis.__lightNativeRuntime={core:{printNamespaceMaps:true}};try {return __rt.formGet(__rt.formSortedMap(__rt.formVector(1,0)),key,null);} catch(e) {return e.message;}}",
        ).unwrap();
        for hashed in [false, true] {
            for cancel in [false, true] {
                let rt = Runtime::try_with_heap_policy(
                    Default::default(),
                    crate::HeapPolicy {
                        hard_limit_bytes: Some(16 << 20),
                        ..Default::default()
                    },
                )
                .unwrap();
                let (key, weak, a_first) = rt.scope(|s| {
                    let left = JsString::from(text.as_str());
                    let right = JsString::from(text.as_str());
                    assert!(
                        !left.same_body(&right),
                        "equal namespaces need distinct bodies"
                    );
                    let a = s.new_keyword(Some(left), JsString::from("a")).unwrap();
                    let b = s.new_keyword(Some(right), JsString::from("b")).unwrap();
                    let one = s.primitive(Primitive::Number(1.0));
                    let pairs = [(a, one), (b, one)];
                    let map = if hashed {
                        s.new_form_map(&pairs).unwrap()
                    } else {
                        s.new_literal_map(&pairs).unwrap()
                    };
                    let order = |key| {
                        let hash = s.form_hash(key) as u32;
                        std::array::from_fn::<_, 7, _>(|i| ((hash >> (i * 5)) & 31) as u8)
                    };
                    (
                        s.persist(map),
                        s.downgrade(map),
                        !hashed || order(a) < order(b),
                    )
                });
                let task = rt.spawn(&code, &[&key]).unwrap();
                drop(key);
                let before = COMPARED_NAMESPACE_UNITS.get();
                let mut comparing_steps = 0;
                let result = 'run: {
                    for _ in 0..10_000 {
                        let compared = COMPARED_NAMESPACE_UNITS.get();
                        let copied = JsString::copied_code_units();
                        let occupied = rt.heap_stats().managed_bytes;
                        let outcome = task.run_with(Budget::instructions(1), &mut NoHost).unwrap();
                        let work = COMPARED_NAMESPACE_UNITS.get() - compared;
                        assert!(
                            work + (JsString::copied_code_units() - copied) as usize
                                <= NAMESPACE_STEP_UNITS,
                            "one run exceeded the namespace work budget"
                        );
                        if work > 0 {
                            comparing_steps += 1;
                            assert!(matches!(outcome, RunOutcome::Yielded));
                            let total = COMPARED_NAMESPACE_UNITS.get() - before;
                            if total == text.len() {
                                assert_eq!(
                                    occupied - rt.heap_stats().managed_bytes,
                                    std::mem::size_of::<NamespaceComparison>(),
                                    "completion releases the namespace continuation charge"
                                );
                            }
                            if comparing_steps == 2 {
                                assert_eq!(total, 2 * NAMESPACE_STEP_UNITS);
                                assert!(total < text.len(), "must yield during namespace equality");
                                rt.collect().unwrap();
                                assert!(weak.is_live(), "pending namespaces must stay rooted");
                                if cancel {
                                    let occupied = rt.heap_stats().managed_bytes;
                                    let _ = task.cancel();
                                    assert!(matches!(
                                        task.run_with(Budget::instructions(1), &mut NoHost)
                                            .unwrap(),
                                        RunOutcome::Cancelled
                                    ));
                                    assert_eq!(COMPARED_NAMESPACE_UNITS.get() - before, total);
                                    assert!(
                                        occupied - rt.heap_stats().managed_bytes
                                            >= std::mem::size_of::<NamespaceComparison>()
                                    );
                                    rt.collect().unwrap();
                                    assert!(
                                        !weak.is_live(),
                                        "cancellation releases the map operand"
                                    );
                                    break 'run None;
                                }
                            }
                        }
                        match outcome {
                            RunOutcome::Yielded => (),
                            RunOutcome::Complete(value) => break 'run Some(value),
                            other => {
                                panic!("expected yield or caught comparison error, got {other:?}")
                            }
                        }
                    }
                    panic!("comparison failed to finish or scan namespaces");
                };
                if let Some(result) = result {
                    assert!(!cancel);
                    assert_eq!(COMPARED_NAMESPACE_UNITS.get() - before, text.len());
                    assert_eq!(comparing_steps, text.len().div_ceil(NAMESPACE_STEP_UNITS));
                    let pairs = if a_first { ":a 1, :b 1" } else { ":b 1, :a 1" };
                    assert_eq!(
                        result.string(),
                        Some(JsString::from(
                            format!("Cannot compare 1 to #:{text}{{{pairs}}}").as_str()
                        ))
                    );
                    rt.collect().unwrap();
                    assert!(!weak.is_live(), "completion releases the map operand");
                }
            }
        }
    }

    #[test]
    fn sorted_comparison_unequal_namespaces_remain_qualified() {
        let left = "a".repeat(NAMESPACE_STEP_UNITS * 2 + 17);
        let right = format!("{}b", &left[..left.len() - 1]);
        let code = compile_function(
            "function f(key) {globalThis.__lightNativeRuntime={core:{printNamespaceMaps:true}};try {return __rt.formGet(__rt.formSortedMap(__rt.formVector(1,0)),key,null);} catch(e) {return e.message;}}",
        ).unwrap();
        for hashed in [false, true] {
            let rt = Runtime::new();
            let (key, expected) = rt.scope(|s| {
                let namespaces = [
                    JsString::from(left.as_str()),
                    JsString::from(right.as_str()),
                ];
                // Two keys per namespace guarantee a subsequent observation
                // after the first mismatch, in either traversal order.
                let mut entries = ["a", "b", "c", "d"]
                    .into_iter()
                    .enumerate()
                    .map(|(i, name)| {
                        let namespace = namespaces[i % 2].clone();
                        let label = format!(":{namespace}/{name} 1");
                        let key = s
                            .new_keyword(Some(namespace), JsString::from(name))
                            .unwrap();
                        (key, label)
                    })
                    .collect::<Vec<_>>();
                let one = s.primitive(Primitive::Number(1.0));
                let pairs = entries
                    .iter()
                    .map(|(key, _)| (*key, one))
                    .collect::<Vec<_>>();
                let map = if hashed {
                    s.new_form_map(&pairs).unwrap()
                } else {
                    s.new_literal_map(&pairs).unwrap()
                };
                if hashed {
                    entries.sort_by_key(|(key, _)| {
                        let hash = s.form_hash(*key) as u32;
                        std::array::from_fn::<_, 7, _>(|i| ((hash >> (i * 5)) & 31) as u8)
                    });
                }
                let pairs = entries
                    .iter()
                    .map(|(_, label)| label.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                (s.persist(map), format!("Cannot compare 1 to {{{pairs}}}"))
            });
            let task = rt.spawn(&code, &[&key]).unwrap();
            let before = COMPARED_NAMESPACE_UNITS.get();
            let mut comparing_steps = 0;
            let result = 'run: {
                for _ in 0..1_000 {
                    let compared = COMPARED_NAMESPACE_UNITS.get();
                    let copied = JsString::copied_code_units();
                    let outcome = task.run_with(Budget::instructions(1), &mut NoHost).unwrap();
                    let work = COMPARED_NAMESPACE_UNITS.get() - compared;
                    assert!(
                        work + (JsString::copied_code_units() - copied) as usize
                            <= NAMESPACE_STEP_UNITS,
                        "one run exceeded the namespace work budget (hashed: {hashed})"
                    );
                    if work > 0 {
                        comparing_steps += 1;
                        assert!(matches!(outcome, RunOutcome::Yielded));
                    }
                    match outcome {
                        RunOutcome::Yielded => (),
                        RunOutcome::Complete(value) => break 'run value,
                        other => panic!("expected yield or caught comparison error, got {other:?}"),
                    }
                }
                panic!("comparison failed to finish or scan namespaces");
            };
            assert_eq!(
                result.string(),
                Some(JsString::from(expected.as_str())),
                "hashed: {hashed}"
            );
            assert_eq!(COMPARED_NAMESPACE_UNITS.get() - before, left.len());
            assert_eq!(comparing_steps, left.len().div_ceil(NAMESPACE_STEP_UNITS));
        }
    }

    #[test]
    fn string_limit_is_checked_before_output_growth() {
        let heap = Heap::default();
        let roots = ComparisonOperands(Value::Number(1.0), Value::Number(2.0));
        let mut diagnostic =
            Diagnostic::new(&heap, roots).unwrap_or_else(|_| panic!("diagnostic setup must fit"));
        let before = heap.byte_budget().used();
        assert!(matches!(
            diagnostic.append_with_limit::<8>(&heap, &[0; 9]),
            Err(Fault::StringTooLong)
        ));
        assert!(diagnostic.units.is_empty());
        assert_eq!(heap.byte_budget().used(), before);
        diagnostic
            .append_with_limit::<8>(&heap, &[0; 8])
            .unwrap_or_else(|_| panic!("output at the limit must fit"));
        let before = heap.byte_budget().used();
        assert!(matches!(
            diagnostic.append_with_limit::<8>(&heap, &[0]),
            Err(Fault::StringTooLong)
        ));
        assert_eq!(diagnostic.units.len(), 8);
        assert_eq!(heap.byte_budget().used(), before);
    }
}
