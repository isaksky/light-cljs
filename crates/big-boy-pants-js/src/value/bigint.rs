//! Immutable BigInt primitives. Small values are inline; large sign/magnitude
//! bodies are shared leaves with no traced heap edges. All construction passes
//! through normalization, so each mathematical value has exactly one form.
//! In-tree arithmetic uses little-endian base-2^32 limbs; no external crate.

use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

/// Maximum magnitude size, matching the runtime's BigInt policy (1 Mbit).
pub const MAX_BIGINT_BITS: usize = 1 << 20;

/// Invalid digits, an oversized magnitude, or an invalid arithmetic operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BigIntError {
    InvalidDigits,
    TooLarge,
    DivisionByZero,
    NegativeExponent,
}

impl fmt::Display for BigIntError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidDigits => "invalid BigInt digits",
            Self::TooLarge => "BigInt is too large",
            Self::DivisionByZero => "BigInt division by zero",
            Self::NegativeExponent => "BigInt negative exponent",
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
    Large {
        negative: bool,
        limbs: Arc<BigIntLimbs>,
    },
}

#[derive(Debug)]
struct BigIntLimbs {
    words: Box<[u32]>,
    reservation: Option<Arc<crate::heap::budget::ByteReservation>>,
}
impl std::ops::Deref for BigIntLimbs {
    type Target = [u32];
    fn deref(&self) -> &[u32] {
        &self.words
    }
}
impl PartialEq for BigIntLimbs {
    fn eq(&self, other: &Self) -> bool {
        self.words == other.words
    }
}
impl Eq for BigIntLimbs {}
impl std::hash::Hash for BigIntLimbs {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.words, state);
    }
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
    pub(crate) fn bit_length(&self) -> usize {
        let mut small = [0; 2];
        let (_, limbs) = self.magnitude(&mut small);
        limbs.last().map_or(0, |high| {
            (limbs.len() - 1) * 32 + (32 - high.leading_zeros() as usize)
        })
    }

    pub(crate) fn accounted(
        heap: &crate::heap::Heap,
        bits: usize,
        operation: impl FnOnce() -> Result<Self, BigIntError>,
    ) -> Result<Self, crate::vm::dispatch::Fault> {
        let reservation = if bits <= 63 || !heap.has_byte_limit() {
            None
        } else {
            let bytes = bits.min(MAX_BIGINT_BITS).div_ceil(32) * 16 + 256;
            Some(Arc::new(
                heap.reserve_bytes(bytes)
                    .ok_or(crate::vm::dispatch::Fault::HeapLimit)?,
            ))
        };
        let mut result = operation()
            .map_err(|error| crate::vm::dispatch::Fault::RangeError(error.to_string()))?;
        if let Repr::Large { limbs, .. } = &mut result.0
            && let Some(body) = Arc::get_mut(limbs)
        {
            body.reservation = reservation;
        }
        Ok(result)
    }
    /// Wrap to a signed two's-complement width without constructing 2^bits.
    pub fn checked_as_int_n(&self, bits: u64) -> Result<Self, BigIntError> {
        self.checked_as_n(bits, true)
    }

    /// Wrap to an unsigned width. A negative value needs `bits` result bits;
    /// reject widths above the magnitude cap before allocating a buffer.
    pub fn checked_as_uint_n(&self, bits: u64) -> Result<Self, BigIntError> {
        self.checked_as_n(bits, false)
    }

    fn checked_as_n(&self, bits: u64, signed: bool) -> Result<Self, BigIntError> {
        if bits == 0 {
            return Ok(Self::from(0));
        }
        let mut small = [0; 2];
        let (negative, limbs) = self.magnitude(&mut small);
        let magnitude_bits = limbs.last().map_or(0, |high| {
            (limbs.len() - 1) * 32 + 32 - high.leading_zeros() as usize
        });
        if (signed && bits > magnitude_bits as u64)
            || (!signed && !negative && bits >= magnitude_bits as u64)
        {
            return Ok(self.clone());
        }
        if bits > MAX_BIGINT_BITS as u64 {
            return Err(BigIntError::TooLarge);
        }
        let bits = bits as usize;
        let mut carry = negative;
        let mut out = (0..bits.div_ceil(32))
            .map(|i| complement_word(limbs.get(i).copied().unwrap_or(0), negative, &mut carry))
            .collect::<Vec<_>>();
        let mask = u32::MAX >> ((32 - bits % 32) % 32);
        *out.last_mut().unwrap() &= mask;
        let result_negative = signed && out.last().unwrap() & (1 << ((bits - 1) % 32)) != 0;
        if result_negative {
            // Invert-plus-one converts the truncated two's-complement words
            // back to a magnitude. Mask again at a partial high limb.
            let mut carry = true;
            for word in &mut out {
                *word = complement_word(*word, true, &mut carry);
            }
            *out.last_mut().unwrap() &= mask;
        }
        Self::from_sign_magnitude(result_negative, out)
    }

    /// Round to binary64 once, to nearest with ties to even. Overflow rounds
    /// to signed infinity; canonical BigInt zero always gives positive zero.
    /// Borrow the existing magnitude, and examine at most 1024 bits: larger
    /// magnitudes are already beyond the finite binary64 range.
    pub fn to_number(&self) -> f64 {
        let mut small = [0; 2];
        let (negative, limbs) = self.magnitude(&mut small);
        let Some(high) = limbs.last() else {
            return 0.0;
        };
        let bits = (limbs.len() - 1) * 32 + (32 - high.leading_zeros() as usize);
        let sign = u64::from(negative) << 63;
        if bits > 1024 {
            return f64::from_bits(sign | f64::INFINITY.to_bits());
        }
        if bits <= 53 {
            let integer = limbs[0] as u64 | ((limbs.get(1).copied().unwrap_or(0) as u64) << 32);
            return if negative {
                -(integer as f64)
            } else {
                integer as f64
            };
        }

        let shift = bits - 53;
        let bit = |index: usize| (limbs[index / 32] >> (index % 32)) & 1;
        let mut significand = 0u64;
        for index in (shift..bits).rev() {
            significand = (significand << 1) | u64::from(bit(index));
        }
        let guard = shift - 1;
        let sticky = limbs[..guard / 32].iter().any(|limb| *limb != 0)
            || limbs[guard / 32] & ((1u32 << (guard % 32)) - 1) != 0;
        if bit(guard) != 0 && (sticky || significand & 1 != 0) {
            significand += 1;
        }
        let mut exponent = bits - 1;
        if significand == 1u64 << 53 {
            significand >>= 1;
            exponent += 1;
        }
        if exponent > 1023 {
            return f64::from_bits(sign | f64::INFINITY.to_bits());
        }
        f64::from_bits(
            sign | (((exponent + 1023) as u64) << 52) | (significand & ((1u64 << 52) - 1)),
        )
    }

    /// Convert an integral finite binary64 Number exactly, including values
    /// outside i64 and canonical negative zero. Callers validate integrality.
    pub fn from_integral_number(number: f64) -> Option<Self> {
        if !number.is_finite() || number.trunc() != number {
            return None;
        }
        let bits = number.to_bits();
        let biased_exponent = ((bits >> 52) & 0x7ff) as i32;
        let significand =
            (bits & ((1u64 << 52) - 1)) | if biased_exponent == 0 { 0 } else { 1u64 << 52 };
        let exponent = biased_exponent.max(1) - 1023 - 52;
        let limbs = if exponent < 0 {
            let integer = significand.checked_shr((-exponent) as u32).unwrap_or(0);
            vec![integer as u32, (integer >> 32) as u32]
        } else {
            let words = exponent as usize / 32;
            let shift = exponent as u32 % 32;
            let mut limbs = vec![0; words + 3];
            for (i, limb) in [significand as u32, (significand >> 32) as u32]
                .into_iter()
                .enumerate()
            {
                let word = (limb as u64) << shift;
                limbs[words + i] |= word as u32;
                limbs[words + i + 1] |= (word >> 32) as u32;
            }
            limbs
        };
        Some(
            Self::from_sign_magnitude(number.is_sign_negative(), limbs)
                .expect("a finite binary64 integer fits the BigInt cap"),
        )
    }

    /// Infinite two's-complement NOT: ~x = -x - 1. The intermediate negation
    /// preserves magnitude; the subtraction checks any extra result bit.
    pub fn checked_not(&self) -> Result<Self, BigIntError> {
        self.negated().checked_sub(&Self::from(1))
    }

    pub fn checked_and(&self, other: &Self) -> Result<Self, BigIntError> {
        self.checked_logic(other, |a, b| a & b)
    }

    pub fn checked_or(&self, other: &Self) -> Result<Self, BigIntError> {
        self.checked_logic(other, |a, b| a | b)
    }

    pub fn checked_xor(&self, other: &Self) -> Result<Self, BigIntError> {
        self.checked_logic(other, |a, b| a ^ b)
    }

    /// Stream finite two's-complement words, keeping the infinite sign word
    /// separately. Convert back in place, without allocating sign extension.
    fn checked_logic(
        &self,
        other: &Self,
        op: impl Fn(u32, u32) -> u32,
    ) -> Result<Self, BigIntError> {
        let (mut small_a, mut small_b) = ([0; 2], [0; 2]);
        let (negative_a, a) = self.magnitude(&mut small_a);
        let (negative_b, b) = other.magnitude(&mut small_b);
        let negative = op(u32::from(negative_a), u32::from(negative_b)) != 0;
        let (mut carry_a, mut carry_b, mut carry_out) = (negative_a, negative_b, negative);
        let mut out = Vec::with_capacity(a.len().max(b.len()));
        for i in 0..a.len().max(b.len()) {
            let x = complement_word(a.get(i).copied().unwrap_or(0), negative_a, &mut carry_a);
            let y = complement_word(b.get(i).copied().unwrap_or(0), negative_b, &mut carry_b);
            out.push(complement_word(op(x, y), negative, &mut carry_out));
        }
        if carry_out {
            // A negative result with all-zero finite words needs one more
            // magnitude word (e.g. -1 ^ (2^32 - 1) = -2^32).
            if out.len() == MAX_BIGINT_BITS / 32 {
                return Err(BigIntError::TooLarge);
            }
            out.push(1);
        }
        Self::from_sign_magnitude(negative, out)
    }

    /// Signed shift counts are exact BigInts: negative counts reverse direction.
    pub fn checked_shl(&self, count: &Self) -> Result<Self, BigIntError> {
        self.checked_shift(count, true)
    }

    /// Arithmetic right shift, rounding negative results toward minus infinity.
    pub fn checked_shr(&self, count: &Self) -> Result<Self, BigIntError> {
        self.checked_shift(count, false)
    }

    fn checked_shift(&self, count: &Self, left: bool) -> Result<Self, BigIntError> {
        let (mut small_a, mut small_count) = ([0; 2], [0; 2]);
        let (negative, a) = self.magnitude(&mut small_a);
        let (negative_count, count) = count.magnitude(&mut small_count);
        if a.is_empty() || count.is_empty() {
            return Ok(self.clone());
        }
        let left = left ^ negative_count;
        // Counts above the cap cannot produce any nonzero left-shift result
        // or any right-shift result beyond 0/-1. Never narrow a huge count.
        if count.len() > 1 || count[0] as usize > MAX_BIGINT_BITS {
            return if left {
                Err(BigIntError::TooLarge)
            } else {
                Ok(Self::from(if negative { -1 } else { 0 }))
            };
        }
        let count = count[0] as usize;
        let bits = (a.len() - 1) * 32 + 32 - a.last().unwrap().leading_zeros() as usize;
        let (words, shift) = (count / 32, count % 32);
        if left {
            // Exact bit length is known before allocating the result buffer.
            if count > MAX_BIGINT_BITS - bits {
                return Err(BigIntError::TooLarge);
            }
            let mut out = vec![0; (bits + count).div_ceil(32)];
            for (i, &limb) in a.iter().enumerate() {
                let word = (limb as u64) << shift;
                out[words + i] |= word as u32;
                if word >> 32 != 0 {
                    out[words + i + 1] |= (word >> 32) as u32;
                }
            }
            Self::from_sign_magnitude(negative, out)
        } else {
            if count >= bits {
                return Ok(Self::from(if negative { -1 } else { 0 }));
            }
            let mut out = Vec::with_capacity(a.len() - words);
            for i in words..a.len() {
                let mut word = a[i] >> shift;
                if shift != 0 {
                    word |= a.get(i + 1).copied().unwrap_or(0) << (32 - shift);
                }
                out.push(word);
            }
            let discarded =
                a[..words].iter().any(|&word| word != 0) || a[words] & ((1u32 << shift) - 1) != 0;
            if negative && discarded {
                // Floor(-a/2^count) = -ceil(a/2^count). The increment fits
                // within the original magnitude because count is nonzero.
                let mut carry = true;
                for word in &mut out {
                    (*word, carry) = word.overflowing_add(u32::from(carry));
                    if !carry {
                        break;
                    }
                }
                if carry {
                    out.push(1);
                }
            }
            Self::from_sign_magnitude(negative, out)
        }
    }

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

    /// Truncating quotient and remainder (the latter has the dividend's sign).
    /// Normalized base-2^32 long division estimates and corrects one quotient
    /// limb at a time, rather than performing a division step for every bit.
    pub fn checked_div_rem(&self, other: &Self) -> Result<(Self, Self), BigIntError> {
        let (mut small_a, mut small_b) = ([0; 2], [0; 2]);
        let (negative_a, a) = self.magnitude(&mut small_a);
        let (negative_b, b) = other.magnitude(&mut small_b);
        if b.is_empty() {
            return Err(BigIntError::DivisionByZero);
        }
        if a.len()
            .cmp(&b.len())
            .then_with(|| a.iter().rev().cmp(b.iter().rev()))
            .is_lt()
        {
            return Ok((Self::from(0), self.clone()));
        }
        let (quotient, remainder) = divide_magnitudes(a, b);
        Ok((
            Self::from_sign_magnitude(negative_a ^ negative_b, quotient)?,
            Self::from_sign_magnitude(negative_a, remainder)?,
        ))
    }

    /// Exact integer power. Bound the result before computing, then check each
    /// multiplication; exponentiation by squaring never builds an unused power.
    pub fn checked_pow(&self, exponent: &Self) -> Result<Self, BigIntError> {
        let (mut small_a, mut small_e) = ([0; 2], [0; 2]);
        let (negative, a) = self.magnitude(&mut small_a);
        let (negative_e, e) = exponent.magnitude(&mut small_e);
        if negative_e {
            return Err(BigIntError::NegativeExponent);
        }
        if e.is_empty() {
            return Ok(Self::from(1));
        }
        if a.is_empty() {
            return Ok(Self::from(0));
        }
        let odd = e[0] & 1 != 0;
        if a == [1] {
            return Ok(Self::from(if negative && odd { -1 } else { 1 }));
        }
        let bits = (a.len() - 1) * 32 + 32 - a.last().unwrap().leading_zeros() as usize;
        // |a| >= 2^(bits-1): powers with this lower bound above the cap
        // cannot fit. Remaining exponents fit u32, independently of host size.
        if e.len() > 1 || e[0] as usize > (MAX_BIGINT_BITS - 1) / (bits - 1) {
            return Err(BigIntError::TooLarge);
        }
        let mut exponent = e[0];
        // Powers of two have an exact size bound and can be placed directly,
        // including the largest legal power, without quadratic multiplication.
        if a.iter().map(|limb| limb.count_ones()).sum::<u32>() == 1 {
            let bit = (bits - 1) * exponent as usize;
            let mut limbs = vec![0; bit / 32 + 1];
            limbs[bit / 32] = 1 << (bit % 32);
            return Self::from_sign_magnitude(negative && odd, limbs);
        }
        let mut result = Self::from(1);
        let mut power = self.clone();
        loop {
            if exponent & 1 != 0 {
                result = result.checked_mul(&power)?;
            }
            exponent >>= 1;
            if exponent == 0 {
                return Ok(result);
            }
            power = power.checked_mul(&power)?;
        }
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
            limbs: Arc::new(BigIntLimbs {
                words: limbs.into_boxed_slice(),
                reservation: None,
            }),
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

/// Shared, resumable StringToBigInt. Each advance performs at most 256 bounded
/// scan/limb steps; a decimal group reads at most nine digits. Validation precedes size rejection so
/// malformed text remains invalid even when its digit prefix exceeds the cap.
/// The string may flatten once (linear in its length); decimal multiplication
/// is chunked and budgeted, with storage capped at MAX_BIGINT_BITS / 32 limbs.
#[derive(Debug)]
pub struct StringToBigInt {
    text: super::JsString,
    start: usize,
    end: usize,
    next: usize,
    stage: ParseStage,
    radix: u32,
    negative: bool,
    limbs: Vec<u32>,
    base: u64,
    carry: u64,
    limb: usize,
}

#[derive(Debug)]
enum ParseStage {
    TrimStart,
    TrimEnd,
    Prefix,
    Validate,
    Build,
}

impl StringToBigInt {
    pub fn new(text: super::JsString) -> Self {
        Self {
            end: text.len(),
            text,
            start: 0,
            next: 0,
            stage: ParseStage::TrimStart,
            radix: 10,
            negative: false,
            limbs: vec![],
            base: 0,
            carry: 0,
            limb: 0,
        }
    }

    /// None means pending; Some contains the finished result or parse error.
    pub fn advance(&mut self) -> Option<Result<JsBigInt, BigIntError>> {
        use super::string::is_str_white_space;
        let units = self.text.code_units();
        for _ in 0..256 {
            match self.stage {
                ParseStage::TrimStart => {
                    if self.start == self.end {
                        return Some(Ok(JsBigInt::from(0)));
                    }
                    if is_str_white_space(units[self.start]) {
                        self.start += 1;
                    } else {
                        self.stage = ParseStage::TrimEnd;
                    }
                }
                ParseStage::TrimEnd => {
                    if is_str_white_space(units[self.end - 1]) {
                        self.end -= 1;
                    } else {
                        self.stage = ParseStage::Prefix;
                    }
                }
                ParseStage::Prefix => {
                    let signed = matches!(units[self.start], 43 | 45);
                    if signed {
                        self.negative = units[self.start] == 45;
                        self.start += 1;
                    } else if self.end - self.start >= 2 && units[self.start] == 48 {
                        self.radix = match units[self.start + 1] {
                            120 | 88 => 16,
                            111 | 79 => 8,
                            98 | 66 => 2,
                            _ => 10,
                        };
                        if self.radix != 10 {
                            self.start += 2;
                        }
                    }
                    if self.start == self.end {
                        return Some(Err(BigIntError::InvalidDigits));
                    }
                    self.next = self.start;
                    self.stage = ParseStage::Validate;
                }
                ParseStage::Validate => {
                    if self.next < self.end {
                        let Some(digit) = char::from_u32(units[self.next] as u32)
                            .and_then(|c| c.to_digit(self.radix))
                        else {
                            return Some(Err(BigIntError::InvalidDigits));
                        };
                        if digit == 0 && self.next == self.start {
                            self.start += 1;
                        }
                        self.next += 1;
                    } else {
                        let digits = self.end - self.start;
                        if digits == 0 {
                            return Some(Ok(JsBigInt::from(0)));
                        }
                        if self.radix == 10 {
                            // 30103/100000 is an upper bound on log10(2).
                            if digits > MAX_BIGINT_BITS * 30103 / 100000 + 1 {
                                return Some(Err(BigIntError::TooLarge));
                            }
                            self.next = self.start;
                        } else {
                            let width = self.radix.trailing_zeros() as usize;
                            let high = char::from_u32(units[self.start] as u32)
                                .unwrap()
                                .to_digit(self.radix)
                                .unwrap();
                            let bits = (digits - 1) * width + 32 - high.leading_zeros() as usize;
                            if bits > MAX_BIGINT_BITS {
                                return Some(Err(BigIntError::TooLarge));
                            }
                            self.limbs.resize(bits.div_ceil(32), 0);
                            self.next = self.end;
                        }
                        self.stage = ParseStage::Build;
                    }
                }
                ParseStage::Build if self.radix != 10 => {
                    if self.next == self.start {
                        return Some(self.finish());
                    }
                    self.next -= 1;
                    let digit = char::from_u32(units[self.next] as u32)
                        .unwrap()
                        .to_digit(self.radix)
                        .unwrap();
                    let width = self.radix.trailing_zeros() as usize;
                    let bit = (self.end - 1 - self.next) * width;
                    self.limbs[bit / 32] |= digit << (bit % 32);
                    if bit % 32 + width > 32 && bit / 32 + 1 < self.limbs.len() {
                        self.limbs[bit / 32 + 1] |= digit >> (32 - bit % 32);
                    }
                }
                ParseStage::Build => {
                    if self.base == 0 {
                        if self.next == self.end {
                            return Some(self.finish());
                        }
                        self.base = 1;
                        self.carry = 0;
                        self.limb = 0;
                        for &unit in &units[self.next..(self.next + 9).min(self.end)] {
                            self.carry = self.carry * 10 + (unit - 48) as u64;
                            self.base *= 10;
                        }
                        self.next = (self.next + 9).min(self.end);
                    } else if self.limb < self.limbs.len() {
                        let product = self.limbs[self.limb] as u64 * self.base + self.carry;
                        self.limbs[self.limb] = product as u32;
                        self.carry = product >> 32;
                        self.limb += 1;
                    } else {
                        if self.carry != 0 {
                            if self.limbs.len() == MAX_BIGINT_BITS / 32 {
                                return Some(Err(BigIntError::TooLarge));
                            }
                            self.limbs.push(self.carry as u32);
                        }
                        self.base = 0;
                    }
                }
            }
        }
        None
    }

    fn finish(&mut self) -> Result<JsBigInt, BigIntError> {
        JsBigInt::from_sign_magnitude(self.negative, std::mem::take(&mut self.limbs))
    }
}

/// One word of a sign/magnitude <-> two's-complement conversion. The same
/// invert-plus-one operation works in either direction for a negative value.
fn complement_word(word: u32, negative: bool, carry: &mut bool) -> u32 {
    if negative {
        let (word, next) = (!word).overflowing_add(u32::from(*carry));
        *carry = next;
        word
    } else {
        word
    }
}

/// Unsigned division, with nonempty b and a >= b. Knuth's normalized long
/// division (Algorithm D), implemented here over the in-tree limb format.
fn divide_magnitudes(a: &[u32], b: &[u32]) -> (Vec<u32>, Vec<u32>) {
    if b.len() == 1 {
        let mut q = vec![0; a.len()];
        let mut rem = 0u64;
        for (out, &limb) in q.iter_mut().zip(a).rev() {
            let dividend = (rem << 32) | limb as u64;
            *out = (dividend / b[0] as u64) as u32;
            rem = dividend % b[0] as u64;
        }
        return (q, vec![rem as u32]);
    }

    const BASE: u64 = 1u64 << 32;
    let n = b.len();
    let shift = b[n - 1].leading_zeros();
    let normalize = |limbs: &[u32]| {
        let mut out = Vec::with_capacity(limbs.len() + 1);
        let mut carry = 0u64;
        for &limb in limbs {
            let word = ((limb as u64) << shift) | carry;
            out.push(word as u32);
            carry = word >> 32;
        }
        (out, carry as u32)
    };
    let (v, carry) = normalize(b);
    debug_assert_eq!(carry, 0);
    let (mut u, carry) = normalize(a);
    // This guard limb is normalized division workspace, not an intermediate
    // BigInt magnitude. Output quotient and remainder never exceed the input.
    u.push(carry);
    let mut q = vec![0; a.len() - n + 1];
    for j in (0..q.len()).rev() {
        let top = ((u[j + n] as u64) << 32) | u[j + n - 1] as u64;
        let mut estimate = (top / v[n - 1] as u64).min(BASE - 1);
        let mut rem = top - estimate * v[n - 1] as u64;
        while rem < BASE && estimate * v[n - 2] as u64 > ((rem << 32) | u[j + n - 2] as u64) {
            estimate -= 1;
            rem += v[n - 1] as u64;
        }

        // Subtract estimate * v, keeping the multiply carry and subtract
        // borrow together. Each product plus incoming borrow fits u64.
        let mut borrow = 0u64;
        for i in 0..n {
            let product = estimate * v[i] as u64 + borrow;
            let (difference, underflow) = u[j + i].overflowing_sub(product as u32);
            u[j + i] = difference;
            borrow = (product >> 32) + u64::from(underflow);
        }
        let (high, underflow) = u[j + n].overflowing_sub(borrow as u32);
        u[j + n] = high;
        if underflow {
            // The estimate was one too high: restore one divisor.
            estimate -= 1;
            let mut carry = 0u64;
            for i in 0..n {
                let sum = u[j + i] as u64 + v[i] as u64 + carry;
                u[j + i] = sum as u32;
                carry = sum >> 32;
            }
            u[j + n] = u[j + n].wrapping_add(carry as u32);
        }
        q[j] = estimate as u32;
    }
    u.truncate(n);
    if shift != 0 {
        for i in 0..n {
            u[i] = (u[i] >> shift) | (u.get(i + 1).copied().unwrap_or(0) << (32 - shift));
        }
    }
    (q, u)
}

/// Task-owned radix conversion. Non-binary radices divide by the largest
/// radix power fitting u32 (10^9 for decimal), rather than one digit at a time.
/// Each advance does at most 256 limb updates or output-digit copies.
#[derive(Debug)]
pub(crate) struct BigIntToString {
    negative: bool,
    limbs: Vec<u32>,
    radix: u32,
    base: u32,
    width: usize,
    cursor: usize,
    remainder: u64,
    digits: Vec<u16>,
    output: Vec<u16>,
    copying: bool,
    binary_bit: usize,
}

impl BigIntToString {
    pub(crate) fn new(value: &JsBigInt, radix: u32) -> Self {
        assert!((2..=36).contains(&radix));
        let (negative, limbs) = value.to_sign_magnitude();
        let mut base = radix;
        let mut width = 1;
        while let Some(next) = base.checked_mul(radix) {
            base = next;
            width += 1;
        }
        let bits = limbs.last().map_or(0, |last| {
            (limbs.len() - 1) * 32 + 32 - last.leading_zeros() as usize
        });
        Self {
            negative,
            cursor: limbs.len(),
            limbs,
            radix,
            base,
            width,
            remainder: 0,
            digits: if bits == 0 { vec![b'0' as u16] } else { vec![] },
            output: vec![],
            copying: bits == 0,
            binary_bit: bits,
        }
    }

    fn digit(n: u32) -> u16 {
        (if n < 10 {
            b'0' + n as u8
        } else {
            b'a' + (n - 10) as u8
        }) as u16
    }

    pub(crate) fn advance(&mut self) -> Option<super::JsString> {
        for _ in 0..256 {
            if self.copying {
                if self.output.is_empty() && self.negative {
                    self.output.push(b'-' as u16);
                }
                if let Some(digit) = self.digits.pop() {
                    self.output.push(digit);
                } else {
                    return super::JsString::from_owned_code_units(std::mem::take(
                        &mut self.output,
                    ));
                }
            } else if self.radix.is_power_of_two() {
                // Direct high-to-low extraction is linear for 2/4/8/16/32.
                let width = self.radix.trailing_zeros() as usize;
                let take = if self.output.is_empty() {
                    (self.binary_bit - 1) % width + 1
                } else {
                    width
                };
                if self.output.is_empty() && self.negative {
                    self.output.push(b'-' as u16);
                }
                self.binary_bit -= take;
                let limb = self.binary_bit / 32;
                let shift = self.binary_bit % 32;
                let pair = self.limbs[limb] as u64
                    | ((self.limbs.get(limb + 1).copied().unwrap_or(0) as u64) << 32);
                self.output
                    .push(Self::digit(((pair >> shift) & ((1 << take) - 1)) as u32));
                if self.binary_bit == 0 {
                    return super::JsString::from_owned_code_units(std::mem::take(
                        &mut self.output,
                    ));
                }
            } else if self.cursor != 0 {
                self.cursor -= 1;
                let value = (self.remainder << 32) | self.limbs[self.cursor] as u64;
                self.limbs[self.cursor] = (value / self.base as u64) as u32;
                self.remainder = value % self.base as u64;
            } else {
                while self.limbs.last() == Some(&0) {
                    self.limbs.pop();
                }
                let mut chunk = self.remainder as u32;
                // At most 31 digits: a constant amount per division pass.
                for i in 0..self.width {
                    self.digits.push(Self::digit(chunk % self.radix));
                    chunk /= self.radix;
                    if chunk == 0 && self.limbs.is_empty() && i + 1 < self.width {
                        break;
                    }
                }
                self.remainder = 0;
                self.cursor = self.limbs.len();
                self.copying = self.limbs.is_empty();
            }
        }
        None
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

    #[test]
    fn wrapping_matches_small_integer_modulo_and_stays_canonical() {
        for bits in 0..=16 {
            let modulus = 1i64 << bits;
            for n in -1025i64..=1025 {
                let unsigned = n.rem_euclid(modulus);
                let signed = if bits != 0 && unsigned >= modulus / 2 {
                    unsigned - modulus
                } else {
                    unsigned
                };
                let value = JsBigInt::from(n);
                assert_eq!(
                    value.checked_as_uint_n(bits).unwrap(),
                    JsBigInt::from(unsigned)
                );
                assert_eq!(
                    value.checked_as_int_n(bits).unwrap(),
                    JsBigInt::from(signed)
                );
            }
        }
        for bits in [31, 32, 33, 63, 64, 65, 95, 96, 97, 127] {
            let power = JsBigInt::from(1)
                .checked_shl(&JsBigInt::from(bits))
                .unwrap();
            let max = power.checked_sub(&JsBigInt::from(1)).unwrap();
            assert_eq!(
                JsBigInt::from(-1).checked_as_uint_n(bits as u64).unwrap(),
                max
            );
            assert_eq!(
                max.checked_as_int_n(bits as u64).unwrap(),
                JsBigInt::from(-1)
            );
            assert_eq!(
                power.negated().checked_as_uint_n(bits as u64).unwrap(),
                JsBigInt::from(0)
            );
        }
        assert_eq!(
            JsBigInt::from(-1).checked_as_uint_n(u64::MAX),
            Err(BigIntError::TooLarge)
        );
        assert_eq!(
            JsBigInt::from(-1).checked_as_int_n(u64::MAX).unwrap(),
            JsBigInt::from(-1)
        );
        assert_eq!(
            JsBigInt::from(5).checked_as_uint_n(u64::MAX).unwrap(),
            JsBigInt::from(5)
        );
        let cap =
            JsBigInt::from_sign_magnitude(false, vec![u32::MAX; MAX_BIGINT_BITS / 32]).unwrap();
        assert_eq!(
            cap.checked_as_int_n(MAX_BIGINT_BITS as u64).unwrap(),
            JsBigInt::from(-1)
        );
        assert_eq!(
            JsBigInt::from(-1)
                .checked_as_uint_n(MAX_BIGINT_BITS as u64)
                .unwrap(),
            cap
        );
    }

    #[test]
    fn wrapping_matches_i128_across_limb_boundaries() {
        let from_i128 = |n: i128| {
            let magnitude = n.unsigned_abs();
            JsBigInt::from_sign_magnitude(
                n < 0,
                (0..4).map(|i| (magnitude >> (32 * i)) as u32).collect(),
            )
            .unwrap()
        };
        let mut random = 204u128;
        for _ in 0..200 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let n = (random >> 2) as i128;
            for n in [n, -n, i64::MIN as i128, i64::MAX as i128] {
                for bits in [1, 31, 32, 33, 63, 64, 65, 95, 96, 97, 126] {
                    let modulus = 1i128 << bits;
                    let unsigned = n.rem_euclid(modulus);
                    let signed = if unsigned >= modulus / 2 {
                        unsigned - modulus
                    } else {
                        unsigned
                    };
                    assert_eq!(
                        from_i128(n).checked_as_uint_n(bits).unwrap(),
                        from_i128(unsigned)
                    );
                    assert_eq!(
                        from_i128(n).checked_as_int_n(bits).unwrap(),
                        from_i128(signed)
                    );
                }
            }
        }
    }

    #[test]
    fn number_rounding_at_every_binary64_integer_exponent() {
        // Independent binary64 bit-pattern expectations exercise guard and
        // sticky bits on both sides of every limb boundary, including overflow.
        for exponent in 53..=1023 {
            for fraction in [0, 1, 2, 0x5_5555_5555_5555, (1u64 << 52) - 1] {
                let bits = ((exponent + 1023) << 52) | fraction;
                let number = f64::from_bits(bits);
                let integer = JsBigInt::from_integral_number(number).unwrap();
                assert_eq!(integer.to_number().to_bits(), bits);
                let half = JsBigInt::from(1)
                    .checked_shl(&JsBigInt::from((exponent - 53) as i64))
                    .unwrap();
                let tie = integer.checked_add(&half).unwrap();
                let even = bits + (bits & 1);
                assert_eq!(tie.to_number().to_bits(), even, "{exponent}, {fraction}");
                assert_eq!(tie.negated().to_number().to_bits(), even | (1u64 << 63));
                let below = tie.checked_sub(&JsBigInt::from(1)).unwrap();
                let above = tie.checked_add(&JsBigInt::from(1)).unwrap();
                assert_eq!(below.to_number().to_bits(), bits);
                assert_eq!(above.to_number().to_bits(), bits + 1);
            }
        }
    }

    #[test]
    fn radix_format_matches_unsigned_arithmetic() {
        let mut random = 123u128;
        for _ in 0..80 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            for value in [0, 1, u64::MAX as u128, random] {
                for radix in 2..=36 {
                    let mut n = value;
                    let mut expected = Vec::new();
                    loop {
                        expected.push(
                            char::from_digit((n % radix as u128) as u32, radix).unwrap() as u16
                        );
                        n /= radix as u128;
                        if n == 0 {
                            break;
                        }
                    }
                    expected.reverse();
                    for negative in [false, true] {
                        let mut limbs = vec![];
                        let mut n = value;
                        while n != 0 {
                            limbs.push(n as u32);
                            n >>= 32;
                        }
                        let value = JsBigInt::from_sign_magnitude(negative, limbs).unwrap();
                        let mut formatter = BigIntToString::new(&value, radix);
                        let text = loop {
                            if let Some(text) = formatter.advance() {
                                break text;
                            }
                        };
                        let mut expected = expected.clone();
                        if negative && !value.is_zero() {
                            expected.insert(0, b'-' as u16);
                        }
                        assert_eq!(text.code_units(), expected, "radix {radix}, value {value}");
                    }
                }
            }
        }
    }

    #[test]
    fn radix_format_resumes_preserving_digits() {
        for radix in [3, 10, 36] {
            // Build the mathematical value independently with arithmetic,
            // starting from a nonuniform, known representation in each base.
            let digits: Vec<u32> = (0..2500).map(|i| (i * 17 + 1) % radix).collect();
            let expected: Vec<u16> = digits
                .iter()
                .map(|d| char::from_digit(*d, radix).unwrap() as u16)
                .collect();
            let mut value = JsBigInt::from(0);
            for digit in digits {
                value = value
                    .checked_mul(&JsBigInt::from(radix as i64))
                    .unwrap()
                    .checked_add(&JsBigInt::from(digit as i64))
                    .unwrap();
            }
            for negative in [false, true] {
                let value = if negative {
                    value.negated()
                } else {
                    value.clone()
                };
                let mut formatter = BigIntToString::new(&value, radix);
                let mut steps = 0;
                let text = loop {
                    steps += 1;
                    assert!(steps < 2000, "radix {radix}");
                    if let Some(text) = formatter.advance() {
                        break text;
                    }
                };
                let mut expected = expected.clone();
                if negative {
                    expected.insert(0, b'-' as u16);
                }
                assert!(steps > 2, "must resume division and digit copying");
                assert_eq!(
                    text.code_units(),
                    expected,
                    "radix {radix}, negative {negative}"
                );
            }
        }
    }

    #[test]
    fn radix_format_at_the_size_cap_is_incremental() {
        let value =
            JsBigInt::from_sign_magnitude(true, vec![u32::MAX; MAX_BIGINT_BITS / 32]).unwrap();
        let mut formatter = BigIntToString::new(&value, 2);
        let mut steps = 0;
        let result = loop {
            steps += 1;
            if let Some(text) = formatter.advance() {
                break text;
            }
        };
        assert_eq!(steps, MAX_BIGINT_BITS / 256);
        assert_eq!(result.len(), MAX_BIGINT_BITS + 1);
        assert_eq!(result.code_units()[0], b'-' as u16);
        assert!(
            result.code_units()[1..]
                .iter()
                .all(|unit| *unit == b'1' as u16)
        );
    }

    fn parse(text: &str) -> Result<JsBigInt, BigIntError> {
        let mut parser = StringToBigInt::new(text.into());
        for _ in 0..100_000 {
            if let Some(result) = parser.advance() {
                return result;
            }
        }
        panic!("parser did not finish");
    }

    #[test]
    fn string_to_bigint_uses_canonical_values_and_all_radices() {
        for number in [0, 1, -1, i64::MAX, i64::MIN] {
            assert_eq!(
                parse(&format!("  {number}  ")).unwrap(),
                JsBigInt::from(number)
            );
        }
        for digits in 1..=150 {
            let text = "9".repeat(digits);
            assert_eq!(
                parse(&text).unwrap(),
                JsBigInt::from_digits(&text, 10).unwrap()
            );
            let text = "7".repeat(digits);
            assert_eq!(
                parse(&format!("0o{text}")).unwrap(),
                JsBigInt::from_digits(&text, 8).unwrap()
            );
            let text = "f".repeat(digits);
            assert_eq!(
                parse(&format!("0x{text}")).unwrap(),
                JsBigInt::from_digits(&text, 16).unwrap()
            );
        }
        assert_eq!(
            parse(&format!("{}1", "0".repeat(10_000))).unwrap(),
            JsBigInt::from(1)
        );
        assert_eq!(parse("\u{feff}\n-0\u{3000}").unwrap(), JsBigInt::from(0));
    }

    #[test]
    fn integral_numbers_convert_exactly_at_every_binary_exponent() {
        for exponent in 0..=1023 {
            let number = 2f64.powi(exponent);
            let mut limbs = vec![0; exponent as usize / 32 + 1];
            limbs[exponent as usize / 32] = 1 << (exponent % 32);
            let expected = JsBigInt::from_sign_magnitude(false, limbs).unwrap();
            assert_eq!(
                JsBigInt::from_integral_number(number),
                Some(expected.clone())
            );
            assert_eq!(
                JsBigInt::from_integral_number(-number),
                Some(expected.negated())
            );
        }
        for number in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            1.5,
            -1.5,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
        ] {
            assert_eq!(JsBigInt::from_integral_number(number), None);
        }
        assert_eq!(
            JsBigInt::from_integral_number(-0.0),
            Some(JsBigInt::from(0))
        );
    }
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
    fn bitwise_and_shifts_match_native_i128() {
        let exact = |n: i128| {
            let magnitude = n.unsigned_abs();
            JsBigInt::from_sign_magnitude(
                n < 0,
                (0..4).map(|i| (magnitude >> (i * 32)) as u32).collect(),
            )
            .unwrap()
        };
        let mut state = 0x9824_7b6c_a5d3_f102u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..2000 {
            let a = (((next() as u128) << 64) | next() as u128) as i128;
            let b = (((next() as u128) << 64) | next() as u128) as i128;
            let (x, y) = (exact(a), exact(b));
            assert_eq!(x.checked_not().unwrap(), exact(!a));
            assert_eq!(x.checked_and(&y).unwrap(), exact(a & b));
            assert_eq!(x.checked_or(&y).unwrap(), exact(a | b));
            assert_eq!(x.checked_xor(&y).unwrap(), exact(a ^ b));
            for shift in [0, 1, 31, 32, 33, 63, 64, 65, 95, 96, 127] {
                let count = JsBigInt::from(shift);
                assert_eq!(x.checked_shr(&count).unwrap(), exact(a >> shift));
                assert_eq!(x.checked_shl(&count.negated()).unwrap(), exact(a >> shift));
                // Restrict the left operand so its exact shifted value fits
                // i128; native overflowing shifts would wrap instead.
                let small = a >> shift;
                assert_eq!(
                    exact(small).checked_shl(&count).unwrap(),
                    exact(small << shift)
                );
                assert_eq!(
                    exact(small).checked_shr(&count.negated()).unwrap(),
                    exact(small << shift)
                );
            }
        }
        // Explicit zeros, signed boundaries and trailing-zero limbs exercise
        // canonical inline results and complement carry chains.
        for a in [
            i128::MIN,
            i64::MIN as i128,
            -4294967296,
            -1,
            0,
            1,
            i64::MAX as i128,
            i128::MAX,
        ] {
            assert_eq!(exact(a).checked_not().unwrap(), exact(!a));
            for b in [-4294967296, -1, 0, 1, 4294967295] {
                assert_eq!(exact(a).checked_and(&exact(b)).unwrap(), exact(a & b));
                assert_eq!(exact(a).checked_or(&exact(b)).unwrap(), exact(a | b));
                assert_eq!(exact(a).checked_xor(&exact(b)).unwrap(), exact(a ^ b));
            }
        }
    }

    #[test]
    fn division_matches_native_i128() {
        let exact = |n: i128| {
            let magnitude = n.unsigned_abs();
            JsBigInt::from_sign_magnitude(
                n < 0,
                (0..4).map(|i| (magnitude >> (i * 32)) as u32).collect(),
            )
            .unwrap()
        };
        let mut state = 0xb789_4d3a_21f0_769bu64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        // Deterministic, independent native arithmetic expectations exercise
        // each divisor length and both normalization/no-normalization paths.
        for i in 0..10000 {
            let a = (((next() as u128) << 64) | next() as u128) as i128;
            let width = 1 + i % 127;
            let b = ((((next() as u128) << 64) | next() as u128) >> (128 - width)) as i128 | 1;
            let b = if i % 2 == 0 { b } else { -b };
            let (q, r) = exact(a).checked_div_rem(&exact(b)).unwrap();
            assert_eq!(q, exact(a / b), "{a} / {b}");
            assert_eq!(r, exact(a % b), "{a} % {b}");
        }
        for a in [i64::MIN as i128, -1, 0, 1, i64::MAX as i128] {
            for b in [i64::MIN as i128, -1, 1, i64::MAX as i128] {
                assert_eq!(
                    exact(a).checked_div_rem(&exact(b)).unwrap(),
                    (exact(a / b), exact(a % b))
                );
            }
        }
    }

    #[test]
    fn division_corrects_quotient_estimates_and_handles_wide_operands() {
        // The leading two dividend/divisor limbs suggest 1, but the low
        // limb makes the dividend smaller: the subtraction must add back.
        for high in [1, 0x8000_0000, u32::MAX] {
            let b = JsBigInt::from_sign_magnitude(false, vec![1, 0, high]).unwrap();
            let r = b.checked_sub(&JsBigInt::from(1)).unwrap();
            let q = JsBigInt::from_sign_magnitude(false, vec![0, 1]).unwrap();
            let a = b.checked_mul(&q).unwrap().checked_add(&r).unwrap();
            assert_eq!(a.checked_div_rem(&b).unwrap(), (q, r));
        }
        for high in [1, 0x8000_0000, u32::MAX] {
            let mut limbs: Vec<u32> = (0..406)
                .map(|i| (i as u32).wrapping_mul(0x6f28_9e31))
                .collect();
            limbs.push(high);
            let b = JsBigInt::from_sign_magnitude(false, limbs).unwrap();
            let q = JsBigInt::from_sign_magnitude(false, vec![u32::MAX; 205]).unwrap();
            let r = b.checked_sub(&JsBigInt::from(1)).unwrap();
            let a = b.checked_mul(&q).unwrap().checked_add(&r).unwrap();
            for (a, b, q, r) in [
                (a.clone(), b.clone(), q.clone(), r.clone()),
                (a.negated(), b.clone(), q.negated(), r.negated()),
                (a.clone(), b.negated(), q.negated(), r.clone()),
                (a.negated(), b.negated(), q.clone(), r.negated()),
            ] {
                assert_eq!(a.checked_div_rem(&b).unwrap(), (q, r));
            }
        }
        assert_eq!(
            JsBigInt::from(0).checked_div_rem(&JsBigInt::from(0)),
            Err(BigIntError::DivisionByZero)
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
