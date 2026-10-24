//! Immutable BigInt primitives. Small values are inline; large sign/magnitude
//! bodies are shared leaves with no traced heap edges. All construction passes
//! through normalization, so each mathematical value has exactly one form.
//! In-tree arithmetic uses little-endian base-2^32 limbs; no external crate.

use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

/// Maximum magnitude size, matching the runtime's BigInt policy (1 Mbit).
pub const MAX_BIGINT_BITS: usize = 1 << 20;

/// A malformed digit sequence or a magnitude exceeding the size cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BigIntError {
    InvalidDigits,
    TooLarge,
}

impl fmt::Display for BigIntError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidDigits => "invalid BigInt digits",
            Self::TooLarge => "BigInt is too large",
        })
    }
}

impl std::error::Error for BigIntError {}

/// An exact immutable integer primitive. Equality and hashing compare value.
/// Cloning a large value shares its immutable, heap-edge-free limb body.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JsBigInt(Repr);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Repr {
    Small(i64),
    Large { negative: bool, limbs: Arc<[u32]> },
}

impl From<i64> for JsBigInt {
    fn from(value: i64) -> Self {
        Self(Repr::Small(value))
    }
}

impl Ord for JsBigInt {
    fn cmp(&self, other: &Self) -> Ordering {
        if let (Repr::Small(a), Repr::Small(b)) = (&self.0, &other.0) {
            return a.cmp(b);
        }
        let (mut a, mut b) = ([0; 2], [0; 2]);
        let (negative_a, a) = self.magnitude(&mut a);
        let (negative_b, b) = other.magnitude(&mut b);
        match negative_b.cmp(&negative_a) {
            Ordering::Equal => {
                let order = a
                    .len()
                    .cmp(&b.len())
                    .then_with(|| a.iter().rev().cmp(b.iter().rev()));
                if negative_a { order.reverse() } else { order }
            }
            order => order,
        }
    }
}

impl PartialOrd for JsBigInt {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl JsBigInt {
    /// Exact addition, checked against the magnitude cap.
    pub fn checked_add(&self, other: &Self) -> Result<Self, BigIntError> {
        self.add_signed(other, false)
    }

    /// Exact subtraction; opposite signs never need an oversized temporary.
    pub fn checked_sub(&self, other: &Self) -> Result<Self, BigIntError> {
        self.add_signed(other, true)
    }

    fn add_signed(&self, other: &Self, subtract: bool) -> Result<Self, BigIntError> {
        let (mut small_a, mut small_b) = ([0; 2], [0; 2]);
        let (negative_a, mut a) = self.magnitude(&mut small_a);
        let (negative_b, mut b) = other.magnitude(&mut small_b);
        let negative_b = negative_b ^ subtract;
        let mut negative = negative_a;
        let mut out = Vec::with_capacity(a.len().max(b.len()));
        if negative_a == negative_b {
            let mut carry = 0u64;
            for i in 0..a.len().max(b.len()) {
                let sum = a.get(i).copied().unwrap_or(0) as u64
                    + b.get(i).copied().unwrap_or(0) as u64
                    + carry;
                out.push(sum as u32);
                carry = sum >> 32;
            }
            if carry != 0 {
                if out.len() == MAX_BIGINT_BITS / 32 {
                    return Err(BigIntError::TooLarge);
                }
                out.push(carry as u32);
            }
        } else {
            if a.len()
                .cmp(&b.len())
                .then_with(|| a.iter().rev().cmp(b.iter().rev()))
                .is_lt()
            {
                std::mem::swap(&mut a, &mut b);
                negative = negative_b;
            }
            let mut borrow = false;
            for (i, &limb) in a.iter().enumerate() {
                let (difference, first) = limb.overflowing_sub(b.get(i).copied().unwrap_or(0));
                let (difference, second) = difference.overflowing_sub(u32::from(borrow));
                out.push(difference);
                borrow = first || second;
            }
            debug_assert!(!borrow);
        }
        Self::from_sign_magnitude(negative, out)
    }

