//! Saved locale-list, option-getter and argument-coercion stages.
use super::intl_service::{self, OptionKind};
use super::*;
use crate::{
    heap::HeapObject,
    intl::{
        relative_time_format::{self, RelativeTimeFormat},
        service,
    },
    object::OrdinaryObject,
};
const OPTIONS: &[&str] = &["localeMatcher", "numberingSystem", "style", "numeric"];
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
    Number,
    NumberValue,
    Unit,
    UnitValue,
    Format,
}
#[derive(Debug)]
pub(super) struct State {
    list: Option<super::intl::Intl>,
    locales: Value,
    options: Value,
    this: Value,
    value: Value,
    unit: Value,
    stage: Stage,
    supported: bool,
    to_parts: bool,
    option: usize,
    numbering: Option<String>,
    slots: Option<RelativeTimeFormat>,
}
impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        if let Some(list) = &self.list {
            list.trace(visit);
        }
        self.locales.trace(visit);
        self.options.trace(visit);
        self.this.trace(visit);
        self.value.trace(visit);
        self.unit.trace(visit);
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
        let format = matches!(
            which,
            builtin::Builtin::IntlRelativeTimeFormatFormat
                | builtin::Builtin::IntlRelativeTimeFormatToParts
        );
        if format {
            relative_time_format::record(heap, task, this)?;
        }
        let source = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Self {
            list: if format {
                None
            } else {
                Some(super::intl::Intl::new(
                    std::slice::from_ref(&source),
                    false,
                )?)
            },
            locales: Value::Undefined,
            options: args.get(1).cloned().unwrap_or(Value::Undefined),
            this: this.clone(),
            value: source,
            unit: args.get(1).cloned().unwrap_or(Value::Undefined),
            stage: if format { Stage::Number } else { Stage::List },
            supported: which == builtin::Builtin::IntlRelativeTimeFormatSupportedLocales,
            to_parts: which == builtin::Builtin::IntlRelativeTimeFormatToParts,
            option: 0,
            numbering: None,
            slots: None,
        })
    }
    fn result(input: Input) -> Result<Value, Fault> {
        if let Input::CallResult(value) = input {
            Ok(value)
        } else {
            Err(Fault::Internal("RelativeTimeFormat expected callback"))
        }
    }
    fn set_locale(&mut self, tag: String, data: &'static crate::intl::LocaleData) {
        let mut slots = RelativeTimeFormat::new(tag, data);
        if let Some(system) = self
            .numbering
            .as_deref()
            .and_then(crate::intl::number_format::numbering_system)
        {
            if system != slots.digits.numbering {
                let mut tag = crate::intl::tag::LanguageTag::parse(&slots.digits.locale).unwrap();
                tag.keywords.remove("nu");
                slots.digits.locale = tag.to_string();
            }
            slots.digits.numbering = system;
        }
        self.slots = Some(slots);
    }
    fn next_option(&mut self) -> Action {
        self.option += 1;
        self.stage = if self.option == 2 && !self.supported {
            Stage::Resolve
        } else {
            Stage::Read
        };
        Action::Continue
    }
    fn option_value(&mut self, value: Value) -> Result<Action, Fault> {
        if matches!(value, Value::Undefined) {
            return Ok(self.next_option());
        }
        self.stage = Stage::Converted;
        match intl_service::convert(value, OptionKind::String)? {
            Ok(value) => self.apply(value),
            Err(action) => Ok(action),
        }
    }
    fn apply(&mut self, value: Value) -> Result<Action, Fault> {
        match self.option {
            0 => {
                service::string_option(&value, &["lookup", "best fit"])?;
            }
            1 => {
                let text = dispatch::to_js_string(&value)?.to_string_lossy();
                if !crate::intl::tag::unicode_type(&text) {
                    return Err(Fault::RangeError("invalid numberingSystem option".into()));
                }
                self.numbering = Some(text);
            }
            2 => {
                self.slots.as_mut().unwrap().style =
                    service::string_option(&value, &["long", "short", "narrow"])?
            }
            3 => {
                self.slots.as_mut().unwrap().numeric =
                    service::string_option(&value, &["always", "auto"])?
            }
            _ => unreachable!(),
        }
        Ok(self.next_option())
    }
    fn number(&mut self, value: Value) -> Result<Action, Fault> {
        let number = dispatch::to_number(&value)?;
        if !number.is_finite() {
            return Err(Fault::RangeError("invalid relative-time value".into()));
        }
        self.value = Value::Number(number);
        self.stage = Stage::Unit;
        Ok(Action::Continue)
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
                self.stage = Stage::Read;
                Ok(Action::Continue)
            }
            Stage::Read => {
                let end = if self.supported { 1 } else { OPTIONS.len() };
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
                if let Some((tag, data)) =
                    service::resolve_locale(heap, task, &self.locales, &["nu"])?
                {
                    self.set_locale(tag, data);
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
                let (tag, data) = service::supported_locale(&default.to_string_lossy(), &["nu"])
                    .unwrap_or_else(|| ("en-US".into(), crate::intl::lookup("en-US").unwrap()));
                self.set_locale(tag, data);
                self.stage = Stage::Read;
                Ok(Action::Continue)
            }
            Stage::Build => {
                if self.supported {
                    return service::supported_locales(heap, task, &self.locales)
                        .map(Action::Return);
                }
                let proto = crate::realm::intrinsics(heap)
                    .ok_or_else(|| Fault::OutOfMemory("realm exhausted"))?
                    .intl_relative_time_format_prototype;
                let object = OrdinaryObject::with_prototype(heap, task, proto)
                    .with_relative_time_format(self.slots.take().unwrap());
                heap.alloc(HeapObject::Plain(object))
                    .map(Value::Object)
                    .map(Action::Return)
                    .ok_or_else(|| Fault::OutOfMemory("heap exhausted"))
            }
            Stage::Number => {
                self.stage = Stage::NumberValue;
                if matches!(self.value, Value::Object(_) | Value::BigInt(_)) {
                    Ok(Action::ToNumber(self.value.clone()))
                } else {
                    self.number(self.value.clone())
                }
            }
            Stage::NumberValue => self.number(Self::result(input)?),
            Stage::Unit => {
                self.stage = Stage::UnitValue;
                if matches!(self.unit, Value::Object(_) | Value::BigInt(_)) {
                    Ok(Action::ToString(self.unit.clone()))
                } else {
                    self.unit = Value::String(dispatch::to_js_string(&self.unit)?);
                    self.stage = Stage::Format;
                    Ok(Action::Continue)
                }
            }
            Stage::UnitValue => {
                self.unit = Self::result(input)?;
                self.stage = Stage::Format;
                Ok(Action::Continue)
            }
            Stage::Format => {
                let Value::Number(number) = self.value else {
                    unreachable!()
                };
                let Value::String(unit) = &self.unit else {
                    unreachable!()
                };
                relative_time_format::record(heap, task, &self.this)?
                    .format(heap, task, number, &unit.to_string_lossy(), self.to_parts)
                    .map(Action::Return)
            }
        }
    }
}
