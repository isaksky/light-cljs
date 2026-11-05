//! Saved DurationFormat locale, option and ToDurationRecord stages. JavaScript
//! runs only through the trampoline; numeric strings use a budgeted scanner.
use super::intl_service;
use super::*;
use crate::{
    heap::HeapObject,
    intl::{
        duration_format::{self, DISPLAYS, DurationFormat, READ_ORDER, UNITS},
        service,
    },
    object::OrdinaryObject,
    value::string::StringToNumber,
};

// Keep this constructor's saved record/scanner storage out of every unrelated
// builtin's entry frame, including eval on 64 KiB migrating workers.
#[inline(never)]
pub(super) fn start(
    heap: &Heap,
    state: &mut VmState,
    which: builtin::Builtin,
    this: &Value,
    args: &[Value],
    popped: usize,
) -> Result<(), Fault> {
    let native = State::new(which, this, args, heap, state.diag.task)?;
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, NativeState::DurationFormat(Box::new(native)))
}

#[derive(Debug)]
enum Stage {
    List,
    Options,
    Read,
    OptionValue,
    Converted,
    Resolve,
    Default,
    Build,
    Start,
    Field,
    FieldValue,
    Primitive,
    Scan,
    Format,
}
#[derive(Debug)]
pub(super) struct State {
    list: Option<Box<intl::Intl>>,
    locales: Value,
    options: Value,
    source: Value,
    stage: Stage,
    supported: bool,
    parts: bool,
    option: usize,
    numbering: Option<String>,
    slots: Option<Box<DurationFormat>>,
    field: usize,
    any: bool,
    values: [f64; 10],
    scanner: Option<Box<StringToNumber>>,
    scanning_option: bool,
}
impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        if let Some(list) = &self.list {
            list.trace(visit);
        }
        self.locales.trace(visit);
        self.options.trace(visit);
        self.source.trace(visit);
        // Slots and numeric scanner contain only immutable Rust storage.
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
        use builtin::Builtin::*;
        let formatting = matches!(which, IntlDurationFormatFormat | IntlDurationFormatToParts);
        let source = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Self {
            list: if formatting {
                None
            } else {
                Some(Box::new(intl::Intl::new(
                    std::slice::from_ref(&source),
                    false,
                )?))
            },
            locales: Value::Undefined,
            options: args.get(1).cloned().unwrap_or(Value::Undefined),
            source,
            stage: if formatting {
                Stage::Start
            } else {
                Stage::List
            },
            supported: which == IntlDurationFormatSupportedLocales,
            parts: which == IntlDurationFormatToParts,
            option: 0,
            numbering: None,
            slots: if formatting {
                Some(Box::new(duration_format::record(heap, task, this)?))
            } else {
                None
            },
            field: 0,
            any: false,
            values: [0.0; 10],
            scanner: None,
            scanning_option: false,
        })
    }
    fn result(input: Input) -> Result<Value, Fault> {
        if let Input::CallResult(value) = input {
            Ok(value)
        } else {
            Err(Fault::Internal("DurationFormat expected callback"))
        }
    }
    fn set_locale(&mut self, tag: String, data: &'static crate::intl::LocaleData) {
        let mut slots = DurationFormat::new(tag, data);
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
        self.slots = Some(Box::new(slots));
    }
    fn option_name(&self) -> &'static str {
        match self.option {
            0 => "localeMatcher",
            1 => "numberingSystem",
            2 => "style",
            23 => "fractionalDigits",
            n => {
                let i = (n - 3) / 2;
                if (n - 3) % 2 == 0 {
                    UNITS[i]
                } else {
                    DISPLAYS[i]
                }
            }
        }
    }
    fn advance_option(&mut self) -> Action {
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
            return self.apply(value);
        }
        self.stage = Stage::Converted;
        if self.option == 23 {
            self.scanning_option = true;
            self.stage = Stage::Primitive;
            Ok(Action::ToPrimitiveNumber(value))
        } else {
            Ok(Action::ToString(value))
        }
    }
    fn apply(&mut self, value: Value) -> Result<Action, Fault> {
        let absent = matches!(value, Value::Undefined);
        // Every accepted string is short. Length checks precede rope flattening.
        if matches!(&value,Value::String(s) if s.len()>11) {
            return Err(Fault::RangeError("invalid duration option".into()));
        }
        match self.option {
            0 => {
                if !absent {
                    service::string_option(&value, &["lookup", "best fit"])?;
                }
            }
            1 => {
                if !absent {
                    let text = dispatch::to_js_string(&value)?.to_string_lossy();
                    if !(3..=8).contains(&text.len())
                        || !text.bytes().all(|c| c.is_ascii_alphanumeric())
                    {
                        return Err(Fault::RangeError("invalid numberingSystem".into()));
                    }
                    self.numbering = Some(text);
                }
            }
            2 => {
                if !absent {
                    self.slots.as_mut().unwrap().style =
                        service::string_option(&value, &["long", "short", "narrow", "digital"])?;
                }
            }
            23 => {
                if !absent {
                    self.slots.as_mut().unwrap().fractional =
                        Some(service::number_option(&value, 0, 9)?);
                }
            }
            n => {
                let i = (n - 3) / 2;
                let slots = self.slots.as_mut().unwrap();
                if (n - 3) % 2 == 0 {
                    let allowed: &[&'static str] = if i < 4 {
                        &["long", "short", "narrow"]
                    } else if i < 7 {
                        &["long", "short", "narrow", "numeric", "2-digit"]
                    } else {
                        &["long", "short", "narrow", "numeric"]
                    };
                    let style = if absent {
                        None
                    } else {
                        Some(service::string_option(&value, allowed)?)
                    };
                    slots.unit_style(i, style);
                } else {
                    if !absent {
                        slots.always[i] =
                            service::string_option(&value, &["auto", "always"])? == "always";
                    }
                    slots.finish_unit(i)?;
                }
            }
        }
        Ok(self.advance_option())
    }
    fn primitive(&mut self, heap: &Heap, value: Value) -> Result<Action, Fault> {
        if let Value::String(text) = value {
            self.scanner = Some(Box::new(StringToNumber::new_accounted(heap, text)?));
            self.stage = Stage::Scan;
            Ok(Action::Continue)
        } else {
            self.number(dispatch::to_number(&value)?)
        }
    }
    fn number(&mut self, number: f64) -> Result<Action, Fault> {
        if self.scanning_option {
            self.scanning_option = false;
            return self.apply(Value::Number(number));
        }
        if !number.is_finite() || number.fract() != 0.0 {
            return Err(Fault::RangeError(
                "duration fields must be integral finite numbers".into(),
            ));
        }
        self.values[READ_ORDER[self.field]] = number;
        self.any = true;
        self.field += 1;
        self.stage = Stage::Field;
        Ok(Action::Continue)
    }
    fn field_value(&mut self, value: Value) -> Result<Action, Fault> {
        if matches!(value, Value::Undefined) {
            self.field += 1;
            self.stage = Stage::Field;
            return Ok(Action::Continue);
        }
        self.stage = Stage::Primitive;
        Ok(Action::ToPrimitiveNumber(value))
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
                if self.option == if self.supported { 1 } else { 24 } {
                    self.stage = Stage::Build;
                    return Ok(Action::Continue);
                }
                self.stage = Stage::OptionValue;
                match intl_service::read(heap, task, &self.options, self.option_name())? {
                    Ok(value) => self.option_value(value),
                    Err(action) => Ok(action),
                }
            }
            Stage::OptionValue => self.option_value(Self::result(input)?),
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
                    .intl_duration_format_prototype;
                let object = OrdinaryObject::with_prototype(heap, task, proto)
                    .with_duration_format(*self.slots.take().unwrap());
                heap.alloc(HeapObject::Plain(object))
                    .map(Value::Object)
                    .map(Action::Return)
                    .ok_or_else(|| Fault::OutOfMemory("heap exhausted"))
            }
            Stage::Start => {
                if !matches!(self.source, Value::Object(_)) {
                    return Err(Fault::TypeError(
                        "duration object required; duration strings are unsupported".into(),
                    ));
                }
                self.stage = Stage::Field;
                Ok(Action::Continue)
            }
            Stage::Field => {
                if self.field == 10 {
                    if !self.any {
                        return Err(Fault::TypeError("no duration fields found".into()));
                    }
                    self.stage = Stage::Format;
                    return Ok(Action::Continue);
                }
                self.stage = Stage::FieldValue;
                match intl_service::read(heap, task, &self.source, UNITS[READ_ORDER[self.field]])? {
                    Ok(value) => self.field_value(value),
                    Err(action) => Ok(action),
                }
            }
            Stage::FieldValue => self.field_value(Self::result(input)?),
            Stage::Primitive => self.primitive(heap, Self::result(input)?),
            Stage::Scan => match self.scanner.as_mut().unwrap().advance(256) {
                Some(number) => {
                    self.scanner = None;
                    self.number(number)
                }
                None => Ok(Action::Continue),
            },
            Stage::Format => self
                .slots
                .as_ref()
                .unwrap()
                .format(heap, task, &self.values, self.parts)
                .map(Action::Return),
        }
    }
}
