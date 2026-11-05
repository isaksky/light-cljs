//! Plural operands and finite locale rule families for the agreed test coverage.
//! Predicates follow Unicode CLDR release-48 plurals.xml / ordinals.xml;
//! Copyright © 1991-2025 Unicode, Inc. SPDX-License-Identifier: Unicode-3.0.
//! See docs/attribution.md. Standard notation has exponent operand e = 0.
use super::{
    number_format::{Decimal, NumberFormat},
    service,
};
use crate::{
    heap::{Heap, HeapObject},
    ids::TaskId,
    value::Value,
    vm::dispatch::Fault,
};

#[derive(Clone, Debug)]
pub(crate) struct PluralRules {
    pub digits: NumberFormat,
    pub ordinal: bool,
}

#[derive(Debug)]
struct Operands {
    n: f64,
    i: String,
    v: usize,
    // The covered families do not test w or f; preserve their exact decimal
    // representation along with t for extending coverage safely.
    #[allow(dead_code, reason = "operand reserved for additional CLDR families")]
    w: usize,
    #[allow(dead_code, reason = "operand reserved for additional CLDR families")]
    f: String,
    t: String,
    e: usize,
}
impl Operands {
    fn new(i: String, fraction: String) -> Self {
        let t = fraction.trim_end_matches('0').to_owned();
        Self {
            n: format!("{i}.{fraction}").parse().unwrap_or(f64::INFINITY),
            i,
            v: fraction.len(),
            w: t.len(),
            f: fraction,
            t,
            e: 0,
        }
    }
    fn modulo(&self, m: u32) -> u32 {
        self.i
            .bytes()
            .fold(0, |n, digit| (n * 10 + u32::from(digit - b'0')) % m)
    }
    fn category(&self, language: &str, ordinal: bool) -> &'static str {
        if ordinal {
            return match language {
                "en" if self.t.is_empty() => match (self.modulo(10), self.modulo(100)) {
                    (1, n) if n != 11 => "one",
                    (2, n) if n != 12 => "two",
                    (3, n) if n != 13 => "few",
                    _ => "other",
                },
                "fr" if self.n == 1.0 => "one",
                _ => "other",
            };
        }
        match language {
            "ja" | "zh" => "other",
            "ru" if self.v == 0 => match (self.modulo(10), self.modulo(100)) {
                (1, n) if n != 11 => "one",
                (2..=4, n) if !(12..=14).contains(&n) => "few",
                _ => "many",
            },
            "ru" => "other",
            "ar" => {
                let rem = self.modulo(100);
                if self.n == 0.0 {
                    "zero"
                } else if self.n == 1.0 {
                    "one"
                } else if self.n == 2.0 {
                    "two"
                } else if self.t.is_empty() && (3..=10).contains(&rem) {
                    "few"
                } else if self.t.is_empty() && (11..=99).contains(&rem) {
                    "many"
                } else {
                    "other"
                }
            }
            "fr" | "pt" if self.i == "0" || self.i == "1" => "one",
            "es" | "tr" if self.n == 1.0 => "one",
            "fr" | "pt" | "es"
                if (self.e == 0 && self.i != "0" && self.modulo(1_000_000) == 0 && self.v == 0)
                    || self.e > 5 =>
            {
                "many"
            }
            "en" | "de" if self.i == "1" && self.v == 0 => "one",
            _ => "other",
        }
    }
}
impl PluralRules {
    /// The pinned NumberFormat unit tables have only singular/other forms and
    /// select singular for a rounded mathematical value of one, regardless of
    /// visible zero padding. Reuse exact plural operands without a float roundtrip.
    pub fn unit_category(decimal: &Decimal) -> &'static str {
        let (integer, fraction) = decimal.parts();
        Operands::new(integer, fraction.trim_end_matches('0').into()).category("en", false)
    }
    pub fn select(&self, n: f64) -> Result<&'static str, Fault> {
        if !n.is_finite() {
            return Ok("other");
        }
        let text = crate::value::string::number_to_string(n.abs());
        let mut decimal = Decimal::parse(&text)?.ok_or(Fault::Internal("finite decimal"))?;
        let d = &self.digits;
        // Significant digit bounds use the coefficient's point, with the same
        // exact decimal rounding used by NumberFormat.
        let (places, _) = decimal.rounding_places(d.min_fraction, d.max_fraction, d.significant);
        decimal.round(places, d.rounding, n.is_sign_negative());
        let (_, minimum) = decimal.rounding_places(d.min_fraction, d.max_fraction, d.significant);
        let (integer, mut fraction) = decimal.parts();
        while fraction.ends_with('0') && fraction.len() > minimum {
            fraction.pop();
        }
        fraction.extend(std::iter::repeat_n(
            '0',
            minimum.saturating_sub(fraction.len()),
        ));
        let language = self.digits.locale.split('-').next().unwrap();
        Ok(Operands::new(integer, fraction).category(language, self.ordinal))
    }
    fn categories(&self) -> &'static [&'static str] {
        let language = self.digits.locale.split('-').next().unwrap();
        if self.ordinal {
            return match language {
                "en" => &["one", "two", "few", "other"],
                "fr" => &["one", "other"],
                _ => &["other"],
            };
        }
        match language {
            "ar" => &["zero", "one", "two", "few", "many", "other"],
            "ru" => &["one", "few", "many", "other"],
            "fr" | "pt" | "es" => &["one", "many", "other"],
            "ja" | "zh" => &["other"],
            _ => &["one", "other"],
        }
    }
    pub fn resolved(&self, heap: &Heap, task: TaskId) -> Result<Value, Fault> {
        let d = &self.digits;
        let string = service::fixed_option;
        let mut fields = vec![
            ("locale", service::text_option(heap, &d.locale)?),
            (
                "type",
                string(if self.ordinal { "ordinal" } else { "cardinal" }),
            ),
            ("minimumIntegerDigits", Value::Number(d.min_integer as f64)),
            (
                "minimumFractionDigits",
                Value::Number(d.min_fraction as f64),
            ),
            (
                "maximumFractionDigits",
                Value::Number(d.max_fraction as f64),
            ),
        ];
        if let Some((min, max)) = d.significant {
            fields.extend([
                ("minimumSignificantDigits", Value::Number(min as f64)),
                ("maximumSignificantDigits", Value::Number(max as f64)),
            ]);
        }
        let categories = crate::vm::property::new_array(heap, task)?;
        for name in self.categories() {
            crate::vm::property::append(heap, task, &categories, Some(string(name)))?;
        }
        fields.push(("pluralCategories", categories));
        fields.push(("roundingMode", string(d.rounding)));
        service::resolved_options(heap, task, fields)
    }
}
pub(crate) fn record(heap: &Heap, task: TaskId, value: &Value) -> Result<PluralRules, Fault> {
    let invalid = || Fault::TypeError("PluralRules receiver required".into());
    let Value::Object(id) = value else {
        return Err(invalid());
    };
    heap.read(*id, |o| match o {
        HeapObject::Plain(o) => {
            if o.owner().is_some_and(|owner| owner != task) {
                return Err(Fault::ForeignObject {
                    owner: o.owner(),
                    caller: task,
                });
            }
            o.plural_rules().cloned().ok_or_else(invalid)
        }
        _ => Err(invalid()),
    })
    .ok_or(Fault::Internal("PluralRules reclaimed"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plural_rule_operands_and_categories() {
        for (locale, n, expected) in [
            ("en", 1.0, "one"),
            ("en", 2.0, "other"),
            ("ru", 1.0, "one"),
            ("ru", 2.0, "few"),
            ("ru", 5.0, "many"),
            ("ru", 21.0, "one"),
            ("ru", 1.5, "other"),
            ("ja", 1.0, "other"),
        ] {
            let rules = PluralRules {
                digits: NumberFormat::new(locale.into(), crate::intl::lookup(locale).unwrap()),
                ordinal: false,
            };
            assert_eq!(rules.select(n).ok().unwrap(), expected);
        }
        assert_eq!(
            Operands::new("1".into(), "".into()).category("en", false),
            "one"
        );
        assert_eq!(
            Operands::new("1".into(), "0".into()).category("en", false),
            "other"
        );
        let op = Operands::new("21".into(), "2300".into());
        assert_eq!(
            (op.v, op.w, op.f.as_str(), op.t.as_str(), op.e),
            (4, 2, "2300", "23", 0)
        );
    }
}
