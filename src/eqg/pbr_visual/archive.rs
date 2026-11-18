use super::*;
use libeq_pfs::PfsReader;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};

pub(super) struct Snapshot {
    file: File,
    path: PathBuf,
    len: u64,
    pub hash: String,
    cap: usize,
}
impl Snapshot {
    pub fn open(path: &Path, cap: usize) -> Result<Self> {
        ensure!(
            std::fs::metadata(path)?.is_file(),
            "input must be a regular file"
        );
        let file = File::open(path)?;
        let meta = file.metadata()?;
        ensure!(
            meta.is_file() && meta.len() <= cap as u64,
            "regular input byte limit exceeded"
        );
        let mut s = Self {
            file,
            path: path.to_owned(),
            len: meta.len(),
            hash: String::new(),
            cap,
        };
        s.hash = s.digest()?;
        Ok(s)
    }
    fn digest(&mut self) -> Result<String> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut h = blake3::Hasher::new();
        let mut n = 0usize;
        let mut buf = [0u8; 65536];
        loop {
            let read = self.file.read(&mut buf)?;
            if read == 0 {
                break;
            }
            charge(&mut n, read, self.cap, "file byte")?;
            h.update(&buf[..read]);
        }
        ensure!(n as u64 == self.len, "input changed length");
        Ok(h.finalize().to_hex().to_string())
    }
    pub fn bytes(&mut self) -> Result<Vec<u8>> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut b = Vec::new();
        self.file
            .by_ref()
            .take(self.cap as u64 + 1)
            .read_to_end(&mut b)?;
        ensure!(
            b.len() as u64 == self.len && hash(&b) == self.hash,
            "input changed during read"
        );
        Ok(b)
    }
    pub fn verify(&mut self) -> Result<()> {
        ensure!(
            self.file.metadata()?.len() == self.len,
            "input changed length"
        );
        ensure!(
            self.digest()? == self.hash,
            "input changed on original handle"
        );
        let reopened = Self::open(&self.path, self.cap)?;
        ensure!(
            reopened.len == self.len && reopened.hash == self.hash,
            "input changed at pathname"
        );
        Ok(())
    }
}
pub(super) struct Archive {
    pub snapshot: Snapshot,
    pfs: PfsReader<File>,
    pub members: BTreeMap<String, (String, usize)>,
    pub advertised_bytes: usize,
}
impl Archive {
    pub fn open(path: &Path, l: &EqgPbrLimits) -> Result<Self> {
        let mut snapshot = Snapshot::open(path, l.max_archive_bytes)?;
        snapshot.file.seek(SeekFrom::Start(0))?;
        let mut pfs = PfsReader::open(snapshot.file.try_clone()?)?;
        ensure!(
            pfs.archive_info()?.file_count as usize <= l.max_archive_members,
            "archive member count limit exceeded"
        );
        ensure!(
            pfs.directory_info()?.uncompressed_size as usize <= l.max_directory_bytes,
            "archive directory byte limit exceeded"
        );
        let names = pfs.filenames()?;
        ensure!(
            names.len() <= l.max_archive_members,
            "archive member count limit exceeded"
        );
        let mut members = BTreeMap::new();
        let mut name_bytes = 0;
        let mut advertised_bytes = 0;
        for n in names {
            ensure!(
                !n.is_empty() && n.len() <= l.max_name_bytes,
                "archive name byte limit exceeded"
            );
            charge(
                &mut name_bytes,
                n.len(),
                l.max_total_name_bytes,
                "archive total name",
            )?;
            let len = pfs
                .info(&n)?
                .context("archive member missing metadata")?
                .uncompressed_size as usize;
            ensure!(
                len <= l.max_member_bytes,
                "archive member byte limit exceeded"
            );
            charge(
                &mut advertised_bytes,
                len,
                l.max_archive_uncompressed_bytes,
                "all archive advertised byte",
            )?;
            ensure!(
                members.insert(n.to_ascii_lowercase(), (n, len)).is_none(),
                "ambiguous ASCII-case-equivalent archive members"
            );
        }
        Ok(Self {
            snapshot,
            pfs,
            members,
            advertised_bytes,
        })
    }
    pub fn exact(&self, name: &str, l: &EqgPbrLimits) -> Result<(String, usize)> {
        safe_name(name, l)?;
        self.members
            .get(&name.to_ascii_lowercase())
            .cloned()
            .with_context(|| format!("missing archive member {name:?}"))
    }
    pub fn read(&mut self, name: &str, cap: usize) -> Result<Vec<u8>> {
        let declared = self
            .members
            .get(&name.to_ascii_lowercase())
            .context("missing archive member")?
            .1;
        ensure!(declared <= cap, "selected member byte limit exceeded");
        let mut b = Vec::new();
        self.pfs
            .get_reader(name)?
            .context("selected member disappeared")?
            .take(cap as u64 + 1)
            .read_to_end(&mut b)?;
        ensure!(
            b.len() == declared && b.len() <= cap,
            "selected member declared/actual size mismatch"
        );
        Ok(b)
    }
}
pub(super) fn safe_name(n: &str, l: &EqgPbrLimits) -> Result<()> {
    ensure!(
        !n.is_empty()
            && n.len() <= l.max_name_bytes
            && n != "."
            && n != ".."
            && !n
                .bytes()
                .any(|b| b.is_ascii_control() || matches!(b, b'/' | b'\\' | b':')),
        "unsafe resource name"
    );
    Ok(())
}
