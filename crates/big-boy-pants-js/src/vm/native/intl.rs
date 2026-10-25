//! Saved stages for Locale construction and the shared CanonicalizeLocaleList.
//! Getters and object coercions always return to the native trampoline.
use super::*;
use crate::heap::HeapObject;
use crate::intl::{
    locale,
    tag::{self, LanguageTag},
};
use crate::object::OrdinaryObject;

const OPTIONS: &[&str] = &[
    "language",
    "script",
    "region",
    "calendar",
    "collation",
    "hourCycle",
    "caseFirst",
    "numeric",
    "numberingSystem",
];

#[derive(Debug)]
enum Stage {
    Tag,
    TagString,
    Options,
    OptionRead,
    OptionValue,
    OptionString,
    Build,
    List,
    Length,
    LengthValue,
    Element,
    ElementValue,
    ElementString,
    Done,
}

/// Shared entry point for locale lists. Later Intl constructors can embed this
/// continuation and consume its canonical array through the ordinary trampoline.
#[derive(Debug)]
pub(super) struct Intl {
    source: Value,
    options: Value,
    stage: Stage,
    tag: Option<LanguageTag>,
    option: usize,
    length: u64,
    index: u64,
    single: bool,
    result: Value,
    seen: std::collections::BTreeSet<String>,
    base_applied: bool,
}
impl Trace for Intl {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.source.trace(visit);
        self.options.trace(visit);
        self.result.trace(visit);
    }
}
impl Intl {
    pub(super) fn new(args: &[Value], construct: bool) -> Result<Self, Fault> {
        let source = args.first().cloned().unwrap_or(Value::Undefined);
        if construct && !matches!(source, Value::String(_) | Value::Object(_)) {
            return Err(Fault::TypeError(
                "locale tag must be a string or object".into(),
            ));
        }
        Ok(Self {
            source,
            options: args.get(1).cloned().unwrap_or(Value::Undefined),
            stage: if construct { Stage::Tag } else { Stage::List },
            tag: None,
            option: 0,
            length: 0,
            index: 0,
            single: false,
            result: Value::Undefined,
            seen: Default::default(),
            base_applied: false,
        })
    }
    fn value(input: Input) -> Result<Value, Fault> {
        if let Input::CallResult(value) = input {
            Ok(value)
        } else {
            Err(Fault::Internal("Intl expected callback result"))
        }
    }
    fn parse(value: &Value) -> Result<LanguageTag, Fault> {
        let Value::String(text) = value else {
            return Err(Fault::Internal("Intl expected string"));
        };
        if text.len() >= 512 {
            return Err(Fault::RangeError("invalid language tag".into()));
        }
        LanguageTag::parse(&text.to_string_lossy())
            .map_err(|_| Fault::RangeError("invalid language tag".into()))
    }
    fn read(
        &self,
        heap: &Heap,
        task: TaskId,
        base: &Value,
        key: property::Key,
    ) -> Result<Result<Value, Action>, Fault> {
        Ok(match property::read(heap, task, base, &key)? {
            Some(property::Read::Getter(callee)) => Err(Action::Call {
                callee,
                this: base.clone(),
                args: vec![],
            }),
            Some(property::Read::Value(value)) => Ok(value),
            None => Ok(Value::Undefined),
        })
    }
    fn option_value(&mut self, value: Value) -> Result<Action, Fault> {
        if matches!(value, Value::Undefined) {
            self.option += 1;
            self.stage = Stage::OptionRead;
            return Ok(Action::Continue);
        }
        let name = OPTIONS[self.option];
        if name == "numeric" {
            self.tag.as_mut().unwrap().keywords.insert(
                "kn".into(),
                if value.truthy() { "" } else { "false" }.into(),
            );
            self.option += 1;
            self.stage = Stage::OptionRead;
            return Ok(Action::Continue);
        }
        self.stage = Stage::OptionString;
        if matches!(value, Value::Object(_) | Value::BigInt(_)) {
            return Ok(Action::ToString(value));
        }
        self.apply_option(dispatch::to_js_string(&value)?.to_string_lossy())
    }
    fn apply_option(&mut self, value: String) -> Result<Action, Fault> {
        let name = OPTIONS[self.option];
        let invalid = || Fault::RangeError(format!("invalid Locale {name} option"));
        let tag = self.tag.as_mut().unwrap();
        match name {
            "language" => {
                if !tag::language(&value) {
                    return Err(invalid());
                }
                tag.language = value.to_ascii_lowercase();
            }
            "script" => {
                if !tag::script(&value) {
                    return Err(invalid());
                }
                tag.set_script(&value);
            }
            "region" => {
                if !tag::region(&value) {
                    return Err(invalid());
                }
                tag.region = Some(value.to_ascii_uppercase());
            }
            _ => {
                // GetOption's enumerated strings are case sensitive.
                if name == "hourCycle" && !["h11", "h12", "h23", "h24"].contains(&value.as_str()) {
                    return Err(invalid());
                }
                if name == "caseFirst" && !["upper", "lower", "false"].contains(&value.as_str()) {
                    return Err(invalid());
                }
                if !tag::unicode_type(&value) {
                    return Err(invalid());
                }
                let key = match name {
                    "calendar" => "ca",
                    "collation" => "co",
                    "hourCycle" => "hc",
                    "caseFirst" => "kf",
                    "numberingSystem" => "nu",
                    _ => unreachable!(),
                };
                let lower = value.to_ascii_lowercase();
                tag.keywords
                    .insert(key.into(), tag::keyword_alias(key, &lower).into());
            }
        }
        self.option += 1;
        self.stage = Stage::OptionRead;
        Ok(Action::Continue)
    }
    fn add(&mut self, heap: &Heap, task: TaskId, value: Value) -> Result<Action, Fault> {
        let canonical = Self::parse(&value)?.to_string();
        if self.seen.insert(canonical.clone()) {
            property::append(
                heap,
                task,
                &self.result,
                Some(Value::String(JsString::from(canonical.as_str()))),
            )?;
        }
        self.index += 1;
        self.stage = if self.single {
            Stage::Done
        } else {
            Stage::Element
        };
        Ok(Action::Continue)
    }
    fn element(&mut self, heap: &Heap, task: TaskId, value: Value) -> Result<Action, Fault> {
        if !matches!(value, Value::String(_) | Value::Object(_)) {
            return Err(Fault::TypeError(
                "locale list element must be a string or object".into(),
            ));
        }
        self.stage = Stage::ElementString;
        if let Some(tag) = locale::record(heap, task, &value)? {
            self.add(
                heap,
                task,
                Value::String(JsString::from(tag.to_string().as_str())),
            )
        } else if matches!(value, Value::Object(_)) {
            Ok(Action::ToString(value))
        } else {
            self.add(heap, task, value)
        }
    }
    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        match self.stage {
            Stage::Tag => {
                self.stage = Stage::TagString;
                if let Some(tag) = locale::record(heap, task, &self.source)? {
                    self.tag = Some(tag);
                    self.stage = Stage::Options;
                    Ok(Action::Continue)
                } else if matches!(self.source, Value::Object(_)) {
                    Ok(Action::ToString(self.source.clone()))
                } else {
                    self.stage = Stage::Options;
                    Ok(Action::Continue)
                }
            }
            Stage::TagString => {
                self.source = Self::value(input)?;
                self.stage = Stage::Options;
                Ok(Action::Continue)
            }
            Stage::Options => {
                if !matches!(self.options, Value::Undefined | Value::Object(_)) {
                    return Err(Fault::TypeError("Locale options must be an object".into()));
                }
                if self.tag.is_none() {
                    self.tag = Some(Self::parse(&self.source)?);
                }
                self.stage = Stage::OptionRead;
                Ok(Action::Continue)
            }
            Stage::OptionRead => {
                if self.option == 3 && !self.base_applied {
                    self.tag.as_mut().unwrap().apply_base_aliases();
                    self.base_applied = true;
                }
                if self.option == OPTIONS.len() {
                    self.stage = Stage::Build;
                    return Ok(Action::Continue);
                }
                if matches!(self.options, Value::Undefined) {
                    self.option += 1;
                    return Ok(Action::Continue);
                }
                self.stage = Stage::OptionValue;
                match self.read(
                    heap,
                    task,
                    &self.options,
                    property::Key::Name(JsString::from(OPTIONS[self.option])),
                )? {
                    Err(action) => Ok(action),
                    Ok(value) => self.option_value(value),
                }
            }
            Stage::OptionValue => self.option_value(Self::value(input)?),
            Stage::OptionString => {
                let Value::String(value) = Self::value(input)? else {
                    return Err(Fault::Internal("Intl expected option string"));
                };
                self.apply_option(value.to_string_lossy())
            }
            Stage::Build => {
                let proto = crate::realm::intrinsics(heap)
                    .ok_or(Fault::OutOfMemory("realm exhausted"))?
                    .intl_locale_prototype;
                let object = OrdinaryObject::with_prototype(heap, task, proto)
                    .with_locale_data(self.tag.take().unwrap());
                let value = heap
                    .alloc(HeapObject::Plain(object))
                    .map(Value::Object)
                    .ok_or(Fault::OutOfMemory("heap exhausted"))?;
                Ok(Action::Return(value))
            }
            Stage::List => {
                self.result = property::new_array(heap, task)?;
                // Non-string primitive wrappers have no length in this realm:
                // intrinsic prototypes are immutable, and Boolean/Symbol/BigInt
                // wrappers are not otherwise exposed. Avoid a wrapper allocation.
                if matches!(
                    self.source,
                    Value::Undefined
                        | Value::Bool(_)
                        | Value::Number(_)
                        | Value::Symbol(_)
                        | Value::BigInt(_)
                ) {
                    self.stage = Stage::Done;
                } else if matches!(self.source, Value::Null) {
                    return Err(Fault::TypeError("locales cannot be null".into()));
                } else if matches!(self.source, Value::String(_))
                    || locale::record(heap, task, &self.source)?.is_some()
                {
                    self.single = true;
                    return self.element(heap, task, self.source.clone());
                } else {
                    self.stage = Stage::Length;
                }
                Ok(Action::Continue)
            }
            Stage::Length => {
                self.stage = Stage::LengthValue;
                match self.read(
                    heap,
                    task,
                    &self.source,
                    property::Key::Name(JsString::from("length")),
                )? {
                    Err(action) => Ok(action),
                    Ok(value) => self.length_value(value),
                }
            }
            Stage::LengthValue => self.length_value(Self::value(input)?),
            Stage::Element => {
                if self.index >= self.length {
                    self.stage = Stage::Done;
                    return Ok(Action::Continue);
                }
                let key = index_key(self.index);
                if !property::has_property(heap, task, &self.source, &key)? {
                    self.index += 1;
                    return Ok(Action::Continue);
                }
                self.stage = Stage::ElementValue;
                match self.read(heap, task, &self.source, key)? {
                    Err(action) => Ok(action),
                    Ok(value) => self.element(heap, task, value),
                }
            }
            Stage::ElementValue => self.element(heap, task, Self::value(input)?),
            Stage::ElementString => self.add(heap, task, Self::value(input)?),
            Stage::Done => Ok(Action::Return(self.result.clone())),
        }
    }
    fn length_value(&mut self, value: Value) -> Result<Action, Fault> {
        if matches!(value, Value::Object(_)) {
            return Ok(Action::ToNumber(value));
        }
        self.length = to_length(&value)?;
        self.stage = Stage::Element;
        Ok(Action::Continue)
    }
}
