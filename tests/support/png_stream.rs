pub fn chunk(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    let mut out = (bytes.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(bytes);
    out.extend_from_slice(&crc32fast::hash(&out[4..]).to_be_bytes());
    out
}

pub fn stored_zlib(raw: &[u8]) -> Vec<u8> {
    assert!(raw.len() <= u16::MAX as usize);
    let length = raw.len() as u16;
    let mut out = vec![0x78, 0x01, 0x01];
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&(!length).to_le_bytes());
    out.extend_from_slice(raw);
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in raw {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

pub fn png_from_zlib(
    width: u32,
    height: u32,
    depth: u8,
    color: u8,
    interlace: u8,
    streams: &[&[u8]],
) -> Vec<u8> {
    let mut header = width.to_be_bytes().to_vec();
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[depth, color, 0, 0, interlace]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &header));
    if color == 3 {
        out.extend(chunk(b"PLTE", &[0, 0, 0, 255, 255, 255]));
    }
    for bytes in streams {
        out.extend(chunk(b"IDAT", bytes));
    }
    out.extend(chunk(b"IEND", &[]));
    out
}

pub fn png(
    width: u32,
    height: u32,
    depth: u8,
    color: u8,
    interlace: u8,
    raw: &[u8],
    compressed_padding: &[u8],
) -> Vec<u8> {
    let mut bytes = stored_zlib(raw);
    bytes.extend_from_slice(compressed_padding);
    png_from_zlib(width, height, depth, color, interlace, &[&bytes])
}
