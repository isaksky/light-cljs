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
    Cleanup,
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
    discard: Option<UnitCursor>,
    failure: Option<&'static str>,
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
        let slots = if of {
            Some(display_names::record(heap, task, this)?)
        } else {
            None
        };
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
            slots,
            calendar: None,
            normalized: JsString::from(""),
            subtag_len: 0,
            discard: None,
            failure: None,
        })
    }
    fn reject(&mut self, kind: &'static str) -> Action {
        self.failure = Some(kind);
        self.discard = Some(UnitCursor::new(std::mem::replace(
            &mut self.normalized,
            JsString::from(""),
        )));
        self.stage = Stage::Cleanup;
        Action::Continue
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
                if self.supported {
                    if matches!(self.options, Value::Null) {
                        return Err(Fault::TypeError("Intl options cannot be null".into()));
                    }
                    // Primitive wrappers have no localeMatcher own property;
                    // the fixed realm prototypes cannot add one. Avoid the
                    // general Boolean/BigInt primitive-property gap here.
                    if !matches!(self.options, Value::Undefined | Value::Object(_)) {
                        self.options = Value::Undefined;
                    }
                } else {
                    service::get_options_object(&self.options)?;
                }
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
                let Value::String(code) = std::mem::replace(&mut self.code, Value::Undefined)
                else {
                    unreachable!()
                };
                let slots = self.slots.as_ref().unwrap();
                // Locale's parser accepts at most 511 ASCII units. All valid
                // language ids therefore fit a bounded leaf parse; longer
                // ropes still drain through the saved cleanup cursor.
                let limit = if slots.kind == "language" { 511 } else { 32 };
                if code.len() > limit {
                    self.calendar = Some(UnitCursor::new(code));
                    if slots.kind != "calendar" {
                        return Ok(self.reject(slots.kind));
                    }
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
                                return Ok(self.reject("calendar"));
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
                                return Ok(self.reject("calendar"));
                            }
                            chunk.push((c as u8).to_ascii_lowercase() as char);
                        }
                    }
                }
                if self.slots.as_ref().unwrap().fallback == "code" {
                    self.normalized = self
                        .normalized
                        .concat(&JsString::from(chunk.as_str()))
                        .ok_or_else(|| Fault::RangeError("calendar code too long".into()))?;
                }
                if done {
                    // All snapshot calendar keys and aliases fit in 32 units:
                    // a longer validated type is an unknown canonical code.
                    return Ok(Action::Return(
                        if self.slots.as_ref().unwrap().fallback == "none" {
                            Value::Undefined
                        } else {
                            Value::String(self.normalized.clone())
                        },
                    ));
                }
                Ok(Action::Continue)
            }
            Stage::Cleanup => {
                for _ in 0..128 {
                    if let Some(cursor) = &mut self.calendar {
                        if !cursor.discard_step() {
                            self.calendar = None;
                        }
                    } else if let Some(cursor) = &mut self.discard {
                        if !cursor.discard_step() {
                            self.discard = None;
                        }
                    } else {
                        let kind = self.failure.take().unwrap();
                        return Err(Fault::RangeError(format!("invalid {kind} code")));
                    }
                }
                Ok(Action::Continue)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::string::cursor_release_counts;

    #[test]
    fn long_calendar_cleanup_is_budgeted() {
        for (kind, fallback, invalid) in [
            ("calendar", "none", 0),
            ("calendar", "code", 0),
            ("calendar", "none", 1),
            ("calendar", "code", 1),
            ("calendar", "none", 2),
            ("calendar", "code", 2),
            ("region", "code", 1),
            ("language", "code", 1),
        ] {
            let heap = Heap::default();
            let task = TaskId::from_raw(1);
            let proto = crate::realm::intrinsics(&heap)
                .unwrap()
                .intl_display_names_prototype;
            let mut slots = DisplayNames::new("en".into());
            slots.kind = kind;
            slots.fallback = fallback;
            let this = Value::Object(
                heap.alloc(HeapObject::Plain(
                    OrdinaryObject::with_prototype(task, proto).with_display_names(slots),
                ))
                .unwrap(),
            );
            let mut text = JsString::from(if invalid == 1 { "!BC" } else { "ABC" });
            for _ in 0..3000 {
                text = text.concat(&JsString::from("-DEF")).unwrap();
            }
            if invalid == 2 {
                text = text.concat(&JsString::from("-X")).unwrap();
            }
            // State owns the only Rust reference to this temporary rope.
            let mut state = State::new(
                builtin::Builtin::IntlDisplayNamesOf,
                &this,
                &[Value::String(text)],
                &heap,
                task,
            )
            .unwrap_or_else(|_| panic!("DisplayNames state construction failed"));
            let mut returned = Value::Undefined;
            let mut cleanup = 0;
            let mut finished = false;
            for _ in 0..10000 {
                let before = cursor_release_counts();
                if matches!(state.stage, Stage::Cleanup) {
                    cleanup += 1;
                }
                let result = state.step(&heap, task, Input::Start);
                let after = cursor_release_counts();
                assert!(after.0 - before.0 <= 128, "cursor cleanup exceeded budget");
                assert!(after.1 - before.1 <= 144, "rope cleanup exceeded budget");
                match result {
                    Ok(Action::Continue) => (),
                    Ok(Action::Return(value)) => {
                        assert_eq!(invalid, 0);
                        if fallback == "none" {
                            assert!(matches!(value, Value::Undefined));
                        } else {
                            assert!(matches!(value,Value::String(ref s) if s.len()==12003));
                        }
                        returned = value;
                        finished = true;
                        break;
                    }
                    Err(Fault::RangeError(_)) => {
                        assert_ne!(invalid, 0);
                        finished = true;
                        break;
                    }
                    _ => panic!("unexpected native action"),
                }
            }
            assert!(finished);
            if invalid == 1 || (invalid == 2 && fallback == "code") {
                assert!(cleanup > 1, "{kind} {fallback} {invalid}: {cleanup}");
            }
            let before = cursor_release_counts();
            drop(state);
            let after = cursor_release_counts();
            assert!(
                after.0 - before.0 <= 2,
                "native-frame cursor disposal exceeded budget"
            );
            // Successful fallback code hands its output to the caller,
            // which retains it while the native frame is disposed.
            assert!(
                after.1 - before.1 <= 16,
                "native-frame rope disposal exceeded budget"
            );
            drop(returned);
        }
    }
}
