use libeq_wld::parser::WldDoc;

pub struct MeshFixture {
    pub flags: u32,
    pub center: [f32; 3],
    pub exponent: u16,
    pub animation: i32,
    pub positions: Vec<[i16; 3]>,
    pub faces: Vec<(u16, [u16; 3])>,
}
impl Default for MeshFixture {
    fn default() -> Self {
        Self {
            flags: 0x18003,
            center: [10., -20., 3.],
            exponent: 1,
            animation: 0,
            positions: vec![[-2, 4, -6], [6, 4, -6], [-2, 10, -6]],
            faces: vec![(0, [0, 1, 2])],
        }
    }
}
impl MeshFixture {
    pub fn bytes(&self, new_uvs: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        for v in [
            0,
            self.flags,
            i32::MAX as u32,
            self.animation as u32,
            (-4i32) as u32,
            123456,
        ] {
            bytes.extend(v.to_le_bytes());
        }
        for v in self.center {
            bytes.extend(v.to_le_bytes());
        }
        for v in [0u32; 3] {
            bytes.extend(v.to_le_bytes());
        }
        for v in [f32::NAN; 7] {
            bytes.extend(v.to_le_bytes());
        }
        for v in [
            self.positions.len() as u16,
            1,
            0,
            0,
            self.faces.len() as u16,
            0,
            0,
            0,
            0,
            self.exponent,
        ] {
            bytes.extend(v.to_le_bytes());
        }
        for p in &self.positions {
            for v in p {
                bytes.extend(v.to_le_bytes());
            }
        }
        if new_uvs {
            bytes.extend(f32::NAN.to_le_bytes());
            bytes.extend(f32::INFINITY.to_le_bytes());
        } else {
            bytes.extend(i16::MIN.to_le_bytes());
            bytes.extend(i16::MAX.to_le_bytes());
        }
        for (flags, indices) in &self.faces {
            bytes.extend(flags.to_le_bytes());
            for i in indices {
                bytes.extend(i.to_le_bytes());
            }
        }
        bytes
    }
}
pub fn document(fragments: &[(u32, Vec<u8>)], new_uvs: bool) -> WldDoc {
    let mut bytes = Vec::new();
    for v in [
        0x54503d02u32,
        if new_uvs { 0x1000c800 } else { 0x15500 },
        fragments.len() as u32,
        0,
        0,
        0,
        0,
    ] {
        bytes.extend(v.to_le_bytes());
    }
    for (kind, body) in fragments {
        bytes.extend((body.len() as u32).to_le_bytes());
        bytes.extend(kind.to_le_bytes());
        bytes.extend(body);
    }
    WldDoc::parse(&bytes).unwrap()
}
