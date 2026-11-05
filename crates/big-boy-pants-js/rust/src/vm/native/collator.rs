//! Collator option getters and ToString calls use ordinary resumable frames.
use super::intl_service::{self, OptionKind};
use super::*;
use crate::value::string::{CursorStep, UnitCursor};
use crate::{
    heap::HeapObject,
    intl::{
        collator::{self, Collator, Comparison},
        service,
        tag::LanguageTag,
    },
    object::OrdinaryObject,
};
const OPTIONS: &[&str] = &[
    "localeMatcher",
    "usage",
    "sensitivity",
    "ignorePunctuation",
    "collation",
    "numeric",
    "caseFirst",
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
    Left,
    LeftString,
    Right,
    RightString,
    Compare,
    Collation,
}
#[derive(Debug)]
pub(super) struct State {
    list: Option<super::intl::Intl>,
    locales: Value,
    options: Value,
    arguments: [Value; 2],
    strings: [Option<JsString>; 2],
    stage: Stage,
    supported: bool,
    option: usize,
    slots: Option<Collator>,
    comparison: Option<Comparison>,
    validation: Option<UnitCursor>,
    subtag_len: usize,
    collation_match: usize,
}
impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        if let Some(list) = &self.list {
            list.trace(visit);
        }
        self.locales.trace(visit);
        self.options.trace(visit);
        for arg in &self.arguments {
            arg.trace(visit);
        }
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
        let compare = which == builtin::Builtin::IntlCollatorBound;
        let source = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Self {
            list: if compare {
                None
            } else {
                Some(super::intl::Intl::new(
                    std::slice::from_ref(&source),
                    false,
                )?)
            },
            locales: Value::Undefined,
            options: if compare {
                Value::Undefined
            } else {
                args.get(1).cloned().unwrap_or(Value::Undefined)
            },
            arguments: if compare {
                [source, args.get(1).cloned().unwrap_or(Value::Undefined)]
            } else {
                [Value::Undefined, Value::Undefined]
            },
            strings: [None, None],
            stage: if compare { Stage::Left } else { Stage::List },
            supported: which == builtin::Builtin::IntlCollatorSupportedLocales,
            option: 0,
            slots: if compare {
                Some(collator::record(heap, task, this)?)
            } else {
                None
            },
            comparison: None,
            validation: None,
            subtag_len: 0,
            collation_match: 0,
        })
    }
    fn result(input: Input) -> Result<Value, Fault> {
        if let Input::CallResult(value) = input {
            Ok(value)
        } else {
            Err(Fault::Internal("Collator expected callback"))
        }
    }
    fn set_locale(&mut self, tag: String) {
        let mut parsed = LanguageTag::parse(&tag).expect("resolved locale");
        let mut slots = Collator::new(tag);
        if let Some(co) = parsed.keywords.get("co") {
            slots.collation = slots.supported_collation(co);
            if slots.collation == "default" {
                parsed.keywords.remove("co");
            }
        }
        if let Some(kn) = parsed.keywords.get("kn") {
            if kn.is_empty() || kn == "true" {
                slots.numeric = true;
            } else if kn != "false" {
                parsed.keywords.remove("kn");
            }
        }
        if let Some(kf) = parsed.keywords.get("kf") {
            match kf.as_str() {
                "upper" => slots.case_first = "upper",
                "lower" => slots.case_first = "lower",
                "false" => (),
                _ => {
                    parsed.keywords.remove("kf");
                }
            }
        }
        slots.locale = parsed.to_string();
        self.slots = Some(slots);
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
        let kind = if matches!(self.option, 3 | 5) {
            OptionKind::Boolean
        } else {
            OptionKind::String
        };
        match intl_service::convert(value, kind)? {
            Ok(value) => self.apply(value),
            Err(action) => Ok(action),
        }
    }
    fn apply(&mut self, value: Value) -> Result<Action, Fault> {
        // Enumerated options have no valid spelling longer than eight UTF-16
        // units. Reject by cached length before shared string conversion can
        // flatten an arbitrarily long rope.
        if matches!(self.option, 0 | 1 | 2 | 6)
            && let Value::String(text) = &value
            && text.len() > 8
        {
            return Err(Fault::RangeError("invalid Intl option".into()));
        }
        if self.option == 0 {
            service::string_option(&value, &["lookup", "best fit"])?;
            self.option = 1;
            self.stage = Stage::Resolve;
            return Ok(Action::Continue);
        }
        let slots = self.slots.as_mut().unwrap();
        let mut remove = None;
        match self.option {
            1 => {
                slots.usage = service::string_option(&value, &["sort", "search"])?;
                if slots.usage == "search" {
                    slots.collation = "default";
                    remove = Some("co");
                }
            }
            2 => {
                slots.sensitivity =
                    service::string_option(&value, &["base", "accent", "case", "variant"])?
            }
            3 => slots.ignore_punctuation = value.truthy(),
            4 => {
                // Validate arbitrarily long ropes while remembering only a
                // bounded match against the one supported collation name.
                self.validation = Some(UnitCursor::new(dispatch::to_js_string(&value)?));
                self.subtag_len = 0;
                self.collation_match = 0;
                self.stage = Stage::Collation;
                return Ok(Action::Continue);
            }
            5 => {
                if slots.numeric != value.truthy() {
                    remove = Some("kn");
                }
                slots.numeric = value.truthy();
            }
            6 => {
                let case = service::string_option(&value, &["upper", "lower", "false"])?;
                if slots.case_first != case {
                    remove = Some("kf");
                }
                slots.case_first = case;
            }
            _ => unreachable!(),
        }
        if let Some(key) = remove {
            let mut parsed = LanguageTag::parse(&slots.locale).expect("resolved locale");
            parsed.keywords.remove(key);
            slots.locale = parsed.to_string();
        }
        self.option += 1;
        self.stage = Stage::Read;
        Ok(Action::Continue)
    }
    fn string(&mut self, side: usize, value: Value) -> Result<Action, Fault> {
        self.strings[side] = Some(dispatch::to_js_string(&value)?);
        self.stage = if side == 0 {
            Stage::Right
        } else {
            Stage::Compare
        };
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
                let selected =
                    service::resolve_locale(heap, task, &self.locales, &["co", "kn", "kf"])?;
                if selected.is_none() && !self.supported {
                    self.stage = Stage::Default;
                    return Ok(Action::DefaultLocale);
                }
                self.set_locale(selected.map_or_else(|| "en-US".into(), |(tag, _)| tag));
                self.stage = Stage::Read;
                Ok(Action::Continue)
            }
            Stage::Default => {
                let Value::String(default) = Self::result(input)? else {
                    return Err(Fault::Internal("default locale"));
                };
                let tag =
                    service::supported_locale(&default.to_string_lossy(), &["co", "kn", "kf"])
                        .map_or_else(|| "en-US".into(), |(tag, _)| tag);
                self.set_locale(tag);
                self.stage = Stage::Read;
                Ok(Action::Continue)
            }
            Stage::Read => {
                if self.option == if self.supported { 1 } else { OPTIONS.len() } {
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
            Stage::Collation => {
                for _ in 0..128 {
                    match self.validation.as_mut().unwrap().step() {
                        CursorStep::Continue => (),
                        CursorStep::End => {
                            if !(3..=8).contains(&self.subtag_len) {
                                return Err(Fault::RangeError("invalid collation option".into()));
                            }
                            self.validation = None;
                            let slots = self.slots.as_mut().unwrap();
                            let collation = slots.supported_collation(
                                if self.collation_match == b"phonebk".len() {
                                    "phonebk"
                                } else {
                                    "default"
                                },
                            );
                            if slots.collation != collation {
                                let mut parsed =
                                    LanguageTag::parse(&slots.locale).expect("resolved locale");
                                parsed.keywords.remove("co");
                                slots.locale = parsed.to_string();
                            }
                            slots.collation = collation;
                            self.option += 1;
                            self.stage = Stage::Read;
                            return Ok(Action::Continue);
                        }
                        CursorStep::Unit(c) => {
                            if b"phonebk".get(self.collation_match).copied().map(u16::from)
                                == Some(c)
                            {
                                self.collation_match += 1;
                            } else {
                                self.collation_match = usize::MAX;
                            }
                            if c == u16::from(b'-') && (3..=8).contains(&self.subtag_len) {
                                self.subtag_len = 0;
                            } else if c <= 127
                                && (c as u8).is_ascii_alphanumeric()
                                && self.subtag_len < 8
                            {
                                self.subtag_len += 1;
                            } else {
                                return Err(Fault::RangeError("invalid collation option".into()));
                            }
                        }
                    }
                }
                Ok(Action::Continue)
            }
            Stage::Build => {
                if self.supported {
                    return service::supported_locales(heap, task, &self.locales)
                        .map(Action::Return);
                }
                let proto = crate::realm::intrinsics(heap)
                    .ok_or_else(|| Fault::OutOfMemory("realm exhausted"))?
                    .intl_collator_prototype;
                let object = OrdinaryObject::with_prototype(heap, task, proto)
                    .with_collator(self.slots.take().unwrap());
                heap.alloc(HeapObject::Plain(object))
                    .map(Value::Object)
                    .map(Action::Return)
                    .ok_or_else(|| Fault::OutOfMemory("heap exhausted"))
            }
            Stage::Left | Stage::Right => {
                let side = usize::from(matches!(self.stage, Stage::Right));
                self.stage = if side == 0 {
                    Stage::LeftString
                } else {
                    Stage::RightString
                };
                // ToString also budgets BigInt conversion and invokes object
                // hooks in the trampoline; conversion order is strictly a,b.
                Ok(Action::ToString(std::mem::replace(
                    &mut self.arguments[side],
                    Value::Undefined,
                )))
            }
            Stage::LeftString => self.string(0, Self::result(input)?),
            Stage::RightString => self.string(1, Self::result(input)?),
            Stage::Compare => {
                if self.comparison.is_none() {
                    self.comparison = Some(Comparison::new(
                        self.slots.take().unwrap(),
                        self.strings[0].take().unwrap(),
                        self.strings[1].take().unwrap(),
                    )?);
                }
                match self.comparison.as_mut().unwrap().advance()? {
                    None => Ok(Action::Continue),
                    Some(order) => Ok(Action::Return(Value::Number(match order {
                        std::cmp::Ordering::Less => -1.0,
                        std::cmp::Ordering::Equal => 0.0,
                        std::cmp::Ordering::Greater => 1.0,
                    }))),
                }
            }
        }
    }
}
