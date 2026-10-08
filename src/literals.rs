//! Literal spelling validation; numeric values and namespace resolution are deferred.

use std::ops::Range;

use crate::{
    char_classes,
    parser::{ErrorKind, NodeKind, NumberKind, StringValue},
};

fn numeric(ch: char) -> bool {
    ch.is_ascii_digit() || char_classes::unicode_digit(ch)
}

pub(crate) fn symbol(text: &str) -> bool {
    if text.is_empty() || text.ends_with(':') || text.starts_with("::") {
        return false;
    }
    if text == "/" {
        return true;
    }
    if let Some((namespace, name)) = text.split_once('/') {
        !namespace.is_empty()
            && !namespace.ends_with(':')
            && !name.is_empty()
            && !numeric(name.chars().next().unwrap())
            && (name == "/" || !name.contains('/'))
    } else {
        true
    }
}

pub(crate) fn atom(text: &str) -> Result<NodeKind, ErrorKind> {
    match text {
        "nil" => return Ok(NodeKind::Nil),
        "true" => return Ok(NodeKind::Boolean(true)),
        "false" => return Ok(NodeKind::Boolean(false)),
        _ => {}
    }
    if let Some(keyword) = text.strip_prefix(':') {
        return if symbol(keyword) {
            Ok(NodeKind::Keyword {
                auto_resolve: keyword.starts_with(':'),
            })
        } else {
            Err(ErrorKind::InvalidKeyword)
        };
    }
    let mut chars = text.chars();
    let first = chars.next().unwrap();
    if numeric(first) || (matches!(first, '+' | '-') && chars.next().is_some_and(numeric)) {
        return number(text)
            .map(NodeKind::Number)
            .ok_or(ErrorKind::InvalidNumber);
    }
    if symbol(text) {
        Ok(NodeKind::Symbol)
    } else {
        Err(ErrorKind::InvalidSymbol)
    }
}

fn digits(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit)
}

fn integer(text: &str) -> bool {
    let bytes = text.as_bytes();
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    if let Some((base, value)) = text.split_once(['r', 'R']) {
        if base.len() > 2 || base.starts_with('0') || !digits(base.as_bytes()) {
            return false;
        }
        let base = base.parse::<u32>().unwrap();
        return (2..=36).contains(&base)
            && !value.is_empty()
            && value.chars().all(|ch| ch.is_ascii() && ch.is_digit(base));
    }
    digits(bytes)
        && (bytes.len() == 1 || bytes[0] != b'0' || bytes.iter().all(|b| matches!(b, b'0'..=b'7')))
}

fn decimal(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    if i == 0 {
        return false;
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if start == i {
            return false;
        }
    }
    i == bytes.len()
}

fn number(text: &str) -> Option<NumberKind> {
    let text = text.strip_prefix(['+', '-']).unwrap_or(text);
    if let Some((numerator, denominator)) = text.split_once('/') {
        return (digits(numerator.as_bytes())
            && digits(denominator.as_bytes())
            && denominator.bytes().any(|b| b != b'0'))
        .then_some(NumberKind::Ratio);
    }
    // The reference radix pattern greedily treats N as a radix digit; unlike
    // decimal/hex/octal integers, 36r10N does not have a bigint suffix.
    let radix = text.contains(['r', 'R']);
    let big = !radix && text.ends_with('N');
    if integer(if big { &text[..text.len() - 1] } else { text }) {
        return Some(if big {
            NumberKind::BigInteger
        } else {
            NumberKind::Integer
        });
    }
    // Invalid all-digit integers (e.g. 09) don't fall through to float parsing.
    if digits(text.as_bytes()) {
        return None;
    }
    if let Some(value) = text.strip_suffix('M') {
        decimal(value).then_some(NumberKind::Decimal)
    } else {
        decimal(text).then_some(NumberKind::Float)
    }
}

