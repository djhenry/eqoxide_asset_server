use eqoxide_asset_server::eqg::terrain_arithmetic::{
    deviation_exceeds_53rz, midpoint_53rz, NonFiniteOperand,
};

#[test]
fn midpoint_stores_toward_zero_without_intermediate_f32_overflow() {
    let cases = [
        (0x3f800000, 0x3f800000, 0x3f800000),
        (0x3f800001, 0x3f800002, 0x3f800001),
        (0xbf800001, 0xbf800002, 0xbf800001),
        (0x7f7fffff, 0x7f7fffff, 0x7f7fffff),
        (0xff7fffff, 0xff7fffff, 0xff7fffff),
        (0x7f7fffff, 0x7f7ffffe, 0x7f7ffffe),
        (0xff7fffff, 0xff7ffffe, 0xff7ffffe),
        (0x00800000, 0x007fffff, 0x007fffff),
        (0x80800000, 0x807fffff, 0x807fffff),
        (1, 1, 1),
        (1, 0, 0),
        (0x80000001, 0, 0x80000000),
        (0x80000000, 0x80000000, 0x80000000),
        (0x80000000, 0, 0),
        (0x7f7fffff, 0xff7fffff, 0),
        (1, 0x80000001, 0),
    ];
    for (p, q, want) in cases {
        assert_eq!(
            midpoint_53rz(f32::from_bits(p), f32::from_bits(q))
                .unwrap()
                .to_bits(),
            want,
            "p={p:08x} q={q:08x}"
        );
    }
}

#[test]
fn deviation_retains_internal_mean_and_rounds_each_arithmetic_stage() {
    let cases = [
        (0x3f800000, 0x3f800001, 0, 0x3f800000, true),
        (0x40000000, 0x25c00000, 0, 0x3f800000, false),
        (0x3f800000, 0x3f800000, 0xa5400000, 0x3f800000, false),
        (0xbf800000, 0xbf800000, 0x25400000, 0x3f800000, false),
        (0x3f800000, 1, 0x3f000000, 0, false),
        (0x3f800000, 0x80000001, 0x3f000000, 0, true),
        (0xbf800000, 1, 0xbf000000, 0, true),
        (1, 0, 0, 0, true),
        (1, 0, 0, 1, false),
        (0, 0, 0xbf800000, 0x3f800000, false),
        (0, 0, 0xbf800001, 0x3f800000, true),
        (0, 0, 0, 0x80000000, false),
        (0, 0, 0, 0x80000001, true),
        (0x7f7fffff, 0x7f7fffff, 0xff7fffff, 0x7f7fffff, true),
        (0x7f7fffff, 1, 0x7effffff, 0, false),
        (0x7f7fffff, 0x80000001, 0x7effffff, 0, true),
    ];
    for (p, q, sample, tolerance, want) in cases {
        assert_eq!(
            deviation_exceeds_53rz(
                f32::from_bits(p),
                f32::from_bits(q),
                f32::from_bits(sample),
                f32::from_bits(tolerance),
            ),
            Ok(want),
            "p={p:08x} q={q:08x} m={sample:08x} t={tolerance:08x}"
        );
    }
}

#[test]
fn nonfinite_operands_are_explicit_errors_instead_of_successful_answers() {
    for bits in [0x7f800000, 0xff800000, 0x7fc00001, 0x7f800001] {
        let value = f32::from_bits(bits);
        assert_eq!(
            midpoint_53rz(value, 0.0),
            Err(NonFiniteOperand::FirstEndpoint)
        );
        assert_eq!(
            midpoint_53rz(0.0, value),
            Err(NonFiniteOperand::SecondEndpoint)
        );
        assert_eq!(
            deviation_exceeds_53rz(value, 0.0, 0.0, 0.0),
            Err(NonFiniteOperand::FirstEndpoint)
        );
        assert_eq!(
            deviation_exceeds_53rz(0.0, value, 0.0, 0.0),
            Err(NonFiniteOperand::SecondEndpoint)
        );
        assert_eq!(
            deviation_exceeds_53rz(0.0, 0.0, value, 0.0),
            Err(NonFiniteOperand::MidpointSample)
        );
        assert_eq!(
            deviation_exceeds_53rz(0.0, 0.0, 0.0, value),
            Err(NonFiniteOperand::Tolerance)
        );
    }
}

