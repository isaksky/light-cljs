//! Decimal NumberFormat slots and exact decimal arithmetic. Numbers enter via
//! the shared Number::toString helper, as QuickJS's Intl mathematical values do.
use super::{LocaleData, data::numbers, table_string};
use crate::heap::{Heap, HeapObject};
use crate::ids::TaskId;
use crate::object::{OrdinaryObject, PropertyFlags};
use crate::value::{JsString, Value};
use crate::vm::{
    dispatch::{self, Fault},
    property,
};

#[derive(Clone, Debug)]
pub(crate) struct NumberFormat {
    pub locale: String,
    pub data: &'static LocaleData,
    pub grouping: &'static str,
    pub min_integer: usize,
    pub min_fraction: usize,
    pub max_fraction: usize,
    pub significant: Option<(usize, usize)>,
    pub rounding: &'static str,
}
impl NumberFormat {
    pub fn new(locale: String, data: &'static LocaleData) -> Self {
        Self {
            locale,
            data,
            grouping: "auto",
            min_integer: 1,
            min_fraction: 0,
            max_fraction: 3,
            significant: None,
            rounding: "halfExpand",
        }
    }
    pub fn format(&self, value: &Value) -> Result<Value, Fault> {
        let text = match value {
            Value::BigInt(n) => n.to_string(),
            Value::String(s) => s.to_string_lossy(),
            _ => {
                let n = dispatch::to_number(value)?;
                if n == 0.0 && n.is_sign_negative() {
                    "-0".into()
                } else {
                    crate::value::string::number_to_string(n)
                }
            }
        };
        let text = text.trim();
        let negative = text.starts_with('-');
        let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
        let symbol =
            |table, index| table_string(table, index as usize).expect("pinned number symbol");
        let body = if unsigned == "Infinity" {
            symbol(numbers::infinity_tab, self.data.symbols.infinity_idx).to_owned()
        } else {
            match Decimal::parse(unsigned)? {
                None => {
                    return Ok(Value::String(JsString::from(symbol(
                        numbers::nan_tab,
                        self.data.symbols.nan_idx,
                    ))));
                }
                Some(mut n) => {
                    let (places, minimum) = if let Some((min, max)) = self.significant {
                        (max as i32 - n.point, (min as i32 - n.point).max(0) as usize)
                    } else {
                        (self.max_fraction as i32, self.min_fraction)
                    };
                    n.round(places, self.rounding, negative);
                    let minimum = if let Some((min, _)) = self.significant {
                        (min as i32 - n.point).max(0) as usize
                    } else {
                        minimum
                    };
                    let (mut integer, mut fraction) = n.parts();
                    while fraction.ends_with('0') && fraction.len() > minimum {
                        fraction.pop();
                    }
                    fraction.extend(std::iter::repeat_n(
                        '0',
                        minimum.saturating_sub(fraction.len()),
                    ));
                    if integer.len() < self.min_integer {
                        integer = "0".repeat(self.min_integer - integer.len()) + &integer;
                    }
                    let first = (self.data.groupSize1 as usize).max(1);
                    let second = if self.data.groupSize2 == 0 {
                        first
                    } else {
                        self.data.groupSize2 as usize
                    };
                    let threshold = first + if self.grouping == "min2" { 2 } else { 1 };
                    if self.grouping != "false" && integer.len() >= threshold {
                        let mut grouped = String::new();
                        for (i, c) in integer.chars().enumerate() {
                            let right = integer.len() - i;
                            if i > 0
                                && (right == first
                                    || (right > first && (right - first).is_multiple_of(second)))
                            {
                                grouped.push_str(self.data.group_separator().unwrap());
                            }
                            grouped.push(c);
                        }
                        integer = grouped;
                    }
                    if !fraction.is_empty() {
                        integer.push_str(self.data.decimal_separator().unwrap());
                        integer.push_str(&fraction);
                    }
                    integer
                }
            }
        };
        let result = if negative {
            format!(
                "{}{body}",
                symbol(numbers::minusSign_tab, self.data.symbols.minusSign_idx)
            )
        } else {
            body
        };
        Ok(Value::String(JsString::from(result.as_str())))
    }
    pub fn resolved(&self, heap: &Heap, task: TaskId) -> Result<Value, Fault> {
        let proto = crate::realm::intrinsics(heap)
            .ok_or(Fault::OutOfMemory("realm exhausted"))?
            .object_prototype;
        let value = heap
            .alloc(HeapObject::Plain(OrdinaryObject::with_prototype(
                task, proto,
            )))
            .map(Value::Object)
            .ok_or(Fault::OutOfMemory("heap exhausted"))?;
        let mut fields = vec![
            (
                "locale",
                Value::String(JsString::from(self.locale.as_str())),
            ),
            ("numberingSystem", Value::String(JsString::from("latn"))),
            ("style", Value::String(JsString::from("decimal"))),
            (
                "minimumIntegerDigits",
                Value::Number(self.min_integer as f64),
            ),
        ];
        // The pinned QuickJS snapshot reports fraction bounds even when
        // significant-digit rounding takes precedence.
        fields.extend([
            (
                "minimumFractionDigits",
                Value::Number(self.min_fraction as f64),
            ),
            (
                "maximumFractionDigits",
                Value::Number(self.max_fraction as f64),
            ),
        ]);
        if let Some((min, max)) = self.significant {
            fields.extend([
                ("minimumSignificantDigits", Value::Number(min as f64)),
                ("maximumSignificantDigits", Value::Number(max as f64)),
            ]);
        }
        fields.extend([
            (
                "useGrouping",
                if self.grouping == "false" {
                    Value::Bool(false)
                } else {
                    Value::String(JsString::from(self.grouping))
                },
            ),
            ("notation", Value::String(JsString::from("standard"))),
            ("signDisplay", Value::String(JsString::from("auto"))),
            ("roundingIncrement", Value::Number(1.0)),
            ("roundingMode", Value::String(JsString::from(self.rounding))),
            ("roundingPriority", Value::String(JsString::from("auto"))),
            ("trailingZeroDisplay", Value::String(JsString::from("auto"))),
        ]);
        for (name, field) in fields {
            property::define_field(heap, task, &value, &JsString::from(name), field)?;
        }
        Ok(value)
    }
}

