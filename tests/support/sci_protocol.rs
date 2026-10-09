//! Lossless, test-only SCI comparison values. This is transport, not a reader
//! or evaluator; production code must not depend on it.
use std::collections::HashSet;

pub const REVISION: &str = "5fe89056384418be50d232b4278344cb3a1a526b";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    /// IEEE-754 bits, including signed zero; NaNs use one canonical bit pattern.
    Float(u64),
    Ratio(i64, i64),
    /// Oracle-only categories explicitly excluded from the bounded Rust profile.
    BigInt(String),
    Decimal(String, i32),
    String(Vec<u16>),
    Char(u16),
    Symbol(Option<Vec<u16>>, Vec<u16>),
    Keyword(Option<Vec<u16>>, Vec<u16>),
    List(Vec<Value>),
    Vector(Vec<Value>),
    Seq(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Set(Vec<Value>),
}

fn utf16(units: &[u16]) -> String {
    let mut result = String::from("u");
    for unit in units {
        use std::fmt::Write;
        write!(result, "{unit:04x}").unwrap();
    }
    result
}

fn tagged(tag: &str, fields: impl IntoIterator<Item = String>) -> String {
    let mut result = format!("({tag}");
    for field in fields {
        result.push(' ');
        result.push_str(&field);
    }
    result.push(')');
    result
}