    /// Exact product. Each base-2^32 multiply/add fits u64. Reject products
    /// known to exceed the cap before allocating or doing quadratic work;
    /// carry propagation never stores a magnitude beyond the cap either.
    pub fn checked_mul(&self, other: &Self) -> Result<Self, BigIntError> {
        let (mut small_a, mut small_b) = ([0; 2], [0; 2]);
        let (negative_a, a) = self.magnitude(&mut small_a);
        let (negative_b, b) = other.magnitude(&mut small_b);
        if a.is_empty() || b.is_empty() {
            return Ok(Self::from(0));
        }
        let bits = |limbs: &[u32]| {
            (limbs.len() - 1) * 32 + 32 - limbs.last().unwrap().leading_zeros() as usize
        };
        if bits(a) + bits(b) - 1 > MAX_BIGINT_BITS {
            return Err(BigIntError::TooLarge);
        }
        let mut out = vec![0u32; (a.len() + b.len()).min(MAX_BIGINT_BITS / 32)];
        for (i, &x) in a.iter().enumerate() {
            if x == 0 {
                continue;
            }
            let mut carry = 0u64;
            for (j, &y) in b.iter().enumerate() {
                let product = x as u64 * y as u64 + carry;
                if let Some(limb) = out.get_mut(i + j) {
                    let sum = product + *limb as u64;
                    *limb = sum as u32;
                    carry = sum >> 32;
                } else if product != 0 {
                    return Err(BigIntError::TooLarge);
                }
            }
            if carry != 0 {
                let limb = out.get_mut(i + b.len()).ok_or(BigIntError::TooLarge)?;
                *limb = carry as u32;
            }
        }
        Self::from_sign_magnitude(negative_a ^ negative_b, out)
    }

    /// Negation has the same magnitude, with canonical zero and i64 edges.
    pub fn negated(&self) -> Self {
        match &self.0 {
            Repr::Small(n) if *n != i64::MIN => Self::from(-n),
            Repr::Large { negative, limbs } if limbs.len() > 2 => Self(Repr::Large {
                negative: !negative,
                limbs: limbs.clone(),
            }),
            _ => {
                let (negative, limbs) = self.to_sign_magnitude();
                Self::from_sign_magnitude(!negative, limbs).expect("negation preserves magnitude")
            }
        }
    }

    /// Borrow canonical limbs, using caller-owned scratch space for inline values.
    fn magnitude<'a>(&'a self, small: &'a mut [u32; 2]) -> (bool, &'a [u32]) {
        match &self.0 {
            Repr::Small(value) => {
                let magnitude = value.unsigned_abs();
                *small = [magnitude as u32, (magnitude >> 32) as u32];
                let len = if small[1] != 0 {
                    2
                } else {
                    usize::from(small[0] != 0)
                };
                (*value < 0, &small[..len])
            }
            Repr::Large { negative, limbs } => (*negative, limbs),
        }
    }

