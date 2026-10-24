//! Saved locale-list, option-getter and code-coercion stages.
use super::intl_service::{self, OptionKind};
use super::*;
use crate::value::string::{CursorStep, UnitCursor};
use crate::{
    heap::HeapObject,
    intl::{
        display_names::{self, DisplayNames},
        service,
    },
    object::OrdinaryObject,
};
const OPTIONS: &[&str] = &[
    "localeMatcher",
    "style",
    "type",
    "fallback",
    "languageDisplay",
];
#[derive(Debug)]
enum Stage {
    List,
    Options,
    Read,
    Value,
    Converted,
    Resolve,
    Default,
    Build,
    Code,
    CodeValue,
    Of,
    Calendar,
}
#[derive(Debug)]
pub(super) struct State {
    list: Option<intl::Intl>,
    locales: Value,
    options: Value,
    this: Value,
    code: Value,
    stage: Stage,
    supported: bool,
    option: usize,
    slots: Option<DisplayNames>,
    calendar: Option<UnitCursor>,
    normalized: JsString,
    subtag_len: usize,
}
impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        if let Some(list) = &self.list {
            list.trace(visit);
        }
        for value in [&self.locales, &self.options, &self.this, &self.code] {
            value.trace(visit);
        }
        // Immutable slots contain Rust strings/static references, no heap edges.
    }
}
impl State {
    pub(super) fn new(
        which: builtin::Builtin,
        this: &Value,
        args: &[Value],
        heap: &Heap,
        task: TaskId,
    ) -> Result<Self, Fault> {
        let of = which == builtin::Builtin::IntlDisplayNamesOf;
        if of {
            display_names::record(heap, task, this)?;
        }
        let code = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Self {
            list: if of {
                None
            } else {
                Some(intl::Intl::new(std::slice::from_ref(&code), false)?)
            },
            locales: Value::Undefined,
            options: args.get(1).cloned().unwrap_or(Value::Undefined),
            this: this.clone(),
            code,
            stage: if of { Stage::Code } else { Stage::List },
            supported: which == builtin::Builtin::IntlDisplayNamesSupportedLocales,
            option: 0,
            slots: None,
            calendar: None,
            normalized: JsString::from(""),
            subtag_len: 0,
        })
    }
    fn result(input: Input) -> Result<Value, Fault> {
        if let Input::CallResult(value) = input {
            Ok(value)
        } else {
            Err(Fault::Internal("DisplayNames expected callback"))
        }
    }
    fn next_option(&mut self) -> Action {
        self.option += 1;
        self.stage = if self.option == 1 && !self.supported {
            Stage::Resolve
        } else {
            Stage::Read
        };
        Action::Continue
    }
    fn option_value(&mut self, value: Value) -> Result<Action, Fault> {
        if matches!(value, Value::Undefined) {
            if self.option == 2 {
                return Err(Fault::TypeError(
                    "DisplayNames type option is required".into(),
                ));
            }
            return Ok(self.next_option());
        }
        self.stage = Stage::Converted;
        match intl_service::convert(value, OptionKind::String)? {
            Ok(value) => self.apply(value),
            Err(action) => Ok(action),
        }
    }
    fn apply(&mut self, value: Value) -> Result<Action, Fault> {
        if matches!(&value, Value::String(s) if s.len() > 13) {
            return Err(Fault::RangeError("invalid Intl option".into()));
        }
        match self.option {
            0 => {
                service::string_option(&value, &["lookup", "best fit"])?;
            }
            1 => {
                self.slots.as_mut().unwrap().style =
                    service::string_option(&value, &["long", "short", "narrow"])?
            }
            2 => {
                self.slots.as_mut().unwrap().kind =
                    service::string_option(&value, display_names::TYPES)?
            }
            3 => {
                self.slots.as_mut().unwrap().fallback =
                    service::string_option(&value, &["code", "none"])?
            }
            4 => {
                self.slots.as_mut().unwrap().language_display =
                    service::string_option(&value, &["dialect", "standard"])?
            }
            _ => unreachable!(),
        }
        Ok(self.next_option())
    }
    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        match self.stage {
            Stage::List => match self.list.as_mut().unwrap().step(heap, task, input)? {
                Action::Return(value) => {
                    self.locales = value;
                    self.list = None;
                    self.stage = Stage::Options;
                    Ok(Action::Continue)
                }
                action => Ok(action),
            },
            Stage::Options => {
                service::get_options_object(&self.options)?;
                if !self.supported && matches!(self.options, Value::Undefined) {
                    return Err(Fault::TypeError("DisplayNames options are required".into()));
                }
                self.stage = Stage::Read;
                Ok(Action::Continue)
            }
            Stage::Read => {
                let end = if self.supported {
                    1
                } else if self.slots.as_ref().is_some_and(|s| s.kind == "language") {
                    OPTIONS.len()
                } else {
                    4
                };
                if self.option == end {
                    self.stage = Stage::Build;
                    return Ok(Action::Continue);
                }
                self.stage = Stage::Value;
                match intl_service::read(heap, task, &self.options, OPTIONS[self.option])? {
                    Ok(value) => self.option_value(value),
                    Err(action) => Ok(action),
                }
            }
            Stage::Value => self.option_value(Self::result(input)?),
            Stage::Converted => self.apply(Self::result(input)?),
            Stage::Resolve => {
                if let Some((tag, _)) = service::resolve_locale(heap, task, &self.locales, &[])? {
                    self.slots = Some(DisplayNames::new(tag));
                    self.stage = Stage::Read;
                    Ok(Action::Continue)
                } else {
                    self.stage = Stage::Default;
                    Ok(Action::DefaultLocale)
                }
            }
            Stage::Default => {
                let Value::String(default) = Self::result(input)? else {
                    return Err(Fault::Internal("default locale"));
                };
                let tag = service::supported_locale(&default.to_string_lossy(), &[])
                    .map(|(tag, _)| tag)
                    .unwrap_or_else(|| "en-US".into());
                self.slots = Some(DisplayNames::new(tag));
                self.stage = Stage::Read;
                Ok(Action::Continue)
            }
            Stage::Build => {
                if self.supported {
                    return service::supported_locales(heap, task, &self.locales)
                        .map(Action::Return);
                }
                let proto = crate::realm::intrinsics(heap)
                    .ok_or(Fault::OutOfMemory("realm exhausted"))?
                    .intl_display_names_prototype;
                let object = OrdinaryObject::with_prototype(task, proto)
                    .with_display_names(self.slots.take().unwrap());
                heap.alloc(HeapObject::Plain(object))
                    .map(Value::Object)
                    .map(Action::Return)
                    .ok_or(Fault::OutOfMemory("heap exhausted"))
            }
            Stage::Code => {
                self.stage = Stage::CodeValue;
                if matches!(self.code, Value::Object(_) | Value::BigInt(_)) {
                    Ok(Action::ToString(self.code.clone()))
                } else {
                    self.code = Value::String(dispatch::to_js_string(&self.code)?);
                    self.stage = Stage::Of;
                    Ok(Action::Continue)
                }
            }
            Stage::CodeValue => {
                self.code = Self::result(input)?;
                self.stage = Stage::Of;
                Ok(Action::Continue)
            }
            Stage::Of => {
                let Value::String(code) = &self.code else {
                    unreachable!()
                };
                let slots = display_names::record(heap, task, &self.this)?;
                if slots.kind == "language" {
                    return slots.of("").map(Action::Return);
                }
                if code.len() > 32 {
                    if slots.kind != "calendar" {
                        return Err(Fault::RangeError(format!("invalid {} code", slots.kind)));
                    }
                    // Calendar's type production permits arbitrarily many
                    // subtags. Validate and lowercase without flattening a rope
                    // or copying an input-sized buffer in one native step.
                    self.calendar = Some(UnitCursor::new(code.clone()));
                    self.stage = Stage::Calendar;
                    return Ok(Action::Continue);
                }
                slots.of(&code.to_string_lossy()).map(Action::Return)
            }
            Stage::Calendar => {
                let mut chunk = String::with_capacity(128);
                let mut done = false;
                for _ in 0..128 {
                    match self.calendar.as_mut().unwrap().step() {
                        CursorStep::Continue => (),
                        CursorStep::End => {
                            if !(3..=8).contains(&self.subtag_len) {
                                return Err(Fault::RangeError("invalid calendar code".into()));
                            }
                            done = true;
                            break;
                        }
                        CursorStep::Unit(c) => {
                            if c == u16::from(b'-') && (3..=8).contains(&self.subtag_len) {
                                self.subtag_len = 0;
                            } else if c <= 127
                                && (c as u8).is_ascii_alphanumeric()
                                && self.subtag_len < 8
                            {
                                self.subtag_len += 1;
                            } else {
                                return Err(Fault::RangeError("invalid calendar code".into()));
                            }
                            chunk.push((c as u8).to_ascii_lowercase() as char);
                        }
                    }
                }
                self.normalized = self
                    .normalized
                    .concat(&JsString::from(chunk.as_str()))
                    .ok_or_else(|| Fault::RangeError("calendar code too long".into()))?;
                if done {
                    // All snapshot calendar keys and aliases fit in 32 units:
                    // a longer validated type is an unknown canonical code.
                    return Ok(Action::Return(
                        if display_names::record(heap, task, &self.this)?.fallback == "none" {
                            Value::Undefined
                        } else {
                            Value::String(self.normalized.clone())
                        },
                    ));
                }
                Ok(Action::Continue)
            }
        }
    }
}
