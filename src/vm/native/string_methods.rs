//! Saved coercions, rope traversal and bounded Unicode work for String methods.
use super::*;
use crate::value::string::{CursorStep, StringBuild, StringSearch, UnitCursor, is_str_white_space};
use crate::value::string_unicode as unicode;

pub(super) fn handles(which: builtin::Builtin) -> bool {
    use builtin::Builtin::*;
    matches!(
        which,
        StringToUpperCase
            | StringToLowerCase
            | StringToLocaleUpperCase
            | StringToLocaleLowerCase
            | StringTrim
            | StringTrimStart
            | StringTrimEnd
            | StringRepeat
            | StringStartsWith
            | StringEndsWith
            | StringIncludes
            | StringAt
            | StringLocaleCompare
            | StringNormalize
    )
}
#[derive(Debug, PartialEq, Eq)]
enum Stage {
    Locales,
    DefaultLocale,
    Text,
    TextValue,
    Argument,
    ArgumentValue,
    IsRegExp,
    Position,
    PositionValue,
    Collect,
    CollectNeedle,
    Prepare,
    Trim,
    Build,
    Search,
    Decode,
    Following,
    Case,
    Lookahead,
    Decompose,
    Reorder,
    Compose,
    Encode,
    Collator,
    Compare,
    Discard,
}
#[derive(Debug)]
enum EarlyResult {
    Empty,
    Undefined,
    RepeatCountError,
    StringTooLong,
}
#[derive(Debug)]
pub(super) struct State {
    which: builtin::Builtin,
    this: Value,
    args: [Value; 3],
    stage: Stage,
    locale_list: Option<Box<intl::Intl>>,
    language: String,
    collator: Option<Box<collator::State>>,
    comparison: Option<crate::intl::collator::Comparison>,
    cursor: Option<UnitCursor>,
    units: Vec<u16>,
    text_flat: bool,
    needle_flat: bool,
    text: JsString,
    needle: JsString,
    number: f64,
    start: usize,
    end: usize,
    at: usize,
    search: Option<StringSearch>,
    build: Option<StringBuild>,
    scalars: Vec<u32>,
    following: Vec<bool>,
    cased: bool,
    cased_property: crate::regexp::property::UnicodeProperty,
    ignorable_property: crate::regexp::property::UnicodeProperty,
    soft_dotted: crate::regexp::property::UnicodeProperty,
    normalized: Vec<u32>,
    expansion: &'static [u32],
    hangul: [u32; 3],
    hangul_at: usize,
    hangul_len: usize,
    insert: Option<usize>,
    starter: Option<usize>,
    last_cc: u8,
    form: usize,
    out: Vec<u16>,
    look: usize,
    blocked: bool,
    skip_dot: Option<usize>,
    passthrough_end: usize,
    add_dot: bool,
    early: Option<EarlyResult>,
}
impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.this.trace(visit);
        for value in &self.args {
            value.trace(visit);
        }
        if let Some(list) = &self.locale_list {
            list.trace(visit);
        }
        if let Some(collator) = &self.collator {
            collator.trace(visit);
        }
    }
}
fn result(input: Input) -> Result<Value, Fault> {
    match input {
        Input::CallResult(v) => Ok(v),
        _ => Err(Fault::Internal("String expected callback result")),
    }
}
fn text(value: Value) -> Result<JsString, Fault> {
    match value {
        Value::String(s) => Ok(s),
        _ => Err(Fault::Internal("String expected string")),
    }
}
fn reserve<T>(buffer: &mut Vec<T>, size: usize) -> Result<(), Fault> {
    buffer
        .try_reserve_exact(size)
        .map_err(|_| Fault::OutOfMemory("String method buffer"))
}
fn integer(number: f64) -> f64 {
    if number.is_nan() { 0.0 } else { number.trunc() }
}
impl State {
    pub(super) fn new(
        which: builtin::Builtin,
        this: &Value,
        args: &[Value],
    ) -> Result<Self, Fault> {
        Ok(Self {
            which,
            this: this.clone(),
            args: std::array::from_fn(|i| args.get(i).cloned().unwrap_or(Value::Undefined)),
            stage: Stage::Text,
            locale_list: None,
            language: String::new(),
            collator: None,
            comparison: None,
            cursor: None,
            units: vec![],
            text_flat: false,
            needle_flat: false,
            text: JsString::from(""),
            needle: JsString::from(""),
            number: 0.0,
            start: 0,
            end: 0,
            at: 0,
            search: None,
            build: None,
            scalars: vec![],
            following: vec![],
            cased: false,
            cased_property: crate::regexp::property::lookup("Cased").unwrap(),
            ignorable_property: crate::regexp::property::lookup("Case_Ignorable").unwrap(),
            soft_dotted: crate::regexp::property::lookup("Soft_Dotted").unwrap(),
            normalized: vec![],
            expansion: &[],
            hangul: [0; 3],
            hangul_at: 0,
            hangul_len: 0,
            insert: None,
            starter: None,
            last_cc: 0,
            form: 0,
            out: vec![],
            look: 0,
            blocked: false,
            skip_dot: None,
            passthrough_end: 0,
            add_dot: false,
            early: None,
        })
    }
    fn lower(&self) -> bool {
        matches!(
            self.which,
            builtin::Builtin::StringToLowerCase | builtin::Builtin::StringToLocaleLowerCase
        )
    }
    fn case(&self) -> bool {
        use builtin::Builtin::*;
        matches!(
            self.which,
            StringToUpperCase
                | StringToLowerCase
                | StringToLocaleUpperCase
                | StringToLocaleLowerCase
        )
    }
    fn collect(&mut self, source: JsString, stage: Stage) -> Result<Action, Fault> {
        reserve(&mut self.units, source.len())?;
        self.cursor = Some(UnitCursor::new(source));
        self.stage = stage;
        Ok(Action::Continue)
    }
    fn discard(&mut self, result: EarlyResult) -> Action {
        self.early = Some(result);
        self.cursor = Some(UnitCursor::new(std::mem::replace(
            &mut self.text,
            JsString::from(""),
        )));
        self.stage = Stage::Discard;
        Action::Continue
    }
    fn emit(&mut self, cp: u32) -> Result<(), Fault> {
        let len = if cp > 0xffff { 2 } else { 1 };
        if self.out.len() + len > crate::value::string::MAX_STRING_LEN {
            return Err(Fault::StringTooLong);
        }
        if cp > 0xffff {
            self.out.push((0xd800 + ((cp - 0x10000) >> 10)) as u16);
            self.out.push((0xdc00 + ((cp - 0x10000) & 1023)) as u16);
        } else {
            self.out.push(cp as u16);
        }
        Ok(())
    }
    fn finish(&mut self) -> Result<Action, Fault> {
        Ok(Action::Return(Value::String(
            JsString::from_owned_code_units(std::mem::take(&mut self.out))
                .ok_or(Fault::StringTooLong)?,
        )))
    }
    fn language(&mut self, locale: JsString) {
        // Explicit canonical tags are already bounded. The host boundary is
        // trusted but unvalidated; overlong host tags use root case rules.
        if locale.len() >= 512 {
            self.language.clear();
            return;
        }
        self.language = locale
            .to_string_lossy()
            .split('-')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
    }
    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        use builtin::Builtin::*;
        match self.stage {
            Stage::Discard => {
                for _ in 0..128 {
                    if !self.cursor.as_mut().unwrap().discard_step() {
                        self.cursor = None;
                        return match self.early.take().unwrap() {
                            EarlyResult::Empty => {
                                Ok(Action::Return(Value::String(JsString::from(""))))
                            }
                            EarlyResult::Undefined => Ok(Action::Return(Value::Undefined)),
                            EarlyResult::RepeatCountError => {
                                Err(Fault::RangeError("invalid repeat count".into()))
                            }
                            EarlyResult::StringTooLong => Err(Fault::StringTooLong),
                        };
                    }
                }
                Ok(Action::Continue)
            }
            Stage::Locales => match self.locale_list.as_mut().unwrap().step(heap, task, input)? {
                Action::Return(list) => {
                    let first = property::get(heap, task, &list, &index_key(0))?;
                    self.locale_list = None;
                    if let Value::String(locale) = first {
                        self.language(locale);
                        self.stage = Stage::Argument;
                        Ok(Action::Continue)
                    } else {
                        self.stage = Stage::DefaultLocale;
                        Ok(Action::DefaultLocale)
                    }
                }
                action => Ok(action),
            },
            Stage::DefaultLocale => {
                self.language(text(result(input)?)?);
                self.stage = Stage::Argument;
                Ok(Action::Continue)
            }
            Stage::Text => {
                if matches!(self.this, Value::Null | Value::Undefined) {
                    return Err(Fault::TypeError("String method requires a receiver".into()));
                }
                self.stage = Stage::TextValue;
                Ok(Action::ToString(std::mem::replace(
                    &mut self.this,
                    Value::Undefined,
                )))
            }
            Stage::TextValue => {
                self.text = text(result(input)?)?;
                if matches!(
                    self.which,
                    StringToLocaleUpperCase | StringToLocaleLowerCase
                ) {
                    // Keep large nested-native construction out of this step's
                    // frame, which remains live while stepping locale parsing.
                    self.locale_list = Some(box_constructor_state(|| {
                        intl::Intl::new(&self.args[..1], false)
                    })?);
                    self.stage = Stage::Locales;
                } else {
                    self.stage = Stage::Argument;
                }
                Ok(Action::Continue)
            }
            Stage::Argument => {
                self.stage = Stage::ArgumentValue;
                match self.which {
                    StringStartsWith | StringEndsWith | StringIncludes => {
                        if matches!(self.args[0], Value::Object(_)) {
                            self.stage = Stage::IsRegExp;
                            return Ok(Action::Instruction {
                                instr: Instr::GetElem,
                                operands: vec![
                                    self.args[0].clone(),
                                    Value::Symbol(crate::value::symbol::well_known("match")),
                                ],
                            });
                        }
                        Ok(Action::ToString(std::mem::replace(
                            &mut self.args[0],
                            Value::Undefined,
                        )))
                    }
                    StringRepeat | StringAt => Ok(Action::ToNumber(std::mem::replace(
                        &mut self.args[0],
                        Value::Undefined,
                    ))),
                    StringLocaleCompare => Ok(Action::ToString(std::mem::replace(
                        &mut self.args[0],
                        Value::Undefined,
                    ))),
                    StringNormalize if !matches!(self.args[0], Value::Undefined) => Ok(
                        Action::ToString(std::mem::replace(&mut self.args[0], Value::Undefined)),
                    ),
                    _ => {
                        self.stage = Stage::Prepare;
                        Ok(Action::Continue)
                    }
                }
            }
            Stage::IsRegExp => {
                let marker = result(input)?;
                let is_regexp = if matches!(marker, Value::Undefined) {
                    crate::regexp::object::data(heap, task, &self.args[0])?.is_some()
                } else {
                    marker.truthy()
                };
                if is_regexp {
                    return Err(Fault::TypeError(
                        "String search predicate does not accept a RegExp".into(),
                    ));
                }
                self.stage = Stage::ArgumentValue;
                Ok(Action::ToString(std::mem::replace(
                    &mut self.args[0],
                    Value::Undefined,
                )))
            }
            Stage::ArgumentValue => {
                let value = result(input)?;
                match self.which {
                    StringRepeat | StringAt => {
                        self.number = integer(dispatch::to_number(&value)?);
                    }
                    StringNormalize => {
                        let form = text(value)?;
                        if form.len() > 4 {
                            return Err(Fault::RangeError("invalid normalization form".into()));
                        }
                        self.form = match form.to_string_lossy().as_str() {
                            "NFC" => 0,
                            "NFD" => 1,
                            "NFKC" => 2,
                            "NFKD" => 3,
                            _ => {
                                return Err(Fault::RangeError("invalid normalization form".into()));
                            }
                        };
                    }
                    _ => self.needle = text(value)?,
                }
                self.stage = if matches!(
                    self.which,
                    StringStartsWith | StringEndsWith | StringIncludes
                ) {
                    Stage::Position
                } else {
                    Stage::Prepare
                };
                Ok(Action::Continue)
            }
            Stage::Position => {
                self.stage = Stage::PositionValue;
                if self.which == StringEndsWith && matches!(self.args[1], Value::Undefined) {
                    self.number = self.text.len() as f64;
                    self.stage = Stage::Prepare;
                    Ok(Action::Continue)
                } else {
                    Ok(Action::ToNumber(std::mem::replace(
                        &mut self.args[1],
                        Value::Undefined,
                    )))
                }
            }
            Stage::PositionValue => {
                self.number = integer(dispatch::to_number(&result(input)?)?);
                self.stage = Stage::Prepare;
                Ok(Action::Continue)
            }
            Stage::Collect | Stage::CollectNeedle => {
                for _ in 0..128 {
                    match self.cursor.as_mut().unwrap().step() {
                        CursorStep::Continue => (),
                        CursorStep::Unit(unit) => self.units.push(unit),
                        CursorStep::End => {
                            self.cursor = None;
                            let flat =
                                JsString::from_owned_code_units(std::mem::take(&mut self.units))
                                    .ok_or(Fault::StringTooLong)?;
                            if self.stage == Stage::CollectNeedle {
                                self.needle = flat;
                                self.needle_flat = true;
                            } else {
                                self.text = flat;
                                self.text_flat = true;
                            }
                            self.stage = Stage::Prepare;
                            return Ok(Action::Continue);
                        }
                    }
                }
                Ok(Action::Continue)
            }
            Stage::Prepare => self.prepare(heap, task),
            Stage::Trim => {
                let units = self.text.code_units();
                for _ in 0..128 {
                    if self.which != StringTrimEnd
                        && self.start < self.end
                        && is_str_white_space(units[self.start])
                    {
                        self.start += 1;
                    } else if self.which != StringTrimStart
                        && self.end > self.start
                        && is_str_white_space(units[self.end - 1])
                    {
                        self.end -= 1;
                    } else {
                        self.build = Some(
                            StringBuild::new(vec![(
                                self.text.clone(),
                                self.start,
                                self.end - self.start,
                            )])
                            .ok_or(Fault::StringTooLong)?,
                        );
                        self.stage = Stage::Build;
                        break;
                    }
                }
                Ok(Action::Continue)
            }
            Stage::Build => Ok(match self.build.as_mut().unwrap().advance(128) {
                Some(s) => Action::Return(Value::String(s)),
                None => Action::Continue,
            }),
            Stage::Search => Ok(match self.search.as_mut().unwrap().advance(128) {
                Some(index) => Action::Return(Value::Bool(if self.which == StringIncludes {
                    index >= 0
                } else {
                    index == self.start as isize
                })),
                None => Action::Continue,
            }),
            Stage::Decode => {
                let units = self.text.code_units();
                for _ in 0..128 {
                    if self.at == units.len() {
                        self.at = self.scalars.len();
                        self.stage = if self.case() {
                            Stage::Following
                        } else {
                            self.at = 0;
                            Stage::Decompose
                        };
                        return Ok(Action::Continue);
                    }
                    let mut cp = u32::from(units[self.at]);
                    self.at += 1;
                    if (0xd800..=0xdbff).contains(&cp)
                        && self.at < units.len()
                        && (0xdc00..=0xdfff).contains(&units[self.at])
                    {
                        cp = 0x10000 + ((cp - 0xd800) << 10) + u32::from(units[self.at]) - 0xdc00;
                        self.at += 1;
                    }
                    self.scalars.push(cp);
                }
                Ok(Action::Continue)
            }
            Stage::Following => {
                for _ in 0..128 {
                    if self.at == 0 {
                        self.cased = false;
                        self.stage = Stage::Case;
                        return Ok(Action::Continue);
                    }
                    self.at -= 1;
                    self.following.push(self.cased);
                    let cp = self.scalars[self.at];
                    if !self.ignorable_property.contains(cp, false, false) {
                        self.cased = self.cased_property.contains(cp, false, false);
                    }
                }
                Ok(Action::Continue)
            }
            Stage::Case => self.case_step(),
            Stage::Lookahead => self.lookahead(),
            Stage::Decompose | Stage::Reorder | Stage::Compose | Stage::Encode => {
                self.normalize_step()
            }
            Stage::Collator => match self.collator.as_mut().unwrap().step(heap, task, input)? {
                Action::Return(object) => {
                    let slots = crate::intl::collator::record(heap, task, &object)?;
                    self.collator = None;
                    self.comparison = Some(crate::intl::collator::Comparison::new(
                        slots,
                        self.text.clone(),
                        self.needle.clone(),
                    )?);
                    self.stage = Stage::Compare;
                    Ok(Action::Continue)
                }
                action => Ok(action),
            },
            Stage::Compare => Ok(match self.comparison.as_mut().unwrap().advance()? {
                Some(order) => Action::Return(Value::Number(match order {
                    std::cmp::Ordering::Less => -1.0,
                    std::cmp::Ordering::Equal => 0.0,
                    std::cmp::Ordering::Greater => 1.0,
                })),
                None => Action::Continue,
            }),
        }
    }
    fn prepare(&mut self, heap: &Heap, task: TaskId) -> Result<Action, Fault> {
        use builtin::Builtin::*;
        if self.which == StringLocaleCompare {
            self.collator = Some(box_constructor_state(|| {
                collator::State::new(IntlCollator, &Value::Undefined, &self.args[1..], heap, task)
            })?);
            self.stage = Stage::Collator;
            return Ok(Action::Continue);
        }
        let len = self.text.len();
        if self.which == StringRepeat {
            if self.number < 0.0 || !self.number.is_finite() {
                return Ok(self.discard(EarlyResult::RepeatCountError));
            }
            if self.number == 0.0 || len == 0 {
                return Ok(self.discard(EarlyResult::Empty));
            }
            if self.number > (crate::value::string::MAX_STRING_LEN / len) as f64 {
                return Ok(self.discard(EarlyResult::StringTooLong));
            }
        }
        if self.which == StringAt {
            let index = if self.number < 0.0 {
                len as f64 + self.number
            } else {
                self.number
            };
            if !(0.0..len as f64).contains(&index) {
                return Ok(self.discard(EarlyResult::Undefined));
            }
            self.start = index as usize;
        }
        if !self.text_flat {
            let source = std::mem::replace(&mut self.text, JsString::from(""));
            return self.collect(source, Stage::Collect);
        }
        if matches!(
            self.which,
            StringStartsWith | StringEndsWith | StringIncludes
        ) {
            if !self.needle_flat {
                let source = std::mem::replace(&mut self.needle, JsString::from(""));
                return self.collect(source, Stage::CollectNeedle);
            }
            let pos = self.number.max(0.0).min(len as f64) as usize;
            self.start = if self.which == StringEndsWith {
                let Some(start) = pos.checked_sub(self.needle.len()) else {
                    return Ok(Action::Return(Value::Bool(false)));
                };
                start
            } else {
                pos
            };
            let mut search =
                StringSearch::new(self.text.clone(), self.needle.clone(), self.start, false);
            search
                .reserve_prefix()
                .map_err(|_| Fault::OutOfMemory("String search prefix"))?;
            self.search = Some(search);
            self.stage = Stage::Search;
        } else if matches!(self.which, StringTrim | StringTrimStart | StringTrimEnd) {
            self.start = 0;
            self.end = len;
            self.stage = Stage::Trim;
        } else if self.which == StringRepeat {
            self.build = Some(
                StringBuild::new(vec![(self.text.clone(), 0, len * self.number as usize)])
                    .ok_or(Fault::StringTooLong)?,
            );
            self.stage = Stage::Build;
        } else if self.which == StringAt {
            return Ok(Action::Return(Value::String(
                self.text.code_unit_at(self.start).unwrap(),
            )));
        } else {
            reserve(&mut self.scalars, len)?;
            // Unicode 17 maximum default case expansion is 3 UTF-16 units per
            // source unit; maximum fully recursive compatibility expansion 18.
            let cap = len
                .saturating_mul(if self.case() { 3 } else { 18 })
                .min(crate::value::string::MAX_STRING_LEN);
            reserve(&mut self.out, cap)?;
            if self.case() {
                reserve(&mut self.following, len)?;
            } else {
                reserve(&mut self.normalized, cap)?;
            }
            self.at = 0;
            self.stage = Stage::Decode;
        }
        Ok(Action::Continue)
    }
    fn advance_case(&mut self) {
        let cp = self.scalars[self.at];
        if !self.ignorable_property.contains(cp, false, false) {
            self.cased = self.cased_property.contains(cp, false, false);
        }
        self.at += 1;
    }
    fn default_case(&mut self, cp: u32) -> Result<(), Fault> {
        if let Some(map) = unicode::mapping(cp, usize::from(self.lower())) {
            for &mapped in map {
                self.emit(mapped)?;
            }
        } else {
            self.emit(cp)?;
        }
        Ok(())
    }
    fn case_step(&mut self) -> Result<Action, Fault> {
        for _ in 0..128 {
            if self.at == self.scalars.len() {
                return self.finish();
            }
            let cp = self.scalars[self.at];
            if self.skip_dot == Some(self.at) {
                self.skip_dot = None;
                self.advance_case();
                continue;
            }
            if self.at < self.passthrough_end {
                self.emit(cp)?;
                self.advance_case();
                continue;
            }
            let turkish = matches!(self.language.as_str(), "tr" | "az");
            let lithuanian = self.language == "lt";
            if self.lower()
                && cp == 0x3a3
                && self.cased
                && !self.following[self.scalars.len() - 1 - self.at]
            {
                self.emit(0x3c2)?;
            } else if self.lower()
                && ((turkish && cp == 0x49) || (lithuanian && matches!(cp, 0x49 | 0x4a | 0x12e)))
                || (!self.lower() && lithuanian && self.soft_dotted.contains(cp, false, false))
            {
                self.look = self.at + 1;
                self.blocked = false;
                self.add_dot = false;
                self.stage = Stage::Lookahead;
                return Ok(Action::Continue);
            } else if turkish && self.lower() && cp == 0x130 {
                self.emit(0x69)?;
            } else if turkish && !self.lower() && cp == 0x69 {
                self.emit(0x130)?;
            } else if lithuanian && self.lower() && matches!(cp, 0xcc | 0xcd | 0x128) {
                self.emit(0x69)?;
                self.emit(0x307)?;
                self.emit(match cp {
                    0xcc => 0x300,
                    0xcd => 0x301,
                    _ => 0x303,
                })?;
            } else {
                self.default_case(cp)?;
            }
            self.advance_case();
        }
        Ok(Action::Continue)
    }
    fn lookahead(&mut self) -> Result<Action, Fault> {
        let turkish = matches!(self.language.as_str(), "tr" | "az");
        let mut complete = false;
        for _ in 0..128 {
            let cp = self.scalars.get(self.look).copied();
            if let Some(cp) = cp.filter(|&cp| self.ignorable_property.contains(cp, false, false)) {
                if cp == 0x307 && (turkish || !self.lower()) {
                    if !self.blocked {
                        self.skip_dot = Some(self.look);
                    }
                    complete = true;
                    break;
                }
                if unicode::cc(cp) == 230 {
                    if self.lower() && !turkish {
                        self.add_dot = true;
                        complete = true;
                        break;
                    }
                    if turkish {
                        self.blocked = true;
                    }
                }
                self.look += 1;
                continue;
            }
            complete = true;
            break;
        }
        if !complete {
            return Ok(Action::Continue);
        }
        let cp = self.scalars[self.at];
        if self.skip_dot.is_some() || !self.lower() {
            self.passthrough_end = self.look;
        }
        if turkish {
            self.emit(if self.skip_dot.is_some() { 0x69 } else { 0x131 })?;
        } else {
            self.default_case(cp)?;
            if self.add_dot {
                self.emit(0x307)?;
            }
        }
        self.advance_case();
        self.stage = Stage::Case;
        Ok(Action::Continue)
    }
    fn normalize_step(&mut self) -> Result<Action, Fault> {
        for _ in 0..128 {
            match self.stage {
                Stage::Decompose => {
                    let next = if let Some((&cp, rest)) = self.expansion.split_first() {
                        self.expansion = rest;
                        Some(cp)
                    } else if self.hangul_at < self.hangul_len {
                        let cp = self.hangul[self.hangul_at];
                        self.hangul_at += 1;
                        Some(cp)
                    } else {
                        None
                    };
                    if let Some(cp) = next {
                        if self.normalized.len() == crate::value::string::MAX_STRING_LEN {
                            return Err(Fault::StringTooLong);
                        }
                        self.normalized.push(cp);
                        self.insert = Some(self.normalized.len() - 1);
                        self.stage = Stage::Reorder;
                    } else if self.at < self.scalars.len() {
                        let cp = self.scalars[self.at];
                        self.at += 1;
                        if let Some(map) = unicode::mapping(cp, if self.form >= 2 { 3 } else { 2 })
                        {
                            self.expansion = map;
                        } else if (0xac00..=0xd7a3).contains(&cp) {
                            let n = cp - 0xac00;
                            self.hangul =
                                [0x1100 + n / 588, 0x1161 + (n % 588) / 28, 0x11a7 + n % 28];
                            self.hangul_at = 0;
                            self.hangul_len = if n.is_multiple_of(28) { 2 } else { 3 };
                        } else {
                            self.hangul[0] = cp;
                            self.hangul_at = 0;
                            self.hangul_len = 1;
                        }
                    } else {
                        self.at = 0;
                        self.start = 0;
                        self.stage = if self.form.is_multiple_of(2) {
                            Stage::Compose
                        } else {
                            Stage::Encode
                        };
                    }
                }
                Stage::Reorder => {
                    let i = self.insert.unwrap();
                    let cc = unicode::cc(self.normalized[i]);
                    if i > 0 && cc != 0 && unicode::cc(self.normalized[i - 1]) > cc {
                        self.normalized.swap(i - 1, i);
                        self.insert = Some(i - 1);
                    } else {
                        self.insert = None;
                        self.stage = Stage::Decompose;
                    }
                }
                Stage::Compose => {
                    if self.at == self.normalized.len() {
                        self.normalized.truncate(self.start);
                        self.at = 0;
                        self.stage = Stage::Encode;
                        continue;
                    }
                    let cp = self.normalized[self.at];
                    self.at += 1;
                    let cc = unicode::cc(cp);
                    if let Some(starter) = self.starter
                        && (self.last_cc == 0 || self.last_cc < cc)
                        && let Some(composed) = unicode::compose(self.normalized[starter], cp)
                    {
                        self.normalized[starter] = composed;
                    } else {
                        if cc == 0 {
                            self.starter = Some(self.start);
                        }
                        self.normalized[self.start] = cp;
                        self.start += 1;
                        self.last_cc = cc;
                    }
                }
                Stage::Encode => {
                    if self.at == self.normalized.len() {
                        return self.finish();
                    }
                    self.emit(self.normalized[self.at])?;
                    self.at += 1;
                }
                _ => unreachable!(),
            }
        }
        Ok(Action::Continue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::string::cursor_release_counts;

    #[test]
    fn methods_release_deep_rope_nodes_in_bounded_steps() {
        for (which, argument) in [
            (builtin::Builtin::StringToUpperCase, 2.0),
            (builtin::Builtin::StringNormalize, 2.0),
            (builtin::Builtin::StringTrim, 2.0),
            (builtin::Builtin::StringRepeat, 2.0),
            (builtin::Builtin::StringRepeat, 0.0),
            (builtin::Builtin::StringRepeat, f64::NAN),
            (builtin::Builtin::StringAt, f64::INFINITY),
            (builtin::Builtin::StringAt, -50_000.0),
            (builtin::Builtin::StringAt, 50_000.0),
        ] {
            for prepend in [false, true] {
                let heap = Heap::default();
                let task = TaskId::from_raw(300);
                let mut rope = JsString::from("                                ");
                for _ in 0..4096 {
                    let unit = JsString::from("a");
                    rope = if prepend {
                        unit.concat(&rope)
                    } else {
                        rope.concat(&unit)
                    }
                    .unwrap();
                }
                let mut native =
                    State::new(which, &Value::String(rope), &[Value::Number(argument)])
                        .unwrap_or_else(|_| panic!("state"));
                if which == builtin::Builtin::StringNormalize {
                    native.args[0] = Value::Undefined;
                }
                let mut input = Input::Start;
                let mut complete = false;
                let initial = cursor_release_counts();
                for _ in 0..5000 {
                    let before = cursor_release_counts();
                    let action = native
                        .step(&heap, task, input)
                        .unwrap_or_else(|_| panic!("native step"));
                    let after = cursor_release_counts();
                    assert!(after.0 - before.0 <= 128, "cursor disposal exceeded budget");
                    assert!(after.1 - before.1 <= 132, "rope disposal exceeded budget");
                    input = match action {
                        Action::Continue => Input::Start,
                        Action::ToString(v) => Input::CallResult(Value::String(
                            dispatch::to_js_string(&v).unwrap_or_else(|_| panic!("ToString")),
                        )),
                        Action::ToNumber(v) => Input::CallResult(Value::Number(
                            dispatch::to_number(&v).unwrap_or_else(|_| panic!("ToNumber")),
                        )),
                        Action::Return(Value::String(_) | Value::Undefined) => {
                            complete = true;
                            break;
                        }
                        _ => panic!("unexpected action"),
                    };
                }
                assert!(complete);
                assert!(cursor_release_counts().1 - initial.1 >= 8192);
                let before = cursor_release_counts();
                drop(native);
                let after = cursor_release_counts();
                assert!(after.0 - before.0 <= 2);
                assert!(after.1 - before.1 <= 4);
            }
        }
    }
}
