//! Compiler macro sessions in a JavaScript engine that the embedder supplies
//! (light-cljs-307, a prototype). The browser playground uses it to run
//! macros in the browser's own engine instead of in QuickJS inside the
//! WebAssembly module: [`MacroHost::set_script_host`] selects it for clean
//! compilations, and docs/browser-macros.md records what it preserves, what
//! it gives up and what it measured.
//!
//! The session loads the same services as the V8 evaluator
//! (`src/v8_services.js` over the canonical runtime) into the host engine
//! and drives them through [`ScriptHost`] calls whose arguments and answers
//! are JSON texts holding the bridge's ABI 5 descriptors. A provenance is a
//! positive integer naming the session's borrowed compiler data, so a value
//! a macro returns unchanged is the compiler's original form again. Compiler
//! services the runtime calls (`__lightCompileUnit`, `__lightBuildService`)
//! come back synchronously through [`serve`].
//!
//! [`MacroHost::set_script_host`]: crate::macro_host::MacroHost::set_script_host
use crate::{
    CollectionKind as C,
    compiler_data::{CompilerLiteralKind as L, Datum, DatumKind as D, ExpansionOrigin, Form},
    compiler_evaluator::{CompilerEvaluator, StringService, UnitService},
    evaluator::{
        EvalError, EvaluatorOptions, ExecutionControl, MacroEvaluator, NEXT_SESSION,
        RUNTIME_SOURCE, ValueId,
    },
    form_bridge::ConversionLimits,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    fmt::Write as _,
    rc::{Rc, Weak},
    sync::atomic::Ordering,
};

/// A JavaScript engine outside this crate that runs compiler macro sessions.
/// Every call is synchronous; the engine may call [`serve`] while a call is
/// running, and that call may call back into the engine again.
pub trait ScriptHost {
    /// The engine's name and version, which keys cached compiler artifacts.
    fn engine(&self) -> String;
    /// Open `session`: run `services` as a classic script and keep its
    /// completion value, the session's service object.
    fn open(&self, session: u64, services: &str) -> Result<(), String>;
    /// Call the session's service `name` with the elements of the JSON array
    /// `arguments`; answer the result as JSON. Numbers JSON cannot spell
    /// cross both ways as `{"$n": "NaN"}` (also `Infinity`, `-Infinity`
    /// and `-0`). An exception answers its `String` value as the error.
    fn call(&self, session: u64, name: &str, arguments: &str) -> Result<String, String>;
    /// Define the global function `name`, which calls
    /// [`serve`]`(session, index, strings, request)` and returns its answer.
    /// With `strings` its one argument must be a string (a TypeError
    /// otherwise); without, a nonnegative safe integer Number, passed as
    /// decimal text.
    fn install(&self, session: u64, name: &str, index: usize, strings: bool) -> Result<(), String>;
    /// Forget the session's service object.
    fn close(&self, session: u64);
}

thread_local! {
    static SESSIONS: RefCell<HashMap<u64, Weak<State>>> = RefCell::new(HashMap::new());
    static SERVICES: String = format!(
        "{}\n__lightEvaluator",
        include_str!("v8_services.js").replace("__LIGHT_RUNTIME_SOURCE__", RUNTIME_SOURCE)
    );
}

/// Answer a compiler service the host's session called: the unit or string
/// service `index` that the session installed with
/// [`ScriptHost::install`]. A failure is terminal for the session: the
/// engine throws it, and the outer operation reports it even when
/// JavaScript catches it.
pub fn serve(session: u64, index: usize, strings: bool, request: &str) -> Result<String, String> {
    let state = SESSIONS
        .with(|sessions| sessions.borrow().get(&session).and_then(Weak::upgrade))
        .ok_or("compiler session expired")?;
    let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.control.poll().map_err(|e| e.0)?;
        let answer = if strings {
            let callback = state.strings.borrow().get(index).cloned();
            callback.ok_or("unknown compiler string service")?(request)
        } else {
            let id = request
                .parse::<u64>()
                .map_err(|_| "invalid compiler unit service ID")?;
            let callback = state.callbacks.borrow().get(index).cloned();
            let callback = callback.ok_or("unknown compiler unit service")?;
            callback(&HostEvaluator(state.clone()), id)?
        };
        state.control.poll().map_err(|e| e.0)?;
        Ok::<_, String>(answer)
    }));
    let failure = match output {
        Ok(Ok(answer)) => return Ok(answer),
        Ok(Err(error)) => EvalError(error),
        Err(_) => EvalError("compiler service panicked".into()),
    };
    state.remember_error(&failure);
    state.control.invalidate();
    Err(failure.0)
}

