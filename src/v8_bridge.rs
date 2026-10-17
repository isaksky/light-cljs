//! Structured compiler-form transport for V8 macro sessions (ABI 5).
use crate::{
    CollectionKind as C,
    compiler_data::{CompilerLiteralKind as L, Datum, DatumKind as D, Form},
    evaluator::{EvalError, ExecutionControl},
    form_bridge::ConversionLimits,
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    rc::Rc,
};
use v8::{Local, PinScope};

pub(crate) fn error(message: &str) -> EvalError {
    EvalError(message.into())
}
pub(crate) fn string<'s>(
    scope: &PinScope<'s, '_>,
    text: &str,
) -> Result<Local<'s, v8::String>, EvalError> {
    v8::String::new(scope, text).ok_or_else(|| error("V8 string allocation failed"))
}
fn units<'s>(scope: &PinScope<'s, '_>, text: &[u16]) -> Result<Local<'s, v8::String>, EvalError> {
    v8::String::new_from_two_byte(scope, text, v8::NewStringType::Normal)
        .ok_or_else(|| error("V8 UTF-16 allocation failed"))
}
pub(crate) fn get<'s>(
    scope: &mut PinScope<'s, '_>,
    array: Local<'s, v8::Array>,
    index: u32,
) -> Result<Local<'s, v8::Value>, EvalError> {
    array
        .get_index(scope, index)
        .ok_or_else(|| error("V8 descriptor read failed"))
}
pub(crate) fn array(value: Local<'_, v8::Value>) -> Result<Local<'_, v8::Array>, EvalError> {
    value
        .try_into()
        .map_err(|_| error("Expected compiler descriptor array"))
}
pub(crate) fn number(
    scope: &PinScope<'_, '_>,
    value: Local<'_, v8::Value>,
) -> Result<f64, EvalError> {
    if !value.is_number() {
        return Err(error("Expected compiler Number"));
    }
    value
        .number_value(scope)
        .ok_or_else(|| error("Invalid compiler Number"))
}
fn text_units(
    scope: &PinScope<'_, '_>,
    value: Local<'_, v8::Value>,
) -> Result<Vec<u16>, EvalError> {
    let text: Local<v8::String> = value
        .try_into()
        .map_err(|_| error("Expected compiler string"))?;
    let mut result = vec![0; text.length()];
    text.write_v2(scope, 0, &mut result, v8::WriteFlags::empty());
    Ok(result)
}
pub(crate) fn put<'s>(
    scope: &mut PinScope<'s, '_>,
    array: Local<'s, v8::Array>,
    index: u32,
    value: Local<'s, v8::Value>,
) -> Result<(), EvalError> {
    array
        .set_index(scope, index, value)
        .filter(|v| *v)
        .map(|_| ())
        .ok_or_else(|| error("V8 descriptor write failed"))
}

struct BorrowedDatum {
    original: Datum,
    canonical: Option<Datum>,
    form: Option<Form>,
}
struct Provenance {
    payload: RefCell<BorrowedDatum>,
    external: Rc<Cell<i64>>,
    charge: i64,
}
impl Drop for Provenance {
    fn drop(&mut self) {
        self.external.set(self.external.get() - self.charge);
    }
}
// The evaluator configures atomic cppgc marking and sweeping. Payloads own no
// engine handles and are only accessed or dropped on this heap's owning thread.
unsafe impl v8::cppgc::GarbageCollected for Provenance {
    fn trace(&self, _: &mut v8::cppgc::Visitor) {}
    fn get_name(&self) -> &'static std::ffi::CStr {
        c"CompilerProvenance"
    }
}
const TAG: u16 = 1;
fn empty(_: &mut PinScope<'_, '_>, _: v8::FunctionCallbackArguments, _: v8::ReturnValue) {}

