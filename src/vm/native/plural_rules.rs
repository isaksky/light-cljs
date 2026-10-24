//! PluralRules service constructor and select, with saved getter/coercion stages.
use super::intl_service::{self, OptionKind};
use super::*;
use crate::{
    heap::HeapObject,
    intl::{
        number_format::NumberFormat,
        plural_rules::{self, PluralRules},
        service,
    },
    object::OrdinaryObject,
};
const OPTIONS: &[&str] = &[
    "localeMatcher",
    "type",
    "minimumIntegerDigits",
    "minimumFractionDigits",
    "maximumFractionDigits",
    "minimumSignificantDigits",
    "maximumSignificantDigits",
    "roundingMode",
];
#[derive(Debug)]
enum Stage {
    List,
    Options,
    Resolve,
    Default,
    Read,
    Value,
    Converted,
    Build,
    Select,
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
    slots: Option<PluralRules>,
    min_fraction: Option<usize>,
    max_fraction: Option<usize>,
    min_significant: Option<usize>,
    max_significant: Option<usize>,
}
impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
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
        let select = which == builtin::Builtin::IntlPluralRulesSelect;
        if select {
            plural_rules::record(heap, task, this)?;
        }
        let source = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Self {
            list: if select {
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
            stage: if select { Stage::Select } else { Stage::List },
            supported: which == builtin::Builtin::IntlPluralRulesSupportedLocales,
            option: 0,
            slots: None,
            min_fraction: None,
            max_fraction: None,
            min_significant: None,
            max_significant: None,
        })
    }
    fn result(input: Input) -> Result<Value, Fault> {
        if let Input::CallResult(value) = input {
            Ok(value)
        } else {
            Err(Fault::Internal("PluralRules expected callback"))
        }
    }
    fn set_locale(&mut self, tag: String, data: &'static crate::intl::LocaleData) {
        self.slots = Some(PluralRules {
            digits: NumberFormat::new(tag, data),
            ordinal: false,
        });
    }
    fn option_value(&mut self, value: Value) -> Result<Action, Fault> {
        if matches!(value, Value::Undefined) {
            self.option += 1;
            self.stage = if self.option == 1 {
                Stage::Resolve
            } else {
                Stage::Read
            };
            return Ok(Action::Continue);
        }
        self.stage = Stage::Converted;
        match intl_service::convert(
            value,
            if (2..=6).contains(&self.option) {
                OptionKind::Number
            } else {
                OptionKind::String
            },
        )? {
            Ok(value) => self.apply(value),
            Err(action) => Ok(action),
        }
    }
    fn apply(&mut self, value: Value) -> Result<Action, Fault> {
        // ResolveLocale observes the host only after GetOption(localeMatcher),
        // including its getter and ToString, has completed successfully.
        if self.option == 0 {
            service::string_option(&value, &["lookup", "best fit"])?;
            self.option = 1;
            self.stage = Stage::Resolve;
            return Ok(Action::Continue);
        }
        let slots = self.slots.as_mut().unwrap();
        match self.option {
            1 => {
                slots.ordinal =
                    service::string_option(&value, &["cardinal", "ordinal"])? == "ordinal"
            }
            2 => slots.digits.min_integer = service::number_option(&value, 1, 21)?,
            3 => self.min_fraction = Some(service::number_option(&value, 0, 100)?),
            4 => self.max_fraction = Some(service::number_option(&value, 0, 100)?),
            5 => self.min_significant = Some(service::number_option(&value, 1, 21)?),
            6 => self.max_significant = Some(service::number_option(&value, 1, 21)?),
            7 => {
                slots.digits.rounding = service::string_option(
                    &value,
                    &[
                        "ceil",
                        "floor",
                        "expand",
                        "trunc",
                        "halfCeil",
                        "halfFloor",
                        "halfExpand",
                        "halfTrunc",
                        "halfEven",
                    ],
                )?
            }
            _ => unreachable!(),
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
            Stage::Resolve => {
                let selected = service::resolve_locale(heap, task, &self.locales, &[])?;
                if selected.is_none() && !self.supported {
                    self.stage = Stage::Default;
                    return Ok(Action::DefaultLocale);
                }
                let (tag, data) = selected
                    .unwrap_or_else(|| ("en-US".into(), crate::intl::lookup("en-US").unwrap()));
                self.set_locale(tag, data);
                self.stage = Stage::Read;
                Ok(Action::Continue)
            }
            Stage::Default => {
                let Value::String(default) = Self::result(input)? else {
                    return Err(Fault::Internal("default locale"));
                };
                let (tag, data) = service::supported_locale(&default.to_string_lossy(), &[])
                    .unwrap_or_else(|| ("en-US".into(), crate::intl::lookup("en-US").unwrap()));
                self.set_locale(tag, data);
                self.stage = Stage::Read;
                Ok(Action::Continue)
            }
            Stage::Read => {
                if self.option == 7 {
                    let slots = self.slots.as_mut().unwrap();
                    slots.digits.min_fraction = self.min_fraction.unwrap_or(0);
                    slots.digits.max_fraction = self
                        .max_fraction
                        .unwrap_or(3.max(slots.digits.min_fraction));
                    if slots.digits.min_fraction > slots.digits.max_fraction {
                        return Err(Fault::RangeError("fraction digit bounds".into()));
                    }
                    if self.min_significant.is_some() || self.max_significant.is_some() {
                        let min = self.min_significant.unwrap_or(1);
                        let max = self.max_significant.unwrap_or(21);
                        if min > max {
                            return Err(Fault::RangeError("significant digit bounds".into()));
                        }
                        slots.digits.significant = Some((min, max));
                    }
                }
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
            Stage::Build => {
                if self.supported {
                    return service::supported_locales(heap, task, &self.locales)
                        .map(Action::Return);
                }
                let proto = crate::realm::intrinsics(heap)
                    .ok_or(Fault::OutOfMemory("realm exhausted"))?
                    .intl_plural_rules_prototype;
                let object = OrdinaryObject::with_prototype(task, proto)
                    .with_plural_rules(self.slots.take().unwrap());
                heap.alloc(HeapObject::Plain(object))
                    .map(Value::Object)
                    .map(Action::Return)
                    .ok_or(Fault::OutOfMemory("heap exhausted"))
            }
            Stage::Select => {
                self.stage = Stage::Number;
                if matches!(self.argument, Value::Object(_) | Value::BigInt(_)) {
                    Ok(Action::ToNumber(self.argument.clone()))
                } else {
                    self.select(heap, task, self.argument.clone())
                }
            }
            Stage::Number => self.select(heap, task, Self::result(input)?),
        }
    }
    fn select(&self, heap: &Heap, task: TaskId, value: Value) -> Result<Action, Fault> {
        let result =
            plural_rules::record(heap, task, &self.this)?.select(dispatch::to_number(&value)?)?;
        Ok(Action::Return(Value::String(JsString::from(result))))
    }
}
