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