const ENVIRONMENT_SLOTS: usize = 64;
const ENVIRONMENT_CHILD_SLOTS: usize = 128;
#[derive(Clone)]
struct EnvironmentEntry {
    data: Datum,
    slot: usize,
    nodes: usize,
    depth: usize,
}
struct ChildImport<'s> {
    previous: HashMap<usize, EnvironmentEntry>,
    updates: Vec<(usize, Local<'s, v8::Array>)>,
}
#[derive(Default)]
pub(crate) struct BridgeState {
    pub forms: HashMap<usize, Form>,
    pub importing_environment: bool,
    environments: Vec<EnvironmentEntry>,
    environment_children: HashMap<usize, EnvironmentEntry>,
    environment_child_order: VecDeque<usize>,
    constructor: Option<v8::Global<v8::ObjectTemplate>>,
    external: Rc<Cell<i64>>,
    external_reported: i64,
}
impl BridgeState {
    pub(crate) fn external_estimate(&self) -> i64 {
        self.external.get()
    }
    // GC heuristic only, not a hard allocation budget: cppgc sees the inline
    // wrapper but cannot see its retained Rust Datum/Form allocations.
    pub(crate) fn sync_external(&mut self, scope: &mut PinScope<'_, '_>) {
        let current = self.external.get();
        let delta = current - self.external_reported;
        self.external_reported = current;
        if delta != 0 {
            scope.adjust_amount_of_external_allocated_memory(delta);
        }
    }
    pub(crate) fn clear_engine_roots(&mut self) {
        self.constructor.take();
    }
    pub(crate) fn reset_environment_cache(&mut self) {
        self.environments.clear();
        self.reset_environment_children();
    }
    pub(crate) fn reset_environment_children(&mut self) {
        self.environment_children.clear();
        self.environment_child_order.clear();
    }
    fn constructor<'s>(
        &mut self,
        scope: &mut PinScope<'s, '_>,
    ) -> Result<Local<'s, v8::ObjectTemplate>, EvalError> {
        if let Some(constructor) = &self.constructor {
            return Ok(Local::new(scope, constructor));
        }
        let constructor = v8::FunctionTemplate::new(scope, empty).instance_template(scope);
        self.constructor = Some(v8::Global::new(scope, constructor));
        Ok(constructor)
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
    pub(crate) fn encode<'s>(
        &mut self,
        scope: &mut PinScope<'s, '_>,
        data: &Datum,
        control: &ExecutionControl,
        limits: ConversionLimits,
    ) -> Result<Local<'s, v8::Array>, EvalError> {
        let constructor = self.constructor(scope)?;
        let result = self.encode_node(
            scope,
            data,
            control,
            limits.depth,
            &mut limits.nodes.clone(),
            constructor,
            (true, None),
        );
        self.forms.clear();
        result
    }
    pub(crate) fn encode_environment<'s>(
        &mut self,
        scope: &mut PinScope<'s, '_>,
        data: &Datum,
        control: &ExecutionControl,
        limits: ConversionLimits,
    ) -> Result<Local<'s, v8::Array>, EvalError> {
        let D::Collection {
            kind: C::Map,
            children,
        } = data.kind()
        else {
            return self.encode(scope, data, control, limits);
        };
        if data.metadata().is_some() {
            return self.encode(scope, data, control, limits);
        }
        control.poll()?;
        if limits.depth < 2 || limits.nodes == 0 {
            control.invalidate();
            return Err(error("compiler input conversion limit"));
        }
        let constructor = self.constructor(scope)?;
        let mut remaining = limits.nodes - 1;
        let mut sharing = ChildImport {
            previous: self.environment_children.clone(),
            updates: Vec::new(),
        };
        let items = v8::Array::new(scope, 0);
        for (i, child) in children.iter().enumerate() {
            let cached = (i % 2 == 1)
                .then(|| {
                    self.environments
                        .iter()
                        .position(|entry| entry.data.identity() == child.identity())
                })
                .flatten();
            let descriptor = if let Some(position) = cached {
                let entry = &self.environments[position];
                if entry.nodes > remaining || entry.depth >= limits.depth {
                    control.invalidate();
                    return Err(error("compiler input conversion limit"));
                }
                remaining -= entry.nodes;
                let entry = self.environments.remove(position);
                let reference = environment_reference(scope, entry.slot)?;
                self.environments.push(entry);
                reference
            } else {
                let descriptor = self.encode_node(
                    scope,
                    child,
                    control,
                    limits.depth - 1,
                    &mut remaining,
                    constructor,
                    (
                        i % 2 == 1,
                        if i % 2 == 1 { Some(&mut sharing) } else { None },
                    ),
                )?;
                if i % 2 == 1 && matches!(child.kind(), D::Collection { .. }) && immutable(child) {
                    let slot = if self.environments.len() < ENVIRONMENT_SLOTS {
                        self.environments.len()
                    } else {
                        self.environments.remove(0).slot
                    };
                    let slot_value = v8::Number::new(scope, slot as f64);
                    put(scope, descriptor, 5, slot_value.into())?;
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
            put(scope, items, i as u32, descriptor.into())?;
        }
        let result = v8::Array::new(scope, 0);
        let tag = string(scope, "map")?;
        put(scope, result, 0, tag.into())?;
        let nil = v8::null(scope);
        put(scope, result, 1, nil.into())?;
        put(scope, result, 2, nil.into())?;
        put(scope, result, 3, items.into())?;
        let updates = v8::Array::new(scope, 0);
        for (i, (slot, descriptor)) in sharing.updates.into_iter().enumerate() {
            let pair = v8::Array::new(scope, 0);
            let slot = v8::Number::new(scope, slot as f64);
            put(scope, pair, 0, slot.into())?;
            put(scope, pair, 1, descriptor.into())?;
            put(scope, updates, i as u32, pair.into())?;
        }
        put(scope, result, 6, updates.into())?;
        Ok(result)
    }
    // Recursion threads the scope, limits, provenance and child-cache state together.
    #[allow(clippy::too_many_arguments)]
    fn encode_node<'s>(
        &mut self,
        scope: &mut PinScope<'s, '_>,
        data: &Datum,
        control: &ExecutionControl,
        depth: usize,
        remaining: &mut usize,
        constructor: Local<'s, v8::ObjectTemplate>,
        (provenance, mut sharing): (bool, Option<&mut ChildImport<'s>>),
    ) -> Result<Local<'s, v8::Array>, EvalError> {
        control.poll()?;
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
            return environment_reference(scope, entry.slot);
        }
        *remaining -= 1;
        let nil = v8::null(scope).into();
        let mut fields = vec![nil, nil, nil];
        if provenance {
            let wrapper = constructor
                .new_instance(scope)
                .ok_or_else(|| error("V8 provenance allocation failed"))?;
            let charge = 512
                + match data.kind() {
                    D::String(value) => value.len() * 2,
                    D::Symbol(name) | D::Keyword(name) => {
                        (name.name().len() + name.namespace().map_or(0, |s| s.len())) * 2
                    }
                    D::Collection { children, .. } => children.len() * std::mem::size_of::<Datum>(),
                    _ => 0,
                } as i64;
            self.external.set(self.external.get() + charge);
            let payload = Provenance {
                external: self.external.clone(),
                charge,
                payload: RefCell::new(BorrowedDatum {
                    original: data.clone(),
                    canonical: None,
                    form: self.forms.get(&data.identity()).map(Form::compact_leaf),
                }),
            };
            let payload = unsafe {
                v8::cppgc::make_garbage_collected(scope.get_cpp_heap().unwrap(), payload)
            };
            unsafe {
                v8::Object::wrap::<TAG, Provenance>(scope, wrapper, &payload);
            }
            fields[1] = wrapper.into();
        } else {
            let nil = v8::null(scope);
            fields[1] = nil.into();
        }
        let metadata = match data.metadata() {
            Some(meta) => self
                .encode_node(
                    scope,
                    &meta,
                    control,
                    depth - 1,
                    remaining,
                    constructor,
                    (provenance, sharing.as_deref_mut()),
                )?
                .into(),
            None => v8::null(scope).into(),
        };
        fields[2] = metadata;
        let tag = match data.kind() {
            D::Nil => "nil",
            D::Boolean(value) => {
                let value = v8::Boolean::new(scope, *value).into();
                fields.push(value);
                "bool"
            }
            D::Integer(value) => {
                let value = v8::Number::new(scope, *value as f64).into();
                fields.push(value);
                "number"
            }
            D::Double(value) => {
                let value = v8::Number::new(scope, *value).into();
                fields.push(value);
                "number"
            }
            D::Ratio {
                numerator,
                denominator,
            } => {
                let value = v8::Number::new(scope, *numerator as f64 / *denominator as f64).into();
                fields.push(value);
                "number"
            }
            D::Character(value) => {
                let value = units(scope, &[*value])?.into();
                fields.push(value);
                "character"
            }
            D::String(value) => {
                let value = units(scope, value)?.into();
                fields.push(value);
                "string"
            }
            D::Symbol(name) | D::Keyword(name) => {
                let namespace = match name.namespace() {
                    Some(ns) => units(scope, ns)?.into(),
                    None => v8::null(scope).into(),
                };
                fields.push(namespace);
                let name = units(scope, name.name())?.into();
                fields.push(name);
                if matches!(data.kind(), D::Symbol(_)) {
                    "symbol"
                } else {
                    "keyword"
                }
            }
            D::Collection { kind, children } => {
                let mut items = Vec::with_capacity(children.len());
                for child in children.iter() {
                    let child = self.encode_node(
                        scope,
                        child,
                        control,
                        depth - 1,
                        remaining,
                        constructor,
                        (provenance, sharing.as_deref_mut()),
                    )?;
                    items.push(child.into());
                }
                fields.push(v8::Array::new_with_elements(scope, &items).into());
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
                let value = String::from_utf16(text)
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok())
                    .ok_or_else(|| error("invalid Number payload"))?;
                let value = v8::Number::new(scope, value);
                fields.push(value.into());
                "number"
            }
            D::CompilerLiteral { kind, payload } => {
                let payload = self.encode_node(
                    scope,
                    payload,
                    control,
                    depth - 1,
                    remaining,
                    constructor,
                    (provenance, sharing.as_deref_mut()),
                )?;
                fields.push(payload.into());
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
        fields[0] = string(scope, tag)?.into();
        let result = v8::Array::new_with_elements(scope, &fields);
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
            sharing.updates.push((slot, result));
        }
        Ok(result)
    }
    pub(crate) fn decode<'s>(
        &mut self,
        scope: &mut PinScope<'s, '_>,
        descriptor: Local<'s, v8::Array>,
        control: &ExecutionControl,
        limits: ConversionLimits,
        snapshot: bool,
    ) -> Result<Datum, EvalError> {
        self.forms.clear();
        self.decode_node(
            scope,
            descriptor,
            control,
            limits.depth,
            &mut limits.nodes.clone(),
            snapshot,
        )
        .map(|(value, _)| value)
    }
    fn decode_node<'s>(
        &mut self,
        scope: &mut PinScope<'s, '_>,
        descriptor: Local<'s, v8::Array>,
        control: &ExecutionControl,
        depth: usize,
        remaining: &mut usize,
        snapshot: bool,
    ) -> Result<(Datum, Datum), EvalError> {
        control.poll()?;
        if depth == 0 || *remaining == 0 {
            control.invalidate();
            return Err(error("compiler output conversion limit"));
        }
        *remaining -= 1;
        let tag = get(scope, descriptor, 0)?;
        let tag: Local<v8::String> = tag
            .try_into()
            .map_err(|_| error("invalid descriptor tag"))?;
        let tag = tag.to_rust_string_lossy(scope);
        let id = get(scope, descriptor, 1)?;
        let metadata = get(scope, descriptor, 2)?;
        let (metadata, canonical_metadata) = if metadata.is_null_or_undefined() {
            (None, None)
        } else {
            let (value, canonical) = self.decode_node(
                scope,
                array(metadata)?,
                control,
                depth - 1,
                remaining,
                snapshot,
            )?;
            (Some(value), Some(canonical))
        };
        let (kind, canonical_kind) = match tag.as_str() {
            "nil" => (D::Nil, D::Nil),
            "bool" => {
                let value = get(scope, descriptor, 3)?;
                if !value.is_boolean() {
                    return Err(error("invalid bool descriptor"));
                }
                let value = value.is_true();
                (D::Boolean(value), D::Boolean(value))
            }
            "number" => {
                let value = get(scope, descriptor, 3)?;
                let value = number(scope, value)?;
                (D::Double(value), D::Double(value))
            }
            "string" | "character" => {
                let value = get(scope, descriptor, 3)?;
                let value = text_units(scope, value)?;
                (D::String(value.clone().into()), D::String(value.into()))
            }
            "symbol" | "keyword" => {
                let namespace = get(scope, descriptor, 3)?;
                let namespace = if namespace.is_null_or_undefined() {
                    None
                } else {
                    Some(text_units(scope, namespace)?)
                };
                let name = get(scope, descriptor, 4)?;
                let name = text_units(scope, name)?;
                let name = crate::compiler_data::Name::from_utf16(namespace.as_deref(), &name);
                let kind = if tag == "symbol" {
                    D::Symbol(name)
                } else {
                    D::Keyword(name)
                };
                (kind.clone(), kind)
            }
            "list" | "vector" | "map" | "set" => {
                let children = get(scope, descriptor, 3)?;
                let children = array(children)?;
                let mut values = Vec::with_capacity(children.length() as usize);
                let mut canonical = Vec::with_capacity(children.length() as usize);
                for i in 0..children.length() {
                    let child = get(scope, children, i)?;
                    let (value, canon) = self.decode_node(
                        scope,
                        array(child)?,
                        control,
                        depth - 1,
                        remaining,
                        snapshot,
                    )?;
                    values.push(value);
                    canonical.push(canon);
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
                let payload = get(scope, descriptor, 3)?;
                let (payload, canonical) = self.decode_node(
                    scope,
                    array(payload)?,
                    control,
                    depth - 1,
                    remaining,
                    snapshot,
                )?;
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
                    if String::from_utf16(text)
                        .ok()
                        .and_then(|s| crate::reader_literals::uuid(&s))
                        .is_none()
                    {
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
        if let Some(metadata) = metadata {
            value = value.with_metadata(metadata);
        }
        if let Some(metadata) = canonical_metadata {
            canonical = canonical.with_metadata(metadata);
        }
        if !id.is_null_or_undefined() {
            let id: Local<v8::Object> = id
                .try_into()
                .map_err(|_| error("invalid compiler provenance"))?;
            if !id.is_api_wrapper() {
                return Err(error("invalid compiler provenance object"));
            }
            let payload = unsafe { v8::Object::unwrap::<TAG, Provenance>(scope, id) }
                .ok_or_else(|| error("invalid compiler provenance tag"))?;
            let mut borrowed = unsafe { payload.as_ref() }.payload.borrow_mut();
            if snapshot {
                borrowed.canonical = Some(canonical.clone());
            }
            if borrowed
                .canonical
                .as_ref()
                .is_some_and(|c| crate::form_bridge::exact(c, &canonical))
            {
                value = borrowed.original.clone();
                if !snapshot && let Some(form) = &borrowed.form {
                    self.retain(form);
                }
            }
        }
        Ok((value, canonical))
    }
}
fn environment_reference<'s>(
    scope: &mut PinScope<'s, '_>,
    slot: usize,
) -> Result<Local<'s, v8::Array>, EvalError> {
    let result = v8::Array::new(scope, 0);
    let tag = string(scope, "environment")?;
    put(scope, result, 0, tag.into())?;
    let slot = v8::Number::new(scope, slot as f64);
    put(scope, result, 3, slot.into())?;
    Ok(result)
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
