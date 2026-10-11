//! Structured compiler/engine transport. Descriptors are actual engine arrays;
//! no printing, reader invocation, serde or JSON participates in conversion.
use crate::CollectionKind as C;
use crate::compiler_data::{CompilerLiteralKind as L, Datum, DatumKind as D, Form};
use crate::evaluator::ExecutionControl;
use rquickjs::{Array, Ctx};
use std::collections::HashMap;

/// In-process descriptor ABI version shared with the canonical runtime.
pub const FORM_BRIDGE_VERSION: u32 = 1;

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
    originals: Vec<Datum>,
    identities: HashMap<usize, usize>,
    canonical: HashMap<usize, Datum>,
    pub(crate) forms: HashMap<usize, Form>,
}
fn error(message: impl Into<String>) -> rquickjs::Error {
    rquickjs::Error::new_from_js_message("compiler descriptor", "form", message.into())
}
impl BridgeState {
    pub(crate) fn retain(&mut self, form: &Form) {
        self.forms.insert(form.data().identity(), form.clone());
        for child in form.children() {
            self.retain(&child);
        }
        if let Some(metadata) = form.metadata() {
            self.retain(&metadata);
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
        self.encode_node(ctx, data, control, limits.depth, &mut remaining)
    }
    fn encode_node<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        data: &Datum,
        control: &ExecutionControl,
        depth: usize,
        remaining: &mut usize,
    ) -> rquickjs::Result<Array<'js>> {
        control.poll().map_err(|e| error(e.0))?;
        if depth == 0 || *remaining == 0 {
            control.invalidate();
            return Err(error("compiler input conversion limit"));
        }
        *remaining -= 1;
        let id = if let Some(id) = self.identities.get(&data.identity()) {
            *id
        } else {
            let id = self.originals.len() + 1;
            self.originals.push(data.clone());
            self.identities.insert(data.identity(), id);
            id
        };
        let result = Array::new(ctx.clone())?;
        result.set(1, id)?;
        if let Some(meta) = data.metadata() {
            result.set(
                2,
                self.encode_node(ctx, &meta, control, depth - 1, remaining)?,
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
                        self.encode_node(ctx, child, control, depth - 1, remaining)?,
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
                    self.encode_node(ctx, payload, control, depth - 1, remaining)?,
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
        let id: usize = d.get(1)?;
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
        if id != 0 {
            let original = self
                .originals
                .get(id - 1)
                .ok_or_else(|| error("unknown borrowed form identity"))?;
            if snapshot {
                self.canonical.insert(id, canonical.clone());
            }
            if self
                .canonical
                .get(&id)
                .is_some_and(|c| exact(c, &canonical))
            {
                value = original.clone();
            }
        }
        Ok((value, canonical))
    }
}
fn exact(a: &Datum, b: &Datum) -> bool {
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