impl Value {
    pub fn wire(&self) -> String {
        match self {
            Self::Nil => "nil".into(),
            Self::Bool(x) => tagged("bool", [x.to_string()]),
            Self::Int(x) => tagged("int", [x.to_string()]),
            Self::Float(bits) => tagged("float", [format!("{bits:016x}")]),
            Self::Ratio(n, d) => tagged("ratio", [n.to_string(), d.to_string()]),
            Self::BigInt(n) => tagged("bigint", [n.clone()]),
            Self::Decimal(n, scale) => tagged("decimal", [n.clone(), scale.to_string()]),
            Self::String(units) => tagged("string", [utf16(units)]),
            Self::Char(unit) => tagged("char", [format!("{unit:04x}")]),
            Self::Symbol(ns, name) | Self::Keyword(ns, name) => tagged(
                if matches!(self, Self::Symbol(..)) {
                    "symbol"
                } else {
                    "keyword"
                },
                [ns.as_deref().map_or_else(|| "-".into(), utf16), utf16(name)],
            ),
            Self::List(xs) | Self::Vector(xs) | Self::Seq(xs) => tagged(
                match self {
                    Self::List(_) => "list",
                    Self::Vector(_) => "vector",
                    _ => "seq",
                },
                xs.iter().map(Self::wire),
            ),
            Self::Set(xs) => {
                let mut fields: Vec<_> = xs.iter().map(Self::wire).collect();
                fields.sort();
                tagged("set", fields)
            }
            Self::Map(xs) => {
                let mut fields: Vec<_> = xs
                    .iter()
                    .map(|(k, v)| tagged("entry", [k.wire(), v.wire()]))
                    .collect();
                fields.sort();
                tagged("map", fields)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Read,
    Analysis,
    Execution,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Reader,
    UnresolvedSymbol,
    InvalidForm,
    Arity,
    Arithmetic,
    Bounds,
    Type,
    ExceptionInfo,
    DuplicateKey,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Returned(Value),
    Error { phase: Phase, category: Category },
}

struct Reader<'a> {
    tokens: Vec<&'a str>,
    position: usize,
    values: usize,
}

type Result<T> = std::result::Result<T, String>;

impl<'a> Reader<'a> {
    fn new(text: &'a str) -> Result<Self> {
        if !text.is_ascii() || text.len() > 1_000_000 {
            return Err("protocol must be bounded ASCII".into());
        }
        let mut tokens = Vec::new();
        let mut start = 0;
        for (i, byte) in text.bytes().enumerate() {
            if matches!(byte, b'(' | b')' | b' ') {
                if start != i {
                    tokens.push(&text[start..i]);
                }
                if byte != b' ' {
                    tokens.push(&text[i..i + 1]);
                }
                start = i + 1;
            }
        }
        if start < text.len() {
            tokens.push(&text[start..]);
        }
        Ok(Self {
            tokens,
            position: 0,
            values: 0,
        })
    }

    fn take(&mut self) -> Result<&'a str> {
        let token = self
            .tokens
            .get(self.position)
            .copied()
            .ok_or("truncated protocol")?;
        self.position += 1;
        Ok(token)
    }

    fn expect(&mut self, expected: &str) -> Result<()> {
        let actual = self.take()?;
        if actual == expected {
            Ok(())
        } else {
            Err(format!("expected {expected}, got {actual}"))
        }
    }

    fn end(&self) -> Result<()> {
        if self.position == self.tokens.len() {
            Ok(())
        } else {
            Err("trailing protocol data".into())
        }
    }

    fn closing(&self) -> bool {
        self.tokens.get(self.position) == Some(&")")
    }

    fn integer(&mut self) -> Result<i64> {
        let text = self.take()?;
        decimal(text)?;
        text.parse().map_err(|_| "integer does not fit i64".into())
    }

    fn value(&mut self, depth: usize) -> Result<Value> {
        self.values += 1;
        if depth > 128 || self.values > 10000 {
            return Err("protocol nesting/item limit".into());
        }
        let first = self.take()?;
        if first == "nil" {
            return Ok(Value::Nil);
        }
        if first != "(" {
            return Err(format!("expected tagged value, got {first}"));
        }
        let tag = self.take()?;
        let value = match tag {
            "bool" => Value::Bool(match self.take()? {
                "true" => true,
                "false" => false,
                _ => return Err("invalid boolean".into()),
            }),
            "int" => Value::Int(self.integer()?),
            "float" => {
                let bits = hex(self.take()?, 16)?;
                if f64::from_bits(bits).is_nan() && bits != 0x7ff8000000000000 {
                    return Err("noncanonical NaN".into());
                }
                Value::Float(bits)
            }
            "ratio" => {
                let n = self.integer()?;
                let d = self.integer()?;
                let (mut a, mut b) = (n.unsigned_abs(), d.unsigned_abs());
                while b != 0 {
                    (a, b) = (b, a % b);
                }
                if d <= 1 || n == 0 || a != 1 {
                    return Err("ratio must be reduced with positive denominator > 1".into());
                }
                Value::Ratio(n, d)
            }
            "bigint" => Value::BigInt(decimal(self.take()?)?.into()),
            "decimal" => {
                let n = decimal(self.take()?)?.into();
                let scale = self
                    .integer()?
                    .try_into()
                    .map_err(|_| "decimal scale outside i32")?;
                Value::Decimal(n, scale)
            }
            "string" => Value::String(units(self.take()?)?),
            "char" => Value::Char(hex(self.take()?, 4)? as u16),
            "symbol" | "keyword" => {
                let namespace = self.take()?;
                let namespace = if namespace == "-" {
                    None
                } else {
                    Some(units(namespace)?)
                };
                let name = units(self.take()?)?;
                if name.is_empty() || namespace.as_ref().is_some_and(Vec::is_empty) {
                    return Err("empty symbol/keyword component".into());
                }
                if tag == "symbol" {
                    Value::Symbol(namespace, name)
                } else {
                    Value::Keyword(namespace, name)
                }
            }
            "list" | "vector" | "seq" | "set" => {
                let mut values = Vec::new();
                while !self.closing() {
                    values.push(self.value(depth + 1)?);
                }
                if tag == "set"
                    && values.iter().map(Value::wire).collect::<HashSet<_>>().len() != values.len()
                {
                    return Err("duplicate set value".into());
                }
                match tag {
                    "list" => Value::List(values),
                    "vector" => Value::Vector(values),
                    "seq" => Value::Seq(values),
                    _ => Value::Set(values),
                }
            }
            "map" => {
                let mut entries = Vec::new();
                let mut keys = HashSet::new();
                while !self.closing() {
                    self.expect("(")?;
                    self.expect("entry")?;
                    let key = self.value(depth + 1)?;
                    if !keys.insert(key.wire()) {
                        return Err("duplicate map key".into());
                    }
                    let value = self.value(depth + 1)?;
                    self.expect(")")?;
                    entries.push((key, value));
                }
                Value::Map(entries)
            }
            _ => return Err(format!("unknown value tag: {tag}")),
        };
        self.expect(")")?;
        Ok(value)
    }
}

fn decimal(text: &str) -> Result<&str> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty()
        || !digits.bytes().all(|b| b.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
        || text == "-0"
    {
        return Err("noncanonical decimal integer".into());
    }
    Ok(text)
}

fn hex(text: &str, size: usize) -> Result<u64> {
    if text.len() != size
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("invalid fixed-width hexadecimal".into());
    }
    u64::from_str_radix(text, 16).map_err(|_| "hexadecimal overflow".into())
}

