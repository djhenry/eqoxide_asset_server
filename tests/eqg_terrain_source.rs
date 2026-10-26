use eqoxide_asset_server::eqg::terrain_source::{
    prepare_explicit_project_53rz, NamedSource, SourceError, SourceLimits, ValueOrigin,
};

const PROJECT: &[u8] = b"EQTZP\n*NAME Custom\n*QUADSPERTILE 32\n*UNITSPERVERT 5.00\n";
fn dat(tiles: &[([i32; 2], f32, u8)]) -> Vec<u8> {
    let mut b = Vec::new();
    for w in [20u32, 55, 7] {
        b.extend(w.to_le_bytes());
    }
    b.extend(b"independent-header\0");
    b.extend((tiles.len() as u32).to_le_bytes());
    for (coord, height, flag) in tiles {
        for w in [coord[0] as u32, coord[1] as u32, 123] {
            b.extend(w.to_le_bytes());
        }
        for grid in 0..3 {
            for _ in 0..1089 {
                b.extend(
                    if grid == 0 {
                        height.to_bits()
                    } else {
                        0x7fc01234u32
                    }
                    .to_le_bytes(),
                );
            }
        }
        b.extend([*flag; 1024]);
        for _ in 0..7 {
            b.extend(0u32.to_le_bytes());
        }
    }
    b
}
fn descriptor(bytes: &[u8]) -> NamedSource<'_> {
    NamedSource {
        name: b"wrong-stem.zon",
        bytes,
    }
}
fn provider(bytes: &[u8]) -> NamedSource<'_> {
    NamedSource {
        name: b"Custom.dat",
        bytes,
    }
}

#[test]
fn explicit_name_binding_defaults_direct_height_bits_and_opaque_auxiliary_grids() {
    let bytes = dat(&[([2, -3], -0.0, 0), ([8, 9], 7.0, 0)]);
    let out = prepare_explicit_project_53rz(
        descriptor(PROJECT),
        &[provider(&bytes)],
        &SourceLimits::default(),
    )
    .unwrap();
    assert_eq!(out.dat_name, b"Custom.dat");
    assert_eq!(out.descriptor_name, b"wrong-stem.zon");
    assert_eq!(out.metadata.name.value, b"Custom");
    assert_eq!(out.metadata.quads.value, 32);
    assert_eq!(out.metadata.units.value.to_bits(), 0x40a00000);
    assert_eq!(out.metadata.tile_width.to_bits(), 0x43200000);
    assert_eq!(out.metadata.version.value, 1);
    assert_eq!(out.metadata.version.origin, ValueOrigin::Default);
    assert_eq!(out.metadata.name.origin, ValueOrigin::Token(12));
    assert_eq!(out.metadata.unmodeled_values, 0);
    assert_eq!(out.dat.header().name.bytes, b"independent-header");
    assert_eq!(
        out.dat.tiles()[0].grid_bytes[1][..4],
        0x7fc01234u32.to_le_bytes()
    );
    assert_eq!(out.prepared.len(), 2);
    assert_eq!(out.prepared[0].coordinate(), [2, -3]);
    assert_eq!(
        out.prepared[0].heights()[0][0].to_bits(),
        (-0.0f32).to_bits()
    );
    assert_eq!(out.prepared[1].heights()[4][7], 7.0);
    assert_eq!(
        out.prepared[1].triangles(),
        [[1056, 32, 0], [32, 1056, 1088]]
    );
    assert_eq!(out.project.source().as_ptr(), PROJECT.as_ptr());
    assert_eq!(out.dat.source().as_ptr(), bytes.as_ptr());
}

#[test]
fn native_arity_consumption_and_last_effective_assignments_keep_value_offsets() {
    let project=b"EQTZP *NAME first *QUADSPERTILE 16 *UNITSPERVERT 7 *VERSION 2 *MIN_EXTENTS *NAME decoy 0 *NAME Custom *QUADSPERTILE +032 *UNITSPERVERT 005.00 *VERSION +1";
    let bytes = dat(&[]);
    let out = prepare_explicit_project_53rz(
        descriptor(project),
        &[provider(&bytes)],
        &SourceLimits::default(),
    )
    .unwrap();
    assert_eq!(out.metadata.name.value, b"Custom");
    assert_eq!(out.metadata.quads.value, 32);
    assert_eq!(out.metadata.units.value, 5.0);
    assert_eq!(out.metadata.unmodeled_values, 3);
    let off = project.windows(2).rposition(|s| s == b"+1").unwrap();
    assert_eq!(out.metadata.version.origin, ValueOrigin::Token(off));
}

