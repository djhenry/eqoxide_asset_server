//! Value-only terrain arithmetic for a conditional 53-bit, toward-zero profile.
//!
//! These allocation-free operators use integer arithmetic rather than the host's
//! floating-point mode. They accept finite binary32 inputs, including signed
//! zeros and subnormals. They neither select a bake profile nor certify the mode
//! active at any native terrain entry, and do not emulate exception/status flags.

use std::cmp::Ordering;

/// Explicit refusal of a nonfinite operand; no NaN or infinity policy is implied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NonFiniteOperand {
    #[error("first terrain endpoint is not finite")]
    FirstEndpoint,
    #[error("second terrain endpoint is not finite")]
    SecondEndpoint,
    #[error("terrain midpoint sample is not finite")]
    MidpointSample,
    #[error("terrain tolerance is not finite")]
    Tolerance,
}

// Magnitudes are integers in units of 2^-150. Finite inputs are bounded by
// (2^24 - 1) << 254 < 2^278. Endpoint sums and mean/sample differences are
// strictly below 2^279: a rounded mean is no larger in magnitude than the
// largest endpoint. Five limbs provide 320 bits, so reachable additions cannot
// carry out. Subtraction is used only after ordering the two magnitudes.
// These bounds concern these private call paths, not arbitrary 320-bit inputs.
#[derive(Clone, Copy, Default)]
struct Magnitude([u64; 5]);

impl Magnitude {
    fn from_binary32(bits: u32) -> Self {
        let exponent = (bits >> 23) & 0xff;
        let fraction = bits & 0x7fffff;
        let (coefficient, shift) = if exponent == 0 {
            (fraction, 1)
        } else {
            (fraction | 0x800000, exponent as usize)
        };
        let mut result = Self::default();
        let word = shift / 64;
        let offset = shift % 64;
        result.0[word] = u64::from(coefficient) << offset;
        if offset != 0 && word + 1 < 5 {
            result.0[word + 1] = u64::from(coefficient) >> (64 - offset);
        }
        result
    }

    fn compare(self, other: Self) -> Ordering {
        self.0.iter().rev().cmp(other.0.iter().rev())
    }

    fn add(self, other: Self) -> Self {
        let mut result = Self::default();
        let mut carry = false;
        for (index, word) in result.0.iter_mut().enumerate() {
            let (sum, first) = self.0[index].overflowing_add(other.0[index]);
            let (sum, second) = sum.overflowing_add(u64::from(carry));
            *word = sum;
            carry = first || second;
        }
        debug_assert!(!carry);
        result
    }

    fn subtract(self, smaller: Self) -> Self {
        let mut result = Self::default();
        let mut borrow = false;
        for (index, word) in result.0.iter_mut().enumerate() {
            let (difference, first) = self.0[index].overflowing_sub(smaller.0[index]);
            let (difference, second) = difference.overflowing_sub(u64::from(borrow));
            *word = difference;
            borrow = first || second;
        }
        debug_assert!(!borrow);
        result
    }

    fn bit_length(self) -> usize {
        for index in (0..5).rev() {
            if self.0[index] != 0 {
                return index * 64 + 64 - self.0[index].leading_zeros() as usize;
            }
        }
        0
    }

    fn truncate_53(&mut self) {
        let discard = self.bit_length().saturating_sub(53);
        let words = discard / 64;
        self.0[..words].fill(0);
        let remainder = discard % 64;
        if remainder != 0 {
            self.0[words] &= u64::MAX << remainder;
        }
    }

    fn half(self) -> Self {
        // Only internal_mean calls half, on an endpoint sum. Every input
        // numerator is even, and truncation either preserves it or clears low
        // bits, so halving is exact. The result may be odd (half a minimum
        // subnormal); the later deviation is never halved again.
        debug_assert_eq!(self.0[0] & 1, 0);
        let mut result = Self::default();
        for (index, word) in result.0.iter_mut().enumerate() {
            *word = self.0[index] >> 1;
            if index + 1 < 5 {
                *word |= self.0[index + 1] << 63;
            }
        }
        result
    }

