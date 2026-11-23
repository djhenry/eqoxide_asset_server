//! Apply explicit standard material edits to original geometry, without package activation.
use anyhow::{ensure, Context, Result};
use eqoxide_asset_server::material_edit::{
    material_edit_report_json, publish_material_edits, reimport_material_edits, MaterialEditLimits,
};
use std::{
    ffi::OsString,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};
const USAGE:&str="usage: editable_materials --material material-NNNNNN [--material material-NNNNNN ...] [--] BUNDLE_DIRECTORY EDITED.glb OUTPUT.glb";
fn arguments(
    args: impl IntoIterator<Item = OsString>,
) -> Result<(PathBuf, PathBuf, PathBuf, Vec<String>)> {
    let mut args = args.into_iter();
    let mut ids = Vec::new();
    let mut paths = Vec::new();
    let mut options = true;
    while let Some(arg) = args.next() {
        if options && arg == "--" {
            options = false;
        } else if options && arg == "--material" {
            let id = args
                .next()
                .context(USAGE)?
                .into_string()
                .map_err(|_| anyhow::anyhow!(USAGE))?;
            ensure!(
                id.strip_prefix("material-").is_some_and(
                    |digits| digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_digit())
                ),
                USAGE
            );
            ids.push(id);
        } else {
            ensure!(!options || !arg.to_string_lossy().starts_with('-'), USAGE);
            paths.push(PathBuf::from(arg));
        }
    }
    ensure!(!ids.is_empty() && paths.len() == 3, USAGE);
    Ok((paths.remove(0), paths.remove(0), paths.remove(0), ids))
}
fn read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    ensure!(file.metadata()?.is_file(), "input must be a regular file");
    ensure!(
        file.metadata()?.len() <= u64::try_from(limit)?,
        "input byte limit exceeded"
    );
    let mut bytes = Vec::new();
    file.take(
        u64::try_from(limit)?
            .checked_add(1)
            .context("read limit overflow")?,
    )
    .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "input byte limit exceeded");
    Ok(bytes)
}
fn lexical(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut clean = PathBuf::new();
    for c in absolute.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                clean.pop();
            }
            _ => clean.push(c.as_os_str()),
        }
    }
    Ok(clean)
}
fn check_output(bundle: &Path, inputs: &[PathBuf], output: &Path) -> Result<()> {
    let canonical_bundle = fs::canonicalize(bundle).context("canonical authoring bundle")?;
    let output_name = output.file_name().context("output filename required")?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let target = if output.exists() {
        fs::canonicalize(output)?
    } else {
        fs::canonicalize(parent)?.join(output_name)
    };
    ensure!(
        !target.starts_with(&canonical_bundle) && !lexical(output)?.starts_with(lexical(bundle)?),
        "output must be outside the authoring bundle"
    );
    for input in inputs {
        let original = fs::metadata(input).context("input metadata")?;
        ensure!(original.is_file(), "input must be a regular file");
        ensure!(
            target != fs::canonicalize(input)?,
            "output aliases an input"
        );
        if let Ok(existing) = fs::metadata(output) {
            ensure!(existing.is_file(), "output must be a regular file");
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                ensure!(
                    original.dev() != existing.dev() || original.ino() != existing.ino(),
                    "output hardlinks an input"
                );
            }
        }
    }
    Ok(())
}
/// Execute the same filesystem boundary used by the standalone command.
pub fn run(args: impl IntoIterator<Item = OsString>, mut stdout: impl Write) -> Result<()> {
    let (bundle, edited, output, ids) = arguments(args)?;
    let limits = MaterialEditLimits::default();
    let reference = bundle.join("reference.glb");
    let sidecar = bundle.join("authoring.json");
    check_output(
        &bundle,
        &[reference.clone(), sidecar.clone(), edited.clone()],
        &output,
    )?;
    let original = read(&reference, limits.reference.max_glb_bytes)?;
    let sidecar = read(&sidecar, limits.reference.max_output_bytes)?;
    let edited = read(&edited, limits.max_edited_glb_bytes)?;
    let result = reimport_material_edits(&original, &sidecar, &edited, &ids, &limits)?;
    let report = material_edit_report_json(&result.report, limits.max_report_bytes)?;
    publish_material_edits(&result.glb, &output)?;
    stdout
        .write_all(&report)
        .context("visual output committed, but writing the audit report failed")?;
    Ok(())
}
#[allow(dead_code)]
fn main() -> Result<()> {
    run(std::env::args_os().skip(1), io::stdout().lock())
}

#[cfg(test)]
mod argument_tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<(PathBuf, PathBuf, PathBuf, Vec<String>)> {
        arguments(args.iter().map(OsString::from))
    }
    #[test]
    fn end_of_options_admits_leading_dash_paths() {
        let parsed = parse(&[
            "--material",
            "material-000000",
            "--",
            "-bundle",
            "-edited.glb",
            "-output.glb",
        ])
        .unwrap();
        assert_eq!(parsed.0, PathBuf::from("-bundle"));
        assert_eq!(parsed.1, PathBuf::from("-edited.glb"));
        assert_eq!(parsed.2, PathBuf::from("-output.glb"));
        assert_eq!(parsed.3, ["material-000000"]);
    }
    #[test]
    fn selectors_require_exact_six_ascii_digits_before_file_io() {
        for id in [
            "--foo",
            "--",
            "material-0",
            "material-0000000",
            "material-００００００",
            "mesh-000000",
        ] {
            assert!(parse(&["--material", id, "B", "E", "O"]).is_err(), "{id}");
        }
        assert!(parse(&["--material", "material-999999", "B", "E", "O"]).is_ok());
        assert!(parse(&["--material", "material-000000", "B", "E", "O", "--unknown"]).is_err());
        assert!(parse(&["--", "--material", "material-000000", "B", "E", "O"]).is_err());
    }
}