#[test]
fn metadata_admission_precedes_dimension_dependent_dat_parsing() {
    for (project, key) in [
        (
            b"EQTZP *NAME Custom *QUADSPERTILE 16 *UNITSPERVERT 5".as_slice(),
            "*QUADSPERTILE",
        ),
        (
            b"EQTZP *NAME Custom *QUADSPERTILE 32 *UNITSPERVERT 5.25".as_slice(),
            "*UNITSPERVERT",
        ),
        (
            b"EQTZP *NAME Custom *QUADSPERTILE 32 *UNITSPERVERT 524289".as_slice(),
            "*UNITSPERVERT",
        ),
        (
            b"EQTZP *NAME Custom *QUADSPERTILE 32 *UNITSPERVERT 5 *VERSION 4".as_slice(),
            "*VERSION",
        ),
        (
            b"EQTZP *NAME ../Custom *QUADSPERTILE 32 *UNITSPERVERT 5".as_slice(),
            "*NAME",
        ),
    ] {
        assert!(
            matches!(prepare_explicit_project_53rz(descriptor(project),&[provider(&[])],&SourceLimits::default()),Err(SourceError::Metadata{key:k,..}) if k==key)
        );
    }
    assert!(matches!(
        prepare_explicit_project_53rz(
            descriptor(b"EQTZP *FUTURE *NAME Custom"),
            &[],
            &SourceLimits::default()
        ),
        Err(SourceError::UnknownDirective { offset: 6 })
    ));
    assert!(matches!(
        prepare_explicit_project_53rz(
            descriptor(b"EQTZP *NAME Custom *QUADSPERTILE 32"),
            &[],
            &SourceLimits::default()
        ),
        Err(SourceError::MissingField("*UNITSPERVERT"))
    ));
    assert!(matches!(
        prepare_explicit_project_53rz(
            descriptor(b"EQTZP *MIN_EXTENTS 0 0"),
            &[],
            &SourceLimits::default()
        ),
        Err(SourceError::TruncatedField { offset: 6 })
    ));
}

#[test]
fn exact_namespace_never_uses_descriptor_stem_case_folding_or_duplicate_fallback() {
    let bytes = dat(&[]);
    let limits = SourceLimits::default();
    assert!(matches!(
        prepare_explicit_project_53rz(
            descriptor(PROJECT),
            &[NamedSource {
                name: b"wrong-stem.dat",
                bytes: &bytes
            }],
            &limits
        ),
        Err(SourceError::MissingDat)
    ));
    assert!(matches!(
        prepare_explicit_project_53rz(
            descriptor(PROJECT),
            &[NamedSource {
                name: b"custom.dat",
                bytes: &bytes
            }],
            &limits
        ),
        Err(SourceError::MissingDat)
    ));
    assert!(matches!(
        prepare_explicit_project_53rz(
            descriptor(PROJECT),
            &[provider(&bytes), provider(&bytes)],
            &limits
        ),
        Err(SourceError::AmbiguousDat)
    ));
}

