//! Structured compiler/engine transport. Descriptors are actual engine arrays;
//! no printing, reader invocation, serde or JSON participates in conversion.
use crate::CollectionKind as C;
use crate::compiler_data::{CompilerLiteralKind as L, Datum, DatumKind as D, Form};
use crate::evaluator::ExecutionControl;
use rquickjs::class::{JsClass, Trace, Tracer, Writable};
use rquickjs::{Array, Class, Ctx, JsLifetime};
use std::collections::HashMap;

/// In-process descriptor ABI version shared with the canonical runtime.
pub const FORM_BRIDGE_VERSION: u32 = 2;

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
pub(crate) struct BridgeState {
    pub(crate) forms: HashMap<usize, Form>,
    pub(crate) importing_environment: bool,
    // A bounded cache of immutable environment components. Holding the Datum
    // prevents allocator address reuse from masquerading as an identity hit.
    environments: Vec<EnvironmentEntry>,
    #[cfg(test)]
    pub(crate) live_provenance: std::rc::Rc<std::cell::Cell<usize>>,
}
struct EnvironmentEntry {
    data: Datum,
    slot: usize,
    nodes: usize,
    depth: usize,
}

/// The engine owns object provenance through its weak map of imported objects;
/// scalars retain their latest provenance by value. There are no engine edges
/// here: collecting an object releases its Rust data, while a macro that saves
/// an input keeps its source and lexical spelling.
struct BorrowedDatum {
    original: Datum,
    canonical: Option<Datum>,
    form: Option<Form>,
    #[cfg(test)]
    live: std::rc::Rc<std::cell::Cell<usize>>,
}
#[cfg(test)]
impl Drop for BorrowedDatum {
    fn drop(&mut self) {
        self.live.set(self.live.get() - 1);
    }
}
// SAFETY: BorrowedDatum contains only owned Rust data, with no JS lifetimes.
unsafe impl<'js> JsLifetime<'js> for BorrowedDatum {
    type Changed<'to> = Self;
}
impl<'js> Trace<'js> for BorrowedDatum {
    fn trace<'a>(&self, _: Tracer<'a, 'js>) {}
}
impl<'js> JsClass<'js> for BorrowedDatum {
    const NAME: &'static str = "CompilerProvenance";
    type Mutable = Writable;
    fn constructor(_: &Ctx<'js>) -> rquickjs::Result<Option<rquickjs::function::Constructor<'js>>> {
        Ok(None)
    }
}
fn error(message: impl Into<String>) -> rquickjs::Error {
    rquickjs::Error::new_from_js_message("compiler descriptor", "form", message.into())
}
impl BridgeState {
    pub(crate) fn reset_environment_cache(&mut self) {
        // A failed import may have installed only some engine slots. Never
        // reference them on a later interactive operation; refill from slot 0.
        self.environments.clear();
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
        let result = self.encode_node(ctx, data, control, limits.depth, &mut remaining, true);
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
            return self.encode_node(ctx, data, control, limits.depth, &mut remaining, true);
        };
        if data.metadata().is_some() {
            let mut remaining = limits.nodes;
            return self.encode_node(ctx, data, control, limits.depth, &mut remaining, true);
        }
        control.poll().map_err(|e| error(e.0))?;
        if limits.depth < 2 || limits.nodes == 0 {
            control.invalidate();
            return Err(error("compiler input conversion limit"));
        }
        let mut remaining = limits.nodes - 1;
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
                    i % 2 == 1,
                )?;
                if i % 2 == 1 && matches!(child.kind(), D::Collection { .. }) && immutable(child) {
                    let slot = if self.environments.len() < 64 {
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
        Ok(result)
    }
    fn encode_node<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        data: &Datum,
        control: &ExecutionControl,
        depth: usize,
        remaining: &mut usize,
        provenance: bool,
    ) -> rquickjs::Result<Array<'js>> {
        control.poll().map_err(|e| error(e.0))?;
        if depth == 0 || *remaining == 0 {
            control.invalidate();
            return Err(error("compiler input conversion limit"));
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
                self.encode_node(ctx, &meta, control, depth - 1, remaining, provenance)?,
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
                result.set(3, vec![*v])?;
                "character"
            }
            D::String(v) => {
                result.set(3, v.to_vec())?;
                "string"
            }
            D::Symbol(n) | D::Keyword(n) => {
                result.set(3, n.namespace().map(|s| s.to_vec()))?;
                result.set(4, n.name().to_vec())?;
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
                        self.encode_node(ctx, child, control, depth - 1, remaining, provenance)?,
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
                    self.encode_node(ctx, payload, control, depth - 1, remaining, provenance)?,
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
                let v: Vec<u16> = d.get(3)?;
                (D::String(v.clone().into()), D::String(v.into()))
            }
            "symbol" | "keyword" => {
                let ns: Option<Vec<u16>> = d.get(3)?;
                let name: Vec<u16> = d.get(4)?;
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
fn exact(a: &Datum, b: &Datum) -> bool {
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

fn immutable(data: &Datum) -> bool {
    data.metadata().is_none_or(|m| immutable(&m))
        && match data.kind() {
            D::CompilerLiteral { .. } => false,
            D::Collection { children, .. } => children.iter().all(immutable),
            _ => true,
        }
}

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
