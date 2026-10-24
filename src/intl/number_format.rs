//! NumberFormat slots and exact decimal arithmetic. Numbers enter via
//! the shared Number::toString helper, as QuickJS's Intl mathematical values do.
use super::{LocaleData, data::numbers, table_string};
use crate::heap::{Heap, HeapObject};
use crate::ids::TaskId;
pub(super) use crate::number::Decimal;
use crate::object::{OrdinaryObject, PropertyFlags};
use crate::value::{JsString, Value};
use crate::vm::dispatch::{self, Fault};

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
    pub numbering: &'static str,
    pub style: &'static str,
    pub currency: Option<String>,
    pub currency_display: &'static str,
    pub currency_sign: &'static str,
}
impl NumberFormat {
    pub fn new(locale: String, data: &'static LocaleData) -> Self {
        let numbering = super::tag::LanguageTag::parse(&locale)
            .ok()
            .and_then(|tag| tag.keywords.get("nu").and_then(|nu| numbering_system(nu)))
            .unwrap_or("latn");
        Self {
            locale,
            data,
            grouping: "auto",
            min_integer: 1,
            min_fraction: 0,
            max_fraction: 3,
            significant: None,
            rounding: "halfExpand",
            numbering,
            style: "decimal",
            currency: None,
            currency_display: "symbol",
            currency_sign: "standard",
        }
    }
    pub fn format(&self, value: &Value) -> Result<Value, Fault> {
        let text = match value {
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
        let text = text.trim_matches(|c: char| {
            u16::try_from(u32::from(c)).is_ok_and(crate::value::string::is_str_white_space)
        });
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
                    // Scale the mathematical decimal value, including BigInts,
                    // before rounding. Zero must keep its canonical point.
                    if self.style == "percent" && n.digits != [b'0'] {
                        n.point += 2;
                    }
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
        // QuickJS handles nonfinite values before applying style patterns.
        let result = if unsigned == "Infinity" && negative {
            format!(
                "{}{body}",
                symbol(numbers::minusSign_tab, self.data.symbols.minusSign_idx)
            )
        } else if unsigned == "Infinity" {
            body
        } else {
            let p = &self.data.patterns;
            let (positive, minus) = match (self.style, self.currency_sign) {
                ("percent", _) => (p.pct_pos, p.pct_neg),
                ("currency", "accounting") => (p.acc_pos, p.acc_neg),
                ("currency", _) => (p.cur_pos, p.cur_neg),
                _ => (p.dec_pos, p.dec_neg),
            };
            let pattern = symbol(
                numbers::pattern_tab,
                if negative { minus } else { positive },
            );
            let currency = self.currency.as_deref().unwrap_or("");
            let currency = if self.style != "currency" || self.currency_display == "code" {
                currency
            } else {
                currency_symbol(self.data, currency).unwrap_or(currency)
            };
            // Tokens come only from the pinned patterns. Append replacements
            // once, so user currency text cannot be interpreted as a pattern.
            let mut result = String::new();
            let mut rest = pattern;
            while let Some(start) = rest.find('{') {
                result.push_str(&rest[..start]);
                let (token, tail) = rest[start..].split_once('}').expect("pinned pattern token");
                result.push_str(match token {
                    "{number" => &body,
                    "{minusSign" => symbol(numbers::minusSign_tab, self.data.symbols.minusSign_idx),
                    "{percentSign" => {
                        symbol(numbers::percentSign_tab, self.data.symbols.percentSign_idx)
                    }
                    "{currency" => currency,
                    _ => unreachable!("pinned number pattern"),
                });
                rest = tail;
            }
            result.push_str(rest);
            result
        };
        let result = map_digits(self.numbering, &result);
        Ok(Value::String(JsString::from(result.as_str())))
    }
    pub fn resolved(&self, heap: &Heap, task: TaskId) -> Result<Value, Fault> {
        let mut fields = vec![
            (
                "locale",
                Value::String(JsString::from(self.locale.as_str())),
            ),
            (
                "numberingSystem",
                Value::String(JsString::from(self.numbering)),
            ),
            ("style", Value::String(JsString::from(self.style))),
        ];
        if self.style == "currency" {
            fields.extend([
                (
                    "currency",
                    Value::String(JsString::from(self.currency.as_deref().unwrap())),
                ),
                (
                    "currencyDisplay",
                    Value::String(JsString::from(self.currency_display)),
                ),
                (
                    "currencySign",
                    Value::String(JsString::from(self.currency_sign)),
                ),
            ]);
        }
        fields.push((
            "minimumIntegerDigits",
            Value::Number(self.min_integer as f64),
        ));
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
        super::service::resolved_options(heap, task, fields)
    }
}

/// Minor units follow the pinned QuickJS snapshot (including its CLF=3).
pub(crate) fn currency_digits(code: &str) -> usize {
    match code {
        "JPY" | "KRW" | "VND" | "ISK" | "CLP" | "KMF" | "GNF" | "PYG" | "RWF" => 0,
        "CLF" | "BHD" | "JOD" | "KWD" | "OMR" | "TND" => 3,
        _ => 2,
    }
}

/// Decode the snapshot's unsigned LEB128 indices with checked bounds. There
/// are two variable-length indices per currency record, not two bytes.
fn currency_index(bytes: &mut &[u8]) -> Option<usize> {
    let mut value = 0u32;
    for shift in (0..=28).step_by(7) {
        let (&byte, tail) = bytes.split_first()?;
        *bytes = tail;
        if shift == 28 && byte > 15 {
            return None;
        }
        value |= u32::from(byte & 127) << shift;
        if byte & 128 == 0 {
            return usize::try_from(value).ok();
        }
    }
    None
}

fn currency_symbol(data: &LocaleData, code: &str) -> Option<&'static str> {
    let mut bytes = numbers::locale_currency_data.get(data.currency_offset as usize..)?;
    for _ in 0..data.num_currencies {
        let code_index = currency_index(&mut bytes)?;
        let symbol_index = currency_index(&mut bytes)?;
        if table_string(numbers::currency_code_tab, code_index)? == code {
            return table_string(numbers::currency_sym_tab, symbol_index);
        }
    }
    None
}

