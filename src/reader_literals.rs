//! Compiler-only validation; native reader values never take a print/reparse path.

/// Reader identity precedes Number emission: integer suffixes and integral
/// ratios share a category, while floating and decimal spellings retain theirs.
pub(crate) fn number_key(text: &str, kind: crate::NumberKind) -> Option<(u8, String)> {
    use crate::NumberKind as N;
    match kind {
        N::Integer => Some((0, integer_key(text))),
        N::BigInteger => Some((0, integer_key(text.strip_suffix('N')?))),
        N::Ratio => {
            let (n, d) = text.split_once('/')?;
            integral_ratio_text(n, d).map(|n| (0, n))
        }
        N::Float => {
            let value = crate::analyzer::number(text, kind)?;
            Some((
                1,
                format!("{:x}", if value == 0.0 { 0 } else { value.to_bits() }),
            ))
        }
        N::Decimal => {
            let text = text.strip_suffix('M')?;
            let negative = text.starts_with('-');
            let unsigned = text.trim_start_matches(['+', '-']);
            let (mantissa, exponent) = unsigned.split_once(['e', 'E']).unwrap_or((unsigned, "0"));
            let fraction = mantissa.split_once('.').map_or(0, |(_, rest)| rest.len());
            let mut exponent = exponent.parse::<i64>().ok()?.checked_sub(fraction as i64)?;
            let mut digits = mantissa.replace('.', "").trim_start_matches('0').to_owned();
            if digits.is_empty() {
                return Some((2, "0".into()));
            }
            while digits.ends_with('0') {
                digits.pop();
                exponent = exponent.checked_add(1)?;
            }
            Some((
                2,
                format!("{}{digits}e{exponent}", if negative { "-" } else { "" }),
            ))
        }
    }
}

pub(crate) fn uuid(text: &str) -> Option<String> {
    // java.util.UUID/fromString accepts abbreviated groups and canonicalizes.
    let widths = [8, 4, 4, 4, 12];
    let parts = text.split('-').collect::<Vec<_>>();
    if parts.len() != 5 || text.len() > 36 {
        return None;
    }
    let mut canonical = Vec::new();
    for (part, width) in parts.into_iter().zip(widths) {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let n = u64::from_str_radix(part, 16).ok()?;
        let mask = (1u64 << (width * 4)) - 1;
        canonical.push(format!("{:0width$x}", n & mask));
    }
    Some(canonical.join("-"))
}

pub(crate) fn instant(text: &str) -> Option<i64> {
    // Gregorian civil days, including dates before the 1582 calendar transition.
    fn digits(text: &str, n: usize) -> Option<(i64, &str)> {
        let (head, tail) = (text.get(..n)?, text.get(n..)?);
        if !head.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some((head.parse().ok()?, tail))
    }
    let (year, mut tail) = digits(text, 4)?;
    let mut fields = [1, 1, 0, 0, 0];
    let mut supplied = 0;
    for (i, delimiter) in ['-', '-', 'T', ':', ':'].into_iter().enumerate() {
        if let Some(rest) = tail.strip_prefix(delimiter) {
            (fields[i], tail) = digits(rest, 2)?;
            supplied += 1;
        } else {
            break;
        }
    }
    let [month, day, hour, minute, second] = fields;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=12).contains(&month)
        || day < 1
        || day > days[(month - 1) as usize]
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let mut millis = 0;
    if let Some(rest) = tail.strip_prefix('.') {
        if supplied != 5 {
            return None;
        }
        let length = rest.bytes().take_while(u8::is_ascii_digit).count();
        if length == 0 {
            return None;
        }
        let fraction = &rest[..length];
        let mut first = fraction.chars().take(3).collect::<String>();
        while first.len() < 3 {
            first.push('0');
        }
        millis = first.parse::<i64>().ok()?;
        tail = &rest[length..];
    }
    let mut offset = 0;
    if tail == "Z" {
        tail = "";
    } else if tail.starts_with(['+', '-']) {
        let sign = if tail.starts_with('-') { -1 } else { 1 };
        let (h, rest) = digits(&tail[1..], 2)?;
        let (m, rest) = digits(rest.strip_prefix(':')?, 2)?;
        if h > 18 || m > 59 || (h == 18 && m != 0) {
            return None;
        }
        offset = sign * (h * 60 + m);
        tail = rest;
    }
    if !tail.is_empty() {
        return None;
    }
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(
        ((era * 146097 + doe - 719468) * 86400 + hour * 3600 + minute * 60 + second - offset * 60)
            * 1000
            + millis,
    )
}

pub(crate) fn integral_ratio(n: &str, d: &str) -> Option<f64> {
    integral_ratio_text(n, d)?.parse().ok()
}

pub(crate) fn integral_ratio_text(n: &str, d: &str) -> Option<String> {
    let digits = |s: &str| {
        s.trim_start_matches(['+', '-'])
            .trim_start_matches('0')
            .bytes()
            .map(|b| b - b'0')
            .collect::<Vec<_>>()
    };
    let divisor = digits(d);
    if divisor.is_empty() {
        return None;
    }
    let mut remainder: Vec<u8> = Vec::new();
    let mut quotient = String::new();
    for digit in digits(n) {
        remainder.push(digit);
        while remainder.first() == Some(&0) {
            remainder.remove(0);
        }
        let mut digit = 0;
        while remainder.len() > divisor.len()
            || (remainder.len() == divisor.len() && remainder >= divisor)
        {
            let mut borrow = 0i16;
            let offset = remainder.len() - divisor.len();
            for i in (0..remainder.len()).rev() {
                let mut value = remainder[i] as i16
                    - borrow
                    - if i >= offset {
                        divisor[i - offset] as i16
                    } else {
                        0
                    };
                borrow = i16::from(value < 0);
                if value < 0 {
                    value += 10;
                }
                remainder[i] = value as u8;
            }
            while remainder.first() == Some(&0) {
                remainder.remove(0);
            }
            digit += 1;
        }
        quotient.push(char::from(b'0' + digit));
    }
    if !remainder.is_empty() {
        return None;
    }
    let magnitude = quotient.trim_start_matches('0');
    if magnitude.is_empty() {
        return Some("0".into());
    }
    Some(if n.starts_with('-') {
        format!("-{magnitude}")
    } else {
        magnitude.into()
    })
}

// Canonical exact decimal identity for source-reader duplicate checks. Runtime
// Number rounding happens later, after distinct integer keys have been read.
pub(crate) fn integer_key(text: &str) -> String {
    let unsigned = text.trim_start_matches(['+', '-']);
    let (base, digits) = if let Some(s) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        (16, s)
    } else if let Some((base, digits)) = unsigned.split_once(['r', 'R']) {
        (base.parse().unwrap(), digits)
    } else if unsigned.starts_with('0') {
        (8, unsigned)
    } else {
        (10, unsigned)
    };
    let mut decimal = vec![0u32];
    for digit in digits.chars() {
        let mut carry = digit.to_digit(base).expect("validated integer");
        for part in &mut decimal {
            let value = *part * base + carry;
            *part = value % 10;
            carry = value / 10;
        }
        while carry != 0 {
            decimal.push(carry % 10);
            carry /= 10;
        }
    }
    while decimal.len() > 1 && decimal.last() == Some(&0) {
        decimal.pop();
    }
    let mut result = if text.starts_with('-') && decimal != [0] {
        "-".to_owned()
    } else {
        String::new()
    };
    result.extend(
        decimal
            .into_iter()
            .rev()
            .map(|n| char::from(b'0' + n as u8)),
    );
    result
}
