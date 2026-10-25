//! NumberFormat constructor / supportedLocalesOf / numeric coercion stages.
use super::*;
use crate::heap::HeapObject;
use crate::intl::number_format::{self, NumberFormat};
use crate::intl::service;
use crate::object::OrdinaryObject;

const OPTIONS: &[&str] = &[
    "localeMatcher",
    "numberingSystem",
    "style",
    "currency",
    "currencyDisplay",
    "currencySign",
    "unit",
    "unitDisplay",
    "notation",
    "minimumIntegerDigits",
    "minimumFractionDigits",
    "maximumFractionDigits",
    "minimumSignificantDigits",
    "maximumSignificantDigits",
    "roundingMode",
    "useGrouping",
];
fn digit_option(name: &str) -> bool {
    matches!(
        name,
        "minimumIntegerDigits"
            | "minimumFractionDigits"
            | "maximumFractionDigits"
            | "minimumSignificantDigits"
            | "maximumSignificantDigits"
    )
}
#[derive(Debug)]
enum Stage {
    List,
    Resolve,
    DefaultLocale,
    Read,
    Value,
    Converted,
    Build,
    Format,
    Primitive,
    Parts,
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
    slots: Option<NumberFormat>,
    min_fraction: Option<usize>,
    max_fraction: Option<usize>,
    min_significant: Option<usize>,
    max_significant: Option<usize>,
    to_parts: bool,
    parts: std::collections::VecDeque<(&'static str, String)>,
    result: Value,
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
        self.result.trace(visit);
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
            builtin::Builtin::IntlNumberFormatBound | builtin::Builtin::IntlNumberFormatToParts
        );
        if format {
            number_format::record(heap, task, this)?;
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
            argument: source,
            stage: if format { Stage::Format } else { Stage::List },
            supported: which == builtin::Builtin::IntlNumberFormatSupportedLocales,
            option: 0,
            slots: None,
            min_fraction: None,
            max_fraction: None,
            min_significant: None,
            max_significant: None,
            to_parts: which == builtin::Builtin::IntlNumberFormatToParts,
            parts: Default::default(),
            result: Value::Undefined,
        })
    }
    fn result(input: Input) -> Result<Value, Fault> {
        if let Input::CallResult(v) = input {
            Ok(v)
        } else {
            Err(Fault::Internal("NumberFormat expected callback"))
        }
    }
    fn option_value(&mut self, value: Value) -> Result<Action, Fault> {
        if matches!(value, Value::Undefined) {
            self.option += 1;
            self.stage = Stage::Read;
            return Ok(Action::Continue);
        }
        let name = OPTIONS[self.option];
        if name == "useGrouping" && (matches!(value, Value::Bool(true)) || !value.truthy()) {
            let Ok(Value::Bool(grouping)) =
                super::intl_service::convert(value, super::intl_service::OptionKind::Boolean)?
            else {
                unreachable!()
            };
            self.slots.as_mut().unwrap().grouping = if grouping { "always" } else { "false" };
            self.option += 1;
            self.stage = Stage::Read;
            return Ok(Action::Continue);
        }
        self.stage = Stage::Converted;
        match super::intl_service::convert(
            value,
            if digit_option(name) {
                super::intl_service::OptionKind::Number
            } else {
                super::intl_service::OptionKind::String
            },
        )? {
            Ok(value) => self.apply(value),
            Err(action) => Ok(action),
        }
    }
    fn apply(&mut self, value: Value) -> Result<Action, Fault> {
        let name = OPTIONS[self.option];
        let invalid = || Fault::RangeError(format!("unsupported or invalid NumberFormat {name}"));
        if digit_option(name) {
            let (min, max) = if name == "minimumIntegerDigits" {
                (1.0, 21.0)
            } else if name.ends_with("FractionDigits") {
                (0.0, 100.0)
            } else {
                (1.0, 21.0)
            };
            let n = service::number_option(&value, min as usize, max as usize)?;
            match name {
                "minimumIntegerDigits" => self.slots.as_mut().unwrap().min_integer = n,
                "minimumFractionDigits" => self.min_fraction = Some(n),
                "maximumFractionDigits" => self.max_fraction = Some(n),
                "minimumSignificantDigits" => self.min_significant = Some(n),
                "maximumSignificantDigits" => self.max_significant = Some(n),
                _ => unreachable!(),
            }
        } else {
            let text = dispatch::to_js_string(&value)?.to_string_lossy();
            match name {
                "localeMatcher" if ["lookup", "best fit"].contains(&text.as_str()) => {}
                "numberingSystem" if crate::intl::tag::unicode_type(&text) => {
                    if let Some(system) = number_format::numbering_system(&text) {
                        let slots = self.slots.as_mut().unwrap();
                        if system != slots.numbering {
                            let mut tag = crate::intl::tag::LanguageTag::parse(&slots.locale)
                                .expect("resolved locale");
                            tag.keywords.remove("nu");
                            slots.locale = tag.to_string();
                        }
                        slots.numbering = system;
                    }
                }
                "style" => {
                    self.slots.as_mut().unwrap().style =
                        service::string_option(&value, &["decimal", "percent", "currency", "unit"])?
                }
                "currency" => {
                    if text.len() != 3 || !text.bytes().all(|c| c.is_ascii_alphabetic()) {
                        return Err(Fault::RangeError("invalid currency code".into()));
                    }
                    self.slots.as_mut().unwrap().currency = Some(text.to_ascii_uppercase());
                }
                "currencyDisplay" => {
                    self.slots.as_mut().unwrap().currency_display =
                        service::string_option(&value, &["symbol", "narrowSymbol", "code", "name"])?
                }
                "currencySign" => {
                    self.slots.as_mut().unwrap().currency_sign =
                        service::string_option(&value, &["standard", "accounting"])?
                }
                "unit" => {
                    if !number_format::valid_unit(&text) {
                        return Err(Fault::RangeError("invalid unit identifier".into()));
                    }
                    self.slots.as_mut().unwrap().unit = Some(text);
                }
                "unitDisplay" => {
                    self.slots.as_mut().unwrap().unit_display =
                        service::string_option(&value, &["short", "long", "narrow"])?;
                }
                "notation" if text == "standard" => {}
                "roundingMode" => {
                    self.slots.as_mut().unwrap().rounding = [
                        "ceil",
                        "floor",
                        "expand",
                        "trunc",
                        "halfCeil",
                        "halfFloor",
                        "halfExpand",
                        "halfTrunc",
                        "halfEven",
                    ]
                    .into_iter()
                    .find(|&v| v == text)
                    .ok_or_else(invalid)?;
                }
                "useGrouping" => {
                    self.slots.as_mut().unwrap().grouping = match text.as_str() {
                        "true" | "false" => "auto",
                        "always" => "always",
                        "auto" => "auto",
                        "min2" => "min2",
                        _ => return Err(invalid()),
                    };
                }
                _ => return Err(invalid()),
            }
        }
        self.option += 1;
        self.stage = Stage::Read;
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
                    self.stage = Stage::Resolve;
                    Ok(Action::Continue)
                }
                action => Ok(action),
            },
            Stage::Resolve => {
                if matches!(self.options, Value::Null) {
                    return Err(Fault::TypeError(
                        "NumberFormat options cannot be null".into(),
                    ));
                }
                let selected = service::resolve_locale(heap, task, &self.locales, &["nu"])?;
                if selected.is_none() && !self.supported {
                    self.stage = Stage::DefaultLocale;
                    return Ok(Action::DefaultLocale);
                }
                let (tag, data) = selected
                    .unwrap_or_else(|| ("en-US".into(), crate::intl::lookup("en-US").unwrap()));
                self.slots = Some(NumberFormat::new(tag, data));
                self.stage = Stage::Read;
                Ok(Action::Continue)
            }
            Stage::DefaultLocale => {
                let Value::String(default) = Self::result(input)? else {
                    return Err(Fault::Internal("default locale"));
                };
                let (tag, data) = service::supported_locale(&default.to_string_lossy(), &["nu"])
                    .unwrap_or_else(|| ("en-US".into(), crate::intl::lookup("en-US").unwrap()));
                self.slots = Some(NumberFormat::new(tag, data));
                self.stage = Stage::Read;
                Ok(Action::Continue)
            }
            Stage::Read => {
                // SetNumberFormatDigitOptions rejects inconsistent significant
                // bounds before observing roundingMode or useGrouping.
                if OPTIONS.get(self.option) == Some(&"unit") {
                    let slots = self.slots.as_ref().unwrap();
                    if slots.style == "currency" && slots.currency.is_none() {
                        return Err(Fault::TypeError("currency option is required".into()));
                    }
                }
                if OPTIONS.get(self.option) == Some(&"unitDisplay") {
                    let slots = self.slots.as_ref().unwrap();
                    if slots.style == "unit" && slots.unit.is_none() {
                        return Err(Fault::TypeError("unit option is required".into()));
                    }
                }
                if OPTIONS.get(self.option) == Some(&"roundingMode")
                    && (self.min_significant.is_some() || self.max_significant.is_some())
                {
                    let min = self.min_significant.unwrap_or(1);
                    let max = self.max_significant.unwrap_or(21);
                    if min > max {
                        return Err(Fault::RangeError("significant digit bounds".into()));
                    }
                    self.slots.as_mut().unwrap().significant = Some((min, max));
                }
                let end = if self.supported { 1 } else { OPTIONS.len() };
                if self.option == end {
                    self.stage = Stage::Build;
                    return Ok(Action::Continue);
                }
                // Primitive options are boxed by GetOptionsObject; their prototypes
                // have no Intl option properties in the immutable realm.
                if !matches!(self.options, Value::Object(_)) {
                    self.option += 1;
                    return Ok(Action::Continue);
                }
                self.stage = Stage::Value;
                match super::intl_service::read(heap, task, &self.options, OPTIONS[self.option])? {
                    Ok(value) => self.option_value(value),
                    Err(action) => Ok(action),
                }
            }
            Stage::Value => self.option_value(Self::result(input)?),
            Stage::Converted => self.apply(Self::result(input)?),
            Stage::Build => {
                if self.supported {
                    return service::supported_locales(heap, task, &self.locales)
                        .map(Action::Return);
                }
                let slots = self.slots.as_mut().unwrap();
                let (default_min, default_max) = match slots.style {
                    "currency" => {
                        let digits =
                            number_format::currency_digits(slots.currency.as_deref().unwrap());
                        (digits, digits)
                    }
                    "percent" => (0, 0),
                    _ => (0, 3),
                };
                slots.min_fraction = self
                    .min_fraction
                    .unwrap_or_else(|| default_min.min(self.max_fraction.unwrap_or(default_max)));
                slots.max_fraction = self
                    .max_fraction
                    .unwrap_or(default_max.max(slots.min_fraction));
                if slots.min_fraction > slots.max_fraction {
                    return Err(Fault::RangeError("fraction digit bounds".into()));
                }
                let proto = crate::realm::intrinsics(heap)
                    .ok_or(Fault::OutOfMemory("realm exhausted"))?
                    .intl_number_format_prototype;
                let object = OrdinaryObject::with_prototype(heap, task, proto)
                    .with_number_format(slots.clone());
                heap.alloc(HeapObject::Plain(object))
                    .map(Value::Object)
                    .map(Action::Return)
                    .ok_or(Fault::OutOfMemory("heap exhausted"))
            }
            Stage::Format => {
                if matches!(self.argument, Value::Object(_)) {
                    self.stage = Stage::Primitive;
                    Ok(Action::ToPrimitiveNumber(self.argument.clone()))
                } else if matches!(self.argument, Value::BigInt(_)) {
                    self.stage = Stage::Primitive;
                    Ok(Action::ToString(self.argument.clone()))
                } else {
                    let slots = number_format::record(heap, task, &self.this)?;
                    if self.to_parts {
                        self.parts = slots.parts(&self.argument)?.into();
                        self.result = property::new_array(heap, task)?;
                        self.stage = Stage::Parts;
                        Ok(Action::Continue)
                    } else {
                        slots.format(&self.argument).map(Action::Return)
                    }
                }
            }
            Stage::Primitive => {
                self.argument = Self::result(input)?;
                self.stage = Stage::Format;
                Ok(Action::Continue)
            }
            Stage::Parts => {
                let Some((kind, text)) = self.parts.pop_front() else {
                    return Ok(Action::Return(self.result.clone()));
                };
                let item = service::resolved_options(
                    heap,
                    task,
                    [
                        ("type", Value::String(JsString::from(kind))),
                        ("value", Value::String(JsString::from(text.as_str()))),
                    ],
                )?;
                property::append(heap, task, &self.result, Some(item))?;
                Ok(Action::Continue)
            }
        }
    }
}
