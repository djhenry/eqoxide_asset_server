use libeq_wld::parser::WldDoc;

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
pub fn region(flags: u32, reference: i32, word_visibility: bool) -> Vec<u8> {
    let mut values = vec![0, flags, 0];
    values.extend([0; 7]);
    values.push(u32::from(word_visibility));
    let mut body = words(&values);
    if word_visibility {
        body.extend(1u16.to_le_bytes());
        body.extend(513u16.to_le_bytes());
    }
    if flags & 1 != 0 {
        body.extend(words(&[0; 4]));
    }
    body.extend(words(&[0]));
    if flags & 0x100 != 0 {
        body.extend(reference.to_le_bytes());
    }
    body
}
pub fn modern() -> Vec<u8> {
    // Empty modern body: six initial words, center, three parameters, seven floats,
    // then ten zero u16 pool counts/exponent. No geometry is needed by this API.
    words(&[0; 19]).into_iter().chain([0u8; 20]).collect()
}
pub fn tree(nodes: &[[i32; 3]]) -> Vec<u8> {
    let mut body = words(&[0, nodes.len() as u32]);
    for [front, back, region] in nodes {
        body.extend(words(&[0; 4]));
        body.extend(words(&[*region as u32, *front as u32, *back as u32]));
    }
    body
}
pub fn bytes(fragments: &[(u32, Vec<u8>)], regions: u32) -> Vec<u8> {
    let mut body = words(&[
        0x54503d02,
        0x15500,
        fragments.len() as u32,
        regions,
        0,
        0,
        0,
    ]);
    for (kind, fragment) in fragments {
        body.extend(words(&[fragment.len() as u32, *kind]));
        body.extend(fragment);
    }
    body
}
pub fn document(fragments: &[(u32, Vec<u8>)], regions: u32) -> WldDoc {
    WldDoc::parse(&bytes(fragments, regions)).unwrap()
}
pub fn asymmetric() -> WldDoc {
    document(
        &[
            (0x36, modern()),
            (0x22, region(0x181, 1, false)),
            (0x22, vec![]),
            (0x36, modern()),
            (0x21, tree(&[[2, 0, 1], [0, 0, 2], [3, 0, 3]])),
            (0x22, region(0x101, 1, true)),
            (0x36, modern()),
            (0x22, region(0x81, 0, false)),
        ],
        4,
    )
}
pub fn older_geometry() -> Vec<u8> {
    let mut body = words(&[0; 17]);
    body[0..4].copy_from_slice(&(-1i32).to_le_bytes());
    body
}
pub fn alternate_2c() -> Vec<u8> {
    let mut body = words(&[0, 0, 0]);
    body.extend(0u16.to_le_bytes());
    body
}
