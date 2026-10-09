//! Bounded exact arithmetic; the compiler's JavaScript numbers are untouched.
use super::*;
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArithmeticError {
    DivideByZero,
    Overflow,
    ConversionRange,
    NonFiniteQuotient,
}
#[derive(Clone, Copy)]
pub(super) enum Operation {
    Add,
    Subtract,
    Multiply,
    Divide,
    Inc,
    Dec,
    Quot,
    Rem,
    Mod,
    Equal,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Zero,
    Positive,
    Negative,
    Even,
    Odd,
    Min,
    Max,
    Byte,
    Short,
    Int,
    Long,
    Float,
    Double,
    Char,
    Numerator,
    Denominator,
}
#[derive(Clone, Copy)]
enum Number {
    Exact(i64, i64),
    Double(f64),
}
fn number(value: &RootedValue, argument: usize) -> Result<Number, NativeError> {
    match value.inspect() {
        ValueView::Integer(n) => Ok(Number::Exact(n, 1)),
        ValueView::Ratio {
            numerator,
            denominator,
        } => Ok(Number::Exact(numerator, denominator)),
        ValueView::Double(n) => Ok(Number::Double(n)),
        _ => Err(NativeError::Type {
            argument,
            expected: "number",
        }),
    }
}
fn arithmetic(error: ArithmeticError) -> NativeError {
    NativeError::Arithmetic(error)
}
fn overflow() -> NativeError {
    arithmetic(ArithmeticError::Overflow)
}
impl Number {
    fn double(self) -> f64 {
        match self {
            Self::Double(n) => n,
            Self::Exact(n, 1) => n as f64,
            Self::Exact(n, d) => {
                // JVM Ratio.doubleValue uses DECIMAL64: 16 significant decimal
                // digits, ties to even. Round the bounded fraction exactly,
                // then use Rust's correctly rounded decimal-to-binary parser.
                let mut numerator = i128::from(n).abs();
                let mut denominator = i128::from(d);
                let mut exponent = 0;
                if numerator >= denominator {
                    while numerator >= denominator * 10 {
                        denominator *= 10;
                        exponent += 1;
                    }
                } else {
                    while numerator < denominator {
                        numerator *= 10;
                        exponent -= 1;
                    }
                }
                numerator *= 1_000_000_000_000_000;
                let mut coefficient = numerator / denominator;
                let remainder = numerator % denominator;
                if remainder * 2 > denominator
                    || (remainder * 2 == denominator && coefficient % 2 != 0)
                {
                    coefficient += 1;
                }
                if n < 0 {
                    coefficient = -coefficient;
                }
                format!("{coefficient}e{}", exponent - 15)
                    .parse()
                    .expect("bounded decimal coefficient")
            }
        }
    }
    fn compare(self, other: Self) -> Option<Ordering> {
        match (self, other) {
            (Self::Exact(a, b), Self::Exact(c, d)) => {
                (i128::from(a) * i128::from(d)).partial_cmp(&(i128::from(c) * i128::from(b)))
            }
            _ => self.double().partial_cmp(&other.double()),
        }
    }
}

/// Inputs to every binary exact operation are i64 components; their products
/// and sums fit i128. Reduce before narrowing, including sign normalization.
fn exact(context: &Context, mut n: i128, mut d: i128) -> NativeResult {
    if d == 0 {
        return Err(arithmetic(ArithmeticError::DivideByZero));
    }
    if d < 0 {
        n = n.checked_neg().ok_or_else(overflow)?;
        d = d.checked_neg().ok_or_else(overflow)?;
    }
    let (mut a, mut b) = (n.unsigned_abs(), d as u128);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    let gcd = i128::try_from(a).map_err(|_| overflow())?;
    n /= gcd;
    d /= gcd;
    let n = i64::try_from(n).map_err(|_| overflow())?;
    let d = i64::try_from(d).map_err(|_| overflow())?;
    Ok(context.owner.value(if d == 1 {
        Value::Integer(n)
    } else {
        Value::Ratio(n, d)
    }))
}

