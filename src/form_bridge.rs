//! Structured compiler/engine transport. Descriptors are actual engine arrays;
//! no printing, reader invocation, serde or JSON participates in conversion.
#[cfg(light_quickjs)]
use crate::CollectionKind as C;
#[cfg(light_quickjs)]
use crate::compiler_data::{CompilerLiteralKind as L, Form};
use crate::compiler_data::{Datum, DatumKind as D};
#[cfg(light_quickjs)]
use crate::evaluator::ExecutionControl;
#[cfg(light_quickjs)]
use rquickjs::class::{JsClass, Trace, Tracer, Writable};
#[cfg(light_quickjs)]
use rquickjs::{Array, Class, Ctx, JsLifetime};
#[cfg(light_quickjs)]
use std::collections::{HashMap, VecDeque};

/// In-process descriptor ABI version shared with the canonical runtime.
pub const FORM_BRIDGE_VERSION: u32 = 5;
#[cfg(light_quickjs)]
const ENVIRONMENT_SLOTS: usize = 64;
#[cfg(light_quickjs)]
const ENVIRONMENT_CHILD_SLOTS: usize = 128;

/// Transport text as a native engine string, without an array allocation and
/// property operation for every code unit. The UTF-16 API preserves unpaired
/// surrogates, which converting through a Rust UTF-8 string would lose.
#[cfg(light_quickjs)]
fn engine_string<'js>(ctx: &Ctx<'js>, units: &[u16]) -> rquickjs::Result<rquickjs::String<'js>> {
    // SAFETY: QuickJS copies this live slice and returns an owned value in ctx.
    let value = unsafe {
        let raw = rquickjs::qjs::JS_NewStringUTF16(
            ctx.as_raw().as_ptr(),
            units.as_ptr(),
            units.len() as _,
        );
        rquickjs::Value::from_raw(ctx.clone(), raw)
    };
    if value.is_exception() {
        return Err(rquickjs::Error::Exception);
    }
    rquickjs::String::from_value(value)
}

#[cfg(light_quickjs)]
fn string_units(value: rquickjs::String<'_>) -> rquickjs::Result<Vec<u16>> {
    let ctx = value.ctx().as_raw().as_ptr();
    let mut len = 0;
    // SAFETY: value belongs to this live context. The returned buffer is owned
    // by QuickJS until freed with the matching UTF-16 deallocator below.
    let ptr = unsafe { rquickjs::qjs::JS_ToCStringLenUTF16(ctx, &mut len, value.as_raw()) };
    if ptr.is_null() {
        return Err(rquickjs::Error::Exception);
    }
    // SAFETY: QuickJS returned exactly len initialized UTF-16 code units.
    let units = unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec();
    // SAFETY: This is the buffer returned above, freed once after copying.
    unsafe { rquickjs::qjs::JS_FreeCStringUTF16(ctx, ptr) };
    Ok(units)
}

#[derive(Clone, Copy, Debug)]
pub struct ConversionLimits {
    pub nodes: usize,
    pub depth: usize,
}
impl Default for ConversionLimits {
    fn default() -> Self {
        Self {
            nodes: 100_000,
            depth: 128,
        }
    }
}
#[derive(Default)]
#[cfg(light_quickjs)]
pub(crate) struct BridgeState {
    pub(crate) forms: HashMap<usize, Form>,
    pub(crate) importing_environment: bool,
    // A bounded cache of immutable environment components. Holding the Datum
    // prevents allocator address reuse from masquerading as an identity hit.
    environments: Vec<EnvironmentEntry>,
    // Separate slots for immutable descendants. The FIFO bound keeps changed
    // lexical views from retaining every binding ever imported.
    environment_children: HashMap<usize, EnvironmentEntry>,
    environment_child_order: VecDeque<usize>,
    #[cfg(test)]
    pub(crate) live_provenance: std::rc::Rc<std::cell::Cell<usize>>,
}
#[derive(Clone)]
#[cfg(light_quickjs)]
struct EnvironmentEntry {
    data: Datum,
    slot: usize,
    nodes: usize,
    depth: usize,
}
#[cfg(light_quickjs)]
struct ChildImport<'js> {
    // Only previous imports are eligible references. A new descriptor may
    // decode after another reference to it, particularly through metadata.
    previous: HashMap<usize, EnvironmentEntry>,
    // Scoped engine arrays, never stored in BridgeState or a persistent root.
    updates: Vec<(usize, Array<'js>)>,
}