// These sweeps derive expected encodings from binary32 spacing and powers of
// two, without reproducing the implementation's fixed-width integer arithmetic.
#[test]
fn midpoint_carry_and_cancellation_cover_every_normal_exponent() {
    for exponent in 1..=254 {
        for fraction in [1, 0x400001, 0x7fffff] {
            for sign in [0, 0x80000000] {
                let bits = sign | (exponent << 23) | fraction;
                let p = f32::from_bits(bits);
                assert_eq!(
                    midpoint_53rz(p, p).unwrap().to_bits(),
                    bits,
                    "doubling then halving {bits:08x}"
                );
                assert_eq!(
                    midpoint_53rz(p, f32::from_bits(bits ^ 0x80000000))
                        .unwrap()
                        .to_bits(),
                    0,
                    "exact cancellation {bits:08x}"
                );

                // Adjacent values within one binade differ by 2^(exponent-150).
                // Their difference's mean is 2^(exponent-151); the first binade
                // underflows to signed zero, then subnormal stores become exact.
                let adjacent = f32::from_bits((bits - 1) ^ 0x80000000);
                let magnitude = match exponent {
                    1 => 0,
                    2..=24 => 1 << (exponent - 2),
                    _ => (exponent - 24) << 23,
                };
                for (p, q) in [(p, adjacent), (adjacent, p)] {
                    assert_eq!(
                        midpoint_53rz(p, q).unwrap().to_bits(),
                        sign | magnitude,
                        "adjacent cancellation {bits:08x}"
                    );
                }
            }
        }
    }
}

#[test]
fn midpoint_wide_gaps_cover_input_and_store_word_boundaries() {
    for exponent in 2..=254 {
        for fraction in [0, 1, 0x7fffff] {
            for sign in [0, 0x80000000] {
                let bits = sign | (exponent << 23) | fraction;
                let p = f32::from_bits(bits);
                let half_magnitude = ((exponent - 1) << 23) | fraction;
                // The exact half is representable. A same-sign minimum
                // subnormal cannot reach the next binary32 store; an opposite
                // one lies below this store and selects its predecessor.
                for (tiny_sign, magnitude) in [
                    (sign, half_magnitude),
                    (sign ^ 0x80000000, half_magnitude - 1),
                ] {
                    let tiny = f32::from_bits(tiny_sign | 1);
                    for (p, q) in [(p, tiny), (tiny, p)] {
                        assert_eq!(
                            midpoint_53rz(p, q).unwrap().to_bits(),
                            sign | magnitude,
                            "wide gap p={bits:08x} q={:08x}",
                            tiny.to_bits()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn deviation_wide_gap_borrow_preserves_first_rounding_deficit() {
    for exponent in 2..=254 {
        for sign in [0, 0x80000000] {
            let p = f32::from_bits(sign | (exponent << 23));
            let tiny = f32::from_bits((sign ^ 0x80000000) | 1);
            let sample = f32::from_bits(sign | ((exponent - 1) << 23));
            // For p = 2^(exponent-127), subtracting a minimum subnormal
            // borrows through the low bits. With exponent >= 32, the 53-bit
            // rounded mean falls below p/2 by exactly 2^(exponent-181).
            // For smaller exponents the exact deficit is half a subnormal.
            let thresholds = if exponent < 32 {
                [(0, true), (1, false), (0x80000000, true)]
            } else {
                let deficit = if exponent <= 54 {
                    1 << (exponent - 32)
                } else {
                    (exponent - 54) << 23
                };
                [(deficit - 1, true), (deficit, false), (deficit + 1, false)]
            };
            for (tolerance, want) in thresholds {
                assert_eq!(
                    deviation_exceeds_53rz(p, tiny, sample, f32::from_bits(tolerance)),
                    Ok(want),
                    "first rounding exponent={exponent} sign={sign:08x} tolerance={tolerance:08x}"
                );
            }
        }
    }
}

#[test]
fn deviation_rounds_a_subnormal_tail_separately_at_every_wide_exponent() {
    for exponent in 30..=254 {
        for fraction in [0, 1, 0x7fffff] {
            for sign in [0, 0x80000000] {
                let sample_bits = sign | (exponent << 23) | fraction;
                let sample = f32::from_bits(sample_bits);
                let endpoint = f32::from_bits((sign ^ 0x80000000) | 1);
                let tolerance = (exponent << 23) | fraction;
                // The internal mean has magnitude half a minimum subnormal
                // and opposes sample. Subtraction adds this positive tail to
                // abs(sample); its separate 53-bit rounding discards the tail.
                for (tolerance, want) in [(tolerance - 1, true), (tolerance, false)] {
                    assert_eq!(
                        deviation_exceeds_53rz(endpoint, 0.0, sample, f32::from_bits(tolerance)),
                        Ok(want),
                        "second rounding sample={sample_bits:08x} tolerance={tolerance:08x}"
                    );
                }
            }
        }
    }
}
