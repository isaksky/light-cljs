//! NumberFormat constructor / supportedLocalesOf / numeric coercion stages.
use super::*;
use crate::heap::HeapObject;
use crate::intl::number_format::{self, NumberFormat};
use crate::object::OrdinaryObject;

const OPTIONS: &[&str] = &[
    "localeMatcher",
    "numberingSystem",
    "style",
    "notation",
    "minimumIntegerDigits",
    "minimumFractionDigits",
    "maximumFractionDigits",
    "minimumSignificantDigits",
    "maximumSignificantDigits",
    "roundingMode",
    "useGrouping",
];
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
        let format = which == builtin::Builtin::IntlNumberFormatBound;
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
            self.slots.as_mut().unwrap().grouping = if value.truthy() { "always" } else { "false" };
            self.option += 1;
            self.stage = Stage::Read;
            return Ok(Action::Continue);
        }
        self.stage = Stage::Converted;
        if matches!(value, Value::Object(_)) {
            return Ok(if (4..=8).contains(&self.option) {
                Action::ToNumber(value)
            } else {
                Action::ToString(value)
            });
        }
        self.apply(value)
    }
    fn apply(&mut self, value: Value) -> Result<Action, Fault> {
        let name = OPTIONS[self.option];
        let invalid = || Fault::RangeError(format!("unsupported or invalid NumberFormat {name}"));
        if (4..=8).contains(&self.option) {
            let n = dispatch::to_number(&value)?;
            let (min, max) = if self.option == 4 {
                (1.0, 21.0)
            } else if self.option <= 6 {
                (0.0, 100.0)
            } else {
                (1.0, 21.0)
            };
            if n.is_nan() || n < min || n > max {
                return Err(invalid());
            }
            let n = n.floor() as usize;
            match self.option {
                4 => self.slots.as_mut().unwrap().min_integer = n,
                5 => self.min_fraction = Some(n),
                6 => self.max_fraction = Some(n),
                7 => self.min_significant = Some(n),
                8 => self.max_significant = Some(n),
                _ => unreachable!(),
            }
        } else {
            let text = dispatch::to_js_string(&value)?.to_string_lossy();
            match name {
                "localeMatcher" if ["lookup", "best fit"].contains(&text.as_str()) => {}
                "numberingSystem" if crate::intl::tag::unicode_type(&text) => {} // only latn is supported; well-formed others fall back
                "style" if text == "decimal" => {}
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
                let len =
                    property::get_field(heap, task, &self.locales, &JsString::from("length"))?;
                let len = to_length(&len)?;
                let mut selected = None;
                for i in 0..len {
                    let Value::String(tag) =
                        property::get(heap, task, &self.locales, &index_key(i))?
                    else {
                        return Err(Fault::Internal("canonical locale list"));
                    };
                    let tag = tag.to_string_lossy();
                    if let Some(found) = supported_locale(&tag) {
                        selected = Some(found);
                        break;
                    }
                }
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
                let (tag, data) = supported_locale(&default.to_string_lossy())
                    .unwrap_or_else(|| ("en-US".into(), crate::intl::lookup("en-US").unwrap()));
                self.slots = Some(NumberFormat::new(tag, data));
                self.stage = Stage::Read;
                Ok(Action::Continue)
            }
            Stage::Read => {
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
                match property::read(
                    heap,
                    task,
                    &self.options,
                    &property::Key::Name(JsString::from(OPTIONS[self.option])),
                )? {
                    Some(property::Read::Getter(callee)) => Ok(Action::Call {
                        callee,
                        this: self.options.clone(),
                        args: vec![],
                    }),
                    Some(property::Read::Value(value)) => self.option_value(value),
                    None => self.option_value(Value::Undefined),
                }
            }
            Stage::Value => self.option_value(Self::result(input)?),
            Stage::Converted => self.apply(Self::result(input)?),
            Stage::Build => {
                if self.supported {
                    let result = property::new_array(heap, task)?;
                    let len = to_length(&property::get_field(
                        heap,
                        task,
                        &self.locales,
                        &JsString::from("length"),
                    )?)?;
                    for i in 0..len {
                        let value = property::get(heap, task, &self.locales, &index_key(i))?;
                        if let Value::String(tag) = &value
                            && supported_locale(&tag.to_string_lossy()).is_some()
                        {
                            property::append(heap, task, &result, Some(value))?;
                        }
                    }
                    return Ok(Action::Return(result));
                }
                let slots = self.slots.as_mut().unwrap();
                slots.min_fraction = self.min_fraction.unwrap_or(0);
                slots.max_fraction = self.max_fraction.unwrap_or(3.max(slots.min_fraction));
                if slots.min_fraction > slots.max_fraction {
                    return Err(Fault::RangeError("fraction digit bounds".into()));
                }
                if self.min_significant.is_some() || self.max_significant.is_some() {
                    let min = self.min_significant.unwrap_or(1);
                    let max = self.max_significant.unwrap_or(21);
                    if min > max {
                        return Err(Fault::RangeError("significant digit bounds".into()));
                    }
                    slots.significant = Some((min, max));
                }
                let proto = crate::realm::intrinsics(heap)
                    .ok_or(Fault::OutOfMemory("realm exhausted"))?
                    .intl_number_format_prototype;
                let object =
                    OrdinaryObject::with_prototype(task, proto).with_number_format(slots.clone());
                heap.alloc(HeapObject::Plain(object))
                    .map(Value::Object)
                    .map(Action::Return)
                    .ok_or(Fault::OutOfMemory("heap exhausted"))
            }
            Stage::Format => {
                if matches!(self.argument, Value::Object(_)) {
                    self.stage = Stage::Primitive;
                    Ok(Action::ToPrimitiveNumber(self.argument.clone()))
                } else {
                    number_format::record(heap, task, &self.this)?
                        .format(&self.argument)
                        .map(Action::Return)
                }
            }
            Stage::Primitive => {
                self.argument = Self::result(input)?;
                self.stage = Stage::Format;
                Ok(Action::Continue)
            }
        }
    }
}

/// Lookup is limited to the policy's test locales, including their children.
/// Unsupported Unicode numbering systems fall back to Latin and are omitted
/// from the resolved locale, matching the pinned QuickJS service policy.
fn supported_locale(tag: &str) -> Option<(String, &'static crate::intl::LocaleData)> {
    let parsed = crate::intl::tag::LanguageTag::parse(tag).ok()?;
    let mut base = parsed.base_name();
    loop {
        if crate::intl::LOCALES.contains(&base.as_str()) {
            let data = crate::intl::lookup(&base)?;
            if parsed.keywords.get("nu").is_some_and(|nu| nu == "latn") {
                base.push_str("-u-nu-latn");
            }
            return Some((base, data));
        }
        base = base.rsplit_once('-')?.0.to_owned();
    }
}
