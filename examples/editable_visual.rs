//! Export portable authoring content; does not reimport or activate an edited scene.
use anyhow::{bail, ensure, Context, Result};
use eqoxide_asset_server::editable_visual::{export_editable_visual, EditableVisualLimits};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

const USAGE: &str = "usage: editable_visual INPUT.glb NEW_DIRECTORY";

fn arguments(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<(PathBuf, PathBuf)> {
    let mut args = args.into_iter();
    let source = args.next().context(USAGE)?;
    let destination = args.next().context(USAGE)?;
    ensure!(args.next().is_none(), USAGE);
    ensure!(
        !source.to_string_lossy().starts_with('-')
            && !destination.to_string_lossy().starts_with('-'),
        USAGE
    );
    Ok((source.into(), destination.into()))
}

fn bounded_read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let file = File::open(path).context("open reference GLB")?;
    ensure!(
        file.metadata()?.is_file(),
        "reference GLB must be a regular file"
    );
    ensure!(
        file.metadata()?.len() <= u64::try_from(limit)?,
        "reference GLB byte limit exceeded"
    );
    let read_limit = u64::try_from(limit)?
        .checked_add(1)
        .context("reference read limit overflow")?;
    let mut bytes = Vec::new();
    file.take(read_limit).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "reference GLB byte limit exceeded");
    Ok(bytes)
}

struct ReservedDirectory {
    path: PathBuf,
    complete: bool,
}
impl Drop for ReservedDirectory {
    fn drop(&mut self) {
        if !self.complete {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn publish(files: &BTreeMap<String, Vec<u8>>, destination: &Path) -> Result<()> {
    ensure!(
        files.contains_key("authoring.json"),
        "authoring completion marker missing"
    );
    for path in files.keys() {
        ensure!(
            Path::new(path)
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
            "unsafe generated authoring path"
        );
    }
    match fs::symlink_metadata(destination) {
        Ok(_) => bail!("authoring destination already exists"),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).context("inspect authoring destination"),
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let staging = tempfile::Builder::new()
        .prefix(".editable-visual-")
        .tempdir_in(parent)?;
    for (path, bytes) in files {
        let staged = staging.path().join(path);
        if let Some(parent) = staged.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    // create_dir is a no-replace reservation, including an existing empty directory
    // or symlink. Populating it is not directory-atomic: consumers must require the
    // authoring.json completion marker, which is moved only after every payload.
    fs::create_dir(destination).context("reserve new authoring destination")?;
    let mut reserved = ReservedDirectory {
        path: destination.into(),
        complete: false,
    };
    for path in files.keys().filter(|p| p.as_str() != "authoring.json") {
        let target = destination.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(staging.path().join(path), target)?;
    }
    fs::rename(
        staging.path().join("authoring.json"),
        destination.join("authoring.json"),
    )?;
    reserved.complete = true;
    Ok(())
}

fn main() -> Result<()> {
    let (source, destination) = arguments(std::env::args_os().skip(1))?;
    let limits = EditableVisualLimits::default();
    let source = bounded_read(&source, limits.max_glb_bytes)?;
    let bundle = export_editable_visual(&source, &limits)?;
    publish(&bundle.files, &destination)?;
    // Output errors after the completion marker do not roll back the new bundle.
    serde_json::to_writer_pretty(io::stdout().lock(), &bundle.report)?;
    println!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arguments_are_exact() {
        for args in [
            vec![],
            vec!["one"],
            vec!["one", "two", "three"],
            vec!["--help", "two"],
        ] {
            assert!(arguments(args.into_iter().map(Into::into)).is_err());
        }
        assert!(arguments(["one", "two"].into_iter().map(Into::into)).is_ok());
    }
    #[test]
    fn publication_preserves_existing_paths_and_completes_new_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let files = BTreeMap::from([
            ("scene.bin".into(), vec![1, 2, 3]),
            ("images/image-000000.png".into(), vec![4]),
            ("authoring.json".into(), b"{}".to_vec()),
        ]);
        let output = dir.path().join("new");
        publish(&files, &output).unwrap();
        assert_eq!(fs::read(output.join("scene.bin")).unwrap(), [1, 2, 3]);
        assert!(output.join("authoring.json").is_file());
        assert!(publish(&files, &output).is_err());
        assert_eq!(fs::read(output.join("scene.bin")).unwrap(), [1, 2, 3]);
        let empty = dir.path().join("empty");
        fs::create_dir(&empty).unwrap();
        assert!(publish(&files, &empty).is_err());
        assert_eq!(fs::read_dir(empty).unwrap().count(), 0);
    }
    #[test]
    fn invalid_marker_or_path_creates_no_destination() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("new");
        assert!(publish(&BTreeMap::new(), &output).is_err());
        let files = BTreeMap::from([
            ("../escape".into(), vec![1]),
            ("authoring.json".into(), vec![2]),
        ]);
        assert!(publish(&files, &output).is_err());
        assert!(!output.exists());
    }
    #[cfg(unix)]
    #[test]
    fn destination_symlinks_are_never_replaced() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("absent");
        let link = dir.path().join("link");
        symlink(&target, &link).unwrap();
        assert!(publish(&BTreeMap::from([("authoring.json".into(), vec![1])]), &link).is_err());
        assert_eq!(fs::read_link(link).unwrap(), target);
    }
}
