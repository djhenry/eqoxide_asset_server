use anyhow::Result;
use clap::{ArgGroup, Parser};
use eqoxide_asset_server::eqg::{
    pbr_visual::{bounded_json, inspect_eqg_pbr_materials, EqgPbrLimits},
    DescriptorSource,
};
use std::{io::Write, path::PathBuf};

/// Read-only binary EQG material schema inventory; readiness is not evaluated.
#[derive(Debug, Parser)]
#[command(group(ArgGroup::new("descriptor").required(true).args(["descriptor_file", "descriptor_member"])))]
struct Arguments {
    #[arg(long)]
    descriptor_file: Option<PathBuf>,
    #[arg(long)]
    descriptor_member: Option<String>,
    archive: PathBuf,
}
fn main() -> Result<()> {
    let args = Arguments::parse();
    let descriptor = match &args.descriptor_file {
        Some(path) => DescriptorSource::Loose(path),
        None => DescriptorSource::Archive(args.descriptor_member.as_deref().unwrap()),
    };
    let limits = EqgPbrLimits::default();
    let report = inspect_eqg_pbr_materials(&args.archive, descriptor, &limits)?;
    let bytes = bounded_json(&report, limits.max_report_json_bytes)?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&bytes)?;
    stdout.write_all(b"\n")?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    #[test]
    fn strict_inventory_arguments() {
        for selector in ["--descriptor-file", "--descriptor-member"] {
            assert!(
                Arguments::try_parse_from(["inventory", selector, "zone.zon", "zone.eqg"]).is_ok()
            );
            assert!(Arguments::try_parse_from([
                "inventory",
                selector,
                "zone.zon",
                "--",
                "--zone.eqg"
            ])
            .is_ok());
        }
        for args in [
            vec!["inventory", "zone.eqg"],
            vec![
                "inventory",
                "--descriptor-file",
                "zone.zon",
                "--descriptor-member",
                "zone.zon",
                "zone.eqg",
            ],
            vec![
                "inventory",
                "--descriptor-file",
                "zone.zon",
                "zone.eqg",
                "out.glb",
            ],
            vec![
                "inventory",
                "--descriptor-file",
                "zone.zon",
                "--unknown",
                "zone.eqg",
            ],
            vec!["inventory", "--descriptor-file"],
            vec![
                "inventory",
                "--descriptor-file",
                "one.zon",
                "--descriptor-file",
                "two.zon",
                "zone.eqg",
            ],
        ] {
            assert!(Arguments::try_parse_from(args.clone()).is_err(), "{args:?}");
        }
        let parsed =
            Arguments::try_parse_from(["inventory", "--descriptor-file=--zone.zon", "zone.eqg"])
                .unwrap();
        assert_eq!(
            parsed.descriptor_file.unwrap(),
            std::path::PathBuf::from("--zone.zon")
        );
    }
}
