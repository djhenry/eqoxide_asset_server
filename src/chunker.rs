use crate::cas::Cas;
use fastcdc::v2020::FastCDC;

pub const MIN: u32 = 16 * 1024;
pub const AVG: u32 = 64 * 1024;
pub const MAX: u32 = 256 * 1024;

pub fn chunk_into(cas: &Cas, bytes: &[u8]) -> std::io::Result<Vec<String>> {
    let mut hashes = Vec::new();
    let chunker = FastCDC::new(bytes, MIN, AVG, MAX);
    for chunk in chunker {
        let slice = &bytes[chunk.offset..chunk.offset + chunk.length];
        hashes.push(cas.put(slice)?);
    }
    Ok(hashes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_input_is_one_chunk_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let data = b"a small file under the min chunk size".to_vec();
        let hashes = chunk_into(&cas, &data).unwrap();
        assert_eq!(hashes.len(), 1);
        let reassembled: Vec<u8> =
            hashes.iter().flat_map(|h| cas.get(h).unwrap()).collect();
        assert_eq!(reassembled, data);
    }

    #[test]
    fn large_input_splits_and_reassembles_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        // 1 MiB of pseudo-random-ish data so CDC finds multiple boundaries
        let data: Vec<u8> = (0..(1024 * 1024)).map(|i| (i * 2654435761u64 as usize) as u8).collect();
        let hashes = chunk_into(&cas, &data).unwrap();
        assert!(hashes.len() > 1, "expected multiple chunks, got {}", hashes.len());
        let reassembled: Vec<u8> =
            hashes.iter().flat_map(|h| cas.get(h).unwrap()).collect();
        assert_eq!(reassembled, data);
    }

    #[test]
    fn identical_chunks_dedup_in_cas() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let block = vec![7u8; 200 * 1024]; // > MAX so it splits into >1 identical-ish region
        let h1 = chunk_into(&cas, &block).unwrap();
        let h2 = chunk_into(&cas, &block).unwrap();
        assert_eq!(h1, h2); // same content => same hash list
    }
}