fn quotient(a: f64, b: f64) -> Result<f64, NativeError> {
    if b == 0.0 {
        return Err(arithmetic(ArithmeticError::DivideByZero));
    }
    let q = a / b;
    if !q.is_finite() {
        return Err(arithmetic(ArithmeticError::NonFiniteQuotient));
    }
    // Clojure uses a long when it fits, making a zero quotient positive. For
    // larger magnitudes the double is already integral at its precision.
    Ok(
        if (-9_223_372_036_854_775_808.0..=9_223_372_036_854_775_808.0).contains(&q) {
            (q as i64) as f64
        } else {
            q.trunc()
        },
    )
}
fn binary(context: &Context, left: Number, right: Number, operation: Operation) -> NativeResult {
    use Operation::*;
    if let (Number::Exact(a, b), Number::Exact(c, d)) = (left, right) {
        let (a, b, c, d) = (i128::from(a), i128::from(b), i128::from(c), i128::from(d));
        return match operation {
            Add => exact(
                context,
                (a * d).checked_add(c * b).ok_or_else(overflow)?,
                b * d,
            ),
            Subtract => exact(
                context,
                (a * d).checked_sub(c * b).ok_or_else(overflow)?,
                b * d,
            ),
            Multiply => exact(context, a * c, b * d),
            Divide => exact(context, a * d, b * c),
            Quot | Rem | Mod => {
                if c == 0 {
                    return Err(arithmetic(ArithmeticError::DivideByZero));
                }
                let n = a * d;
                let divisor = b * c;
                if matches!(operation, Quot) {
                    return exact(context, n / divisor, 1);
                }
                let mut remainder = n % divisor;
                if matches!(operation, Mod) && remainder != 0 && (remainder < 0) != (divisor < 0) {
                    remainder += divisor;
                }
                exact(context, remainder, b * d)
            }
            _ => unreachable!(),
        };
    }
    let (a, b) = (left.double(), right.double());
    let result = match operation {
        Add => a + b,
        Subtract => a - b,
        Multiply => a * b,
        Divide => {
            if b == 0.0 {
                return Err(arithmetic(ArithmeticError::DivideByZero));
            }
            a / b
        }
        Quot => quotient(a, b)?,
        Rem | Mod => {
            let q = quotient(a, b)?;
            let remainder = a - q * b;
            if matches!(operation, Mod) && remainder != 0.0 && ((a < 0.0) != (b < 0.0)) {
                remainder + b
            } else {
                remainder
            }
        }
        _ => unreachable!(),
    };
    Ok(context.double(result))
}

fn convert(context: &Context, value: &RootedValue, operation: Operation) -> NativeResult {
    use Operation::*;
    if matches!(operation, Numerator | Denominator) {
        let ValueView::Ratio {
            numerator,
            denominator,
        } = value.inspect()
        else {
            return Err(NativeError::Type {
                argument: 0,
                expected: "ratio",
            });
        };
        return Ok(context.integer(if matches!(operation, Numerator) {
            numerator
        } else {
            denominator
        }));
    }
    let number = if let ValueView::Character(c) = value.inspect() {
        if matches!(operation, Float | Double) {
            return Err(NativeError::Type {
                argument: 0,
                expected: "number",
            });
        }
        Number::Exact(i64::from(c), 1)
    } else {
        number(value, 0)?
    };
    if matches!(operation, Double | Float) {
        let n = number.double();
        if matches!(operation, Double) {
            return Ok(context.double(n));
        }
        let single = n as f32;
        if n < -f64::from(f32::MAX) || n > f64::from(f32::MAX) {
            return Err(arithmetic(ArithmeticError::ConversionRange));
        }
        return Ok(context.double(f64::from(single)));
    }
    let (min, max) = match operation {
        Byte => (i128::from(i8::MIN), i128::from(i8::MAX)),
        Short => (i128::from(i16::MIN), i128::from(i16::MAX)),
        Int => (i128::from(i32::MIN), i128::from(i32::MAX)),
        Long => (i128::from(i64::MIN), i128::from(i64::MAX)),
        Char => (0, i128::from(u16::MAX)),
        _ => unreachable!(),
    };
    let n = match number {
        Number::Exact(n, d) => i128::from(n) / i128::from(d),
        Number::Double(n) if n.is_nan() => 0,
        Number::Double(n) => {
            if !n.is_finite() || n.trunc() < min as f64 || n.trunc() > max as f64 {
                return Err(arithmetic(ArithmeticError::ConversionRange));
            }
            n.trunc() as i128
        }
    };
    if n < min || n > max {
        return Err(
            if matches!(operation, Int) && matches!(number, Number::Exact(_, _)) {
                overflow()
            } else {
                arithmetic(ArithmeticError::ConversionRange)
            },
        );
    }
    Ok(if matches!(operation, Char) {
        context.character(n as u16)
    } else {
        context.integer(n as i64)
    })
}