pub(crate) fn record(heap: &Heap, task: TaskId, value: &Value) -> Result<NumberFormat, Fault> {
    let Value::Object(id) = value else {
        return Err(Fault::TypeError("NumberFormat receiver required".into()));
    };
    heap.read(*id, |o| match o {
        HeapObject::Plain(o) => {
            if o.owner().is_some_and(|owner| owner != task) {
                return Err(Fault::ForeignObject {
                    owner: o.owner(),
                    caller: task,
                });
            }
            o.number_format()
                .cloned()
                .ok_or_else(|| Fault::TypeError("NumberFormat receiver required".into()))
        }
        _ => Err(Fault::TypeError("NumberFormat receiver required".into())),
    })
    .ok_or(Fault::Internal("NumberFormat reclaimed"))?
}

pub(crate) fn get_format(heap: &Heap, task: TaskId, this: &Value) -> Result<Value, Fault> {
    record(heap, task, this)?;
    let Value::Object(id) = this else {
        unreachable!()
    };
    if let Some(cached) = heap
        .read(*id, |o| match o {
            HeapObject::Plain(o) => o.number_format_bound().cloned(),
            _ => None,
        })
        .flatten()
    {
        return Ok(cached);
    }
    let realm = crate::realm::intrinsics(heap).ok_or(Fault::OutOfMemory("realm exhausted"))?;
    let mut metadata = OrdinaryObject::intrinsic(Some(realm.function_prototype));
    for (name, value) in [
        ("name", Value::String(JsString::from(""))),
        ("length", Value::Number(1.0)),
    ] {
        let key = heap
            .keys()
            .intern(&JsString::from(name))
            .ok_or(Fault::OutOfMemory("keys exhausted"))?;
        metadata.define_with(key, value, PropertyFlags::FUNCTION_INFO);
    }
    let metadata = heap
        .alloc(HeapObject::Plain(metadata))
        .ok_or(Fault::OutOfMemory("heap exhausted"))?;
    let bound = heap
        .alloc(HeapObject::BoundFunction(
            crate::vm::function::BoundFunction {
                target: Value::Object(realm.intl_number_format_bound),
                this: this.clone(),
                args: Box::new([]),
                owner: task,
                metadata,
                properties: None,
            },
        ))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("heap exhausted"))?;
    heap.with_object_mut(*id, |o| {
        if let HeapObject::Plain(o) = o {
            o.set_number_format_bound(bound.clone());
        }
    })
    .ok_or(Fault::Internal("NumberFormat reclaimed"))?;
    Ok(bound)
}