struct State {
    host: Rc<dyn ScriptHost>,
    control: ExecutionControl,
    generation: u64,
    bridge: RefCell<Bridge>,
    callbacks: RefCell<Vec<UnitService>>,
    strings: RefCell<Vec<StringService>>,
    pending_error: RefCell<Option<EvalError>>,
    /// What the session has registered of the compilation's namespaces
    /// ([`crate::reader_registration`]).
    reader_namespaces: Rc<RefCell<crate::reader_registration::ReaderNamespaces>>,
}
impl State {
    fn remember_error(&self, error: &EvalError) {
        let mut pending = self.pending_error.borrow_mut();
        if pending.is_none()
            || !error
                .0
                .contains("invalidated after engine or callback failure")
        {
            *pending = Some(error.clone());
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        let _ = SESSIONS.try_with(|sessions| sessions.borrow_mut().remove(&self.generation));
        self.host.close(self.generation);
    }
}

/// A compiler macro session in a [`ScriptHost`].
pub(crate) struct HostEvaluator(Rc<State>);

impl HostEvaluator {
    pub(crate) fn new(
        host: Rc<dyn ScriptHost>,
        options: &EvaluatorOptions,
    ) -> Result<Self, EvalError> {
        let control = ExecutionControl::new(options)?;
        control.poll()?;
        let generation = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
        let state = Rc::new(State {
            host,
            control,
            generation,
            bridge: RefCell::new(Bridge::default()),
            callbacks: RefCell::new(Vec::new()),
            strings: RefCell::new(Vec::new()),
            pending_error: RefCell::new(None),
            reader_namespaces: Rc::default(),
        });
        SESSIONS.with(|sessions| {
            sessions
                .borrow_mut()
                .insert(generation, Rc::downgrade(&state))
        });
        let evaluator = Self(state);
        evaluator.enter(|| {
            SERVICES.with(|services| {
                evaluator
                    .0
                    .host
                    .open(generation, services)
                    .map_err(EvalError)
            })
        })?;
        Ok(evaluator)
    }
    fn enter<T>(&self, f: impl FnOnce() -> Result<T, EvalError>) -> Result<T, EvalError> {
        let state = &self.0;
        state.control.poll()?;
        let result = f();
        if let Err(error) = state.control.poll() {
            // A failed compiler service invalidated the session, possibly
            // under a JavaScript catch: report its own diagnostic.
            let pending = state.pending_error.borrow().clone();
            return Err(match pending {
                Some(pending) if error.0.contains("invalidated") => pending,
                _ => error,
            });
        }
        if let Err(error) = &result {
            state.remember_error(error);
            state.control.invalidate();
        }
        result
    }
    fn invoke(&self, name: &str, arguments: &str) -> Result<Json, EvalError> {
        let state = &self.0;
        let answer = state
            .host
            .call(state.generation, name, arguments)
            .map_err(EvalError)?;
        Json::parse(&answer)
    }
    fn invoke_id(&self, name: &str, arguments: &str) -> Result<u64, EvalError> {
        self.invoke(name, arguments)?.id()
    }
    fn check(&self, id: ValueId) -> Result<u64, EvalError> {
        if id.session == self.0.generation {
            Ok(id.index)
        } else {
            Err(error("Value ID belongs to another JavaScript session"))
        }
    }
    fn value(&self, index: u64) -> ValueId {
        ValueId {
            session: self.0.generation,
            index,
        }
    }
    fn release_index(&self, index: u64) {
        let _ = self.invoke("release", &format!("[{index}]"));
    }
    fn import(
        &self,
        data: &Datum,
        limits: ConversionLimits,
        service: &str,
    ) -> Result<ValueId, EvalError> {
        let index = self.enter(|| {
            let mut arguments = String::from("[");
            self.0
                .bridge
                .borrow_mut()
                .encode(&mut arguments, data, &self.0.control, limits)?;
            arguments.push(']');
            let index = self.invoke_id(service, &arguments)?;
            let snapshot = (|| {
                let snapshot = self.invoke(
                    "exportForm",
                    &format!("[{index},{},{}]", limits.nodes, limits.depth),
                )?;
                self.0
                    .bridge
                    .borrow_mut()
                    .decode(&snapshot, &self.0.control, limits, true)?;
                Ok(())
            })();
            if let Err(error) = snapshot {
                self.release_index(index);
                return Err(error);
            }
            Ok(index)
        })?;
        Ok(self.value(index))
    }
}

impl MacroEvaluator for HostEvaluator {
    fn control(&self) -> ExecutionControl {
        self.0.control.clone()
    }
    fn reader_namespaces(
        &self,
    ) -> Option<Rc<RefCell<crate::reader_registration::ReaderNamespaces>>> {
        Some(self.0.reader_namespaces.clone())
    }
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {
        let mut arguments = String::from("[");
        write_text(&mut arguments, source.encode_utf16());
        arguments.push(']');
        let index = self.enter(|| self.invoke_id("evaluate", &arguments))?;
        Ok(self.value(index))
    }
    fn compiler_frontier(&self, next: Option<usize>) -> Result<(), EvalError> {
        let arguments = next.map_or_else(|| "[null]".into(), |next| format!("[{next}]"));
        self.enter(|| self.invoke("compilerFrontier", &arguments).map(drop))
    }
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError> {
        let mut text = format!("[{},[", self.check(function)?);
        for (i, argument) in arguments.iter().enumerate() {
            if i > 0 {
                text.push(',');
            }
            let _ = write!(text, "{}", self.check(*argument)?);
        }
        text.push_str("]]");
        let index = self.enter(|| self.invoke_id("call", &text))?;
        Ok(self.value(index))
    }
    fn import_form(&self, form: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.0.bridge.borrow_mut().retain(form);
        self.import(&form.data(), limits, "importForm")
    }
    fn import_macro_form(
        &self,
        form: &Form,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        self.0.bridge.borrow_mut().retain(form);
        self.import(&form.data(), limits, "importMacroForm")
    }
    fn macro_arguments(
        &self,
        form: ValueId,
        _: &Form,
        limits: ConversionLimits,
    ) -> Result<Vec<ValueId>, EvalError> {
        let form = self.check(form)?;
        let indices = self.enter(|| {
            let result = self.invoke(
                "macroArguments",
                &format!("[{form},{},{}]", limits.nodes, limits.depth),
            )?;
            let ids = result
                .at(0)
                .items()?
                .iter()
                .map(Json::id)
                .collect::<Result<Vec<_>, _>>()?;
            let snapshots = (|| {
                for snapshot in result.at(1).items()? {
                    self.0
                        .bridge
                        .borrow_mut()
                        .decode(snapshot, &self.0.control, limits, true)?;
                }
                Ok(())
            })();
            if let Err(error) = snapshots {
                for id in ids {
                    self.release_index(id);
                }
                return Err(error);
            }
            Ok(ids)
        })?;
        Ok(indices.into_iter().map(|id| self.value(id)).collect())
    }
    fn import_data(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.import(data, limits, "importForm")
    }
    fn export_form(
        &self,
        id: ValueId,
        call: &Form,
        origin: ExpansionOrigin,
        limits: ConversionLimits,
    ) -> Result<Form, EvalError> {
        let id = self.check(id)?;
        let data = self.enter(|| {
            let descriptor = self.invoke(
                "exportForm",
                &format!("[{id},{},{}]", limits.nodes, limits.depth),
            )?;
            self.0
                .bridge
                .borrow_mut()
                .decode(&descriptor, &self.0.control, limits, false)
        })?;
        Ok(Form::from_bridge_expansion(
            data,
            call,
            origin,
            &self.0.bridge.borrow().forms,
        ))
    }
    fn release(&self, id: ValueId) -> Result<(), EvalError> {
        let id = self.check(id)?;
        self.enter(|| self.invoke("release", &format!("[{id}]")).map(drop))
    }
}

impl CompilerEvaluator for HostEvaluator {
    fn evaluate_js(&self, expression: &str) -> Result<ValueId, EvalError> {
        self.evaluate_unit(&format!("(rt) => ({expression}\n)"))
    }
    fn install_compiler_service(
        self: Rc<Self>,
        name: &str,
        callback: UnitService,
    ) -> Result<(), EvalError> {
        let index = self.0.callbacks.borrow().len();
        self.0.callbacks.borrow_mut().push(callback);
        let state = &self.0;
        self.enter(|| {
            (state.host)
                .install(state.generation, name, index, false)
                .map_err(EvalError)
        })
    }
    fn install_string_service(&self, name: &str, callback: StringService) -> Result<(), EvalError> {
        let index = self.0.strings.borrow().len();
        self.0.strings.borrow_mut().push(callback);
        let state = &self.0;
        self.enter(|| {
            (state.host)
                .install(state.generation, name, index, true)
                .map_err(EvalError)
        })
    }
    #[cfg(all(test, light_v8))]
    fn number(&self, id: ValueId) -> Result<f64, EvalError> {
        let id = self.check(id)?;
        self.enter(|| match self.invoke("number", &format!("[{id}]"))? {
            Json::Number(value) => Ok(value),
            _ => Err(error("Expected Number")),
        })
    }
}

fn error(message: &str) -> EvalError {
    EvalError(message.into())
}

/// Borrowed compiler data behind one provenance number (the V8 bridge's
/// wrapped `Provenance`). The session keeps every one until it closes: the
/// host engine has no finalization this synchronous bridge could observe.
struct Borrowed {
    original: Datum,
    canonical: Option<Datum>,
    form: Option<Form>,
}

#[derive(Default)]
struct Bridge {
    forms: HashMap<usize, Form>,
    /// Provenance `n` is `borrowed[n - 1]`.
    borrowed: Vec<Borrowed>,
}

impl Bridge {
    fn retain(&mut self, form: &Form) {
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
    fn encode(
        &mut self,
        out: &mut String,
        data: &Datum,
        control: &ExecutionControl,
        limits: ConversionLimits,
    ) -> Result<(), EvalError> {
        let result = self.encode_node(out, data, control, limits.depth, &mut limits.nodes.clone());
        self.forms.clear();
        result
    }
    fn encode_node(
        &mut self,
        out: &mut String,
        data: &Datum,
        control: &ExecutionControl,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<(), EvalError> {
        control.poll()?;
        if depth == 0 || *remaining == 0 {
            control.invalidate();
            return Err(error("compiler input conversion limit"));
        }
        *remaining -= 1;
        let tag = match data.kind() {
            D::Nil => "nil",
            D::Boolean(_) => "bool",
            D::Integer(_) | D::Double(_) | D::Ratio { .. } => "number",
            D::CompilerLiteral {
                kind: L::BigInteger,
                ..
            } => "number",
            D::Character(_) => "character",
            D::String(_) => "string",
            D::Symbol(_) => "symbol",
            D::Keyword(_) => "keyword",
            D::Collection { kind, .. } => match kind {
                C::List | C::AnonymousFn => "list",
                C::Vector => "vector",
                C::Map => "map",
                C::Set => "set",
            },
            D::CompilerLiteral { kind, .. } => match kind {
                L::Regex => "regex",
                L::Instant => "instant",
                L::Uuid => "uuid",
                L::JsArray => "js-array",
                L::JsObject => "js-object",
                L::Queue => "queue",
                L::BigInteger => unreachable!(),
            },
        };
        self.borrowed.push(Borrowed {
            original: data.clone(),
            canonical: None,
            form: self.forms.get(&data.identity()).map(Form::compact_leaf),
        });
        let _ = write!(out, "[\"{tag}\",{},", self.borrowed.len());
        match data.metadata() {
            Some(meta) => self.encode_node(out, &meta, control, depth - 1, remaining)?,
            None => out.push_str("null"),
        }
        match data.kind() {
            D::Nil => {}
            D::Boolean(value) => {
                let _ = write!(out, ",{value}");
            }
            D::Integer(value) => write_number(out, *value as f64),
            D::Double(value) => write_number(out, *value),
            D::Ratio {
                numerator,
                denominator,
            } => write_number(out, *numerator as f64 / *denominator as f64),
            D::Character(value) => {
                out.push(',');
                write_text(out, [*value]);
            }
            D::String(value) => {
                out.push(',');
                write_text(out, value.iter().copied());
            }
            D::Symbol(name) | D::Keyword(name) => {
                out.push(',');
                match name.namespace() {
                    Some(ns) => write_text(out, ns.iter().copied()),
                    None => out.push_str("null"),
                }
                out.push(',');
                write_text(out, name.name().iter().copied());
            }
            D::Collection { children, .. } => {
                out.push_str(",[");
                for (i, child) in children.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    self.encode_node(out, child, control, depth - 1, remaining)?;
                }
                out.push(']');
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
                write_number(out, value);
            }
            D::CompilerLiteral { payload, .. } => {
                out.push(',');
                self.encode_node(out, payload, control, depth - 1, remaining)?;
            }
        }
        out.push(']');
        Ok(())
    }
    fn decode(
        &mut self,
        descriptor: &Json,
        control: &ExecutionControl,
        limits: ConversionLimits,
        snapshot: bool,
    ) -> Result<Datum, EvalError> {
        self.forms.clear();
        self.decode_node(
            descriptor,
            control,
            limits.depth,
            &mut limits.nodes.clone(),
            snapshot,
        )
        .map(|(value, _)| value)
    }
    fn decode_node(
        &mut self,
        descriptor: &Json,
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
        let Json::String(tag) = descriptor.at(0) else {
            return Err(error("invalid descriptor tag"));
        };
        let tag = String::from_utf16_lossy(tag);
        let (metadata, canonical_metadata) = match descriptor.at(2) {
            Json::Null => (None, None),
            metadata => {
                let (value, canonical) =
                    self.decode_node(metadata, control, depth - 1, remaining, snapshot)?;
                (Some(value), Some(canonical))
            }
        };
        let (kind, canonical_kind) = match tag.as_str() {
            "nil" => (D::Nil, D::Nil),
            "bool" => {
                let Json::Boolean(value) = descriptor.at(3) else {
                    return Err(error("invalid bool descriptor"));
                };
                (D::Boolean(*value), D::Boolean(*value))
            }
            "number" => {
                let Json::Number(value) = descriptor.at(3) else {
                    return Err(error("Expected compiler Number"));
                };
                (D::Double(*value), D::Double(*value))
            }
            "string" | "character" => {
                let value = descriptor.at(3).text()?;
                (
                    D::String(value.to_vec().into()),
                    D::String(value.to_vec().into()),
                )
            }
            "symbol" | "keyword" => {
                let namespace = match descriptor.at(3) {
                    Json::Null => None,
                    namespace => Some(namespace.text()?),
                };
                let name =
                    crate::compiler_data::Name::from_utf16(namespace, descriptor.at(4).text()?);
                let kind = if tag == "symbol" {
                    D::Symbol(name)
                } else {
                    D::Keyword(name)
                };
                (kind.clone(), kind)
            }
            "list" | "vector" | "map" | "set" => {
                let children = descriptor.at(3).items()?;
                let mut values = Vec::with_capacity(children.len());
                let mut canonical = Vec::with_capacity(children.len());
                for child in children {
                    let (value, canon) =
                        self.decode_node(child, control, depth - 1, remaining, snapshot)?;
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
                let (payload, canonical) =
                    self.decode_node(descriptor.at(3), control, depth - 1, remaining, snapshot)?;
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
        match descriptor.at(1) {
            Json::Null => {}
            Json::Number(id) => {
                let index =
                    (*id >= 1.0 && id.fract() == 0.0 && *id as usize <= self.borrowed.len())
                        .then(|| *id as usize - 1)
                        .ok_or_else(|| error("invalid compiler provenance"))?;
                let borrowed = &mut self.borrowed[index];
                if snapshot {
                    borrowed.canonical = Some(canonical.clone());
                }
                if borrowed
                    .canonical
                    .as_ref()
                    .is_some_and(|c| crate::form_bridge::exact(c, &canonical))
                {
                    value = borrowed.original.clone();
                    if !snapshot && let Some(form) = borrowed.form.clone() {
                        self.retain(&form);
                    }
                }
            }
            _ => return Err(error("invalid compiler provenance")),
        }
        Ok((value, canonical))
    }
}

/// Write a descriptor field `,number`; JSON has no NaN, infinities or -0.
fn write_number(out: &mut String, value: f64) {
    if value.is_nan() {
        out.push_str(",{\"$n\":\"NaN\"}");
    } else if value.is_infinite() {
        out.push_str(if value > 0.0 {
            ",{\"$n\":\"Infinity\"}"
        } else {
            ",{\"$n\":\"-Infinity\"}"
        });
    } else if value == 0.0 && value.is_sign_negative() {
        out.push_str(",{\"$n\":\"-0\"}");
    } else {
        // Rust prints the shortest text that reads back as the same double.
        let _ = write!(out, ",{value}");
    }
}

/// Write UTF-16 code units as a JSON string: printable ASCII as is, every
/// other unit escaped, so unpaired surrogates survive.
fn write_text(out: &mut String, units: impl IntoIterator<Item = u16>) {
    out.push('"');
    for unit in units {
        match unit {
            0x22 => out.push_str("\\\""),
            0x5c => out.push_str("\\\\"),
            0x20..=0x7e => out.push(unit as u8 as char),
            _ => {
                let _ = write!(out, "\\u{unit:04x}");
            }
        }
    }
    out.push('"');
}

/// A parsed JSON answer. Strings are UTF-16 code units, as the engine holds
/// them; `{"$n": text}` is the number it names.
enum Json {
    Null,
    Boolean(bool),
    Number(f64),
    String(Vec<u16>),
    Array(Vec<Json>),
}

const NULL: Json = Json::Null;

impl Json {
    fn parse(text: &str) -> Result<Self, EvalError> {
        let mut parser = Parser {
            bytes: text.as_bytes(),
            at: 0,
        };
        let value = parser.value()?;
        parser.space();
        if parser.at != parser.bytes.len() {
            return Err(error("invalid host answer: trailing text"));
        }
        Ok(value)
    }
    /// Element `index` of an array; null past its end, as JavaScript reads
    /// a descriptor's absent fields.
    fn at(&self, index: usize) -> &Json {
        match self {
            Json::Array(items) => items.get(index).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
    fn items(&self) -> Result<&[Json], EvalError> {
        match self {
            Json::Array(items) => Ok(items),
            _ => Err(error("Expected compiler descriptor array")),
        }
    }
    fn text(&self) -> Result<&[u16], EvalError> {
        match self {
            Json::String(units) => Ok(units),
            _ => Err(error("Expected compiler string")),
        }
    }
    fn id(&self) -> Result<u64, EvalError> {
        match self {
            Json::Number(n) if *n >= 0.0 && n.fract() == 0.0 => Ok(*n as u64),
            _ => Err(error("Expected a value ID")),
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while matches!(self.bytes.get(self.at), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.at += 1;
        }
    }
    fn fail<T>(&self) -> Result<T, EvalError> {
        Err(EvalError(format!(
            "invalid host answer at byte {}",
            self.at
        )))
    }
    fn expect(&mut self, byte: u8) -> Result<(), EvalError> {
        self.space();
        if self.bytes.get(self.at) != Some(&byte) {
            return self.fail();
        }
        self.at += 1;
        Ok(())
    }
    fn value(&mut self) -> Result<Json, EvalError> {
        self.space();
        match self.bytes.get(self.at) {
            Some(b'n') => self.word("null", Json::Null),
            Some(b't') => self.word("true", Json::Boolean(true)),
            Some(b'f') => self.word("false", Json::Boolean(false)),
            Some(b'"') => Ok(Json::String(self.string()?)),
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                self.space();
                if self.bytes.get(self.at) == Some(&b']') {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                loop {
                    items.push(self.value()?);
                    self.space();
                    match self.bytes.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(Json::Array(items));
                        }
                        _ => return self.fail(),
                    }
                }
            }
            Some(b'{') => {
                // Only the special numbers travel as objects.
                self.at += 1;
                self.space();
                if self.string()? != "$n".encode_utf16().collect::<Vec<_>>() {
                    return self.fail();
                }
                self.expect(b':')?;
                self.space();
                let name = String::from_utf16_lossy(&self.string()?);
                self.expect(b'}')?;
                match name.as_str() {
                    "NaN" => Ok(Json::Number(f64::NAN)),
                    "Infinity" => Ok(Json::Number(f64::INFINITY)),
                    "-Infinity" => Ok(Json::Number(f64::NEG_INFINITY)),
                    "-0" => Ok(Json::Number(-0.0)),
                    _ => self.fail(),
                }
            }
            Some(b'-' | b'0'..=b'9') => {
                let start = self.at;
                while matches!(
                    self.bytes.get(self.at),
                    Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                ) {
                    self.at += 1;
                }
                let text = std::str::from_utf8(&self.bytes[start..self.at]).unwrap_or("");
                match text.parse::<f64>() {
                    Ok(value) => Ok(Json::Number(value)),
                    Err(_) => self.fail(),
                }
            }
            _ => self.fail(),
        }
    }
    fn word(&mut self, word: &str, value: Json) -> Result<Json, EvalError> {
        if !self.bytes[self.at..].starts_with(word.as_bytes()) {
            return self.fail();
        }
        self.at += word.len();
        Ok(value)
    }
    fn string(&mut self) -> Result<Vec<u16>, EvalError> {
        if self.bytes.get(self.at) != Some(&b'"') {
            return self.fail();
        }
        self.at += 1;
        let mut units = Vec::new();
        loop {
            let start = self.at;
            while !matches!(self.bytes.get(self.at), Some(b'"' | b'\\') | None) {
                self.at += 1;
            }
            // The answer is a Rust string, so each run is whole UTF-8.
            let run = std::str::from_utf8(&self.bytes[start..self.at]).unwrap_or("");
            units.extend(run.encode_utf16());
            match self.bytes.get(self.at) {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(units);
                }
                Some(b'\\') => {
                    let escape = self.bytes.get(self.at + 1).copied();
                    self.at += 2;
                    units.push(match escape {
                        Some(b'"') => 0x22,
                        Some(b'\\') => 0x5c,
                        Some(b'/') => 0x2f,
                        Some(b'b') => 0x08,
                        Some(b'f') => 0x0c,
                        Some(b'n') => 0x0a,
                        Some(b'r') => 0x0d,
                        Some(b't') => 0x09,
                        Some(b'u') => {
                            let hex = self
                                .bytes
                                .get(self.at..self.at + 4)
                                .and_then(|hex| std::str::from_utf8(hex).ok())
                                .and_then(|hex| u16::from_str_radix(hex, 16).ok());
                            let Some(unit) = hex else {
                                return self.fail();
                            };
                            self.at += 4;
                            unit
                        }
                        _ => return self.fail(),
                    });
                }
                _ => return self.fail(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host that records the units it is asked to evaluate.
    #[derive(Default)]
    struct Recording(RefCell<Vec<String>>);
    impl ScriptHost for Recording {
        fn engine(&self) -> String {
            "recording".into()
        }
        fn open(&self, _: u64, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn call(&self, _: u64, name: &str, arguments: &str) -> Result<String, String> {
            if name == "evaluate" {
                self.0.borrow_mut().push(arguments.to_owned());
                return Ok("1".into());
            }
            Ok("null".into())
        }
        fn install(&self, _: u64, _: &str, _: usize, _: bool) -> Result<(), String> {
            Ok(())
        }
        fn close(&self, _: u64) {}
    }

    #[test]
    fn hosted_reader_registration_names_only_what_changed() {
        let host = Rc::new(Recording::default());
        let evaluator = HostEvaluator::new(host.clone(), &EvaluatorOptions::default()).unwrap();
        let source = "(def x 1)";
        let program = crate::analyze(source, &crate::parse(source).unwrap()).unwrap();
        let mut tables = crate::reader_registration::ReaderTables::of(&program);
        let registrations = |host: &Recording| {
            host.0
                .borrow()
                .iter()
                .filter(|unit| unit.contains("registerReaderNamespace"))
                .cloned()
                .collect::<Vec<_>>()
        };
        crate::reader_registration::register(&evaluator, &tables).unwrap();
        crate::reader_registration::register(&evaluator, &tables).unwrap();
        assert_eq!(registrations(&host).len(), 1);
        let mut later = tables.globals[0].clone();
        later.name = "later".into();
        tables.globals.push(later);
        crate::reader_registration::register(&evaluator, &tables).unwrap();
        let units = registrations(&host);
        assert_eq!(units.len(), 2);
        assert!(units[0].contains("\\\"x\\\""), "{}", units[0]);
        assert!(
            units[1].contains("later") && !units[1].contains("\\\"x\\\""),
            "{}",
            units[1]
        );
    }

    #[test]
    fn json_answers_keep_code_units_and_special_numbers() {
        let mut out = String::from("[");
        write_text(&mut out, [0x41, 0xd800, 0x22, 0x5c, 0x0a, 0xe9]);
        write_number(&mut out, f64::NAN);
        write_number(&mut out, -0.0);
        write_number(&mut out, f64::NEG_INFINITY);
        write_number(&mut out, 0.1);
        write_number(&mut out, 1e300);
        out.push(']');
        let Json::Array(items) = Json::parse(&out).unwrap() else {
            panic!("array expected")
        };
        assert_eq!(
            items[0].text().unwrap(),
            [0x41, 0xd800, 0x22, 0x5c, 0x0a, 0xe9]
        );
        let numbers: Vec<f64> = items[1..]
            .iter()
            .map(|item| match item {
                Json::Number(n) => *n,
                _ => panic!("number expected"),
            })
            .collect();
        assert!(numbers[0].is_nan());
        assert!(numbers[1] == 0.0 && numbers[1].is_sign_negative());
        assert_eq!(numbers[2..], [f64::NEG_INFINITY, 0.1, 1e300]);
        // JSON.stringify writes non-ASCII text raw and escapes lone surrogates.
        let Json::String(units) = Json::parse("\"é😀\\udc00\"").unwrap() else {
            panic!("string expected")
        };
        assert_eq!(units, [0xe9, 0xd83d, 0xde00, 0xdc00]);
        assert!(Json::parse("[1,]").is_err());
        assert!(Json::parse("{\"x\":1}").is_err());
    }
}
