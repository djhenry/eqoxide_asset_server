use super::*;
fn pose(rotation: [f32; 3]) -> ZonePlacement {
    ZonePlacement {
        model_index: Some(0),
        name_offset: 0,
        position: [0.; 3],
        rotation,
        scale: 1.,
        extension_data: vec![],
    }
}
fn close(a: Vec3, b: Vec3) {
    assert!((a - b).length() < 0.00001, "{a:?} != {b:?}");
}
#[test]
fn quarter_turn_axes_follow_source_rotation_order() {
    let q = std::f32::consts::FRAC_PI_2;
    for (rotation, input, expected) in [
        ([q, 0., 0.], [1., 0., 0.], [0., 1., 0.]),
        ([0., q, 0.], [1., 0., 0.], [0., 0., -1.]),
        ([0., 0., q], [0., 1., 0.], [0., 0., 1.]),
    ] {
        close(
            placement_matrix(&pose(rotation))
                .unwrap()
                .transform_point3(convert(input)),
            convert(expected),
        );
    }
}
#[test]
fn noncommuting_rotations_apply_x_then_y_then_z_before_scale_translation() {
    let q = std::f32::consts::FRAC_PI_2;
    let mut p = pose([q, q, q]);
    p.scale = 2.;
    p.position = [10., 20., 30.];
    // (1,2,3) -> Rx (1,-3,2) -> Ry (2,-3,-1) -> Rz (3,2,-1).
    close(
        placement_matrix(&p)
            .unwrap()
            .transform_point3(convert([1., 2., 3.])),
        convert([16., 24., 28.]),
    );
}
#[test]
fn unsupported_scales_and_nonfinite_transforms_fail() {
    for scale in [0., -1., f32::NAN, f32::INFINITY] {
        let mut p = pose([0.; 3]);
        p.scale = scale;
        assert!(placement_matrix(&p).is_err());
    }
    let mut p = pose([0.; 3]);
    p.position[0] = f32::INFINITY;
    assert!(placement_matrix(&p).is_err());
}
#[test]
fn overflowing_world_positions_fail() {
    let mut p = pose([0.; 3]);
    p.scale = f32::MAX;
    assert!(validate_world_vertices(&placement_matrix(&p).unwrap(), &[[2., 0., 0.]]).is_err());
}