fn units(text: &str) -> Result<Vec<u16>> {
    let text = text
        .strip_prefix('u')
        .ok_or("UTF-16 units require u prefix")?;
    if text.len() % 4 != 0 {
        return Err("incomplete UTF-16 unit".into());
    }
    (0..text.len())
        .step_by(4)
        .map(|i| hex(&text[i..i + 4], 4).map(|n| n as u16))
        .collect()
}

pub fn value(text: &str) -> Result<Value> {
    let mut reader = Reader::new(text)?;
    let value = reader.value(0)?;
    reader.end()?;
    if value.wire() != text {
        return Err("noncanonical value encoding/order".into());
    }
    Ok(value)
}

pub fn outcome(text: &str) -> Result<Outcome> {
    if let Some(value_text) = text
        .strip_prefix("(return ")
        .and_then(|s| s.strip_suffix(')'))
    {
        return value(value_text).map(Outcome::Returned);
    }
    let mut reader = Reader::new(text)?;
    reader.expect("(")?;
    reader.expect("error")?;
    let phase_text = reader.take()?;
    let category_text = reader.take()?;
    let phase = match phase_text {
        "read" => Phase::Read,
        "analysis" => Phase::Analysis,
        "execution" => Phase::Execution,
        _ => return Err("unknown error phase".into()),
    };
    let category = match (phase, category_text) {
        (Phase::Read, "reader") => Category::Reader,
        (Phase::Analysis, "unresolved-symbol") => Category::UnresolvedSymbol,
        (Phase::Analysis | Phase::Execution, "invalid-form") => Category::InvalidForm,
        (Phase::Execution, "arity") => Category::Arity,
        (Phase::Execution, "arithmetic") => Category::Arithmetic,
        (Phase::Execution, "bounds") => Category::Bounds,
        (Phase::Execution, "type") => Category::Type,
        (Phase::Execution, "exception-info") => Category::ExceptionInfo,
        (Phase::Execution, "duplicate-key") => Category::DuplicateKey,
        _ => return Err("unknown error category for phase".into()),
    };
    reader.expect(")")?;
    reader.end()?;
    if text != format!("(error {phase_text} {category_text})") {
        return Err("noncanonical error encoding".into());
    }
    Ok(Outcome::Error { phase, category })
}

#[derive(Debug)]
pub struct Case<'a> {
    pub id: &'a str,
    pub profile: &'a str,
    pub source: &'a str,
    pub outcome: Outcome,
    pub effects: Vec<Value>,
    pub stdout: Vec<u16>,
    pub stderr: Vec<u16>,
}

pub fn cases(text: &str) -> Result<Vec<Case<'_>>> {
    let mut ids = HashSet::new();
    let mut result = Vec::new();
    for row in text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let columns: Vec<_> = row.split('\t').collect();
        let [id, profile, source, returned, effects, stdout, stderr] = columns.as_slice() else {
            return Err("expected seven fixture fields".into());
        };
        if !id.starts_with("sci-")
            || !id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || !ids.insert(*id)
        {
            return Err("invalid/duplicate SCI case ID".into());
        }
        if !matches!(
            *profile,
            "state-validators"
                | "portable"
                | "bounded-number"
                | "reader-difference"
                | "reader-bb"
                | "reader-tags"
                | "reader-fallback"
                | "loading"
        ) || source.is_empty()
        {
            return Err("invalid profile/source".into());
        }
        let Value::Vector(effects) = value(effects)? else {
            return Err("effects must be a vector".into());
        };
        let Value::String(stdout) = value(stdout)? else {
            return Err("stdout must be UTF-16".into());
        };
        let Value::String(stderr) = value(stderr)? else {
            return Err("stderr must be UTF-16".into());
        };
        result.push(Case {
            id,
            profile,
            source,
            outcome: outcome(returned)?,
            effects,
            stdout,
            stderr,
        });
    }
    if result.is_empty() {
        return Err("missing SCI cases".into());
    }
    Ok(result)
}
