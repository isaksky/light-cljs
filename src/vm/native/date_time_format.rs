//! DateTimeFormat option reads and conversions are saved across VM callbacks.
use super::intl_service::{self, OptionKind};
use super::*;
use crate::{
    heap::HeapObject,
    intl::{
        date_time_format::{self, DateTimeFormat},
        number_format, service,
        tag::LanguageTag,
    },
    object::OrdinaryObject,
};
const OPTIONS: &[&str] = &[
    "localeMatcher",
    "calendar",
    "numberingSystem",
    "hour12",
    "hourCycle",
    "timeZone",
    "weekday",
    "era",
    "year",
    "month",
    "day",
    "dayPeriod",
    "hour",
    "minute",
    "second",
    "fractionalSecondDigits",
    "timeZoneName",
    "formatMatcher",
    "dateStyle",
    "timeStyle",
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
    Zone,
    Build,
    Format,
    Number,
}
#[derive(Debug)]
pub(super) struct State {
    list: Option<super::intl::Intl>,
    locales: Value,
    options: Value,
    this: Value,
    argument: Value,
    stage: Stage,
    supported: bool,
    option: usize,
    slots: Option<DateTimeFormat>,
    calendar: Option<String>,
    numbering: Option<&'static str>,
    hour_cycle: Option<&'static str>,
    hour12: Option<bool>,
    time_zone: Option<String>,
}
impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        if let Some(list) = &self.list {
            list.trace(visit);
        }
        self.locales.trace(visit);
        self.options.trace(visit);
        self.this.trace(visit);
        self.argument.trace(visit);
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
        let format = which == builtin::Builtin::IntlDateTimeFormatBound;
        if format {
            date_time_format::record(heap, task, this)?;
        }
        let argument = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Self {
            list: if format {
                None
            } else {
                Some(super::intl::Intl::new(
                    std::slice::from_ref(&argument),
                    false,
                )?)
            },
            locales: Value::Undefined,
            options: args.get(1).cloned().unwrap_or(Value::Undefined),
            this: this.clone(),
            argument,
            stage: if format { Stage::Format } else { Stage::List },
            supported: which == builtin::Builtin::IntlDateTimeFormatSupportedLocales,
            option: 0,
            slots: None,
            calendar: None,
            numbering: None,
            hour_cycle: None,
            hour12: None,
            time_zone: None,
        })
    }
    fn result(input: Input) -> Result<Value, Fault> {
        if let Input::CallResult(v) = input {
            Ok(v)
        } else {
            Err(Fault::Internal("DateTimeFormat callback"))
        }
    }
    fn advance(&mut self) -> Action {
        self.option += 1;
        self.stage = if self.option == 5 {
            Stage::Resolve
        } else if self.option == 6 {
            Stage::Zone
        } else {
            Stage::Read
        };
        Action::Continue
    }
    fn option_value(&mut self, value: Value) -> Result<Action, Fault> {
        if matches!(value, Value::Undefined) {
            return Ok(self.advance());
        }
        self.stage = Stage::Converted;
        let kind = match OPTIONS[self.option] {
            "hour12" => OptionKind::Boolean,
            "fractionalSecondDigits" => OptionKind::Number,
            _ => OptionKind::String,
        };
        match intl_service::convert(value, kind)? {
            Ok(v) => self.apply(v),
            Err(a) => Ok(a),
        }
    }
    fn apply(&mut self, value: Value) -> Result<Action, Fault> {
        let name = OPTIONS[self.option];
        let numeric = &["2-digit", "numeric"][..];
        let names = &["narrow", "short", "long"][..];
        match name {
            "localeMatcher" => {
                service::string_option(&value, &["lookup", "best fit"])?;
            }
            "calendar" | "numberingSystem" => {
                let text = dispatch::to_js_string(&value)?.to_string_lossy();
                if !crate::intl::tag::unicode_type(&text) {
                    return Err(Fault::RangeError(format!("invalid {name}")));
                }
                if name == "calendar" {
                    self.calendar = Some(text.to_ascii_lowercase());
                } else {
                    self.numbering = number_format::numbering_system(&text);
                }
            }
            "hour12" => self.hour12 = Some(value.truthy()),
            "hourCycle" => {
                self.hour_cycle = Some(service::string_option(
                    &value,
                    &["h11", "h12", "h23", "h24"],
                )?)
            }
            "timeZone" => self.time_zone = Some(dispatch::to_js_string(&value)?.to_string_lossy()),
            "weekday" => {
                self.slots.as_mut().unwrap().weekday = Some(service::string_option(&value, names)?)
            }
            "era" => {
                self.slots.as_mut().unwrap().era = Some(service::string_option(&value, names)?)
            }
            "year" => {
                self.slots.as_mut().unwrap().year = Some(service::string_option(&value, numeric)?)
            }
            "month" => {
                self.slots.as_mut().unwrap().month = Some(service::string_option(
                    &value,
                    &["2-digit", "numeric", "narrow", "short", "long"],
                )?)
            }
            "day" => {
                self.slots.as_mut().unwrap().day = Some(service::string_option(&value, numeric)?)
            }
            "formatMatcher" => {
                service::string_option(&value, &["basic", "best fit"])?;
            }
            _ => {
                match name {
                    "dayPeriod" => {
                        service::string_option(&value, names)?;
                    }
                    "hour" | "minute" | "second" => {
                        service::string_option(&value, numeric)?;
                    }
                    "fractionalSecondDigits" => {
                        service::number_option(&value, 1, 3)?;
                    }
                    "timeZoneName" => {
                        service::string_option(
                            &value,
                            &[
                                "short",
                                "long",
                                "shortOffset",
                                "longOffset",
                                "shortGeneric",
                                "longGeneric",
                            ],
                        )?;
                    }
                    "dateStyle" | "timeStyle" => {
                        service::string_option(&value, &["full", "long", "medium", "short"])?;
                    }
                    _ => unreachable!(),
                }
                return Err(Fault::RangeError(format!(
                    "DateTimeFormat {name} formatting is not supported yet"
                )));
            }
        }
        Ok(self.advance())
    }
    fn set_locale(
        &mut self,
        tag: String,
        data: &'static crate::intl::LocaleData,
    ) -> Result<Action, Fault> {
        let mut tag = LanguageTag::parse(&tag).expect("resolved locale");
        let calendar = self
            .calendar
            .as_deref()
            .or_else(|| tag.keywords.get("ca").map(String::as_str))
            .unwrap_or("gregory");
        if calendar != "gregory" {
            return Err(Fault::RangeError(
                "DateTimeFormat supports only the gregory calendar".into(),
            ));
        }
        if self.calendar.is_some() && tag.keywords.get("ca").is_some_and(|ca| ca != calendar) {
            tag.keywords.remove("ca");
        }
        if let Some(hc) = tag.keywords.get("hc")
            && (!["h11", "h12", "h23", "h24"].contains(&hc.as_str())
                || self.hour12.is_some()
                || self.hour_cycle.is_some_and(|v| v != hc))
        {
            tag.keywords.remove("hc");
        }
        if let Some(nu) = self.numbering
            && tag.keywords.get("nu").is_some_and(|v| v != nu)
        {
            tag.keywords.remove("nu");
        }
        let mut slots = DateTimeFormat::new(tag.to_string(), data);
        if let Some(nu) = self.numbering {
            slots.numbering = nu;
        }
        self.slots = Some(slots);
        self.stage = Stage::Read;
        Ok(Action::Continue)
    }
    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
        host: &mut dyn crate::Host,
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
                if matches!(self.options, Value::Null) {
                    return Err(Fault::TypeError(
                        "DateTimeFormat options cannot be null".into(),
                    ));
                }
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
                if !matches!(self.options, Value::Object(_)) {
                    return self.option_value(Value::Undefined);
                }
                match intl_service::read(heap, task, &self.options, OPTIONS[self.option])? {
                    Ok(v) => self.option_value(v),
                    Err(a) => Ok(a),
                }
            }
            Stage::Value => self.option_value(Self::result(input)?),
            Stage::Converted => self.apply(Self::result(input)?),
            Stage::Resolve => {
                match service::resolve_locale(heap, task, &self.locales, &["ca", "hc", "nu"])? {
                    Some((tag, data)) => self.set_locale(tag, data),
                    None => {
                        self.stage = Stage::Default;
                        Ok(Action::DefaultLocale)
                    }
                }
            }
            Stage::Default => {
                let Value::String(tag) = Self::result(input)? else {
                    return Err(Fault::Internal("default locale"));
                };
                let (tag, data) =
                    service::supported_locale(&tag.to_string_lossy(), &["ca", "hc", "nu"])
                        .unwrap_or_else(|| ("en-US".into(), crate::intl::lookup("en-US").unwrap()));
                self.set_locale(tag, data)
            }
            Stage::Zone => {
                let zone = super::super::date::zone(heap, host);
                self.slots.as_mut().unwrap().zone = match &self.time_zone {
                    Some(text) => date_time_format::explicit_zone(text, zone)?,
                    None => zone,
                };
                let slots = self.slots.as_mut().unwrap();
                slots.time_zone = if slots.zone == crate::host::TimeZone::UTC
                    && self
                        .time_zone
                        .as_ref()
                        .is_some_and(|s| s.starts_with(['+', '-']))
                {
                    "+00:00".into()
                } else {
                    slots.zone.identifier()
                };
                self.stage = Stage::Read;
                Ok(Action::Continue)
            }
            Stage::Build => {
                if self.supported {
                    return service::supported_locales(heap, task, &self.locales)
                        .map(Action::Return);
                }
                let mut slots = self.slots.take().unwrap();
                slots.finish();
                let proto = crate::realm::intrinsics(heap)
                    .ok_or(Fault::OutOfMemory("realm exhausted"))?
                    .intl_date_time_format_prototype;
                heap.alloc(HeapObject::Plain(
                    OrdinaryObject::with_prototype(task, proto).with_date_time_format(slots),
                ))
                .map(Value::Object)
                .map(Action::Return)
                .ok_or(Fault::OutOfMemory("heap exhausted"))
            }
            Stage::Format => {
                if matches!(self.argument, Value::Object(_)) {
                    self.stage = Stage::Number;
                    return Ok(Action::ToNumber(self.argument.clone()));
                }
                let time = if matches!(self.argument, Value::Undefined) {
                    super::super::date::now(host)?
                } else {
                    dispatch::to_number(&self.argument)?
                };
                date_time_format::record(heap, task, &self.this)?
                    .format(time)
                    .map(Action::Return)
            }
            Stage::Number => {
                let value = Self::result(input)?;
                date_time_format::record(heap, task, &self.this)?
                    .format(dispatch::to_number(&value)?)
                    .map(Action::Return)
            }
        }
    }
}