/// The engine owns object provenance through its weak map of imported objects;
/// scalars retain their latest provenance by value. There are no engine edges
/// here: collecting an object releases its Rust data, while a macro that saves
/// an input keeps its source and lexical spelling.
#[cfg(light_quickjs)]
struct BorrowedDatum {
    original: Datum,
    canonical: Option<Datum>,
    form: Option<Form>,
    #[cfg(test)]
    live: std::rc::Rc<std::cell::Cell<usize>>,
}
#[cfg(test)]
#[cfg(light_quickjs)]
impl Drop for BorrowedDatum {
    fn drop(&mut self) {
        self.live.set(self.live.get() - 1);
    }
}
// SAFETY: BorrowedDatum contains only owned Rust data, with no JS lifetimes.
#[cfg(light_quickjs)]
unsafe impl<'js> JsLifetime<'js> for BorrowedDatum {
    type Changed<'to> = Self;
}
#[cfg(light_quickjs)]
impl<'js> Trace<'js> for BorrowedDatum {
    fn trace<'a>(&self, _: Tracer<'a, 'js>) {}
}
#[cfg(light_quickjs)]
impl<'js> JsClass<'js> for BorrowedDatum {
    const NAME: &'static str = "CompilerProvenance";
    type Mutable = Writable;
    fn constructor(_: &Ctx<'js>) -> rquickjs::Result<Option<rquickjs::function::Constructor<'js>>> {
        Ok(None)
    }
}
#[cfg(light_quickjs)]
fn error(message: impl Into<String>) -> rquickjs::Error {
    rquickjs::Error::new_from_js_message("compiler descriptor", "form", message.into())
}
#[cfg(light_quickjs)]
impl BridgeState {
    pub(crate) fn reset_environment_cache(&mut self) {
        // A failed import may have installed only some engine slots. Never
        // reference them on a later interactive operation; refill from slot 0.
        self.environments.clear();
        self.reset_environment_children();
    }
    pub(crate) fn reset_environment_children(&mut self) {
        self.environment_children.clear();
        self.environment_child_order.clear();
    }
    pub(crate) fn retain(&mut self, form: &Form) {
        self.forms.insert(form.data().identity(), form.clone());
        for child in form.children() {
            self.retain(&child);
        }
        if let Some(metadata) = form.metadata() {
            self.retain(&metadata);
        }
        if let Some(payload) = form.node().compiler_payload.and_then(|id| form.at(id)) {
            self.retain(&payload);
        }
    }
    pub(crate) fn encode<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        data: &Datum,
        control: &ExecutionControl,
        limits: ConversionLimits,
    ) -> rquickjs::Result<Array<'js>> {
        let mut remaining = limits.nodes;
        let result = self.encode_node(
            ctx,
            data,
            control,
            limits.depth,
            &mut remaining,
            (true, None),
        );
        self.forms.clear();
        result
    }
    pub(crate) fn encode_environment<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        data: &Datum,
        control: &ExecutionControl,
        limits: ConversionLimits,
    ) -> rquickjs::Result<Array<'js>> {
        let D::Collection {
            kind: C::Map,
            children,
        } = data.kind()
        else {
            let mut remaining = limits.nodes;
            return self.encode_node(
                ctx,
                data,
                control,
                limits.depth,
                &mut remaining,
                (true, None),
            );
        };
        if data.metadata().is_some() {
            let mut remaining = limits.nodes;
            return self.encode_node(
                ctx,
                data,
                control,
                limits.depth,
                &mut remaining,
                (true, None),
            );
        }
        control.poll().map_err(|e| error(e.0))?;
        if limits.depth < 2 || limits.nodes == 0 {
            control.invalidate();
            return Err(error("compiler input conversion limit"));
        }
        let mut remaining = limits.nodes - 1;
        let mut child_import = ChildImport {
            previous: self.environment_children.clone(),
            updates: Vec::new(),
        };
        let items = Array::new(ctx.clone())?;
        for (i, child) in children.iter().enumerate() {
            let cached = (i % 2 == 1)
                .then(|| {
                    self.environments
                        .iter()
                        .position(|entry| entry.data.identity() == child.identity())
                })
                .flatten();
            let descriptor = if let Some(position) = cached {
                // Cache hits still account for the full logical input budget.
                let (nodes, depth) = (
                    self.environments[position].nodes,
                    self.environments[position].depth,
                );
                if nodes > remaining || depth >= limits.depth {
                    control.invalidate();
                    return Err(error("compiler input conversion limit"));
                }
                remaining -= nodes;
                let entry = self.environments.remove(position);
                let reference = Array::new(ctx.clone())?;
                reference.set(0, "environment")?;
                reference.set(3, entry.slot)?;
                self.environments.push(entry);
                reference
            } else {
                let descriptor = self.encode_node(
                    ctx,
                    child,
                    control,
                    limits.depth - 1,
                    &mut remaining,
                    (
                        i % 2 == 1,
                        if i % 2 == 1 {
                            Some(&mut child_import)
                        } else {
                            None
                        },
                    ),
                )?;
                if i % 2 == 1 && matches!(child.kind(), D::Collection { .. }) && immutable(child) {
                    let slot = if self.environments.len() < ENVIRONMENT_SLOTS {
                        self.environments.len()
                    } else {
                        self.environments.remove(0).slot
                    };
                    descriptor.set(5, slot)?;
                    let (nodes, depth) = dimensions(child);
                    self.environments.push(EnvironmentEntry {
                        data: child.clone(),
                        slot,
                        nodes,
                        depth,
                    });
                }
                descriptor
            };
            items.set(i, descriptor)?;
        }
        let result = Array::new(ctx.clone())?;
        result.set(0, "map")?;
        result.set(1, ())?;
        result.set(2, ())?;
        result.set(3, items)?;
        // Child slots are read from the completed previous import. Publish
        // replacements only after every descriptor was decoded, in encoding
        // order: metadata and children need not decode in that same order.
        let updates = Array::new(ctx.clone())?;
        for (i, (slot, descriptor)) in child_import.updates.into_iter().enumerate() {
            let pair = Array::new(ctx.clone())?;
            pair.set(0, slot)?;
            pair.set(1, descriptor)?;
            updates.set(i, pair)?;
        }
        result.set(6, updates)?;
        Ok(result)
    }
    fn encode_node<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        data: &Datum,
        control: &ExecutionControl,
        depth: usize,
        remaining: &mut usize,
        (provenance, mut sharing): (bool, Option<&mut ChildImport<'js>>),
    ) -> rquickjs::Result<Array<'js>> {
        control.poll().map_err(|e| error(e.0))?;
        if depth == 0 || *remaining == 0 {
            control.invalidate();
            return Err(error("compiler input conversion limit"));
        }
        if let Some(entry) = sharing
            .as_ref()
            .and_then(|sharing| sharing.previous.get(&data.identity()))
        {
            if entry.nodes > *remaining || entry.depth > depth {
                control.invalidate();
                return Err(error("compiler input conversion limit"));
            }
            *remaining -= entry.nodes;
            let reference = Array::new(ctx.clone())?;
            reference.set(0, "environment")?;
            reference.set(3, entry.slot)?;
            return Ok(reference);
        }
        *remaining -= 1;
        let result = Array::new(ctx.clone())?;
        if provenance {
            #[cfg(test)]
            self.live_provenance.set(self.live_provenance.get() + 1);
            result.set(
                1,
                Class::instance(
                    ctx.clone(),
                    BorrowedDatum {
                        original: data.clone(),
                        canonical: None,
                        form: self.forms.get(&data.identity()).map(Form::compact_leaf),
                        #[cfg(test)]
                        live: self.live_provenance.clone(),
                    },
                )?,
            )?;
        } else {
            result.set(1, ())?;
        }
        if let Some(meta) = data.metadata() {
            result.set(
                2,
                self.encode_node(
                    ctx,
                    &meta,
                    control,
                    depth - 1,
                    remaining,
                    (provenance, sharing.as_deref_mut()),
                )?,
            )?;
        } else {
            result.set(2, ())?;
        }
        let tag = match data.kind() {
            D::Nil => "nil",
            D::Boolean(v) => {
                result.set(3, *v)?;
                "bool"
            }
            D::Integer(v) => {
                result.set(3, *v as f64)?;
                "number"
            }
            D::Double(v) => {
                result.set(3, *v)?;
                "number"
            }
            D::Ratio {
                numerator,
                denominator,
            } => {
                result.set(3, *numerator as f64 / *denominator as f64)?;
                "number"
            }
            D::Character(v) => {
                result.set(3, engine_string(ctx, &[*v])?)?;
                "character"
            }
            D::String(v) => {
                result.set(3, engine_string(ctx, v)?)?;
                "string"
            }
            D::Symbol(n) | D::Keyword(n) => {
                result.set(3, n.namespace().map(|s| engine_string(ctx, s)).transpose()?)?;
                result.set(4, engine_string(ctx, n.name())?)?;
                if matches!(data.kind(), D::Symbol(_)) {
                    "symbol"
                } else {
                    "keyword"
                }
            }
            D::Collection { kind, children } => {
                let items = Array::new(ctx.clone())?;
                for (i, child) in children.iter().enumerate() {
                    items.set(
                        i,
                        self.encode_node(
                            ctx,
                            child,
                            control,
                            depth - 1,
                            remaining,
                            (provenance, sharing.as_deref_mut()),
                        )?,
                    )?;
                }
                result.set(3, items)?;
                match kind {
                    C::List | C::AnonymousFn => "list",
                    C::Vector => "vector",
                    C::Map => "map",
                    C::Set => "set",
                }
            }
            D::CompilerLiteral {
                kind: L::BigInteger,
                payload,
            } => {
                let D::String(text) = payload.kind() else {
                    return Err(error("invalid Number payload"));
                };
                let number = String::from_utf16(text)
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok())
                    .ok_or_else(|| error("invalid Number payload"))?;
                result.set(3, number)?;
                "number"
            }
            D::CompilerLiteral { kind, payload } => {
                result.set(
                    3,
                    self.encode_node(
                        ctx,
                        payload,
                        control,
                        depth - 1,
                        remaining,
                        (provenance, sharing.as_deref_mut()),
                    )?,
                )?;
                match kind {
                    L::Regex => "regex",
                    L::Instant => "instant",
                    L::Uuid => "uuid",
                    L::JsArray => "js-array",
                    L::JsObject => "js-object",
                    L::Queue => "queue",
                    L::BigInteger => unreachable!(),
                }
            }
        };
        result.set(0, tag)?;
        if let Some(sharing) = sharing
            && matches!(data.kind(), D::Collection { .. })
            && immutable(data)
            && !self.environment_children.contains_key(&data.identity())
        {
            let slot = if self.environment_children.len() < ENVIRONMENT_CHILD_SLOTS {
                ENVIRONMENT_SLOTS + self.environment_children.len()
            } else {
                let old = self.environment_child_order.pop_front().unwrap();
                self.environment_children.remove(&old).unwrap().slot
            };
            let (nodes, depth) = dimensions(data);
            self.environment_children.insert(
                data.identity(),
                EnvironmentEntry {
                    data: data.clone(),
                    slot,
                    nodes,
                    depth,
                },
            );
            self.environment_child_order.push_back(data.identity());
            sharing.updates.push((slot, result.clone()));
        }
        Ok(result)
    }
    pub(crate) fn decode(
        &mut self,
        descriptor: Array<'_>,
        control: &ExecutionControl,
        limits: ConversionLimits,
        snapshot: bool,
    ) -> rquickjs::Result<Datum> {
        self.forms.clear();
        let mut remaining = limits.nodes;
        self.decode_node(descriptor, control, limits.depth, &mut remaining, snapshot)
            .map(|(resolved, _)| resolved)
    }
    fn decode_node(
        &mut self,
        d: Array<'_>,
        control: &ExecutionControl,
        depth: usize,
        remaining: &mut usize,
        snapshot: bool,
    ) -> rquickjs::Result<(Datum, Datum)> {
        control.poll().map_err(|e| error(e.0))?;
        if depth == 0 || *remaining == 0 {
            control.invalidate();
            return Err(error("compiler output conversion limit"));
        }
        *remaining -= 1;
        let tag: String = d.get(0)?;
        let id: Option<Class<'_, BorrowedDatum>> = d.get(1)?;
        let metadata: Option<Array<'_>> = d.get(2)?;
        let (mut metadata_value, mut canonical_metadata) = (None, None);
        if let Some(meta) = metadata {
            let (m, c) = self.decode_node(meta, control, depth - 1, remaining, snapshot)?;
            metadata_value = Some(m);
            canonical_metadata = Some(c);
        }
        let (kind, canonical_kind) = match tag.as_str() {
            "nil" => (D::Nil, D::Nil),
            "bool" => {
                let v = d.get(3)?;
                (D::Boolean(v), D::Boolean(v))
            }
            "number" => {
                let v = d.get(3)?;
                (D::Double(v), D::Double(v))
            }
            "string" | "character" => {
                let v = string_units(d.get(3)?)?;
                (D::String(v.clone().into()), D::String(v.into()))
            }
            "symbol" | "keyword" => {
                let ns = d
                    .get::<Option<rquickjs::String<'_>>>(3)?
                    .map(string_units)
                    .transpose()?;
                let name = string_units(d.get(4)?)?;
                let name = crate::compiler_data::Name::from_utf16(ns.as_deref(), &name);
                let kind = if tag == "symbol" {
                    D::Symbol(name)
                } else {
                    D::Keyword(name)
                };
                (kind.clone(), kind)
            }
            "list" | "vector" | "map" | "set" => {
                let children: Array<'_> = d.get(3)?;
                let (mut values, mut canonical) = (vec![], vec![]);
                for child in children.iter::<Array<'_>>() {
                    let (v, c) =
                        self.decode_node(child?, control, depth - 1, remaining, snapshot)?;
                    values.push(v);
                    canonical.push(c);
                }
                let kind = match tag.as_str() {
                    "list" => C::List,
                    "vector" => C::Vector,
                    "map" => C::Map,
                    _ => C::Set,
                };
                if kind == C::Map && values.len() % 2 != 0 {
                    return Err(error("odd map descriptor"));
                }
                (
                    D::Collection {
                        kind,
                        children: values.into(),
                    },
                    D::Collection {
                        kind,
                        children: canonical.into(),
                    },
                )
            }
            "regex" | "instant" | "uuid" | "js-array" | "js-object" | "queue" => {
                let (payload, canonical) =
                    self.decode_node(d.get(3)?, control, depth - 1, remaining, snapshot)?;
                let kind = match tag.as_str() {
                    "regex" => L::Regex,
                    "instant" => L::Instant,
                    "uuid" => L::Uuid,
                    "js-array" => L::JsArray,
                    "js-object" => L::JsObject,
                    _ => L::Queue,
                };
                if kind == L::Uuid {
                    let D::String(text) = canonical.kind() else {
                        return Err(error("invalid UUID payload"));
                    };
                    let valid = String::from_utf16(text)
                        .ok()
                        .and_then(|s| crate::reader_literals::uuid(&s));
                    if valid.is_none() {
                        return Err(error("unsupported invalid UUID at $.payload"));
                    }
                }
                (
                    D::CompilerLiteral { kind, payload },
                    D::CompilerLiteral {
                        kind,
                        payload: canonical,
                    },
                )
            }
            _ => return Err(error("unknown compiler output descriptor")),
        };
        let mut value = Datum::new(kind);
        let mut canonical = Datum::new(canonical_kind);
        if let Some(meta) = metadata_value {
            value = value.with_metadata(meta);
        }
        if let Some(meta) = canonical_metadata {
            canonical = canonical.with_metadata(meta);
        }
        if let Some(id) = id {
            let mut borrowed = id.borrow_mut();
            if snapshot {
                borrowed.canonical = Some(canonical.clone());
            }
            if borrowed
                .canonical
                .as_ref()
                .is_some_and(|c| exact(c, &canonical))
            {
                value = borrowed.original.clone();
                if !snapshot && let Some(form) = &borrowed.form {
                    // Reusing a collection restores its original children too,
                    // including scalar spellings collapsed by JS Number (1M,
                    // 1.0, 1). Their individual exported IDs can be ambiguous.
                    self.retain(form);
                }
            }
        }
        Ok((value, canonical))
    }
}
pub(crate) fn exact(a: &Datum, b: &Datum) -> bool {
    if a.identity() == b.identity() {
        return true;
    }
    let data = match (a.kind(), b.kind()) {
        (D::Nil, D::Nil) => true,
        (D::Boolean(a), D::Boolean(b)) => a == b,
        (D::Double(a), D::Double(b)) => a.to_bits() == b.to_bits(),
        (D::String(a), D::String(b)) => a == b,
        (D::Symbol(a), D::Symbol(b)) | (D::Keyword(a), D::Keyword(b)) => a == b,
        (
            D::Collection {
                kind: a,
                children: ac,
            },
            D::Collection {
                kind: b,
                children: bc,
            },
        ) => a == b && ac.len() == bc.len() && ac.iter().zip(bc).all(|(a, b)| exact(a, b)),
        (
            D::CompilerLiteral {
                kind: a,
                payload: ac,
            },
            D::CompilerLiteral {
                kind: b,
                payload: bc,
            },
        ) => a == b && exact(ac, bc),
        _ => false,
    };
    data && match (a.metadata(), b.metadata()) {
        (None, None) => true,
        (Some(a), Some(b)) => exact(&a, &b),
        _ => false,
    }
}

#[cfg(light_quickjs)]
fn immutable(data: &Datum) -> bool {
    data.metadata().is_none_or(|m| immutable(&m))
        && match data.kind() {
            D::CompilerLiteral { .. } => false,
            D::Collection { children, .. } => children.iter().all(immutable),
            _ => true,
        }
}

#[cfg(light_quickjs)]
fn dimensions(data: &Datum) -> (usize, usize) {
    let mut nodes = 1;
    let mut depth = 1;
    let mut include = |child: &Datum| {
        let (n, d) = dimensions(child);
        nodes += n;
        depth = depth.max(d + 1);
    };
    if let Some(meta) = data.metadata() {
        include(&meta);
    }
    match data.kind() {
        D::Collection { children, .. } => children.iter().for_each(include),
        D::CompilerLiteral { payload, .. } => include(payload),
        _ => {}
    }
    (nodes, depth)
}
