//! Shared decimal digits and rounding for numeric formatting.

use crate::value::JsBigInt;
use crate::vm::dispatch::Fault;

/// Positive ASCII coefficient and decimal point position. Rounding never
/// multiplies binary floats: carry and all nine rounding modes act on digits.
pub(crate) struct Decimal {
    pub(crate) digits: Vec<u8>,
    pub(crate) point: i32,
}
impl Decimal {
    /// Exact positive binary64 coefficient and point, shared by the Number
    /// formatting methods. No shortest-decimal conversion or float scaling.
    /// Binary64 bounds this work to 1074 powers of five, 2547 coefficient bits
    /// and 767 decimal digits; no user-sized arithmetic runs in this leaf.
    pub(crate) fn from_number(number: f64) -> Self {
        assert!(number.is_finite() && number >= 0.0);
        if number == 0.0 {
            return Self {
                digits: vec![b'0'],
                point: 1,
            };
        }
        let bits = number.to_bits();
        let biased = ((bits >> 52) & 0x7ff) as i32;
        let mut significand =
            (bits & ((1u64 << 52) - 1)) | if biased == 0 { 0 } else { 1u64 << 52 };
        let mut exponent = biased.max(1) - 1023 - 52;
        // Cancel denominator powers of two before constructing the decimal.
        let cancel = (significand.trailing_zeros() as i32).min((-exponent).max(0));
        significand >>= cancel;
        exponent += cancel;
        let coefficient = JsBigInt::from(significand as i64);
        let (coefficient, scale) = if exponent >= 0 {
            (
                coefficient.checked_shl(&JsBigInt::from(i64::from(exponent))),
                0,
            )
        } else {
            // m / 2^k = (m * 5^k) / 10^k. Reuse the in-tree limbs.
            let scale = -exponent;
            let five = JsBigInt::from(5)
                .checked_pow(&JsBigInt::from(i64::from(scale)))
                .expect("binary64 power fits BigInt bounds");
            (coefficient.checked_mul(&five), scale)
        };
        let digits = coefficient
            .expect("binary64 coefficient fits BigInt bounds")
            .to_string()
            .into_bytes();
        let point = digits.len() as i32 - scale;
        Self { digits, point }
    }