pub(crate) fn numbering_system(name: &str) -> Option<&'static str> {
    numbers::intl_all_numbering_systems
        .iter()
        .filter_map(|pool| super::pool_string((*pool)?, 0))
        .find(|&system| system == name)
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

/// Shared nf_put_digit mapping for numeric Intl fields.
pub(crate) fn map_digits(numbering: &str, text: &str) -> String {
    let index = numbers::intl_all_numbering_systems
        .iter()
        .position(|bytes| bytes.and_then(|pool| super::pool_string(pool, 0)) == Some(numbering))
        .expect("resolved numbering system");
    let base = numbers::nf_digit_base[index];
    text.chars()
        .map(|c| {
            if !c.is_ascii_digit() {
                return c;
            }
            let digit = c as u32 - '0' as u32;
            if numbering == "hanidec" {
                char::from_u32(numbers::hanidec_cps[digit as usize]).unwrap()
            } else if base != 0 {
                char::from_u32(base + digit).unwrap()
            } else {
                c
            } // QuickJS algorithmic systems retain Latin digits here.
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn currency_indices_and_snapshot_records() {
        for (bytes, expected) in [
            (&[0][..], Some(0)),
            (&[127][..], Some(127)),
            (&[128, 1][..], Some(128)),
            (&[255, 255, 255, 255, 15][..], Some(u32::MAX as usize)),
            (&[][..], None),
            (&[128][..], None),
            (&[255, 255, 255, 255, 16][..], None),
            (&[128, 128, 128, 128, 128, 0][..], None),
        ] {
            let mut input = bytes;
            assert_eq!(currency_index(&mut input), expected);
        }
        // Validate every selected locale's complete variable-length record,
        // including indices greater than 127, against the shared string pools.
        let mut multibyte = false;
        for data in super::super::data::locale::locale_data_table {
            let mut bytes = &numbers::locale_currency_data[data.currency_offset as usize..];
            for _ in 0..data.num_currencies {
                let code = currency_index(&mut bytes).unwrap();
                let symbol = currency_index(&mut bytes).unwrap();
                multibyte |= code > 127 || symbol > 127;
                let code = table_string(numbers::currency_code_tab, code).unwrap();
                let symbol = table_string(numbers::currency_sym_tab, symbol).unwrap();
                assert_eq!(currency_symbol(data, code), Some(symbol), "{code}");
            }
            assert_eq!(currency_symbol(data, "XYZ"), None);
        }
        assert!(multibyte);
        assert_eq!(
            currency_symbol(super::super::lookup("ja").unwrap(), "JPY"),
            Some("￥")
        );
    }
}