// Character.digit(char, radix) additionally accepts BMP decimal digits and
// fullwidth Latin letters. This is only on the escaped-literal path.
fn digit(ch: char, radix: u32) -> Option<u32> {
    let value = if char_classes::unicode_digit(ch) {
        let mut zero = ch as u32;
        while char::from_u32(zero - 1).is_some_and(char_classes::unicode_digit) {
            zero -= 1;
        }
        ch as u32 - zero
    } else if ('\u{ff21}'..='\u{ff3a}').contains(&ch) {
        ch as u32 - 0xff21 + 10
    } else if ('\u{ff41}'..='\u{ff5a}').contains(&ch) {
        ch as u32 - 0xff41 + 10
    } else {
        ch.to_digit(36)?
    };
    (value < radix).then_some(value)
}

pub(crate) fn character(text: &str) -> Option<char> {
    match text {
        "newline" | "\r" | "\r\n" | "\r\x0c" => return Some('\n'),
        "space" => return Some(' '),
        "tab" => return Some('\t'),
        "backspace" => return Some('\u{8}'),
        "formfeed" => return Some('\u{c}'),
        "return" => return Some('\r'),
        _ => {}
    }
    let mut chars = text.chars();
    let first = chars.next()?;
    if chars.next().is_none() {
        return (first.len_utf16() == 1).then_some(first);
    }
    let (digits, radix, exact) = if let Some(value) = text.strip_prefix('u') {
        (value, 16, true)
    } else {
        (text.strip_prefix('o')?, 8, false)
    };
    let length = digits.chars().count();
    if (exact && length != 4) || (!exact && !(1..=3).contains(&length)) {
        return None;
    }
    let value = digits
        .chars()
        .try_fold(0, |value, ch| digit(ch, radix).map(|d| value * radix + d))?;
    if !exact && value > 255 {
        return None;
    }
    char::from_u32(value)
}

fn escape_terminator(ch: char) -> bool {
    (ch.is_ascii() && crate::CLASSES[ch as usize] & (crate::SPACE | crate::MACRO) != 0)
        || matches!(ch, '\u{1680}' | '\u{2000}'..='\u{2006}' | '\u{2008}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{205f}' | '\u{3000}')
}

pub(crate) fn string(source: &str, span: Range<usize>) -> Result<StringValue, Range<usize>> {
    let text = &source[span.clone()];
    if !text.as_bytes().iter().any(|b| matches!(b, b'\\' | b'\r')) {
        return Ok(StringValue::Source(span));
    }
    let mut value = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        if ch == '\r' {
            if chars
                .peek()
                .is_some_and(|(_, ch)| matches!(ch, '\n' | '\u{c}'))
            {
                chars.next();
            }
            value.push(b'\n' as u16);
        } else if ch != '\\' {
            value.extend_from_slice(ch.encode_utf16(&mut [0; 2]));
        } else {
            let error = || span.start + offset..span.start + offset + 1;
            let (_, escape) = chars.next().ok_or_else(error)?;
            let decoded = match escape {
                't' => b'\t' as u16,
                'r' => b'\r' as u16,
                'n' => b'\n' as u16,
                '\\' => b'\\' as u16,
                '"' => b'"' as u16,
                'b' => 8,
                'f' => 12,
                'u' => {
                    let mut n = 0;
                    for _ in 0..4 {
                        let (_, ch) = chars.next().ok_or_else(error)?;
                        n = n * 16 + digit(ch, 16).ok_or_else(error)? as u16;
                    }
                    n
                }
                ch if numeric(ch) => {
                    let mut n = digit(ch, 8).ok_or_else(error)? as u16;
                    for _ in 1..3 {
                        let Some(&(_, ch)) = chars.peek() else {
                            break;
                        };
                        if escape_terminator(ch) {
                            break;
                        }
                        chars.next();
                        n = n * 8 + digit(ch, 8).ok_or_else(error)? as u16;
                    }
                    if n > 255 {
                        return Err(error());
                    }
                    n
                }
                _ => return Err(error()),
            };
            value.push(decoded);
        }
    }
    Ok(StringValue::Decoded(value))
}