    /// Compare by exact mathematical value, without rounding this integer to
    /// f64. NaN is unordered; infinities lie beyond every finite integer.
    pub fn cmp_number(&self, number: f64) -> Option<Ordering> {
        if number.is_nan() {
            return None;
        }
        if number.is_infinite() {
            return Some(if number.is_sign_negative() {
                Ordering::Greater
            } else {
                Ordering::Less
            });
        }
        // Decode |number| = significand * 2^exponent, including subnormals.
        // The integer part needs at most 1024 bits, far below our BigInt cap.
        let bits = number.to_bits();
        let biased_exponent = ((bits >> 52) & 0x7ff) as i32;
        let mut significand = bits & ((1u64 << 52) - 1);
        if biased_exponent != 0 {
            significand |= 1u64 << 52;
        }
        let exponent = biased_exponent.max(1) - 1023 - 52;
        let (limbs, fractional) = if exponent < 0 {
            let shift = (-exponent) as u32;
            let integer = significand.checked_shr(shift).unwrap_or(0);
            (
                vec![integer as u32, (integer >> 32) as u32],
                if shift >= 64 {
                    significand != 0
                } else {
                    significand & ((1u64 << shift) - 1) != 0
                },
            )
        } else {
            let words = exponent as usize / 32;
            let shift = exponent as u32 % 32;
            let mut limbs = vec![0; words + 3];
            for (i, limb) in [significand as u32, (significand >> 32) as u32]
                .into_iter()
                .enumerate()
            {
                let shifted = (limb as u64) << shift;
                limbs[words + i] |= shifted as u32;
                limbs[words + i + 1] |= (shifted >> 32) as u32;
            }
            (limbs, false)
        };
        let integer = Self::from_sign_magnitude(number.is_sign_negative(), limbs)
            .expect("finite f64 integer part fits the BigInt cap");
        let order = self.cmp(&integer);
        Some(if order.is_eq() && fractional {
            if number.is_sign_negative() {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        } else {
            order
        })
    }

    /// Construct from little-endian unsigned limbs and a sign. Leading zero
    /// limbs and negative zero are normalized; values fitting i64 are inline.
    /// Every later arithmetic operation must use this checked boundary.
    pub fn from_sign_magnitude(negative: bool, mut limbs: Vec<u32>) -> Result<Self, BigIntError> {
        while limbs.last() == Some(&0) {
            limbs.pop();
        }
        let bits = limbs.last().map_or(0, |last| {
            (limbs.len() - 1) * 32 + (32 - last.leading_zeros() as usize)
        });
        if bits > MAX_BIGINT_BITS {
            return Err(BigIntError::TooLarge);
        }
        if limbs.len() <= 2 {
            let magnitude = limbs.first().copied().unwrap_or(0) as u64
                | ((limbs.get(1).copied().unwrap_or(0) as u64) << 32);
            if negative && magnitude == 1u64 << 63 {
                return Ok(Self::from(i64::MIN));
            }
            if let Ok(small) = i64::try_from(magnitude) {
                return Ok(Self::from(if negative { -small } else { small }));
            }
        }
        Ok(Self(Repr::Large {
            negative,
            limbs: limbs.into(),
        }))
    }

    /// Read the sign and little-endian magnitude. Zero has no limbs and is
    /// never negative. The returned buffer belongs to the caller.
    pub fn to_sign_magnitude(&self) -> (bool, Vec<u32>) {
        match &self.0 {
            Repr::Small(value) => {
                let magnitude = value.unsigned_abs();
                let mut limbs = Vec::new();
                if magnitude != 0 {
                    limbs.push(magnitude as u32);
                    if magnitude >> 32 != 0 {
                        limbs.push((magnitude >> 32) as u32);
                    }
                }
                (*value < 0, limbs)
            }
            Repr::Large { negative, limbs } => (*negative, limbs.to_vec()),
        }
    }

    /// Parse unsigned ASCII digits in radix 2, 8, 10 or 16, without a prefix
    /// or `n` suffix. This is an internal-value constructor, not StringToBigInt.
    pub fn from_digits(digits: &str, radix: u32) -> Result<Self, BigIntError> {
        if !matches!(radix, 2 | 8 | 10 | 16) || digits.is_empty() {
            return Err(BigIntError::InvalidDigits);
        }
        let mut limbs = Vec::<u32>::new();
        if radix != 10 {
            // Pack power-of-two digits directly, so even cap-sized binary
            // literals take linear time and never allocate beyond the cap.
            let width = radix.trailing_zeros() as usize;
            let mut bit = 0usize;
            for byte in digits.bytes().rev() {
                let digit = (byte as char)
                    .to_digit(radix)
                    .ok_or(BigIntError::InvalidDigits)?;
                if digit != 0 {
                    let bits = bit + (32 - digit.leading_zeros() as usize);
                    if bits > MAX_BIGINT_BITS {
                        return Err(BigIntError::TooLarge);
                    }
                    limbs.resize(limbs.len().max(bits.div_ceil(32)), 0);
                    limbs[bit / 32] |= digit << (bit % 32);
                    if bit % 32 + width > 32 && bit / 32 + 1 < limbs.len() {
                        limbs[bit / 32 + 1] |= digit >> (32 - bit % 32);
                    }
                }
                bit = bit.checked_add(width).ok_or(BigIntError::TooLarge)?;
            }
        } else {
            // Multiply/add in groups of nine decimal digits. Each temporary
            // limb fits u64, including the carry (base <= 10^9).
            for chunk in digits.as_bytes().chunks(9) {
                let mut value = 0u32;
                let mut base = 1u32;
                for &byte in chunk {
                    if !byte.is_ascii_digit() {
                        return Err(BigIntError::InvalidDigits);
                    }
                    value = value * 10 + (byte - b'0') as u32;
                    base *= 10;
                }
                let mut carry = value as u64;
                for limb in &mut limbs {
                    let product = *limb as u64 * base as u64 + carry;
                    *limb = product as u32;
                    carry = product >> 32;
                }
                if carry != 0 {
                    limbs.push(carry as u32);
                }
                if limbs.len() > MAX_BIGINT_BITS / 32 {
                    return Err(BigIntError::TooLarge);
                }
            }
        }
        Self::from_sign_magnitude(false, limbs)
    }

    /// Whether this is the unique zero value (ToBoolean is its negation).
    pub fn is_zero(&self) -> bool {
        matches!(self.0, Repr::Small(0))
    }
}

impl fmt::Display for JsBigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Repr::Small(value) = self.0 {
            return write!(f, "{value}");
        }
        let (negative, mut limbs) = self.to_sign_magnitude();
        let mut chunks = Vec::new();
        while !limbs.is_empty() {
            let mut remainder = 0u64;
            for limb in limbs.iter_mut().rev() {
                let value = (remainder << 32) | *limb as u64;
                *limb = (value / 1_000_000_000) as u32;
                remainder = value % 1_000_000_000;
            }
            chunks.push(remainder as u32);
            while limbs.last() == Some(&0) {
                limbs.pop();
            }
        }
        if negative {
            f.write_str("-")?;
        }
        write!(f, "{}", chunks.pop().expect("nonzero large magnitude"))?;
        for chunk in chunks.iter().rev() {
            write!(f, "{chunk:09}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::{DefaultHasher, Hash, Hasher};

    #[test]
    fn canonical_boundaries_and_radices() {
        for value in [i64::MIN, -1, 0, 1, i64::MAX] {
            let bigint = JsBigInt::from(value);
            let (negative, mut limbs) = bigint.to_sign_magnitude();
            limbs.push(0);
            assert_eq!(
                JsBigInt::from_sign_magnitude(negative, limbs).unwrap(),
                bigint
            );
            assert_eq!(bigint.to_string(), value.to_string());
        }
        let decimal = JsBigInt::from_digits("18446744073709551615", 10).unwrap();
        let hex = JsBigInt::from_digits("ffffffffffffffff", 16).unwrap();
        assert_eq!(decimal, hex);
        assert_eq!(decimal.to_string(), "18446744073709551615");
        let hash = |v: &JsBigInt| {
            let mut h = DefaultHasher::new();
            v.hash(&mut h);
            h.finish()
        };
        assert_eq!(hash(&hex), hash(&decimal));
        let negative = JsBigInt::from_sign_magnitude(true, vec![0, 0x80000000, 1]).unwrap();
        assert_eq!(negative.to_string(), "-27670116110564327424");
        assert_eq!(JsBigInt::from_digits("0", 10).unwrap(), JsBigInt::from(0));
        for radix in [2, 8, 16] {
            let digits = match radix {
                2 => "1".repeat(97),
                8 => "1".repeat(33),
                _ => "1".repeat(25),
            };
            let n = JsBigInt::from_digits(&digits, radix).unwrap();
            assert_eq!(JsBigInt::from_digits(&n.to_string(), 10).unwrap(), n);
        }
        // Independent u128 expectations exercise nonzero octal spill bits
        // across each of the first three 32-bit limb boundaries.
        for digits in [11, 22, 32, 42] {
            let octal = "7".repeat(digits);
            let expected = ((1u128 << (digits * 3)) - 1).to_string();
            assert_eq!(
                JsBigInt::from_digits(&octal, 8).unwrap().to_string(),
                expected
            );
        }
        assert_eq!(
            JsBigInt::from_digits("40000000000", 8).unwrap().to_string(),
            "4294967296"
        );
    }

    #[test]
    fn size_cap() {
        let largest = JsBigInt::from_digits(&"f".repeat(MAX_BIGINT_BITS / 4), 16).unwrap();
        assert_eq!(largest.to_sign_magnitude().1.len(), MAX_BIGINT_BITS / 32);
        assert_eq!(
            JsBigInt::from_digits(&format!("1{}", "0".repeat(MAX_BIGINT_BITS)), 2),
            Err(BigIntError::TooLarge)
        );
        assert_eq!(
            JsBigInt::from_sign_magnitude(false, vec![1; MAX_BIGINT_BITS / 32 + 1]),
            Err(BigIntError::TooLarge)
        );
    }

    #[test]
    fn signed_arithmetic_matches_i128_and_canonical_forms() {
        let values = [
            i64::MIN,
            i64::MIN + 1,
            -4294967296,
            -1,
            0,
            1,
            4294967295,
            i64::MAX,
        ];
        let exact = |n: i128| {
            let magnitude = n.unsigned_abs();
            JsBigInt::from_sign_magnitude(
                n < 0,
                (0..4).map(|i| (magnitude >> (i * 32)) as u32).collect(),
            )
            .unwrap()
        };
        for &a in &values {
            let x = JsBigInt::from(a);
            assert_eq!(x.negated(), exact(-(a as i128)));
            assert_eq!(x.negated().negated(), x);
            for &b in &values {
                let y = JsBigInt::from(b);
                assert_eq!(x.checked_add(&y).unwrap(), exact(a as i128 + b as i128));
                assert_eq!(x.checked_sub(&y).unwrap(), exact(a as i128 - b as i128));
                assert_eq!(x.checked_mul(&y).unwrap(), exact(a as i128 * b as i128));
            }
        }
        let high = JsBigInt::from_sign_magnitude(false, vec![0, 0, 1]).unwrap();
        let below = JsBigInt::from_sign_magnitude(false, vec![u32::MAX; 2]).unwrap();
        assert_eq!(high.checked_sub(&JsBigInt::from(1)).unwrap(), below);
        assert_eq!(below.checked_add(&JsBigInt::from(1)).unwrap(), high);
        assert_eq!(
            high.checked_add(&high.negated()).unwrap(),
            JsBigInt::from(0)
        );
        assert_eq!(JsBigInt::from(0).negated(), JsBigInt::from(0));
    }

    #[test]
    fn arithmetic_size_cap_and_cancellation() {
        let cap =
            JsBigInt::from_sign_magnitude(false, vec![u32::MAX; MAX_BIGINT_BITS / 32]).unwrap();
        let one = JsBigInt::from(1);
        assert_eq!(cap.checked_add(&one), Err(BigIntError::TooLarge));
        assert_eq!(cap.negated().checked_sub(&one), Err(BigIntError::TooLarge));
        assert_eq!(
            cap.checked_mul(&JsBigInt::from(2)),
            Err(BigIntError::TooLarge)
        );
        assert_eq!(cap.checked_mul(&one).unwrap(), cap);
        assert_eq!(
            cap.checked_mul(&JsBigInt::from(0)).unwrap(),
            JsBigInt::from(0)
        );
        assert_eq!(cap.checked_sub(&cap).unwrap(), JsBigInt::from(0));
        let mut limbs = vec![u32::MAX; MAX_BIGINT_BITS / 32];
        *limbs.last_mut().unwrap() = 0x7fffffff;
        let almost = JsBigInt::from_sign_magnitude(false, limbs).unwrap();
        // The bit-length lower bound fits, but the actual product does not.
        assert_eq!(
            almost.checked_mul(&JsBigInt::from(3)),
            Err(BigIntError::TooLarge)
        );
    }

    #[test]
    fn signed_order_and_exact_number_comparison() {
        let mut ordered = Vec::new();
        for (negative, limbs) in [
            (true, vec![0, 0, 1]),
            (true, vec![1, 0x80000000]),
            (true, vec![0, 0x80000000]),
            (true, vec![1]),
            (true, vec![0]),
            (false, vec![1]),
            (false, vec![u32::MAX, 0x7fffffff]),
            (false, vec![0, 0x80000000]),
            (false, vec![0, 0, 1]),
        ] {
            ordered.push(JsBigInt::from_sign_magnitude(negative, limbs).unwrap());
        }
        for (i, a) in ordered.iter().enumerate() {
            for (j, b) in ordered.iter().enumerate() {
                assert_eq!(a.cmp(b), i.cmp(&j), "{a}, {b}");
            }
            assert_eq!(a.cmp_number(f64::NAN), None);
            assert_eq!(a.cmp_number(f64::INFINITY), Some(Ordering::Less));
            assert_eq!(a.cmp_number(f64::NEG_INFINITY), Some(Ordering::Greater));
        }
        for (integer, number, expected) in [
            (0, -0.0, Ordering::Equal),
            (0, f64::from_bits(1), Ordering::Less),
            (0, -f64::from_bits(1), Ordering::Greater),
            (0, f64::MIN_POSITIVE, Ordering::Less),
            (0, -f64::MIN_POSITIVE, Ordering::Greater),
            (0, 0.5, Ordering::Less),
            (0, -0.5, Ordering::Greater),
            (2, 2.1, Ordering::Less),
            (3, 2.1, Ordering::Greater),
            (-2, -2.1, Ordering::Greater),
            (-3, -2.1, Ordering::Less),
            (i64::MIN, -9223372036854775808.0, Ordering::Equal),
            (i64::MAX, 9223372036854775808.0, Ordering::Less),
        ] {
            assert_eq!(JsBigInt::from(integer).cmp_number(number), Some(expected));
        }
        // Every normal power of two at or above 1, and its neighbouring
        // integers, exercise all shifts and limb alignments in f64 decoding.
        for exponent in 0..=1023 {
            let mut limbs = vec![0; exponent / 32 + 1];
            limbs[exponent / 32] = 1 << (exponent % 32);
            let power = JsBigInt::from_sign_magnitude(false, limbs.clone()).unwrap();
            let number = f64::from_bits(((exponent + 1023) as u64) << 52);
            assert_eq!(power.cmp_number(number), Some(Ordering::Equal));
            let negative = JsBigInt::from_sign_magnitude(true, limbs.clone()).unwrap();
            assert_eq!(negative.cmp_number(-number), Some(Ordering::Equal));
            // 2^exponent - 1 and 2^exponent + 1, without BigInt arithmetic.
            limbs[exponent / 32] -= 1;
            for limb in &mut limbs[..exponent / 32] {
                *limb = u32::MAX;
            }
            let below = JsBigInt::from_sign_magnitude(false, limbs).unwrap();
            assert_eq!(below.cmp_number(number), Some(Ordering::Less));
            let (_, mut above) = power.to_sign_magnitude();
            above[0] |= 1;
            if exponent == 0 {
                above[0] = 2;
            }
            let above = JsBigInt::from_sign_magnitude(false, above).unwrap();
            assert_eq!(above.cmp_number(number), Some(Ordering::Greater));
        }
        // The largest finite double is (2^53 - 1) * 2^971.
        let mut max_limbs = vec![0; 32];
        max_limbs[30] = 0xfffff800;
        max_limbs[31] = u32::MAX;
        let max = JsBigInt::from_sign_magnitude(false, max_limbs.clone()).unwrap();
        assert_eq!(max.cmp_number(f64::MAX), Some(Ordering::Equal));
        max_limbs[0] = 1;
        let above = JsBigInt::from_sign_magnitude(false, max_limbs).unwrap();
        assert_eq!(above.cmp_number(f64::MAX), Some(Ordering::Greater));
        let cap =
            JsBigInt::from_sign_magnitude(false, vec![u32::MAX; MAX_BIGINT_BITS / 32]).unwrap();
        assert_eq!(cap.cmp_number(f64::MAX), Some(Ordering::Greater));
        assert_eq!(cap.cmp_number(f64::INFINITY), Some(Ordering::Less));
    }
}