/// Positive ASCII coefficient and decimal point position. Rounding never
/// multiplies binary floats: carry and all nine rounding modes act on digits.
struct Decimal {
    digits: Vec<u8>,
    point: i32,
}
impl Decimal {
    fn parse(text: &str) -> Result<Option<Self>, Fault> {
        if text.is_empty() {
            return Ok(Some(Self {
                digits: vec![b'0'],
                point: 1,
            }));
        }
        let (mantissa, exponent) = text.split_once(['e', 'E']).unwrap_or((text, "0"));
        let Ok(exponent) = exponent.parse::<i32>() else {
            return Ok(None);
        };
        if exponent.unsigned_abs() > 10_000 || mantissa.len() > 100_000 {
            return Err(Fault::RangeError(
                "NumberFormat decimal input exceeds resource limit".into(),
            ));
        }
        let mut point = mantissa.find('.').unwrap_or(mantissa.len()) as i32 + exponent;
        let mut dot = false;
        let mut digits = Vec::new();
        for c in mantissa.bytes() {
            if c == b'.' && !dot {
                dot = true;
            } else if c.is_ascii_digit() {
                digits.push(c);
            } else {
                return Ok(None);
            }
        }
        if digits.is_empty() {
            return Ok(None);
        }
        let leading = digits
            .iter()
            .position(|&c| c != b'0')
            .unwrap_or(digits.len());
        if leading == digits.len() {
            return Ok(Some(Self {
                digits: vec![b'0'],
                point: 1,
            }));
        }
        digits.drain(..leading);
        point -= leading as i32;
        Ok(Some(Self { digits, point }))
    }
    fn round(&mut self, places: i32, mode: &str, negative: bool) {
        let keep = self.point + places;
        if keep >= self.digits.len() as i32 {
            return;
        }
        let at = keep.max(0) as usize;
        let nonzero = self.digits[at..].iter().any(|&c| c != b'0');
        let cmp = if keep < 0 {
            std::cmp::Ordering::Less
        } else {
            self.digits[at].cmp(&b'5').then_with(|| {
                if self.digits[at + 1..].iter().any(|&c| c != b'0') {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            })
        };
        let tie_up = match mode {
            "halfCeil" => !negative,
            "halfFloor" => negative,
            "halfTrunc" => false,
            "halfEven" => at > 0 && (self.digits[at - 1] - b'0') % 2 == 1,
            _ => true,
        };
        let up = nonzero
            && match mode {
                "ceil" => !negative,
                "floor" => negative,
                "expand" => true,
                "trunc" => false,
                _ => cmp.is_gt() || (cmp.is_eq() && tie_up),
            };
        self.digits.truncate(at);
        if up {
            let mut i = self.digits.len();
            while i > 0 && self.digits[i - 1] == b'9' {
                self.digits[i - 1] = b'0';
                i -= 1;
            }
            if i == 0 {
                self.digits.insert(0, b'1');
                self.point = if keep > 0 { self.point + 1 } else { 1 - places };
            } else {
                self.digits[i - 1] += 1;
            }
        }
        if self.digits.is_empty() || self.digits.iter().all(|&c| c == b'0') {
            self.digits = vec![b'0'];
            self.point = 1;
        }
    }
    fn parts(&self) -> (String, String) {
        let digits = std::str::from_utf8(&self.digits).unwrap();
        if self.point <= 0 {
            ("0".into(), "0".repeat((-self.point) as usize) + digits)
        } else if self.point as usize >= digits.len() {
            (
                digits.to_owned() + &"0".repeat(self.point as usize - digits.len()),
                String::new(),
            )
        } else {
            (
                digits[..self.point as usize].into(),
                digits[self.point as usize..].into(),
            )
        }
    }
}