pub(super) fn call(
    context: &mut Context,
    args: &[RootedValue],
    operation: Operation,
) -> NativeResult {
    use Operation::*;
    match operation {
        Byte | Short | Int | Long | Float | Double | Char | Numerator | Denominator => {
            convert(context, &args[0], operation)
        }
        Add | Subtract | Multiply | Divide => {
            if args.is_empty() {
                return Ok(context.integer(if matches!(operation, Multiply) { 1 } else { 0 }));
            }
            if args.len() == 1 {
                return match operation {
                    Add | Multiply => {
                        number(&args[0], 0)?;
                        Ok(args[0].clone())
                    }
                    Subtract => match number(&args[0], 0)? {
                        Number::Exact(n, d) => exact(context, -i128::from(n), i128::from(d)),
                        Number::Double(n) => Ok(context.double(-n)),
                    },
                    Divide => binary(context, Number::Exact(1, 1), number(&args[0], 0)?, Divide),
                    _ => unreachable!(),
                };
            }
            let mut result = args[0].clone();
            for (index, argument) in args.iter().enumerate().skip(1) {
                result = binary(
                    context,
                    number(&result, index - 1)?,
                    number(argument, index)?,
                    operation,
                )?;
            }
            Ok(result)
        }
        Inc | Dec => binary(
            context,
            number(&args[0], 0)?,
            Number::Exact(1, 1),
            if matches!(operation, Inc) {
                Add
            } else {
                Subtract
            },
        ),
        Quot | Rem | Mod => binary(
            context,
            number(&args[0], 0)?,
            number(&args[1], 1)?,
            operation,
        ),
        Equal | Less | LessEqual | Greater | GreaterEqual => {
            for (index, pair) in args.windows(2).enumerate() {
                let cmp = number(&pair[0], index)?.compare(number(&pair[1], index + 1)?);
                let yes = match operation {
                    Equal => cmp == Some(Ordering::Equal),
                    Less => cmp == Some(Ordering::Less),
                    LessEqual => matches!(cmp, Some(Ordering::Less | Ordering::Equal)),
                    Greater => cmp == Some(Ordering::Greater),
                    GreaterEqual => matches!(cmp, Some(Ordering::Greater | Ordering::Equal)),
                    _ => unreachable!(),
                };
                if !yes {
                    return Ok(context.boolean(false));
                }
            }
            Ok(context.boolean(true))
        }
        Zero | Positive | Negative => {
            let cmp = number(&args[0], 0)?.compare(Number::Exact(0, 1));
            Ok(context.boolean(
                cmp == Some(match operation {
                    Zero => Ordering::Equal,
                    Positive => Ordering::Greater,
                    Negative => Ordering::Less,
                    _ => unreachable!(),
                }),
            ))
        }
        Even | Odd => {
            let ValueView::Integer(n) = args[0].inspect() else {
                return Err(NativeError::Type {
                    argument: 0,
                    expected: "integer",
                });
            };
            Ok(context.boolean((n % 2 == 0) == matches!(operation, Even)))
        }
        Min | Max => {
            let mut result = args[0].clone();
            for (index, argument) in args.iter().enumerate().skip(1) {
                // JVM min/max propagate NaN before checking the other type.
                if matches!(result.inspect(), ValueView::Double(n) if n.is_nan()) {
                    continue;
                }
                if matches!(argument.inspect(), ValueView::Double(n) if n.is_nan()) {
                    result = argument.clone();
                    continue;
                }
                let a = number(&result, index - 1)?;
                let b = number(argument, index)?;
                if a.compare(b)
                    != Some(if matches!(operation, Min) {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    })
                {
                    result = argument.clone();
                }
            }
            Ok(result)
        }
    }
}
