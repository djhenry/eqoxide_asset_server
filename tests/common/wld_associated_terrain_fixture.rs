use libeq_wld::parser::WldDoc;
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
pub fn mesh(
    center: [f32; 3],
    flags: u32,
    positions: &[[i16; 3]],
    faces: &[(u16, [u16; 3])],
) -> Vec<u8> {
    let mut b = words(&[0, flags, 0, 0, 0, 0]);
    for v in center {
        b.extend(v.to_le_bytes());
    }
    b.extend(words(&[0; 10]));
    for v in [
        positions.len() as u16,
        0,
        0,
        0,
        faces.len() as u16,
        0,
        0,
        0,
        0,
        1,
    ] {
        b.extend(v.to_le_bytes());
    }
    for p in positions {
        for v in p {
            b.extend(v.to_le_bytes());
        }
    }
    for (f, indices) in faces {
        b.extend(f.to_le_bytes());
        for v in indices {
            b.extend(v.to_le_bytes());
        }
    }
    b
}
pub fn triangle(center: [f32; 3]) -> Vec<u8> {
    mesh(
        center,
        0x18003,
        &[[-2, 4, -6], [6, 4, -6], [-2, 10, -6]],
        &[(0, [0, 1, 2])],
    )
}
pub fn region(flags: u32, reference: i32) -> Vec<u8> {
    let word = flags & 0x80 == 0;
    let mut b = words(&[0, flags, 0, 0, 0, 0, 0, 0, 0, 0, u32::from(word)]);
    if word {
        b.extend(1u16.to_le_bytes());
        b.extend(513u16.to_le_bytes());
    }
    if flags & 1 != 0 {
        b.extend(words(&[0; 4]));
    }
    b.extend(words(&[0]));
    if flags & 0x100 != 0 {
        b.extend(reference.to_le_bytes());
    }
    b
}
pub fn tree(cycle: bool) -> Vec<u8> {
    words(&[0, 1, 0, 0, 0, 0, 1, u32::from(cycle), 0])
}
pub fn document(fragments: &[(u32, Vec<u8>)], regions: u32) -> WldDoc {
    let mut b = words(&[
        0x54503d02,
        0x15500,
        fragments.len() as u32,
        regions,
        0,
        0,
        0,
    ]);
    for (kind, body) in fragments {
        b.extend(words(&[body.len() as u32, *kind]));
        b.extend(body);
    }
    WldDoc::parse(&b).unwrap()
}
pub fn primary() -> WldDoc {
    document(
        &[
            (0x9999, vec![]),
            (0x36, mesh([0.; 3], 0, &[], &[])),
            (0x22, region(0x181, 7)),
            (0x22, region(0x81, 0)),
            (0x21, tree(false)),
            (0x22, region(0x101, 8)),
            (0x36, triangle([10., -20., 3.])),
            (0x36, triangle([100., 200., 300.])),
        ],
        3,
    )
}