    pub(crate) fn rounding_places(
        &self,
        min: usize,
        max: usize,
        significant: Option<(usize, usize)>,
    ) -> (i32, usize) {
        if let Some((min, max)) = significant {
            (
                max as i32 - self.point,
                (min as i32 - self.point).max(0) as usize,
            )
        } else {
            (max as i32, min)
        }
    }
    pub(crate) fn parse(text: &str) -> Result<Option<Self>, Fault> {
        if text.is_empty() {
            return Ok(Some(Self {
                digits: vec![b'0'],
                point: 1,
            }));
        }
        let (mantissa, exponent) = text.split_once(['e', 'E']).unwrap_or((text, "0"));
        // Validate before checking resource limits or allocating coefficient
        // storage: malformed numeric strings must format as NaN at any size.
        let mut dot = false;
        let mut has_digit = false;
        for c in mantissa.bytes() {
            if c == b'.' && !dot {
                dot = true;
            } else if c.is_ascii_digit() {
                has_digit = true;
            } else {
                return Ok(None);
            }
        }
        if !has_digit {
            return Ok(None);
        }
        let negative_exponent = exponent.starts_with('-');
        let magnitude = exponent.strip_prefix(['-', '+']).unwrap_or(exponent);
        // Check the entire syntax before detecting the resource limit, so a
        // malformed large exponent remains NaN. Saturate at the limit sentinel
        // rather than overflowing a machine integer or rejecting leading zeros.
        if magnitude.is_empty() || !magnitude.bytes().all(|c| c.is_ascii_digit()) {
            return Ok(None);
        }
        let magnitude = magnitude
            .bytes()
            .fold(0u32, |n, c| (n * 10 + u32::from(c - b'0')).min(10_001));
        if magnitude > 10_000 || mantissa.len() > 100_000 {
            return Err(Fault::RangeError(
                "NumberFormat decimal input exceeds resource limit".into(),
            ));
        }
        let exponent = if negative_exponent {
            -(magnitude as i32)
        } else {
            magnitude as i32
        };
        let mut point = mantissa.find('.').unwrap_or(mantissa.len()) as i32 + exponent;
        let mut digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
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
    pub(crate) fn round(&mut self, places: i32, mode: &str, negative: bool) {
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
    pub(crate) fn parts(&self) -> (String, String) {
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

/// Number.prototype.toFixed after ToIntegerOrInfinity and range validation.
pub(crate) fn to_fixed(number: f64, fraction_digits: usize) -> String {
    assert!(fraction_digits <= 100);
    if !number.is_finite() || number.abs() >= 1e21 {
        return crate::value::string::number_to_string(number);
    }
    let negative = number < 0.0;
    let mut decimal = Decimal::from_number(number.abs());
    decimal.round(fraction_digits as i32, "halfExpand", negative);
    let (integer, mut fraction) = decimal.parts();
    fraction.extend(std::iter::repeat_n('0', fraction_digits - fraction.len()));
    let mut result = if negative {
        "-".to_owned()
    } else {
        String::new()
    };
    result.push_str(&integer);
    if fraction_digits > 0 {
        result.push('.');
        result.push_str(&fraction);
    }
    result
}

#[cfg(test)]
mod tests {
    // Adapted QuickJS test cases:
    // Copyright (c) 2017-2021 Fabrice Bellard
    // Copyright (c) 2017-2021 Charlie Gordon
    //
    // Permission is hereby granted, free of charge, to any person obtaining a copy
    // of this software and associated documentation files (the "Software"), to deal
    // in the Software without restriction, including without limitation the rights
    // to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
    // copies of the Software, and to permit persons to whom the Software is
    // furnished to do so, subject to the following conditions:
    //
    // The above copyright notice and this permission notice shall be included in
    // all copies or substantial portions of the Software.
    //
    // THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
    // IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
    // FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
    // THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
    // LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
    // OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
    // THE SOFTWARE.
    use super::*;

    // Six upstream rows adapt QuickJS tests/test_builtin.js::test_number,
    // revision 7d1c13c5d8f8; full notice: src/intl/data/QUICKJS-LICENSE.
    #[test]
    fn fixed_formatter_exact_rounding() {
        for (number, places, expected) in [
            (1000000000000000128.0, 0, "1000000000000000128"),
            (1.125, 2, "1.13"),
            (-1.125, 2, "-1.13"),
            (0.5, 0, "1"),
            (-0.5, 0, "-1"),
            (-1e-10, 0, "-0"),
            (0.0, 2, "0.00"),
            (-0.0, 1, "0.0"),
            (1.005, 2, "1.00"),
            (1.25, 1, "1.3"),
            (-1.25, 1, "-1.3"),
            (2.5, 0, "3"),
            (123.456, 0, "123"),
            (1e21, 2, "1e+21"),
            (0.000001, 7, "0.0000010"),
            (2f64.powi(69), 0, "590295810358705651712"),
            (f64::NAN, 2, "NaN"),
            (f64::INFINITY, 2, "Infinity"),
            (f64::NEG_INFINITY, 2, "-Infinity"),
            (9.999, 2, "10.00"),
            (0.999, 2, "1.00"),
            (0.00999, 2, "0.01"),
            (0.005, 2, "0.01"),
        ] {
            assert_eq!(to_fixed(number, places), expected, "{number:?}, {places}");
        }
        assert_eq!(to_fixed(5e-324, 100), "0.".to_owned() + &"0".repeat(100));
        assert_eq!(to_fixed(-5e-324, 100), "-0.".to_owned() + &"0".repeat(100));
        assert_eq!(
            to_fixed(0.1, 100),
            "0.1000000000000000055511151231257827021181583404541015625000000000000000000000000000000000000000000000"
        );
        // The immediately adjacent binary64 values straddle an exact tie.
        assert_eq!(to_fixed(f64::from_bits(1.25f64.to_bits() - 1), 1), "1.2");
        assert_eq!(to_fixed(f64::from_bits(1.25f64.to_bits() + 1), 1), "1.3");
    }
}
