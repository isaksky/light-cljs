//! Immutable BigInt primitives. Small values are inline; large sign/magnitude
//! bodies are shared leaves with no traced heap edges. All construction passes
//! through normalization, so each mathematical value has exactly one form.
//! In-tree arithmetic uses little-endian base-2^32 limbs; no external crate.

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

impl JsBigInt {
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
}