    fn shifted_u32(self, shift: usize) -> u32 {
        let word = shift / 64;
        let offset = shift % 64;
        let mut value = self.0[word] >> offset;
        if offset != 0 && word + 1 < 5 {
            value |= self.0[word + 1] << (64 - offset);
        }
        value as u32
    }
}

#[derive(Clone, Copy)]
struct Number {
    negative: bool,
    magnitude: Magnitude,
}

impl Number {
    fn input(value: f32, operand: NonFiniteOperand) -> Result<Self, NonFiniteOperand> {
        let bits = value.to_bits();
        if (bits >> 23) & 0xff == 0xff {
            return Err(operand);
        }
        Ok(Self {
            negative: bits >> 31 != 0,
            magnitude: Magnitude::from_binary32(bits),
        })
    }

    fn add_53rz(self, other: Self) -> Self {
        let mut result = if self.negative == other.negative {
            Self {
                negative: self.negative,
                magnitude: self.magnitude.add(other.magnitude),
            }
        } else {
            match self.magnitude.compare(other.magnitude) {
                Ordering::Greater => Self {
                    negative: self.negative,
                    magnitude: self.magnitude.subtract(other.magnitude),
                },
                Ordering::Less => Self {
                    negative: other.negative,
                    magnitude: other.magnitude.subtract(self.magnitude),
                },
                Ordering::Equal => Self {
                    negative: false,
                    magnitude: Magnitude::default(),
                },
            }
        };
        result.magnitude.truncate_53();
        result
    }

    fn stored_midpoint(self) -> f32 {
        let length = self.magnitude.bit_length();
        let unsigned = if length <= 24 {
            self.magnitude.shifted_u32(1)
        } else {
            let exponent = length - 24;
            // Only a rounded endpoint mean reaches this store. Exact averaging
            // cannot exceed the larger endpoint magnitude, and toward-zero
            // rounding cannot increase it. Thus length <= 278 and exponent
            // <= 254, even when both finite endpoints have maximum magnitude.
            debug_assert!(exponent < 255);
            ((exponent as u32) << 23) | (self.magnitude.shifted_u32(exponent) & 0x7fffff)
        };
        f32::from_bits(unsigned | (u32::from(self.negative) << 31))
    }
}

fn internal_mean(p: f32, q: f32) -> Result<Number, NonFiniteOperand> {
    let p = Number::input(p, NonFiniteOperand::FirstEndpoint)?;
    let q = Number::input(q, NonFiniteOperand::SecondEndpoint)?;
    let mut mean = p.add_53rz(q);
    mean.magnitude = mean.magnitude.half();
    Ok(mean)
}

/// Add endpoints with 53-bit significand rounding toward zero, halve exactly,
/// then store to binary32 toward zero. This preserves negative zero, including
/// a negative nonzero mean that underflows at the final binary32 store.
pub fn midpoint_53rz(p: f32, q: f32) -> Result<f32, NonFiniteOperand> {
    Ok(internal_mean(p, q)?.stored_midpoint())
}

/// Compare `abs(internal_mean - sample) > tolerance`, rounding the subtraction
/// separately to 53 bits toward zero. The internal mean is not first stored to
/// binary32. Finite negative tolerance is accepted as arithmetic input, not as
/// selection of a bake policy; negative zero compares equal to positive zero.
pub fn deviation_exceeds_53rz(
    p: f32,
    q: f32,
    sample: f32,
    tolerance: f32,
) -> Result<bool, NonFiniteOperand> {
    let mean = internal_mean(p, q)?;
    let mut sample = Number::input(sample, NonFiniteOperand::MidpointSample)?;
    let tolerance = Number::input(tolerance, NonFiniteOperand::Tolerance)?;
    sample.negative = !sample.negative;
    let deviation = mean.add_53rz(sample).magnitude;
    Ok(
        (tolerance.negative && tolerance.magnitude.bit_length() != 0)
            || deviation.compare(tolerance.magnitude) == Ordering::Greater,
    )
}