#[test]
fn byte_budgets_include_unused_resources_names_and_suffixes() {
    let bytes = dat(&[]);
    let entries = [
        provider(&bytes),
        NamedSource {
            name: b"unused",
            bytes: &[1; 8],
        },
    ];
    assert!(matches!(
        prepare_explicit_project_53rz(
            descriptor(PROJECT),
            &entries,
            &SourceLimits {
                max_namespace_bytes: bytes.len(),
                ..SourceLimits::default()
            }
        ),
        Err(SourceError::Budget {
            resource: "namespace bytes",
            ..
        })
    ));
    assert!(matches!(
        prepare_explicit_project_53rz(
            descriptor(PROJECT),
            &entries,
            &SourceLimits {
                max_resources: 1,
                ..SourceLimits::default()
            }
        ),
        Err(SourceError::Budget {
            resource: "resources",
            ..
        })
    ));
    assert!(matches!(
        prepare_explicit_project_53rz(
            descriptor(PROJECT),
            &[provider(&bytes)],
            &SourceLimits {
                max_name_bytes: 1,
                ..SourceLimits::default()
            }
        ),
        Err(SourceError::Budget {
            resource: "resource name bytes",
            ..
        })
    ));
    assert!(matches!(
        prepare_explicit_project_53rz(
            descriptor(PROJECT),
            &[provider(&bytes)],
            &SourceLimits {
                max_project_bytes: PROJECT.len() - 1,
                ..SourceLimits::default()
            }
        ),
        Err(SourceError::Budget {
            resource: "project bytes",
            ..
        })
    ));
    let mut project = PROJECT.to_vec();
    project.extend(b"\0opaque");
    assert!(matches!(
        prepare_explicit_project_53rz(
            descriptor(&project),
            &[provider(&bytes)],
            &SourceLimits::default()
        ),
        Err(SourceError::Suffix {
            resource: "project",
            bytes: 6
        })
    ));
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(matches!(
        prepare_explicit_project_53rz(
            descriptor(PROJECT),
            &[provider(&extra)],
            &SourceLimits::default()
        ),
        Err(SourceError::Suffix {
            resource: "DAT",
            bytes: 1
        })
    ));
}

#[test]
fn finite_height_full_count_and_source_bit_integrity_are_required() {
    let mut bytes = dat(&[([0, 0], 2.0, 0), ([9, 9], 3.0, 0)]);
    let raw = libeq_eqg::terrain_dat::parse_packed_v20(&bytes, 32).unwrap();
    let start = raw.tiles()[1].source_range.start + 12 + 4 * 500;
    bytes[start..start + 4].copy_from_slice(&0x7fc05678u32.to_le_bytes());
    let before = bytes.clone();
    assert!(matches!(
        prepare_explicit_project_53rz(
            descriptor(PROJECT),
            &[provider(&bytes)],
            &SourceLimits::default()
        ),
        Err(SourceError::NonFiniteHeight {
            tile: 1,
            vertex: 500
        })
    ));
    assert_eq!(bytes, before);
    bytes[start..start + 4].copy_from_slice(&3f32.to_bits().to_le_bytes());
    bytes.pop();
    assert!(matches!(
        prepare_explicit_project_53rz(
            descriptor(PROJECT),
            &[provider(&bytes)],
            &SourceLimits::default()
        ),
        Err(SourceError::Dat(_))
    ));
}

#[test]
fn full_content_and_selected_names_bind_identity_without_unused_namespace_data() {
    let bytes = dat(&[([0, 0], 2.0, 0)]);
    let limits = SourceLimits::default();
    let a =
        prepare_explicit_project_53rz(descriptor(PROJECT), &[provider(&bytes)], &limits).unwrap();
    let b = prepare_explicit_project_53rz(
        descriptor(PROJECT),
        &[
            provider(&bytes),
            NamedSource {
                name: b"unused",
                bytes: b"irrelevant",
            },
        ],
        &limits,
    )
    .unwrap();
    assert_eq!(a.source_id, b.source_id);
    let c = prepare_explicit_project_53rz(
        NamedSource {
            name: b"another.zon",
            bytes: PROJECT,
        },
        &[provider(&bytes)],
        &limits,
    )
    .unwrap();
    assert_ne!(a.source_id, c.source_id);
    assert_eq!(a.descriptor_hash, c.descriptor_hash);
    let mut terminated = PROJECT.to_vec();
    terminated.push(0);
    let d = prepare_explicit_project_53rz(descriptor(&terminated), &[provider(&bytes)], &limits)
        .unwrap();
    assert_ne!(a.source_id, d.source_id);
    assert_ne!(a.descriptor_hash, d.descriptor_hash);
    assert_eq!(a.dat_hash, d.dat_hash);
}
